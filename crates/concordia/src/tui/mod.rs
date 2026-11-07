use crate::accounts::{accounts_with_auth, display_account_name, get_account, is_logged_in};
use crate::catalog::list_models;
use crate::commands::{dispatch_slash, parse_slash, CommandContext, CommandOutcome, SlashCommand};
use crate::connect::{add_account, AddResult};
use crate::context::{plan_continuity, ContinuityRequest, PackMessage, PackRole};
use crate::identity::{format_identity_label, inspect_provider};
use crate::provider::stream_cards::{format_card_line, upsert_card, CardStatus, StreamCard};
use crate::provider::turn::{
    plan_provider_turn, run_provider_turn_with_transport, ProviderTurnRequest, RunningTurn,
    TurnOutcome, TurnProgress,
};
use crate::sessions::{
    count_sessions_here, legacy_session_tasks, list_recent_sessions, tasks_to_picker_rows,
    trust_picker_rows, PickerRow,
};
use crate::store::{
    default_store_root, ensure_default_selection, get_settings, get_use_selection, set_trust_mode,
    set_use_selection, ProviderKind, TrustMode, UseSelection,
};
use crate::update::{
    start_check, start_update, Job, JobKind, JobResult, UpdateOutcome, UpdatePreview,
};
use crate::usage::{usage_rows_for_accounts, usage_tone, UsageFetcher, UsageTone};
use crate::version::short_stamp;
use anyhow::Result;
use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    KeyboardEnhancementFlags, MouseButton, MouseEvent, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate,
    EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Widget};
use ratatui::{Frame, Terminal};
use std::collections::{HashMap, HashSet};
use std::io::{self, stdout};
use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

mod activity;
mod app;
pub(in crate::tui) use app::state::*;
mod appearance;
mod artifacts;
mod run;
#[cfg(windows)]
pub use run::run_chat_window;
pub use run::{run_chat_tui, run_file_editor, run_file_preview};
mod brand;
mod chatref;
mod clipboard;
#[cfg(all(windows, not(test)))]
mod clipboard_capture;
mod compat_ui;
mod continuation;
#[cfg(windows)]
mod detached;
mod diagrams;
mod display_backend;
mod editor;
mod files;
mod images;
mod input;
mod input_dispatch;
mod input_stream;
mod integrations;
mod landing;
mod managed_workspaces;
mod markdown;
mod modal;
mod native_agents;
#[cfg(windows)]
mod native_window;
mod palette;
mod persist;
mod pointer;
mod preview;
mod questions;
mod queue;
mod queue_controls;
mod queue_focus;
mod raster;
mod recovery;
mod recovery_ui;
mod reference_journal;
mod related_files;
mod session_ui;
#[cfg(test)]
mod session_ui_tests;
mod settings;
mod stats_ui;
mod syntax;
mod text;
mod theme;
mod tips;
mod token_line;
mod tokens;
mod transcript;
mod turn_checkpoint;
mod usage_style;
#[cfg(test)]
mod ux_tests;
mod worker_input;
mod worker_panel;
mod workers;
mod workspace;
mod workspace_scope;
mod workspace_search;

use editor::Editor;
#[cfg(test)]
use input_dispatch::should_accept_key;
use input_dispatch::{dispatch_key_burst, dispatch_key_event, handle_key, handle_mouse};
use palette::Palette;
use transcript::{Block as TBlock, TranscriptView};

/// Prompt box grows with its content up to this many text rows, then scrolls.
const MAX_INPUT_ROWS: usize = 6;
/// Columns taken by the `> ` prompt (and the matching indent on continuation rows).
const PROMPT_W: usize = 2;
const MULTI_CLICK: Duration = Duration::from_millis(500);
const FLASH_FOR: Duration = Duration::from_secs(3);
const FOOTER_HINT: &str = "Enter send \u{b7} F9 attach \u{b7} / commands \u{b7} Shift+Tab permissions \u{b7} drag to select \u{b7} Ctrl+C copy or cancel";
/// How often (in 200 ms ticks) to look for a finished login, and when to stop looking.
/// A second idle Ctrl+C within this window exits.
const QUIT_WINDOW: Duration = Duration::from_secs(2);
const HISTORY_MAX: usize = persist::HISTORY_CAP;
const LOGIN_POLL_TICKS: u32 = 10;
const LOGIN_GIVE_UP_TICKS: u32 = 3000;

fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

impl App {
    fn continuity_home(&self) -> Option<String> {
        self.home_path
            .as_ref()
            .and_then(|home| home.canonicalize().ok())
            .map(|home| home.to_string_lossy().into_owned())
    }
    fn refresh_binding(&mut self) {
        self.account_names = crate::store::load_store(&self.state_root).account_nicknames;
        if let Some(sel) = get_use_selection(&self.state_root) {
            if self.account != sel.account
                || self.provider != sel.provider
                || self
                    .home_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    != sel.home_path
            {
                self.session_ui.invalidate();
                self.header_email_revealed = false;
                self.header_email_pressed = false;
            }
            self.provider = sel.provider;
            self.account = sel.account;
            self.home_path = sel.home_path.map(PathBuf::from);
            self.model = sel
                .model
                .filter(|m| !m.trim().is_empty())
                .or_else(|| {
                    self.home_path
                        .as_ref()
                        .and_then(|h| inspect_provider(self.provider, h).model)
                })
                .unwrap_or_else(|| "?".into());
        }
        let email = self
            .home_path
            .as_ref()
            .and_then(|h| inspect_provider(self.provider, h).login)
            .filter(|login| login.contains('@'))
            .map(|email| email.chars().filter(|c| !c.is_control()).collect());
        if self.account_email != email {
            self.header_email_revealed = false;
            self.header_email_pressed = false;
        }
        self.account_email = email;
        self.trust = get_settings(&self.state_root).trust;
        self.refresh_usage();
    }

