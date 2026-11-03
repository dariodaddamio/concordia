//! A search editor with its own draft and bounded asynchronous scan/verification jobs.
use super::*;
use crate::workspace_search::{self as search, SearchRequest, SearchSummary, VerifiedHit};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static ACTIVE_JOBS: AtomicUsize = AtomicUsize::new(0);

struct Permit;
impl Permit {
    fn acquire() -> Option<Self> {
        let mut active = ACTIVE_JOBS.load(Ordering::Acquire);
        loop {
            if active >= 2 {
                return None;
            }
            match ACTIVE_JOBS.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(Self),
                Err(current) => active = current,
            }
        }
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        ACTIVE_JOBS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) enum Action {
    None,
    Close,
    Changed,
    Open(VerifiedHit),
    ApplyReplacement(search::replacement::Plan),
    ReconcileReplacement(String),
    ListReceipts,
    ReplacementFinished(search::replacement::Receipt),
}
enum Reply {
    Summary(SearchSummary),
    Verified(Result<VerifiedHit, String>),
    Prepared(Result<search::replacement::Plan, String>),
    Replaced(Result<search::replacement::Receipt, String>),
    Receipts(Result<Vec<(String, String)>, String>),
}
struct Job {
    generation: u64,
    handle: std::thread::JoinHandle<Reply>,
}

#[derive(Default)]
pub(super) struct State {
    active: bool,
    root: PathBuf,
    owner_cwd: PathBuf,
    conversation: String,
    generation: u64,
    query: String,
    case_sensitive: bool,
    selected: usize,
    summary: Option<SearchSummary>,
    jobs: Vec<Job>,
    cancel: Arc<AtomicBool>,
    pending: bool,
    message: String,
    marked: std::collections::BTreeSet<usize>,
    replacement: Option<String>,
    plan: Option<search::replacement::Plan>,
    preview_scroll: usize,
    receipt_id: Option<String>,
    receipt_report: Option<String>,
    receipts: Vec<(String, String)>,
    receipt_selected: usize,
}

impl Drop for State {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl State {
    pub fn active(&self) -> bool {
        self.active
    }

    pub fn notice(&mut self, message: &str) {
        self.message = message.chars().take(1024).collect();
    }

    pub fn open(&mut self, cwd: &Path, conversation: &str) -> Result<(), String> {
        let request = SearchRequest::new(cwd, conversation, self.generation, "validation", true)?;
        self.close();
        self.root = request.root;
        self.owner_cwd = cwd.to_path_buf();
        self.conversation = conversation.into();
        self.query.clear();
        self.case_sensitive = true;
        self.active = true;
        self.message = "Type a literal, then Enter to search. Tab toggles case.".into();
        Ok(())
    }

    pub fn close(&mut self) {
        self.invalidate();
        self.active = false;
        self.summary = None;
        self.query.clear();
        self.message.clear();
        self.conversation.clear();
        self.root.clear();
        self.owner_cwd.clear();
    }

    fn invalidate(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.generation = self.generation.wrapping_add(1);
        self.summary = None;
        self.selected = 0;
        self.marked.clear();
        self.replacement = None;
        self.plan = None;
        self.receipt_id = None;
        self.receipt_report = None;
        self.receipts.clear();
        self.receipt_selected = 0;
        self.preview_scroll = 0;
        self.pending = false;
        self.message = "Query changed; Enter starts a fresh search".into();
    }

    pub fn paste(&mut self, text: &str) {
        if !self.active {
            return;
        }
        if let Some(value) = self.receipt_id.as_mut().or(self.replacement.as_mut()) {
            if self.plan.is_none() && !self.pending {
                for c in text.chars().filter(|c| !c.is_control()) {
                    if value.len() + c.len_utf8() > 4096 {
                        break;
                    }
                    value.push(c);
                }
            }
            return;
        }
        self.invalidate();
        for c in text.chars().filter(|c| !c.is_control()) {
            if self.query.len() + c.len_utf8() > search::MAX_QUERY_BYTES {
                break;
            }
            self.query.push(c);
        }
        self.message = "Enter searches · Tab toggles case · Esc closes".into();
    }

