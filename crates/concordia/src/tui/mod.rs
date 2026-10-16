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
use crate::usage::{
    picker_row, usage_icon, usage_rows_for_accounts, usage_tone, UsageFetcher, UsageTone,
};
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
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};
use std::collections::HashMap;
use std::io::{self, stdout, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

mod clipboard;
mod editor;
mod markdown;
mod palette;
mod text;
mod tips;
mod token_line;
mod transcript;
mod workers;

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
const HISTORY_MAX: usize = 200;
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum PickerKind {
    Resume,
    Trust,
    Use,
    AddProvider,
    Remove,
    RemoveConfirm,
    Model,
    Usage,
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
    state_root: PathBuf,
    cwd: PathBuf,
    messages: Vec<ChatLine>,
    editor: Editor,
    palette: Palette,
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
    clicks: ClickTracker,
    /// Transient footer message (e.g. "copied 120 chars").
    flash: Option<(String, Instant)>,
    busy: bool,
    pulse_on: bool,
    tick_count: u64,
    should_quit: bool,
    provider: ProviderKind,
    account: String,
    home_path: Option<PathBuf>,
    model: String,
    trust: TrustMode,
    native_session_id: Option<String>,
    session_chip: Option<String>,
    footer_left: String,
    picker: Option<Picker>,
    cancel_handle: Option<Arc<RunningTurn>>,
    prior_provider: ProviderKind,
    prior_home: Option<String>,
    need_context_pack: bool,
    usage_summary: String,
    usage_tone: UsageTone,
    /// Width-1 ASCII bar drawn before the footer usage text.
    usage_icon: String,
    /// Claude limits are fetched on a worker thread and cached; never block the UI on them.
    usage_fetcher: UsageFetcher,
    /// Index of the `/usage` result block, rewritten in place when a background check lands.
    usage_block: Option<usize>,
    /// Tone of each `/use` or `/usage all` picker row, by row id.
    picker_tones: HashMap<String, UsageTone>,
    live_cards: Vec<StreamCard>,
    live_text: String,
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
}

impl App {
    fn refresh_binding(&mut self) {
        if let Some(sel) = get_use_selection(&self.state_root) {
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
            self.usage_icon = usage_icon(&crate::usage::UsageSnapshot::no_data("", ""));
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
        self.usage_icon = usage_icon(snap);
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
        format!(
            "concordia · {} · {} · {}",
            self.account_label(),
            if self.model.is_empty() {
                "?"
            } else {
                &self.model
            },
            short_stamp()
        )
    }

    /// Suggestions for the prompt as typed (empty when closed, dismissed, or not a slash prompt).
    fn palette_entries(&self) -> Vec<palette::Entry> {
        // Not while a recalled prompt is on screen: up/down must keep walking history.
        if self.palette.dismissed || self.picker.is_some() || self.hist_pos.is_some() {
            return Vec::new();
        }
        palette::matching(&self.editor.text())
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
        if self.history.last().map(String::as_str) != Some(line) {
            self.history.push(line.to_string());
            if self.history.len() > HISTORY_MAX {
                self.history.remove(0);
            }
        }
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
        self.view.clear_selection();
        self.palette.reset();
        self.editor.insert_str(text);
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

    fn push_user(&mut self, text: String) {
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
        let meta = Some(self.reply_meta());
        self.messages.push(ChatLine {
            role: MsgRole::Assistant,
            text,
            card: None,
            meta,
        });
    }

    fn push_card(&mut self, card: StreamCard) {
        let text = format_card_line(&card);
        if let Some(existing) = self.messages.iter_mut().rev().find(|m| {
            m.role == MsgRole::Card
                && m.card.as_ref().map(|c| c.id.as_str()) == Some(card.id.as_str())
        }) {
            existing.card = Some(card);
            existing.text = text;
            return;
        }
        let meta = Some(self.reply_meta());
        self.messages.push(ChatLine {
            role: MsgRole::Card,
            text,
            card: Some(card),
            meta,
        });
    }

    fn apply_progress(&mut self, ev: TurnProgress) {
        match ev {
            TurnProgress::Session(id) => {
                self.native_session_id = Some(id);
            }
            TurnProgress::Thinking(t) => {
                self.phase = Phase::Thinking;
                self.live_thinking.push_str(&t);
                if !t.ends_with('\n') {
                    self.live_thinking.push('\n');
                }
            }
            TurnProgress::Text(t) => {
                self.phase = Phase::Writing;
                self.live_text.push_str(&t);
                if !t.ends_with('\n') {
                    self.live_text.push('\n');
                }
            }
            TurnProgress::Card(card) => {
                self.phase = if card.status == CardStatus::Running {
                    Phase::Tool(card.title.clone())
                } else {
                    Phase::Waiting
                };
                upsert_card(&mut self.live_cards, card.clone());
                self.push_card(card);
            }
            TurnProgress::Error(e) => {
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
        }
        let had_cards = !self.live_cards.is_empty() || !outcome.cards.is_empty();
        // Prefer streamed cards; fall back to outcome cards if nothing landed live.
        if self.live_cards.is_empty() {
            for card in outcome.cards {
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
                card.status = CardStatus::Done;
                self.push_card(card);
            }
        }
        let text = if !self.live_text.trim().is_empty() {
            self.live_text.trim().to_string()
        } else {
            outcome.text.trim().to_string()
        };
        let scan_text = if outcome.cancelled {
            String::new()
        } else {
            text.clone()
        };
        // A provider that failed to start, or exited non-zero, must say why. Its stderr is the
        // only place the reason (expired login, bad flag, missing CLI) ever appears.
        let failed = !outcome.cancelled
            && match outcome.exit_code {
                Some(code) => code != 0,
                None => !outcome.stderr_tail.trim().is_empty(),
            };
        let thought = condense_thinking(&self.live_thinking);
        if !thought.is_empty() {
            let meta = Some(self.reply_meta());
            self.messages.push(ChatLine {
                role: MsgRole::Thinking,
                text: thought,
                card: None,
                meta,
            });
        }
        if outcome.cancelled {
            self.push_system("cancelled");
        } else if !text.is_empty() {
            self.push_assistant(text);
            if token_line::ui_token_line_enabled() {
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
        } else if !had_cards && !failed {
            self.push_system("(no text in provider stream)");
        }
        if failed {
            self.push_system(match outcome.exit_code {
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
        self.live_thinking.clear();
        self.scan_delegation(&scan_text);
        self.refresh_usage();
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
                MsgRole::System
                | MsgRole::Command
                | MsgRole::Card
                | MsgRole::Thinking
                | MsgRole::Meta => None,
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
        self.poll_pending_login();
        self.poll_job();
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
                            "mode \u{2192} {} - {}",
                            mode_label(mode),
                            mode_blurb(mode)
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
        self.show_picker(PickerKind::Use, "accounts".into(), rows, true);
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
            "↑↓ Enter · Esc".into()
        };
    }

    fn dismiss_picker(&mut self) {
        self.picker = None;
        self.footer_left = FOOTER_HINT.into();
    }

    fn apply_picker_selection(&mut self) {
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
        match kind {
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
                    let prev_home = self
                        .home_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned());
                    let prev_prov = self.provider;
                    if let Err(e) = select_account(&self.state_root, &account) {
                        self.push_system(format!("resume account switch failed — {e}"));
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
                        session_id: Some(&session_id),
                        messages: &msgs,
                        include_pack_if_needed: true,
                    });
                    self.native_session_id = decision.session_id.clone();
                    self.need_context_pack = decision.pack.is_some();
                    self.session_chip = Some(label.clone());
                    if decision.native_resume {
                        self.messages.clear();
                        self.push_system(format!(
                            "resumed · {} · {label} — next message continues that session (pack omitted)",
                            self.account_label()
                        ));
                    } else if let Some(pack) = &decision.pack {
                        self.push_system(format!("{} · ~{} tok", pack.labeled, pack.est_tokens));
                    } else {
                        self.push_system(decision.reason);
                    }
                    self.prior_provider = self.provider;
                    self.prior_home = new_home;
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
                        include_pack_if_needed: true,
                    });
                    self.native_session_id = decision.session_id.clone();
                    if !decision.native_resume {
                        self.session_chip = None;
                    }
                    self.need_context_pack = decision.pack.is_some();
                    if let Some(pack) = &decision.pack {
                        self.push_system(format!("{} · ~{} tok", pack.labeled, pack.est_tokens));
                    } else if !decision.native_resume {
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
                        include_pack_if_needed: true,
                    });
                    self.native_session_id = decision.session_id.clone();
                    self.session_chip = None;
                    self.need_context_pack = decision.pack.is_some();
                    if let Some(pack) = &decision.pack {
                        self.push_system(format!("{} · ~{} tok", pack.labeled, pack.est_tokens));
                    }
                    self.prior_provider = self.provider;
                    self.prior_home = new_home;
                    self.push_system(format!("using {} · {}", self.account_label(), self.model));
                }
            }
        }
        let _ = label;
    }

    fn handle_slash(&mut self, line: &str) -> bool {
        let Some((cmd, split_vertical)) = parse_slash(line) else {
            return false;
        };
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
            self.messages.clear();
            self.clear_selections();
            self.native_session_id = None;
            self.session_chip = None;
            self.need_context_pack = false;
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
            CommandOutcome::OpenUse => self.open_use_picker(),
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
        let line = self.editor.text().trim().to_string();
        if line.is_empty() {
            return;
        }
        if self.job.as_ref().is_some_and(|j| j.kind == JobKind::Update) && !line.starts_with('/') {
            self.push_system("an update is in progress - Esc cancels it, or wait for it to finish");
            return;
        }
        self.remember_prompt(&line);
        self.editor.clear();
        self.clear_selections();
        self.view.follow_tail();
        if self.handle_slash(&line) {
            self.flush_delegate_prompt(turn_tx);
            return;
        }
        self.push_user(line.clone());
        self.turn_started = Some(Instant::now());
        self.phase = Phase::Waiting;
        self.live_thinking.clear();
        self.busy = true;
        self.pulse_on = true;
        let _ = turn_tx.send(line);
    }

    fn request_cancel(&mut self) {
        if let Some(h) = &self.cancel_handle {
            h.cancel();
            self.push_system("cancelling...");
        } else {
            self.busy = false;
            self.push_system("cancelled");
        }
    }
}