    fn window_title(&self) -> String {
        let status = if self.job.as_ref().is_some_and(|j| j.kind == JobKind::Update) {
            Some("Updating".to_owned())
        } else if self.cancel_requested && self.busy {
            Some("Stopping".to_owned())
        } else if worker_input::active(self) || (self.busy && self.questions.waiting()) {
            Some("Needs input".to_owned())
        } else if self.busy {
            Some(match &self.phase {
                Phase::Thinking => "Thinking".into(),
                Phase::Writing => "Writing".into(),
                Phase::Waiting => "Working".into(),
                Phase::Tool(name) => {
                    let name = name.to_ascii_lowercase();
                    if name.contains("patch") || name.contains("edit") || name.contains("write") {
                        "Editing"
                    } else if name.contains("web") {
                        "Searching the web"
                    } else if name.contains("search") || name.contains("find") {
                        "Searching"
                    } else if name.contains("read") {
                        "Reading"
                    } else if name.contains("shell")
                        || name.contains("exec")
                        || name.contains("command")
                    {
                        "Running command"
                    } else {
                        "Working"
                    }
                    .into()
                }
            })
        } else if self.continuation.waiting() {
            Some("Paused".to_owned())
        } else if self.deleg.d.any_active() {
            Some("Workers running".to_owned())
        } else {
            None
        };
        let project = self
            .cwd
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "New chat".into());
        let task = if self.landing.is_some() {
            "Home"
        } else {
            self.session_ui.title.as_deref().unwrap_or(&project)
        };
        let clean = |value: &str, max: usize| -> String {
            value
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .replace(['\u{2014}', '\u{2013}'], "-")
                /*
                .replace(['Ã¢â‚¬â€', 'Ã¢â‚¬â€œ'], "-")
                */
                .chars()
                .filter(|c| !c.is_control())
                .take(max)
                .collect()
        };
        let task = clean(task, 40);
        let task = if task.chars().count() == 40 {
            format!("{}...", task.trim_end())
        } else {
            task
        };
        match status {
            Some(status) => format!("Concordia | {} | {task}", clean(&status, 24)),
            None => format!("Concordia | {task}"),
        }
    }

    fn run_status_label(&self) -> String {
        if self.cancel_requested {
            return if self.queue.force_pending {
                "Stopping to send the queued message\u{2026}".into()
            } else {
                "Stopping this turn\u{2026}".into()
            };
        }
        match &self.phase {
            Phase::Waiting if self.cancel_requested => "Stopping this turn\u{2026}".into(),
            Phase::Waiting if self.live_cards.is_empty() => {
                format!("{} \u{b7} starting the turn", self.account_label())
            }
            Phase::Waiting => "Reviewing the latest tool results".into(),
            Phase::Thinking => "Working through the next step".into(),
            Phase::Writing => "Putting the answer together".into(),
            Phase::Tool(name) => self
                .live_cards
                .iter()
                .rev()
                .find(|c| c.status == CardStatus::Running)
                .map(crate::provider::stream_cards::format_card_line)
                .unwrap_or_else(|| format!("Running {name}")),
        }
    }

    /// Suggestions for the prompt as typed (empty when closed, dismissed, or not a slash prompt).
    fn palette_entries(&self) -> Vec<palette::Entry> {
        if self.editor.text().starts_with("/open ") || self.editor.text().starts_with("/preview ") {
            return Vec::new();
        }
        // Not while a recalled prompt is on screen: up/down must keep walking history.
        if self.palette.dismissed || self.picker.is_some() || self.hist_pos.is_some() {
            return Vec::new();
        }
        let input = self.editor.text();
        let lower = input.to_ascii_lowercase();
        if palette::has_argument_menu(&input) {
            let accounts = if lower.starts_with("/use") || lower.starts_with("/workers") {
                accounts_with_auth(&self.state_root)
                    .into_iter()
                    .map(|a| (a.account.clone(), a.label))
                    .collect()
            } else {
                Vec::new()
            };
            let models = if lower.starts_with("/model") {
                self.home_path
                    .as_deref()
                    .map(|h| list_models(self.provider, h))
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            return palette::matching_arguments(&input, &accounts, &models);
        }
        palette::matching_with(&input, &self.user_cmds)
    }

    /// Put the chosen command in the prompt.
    fn complete_palette(&mut self, entry: palette::Entry) {
        let complete_branch = self.editor.text().eq_ignore_ascii_case(&entry.command)
            && palette::is_branch(&entry.command);
        self.editor.set_text(&if complete_branch {
            format!("{} ", entry.command)
        } else {
            entry.command
        });
        self.palette.reset();
    }

    /// Tab: complete the highlighted suggestion.
    fn tab_complete_slash(&mut self) {
        let entries = self.palette_entries();
        if let Some(e) = entries.get(self.palette.selected.min(entries.len().saturating_sub(1))) {
            let e = e.clone();
            self.complete_palette(e);
        }
    }

    fn remember_prompt(&mut self, line: &str) {
        self.hist_pos = None;
        self.push_history(line);
    }

    fn history_prev(&mut self) {
        if self.hist_pos.is_none() && self.fresh_draft {
            let draft = self
                .parked_draft
                .replace(self.editor.text())
                .unwrap_or_default();
            self.restore_composer(&draft);
            self.fresh_draft = false;
            return;
        }
        if self.history.is_empty() {
            return;
        }
        let pos = match self.hist_pos {
            None => {
                self.hist_draft = self.composer_value();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.hist_pos = Some(pos);
        self.restore_composer(&self.history[pos].clone());
    }

    fn history_next(&mut self) {
        let Some(i) = self.hist_pos else {
            if !self.fresh_draft
                && (!self.editor.text().is_empty()
                    || self.parked_draft.as_ref().is_some_and(|s| !s.is_empty()))
            {
                let next = self
                    .parked_draft
                    .replace(self.editor.text())
                    .unwrap_or_default();
                self.restore_composer(&next);
                self.fresh_draft = true;
                self.flash("Fresh prompt; Up on the first row returns to your draft");
            }
            return;
        };
        if i + 1 < self.history.len() {
            self.hist_pos = Some(i + 1);
            self.restore_composer(&self.history[i + 1].clone());
        } else {
            self.hist_pos = None;
            let draft = std::mem::take(&mut self.hist_draft);
            self.restore_composer(&draft);
        }
    }

    fn paste_clipboard(&mut self) {
        if !cfg!(test)
            && clipboard::has_attachment()
            && self.picker.is_none()
            && self.workspace.focused.is_none()
            && self.queue.edit.is_none()
            && !self.questions.focused
            && self.activity.detail.is_none()
            && !worker_input::active(self)
            && !self.project_search.active()
        {
            self.start_image_import(None, true);
            return;
        }
        match clipboard::get_text() {
            Ok(t) if !t.is_empty() => self.paste_text(&t),
            Ok(_) => {}
            Err(e) => self.flash(format!("paste failed Ã¢â‚¬â€ {e}")),
        }
    }

    /// Copy the current selection. With `fall_back_to_reply`, copy the last assistant reply
    /// when nothing is selected. Returns whether anything was copied.
    fn copy(&mut self, fall_back_to_reply: bool) -> bool {
        let text = self.selected_text().or_else(|| {
            fall_back_to_reply
                .then(|| self.last_assistant_text())
                .flatten()
        });
        match text {
            Some(t) => self.write_clipboard(&t),
            None => {
                self.flash("nothing to copy Ã¢â‚¬â€ drag to select, or /copy for the last reply");
                false
            }
        }
    }

    fn push_system(&mut self, text: impl Into<String>) {
        let text = text.into();
        // A provider often reports one failure several ways (an event, a result, stderr).
        // Say it once.
        if self
            .messages
            .last()
            .is_some_and(|m| m.role == MsgRole::System && m.text.trim() == text.trim())
        {
            return;
        }
        self.messages.push(ChatLine {
            role: MsgRole::System,
            text,
            card: None,
            meta: None,
        });
    }

    fn clear_idle_notice(&mut self) {
        if self.idle_notice.take().is_some() {
            if let Some(index) = self.messages.iter().position(|line| {
                line.meta
                    .as_ref()
                    .is_some_and(|(_, label)| label == "__concordia_idle_notice__")
            }) {
                self.messages.remove(index);
            }
        }
    }

    fn idle_handoff(&mut self) {
        let has_other = accounts_with_auth(&self.state_root)
            .iter()
            .any(|a| a.account != self.account && a.home_path.is_some());
        if !has_other {
            self.push_system("hand off disabled: no other signed-in account");
            return;
        }
        let Some(prompt) = self
            .submitted_prompt
            .clone()
            .or_else(|| self.last_sent.clone())
        else {
            self.push_system("hand off unavailable: the original request is not available");
            return;
        };
        self.continuation.prepare_manual_handoff(prompt);
        self.handoff_after_cancel = true;
        self.request_cancel();
    }

    fn open_idle_picker(&mut self) {
        let has_other = accounts_with_auth(&self.state_root)
            .iter()
            .any(|a| a.account != self.account && a.home_path.is_some());
        self.show_picker(
            PickerKind::Idle,
            "turn is quiet".into(),
            vec![
                PickerRow {
                    id: "idle:wait".into(),
                    label: "keep waiting".into(),
                    detail: "leave the notice and continue waiting".into(),
                },
                PickerRow {
                    id: "idle:cancel".into(),
                    label: "cancel".into(),
                    detail: "stop this turn".into(),
                },
                PickerRow {
                    id: "idle:handoff".into(),
                    label: "hand off".into(),
                    detail: if has_other {
                        "choose another account".into()
                    } else {
                        "disabled: no other signed-in account".into()
                    },
                },
            ],
            false,
        );
    }

    fn needs_you_session(&self) -> String {
        self.questions
            .native_session_id()
            .or_else(|| self.native_session_id.clone())
            .or_else(|| self.task_id.clone())
            .unwrap_or_else(|| "chat".into())
    }

    fn sync_needs_you(&mut self) {
        use crate::needs_you::{Item, ItemKind, SessionState};
        self.questions.refresh_native();
        let session = self.needs_you_session();
        let state = if self.busy {
            SessionState::Running
        } else if self.signed_out_confirmation.is_some()
            || self.questions.waiting()
            || self.queue.paused
        {
            SessionState::Waiting
        } else {
            SessionState::Idle
        };
        self.needs_you.set_state(session.clone(), state);
        let approval = self.questions.native_approval_waiting();
        let current = [
            (
                "question",
                self.questions.waiting() && !approval,
                ItemKind::Question,
                "answer provider question",
            ),
            (
                "approval",
                approval,
                ItemKind::Approval,
                "answer permission request",
            ),
            (
                "paused-queue",
                self.queue.paused,
                ItemKind::PausedQueue,
                "resume paused prompt queue",
            ),
            (
                "idle",
                self.idle_notice.is_some(),
                ItemKind::IdleNotice,
                "turn is quiet",
            ),
            (
                "signed-out",
                self.signed_out_confirmation.is_some(),
                ItemKind::SignedOut,
                "confirm signed-out account",
            ),
        ];
        for (suffix, present, kind, title) in current {
            self.needs_you.remove_suffix(suffix);
            let id = format!("{session}:{suffix}");
            if present {
                self.needs_you.add(Item {
                    id,
                    session_id: session.clone(),
                    title: title.into(),
                    kind,
                });
            }
        }
        for job in &self.deleg.d.jobs {
            let id = format!("worker:{}", job.id);
            if job.state == crate::delegate::WorkerState::Done && !job.collected {
                self.needs_you.add(Item {
                    id,
                    session_id: job.session_id.clone().unwrap_or_else(|| job.id.to_string()),
                    title: format!("worker {} finished", job.id),
                    kind: ItemKind::WorkerResult,
                });
            } else {
                self.needs_you.remove(&id);
            }
        }
    }

    fn open_needs_you_picker(&mut self) {
        self.sync_needs_you();
        let rows = self
            .needs_you
            .items()
            .map(|item| PickerRow {
                id: format!("needs:{}", item.id),
                label: item.title.clone(),
                detail: format!("{} Ã‚· {}", item.kind.label(), item.session_id),
            })
            .collect();
        self.show_picker(PickerKind::NeedsYou, "needs you".into(), rows, false);
    }

    fn idle_provider_name(provider: ProviderKind) -> &'static str {
        match provider {
            ProviderKind::Codex => "Codex",
            ProviderKind::Claude => "Claude Code",
            ProviderKind::Cursor => "Cursor",
        }
    }

    fn poll_model_refresh(&mut self) {
        let Some(refresh) = self.model_refresh.as_ref() else {
            return;
        };
        let Ok(models) = refresh.rx.try_recv() else {
            return;
        };
        let provider = refresh.provider;
        let account = refresh.account.clone();
        self.model_refresh = None;
        if self.provider != provider || self.account != account {
            return;
        }
        let Some(picker) = self.picker.as_mut().filter(|p| p.kind == PickerKind::Model) else {
            return;
        };
        let filter = picker.filter.clone();
        let selected = picker.state.selected().unwrap_or(0);
        let chosen = Some(self.model.trim()).filter(|m| !m.is_empty() && *m != "?");
        picker.rows = model_picker_rows(models, chosen);
        picker.title = "model".into();
        picker.filter = filter;
        picker.state.select(Some(selected));
        picker.select_clamp();
        self.footer_left = "up/down Enter Ã‚· r refresh Ã‚· type to filter Ã‚· Esc".into();
    }

    fn handle_slash(&mut self, line: &str) -> bool {
        let trimmed = line.trim();
        if let Some(id) = trimmed
            .strip_prefix("/resume ")
            .or_else(|| trimmed.strip_prefix("/chats "))
            .map(str::trim)
            .filter(|id| !id.is_empty() && !id.chars().any(char::is_whitespace))
        {
            self.push_command(line);
            self.resume_explicit_session(id);
            return true;
        }
        if line.trim() == "/native" || line.trim().starts_with("/native ") {
            let parts: Vec<_> = line.split_whitespace().collect();
            let result = match parts.as_slice() {
                ["/native", account] | ["/native", account, "status"] => {
                    crate::native_input::command(&self.state_root, account, "status")
                }
                ["/native", account, mode @ ("on" | "off")] => {
                    let account = crate::accounts::normalize_account_id(account);
                    if self.busy && self.account == account
                        || self.deleg.d.running_for(&account).is_some()
                    {
                        Err("finish or cancel that account's active native turn before changing transport".into())
                    } else {
                        crate::native_input::command(&self.state_root, &account, mode)
                    }
                }
                _ => Err("/native ACCOUNT on|off|status".into()),
            };
            self.push_system(result.unwrap_or_else(|error| format!("native input: {error}")));
            return true;
        }
        if line.trim() == "/delegate input" {
            self.flash("usage: /delegate input ID");
            return true;
        }
        if let Some(id) = line.trim().strip_prefix("/delegate input ") {
            match id.parse::<u32>() {
                Ok(id) => worker_input::open(self, id),
                Err(_) => self.flash("usage: /delegate input ID"),
            }
            return true;
        }
        let trimmed = line.trim();
        if ["/delegate fallback", "/delegate recover", "/delegate retry"]
            .iter()
            .any(|p| trimmed == *p || trimmed.starts_with(&format!("{p} ")))
        {
            landing::close(self);
            self.push_command(line);
            self.worker_recovery_command(trimmed.strip_prefix("/delegate ").unwrap_or_default());
            return true;
        }
        if trimmed == "/integrations" {
            landing::close(self);
            self.push_command(line);
            integrations::open(self);
            return true;
        }
        if trimmed == "/context" || trimmed.starts_with("/context ") {
            landing::close(self);
            self.push_command(line);
            let result = self
                .home_path
                .as_ref()
                .ok_or_else(|| "account home is unavailable".to_string())
                .and_then(|h| {
                    crate::native_memory::Binding::new(
                        self.provider,
                        &self.account,
                        h,
                        self.native_session_id
                            .as_deref()
                            .unwrap_or("context-policy"),
                    )
                })
                .and_then(|b| {
                    crate::capacity::set(
                        &self.state_root,
                        &b,
                        trimmed.strip_prefix("/context").unwrap_or_default().trim(),
                    )
                });
            self.push_system(result.unwrap_or_else(|e| format!("context policy: {e}")));
            return true;
        }
        if trimmed == "/memory" || trimmed.starts_with("/memory ") {
            landing::close(self);
            self.push_command(line);
            self.memory_command(trimmed.strip_prefix("/memory").unwrap_or_default().trim());
            return true;
        }
        if trimmed == "/trust" || trimmed.starts_with("/trust ") {
            self.push_command(line);
            self.push_system("/trust is now /mode");
            self.run_mode_command(
                trimmed
                    .strip_prefix("/trust")
                    .map(str::trim)
                    .filter(|s| !s.is_empty()),
            );
            return true;
        }
        if trimmed == "/threads" || trimmed.starts_with("/threads ") {
            self.push_command(line);
            self.push_system("/threads is now /resume");
            self.open_resume_picker();
            return true;
        }
        if trimmed == "/agents" {
            self.push_command(line);
            self.push_system("/agents is now Ctrl+G");
            self.open_agents_picker();
            return true;
        }
        if trimmed == "/skills" || trimmed == "/hooks" {
            settings::catalog(self, trimmed == "/hooks");
            return true;
        }
        if trimmed == "/config" || trimmed.starts_with("/config ") {
            let args = trimmed.strip_prefix("/config").unwrap_or("").trim();
            match args {
                "" => settings::open(self),
                "appearance" => appearance::open(self),
                "skills" => settings::catalog(self, false),
                "hooks" => settings::catalog(self, true),
                _ => appearance::command(self, args),
            }
            return true;
        }
        if trimmed == "/version" || trimmed == "/ver" || trimmed == "/about" {
            self.push_command(line);
            self.push_system(format!("{} is now /doctor", trimmed));
            for report_line in crate::doctor::run_fast_report().lines() {
                self.push_system(report_line.to_owned());
            }
            return true;
        }
        if trimmed == "/use rename" || trimmed.starts_with("/use rename ") {
            let args = trimmed
                .strip_prefix("/use rename")
                .unwrap_or_default()
                .trim();
            let (account, name) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
            if account.is_empty() {
                self.push_system("usage: /use rename ACCOUNT NICKNAME (omit nickname to reset)");
            } else {
                match crate::accounts::set_nickname(&self.state_root, account, name) {
                    Ok(()) => {
                        self.account_names =
                            crate::store::load_store(&self.state_root).account_nicknames;
                        self.flash("account nickname saved");
                    }
                    Err(error) => self.push_system(format!("nickname: {error}")),
                }
            }
            return true;
        }
        if trimmed == "/preview" || trimmed.starts_with("/preview ") {
            self.push_command(line);
            if let Some(reference) = trimmed.strip_prefix("/preview ") {
                self.file_command(reference, false, true);
            } else {
                self.push_system("usage: /preview FILE");
            }
            return true;
        }
        if trimmed == "/usage all" || trimmed == "/usage refresh" {
            self.push_system(format!(
                "{} is now /usage (use r inside the screen to refresh)",
                trimmed
            ));
        }
        // /workers, /queue, /edit, /close, /split, /stats and /recover keep working unlisted
        // until the screens that replace them exist.
        if trimmed == "/exit" || trimmed == "/quit" || trimmed == "/q" {
            self.push_command(line);
            self.push_system("/exit is now Ctrl+C twice");
            return true;
        }
        if line.trim() == "/search" {
            self.open_project_search();
            return true;
        }
        if line.trim() == "/open" {
            self.open_related_files();
            return true;
        }
        if matches!(line.trim(), "/preview" | "/edit") {
            self.push_system("usage: /open FILE");
            return true;
        }
        if let Some(reference) = line.trim().strip_prefix("/open ") {
            self.file_command(reference, false, false);
            return true;
        }
        if let Some(reference) = line.trim().strip_prefix("/edit ") {
            self.file_command(reference, true, false);
            return true;
        }
        if line.trim() == "/close" || line.trim().starts_with("/close ") {
            let target = line
                .trim()
                .strip_prefix("/close")
                .unwrap_or_default()
                .trim();
            let id = if target.is_empty() {
                self.workspace.focused
            } else {
                target.parse::<u32>().ok()
            };
            match id {
                Some(id) if self.workspace.close(id) => {
                    if self.workspace.panes.iter().any(|p| p.id == id) {
                        self.flash("Unsaved file: S saves, D discards, Esc cancels");
                    } else {
                        self.flash(format!("closed file pane #{id}"));
                    }
                }
                _ => self.flash("usage: /close PANE_ID (or /close with a file pane focused)"),
            }
            return true;
        }
        if line.trim() == "/recover" {
            self.open_recovery_picker();
            return true;
        }
        if line.trim() == "/recover legacy" {
            self.choose_recovery("legacy");
            return true;
        }
        if line.trim() == "/recover retry" {
            self.retry_recovery_now();
            self.push_system(
                self.recovery_warning()
                    .unwrap_or("saved this pane locally")
                    .to_owned(),
            );
            return true;
        }
        if let Some(arg) = queue::slash_arg(line) {
            self.push_command(line);
            let notices = if arg == "resume" {
                queue_controls::resume(self)
            } else {
                self.queue_command(arg)
            };
            for l in notices {
                self.push_system(l);
            }
            return true;
        }
        if line.trim() == "/threads" || line.trim_start().starts_with("/threads ") {
            self.push_command(line);
            for l in self.threads_command(line.trim_start()["/threads".len()..].trim()) {
                self.push_system(l);
            }
            return true;
        }
        let Some((cmd, split_vertical)) = parse_slash(line) else {
            return false;
        };
        if self.busy
            && matches!(
                &cmd,
                SlashCommand::New
                    | SlashCommand::Resume
                    | SlashCommand::Use
                    | SlashCommand::UseRemove(_)
                    | SlashCommand::UseAccount(_)
                    | SlashCommand::ModelSet(_)
                    | SlashCommand::Model
            )
        {
            self.push_system("finish or cancel the current turn before switching conversation, provider or model");
            return true;
        }
        if let SlashCommand::UseAccount(account) = &cmd {
            let normalized = crate::accounts::normalize_account_id(account);
            if !accounts_with_auth(&self.state_root)
                .iter()
                .any(|a| a.account == normalized)
            {
                self.push_system(
                    "account is not connected; /use lists accounts and /use add connects one",
                );
                return true;
            }
            self.show_picker(
                PickerKind::Use,
                "accounts".into(),
                vec![PickerRow {
                    id: format!("use:{normalized}"),
                    label: normalized,
                    detail: String::new(),
                }],
                false,
            );
            self.apply_picker_selection(None);
            return true;
        }
        if let SlashCommand::ModelSet(model) = &cmd {
            if !valid_model_id(model) {
                self.push_system("model id contains unsupported characters");
                return true;
            }
            self.show_picker(
                PickerKind::Model,
                "model".into(),
                vec![PickerRow {
                    id: format!("model:{model}"),
                    label: model.clone(),
                    detail: String::new(),
                }],
                false,
            );
            self.apply_picker_selection(None);
            return true;
        }
        if let SlashCommand::UseRemove(named) = &cmd {
            self.open_remove_picker(named.clone());
            return true;
        }
        if let SlashCommand::Mode(arg) = &cmd {
            self.push_command(line);
            self.run_mode_command(arg.as_deref());
            return true;
        }
        if self.try_delegate_slash(&cmd) {
            return true;
        }
        if let SlashCommand::Harness(arg) = &cmd {
            self.push_command(line);
            let accounts = accounts_with_auth(&self.state_root)
                .into_iter()
                .map(|a| (a.provider, a.home_path))
                .collect();
            let session = crate::harness::command::Session {
                store: &self.state_root,
                cwd: &self.cwd,
                provider: self.provider,
                home: self.home_path.as_deref(),
                accounts,
            };
            let cwd = self.cwd.clone();
            let lines = crate::harness::command::dispatch(&session, arg, &|argv| {
                crate::harness::run::run_command(argv, &cwd, crate::harness::run::DEFAULT_TIMEOUT)
            });
            for l in lines {
                self.push_system(l);
            }
            return true;
        }
        if matches!(&cmd, SlashCommand::Unknown(n) if n == "stats") {
            self.push_command(line);
            self.toggle_stats();
            return true;
        }
        let is_new = matches!(cmd, SlashCommand::New);
        if is_new && !self.save_turn_checkpoint(true) {
            self.keep.turn_recovery_notice = true;
            return true;
        }
        let ctx = CommandContext {
            state_root: &self.state_root,
            cwd: &self.cwd,
            provider: self.provider,
            account: &self.account,
            trust: self.trust,
            split_vertical,
        };
        let (outcome, lines) = dispatch_slash(cmd, &ctx);
        if is_new {
            self.project_search.close();
            self.native_owner = None;
            self.deleg.change_conversation();
            self.keep.turn_recovery_notice = false;
            self.continuation.invalidate();
            self.native_sources.clear();
            self.native_agents = native_agents::State::default();
            self.session_ui.invalidate();
            self.session_ui.title = None;
            self.messages.clear();
            self.related_files.reset();
            self.questions.clear();
            self.activity = activity::Activity::default();
            self.expanded_activity = false;
            self.view.follow_tail();
            self.clear_selections();
            self.native_session_id = None;
            self.session_chip = None;
            self.usage_block = None;
            self.session_input = 0;
            self.session_output = 0;
            self.session_cost = 0.0;
            self.need_context_pack = false;
            self.thread = crate::threads::start(&self.state_root);
            self.thread_meta_done = false;
            landing::open(self);
        }
        if !lines.is_empty() {
            self.push_command(line);
        }
        for l in lines {
            self.push_system(l);
        }
        match outcome {
            CommandOutcome::Exit => self.should_quit = true,
            CommandOutcome::OpenResume => self.open_resume_picker(),
            CommandOutcome::OpenTrust => self.open_trust_picker(),
            CommandOutcome::OpenUse => {
                for l in crate::integration::refresh_summary(&self.state_root) {
                    self.push_system(l);
                }
                self.open_use_picker();
            }
            CommandOutcome::OpenAddProvider => self.open_add_provider_picker(),
            CommandOutcome::OpenRemovePicker => self.open_remove_picker(None),
            CommandOutcome::AddAccount(p) => self.begin_add_account(p),
            CommandOutcome::OpenModel => self.open_model_picker(),
            CommandOutcome::OpenDoctor => self.open_doctor(),
            CommandOutcome::ShowUsage => self.show_usage(false),
            CommandOutcome::ShowUsageRefresh => self.show_usage(true),
            CommandOutcome::OpenUsage => self.open_usage_picker(),
            CommandOutcome::CopyLastReply => match self.last_assistant_text() {
                Some(t) => {
                    self.write_clipboard(&t);
                }
                None => self.flash("nothing to copy yet"),
            },
            CommandOutcome::Update => self.open_update_preview(),
            CommandOutcome::Ok => {}
        }
        true
    }

    fn update_running(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.kind == JobKind::Update)
    }

    fn update_blocked_by_work(&self) -> bool {
        self.busy
            || self.deleg.d.any_active()
            || self.deleg.has_pending_prompt()
            || self.workspace.has_unsaved()
    }

    fn start_job(&mut self, kind: JobKind) {
        if self.job.is_some() {
            self.push_system("an update is already running - Esc cancels it");
            return;
        }
        if kind == JobKind::Update && self.update_blocked_by_work() {
            self.push_system("Cannot update while work or unsaved file edits remain. Finish or cancel chat/workers and save or close edited files, then press Enter again. The update has not been queued.");
            return;
        }
        crate::logging::info(&format!("job-start {kind:?}"));
        self.job = Some(match kind {
            JobKind::Update => start_update(self.cwd.clone()),
            JobKind::Check => start_check(self.cwd.clone()),
        });
    }

    fn open_update_preview(&mut self) {
        self.update_preview = None;
        self.show_picker(PickerKind::Update, "update".into(), Vec::new(), false);
        self.start_job(JobKind::Check);
    }

    /// Deliver a finished background job (called on every tick).
    fn poll_job(&mut self) {
        let Some(result) = self.job.as_ref().and_then(Job::poll) else {
            return;
        };
        let cancelled = self.job.as_ref().is_some_and(Job::is_cancelled);
        self.job = None;
        if matches!(result, JobResult::Update(_)) {
            self.close_update_screen();
        }
        match result {
            JobResult::Update(Ok(UpdateOutcome::Installed(result))) => {
                crate::logging::info(&format!(
                    "update-installed {}",
                    result.install_path.display()
                ));
                if result.summary.is_empty() {
                    self.push_system(format!(
                        "built \u{2192} {} \u{b7} restarting into {}",
                        result.built_from.display(),
                        result.install_path.display()
                    ));
                } else {
                    self.push_system(format!(
                        "{} \u{b7} restarting into {}",
                        result.summary,
                        result.install_path.display()
                    ));
                }
                for line in result.log_tail.lines().take(4) {
                    self.push_system(line.to_string());
                }
                self.reexec_path = Some(result.install_path);
                self.should_quit = true;
            }
            JobResult::Update(Ok(UpdateOutcome::UpToDate(msg))) => self.push_system(msg),
            JobResult::Update(Err(e)) if cancelled || e.to_string() == "cancelled" => {
                crate::logging::info("update-cancelled");
                self.push_system("update cancelled");
            }
            JobResult::Update(Err(e)) => {
                crate::logging::error(&format!("update-failed {e:#}"));
                self.push_system(format!("update failed - {e:#}"));
            }
            JobResult::Check(Ok(preview)) => {
                self.footer_left = if preview.install_available {
                    "Enter installs \u{b7} \u{2191}\u{2193} scroll \u{b7} Esc close"
                } else {
                    "\u{2191}\u{2193} scroll \u{b7} Esc close"
                }
                .into();
                self.update_preview = Some(preview);
            }
            JobResult::Check(Err(e)) => {
                let message = format!("update check failed - {e:#}");
                if self
                    .picker
                    .as_ref()
                    .is_some_and(|p| p.kind == PickerKind::Update)
                {
                    self.update_preview = Some(UpdatePreview {
                        history: Vec::new(),
                        history_expanded: false,
                        lines: vec![
                            crate::update::PreviewLine::Text(message),
                            crate::update::PreviewLine::Hint("Esc returns to your chat".into()),
                        ],
                        install_available: false,
                        release_url: None,
                    });
                    self.footer_left = "Esc close".into();
                } else {
                    self.push_system(message);
                }
            }
        }
    }

    fn submit_input(&mut self, turn_tx: &mpsc::UnboundedSender<String>) {
        if self.images.importing() {
            self.flash("Wait for the attachment to finish importing");
            return;
        }
        let original_prompt = self.composer_value();
        let mut line = self.editor.text();
        // Commands retain their forgiving whitespace handling; prompts retain formatting.
        if line.trim_start().starts_with('/') && !line.contains('\n') {
            line = line.trim().to_owned();
        }
        if line.trim().is_empty() && self.images.count() > 0 {
            line = self.images.default_prompt().into();
        }
        if line.trim().is_empty() {
            return;
        }
        let user_prompt = crate::user_commands::expand(&self.user_cmds, &line);
        if self.session_ui.loading() && (!line.starts_with('/') || user_prompt.is_some()) {
            self.flash("recent messages are loading; your draft is kept");
            return;
        }
        if self.busy && (!line.starts_with('/') || user_prompt.is_some()) {
            let confirmed = self.busy_send_confirmation.take().is_some_and(|armed| {
                armed.prompt == original_prompt
                    && armed.conversation == self.deleg.d.conversation_id()
                    && armed.turn_started == self.turn_started
                    && armed.armed.elapsed() <= Duration::from_secs(5)
            });
            if !confirmed {
                self.busy_send_confirmation = Some(BusySendConfirmation {
                    prompt: original_prompt,
                    conversation: self.deleg.d.conversation_id().to_owned(),
                    turn_started: self.turn_started,
                    armed: Instant::now(),
                });
                self.flash("Enter again to interrupt and send this message; Tab queues it.");
                return;
            }
            if self.queue.force_pending {
                self.flash("Stopping the active turn before sending the pending messageÃ¢â‚¬Â¦");
                return;
            }
            let index = self.queue.items.len();
            let mut followup = crate::attachments::Message::decode(&original_prompt);
            followup.text = line;
            self.queue.items.push(followup.encode());
            let notice = self.force_queued(index);
            if self.queue.force_pending {
                self.editor.clear();
                self.images.draft.clear();
                self.clear_selections();
                self.persist_draft_now();
            } else {
                self.queue.items.remove(index);
            }
            self.flash(notice);
            return;
        }
        self.busy_send_confirmation = None;
        if !self.busy {
            self.cancel_requested = false;
        }
        if self.job.as_ref().is_some_and(|j| j.kind == JobKind::Update) && !line.starts_with('/') {
            self.push_system("an update is in progress - Esc cancels it, or wait for it to finish");
            return;
        }
        if !self.busy
            && (!line.starts_with('/') || user_prompt.is_some())
            && self.reference_journal.backlogged()
        {
            self.flash("Finishing history references; your draft is kept.");
            return;
        }
        let dispatch = if line.starts_with('/') && user_prompt.is_none() {
            line.clone()
        } else {
            crate::attachments::Message {
                text: line.clone(),
                attachments: std::mem::take(&mut self.images.draft),
                invalid: None,
            }
            .encode()
        };
        self.remember_prompt(&dispatch);
        self.editor.clear();
        self.clear_selections();
        self.persist_draft_now();
        let was_busy = self.busy;
        if line != "/config" && !line.starts_with("/config ") {
            landing::close(self);
        }
        self.run_line(dispatch, turn_tx);
        if !was_busy && self.busy && self.submitted_prompt.is_some() {
            self.submitted_prompt = Some(original_prompt);
        }
    }

    /// Run a prompt that has left the box: a slash command, or a turn. Typed prompts and queued
    /// ones both come through here, so trust mode, account and provider apply at send time.
    fn run_line(&mut self, line: String, turn_tx: &mpsc::UnboundedSender<String>) {
        let original_line = line;
        let incoming = crate::attachments::Message::decode(&original_line);
        if let Err(error) = incoming.validate() {
            self.restore_composer(&original_line);
            self.persist_draft_now();
            self.flash(error);
            return;
        }
        if incoming.text.starts_with('/') && !incoming.attachments.is_empty() {
            self.restore_composer(&original_line);
            self.persist_draft_now();
            self.flash("Keep attachments with a message; commands leave the draft intact");
            return;
        }
        if incoming.has_pdfs() && self.provider != ProviderKind::Claude {
            self.restore_composer(&original_line);
            self.persist_draft_now();
            self.flash("Native PDF attachments require Claude; select Claude before sending. The draft is retained");
            return;
        }
        let line = incoming.text.clone();
        if let Some(arg) = line
            .strip_prefix("/attach")
            .filter(|arg| arg.is_empty() || arg.starts_with(' '))
        {
            self.image_command(arg);
            return;
        }
        if !line.starts_with('/') {
            self.questions.clear();
        }
        if let Some(arg) = line
            .strip_prefix("/continue")
            .filter(|arg| arg.is_empty() || arg.starts_with(' '))
        {
            if arg.trim().is_empty() {
                self.show_unfinished_request();
                return;
            }
            if self
                .picker
                .as_ref()
                .is_some_and(|picker| picker.kind == PickerKind::Continuation)
            {
                self.dismiss_picker();
            }
            self.continue_usage(arg.trim(), turn_tx);
            return;
        }
        if let Some(arg) = line
            .strip_prefix("/worktree")
            .filter(|arg| arg.is_empty() || arg.starts_with(' '))
        {
            self.managed_workspace_command(arg.trim());
            return;
        }
        if let Some(arg) = line
            .strip_prefix("/scope")
            .filter(|arg| arg.is_empty() || arg.starts_with(' '))
        {
            self.scope_command(arg.trim());
            return;
        }
        if let Some(arg) = line
            .strip_prefix("/continuation")
            .filter(|arg| arg.is_empty() || arg.starts_with(' '))
        {
            self.continuation_command(arg.trim());
            return;
        }
        if self.session_ui.loading() && !line.starts_with('/') {
            self.flash("recent messages are loading; wait before sending");
            if self.editor.text().is_empty() {
                self.editor.set_text(&line);
            }
            return;
        }
        self.view.follow_tail();
        // A user command shows as typed and sends its rendered template as an ordinary turn.
        let mut send = line.clone();
        if let Some(prompt) = crate::user_commands::expand(&self.user_cmds, &line) {
            send = prompt;
        } else if self.handle_slash(&line) {
            self.flush_delegate_prompt(turn_tx);
            return;
        }
        let selected_account = get_use_selection(&self.state_root).is_some_and(|selection| {
            selection.provider == self.provider && selection.account == self.account
        });
        let signed_out_key = format!("{}:{}", self.provider.as_str(), self.account);
        if !line.starts_with('/')
            && selected_account
            && self
                .home_path
                .as_ref()
                .is_some_and(|home| !is_logged_in(self.provider, home))
            && !self.signed_out_overrides.contains(&signed_out_key)
        {
            let confirmed = self
                .signed_out_confirmation
                .as_ref()
                .is_some_and(|(key, prompt)| key == &signed_out_key && prompt == &original_line);
            if !confirmed {
                self.signed_out_confirmation = Some((signed_out_key, original_line.clone()));
                self.push_system(format!(
                    "{} looks signed out. Use /use to sign in, or press Enter again to send anyway.",
                    self.account_label()
                ));
                self.restore_composer(&original_line);
                self.persist_draft_now();
                return;
            }
            self.signed_out_confirmation = None;
            self.signed_out_overrides.insert(signed_out_key);
        } else if !line.starts_with('/') {
            self.signed_out_confirmation = None;
        }
        let handoff = self.continuation.dispatch_request().is_some();
        let request = self
            .continuation
            .dispatch_request()
            .unwrap_or(&original_line)
            .to_owned();
        self.submitted_prompt = Some(request.clone());
        self.continuation
            .begin(&self.account, &self.model, &original_line);
        self.freeze_continuation_routes();
        if let Err(error) = self.bind_main_scope() {
            self.push_system(format!(
                "workspace scope not approved: {error}; request kept"
            ));
            self.restore_composer(&request);
            self.submitted_prompt = None;
            return;
        }
        self.last_sent = Some(request.clone());
        self.ensure_session_title(&line);
        if !line.starts_with('/') && self.task_id.is_none() {
            self.task_id = Some(uuid::Uuid::new_v4().to_string());
        }
        if !self.save_turn_checkpoint(false) {
            self.restore_composer(&request);
            self.submitted_prompt = None;
            self.persist_draft_now();
            return;
        }
        if !handoff {
            self.push_user(incoming.display());
        }
        self.turn_started = Some(Instant::now());
        self.phase = Phase::Waiting;
        self.live_thinking.clear();
        self.busy = true;
        self.pulse_on = true;
        let _ = turn_tx.send(
            crate::attachments::Message {
                text: send,
                attachments: incoming.attachments,
                invalid: None,
            }
            .encode(),
        );
    }

    fn request_cancel(&mut self) {
        self.questions.cancel_native();
        self.native_owner = None;
        if self.cancel_requested {
            return;
        }
        self.cancel_requested |= self.submitted_prompt.is_some();
        self.submitted_prompt = None;
        self.pause_queue();
        if let Some(cancel) = &self.preparing_cancel {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if let Some(h) = &self.cancel_handle {
            h.cancel();
            self.push_system("cancelling...");
        } else if self.cancel_requested {
            // Keep the active turn busy until dispatch observes this flag or a handle arrives.
            self.push_system("cancelling...");
        } else {
            self.busy = false;
            self.push_system("cancelled");
        }
    }

    /// Record the process handle and stop it immediately if cancellation raced with startup.
    fn accept_cancel_handle(&mut self, handle: Arc<RunningTurn>) {
        if self.cancel_requested {
            handle.cancel();
        }
        self.cancel_handle = Some(handle);
    }

    /// Consume a queued turn without starting a provider after an early cancellation request.
    fn discard_cancelled_pending_turn(&mut self) -> bool {
        if !self.cancel_requested {
            return false;
        }
        self.finalize_turn(TurnOutcome {
            cancelled: true,
            ..Default::default()
        });
        self.native_accounting_owner = None;
        self.busy = false;
        self.pulse_on = false;
        self.cancel_handle = None;
        self.cancel_requested = false;
        true
    }
}

impl App {
    fn new(state_root: PathBuf, cwd: PathBuf) -> App {
        let related_files = related_files::State::new(&state_root, &cwd);
        let state_root_for_usage = state_root.clone();
        let state_root_for_cmds = state_root.clone();
        App {
            appearance: appearance::State::load(&state_root),
            continuation: continuation::State::new(&state_root),
            continuation_target: None,
            state_root,
            cwd,
            account_names: Default::default(),
            account_email: None,
            header_email_revealed: false,
            header_email_pressed: false,
            header_home_pressed: false,
            permissions_pressed: false,
            permission_rows: Vec::new(),
            permission_row_pressed: None,
            card_header_pressed: None,
            header_credits_pressed: false,
            messages: vec![ChatLine {
                role: MsgRole::System,
                text: "Type a message and press Enter. Start with / for commands. Drag to select; Ctrl+C copies; Esc interrupts."
                    .into(),
                card: None,
            meta: None,
            }],
            idle_notice: None,
            handoff_after_cancel: false,
            editor: Editor::default(),
            submitted_prompt: None,
            expanded_activity: false,
            activity: activity::Activity::default(),
            palette: Palette::default(),
            key_sheet: false,
            chatref: chatref::ChatRef::default(),
            related_files,
            questions: questions::State::default(),
            native_owner: None,
            native_usage_model: None,
            native_usage_session: None,
            native_accounting_owner: None,
            scopes: workspace_scope::State::default(),
            managed_workspaces: managed_workspaces::State::default(),
            project_search: workspace_search::State::default(),
            file_action_target: None,
            images: images::State::default(),
            raster: raster::State::default(),
            history: Vec::new(),
            hist_pos: None,
            hist_draft: String::new(),
            parked_draft: None,
            fresh_draft: false,
            quit_armed: None,
            busy_send_confirmation: None,
            full_mode_confirmed: false,
            input_scroll: 0,
            input_scroll_anchor: None,
            view: TranscriptView::default(),
            areas: Areas::default(),
            drag: Drag::None,
            file_jobs: Vec::new(),
            workspace: workspace::Workspace::default(),
            file_click: None,
            #[cfg(test)]
            last_file_open: None,
            clicks: ClickTracker::default(),
            modal_close_pressed: None,
            flash: None,
            busy: false,
            pulse_on: false,
            cancel_requested: false,
            tick_count: 0,
            should_quit: false,
            provider: ProviderKind::Codex,
            account: "codex".into(),
            home_path: None,
            model: "?".into(),
            trust: TrustMode::Workspace,
            native_session_id: None,
            task_id: None,
            session_chip: None,
            session_ui: session_ui::State::default(),
            native_agents: native_agents::State::default(),
            footer_left: FOOTER_HINT.into(),
            picker: None,
            doctor: None,
            doctor_applier: Box::new(crate::doctor::DoctorAppliers),
            model_refresh: None,
            token_reads: tokens::State::default(),
            integrations: integrations::State::default(),
            artifacts: artifacts::State::default(),
            landing: None,
            update_preview: None,
            update_rows: 0,
            update_view: 0,
            cancel_handle: None,
            preparing_cancel: None,
            native_sources: Vec::new(),
            prior_provider: ProviderKind::Codex,
            prior_home: None,
            need_context_pack: false,
            task_map: None,
            thread: crate::threads::start(&state_root_for_usage),
            reference_journal: reference_journal::State::default(),
            thread_meta_done: false,
            usage_summary: "limits unknown".into(),
            usage_credits: "Credits: not reported".into(),
            usage_reset: None,
            usage_tone: UsageTone::Mute,
            usage_warning: None,
            usage_fetcher: UsageFetcher::new(state_root_for_usage.clone()),
            usage_block: None,
            stats: appearance::State::load(&state_root_for_usage).ui.stats.then(crate::sysstats::StatsHandle::start),
            compat_rx: None,
            picker_tones: HashMap::new(),
            live_cards: Vec::new(),
            live_text: String::new(),
            streamed_text_committed: false,
            live_thinking: String::new(),
            phase: Phase::Waiting,
            job: None,
            no_color: std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()),
            turn_started: None,
            turn_usage: Default::default(),
            session_input: 0,
            session_output: 0,
            session_cost: 0.0,
            idle_ticks: 0,
            tip_seed: tips::launch_seed(),
            tip_limiter: tips::RateLimiter::new(3),
            pending_login: None,
            connect: Box::new(add_account),
            reexec_path: None,
            deleg: workers::Panel::default(),
            worker_panel_open: false,
            user_cmds: crate::user_commands::scan(&state_root_for_cmds).commands,
            queue: queue::Queue::default(),
            last_sent: None,
            keep: persist::Keep::new(&state_root_for_cmds),
            signed_out_confirmation: None,
            signed_out_overrides: HashSet::new(),
            needs_you: crate::needs_you::Queue::default(),
        }
    }
}

