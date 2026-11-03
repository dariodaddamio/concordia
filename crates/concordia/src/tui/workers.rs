//! Delegation in the TUI: the worker strip above the prompt (one row per active or recent worker),
//! the transcript viewer overlay, the protocol confirm prompt, and the `/workers` and `/delegate`
//! commands. The logic (roles, jobs, cancellation, results) lives in `crate::delegate`.
//!
//! Hooks in `tui/mod.rs` are deliberately tiny: a `Panel` field, `carve`/`draw` in `draw_ui`,
//! `handle_key`/`handle_mouse` at the top of the input handlers, and `poll_delegates`.

use super::text::char_width;
use super::{contains, provider_color, App, Phase};
use crate::accounts::{get_account, is_logged_in, normalize_account_id};
use crate::commands::SlashCommand;
use crate::delegate::{
    add_worker, build_worker_context, extract_delegate_blocks, fmt_elapsed, help_lines,
    remove_worker, result_block, use_prompt, worker_trust, Delegates, Job, JobSpec, Origin, Submit,
    WorkerState, MAX_PENDING_CONFIRMS,
};
use crate::provider::stream_cards::format_card_line;
use crate::store::{get_settings, set_delegation_protocol, TrustMode};
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
    route_read: Option<(
        u64,
        std::sync::mpsc::Receiver<Result<crate::routing::Decision, String>>,
    )>,
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

fn wrap_plain(s: &str, width: usize) -> Vec<String> {
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
    t.push_str(&identity);
    t.push_str(&j.usage_summary());
    t.push('\n');
    t.push_str(&j.routing_summary());
    t.push('\n');
    t.push_str(&j.workspace_summary());
    t.push('\n');
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
    t.push_str("\noutput (untrusted data - shown, never executed):\n");
    let out = j.final_text();
    t.push_str(if out.is_empty() { "(none yet)" } else { &out });
    if let Some(e) = &j.error {
        t.push_str(&format!("\n\nerror:\n{e}"));
    }
    t
}

