//! Shared file choices; launcher work stays off the input/render loop.
use super::*;
use crate::file_open::{self, EditorSpec, FileAction, ResolvedAction};

impl App {
    pub(in crate::tui) fn show_file_actions(
        &mut self,
        candidate: file_links::Candidate,
        base: PathBuf,
    ) {
        let file = match file_links::resolve(&candidate, &base) {
            Ok(file) => file,
            Err(error) => {
                self.flash(error);
                return;
            }
        };
        let dirty = self
            .workspace
            .replacement_blockers()
            .iter()
            .any(|path| path == &file.path);
        let mut rows = Vec::new();
        for (index, action) in [FileAction::Preview, FileAction::Split, FileAction::Editor]
            .into_iter()
            .enumerate()
        {
            rows.push(PickerRow {
                id: format!("action:{index}"),
                label: action.label().into(),
                detail: if dirty && action != FileAction::Preview {
                    "Opens the disk version; your unsaved pane is retained".into()
                } else {
                    file.path.to_string_lossy().into_owned()
                },
            });
        }
        for (index, action) in [FileAction::Preview, FileAction::Split, FileAction::Editor]
            .into_iter()
            .enumerate()
        {
            rows.push(PickerRow {
                id: format!("default:{index}"),
                label: format!("Always {}", action.label().to_lowercase()),
                detail: "Use this action for future file links and browser selections".into(),
            });
        }
        self.file_action_target = Some((file, base));
        self.show_picker(PickerKind::FileActions, "File actions".into(), rows, false);
    }
    pub(in crate::tui) fn choose_file_action(&mut self, id: &str) {
        let Some((file, base)) = self.file_action_target.take() else {
            return;
        };
        let Some((kind, index)) = id.split_once(':') else {
            return;
        };
        let action = match index {
            "0" => FileAction::Preview,
            "1" => FileAction::Split,
            "2" => FileAction::Editor,
            _ => return,
        };
        if kind == "default" {
            if let Err(error) = crate::store::set_file_action(&self.state_root, action) {
                self.flash(error.to_string());
                return;
            }
        }
        self.perform_file_action(file, action, base, None);
    }
    pub(in crate::tui) fn default_file_action(&self) -> FileAction {
        crate::store::get_settings(&self.state_root)
            .file_action
            .unwrap_or_else(|| {
                crate::config::Config::load(&self.state_root)
                    .config
                    .ui
                    .file_action
            })
    }
    pub(in crate::tui) fn perform_file_action(
        &mut self,
        file: file_links::ResolvedFile,
        action: FileAction,
        base: PathBuf,
        verified: Option<crate::workspace_search::VerifiedHit>,
    ) -> bool {
        let host = file_open::host_from_environment(
            crate::host::wezterm::find_wezterm(),
            std::env::var_os("WEZTERM_PANE"),
        )
        .ok();
        let config = crate::config::Config::load(&self.state_root).config.editor;
        let editor = if action == FileAction::Editor {
            if config.program.is_empty() && config.launch == file_open::EditorLaunch::Gui {
                None
            } else if config.program.is_empty() {
                ["nvim", "hx", "vim"]
                    .iter()
                    .find_map(|name| which::which(name).ok())
                    .map(|program| {
                        if program.file_stem().is_some_and(|name| name == "hx") {
                            EditorSpec::new(program, ["{file}:{line}:{column}"])
                        } else {
                            EditorSpec::new(program, ["+{line}", "--", "{file}"])
                        }
                    })
            } else {
                Some(EditorSpec::new(&config.program, &config.args))
            }
        } else {
            None
        };
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => {
                self.flash(error.to_string());
                return false;
            }
        };
        let resolved = match file_open::resolve_action(
            action,
            &file,
            &base,
            host.as_ref(),
            editor.as_ref().map(|editor| (editor, config.launch)),
            &exe,
        ) {
            Ok(action) => action,
            Err(error) => {
                self.flash(error);
                return false;
            }
        };
        match resolved {
            ResolvedAction::Preview(file) => {
                #[cfg(test)]
                {
                    self.last_file_open = Some(file.clone());
                }
                if file.path.extension().is_some_and(|extension| {
                    matches!(
                        extension.to_string_lossy().to_ascii_lowercase().as_str(),
                        "gif" | "png" | "jpg" | "jpeg" | "webp"
                    )
                }) {
                    self.start_raster(file.path, None);
                    return true;
                }
                let path = file.path.clone();
                let result = if let Some(verified) = verified {
                    self.workspace.open_verified(verified)
                } else {
                    self.workspace.open(file)
                };
                match result {
                    Ok(()) => {
                        self.related_files.remember(&path);
                        true
                    }
                    Err(error) => {
                        self.flash(error);
                        false
                    }
                }
            }
            ResolvedAction::Launch { plan, detached } => {
                #[cfg(test)]
                {
                    let _ = (plan, detached);
                    self.last_file_open = Some(file);
                    true
                }
                #[cfg(not(test))]
                {
                    if self.file_jobs.len() >= 8 {
                        self.flash("File launchers are busy; try shortly");
                        return false;
                    }
                    let (tx, rx) = mpsc::unbounded_channel();
                    self.file_jobs.push(rx);
                    self.flash(if action == FileAction::Split {
                        "Opening independent file editor"
                    } else {
                        "Opening editor on disk version; internal edits retained"
                    });
                    tokio::spawn(async move {
                        let message = launch(plan, detached).await.unwrap_or_else(|error| error);
                        let _ = tx.send(message);
                    });
                    true
                }
            }
        }
    }
}

#[cfg(not(test))]
async fn launch(plan: file_open::LaunchPlan, detached: bool) -> Result<String, String> {
    use tokio::io::AsyncReadExt;
    let mut command = tokio::process::Command::new(&plan.program);
    crate::process_ui::background(&mut command);
    command
        .args(plan.args)
        .current_dir(plan.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(if detached {
            std::process::Stdio::null()
        } else {
            std::process::Stdio::piped()
        })
        .kill_on_drop(!detached);
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start editor: {error}"))?;
    if detached {
        drop(child);
        return Ok("Editor started; saves belong to its independent window".into());
    }
    let pid = child.id().unwrap_or(0);
    let stderr = child.stderr.take();
    let mut error_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        if let Some(stderr) = stderr {
            let _ = stderr.take(8193).read_to_end(&mut bytes).await;
        }
        String::from_utf8_lossy(&bytes)
            .chars()
            .take(2000)
            .collect::<String>()
    });
    match tokio::time::timeout(Duration::from_secs(12), child.wait()).await {
        Ok(Ok(status)) if status.success() => {
            error_task.abort();
            Ok("File editor terminal opened".into())
        }
        Ok(Ok(status)) => {
            let detail = tokio::time::timeout(Duration::from_millis(500), &mut error_task)
                .await
                .ok()
                .and_then(|result| result.ok())
                .unwrap_or_default();
            error_task.abort();
            Err(format!("Editor launcher exited {status}: {detail}"))
        }
        result => {
            crate::provider::turn::kill_process_tree(pid);
            let _ = child.kill().await;
            let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
            error_task.abort();
            Err(format!("Editor launcher did not finish: {result:?}"))
        }
    }
}
