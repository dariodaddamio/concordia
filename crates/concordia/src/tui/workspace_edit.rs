//! Raw file editing, explicit asynchronous saves and pane-lease-owned file recovery.
use super::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Close,
    Reload,
    Quit,
}
type Signature = (
    Vec<(u32, u64, bool, bool, usize, usize, (usize, usize))>,
    Option<u32>,
    u16,
    Vec<u16>,
);
type SaveJob = (u32, String, std::thread::JoinHandle<Result<(), String>>);
type ColorJob = (u32, u64, std::thread::JoinHandle<syntax::Runs>);
#[derive(Default)]
pub(super) struct State {
    saves: Vec<SaveJob>,
    colors: Vec<ColorJob>,
    lease: Option<Arc<recovery::RecoveryLease>>,
    workspace: PathBuf,
    observed: Signature,
    persisted: Signature,
    changed: Option<Instant>,
    persistence: Option<(Signature, std::thread::JoinHandle<Result<(), String>>)>,
    pending: Option<Record>,
    pub(super) error: Option<String>,
    pub(super) notice: Option<String>,
    pub(super) quit: bool,
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    workspace: PathBuf,
    panes: Vec<Draft>,
    focused: Option<u32>,
    chat_percent: u16,
    cuts: Vec<u16>,
}
#[derive(Serialize, Deserialize)]
struct Draft {
    id: u32,
    path: PathBuf,
    formatted: bool,
    scroll: usize,
    column: usize,
    cursor: (usize, usize),
    baseline: Option<String>,
    text: Option<String>,
}

fn signature(ws: &Workspace) -> Signature {
    (
        ws.panes
            .iter()
            .map(|p| {
                (
                    p.id,
                    p.document.as_ref().map_or(0, |d| d.revision()),
                    p.document.as_ref().is_some_and(|d| d.dirty()),
                    p.formatted_mode,
                    p.scroll,
                    p.column,
                    p.document.as_ref().map_or((0, 0), |d| d.position()),
                )
            })
            .collect(),
        ws.focused,
        ws.chat_percent,
        ws.cuts.clone(),
    )
}
fn encoded(ws: &Workspace) -> Result<Vec<u8>, String> {
    if ws.panes.is_empty() {
        return Ok(b"[]".to_vec());
    }
    let panes = ws
        .panes
        .iter()
        .map(|p| {
            let dirty = p.document.as_ref().filter(|d| d.dirty());
            Draft {
                id: p.id,
                path: p.path.clone(),
                formatted: p.formatted_mode,
                scroll: p.scroll,
                column: p.column,
                cursor: p.document.as_ref().map_or((0, 0), |d| d.position()),
                baseline: dirty.map(|d| d.baseline().to_owned()),
                text: dirty.map(|d| d.text().to_owned()),
            }
        })
        .collect();
    serde_json::to_vec(&Record {
        version: 1,
        workspace: ws.editor.workspace.clone(),
        panes,
        focused: ws.focused,
        chat_percent: ws.chat_percent,
        cuts: ws.cuts.clone(),
    })
    .map_err(|e| e.to_string())
}

