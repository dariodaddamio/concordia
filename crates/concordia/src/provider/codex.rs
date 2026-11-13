use crate::provider::stream_cards::{
    StreamCard, StreamPart, parse_codex_stream_chunk, upsert_card,
};
use crate::store::TrustMode;
use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct TurnRequest<'a> {
    pub prompt: &'a str,
    pub cwd: &'a Path,
    pub codex_home: Option<&'a Path>,
    pub model: Option<&'a str>,
    pub native_session_id: Option<&'a str>,
    pub trust: TrustMode,
    pub standing_context: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct TurnPlan {
    pub command: String,
    pub argv: Vec<String>,
    pub cwd: String,
}

#[derive(Debug, Default, Clone)]
pub struct StreamOutcome {
    pub text: String,
    pub session_id: Option<String>,
    pub cards: Vec<StreamCard>,
    pub errors: Vec<String>,
}

pub fn find_codex_bin() -> Result<String, String> {
    super::locate_real(super::ProviderKind::Codex)
        .map(|p| p.path.to_string_lossy().into_owned())
        .map_err(|e| crate::doctor::format_not_found_turn_error(super::ProviderKind::Codex, &e))
}

pub fn plan_turn_argv(req: &TurnRequest<'_>) -> Result<TurnPlan> {
    plan_turn_argv_with(
        req,
        &super::LocateEnv::from_process(super::ProviderKind::Codex),
    )
}

pub fn plan_turn_argv_with(req: &TurnRequest<'_>, env: &super::LocateEnv) -> Result<TurnPlan> {
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
        TrustMode::Edit => {
            argv.push("--skip-git-repo-check".into());
            argv.push("--approve-for-me".into());
        }
        TrustMode::Full => {
            argv.push("--skip-git-repo-check".into());
            argv.push("--dangerously-bypass-approvals-and-sandbox".into());
        }
    }
    if let Some(context) = req.standing_context.filter(|text| !text.is_empty()) {
        argv.push("-c".into());
        argv.push(format!("developer_instructions={}", serde_json::to_string(context)?));
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
    Ok(TurnPlan {
        command: super::locate(super::ProviderKind::Codex, env)
            .map(|p| p.path.to_string_lossy().into_owned())
            .map_err(|e| crate::doctor::format_not_found_turn_error(super::ProviderKind::Codex, &e))
            .map_err(anyhow::Error::msg)?,
        argv,
        cwd: req.cwd.to_string_lossy().into_owned(),
    })
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
