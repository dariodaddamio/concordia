//! Explicit file navigation. Launchers are reaped away from the input/render loop.
use super::*;
use crate::file_links;
#[path = "file_actions.rs"]
mod actions;

impl App {
    pub(super) fn open_web_link(&mut self, url: String) {
        if !markdown::web_target(&url) {
            self.flash("unsupported link destination");
            return;
        }
        self.open_host_target(url, "link");
    }

    pub(super) fn open_host_target(&mut self, target: String, label: &'static str) {
        #[cfg(test)]
        {
            self.flash(format!("opened {label} {target}"));
        }
        #[cfg(not(test))]
        {
            if self.file_jobs.len() >= 8 {
                self.flash("file launchers are busy");
                return;
            }
            let (tx, rx) = mpsc::unbounded_channel();
            self.file_jobs.push(rx);
            tokio::spawn(async move {
                // Explorer is intentionally visible: this is the requested URL/file window.
                let result = tokio::process::Command::new("explorer.exe") // visible-spawn: user-requested file/URL window.
                    .arg(target)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .await;
                let _ = tx.send(match result {
                    Ok(status) if status.success() => format!("{label} opened"),
                    Ok(status) => format!("{label} opener exited: {status}"),
                    Err(error) => format!("could not open {label}: {error}"),
                });
            });
        }
    }

    pub(super) fn scroll_input(&mut self, delta: isize) {
        let width = self.areas.input_text_w.max(1);
        let (row, col) = self.editor.cursor_pos(width);
        let height = self.areas.input.height.max(1) as usize;
        let max = self.editor.layout(width).len().saturating_sub(height);
        self.input_scroll = self.input_scroll.saturating_add_signed(delta).min(max);
        self.input_scroll_anchor = Some((self.editor.revision(), row, col, width));
    }

    pub(super) fn open_file_candidate(&mut self, candidate: file_links::Candidate, preview: bool) {
        self.open_file_candidate_from(candidate, preview, self.cwd.clone());
    }

    pub(super) fn open_file_candidate_from(
        &mut self,
        candidate: file_links::Candidate,
        preview: bool,
        base: PathBuf,
    ) {
        let file = match file_links::resolve(&candidate, &base) {
            Ok(file) => file,
            Err(error) => {
                self.flash(error);
                return;
            }
        };
        let action = self.default_file_action();
        self.perform_file_action(file, action, base, None);
        if action == crate::file_open::FileAction::Preview && preview {
            if let Some(id) = self.workspace.focused {
                self.workspace.preview(id, true);
            }
        }
    }

    fn launch_external_file_candidate(&mut self, candidate: file_links::Candidate) {
        match file_links::resolve(&candidate, &self.cwd) {
            Ok(file) => {
                self.perform_file_action(
                    file,
                    crate::file_open::FileAction::Editor,
                    self.cwd.clone(),
                    None,
                );
            }
            Err(error) => self.flash(error),
        }
    }
    pub(super) fn file_command(&mut self, reference: &str, external: bool, preview: bool) {
        let reference = reference.trim().trim_matches(['"', '`']);
        let reference = reference.strip_prefix('@').unwrap_or(reference);
        let path = Path::new(reference);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        };
        if path.is_dir() && !external {
            self.related_files.browse(path);
            self.request_related_files();
            self.build_related_files_picker();
            return;
        }
        if let Some(explicit) = file_links::explicit_reference(reference) {
            match file_links::resolve_explicit(&explicit, &self.cwd) {
                Ok(file) => {
                    if external {
                        self.perform_file_action(
                            file,
                            crate::file_open::FileAction::Editor,
                            self.cwd.clone(),
                            None,
                        );
                    } else {
                        self.perform_file_action(
                            file,
                            crate::file_open::FileAction::Preview,
                            self.cwd.clone(),
                            None,
                        );
                    }
                }
                Err(error) => self.flash(error),
            }
            return;
        }
        let mut candidates = file_links::candidates(reference);
        candidates.sort_by_key(|c| std::cmp::Reverse(c.range.len()));
        if let Some(candidate) = candidates
            .into_iter()
            .filter(|c| c.range.start == 0 || c.markdown)
            .find(|c| file_links::resolve(c, &self.cwd).is_ok())
        {
            if external {
                self.launch_external_file_candidate(candidate);
            } else {
                match file_links::resolve(&candidate, &self.cwd) {
                    Ok(file) => {
                        self.perform_file_action(
                            file,
                            crate::file_open::FileAction::Preview,
                            self.cwd.clone(),
                            None,
                        );
                        if preview {
                            if let Some(id) = self.workspace.focused {
                                self.workspace.preview(id, true);
                            }
                        }
                    }
                    Err(error) => self.flash(error),
                }
            }
        } else {
            self.push_system("file not found; use /open path/to/file");
        }
    }

    pub(super) fn poll_file_jobs(&mut self) -> bool {
        let mut messages = Vec::new();
        self.file_jobs.retain_mut(|rx| {
            while let Ok(message) = rx.try_recv() {
                messages.push(message);
            }
            !rx.is_closed()
        });
        let received = !messages.is_empty();
        for message in messages {
            self.flash(message);
        }
        received
    }
}

impl App {
    pub(super) fn open_project_search(&mut self) {
        match self
            .project_search
            .open(&self.cwd, self.deleg.d.conversation_id())
        {
            Ok(()) => {
                self.picker = None;
            }
            Err(error) => self.flash(error),
        }
    }

    pub(super) fn poll_project_search(&mut self) -> bool {
        if let Some(action) = self
            .project_search
            .poll(&self.cwd, self.deleg.d.conversation_id())
        {
            self.apply_search_action(action);
            return true;
        }
        false
    }

    pub(super) fn apply_search_action(&mut self, action: workspace_search::Action) {
        match action {
            workspace_search::Action::Close => self.project_search.close(),
            workspace_search::Action::ApplyReplacement(plan) => {
                let blocked = self.workspace.replacement_blockers();
                self.project_search
                    .apply_replacement(self.state_root.clone(), plan, blocked);
            }
            workspace_search::Action::ReconcileReplacement(id) => self
                .project_search
                .reconcile_replacement(self.state_root.clone(), id),
            workspace_search::Action::ListReceipts => {
                self.project_search.list_receipts(self.state_root.clone())
            }
            workspace_search::Action::ReplacementFinished(receipt) => {
                self.workspace.replacement_finished(&receipt);
            }
            workspace_search::Action::Open(verified) => {
                let file = file_links::ResolvedFile {
                    path: verified.hit.path.clone(),
                    line: Some(verified.hit.line),
                    column: Some(verified.hit.column),
                };
                if self.perform_file_action(
                    file,
                    self.default_file_action(),
                    self.cwd.clone(),
                    Some(verified),
                ) {
                    self.project_search.close();
                } else if let Some((message, _)) = &self.flash {
                    self.project_search.notice(message);
                }
            }
            _ => {}
        }
    }
}
