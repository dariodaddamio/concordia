//! Bounded internal read-only file panes. Files are read away from the render/input loop.
use super::*;
use crate::file_links;
use std::io::Read;

#[path = "workspace_edit.rs"]
mod edit;

const MAX_PANES: usize = 3;
const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LINES: usize = 50_000;
const MIN_CHAT: u16 = 32;
const MIN_FILE: u16 = 24;
const MIN_HEIGHT: u16 = 4;
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
}
struct Snapshot {
    lines: Vec<String>,
    stamp: Stamp,
    text: String,
    colors: syntax::Runs,
}
type ReadJob = std::thread::JoinHandle<Result<Snapshot, String>>;
type WatchJob = std::thread::JoinHandle<Vec<(u32, u64, Option<Stamp>)>>;
fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(Stamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}
fn read_snapshot(path: &Path) -> Result<Snapshot, String> {
    let before = stamp(path).ok_or("file is unavailable")?;
    let text = read_text(path)?;
    let lines = display_lines(&text)?;
    let colors = syntax::highlight(path, &text);
    if stamp(path) != Some(before) {
        return Err("file changed while reading; press r to reload".into());
    }
    Ok(Snapshot {
        lines,
        stamp: before,
        text,
        colors,
    })
}

pub(super) struct Pane {
    pub id: u32,
    path: PathBuf,
    lines: Vec<String>,
    formatted: Option<String>,
    formatted_mode: bool,
    view: TranscriptView,
    error: Option<String>,
    loading: bool,
    scroll: usize,
    column: usize,
    pub area: Rect,
    stamp: Option<Stamp>,
    changed: bool,
    generation: u64,
    query: String,
    searching: bool,
    matches: Vec<usize>,
    document: Option<crate::file_document::Document>,
    colors: syntax::Runs,
    color_revision: u64,
    saving: bool,
    prompt: Option<edit::Action>,
    selecting: bool,
    initial_cursor: Option<(usize, usize)>,
    restoring: bool,
}

#[derive(Clone, Copy)]
enum Resize {
    Columns,
    Rows(usize),
}

pub(super) struct Workspace {
    pub panes: Vec<Pane>,
    pub focused: Option<u32>,
    next_id: u32,
    jobs: Vec<(u32, ReadJob)>,
    screen: Rect,
    divider: Rect,
    files: Rect,
    chat_percent: u16,
    cuts: Vec<u16>,
    resizing: Option<Resize>,
    watch: Option<WatchJob>,
    watched_at: Option<Instant>,
    editor: edit::State,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            panes: Vec::new(),
            focused: None,
            next_id: 1,
            jobs: Vec::new(),
            screen: Rect::default(),
            divider: Rect::default(),
            files: Rect::default(),
            chat_percent: 55,
            cuts: Vec::new(),
            resizing: None,
            watch: None,
            watched_at: None,
            editor: edit::State::default(),
        }
    }
}