impl Workspace {
    pub(in crate::tui) fn has_unsaved(&self) -> bool {
        self.panes
            .iter()
            .any(|p| p.saving || p.document.as_ref().is_some_and(|d| d.dirty()))
            || self
                .editor
                .pending
                .as_ref()
                .is_some_and(|r| r.panes.iter().any(|p| p.text != p.baseline))
    }
    pub(in crate::tui) fn file_quit_requested(&self) -> bool {
        self.editor.quit
    }
    pub(in crate::tui) fn take_notice(&mut self) -> Option<String> {
        self.editor.notice.take()
    }
    pub(in crate::tui) fn set_file_recovery(
        &mut self,
        lease: Arc<recovery::RecoveryLease>,
        workspace: &Path,
    ) -> Result<(), String> {
        if self.has_unsaved() {
            return Err("save or close current file edits before switching recovery".into());
        }
        let workspace = workspace.canonicalize().map_err(|e| e.to_string())?;
        let bytes = lease.files().map_err(|e| e.to_string())?;
        if bytes == b"[]" || bytes.is_empty() {
            self.flush_file_recovery()?;
            if self.editor.lease.is_some() {
                self.panes.clear();
                self.focused = None;
            }
            self.editor = State {
                lease: Some(lease),
                workspace,
                ..State::default()
            };
            return Ok(());
        }
        let record: Record =
            serde_json::from_slice(&bytes).map_err(|e| format!("file recovery: {e}"))?;
        if record.version != 1 || record.workspace != workspace || record.panes.len() > MAX_PANES {
            return Err("file recovery binding or bounds mismatch".into());
        }
        let mut paths = std::collections::HashSet::new();
        for d in &record.panes {
            if !d.path.is_absolute()
                || !paths.insert(d.path.clone())
                || d.path.to_string_lossy().len() > 4096
                || d.text.is_some() != d.baseline.is_some()
                || d.text.as_ref().is_some_and(|s| {
                    s.len() > MAX_BYTES as usize
                        || s.contains('\0')
                        || s.lines().count() > MAX_LINES
                })
                || d.baseline
                    .as_ref()
                    .is_some_and(|s| s.len() > MAX_BYTES as usize)
            {
                return Err("invalid file recovery record; originals retained".into());
            }
            if d.path.exists() && d.path.canonicalize().map_err(|e| e.to_string())? != d.path {
                return Err("recovered file path was retargeted; originals retained".into());
            }
        }
        self.flush_file_recovery()?;
        self.panes.clear();
        self.focused = None;
        self.editor = State {
            lease: Some(lease),
            workspace,
            pending: Some(record),
            ..State::default()
        };
        Ok(())
    }
    pub(in crate::tui) fn flush_file_recovery(&mut self) -> Result<(), String> {
        if self.editor.pending.is_some() {
            return Ok(());
        }
        if let Some((_, job)) = self.editor.persistence.take() {
            job.join().map_err(|_| "file recovery writer failed")??;
        }
        if let Some(lease) = &self.editor.lease {
            lease
                .save_files(&encoded(self)?)
                .map_err(|e| e.to_string())?;
            self.editor.persisted = signature(self);
        }
        Ok(())
    }
    pub(in crate::tui) fn request_file_exit(&mut self) -> bool {
        if let Some(p) = self
            .panes
            .iter_mut()
            .find(|p| p.saving || p.document.as_ref().is_some_and(|d| d.dirty()))
        {
            p.prompt = Some(Action::Quit);
            self.focused = Some(p.id);
            return false;
        }
        if let Err(e) = self.flush_file_recovery() {
            self.editor.error = Some(e);
            return false;
        }
        true
    }
}

pub(super) fn prepare_open(ws: &mut Workspace) -> Result<(), String> {
    if ws.editor.pending.is_some() {
        if !ws.jobs.is_empty() {
            return Err("file recovery is loading; wait before opening another file".into());
        }
        restore(ws);
    }
    Ok(())
}

fn restore(ws: &mut Workspace) {
    let Some(record) = ws.editor.pending.take() else {
        return;
    };
    ws.chat_percent = record.chat_percent.clamp(20, 80);
    let mut focus = None;
    for draft in record.panes {
        let id = ws.next_id;
        let Some(next) = id.checked_add(1) else { break };
        ws.next_id = next;
        if record.focused == Some(draft.id) {
            focus = Some(id);
        }
        let document = match (draft.baseline, draft.text) {
            (Some(baseline), Some(text)) => {
                crate::file_document::Document::restored(baseline, text).ok()
            }
            _ => None,
        };
        let recovered = document.is_some();
        let mut pane = Pane {
            id,
            detached: false,
            window_area: Rect::default(),
            window_resizing: None,
            path: draft.path.clone(),
            lines: Vec::new(),
            formatted: None,
            formatted_mode: draft.formatted,
            view: TranscriptView::default(),
            error: None,
            loading: true,
            scroll: draft.scroll,
            column: draft.column,
            horizontal_bar: Rect::default(),
            horizontal_measure: None,
            horizontal_max: 0,
            area: Rect::default(),
            stamp: stamp(&draft.path),
            changed: false,
            generation: 0,
            query: String::new(),
            searching: false,
            matches: Vec::new(),
            document,
            colors: Vec::new(),
            color_revision: u64::MAX,
            saving: false,
            prompt: None,
            selecting: false,
            initial_cursor: Some(draft.cursor),
            restoring: recovered,
        };
        if let Some(d) = &mut pane.document {
            d.move_to(draft.cursor.0, draft.cursor.1);
        }
        if recovered {
            refresh_preview(&mut pane);
        }
        {
            let path = draft.path;
            match std::thread::Builder::new()
                .name("concordia-file-restore".into())
                .spawn(move || read_snapshot(&path))
            {
                Ok(job) => ws.jobs.push((id, job)),
                Err(e) => {
                    pane.loading = false;
                    pane.error = Some(e.to_string());
                }
            }
        }
        ws.panes.push(pane);
    }
    ws.focused = focus;
    ws.reset_cuts();
    if record.cuts.len() == ws.panes.len().saturating_sub(1)
        && record.cuts.iter().all(|c| *c > 0 && *c < 100)
        && record.cuts.windows(2).all(|w| w[0] < w[1])
    {
        ws.cuts = record.cuts;
    }
    ws.editor.notice =
        Some("restored file panes; unsaved edits remain unsaved until Ctrl+S".into());
}

