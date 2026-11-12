use crate::identity::cursor_api_key_for_home;
use crate::store::TrustMode;
use anyhow::{Result, bail};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct CursorTurnRequest<'a> {
    pub prompt: &'a str,
    pub cwd: &'a Path,
    pub home_path: Option<&'a Path>,
    pub model: Option<&'a str>,
    pub native_session_id: Option<&'a str>,
    pub trust: TrustMode,
}

#[derive(Debug, Clone)]
pub struct CursorPlan {
    pub command: String,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub cwd: String,
}

pub fn find_agent_bin() -> Result<String, String> {
    super::locate_real(super::ProviderKind::Cursor)
        .map(|p| p.path.to_string_lossy().into_owned())
        .map_err(|e| crate::doctor::format_not_found_turn_error(super::ProviderKind::Cursor, &e))
}

pub fn plan_cursor_turn(req: &CursorTurnRequest<'_>) -> Result<CursorPlan> {
    plan_cursor_turn_with(
        req,
        &super::LocateEnv::from_process(super::ProviderKind::Cursor),
    )
}

pub fn plan_cursor_turn_with(
    req: &CursorTurnRequest<'_>,
    locate_env: &super::LocateEnv,
) -> Result<CursorPlan> {
    let mut argv = vec![
        "--print".into(),
        "--output-format".into(),
        "stream-json".into(),
        // Reply text arrives as it is written instead of in one piece at the end.
        "--stream-partial-output".into(),
    ];
    if let Some(model) = req.model.map(str::trim).filter(|m| !m.is_empty()) {
        argv.push("--model".into());
        argv.push(model.to_string());
    }
    match req.trust {
        TrustMode::Full => {
            argv.push("--force".into());
        }
        TrustMode::Edit => {
            // Cursor agent 2026.10.22 trust table: Plan=`--mode plan`, Ask=default,
            // Workspace=default workspace sandbox, Full=`--force`. Its help only offers
            // `--sandbox enabled|disabled`; Workspace must never use `disabled`.
        }
        TrustMode::Plan => {
            argv.push("--mode".into());
            argv.push("plan".into());
        }
        TrustMode::Ask => {}
    }
    if let Some(sid) = req
        .native_session_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        argv.push("--resume".into());
        argv.push(sid.to_string());
    }
    argv.push(req.prompt.to_string());
    assert_safe_cursor_argv(&argv[..argv.len() - 1], req.trust == TrustMode::Full)?;
    let auth = account_auth(req.home_path)?;
    Ok(CursorPlan {
        command: super::locate(super::ProviderKind::Cursor, locate_env)
            .map(|p| p.path.to_string_lossy().into_owned())
            .map_err(|e| {
                crate::doctor::format_not_found_turn_error(super::ProviderKind::Cursor, &e)
            })
            .map_err(anyhow::Error::msg)?,
        argv,
        env: auth.env,
        env_remove: auth.env_remove,
        cwd: req.cwd.to_string_lossy().into_owned(),
    })
}

/// Authentication surfaces supported by the current native CLI. Custom OAuth homes
/// cannot be selected with an undocumented environment variable.
pub(crate) struct AccountAuth {
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
}

pub(crate) fn account_auth(home: Option<&Path>) -> Result<AccountAuth> {
    if let Ok(key) = std::env::var("CURSOR_API_KEY") {
        crate::redact::register_runtime_secret(&key);
    }
    if let Some(home) = home {
        let selected = home
            .canonicalize()
            .map_err(|_| anyhow::anyhow!("selected Cursor home is unavailable"))?;
        anyhow::ensure!(selected.is_dir(), "selected Cursor home is not a directory");
        if let Some(key) = cursor_api_key_for_home(&selected) {
            return Ok(AccountAuth {
                env: vec![("CURSOR_API_KEY".into(), key)],
                env_remove: vec!["CURSOR_AUTH_TOKEN".into()],
            });
        }
        let default_home =
            dirs::home_dir().and_then(|profile| profile.join(".cursor").canonicalize().ok());
        anyhow::ensure!(
            default_home.as_ref() == Some(&selected),
            "custom Cursor OAuth homes are unsupported by the native CLI; select its default home or an account with its own native API key"
        );
    }
    Ok(AccountAuth {
        env: Vec::new(),
        env_remove: vec!["CURSOR_API_KEY".into(), "CURSOR_AUTH_TOKEN".into()],
    })
}