fn select_account(state_root: &Path, account_id: &str) -> anyhow::Result<()> {
    let Some(acct) = get_account(state_root, account_id) else {
        anyhow::bail!("unknown account {account_id}");
    };
    set_use_selection(
        state_root,
        UseSelection {
            provider: acct.provider,
            account: acct.account,
            home_path: acct
                .home_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            model: None,
            updated_at: now_stamp(),
        },
    )
}

const TYPED_MODEL_PREFIX: &str = "typed-model:";

fn model_source_tag(source: crate::models::ModelSource) -> &'static str {
    match source {
        crate::models::ModelSource::ListedByCli => "reported by CLI",
        crate::models::ModelSource::Cached => "cached",
        crate::models::ModelSource::Catalog => "catalog",
        crate::models::ModelSource::Typed => "typed",
    }
}

fn model_picker_rows(
    models: Vec<crate::models::ModelEntry>,
    chosen: Option<&str>,
) -> Vec<PickerRow> {
    let mut rows = vec![PickerRow {
        id: "model:".into(),
        label: format!(
            "provider default{}",
            if chosen.is_none() {
                " Ã‚· current"
            } else {
                ""
            }
        ),
        detail: "the CLI picks its own default model".into(),
    }];
    let mut families = std::collections::BTreeMap::<String, Vec<crate::models::ModelEntry>>::new();
    for model in models {
        families
            .entry(model.family.clone())
            .or_default()
            .push(model);
    }
    for (family, mut entries) in families {
        entries.sort_by_key(|m| (m.deprecated, m.display_name.to_ascii_lowercase()));
        rows.push(PickerRow {
            id: format!("model-group:{family}"),
            label: family,
            detail: String::new(),
        });
        for model in entries {
            let current = chosen == Some(model.id.as_str());
            rows.push(PickerRow {
                id: format!("model:{}", model.id),
                label: format!(
                    "{}{}",
                    model.display_name,
                    if current { " Ã‚· current" } else { "" }
                ),
                detail: format!(
                    "{} Ã‚· {}{}",
                    model.id,
                    model_source_tag(model.source),
                    if model.deprecated {
                        " Ã‚· deprecated"
                    } else {
                        ""
                    }
                ),
            });
        }
    }
    rows
}

