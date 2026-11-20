//! A bounded MCP exchange with one child and one cancellation owner.
use super::{MAX_REQUEST_BYTES, MAX_UPSTREAM_BYTES, MAX_UPSTREAM_FRAME};
use crate::mcp_registry::SecretRef;
use serde_json::Value;
use std::{collections::BTreeMap, process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub(super) fn sequence(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, SecretRef>,
    requests: Vec<Value>,
) -> Result<Vec<Value>, String> {
    // The CLI itself runs inside Tokio. A scoped thread keeps this synchronous MCP
    // entry usable there and in synchronous callers without nesting runtimes.
    std::thread::scope(|scope| {
        scope
            .spawn(move || exchange(command, args, env, requests))
            .join()
            .unwrap_or_else(|_| Err("MCP I/O worker stopped unexpectedly".into()))
    })
}

fn exchange(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, SecretRef>,
    requests: Vec<Value>,
) -> Result<Vec<Value>, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "MCP I/O runtime unavailable")?;
    runtime.block_on(async {
        let mut launch = crate::process_ui::tokio_command(command);
        launch
            .args(args)
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        launch.process_group(0);
        for (key, reference) in env {
            let value = crate::mcp_registry::resolve_secret(reference).ok_or(
                "MCP credential unavailable; set the referenced environment variable or sign in",
            )?;
            crate::redact::register_runtime_secret(value.clone());
            launch.env(key, value);
        }
        let mut child = launch
            .spawn()
            .map_err(|_| "MCP stdio server could not start")?;
        let tree = match crate::process_lifetime::own_tokio_process(&child) {
            Ok(tree) => tree,
            Err(error) => {
                let _ = child.kill().await;
                return Err(error);
            }
        };
        let exchange = async {
            let mut input = child.stdin.take().ok_or("MCP stdio input unavailable")?;
            let output = child.stdout.take().ok_or("MCP stdio output unavailable")?;
            let mut output = BufReader::new(output);
            let mut total = 0usize;
            let mut frames = 0usize;
            let mut values = Vec::with_capacity(requests.len());
            for (index, request) in requests.into_iter().enumerate() {
                let wire =
                    serde_json::to_vec(&request).map_err(|_| "MCP request could not be encoded")?;
                if wire.len() > MAX_REQUEST_BYTES {
                    return Err("MCP request exceeds the size limit".into());
                }
                input
                    .write_all(&wire)
                    .await
                    .map_err(|_| "MCP stdio request failed")?;
                input
                    .write_all(b"\n")
                    .await
                    .map_err(|_| "MCP stdio request failed")?;
                input
                    .flush()
                    .await
                    .map_err(|_| "MCP stdio request failed")?;
                loop {
                    let mut line = Vec::new();
                    loop {
                        let buffer = output
                            .fill_buf()
                            .await
                            .map_err(|_| "MCP stdio read failed")?;
                        if buffer.is_empty() {
                            return Err("MCP stdio output ended before a response".into());
                        }
                        let count = buffer
                            .iter()
                            .position(|byte| *byte == b'\n')
                            .map_or(buffer.len(), |index| index + 1);
                        if line.len().saturating_add(count) > MAX_UPSTREAM_FRAME {
                            return Err("MCP stdio response exceeds the frame limit".into());
                        }
                        line.extend_from_slice(&buffer[..count]);
                        output.consume(count);
                        if line.last() == Some(&b'\n') {
                            break;
                        }
                    }
                    total = total.saturating_add(line.len());
                    frames += 1;
                    if total > MAX_UPSTREAM_BYTES || frames > 256 {
                        return Err("MCP stdio exchange exceeds its response budget".into());
                    }
                    let value: Value = serde_json::from_slice(&line)
                        .map_err(|_| "MCP stdio response was not JSON")?;
                    if value.get("id") == request.get("id") {
                        if value.get("result").is_none() && value.get("error").is_none() {
                            return Err("MCP response has no result or error".into());
                        }
                        if index == 0 {
                            super::http::negotiated_protocol(&value)?;
                        }
                        values.push(value);
                        break;
                    }
                }
                if index == 0 {
                    input
                        .write_all(
                            b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
                        )
                        .await
                        .map_err(|_| "MCP initialized notification failed")?;
                    input
                        .flush()
                        .await
                        .map_err(|_| "MCP initialized notification failed")?;
                }
            }
            Ok(values)
        };
        let result = tokio::time::timeout(Duration::from_secs(10), exchange)
            .await
            .unwrap_or_else(|_| Err("MCP stdio exchange timed out".into()));
        drop(tree); // close the job/process group before awaiting the direct child
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        result
    })
}
