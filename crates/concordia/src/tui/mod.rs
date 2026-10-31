use crate::accounts::{accounts_with_auth, display_account_name, get_account, is_logged_in};
use crate::catalog::list_models;
use crate::commands::{dispatch_slash, parse_slash, CommandContext, CommandOutcome, SlashCommand};
use crate::connect::{add_account, AddResult};
use crate::context::{plan_continuity, ContinuityRequest, PackMessage, PackRole};
use crate::identity::{format_identity_label, inspect_provider};
use crate::provider::stream_cards::{format_card_line, upsert_card, CardStatus, StreamCard};
use crate::provider::turn::{
    plan_provider_turn, run_provider_turn_with_progress, ProviderTurnRequest, RunningTurn,
    TurnOutcome, TurnProgress,
};
use crate::sessions::{
    count_sessions_here, list_recent_sessions, sessions_to_picker_rows, trust_picker_rows,
    PickerRow,
};
use crate::store::{
    default_store_root, ensure_default_selection, get_settings, get_use_selection, set_trust_mode,
    set_use_selection, ProviderKind, TrustMode, UseSelection,
};
use crate::update::{
    reexec_into, start_check, start_update, Job, JobKind, JobResult, UpdateOutcome,
};
use crate::usage::{picker_row, usage_rows_for_accounts, usage_tone, UsageFetcher, UsageTone};
use crate::version::short_stamp;
use anyhow::Result;
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags, MouseButton,
    MouseEvent, MouseEventKind, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Widget};
use ratatui::{Frame, Terminal};
use std::collections::HashMap;
use std::io::{self, stdout};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

mod activity;
mod chatref;
mod clipboard;
mod compat_ui;
mod continuation;
mod editor;
mod files;
mod input;
mod input_stream;
mod markdown;
mod native_agents;
mod palette;
mod persist;
mod preview;
mod queue;
mod queue_controls;
mod queue_focus;
mod recovery;
mod recovery_ui;
mod session_ui;
#[cfg(test)]
mod session_ui_tests;
mod stats_ui;
mod text;
mod tips;
mod token_line;
mod transcript;
mod usage_style;
mod workers;
mod workspace;

use editor::Editor;
use palette::Palette;
use transcript::{Block as TBlock, TranscriptView};

/// Prompt box grows with its content up to this many text rows, then scrolls.
const MAX_INPUT_ROWS: usize = 6;
/// Columns taken by the `> ` prompt (and the matching indent on continuation rows).
const PROMPT_W: usize = 2;
const MULTI_CLICK: Duration = Duration::from_millis(500);
const FLASH_FOR: Duration = Duration::from_secs(3);
const FOOTER_HINT: &str = "Enter send \u{b7} / commands \u{b7} Shift+Tab mode \u{b7} drag to select \u{b7} Ctrl+C copy or cancel";
/// How often (in 200 ms ticks) to look for a finished login, and when to stop looking.
/// A second idle Ctrl+C within this window exits.
const QUIT_WINDOW: Duration = Duration::from_secs(2);
const HISTORY_MAX: usize = persist::HISTORY_CAP;
const LOGIN_POLL_TICKS: u32 = 10;
const LOGIN_GIVE_UP_TICKS: u32 = 3000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    None,
    Transcript,
    Input,
}

/// Screen rectangles from the last draw, for mouse hit-testing.
#[derive(Default, Clone, Copy)]
struct Areas {
    transcript: Rect,
    input: Rect,
    /// Columns available for prompt text (inner width minus the `> ` prefix).
    input_text_w: usize,
    /// Slash-command suggestions (empty rect when closed); rows start one below `y`.
    palette: Rect,
}

/// Counts rapid clicks on the same cell: 1 = click, 2 = double (word), 3 = triple (line).
#[derive(Default)]
struct ClickTracker {
    last: Option<(u16, u16, Instant, u8)>,
}

impl ClickTracker {
    fn register(&mut self, x: u16, y: u16, now: Instant) -> u8 {
        let n = match self.last {
            Some((lx, ly, at, n))
                if lx == x && ly == y && now.duration_since(at) <= MULTI_CLICK =>
            {
                n % 3 + 1
            }
            _ => 1,
        };
        self.last = Some((x, y, now, n));
        n
    }
}

fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MsgRole {
    User,
    Assistant,
    System,
    /// The echo of a slash command, titling the output block that follows it.
    Command,
    Card,
    /// The model's reasoning for a finished turn: dimmed, never part of the context pack.
    Thinking,
    /// The token line under a reply: dim, never part of the context pack or /copy.
    Meta,
}

/// What the running turn is doing right now, so the wait is never a blank spinner.
#[derive(Clone, PartialEq, Eq)]
enum Phase {
    /// Sent, nothing back yet.
    Waiting,
    Thinking,
    Tool(String),
    Writing,
}

impl Phase {
    fn label(&self) -> String {
        match self {
            Phase::Waiting => "waiting for the model".into(),
            Phase::Thinking => "thinking".into(),
            Phase::Tool(name) => format!("running {name}"),
            Phase::Writing => "writing".into(),
        }
    }

    fn color(&self) -> Color {
        match self {
            Phase::Waiting => Color::Cyan,
            Phase::Thinking => Color::Magenta,
            Phase::Tool(_) => Color::Yellow,
            Phase::Writing => Color::Green,
        }
    }

    /// Each phase has its own width-1 braille animation, so the state reads from motion alone.
    fn frames(&self) -> &'static [&'static str] {
        match self {
            Phase::Waiting => &SPINNER,
            Phase::Thinking => &[
                "\u{2801}", "\u{2802}", "\u{2804}", "\u{2840}", "\u{2880}", "\u{2820}", "\u{2810}",
                "\u{2808}",
            ],
            Phase::Tool(_) => &[
                "\u{28fe}", "\u{28fd}", "\u{28fb}", "\u{28bf}", "\u{287f}", "\u{28df}", "\u{28ef}",
                "\u{28f7}",
            ],
            Phase::Writing => &[
                "\u{2809}", "\u{2819}", "\u{281a}", "\u{2812}", "\u{2832}", "\u{2834}", "\u{2824}",
                "\u{2834}", "\u{2832}", "\u{2812}", "\u{281a}", "\u{2819}",
            ],
        }
    }

    fn frame(&self, tick: u64) -> &'static str {
        let f = self.frames();
        f[(tick as usize) % f.len()]
    }
}

#[derive(Clone)]
struct ChatLine {
    role: MsgRole,
    text: String,
    card: Option<StreamCard>,
    /// Who produced an agent reply (`Provider · account · model`), so each turn is labelled.
    meta: Option<(ProviderKind, String)>,
}

type ConnectFn = Box<dyn Fn(&Path, ProviderKind) -> Result<AddResult>>;

/// A login window we opened; polled until the home reports a usable login.
struct PendingLogin {
    account: String,
    provider: ProviderKind,
    label: String,
    home: PathBuf,
    ticks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerKind {
    Agents,
    Recovery,
    Resume,
    Trust,
    Use,
    AddProvider,
    Remove,
    RemoveConfirm,
    Model,
    Usage,
    Continuation,
    ResumeDeleteConfirm,
}

struct Picker {
    kind: PickerKind,
    title: String,
    rows: Vec<PickerRow>,
    filter: String,
    state: ListState,
}

impl Picker {
    fn filtered(&self) -> Vec<&PickerRow> {
        let q = self.filter.to_ascii_lowercase();
        if q.is_empty() {
            return self.rows.iter().collect();
        }
        self.rows
            .iter()
            .filter(|r| {
                r.label.to_ascii_lowercase().contains(&q)
                    || r.detail.to_ascii_lowercase().contains(&q)
            })
            .collect()
    }

    fn select_clamp(&mut self) {
        let n = self.filtered().len();
        if n == 0 {
            self.state.select(None);
            return;
        }
        let i = self.state.selected().unwrap_or(0).min(n - 1);
        self.state.select(Some(i));
    }
}

pub struct App {
    continuation: continuation::State,
    continuation_target: Option<String>,
    state_root: PathBuf,
    cwd: PathBuf,
    messages: Vec<ChatLine>,
    editor: Editor,
    /// The visible prompt for the active turn; cancellation never inserts it into the editor.
    submitted_prompt: Option<String>,
    expanded_activity: bool,
    activity: activity::Activity,
    palette: Palette,
    chatref: chatref::ChatRef,
    /// Sent prompts, oldest first. In memory only: prompts may contain pasted secrets.
    history: Vec<String>,
    /// Index into `history` while browsing with the arrow keys.
    hist_pos: Option<usize>,
    /// What was in the prompt before browsing started, restored by Down past the newest entry.
    hist_draft: String,
    /// Set by the first idle Ctrl+C; a second one inside QUIT_WINDOW exits.
    quit_armed: Option<Instant>,
    /// First visible prompt row when the prompt is taller than its box.
    input_scroll: usize,
    view: TranscriptView,
    areas: Areas,
    drag: Drag,
    file_jobs: Vec<mpsc::UnboundedReceiver<String>>,
    workspace: workspace::Workspace,
    file_click: Option<transcript::Pos>,
    #[cfg(test)]
    last_file_open: Option<crate::file_links::ResolvedFile>,
    clicks: ClickTracker,
    /// Transient footer message (e.g. "copied 120 chars").
    flash: Option<(String, Instant)>,
    busy: bool,
    pulse_on: bool,
    /// Cancellation requested before the runner has provided its process handle.
    cancel_requested: bool,
    tick_count: u64,
    should_quit: bool,
    provider: ProviderKind,
    account: String,
    home_path: Option<PathBuf>,
    model: String,
    trust: TrustMode,
    native_session_id: Option<String>,
    session_chip: Option<String>,
    session_ui: session_ui::State,
    native_agents: native_agents::State,
    footer_left: String,
    picker: Option<Picker>,
    cancel_handle: Option<Arc<RunningTurn>>,
    prior_provider: ProviderKind,
    prior_home: Option<String>,
    need_context_pack: bool,
    /// Opt-in thread log for this conversation (see threads.rs); None when recording is off.
    thread: Option<crate::threads::ThreadLog>,
    thread_meta_done: bool,
    usage_summary: String,
    usage_tone: UsageTone,
    usage_warning: Option<String>,
    /// Width-1 ASCII bar drawn before the footer usage text.
    /// Claude limits are fetched on a worker thread and cached; never block the UI on them.
    usage_fetcher: UsageFetcher,
    /// Index of the `/usage` result block, rewritten in place when a background check lands.
    usage_block: Option<usize>,
    /// Footer process stats while switched on (`[ui] stats` or `/stats`).
    stats: Option<crate::sysstats::StatsHandle>,
    /// Provider capability detection started at launch (see `provider::compat`).
    compat_rx: Option<compat_ui::CompatRx>,
    /// Tone of each `/use` or `/usage all` picker row, by row id.
    picker_tones: HashMap<String, UsageTone>,
    live_cards: Vec<StreamCard>,
    live_text: String,
    streamed_text_committed: bool,
    live_thinking: String,
    phase: Phase,
    /// A background `/update` or `/update check`; the UI stays live while it runs.
    job: Option<Job>,
    /// `NO_COLOR` is set: draw with the terminal's own colours only.
    no_color: bool,
    turn_started: Option<Instant>,
    /// What the provider reported for the running turn (summed if it reports more than once).
    turn_usage: crate::provider::stream_cards::TurnUsage,
    /// Session totals (tokens and cost the providers reported).
    session_input: u64,
    session_output: u64,
    session_cost: f64,
    /// Ticks spent idle with an empty prompt; drives the mid-session tip.
    idle_ticks: u64,
    tip_seed: u64,
    pending_login: Option<PendingLogin>,
    /// Account-connect entry point; swapped in tests so no login window ever opens.
    connect: ConnectFn,
    /// Set when /update succeeds — main loop tears down TUI then re-execs.
    reexec_path: Option<PathBuf>,
    /// Delegated workers: the jobs, the strip above the prompt, the viewer.
    deleg: workers::Panel,
    /// `<state>/commands/*.md`, scanned at start (see `user_commands`).
    user_cmds: Vec<crate::user_commands::UserCommand>,
    /// Prompts lined up behind the running turn (see queue.rs).
    queue: queue::Queue,
    /// The prompt of the turn in flight, offered for retry only if the turn fails.
    last_sent: Option<String>,
    /// What the draft/history persistence has written (see persist.rs).
    keep: persist::Keep,
}

impl App {
    fn refresh_binding(&mut self) {
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
        self.trust = get_settings(&self.state_root).trust;
        self.refresh_usage();
    }

    fn refresh_usage(&mut self) {
        let home = self.home_path.clone().or_else(|| {
            get_use_selection(&self.state_root)
                .and_then(|s| s.home_path)
                .map(PathBuf::from)
        });
        let Some(home) = home else {
            self.usage_summary = "no account yet".into();
            self.usage_tone = UsageTone::Mute;
            self.usage_warning = None;
            return;
        };
        let snap = self
            .usage_fetcher
            .snapshot(self.provider, &self.account, &home);
        self.set_usage_footer(&snap);
    }

    fn set_usage_footer(&mut self, snap: &crate::usage::UsageSnapshot) {
        self.usage_summary = snap.summary.clone();
        self.usage_tone = usage_tone(snap);
        self.usage_warning = crate::usage::low_limit_warning(snap, self.provider);
    }

    /// `/usage`: the current account's limits as one block. It reads the cache (a check starts in
    /// the background when it is stale, and `/usage refresh` forces one) and is rewritten in place
    /// when the answer arrives, so it never shows a stale "checking" line.
    fn show_usage(&mut self, force: bool) {
        let Some(home) = self.home_path.clone() else {
            self.push_system("usage - no account selected; try /use");
            return;
        };
        let snap = if force {
            self.usage_fetcher
                .snapshot_refresh(self.provider, &self.account, &home)
        } else {
            self.usage_fetcher
                .snapshot(self.provider, &self.account, &home)
        };
        self.set_usage_footer(&snap);
        self.push_command(if force { "/usage refresh" } else { "/usage" });
        self.messages.push(ChatLine {
            role: MsgRole::System,
            text: self.usage_block_text(&snap),
            card: None,
            meta: None,
        });
        self.usage_block = Some(self.messages.len() - 1);
    }

    /// Header (account) plus one line per limit window, as a single message.
    fn usage_block_text(&self, snap: &crate::usage::UsageSnapshot) -> String {
        let body = snap.detail.trim();
        format!("{} - limits\n{}", self.account_label(), body)
    }

    /// Rewrite the last `/usage` block with the newest reading (cache only, never blocks).
    fn update_usage_block(&mut self) {
        let Some(idx) = self.usage_block else {
            return;
        };
        let Some(home) = self.home_path.clone() else {
            return;
        };
        if self
            .messages
            .get(idx)
            .is_none_or(|m| m.role != MsgRole::System)
        {
            self.usage_block = None;
            return;
        }
        let snap = self
            .usage_fetcher
            .snapshot(self.provider, &self.account, &home);
        self.messages[idx].text = self.usage_block_text(&snap);
    }

    fn account_label(&self) -> String {
        display_account_name(self.provider, &self.account)
    }

    fn window_title(&self) -> String {
        let base = format!(
            "concordia · {} · {} · {}",
            self.account_label(),
            if self.model.is_empty() {
                "?"
            } else {
                &self.model
            },
            short_stamp()
        );
        self.session_ui
            .title
            .as_ref()
            .map(|title| format!("{title} · {base}"))
            .unwrap_or(base)
    }

    /// Suggestions for the prompt as typed (empty when closed, dismissed, or not a slash prompt).
    fn palette_entries(&self) -> Vec<palette::Entry> {
        // Not while a recalled prompt is on screen: up/down must keep walking history.
        if self.palette.dismissed || self.picker.is_some() || self.hist_pos.is_some() {
            return Vec::new();
        }
        palette::matching_with(&self.editor.text(), &self.user_cmds)
    }