fn draw_viewer(f: &mut Frame, over: Rect, app: &mut App, now: Instant) {
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
        .title_bottom(" Esc close \u{b7} \u{2191}\u{2193} PgUp PgDn scroll ");
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
    if let Some(v) = app.deleg.viewer.as_mut() {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => app.deleg.viewer = None,
            KeyCode::Char('c') if ctrl => app.deleg.viewer = None,
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
    let armed = app
        .deleg
        .confirms
        .front()
        .is_some_and(|c| c.since.elapsed() >= ARM_AFTER);
    if armed && app.workspace.focused.is_none() && app.editor.text().is_empty() && !ctrl && !alt {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.answer_confirm(true);
                return true;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.answer_confirm(false);
                return true;
            }
            _ => {}
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
    pub(super) fn worker_route_inputs(
        &self,
    ) -> Result<(Vec<crate::routing::Target>, crate::routing::Context), String> {
        let settings = get_settings(&self.state_root);
        let mut context = crate::routing::Context::new(
            crate::routing::Purpose::Worker,
            &self.account,
            self.provider,
            self.home_path.clone(),
            Some(self.model.clone()),
        );
        context.workers = Some(
            settings
                .workers
                .iter()
                .map(|account| normalize_account_id(account))
                .collect(),
        );
        let targets = crate::routing::worker_targets(&self.state_root)?;
        for account in &settings.workers {
            if self.deleg.d.running_for(account).is_some() {
                context.running.insert(account.clone());
            }
            context
                .queued
                .insert(account.clone(), self.deleg.d.queued_for(account));
        }
        Ok((targets, context))
    }

    pub(super) fn inspect_routes(
        &mut self,
        targets: Vec<crate::routing::Target>,
        context: crate::routing::Context,
        approved: Option<crate::routing::Approved>,
    ) {
        if self.deleg.route_read.is_some() {
            self.push_system("A local route inspection is already running.");
            return;
        }
        let root = self.state_root.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.deleg.route_read = Some((self.deleg.generation, rx));
        std::thread::spawn(move || {
            let decision = approved
                .map(|approved| approved.evaluate(&root, &context))
                .unwrap_or_else(|| Ok(crate::routing::capture(&root, &targets, &context)));
            let _ = tx.send(decision);
        });
        self.push_system(
            "Reading local route facts; capacity remains unknown without a verified observation.",
        );
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
            ["retry", id, "reconciled"] => id.parse::<u32>().map_err(|_| "invalid run id".to_string()).and_then(|id| self.deleg.d.recovered_task(id)).and_then(|(account, task)| self.start_delegate(&account, &task, Origin::User)),
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
    pub(super) fn poll_delegates(&mut self) {
        self.poll_scope_inspection();
        if let Some((generation, rx)) = &self.deleg.route_read {
            match rx.try_recv() {
                Ok(decision) => {
                    let current = *generation == self.deleg.generation;
                    self.deleg.route_read = None;
                    if current {
                        self.push_system(
                            decision
                                .map(|decision| decision.report().join("\n"))
                                .unwrap_or_else(|error| error),
                        );
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    let current = *generation == self.deleg.generation;
                    self.deleg.route_read = None;
                    if current {
                        self.push_system(
                            "Local route inspection stopped; retry the inspection command.",
                        );
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
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
        for id in finished {
            if !self.deleg.owns(id) {
                self.flash(format!(
                    "worker #{id} finished for an earlier conversation; Ctrl+G inspects it"
                ));
                continue;
            }
            let Some(j) = self.deleg.d.get(id) else {
                continue;
            };
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
        }
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
            || self.images.height() > self.images.draft.len()
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

    /// Proposals recorded by `concordia mcp`'s `delegate` tool (files in `<state>/proposals`).
    /// Checked about every 2 s, and only consumed while the delegation protocol is on. They go
    /// through the same confirm prompt as protocol blocks: nothing runs without the user's y.
    pub(super) fn poll_proposals(&mut self) {
        if !self.tick_count.is_multiple_of(PROPOSAL_POLL_TICKS)
            || !crate::mcp::has_proposals(&self.state_root)
            || !crate::store::delegation_protocol_on(&self.state_root)
        {
            return;
        }
        let blocks: Vec<_> = crate::mcp::take_proposals(&self.state_root)
            .into_iter()
            .filter_map(|p| crate::delegate::block_from(&p.to, &p.task))
            .collect();
        self.queue_proposals(blocks);
    }

    fn queue_proposals(&mut self, blocks: Vec<crate::delegate::DelegateBlock>) {
        let settings = get_settings(&self.state_root);
        let orch = normalize_account_id(&self.account);
        for b in blocks {
            let is_worker = settings
                .workers
                .iter()
                .any(|w| normalize_account_id(w) == b.to);
            if !is_worker || b.to == orch {
                self.push_system(format!(
                    "the orchestrator proposed delegating to {} - ignored: not one of your workers (/workers)",
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
            });
        }
    }

    fn answer_confirm(&mut self, yes: bool) {
        let Some(c) = self.deleg.confirms.pop_front() else {
            return;
        };
        if let Some(next) = self.deleg.confirms.front_mut() {
            next.since = Instant::now();
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
        let workers: Vec<String> = settings
            .workers
            .iter()
            .map(|w| normalize_account_id(w))
            .collect();
        if workers.is_empty() {
            return Err("no workers yet - /workers add <account>".into());
        }
        let manual = if target != "auto" {
            let t = normalize_account_id(target);
            if t == orch {
                return Err(format!(
                    "{t} is the orchestrator now; pick one of your workers (/workers)"
                ));
            }
            if !workers.contains(&t) {
                return Err(format!("{t} is not a worker - /workers add {t}"));
            }
            Some(t)
        } else {
            None
        };
        let (mut targets, context) = self.worker_route_inputs()?;
        if let Some(account) = &manual {
            targets = vec![crate::routing::Target {
                account: account.clone(),
                model: settings.worker_models.get(account).cloned(),
            }];
        }
        let decision = crate::routing::capture(&self.state_root, &targets, &context);
        let selected = if manual.is_some() {
            decision
                .candidates
                .iter()
                .find(|candidate| candidate.eligible)
        } else {
            decision.selected()
        };
        let model = selected.map(|row| row.candidate.target.model.clone());
        let account = selected
            .map(|row| row.candidate.target.account.clone())
            .ok_or_else(|| format!("no eligible worker; {}", decision.report().join("\n")))?;
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
        let trust = worker_trust(self.trust, settings.child_trust);
        let queued_behind = self.deleg.d.running_for(&account).map(|j| j.id);
        let id = self.deleg.d.submit_scoped(
            Submit {
                account: account.clone(),
                provider: acct.provider,
                task: task.to_string(),
                origin,
                spec: JobSpec {
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
        )?;
        self.deleg.owners.insert(id, self.deleg.generation);
        crate::logging::info(&format!(
            "delegate-start id={id} worker={account} provider={} origin={origin:?} trust={} task_chars={}",
            acct.provider.as_str(),
            trust.as_str(),
            task.chars().count()
        ));
        Ok(format!(
            "delegated #{id} to {} ({}) \u{b7} {} \u{b7} trust {}{} \u{b7} {note} \u{b7} /delegate stop {account} cancels",
            crate::accounts::display_account_name(acct.provider, &account),
            acct.provider.as_str(),
            match queued_behind {
                Some(b) => format!("queued behind #{b}"),
                None if !after.is_empty() => format!("approved after {after:?}; /delegate graph shows readiness"),
                None => "running in the background".into(),
            },
            trust.as_str(),
            if trust == TrustMode::Full {
                " (your current mode)"
            } else {
                ""
            },
        ))
    }

    /// Handle `/workers` and `/delegate`. Returns false for every other command.
    pub(super) fn try_delegate_slash(&mut self, cmd: &SlashCommand) -> bool {
        let root = self.state_root.clone();
        match cmd {
            SlashCommand::Workers => {
                let lines =
                    crate::delegate::workers_report(&root, &self.account, &self.deleg.d, Instant::now());
                for l in lines {
                    self.push_system(l);
                }
            }
            SlashCommand::WorkersAdd(None) | SlashCommand::WorkersRemove(None) => {
                self.push_system("usage: /workers add <account> | /workers remove <account> (accounts: /use)");
            }
            SlashCommand::WorkersAdd(Some(a)) => {
                match add_worker(&root, &self.account, a) {
                    Ok(m) | Err(m) => self.push_system(m),
                }
            }
            SlashCommand::WorkersRemove(Some(a)) => {
                let id = normalize_account_id(a);
                if self.deleg.d.running_for(&id).is_some() {
                    self.push_system(format!("{id} is running a task - /delegate stop {id} first"));
                } else {
                    match remove_worker(&root, a) {
                        Ok(m) | Err(m) => self.push_system(m),
                    }
                }
            }
            SlashCommand::Delegate { target, task } => {
                if task.trim().is_empty() {
                    self.push_system("usage: /delegate <account|auto> <task> (see /delegate help)");
                } else {
                    match self.start_delegate(target, task.trim(), Origin::User) {
                        Ok(m) => self.push_system(m),
                        Err(e) => self.push_system(format!("delegate failed - {e}")),
                    }
                }
            }
            SlashCommand::DelegateStop(acct) => self.delegate_stop(acct.as_deref()),
            SlashCommand::DelegateAfter { after, target, task } => {
                match self.start_delegate_after(target, task, Origin::User, after) {
                    Ok(message) => self.push_system(message),
                    Err(error) => self.push_system(format!("delegate failed - {error}")),
                }
            }
            SlashCommand::DelegateGraph => {
                let lines: Vec<_> = self.deleg.d.jobs.iter()
                    .filter(|job| job.conversation_id == self.deleg.d.conversation_id())
                    .map(|job| {
                        let status = self.deleg.d.dependency_status(job.id);
                        let parents: Vec<_> = self.deleg.d.dependency_graph().tasks().iter()
                            .find(|task| task.run_id() == job.run_id)
                            .map(|task| task.prerequisites().iter().map(|p| p.run_id.clone()).collect())
                            .unwrap_or_default();
                        format!("#{} {}: {:?}\nrun {}\nprerequisite runs: {}", job.id, job.account, status,
                            job.run_id, if parents.is_empty() { "none".into() } else { parents.join(", ") })
                    }).collect();
                if lines.is_empty() {
                    self.push_system("No dependency tasks in this conversation.");
                }
                for line in lines { self.push_system(line); }
            }
            SlashCommand::DelegateRoutes(Some(id)) => {
                self.push_system(self.deleg.d.get(*id).map(|job| job.routing_summary()).unwrap_or_else(|| "Unknown worker job".into()));
            }
            SlashCommand::DelegateRoutes(None) => {
                match self.worker_route_inputs() {
                    Ok((targets, context)) => self.inspect_routes(targets, context, None),
                    Err(error) => self.push_system(error),
                }
            }
            SlashCommand::DelegatePolicy(command) => {
                let result = crate::routing::policy_command(&self.state_root, true, command).unwrap_or_else(|error| error);
                self.push_system(result);
            }
            SlashCommand::DelegateScoped { account, declarations, task } => {
                let result = self.start_delegate_scoped(account, task, Origin::User, &[], declarations)
                    .unwrap_or_else(|error| format!("scoped worker not dispatched: {error}"));
                self.push_system(result);
            }
            SlashCommand::DelegateScope(command) => {
                let parts: Vec<_> = command.split_whitespace().collect();
                let message = match parts.as_slice() {
                    [] => {
                        let mut jobs: Vec<_> = self.deleg.d.jobs.iter().collect();
                        jobs.sort_by_key(|job| (job.state.is_finished(), job.id));
                        let mut rows: Vec<_> = jobs.iter().take(16).map(|job| {
                            let cwd = job.workspace.as_ref().map(|request| request.binding.execution.to_string_lossy().chars().take(180).collect::<String>()).unwrap_or_else(|| "legacy/unmanaged".into());
                            format!("#{} {}: {} / {} declared paths / cwd {}\n/delegate scope {} inspects the full captured scope", job.id, job.account, job.state.label(), job.workspace.as_ref().map_or(0, |request| request.claims.len()), cwd, job.id)
                        }).collect();
                        if jobs.len() > 16 { rows.push(format!("Partial list: 16 of {} jobs; inspect an exact job ID for older entries", jobs.len())); }
                        rows.join("\n")
                    },
                    [id] => id.parse::<u32>().ok().and_then(|id| self.deleg.d.get(id)).map(|job| job.workspace_summary()).unwrap_or_else(|| "Unknown worker job".into()),
                    ["stopped", id] => match id.parse::<u32>() {
                        Ok(id) => self.deleg.d.acknowledge_workspace_stopped(id).map(|_| "Stopped-process acknowledgment saved; inspect before retrying the task".into()).unwrap_or_else(|error| error),
                        Err(_) => "Invalid worker job ID".into(),
                    },
                    _ => "usage: /delegate scope [ID|stopped ID]".into(),
                };
                self.push_system(if message.is_empty() { "No worker workspace declarations yet".into() } else { message });
            }
            SlashCommand::DelegateUse(acct) => self.delegate_use(acct.as_deref()),
            SlashCommand::DelegateCollect => self.delegate_collect(),
            SlashCommand::DelegateReturns(mode) => {
                if let Some(auto) = mode {
                    if let Err(e) = crate::store::modify_store(&root, |s| {
                        s.settings.delegate_auto_return = *auto;
                        Ok(())
                    }) {
                        self.push_system(format!("could not save return mode: {e}"));
                        return true;
                    }
                    self.deleg.auto_return_paused = false;
                    if !auto {
                        self.deleg.outbox = None;
                    }
                }
                self.push_system(if get_settings(&root).delegate_auto_return {
                    "Worker returns: auto. Completed batches start a main-agent turn when idle; drafts stay intact. Proposals still need approval. /delegate returns manual disables this."
                } else {
                    "Worker returns: manual. /delegate collect sends new completed results together; /delegate returns auto enables automatic batches."
                });
            }
            SlashCommand::DelegateModel { account, model } => {
                let result = self.worker_model(account, model.as_deref());
                self.push_system(result);
            }
            SlashCommand::DelegateHelp => {
                for l in help_lines(crate::store::delegation_protocol_on(&root)) {
                    self.push_system(l);
                }
            }
            SlashCommand::DelegateProtocol(None) => {
                let on = crate::store::delegation_protocol_on(&root);
                self.push_system(format!(
                    "delegation protocol is {} - /delegate protocol on|off (see /delegate help)",
                    if on { "on" } else { "off" }
                ));
            }
            SlashCommand::DelegateProtocol(Some(on)) => match set_delegation_protocol(&root, *on) {
                Ok(()) => self.push_system(if *on {
                    "delegation protocol on: orchestrator replies may propose delegations; each one needs your y"
                } else {
                    "delegation protocol off"
                }),
                Err(e) => self.push_system(format!("could not save - {e}")),
            },
            _ => return false,
        }
        true
    }

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
        let settings = get_settings(&self.state_root);
        if account.is_empty() {
            return format!(
                "/delegate model ACCOUNT [MODEL|default]\nWorkers: {}",
                settings.workers.join(", ")
            );
        }
        if !settings.workers.contains(&account) {
            return format!("{account} is not a worker; /workers add {account} first");
        }
        let Some(acct) = get_account(&self.state_root, &account) else {
            return "worker account is unavailable".into();
        };
        let models = acct
            .home_path
            .as_deref()
            .map(|h| crate::catalog::list_models(acct.provider, h))
            .unwrap_or_default();
        let Some(model) = model else {
            return format!("{account}: {}\nNative models: {}\n/delegate model {account} MODEL | default (applies to new jobs)",
                settings.worker_models.get(&account).map(String::as_str).unwrap_or("provider default"),
                if models.is_empty() { "catalog unavailable".into() } else { models.join(", ") });
        };
        if model != "default" && !models.iter().any(|m| m == model) {
            return format!("model is not in {account}'s local catalog; /delegate model {account} lists available ids");
        }
        match crate::store::modify_store(&self.state_root, |store| {
            if model == "default" {
                store.settings.worker_models.remove(&account);
            } else {
                store
                    .settings
                    .worker_models
                    .insert(account.clone(), model.to_string());
            }
            Ok(())
        }) {
            Ok(()) => format!("{account} model: {model}; applies to new jobs, including proposals"),
            Err(e) => format!("could not save worker model: {e}"),
        }
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
            .take(crate::delegate::MAX_WORKERS)
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

#[cfg(test)]
mod tests;

pub(super) fn clickable(app: &App, x: u16, y: u16) -> bool {
    app.deleg.viewer.is_none() && app.deleg.rects.iter().any(|(r, _)| contains(*r, x, y))
}

pub(super) fn overlay_open(app: &App) -> bool {
    app.deleg.viewer.is_some() || !app.deleg.confirms.is_empty()
}
