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
}
enum Reply {
    Summary(SearchSummary),
    Verified(Result<VerifiedHit, String>),
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
        self.pending = false;
        self.message = "Query changed; Enter starts a fresh search".into();
    }

    pub fn paste(&mut self, text: &str) {
        if !self.active {
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
        match key.code {
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
            .title(" Project content search · read only ");
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
        frame.render_widget(
            Paragraph::new(format!(
                "Literal [{}]: {}",
                if self.case_sensitive {
                    "case sensitive"
                } else {
                    "Unicode lowercase"
                },
                clean(&self.query)
            )),
            parts[1],
        );
        frame.render_widget(Paragraph::new(clean(&self.message)), parts[2]);
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
                        if i == self.selected { ">" } else { " " },
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
                "Enter search/open · ↑/↓ choose · Tab case · Esc close (chat draft preserved)",
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
        assert!(matches!(
            state.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Action::Close
        ));
    }
}