impl App {
    fn new(state_root: PathBuf, cwd: PathBuf) -> App {
        let state_root_for_usage = state_root.clone();
        App {
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
            palette: Palette::default(),
            history: Vec::new(),
            hist_pos: None,
            hist_draft: String::new(),
            quit_armed: None,
            input_scroll: 0,
            view: TranscriptView::default(),
            areas: Areas::default(),
            drag: Drag::None,
            clicks: ClickTracker::default(),
            flash: None,
            busy: false,
            pulse_on: false,
            tick_count: 0,
            should_quit: false,
            provider: ProviderKind::Codex,
            account: "codex".into(),
            home_path: None,
            model: "?".into(),
            trust: TrustMode::Workspace,
            native_session_id: None,
            session_chip: None,
            footer_left: FOOTER_HINT.into(),
            picker: None,
            cancel_handle: None,
            prior_provider: ProviderKind::Codex,
            prior_home: None,
            need_context_pack: false,
            usage_summary: "limits unknown".into(),
            usage_tone: UsageTone::Mute,
            usage_icon: usage_icon(&crate::usage::UsageSnapshot::no_data("", "")),
            usage_fetcher: UsageFetcher::new(state_root_for_usage),
            usage_block: None,
            picker_tones: HashMap::new(),
            live_cards: Vec::new(),
            live_text: String::new(),
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
    let cwd = std::env::current_dir()?;

    let (turn_tx, mut turn_rx) = mpsc::unbounded_channel::<String>();
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<TurnOutcome>();
    let (handle_tx, mut handle_rx) = mpsc::unbounded_channel::<Arc<RunningTurn>>();
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<TurnProgress>();

    let mut app = App::new(state_root.clone(), cwd.clone());
    app.refresh_binding();
    app.prior_provider = app.provider;
    app.prior_home = app
        .home_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // Mouse capture replaces the terminal's own selection (concordia draws its own); bracketed paste
    // delivers pasted newlines as text instead of Enter presses. Both are best-effort.
    let _ = execute!(stdout, EnableMouseCapture, EnableBracketedPaste);
    // A panic must not leave the terminal in raw/mouse-reporting mode.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
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

    let tick_rate = Duration::from_millis(200);
    let mut last_tick = Instant::now();
    let mut last_key: Option<(KeyCode, Instant)> = None;

    loop {
        terminal.draw(|f| draw_ui(f, &mut app))?;
        write_osc_title(&app.window_title())?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            // Drain a burst (mouse drags, pastes) before redrawing; bounded so a flood can't
            // starve the turn/progress channels below.
            let mut quit = false;
            for _ in 0..256 {
                match event::read()? {
                    Event::Key(key) => {
                        if should_accept_key(&key, &mut last_key)
                            && handle_key(&mut app, key, &turn_tx)
                        {
                            quit = true;
                            break;
                        }
                    }
                    Event::Mouse(m) => handle_mouse(&mut app, m),
                    Event::Paste(s) => app.paste_text(&s),
                    _ => {}
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
            if quit {
                break;
            }
        }

        while let Ok(h) = handle_rx.try_recv() {
            app.cancel_handle = Some(h);
        }

        while let Ok(prompt) = turn_rx.try_recv() {
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
            app.busy = false;
            app.pulse_on = false;
            app.cancel_handle = None;
            app.finalize_turn(outcome);
        }

        if last_tick.elapsed() >= tick_rate {
            app.on_tick();
            last_tick = Instant::now();
        }

        if app.should_quit || app.reexec_path.is_some() {
            break;
        }
    }

    // Leaving mid-turn must not orphan the provider (it would keep running and spending).
    if let Some(h) = app.cancel_handle.take() {
        h.cancel();
    }
    app.cancel_all_delegates();
    let reexec = app.reexec_path.take();
    let cwd = app.cwd.clone();
    let _ = std::panic::take_hook(); // drop our hook; default behavior resumes
    let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    let _ = execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        DisableMouseCapture
    );
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    if let Some(path) = reexec {
        let code = reexec_into(&path, &cwd)?;
        std::process::exit(code);
    }
    Ok(())
}

/// Drop Release events and Press+Release duplicates (Windows often reports both as Press).
fn should_accept_key(key: &KeyEvent, last: &mut Option<(KeyCode, Instant)>) -> bool {
    if matches!(key.kind, KeyEventKind::Release) {
        return false;
    }
    // Enter also arrives as Char('\r') / '\n' on some hosts — ignore those chars.
    if matches!(key.code, KeyCode::Char('\r') | KeyCode::Char('\n')) {
        return false;
    }
    let now = Instant::now();
    if let Some((code, at)) = *last {
        if code == key.code && now.duration_since(at) < Duration::from_millis(40) {
            // Duplicate Press/Release pair without distinct kinds.
            if matches!(key.kind, KeyEventKind::Press) {
                return false;
            }
        }
    }
    *last = Some((key.code, now));
    true
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
            KeyCode::Enter => app.apply_picker_selection(),
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

    if workers::handle_key(app, &key) {
        return false;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let tw = app.areas.input_text_w.max(1);

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
        KeyCode::Enter if alt || shift => {
            app.editor.insert_char('\n');
            false
        }
        KeyCode::Enter => {
            app.submit_input(turn_tx);
            false
        }
        KeyCode::Tab => {
            app.tab_complete_slash();
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
                app.history_prev();
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
    if app.picker.is_some() || workers::handle_mouse(app, &m) {
        return;
    }
    let (x, y) = (m.column, m.row);
    let t = app.areas.transcript;
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
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
                app.editor.clear_selection();
                app.drag = Drag::Transcript;
                let pos = app.view.pos_at((y - t.y) as isize, (x - t.x) as usize);
                match (pos, clicks) {
                    (None, _) => {
                        app.view.clear_selection();
                        app.drag = Drag::None;
                    }
                    (Some(p), 1) => app.view.start_selection(p),
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
        MouseEventKind::Drag(MouseButton::Left) => match app.drag {
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
        },
        MouseEventKind::Up(MouseButton::Left) => {
            if app.drag == Drag::Transcript {
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
fn restore_terminal() {
    let mut out = stdout();
    let _ = execute!(
        out,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    );
    let _ = disable_raw_mode();
}

fn write_osc_title(title: &str) -> io::Result<()> {
    let mut out = stdout();
    write!(out, "\x1b]0;{title}\x07")?;
    out.flush()
}

fn draw_ui(f: &mut Frame, app: &mut App) {
    // The prompt box grows with its content (up to MAX_INPUT_ROWS), so size it first.
    let text_w = (f.area().width as usize)
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
        .split(f.area())
        .to_vec();
    // Worker rows sit between the transcript and the prompt (nothing when no worker exists).
    let strip = workers::carve(&mut chunks[1], app);

    draw_header(f, chunks[0], app);
    draw_transcript(f, chunks[1], app);
    draw_input(f, chunks[2], app);
    if app.picker.is_some() {
        draw_picker(f, chunks[1], app);
    }
    draw_palette(f, chunks[1], chunks[2], app);
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
        .map(|e| e.help.chars().count())
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
    let right = format!(" {} {} ", app.usage_icon, app.usage_summary);
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
        gutter: "",
        gutter_style: Style::default(),
        style: Style::default(),
        text: "\n",
        markdown: false,
    };
    let live_thought = tail_lines(&condense_thinking(&app.live_thinking), 3);
    let tip_text = app.visible_tip().map(|t| format!("tip: {t}"));
    let cur_meta = app.reply_meta();
    let mut blocks: Vec<TBlock> = Vec::with_capacity(app.messages.len() * 3 + 4);
    // Inside a slash-command output block (after its echo) plain lines get no icon.
    let mut in_block = false;
    let count = app.messages.len();
    for (i, m) in app.messages.iter().enumerate() {
        let (gutter, gutter_style, style) = match m.role {
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
            gutter,
            gutter_style,
            style,
            text: &m.text,
            markdown: m.role == MsgRole::Assistant,
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
            gutter: "~ ",
            gutter_style: Style::default().fg(Color::Magenta),
            style: Style::default()
                .fg(Color::Indexed(244))
                .add_modifier(Modifier::ITALIC),
            text: &live_thought,
            markdown: false,
        });
    }
    // Live assistant text while the turn is still running.
    if app.busy && !app.live_text.trim().is_empty() {
        blocks.push(TBlock {
            gutter: "\u{25cf} ",
            gutter_style: Style::default().fg(Color::Indexed(179)),
            style: Style::default(),
            text: &app.live_text,
            markdown: true,
        });
    }
    if let Some(tip) = &tip_text {
        blocks.push(TBlock {
            gutter: "  ",
            gutter_style: Style::default(),
            style: Style::default()
                .fg(Color::Indexed(244))
                .add_modifier(Modifier::DIM | Modifier::ITALIC),
            text: tip,
            markdown: false,
        });
    }
    app.view
        .rebuild(&blocks, inner.width as usize, inner.height as usize);
    f.render_widget(Paragraph::new(app.view.lines()).block(block), area);
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
            let mut spans = vec![Span::styled(head, label_style)];
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
    if app.picker.is_none() {
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
    let (left, left_style) = if let Some(m) = flash {
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
            "thinking \u{b7} Ctrl+C cancel",
            Style::default().fg(Color::DarkGray),
        )
    } else {
        (
            app.footer_left.as_str(),
            Style::default().fg(Color::DarkGray),
        )
    };
    let avail = (cols[0].width as usize).saturating_sub(2);
    let shown = if left == FOOTER_HINT {
        fit_hints(avail)
    } else {
        fit_segments(left, avail)
    };
    f.render_widget(
        Paragraph::new(format!(" {shown}")).style(left_style),
        cols[0],
    );
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
mod tests;

fn reply_header(p: ProviderKind, label: &str) -> TBlock<'_> {
    TBlock {
        gutter: "",
        gutter_style: Style::default(),
        style: Style::default()
            .fg(provider_color(p))
            .add_modifier(Modifier::BOLD),
        text: label,
        markdown: false,
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