/// First UUID-shaped token in `text` (a chat id), lower-cased.
pub fn extract_uuid(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let shape = [8usize, 4, 4, 4, 12];
    let total = 36;
    let mut i = 0;
    while i + total <= bytes.len() {
        let cand = &bytes[i..i + total];
        let mut pos = 0;
        let mut ok = true;
        for (n, len) in shape.iter().enumerate() {
            if n > 0 {
                if cand[pos] != b'-' {
                    ok = false;
                    break;
                }
                pos += 1;
            }
            if !cand[pos..pos + len].iter().all(u8::is_ascii_hexdigit) {
                ok = false;
                break;
            }
            pos += len;
        }
        // Not the middle of a longer hex run.
        let bounded = (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
            && (i + total == bytes.len() || !bytes[i + total].is_ascii_alphanumeric());
        if ok && bounded {
            return Some(text[i..i + total].to_ascii_lowercase());
        }
        i += 1;
    }
    None
}

/// Ask the Cursor CLI for a fresh chat id (`agent create-chat`) so the conversation can be resumed
/// explicitly. Missing commands, timeouts or invalid output fail before a turn is dispatched.
pub async fn create_chat(
    command: &str,
    env: &[(String, String)],
    env_remove: &[String],
    cwd: &str,
) -> Option<String> {
    use std::process::Stdio;
    let mut cmd = crate::process_ui::command(command);
    cmd.arg("create-chat")
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for key in env_remove {
        cmd.env_remove(key);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    if !env.iter().any(|(k, _)| k == "CURSOR_API_KEY") {
        cmd.env_remove("CURSOR_API_KEY");
    }
    let out = tokio::task::spawn_blocking(move || {
        crate::process_lifetime::capture(cmd, std::time::Duration::from_secs(20), 1024 * 1024)
    })
    .await
    .ok()?
    .ok()?;
    if !out.status.success() {
        return None;
    }
    extract_uuid(&String::from_utf8_lossy(&out.stdout))
}

pub fn assert_safe_cursor_argv(argv: &[String], allow_force: bool) -> Result<()> {
    if argv.iter().any(|a| a == "--continue" || a == "--last") {
        bail!("refusing Cursor --continue/--last; bind an explicit chat id");
    }
    if argv.first().map(|s| s.as_str()) == Some("resume") {
        bail!("refusing Cursor `resume` subcommand (latest chat); use --resume <chatId>");
    }
    if let Some(i) = argv.iter().position(|a| a == "--resume") {
        let value = argv.get(i + 1);
        if value.map(|v| v.starts_with('-')).unwrap_or(true) {
            bail!("refusing Cursor --resume without an explicit chat id");
        }
    }
    if !allow_force {
        for token in argv {
            if token == "--force" || token == "-f" || token == "--yolo" {
                bail!("refusing Cursor {token}; set trust=full to opt in");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::extract_uuid;

    #[test]
    fn custom_cursor_home_requires_its_own_native_authentication() {
        let root = tempfile::tempdir().unwrap();
        let error = super::account_auth(Some(root.path()))
            .err()
            .expect("custom OAuth home must be refused");
        assert!(
            error
                .to_string()
                .contains("custom Cursor OAuth homes are unsupported")
        );
        let native = super::account_auth(None).unwrap();
        assert!(native.env.is_empty());
        assert!(native.env_remove.iter().any(|key| key == "CURSOR_API_KEY"));
        std::fs::write(
            root.path().join("api-key"),
            "synthetic-selected-account-key",
        )
        .unwrap();
        let selected = super::account_auth(Some(root.path())).unwrap();
        assert!(selected.env.iter().any(
            |(key, value)| key == "CURSOR_API_KEY" && value == "synthetic-selected-account-key"
        ));
        assert!(
            !selected
                .env_remove
                .iter()
                .any(|key| key == "CURSOR_API_KEY")
        );
        assert!(
            selected
                .env_remove
                .iter()
                .any(|key| key == "CURSOR_AUTH_TOKEN")
        );
    }

    #[test]
    fn finds_a_chat_id_in_plain_json_or_noisy_output() {
        let id = "3f2a9c1e-7b44-4d0a-9e51-0a1b2c3d4e5f";
        assert_eq!(extract_uuid(id).as_deref(), Some(id));
        assert_eq!(extract_uuid(&format!("{id}\n")).as_deref(), Some(id));
        assert_eq!(
            extract_uuid(&format!(r#"{{"chatId":"{}"}}"#, id.to_uppercase())).as_deref(),
            Some(id)
        );
        assert_eq!(
            extract_uuid(&format!("created chat {id} ok")).as_deref(),
            Some(id)
        );
    }

    #[test]
    fn rejects_things_that_only_look_like_one() {
        assert!(extract_uuid("").is_none());
        assert!(extract_uuid("not a uuid at all").is_none());
        assert!(
            extract_uuid("3f2a9c1e-7b44-4d0a-9e51").is_none(),
            "too short"
        );
        assert!(
            extract_uuid("3f2a9c1e-7b44-4d0a-9e51-0a1b2c3d4e5fffff").is_none(),
            "longer hex run"
        );
        assert!(
            extract_uuid("zzzzzzzz-7b44-4d0a-9e51-0a1b2c3d4e5f").is_none(),
            "not hex"
        );
    }

    #[test]
    fn runtime_cursor_key_is_registered_and_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let key = "cursor-runtime-secret-0123456789";
        std::fs::write(dir.path().join("api-key"), key).unwrap();
        assert_eq!(
            crate::identity::cursor_api_key_for_home(dir.path()).as_deref(),
            Some(key)
        );
        let text = format!("key={key}");
        let redacted = crate::redact::redact(&text);
        assert!(!redacted.contains(key));
        assert!(redacted.contains("[REDACTED:runtime_secret:"));
    }
}
