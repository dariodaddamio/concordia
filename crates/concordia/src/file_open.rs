//! Argument-array planning for opening an explicitly selected file in a separate WezTerm tab or
//! window. This module never evaluates path or model text as a command.

use crate::file_links::ResolvedFile;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tokio::process::{Child, Command};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSpec {
    /// Executable path/name is one argv element, never a shell command string.
    pub program: PathBuf,
    /// Optional argv templates. `{file}`, `{line}`, and `{column}` expand inside one argument.
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
pub enum Host {
    /// The current WezTerm pane, used to create a new tab in the same window.
    WezTermPane { wezterm: PathBuf, pane_id: OsString },
    /// No current pane: start a separate WezTerm GUI window.
    Standalone { wezterm: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
}

/// Build a plan to launch the configured editor. If args contain no `{file}` placeholder, the
/// resolved file path is appended as its own argument. Position placeholders expand to 1 when the
/// reference did not specify a line or column.
pub fn editor_plan(
    host: &Host,
    editor: &EditorSpec,
    file: &ResolvedFile,
    cwd: &Path,
) -> LaunchPlan {
    let mut editor_argv = vec![editor.program.as_os_str().to_owned()];
    let mut has_file = false;
    for arg in &editor.args {
        let source = arg.to_string_lossy();
        if source.contains("{file}") {
            has_file = true;
        }
        let expanded = expand_template(
            &source,
            &file.path.to_string_lossy(),
            &file.line.unwrap_or(1).to_string(),
            &file.column.unwrap_or(1).to_string(),
        );
        editor_argv.push(OsString::from(expanded));
    }
    if !has_file {
        editor_argv.push(file.path.as_os_str().to_owned());
    }
    host_plan(host, cwd, editor_argv)
}

/// Build a plan for Concordia's bounded Markdown preview command. `preview_argv` must contain the
/// executable first and any fixed arguments after it; the selected path is always appended alone.
pub fn preview_plan(
    host: &Host,
    preview_argv: &[OsString],
    file: &Path,
    cwd: &Path,
) -> Result<LaunchPlan, String> {
    if preview_argv.is_empty() {
        return Err("Markdown preview command is unavailable".into());
    }
    let mut argv = preview_argv.to_vec();
    argv.push(file.as_os_str().to_owned());
    Ok(host_plan(host, cwd, argv))
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
        match needle {
            "{file}" => out.push_str(file),
            "{line}" => out.push_str(line),
            "{column}" => out.push_str(column),
            _ => unreachable!(),
        }
        rest = &rest[at + needle.len()..];
    }
    out
}

fn host_plan(host: &Host, cwd: &Path, command: Vec<OsString>) -> LaunchPlan {
    match host {
        Host::WezTermPane { wezterm, pane_id } => {
            let mut args = vec![
                "cli".into(),
                "spawn".into(),
                "--pane-id".into(),
                pane_id.clone(),
                "--cwd".into(),
                cwd.as_os_str().to_owned(),
                "--".into(),
            ];
            args.extend(command);
            LaunchPlan {
                program: wezterm.clone(),
                args,
                cwd: cwd.to_path_buf(),
            }
        }
        Host::Standalone { wezterm } => {
            let mut args = vec![
                "start".into(),
                "--cwd".into(),
                cwd.as_os_str().to_owned(),
                "--".into(),
            ];
            args.extend(command);
            LaunchPlan {
                program: wezterm.clone(),
                args,
                cwd: cwd.to_path_buf(),
            }
        }
    }
}

/// Detect whether the app has enough information to launch an editor without guessing a shell
/// command. The caller owns config diagnostics and decides when to call this after a click.
pub fn host_from_environment(
    wezterm: Option<PathBuf>,
    pane_id: Option<OsString>,
) -> Result<Host, String> {
    let wezterm = wezterm.ok_or_else(|| {
        "WezTerm was not found; install it to open files in a separate terminal".to_string()
    })?;
    Ok(match pane_id {
        Some(pane_id) => Host::WezTermPane { wezterm, pane_id },
        None => Host::Standalone { wezterm },
    })
}

/// Launch an already-built plan without waiting for the child process to exit.
pub async fn spawn(plan: LaunchPlan) -> Result<Child, String> {
    Command::new(&plan.program)
        .args(&plan.args)
        .current_dir(&plan.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", plan.program.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> ResolvedFile {
        ResolvedFile {
            path: PathBuf::from(r"C:\work\folder with spaces\résumé;$(echo bad).rs"),
            line: Some(8),
            column: Some(3),
        }
    }

    #[test]
    fn editor_argv_expands_line_position_without_shell_interpretation() {
        let host = Host::WezTermPane {
            wezterm: PathBuf::from(r"C:\WezTerm\wezterm.exe"),
            pane_id: "7".into(),
        };
        let editor = EditorSpec::new(r"C:\Editors\nvim.exe", ["+{line}", "{file}"]);
        let plan = editor_plan(&host, &editor, &file(), Path::new(r"C:\work"));
        assert_eq!(plan.program, PathBuf::from(r"C:\WezTerm\wezterm.exe"));
        assert_eq!(
            plan.args,
            [
                OsString::from("cli"),
                OsString::from("spawn"),
                OsString::from("--pane-id"),
                OsString::from("7"),
                OsString::from("--cwd"),
                OsString::from(r"C:\work"),
                OsString::from("--"),
                OsString::from(r"C:\Editors\nvim.exe"),
                OsString::from("+8"),
                OsString::from(r"C:\work\folder with spaces\résumé;$(echo bad).rs"),
            ]
        );
        assert_eq!(
            plan.args
                .iter()
                .filter(|a| a.to_string_lossy().contains("$(echo bad)"))
                .count(),
            1
        );
    }

    #[test]
    fn placeholder_text_inside_the_selected_path_is_preserved() {
        let mut file = file();
        file.path = PathBuf::from(r"C:\work\{line}\{column}\{file}.md");
        let host = Host::Standalone {
            wezterm: "wezterm".into(),
        };
        let editor = EditorSpec::new("nvim", ["+{line}", "{file}"]);
        let plan = editor_plan(&host, &editor, &file, Path::new(r"C:\work"));
        assert_eq!(
            plan.args[6],
            OsString::from(r"C:\work\{line}\{column}\{file}.md")
        );
    }

    #[test]
    fn no_file_placeholder_appends_path_as_one_argument_in_new_window() {
        let host = Host::Standalone {
            wezterm: "wezterm".into(),
        };
        let editor = EditorSpec::new("hx", ["--"]);
        let plan = editor_plan(&host, &editor, &file(), Path::new(r"C:\work"));
        assert_eq!(
            &plan.args[..5],
            ["start", "--cwd", r"C:\work", "--", "hx"].map(OsString::from)
        );
        assert_eq!(plan.args.last().unwrap(), &file().path.into_os_string());
    }

    #[test]
    fn preview_argv_is_passed_as_separate_arguments() {
        let host = Host::WezTermPane {
            wezterm: "wezterm".into(),
            pane_id: "3".into(),
        };
        let args = vec![
            OsString::from(r"C:\app\concordia.exe"),
            OsString::from("preview"),
        ];
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("guide.md");
        std::fs::write(&md, "# Guide").unwrap();
        let plan = preview_plan(&host, &args, &md, Path::new("C:/work")).unwrap();
        assert_eq!(plan.program, PathBuf::from("wezterm"));
        assert_eq!(
            &plan.args[7..],
            [
                OsString::from(r"C:\app\concordia.exe"),
                OsString::from("preview"),
                md.into_os_string()
            ]
        );
    }

    #[test]
    fn preview_requires_command_and_missing_host_is_visible() {
        assert!(preview_plan(
            &Host::Standalone {
                wezterm: "wezterm".into()
            },
            &[],
            Path::new("a.md"),
            Path::new(".")
        )
        .unwrap_err()
        .contains("unavailable"));
        assert!(host_from_environment(None, None)
            .unwrap_err()
            .contains("WezTerm"));
    }
}
