//! Read-only native app-server RPC. Never creates a thread or starts a model turn.
use serde_json::{json, Value};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

pub(crate) fn fetch(home: &Path) -> Result<String, String> {
    if crate::provider::bin_override("CONCORDIA_CODEX_BIN").is_some() {
        return Err("live usage disabled for overridden provider executables".into());
    }
    #[cfg(test)]
    {
        let _ = home;
        Err("live usage disabled in unit tests".into())
    }
    #[cfg(not(test))]
    {
        let bin = crate::provider::codex::find_codex_bin();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?
            .block_on(fetch_with(&bin, home))
    }
}

pub(crate) async fn fetch_with(bin: &str, home: &Path) -> Result<String, String> {
    use std::process::Stdio;
    let native = crate::provider::npm::codex(bin);
    let mut cmd = tokio::process::Command::new(
        native
            .as_ref()
            .map_or(Path::new(bin), |n| n.program.as_path()),
    );
    crate::process_ui::background(&mut cmd);
    cmd.arg("app-server")
        .env("CODEX_HOME", home)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(native) = native {
        cmd.envs(native.env);
    }
    let mut child = cmd
        .spawn()
        .map_err(|_| "could not start Codex usage reader".to_owned())?;
    let pid = child.id().unwrap_or(0);
    let mut input = child.stdin.take().ok_or("usage reader stdin unavailable")?;
    let mut lines = BufReader::new(
        child
            .stdout
            .take()
            .ok_or("usage reader stdout unavailable")?
            .take(512 * 1024),
    )
    .lines();
    let result = tokio::time::timeout(std::time::Duration::from_secs(12), async {
        async fn receive<R: tokio::io::AsyncBufRead + Unpin>(lines: &mut tokio::io::Lines<R>, id: u64) -> Result<Value,String> {
            for _ in 0..128 {
                let line = lines.next_line().await.map_err(|_| "usage reader failed")?.ok_or("usage reader closed before replying")?;
                let Ok(value) = serde_json::from_str::<Value>(&line) else { continue; };
                if value["id"] == id {
                    if value.get("error").is_some() { return Err("Codex could not read live limits; check its native sign-in".into()); }
                    return value.get("result").cloned().ok_or_else(|| "usage reader returned no result".into());
                }
            }
            Err("usage reader response bound exceeded".into())
        }
        let init = json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"concordia_usage","version":crate::version::VERSION}}});
        input.write_all(format!("{init}\n").as_bytes()).await.map_err(|_| "usage initialization failed")?;
        receive(&mut lines,1).await?;
        input.write_all(b"{\"method\":\"initialized\"}\n{\"id\":2,\"method\":\"account/rateLimits/read\"}\n").await.map_err(|_| "usage request failed")?;
        let result = receive(&mut lines,2).await?;
        let limits = result.get("rateLimitsByLimitId").and_then(|m| m.get("codex")).filter(|v|v.is_object()).or_else(||result.get("rateLimits").filter(|v| v.is_object())).ok_or("Codex returned no quota windows")?;
        Ok(json!({"rate_limits":limits}).to_string())
    }).await.unwrap_or_else(|_|Err("live usage read timed out".into()));
    crate::provider::turn::kill_process_tree(pid);
    let _ = child.kill().await;
    let _ = child.wait().await;
    result
}