fn finish_action(ws: &mut Workspace, id: u32, action: Action) {
    match action {
        Action::Close => {
            ws.close_clean(id);
        }
        Action::Reload => {
            let _ = ws.reload(id);
        }
        Action::Quit => {
            ws.close_clean(id);
            ws.editor.quit = ws.request_file_exit();
        }
    }
}
fn start_save(ws: &mut Workspace, id: u32, state_root: &Path) {
    let Some(pane) = ws.panes.iter_mut().find(|p| p.id == id) else {
        return;
    };
    if pane.saving || pane.loading {
        return;
    }
    let Some(doc) = &pane.document else { return };
    if !doc.dirty() {
        if let Some(action) = pane.prompt.take() {
            finish_action(ws, id, action);
        }
        return;
    }
    let (path, baseline, text) = (
        pane.path.clone(),
        doc.baseline().to_owned(),
        doc.text().to_owned(),
    );
    let saved = text.clone();
    let state_root = state_root.to_path_buf();
    match std::thread::Builder::new()
        .name("concordia-file-save".into())
        .spawn(move || {
            crate::workspaces::with_write_guard(&state_root, &path, || {
                crate::file_document::save(&path, &baseline, &text)
            })
        }) {
        Ok(job) => {
            pane.saving = true;
            pane.error = None;
            ws.editor.saves.push((id, saved, job));
        }
        Err(e) => pane.error = Some(e.to_string()),
    }
}

pub(super) fn poll(ws: &mut Workspace) {
    if ws.screen.width > 0
        && ws.panes.is_empty()
        && ws.jobs.is_empty()
        && ws.editor.pending.is_some()
    {
        restore(ws);
    }
    let mut i = 0;
    while i < ws.editor.saves.len() {
        if !ws.editor.saves[i].2.is_finished() {
            i += 1;
            continue;
        }
        let (id, text, job) = ws.editor.saves.swap_remove(i);
        let result = job
            .join()
            .unwrap_or_else(|_| Err("file save worker failed".into()));
        let mut action = None;
        if let Some(pane) = ws.panes.iter_mut().find(|p| p.id == id) {
            pane.saving = false;
            match result {
                Ok(()) => {
                    if let Some(d) = &mut pane.document {
                        d.acknowledge_save(text);
                    }
                    pane.stamp = stamp(&pane.path);
                    pane.changed = false;
                    pane.generation = pane.generation.wrapping_add(1);
                    ws.editor.notice = Some(format!(
                        "saved {}",
                        pane.path.file_name().unwrap_or_default().to_string_lossy()
                    ));
                    if !pane.document.as_ref().is_some_and(|d| d.dirty()) {
                        action = pane.prompt.take();
                    }
                }
                Err(e) => {
                    pane.error = Some(e.clone());
                    pane.prompt = None;
                    ws.editor.notice = Some(e);
                }
            }
        }
        if let Some(action) = action {
            finish_action(ws, id, action);
        }
    }
    let mut i = 0;
    while i < ws.editor.colors.len() {
        if !ws.editor.colors[i].2.is_finished() {
            i += 1;
            continue;
        }
        let (id, rev, job) = ws.editor.colors.swap_remove(i);
        if let Ok(colors) = job.join() {
            if let Some(p) = ws
                .panes
                .iter_mut()
                .find(|p| p.id == id && p.document.as_ref().is_some_and(|d| d.revision() == rev))
            {
                p.colors = colors;
                p.color_revision = rev;
            }
        }
    }
    for p in &ws.panes {
        if let Some(doc) = &p.document {
            if p.color_revision != doc.revision()
                && !ws.editor.colors.iter().any(|(id, _, _)| *id == p.id)
            {
                let (path, text) = (p.path.clone(), doc.text().to_owned());
                if let Ok(job) = std::thread::Builder::new()
                    .name("concordia-file-colors".into())
                    .spawn(move || syntax::highlight(&path, &text))
                {
                    ws.editor.colors.push((p.id, doc.revision(), job));
                }
            }
        }
    }
    if let Some((_, job)) = &ws.editor.persistence {
        if job.is_finished() {
            let (sig, job) = ws.editor.persistence.take().unwrap();
            match job
                .join()
                .unwrap_or_else(|_| Err("file recovery writer failed".into()))
            {
                Ok(()) => {
                    ws.editor.persisted = sig;
                    ws.editor.error = None;
                }
                Err(e) => ws.editor.error = Some(e),
            }
        }
    }
    let current = signature(ws);
    if current != ws.editor.observed {
        ws.editor.observed = current.clone();
        ws.editor.changed = Some(Instant::now());
    }
    if ws.editor.pending.is_none()
        && ws.editor.persistence.is_none()
        && current != ws.editor.persisted
        && ws
            .editor
            .changed
            .is_some_and(|t| t.elapsed() >= Duration::from_millis(250))
    {
        if let Some(lease) = ws.editor.lease.clone() {
            match encoded(ws) {
                Ok(bytes) => match std::thread::Builder::new()
                    .name("concordia-file-recovery".into())
                    .spawn(move || lease.save_files(&bytes).map_err(|e| e.to_string()))
                {
                    Ok(job) => ws.editor.persistence = Some((current, job)),
                    Err(e) => ws.editor.error = Some(e.to_string()),
                },
                Err(e) => ws.editor.error = Some(e),
            }
        }
    }
}

