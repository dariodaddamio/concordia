//! Argument-array planning for opening an explicitly selected file in the user's editor.

use crate::file_links::ResolvedFile;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tokio::process::Child;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileAction {
    #[default]
    Preview,
    Editor,
}
impl FileAction {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "preview" => Some(Self::Preview),
            "editor" => Some(Self::Editor),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Preview => "Preview here",
            Self::Editor => "Open in editor",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EditorLaunch {
    #[default]
    Gui,
}

#[derive(Debug)]
pub enum ResolvedAction {
    Preview(ResolvedFile),
    Launch { plan: LaunchPlan, detached: bool },
}

pub fn resolve_action(
    action: FileAction,
    file: &ResolvedFile,
    cwd: &Path,
    editor: Option<(&EditorSpec, EditorLaunch)>,
) -> Result<ResolvedAction, String> {
    if crate::workspace_paths::sensitive(&file.path) {
        return Err("File is excluded by workspace policy".into());
    }
    let metadata = std::fs::symlink_metadata(&file.path).map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || crate::workspaces::linked(&metadata)
        || file
            .path
            .canonicalize()
            .map_err(|error| error.to_string())?
            != file.path
    {
        return Err("Selected file changed identity; select it again".into());
    }
    match action {
        FileAction::Preview => Ok(ResolvedAction::Preview(file.clone())),
        FileAction::Editor => {
            let (editor, kind) =
                editor.ok_or("Configure a GUI editor executable before opening this file")?;
            if kind != EditorLaunch::Gui {
                return Err("Configure the editor to launch as a GUI application".into());
            }
            let program = which::which(&editor.program)
                .map_err(|_| "Configured editor executable was not found")?;
            if crate::provider::launch::is_batch_shim(&program.to_string_lossy()) {
                return Err("Select the editor's native executable; batch/shell editor launchers are unsupported".into());
            }
            let editor = EditorSpec {
                program,
                args: editor.args.clone(),
            };
            Ok(ResolvedAction::Launch {
                plan: editor_plan(&editor, file, cwd),
                detached: true,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}
impl EditorSpec {
    pub fn new(
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
}
pub fn editor_plan(editor: &EditorSpec, file: &ResolvedFile, cwd: &Path) -> LaunchPlan {
    LaunchPlan {
        program: editor.program.clone(),
        args: editor_arguments(editor, file),
        cwd: cwd.to_owned(),
    }
}

fn editor_arguments(editor: &EditorSpec, file: &ResolvedFile) -> Vec<OsString> {
    let mut args = Vec::new();
    let mut has_file = false;
    for arg in &editor.args {
        let source = arg.to_string_lossy();
        has_file |= source.contains("{file}");
        args.push(OsString::from(expand_template(
            &source,
            &file.path.to_string_lossy(),
            &file.line.unwrap_or(1).to_string(),
            &file.column.unwrap_or(1).to_string(),
        )));
    }
    if !has_file {
        args.push(file.path.as_os_str().to_owned());
    }
    args
}
fn expand_template(template: &str, file: &str, line: &str, column: &str) -> String {
    let mut out = String::with_capacity(template.len() + file.len());
    let mut rest = template;
    while !rest.is_empty() {
        let next = ["{file}", "{line}", "{column}"]
            .iter()
            .filter_map(|needle| rest.find(needle).map(|at| (at, *needle)))
            .min_by_key(|(at, _)| *at);
        let Some((at, needle)) = next else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..at]);
        out.push_str(match needle {
            "{file}" => file,
            "{line}" => line,
            "{column}" => column,
            _ => unreachable!(),
        });
        rest = &rest[at + needle.len()..];
    }
    out
}

pub async fn spawn(plan: LaunchPlan) -> Result<Child, String> {
    crate::process_ui::tokio_command(&plan.program)
        .args(&plan.args)
        .current_dir(&plan.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", plan.program.display()))
}
