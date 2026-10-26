//! Bounded internal read-only file panes. Files are read away from the render/input loop.
use super::*;
use crate::file_links;
use std::io::Read;

const MAX_PANES: usize = 3;
const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LINES: usize = 50_000;
const MIN_CHAT: u16 = 32;
const MIN_FILE: u16 = 24;
const MIN_HEIGHT: u16 = 4;
type ReadJob = std::thread::JoinHandle<Result<Vec<String>, String>>;

pub(super) struct Pane {
    pub id: u32,
    path: PathBuf,
    lines: Vec<String>,
    error: Option<String>,
    loading: bool,
    scroll: usize,
    column: usize,
    pub area: Rect,
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
        }
    }
}

impl Workspace {
    fn reset_cuts(&mut self) {
        self.cuts = (1..self.panes.len())
            .map(|i| (100 * i / self.panes.len()) as u16)
            .collect();
    }

    pub fn open(&mut self, file: file_links::ResolvedFile) -> Result<(), String> {
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
            .spawn(move || read_lines(&path))
            .map_err(|e| format!("file read: {e}"))?;
        self.jobs.push((id, job));
        self.panes.push(Pane {
            id,
            path: file.path,
            lines: Vec::new(),
            error: None,
            loading: true,
            scroll: file.line.unwrap_or(1).saturating_sub(1) as usize,
            column: file.column.unwrap_or(1).saturating_sub(1) as usize,
            area: Rect::default(),
        });
        self.focused = Some(id);
        self.reset_cuts();
        Ok(())
    }

    pub fn close(&mut self, id: u32) -> bool {
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
                    Ok(lines) => {
                        pane.lines = lines;
                        pane.scroll = pane.scroll.min(pane.lines.len().saturating_sub(1));
                    }
                    Err(error) => pane.error = Some(error),
                }
            }
        }
    }
}

fn read_lines(path: &Path) -> Result<Vec<String>, String> {
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
    let mut lines = Vec::new();
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

/// Draw file panes, returning the remaining chat rectangle. No disk I/O occurs here.
pub(super) fn draw(f: &mut Frame, app: &mut App) -> Rect {
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
            " #{} {} · read-only ",
            pane.id,
            pane.path.file_name().unwrap_or_default().to_string_lossy()
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(style)
            .title(title);
        let inner = block.inner(rect);
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
        let lines: Vec<Line> = if pane.loading {
            vec![Line::raw("Loading…")]
        } else if let Some(error) = &pane.error {
            vec![Line::styled(error.clone(), Style::default().fg(Color::Red))]
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
        if focused {
            f.render_widget(
                Paragraph::new("Esc chat · F6 focus · Ctrl+W close").style(style),
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
    if key.code == KeyCode::Esc {
        state.focused = None;
        return true;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('w' | 'W'))
    {
        state.close(id);
        return true;
    }
    if let Some(pane) = state.panes.iter_mut().find(|p| p.id == id) {
        let page = pane.area.height.saturating_sub(2).max(1) as usize;
        match key.code {
            KeyCode::Up => pane.scroll = pane.scroll.saturating_sub(1),
            KeyCode::Down => {
                pane.scroll = (pane.scroll + 1).min(pane.lines.len().saturating_sub(1))
            }
            KeyCode::PageUp => pane.scroll = pane.scroll.saturating_sub(page),
            KeyCode::PageDown => {
                pane.scroll = (pane.scroll + page).min(pane.lines.len().saturating_sub(1))
            }
            KeyCode::Home => {
                pane.scroll = 0;
                pane.column = 0;
            }
            KeyCode::End => pane.scroll = pane.lines.len().saturating_sub(page),
            KeyCode::Left => pane.column = pane.column.saturating_sub(1),
            KeyCode::Right => pane.column = pane.column.saturating_add(1).min(MAX_BYTES as usize),
            _ => {}
        }
    }
    true // Read-only focus consumes typing and Enter, never sending chat.
}

pub(super) fn handle_mouse(app: &mut App, mouse: &MouseEvent) -> bool {
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
                let id = pane.id;
                if y == pane.area.y && x == pane.area.right() - 2 {
                    state.close(id);
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
                pane.scroll = (pane.scroll + 3).min(pane.lines.len().saturating_sub(1));
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