    /// Put the chosen command in the prompt.
    fn complete_palette(&mut self, entry: palette::Entry) {
        self.editor.set_text(entry.command);
        self.palette.reset();
    }

    /// Tab: complete the highlighted suggestion.
    fn tab_complete_slash(&mut self) {
        let entries = self.palette_entries();
        if let Some(e) = entries.get(self.palette.selected.min(entries.len().saturating_sub(1))) {
            let e = *e;
            self.complete_palette(e);
        }
    }

    fn remember_prompt(&mut self, line: &str) {
        self.hist_pos = None;
        self.push_history(line);
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let pos = match self.hist_pos {
            None => {
                self.hist_draft = self.editor.text();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.hist_pos = Some(pos);
        self.editor.set_text(&self.history[pos].clone());
    }

    fn history_next(&mut self) {
        let Some(i) = self.hist_pos else { return };
        if i + 1 < self.history.len() {
            self.hist_pos = Some(i + 1);
            self.editor.set_text(&self.history[i + 1].clone());
        } else {
            self.hist_pos = None;
            let draft = std::mem::take(&mut self.hist_draft);
            self.editor.set_text(&draft);
        }
    }

    /// After Ctrl+Z / Ctrl+Y: the text changed under the palette and any history browsing.
    fn after_undo(&mut self) {
        self.palette.reset();
        self.hist_pos = None;
        self.view.clear_selection();
    }

    fn flash(&mut self, msg: impl Into<String>) {
        self.flash = Some((msg.into(), Instant::now()));
    }

    fn paste_clipboard(&mut self) {
        match clipboard::get_text() {
            Ok(t) if !t.is_empty() => self.paste_text(&t),
            Ok(_) => {}
            Err(e) => self.flash(format!("paste failed — {e}")),
        }
    }

    /// Insert pasted text into the prompt (replacing any selection).
    fn paste_text(&mut self, text: &str) {
        self.paste_text_chunk(text, false);
    }

    fn paste_text_chunk(&mut self, text: &str, continues: bool) {
        // Paste can arrive as one Event::Paste or as a buffered burst split into overflow
        // chunks. Route every chunk here so a modal never edits the prompt or queue editor.
        if self.activity.detail.is_some() {
            self.activity.insert_query(text);
            return;
        }
        if let Some(picker) = self.picker.as_mut() {
            let clean: String = text
                .chars()
                .filter(|c| *c != '\r')
                .flat_map(|c| if c == '\t' { vec![' '; 4] } else { vec![c] })
                .collect();
            picker.filter.push_str(&clean);
            picker.select_clamp();
            return;
        }
        if queue::paste(self, text, continues) {
            return;
        }
        if self.workspace.focused.is_some() {
            self.flash("read-only file pane; Esc returns to chat before pasting");
            return;
        }
        self.view.clear_selection();
        self.palette.reset();
        self.editor.insert_paste_chunk(text, continues);
    }

    /// Selected text in the transcript, else in the prompt.
    fn selected_text(&self) -> Option<String> {
        self.view
            .selected_text()
            .or_else(|| self.editor.selected_text())
    }

    fn has_selection(&self) -> bool {
        self.selected_text().is_some()
    }

    fn clear_selections(&mut self) {
        self.view.clear_selection();
        self.editor.clear_selection();
        self.drag = Drag::None;
    }

    fn last_assistant_text(&self) -> Option<String> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == MsgRole::Assistant && !m.text.trim().is_empty())
            .map(|m| m.text.clone())
    }

    fn navigate_activity_card(&mut self, older: bool) {
        let current = self.activity.detail.unwrap_or(self.messages.len());
        let next = if older {
            (0..current).rev().find(|&i| {
                self.messages
                    .get(i)
                    .is_some_and(|m| m.role == MsgRole::Card)
            })
        } else {
            (current + 1..self.messages.len()).find(|&i| {
                self.messages
                    .get(i)
                    .is_some_and(|m| m.role == MsgRole::Card)
            })
        };
        if let Some(i) = next {
            self.activity.open(i);
        }
    }

    fn write_clipboard(&mut self, text: &str) {
        match clipboard::set_text(text) {
            Ok(()) => self.flash(format!("copied {} chars", text.chars().count())),
            Err(e) => self.flash(e),
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
            Some(t) => {
                self.write_clipboard(&t);
                self.clear_selections();
                true
            }
            None => {
                self.flash("nothing to copy — drag to select, or /copy for the last reply");
                false
            }
        }
    }

    fn cut(&mut self) {
        if let Some(t) = self.editor.cut() {
            self.write_clipboard(&t);
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

    fn thread_id(&self) -> Option<&str> {
        self.thread.as_ref().map(|t| t.id())
    }

    /// Append to the thread log when recording is on. Errors are ignored: logging is best effort.
    fn log_event(&mut self, ev: crate::threads::ThreadEvent) {
        if let Some(t) = &self.thread {
            let _ = t.append(&ev);
        }
    }

    fn log_who(&self, ev: crate::threads::ThreadEvent) -> crate::threads::ThreadEvent {
        ev.who(self.provider.as_str(), &self.account, &self.model)
    }

    fn log_user(&mut self, text: &str) {
        if let Some(t) = &self.thread {
            if !self.thread_meta_done {
                self.thread_meta_done = true;
                let _ = t.write_meta(&crate::threads::ThreadMeta {
                    id: t.id().to_string(),
                    started: crate::threads::now_secs(),
                    title: self
                        .session_ui
                        .title
                        .clone()
                        .unwrap_or_else(|| crate::threads::title_from_prompt(text)),
                    cwd: self.cwd.to_string_lossy().into_owned(),
                    account: Some(self.account.clone()),
                });
            }
        }
        self.log_event(crate::threads::ThreadEvent::with_text("user", text));
    }

    fn log_switch(&mut self, from: ProviderKind) {
        let ev = crate::threads::ThreadEvent::with_text(
            "switch",
            &format!("{} -> {}", from.as_str(), self.account),
        );
        let ev = self.log_who(ev);
        self.log_event(ev);
    }

    fn push_user(&mut self, text: String) {
        self.log_user(&text);
        self.messages.push(ChatLine {
            role: MsgRole::User,
            text,
            card: None,
            meta: None,
        });
    }

    fn reply_meta(&self) -> (ProviderKind, String) {
        let mut label = format!("{} \u{b7} {}", provider_name(self.provider), self.account);
        if !self.model.is_empty() && self.model != "?" {
            label.push_str(&format!(" \u{b7} {}", self.model));
        }
        (self.provider, label)
    }

    fn push_command(&mut self, line: &str) {
        self.messages.push(ChatLine {
            role: MsgRole::Command,
            text: line.trim().to_string(),
            card: None,
            meta: None,
        });
    }

    fn push_assistant(&mut self, text: String) {
        let ev = self.log_who(crate::threads::ThreadEvent::with_text("assistant", &text));
        self.log_event(ev);
        let meta = Some(self.reply_meta());
        self.messages.push(ChatLine {
            role: MsgRole::Assistant,
            text,
            card: None,
            meta,
        });
    }

    fn push_card(&mut self, card: StreamCard) {
        if card.status != CardStatus::Running {
            let status = if card.status == CardStatus::Error {
                "error"
            } else {
                "done"
            };
            let ev = self.log_who(crate::threads::ThreadEvent::card(
                &card.title,
                card.detail.as_deref(),
                status,
            ));
            self.log_event(ev);
        }
        let text = format_card_line(&card);
        let turn_start = self
            .messages
            .iter()
            .rposition(|m| m.role == MsgRole::User)
            .unwrap_or(0);
        let existing_index = (turn_start..self.messages.len()).rev().find(|&i| {
            let m = &self.messages[i];
            m.role == MsgRole::Card
                && m.card.as_ref().map(|c| c.id.as_str()) == Some(card.id.as_str())
        });
        if let Some(i) = existing_index {
            let existing = &mut self.messages[i];
            existing.card = Some(card);
            existing.text = text;
            self.native_agents
                .record(existing.card.as_ref().unwrap(), i);
            if self.activity.detail == Some(i) {
                self.activity.invalidate_search();
            }
            self.refresh_agents_picker();
            return;
        }
        let meta = Some(self.reply_meta());
        self.messages.push(ChatLine {
            role: MsgRole::Card,
            text,
            card: Some(card),
            meta,
        });
        if let Some(card) = self.messages.last().and_then(|m| m.card.as_ref()) {
            self.native_agents.record(card, self.messages.len() - 1);
        }
        self.refresh_agents_picker();
    }

    fn apply_progress(&mut self, ev: TurnProgress) {
        match ev {
            TurnProgress::Session(id) => {
                self.native_session_id = Some(id);
                self.persist_session_title();
            }
            TurnProgress::Thinking(t) => {
                self.commit_live_text();
                self.phase = Phase::Thinking;
                self.live_thinking.push_str(&t);
                if !t.ends_with('\n') {
                    self.live_thinking.push('\n');
                }
            }
            TurnProgress::ThinkingDelta(t) => {
                self.commit_live_text();
                self.phase = Phase::Thinking;
                self.live_thinking.push_str(&t);
            }
            TurnProgress::Text(t) => {
                self.commit_live_thinking();
                self.phase = Phase::Writing;
                self.live_text.push_str(&t);
                if !t.ends_with('\n') {
                    self.live_text.push('\n');
                }
            }
            TurnProgress::TextDelta(t) => {
                self.commit_live_thinking();
                self.phase = Phase::Writing;
                self.live_text.push_str(&t);
            }
            TurnProgress::Card(card) => {
                self.commit_live_thinking();
                self.commit_live_text();
                self.phase = if card.status == CardStatus::Running {
                    Phase::Tool(card.title.clone())
                } else {
                    Phase::Waiting
                };
                let id = card.id.clone();
                upsert_card(&mut self.live_cards, card);
                if let Some(merged) = self.live_cards.iter().find(|c| c.id == id).cloned() {
                    self.push_card(merged);
                }
            }
            TurnProgress::Notice(n) => self.push_system(n),
            TurnProgress::Error(e) => {
                if self.continuation.errors.len() < 12 {
                    self.continuation
                        .errors
                        .push(e.chars().take(2000).collect());
                }
                self.push_system(e);
            }
            TurnProgress::Usage(u) => {
                fn add(a: &mut Option<u64>, b: Option<u64>) {
                    if let Some(b) = b {
                        *a = Some(a.unwrap_or(0) + b);
                    }
                }
                let t = &mut self.turn_usage;
                add(&mut t.input, u.input);
                add(&mut t.output, u.output);
                add(&mut t.cached, u.cached);
                if let Some(c) = u.cost_usd {
                    t.cost_usd = Some(t.cost_usd.unwrap_or(0.0) + c);
                }
            }
        }
    }

    fn commit_live_text(&mut self) {
        let text = std::mem::take(&mut self.live_text);
        if !text.trim().is_empty() {
            self.push_assistant(text.trim().to_string());
            self.streamed_text_committed = true;
        }
    }

    fn commit_live_thinking(&mut self) {
        let text = std::mem::take(&mut self.live_thinking);
        if !text.is_empty() {
            let meta = Some(self.reply_meta());
            self.messages.push(ChatLine {
                role: MsgRole::Thinking,
                text,
                card: None,
                meta,
            });
        }
    }

    /// The tip to show dimmed under the transcript, if the moment is right.
    fn visible_tip(&self) -> Option<&'static str> {
        let quiet = !self.busy
            && self.picker.is_none()
            && self.job.is_none()
            && self.pending_login.is_none()
            && self.editor.chars().is_empty();
        let fresh = self.messages.iter().all(|m| m.role == MsgRole::System);
        if !quiet || !(fresh || self.idle_ticks >= tips::IDLE_TICKS) || !tips::ui_tips_enabled() {
            return None;
        }
        Some(tips::tip_for(self.tip_seed))
    }

    fn finalize_turn(&mut self, outcome: TurnOutcome) {
        self.submitted_prompt = None;
        let elapsed = self.turn_started.map(|t| t.elapsed());
        let usage = std::mem::take(&mut self.turn_usage);
        self.session_input += usage.input.unwrap_or(0);
        self.session_output += usage.output.unwrap_or(0);
        self.session_cost += usage.cost_usd.unwrap_or(0.0);
        // Events only: never the prompt or the reply.
        crate::logging::info(&format!(
            "turn provider={} account={} exit={:?} cancelled={} ms={} stderr_lines={}",
            self.provider.as_str(),
            self.account,
            outcome.exit_code,
            outcome.cancelled,
            self.turn_started
                .take()
                .map(|t| t.elapsed().as_millis())
                .unwrap_or(0),
            outcome.stderr_tail.lines().count()
        ));
        if let Some(sid) = outcome.session_id {
            self.native_session_id = Some(sid);
            self.persist_session_title();
        }
        let had_cards = !self.live_cards.is_empty() || !outcome.cards.is_empty();
        let failed = !outcome.cancelled
            && (outcome.failed
                || match outcome.exit_code {
                    Some(code) => code != 0,
                    None => !outcome.stderr_tail.trim().is_empty(),
                });
        let finish_incomplete = |mut card: StreamCard| {
            card.status = CardStatus::Error;
            let reason = if outcome.cancelled {
                "Interrupted: turn cancelled before the provider reported tool completion."
                    .to_string()
            } else if failed {
                match outcome.exit_code {
                    Some(code) => format!(
                        "Provider failed with exit code {code} before reporting tool completion."
                    ),
                    None => "Provider failed before reporting tool completion.".to_string(),
                }
            } else {
                "Incomplete: provider stream ended before reporting tool completion.".to_string()
            };
            card.detail = Some(match card.detail.take() {
                Some(detail) if !detail.is_empty() => format!("{detail}\n{reason}"),
                _ => reason,
            });
            card
        };
        // Prefer streamed cards; fall back to outcome cards if nothing landed live.
        if self.live_cards.is_empty() {
            for card in outcome.cards {
                let card = if card.status == CardStatus::Running {
                    finish_incomplete(card)
                } else {
                    card
                };
                self.push_card(card);
            }
        } else {
            let running: Vec<_> = self
                .live_cards
                .iter()
                .filter(|c| c.status == CardStatus::Running)
                .cloned()
                .collect();
            for mut card in running {
                card = finish_incomplete(card);
                self.push_card(card);
            }
        }
        let text = if !self.live_text.trim().is_empty() {
            self.live_text.trim().to_string()
        } else if !self.streamed_text_committed && !outcome.cancelled {
            outcome.text.trim().to_string()
        } else {
            String::new()
        };
        let scan_text = if outcome.cancelled {
            String::new()
        } else if !outcome.text.is_empty() {
            outcome.text.clone()
        } else {
            text.clone()
        };
        // A provider that failed to start, or exited non-zero, must say why. Its stderr is the
        // only place the reason (expired login, bad flag, missing CLI) ever appears.
        let thought = self.live_thinking.clone();
        if !thought.is_empty() {
            let meta = Some(self.reply_meta());
            self.messages.push(ChatLine {
                role: MsgRole::Thinking,
                text: thought,
                card: None,
                meta,
            });
        }
        let has_reply = !text.is_empty() || self.streamed_text_committed;
        if !text.is_empty() {
            self.push_assistant(text);
        } else if !had_cards && !failed && !outcome.cancelled && !self.streamed_text_committed {
            self.push_system("(no text in provider stream)");
        }
        if has_reply && !outcome.cancelled && token_line::ui_token_line_enabled() {
            let line = token_line::format_line(&usage, elapsed);
            if !line.is_empty() {
                self.messages.push(ChatLine {
                    role: MsgRole::Meta,
                    text: line,
                    card: None,
                    meta: None,
                });
            }
        }
        if outcome.cancelled {
            self.push_system("cancelled - queued work paused; completed actions were not undone");
        }
        if failed && outcome.stderr_tail.contains("already has an active writer") {
            self.push_system("this Codex session is open in another client or pane. Close its active turn, then retry the restored prompt; /new starts a separate conversation. Concordia will not replay it automatically.");
        }
        if failed {
            self.push_system(match outcome.exit_code {
                Some(0) => "provider reported a turn failure".to_string(),
                Some(code) => format!("provider exited with code {code}"),
                None => "provider could not run".to_string(),
            });
            let tail = outcome.stderr_tail.trim();
            let lines: Vec<&str> = tail.lines().collect();
            let said: Vec<String> = self
                .messages
                .iter()
                .rev()
                .take(8)
                .filter(|m| m.role == MsgRole::System)
                .map(|m| m.text.trim().to_string())
                .collect();
            for l in lines.iter().skip(lines.len().saturating_sub(6)) {
                let t = l.trim();
                // Skip CLI chatter and anything already reported above.
                if t.is_empty() || is_cli_noise(t) || said.iter().any(|s| s == t) {
                    continue;
                }
                self.push_system(format!("  {}", l.trim_end()));
            }
        }
        self.live_cards.clear();
        self.live_text.clear();
        self.streamed_text_committed = false;
        self.live_thinking.clear();
        self.turn_finished(outcome.cancelled, failed);
        self.scan_delegation(&scan_text);
        // The turn used some of this account's limits: re-check them in the background.
        self.usage_fetcher.invalidate(&self.account);
        self.refresh_usage();
        self.update_usage_block();
    }

    /// `/threads` lists recent recorded threads; `/threads delete <id>` removes one.
    fn threads_command(&mut self, arg: &str) -> Vec<String> {
        use crate::threads;
        if let Some(id) = arg.strip_prefix("delete").map(str::trim) {
            if id.is_empty() {
                return vec!["usage: /threads delete <id>".into()];
            }
            return match threads::resolve_prefix(&self.state_root, id) {
                Ok(full) if Some(full.as_str()) == self.thread_id() => {
                    vec!["that is the current conversation; /new first".into()]
                }
                Ok(full) => match threads::delete(&self.state_root, &full) {
                    Ok(()) => vec![format!("deleted thread {full}")],
                    Err(e) => vec![format!("delete failed: {e:#}")],
                },
                Err(e) => vec![e.to_string()],
            };
        }
        let metas = threads::list(&self.state_root);
        if metas.is_empty() {
            return vec![if threads::enabled(&self.state_root) {
                "no recorded threads yet".into()
            } else {
                "thread log is off (set CONCORDIA_THREADS=1 to record conversations)".into()
            }];
        }
        metas
            .iter()
            .take(15)
            .map(|m| {
                format!(
                    "{}  {:>4}  {}  {}",
                    m.id.chars().take(8).collect::<String>(),
                    threads::age_label(m.started),
                    m.account.as_deref().unwrap_or("-"),
                    m.title
                )
            })
            .collect()
    }

    fn pack_messages(&self) -> Vec<PackMessage> {
        self.messages
            .iter()
            .filter_map(|m| match m.role {
                MsgRole::User => Some(PackMessage {
                    role: PackRole::User,
                    text: m.text.clone(),
                }),
                MsgRole::Assistant => Some(PackMessage {
                    role: PackRole::Assistant,
                    text: m.text.clone(),
                }),
                MsgRole::Card => m.card.as_ref().map(|c| PackMessage {
                    role: PackRole::Card,
                    text: crate::threads::encode_card(
                        &c.title,
                        c.detail.as_deref(),
                        if c.status == CardStatus::Error {
                            "error"
                        } else {
                            "done"
                        },
                    ),
                }),
                MsgRole::System | MsgRole::Command | MsgRole::Thinking | MsgRole::Meta => None,
            })
            .collect()
    }

    fn on_tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
        if self.busy {
            self.pulse_on = self.tick_count.is_multiple_of(2);
        } else {
            self.pulse_on = false;
        }
        if self.busy || !self.editor.chars().is_empty() || self.picker.is_some() {
            self.idle_ticks = 0;
        } else {
            self.idle_ticks = self.idle_ticks.saturating_add(1);
        }
        self.persist_tick();
        self.poll_file_jobs();
        self.poll_session_history();
        self.workspace.poll();
        self.poll_pending_login();
        self.poll_job();
        self.poll_compat();
        self.poll_proposals();
        if self.usage_fetcher.poll() {
            self.refresh_usage();
            self.update_usage_block();
        }
    }