/// Typed model ids become one `--model` argv entry; keep them to model-id characters.
fn valid_model_id(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 100
        && !model.contains("..")
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-:/[]".contains(c))
}

fn set_model(state_root: &Path, model: &str) -> anyhow::Result<()> {
    let mut sel = get_use_selection(state_root).ok_or_else(|| anyhow::anyhow!("no selection"))?;
    sel.model = Some(model.to_string()).filter(|m| !m.is_empty());
    sel.updated_at = now_stamp();
    set_use_selection(state_root, sel)
}

fn now_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}Z", dur.as_secs(), dur.subsec_millis())
}

mod draw;
#[cfg(test)]
use draw::{detail_lines_parts, detail_scroll_target_parts};

fn draw_ui(f: &mut Frame, app: &mut App) {
    draw::draw_ui(f, app);
}

fn spinner(tick: u64) -> &'static str {
    draw::spinner(tick)
}

fn provider_color(p: ProviderKind) -> Color {
    draw::provider_color(p)
}

#[cfg(test)]
mod activity_tests;
#[cfg(test)]
mod performance;
#[cfg(test)]
mod queue_controls_tests;
#[cfg(test)]
mod queue_editor_tests;
#[cfg(test)]
mod queue_focus_tests;
#[cfg(test)]
mod queue_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod cancel_tests;