pub(super) fn refresh_preview(p: &mut Pane) {
    let Some(doc) = &p.document else { return };
    let source = text::terminal_text(doc.text()).0;
    p.formatted = match p
        .path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("md" | "markdown" | "mdx") => Some(source),
        Some("json") => {
            serde_json::from_str::<serde_json::Value>(source.trim_start_matches('\u{feff}'))
                .ok()
                .and_then(|v| serde_json::to_string_pretty(&v).ok())
        }
        _ => None,
    };
}

pub(super) fn loaded(p: &mut Pane, snapshot: Snapshot) {
    p.stamp = Some(snapshot.stamp);
    p.changed = false;
    let mut matches = true;
    if p.restoring {
        if let Some(d) = &mut p.document {
            matches = d.text() == snapshot.text;
            if matches {
                d.acknowledge_save(snapshot.text.clone());
            } else if d.baseline() != snapshot.text {
                p.error = Some("file changed on disk; recovered edits retained".into());
                p.changed = true;
            }
        }
    } else {
        let mut d = crate::file_document::Document::new(snapshot.text);
        let cursor = p.initial_cursor.take().unwrap_or((p.scroll, p.column));
        d.move_to(cursor.0, cursor.1);
        p.document = Some(d);
    }
    p.initial_cursor = None;
    p.restoring = false;
    p.lines = snapshot.lines;
    p.horizontal_measure = None;
    p.colors = if matches { snapshot.colors } else { Vec::new() };
    p.color_revision = if matches {
        p.document.as_ref().map_or(0, |d| d.revision())
    } else {
        u64::MAX
    };
    refresh_preview(p);
    if p.formatted.is_none() {
        p.formatted_mode = false;
    }
    p.scroll = p.scroll.min(
        p.document
            .as_ref()
            .map_or(p.lines.len(), |d| d.line_count())
            .saturating_sub(p.area.height.saturating_sub(3).max(1) as usize),
    );
}
fn follow(p: &mut Pane) {
    let Some(doc) = &p.document else { return };
    let (row, col) = doc.position();
    let height = p
        .area
        .height
        .saturating_sub(3 + u16::from(!p.horizontal_bar.is_empty()))
        .max(1) as usize;
    let width = p.area.width.saturating_sub(8).max(1) as usize;
    if row < p.scroll {
        p.scroll = row;
    } else if row >= p.scroll + height {
        p.scroll = row + 1 - height;
    }
    if col < p.column {
        p.column = col;
    } else if col >= p.column + width {
        p.column = col + 1 - width;
    }
}

