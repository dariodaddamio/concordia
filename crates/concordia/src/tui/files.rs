//! Explicit file navigation. Launchers are reaped away from the input/render loop.
use super::*;
use crate::file_links;
#[cfg(not(test))]
use crate::file_open;
#[cfg(not(test))]
use std::ffi::OsString;

impl App {
    pub(super) fn open_web_link(&mut self, url: String) {
        if !markdown::web_target(&url) {
            self.flash("unsupported link destination");
            return;
        }
        self.open_host_target(url, "link");
    }

    fn open_host_target(&mut self, target: String, label: &'static str) {
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
                let result = tokio::process::Command::new("explorer.exe")
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
        #[cfg(test)]
        {
            self.last_file_open = Some(file.clone());
        }
        if file
            .path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| {
                matches!(
                    e.to_ascii_lowercase().as_str(),
                    "gif" | "png" | "jpg" | "jpeg" | "webp"
                )
            })
        {
            let path = file.path.to_string_lossy();
            self.open_host_target(
                path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned(),
                "image",
            );
            return;
        }
        let path = file.path.clone();
        if let Err(error) = self.workspace.open(file) {
            self.flash(error);
        } else {
            self.related_files.remember(&path);
            if let Some(id) = self.workspace.focused.filter(|_| preview) {
                self.workspace.preview(id, preview);
            }
        }
    }

    fn launch_external_file_candidate(&mut self, candidate: file_links::Candidate) {
        let file = match file_links::resolve(&candidate, &self.cwd) {
            Ok(file) => file,
            Err(error) => {
                self.flash(error);
                return;
            }
        };
        #[cfg(test)]
        {
            self.last_file_open = Some(file);
        }
        #[cfg(not(test))]
        {
            if self.file_jobs.len() >= 8 {
                self.flash("file launchers are still starting; try again shortly");
                return;
            }
            let plan = match file_plan(&file, &self.cwd, &self.state_root, false) {
                Ok(plan) => plan,
                Err(error) => {
                    self.flash(error);
                    return;
                }
            };
            let (tx, rx) = mpsc::unbounded_channel();
            self.file_jobs.push(rx);
            self.flash("opening file in another terminal editor");
            tokio::spawn(async move {
                let message = match file_open::spawn(plan).await {
                    Ok(child) => match child.wait_with_output().await {
                        Ok(output) if output.status.success() => "file terminal opened".into(),
                        Ok(output) => format!(
                            "file terminal failed: {}",
                            String::from_utf8_lossy(&output.stderr)
                                .chars()
                                .take(400)
                                .collect::<String>()
                        ),
                        Err(error) => format!("file terminal failed: {error}"),
                    },
                    Err(error) => error,
                };
                let _ = tx.send(message);
            });
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
        let mut candidates = file_links::candidates(reference);
        if let Some(explicit) = file_links::explicit_reference(reference) {
            candidates.push(explicit);
        }
        candidates.sort_by_key(|c| std::cmp::Reverse(c.range.len()));
        if let Some(candidate) = candidates
            .into_iter()
            .filter(|c| c.range.start == 0 || c.markdown)
            .find(|c| file_links::resolve(c, &self.cwd).is_ok())
        {
            if external {
                self.launch_external_file_candidate(candidate);
            } else {
                self.open_file_candidate(candidate, preview);
            }
        } else {
            self.push_system("file not found; use /open path/to/file or /preview path/to/file.md");
        }
    }

    pub(super) fn poll_file_jobs(&mut self) {
        let mut messages = Vec::new();
        self.file_jobs.retain_mut(|rx| {
            while let Ok(message) = rx.try_recv() {
                messages.push(message);
            }
            !rx.is_closed()
        });
        for message in messages {
            self.flash(message);
        }
    }
}

#[cfg(not(test))]
fn file_plan(
    file: &file_links::ResolvedFile,
    cwd: &Path,
    state_root: &Path,
    preview: bool,
) -> Result<file_open::LaunchPlan, String> {
    let host = file_open::host_from_environment(
        crate::host::wezterm::find_wezterm(),
        std::env::var_os("WEZTERM_PANE"),
    )?;
    if preview {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        return file_open::preview_plan(
            &host,
            &[exe.into_os_string(), OsString::from("preview")],
            &file.path,
            cwd,
        );
    }
    let config = crate::config::Config::load(state_root).config.editor;
    let editor = if config.program.is_empty() {
        let program = ["nvim", "hx", "vim"].iter().find_map(|name| which::which(name).ok())
            .ok_or("no terminal editor found; configure [editor].program and args in config.toml (or use /preview)")?;
        let name = program
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name == "hx" {
            file_open::EditorSpec::new(
                program,
                [format!(
                    "{{file}}:{}:{}",
                    file.line.unwrap_or(1),
                    file.column.unwrap_or(1)
                )],
            )
        } else {
            file_open::EditorSpec::new(program, ["+{line}", "--", "{file}"])
        }
    } else {
        file_open::EditorSpec::new(config.program, config.args)
    };
    Ok(file_open::editor_plan(&host, &editor, file, cwd))
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

    pub(super) fn poll_project_search(&mut self) {
        if let Some(action) = self
            .project_search
            .poll(&self.cwd, self.deleg.d.conversation_id())
        {
            self.apply_search_action(action);
        }
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
                let path = verified.hit.path.clone();
                match self.workspace.open_verified(verified) {
                    Ok(()) => {
                        self.related_files.remember(&path);
                        self.project_search.close();
                    }
                    Err(error) => self.project_search.notice(&error),
                }
            }
            _ => {}
        }
    }
}
