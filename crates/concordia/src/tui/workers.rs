//! Delegation in the TUI: the worker strip above the prompt (one row per active or recent worker),
//! the transcript viewer overlay, the protocol confirm prompt, and the `/workers` and `/delegate`
//! commands. The logic (roles, jobs, cancellation, results) lives in `crate::delegate`.
//!
//! Hooks in `tui/mod.rs` are deliberately tiny: a `Panel` field, `carve`/`draw` in `draw_ui`,
//! `handle_key`/`handle_mouse` at the top of the input handlers, and `poll_delegates`.

use super::text::char_width;
use super::{contains, provider_color, App, Phase};
use crate::accounts::{get_account, is_logged_in, normalize_account_id};
use crate::delegate::{
    build_worker_context, extract_delegate_blocks, fmt_elapsed, result_block, use_prompt,
    worker_trust, Delegates, Job, JobSpec, Origin, Submit,
    WorkerState, MAX_PENDING_CONFIRMS,
};
use crate::provider::stream_cards::format_card_line;
use crate::store::{get_settings, ChildTrust, TrustMode};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Rows the strip may use, "+N more" included.
const MAX_STRIP_ROWS: usize = 4;
/// A finished worker stays in the strip this long (its result stays available regardless).
const RECENT: Duration = Duration::from_secs(90);

#[derive(Default)]
pub(super) struct Panel {
    pub d: Delegates,
    /// The focused worker row (Alt+Up/Down or a click). Esc cancels it; Enter opens it.
    pub selected: Option<u32>,
    viewer: Option<Viewer>,
    pub(super) input: Option<super::worker_input::Review>,
    confirms: VecDeque<Confirm>,
    /// Screen rows from the last draw, for clicks.
    rects: Vec<(Rect, u32)>,
    /// A prompt for the orchestrator produced by `/delegate use`, sent by `flush_delegate_prompt`.
    outbox: Option<(String, Vec<u32>)>,
    generation: u64,
    owners: std::collections::BTreeMap<u32, u64>,
    auto_return_paused: bool,
    recovery_notice: bool,
    recoveries: Vec<RecoveryRead>,
    delivery_read: Option<DeliveryRead>,
}