impl Workspace {
    #[cfg(test)]
    pub(super) fn file_workers_idle(&self) -> bool {
        self.jobs.is_empty() && self.panes.iter().all(|p| !p.loading && !p.saving)
    }
    pub fn pointer_shape(&self, x: u16, y: u16) -> Option<&'static str> {
        if matches!(self.resizing, Some(Resize::Columns)) || contains(self.divider, x, y) {
            return Some("ew-resize");
        }
        if matches!(self.resizing, Some(Resize::Rows(_)))
            || self
                .panes
                .iter()
                .take(self.panes.len().saturating_sub(1))
                .any(|p| contains(p.area, x, y) && y == p.area.bottom() - 1)
        {
            return Some("ns-resize");
        }
        self.panes.iter().find(|p| contains(p.area, x, y)).map(|p| {
            let link = p.formatted_mode
                && y >= p.area.y + 2
                && p.view
                    .pos_at(
                        (y - p.area.y - 2) as isize,
                        x.saturating_sub(p.area.x + 1) as usize,
                    )
                    .and_then(|pos| p.view.rows.get(pos.row).map(|row| (row, pos.col)))
                    .is_some_and(|(row, col)| {
                        row.links.iter().any(|(a, b, _)| *a <= col && col < *b)
                            || row.web_links.iter().any(|(a, b, _)| *a <= col && col < *b)
                    });
            if (y == p.area.y && x == p.area.right() - 2) || contains(p.mode_button(), x, y) || link
            {
                "pointer"
            } else {
                "text"
            }
        })
    }

    pub fn preview(&mut self, id: u32, formatted: bool) {
        if let Some(pane) = self.panes.iter_mut().find(|p| p.id == id) {
            edit::refresh_preview(pane);
            pane.formatted_mode = formatted;
        }
    }
    fn reset_cuts(&mut self) {
        self.cuts = (1..self.panes.len())
            .map(|i| (100 * i / self.panes.len()) as u16)
            .collect();
    }

    pub fn open(&mut self, file: file_links::ResolvedFile) -> Result<(), String> {
        self.open_content(file, None)
    }

    pub fn open_verified(
        &mut self,
        verified: crate::workspace_search::VerifiedHit,
    ) -> Result<(), String> {
        if self.panes.iter().any(|pane| pane.path == verified.hit.path) {
            return Err(
                "file is already open; close its pane before opening a search snapshot".into(),
            );
        }
        let prefix = verified
            .text
            .get(..verified.hit.range.start)
            .ok_or("search result byte location is invalid")?;
        let line_prefix = prefix.rsplit('\n').next().unwrap_or(prefix);
        let display_column = line_prefix
            .chars()
            .map(crate::file_document::display_width)
            .sum::<usize>();
        let file = file_links::ResolvedFile {
            path: verified.hit.path,
            line: Some(verified.hit.line),
            column: Some(display_column.saturating_add(1) as u32),
        };
        let stamp = Stamp {
            len: verified.text.len() as u64,
            modified: verified.modified,
        };
        self.open_content(file, Some((verified.text, stamp)))?;
        // Search locations refer to raw source lines, not formatted Markdown/JSON rows.
        if let Some(pane) = self.panes.last_mut() {
            pane.formatted_mode = false;
        }
        Ok(())
    }

    fn open_content(
        &mut self,
        file: file_links::ResolvedFile,
        text: Option<(String, Stamp)>,
    ) -> Result<(), String> {
        edit::prepare_open(self)?;
        if let Some(pane) = self.panes.iter_mut().find(|p| p.path == file.path) {
            if pane.area.width > 0 {
                self.focused = Some(pane.id);
            }
            pane.scroll = file
                .line
                .map(|n| n.saturating_sub(1) as usize)
                .unwrap_or(pane.scroll);
            if let Some(column) = file.column {
                pane.column = column.saturating_sub(1) as usize;
            }
            if !pane.loading {
                pane.scroll = pane.scroll.min(pane.lines.len().saturating_sub(1));
            }
            return Ok(());
        }
        if self.panes.len() >= MAX_PANES || self.jobs.len() >= MAX_PANES {
            return Err("file pane/read limit reached (3); close a pane or wait for reads".into());
        }
        if self.screen.width < MIN_CHAT + MIN_FILE + 1
            || self.screen.height < (self.panes.len() as u16 + 1) * MIN_HEIGHT
        {
            return Err("terminal too small for another file pane; enlarge it first".into());
        }
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("file pane id limit reached")?;
        let path = file.path.clone();
        let job = std::thread::Builder::new()
            .name("concordia-file-read".into())
            .spawn(move || {
                if let Some((text, stamp)) = text {
                    let lines = display_lines(&text)?;
                    let colors = syntax::highlight(&path, &text);
                    Ok(Snapshot {
                        lines,
                        stamp,
                        text,
                        colors,
                    })
                } else {
                    read_snapshot(&path)
                }
            })
            .map_err(|e| format!("file read: {e}"))?;
        self.jobs.push((id, job));
        self.panes.push(Pane {
            id,
            path: file.path,
            lines: Vec::new(),
            formatted: None,
            formatted_mode: true,
            view: TranscriptView::default(),
            error: None,
            loading: true,
            scroll: file.line.unwrap_or(1).saturating_sub(1) as usize,
            column: file.column.unwrap_or(1).saturating_sub(1) as usize,
            area: Rect::default(),
            stamp: None,
            changed: false,
            generation: 0,
            query: String::new(),
            searching: false,
            matches: Vec::new(),
            document: None,
            colors: Vec::new(),
            color_revision: 0,
            saving: false,
            prompt: None,
            selecting: false,
            initial_cursor: None,
            restoring: false,
        });
        self.focused = Some(id);
        self.reset_cuts();
        Ok(())
    }

    pub fn close(&mut self, id: u32) -> bool {
        if let Some(pane) = self.panes.iter_mut().find(|p| p.id == id) {
            if pane.saving || pane.document.as_ref().is_some_and(|d| d.dirty()) {
                pane.prompt = Some(edit::Action::Close);
                self.focused = Some(id);
                return true;
            }
        }
        self.close_clean(id)
    }

    fn close_clean(&mut self, id: u32) -> bool {
        let before = self.panes.len();
        self.panes.retain(|p| p.id != id);
        if self.panes.len() == before {
            return false;
        }
        if self.focused == Some(id) {
            self.focused = None;
        }
        self.resizing = None;
        self.reset_cuts();
        true
    }

    pub fn poll(&mut self) {
        let mut index = 0;
        while index < self.jobs.len() {
            if !self.jobs[index].1.is_finished() {
                index += 1;
                continue;
            }
            let (id, job) = self.jobs.swap_remove(index);
            let result = job
                .join()
                .unwrap_or_else(|_| Err("file reader failed".into()));
            if let Some(pane) = self.panes.iter_mut().find(|p| p.id == id) {
                pane.loading = false;
                match result {
                    Ok(snapshot) => {
                        edit::loaded(pane, snapshot);
                    }
                    Err(error) => {
                        pane.error = Some(error);
                        pane.restoring = false;
                    }
                }
            }
        }
        edit::poll(self);

        if self.watch.as_ref().is_some_and(|job| job.is_finished()) {
            if let Ok(results) = self.watch.take().unwrap().join() {
                for (id, generation, observed) in results {
                    if let Some(pane) = self
                        .panes
                        .iter_mut()
                        .find(|p| p.id == id && p.generation == generation && !p.loading)
                    {
                        // Conflicts stay visible until explicit reload/save reconciles them.
                        pane.changed |= observed != pane.stamp;
                    }
                }
            }
        }
        if self.watch.is_none()
            && !self.panes.is_empty()
            && self
                .watched_at
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(1))
        {
            let paths: Vec<_> = self
                .panes
                .iter()
                .map(|p| (p.id, p.generation, p.path.clone()))
                .collect();
            self.watched_at = Some(Instant::now());
            self.watch = std::thread::Builder::new()
                .name("concordia-file-watch".into())
                .spawn(move || {
                    paths
                        .into_iter()
                        .map(|(id, generation, path)| (id, generation, stamp(&path)))
                        .collect()
                })
                .ok();
        }
    }
    fn reload(&mut self, id: u32) -> Result<(), String> {
        if let Some(pane) = self.panes.iter_mut().find(|p| p.id == id) {
            if pane.saving || pane.document.as_ref().is_some_and(|d| d.dirty()) {
                pane.prompt = Some(edit::Action::Reload);
                return Ok(());
            }
        }
        if self.jobs.len() >= MAX_PANES {
            return Err("file readers are busy; try reload shortly".into());
        }
        let pane = self
            .panes
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("file pane is unavailable")?;
        if pane.loading {
            return Ok(());
        }
        let path = pane.path.clone();
        let job = std::thread::Builder::new()
            .name("concordia-file-reload".into())
            .spawn(move || read_snapshot(&path))
            .map_err(|e| e.to_string())?;
        pane.generation = pane.generation.wrapping_add(1);
        pane.initial_cursor = pane.document.as_ref().map(|d| d.position());
        pane.loading = true;
        pane.error = None;
        pane.matches.clear();
        pane.query.clear();
        pane.searching = false;
        self.jobs.push((id, job));
        Ok(())
    }
}

