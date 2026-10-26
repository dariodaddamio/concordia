//! Explicit file navigation. Launchers are reaped away from the input/render loop.
use super::*;
use crate::file_links;
#[cfg(not(test))]
use crate::file_open;
#[cfg(not(test))]
use std::ffi::OsString;

impl App {
    pub(super) fn open_file_candidate(&mut self, candidate: file_links::Candidate, preview: bool) {
        let _ = preview; // Both navigation routes initially use the internal read-only view.
        let file = match file_links::resolve(&candidate, &self.cwd) {
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
        if let Err(error) = self.workspace.open(file) {
            self.flash(error);
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

    pub(super) fn file_command(&mut self, reference: &str, external: bool) {
        let reference = reference.trim().trim_matches(['"', '`']);
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
                self.open_file_candidate(candidate, false);
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