struct DeliveryRead {
    job: u32,
    generation: u64,
    request: crate::delegate::delivery::Request,
    mode: Option<crate::delegate::delivery::Mode>,
    expected: Option<String>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    rx: std::sync::mpsc::Receiver<Result<crate::delegate::delivery::Review, String>>,
}
impl Drop for DeliveryRead {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

struct RecoveryRead {
    job: u32,
    generation: u64,
    rx: std::sync::mpsc::Receiver<Result<String, String>>,
}

struct Viewer {
    job: u32,
    scroll: usize,
    /// Largest useful scroll, learned at draw time.
    max_scroll: usize,
}

impl Panel {
    pub(super) fn change_conversation(&mut self) {
        if let Some(read) = &self.delivery_read {
            read.cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.input = None;
        self.recovery_notice = false;
        self.d.new_conversation();
        self.generation = self.generation.wrapping_add(1);
        self.outbox = None;
        self.auto_return_paused = false;
        self.confirms.clear();
    }

    fn owns(&self, id: u32) -> bool {
        self.owners.get(&id) == Some(&self.generation)
    }
    pub(super) fn has_recovery_notice(&self) -> bool {
        self.recovery_notice
    }

    pub(super) fn has_pending_prompt(&self) -> bool {
        self.outbox.is_some()
    }
}

struct Confirm {
    to: String,
    task: String,
    /// When it became the front prompt. A bare y/n only answers after `ARM_AFTER`, so type-ahead
    /// that happens to start with y or n can never approve or decline by accident.
    since: Instant,
    goal: Option<()>,
    goal_armed: std::cell::Cell<bool>,
    goal_visible: std::cell::Cell<bool>,
    goal_draft_revision: std::cell::Cell<u64>,
}

/// Ticks (about 200 ms each) between looks at `<state>/proposals`: about 2 s.
const PROPOSAL_POLL_TICKS: u64 = 10;

const ARM_AFTER: Duration = Duration::from_millis(1500);

fn phase_of(state: WorkerState) -> Option<Phase> {
    match state {
        WorkerState::Starting => Some(Phase::Waiting),
        WorkerState::Thinking => Some(Phase::Thinking),
        WorkerState::Tool => Some(Phase::Tool(String::new())),
        WorkerState::Writing => Some(Phase::Writing),
        _ => None,
    }
}

/// Spinner (width-1 glyph) and colour for a state. Live states reuse the main wait's per-phase
/// animation and colour; the rest are static marks.
fn icon_of(state: WorkerState, tick: u64) -> (&'static str, Color) {
    if let Some(p) = phase_of(state) {
        return (p.frame(tick), p.color());
    }
    match state {
        WorkerState::Queued => ("\u{25cc}", Color::DarkGray),
        WorkerState::WaitingInput => ("?", Color::Yellow),
        WorkerState::Done => ("\u{2713}", Color::Green),
        WorkerState::Failed => ("\u{2717}", Color::Red),
        _ => ("-", Color::DarkGray),
    }
}

fn truncate(s: &str, width: usize) -> String {
    let total: usize = s.chars().map(char_width).sum();
    if total <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = char_width(c);
        if used + w + 1 > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('\u{2026}');
    out
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Rows the strip takes for `n` visible workers: none when empty, at most four with "+N more".
pub(super) fn strip_rows(n: usize) -> usize {
    n.min(MAX_STRIP_ROWS)
}

/// Reserve the strip at the bottom of the transcript area; returns its rectangle (empty when no
/// worker is shown). Also drops a focus on a worker that is no longer listed.
pub(super) fn carve(transcript: &mut Rect, app: &mut App) -> Rect {
    let now = Instant::now();
    let ids: Vec<u32> = app
        .deleg
        .d
        .visible(now, RECENT)
        .iter()
        .map(|j| j.id)
        .collect();
    if let Some(s) = app.deleg.selected {
        if !ids.contains(&s) {
            app.deleg.selected = None;
        }
    }
    let rows = strip_rows(ids.len()).min(transcript.height.saturating_sub(3) as usize) as u16;
    transcript.height -= rows;
    Rect {
        x: transcript.x,
        y: transcript.y + transcript.height,
        width: transcript.width,
        height: rows,
    }
}

fn row_line(
    j: &Job,
    label: String,
    selected: bool,
    width: usize,
    tick: u64,
    now: Instant,
) -> Line<'static> {
    let (icon, color) = icon_of(j.state, tick);
    let base = if selected {
        Style::default()
            .bg(Color::Indexed(238))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let badge = if width >= 90 {
        format!(
            "{} ({}/{})",
            label,
            j.provider.as_str(),
            j.model.as_deref().unwrap_or("default")
        )
    } else {
        label
    };
    let secs = if j.state == WorkerState::Queued {
        "queued".to_string()
    } else {
        fmt_elapsed(j.elapsed(now))
    };
    let state = match j.state {
        WorkerState::Tool => "tool",
        s => s.label(),
    };
    let head = format!("{} {icon} ", if selected { "\u{25b8}" } else { " " });
    let mid = format!(" {secs} \u{b7} {state}");
    let fixed: usize = head
        .chars()
        .chain(badge.chars())
        .chain(mid.chars())
        .map(char_width)
        .sum();
    let snippet = one_line(&j.snippet);
    let room = width.saturating_sub(fixed + 3);
    let mut spans = vec![
        Span::styled(head, base.fg(color)),
        Span::styled(
            badge,
            base.fg(provider_color(j.provider))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(mid, base.fg(color)),
    ];
    if !snippet.is_empty() && room >= 4 {
        spans.push(Span::styled(
            format!(" \u{b7} {}", truncate(&snippet, room)),
            base.fg(Color::Gray),
        ));
    }
    Line::from(spans).style(base)
}

/// Draw the strip into `strip` and the overlays (viewer, confirm) over `over`.
pub(super) fn draw(f: &mut Frame, strip: Rect, over: Rect, app: &mut App) {
    app.deleg.rects.clear();
    let now = Instant::now();
    let tick = app.tick_count;
    if strip.height > 0 {
        let visible = app.deleg.d.visible(now, RECENT);
        let n = visible.len();
        let more_row = n > strip.height as usize;
        let shown = strip.height as usize - usize::from(more_row);
        let sel_idx = app
            .deleg
            .selected
            .and_then(|s| visible.iter().position(|j| j.id == s));
        let first = match sel_idx {
            Some(i) if i >= shown => i + 1 - shown,
            _ => 0,
        };
        let mut lines = Vec::new();
        for (row, j) in visible.iter().skip(first).take(shown).enumerate() {
            lines.push(row_line(
                j,
                app.named_account(j.provider, &j.account),
                app.deleg.selected == Some(j.id),
                strip.width as usize,
                tick,
                now,
            ));
            app.deleg.rects.push((
                Rect {
                    x: strip.x,
                    y: strip.y + row as u16,
                    width: strip.width,
                    height: 1,
                },
                j.id,
            ));
        }
        if more_row {
            lines.push(Line::from(Span::styled(
                format!(
                    "   +{} more \u{b7} Alt+\u{2191}\u{2193} to select",
                    n - shown
                ),
                Style::default().fg(Color::DarkGray),
            )));
        }
        f.render_widget(Paragraph::new(lines), strip);
    }
    if app.deleg.viewer.is_some() {
        draw_viewer(f, over, app, now);
    } else if let Some(c) = app.deleg.confirms.front() {
        draw_confirm(f, over, app, c);
    }
}

fn draw_confirm(f: &mut Frame, over: Rect, app: &App, c: &Confirm) {
    if let Some(goal) = &c.goal {
        c.goal_visible.set(false);
        if app.picker.is_some()
            || app.workspace.focused.is_some()
            || app.activity.detail.is_some()
            || app.questions.focused
            || app.deleg.input.is_some()
        {
            c.goal_armed.set(false);
            return;
        }
        if over.height < 3 || over.width < 20 {
            c.goal_armed.set(false);
            return;
        }
        let prompt = if c.goal_armed.get() {
            "Goal review focused: Y applies · N/Esc declines (message draft preserved)"
        } else {
            "F7 focuses goal review; your message draft is preserved."
        };
        let rows = wrap_plain(
            &format!("{prompt}"),
            over.width.saturating_sub(4) as usize,
        );
        let full = rows.len().saturating_add(2) <= over.height as usize;
        c.goal_visible.set(full);
        if !full {
            c.goal_armed.set(false);
        }
        let lines = if full {
            rows.into_iter().map(Line::from).collect::<Vec<_>>()
        } else {
            vec![Line::from("Goal proposal waiting. Enlarge the terminal to review the full text; approval is disabled.")]
        };
        let height = (lines.len() as u16 + 2).min(over.height);
        let area = Rect {
            x: over.x,
            y: over.y + over.height - height,
            width: over.width,
            height,
        };
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow))
                    .title(" goal proposal · requester unverified "),
            ),
            area,
        );
        return;
    }
    if over.height < 5 || over.width < 20 {
        return;
    }
    let area = Rect {
        x: over.x,
        y: over.y + over.height - 3,
        width: over.width,
        height: 3,
    };
    let label = get_account(&app.state_root, &c.to)
        .map(|a| crate::accounts::display_account_name(a.provider, &a.account))
        .unwrap_or_else(|| c.to.clone());
    let pending = app.deleg.confirms.len();
    let tail = if pending > 1 {
        format!(" [y/n] (+{} more)", pending - 1)
    } else {
        " [y/n]".to_string()
    };
    let head = format!("Orchestrator wants to delegate to {label}: ");
    let room =
        (area.width as usize).saturating_sub(2 + head.chars().count() + tail.chars().count());
    let hint = if app.editor.text().is_empty() {
        ""
    } else {
        " (clear the prompt to answer)"
    };
    let line = Line::from(vec![
        Span::styled(head, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(truncate(&one_line(&c.task), room)),
        Span::styled(
            tail,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(hint, Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" delegation requested "),
        ),
        area,
    );
}

pub(super) fn wrap_plain(s: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for raw in s.lines() {
        let mut cur = String::new();
        let mut used = 0;
        for c in raw.chars() {
            let w = char_width(c);
            if used + w > width {
                out.push(std::mem::take(&mut cur));
                used = 0;
            }
            cur.push(c);
            used += w;
        }
        out.push(cur);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// What the viewer shows for a worker: header facts, tool cards, then the untrusted output.
pub(super) fn viewer_text(j: &Job) -> String {
    let identity = format!(
        "run: {} · conversation: {}\nsource: {}\n",
        j.run_id,
        j.conversation_id,
        j.source_id.as_deref().unwrap_or("unavailable")
    );
    let mut t = String::new();
    t.push_str(&format!(
        "task: {}\nresult: {}\n",
        one_line(&j.task),
        if j.collected {
            "sent to main agent"
        } else {
            "available to inspect"
        }
    ));
    t.push_str("\noutput (untrusted data - shown, never executed):\n");
    let out = j.final_text();
    t.push_str(if out.is_empty() { "(none yet)" } else { &out });
    t.push_str("\n\nDetails:\n");
    t.push_str(&identity);
    t.push_str(&j.usage_summary());
    t.push('\n');
    t.push_str(&j.routing_summary());
    t.push('\n');
    t.push_str(&j.workspace_summary());
    t.push('\n');
    if j.snippet.starts_with("integration_required:") {
        t.push_str(&format!(
            "Dependency: {}\nD reviews cross-worktree delivery without consuming the main draft.\n",
            j.snippet
        ));
    }
    t.push_str(&format!("task: {}\n", one_line(&j.task)));
    t.push_str(&format!(
        "model: {} | result: {}\n",
        j.model.as_deref().unwrap_or("provider default"),
        if j.collected {
            "sent to main agent"
        } else {
            "not sent"
        }
    ));
    t.push_str(&format!(
        "worker: {} ({}) \u{b7} trust {} \u{b7} session {}\n",
        j.label(),
        j.provider.as_str(),
        j.trust.as_str(),
        j.session_id.as_deref().unwrap_or("-")
    ));
    t.push_str(&format!("\ntool cards ({}):\n", j.cards.len()));
    for c in &j.cards {
        t.push_str(&format_card_line(c));
        t.push('\n');
    }
    if let Some(e) = &j.error {
        t.push_str(&format!("\n\nerror:\n{e}"));
    }
    t
}

fn draw_viewer(f: &mut Frame, over: Rect, app: &mut App, now: Instant) {
    let over = super::native_agents::panel_area(over);
    let Some(v) = app.deleg.viewer.as_ref() else {
        return;
    };
    let Some(j) = app.deleg.d.get(v.job) else {
        app.deleg.viewer = None;
        return;
    };
    let title = format!(
        " worker #{} \u{b7} {} ({}) \u{b7} {} \u{b7} {} ",
        j.id,
        j.label(),
        j.provider.as_str(),
        j.state.label(),
        fmt_elapsed(j.elapsed(now))
    );
    let body = wrap_plain(&viewer_text(j), over.width.saturating_sub(4) as usize);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(title)
        .title_bottom(
            " Esc close \u{b7} D delivery review \u{b7} \u{2191}\u{2193} PgUp PgDn scroll ",
        );
    let inner = block.inner(over);
    let max = body.len().saturating_sub(inner.height as usize);
    let scroll = {
        let v = app.deleg.viewer.as_mut().expect("checked above");
        v.max_scroll = max;
        v.scroll = v.scroll.min(max);
        v.scroll
    };
    let lines: Vec<Line> = body
        .into_iter()
        .skip(scroll)
        .take(inner.height as usize)
        .map(|l| Line::from(format!(" {l}")))
        .collect();
    f.render_widget(Clear, over);
    f.render_widget(Paragraph::new(lines).block(block), over);
}

/// Keys the worker UI owns. Returns true when the key was consumed.
pub(super) fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let goal_focus = app.workspace.focused.is_none()
        && app.picker.is_none()
        && app.activity.detail.is_none()
        && !app.questions.focused
        && app.deleg.viewer.is_none()
        && app.queue.edit.is_none()
        && !app.queue.focused
        && !super::landing::editing_goal(app);
    if goal_focus && !ctrl && !alt && key.code == KeyCode::F(7) {
        if let Some(confirm) = app
            .deleg
            .confirms
            .front()
            .filter(|confirm| confirm.goal.is_some())
        {
            if confirm.goal_visible.get() {
                confirm.goal_armed.set(!confirm.goal_armed.get());
                confirm.goal_draft_revision.set(app.editor.revision());
            } else {
                app.flash("The entire goal proposal must be visible before approval");
            }
            return true;
        }
    }
    if let Some(v) = app.deleg.viewer.as_mut() {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => app.deleg.viewer = None,
            KeyCode::Char('c') if ctrl => app.deleg.viewer = None,
            KeyCode::Char('d' | 'D') if !ctrl && !alt => {
                let id = v.job;
                app.deleg.viewer = None;
                app.dependency_delivery_command(&id.to_string());
            }
            KeyCode::Up => v.scroll = v.scroll.saturating_sub(1),
            KeyCode::Down => v.scroll = (v.scroll + 1).min(v.max_scroll),
            KeyCode::PageUp => v.scroll = v.scroll.saturating_sub(10),
            KeyCode::PageDown => v.scroll = (v.scroll + 10).min(v.max_scroll),
            KeyCode::Home => v.scroll = 0,
            KeyCode::End => v.scroll = v.max_scroll,
            _ => {}
        }
        return true;
    }
    // Only chat focus may answer; read-only file typing must never approve or reject work.
    let armed = app.deleg.confirms.front().is_some_and(|c| {
        if c.goal.is_some() {
            c.goal_armed.get()
                && c.goal_visible.get()
                && c.goal_draft_revision.get() == app.editor.revision()
        } else {
            c.since.elapsed() >= ARM_AFTER
        }
    });
    let goal_answer = app
        .deleg
        .confirms
        .front()
        .is_some_and(|confirm| confirm.goal.is_some());
    if armed && goal_focus && (app.editor.text().is_empty() || goal_answer) && !ctrl && !alt {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.answer_confirm(true);
                return true;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.answer_confirm(false);
                return true;
            }
            KeyCode::Enter if goal_answer => {
                app.flash("Goal review: Y applies, N declines, F7 returns to your message");
                return true;
            }
            _ => {
                if goal_answer {
                    if let Some(confirm) = app.deleg.confirms.front() {
                        confirm.goal_armed.set(false);
                    }
                }
            }
        }
    }
    if alt && matches!(key.code, KeyCode::Up | KeyCode::Down) {
        app.select_worker(if key.code == KeyCode::Down { 1 } else { -1 });
        return true;
    }
    let plain = key.modifiers.is_empty() && app.workspace.focused.is_none();
    if plain && key.code == KeyCode::Enter && app.editor.text().is_empty() {
        if let Some(id) = app.deleg.selected {
            app.deleg.viewer = Some(Viewer {
                job: id,
                scroll: 0,
                max_scroll: 0,
            });
            return true;
        }
    }
    if plain && key.code == KeyCode::Esc {
        if let Some(id) = app.deleg.selected {
            let running = app.deleg.d.get(id).is_some_and(|j| !j.state.is_finished());
            if running {
                app.deleg.d.cancel(id, Instant::now());
                app.flash("cancelling worker (Esc); /delegate stop <account> also works");
            } else {
                app.deleg.selected = None;
            }
            return true;
        }
    }
    false
}

