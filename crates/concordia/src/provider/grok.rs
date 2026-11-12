use crate::store::{ProviderKind, TrustMode};
use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct GrokTurnRequest<'a> {
    pub prompt: &'a str,
    pub cwd: &'a Path,
    pub home_path: Option<&'a Path>,
    pub model: Option<&'a str>,
    pub native_session_id: Option<&'a str>,
    pub trust: TrustMode,
}

pub fn find_grok_bin() -> Result<String, String> {
    super::locate_real(ProviderKind::Grok)
        .map(|p| p.path.to_string_lossy().into_owned())
        .map_err(|e| crate::doctor::format_not_found_turn_error(ProviderKind::Grok, &e))
}

pub fn plan_grok_turn(req: &GrokTurnRequest<'_>) -> Result<super::cursor::CursorPlan> {
    let mut argv = vec!["agent".into(), "stdio".into()];
    if let Some(model) = req.model.map(str::trim).filter(|m| !m.is_empty()) {
        argv.extend(["--model".into(), model.to_owned()]);
    }
    if req.trust == TrustMode::Full {
        argv.push("--always-approve".into());
    }
    // ACP owns the exact resume request; the native session id is never put in a prompt.
    argv.push(req.prompt.to_owned());
    let mut env = Vec::new();
    if let Some(home) = req.home_path {
        env.push(("GROK_HOME".into(), home.to_string_lossy().into_owned()));
    }
    Ok(super::cursor::CursorPlan {
        command: find_grok_bin().map_err(anyhow::Error::msg)?,
        argv,
        env,
        env_remove: vec!["XAI_API_KEY".into()],
        cwd: req.cwd.to_string_lossy().into_owned(),
    })
}