    /// Announce when a login we launched has completed (cheap file-existence checks, ~2 s apart).
    fn poll_pending_login(&mut self) {
        let Some(p) = self.pending_login.as_mut() else {
            return;
        };
        p.ticks += 1;
        if p.ticks % LOGIN_POLL_TICKS != 0 {
            return;
        }
        if is_logged_in(p.provider, &p.home) {
            let p = self.pending_login.take().expect("checked above");
            self.finish_login(p);
        } else if p.ticks >= LOGIN_GIVE_UP_TICKS {
            let p = self.pending_login.take().expect("checked above");
            self.push_system(format!(
                "{} is not signed in yet - run /use add {} to try again",
                p.label,
                p.provider.as_str()
            ));
        }
    }

    /// A login we launched just completed. If it is the same login as an account that is already
    /// connected (you signed in to the same subscription twice), the new empty slot is removed
    /// instead of being kept as a confusing duplicate.
    fn finish_login(&mut self, p: PendingLogin) {
        crate::logging::info(&format!(
            "login-complete account={} provider={}",
            p.account,
            p.provider.as_str()
        ));
        let who = inspect_provider(p.provider, &p.home)
            .login
            .unwrap_or_else(|| "this login".into());
        let Some(existing) = crate::accounts::duplicate_of(&self.state_root, &p.account) else {
            self.push_system(format!(
                "{} connected as {who} - /use to switch to it",
                p.label
            ));
            self.flash(format!("{} connected", p.label));
            return;
        };
        let owned = crate::connect::removable(&self.state_root)
            .iter()
            .any(|r| r.id == p.account && r.owned);
        let advice = format!(
            "to add another account, sign in with a different one (/use add {})",
            p.provider.as_str()
        );
        if owned {
            match crate::connect::remove_account(
                &self.state_root,
                &p.account,
                crate::connect::RemoveMode::DeleteData,
            ) {
                Ok(_) => self.push_system(format!(
                    "{who} is already connected as {}, so nothing new was added and the duplicate slot was removed - {advice}",
                    existing.label
                )),
                Err(e) => self.push_system(format!(
                    "{} is the same login as {} ({who}) but could not be removed: {e} - use /use remove",
                    p.label, existing.label
                )),
            }
        } else {
            self.push_system(format!(
                "{} is the same login as {} ({who}) - use /use remove to drop one of them",
                p.label, existing.label
            ));
        }
        self.flash("duplicate account");
    }

    fn open_resume_picker(&mut self) {
        let sessions = list_recent_sessions(&self.state_root, 40);
        let here = count_sessions_here(&sessions, Some(&self.cwd));
        let folder = self
            .cwd
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "cwd".into());
        let title = if here > 0 {
            format!("chats · {folder} ({here})")
        } else {
            format!("chats · {}", sessions.len())
        };
        let rows = if sessions.is_empty() {
            vec![PickerRow {
                id: "none".into(),
                label: "no chats yet".into(),
                detail: "send a message first — then switch here".into(),
            }]
        } else {
            sessions_to_picker_rows(&sessions, Some(&self.account), Some(&self.cwd))
        };
        self.show_picker(PickerKind::Resume, title, rows, true);
    }