pub(super) fn handle_mouse(app: &mut App, m: &MouseEvent) -> bool {
    if let Some(v) = app.deleg.viewer.as_mut() {
        match m.kind {
            MouseEventKind::ScrollUp => v.scroll = v.scroll.saturating_sub(3),
            MouseEventKind::ScrollDown => v.scroll = (v.scroll + 3).min(v.max_scroll),
            _ => {}
        }
        return true;
    }
    if m.kind == MouseEventKind::Down(MouseButton::Left) {
        let hit = app
            .deleg
            .rects
            .iter()
            .find(|(r, _)| contains(*r, m.column, m.row))
            .map(|(_, id)| *id);
        if let Some(id) = hit {
            app.deleg.selected = Some(id);
            return true;
        }
    }
    false
}

impl App {
    fn dependency_delivery_command(&mut self, arguments: &str) {
        use crate::delegate::delivery::Mode;
        if arguments.trim() == "cancel" {
            if let Some(read) = &self.deleg.delivery_read {
                read.cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            self.push_system("Dependency review cancelled; no delivery acceptance or Git integration was performed");
            return;
        }
        if self.deleg.delivery_read.is_some() {
            self.push_system(
                "A dependency review is already running; /delegate delivery cancel interrupts it",
            );
            return;
        }
        let parts: Vec<_> = arguments.split_whitespace().collect();
        if !matches!(parts.as_slice(), [_] | [_, "result-only" | "integrated", _]) {
            self.push_system("usage: /delegate delivery JOB | /delegate delivery JOB result-only|integrated RECEIPT | /delegate delivery cancel. Review never integrates changes or launches a new task.");
            return;
        }
        let parsed = parts[0]
            .parse::<u32>()
            .map_err(|_| "invalid downstream job id".to_string())
            .and_then(|job| {
                let request = self.deleg.d.delivery_request(job)?;
                let mode = parts.get(1).map(|mode| Mode::parse(mode)).transpose()?;
                let expected = parts.get(2).map(|receipt| receipt.to_string());
                if expected.as_ref().is_some_and(|receipt| {
                    receipt.len() != 64 || !receipt.bytes().all(|byte| byte.is_ascii_hexdigit())
                }) {
                    return Err("acceptance needs the exact preview receipt".into());
                }
                Ok((job, request, mode, expected))
            });
        let (job, request, mode, expected) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                self.push_system(error);
                return;
            }
        };
        let root = self.state_root.clone();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = cancel.clone();
        let input = request.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        if std::thread::Builder::new()
            .name("concordia-dependency-review".into())
            .spawn(move || {
                let result = crate::delegate::delivery::review(&root, &input, &flag);
                let _ = tx.send(result);
            })
            .is_err()
        {
            self.push_system("Dependency review could not start; nothing accepted");
            return;
        }
        self.deleg.delivery_read = Some(DeliveryRead {
            job,
            generation: self.deleg.generation,
            request,
            mode,
            expected,
            cancel,
            rx,
        });
        self.push_system("Inspecting exact prerequisite sources and managed heads; no Git integration is running");
    }

    fn poll_dependency_delivery(&mut self) -> bool {
        let Some(read) = self.deleg.delivery_read.as_ref() else {
            return false;
        };
        if read.generation != self.deleg.generation {
            read.cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let result = match read.rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(_) => Err("Dependency reviewer stopped; inspect again".into()),
        };
        let Some(read) = self.deleg.delivery_read.take() else {
            return false;
        };
        if read.generation != self.deleg.generation
            || read.cancel.load(std::sync::atomic::Ordering::Relaxed)
        {
            return false;
        }
        let result = result.and_then(|review| {
            if self.deleg.d.delivery_request(read.job)? != read.request || review.request != read.request { return Err("Dependency ownership changed during review; nothing accepted".into()); }
            match read.mode {
                None => review.describe(read.job),
                Some(mode) => {
                    let receipt = review.receipt(mode)?;
                    if read.expected.as_deref() != Some(receipt.hash.as_str()) { return Err("Dependency review changed; obtain a fresh receipt before acceptance".into()); }
                    let hash = receipt.hash.clone();
                    self.deleg.d.accept_delivery(read.job, receipt)?;
                    Ok(format!("Dependency delivery accepted for #{} as {} · receipt {}. Dispatch still verifies sources, repository state, routing and native permissions.", read.job, mode.label(), hash))
                }
            }
        });
        self.push_system(result.unwrap_or_else(|error| format!("Dependency delivery: {error}")));
        true
    }

    pub(super) fn restore_workers(&mut self) {
        let Some(lease) = self.keep.lease.as_ref() else {
            return;
        };
        match self
            .deleg
            .d
            .attach_recovery(&lease.workers_directory(), &self.cwd, &self.state_root)
        {
            Ok(n) => {
                self.deleg.recovery_notice = n > 0;
                self.deleg.owners.clear();
                self.deleg.auto_return_paused = n > 0;
                if n > 0 {
                    self.push_system(format!("Recovered {n} worker run records. All automatic returns are paused. Ctrl+G inspects runs; /delegate recover ID reads a completed result from its exact source. Reconcile uncertain work before /delegate retry ID reconciled."));
                }
            }
            Err(e) => self.push_system(format!("worker recovery unavailable: {e}")),
        }
    }

    pub(super) fn worker_recovery_command(&mut self, argument: &str) {
        let args: Vec<_> = argument.split_whitespace().collect();
        let result: Result<String,String> = match args.as_slice() {
            ["fallback", mode @ ("on" | "off")] => crate::store::modify_store(&self.state_root, |s| { s.settings.delegate_quota_failover = *mode == "on"; Ok(()) }).map(|_| format!("Worker quota failover {mode}. Only confirmed quota failures before tools or partial output can move to the next configured worker; at most four attempts. Uncertain/crash recovery stays paused.")).map_err(|e| e.to_string()),
            ["retry", id, "reconciled"] => id.parse::<u32>().map_err(|_| "invalid run id".to_string()).and_then(|id| {
                let (account, task) = self.deleg.d.recovered_task(id)?;
                let job = self.deleg.d.get(id).ok_or("unknown worker run")?;
                let declarations: Vec<_> = job.workspace.as_ref().map(|request| request.claims.iter().map(|claim| crate::workspaces::Declaration { path: claim.relative.clone(), access: claim.access }).collect()).unwrap_or_default();
                let managed = job.workspace.as_ref().and_then(|request| request.binding.managed.as_ref()).map(|managed| managed.id.clone());
                self.start_delegate_workspace(&account, &task, Origin::User, &[], &declarations, managed.as_deref())
            }),
            ["recover", id] => {
                let request = id.parse::<u32>().map_err(|_| "invalid run id".to_string()).and_then(|id| self.deleg.d.recovery_source(id).map(|(root,source)| (id,root,source)));
                request.and_then(|(id,root,source)| {
                    if self.deleg.recoveries.len() >= 2 { return Err("worker source readers are busy".into()); }
                    let (tx,rx) = std::sync::mpsc::channel();
                    std::thread::Builder::new().name("worker-result-read".into()).spawn(move || { let _ = tx.send(crate::native_memory::assistant_text(&root,&source)); }).map_err(|e| e.to_string())?;
                    self.deleg.recoveries.push(RecoveryRead { job:id, generation:self.deleg.generation, rx });
                    Ok(format!("Reading exact native result for worker #{id}; no provider task is started"))
                })
            }
            _ => Err("/delegate fallback on|off · /delegate recover ID · /delegate retry ID reconciled (inspect uncertain actions first)".into()),
        };
        self.push_system(result.unwrap_or_else(|e| format!("worker recovery: {e}")));
    }
    pub(super) fn inspect_worker(&mut self, id: u32) {
        if self
            .deleg
            .d
            .get(id)
            .is_some_and(|job| !job.native_inputs().is_empty())
        {
            super::worker_input::open(self, id);
            return;
        }
        if self.deleg.d.get(id).is_some() {
            self.deleg.viewer = Some(Viewer {
                job: id,
                scroll: 0,
                max_scroll: 0,
            });
        }
    }

    fn select_worker(&mut self, delta: i32) {
        let ids: Vec<u32> = self
            .deleg
            .d
            .visible(Instant::now(), RECENT)
            .iter()
            .map(|j| j.id)
            .collect();
        if ids.is_empty() {
            self.deleg.selected = None;
            return;
        }
        let cur = self
            .deleg
            .selected
            .and_then(|s| ids.iter().position(|i| *i == s));
        // Past either end the focus is released, so there is always a way back to the prompt.
        let next = match cur {
            None if delta > 0 => Some(0),
            None => Some(ids.len() - 1),
            Some(i) => {
                let n = i as i32 + delta;
                (n >= 0 && (n as usize) < ids.len()).then_some(n as usize)
            }
        };
        self.deleg.selected = next.map(|i| ids[i]);
    }

    /// Apply worker events and post the result of each worker that finished.
    pub(super) fn poll_delegates(&mut self) -> bool {
        let mut changed = self.deleg.d.any_active();
        changed |= self.poll_dependency_delivery();
        changed |= self.poll_managed_workspaces();
        let now = Instant::now();
        let finished = self.deleg.d.poll(now);
        if let Some(error) = self.deleg.d.recovery_error() {
            self.flash(format!(
                "worker storage: {error}; launches/returns require successful recovery storage"
            ));
        }
        let mut i = 0;
        while i < self.deleg.recoveries.len() {
            let result = match self.deleg.recoveries[i].rx.try_recv() {
                Ok(r) => r,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    i += 1;
                    continue;
                }
                Err(_) => Err("worker result reader stopped".into()),
            };
            changed = true;
            let RecoveryRead {
                job: id,
                generation,
                ..
            } = self.deleg.recoveries.swap_remove(i);
            if generation != self.deleg.generation {
                continue;
            }
            match result {
                Ok(text) => {
                    let restored = self.deleg.d.restore_result(id, text.clone()).is_ok();
                    self.push_system(format!("Worker #{id} native source:\n{text}\n{}", if restored { "Completed result available for explicit /delegate use. Automatic return remains paused." } else { "Run completion is uncertain. Inspect actions before an explicit retry." }));
                }
                Err(e) => self.push_system(format!("worker #{id} source unavailable: {e}")),
            }
        }
        self.deleg
            .owners
            .retain(|id, _| self.deleg.d.get(*id).is_some());
        changed |= !finished.is_empty();
        for id in finished {
            if let Some(job) = self.deleg.d.get(id) {
                self.usage_fetcher.invalidate(&job.account);
                for account in job.attempt_accounts() {
                    self.usage_fetcher.invalidate(account);
                }
            }
            if !self.deleg.owns(id) {
                self.flash(format!(
                    "worker #{id} finished for an earlier conversation; Ctrl+G inspects it"
                ));
                continue;
            }
            let Some(j) = self.deleg.d.get(id) else {
                continue;
            };
            crate::mcp::update_matching(
                &self.state_root,
                &j.account,
                &j.task,
                if j.state == WorkerState::Done { "finished" } else { "failed" },
                Some(j.final_text()),
            );
            crate::logging::info(&format!(
                "delegate-finish id={} worker={} state={} ms={} cards={} origin={:?}",
                j.id,
                j.account,
                j.state.label(),
                j.elapsed(now).as_millis(),
                j.tool_cards(),
                j.origin
            ));
            let block = result_block(j, &self.account_label());
            let note = format!("{} {}", j.label(), j.state.label());
            self.push_system(block);
            self.flash(format!("worker {note}"));
        }
        self.refresh_agents_picker();
        if !self.busy
            && !self.deleg.auto_return_paused
            && !self.deleg.d.any_active()
            && self.deleg.outbox.is_none()
            && self.picker.is_none()
            && self.job.is_none()
            && !self.session_ui.loading()
            && !self.continuation.waiting()
            && self.queue.items.is_empty()
            && self.queue.edit.is_none()
            && self
                .deleg
                .d
                .jobs
                .iter()
                .any(|j| self.deleg.owns(j.id) && j.state == WorkerState::Done && !j.collected)
            && get_settings(&self.state_root).delegate_auto_return
        {
            self.collect_results(false);
            changed = true;
        }
        changed
    }

    pub(super) fn cancel_all_delegates(&mut self) {
        self.deleg.d.cancel_all(Instant::now());
    }

    /// Send the prompt `/delegate use` prepared (a user action) to the orchestrator.
    pub(super) fn flush_delegate_prompt(&mut self, turn_tx: &mpsc::UnboundedSender<String>) {
        if self.update_running()
            || self.busy
            || self.session_ui.loading()
            || turn_tx.is_closed()
            || self.images.importing()
        {
            return;
        }
        let Some((p, ids)) = self.deleg.outbox.take() else {
            return;
        };
        if let Err(error) = self.deleg.d.reserve_delivery(&ids) {
            self.deleg.auto_return_paused = true;
            self.push_system(format!(
                "worker result was not dispatched: could not reserve durable delivery: {error}"
            ));
            return;
        }
        let draft = self.composer_value();
        self.cancel_requested = false;
        // Normal dispatch owns checkpoints, context, timing, quota recovery and images.
        self.run_line(p.clone(), turn_tx);
        self.restore_composer(&draft);
        if self.busy {
            self.deleg.d.mark_collected(&ids);
        } else {
            self.deleg.auto_return_paused = true;
            self.flash(
                "worker return paused; fix recovery storage, then /delegate collect to retry",
            );
        }
    }

    /// Queue protocol proposals found in an orchestrator reply. Off unless the user opted in.
    /// Only ever called with the orchestrator's own reply, never with a worker's output.
    pub(super) fn scan_delegation(&mut self, reply: &str) {
        if !crate::store::delegation_protocol_on(&self.state_root) || reply.is_empty() {
            return;
        }
        self.queue_proposals(extract_delegate_blocks(reply));
    }

    /// One bounded proposal queue. Worker proposals require delegation mode; goal proposals
    /// always require complete local review and explicit F7 focus before any goal mutation.
    pub(super) fn poll_proposals(&mut self) -> bool {
        if self.picker.is_some()
            || self.workspace.focused.is_some()
            || self.activity.detail.is_some()
            || self.questions.focused
            || self.deleg.viewer.is_some()
            || self.deleg.input.is_some()
            || self.queue.focused
            || self.queue.edit.is_some()
            || super::landing::editing_goal(self)
        {
            for confirm in &self.deleg.confirms {
                if confirm.goal.is_some() {
                    confirm.goal_armed.set(false);
                    confirm.goal_visible.set(false);
                }
            }
        }
        for confirm in &self.deleg.confirms {
            if confirm.goal.is_some() && confirm.goal_draft_revision.get() != self.editor.revision()
            {
                confirm.goal_armed.set(false);
            }
        }
        if !self.tick_count.is_multiple_of(PROPOSAL_POLL_TICKS)
            || !crate::mcp::has_proposals(&self.state_root)
            || super::landing::editing_goal(self)
            || self.picker.is_some()
            || self.workspace.focused.is_some()
        {
            return false;
        }
        let slots = MAX_PENDING_CONFIRMS.saturating_sub(self.deleg.confirms.len());
        let mut changed = false;
        for proposal in crate::mcp::take_pending(&self.state_root, slots) {
            changed = true;
            match proposal {
                crate::mcp::PendingProposal::Delegate(proposal) => {
                    crate::mcp::update_agent(&self.state_root, &proposal.agent_id, "running", None, "accepted by Concordia");
                    if let Some(block) = crate::delegate::block_from(&proposal.to, &proposal.task) {
                        match self.start_delegate(&block.to, &block.task, Origin::Protocol) {
                            Ok(message) => self.push_system(message),
                            Err(error) => self.push_system(format!("delegate failed - {error}")),
                        }
                    }
                }
            }
        }
        changed
    }

    fn queue_proposals(&mut self, blocks: Vec<crate::delegate::DelegateBlock>) {
        let orch = normalize_account_id(&self.account);
        for b in blocks {
            let is_connected = crate::accounts::get_account(&self.state_root, &b.to).is_some();
            if !is_connected || b.to == orch {
                self.push_system(format!(
                    "delegation to {} ignored: account is not connected or is the parent account",
                    b.to
                ));
                continue;
            }
            if self.deleg.confirms.len() >= MAX_PENDING_CONFIRMS {
                self.push_system("too many delegation proposals pending - ignored the rest");
                break;
            }
            crate::logging::info(&format!(
                "delegate-proposed to={} task_chars={}",
                b.to,
                b.task.chars().count()
            ));
            self.deleg.confirms.push_back(Confirm {
                to: b.to,
                task: b.task,
                since: Instant::now(),
                goal: None,
                goal_armed: std::cell::Cell::new(false),
                goal_visible: std::cell::Cell::new(false),
                goal_draft_revision: std::cell::Cell::new(0),
            });
        }
    }

    fn answer_confirm(&mut self, yes: bool) {
        let Some(c) = self.deleg.confirms.pop_front() else {
            return;
        };
        if let Some(next) = self.deleg.confirms.front_mut() {
            next.since = Instant::now();
            next.goal_armed.set(false);
            next.goal_visible.set(false);
        }
        crate::logging::info(&format!("delegate-answer to={} approved={yes}", c.to));
        if !yes {
            self.push_system(format!("declined delegation to {}", c.to));
            return;
        }
        match self.start_delegate(&c.to, &c.task, Origin::Protocol) {
            Ok(m) => self.push_system(m),
            Err(e) => self.push_system(format!("delegate failed - {e}")),
        }
    }

    fn start_delegate(
        &mut self,
        target: &str,
        task: &str,
        origin: Origin,
    ) -> Result<String, String> {
        self.start_delegate_after(target, task, origin, &[])
    }

    fn start_delegate_after(
        &mut self,
        target: &str,
        task: &str,
        origin: Origin,
        after: &[u32],
    ) -> Result<String, String> {
        self.start_delegate_scoped(target, task, origin, after, &[])
    }

    fn start_delegate_scoped(
        &mut self,
        target: &str,
        task: &str,
        origin: Origin,
        after: &[u32],
        declarations: &[crate::workspaces::Declaration],
    ) -> Result<String, String> {
        self.start_delegate_workspace(target, task, origin, after, declarations, None)
    }

    fn start_delegate_workspace(
        &mut self,
        target: &str,
        task: &str,
        origin: Origin,
        after: &[u32],
        declarations: &[crate::workspaces::Declaration],
        managed: Option<&str>,
    ) -> Result<String, String> {
        #[cfg(not(test))]
        if !self.deleg.d.recovery_ready() {
            return Err("worker recovery storage is unavailable; no task was dispatched".into());
        }
        if self.update_running() {
            return Err(
                "Workers are paused while Concordia updates. Try again after it restarts.".into(),
            );
        }
        let settings = get_settings(&self.state_root);
        let orch = normalize_account_id(&self.account);
        let manual = if target != "auto" {
            let t = normalize_account_id(target);
            if t == orch {
                return Err(format!(
                    "{t} is the parent account; choose another connected account"
                ));
            }
            if crate::accounts::get_account(&self.state_root, &t).is_none() {
                return Err(format!("{t} is not a connected account"));
            }
            Some(t)
        } else {
            None
        };
        let account = manual.unwrap_or_else(|| {
            crate::accounts::list_all_accounts(&self.state_root)
                .into_iter()
                .find(|account| {
                    normalize_account_id(&account.account) != orch
                        && account.home_path.as_deref().is_some_and(|home| {
                            is_logged_in(account.provider, home)
                        })
                })
                .map(|account| account.account)
                .unwrap_or_default()
        });
        if account.is_empty() {
            return Err("no other connected account is logged in".into());
        }
        let model = None;
        let acct = get_account(&self.state_root, &account).ok_or_else(|| {
            format!("account {account} no longer exists - /workers remove {account}")
        })?;
        let home = acct.home_path.clone();
        if !home
            .as_ref()
            .is_some_and(|h| is_logged_in(acct.provider, h))
        {
            return Err(format!(
                "{account} is not logged in - /use add {}",
                acct.provider.as_str()
            ));
        }
        let orch_home = self
            .home_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let (prefix, note) = build_worker_context(
            self.provider,
            orch_home.as_deref(),
            &acct,
            &self.pack_messages(),
        );
        let requested_trust = match settings.child_trust {
            ChildTrust::Inherit => self.trust,
            ChildTrust::Ask => TrustMode::Ask,
            ChildTrust::Workspace => TrustMode::Workspace,
            ChildTrust::Full => TrustMode::Full,
        };
        let trust = worker_trust(self.trust, settings.child_trust);
        let queued_behind = self.deleg.d.running_for(&account).map(|j| j.id);
        let native_input = crate::native_input::for_home(
            &self.state_root,
            acct.provider,
            &account,
            home.as_deref(),
        )?;
        let id = self.deleg.d.submit_workspace(
            Submit {
                account: account.clone(),
                provider: acct.provider,
                task: task.to_string(),
                origin,
                spec: JobSpec {
                    native_input,
                    model: model.flatten(),
                    home,
                    cwd: self.cwd.clone(),
                    prompt: task.to_string(),
                    prefix,
                    trust,
                },
            },
            Instant::now(),
            after,
            declarations,
            managed,
        )?;
        self.deleg.owners.insert(id, self.deleg.generation);
        crate::logging::info(&format!(
            "delegate-start id={id} worker={account} provider={} origin={origin:?} trust={} task_chars={}",
            acct.provider.as_str(),
            trust.as_str(),
            task.chars().count()
        ));
        let workspace_note = managed
            .map(|workspace| {
                format!(" · managed worktree {workspace}; native filesystem confinement unverified")
            })
            .unwrap_or_default();
        Ok(format!(
            "delegated #{id} to {} ({}) \u{b7} {} \u{b7} trust {}{} \u{b7} {note} \u{b7} /delegate stop {account} cancels{workspace_note}",
            crate::accounts::display_account_name(acct.provider, &account),
            acct.provider.as_str(),
            match queued_behind {
                Some(b) => format!("queued behind #{b}"),
                None if !after.is_empty() => format!("approved after {after:?}; /delegate graph shows readiness"),
                None => "running in the background".into(),
            },
            trust.as_str(),
            if trust != requested_trust {
                format!(" (your mode is {})", self.trust.as_str())
            } else if trust == TrustMode::Full {
                " (your current mode)".to_string()
            } else {
                String::new()
            },
        ))
    }

    /// Handle `/workers` and `/delegate`. Returns false for every other command.
    fn delegate_stop(&mut self, acct: Option<&str>) {
        let now = Instant::now();
        let n = match acct {
            Some(a) => self.deleg.d.cancel_account(&normalize_account_id(a), now),
            None => match self.deleg.selected {
                Some(id) => usize::from(self.deleg.d.cancel(id, now)),
                None => {
                    let active: Vec<u32> = self
                        .deleg
                        .d
                        .jobs
                        .iter()
                        .filter(|j| !j.state.is_finished())
                        .map(|j| j.id)
                        .collect();
                    if active.len() == 1 {
                        usize::from(self.deleg.d.cancel(active[0], now))
                    } else {
                        self.push_system(
                            "usage: /delegate stop <account> (or select a worker row first)",
                        );
                        return;
                    }
                }
            },
        };
        crate::logging::info(&format!("delegate-stop jobs={n}"));
        self.push_system(if n == 0 {
            "nothing to stop".to_string()
        } else {
            format!("stopping {n} worker task{}", if n == 1 { "" } else { "s" })
        });
    }

    /// User action: hand a finished result to the orchestrator as (labelled, bounded) context.
    fn delegate_use(&mut self, acct: Option<&str>) {
        if self.update_running() {
            self.push_system("Worker results cannot be sent while Concordia updates. Try again after it restarts.");
            return;
        }
        if self.session_ui.loading() {
            self.push_system("history is loading; use the worker result after it finishes");
            return;
        }
        if self.busy {
            self.push_system("the orchestrator is busy - wait for it (or Esc), then /delegate use");
            return;
        }
        let want = acct.map(normalize_account_id);
        let pick = |j: &&Job| {
            j.state == WorkerState::Done
                && !j.final_text().is_empty()
                && want.as_deref().is_none_or(|w| j.account == w)
        };
        let jobs = &self.deleg.d.jobs;
        let chosen = self
            .deleg
            .selected
            .and_then(|id| jobs.iter().find(|j| j.id == id))
            .filter(|j| want.is_none() && pick(j))
            .or_else(|| jobs.iter().rev().find(pick));
        match chosen {
            Some(j) => {
                crate::logging::info(&format!(
                    "delegate-use id={} chars={}",
                    j.id,
                    j.final_text().chars().count()
                ));
                self.deleg.outbox = Some((use_prompt(j), vec![j.id]));
            }
            None => {
                self.push_system("no finished worker result to send - /workers shows the last ones")
            }
        }
    }

    fn worker_model(&self, account: &str, model: Option<&str>) -> String {
        let account = normalize_account_id(account);
        if account.is_empty() {
            return "/delegate model ACCOUNT [MODEL|default]".into();
        }
        let Some(acct) = get_account(&self.state_root, &account) else {
            return "worker account is unavailable".into();
        };
        let models = acct
            .home_path
            .as_deref()
            .map(|h| crate::models::ids_at(&self.state_root, acct.provider, &account, h))
            .unwrap_or_default();
        let Some(model) = model else {
            return format!("{account}: {}\nNative models: {}\n/delegate model {account} MODEL | default (applies to new jobs)",
                "provider default",
                if models.is_empty() { "catalog unavailable".into() } else { models.join(", ") });
        };
        if model != "default" && !models.iter().any(|m| m == model) {
            return format!("model is not in {account}'s local catalog; /delegate model {account} lists available ids");
        }
        format!("{account} model {model} is available for this request; worker model selection is not persisted")
    }

    /// One main-agent turn for up to three newly completed results, preserving the composer.
    fn delegate_collect(&mut self) {
        if self.busy || self.update_running() || self.session_ui.loading() {
            self.push_system("wait for the current operation, then /delegate collect");
            return;
        }
        self.deleg.auto_return_paused = false;
        self.collect_results(true);
    }

    fn collect_results(&mut self, report_empty: bool) {
        let jobs: Vec<_> = self
            .deleg
            .d
            .jobs
            .iter()
            .filter(|j| {
                self.deleg.owns(j.id)
                    && j.state == WorkerState::Done
                    && !j.collected
                    && !j.final_text().is_empty()
            })
            .take(crate::delegate::MAX_COLLECT_RESULTS)
            .collect();
        if jobs.is_empty() {
            if report_empty {
                self.push_system(
                    "no new completed worker results; /delegate use can resend a selected result",
                );
            }
            return;
        }
        let prompt = jobs
            .iter()
            .map(|j| crate::delegate::use_prompt_capped(j, 3000))
            .collect::<Vec<_>>()
            .join("\n\n");
        self.deleg.outbox = Some((prompt, jobs.iter().map(|j| j.id).collect()));
    }
}

/// Parse the compact local crew spelling `ACCOUNT:MODEL`. Account-only entries use the provider
/// default model. The account id is deliberately opaque; only the final separator is meaningful.
pub(super) fn clickable(app: &App, x: u16, y: u16) -> bool {
    app.deleg.viewer.is_none() && app.deleg.rects.iter().any(|(r, _)| contains(*r, x, y))
}

pub(super) fn overlay_open(app: &App) -> bool {
    app.deleg.viewer.is_some() || !app.deleg.confirms.is_empty()
}
