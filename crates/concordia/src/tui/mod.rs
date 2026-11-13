use crate::accounts::{accounts_with_auth, display_account_name, get_account, is_logged_in};
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
    count_sessions_here, list_recent_sessions, native_session_picker_rows, tasks_to_picker_rows,
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
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
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
pub use run::{run_chat_tui, run_file_editor};
mod brand;
mod chat_ownership;
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
mod local_terminal;
mod managed_workspaces;
mod markdown;
mod modal;
mod native_agents;
#[cfg(windows)]
mod native_window;
mod palette;
mod persist;
mod pointer;
mod questions;
mod queue;
mod queue_controls;
mod queue_focus;
mod raster;
mod recovery;
mod reference_journal;
mod related_files;
mod session_ui;
#[cfg(test)]
mod session_ui_tests;
mod settings;
mod stats_ui;
mod syntax;
pub(crate) mod tabs;
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
const START_HINT: &str = "Type a message and press Enter. Start with / for commands. Drag to select; Ctrl+C copies; Esc interrupts.";
/// How often (in 200 ms ticks) to look for a finished login, and when to stop looking.
/// A second idle Ctrl+C within this window exits.
const QUIT_WINDOW: Duration = Duration::from_secs(2);
const HISTORY_MAX: usize = persist::HISTORY_CAP;
const LOGIN_POLL_TICKS: u32 = 10;
const LOGIN_GIVE_UP_TICKS: u32 = 3000;

fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
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
        decor_prefix: 0,
    }
}

/// Next permission mode for Shift+Tab: plan -> ask -> edit -> plan.
fn next_mode(m: TrustMode) -> TrustMode {
    match m {
        TrustMode::Plan => TrustMode::Ask,
        TrustMode::Ask => TrustMode::Edit,
        TrustMode::Edit | TrustMode::Full => TrustMode::Plan,
    }
}

fn mode_label(m: TrustMode) -> &'static str {
    match m {
        TrustMode::Plan => "plan",
        TrustMode::Ask => "ask",
        TrustMode::Edit => "edit",
        TrustMode::Full => "full",
    }
}

fn mode_blurb(m: TrustMode) -> &'static str {
    match m {
        TrustMode::Plan => "read-only, analyse and propose (advisory where a CLI has no plan flag)",
        TrustMode::Ask => "native approvals (provider support varies)",
        TrustMode::Edit => "edit files with approval for other actions",
        TrustMode::Full => "skips all permissions - careful",
    }
}

fn mode_color(m: TrustMode) -> Color {
    match m {
        TrustMode::Plan => Color::Cyan,
        TrustMode::Ask => Color::Yellow,
        TrustMode::Edit => Color::Green,
        TrustMode::Full => Color::Red,
    }
}

fn parse_mode(s: &str) -> Option<TrustMode> {
    match s.trim().to_lowercase().as_str() {
        "plan" => Some(TrustMode::Plan),
        "ask" => Some(TrustMode::Ask),
        "edit" => Some(TrustMode::Edit),
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
        // Codex emits these capability notices when enterprise policy advertises
        // features this installed client does not know. They are informational,
        // not turn failures, and should not become transcript error cards.
        || l.starts_with("ignoring unknown features requirement")
}

/// A provider may coalesce several informational stderr lines into one progress error.
/// Treat the block as noise only when every non-empty line is known chatter.
pub(super) fn is_cli_noise_block(text: &str) -> bool {
    let mut any = false;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        any = true;
        if !is_cli_noise(line) {
            return false;
        }
    }
    any
}

#[cfg(test)]
mod screenshots;
