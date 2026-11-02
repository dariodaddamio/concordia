use crate::provider::stream_cards::{
    parse_codex_stream_chunk, upsert_card, StreamCard, StreamPart,
};
use crate::store::TrustMode;
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use which::which;

#[derive(Debug, Clone)]
pub struct TurnRequest<'a> {
    pub prompt: &'a str,
    pub cwd: &'a Path,
    pub codex_home: Option<&'a Path>,
    pub model: Option<&'a str>,
    pub native_session_id: Option<&'a str>,
    pub trust: TrustMode,
}

#[derive(Debug, Clone)]
pub struct TurnPlan {
    pub command: String,
    pub argv: Vec<String>,
    pub cwd: String,
}

#[derive(Debug, Default, Clone)]
pub struct StreamOutcome {
    pub exit_code: Option<i32>,
    pub text: String,
    pub session_id: Option<String>,
    pub cards: Vec<StreamCard>,
    pub errors: Vec<String>,
}

pub fn find_codex_bin() -> String {
    if let Some(p) = super::bin_override("CONCORDIA_CODEX_BIN") {
        return p;
    }
    which("codex")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "codex".into())
}

pub fn plan_turn_argv(req: &TurnRequest<'_>) -> TurnPlan {
    let mut argv = vec!["exec".into(), "--json".into()];
    if let Some(model) = req.model.map(str::trim).filter(|m| !m.is_empty()) {
        argv.push("-m".into());
        argv.push(model.to_string());
    }
    match req.trust {
        // Codex has no separate planning mode; a read-only sandbox is the closest guarantee.
        TrustMode::Plan | TrustMode::Ask => {
            argv.push("--skip-git-repo-check".into());
            argv.push("-s".into());
            argv.push("read-only".into());
        }
        TrustMode::Workspace => {
            argv.push("--skip-git-repo-check".into());
            argv.push("--approve-for-me".into());
        }
        TrustMode::Full => {
            argv.push("--skip-git-repo-check".into());
            argv.push("--dangerously-bypass-approvals-and-sandbox".into());
        }
    }
    if let Some(sid) = req
        .native_session_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        argv.push("resume".into());
        argv.push(sid.to_string());
    }
    argv.push(req.prompt.to_string());
    TurnPlan {
        command: find_codex_bin(),
        argv,
        cwd: req.cwd.to_string_lossy().into_owned(),
    }
}

pub fn assert_safe_argv(argv: &[String]) -> Result<()> {
    if argv.iter().any(|a| a == "--last" || a == "--continue") {
        anyhow::bail!("refusing --last/--continue; bind an explicit native session id");
    }
    Ok(())
}

/// Parse a multi-line Codex JSONL chunk (fixture / stream buffer).
pub fn extract_stream_chunk(chunk: &str) -> StreamOutcome {
    let mut out = StreamOutcome::default();
    apply_stream_parts(parse_codex_stream_chunk(chunk), &mut out);
    out
}

pub fn apply_stream_parts(parts: Vec<StreamPart>, out: &mut StreamOutcome) {
    for part in parts {
        match part {
            StreamPart::Session(id) => out.session_id = Some(id),
            StreamPart::Text(text) => {
                out.text.push_str(&text);
                if !text.ends_with('\n') {
                    out.text.push('\n');
                }
            }
            StreamPart::TextDelta(text) => out.text.push_str(&text),
            StreamPart::Card(card) => upsert_card(&mut out.cards, card),
            StreamPart::Thinking(_) | StreamPart::ThinkingDelta(_) | StreamPart::Usage(_) => {}
            // Shown once as a system line; never folded into the reply text.
            StreamPart::Error(msg) => out.errors.push(msg),
        }
    }
}

pub async fn run_turn(req: &TurnRequest<'_>) -> Result<StreamOutcome> {
    let plan = plan_turn_argv(req);
    assert_safe_argv(&plan.argv)?;
    let mut cmd = Command::new(&plan.command);
    crate::process_ui::background(&mut cmd);
    cmd.args(&plan.argv)
        .current_dir(&plan.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(home) = req.codex_home {
        cmd.env("CODEX_HOME", home);
    }
    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawn {} {:?}", plan.command, plan.argv))?;
    let stdout = child.stdout.take().context("codex stdout")?;
    let mut lines = BufReader::new(stdout).lines();
    let mut out = StreamOutcome::default();
    while let Some(line) = read_line(&mut lines).await? {
        apply_stream_parts(parse_codex_stream_chunk(&line), &mut out);
    }
    let status = child.wait().await.context("wait codex")?;
    out.exit_code = status.code();
    if out.text.is_empty() && status.code().unwrap_or(1) != 0 {
        out.text = format!("codex exited {}", status.code().unwrap_or(-1));
    }
    Ok(out)
}

async fn read_line(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
) -> Result<Option<String>> {
    lines.next_line().await.context("read codex stdout line")
}