pub(super) fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    let Some(id) = app.workspace.focused else {
        return false;
    };
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let mutating = matches!(
        key.code,
        KeyCode::Backspace | KeyCode::Delete | KeyCode::Enter | KeyCode::Tab
    ) || matches!(key.code, KeyCode::Char(_))
        && !alt
        && (!ctrl
            || matches!(
                key.code,
                KeyCode::Char('z' | 'Z' | 'y' | 'Y' | 'x' | 'X' | 'v' | 'V')
            ));
    if app.job.as_ref().is_some_and(|j| j.kind == JobKind::Update) && mutating {
        app.flash("file edits are paused while the update finishes");
        return true;
    }
    if let Some(action) = app
        .workspace
        .panes
        .iter()
        .find(|p| p.id == id)
        .and_then(|p| p.prompt)
    {
        match key.code {
            KeyCode::Esc => {
                app.workspace
                    .panes
                    .iter_mut()
                    .find(|p| p.id == id)
                    .unwrap()
                    .prompt = None;
            }
            KeyCode::Char('s' | 'S') => start_save(&mut app.workspace, id, &app.state_root),
            KeyCode::Char('d' | 'D')
                if !app
                    .workspace
                    .panes
                    .iter()
                    .find(|p| p.id == id)
                    .unwrap()
                    .saving =>
            {
                let p = app.workspace.panes.iter_mut().find(|p| p.id == id).unwrap();
                p.prompt = None;
                if let Some(d) = &mut p.document {
                    *d = crate::file_document::Document::new(d.baseline().to_owned());
                }
                finish_action(&mut app.workspace, id, action);
                if let Err(e) = app.workspace.flush_file_recovery() {
                    app.flash(e);
                }
            }
            _ => {}
        }
        return true;
    }
    if matches!(key.code, KeyCode::Char('v' | 'V')) && alt {
        if let Some(p) = app.workspace.panes.iter_mut().find(|p| p.id == id) {
            p.toggle_mode();
        }
        return true;
    }
    if ctrl && matches!(key.code, KeyCode::Char('s' | 'S')) {
        start_save(&mut app.workspace, id, &app.state_root);
        return true;
    }
    if ctrl && matches!(key.code, KeyCode::Char('r' | 'R')) {
        if let Err(e) = app.workspace.reload(id) {
            app.flash(e);
        }
        return true;
    }
    let Some(p) = app.workspace.panes.iter_mut().find(|p| p.id == id) else {
        return false;
    };
    if p.searching || !p.raw() || p.loading {
        return false;
    }
    if key.code == KeyCode::F(3) && !p.query.is_empty() {
        if let Some(d) = &p.document {
            p.lines = display_lines(d.text()).unwrap_or_default();
        }
        p.find(!shift, false);
        return true;
    }
    if ctrl && matches!(key.code, KeyCode::Char('f' | 'F')) {
        if let Some(d) = &p.document {
            p.lines = display_lines(d.text()).unwrap_or_default();
        }
        p.query.clear();
        p.searching = true;
        return true;
    }
    if key.code == KeyCode::Esc {
        return false;
    }
    let Some(d) = &mut p.document else {
        return true;
    };
    if ctrl && matches!(key.code, KeyCode::Char('c' | 'C')) {
        if let Some(text) = d.selected_text().map(str::to_owned) {
            app.write_clipboard(&text);
            return true;
        }
        return false;
    }
    if ctrl && matches!(key.code, KeyCode::Char('v' | 'V')) {
        app.paste_clipboard();
        return true;
    }
    if key.code == KeyCode::F(6) {
        return false;
    }
    #[cfg(not(test))]
    if app.workspace.editor.lease.is_none() {
        app.flash("file recovery unavailable; fix local storage before editing");
        return true;
    }
    let p = app.workspace.panes.iter_mut().find(|p| p.id == id).unwrap();
    let d = p.document.as_mut().unwrap();
    let mut result = Ok(());
    let mut cut_chars = None;
    match key.code {
        KeyCode::Left => {
            d.begin_selection(shift);
            d.horizontal(false);
        }
        KeyCode::Right => {
            d.begin_selection(shift);
            d.horizontal(true);
        }
        KeyCode::Up => {
            d.begin_selection(shift);
            d.vertical(false);
        }
        KeyCode::Down => {
            d.begin_selection(shift);
            d.vertical(true);
        }
        KeyCode::Home => {
            d.begin_selection(shift);
            if ctrl {
                d.move_to(0, 0);
            } else {
                d.line_end(false);
            }
        }
        KeyCode::End => {
            d.begin_selection(shift);
            if ctrl {
                d.move_to(d.line_count() - 1, usize::MAX);
            } else {
                d.line_end(true);
            }
        }
        KeyCode::PageUp => {
            d.begin_selection(shift);
            let (r, c) = d.position();
            d.move_to(
                r.saturating_sub(p.area.height.saturating_sub(3).max(1) as usize),
                c,
            );
        }
        KeyCode::PageDown => {
            d.begin_selection(shift);
            let (r, c) = d.position();
            d.move_to(
                r.saturating_add(p.area.height.saturating_sub(3).max(1) as usize),
                c,
            );
        }
        KeyCode::Backspace => result = d.delete(false),
        KeyCode::Delete => result = d.delete(true),
        KeyCode::Enter => result = d.newline(),
        KeyCode::Tab => result = d.insert("\t"),
        KeyCode::Char('a' | 'A') if ctrl => d.select_all(),
        KeyCode::Char('z' | 'Z') if ctrl => d.undo(shift),
        KeyCode::Char('y' | 'Y') if ctrl => d.undo(true),
        KeyCode::Char('x' | 'X') if ctrl => {
            if let Some(text) = d.selected_text().map(str::to_owned) {
                result = clipboard::set_text(&text)
                    .map_err(|e| e.to_string())
                    .and_then(|_| d.delete(false));
                cut_chars = Some(text.chars().count());
            }
        }
        KeyCode::Char(c) if !ctrl && !alt => result = d.insert(&c.to_string()),
        _ => {}
    }
    if let Err(e) = result {
        app.flash(e);
    } else {
        follow(app.workspace.panes.iter_mut().find(|p| p.id == id).unwrap());
        if let Some(chars) = cut_chars {
            app.flash(format!("cut {chars} chars"));
        }
    }
    true
}