    fn prepare_resume_delete(&mut self) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        let Some(idx) = picker.state.selected() else {
            return;
        };
        let filtered = picker.filtered();
        let Some(row) = filtered.get(idx) else { return };
        let Some(session) = row.id.strip_prefix("resume:") else {
            return;
        };
        let session = session.to_string();
        let Some((account, id)) = session.split_once(':') else {
            return;
        };
        self.show_picker(
            PickerKind::ResumeDeleteConfirm,
            format!("delete chat {id}?"),
            vec![
                PickerRow {
                    id: format!("resume-delete:{id}"),
                    label: "Delete this chat".into(),
                    detail: format!("{account} · this cannot be undone"),
                },
                PickerRow {
                    id: "resume-delete-cancel".into(),
                    label: "Cancel".into(),
                    detail: String::new(),
                },
            ],
            false,
        );
    }

    /// Persist a new permission mode.
    fn set_mode(&mut self, mode: TrustMode) -> bool {
        if let Err(e) = set_trust_mode(&self.state_root, mode) {
            self.push_system(format!("mode save failed - {e}"));
            return false;
        }
        self.trust = mode;
        true
    }

    /// Shift+Tab: plan -> ask -> workspace (auto-edit) -> full -> plan.
    fn cycle_mode(&mut self) {
        let next = next_mode(self.trust);
        if self.set_mode(next) {
            let note = if self.busy { " (next turn)" } else { "" };
            self.flash(format!("mode \u{2192} {}{note}", mode_label(next)));
        }
    }

    fn run_mode_command(&mut self, arg: Option<&str>) {
        match arg {
            None => {
                self.push_system(format!(
                    "mode: {} - {}",
                    mode_label(self.trust),
                    mode_blurb(self.trust)
                ));
                self.push_system("set with /mode plan | ask | auto | full  (Shift+Tab cycles)");
            }
            Some(a) => match parse_mode(a) {
                Some(mode) => {
                    if self.set_mode(mode) {
                        self.push_system(format!(
                            "mode \u{2192} {} - {}{}",
                            mode_label(mode),
                            mode_blurb(mode),
                            if self.busy {
                                " (next turn; the running turn keeps its original permissions)"
                            } else {
                                ""
                            }
                        ));
                    }
                }
                None => self.push_system(format!("unknown mode {a} - use plan, ask, auto or full")),
            },
        }
    }

    fn open_trust_picker(&mut self) {
        let rows = trust_picker_rows(self.trust);
        let idx = rows
            .iter()
            .position(|r| r.id == format!("trust:{}", self.trust.as_str()))
            .unwrap_or(0);
        self.show_picker_at(PickerKind::Trust, "permissions".into(), rows, false, idx);
    }

    fn open_use_picker(&mut self) {
        let ready = accounts_with_auth(&self.state_root);
        let duplicates = crate::accounts::duplicate_groups(&self.state_root);
        let mut rows = Vec::new();
        for a in &ready {
            let Some(home) = a.home_path.as_ref() else {
                continue;
            };
            let idn = inspect_provider(a.provider, home);
            let usage = self.usage_fetcher.snapshot(a.provider, &a.account, home);
            let urow = picker_row(&a.account, a.provider, &usage);
            self.picker_tones
                .insert(format!("use:{}", a.account), urow.tone);
            let mut detail = format_identity_label(&idn);
            if let Some(other) = duplicates
                .iter()
                .find(|g| g.iter().any(|x| x.account == a.account))
                .and_then(|g| g.iter().find(|x| x.account != a.account))
            {
                detail.push_str(&format!(" · same login as {}", other.label));
            }
            rows.push(PickerRow {
                id: format!("use:{}", a.account),
                // The account's own name ("Codex 3"), so several accounts of one provider are
                // told apart at a glance.
                label: urow.label,
                detail,
            });
        }
        rows.push(PickerRow {
            id: "use-add".into(),
            label: "+ add account".into(),
            detail: if ready.is_empty() {
                "no logged-in accounts yet - connect Codex, Claude or Cursor".into()
            } else {
                "connect another Codex, Claude or Cursor account".into()
            },
        });
        if !ready.is_empty() {
            rows.push(PickerRow {
                id: "use-remove".into(),
                label: "- remove account".into(),
                detail: "take an account out of Concordia".into(),
            });
        }
        rows.push(PickerRow {
            id: "use-continuation".into(),
            label: "usage limit handoff".into(),
            detail: if self.continuation.automatic() {
                "continue on another account automatically (Enter toggles to pause)".into()
            } else {
                "pause at a usage limit (Enter toggles to continue)".into()
            },
        });
        self.show_picker(PickerKind::Use, "accounts".into(), rows, true);
    }

    fn open_continuation_picker(&mut self) {
        let rows = accounts_with_auth(&self.state_root)
            .into_iter()
            .filter(|a| a.account != self.account && a.home_path.is_some())
            .map(|a| PickerRow {
                id: format!("continue:{}", a.account),
                label: display_account_name(a.provider, &a.account),
                detail: "continue the paused request on this account".into(),
            })
            .collect::<Vec<_>>();
        if rows.is_empty() {
            self.push_system("usage limit reached; /use add another account to continue");
        } else {
            self.show_picker(
                PickerKind::Continuation,
                "continue on which account?".into(),
                rows,
                true,
            );
        }
    }

    /// Pick an account to remove (or jump straight to its confirmation when one is named).
    fn open_remove_picker(&mut self, named: Option<String>) {
        let all = crate::connect::removable(&self.state_root);
        if all.is_empty() {
            self.push_system("no accounts to remove");
            return;
        }
        if let Some(id) = named {
            let want = crate::accounts::normalize_account_id(&id);
            match all.into_iter().find(|r| r.id == want) {
                Some(r) => self.open_remove_confirm(r),
                None => self.push_system(format!("no account named {id} - try /use remove")),
            }
            return;
        }
        let rows = all
            .iter()
            .map(|r| {
                let what = if r.owned {
                    "created by Concordia - its login folder can be deleted"
                } else if r.stored {
                    "registered with Concordia - its files stay where they are"
                } else {
                    "your own login - it can only be hidden, never deleted"
                };
                let who = r
                    .home
                    .as_ref()
                    .map(|h| format_identity_label(&inspect_provider(r.provider, h)))
                    .unwrap_or_default();
                PickerRow {
                    id: format!("rmpick:{}", r.id),
                    label: format!("{}  {who}", display_account_name(r.provider, &r.id)),
                    detail: what.into(),
                }
            })
            .collect();
        self.show_picker(
            PickerKind::Remove,
            "remove which account?".into(),
            rows,
            true,
        );
    }

    fn open_remove_confirm(&mut self, r: crate::connect::Removable) {
        use crate::connect::RemoveMode;
        let mut rows: Vec<PickerRow> = r
            .modes()
            .into_iter()
            .map(|m| {
                let (key, label, detail) = match m {
                    RemoveMode::DeleteData => (
                        "delete",
                        "Remove and delete its login data",
                        r.home
                            .as_ref()
                            .map(|h| h.display().to_string())
                            .unwrap_or_default(),
                    ),
                    RemoveMode::Unregister => (
                        "unreg",
                        "Remove from Concordia, keep the files",
                        "nothing on disk changes".to_string(),
                    ),
                    RemoveMode::Hide => (
                        "hide",
                        "Hide from Concordia",
                        "your files are not touched; it simply stops being listed".to_string(),
                    ),
                };
                PickerRow {
                    id: format!("rm:{key}:{}", r.id),
                    label: label.into(),
                    detail,
                }
            })
            .collect();
        rows.push(PickerRow {
            id: "rm:cancel".into(),
            label: "Cancel".into(),
            detail: String::new(),
        });
        let title = format!("remove {}?", display_account_name(r.provider, &r.id));
        self.show_picker(PickerKind::RemoveConfirm, title, rows, false);
    }

    fn perform_remove(&mut self, id: &str, mode: crate::connect::RemoveMode) {
        crate::logging::info(&format!("remove-account id={id} mode={mode:?}"));
        match crate::connect::remove_account(&self.state_root, id, mode) {
            Err(e) => self.push_system(format!("remove failed - {e}")),
            Ok(msg) => {
                self.push_system(msg);
                if crate::accounts::normalize_account_id(&self.account)
                    == crate::accounts::normalize_account_id(id)
                {
                    // The account in use is gone: move to another so the next turn has a home.
                    match accounts_with_auth(&self.state_root).into_iter().next() {
                        Some(next) => {
                            if select_account(&self.state_root, &next.account).is_ok() {
                                self.refresh_binding();
                                self.push_system(format!("now using {}", self.account_label()));
                            }
                        }
                        None => self.push_system("no accounts left - /use add to connect one"),
                    }
                }
            }
        }
    }

    fn open_add_provider_picker(&mut self) {
        let rows = [
            (ProviderKind::Codex, "Codex", "codex"),
            (ProviderKind::Claude, "Claude", "claude"),
            (ProviderKind::Cursor, "Cursor", "agent"),
        ]
        .into_iter()
        .map(|(p, title, bin)| PickerRow {
            id: format!("add:{}", p.as_str()),
            label: title.to_string(),
            detail: if which::which(bin).is_ok()
                || (p == ProviderKind::Cursor && which::which("cursor-agent").is_ok())
            {
                "CLI found".into()
            } else {
                format!("{bin} not found on PATH")
            },
        })
        .collect();
        self.show_picker(PickerKind::AddProvider, "add account".into(), rows, false);
    }

    /// Create an concordia-owned home and open the provider login; reports problems in the transcript.
    fn begin_add_account(&mut self, provider: ProviderKind) {
        if self.pending_login.is_some() {
            self.push_system("a login is already in progress - finish it first");
            return;
        }
        match (self.connect)(&self.state_root, provider) {
            Ok(AddResult {
                account,
                provider,
                label,
                home,
                message,
                watch,
            }) => {
                for l in message {
                    self.push_system(l);
                }
                if watch {
                    self.pending_login = Some(PendingLogin {
                        account,
                        provider,
                        label,
                        home,
                        ticks: 0,
                    });
                }
            }
            Err(e) => self.push_system(format!("add account failed - {e}")),
        }
    }

    fn open_model_picker(&mut self) {
        let home = self
            .home_path
            .clone()
            .or_else(|| get_account(&self.state_root, &self.account).and_then(|a| a.home_path));
        let models = home
            .as_ref()
            .map(|h| list_models(self.provider, h))
            .unwrap_or_default();
        let rows: Vec<PickerRow> = if models.is_empty() {
            vec![PickerRow {
                id: "none".into(),
                label: "no cached models".into(),
                detail: "chat once with the provider CLI to populate cache".into(),
            }]
        } else {
            models
                .into_iter()
                .map(|m| {
                    let cur = if m == self.model { " · current" } else { "" };
                    PickerRow {
                        id: format!("model:{m}"),
                        label: format!("{m}{cur}"),
                        detail: self.provider.as_str().into(),
                    }
                })
                .collect()
        };
        self.show_picker(PickerKind::Model, "model".into(), rows, true);
    }

    fn open_usage_picker(&mut self) {
        let ready = accounts_with_auth(&self.state_root);
        let accounts: Vec<_> = ready
            .into_iter()
            .filter_map(|a| a.home_path.map(|h| (a.account, a.provider, h)))
            .collect();
        let rows_data = usage_rows_for_accounts(&mut self.usage_fetcher, &accounts, &self.account);
        let rows: Vec<PickerRow> = if rows_data.is_empty() {
            vec![PickerRow {
                id: "none".into(),
                label: "no accounts".into(),
                detail: String::new(),
            }]
        } else {
            rows_data
                .into_iter()
                .map(|r| {
                    self.picker_tones.insert(format!("usage:{}", r.id), r.tone);
                    PickerRow {
                        id: format!("usage:{}", r.id),
                        label: r.label,
                        detail: r.detail,
                    }
                })
                .collect()
        };
        self.show_picker(PickerKind::Usage, "usage".into(), rows, false);
    }

    fn show_picker(
        &mut self,
        kind: PickerKind,
        title: String,
        rows: Vec<PickerRow>,
        filterable: bool,
    ) {
        self.show_picker_at(kind, title, rows, filterable, 0);
    }

    fn show_picker_at(
        &mut self,
        kind: PickerKind,
        title: String,
        rows: Vec<PickerRow>,
        filterable: bool,
        idx: usize,
    ) {
        let mut state = ListState::default();
        if !rows.is_empty() {
            state.select(Some(idx.min(rows.len() - 1)));
        }
        self.picker = Some(Picker {
            kind,
            title,
            rows,
            filter: String::new(),
            state,
        });
        self.footer_left = if filterable {
            "↑↓ Enter · type to filter · Esc".into()
        } else {
            if kind == PickerKind::Usage {
                "↑↓ Enter · r refresh · Esc".into()
            } else {
                "↑↓ Enter · Esc".into()
            }
        };
    }

    fn dismiss_picker(&mut self) {
        self.picker = None;
        self.footer_left = FOOTER_HINT.into();
    }

    fn apply_picker_selection(&mut self, turn_tx: Option<&mpsc::UnboundedSender<String>>) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        let filtered = picker.filtered();
        let Some(idx) = picker.state.selected() else {
            return;
        };
        let Some(row) = filtered.get(idx) else {
            return;
        };
        let id = row.id.clone();
        let label = row.label.clone();
        let kind = picker.kind;
        self.dismiss_picker();
        if self.busy
            && matches!(
                kind,
                PickerKind::Resume | PickerKind::Use | PickerKind::Model | PickerKind::Usage
            )
        {
            self.push_system("finish or cancel the current turn before switching conversation, provider or model");
            return;
        }
        match kind {
            PickerKind::Agents => {
                if let Some(index) = id
                    .strip_prefix("native-agent:")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    if self.messages.get(index).is_some_and(|m| m.card.is_some()) {
                        self.activity.open(index);
                    }
                }
                if let Some(id) = id
                    .strip_prefix("worker-agent:")
                    .and_then(|id| id.parse::<u32>().ok())
                {
                    self.inspect_worker(id);
                }
            }
            PickerKind::Recovery => {
                if let Some(id) = id.strip_prefix("recover:") {
                    self.choose_recovery(id);
                }
            }
            PickerKind::Resume => {
                if id == "none" {
                    return;
                }
                if let Some(rest) = id.strip_prefix("resume:") {
                    let mut parts = rest.splitn(2, ':');
                    let account = parts.next().unwrap_or("").to_string();
                    let session_id = parts.next().unwrap_or("").to_string();
                    if session_id.is_empty() {
                        self.push_system("resume failed — missing session id");
                        return;
                    }
                    self.resume_session(&account, &session_id);
                }
            }
            PickerKind::Trust => {
                if let Some(mode) = id.strip_prefix("trust:") {
                    let trust = match mode {
                        "plan" => TrustMode::Plan,
                        "ask" => TrustMode::Ask,
                        "full" => TrustMode::Full,
                        _ => TrustMode::Workspace,
                    };
                    if let Err(e) = set_trust_mode(&self.state_root, trust) {
                        self.push_system(format!("trust save failed — {e}"));
                        return;
                    }
                    self.trust = trust;
                    self.push_system(match trust {
                        TrustMode::Plan => "trust → plan (read-only: analyse and propose)",
                        TrustMode::Full => "trust → full (skip all permissions — careful)",
                        TrustMode::Workspace => "trust → workspace (cwd write, no trust dialogs)",
                        TrustMode::Ask => "trust → ask (may hang headless)",
                    });
                    if self.busy {
                        self.push_system("permission change applies to the next turn; the running turn keeps its original permissions");
                    }
                }
            }
            PickerKind::Remove => {
                if let Some(acct) = id.strip_prefix("rmpick:") {
                    self.open_remove_picker(Some(acct.to_string()));
                }
            }
            PickerKind::RemoveConfirm => {
                use crate::connect::RemoveMode;
                let mut parts = id.splitn(3, ':');
                let (_, mode, acct) = (parts.next(), parts.next(), parts.next());
                match (mode, acct) {
                    (Some("delete"), Some(a)) => self.perform_remove(a, RemoveMode::DeleteData),
                    (Some("unreg"), Some(a)) => self.perform_remove(a, RemoveMode::Unregister),
                    (Some("hide"), Some(a)) => self.perform_remove(a, RemoveMode::Hide),
                    _ => {}
                }
            }
            PickerKind::AddProvider => {
                if let Some(p) = id
                    .strip_prefix("add:")
                    .and_then(crate::commands::parse_provider)
                {
                    self.begin_add_account(p);
                }
            }
            PickerKind::Use => {
                if id == "use-add" {
                    self.open_add_provider_picker();
                    return;
                }
                if id == "use-remove" {
                    self.open_remove_picker(None);
                    return;
                }
                if id == "use-continuation" {
                    let automatic = self.continuation.toggle_automatic();
                    self.push_system(if automatic {
                        "usage handoff → automatic"
                    } else {
                        "usage handoff → pause"
                    });
                    return;
                }
                if let Some(account) = id.strip_prefix("use:") {
                    let prev_home = self
                        .home_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned());
                    let prev_prov = self.provider;
                    let prev_session = self.native_session_id.clone();
                    if let Err(e) = select_account(&self.state_root, account) {
                        self.push_system(format!("account switch failed — {e}"));
                        return;
                    }
                    self.refresh_binding();
                    let new_home = self
                        .home_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned());
                    let msgs = self.pack_messages();
                    let decision = plan_continuity(&ContinuityRequest {
                        from_provider: prev_prov,
                        to_provider: self.provider,
                        from_home: prev_home.as_deref(),
                        to_home: new_home.as_deref(),
                        session_id: prev_session.as_deref(),
                        messages: &msgs,
                        thread_id: self.thread_id(),
                        include_pack_if_needed: true,
                    });
                    self.native_session_id = decision.session_id.clone();
                    if !decision.native_resume {
                        self.session_chip = self.session_ui.title.clone();
                    }
                    self.need_context_pack = decision.pack.is_some();
                    self.log_switch(prev_prov);
                    if let Some(pack) = &decision.pack {
                        self.push_system(format!("{} · ~{} tok", pack.labeled, pack.est_tokens));
                    } else if !decision.native_resume
                        && decision.reason != crate::context::NEW_SESSION
                    {
                        self.push_system(decision.reason);
                    }
                    let home = self.home_path.as_ref();
                    let idn = home
                        .map(|h| inspect_provider(self.provider, h))
                        .map(|i| format_identity_label(&i))
                        .unwrap_or_else(|| "unknown".into());
                    self.push_system(format!(
                        "using {} · {idn} · {}",
                        self.account_label(),
                        self.model
                    ));
                    self.prior_provider = self.provider;
                    self.prior_home = new_home;
                }
            }
            PickerKind::Model => {
                if let Some(model) = id.strip_prefix("model:") {
                    if let Err(e) = set_model(&self.state_root, model) {
                        self.push_system(format!("model save failed — {e}"));
                        return;
                    }
                    self.model = model.to_string();
                    self.push_system(format!("model → {model}"));
                }
            }
            PickerKind::Usage => {
                if let Some(account) = id.strip_prefix("usage:") {
                    if account == self.account {
                        return;
                    }
                    let prev_home = self
                        .home_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned());
                    let prev_prov = self.provider;
                    let prev_session = self.native_session_id.clone();
                    if let Err(e) = select_account(&self.state_root, account) {
                        self.push_system(format!("usage switch failed — {e}"));
                        return;
                    }
                    self.refresh_binding();
                    let new_home = self
                        .home_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned());
                    let msgs = self.pack_messages();
                    let decision = plan_continuity(&ContinuityRequest {
                        from_provider: prev_prov,
                        to_provider: self.provider,
                        from_home: prev_home.as_deref(),
                        to_home: new_home.as_deref(),
                        session_id: prev_session.as_deref(),
                        messages: &msgs,
                        thread_id: self.thread_id(),
                        include_pack_if_needed: true,
                    });
                    self.native_session_id = decision.session_id.clone();
                    self.session_chip = self.session_ui.title.clone();
                    self.need_context_pack = decision.pack.is_some();
                    self.log_switch(prev_prov);
                    if let Some(pack) = &decision.pack {
                        self.push_system(format!("{} · ~{} tok", pack.labeled, pack.est_tokens));
                    }
                    self.prior_provider = self.provider;
                    self.prior_home = new_home;
                    self.push_system(format!("using {} · {}", self.account_label(), self.model));
                }
            }
            PickerKind::Continuation => {
                if let Some(account) = id.strip_prefix("continue:") {
                    if let Some(tx) = turn_tx {
                        self.continue_usage(account, tx);
                    } else {
                        self.continuation_target = Some(account.to_string());
                    }
                }
            }
            PickerKind::ResumeDeleteConfirm => {
                if let Some(id) = id.strip_prefix("resume-delete:") {
                    match crate::threads::resolve_prefix(&self.state_root, id) {
                        Ok(full) => match crate::threads::delete(&self.state_root, &full) {
                            Ok(()) => self.push_system(format!("deleted thread {full}")),
                            Err(e) => self.push_system(format!("delete failed: {e:#}")),
                        },
                        Err(e) => self.push_system(e.to_string()),
                    }
                }
            }
        }
        let _ = label;
    }

    fn handle_slash(&mut self, line: &str) -> bool {
        let trimmed = line.trim();
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
        if trimmed == "/config" || trimmed == "/version" || trimmed == "/ver" || trimmed == "/about"
        {
            self.push_command(line);
            self.push_system(format!("{} is now /doctor", trimmed));
            for report_line in crate::doctor::run_fast_report().lines() {
                self.push_system(report_line.to_owned());
            }
            return true;
        }
        if trimmed == "/preview" || trimmed.starts_with("/preview ") {
            self.push_command(line);
            self.push_system("/preview is now /open");
            if let Some(reference) = trimmed.strip_prefix("/preview ") {
                self.file_command(reference, false);
            }
            return true;
        }
        if trimmed == "/usage all" || trimmed == "/usage refresh" {
            self.push_system(format!(
                "{} is now /usage (use r inside the screen to refresh)",
                trimmed
            ));
        }
        if trimmed == "/update check" {
            self.push_system("/update check is now /update");
        }
        // /workers, /queue, /edit, /close, /split, /stats and /recover keep working unlisted
        // until the screens that replace them exist.
        if trimmed == "/exit" || trimmed == "/quit" || trimmed == "/q" {
            self.push_command(line);
            self.push_system("/exit is now Ctrl+C twice");
            return true;
        }
        if matches!(line.trim(), "/open" | "/preview" | "/edit") {
            self.push_system("usage: /open FILE");
            return true;
        }
        if let Some(reference) = line.trim().strip_prefix("/open ") {
            self.file_command(reference, false);
            return true;
        }
        if let Some(reference) = line.trim().strip_prefix("/preview ") {
            self.file_command(reference, false);
            return true;
        }
        if let Some(reference) = line.trim().strip_prefix("/edit ") {
            self.file_command(reference, true);
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
                    self.flash(format!("closed file pane #{id}"))
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
                    | SlashCommand::Model
            )
        {
            self.push_system("finish or cancel the current turn before switching conversation, provider or model");
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
        let ctx = CommandContext {
            state_root: &self.state_root,
            cwd: &self.cwd,
            provider: self.provider,
            account: &self.account,
            trust: self.trust,
            split_vertical,
        };
        let is_new = matches!(cmd, SlashCommand::New);
        let (outcome, lines) = dispatch_slash(cmd, &ctx);
        if is_new {
            self.continuation.invalidate();
            self.native_agents = native_agents::State::default();
            self.session_ui.invalidate();
            self.session_ui.title = None;
            self.messages.clear();
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
            CommandOutcome::ShowUsage => self.show_usage(false),
            CommandOutcome::ShowUsageRefresh => self.show_usage(true),
            CommandOutcome::OpenUsage => self.open_usage_picker(),
            CommandOutcome::CopyLastReply => match self.last_assistant_text() {
                Some(t) => self.write_clipboard(&t),
                None => self.flash("nothing to copy yet"),
            },
            CommandOutcome::UpdateCheck => self.start_job(JobKind::Check),
            CommandOutcome::Update => self.start_job(JobKind::Update),
            CommandOutcome::Ok => {}
        }
        true
    }

    fn start_job(&mut self, kind: JobKind) {
        if self.job.is_some() {
            self.push_system("an update is already running - Esc cancels it");
            return;
        }
        if kind == JobKind::Update && self.busy {
            self.push_system("update deferred - cancel the running turn first (Esc)");
            return;
        }
        crate::logging::info(&format!("job-start {kind:?}"));
        self.job = Some(match kind {
            JobKind::Update => start_update(self.cwd.clone()),
            JobKind::Check => start_check(self.cwd.clone()),
        });
    }

    /// Deliver a finished background job (called on every tick).
    fn poll_job(&mut self) {
        let Some(result) = self.job.as_ref().and_then(Job::poll) else {
            return;
        };
        let cancelled = self.job.as_ref().is_some_and(Job::is_cancelled);
        self.job = None;
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
            JobResult::Check(Ok(msg)) => self.push_system(msg),
            JobResult::Check(Err(e)) => self.push_system(format!("update check failed - {e:#}")),
        }
    }

    fn submit_input(&mut self, turn_tx: &mpsc::UnboundedSender<String>) {
        let original_prompt = self.editor.text();
        let line = original_prompt.trim().to_string();
        if line.is_empty() {
            return;
        }
        let user_prompt = crate::user_commands::expand(&self.user_cmds, &line);
        if self.session_ui.loading() && (!line.starts_with('/') || user_prompt.is_some()) {
            self.flash("recent messages are loading; your draft is kept");
            return;
        }
        if self.busy && (!line.starts_with('/') || user_prompt.is_some()) {
            self.push_system("a turn is already running - wait for it to finish or cancel it");
            return;
        }
        if !self.busy {
            self.cancel_requested = false;
        }
        if self.job.as_ref().is_some_and(|j| j.kind == JobKind::Update) && !line.starts_with('/') {
            self.push_system("an update is in progress - Esc cancels it, or wait for it to finish");
            return;
        }
        self.remember_prompt(&line);
        self.editor.clear();
        self.clear_selections();
        self.persist_draft_now();
        let was_busy = self.busy;
        self.run_line(line, turn_tx);
        if !was_busy && self.busy && self.submitted_prompt.is_some() {
            self.submitted_prompt = Some(original_prompt);
        }
    }

    /// Run a prompt that has left the box: a slash command, or a turn. Typed prompts and queued
    /// ones both come through here, so trust mode, account and provider apply at send time.
    fn run_line(&mut self, line: String, turn_tx: &mpsc::UnboundedSender<String>) {
        if let Some(arg) = line
            .strip_prefix("/continue")
            .filter(|arg| arg.is_empty() || arg.starts_with(' '))
        {
            self.push_system("/continue is now the account choice in /use");
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
            .strip_prefix("/continuation")
            .filter(|arg| arg.is_empty() || arg.starts_with(' '))
        {
            self.push_system("/continuation is now the usage handoff row in /use");
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
        self.submitted_prompt = Some(line.clone());
        self.continuation.begin(&self.account, &line);
        self.last_sent = Some(line.clone());
        self.ensure_session_title(&line);
        let refs = self.expand_chat_refs(&send);
        self.push_user(line.clone());
        send = self.show_chat_refs(refs);
        self.turn_started = Some(Instant::now());
        self.phase = Phase::Waiting;
        self.live_thinking.clear();
        self.busy = true;
        self.pulse_on = true;
        let _ = turn_tx.send(send);
    }

    fn request_cancel(&mut self) {
        if self.cancel_requested {
            return;
        }
        self.cancel_requested |= self.submitted_prompt.is_some();
        self.submitted_prompt = None;
        self.pause_queue();
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
        self.busy = false;
        self.pulse_on = false;
        self.cancel_handle = None;
        self.cancel_requested = false;
        true
    }
}

impl App {
    fn new(state_root: PathBuf, cwd: PathBuf) -> App {
        let state_root_for_usage = state_root.clone();
        let state_root_for_cmds = state_root.clone();
        App {
            continuation: continuation::State::new(&state_root),
            continuation_target: None,
            state_root,
            cwd,
            messages: vec![ChatLine {
                role: MsgRole::System,
                text: "Type a message and press Enter. Start with / for commands. Drag to select; Ctrl+C copies; Esc interrupts."
                    .into(),
                card: None,
            meta: None,
            }],
            editor: Editor::default(),
            submitted_prompt: None,
            expanded_activity: false,
            activity: activity::Activity::default(),
            palette: Palette::default(),
            chatref: chatref::ChatRef::default(),
            history: Vec::new(),
            hist_pos: None,
            hist_draft: String::new(),
            quit_armed: None,
            input_scroll: 0,
            view: TranscriptView::default(),
            areas: Areas::default(),
            drag: Drag::None,
            file_jobs: Vec::new(),
            workspace: workspace::Workspace::default(),
            file_click: None,
            #[cfg(test)]
            last_file_open: None,
            clicks: ClickTracker::default(),
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
            session_chip: None,
            session_ui: session_ui::State::default(),
            native_agents: native_agents::State::default(),
            footer_left: FOOTER_HINT.into(),
            picker: None,
            cancel_handle: None,
            prior_provider: ProviderKind::Codex,
            prior_home: None,
            need_context_pack: false,
            thread: crate::threads::start(&state_root_for_usage),
            thread_meta_done: false,
            usage_summary: "limits unknown".into(),
            usage_tone: UsageTone::Mute,
            usage_warning: None,
            usage_fetcher: UsageFetcher::new(state_root_for_usage),
            usage_block: None,
            stats: crate::config::ui_stats().then(crate::sysstats::StatsHandle::start),
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
            pending_login: None,
            connect: Box::new(add_account),
            reexec_path: None,
            deleg: workers::Panel::default(),
            user_cmds: crate::user_commands::scan(&state_root_for_cmds).commands,
            queue: queue::Queue::default(),
            last_sent: None,
            keep: persist::Keep::new(&state_root_for_cmds),
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

fn set_model(state_root: &Path, model: &str) -> anyhow::Result<()> {
    let mut sel = get_use_selection(state_root).ok_or_else(|| anyhow::anyhow!("no selection"))?;
    sel.model = Some(model.to_string());
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

pub fn run_file_preview(path: &Path) -> Result<()> {
    preview::run(path)
}

pub async fn run_chat_tui() -> Result<()> {
    let state_root = default_store_root();
    crate::logging::init(&state_root);
    crate::logging::info(&format!(
        "start {} exe={}",
        crate::version::version_line(),
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    ));
    ensure_default_selection(&state_root)?;
    crate::config::loaded(); // read config.toml once, at start
    let cwd = std::env::current_dir()?;

    let (turn_tx, mut turn_rx) = mpsc::unbounded_channel::<String>();
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<TurnOutcome>();
    let (handle_tx, mut handle_rx) = mpsc::unbounded_channel::<Arc<RunningTurn>>();
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<TurnProgress>();

    let mut app = App::new(state_root.clone(), cwd.clone());
    app.compat_rx = Some(crate::provider::compat::start_real_detection(
        state_root.clone(),
    ));
    app.refresh_binding();
    app.restore_state();
    if !app.recovery_choices().is_empty() {
        app.open_recovery_picker();
    }
    app.prior_provider = app.provider;
    app.prior_home = app
        .home_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());

    enable_raw_mode()?;
    let terminal_restore = TerminalRestoreGuard::new();
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // Mouse capture replaces the terminal's own selection (concordia draws its own); bracketed paste
    // delivers pasted newlines as text instead of Enter presses. Both are best-effort.
    let _ = execute!(stdout, EnableMouseCapture, EnableBracketedPaste);
    // A panic must not leave the terminal in raw/mouse-reporting mode.
    let default_hook = std::panic::take_hook();
    let panic_restore = terminal_restore.0.clone();
    std::panic::set_hook(Box::new(move |info| {
        if panic_restore.load(std::sync::atomic::Ordering::Acquire) {
            // Show the panic on the primary screen. The guard restores input modes after
            // the VT reader has stopped and restored its own saved console settings.
            let _ = execute!(
                std::io::stdout(),
                LeaveAlternateScreen,
                crossterm::cursor::Show
            );
        }
        persist::flush_on_panic();
        crate::logging::error(&format!(
            "panic: {info}\n{}",
            std::backtrace::Backtrace::force_capture()
        ));
        eprintln!(
            "Concordia crashed; details are in {}",
            crate::logging::log_path(&default_store_root()).display()
        );
        default_hook(info);
    }));
    // So KeyEvent.kind distinguishes Press vs Release on Windows (otherwise both look like Press).
    let _ = execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
    );
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut terminal_input = input_stream::InputStream::spawn()?;

    let tick_rate = Duration::from_millis(200);
    let mut last_tick = Instant::now();
    let mut key_burst = input::BurstBuffer::default();

    let mut previous_title = String::new();
    let input_result = (|| -> Result<()> {
        loop {
            // Drain pending paste chunks before repainting the growing prompt.
            if !key_burst.active() || !terminal_input.poll(Duration::ZERO)? {
                terminal.draw(|f| draw_ui(f, &mut app))?;
            }
            let title = app.window_title();
            if title != previous_title {
                write_osc_title(&title)?;
                previous_title = title;
            }

            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if terminal_input.poll(timeout)? {
                // Drain a burst (mouse drags, pastes) before redrawing; bounded so a flood can't
                // starve the turn/progress channels below.
                let mut quit = false;
                for _ in 0..256 {
                    let (event, captured_at, quiet_before) = terminal_input.read()?;
                    if quiet_before {
                        if let Some(burst) = key_burst.take_quiet() {
                            if dispatch_key_burst(&mut app, burst, &turn_tx) {
                                quit = true;
                                break;
                            }
                        }
                    }
                    let Some(event) = event else {
                        if !terminal_input.poll(Duration::ZERO)? {
                            break;
                        }
                        continue;
                    };
                    match event {
                        Event::Key(key) => {
                            if dispatch_key_event(
                                &mut app,
                                key,
                                &mut key_burst,
                                &turn_tx,
                                captured_at,
                            ) {
                                quit = true;
                                break;
                            }
                        }
                        Event::Mouse(m) => {
                            if let Some(burst) = key_burst.take_pending() {
                                dispatch_key_burst(&mut app, burst, &turn_tx);
                            }
                            handle_mouse(&mut app, m)
                        }
                        Event::Paste(s) => {
                            if let Some(burst) = key_burst.take_pending() {
                                dispatch_key_burst(&mut app, burst, &turn_tx);
                            }
                            app.paste_text(&s)
                        }
                        _ => {}
                    }
                    if !terminal_input.poll(Duration::ZERO)? {
                        break;
                    }
                }
                if quit {
                    break;
                }
            }

            while let Ok(h) = handle_rx.try_recv() {
                app.accept_cancel_handle(h);
            }

            // A finished turn can wait here for an inline queue edit to be committed.
            app.queue_advance(&turn_tx);
            while let Ok(prompt) = turn_rx.try_recv() {
                if app.discard_cancelled_pending_turn() {
                    continue;
                }
                let state_root = app.state_root.clone();
                let cwd = app.cwd.clone();
                let provider = app.provider;
                let account = app.account.clone();
                let model = app.model.clone();
                let trust = app.trust;
                let session = app.native_session_id.clone();
                let prior_prov = app.prior_provider;
                let prior_home = app.prior_home.clone();
                let pack_msgs = app.pack_messages();
                // Drop the just-typed user message from pack (already in prompt).
                let pack_msgs: Vec<_> = if pack_msgs
                    .last()
                    .map(|m| matches!(m.role, PackRole::User) && m.text == prompt)
                    .unwrap_or(false)
                {
                    pack_msgs[..pack_msgs.len().saturating_sub(1)].to_vec()
                } else {
                    pack_msgs
                };
                let need_pack_hint = app.need_context_pack;
                let thread_id = app.thread_id().map(str::to_string);
                let done_tx = done_tx.clone();
                let handle_tx = handle_tx.clone();
                let progress_tx = progress_tx.clone();
                app.need_context_pack = false;
                app.live_cards.clear();
                app.live_text.clear();
                app.live_thinking.clear();
                app.phase = Phase::Waiting;
                tokio::spawn(async move {
                    let home = get_account(&state_root, &account)
                        .and_then(|a| a.home_path)
                        .or_else(|| {
                            get_use_selection(&state_root)
                                .and_then(|s| s.home_path)
                                .map(PathBuf::from)
                        });
                    let home_str = home.as_ref().map(|p| p.to_string_lossy().into_owned());
                    // Pack only after an explicit switch (need_pack_hint). Planner clears
                    // session id on cross-home/provider; omits pack when native resume applies.
                    let decision = plan_continuity(&ContinuityRequest {
                        from_provider: prior_prov,
                        to_provider: provider,
                        from_home: prior_home.as_deref().or(home_str.as_deref()),
                        to_home: home_str.as_deref(),
                        session_id: session.as_deref(),
                        messages: &pack_msgs,
                        thread_id: thread_id.as_deref(),
                        include_pack_if_needed: need_pack_hint,
                    });
                    let session_out = if decision.native_resume {
                        decision.session_id.clone().or(session.clone())
                    } else {
                        // Never leak a foreign / cross-home session id.
                        None
                    };
                    let prefix = if need_pack_hint {
                        decision.pack.as_ref().map(|p| p.text.clone())
                    } else {
                        None
                    };
                    let req = ProviderTurnRequest {
                        provider,
                        prompt: &prompt,
                        cwd: &cwd,
                        home_path: home.as_deref(),
                        model: if model == "?" {
                            None
                        } else {
                            Some(model.as_str())
                        },
                        native_session_id: session_out.as_deref(),
                        trust,
                        context_prefix: prefix.as_deref(),
                    };
                    // Validate plan even if spawn fails (argv safety).
                    if let Err(e) = plan_provider_turn(&req) {
                        let _ = done_tx.send(TurnOutcome {
                            stderr_tail: format!("{e:#}"),
                            ..Default::default()
                        });
                        return;
                    }
                    match run_provider_turn_with_progress(&req, Some(handle_tx), Some(progress_tx))
                        .await
                    {
                        Ok(outcome) => {
                            let _ = done_tx.send(outcome);
                        }
                        Err(e) => {
                            let _ = done_tx.send(TurnOutcome {
                                stderr_tail: format!("{e:#}"),
                                ..Default::default()
                            });
                        }
                    }
                });
            }

            app.poll_delegates();
            while let Ok(ev) = progress_rx.try_recv() {
                app.apply_progress(ev);
            }

            while let Ok(outcome) = done_rx.try_recv() {
                // Progress is sent before completion, but may arrive between the earlier
                // drain and this receive. Finish it before finalizing or switching sessions.
                while let Ok(ev) = progress_rx.try_recv() {
                    app.apply_progress(ev);
                }
                app.cancel_requested = false;
                app.on_turn_done(outcome, &turn_tx);
            }

            if last_tick.elapsed() >= tick_rate {
                app.on_tick();
                last_tick = Instant::now();
            }

            if app.should_quit || app.reexec_path.is_some() {
                break;
            }
        }
        Ok(())
    })();

    app.persist_draft_now();
    handle_rx.close();
    while let Ok(handle) = handle_rx.try_recv() {
        handle.cancel();
    }
    // Leaving mid-turn must not orphan the provider (it would keep running and spending).
    if let Some(h) = app.cancel_handle.take() {
        h.cancel();
    }
    app.cancel_all_delegates();
    let reexec = app.reexec_path.take();
    let cwd = app.cwd.clone();
    let _ = std::panic::take_hook(); // drop our hook; default behavior resumes
    drop(terminal_input);
    terminal_restore.restore();
    terminal.show_cursor()?;
    input_result?;
    if let Some(path) = reexec {
        let code = reexec_into(&path, &cwd)?;
        std::process::exit(code);
    }
    Ok(())
}

/// Releases are identified by the backend. Never infer duplicates from processing time:
/// queued, distinct presses can be drained together after a large paste or slow frame.
fn should_accept_key(key: &KeyEvent) -> bool {
    if matches!(key.kind, KeyEventKind::Release) {
        return false;
    }
    // Enter also arrives as Char('\r') / '\n' on some hosts — ignore those chars.
    if matches!(key.code, KeyCode::Char('\r') | KeyCode::Char('\n')) {
        return false;
    }
    true
}

fn dispatch_key_burst(
    app: &mut App,
    burst: input::KeyBurst,
    turn_tx: &mpsc::UnboundedSender<String>,
) -> bool {
    if burst.text {
        app.paste_text_chunk(
            burst.text_content.as_deref().unwrap_or_default(),
            burst.continues_paste,
        );
        return false;
    }
    burst
        .keys
        .into_iter()
        .any(|key| handle_key(app, key, turn_tx))
}

fn dispatch_key_event(
    app: &mut App,
    key: KeyEvent,
    key_burst: &mut input::BurstBuffer,
    turn_tx: &mpsc::UnboundedSender<String>,
    now: Instant,
) -> bool {
    if matches!(key.kind, KeyEventKind::Release) {
        return false;
    }
    let palette_action = matches!(key.code, KeyCode::Enter | KeyCode::Tab)
        && !app.palette_entries().is_empty()
        && !key_burst.active();
    if input::BurstBuffer::is_candidate(&key) && app.picker.is_none() && !palette_action {
        if let Some(burst) = key_burst.push(key, now) {
            return dispatch_key_burst(app, burst, turn_tx);
        }
        return false;
    }
    if !should_accept_key(&key) {
        return false;
    }
    if let Some(burst) = key_burst.take_pending() {
        if dispatch_key_burst(app, burst, turn_tx) {
            return true;
        }
    }
    key_burst.end_continuation();
    handle_key(app, key, turn_tx)
}

fn handle_key(app: &mut App, key: KeyEvent, turn_tx: &mpsc::UnboundedSender<String>) -> bool {
    // Prefer Press; allow Repeat for held Backspace only.
    match key.kind {
        KeyEventKind::Release => return false,
        KeyEventKind::Repeat if !matches!(key.code, KeyCode::Backspace) => return false,
        _ => {}
    }
    if let Some(picker) = app.picker.as_mut() {
        match key.code {
            KeyCode::Esc => app.dismiss_picker(),
            KeyCode::Enter => app.apply_picker_selection(Some(turn_tx)),
            KeyCode::Delete if picker.kind == PickerKind::Resume => app.prepare_resume_delete(),
            KeyCode::Char('r') if picker.kind == PickerKind::Usage => {
                for account in crate::accounts::accounts_with_auth(&app.state_root) {
                    app.usage_fetcher.invalidate(&account.account);
                }
                app.open_usage_picker();
            }
            KeyCode::Up => {
                let n = picker.filtered().len();
                if n > 0 {
                    let i = picker.state.selected().unwrap_or(0);
                    picker
                        .state
                        .select(Some(if i == 0 { n - 1 } else { i - 1 }));
                }
            }
            KeyCode::Down => {
                let n = picker.filtered().len();
                if n > 0 {
                    let i = picker.state.selected().unwrap_or(0);
                    picker.state.select(Some((i + 1) % n));
                }
            }
            KeyCode::Backspace => {
                picker.filter.pop();
                picker.select_clamp();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                picker.filter.push(c);
                picker.select_clamp();
            }
            // Like Esc: close the picker. It must not quit the app (and lose the draft), which
            // would contradict the press-twice rule for exiting.
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.dismiss_picker();
            }
            _ => {}
        }
        return false;
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if ctrl && !alt && matches!(key.code, KeyCode::Char('g' | 'G')) {
        app.open_agents_picker();
        return false;
    }
    let tw = app.areas.input_text_w.max(1);

    if app.activity.detail.is_none() && ctrl && !alt && matches!(key.code, KeyCode::Char('o' | 'O'))
    {
        app.expanded_activity = !app.expanded_activity;
        app.flash(if app.expanded_activity {
            "full activity - Ctrl+O collapses"
        } else {
            "compact activity - Ctrl+O expands"
        });
        return false;
    }
    if app.activity.detail.is_none() && ctrl && !alt && matches!(key.code, KeyCode::Char('e' | 'E'))
    {
        if let Some(i) = app.messages.iter().rposition(|m| m.role == MsgRole::Card) {
            app.activity.open(i);
        } else {
            app.flash("no activity yet");
        }
        return false;
    }
    if app.activity.detail.is_some() {
        match app.activity.handle_key(key) {
            activity::ModalAction::Close => {
                app.activity.detail = None;
                app.activity.query.clear();
            }
            activity::ModalAction::Copy => {
                if let Some(i) = app.activity.detail {
                    if let Some(text) = app
                        .messages
                        .get(i)
                        .and_then(|m| m.card.as_ref())
                        .map(|c| activity::full_output(&c.title, c.detail.as_deref()))
                    {
                        app.write_clipboard(&text);
                    }
                }
            }
            activity::ModalAction::PreviousCard => app.navigate_activity_card(true),
            activity::ModalAction::NextCard => app.navigate_activity_card(false),
            activity::ModalAction::Paste => app.paste_clipboard(),
            // Modal owns every other key; editing, undo, queue actions and cancellation do not
            // fall through to the prompt or turn handlers.
            activity::ModalAction::Consumed => {}
        }
        return false;
    }
    if queue_controls::handle_key(app, &key)
        || queue_focus::handle_key(app, &key)
        || queue::handle_key(app, &key)
    {
        app.workspace.focused = None;
        return false;
    }
    if workers::handle_key(app, &key) {
        app.workspace.focused = None;
        return false;
    }
    if workspace::handle_key(app, &key) {
        return false;
    }

    // `@chat` completion list (own mode; the slash palette below is unchanged).
    if chatref::handle_key(app, &key) {
        return false;
    }
    // Slash-command suggestions take the navigation keys while they are showing.
    let entries = app.palette_entries();
    if !entries.is_empty() && !ctrl && !alt {
        app.palette.clamp(entries.len());
        let chosen = entries[app.palette.selected];
        match key.code {
            KeyCode::Up | KeyCode::BackTab => {
                app.palette.up(entries.len());
                return false;
            }
            KeyCode::Down => {
                app.palette.down(entries.len());
                return false;
            }
            KeyCode::Tab => {
                app.complete_palette(chosen);
                return false;
            }
            // Enter completes a partial command; once it is exactly the chosen one, it runs.
            KeyCode::Enter if app.editor.text() != chosen.command => {
                app.complete_palette(chosen);
                return false;
            }
            KeyCode::Esc => {
                app.palette.dismissed = true;
                return false;
            }
            _ => {}
        }
    }

    // Anything that edits the prompt drops a transcript selection (like a normal document).
    let edits = matches!(
        key.code,
        KeyCode::Backspace | KeyCode::Delete | KeyCode::Enter | KeyCode::Tab
    ) || matches!(key.code, KeyCode::Char(_)) && (!ctrl || alt);
    if edits {
        app.view.clear_selection();
        app.hist_pos = None;
        // Tab/Enter are handled by the palette above; any other edit reopens and re-ranks it.
        app.palette.reset();
    }

    match key.code {
        // Copy when something is selected (Windows Terminal convention); otherwise Ctrl+C keeps
        // its meaning: cancel the running turn, or quit when idle. Ctrl+Shift+C always copies
        // (falling back to the last reply) on hosts that deliver it to the app.
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'c') => {
            if let Some(i) = app.activity.detail {
                if let Some(text) = app
                    .messages
                    .get(i)
                    .and_then(|m| m.card.as_ref())
                    .map(|c| activity::full_output(&c.title, c.detail.as_deref()))
                {
                    app.write_clipboard(&text);
                }
                return false;
            }
            if shift {
                app.copy(true);
                false
            } else if app.has_selection() {
                app.copy(false);
                false
            } else if app.busy {
                app.request_cancel();
                false
            } else if app.quit_armed.is_some_and(|t| t.elapsed() <= QUIT_WINDOW) {
                true
            } else {
                app.quit_armed = Some(Instant::now());
                app.flash("press Ctrl+C again to exit");
                false
            }
        }
        // Ctrl+Z undo; Ctrl+Shift+Z and Ctrl+Y redo (hosts report Shift as the modifier or as an
        // uppercase letter).
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'z') => {
            if shift || c == 'Z' {
                app.editor.redo();
            } else {
                app.editor.undo();
            }
            app.after_undo();
            false
        }
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'y') => {
            app.editor.redo();
            app.after_undo();
            false
        }
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'v') => {
            app.paste_clipboard();
            false
        }
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'x') => {
            app.cut();
            false
        }
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'a') => {
            app.view.clear_selection();
            app.editor.select_all();
            false
        }
        // Ctrl+J / Alt+Enter / Shift+Enter: newline without sending.
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'j') => {
            app.editor.insert_char('\n');
            false
        }
        KeyCode::Enter if ctrl || alt || shift => {
            app.editor.insert_char('\n');
            false
        }
        KeyCode::Enter => {
            app.submit_input(turn_tx);
            false
        }
        KeyCode::Tab => {
            if !queue::tab(app) {
                app.tab_complete_slash();
            }
            false
        }
        // Shift+Tab (palette closed): cycle the permission mode.
        KeyCode::BackTab => {
            app.cycle_mode();
            false
        }
        KeyCode::Backspace if ctrl => {
            app.editor.delete_word_left();
            false
        }
        KeyCode::Backspace => {
            app.editor.backspace();
            false
        }
        KeyCode::Delete if ctrl => {
            app.editor.delete_word_right();
            false
        }
        KeyCode::Delete => {
            app.editor.delete();
            false
        }
        KeyCode::Left if ctrl => {
            app.editor.word_left(shift);
            false
        }
        KeyCode::Right if ctrl => {
            app.editor.word_right(shift);
            false
        }
        KeyCode::Left => {
            app.editor.move_left(shift);
            false
        }
        KeyCode::Right => {
            app.editor.move_right(shift);
            false
        }
        KeyCode::Home if ctrl => {
            app.editor.move_to(0, shift);
            false
        }
        KeyCode::End if ctrl => {
            app.editor.move_to(usize::MAX, shift);
            false
        }
        KeyCode::Home => {
            app.editor.home(tw, shift);
            false
        }
        KeyCode::End => {
            app.editor.end(tw, shift);
            false
        }
        // Up on the first prompt row (and Down on the last, while browsing) walks sent prompts.
        KeyCode::Up => {
            if !shift && app.editor.selection().is_none() && app.editor.cursor_pos(tw).0 == 0 {
                if !queue::focus_on_up(app) {
                    app.history_prev();
                }
            } else {
                app.editor.up(tw, shift);
            }
            false
        }
        KeyCode::Down => {
            let last_row = app.editor.layout(tw).len() - 1;
            if !shift && app.hist_pos.is_some() && app.editor.cursor_pos(tw).0 == last_row {
                app.history_next();
            } else {
                app.editor.down(tw, shift);
            }
            false
        }
        KeyCode::PageUp => {
            let n = app.view.page();
            app.view.scroll_up(n);
            false
        }
        KeyCode::PageDown => {
            let n = app.view.page();
            app.view.scroll_down(n);
            false
        }
        // AltGr arrives as Ctrl+Alt on Windows; those chars are real input.
        KeyCode::Char(c) if !ctrl || alt => {
            app.editor.insert_char(c);
            false
        }
        KeyCode::Esc => {
            // Esc never quits (an accidental press must not lose a draft): it clears a
            // selection, or interrupts a running turn. Exit is /exit or Ctrl+C twice.
            if let Some(job) = &app.job {
                job.cancel();
                app.flash("cancelling...");
            } else if app.has_selection() {
                app.clear_selections();
            } else if app.busy {
                app.request_cancel();
            }
            false
        }
        _ => false,
    }
}

