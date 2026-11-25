use crate::provider::turn::PlannedTurn;
use crate::store::ProviderKind;
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

const BATCH_ARGV_MAX: usize = 4000;
const EXE_ARGV_MAX: usize = 24_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub stdin: Option<String>,
}

pub fn is_batch_shim(program: &str) -> bool {
    let p = program.to_ascii_lowercase();
    p.ends_with(".cmd") || p.ends_with(".bat")
}

fn batch_safe(prompt: &str) -> bool {
    prompt.len() <= BATCH_ARGV_MAX
        && prompt
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " .,:;_/-?'".contains(c))
}

pub fn prompt_needs_stdin(prompt: &str, batch: bool) -> bool {
    prompt.starts_with('-') || prompt.len() > EXE_ARGV_MAX || (batch && !batch_safe(prompt))
}

pub(crate) fn cursor_ps1_beside(shim: &Path) -> Option<PathBuf> {
    let ps1 = shim.parent()?.join("cursor-agent.ps1");
    ps1.is_file().then_some(ps1)
}

fn cursor_powershell(launch: &mut Launch, shim: &Path, script: &Path) {
    let name = shim
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "agent.cmd".into());
    let mut args = vec![
        "-NoProfile".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-File".into(),
        script.to_string_lossy().into_owned(),
    ];
    args.append(&mut launch.args);
    launch.program = "powershell.exe".into();
    launch.args = args;
    launch.env.push(("CURSOR_INVOKED_AS".into(), name));
}