impl Pane {
    fn raw(&self) -> bool {
        !(self.formatted_mode && self.formatted.is_some())
    }
    fn mode_button(&self) -> Rect {
        Rect {
            x: self.area.x + 1,
            y: self.area.y + 1,
            width: self.area.width.saturating_sub(2),
            height: u16::from(self.area.height > 2),
        }
    }

    fn row_count(&self) -> usize {
        if self.formatted_mode && self.formatted.is_some() {
            self.view.rows.len()
        } else {
            self.document
                .as_ref()
                .map_or(self.lines.len(), |d| d.line_count())
        }
    }

    fn find(&mut self, forward: bool, include_current: bool) {
        let query = self.query.to_lowercase();
        if query.is_empty() {
            self.matches.clear();
            return;
        }
        self.matches = if self.formatted_mode && self.formatted.is_some() {
            self.view
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    row.text
                        .iter()
                        .collect::<String>()
                        .to_lowercase()
                        .contains(&query)
                })
                .map(|(i, _)| i)
                .collect()
        } else {
            self.lines
                .iter()
                .enumerate()
                .filter(|(_, line)| line.to_lowercase().contains(&query))
                .map(|(i, _)| i)
                .collect()
        };
        let found = if forward {
            self.matches
                .iter()
                .copied()
                .find(|i| *i > self.scroll || (include_current && *i == self.scroll))
                .or_else(|| self.matches.first().copied())
        } else {
            self.matches
                .iter()
                .rev()
                .copied()
                .find(|i| *i < self.scroll || (include_current && *i == self.scroll))
                .or_else(|| self.matches.last().copied())
        };
        if let Some(row) = found {
            self.scroll = row;
            self.column = if self.formatted_mode && self.formatted.is_some() {
                0
            } else {
                self.lines
                    .get(row)
                    .and_then(|line| {
                        let lower = line.to_lowercase();
                        lower
                            .find(&query)
                            .map(|at| lower[..at].chars().count().saturating_sub(4))
                    })
                    .unwrap_or(0)
            };
        }
    }
    fn toggle_mode(&mut self) {
        edit::refresh_preview(self);
        if self.formatted.is_some() {
            self.formatted_mode = !self.formatted_mode;
            self.scroll = 0;
            self.column = 0;
        }
    }
}

