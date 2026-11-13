use crate::store::TrustMode;
use anyhow::{bail, Result};

#[derive(Debug, Clone)]
pub struct ClaudeTurnRequest<'a> {
    pub prompt: &'a str,
    pub cwd: &'a std::path::Path,
    pub config_dir: Option<&'a std::path::Path>,
    pub model: Option<&'a str>,
    pub native_session_id: Option<&'a str>,
    pub trust: TrustMode,
    pub standing_context: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct ClaudePlan {
    pub command: String,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: String,
    /// Session id chosen for a conversation's first turn (`--session-id`); recorded by the caller
    /// once that turn succeeds so later turns can `--resume` it.
    pub new_session_id: Option<String>,
}

const PROFILE_VARS: &[&str] = &[
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
];

/// Forward only the documented Claude routing profile. Values come from the user's process
/// environment, with the selected Claude settings file as a fallback. Secrets are not read.
fn profile_env(config_dir: Option<&std::path::Path>) -> Vec<(String, String)> {
    let settings = config_dir
        .and_then(|dir| crate::fs_safe::read_text_bounded(&dir.join("settings.json"), 1024 * 1024))
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    PROFILE_VARS
        .iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    settings
                        .as_ref()
                        .and_then(|value| value.get("env"))
                        .and_then(|env| env.get(*name))
                        .and_then(|value| value.as_str())
                        .map(str::to_owned)
                })
                .map(|value| ((*name).to_owned(), value))
        })
        .collect()
}

pub fn find_claude_bin() -> Result<String, String> {
    super::locate_real(super::ProviderKind::Claude)
        .map(|p| p.path.to_string_lossy().into_owned())
        .map_err(|e| crate::doctor::format_not_found_turn_error(super::ProviderKind::Claude, &e))
}

pub fn plan_claude_turn(req: &ClaudeTurnRequest<'_>) -> Result<ClaudePlan> {
    plan_claude_turn_with(
        req,
        &super::LocateEnv::from_process(super::ProviderKind::Claude),
    )
}

pub fn plan_claude_turn_with(
    req: &ClaudeTurnRequest<'_>,
    locate_env: &super::LocateEnv,
) -> Result<ClaudePlan> {
    let mut new_session_id = None;
    let mut argv = vec![
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
    ];
    if let Some(model) = req.model.map(str::trim).filter(|m| !m.is_empty()) {
        argv.push("--model".into());
        argv.push(model.to_string());
    }
    match req.trust {
        TrustMode::Plan => {
            argv.push("--permission-mode".into());
            argv.push("plan".into());
        }
        TrustMode::Ask => {
            // Claude 2.1.319 help accepts manual; this is its prompting mode for Ask.
            argv.push("--permission-mode".into());
            argv.push("manual".into());
        }
        TrustMode::Edit => {
            argv.push("--permission-mode".into());
            argv.push("acceptEdits".into());
        }
        TrustMode::Full => {
            argv.push("--dangerously-skip-permissions".into());
        }
    }
    if let Some(context) = req.standing_context.filter(|text| !text.is_empty()) {
        argv.push("--append-system-prompt".into());
        argv.push(context.to_owned());
    }
    if let Some(sid) = req
        .native_session_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        argv.push("--resume".into());
        argv.push(sid.to_string());
    } else {
        // First turn of a conversation: pick the id ourselves so we never have to parse it out of
        // the output, and can resume it explicitly next time.
        let id = uuid::Uuid::new_v4().to_string();
        argv.push("--session-id".into());
        argv.push(id.clone());
        new_session_id = Some(id);
    }
    argv.push(req.prompt.to_string());
    assert_safe_claude_argv(&argv[..argv.len() - 1], req.trust == TrustMode::Full)?;
    let mut env = Vec::new();
    if let Some(home) = req.config_dir {
        env.push((
            "CLAUDE_CONFIG_DIR".into(),
            home.to_string_lossy().into_owned(),
        ));
    }
    env.extend(profile_env(req.config_dir));
    Ok(ClaudePlan {
        command: super::locate(super::ProviderKind::Claude, locate_env)
            .map(|p| p.path.to_string_lossy().into_owned())
            .map_err(|e| {
                crate::doctor::format_not_found_turn_error(super::ProviderKind::Claude, &e)
            })
            .map_err(anyhow::Error::msg)?,
        argv,
        env,
        cwd: req.cwd.to_string_lossy().into_owned(),
        new_session_id,
    })
}

pub fn assert_safe_claude_argv(argv: &[String], allow_bypass: bool) -> Result<()> {
    if argv.iter().any(|a| a == "--continue" || a == "-c") {
        bail!("refusing Claude --continue/-c; bind an explicit session id");
    }
    if argv.iter().any(|a| a == "--last") {
        bail!("refusing --last");
    }
    if let Some(i) = argv.iter().position(|a| a == "--resume" || a == "-r") {
        let value = argv.get(i + 1);
        if value.map(|v| v.starts_with('-')).unwrap_or(true) {
            bail!("refusing Claude --resume without an explicit session id");
        }
    }
    if !allow_bypass {
        for token in argv {
            if token == "--dangerously-skip-permissions"
                || token == "--allow-dangerously-skip-permissions"
            {
                bail!("refusing Claude {token}; set trust=full to opt in");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_reads_only_routing_variables_from_settings() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("settings.json"), r#"{"env":{"ANTHROPIC_BASE_URL":"https://api.example.invalid","ANTHROPIC_DEFAULT_SONNET_MODEL":"deepseek-chat","ANTHROPIC_API_KEY":"secret"}}"#).unwrap();
        let vars = profile_env(Some(root.path()));
        assert!(vars.contains(&(
            "ANTHROPIC_BASE_URL".into(),
            "https://api.example.invalid".into()
        )));
        assert!(vars.contains(&(
            "ANTHROPIC_DEFAULT_SONNET_MODEL".into(),
            "deepseek-chat".into()
        )));
        assert!(!vars.iter().any(|(name, _)| name == "ANTHROPIC_API_KEY"));
    }
}