/// Prompt text index under screen cell `(x, y)`; coordinates outside the box clamp to its edge
/// so a drag that leaves the prompt keeps extending the selection.
fn input_index_at(app: &App, x: u16, y: u16) -> usize {
    let r = app.areas.input;
    let rel_y = (y.saturating_sub(r.y) as usize).min(r.height.saturating_sub(1) as usize);
    let rel_x = (x.saturating_sub(r.x) as usize).saturating_sub(PROMPT_W);
    app.editor.index_at(
        app.areas.input_text_w.max(1),
        rel_y + app.input_scroll,
        rel_x,
    )
}

fn handle_mouse(app: &mut App, m: MouseEvent) {
    if app.activity.detail.is_some() {
        match m.kind {
            MouseEventKind::ScrollUp => {
                app.activity.detail_scroll = app.activity.detail_scroll.saturating_sub(3);
                app.activity.follow_match = false;
            }
            MouseEventKind::ScrollDown => {
                app.activity.detail_scroll = app.activity.detail_scroll.saturating_add(3);
                app.activity.follow_match = false;
            }
            _ => {}
        }
        return;
    }
    if app.picker.is_some() {
        return;
    }
    if workers::handle_mouse(app, &m) || queue::handle_mouse(app, &m) {
        app.workspace.focused = None;
        return;
    }
    if workspace::handle_mouse(app, &m) {
        return;
    }
    let (x, y) = (m.column, m.row);
    let t = app.areas.transcript;
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            app.file_click = None;
            let clicks = app.clicks.register(x, y, Instant::now());
            let pal = app.areas.palette;
            if pal.height > 0 && contains(pal, x, y) {
                let entries = app.palette_entries();
                let visible = pal.height.saturating_sub(2) as usize;
                let first = app
                    .palette
                    .selected
                    .saturating_sub(visible.saturating_sub(1));
                let row = (y.saturating_sub(pal.y + 1)) as usize;
                if let Some(e) = entries.get(first + row) {
                    let e = *e;
                    app.complete_palette(e);
                }
                return;
            }
            if contains(t, x, y) {
                if x.saturating_sub(t.x) < 2 {
                    if let Some(i) = app.view.identity_at((y - t.y) as usize) {
                        if app.messages.get(i).is_some_and(|m| m.role == MsgRole::Card) {
                            app.activity.toggle(i);
                            app.drag = Drag::None;
                            return;
                        }
                    }
                }
                app.editor.clear_selection();
                app.drag = Drag::Transcript;
                let pos = app.view.pos_at((y - t.y) as isize, (x - t.x) as usize);
                match (pos, clicks) {
                    (None, _) => {
                        app.view.clear_selection();
                        app.drag = Drag::None;
                    }
                    (Some(p), 1) => {
                        app.file_click = Some(p);
                        app.view.start_selection(p);
                    }
                    (Some(p), 2) => {
                        app.view.select_word(p);
                        app.drag = Drag::None;
                    }
                    (Some(p), _) => {
                        app.view.select_line(p);
                        app.drag = Drag::None;
                    }
                }
            } else if contains(app.areas.input, x, y) {
                app.view.clear_selection();
                app.drag = Drag::Input;
                let idx = input_index_at(app, x, y);
                match clicks {
                    1 => app.editor.move_to(idx, false),
                    2 => app.editor.select_word_at(idx),
                    _ => app.editor.select_line_at(idx),
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            app.file_click = None;
            match app.drag {
                Drag::Transcript => {
                    // Dragging past the top/bottom edge scrolls while extending the selection.
                    if y < t.y {
                        app.view.scroll_up(1);
                    } else if y >= t.y + t.height {
                        app.view.scroll_down(1);
                    }
                    let rel_row = y as isize - t.y as isize;
                    let rel_col = x.saturating_sub(t.x) as usize;
                    let p = app
                        .view
                        .pos_at(rel_row, rel_col)
                        .or_else(|| app.view.last_pos());
                    if let Some(p) = p {
                        app.view.extend_selection(p);
                    }
                }
                Drag::Input => {
                    let idx = input_index_at(app, x, y);
                    app.editor.move_to(idx, true);
                }
                Drag::None => {}
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let clicked = app.file_click.take();
            if app.drag == Drag::Transcript {
                if let Some((anchor, head)) = app.view.sel.filter(|(a, b)| a == b) {
                    let release = app
                        .view
                        .pos_at(y as isize - t.y as isize, x.saturating_sub(t.x) as usize);
                    if clicked == Some(head) && contains(t, x, y) && release == Some(head) {
                        if let Some(row) = app.view.rows.get(anchor.row) {
                            let mut candidates: Vec<_> = row
                                .links
                                .iter()
                                .filter(|(a, b, _)| *a <= head.col && head.col < *b)
                                .map(|(_, _, c)| c.clone())
                                .collect();
                            candidates.sort_by_key(|c| std::cmp::Reverse(c.range.len()));
                            let fallback = candidates.first().cloned();
                            if let Some(candidate) = candidates
                                .into_iter()
                                .find(|c| crate::file_links::resolve(c, &app.cwd).is_ok())
                                .or(fallback)
                            {
                                app.open_file_candidate(
                                    candidate,
                                    m.modifiers.contains(KeyModifiers::ALT),
                                );
                            }
                        }
                    }
                }
                app.view.finish_selection();
            }
            app.drag = Drag::None;
        }
        // Right click: copy the selection, or paste when there is none (Windows Terminal style).
        MouseEventKind::Down(MouseButton::Right) => {
            if app.has_selection() {
                app.copy(false);
            } else {
                app.paste_clipboard();
            }
        }
        MouseEventKind::ScrollUp => app.view.scroll_up(3),
        MouseEventKind::ScrollDown => app.view.scroll_down(3),
        _ => {}
    }
}

/// Best-effort terminal reset for the panic hook (normal exit restores through the backend).
struct TerminalRestoreGuard(Arc<std::sync::atomic::AtomicBool>);

impl TerminalRestoreGuard {
    fn new() -> Self {
        Self(Arc::new(std::sync::atomic::AtomicBool::new(true)))
    }

    fn restore(&self) {
        if self.0.swap(false, std::sync::atomic::Ordering::AcqRel) {
            restore_terminal();
        }
    }
}

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

fn restore_terminal() {
    let mut out = stdout();
    let _ = execute!(
        out,
        PopKeyboardEnhancementFlags,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    );
    let _ = disable_raw_mode();
}

fn write_osc_title(title: &str) -> io::Result<()> {
    let clean: String = title.chars().filter(|c| !c.is_control()).collect();
    execute!(stdout(), crossterm::terminal::SetTitle(clean))
}

fn draw_ui(f: &mut Frame, app: &mut App) {
    let chat_area = workspace::draw(f, app);
    // The prompt box grows with its content (up to MAX_INPUT_ROWS), so size it first.
    let text_w = (chat_area.width as usize)
        .saturating_sub(2 + PROMPT_W)
        .max(1);
    let input_rows = app.editor.layout(text_w).len().clamp(1, MAX_INPUT_ROWS);
    let mut chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(input_rows as u16 + 2),
            Constraint::Length(1),
        ])
        .split(chat_area)
        .to_vec();
    // The queue panel sits right above the prompt, the worker rows above it (each is nothing
    // when empty).
    let queue_area = queue::carve(&mut chunks[1], app);
    let strip = workers::carve(&mut chunks[1], app);

    if let Some(warning) = &app.usage_warning {
        let text = warning.replace(" | ", "\n");
        let height = text
            .lines()
            .count()
            .min(2)
            .min(chunks[1].height.saturating_sub(3) as usize) as u16;
        if height > 0 {
            let area = Rect {
                height,
                ..chunks[1]
            };
            chunks[1].y += height;
            chunks[1].height -= height;
            f.render_widget(
                Paragraph::new(text).alignment(Alignment::Right).style(
                    Style::default()
                        .fg(if app.usage_tone == UsageTone::Bad {
                            Color::Red
                        } else {
                            Color::Yellow
                        })
                        .add_modifier(Modifier::BOLD),
                ),
                area,
            );
        }
    }

    draw_header(f, chunks[0], app);
    draw_transcript(f, chunks[1], app);
    draw_input(f, chunks[2], app);
    queue::draw(f, queue_area, app);
    if app.picker.is_some() {
        draw_picker(f, chunks[1], app);
    }
    draw_palette(f, chunks[1], chunks[2], app);
    chatref::draw(f, chunks[1], chunks[2], app);
    draw_footer(f, chunks[3], app);
    workers::draw(f, strip, chunks[1], app);
    if app.no_color {
        // Keep bold/reverse/italic (they carry meaning); drop every colour.
        for cell in f.buffer_mut().content.iter_mut() {
            cell.fg = Color::Reset;
            cell.bg = Color::Reset;
        }
    }
}