fn reply_header(p: ProviderKind, label: &str) -> TBlock<'_> {
    TBlock {
        identity: None,
        gutter: "",
        gutter_style: Style::default(),
        style: Style::default()
            .fg(draw::provider_color(p))
            .add_modifier(Modifier::BOLD),
        text: label,
        markdown: false,
        linkify: false,
        runs: &[],
        header_files: &[],
    }
}

/// Next permission mode for Shift+Tab: plan -> ask -> workspace -> plan.
fn next_mode(m: TrustMode) -> TrustMode {
    match m {
        TrustMode::Plan => TrustMode::Ask,
        TrustMode::Ask => TrustMode::Workspace,
        TrustMode::Workspace | TrustMode::Full => TrustMode::Plan,
    }
}

fn mode_label(m: TrustMode) -> &'static str {
    match m {
        TrustMode::Plan => "plan",
        TrustMode::Ask => "ask",
        TrustMode::Workspace => "workspace",
        TrustMode::Full => "full",
    }
}

fn mode_blurb(m: TrustMode) -> &'static str {
    match m {
        TrustMode::Plan => "read-only, analyse and propose (advisory where a CLI has no plan flag)",
        TrustMode::Ask => "native approvals (provider support varies)",
        TrustMode::Workspace => "auto-edit inside the workspace",
        TrustMode::Full => "skips all permissions - careful",
    }
}