fn read_text(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("file read: {e}"))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("preview requires a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("file read: {e}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("preview limited to 2 MiB; use /edit FILE".into());
    }
    let text =
        String::from_utf8(bytes).map_err(|_| "preview requires UTF-8 text; use /edit FILE")?;
    if text.contains('\0') {
        return Err("binary files cannot be previewed".into());
    }
    if text.lines().count() > MAX_LINES {
        return Err("file limited to 50000 lines".into());
    }
    Ok(text)
}

fn display_lines(text: &str) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    let (text, _) = super::text::terminal_text(text);
    for line in text.lines() {
        if lines.len() >= MAX_LINES {
            return Err("preview limited to 50000 lines; use /edit FILE".into());
        }
        lines.push(
            line.chars()
                .filter(|c| !c.is_control() || *c == '\t')
                .collect(),
        );
    }
    Ok(lines)
}

#[cfg(test)]
fn read_lines(path: &Path) -> Result<Vec<String>, String> {
    display_lines(&read_text(path)?)
}

/// Draw file panes, returning the remaining chat rectangle. No disk I/O occurs here.
pub(super) fn draw(f: &mut Frame, app: &mut App) -> Rect {
    let show_caret = app.picker.is_none() && app.activity.detail.is_none();
    let state = &mut app.workspace;
    state.screen = f.area();
    state.divider = Rect::default();
    state.files = Rect::default();
    for pane in &mut state.panes {
        pane.area = Rect::default();
    }
    let area = f.area();
    if state.panes.is_empty() {
        return area;
    }
    if area.width < MIN_CHAT + MIN_FILE + 1 || area.height < state.panes.len() as u16 * MIN_HEIGHT {
        state.focused = None;
        state.resizing = None;
        f.render_widget(
            Paragraph::new("file panes hidden: enlarge terminal or /close ID"),
            Rect {
                height: 1.min(area.height),
                ..area
            },
        );
        return Rect {
            y: area.y.saturating_add(1.min(area.height)),
            height: area.height.saturating_sub(1),
            ..area
        };
    }
    let chat_w = ((u32::from(area.width) * u32::from(state.chat_percent) / 100) as u16)
        .clamp(MIN_CHAT, area.width - MIN_FILE - 1);
    let chat = Rect {
        width: chat_w,
        ..area
    };
    state.divider = Rect {
        x: area.x + chat_w,
        width: 1,
        ..area
    };
    state.files = Rect {
        x: area.x + chat_w + 1,
        width: area.width - chat_w - 1,
        ..area
    };
    f.render_widget(
        Paragraph::new((0..area.height).map(|_| Line::raw("│")).collect::<Vec<_>>()),
        state.divider,
    );
    let mut previous = 0;
    let count = state.panes.len();
    for (index, pane) in state.panes.iter_mut().enumerate() {
        let bottom = if index + 1 == count {
            area.height
        } else {
            ((u32::from(area.height) * u32::from(state.cuts[index]) / 100) as u16).clamp(
                previous + MIN_HEIGHT,
                area.height - (count - index - 1) as u16 * MIN_HEIGHT,
            )
        };
        let rect = Rect {
            y: area.y + previous,
            height: bottom - previous,
            ..state.files
        };
        previous = bottom;
        pane.area = rect;
        let focused = state.focused == Some(pane.id);
        let style = Style::default().fg(if focused {
            Color::Cyan
        } else {
            Color::DarkGray
        });
        let title = format!(
            " #{} {}{} · {} ",
            pane.id,
            if pane.document.as_ref().is_some_and(|d| d.dirty()) {
                "* "
            } else {
                ""
            },
            pane.path.file_name().unwrap_or_default().to_string_lossy(),
            if pane.raw() { "raw" } else { "preview" }
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(style)
            .title(if pane.changed {
                format!("{title} changed on disk (Ctrl+R reload) ")
            } else {
                title
            });
        let mut inner = block.inner(rect);
        f.render_widget(block, rect);
        f.render_widget(
            Paragraph::new("X").style(style),
            Rect {
                x: rect.right() - 2,
                y: rect.y,
                width: 1,
                height: 1,
            },
        );
        f.render_widget(
            Paragraph::new(if pane.formatted.is_some() {
                if pane.formatted_mode {
                    "[ Formatted ]  Raw (Alt+V)"
                } else {
                    "[ Raw · edit ]  Preview (Alt+V)"
                }
            } else {
                "Raw · edit · syntax colors"
            })
            .style(style),
            pane.mode_button(),
        );
        inner.y += 1;
        inner.height = inner.height.saturating_sub(1);
        let lines: Vec<Line> = if pane.loading {
            vec![Line::raw("Loading…")]
        } else if let Some(error) = pane.error.as_ref().filter(|_| pane.document.is_none()) {
            vec![Line::styled(error.clone(), Style::default().fg(Color::Red))]
        } else if pane.formatted_mode && pane.formatted.is_some() {
            pane.view.rebuild(
                &[TBlock {
                    identity: None,
                    gutter: "",
                    gutter_style: Style::default(),
                    style: Style::default(),
                    text: pane.formatted.as_deref().unwrap_or_default(),
                    linkify: true,
                    markdown: !pane
                        .path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("json")),
                    runs: &[],
                }],
                inner.width.max(1) as usize,
                inner.height as usize,
            );
            pane.scroll = pane.scroll.min(pane.view.rows.len().saturating_sub(1));
            pane.view.follow_tail();
            pane.view.scroll_up(pane.view.rows.len());
            pane.view.scroll_down(pane.scroll);
            pane.view.lines()
        } else if let Some(doc) = &pane.document {
            (pane.scroll..doc.line_count())
                .take(inner.height as usize)
                .map(|row| {
                    let gutter = format!("{:>4} ", row + 1);
                    let colors = if pane.color_revision == doc.revision() {
                        pane.colors.get(row).map(Vec::as_slice).unwrap_or(&[])
                    } else {
                        &[]
                    };
                    let mut line = syntax::line(
                        doc.line(row),
                        pane.column,
                        inner.width.saturating_sub(5) as usize,
                        colors,
                        doc.selection_columns(row),
                    );
                    line.spans.insert(
                        0,
                        Span::styled(gutter, Style::default().fg(Color::DarkGray)),
                    );
                    line
                })
                .collect()
        } else {
            pane.lines
                .iter()
                .skip(pane.scroll)
                .take(inner.height as usize)
                .map(|line| {
                    Line::raw(
                        line.chars()
                            .skip(pane.column)
                            .take(inner.width as usize)
                            .collect::<String>(),
                    )
                })
                .collect()
        };
        f.render_widget(Paragraph::new(lines), inner);
        if focused && pane.raw() && !pane.searching && pane.prompt.is_none() && show_caret {
            if let Some(doc) = &pane.document {
                let (row, col) = doc.position();
                if row >= pane.scroll
                    && row < pane.scroll + inner.height as usize
                    && col >= pane.column
                    && col < pane.column + inner.width.saturating_sub(5) as usize
                {
                    f.set_cursor_position(Position::new(
                        inner.x + 5 + (col - pane.column) as u16,
                        inner.y + (row - pane.scroll) as u16,
                    ));
                }
            }
        }
        if focused {
            f.render_widget(
                Paragraph::new(if pane.prompt.is_some() {
                    "Unsaved · S save · D discard · Esc cancel".into()
                } else if pane.saving {
                    "Saving… edits retained".into()
                } else if let Some(error) = &state.editor.error {
                    format!("Recovery: {error}")
                } else if let Some(error) = &pane.error {
                    error.clone()
                } else if pane.searching {
                    format!("/{}  {} matches", pane.query, pane.matches.len())
                } else if pane.raw() {
                    "Esc chat · Ctrl+S save · Ctrl+F find".into()
                } else if pane.query.is_empty() {
                    "Esc chat · Alt+V edit · / find · r reload".into()
                } else {
                    format!(
                        "/{}  {} matches \u{b7} n/N next/prev",
                        pane.query,
                        pane.matches.len()
                    )
                })
                .style(style),
                Rect {
                    x: rect.x + 1,
                    y: rect.bottom() - 1,
                    width: rect.width - 2,
                    height: 1,
                },
            );
        }
    }
    chat
}

