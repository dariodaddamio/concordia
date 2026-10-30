use crate::identity::cursor_api_key_for_home;
use crate::store::TrustMode;
use anyhow::{bail, Result};
use std::path::Path;
use which::which;

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
    pub cwd: String,
}

pub fn find_agent_bin() -> String {
    if let Some(p) = super::bin_override("CONCORDIA_AGENT_BIN") {
        return p;
    }
    which("agent")
        .or_else(|_| which("cursor-agent"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "agent".into())
}

pub fn plan_cursor_turn(req: &CursorTurnRequest<'_>) -> Result<CursorPlan> {
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
        TrustMode::Workspace => {
            argv.push("--sandbox".into());
            argv.push("disabled".into());
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
    assert_safe_cursor_argv(&argv, req.trust == TrustMode::Full)?;
    let mut env = Vec::new();
    if let Some(home) = req.home_path {
        if let Some(key) = cursor_api_key_for_home(home) {
            env.push(("CURSOR_API_KEY".into(), key));
        }
    }
    Ok(CursorPlan {
        command: find_agent_bin(),
        argv,
        env,
        cwd: req.cwd.to_string_lossy().into_owned(),
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
/// explicitly. Any failure (missing command, timeout, unexpected output) returns `None`, and the
/// turn simply runs without a bound session as before.
pub async fn create_chat(command: &str, env: &[(String, String)], cwd: &str) -> Option<String> {
    use std::process::Stdio;
    let mut cmd = tokio::process::Command::new(command);
    cmd.arg("create-chat")
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = tokio::time::timeout(std::time::Duration::from_secs(20), cmd.output())
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
}