fn mode_color(m: TrustMode) -> Color {
    match m {
        TrustMode::Plan => Color::Cyan,
        TrustMode::Ask => Color::Yellow,
        TrustMode::Workspace => Color::Green,
        TrustMode::Full => Color::Red,
    }
}

fn parse_mode(s: &str) -> Option<TrustMode> {
    match s.trim().to_lowercase().as_str() {
        "plan" => Some(TrustMode::Plan),
        "ask" => Some(TrustMode::Ask),
        "auto" | "auto-edit" | "autoedit" | "edit" | "workspace" => Some(TrustMode::Workspace),
        "full" => Some(TrustMode::Full),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Info,
    Ok,
    Warn,
    Error,
}

/// Best-effort severity of a system line, from how Concordia words its own messages.
fn level_of(text: &str) -> Level {
    let t = text.trim().to_lowercase();
    if t.contains("failed")
        || t.contains("exited with code")
        || t.contains("could not")
        || t.contains("unknown ")
        || t.contains("error")
        || t.contains("refus")
    {
        Level::Error
    } else if t.starts_with("cancel")
        || t.starts_with("no ")
        || t.starts_with("(no")
        || t.contains("deferred")
        || t.contains("already")
        || t.contains("careful")
    {
        Level::Warn
    } else if t.starts_with("now using")
        || t.starts_with("using ")
        || t.starts_with("model \u{2192}")
        || t.starts_with("mode \u{2192}")
        || t.starts_with("trust \u{2192}")
        || t.starts_with("copied")
        || t.starts_with("cut ")
        || t.contains("up to date")
    {
        Level::Ok
    } else {
        Level::Info
    }
}

/// Fit `text` (segments joined by " \u{b7} ") into `width` columns, dropping whole trailing
/// segments rather than cutting one mid-word.
fn fit_segments(text: &str, width: usize) -> String {
    let segs: Vec<&str> = text.split(" \u{b7} ").collect();
    for keep in (1..=segs.len()).rev() {
        let s = segs[..keep].join(" \u{b7} ");
        if s.chars().count() <= width {
            return s;
        }
    }
    // One segment alone is too long: cut at a word boundary and mark it.
    if width < 2 {
        return String::new();
    }
    let mut cut: String = segs[0].chars().take(width - 1).collect();
    if let Some(sp) = cut.rfind(' ') {
        cut.truncate(sp);
    }
    format!("{cut}\u{2026}")
}

/// Footer hints as (full label, short label), listed in display order.
const HINTS: [(&str, &str); 6] = [
    ("Enter send", "Enter send"),
    ("F9 attach", "F9 attach"),
    ("/ commands", "/ cmds"),
    ("Shift+Tab permissions", "Shift+Tab perms"),
    ("drag to select", "drag select"),
    ("Ctrl+C copy or cancel", "Ctrl+C copy/stop"),
];
/// Priority per hint (0 is kept longest); indexes follow `HINTS`.
const HINT_RANK: [usize; 6] = [0, 1, 3, 4, 5, 2];

/// The default hint line for `width` columns: whole hints only, full labels first, then short
/// ones; lowest-priority hints are dropped first.
fn fit_hints(width: usize) -> String {
    for keep in (1..=HINTS.len()).rev() {
        for short in [false, true] {
            let s = HINTS
                .iter()
                .enumerate()
                .filter(|(i, _)| HINT_RANK[*i] < keep)
                .map(|(_, h)| if short { h.1 } else { h.0 })
                .collect::<Vec<_>>()
                .join(" \u{b7} ");
            if s.chars().count() <= width {
                return s;
            }
        }
    }
    String::new()
}

/// Render markup before shortening so a preview never exposes half a Markdown delimiter.
fn condense_thinking(raw: &str) -> String {
    markdown::thinking_preview(raw, THINKING_PREVIEW_CHARS)
}

const THINKING_PREVIEW_CHARS: usize = 600;

/// Whether the collapsed preview hides anything: it keeps only the tail and drops code fences.
fn thinking_expandable(raw: &str) -> bool {
    raw.trim().chars().count() > THINKING_PREVIEW_CHARS || raw.contains("```")
}

fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Provider chatter on stderr that tells the user nothing.
fn is_cli_noise(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.starts_with("reading additional input from stdin")
        || l.starts_with("reading prompt from stdin")
}

#[cfg(test)]
mod screenshots;