pub(super) fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    if edit::handle_key(app, key) {
        return true;
    }
    let state = &mut app.workspace;
    if key.code == KeyCode::F(6) && !state.panes.is_empty() {
        app.queue.focused = false;
        app.deleg.selected = None;
        state.focused = match state.focused {
            None => state.panes.iter().find(|p| p.area.width > 0).map(|p| p.id),
            Some(id) => state
                .panes
                .iter()
                .position(|p| p.id == id)
                .and_then(|i| state.panes.get(i + 1))
                .filter(|p| p.area.width > 0)
                .map(|p| p.id),
        };
        return true;
    }
    let Some(id) = state.focused else {
        return false;
    };
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'C'))
    {
        return false; // Preserve the existing copy/cancel contract.
    }
    if let Some(pane) = state.panes.iter_mut().find(|p| p.id == id) {
        if pane.searching {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => pane.searching = false,
                KeyCode::Backspace => {
                    pane.query.pop();
                    pane.find(true, true);
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && pane.query.len() < 256 =>
                {
                    pane.query.push(c);
                    pane.find(true, true);
                }
                _ => {}
            }
            return true;
        }
    }
    if key.code == KeyCode::Char('r') && key.modifiers.is_empty() {
        if let Err(error) = state.reload(id) {
            app.flash(error);
        }
        return true;
    }
    if key.code == KeyCode::Esc {
        state.focused = None;
        return true;
    }
    // A read-only pane closes on `q`, as pagers do; Ctrl+W belongs to the terminal's tabs.
    if key.modifiers.is_empty() && key.code == KeyCode::Char('q') {
        state.close(id);
        return true;
    }
    if let Some(pane) = state.panes.iter_mut().find(|p| p.id == id) {
        let page = pane.area.height.saturating_sub(2).max(1) as usize;
        match key.code {
            KeyCode::Char('/') => {
                pane.searching = true;
                pane.query.clear();
                pane.matches.clear();
            }
            KeyCode::Char('n') if key.modifiers.is_empty() => pane.find(true, false),
            KeyCode::Char('N')
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                pane.find(false, false)
            }
            KeyCode::Char('v') if key.modifiers.is_empty() => pane.toggle_mode(),
            KeyCode::Up => pane.scroll = pane.scroll.saturating_sub(1),
            KeyCode::Down => {
                pane.scroll = (pane.scroll + 1).min(pane.row_count().saturating_sub(1))
            }
            KeyCode::PageUp => pane.scroll = pane.scroll.saturating_sub(page),
            KeyCode::PageDown => {
                pane.scroll = (pane.scroll + page).min(pane.row_count().saturating_sub(1))
            }
            KeyCode::Home => {
                pane.scroll = 0;
                pane.column = 0;
            }
            KeyCode::End => pane.scroll = pane.row_count().saturating_sub(page),
            KeyCode::Left => pane.column = pane.column.saturating_sub(1),
            KeyCode::Right => pane.column = pane.column.saturating_add(1).min(MAX_BYTES as usize),
            _ => {}
        }
    }
    true // Read-only focus consumes typing and Enter, never sending chat.
}