/// Slash-command suggestions, floating just above the prompt box.
fn draw_palette(f: &mut Frame, above: Rect, input: Rect, app: &mut App) {
    app.areas.palette = Rect::default();
    let entries = app.palette_entries();
    if entries.is_empty() {
        return;
    }
    app.palette.clamp(entries.len());
    let shown = entries.len().min(10);
    let height = shown as u16 + 2;
    // Command column (padded) + the longest description + highlight marker (2) + borders (2).
    let cmd_col = entries
        .iter()
        .map(|e| e.command.chars().count())
        .max()
        .unwrap_or(8)
        + 2;
    let help_col = entries
        .iter()
        .map(|e| e.help.chars().count() + if e.custom { 8 } else { 0 })
        .max()
        .unwrap_or(12);
    let width =
        ((cmd_col + help_col + 4) as u16).clamp(34, f.area().width.saturating_sub(2).max(10));
    let y = input.y.saturating_sub(height).max(above.y);
    let rect = Rect {
        x: input.x + 1,
        y,
        width: width.min(f.area().width.saturating_sub(input.x + 1)),
        height: height.min(input.y.saturating_sub(above.y).max(3)),
    };
    app.areas.palette = rect;
    f.render_widget(Clear, rect);
    let cmd_w = entries
        .iter()
        .map(|e| e.command.chars().count())
        .max()
        .unwrap_or(8)
        + 2;
    let items: Vec<ListItem> = entries
        .iter()
        .map(|e| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<w$}", e.command, w = cmd_w),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(e.help, Style::default().fg(Color::Indexed(245))),
                Span::styled(
                    if e.custom { "  custom" } else { "" },
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(if entries.len() > shown {
                    format!(" commands · {} ", entries.len())
                } else {
                    " commands ".to_string()
                }),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Indexed(238))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select(Some(app.palette.selected));
    f.render_stateful_widget(list, rect, &mut state);
}

