//! Read-only native Cursor history replay. No session creation, prompts, login or tool execution.
use super::{Binding, Source};
use serde_json::{json, Value};
use std::{io::Cursor, process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};

const WIRE_LIMIT: usize = 16 * 1024 * 1024;
const FRAME_LIMIT: usize = 1024 * 1024;

struct Replay {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    session: String,
    wire_bytes: usize,
    records: Vec<u8>,
}

impl Drop for Replay {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            if let Some(pid) = self.child.id() {
                crate::provider::turn::kill_process_tree(pid);
            }
            let _ = self.child.start_kill();
        }
    }
}

impl Replay {
    async fn write(&mut self, value: Value) -> Result<(), String> {
        let input = self.input.as_mut().ok_or("Cursor history input closed")?;
        input
            .write_all(format!("{value}\n").as_bytes())
            .await
            .map_err(|_| "Cursor history input closed")?;
        input
            .flush()
            .await
            .map_err(|_| "Cursor history input closed".into())
    }

    async fn rpc(&mut self, id: u64, method: &str, params: Value) -> Result<Value, String> {
        self.write(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        loop {
            let mut raw = Vec::new();
            let count = (&mut self.output)
                .take((FRAME_LIMIT + 1) as u64)
                .read_until(b'\n', &mut raw)
                .await
                .map_err(|_| "Cursor history output could not be read")?;
            if count == 0 {
                return Err("Cursor closed before completing native history replay".into());
            }
            self.wire_bytes += count;
            if count > FRAME_LIMIT || self.wire_bytes > WIRE_LIMIT {
                return Err("Cursor native history exceeds the bounded replay budget".into());
            }
            let value: Value = serde_json::from_slice(&raw)
                .map_err(|_| "Cursor history returned invalid JSON/UTF-8")?;
            if value["jsonrpc"] != "2.0" {
                return Err("Cursor history protocol version mismatch".into());
            }
            if value.get("method").is_some() {
                if value.get("id").is_some() {
                    // Loading history must not require filesystem, shell, approval or prompt actions.
                    self.write(json!({"jsonrpc":"2.0","id":value["id"],"error":{"code":-32601,"message":"Read-only history client does not execute tools"}})).await?;
                    return Err("Cursor history replay requested a tool/client action; no action was executed".into());
                }
                if value["method"] == "session/update" {
                    if value.pointer("/params/sessionId").and_then(Value::as_str)
                        != Some(&self.session)
                    {
                        return Err(
                            "Cursor history replay returned another session identity".into()
                        );
                    }
                    if super::message_role(
                        &value,
                        crate::store::ProviderKind::Cursor,
                        &self.session,
                    )
                    .is_some()
                    {
                        self.records.extend_from_slice(&raw);
                        if !raw.ends_with(b"\n") {
                            self.records.push(b'\n');
                        }
                    }
                }
                continue;
            }
            if value["id"] != id {
                return Err("Cursor history response identity mismatch".into());
            }
            if value.get("error").is_some() {
                return Err(format!("Cursor native history request failed (code {}); use the provider's own login/diagnostics", value["error"]["code"].as_i64().unwrap_or(-1)));
            }
            return value
                .get("result")
                .cloned()
                .ok_or_else(|| "Cursor history response has no result".into());
        }
    }
}

pub(super) fn open(root: &std::path::Path, binding: &Binding) -> Result<Source, String> {
    #[cfg(test)]
    if crate::provider::bin_override("CONCORDIA_AGENT_BIN").is_none() {
        return Err("Cursor history unit tests require an explicit fake provider".into());
    }
    let home = binding.account_home(root)?;
    let (session, _) = crate::sessions::find_native_record(
        root,
        &binding.account,
        Some(crate::store::ProviderKind::Cursor),
        &binding.session_id,
    )?
    .ok_or("explicit Cursor session metadata is unavailable")?;
    let cwd = session
        .cwd
        .ok_or("Cursor session has no explicit workspace")?;
    let cwd = std::fs::canonicalize(cwd).map_err(|_| "Cursor history workspace is unavailable")?;
    let sid = binding.session_id.clone();
    // The public API is synchronous (CLI/MCP and blocking TUI source preparation). A private
    // bounded runtime thread also works when a caller already owns a Tokio runtime.
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()
            .map_err(|_|"Cursor history runtime could not start")?;
        runtime.block_on(async move {
            let command = crate::provider::cursor::find_agent_bin()?;
            let plan = crate::provider::PlannedTurn {
                command, argv: vec!["acp".into()], env: Vec::new(), env_remove: Vec::new(), cwd: cwd.to_string_lossy().into_owned(),
                prompt: "acp".into(), new_session_id: None, provider: crate::store::ProviderKind::Cursor,
            };
            let launch = crate::provider::launch::prepare(&plan).map_err(|e|e.to_string())?;
        let mut command = crate::process_ui::tokio_command(&launch.program);
            crate::process_ui::background(&mut command);
            command.args(&launch.args).envs(launch.env)
                .env("CURSOR_CONFIG_DIR", &home)
                .env_remove("CURSOR_API_KEY").env_remove("CURSOR_AUTH_TOKEN")
                .current_dir(&cwd).stdin(Stdio::piped()).stdout(Stdio::piped())
                .stderr(Stdio::null()).kill_on_drop(true);
            let mut child = command.spawn().map_err(|_|"could not start the native Cursor history interface")?;
            let input = child.stdin.take().ok_or("Cursor history input unavailable")?;
            let output = BufReader::new(child.stdout.take().ok_or("Cursor history output unavailable")?);
            let mut replay = Replay { child, input: Some(input), output, session: sid.clone(), wire_bytes: 0, records: Vec::new() };
            let run = async {
                let init = replay.rpc(1,"initialize", json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"concordia-history","version":crate::version::VERSION}})).await?;
                if init["protocolVersion"] != 1 || init.pointer("/agentCapabilities/loadSession").and_then(Value::as_bool) != Some(true) {
                    return Err("installed Cursor does not advertise native history loading".into());
                }
                let loaded = replay.rpc(2,"session/load",json!({"sessionId":sid,"cwd":cwd,"mcpServers":[]})).await?;
                if loaded.get("sessionId").is_some_and(|id| id != &Value::String(sid.clone())) {
                    return Err("Cursor loaded another session identity".into());
                }
                Ok::<(),String>(())
            };
            tokio::time::timeout(Duration::from_secs(25), run).await
                .map_err(|_|"Cursor history replay timed out; no prompt was sent")??;
            let records = std::mem::take(&mut replay.records);
            // Closing stdin lets the unmodified native CLI shut down before the cancellation guard.
            if let Some(mut input) = replay.input.take() {
                let _ = input.shutdown().await;
                drop(input);
            }
            let _ = tokio::time::timeout(Duration::from_secs(2), replay.child.wait()).await;
            Ok(Source::Replay(Cursor::new(records)))
        })
    }).join().map_err(|_|"Cursor history worker stopped unexpectedly")?
}