pub(super) fn handle_mouse(app: &mut App, mouse: &MouseEvent) -> bool {
    if edit::mouse(app, mouse) {
        return true;
    }
    if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
        let hit = app
            .workspace
            .panes
            .iter()
            .filter(|pane| pane.formatted_mode && pane.formatted.is_some())
            .find_map(|pane| {
                let (x, y) = (mouse.column, mouse.row);
                if !contains(pane.area, x, y)
                    || y < pane.area.y + 2
                    || y >= pane.area.bottom().saturating_sub(1)
                {
                    return None;
                }
                let pos = pane.view.pos_at(
                    (y - pane.area.y - 2) as isize,
                    x.saturating_sub(pane.area.x + 1) as usize,
                )?;
                let row = pane.view.rows.get(pos.row)?;
                let file = row
                    .links
                    .iter()
                    .find(|(a, b, _)| *a <= pos.col && pos.col < *b)
                    .map(|(_, _, c)| c.clone());
                let web = row
                    .web_links
                    .iter()
                    .find(|(a, b, _)| *a <= pos.col && pos.col < *b)
                    .map(|(_, _, u)| u.clone());
                (file.is_some() || web.is_some()).then(|| {
                    (
                        file,
                        web,
                        pane.path.parent().unwrap_or(&app.cwd).to_path_buf(),
                    )
                })
            });
        if let Some((file, web, base)) = hit {
            if let Some(file) = file {
                app.open_file_candidate_from(file, true, base);
            } else if let Some(url) = web {
                app.open_web_link(url);
            }
            return true;
        }
    }
    let state = &mut app.workspace;
    let (x, y) = (mouse.column, mouse.row);
    if let Some(resize) = state.resizing {
        match mouse.kind {
            MouseEventKind::Up(MouseButton::Left) => state.resizing = None,
            MouseEventKind::Drag(MouseButton::Left) => match resize {
                Resize::Columns => {
                    let width = x
                        .saturating_sub(state.screen.x)
                        .clamp(MIN_CHAT, state.screen.width - MIN_FILE - 1);
                    state.chat_percent =
                        (u32::from(width) * 100 / u32::from(state.screen.width)) as u16;
                }
                Resize::Rows(index) => {
                    let min = if index == 0 { 0 } else { state.cuts[index - 1] };
                    let max = state.cuts.get(index + 1).copied().unwrap_or(100);
                    let gap = (u32::from(MIN_HEIGHT) * 100).div_ceil(u32::from(state.screen.height))
                        as u16;
                    if min + gap <= max.saturating_sub(gap) {
                        state.cuts[index] = (u32::from(y.saturating_sub(state.screen.y)) * 100
                            / u32::from(state.screen.height))
                        .min(100) as u16;
                        state.cuts[index] = state.cuts[index].clamp(min + gap, max - gap);
                    }
                }
            },
            _ => {}
        }
        return true;
    }
    if mouse.kind == MouseEventKind::Down(MouseButton::Left) && contains(state.divider, x, y) {
        state.resizing = Some(Resize::Columns);
        app.drag = Drag::None;
        app.file_click = None;
        return true;
    }
    let hit = state.panes.iter().position(|p| contains(p.area, x, y));
    if let Some(index) = hit {
        let pane = &mut state.panes[index];
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                app.queue.focused = false;
                let id = pane.id;
                if y == pane.area.y && x == pane.area.right() - 2 {
                    state.close(id);
                } else if contains(pane.mode_button(), x, y) {
                    pane.toggle_mode();
                    state.focused = Some(id);
                } else if y == pane.area.bottom() - 1 && index + 1 < state.panes.len() {
                    state.resizing = Some(Resize::Rows(index));
                } else {
                    state.focused = Some(id);
                    app.deleg.selected = None;
                }
                app.drag = Drag::None;
                app.file_click = None;
                return true;
            }
            MouseEventKind::ScrollUp => {
                pane.scroll = pane.scroll.saturating_sub(3);
                return true;
            }
            MouseEventKind::ScrollDown => {
                pane.scroll = (pane.scroll + 3).min(pane.row_count().saturating_sub(1));
                return true;
            }
            MouseEventKind::Down(MouseButton::Right) => return true,
            _ => {}
        }
    }
    if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
        state.focused = None;
    }
    false
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;

pub(super) fn paste(app: &mut App, text: &str) -> bool {
    edit::paste(app, text)
}