    pub fn key(&mut self, key: KeyEvent) -> Action {
        if !self.active || key.kind == KeyEventKind::Release {
            return Action::None;
        }
        if let Some(id) = self.receipt_id.as_mut() {
            match key.code {
                KeyCode::Esc => self.receipt_id = None,
                KeyCode::Enter if !self.pending => {
                    let selected = if id.is_empty() {
                        self.receipts
                            .get(self.receipt_selected)
                            .map(|row| row.0.clone())
                            .unwrap_or_default()
                    } else {
                        id.clone()
                    };
                    return Action::ReconcileReplacement(selected);
                }
                KeyCode::Up => self.receipt_selected = self.receipt_selected.saturating_sub(1),
                KeyCode::Down => {
                    self.receipt_selected =
                        (self.receipt_selected + 1).min(self.receipts.len().saturating_sub(1))
                }
                KeyCode::Backspace => {
                    id.pop();
                }
                KeyCode::Char(c) if (c.is_ascii_hexdigit() || c == '-') && id.len() < 36 => {
                    id.push(c);
                }
                _ => {}
            }
            return Action::Changed;
        }
        if self.replacement.is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.cancel.store(true, Ordering::Relaxed);
                    self.cancel = Arc::new(AtomicBool::new(false));
                    self.generation = self.generation.wrapping_add(1);
                    self.pending = false;
                    self.plan = None;
                    self.replacement = None;
                    self.message = "Replacement dismissed; search results retained".into();
                }
                KeyCode::F(6) | KeyCode::Enter
                    if !self.pending
                        && (key.code == KeyCode::F(6)
                            || key.modifiers.contains(KeyModifiers::CONTROL)) =>
                {
                    if let Some(plan) = self.plan.take() {
                        return Action::ApplyReplacement(plan);
                    }
                }
                KeyCode::Enter if !self.pending && self.plan.is_none() => {
                    self.prepare_replacement()
                }
                KeyCode::Up if self.plan.is_some() => {
                    self.preview_scroll = self.preview_scroll.saturating_sub(1)
                }
                KeyCode::Down if self.plan.is_some() => {
                    self.preview_scroll = self.preview_scroll.saturating_add(1)
                }
                KeyCode::PageUp if self.plan.is_some() => {
                    self.preview_scroll = self.preview_scroll.saturating_sub(10)
                }
                KeyCode::PageDown if self.plan.is_some() => {
                    self.preview_scroll = self.preview_scroll.saturating_add(10)
                }
                KeyCode::Backspace if !self.pending && self.plan.is_none() => {
                    if let Some(value) = self.replacement.as_mut() {
                        value.pop();
                    }
                }
                KeyCode::Char(c)
                    if !self.pending
                        && self.plan.is_none()
                        && !c.is_control()
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    if let Some(value) = self.replacement.as_mut() {
                        if value.len() + c.len_utf8() <= 4096 {
                            value.push(c);
                        }
                    }
                }
                _ => {}
            }
            return Action::Changed;
        }
        if self.receipt_report.is_some() {
            match key.code {
                KeyCode::Up => {
                    self.preview_scroll = self.preview_scroll.saturating_sub(1);
                    return Action::Changed;
                }
                KeyCode::Down => {
                    self.preview_scroll = self.preview_scroll.saturating_add(1);
                    return Action::Changed;
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::F(2) | KeyCode::Char('r')
                if !self.pending
                    && (key.code == KeyCode::F(2)
                        || key.modifiers.contains(KeyModifiers::CONTROL)) =>
            {
                if self.marked.is_empty() {
                    self.message = "Space marks selected matches before replacement".into();
                } else {
                    self.replacement = Some(String::new());
                    self.message="Type replacement (empty deletes); Enter previews, Ctrl+Enter applies that preview".into();
                }
            }
            KeyCode::F(3) | KeyCode::Char('h')
                if !self.pending
                    && (key.code == KeyCode::F(3)
                        || key.modifiers.contains(KeyModifiers::CONTROL)) =>
            {
                self.receipt_id = Some(String::new());
                self.message =
                    "Choose a replacement receipt; Enter compares current hashes without writes."
                        .into();
                return Action::ListReceipts;
            }
            KeyCode::Char(' ') if !self.pending && self.summary.is_some() => {
                if self.marked.contains(&self.selected) {
                    self.marked.remove(&self.selected);
                } else {
                    self.marked.insert(self.selected);
                }
                self.message = format!(
                    "{} matches selected for replacement · Ctrl+R edits replacement",
                    self.marked.len()
                );
            }
            KeyCode::Esc => {
                self.close();
                return Action::Close;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.close();
                return Action::Close;
            }
            KeyCode::Enter if !self.pending => self.submit(),
            KeyCode::Tab => {
                self.invalidate();
                self.case_sensitive = !self.case_sensitive;
                self.message = "Case setting changed; Enter starts a fresh search".into();
            }
            KeyCode::Backspace => {
                self.invalidate();
                self.query.pop();
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down => {
                let max = self
                    .summary
                    .as_ref()
                    .map_or(0, |s| s.hits.len().saturating_sub(1));
                self.selected = (self.selected + 1).min(max);
            }
            KeyCode::PageUp => {
                self.selected = self.selected.saturating_sub(10);
            }
            KeyCode::PageDown => {
                let max = self
                    .summary
                    .as_ref()
                    .map_or(0, |s| s.hits.len().saturating_sub(1));
                self.selected = self.selected.saturating_add(10).min(max);
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && !c.is_control()
                    && self.query.len() + c.len_utf8() <= search::MAX_QUERY_BYTES =>
            {
                self.invalidate();
                self.query.push(c);
            }
            _ => {}
        }
        Action::Changed
    }

    fn submit(&mut self) {
        self.reap_stale();
        if self.jobs.len() >= 2 {
            self.message = "Previous searches are stopping; try again shortly".into();
            return;
        }
        let Some(permit) = Permit::acquire() else {
            self.message = "Two search jobs are finishing; try again shortly".into();
            return;
        };
        let request = match SearchRequest::new(
            &self.root,
            &self.conversation,
            self.generation,
            &self.query,
            self.case_sensitive,
        ) {
            Ok(request) => request,
            Err(error) => {
                self.message = error;
                return;
            }
        };
        let hit = self
            .summary
            .as_ref()
            .and_then(|s| s.hits.get(self.selected))
            .cloned();
        let cancel = self.cancel.clone();
        let generation = self.generation;
        let verifying = hit.is_some();
        let job = std::thread::Builder::new()
            .name("concordia-project-search".into())
            .spawn(move || {
                let _permit = permit;
                if let Some(hit) = hit {
                    Reply::Verified(search::verify_hit(&request, &hit, &cancel))
                } else {
                    Reply::Summary(search::search(request, &cancel))
                }
            });
        match job {
            Ok(handle) => {
                self.jobs.push(Job { generation, handle });
                self.pending = true;
                self.message = if verifying {
                    "Verifying exact source bytes…"
                } else {
                    "Searching workspace…"
                }
                .into();
            }
            Err(_) => self.message = "Search worker could not start".into(),
        }
    }
    fn prepare_replacement(&mut self) {
        let Some(mut summary) = self.summary.clone() else {
            return;
        };
        summary.request.generation = self.generation;
        let selected = self.marked.iter().copied().collect::<Vec<_>>();
        let replacement = self.replacement.clone().unwrap_or_default();
        let cancel = self.cancel.clone();
        self.start_job("Preparing exact replacement preview…", move || {
            Reply::Prepared(search::replacement::stage(
                &summary,
                &selected,
                &replacement,
                &cancel,
            ))
        });
    }
    fn start_job(&mut self, message: &str, work: impl FnOnce() -> Reply + Send + 'static) {
        self.reap_stale();
        if self.jobs.len() >= 2 {
            self.message = "Previous operations are stopping; try shortly".into();
            return;
        }
        let Some(permit) = Permit::acquire() else {
            self.message = "Workspace jobs are busy; try shortly".into();
            return;
        };
        match std::thread::Builder::new()
            .name("concordia-project-replacement".into())
            .spawn(move || {
                let _permit = permit;
                work()
            }) {
            Ok(handle) => {
                self.jobs.push(Job {
                    generation: self.generation,
                    handle,
                });
                self.pending = true;
                self.message = message.into();
            }
            Err(error) => self.message = error.to_string(),
        }
    }
    pub fn apply_replacement(
        &mut self,
        state: PathBuf,
        plan: search::replacement::Plan,
        blocked: Vec<PathBuf>,
    ) {
        if plan.conversation != self.conversation
            || plan.root != self.root
            || plan.generation != self.generation
        {
            self.message = "Replacement owner changed; preview again".into();
            return;
        }
        let cancel = self.cancel.clone();
        self.start_job(
            "Applying reviewed replacement; Esc stops before the next file",
            move || Reply::Replaced(search::replacement::apply(&state, plan, &blocked, &cancel)),
        );
    }
    pub fn reconcile_replacement(&mut self, state: PathBuf, id: String) {
        let cancel = self.cancel.clone();
        self.start_job("Comparing receipt hashes; no writes…", move || {
            Reply::Replaced(search::replacement::reconcile(&state, &id, &cancel))
        });
    }
    pub fn list_receipts(&mut self, state: PathBuf) {
        let root = self.root.clone();
        let cancel = self.cancel.clone();
        self.start_job("Loading replacement receipts…", move || {
            Reply::Receipts(search::replacement::list(&state, &root, &cancel))
        });
    }

    fn reap_stale(&mut self) {
        let mut i = 0;
        while i < self.jobs.len() {
            if self.jobs[i].generation != self.generation && self.jobs[i].handle.is_finished() {
                let job = self.jobs.swap_remove(i);
                let _ = job.handle.join();
            } else {
                i += 1;
            }
        }
    }

    /// Owner changes cancel before publishing. No result follows the currently focused chat.
    pub fn poll(&mut self, cwd: &Path, conversation: &str) -> Option<Action> {
        // Capture the app's spelling separately from the canonical scan root (including Windows
        // verbatim prefixes). Compare owner identity without filesystem reads in the event loop.
        if self.active && (conversation != self.conversation || cwd != self.owner_cwd) {
            self.close();
            self.reap_stale();
            return Some(Action::Close);
        }
        self.reap_stale();
        let i = self
            .jobs
            .iter()
            .position(|j| j.generation == self.generation && j.handle.is_finished())?;
        let job = self.jobs.swap_remove(i);
        if !self.active {
            let _ = job.handle.join();
            return None;
        }
        self.pending = false;
        match job.handle.join() {
            Ok(Reply::Summary(summary))
                if summary.request.conversation == self.conversation
                    && summary.request.root == self.root
                    && summary.request.generation == self.generation =>
            {
                self.message = match &summary.status {
                    search::Status::Complete => format!(
                        "Complete · {} matches · {} files · {} bytes · {} non-text files skipped",
                        summary.hits.len(),
                        summary.files_examined,
                        summary.bytes_read,
                        summary.skipped_nontext
                    ),
                    search::Status::Incomplete(reason) => format!(
                        "Incomplete: {reason} · {} matches · {} files",
                        summary.hits.len(),
                        summary.files_examined
                    ),
                    search::Status::Cancelled => "Search cancelled".into(),
                    search::Status::Error(error) => format!("Search failed: {error}"),
                };
                self.summary = Some(summary);
            }
            Ok(Reply::Verified(Ok(hit))) => return Some(Action::Open(hit)),
            Ok(Reply::Prepared(Ok(plan))) => {
                self.message = format!(
                    "Preview {} · F6 applies exactly these changes · Esc returns",
                    plan.id
                );
                self.preview_scroll = 0;
                self.plan = Some(plan);
            }
            Ok(Reply::Prepared(Err(error))) | Ok(Reply::Replaced(Err(error))) => {
                self.message = error
            }
            Ok(Reply::Receipts(result)) => match result {
                Ok(rows) => {
                    self.receipts = rows;
                    self.receipt_selected = 0;
                    self.message =
                        "↑/↓ choose a receipt · Enter compares hashes · Esc returns".into();
                }
                Err(error) => self.message = error,
            },
            Ok(Reply::Replaced(Ok(receipt))) => {
                self.message = receipt.summary();
                self.summary = None;
                self.marked.clear();
                self.replacement = None;
                self.receipt_id = None;
                self.receipt_report = Some(format!(
                    "{}\n{}",
                    self.message,
                    receipt
                        .files
                        .iter()
                        .map(|file| format!("{}: {}", file.path.display(), file.status))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
                return Some(Action::ReplacementFinished(receipt));
            }
            Ok(Reply::Verified(Err(error))) => {
                self.message = error;
                self.summary = None;
            }
            Ok(_) => {}
            Err(_) => self.message = "Search worker failed".into(),
        }
        Some(Action::Changed)
    }

    pub fn draw(&self, frame: &mut Frame<'_>, area: Rect) {
        if !self.active {
            return;
        }
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Project search · selected replacement ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let parts = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);
        let clean = |s: &str| text::terminal_text(s).0.replace('\n', " ");
        frame.render_widget(
            Paragraph::new(format!(
                "Root: {} · gitignore + credential/build exclusions",
                clean(&self.root.to_string_lossy())
            ))
            .style(Style::default().fg(Color::DarkGray)),
            parts[0],
        );
        let prefix = if self.receipt_id.is_some() {
            "Receipt UUID (or choose below): "
        } else if self.replacement.is_some() {
            "Replace with: "
        } else if self.case_sensitive {
            "Literal [case sensitive]: "
        } else {
            "Literal [Unicode lowercase]: "
        };
        let input = self
            .receipt_id
            .as_deref()
            .or(self.replacement.as_deref())
            .unwrap_or(&self.query);
        frame.render_widget(
            Paragraph::new(format!("{prefix}{}", clean(input))),
            parts[1],
        );
        if parts[1].width > 0 && parts[1].height > 0 {
            let column = prefix
                .chars()
                .chain(input.chars())
                .map(text::char_width)
                .sum::<usize>()
                .min(parts[1].width.saturating_sub(1) as usize) as u16;
            frame.set_cursor_position((parts[1].x + column, parts[1].y));
        }
        frame.render_widget(Paragraph::new(clean(&self.message)), parts[2]);
        if self.receipt_id.is_some() {
            let first = self
                .receipt_selected
                .saturating_sub(parts[3].height.saturating_sub(1) as usize);
            let rows = self
                .receipts
                .iter()
                .enumerate()
                .skip(first)
                .take(parts[3].height as usize)
                .map(|(index, (id, label))| {
                    Line::from(format!(
                        "{} {} · {}",
                        if index == self.receipt_selected {
                            ">"
                        } else {
                            " "
                        },
                        id,
                        clean(label)
                    ))
                })
                .collect::<Vec<_>>();
            frame.render_widget(Paragraph::new(rows), parts[3]);
            frame.render_widget(
                Paragraph::new("↑/↓ choose receipt · Enter inspect current hashes · Esc return"),
                parts[4],
            );
            return;
        }
        if let Some(report) = &self.receipt_report {
            frame.render_widget(
                Paragraph::new(
                    text::terminal_text(
                        &report
                            .lines()
                            .skip(
                                self.preview_scroll
                                    .min(report.lines().count().saturating_sub(1)),
                            )
                            .take(parts[3].height as usize)
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                    .0,
                )
                .wrap(ratatui::widgets::Wrap { trim: false }),
                parts[3],
            );
            frame.render_widget(
                Paragraph::new("↑/↓ scroll · F3 receipts · type a new search · Esc close"),
                parts[4],
            );
            return;
        }
        if let Some(plan) = &self.plan {
            let lines = plan
                .preview
                .lines()
                .skip(
                    self.preview_scroll
                        .min(plan.preview.lines().count().saturating_sub(1)),
                )
                .take(parts[3].height as usize)
                .map(|line| {
                    Line::styled(
                        text::terminal_text(line).0,
                        transcript::diff_line_style(line),
                    )
                })
                .collect::<Vec<_>>();
            frame.render_widget(Paragraph::new(lines), parts[3]);
            frame.render_widget(
                Paragraph::new("↑/↓ scroll · F6 apply this preview · Esc return"),
                parts[4],
            );
            return;
        }
        let visible = parts[3].height as usize / 2;
        let first = self.selected.saturating_sub(visible.saturating_sub(1));
        let mut lines = Vec::new();
        if let Some(summary) = &self.summary {
            for (i, hit) in summary.hits.iter().enumerate().skip(first).take(visible) {
                let label = hit
                    .path
                    .strip_prefix(&self.root)
                    .unwrap_or(&hit.path)
                    .to_string_lossy();
                let style = if i == self.selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                lines.push(Line::styled(
                    format!(
                        "{} {}:{}:{}",
                        if self.marked.contains(&i) {
                            "✓"
                        } else if i == self.selected {
                            ">"
                        } else {
                            " "
                        },
                        clean(&label),
                        hit.line,
                        hit.column
                    ),
                    style,
                ));
                lines.push(Line::styled(
                    format!("  {}", clean(&hit.preview)),
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
        frame.render_widget(Paragraph::new(lines), parts[3]);
        frame.render_widget(
            Paragraph::new(
                "Enter search/open · Space select · F2 replace · F3 receipts · Tab case · Esc close",
            )
            .style(Style::default().fg(Color::DarkGray)),
            parts[4],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_change_and_closing_reject_late_results() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "needle").unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let mut state = State::default();
        state.open(&cwd, "first-conversation").unwrap();
        state.paste("needle");
        state.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(
            state.poll(&cwd, "second-conversation"),
            Some(Action::Close)
        ));
        assert!(!state.active());
        assert!(state.summary.is_none());
        assert!(state.query.is_empty());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !state.jobs.is_empty() {
            state.poll(&cwd, "second-conversation");
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(state.summary.is_none());
    }
    #[test]
    fn search_input_is_independent_bounded_and_cancels_on_case_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = State::default();
        state.open(dir.path(), "conversation").unwrap();
        state.paste(&"é".repeat(2000));
        assert!(state.query.len() <= search::MAX_QUERY_BYTES);
        let old = state.cancel.clone();
        let generation = state.generation;
        state.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(old.load(Ordering::Relaxed));
        assert!(state.generation > generation);
        assert!(!state.case_sensitive);
        std::fs::write(dir.path().join("notes.txt"), "needle needle").unwrap();
        state.invalidate();
        state.query = "needle".into();
        state.summary = Some(search::search(
            SearchRequest::new(
                &state.root,
                &state.conversation,
                state.generation,
                "needle",
                true,
            )
            .unwrap(),
            &AtomicBool::new(false),
        ));
        state.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        state.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        state.paste("replacement");
        state.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let deadline = Instant::now() + Duration::from_secs(5);
        while state.pending {
            state.poll(dir.path(), "conversation");
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(state.replacement.as_deref(), Some("replacement"));
        assert!(state
            .plan
            .as_ref()
            .unwrap()
            .preview
            .contains("-needle\n+replacement"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
            "needle needle"
        );
        assert!(matches!(
            state.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
            Action::ApplyReplacement(_)
        ));
        state.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(
            state.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Action::Close
        ));
    }
}
