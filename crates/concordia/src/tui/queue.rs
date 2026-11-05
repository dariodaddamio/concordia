//! The prompt queue: lines up the next prompts while a turn runs (see docs/architecture.md,
//! "Prompt queue"). The panel sits above the prompt box and is separate from the draft; the
//! first item is sent through the normal submit path when a turn finishes successfully, and a
//! cancelled or failed turn pauses the queue. Persisted to the pane's recovery record.
//!
//! All state lives in [`Queue`]; this module only reads and writes `App` fields from its own
//! `impl App` block, so `tui/mod.rs` keeps just the call sites.

use super::editor::Editor;
use super::text::char_width;
use super::{contains, App, JobKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use tokio::sync::mpsc;

/// Item rows shown before `+N more`.
const PANEL_MAX: usize = 5;
/// Characters of a prompt shown in its row.
const LABEL_CHARS: usize = 60;
const EDIT_ROWS: usize = 6;

const SELECTED_BG: Color = Color::Indexed(238);
const DIM: Color = Color::Indexed(245);
const WARN: Color = Color::Indexed(179);

#[derive(Default)]
pub(super) struct Queue {
    pub items: Vec<String>,
    /// Set by a cancel or a failed turn (and at launch); explicitly resumed with Ctrl+R.
    pub paused: bool,
    /// Selected row while the panel has focus.
    pub sel: usize,
    pub focused: bool,
    /// Inline editor for `items[sel]`.
    pub edit: Option<Editor>,
    pub(super) edit_scroll: usize,
    pub(super) edit_rect: Option<(Rect, usize)>,
    /// Click targets from the last draw.
    pub rects: Vec<(Rect, usize)>,
    /// The turn that just ended was clean: the next item may be sent.
    pub(super) advance: bool,
    pub(super) force_pending: bool,
    pub(super) send_targets: Vec<(Rect, usize)>,
    pressed_send: Option<usize>,
}

impl Queue {
    fn clamp(&mut self) {
        if self.items.is_empty() {
            self.sel = 0;
            self.focused = false;
            self.edit = None;
            self.edit_scroll = 0;
            self.edit_rect = None;
            // Nothing left to resume: the next queued prompt starts fresh.
            self.paused = false;
        } else {
            self.sel = self.sel.min(self.items.len() - 1);
        }
    }

    fn unfocus(&mut self) {
        self.focused = false;
        self.edit = None;
        self.edit_scroll = 0;
        self.edit_rect = None;
    }
}

/// One-line label: whitespace collapsed, cut to [`LABEL_CHARS`].
fn label(text: &str) -> String {
    let flat = crate::attachments::Message::text(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut it = flat.chars();
    let head: String = it.by_ref().take(LABEL_CHARS).collect();
    if it.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

fn fit(s: &str, width: usize) -> String {
    let mut used = 0;
    let mut out = String::new();
    for c in s.chars() {
        let w = char_width(c);
        if used + w > width {
            break;
        }
        used += w;
        out.push(c);
    }
    out
}

/// Which items the panel shows and how many rows it needs.
struct Plan {
    first: usize,
    count: usize,
    more: usize,
}

impl Plan {
    fn of(q: &Queue) -> Plan {
        let n = q.items.len();
        let count = n.min(PANEL_MAX);
        // Keep the selected row in view when focus moves past the fifth item.
        let first = if q.focused && q.sel >= PANEL_MAX {
            q.sel + 1 - PANEL_MAX
        } else {
            0
        };
        Plan {
            first,
            count,
            more: n - count,
        }
    }

    fn rows(&self, q: &Queue) -> usize {
        if q.items.is_empty() {
            return 0;
        }
        usize::from(q.paused) + self.count + usize::from(self.more > 0)
    }
}

// ---- layout ------------------------------------------------------------------------------

/// Take the panel's rows off the bottom of the transcript area (nothing when the queue is empty).
pub(super) fn carve(transcript: &mut Rect, app: &App) -> Rect {
    let q = &app.queue;
    let rows = if let Some(ed) = q.edit.as_ref() {
        let prefix = edit_prefix(q.sel + 1, transcript.width as usize);
        let text_width = (transcript.width as usize).saturating_sub(display_width(&prefix));
        let editor_rows = ed.layout(text_width.max(1)).len().clamp(1, EDIT_ROWS);
        Plan::of(q).rows(q) + editor_rows
    } else {
        Plan::of(q).rows(q)
    }
    .min(transcript.height.saturating_sub(3) as usize) as u16;
    transcript.height -= rows;
    Rect {
        x: transcript.x,
        y: transcript.y + transcript.height,
        width: transcript.width,
        height: rows,
    }
}

pub(super) fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    app.queue.rects.clear();
    app.queue.send_targets.clear();
    app.queue.edit_rect = None;
    if area.height == 0 || area.width == 0 || app.queue.items.is_empty() {
        return;
    }
    let w = area.width as usize;
    if app.queue.edit.is_some() {
        draw_editor(f, area, app);
        return;
    }
    let plan = Plan::of(&app.queue);
    let q = &app.queue;
    let mut lines: Vec<Line> = Vec::new();
    let mut rects = Vec::new();
    if q.paused {
        lines.push(Line::from(Span::styled(
            fit(" queue paused - Ctrl+R resumes", w),
            Style::default().fg(WARN),
        )));
    }
    for i in plan.first..plan.first + plan.count {
        let row_y = area.y + lines.len() as u16;
        if row_y >= area.y + area.height {
            break;
        }
        let selected = q.focused && q.sel == i;
        let prefix = format!(" queued {}: ", i + 1);
        let base = if selected {
            Style::default()
                .bg(SELECTED_BG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };
        let text = fit(
            &format!("{prefix}{}", label(&q.items[i])),
            if w >= 28 { w.saturating_sub(13) } else { w },
        );
        let pad = w.saturating_sub(text.chars().count());
        let mut line = Line::from(Span::styled(
            if selected {
                format!("{text}{}", " ".repeat(pad.saturating_sub(13)))
            } else {
                text
            },
            base,
        ));
        if w >= 28 {
            let remaining = w.saturating_sub(line.width() + 12);
            line.spans.push(Span::raw(" ".repeat(remaining)));
            line.spans.push(Span::styled(
                "[ Send now ]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        rects.push((
            Rect {
                x: area.x,
                y: row_y,
                width: area.width,
                height: 1,
            },
            i,
        ));
        lines.push(line);
    }
    if plan.more > 0 && (lines.len() as u16) < area.height {
        lines.push(Line::from(Span::styled(
            format!(" +{} more", plan.more),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.truncate(area.height as usize);
    f.render_widget(Paragraph::new(lines), area);
    if w >= 28 {
        app.queue.send_targets = rects
            .iter()
            .map(|(r, i)| (Rect::new(r.right() - 12, r.y, 12, 1), *i))
            .collect();
    }
    app.queue.rects = rects;
}

fn edit_prefix(index: usize, width: usize) -> String {
    let full = format!(" edit {index}: ");
    if width <= 2 {
        String::new()
    } else if width < display_width(&full) + 4 {
        format!("e{index}: ")
    } else {
        full
    }
}

fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

fn draw_editor(f: &mut Frame, area: Rect, app: &mut App) {
    let (lines, rect, scroll, caret, rects) = {
        let q = &app.queue;
        let Some(ed) = q.edit.as_ref() else { return };
        let width = area.width as usize;
        let compact = area.height as usize <= usize::from(q.paused) + 1;
        let prefix = edit_prefix(q.sel + 1, width);
        let prefix_width = display_width(&prefix);
        let text_width = width.saturating_sub(prefix_width);
        let layout_width = text_width.max(1);
        let rows = ed.layout(layout_width);
        let visible_max = rows.len().clamp(1, EDIT_ROWS);
        let (cursor_row, cursor_col) = ed.cursor_pos(layout_width);
        let mut lines = Vec::new();
        if q.paused && area.height > 1 {
            lines.push(Line::from(Span::styled(
                fit(" queue paused - Ctrl+R resumes", width),
                Style::default().fg(WARN),
            )));
        }
        let text_y = if compact {
            area.y + lines.len() as u16
        } else {
            let header_y = area.y + lines.len() as u16;
            lines.push(Line::from(Span::styled(
                fit(
                    &format!(" editing {} - send waits for save", q.sel + 1),
                    width,
                ),
                Style::default().fg(Color::Cyan),
            )));
            header_y + 1
        };
        let available_rows = area.y.saturating_add(area.height).saturating_sub(text_y) as usize;
        let visible = visible_max.min(available_rows);
        let scroll = cursor_row
            .saturating_add(1)
            .saturating_sub(visible.max(1))
            .min(rows.len().saturating_sub(visible.max(1)));
        let mut caret = None;
        let rect = (text_width > 0 && visible > 0).then_some((
            Rect {
                x: area.x + prefix_width as u16,
                y: text_y,
                width: text_width as u16,
                height: visible as u16,
            },
            scroll,
        ));
        for (relative, row) in rows.iter().enumerate().skip(scroll).take(visible) {
            let mut row_text: String = ed.chars()[row.start..row.end].iter().collect();
            if !row.last && row_text.ends_with(' ') {
                row_text.pop();
            }
            let prefix_text = if relative == scroll {
                fit(&prefix, width)
            } else {
                " ".repeat(prefix_width.min(width))
            };
            let mut spans = vec![
                Span::styled(prefix_text, Style::default().fg(Color::Cyan)),
                Span::styled(row_text, Style::default().fg(Color::Cyan)),
            ];
            if let Some((a, b)) = ed.selection() {
                let start = a.max(row.start);
                let end = b.min(row.end);
                if start < end {
                    let before: String = ed.chars()[row.start..start].iter().collect();
                    let selected: String = ed.chars()[start..end].iter().collect();
                    let after: String = ed.chars()[end..row.end].iter().collect();
                    spans = vec![
                        Span::styled(
                            if relative == scroll {
                                fit(&prefix, width)
                            } else {
                                " ".repeat(prefix_width.min(width))
                            },
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(before),
                        Span::styled(selected, Style::default().bg(SELECTED_BG)),
                        Span::raw(after),
                    ];
                }
            }
            let y = text_y + (relative - scroll) as u16;
            lines.push(Line::from(spans));
            if relative == cursor_row {
                let x = area.x as usize + prefix_width + cursor_col;
                let x = x.min(area.x as usize + width.saturating_sub(1));
                caret = Some(Position::new(x as u16, y));
            }
        }
        let mut rects = Vec::new();
        for i in plan_indices(q) {
            if i == q.sel || lines.len() as u16 >= area.height {
                continue;
            }
            let y = area.y + lines.len() as u16;
            let text = fit(&format!(" queued {}: {}", i + 1, label(&q.items[i])), width);
            lines.push(Line::from(Span::styled(text, Style::default().fg(DIM))));
            rects.push((
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                },
                i,
            ));
        }
        if q.items.len() > rects.len() + 1 && (lines.len() as u16) < area.height {
            lines.push(Line::from(Span::styled(
                fit(
                    &format!(" +{} more", q.items.len() - rects.len() - 1),
                    width,
                ),
                Style::default().fg(Color::DarkGray),
            )));
        }
        lines.truncate(area.height as usize);
        (lines, rect, scroll, caret, rects)
    };
    f.render_widget(Paragraph::new(lines), area);
    app.queue.edit_rect = rect;
    app.queue.edit_scroll = scroll;
    app.queue.rects = rects;
    if let Some(p) = caret {
        if app.activity.detail.is_none()
            && app.picker.is_none()
            && p.y < area.y.saturating_add(area.height)
            && p.x < area.x.saturating_add(area.width)
        {
            f.set_cursor_position(p);
        }
    }
}

fn plan_indices(q: &Queue) -> std::ops::Range<usize> {
    let plan = Plan::of(q);
    plan.first..(plan.first + plan.count).min(q.items.len())
}

// ---- keys and mouse ----------------------------------------------------------------------

/// Tab: queue the typed prompt when a turn is running or others are already queued. Returns
/// false when Tab should keep its slash-completion job.
pub(super) fn tab(app: &mut App) -> bool {
    if app.images.importing() {
        app.flash("Wait for the attachment to finish importing");
        return true;
    }
    let mut line = app.editor.text();
    if line.trim().is_empty() && app.images.count() > 0 {
        line = app.images.default_prompt().into();
    }
    if line.trim().is_empty()
        || line.trim_start().starts_with('/')
        || !(app.busy || !app.queue.items.is_empty())
    {
        return false;
    }
    let line = crate::attachments::Message {
        text: line,
        attachments: std::mem::take(&mut app.images.draft),
        invalid: None,
    }
    .encode();
    app.editor.clear();
    app.clear_selections();
    app.palette.reset();
    app.hist_pos = None;
    app.queue.items.push(line);
    app.queue_changed();
    true
}

/// Up on the first prompt row with an empty draft and a queue: focus the panel.
pub(super) fn focus_on_up(app: &mut App) -> bool {
    if app.queue.items.is_empty() || !app.editor.chars().is_empty() || app.hist_pos.is_some() {
        return false;
    }
    app.queue.focused = true;
    app.queue.sel = app.queue.items.len() - 1;
    app.flash("queue: Enter edit \u{b7} Alt+Up/Down move \u{b7} Del remove \u{b7} Esc back");
    true
}

/// Keys while the panel has focus. Returns true when the key was consumed.
pub(super) fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    if app.queue.focused
        && app.queue.edit.is_none()
        && key.code == KeyCode::Enter
        && key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        let index = app.queue.sel;
        let notice = app.force_queued(index);
        app.flash(notice);
        return true;
    }
    if !app.queue.focused {
        return false;
    }
    if app.queue.items.is_empty() {
        app.queue.unfocus();
        return false;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    if app.queue.edit.is_some() {
        let width = app
            .queue
            .edit_rect
            .map(|(r, _)| r.width as usize)
            .unwrap_or(1)
            .max(1);
        return edit_key(app, key.code, ctrl, alt, shift, width);
    }
    let n = app.queue.items.len();
    match key.code {
        KeyCode::Esc => app.queue.unfocus(),
        KeyCode::Up if alt => app.move_queued(-1),
        KeyCode::Down if alt => app.move_queued(1),
        KeyCode::Up => app.queue.sel = app.queue.sel.saturating_sub(1),
        KeyCode::Down if app.queue.sel + 1 >= n => app.queue.unfocus(),
        KeyCode::Down => app.queue.sel += 1,
        KeyCode::Enter => {
            let message = crate::attachments::Message::decode(&app.queue.items[app.queue.sel]);
            if let Err(error) = message.validate() {
                app.flash(error);
                return true;
            }
            let mut ed = Editor::default();
            ed.set_text(&message.text);
            ed.reset_undo();
            app.queue.edit = Some(ed);
            app.queue.edit_scroll = 0;
        }
        KeyCode::Delete => app.remove_queued(),
        // Typing goes back to the prompt: leave the panel and let the key through.
        KeyCode::Char(_) | KeyCode::Backspace if !ctrl || alt => {
            app.queue.unfocus();
            return false;
        }
        _ => return false,
    }
    true
}

fn edit_key(
    app: &mut App,
    code: KeyCode,
    ctrl: bool,
    alt: bool,
    shift: bool,
    width: usize,
) -> bool {
    let Some(ed) = app.queue.edit.as_mut() else {
        return false;
    };
    match code {
        KeyCode::Enter if ctrl || alt || shift => ed.insert_char('\n'),
        KeyCode::Enter => {
            let text = ed.text();
            app.queue.edit = None;
            app.queue.edit_scroll = 0;
            if text.trim().is_empty() {
                // Emptied on purpose: same as deleting the item.
                app.remove_queued();
            } else {
                let i = app.queue.sel;
                let original = crate::attachments::Message::decode(&app.queue.items[i]);
                app.queue.items[i] = crate::attachments::Message { text, ..original }.encode();
                app.queue_changed();
            }
        }
        KeyCode::Esc => {
            app.queue.edit = None;
            app.queue.edit_scroll = 0;
            app.queue_changed();
        }
        KeyCode::Char(c) if ctrl && !alt => match c.to_ascii_lowercase() {
            'c' => {
                app.queue.edit = None;
                app.queue.edit_scroll = 0;
                app.queue_changed();
            }
            'j' => ed.insert_char('\n'),
            'a' => ed.select_all(),
            'z' if shift || c == 'Z' => {
                ed.redo();
            }
            'z' => {
                ed.undo();
            }
            'y' => {
                ed.redo();
            }
            'v' => app.paste_clipboard(),
            _ => {}
        },
        KeyCode::Char(c) => ed.insert_char(c),
        KeyCode::Backspace if ctrl => ed.delete_word_left(),
        KeyCode::Backspace => ed.backspace(),
        KeyCode::Delete if ctrl => ed.delete_word_right(),
        KeyCode::Delete => ed.delete(),
        KeyCode::Left if ctrl => ed.word_left(shift),
        KeyCode::Right if ctrl => ed.word_right(shift),
        KeyCode::Left => ed.move_left(shift),
        KeyCode::Right => ed.move_right(shift),
        KeyCode::Home => ed.home(width, shift),
        KeyCode::End => ed.end(width, shift),
        KeyCode::Up => ed.up(width, shift),
        KeyCode::Down => ed.down(width, shift),
        _ => {}
    }
    true
}

/// Pasted text goes to the inline editor while one is open. True when it took the text.
pub(super) fn paste(app: &mut App, text: &str, continues: bool) -> bool {
    match app.queue.edit.as_mut() {
        Some(ed) => {
            ed.insert_paste_chunk(text, continues);
            true
        }
        None => false,
    }
}

/// Click a queued row to select it; a click anywhere else leaves the panel.
pub(super) fn handle_mouse(app: &mut App, m: &MouseEvent) -> bool {
    let send_hit = app
        .queue
        .send_targets
        .iter()
        .find(|(r, _)| contains(*r, m.column, m.row))
        .map(|(_, i)| *i);
    if m.kind == MouseEventKind::Down(MouseButton::Left) && send_hit.is_some() {
        app.queue.pressed_send = send_hit;
        return true;
    }
    if m.kind == MouseEventKind::Up(MouseButton::Left) {
        if let Some(index) = app.queue.pressed_send.take() {
            if send_hit == Some(index) {
                let notice = app.force_queued(index);
                app.flash(notice);
            }
            return true;
        }
    }
    if m.kind != MouseEventKind::Down(MouseButton::Left) {
        return false;
    }
    if app.queue.edit.is_some() {
        if let (Some(ed), Some((r, scroll))) = (app.queue.edit.as_mut(), app.queue.edit_rect) {
            if contains(r, m.column, m.row) {
                let width = r.width.max(1) as usize;
                let row = scroll + m.row.saturating_sub(r.y) as usize;
                let col = m.column.saturating_sub(r.x) as usize;
                ed.move_to(ed.index_at(width, row, col), false);
                return true;
            }
        }
        app.flash("save with Enter or cancel with Esc/Ctrl+C before leaving the queued edit");
        return true;
    }
    let hit = app
        .queue
        .rects
        .iter()
        .find(|(r, _)| contains(*r, m.column, m.row))
        .map(|(_, i)| *i);
    match hit {
        Some(i) => {
            app.queue.sel = i;
            app.queue.focused = true;
            true
        }
        None => {
            if app.queue.focused {
                app.queue.unfocus();
            }
            false
        }
    }
}

// ---- state changes -----------------------------------------------------------------------

impl App {
    pub(super) fn force_queued(&mut self, index: usize) -> String {
        if self.queue.edit.is_some() {
            return "Save or cancel the queued edit before sending.".into();
        }
        if self.job.is_some() || self.session_ui.loading() {
            return "Finish the active command or history load before sending.".into();
        }
        if index >= self.queue.items.len() {
            return "No queued message at that position.".into();
        }
        if let Err(error) = crate::attachments::Message::decode(&self.queue.items[index]).validate()
        {
            return format!("Queued attachment remains paused: {error}");
        }
        if self.queue.force_pending {
            return "Stopping the active turn before sending the selected message…".into();
        }
        let item = self.queue.items.remove(index);
        self.queue.items.insert(0, item);
        self.queue.force_pending = true;
        self.questions.clear();
        self.queue_changed();
        self.continuation.invalidate();
        if self.busy {
            self.request_cancel();
            "Stopping the active turn; the selected queued message sends once it stops.".into()
        } else {
            "Sending the selected queued message; remaining messages stay paused.".into()
        }
    }
    /// Persist after any change and keep the selection valid.
    pub(super) fn queue_changed(&mut self) {
        self.queue.clamp();
        self.persist_queue_now();
    }

    fn move_queued(&mut self, dir: isize) {
        let i = self.queue.sel;
        let j = i as isize + dir;
        if j < 0 || j as usize >= self.queue.items.len() {
            return;
        }
        self.queue.items.swap(i, j as usize);
        self.queue.sel = j as usize;
        self.queue_changed();
    }

    fn remove_queued(&mut self) {
        let i = self.queue.sel;
        if i < self.queue.items.len() {
            self.queue.items.remove(i);
        }
        self.queue_changed();
    }

    /// Esc / Ctrl+C on a running turn: nothing queued may fire after it.
    pub(super) fn pause_queue(&mut self) {
        if !self.queue.items.is_empty() {
            self.queue.paused = true;
        }
    }

    /// Called at the end of `finalize_turn`. A cancelled or failed turn pauses the queue and
    /// leaves the draft alone on cancellation. Only failure offers its prompt for retry.
    pub(super) fn turn_finished(&mut self, cancelled: bool, failed: bool) {
        let sent = self.last_sent.take();
        if cancelled || failed {
            self.queue.advance = false;
            self.pause_queue();
            if let Some(p) = sent.filter(|_| failed && !cancelled) {
                if self.editor.text().trim().is_empty() && self.images.count() == 0 {
                    self.restore_composer(&p);
                    self.push_system("your prompt is back in the box");
                }
            }
        } else {
            self.queue.advance = true;
        }
    }

    /// A turn ended: finish it, then send the next queued prompt if the turn was clean.
    pub(super) fn on_turn_done(
        &mut self,
        mut outcome: crate::provider::turn::TurnOutcome,
        tx: &mpsc::UnboundedSender<String>,
    ) {
        if self.cancel_requested {
            outcome.cancelled = true;
        }
        let scope_closed = self.finish_main_scope(&outcome);
        if !scope_closed {
            // A failed admission must not consume an explicitly selected queue item.
            self.queue.force_pending = false;
            self.queue.advance = false;
            self.pause_queue();
        }
        if !scope_closed && !outcome.cancelled && (outcome.failed || outcome.exit_code != Some(0)) {
            self.continuation.pause_for_workspace_reconciliation();
        }
        let turn_closed = outcome.cancelled || (!outcome.failed && outcome.exit_code == Some(0));
        self.preparing_cancel = None;
        if let Some(session) = outcome.session_id.as_ref() {
            self.native_session_id = Some(session.clone());
        }
        self.remember_native_source();
        self.cancel_requested = false;
        self.busy = false;
        self.pulse_on = false;
        self.cancel_handle = None;
        self.questions.cancel_native();
        self.native_owner = None;
        let automatic_target = if self.queue.force_pending || !scope_closed {
            None
        } else {
            self.prepare_usage_continuation(&outcome)
        };
        self.finalize_turn(outcome);
        self.reference_journal.finish(
            self.state_root.clone(),
            self.native_session_id.clone(),
            turn_closed && scope_closed,
        );
        self.native_accounting_owner = None;
        if turn_closed && scope_closed && self.reference_journal.pending() {
            // Keep the request recovery transaction until its verified references are durable.
            self.reference_journal
                .defer_close(self.continuation.run_id());
        } else {
            self.reference_journal.prevent_close();
            self.save_turn_checkpoint(turn_closed && scope_closed);
        }
        if self.questions.waiting() {
            self.pause_queue();
            self.queue.advance = false;
        }
        if self.continuation.waiting() {
            self.queue.advance = false;
            self.pause_queue();
        }
        if let Some(target) = automatic_target {
            self.continue_usage(&target, tx);
        }
        self.queue_advance(tx);
    }

    /// Send queued prompts through the normal submit path until one starts a turn.
    pub(super) fn queue_advance(&mut self, tx: &mpsc::UnboundedSender<String>) {
        if self.reference_journal.backlogged() {
            return;
        }
        if self.queue.force_pending || self.queue.advance {
            if let Some(line) = self.queue.items.first() {
                if let Err(error) = crate::attachments::Message::decode(line).validate() {
                    self.queue.paused = true;
                    self.queue.force_pending = false;
                    self.queue.advance = false;
                    self.push_system(format!("Queue paused: {error}"));
                    return;
                }
            }
        }
        if self.questions.waiting() {
            return;
        }
        if self.queue.force_pending {
            if self.busy
                || self.cancel_requested
                || self.queue.edit.is_some()
                || self.job.is_some()
                || self.session_ui.loading()
            {
                return;
            }
            self.queue.force_pending = false;
            self.queue.advance = false;
            self.queue.paused = true;
            if !self.queue.items.is_empty() {
                let line = self.queue.items.remove(0);
                self.queue_changed();
                self.push_history(&line);
                self.run_line(line, tx);
                self.queue.paused = true;
                self.queue.advance = false;
            }
            return;
        }
        if !self.busy {
            if let Some(target) = self.continuation_target.take() {
                self.continue_usage(&target, tx);
            }
            if let Some(target) = self.resume_deferred_usage() {
                self.continue_usage(&target, tx);
            }
        }
        // An inline editor owns the queue until its edit is saved or cancelled. Removing
        // even an earlier row here would change what `sel` refers to and lose unsaved work.
        if self.queue.edit.is_some() || self.session_ui.loading() {
            return;
        }
        if !std::mem::take(&mut self.queue.advance) {
            return;
        }
        while !self.busy && !self.queue.paused && !self.queue.items.is_empty() {
            if self.job.as_ref().is_some_and(|j| j.kind == JobKind::Update) {
                // A prompt cannot start while an update is installing; do not lose the queue.
                self.queue.paused = true;
                self.push_system("queue paused - an update is running");
                break;
            }
            let line = self.queue.items.remove(0);
            self.queue_changed();
            self.push_history(&line);
            self.run_line(line, tx);
        }
    }

    /// `/queue` and `/queue clear`.
    pub(super) fn queue_command(&mut self, arg: &str) -> Vec<String> {
        if arg == "send" || arg.starts_with("send ") {
            let index = if arg == "send" {
                Some(0)
            } else {
                arg[5..]
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| n.checked_sub(1))
            };
            return vec![index
                .map(|i| self.force_queued(i))
                .unwrap_or_else(|| "usage: /queue send [message number]".into())];
        }
        match arg {
            "" => {
                if self.queue.items.is_empty() {
                    return vec![
                        "the queue is empty - Tab queues a prompt while a turn runs".into()
                    ];
                }
                let mut out = vec![format!(
                    "{} queued{}",
                    self.queue.items.len(),
                    if self.queue.paused {
                        " (paused - Ctrl+R resumes)"
                    } else {
                        ""
                    }
                )];
                out.extend(
                    self.queue
                        .items
                        .iter()
                        .enumerate()
                        .map(|(i, t)| format!("queued {}: {}", i + 1, label(t))),
                );
                out
            }
            "clear" => {
                if self.queue.edit.is_some() {
                    return vec![
                        "save with Enter or cancel with Esc/Ctrl+C before clearing the queue"
                            .into(),
                    ];
                }
                let n = self.queue.items.len();
                self.queue.items.clear();
                self.queue_changed();
                vec![format!(
                    "cleared {n} queued prompt{}",
                    if n == 1 { "" } else { "s" }
                )]
            }
            _ => vec!["usage: /queue \u{b7} /queue resume \u{b7} /queue clear".into()],
        }
    }
}

/// The argument of a `/queue` line, or None when `line` is some other command.
pub(super) fn slash_arg(line: &str) -> Option<&str> {
    let t = line.trim();
    if t == "/queue" {
        Some("")
    } else {
        t.strip_prefix("/queue ").map(str::trim)
    }
}