const BAR_BG: Color = Color::Indexed(236);
const SPINNER: [&str; 10] = [
    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}",
    "\u{2807}", "\u{280f}",
];

fn spinner(tick: u64) -> &'static str {
    SPINNER[(tick as usize) % SPINNER.len()]
}

/// A quiet colour cue per provider, so the active account is recognisable at a glance.
fn provider_color(p: ProviderKind) -> Color {
    match p {
        ProviderKind::Codex => Color::Indexed(114),
        ProviderKind::Claude => Color::Indexed(215),
        ProviderKind::Cursor => Color::Indexed(111),
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let bar = Style::default().bg(BAR_BG).fg(Color::Indexed(252));
    let accent = if app.busy && app.pulse_on {
        Color::Cyan
    } else if app.busy {
        Color::Blue
    } else {
        Color::Indexed(75)
    };
    let model = if app.model == "?" {
        "model ?".to_string()
    } else {
        app.model.clone()
    };
    let mut left = vec![
        Span::styled(" \u{25c6} ", bar.fg(accent).add_modifier(Modifier::BOLD)),
        Span::styled("concordia", bar.add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {}", short_stamp()), bar.fg(Color::DarkGray)),
        Span::styled("   ", bar),
        Span::styled(
            format!(" {} ", app.account_label()),
            Style::default()
                .bg(provider_color(app.provider))
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  {model}"), bar.fg(Color::Gray)),
    ];
    if app.native_agents.count() > 0 {
        left.push(Span::styled(
            format!("  /agents {}", app.native_agents.count()),
            bar.fg(Color::Cyan),
        ));
    }
    if let Some(chip) = app.session_chip.as_deref() {
        left.push(Span::styled(
            format!("  \u{25b8} {chip}"),
            bar.fg(Color::DarkGray),
        ));
    }
    let usage_color = match app.usage_tone {
        UsageTone::Ok => Color::Green,
        UsageTone::Warn => Color::Yellow,
        UsageTone::Bad => Color::Red,
        UsageTone::Mute => Color::DarkGray,
    };
    let right = format!(" {} ", app.usage_summary);
    let used: usize = left.iter().map(|s| s.content.chars().count()).sum();
    let pad = (area.width as usize).saturating_sub(used + right.chars().count());
    left.push(Span::styled(" ".repeat(pad), bar));
    left.push(Span::styled(right, bar.fg(usage_color)));
    f.render_widget(Paragraph::new(Line::from(left)).style(bar), area);
}

fn draw_transcript(f: &mut Frame, area: Rect, app: &mut App) {
    // No box: the conversation is the page. A little side padding keeps text off the edges.
    let block = Block::default().padding(ratatui::widgets::Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    app.areas.transcript = inner;

    let spacer = || TBlock {
        identity: None,
        gutter: "",
        gutter_style: Style::default(),
        style: Style::default(),
        text: "\n",
        markdown: false,
        runs: &[],
    };
    let live_thought = if app.expanded_activity {
        app.live_thinking.clone()
    } else {
        tail_lines(&condense_thinking(&app.live_thinking), 3)
    };
    let displayed: Vec<Option<String>> = app
        .messages
        .iter()
        .enumerate()
        .map(|(idx, m)| match m.role {
            MsgRole::Card if app.expanded_activity || app.activity.expanded.contains(&idx) => m
                .card
                .as_ref()
                .map(|c| format!("{}\n{}", c.title, c.detail.as_deref().unwrap_or("")))
                .map(Some)
                .unwrap_or(None),
            MsgRole::Thinking if !app.expanded_activity => Some(format!(
                "{} · Ctrl+O expands activity",
                crate::provider::stream_cards::compact_detail(
                    &tail_lines(&condense_thinking(&m.text), 1),
                    100
                )
            )),
            _ => None,
        })
        .collect();
    let tip_text = app.visible_tip().map(|t| format!("tip: {t}"));
    let cur_meta = app.reply_meta();
    // Coloured runs for `/usage` blocks (empty for every other message).
    let msg_runs: Vec<Vec<transcript::LineRun>> = app
        .messages
        .iter()
        .map(|m| {
            if m.role == MsgRole::System {
                usage_style::runs(&m.text)
            } else {
                Vec::new()
            }
        })
        .collect();
    let mut blocks: Vec<TBlock> = Vec::with_capacity(app.messages.len() * 3 + 4);
    // Inside a slash-command output block (after its echo) plain lines get no icon.
    let mut in_block = false;
    let count = app.messages.len();
    for (i, m) in app.messages.iter().enumerate() {
        let (mut gutter, gutter_style, style) = match m.role {
            MsgRole::User => (
                "> ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            MsgRole::Assistant => (
                "\u{25cf} ",
                Style::default().fg(Color::Indexed(179)),
                Style::default(),
            ),
            // Command echo: the title of a dim, barred output block.
            MsgRole::Command => (
                "| ",
                Style::default().fg(Color::Indexed(244)),
                Style::default()
                    .fg(Color::Indexed(252))
                    .add_modifier(Modifier::BOLD),
            ),
            // Command output and notices: neutral bar, small coloured level icon, dim text.
            MsgRole::System => {
                let lvl = level_of(&m.text);
                let g = match (lvl, in_block) {
                    (Level::Error, _) => "| x ",
                    (Level::Warn, _) => "| ! ",
                    (Level::Ok, _) => "| + ",
                    (Level::Info, false) => "| i ",
                    (Level::Info, true) => "|   ",
                };
                let col = match lvl {
                    Level::Error => Color::Red,
                    Level::Warn => Color::Yellow,
                    Level::Ok => Color::Green,
                    Level::Info if in_block => Color::Indexed(240),
                    Level::Info => Color::Indexed(75),
                };
                let text_col = if lvl == Level::Error {
                    Color::Red
                } else {
                    Color::Indexed(245)
                };
                (g, Style::default().fg(col), Style::default().fg(text_col))
            }
            MsgRole::Thinking => (
                "~ ",
                Style::default().fg(Color::Magenta),
                Style::default()
                    .fg(Color::Indexed(244))
                    .add_modifier(Modifier::ITALIC),
            ),
            MsgRole::Meta => (
                "  ",
                Style::default(),
                Style::default()
                    .fg(Color::Indexed(244))
                    .add_modifier(Modifier::DIM | Modifier::ITALIC),
            ),
            MsgRole::Card => (
                "  ",
                Style::default(),
                Style::default().fg(match m.card.as_ref().map(|c| c.status) {
                    Some(CardStatus::Running) => Color::Cyan,
                    Some(CardStatus::Error) => Color::Red,
                    Some(CardStatus::Done) | None => Color::Indexed(108),
                }),
            ),
        };
        if m.role == MsgRole::Card {
            gutter = if app.expanded_activity || app.activity.expanded.contains(&i) {
                "▾ "
            } else {
                "▸ "
            };
        }
        // Breathing room between turns, but tool cards stay grouped with what triggered them.
        if matches!(m.role, MsgRole::User | MsgRole::Command) && i > 0 {
            blocks.push(spacer());
        }
        match m.role {
            MsgRole::Command => in_block = true,
            MsgRole::System => {}
            _ => in_block = false,
        }
        // Label the start of each agent turn (its cards, thoughts and reply share one label).
        if let Some((p, label)) = &m.meta {
            let same = i > 0 && app.messages[i - 1].meta.as_ref().map(|x| &x.1) == Some(label);
            if !same {
                blocks.push(reply_header(*p, label));
            }
        }
        blocks.push(TBlock {
            identity: Some(i),
            gutter,
            gutter_style,
            style,
            text: displayed[i].as_deref().unwrap_or(&m.text),
            markdown: m.role == MsgRole::Assistant,
            runs: &msg_runs[i],
        });
        let next_is_card = app
            .messages
            .get(i + 1)
            .is_some_and(|n| matches!(n.role, MsgRole::Card | MsgRole::Meta));
        if matches!(m.role, MsgRole::Assistant | MsgRole::Meta) && i + 1 < count && !next_is_card {
            blocks.push(spacer());
        }
    }
    let last_labelled = app
        .messages
        .last()
        .and_then(|x| x.meta.as_ref())
        .map(|x| &x.1);
    if app.busy
        && (!live_thought.is_empty() || !app.live_text.trim().is_empty())
        && last_labelled != Some(&cur_meta.1)
    {
        blocks.push(reply_header(cur_meta.0, &cur_meta.1));
    }
    // Live reasoning while the turn runs: the last few lines, dimmed.
    if app.busy && !live_thought.is_empty() {
        blocks.push(TBlock {
            identity: None,
            gutter: "~ ",
            gutter_style: Style::default().fg(Color::Magenta),
            style: Style::default()
                .fg(Color::Indexed(244))
                .add_modifier(Modifier::ITALIC),
            text: &live_thought,
            markdown: false,
            runs: &[],
        });
    }
    // Live assistant text while the turn is still running.
    if app.busy && !app.live_text.trim().is_empty() {
        blocks.push(TBlock {
            identity: None,
            gutter: "\u{25cf} ",
            gutter_style: Style::default().fg(Color::Indexed(179)),
            style: Style::default(),
            text: &app.live_text,
            markdown: true,
            runs: &[],
        });
    }
    if let Some(tip) = &tip_text {
        blocks.push(TBlock {
            identity: None,
            gutter: "  ",
            gutter_style: Style::default(),
            style: Style::default()
                .fg(Color::Indexed(244))
                .add_modifier(Modifier::DIM | Modifier::ITALIC),
            text: tip,
            markdown: false,
            runs: &[],
        });
    }
    app.view
        .rebuild(&blocks, inner.width as usize, inner.height as usize);
    f.render_widget(Paragraph::new(app.view.lines()).block(block), area);
    if let Some(i) = app.activity.detail {
        if let Some(card) = app.messages.get(i).and_then(|m| m.card.as_ref()) {
            let title = card.title.as_str();
            let body = card.detail.as_deref().unwrap_or("");
            app.activity.search_parts(&[title, body]);
            let popup = centered_rect(80, 75, area);
            f.render_widget(Clear, popup);
            let inner = Block::default().borders(Borders::ALL).inner(popup);
            let avail_w = inner.width.max(1) as usize;
            if app.activity.follow_match || app.activity.render_width != avail_w {
                if let Some(found) = app.activity.selected() {
                    app.activity.detail_scroll = detail_scroll_target_parts(
                        &[title, body],
                        found,
                        avail_w,
                        inner.height as usize,
                    );
                }
                app.activity.follow_match = false;
            }
            if app.activity.render_dirty
                || app.activity.render_width != avail_w
                || app.activity.render_selected != app.activity.selected_match
            {
                let lines = detail_lines_parts(
                    &[title, body],
                    &app.activity.matches,
                    app.activity.selected_match,
                    avail_w,
                );
                app.activity.render_lines = lines
                    .into_iter()
                    .map(|line| {
                        Line::from(
                            line.spans
                                .into_iter()
                                .map(|span| Span::styled(span.content.into_owned(), span.style))
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect();
                app.activity.render_width = avail_w;
                app.activity.render_selected = app.activity.selected_match;
                app.activity.render_dirty = false;
            }
            let query = app.activity.query_with_cursor();
            let block=Block::default().title(format!("Activity · Find: {query} · {} matches · Enter/Shift+Enter find · Alt+↑/↓ card · Ctrl+C copy · Esc close", app.activity.matches.len())).borders(Borders::ALL);
            let inner = block.inner(popup);
            f.render_widget(block, popup);
            let max_scroll = app
                .activity
                .render_lines
                .len()
                .saturating_sub(inner.height as usize);
            let scroll = (app.activity.detail_scroll as usize).min(max_scroll);
            app.activity.detail_scroll = scroll.min(u16::MAX as usize) as u16;
            f.render_widget(
                DetailRows {
                    lines: &app.activity.render_lines,
                    scroll,
                },
                inner,
            );
        }
    }
}

fn detail_scroll_target_parts(
    parts: &[&str],
    found: activity::Match,
    width: usize,
    height: usize,
) -> u16 {
    let width = width.max(1);
    let mut visual = 0usize;
    let mut target = None;
    for (line_no, line) in parts.iter().flat_map(|p| p.lines()).enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let ranges = text::wrap_ranges(&chars, width);
        if line_no == found.line {
            let row = ranges
                .iter()
                .position(|(a, b)| *a <= found.col && found.col < *b)
                .unwrap_or_else(|| ranges.len().saturating_sub(1));
            target = Some(visual + row);
        }
        visual += ranges.len();
    }
    let max_scroll = visual.saturating_sub(height);
    let wanted = target
        .unwrap_or(0)
        .saturating_sub(height / 2)
        .min(max_scroll);
    wanted.min(u16::MAX as usize) as u16
}

fn detail_lines_parts<'a>(
    parts: &[&'a str],
    matches: &[activity::Match],
    selected: usize,
    width: usize,
) -> Vec<Line<'a>> {
    let mut output = Vec::new();
    let mut match_cursor = 0;
    for (line_no, line) in parts.iter().flat_map(|p| p.lines()).enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let byte_offsets: Vec<usize> = line
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(line.len()))
            .collect();
        while match_cursor < matches.len() && matches[match_cursor].line < line_no {
            match_cursor += 1;
        }
        let first = match_cursor;
        while match_cursor < matches.len() && matches[match_cursor].line == line_no {
            match_cursor += 1;
        }
        let line_matches = &matches[first..match_cursor];
        for (row_start, row_end) in text::wrap_ranges(&chars, width.max(1)) {
            let mut cuts = vec![row_start, row_end];
            for m in line_matches {
                let a = m.col.max(row_start);
                let b = (m.col + m.len).min(row_end);
                if a < b {
                    cuts.extend([a, b]);
                }
            }
            cuts.sort_unstable();
            cuts.dedup();
            let mut spans = Vec::new();
            let mut mi = 0;
            for pair in cuts.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if a == b {
                    continue;
                }
                while mi < line_matches.len() && line_matches[mi].col + line_matches[mi].len <= a {
                    mi += 1;
                }
                let active = line_matches
                    .get(mi)
                    .filter(|m| m.col <= a && b <= m.col + m.len);
                let style = active
                    .map(|_| {
                        if first + mi == selected {
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Cyan)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::Black).bg(Color::Yellow)
                        }
                    })
                    .unwrap_or_else(|| {
                        if parts
                            .iter()
                            .any(|part| part.lines().any(|line| line.starts_with("@@")))
                        {
                            transcript::diff_line_style(line)
                        } else {
                            Style::default()
                        }
                    });
                spans.push(Span::styled(&line[byte_offsets[a]..byte_offsets[b]], style));
            }
            output.push(Line::from(spans));
        }
    }
    output
}