pub fn prepare(plan: &PlannedTurn) -> Result<Launch> {
    let batch = is_batch_shim(&plan.command);
    let mut launch = Launch {
        program: plan.command.clone(),
        args: plan.argv.clone(),
        env: Vec::new(),
        stdin: None,
    };
    if plan.provider == ProviderKind::Codex {
        if let Some(native) = super::npm::codex(&plan.command) {
            launch.program = native.program.to_string_lossy().into_owned();
            launch.env = native.env;
        }
    }
    if is_batch_shim(&launch.program) {
        if let Some((node, script)) = super::npm::node_bridge(&launch.program, plan.provider) {
            launch.program = node.to_string_lossy().into_owned();
            launch.args.insert(0, script.to_string_lossy().into_owned());
        }
    }
    if cfg!(windows)
        && plan.provider == ProviderKind::Cursor
        && is_batch_shim(&launch.program)
        && launch
            .args
            .iter()
            .take(launch.args.len().saturating_sub(1))
            .any(|argument| !batch_safe(argument))
    {
        let shim = Path::new(&plan.command);
        if let Some(script) = cursor_ps1_beside(shim) {
            cursor_powershell(&mut launch, shim, &script);
        }
    }
    if cfg!(windows)
        && is_batch_shim(&launch.program)
        && launch
            .args
            .iter()
            .take(launch.args.len().saturating_sub(1))
            .any(|argument| !batch_safe(argument))
    {
        bail!(
            "provider options cannot pass safely through this batch launcher; select the native CLI executable or a supported npm bridge"
        );
    }
    if !prompt_needs_stdin(&plan.prompt, batch) {
        return Ok(launch);
    }
    if launch.args.last().map(String::as_str) != Some(plan.prompt.as_str()) {
        bail!("internal: prompt is not the last argument");
    }
    match plan.provider {
        ProviderKind::Codex => {
            *launch.args.last_mut().expect("checked above") = "-".into();
            launch.stdin = Some(plan.prompt.clone());
        }
        ProviderKind::Claude => {
            launch.args.pop();
            launch.stdin = Some(plan.prompt.clone());
        }
        ProviderKind::Cursor => {
            if plan.prompt.starts_with('-') {
                bail!(
                    "this prompt starts with '-', which the Cursor CLI would read as an option; reword it"
                );
            }
            let shim = Path::new(&plan.command);
            match is_batch_shim(&launch.program).then(|| cursor_ps1_beside(shim)).flatten() {
                Some(ps1) => {
                    cursor_powershell(&mut launch, shim, &ps1);
                }
                None if is_batch_shim(&launch.program) => bail!(
                    "this prompt cannot be passed safely through {} (newlines or special characters); \
                     shorten or simplify it",
                    plan.command
                ),
                None => {}
            }
        }
        ProviderKind::Grok | ProviderKind::Opencode => {
            if plan.prompt.starts_with('-') {
                bail!(
                    "this prompt starts with '-', which Grok Build would read as an option; reword it"
                );
            }
            launch.stdin = Some(plan.prompt.clone());
            launch.args.pop();
        }
    }
    Ok(launch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(provider: ProviderKind, command: &str, prompt: &str) -> PlannedTurn {
        PlannedTurn {
            command: command.into(),
            argv: vec!["--flag".into(), prompt.into()],
            prompt: prompt.into(),
            new_session_id: None,
            env_remove: Vec::new(),
            env: Vec::new(),
            cwd: ".".into(),
            provider,
            standing_context: None,
        }
    }

    #[test]
    fn simple_prompts_keep_the_plain_argv_path_even_behind_a_shim() {
        for p in [
            "hello world",
            "fix the bug in src/main.rs, please?",
            "what's up",
        ] {
            let l = prepare(&plan(ProviderKind::Codex, r"C:\npm\codex.cmd", p)).unwrap();
            assert_eq!(l.stdin, None, "{p}");
            assert_eq!(l.args.last().unwrap(), p);
        }
    }

    #[test]
    fn unsafe_prompts_behind_a_batch_shim_go_over_stdin() {
        let unsafe_prompts = [
            "two\nlines",
            "100% done %PATH%",
            "a & b",
            "say \"hi\"",
            "caret ^ pipe |",
            "bang !",
            "日本語",
            "tab\tsep",
        ];
        for p in unsafe_prompts {
            let l = prepare(&plan(ProviderKind::Codex, r"C:\npm\codex.cmd", p)).unwrap();
            assert_eq!(l.stdin.as_deref(), Some(p), "{p:?}");
            assert_eq!(l.args, ["--flag", "-"], "{p:?}");
        }
        let long = "word ".repeat(1000);
        let l = prepare(&plan(ProviderKind::Codex, "codex.CMD", &long)).unwrap();
        assert_eq!(l.stdin.as_deref(), Some(long.as_str()));
    }

    #[test]
    fn a_real_exe_takes_special_characters_in_argv_unchanged() {
        let p = "two\nlines with % & \"quotes\" 日本語";
        let l = prepare(&plan(ProviderKind::Codex, r"C:\bin\codex.exe", p)).unwrap();
        assert_eq!(l.stdin, None);
        assert_eq!(l.args.last().unwrap(), p);
    }

    #[test]
    fn a_leading_dash_is_never_left_where_the_cli_could_read_it_as_a_flag() {
        for cmd in [r"C:\bin\codex.exe", r"C:\npm\codex.cmd"] {
            let l = prepare(&plan(
                ProviderKind::Codex,
                cmd,
                "--dangerously-bypass-approvals-and-sandbox",
            ))
            .unwrap();
            assert_eq!(l.args.last().unwrap(), "-");
            assert!(l.stdin.is_some());
        }
        let l = prepare(&plan(
            ProviderKind::Claude,
            r"C:\bin\claude.exe",
            "-p injected",
        ))
        .unwrap();
        assert_eq!(l.args, ["--flag"]);
        assert_eq!(l.stdin.as_deref(), Some("-p injected"));
    }

    #[test]
    fn a_huge_prompt_to_an_exe_goes_over_stdin() {
        let huge = "x".repeat(EXE_ARGV_MAX + 1);
        let l = prepare(&plan(ProviderKind::Claude, r"C:\bin\claude.exe", &huge)).unwrap();
        assert_eq!(l.stdin.as_deref(), Some(huge.as_str()));
        assert_eq!(l.args, ["--flag"]);
    }

    #[test]
    fn codex_resume_keeps_the_session_id_and_swaps_only_the_prompt() {
        let mut p = plan(ProviderKind::Codex, r"C:\npm\codex.cmd", "multi\nline");
        p.argv = vec![
            "exec".into(),
            "--json".into(),
            "resume".into(),
            "SID".into(),
            "multi\nline".into(),
        ];
        let l = prepare(&p).unwrap();
        assert_eq!(l.args, ["exec", "--json", "resume", "SID", "-"]);
    }

    #[test]
    fn cursor_without_a_beside_script_refuses_instead_of_corrupting_the_prompt() {
        let e = prepare(&plan(
            ProviderKind::Cursor,
            r"C:\nowhere\agent.cmd",
            "two\nlines",
        ))
        .unwrap_err();
        assert!(e.to_string().contains("cannot be passed safely"), "{e}");
        let e = prepare(&plan(ProviderKind::Cursor, r"C:\nowhere\agent.exe", "-x")).unwrap_err();
        assert!(e.to_string().contains("starts with '-'"), "{e}");
    }

    #[test]
    fn cursor_bypasses_the_shim_when_the_script_is_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cursor-agent.ps1"), "").unwrap();
        let shim = dir.path().join("agent.cmd");
        let l = prepare(&plan(
            ProviderKind::Cursor,
            &shim.to_string_lossy(),
            "two\nlines",
        ))
        .unwrap();
        assert_eq!(l.program, "powershell.exe");
        assert_eq!(
            &l.args[..4],
            ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]
        );
        assert!(l.args[4].ends_with("cursor-agent.ps1"));
        assert_eq!(l.args[5..], ["--flag", "two\nlines"]);
        assert_eq!(
            l.env,
            [("CURSOR_INVOKED_AS".to_string(), "agent.cmd".to_string())]
        );
        assert_eq!(l.stdin, None);
        let mut with_plugin = plan(ProviderKind::Cursor, &shim.to_string_lossy(), "two\nlines");
        with_plugin.argv.splice(
            1..1,
            [
                "--plugin-dir".into(),
                r"C:\Synthetic user\plugin & input".into(),
            ],
        );
        let launch = prepare(&with_plugin).unwrap();
        assert_eq!(launch.program, "powershell.exe");
        assert_eq!(launch.args.iter().filter(|arg| *arg == "-File").count(), 1);
        assert_eq!(&launch.args[5..], &with_plugin.argv);
    }
}