pub(super) fn paste(app: &mut App, text: &str) -> bool {
    let Some(id) = app.workspace.focused else {
        return false;
    };
    if app.job.as_ref().is_some_and(|j| j.kind == JobKind::Update) {
        app.flash("file edits are paused while the update finishes");
        return true;
    }
    let Some(p) = app.workspace.panes.iter_mut().find(|p| p.id == id) else {
        return false;
    };
    if !p.raw() || p.loading || p.prompt.is_some() {
        app.flash("formatted view is read-only; switch to Raw to edit");
        return true;
    }
    #[cfg(not(test))]
    if app.workspace.editor.lease.is_none() {
        app.flash("file recovery unavailable; editing paused");
        return true;
    }
    let p = app.workspace.panes.iter_mut().find(|p| p.id == id).unwrap();
    if let Some(d) = &mut p.document {
        if let Err(e) = d.insert(text) {
            app.flash(e);
        } else {
            follow(p);
        }
    }
    true
}

pub(super) fn mouse(app: &mut App, event: &MouseEvent) -> bool {
    let state = &mut app.workspace;
    if state.resizing.is_some() {
        if matches!(event.kind, MouseEventKind::Up(_)) {
            for p in &mut state.panes {
                p.selecting = false;
            }
        }
        return false;
    }
    let hit = state.panes.iter().position(|p| {
        p.raw()
            && !p.loading
            && p.document.is_some()
            && p.prompt.is_none()
            && contains(p.area, event.column, event.row)
            && event.row >= p.area.y + 2
            && event.row
                < p.area
                    .bottom()
                    .saturating_sub(1 + u16::from(!p.horizontal_bar.is_empty()))
    });
    if let Some(i) = hit {
        let p = &mut state.panes[i];
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                app.queue.focused = false;
                app.deleg.selected = None;
                state.focused = Some(p.id);
                p.selecting = true;
                if let Some(d) = &mut p.document {
                    d.begin_selection(event.modifiers.contains(KeyModifiers::SHIFT));
                    d.move_to(
                        p.scroll + (event.row - p.area.y - 2) as usize,
                        p.column + event.column.saturating_sub(p.area.x + 6) as usize,
                    );
                }
                app.drag = Drag::None;
                return true;
            }
            MouseEventKind::Drag(MouseButton::Left) if p.selecting => {
                if let Some(d) = &mut p.document {
                    d.begin_selection(true);
                    d.move_to(
                        p.scroll + (event.row - p.area.y - 2) as usize,
                        p.column + event.column.saturating_sub(p.area.x + 6) as usize,
                    );
                }
                return true;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                p.selecting = false;
                return true;
            }
            _ => {}
        }
    }
    if matches!(event.kind, MouseEventKind::Up(_)) {
        for p in &mut state.panes {
            p.selecting = false;
        }
    }
    false
}