struct DetailRows<'a> {
    lines: &'a [Line<'static>],
    scroll: usize,
}
impl Widget for DetailRows<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for (row, line) in self
            .lines
            .iter()
            .skip(self.scroll)
            .take(area.height as usize)
            .enumerate()
        {
            buf.set_line(area.x, area.y + row as u16, line, area.width);
        }
    }
}

fn centered_rect(width_pct: u16, height_pct: u16, area: Rect) -> Rect {
    let w = (area.width * width_pct / 100).max(1).min(area.width);
    let h = (area.height * height_pct / 100).max(1).min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

fn draw_picker(f: &mut Frame, area: Rect, app: &mut App) {
    let Some(picker) = app.picker.as_mut() else {
        return;
    };
    let height = (picker.filtered().len().min(10) as u16)
        .saturating_add(2)
        .max(5);
    let width = area.width.saturating_sub(4).max(20);
    let popup = Rect {
        x: area.x + 2,
        y: area.y + area.height.saturating_sub(height),
        width,
        height: height.min(area.height),
    };
    f.render_widget(Clear, popup);
    let title = if picker.filter.is_empty() {
        picker.title.clone()
    } else {
        format!("{} · filter: {}", picker.title, picker.filter)
    };
    let tones = &app.picker_tones;
    let items: Vec<ListItem> = picker
        .filtered()
        .into_iter()
        .map(|r| {
            let label_style = match tones.get(&r.id) {
                Some(UsageTone::Ok) => Style::default().fg(Color::Green),
                Some(UsageTone::Warn) => Style::default().fg(Color::Yellow),
                Some(UsageTone::Bad) => Style::default().fg(Color::Red),
                Some(UsageTone::Mute) => Style::default().fg(Color::DarkGray),
                None => Style::default(),
            };
            let (head, current) = match r.label.split_once(" \u{b7} current") {
                Some((h, _)) => (h.to_string(), true),
                None => (r.label.clone(), false),
            };
            let mut spans = match usage_style::picker_spans(&head) {
                Some(s) => s,
                None => vec![Span::styled(head, label_style)],
            };
            if current {
                spans.push(Span::styled(
                    " \u{b7} current",
                    Style::default().fg(Color::Green),
                ));
            }
            spans.extend([Span::styled(
                format!("  {}", r.detail),
                Style::default().fg(Color::DarkGray),
            )]);
            ListItem::new(Line::from(spans))
        })
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    f.render_stateful_widget(list, popup, &mut picker.state);
}

fn draw_input(f: &mut Frame, area: Rect, app: &mut App) {
    let border = if app.busy {
        app.phase.color()
    } else {
        Color::Indexed(240)
    };
    let title = if app.busy {
        let color = app.phase.color();
        let secs = app.turn_started.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        Line::from(vec![
            Span::raw(" "),
            Span::styled(app.phase.frame(app.tick_count), Style::default().fg(color)),
            Span::styled(
                format!(" {}", app.phase.label()),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" \u{b7} {secs}s \u{b7} Esc to cancel "),
                Style::default().fg(Color::Gray),
            ),
        ])
    } else {
        Line::from(Span::styled(
            " message ",
            Style::default().fg(Color::DarkGray),
        ))
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(title);
    let inner = block.inner(area);
    let text_w = (inner.width as usize).saturating_sub(PROMPT_W).max(1);
    app.areas.input = inner;
    app.areas.input_text_w = text_w;

    let rows = app.editor.layout(text_w);
    let (cur_row, cur_col) = app.editor.cursor_pos(text_w);
    let h = (inner.height as usize).max(1);
    // Keep the caret row visible; never scroll past the last row.
    if cur_row < app.input_scroll {
        app.input_scroll = cur_row;
    } else if cur_row >= app.input_scroll + h {
        app.input_scroll = cur_row + 1 - h;
    }
    app.input_scroll = app.input_scroll.min(rows.len().saturating_sub(h));

    let chars = app.editor.chars();
    let sel = app.editor.selection();
    let hl = Style::default().add_modifier(Modifier::REVERSED);
    let prompt = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let empty = chars.is_empty() && !app.busy;
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .skip(app.input_scroll)
        .take(h)
        .map(|(i, r)| {
            let seg = |a: usize, b: usize| -> String { chars[a..b].iter().collect() };
            let mut spans = vec![if i == 0 {
                Span::styled("> ", prompt)
            } else {
                Span::raw("  ")
            }];
            if empty && i == 0 {
                spans.push(Span::styled(
                    "Message Concordia...  (/ for commands)",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ));
                return Line::from(spans);
            }
            match sel.map(|(a, b)| (a.max(r.start), b.min(r.end))) {
                Some((a, b)) if a < b => {
                    spans.push(Span::raw(seg(r.start, a)));
                    spans.push(Span::styled(seg(a, b), hl));
                    spans.push(Span::raw(seg(b, r.end)));
                }
                _ => spans.push(Span::raw(seg(r.start, r.end))),
            }
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(block), area);

    // Real terminal caret (blinks natively, works with IME); hidden while a picker is open.
    if app.picker.is_none()
        && app.activity.detail.is_none()
        && !app.queue.focused
        && app.workspace.focused.is_none()
    {
        let x = inner.x + (PROMPT_W + cur_col).min(inner.width.saturating_sub(1) as usize) as u16;
        let y = inner.y + (cur_row - app.input_scroll) as u16;
        f.set_cursor_position(Position::new(x, y));
    }
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let w = area.width as usize;
    let mode = app.trust;
    let label = if mode == TrustMode::Full {
        "full \u{26a0}"
    } else {
        mode_label(mode)
    };
    let color = mode_color(mode);
    // The mode column is kept only while it leaves room for the first hint.
    const MIN_LEFT: usize = 12;
    let right = [format!("auth {label} "), format!("{label} ")]
        .into_iter()
        .find(|r| r.chars().count() + MIN_LEFT <= w)
        .unwrap_or_default();
    let rw = right.chars().count() as u16;
    let cols = [
        Rect {
            width: area.width - rw,
            ..area
        },
        Rect {
            x: area.x + area.width - rw,
            width: rw,
            ..area
        },
    ];
    let flash = app
        .flash
        .as_ref()
        .filter(|(_, at)| at.elapsed() < FLASH_FOR)
        .map(|(m, _)| m.as_str());
    let job_line;
    let scrolled = format!("\u{2191} {} rows up \u{b7} PgDn to follow", app.view.scroll);
    let (left, left_style) = if app.recovery_warning().is_some() {
        (
            "draft not saved locally · /recover retry",
            Style::default().fg(Color::Red),
        )
    } else if let Some(m) = flash {
        (m, Style::default().fg(Color::Indexed(179)))
    } else if let Some(job) = &app.job {
        let what = match job.kind {
            JobKind::Update => "updating",
            JobKind::Check => "checking",
        };
        let line = job.status();
        let shown: String = line.chars().take(60).collect();
        job_line = format!(
            "{} {what} \u{b7} {}s{}{} \u{b7} Esc cancels",
            spinner(app.tick_count),
            job.started.elapsed().as_secs(),
            if shown.is_empty() { "" } else { " \u{b7} " },
            shown
        );
        (job_line.as_str(), Style::default().fg(Color::Cyan))
    } else if !app.palette_entries().is_empty() {
        (
            "\u{2191}\u{2193} select \u{b7} Tab complete \u{b7} Enter run \u{b7} Esc close",
            Style::default().fg(Color::DarkGray),
        )
    } else if app.view.scroll > 0 {
        (scrolled.as_str(), Style::default().fg(Color::Indexed(179)))
    } else if app.busy {
        (
            "Tab queue \u{b7} Ctrl+O expand \u{b7} Ctrl+C cancel",
            Style::default().fg(Color::DarkGray),
        )
    } else {
        (
            app.footer_left.as_str(),
            Style::default().fg(Color::DarkGray),
        )
    };
    let focus_hint = queue_focus::focus_hint(app).map(|hint| format!("{left} · {hint}"));
    let left = focus_hint.as_deref().unwrap_or(left);
    let stats_reserve = app
        .stats
        .as_ref()
        .and_then(|h| stats_ui::fit_stats(&h.latest(), (cols[0].width as usize).saturating_sub(20)))
        .map(|s| s.chars().count() + 3)
        .unwrap_or(0);
    let avail = (cols[0].width as usize).saturating_sub(2 + stats_reserve);
    let shown = if left == FOOTER_HINT {
        fit_hints(avail)
    } else {
        fit_segments(left, avail)
    };
    f.render_widget(
        Paragraph::new(format!(" {shown}")).style(left_style),
        cols[0],
    );
    stats_ui::draw_footer_stats(f, cols[0], 1 + shown.chars().count(), app);
    // Approvals and sandboxing are bypassed in full mode: its red glyph must stay visible.
    if rw > 0 {
        f.render_widget(
            Paragraph::new(right)
                .alignment(Alignment::Right)
                .style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
            cols[1],
        );
    }
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
            .fg(provider_color(p))
            .add_modifier(Modifier::BOLD),
        text: label,
        markdown: false,
        runs: &[],
    }
}

/// Next permission mode for Shift+Tab: plan -> ask -> workspace (auto-edit) -> full -> plan.
fn next_mode(m: TrustMode) -> TrustMode {
    match m {
        TrustMode::Plan => TrustMode::Ask,
        TrustMode::Ask => TrustMode::Workspace,
        TrustMode::Workspace => TrustMode::Full,
        TrustMode::Full => TrustMode::Plan,
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
        TrustMode::Ask => "approvals on, may hang headless",
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

fn provider_name(p: ProviderKind) -> &'static str {
    match p {
        ProviderKind::Codex => "Codex",
        ProviderKind::Claude => "Claude",
        ProviderKind::Cursor => "Cursor",
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
const HINTS: [(&str, &str); 5] = [
    ("Enter send", "Enter send"),
    ("/ commands", "/ cmds"),
    ("Shift+Tab mode", "S-Tab mode"),
    ("drag to select", "drag select"),
    ("Ctrl+C copy or cancel", "Ctrl+C copy/stop"),
];
/// Priority per hint (0 is kept longest); indexes follow `HINTS`.
const HINT_RANK: [usize; 5] = [0, 2, 3, 4, 1];

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

/// Reasoning text as a few readable lines: blank lines and runs of spaces collapsed, long ones cut.
fn condense_thinking(raw: &str) -> String {
    let lines: Vec<String> = raw
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect();
    let joined = lines.join("\n");
    let chars: Vec<char> = joined.chars().collect();
    if chars.len() <= 600 {
        joined
    } else {
        let tail: String = chars[chars.len() - 600..].iter().collect();
        format!("...{tail}")
    }
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
