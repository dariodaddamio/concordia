//! Per-turn MCP question bridge used by providers that do not expose a native question tool.
//!
//! The server is intentionally small and stateless.  The parent turn owns the loopback endpoint;
//! this process only validates MCP input and forwards one bounded request at a time.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as AsyncBufReader};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_QUESTIONS: usize = 4;
const MAX_OPTIONS: usize = 8;
const MAX_TEXT: usize = 2_000;
const ENDPOINT_ENV: &str = "CONCORDIA_ASK_ENDPOINT";
const TOKEN_ENV: &str = "CONCORDIA_ASK_TOKEN";

pub struct BridgeRequest {
    pub questions: Value,
    pub answer: oneshot::Sender<Value>,
}

pub async fn spawn_listener(
    token: String,
    tx: tokio::sync::mpsc::UnboundedSender<BridgeRequest>,
) -> std::io::Result<(String, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = listener.local_addr()?.to_string();
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let token = token.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let (reader, mut writer) = stream.into_split();
                let mut reader = AsyncBufReader::new(reader);
                let mut line = Vec::new();
                if reader.read_until(b'\n', &mut line).await.is_err()
                    || line.len() > MAX_REQUEST_BYTES
                {
                    return;
                }
                let Ok(value) = serde_json::from_slice::<Value>(&line) else {
                    return;
                };
                if value.get("token").and_then(Value::as_str) != Some(token.as_str()) {
                    return;
                }
                let questions = value.get("questions").cloned().unwrap_or(Value::Null);
                if validate(&json!({"questions": questions})).is_err() {
                    return;
                }
                let (answer_tx, answer_rx) = oneshot::channel();
                if tx
                    .send(BridgeRequest {
                        questions,
                        answer: answer_tx,
                    })
                    .is_err()
                {
                    return;
                }
                let answer =
                    tokio::time::timeout(std::time::Duration::from_secs(120), answer_rx).await;
                let value = answer
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or_else(|| json!({"error":"the user dismissed the question"}));
                let _ = writer.write_all(value.to_string().as_bytes()).await;
                let _ = writer.write_all(b"\n").await;
            });
        }
    });
    Ok((endpoint, task))
}

fn reply(id: Value, result: Value) -> String {
    json!({"jsonrpc":"2.0", "id":id, "result":result}).to_string()
}
fn error(id: Value, code: i64, message: &str) -> String {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code,"message":message}}).to_string()
}

fn tool() -> Value {
    json!({"name":"ask_user","description":"Use this whenever you need a decision or clarification from the user. Concordia will show the questions in its question popup and return the selected answers.","inputSchema":{"type":"object","properties":{"questions":{"type":"array","minItems":1,"maxItems":4,"items":{"type":"object","properties":{"question":{"type":"string","maxLength":2000},"options":{"type":"array","minItems":2,"maxItems":8,"items":{"type":"string","maxLength":2000}},"multiple":{"type":"boolean"}},"required":["question","options"],"additionalProperties":false}}},"required":["questions"],"additionalProperties":false}})
}

fn validate(args: &Value) -> Result<(), &'static str> {
    let qs = args
        .get("questions")
        .and_then(Value::as_array)
        .ok_or("questions must be an array")?;
    if qs.is_empty() || qs.len() > MAX_QUESTIONS {
        return Err("questions must contain 1 to 4 items");
    }
    for q in qs {
        let text = q
            .get("question")
            .and_then(Value::as_str)
            .ok_or("question must be text")?;
        if text.is_empty() || text.chars().count() > MAX_TEXT {
            return Err("question length is invalid");
        }
        let options = q
            .get("options")
            .and_then(Value::as_array)
            .ok_or("options must be an array")?;
        if options.len() < 2 || options.len() > MAX_OPTIONS {
            return Err("options must contain 2 to 8 items");
        }
        for option in options {
            if option
                .as_str()
                .is_none_or(|s| s.is_empty() || s.chars().count() > MAX_TEXT)
            {
                return Err("option length is invalid");
            }
        }
        if q.get("multiple").is_some_and(|v| !v.is_boolean()) {
            return Err("multiple must be boolean");
        }
    }
    Ok(())
}

fn forward(args: &Value) -> Result<String, String> {
    validate(args).map_err(str::to_owned)?;
    let endpoint = std::env::var(ENDPOINT_ENV)
        .map_err(|_| "Concordia question bridge is unavailable".to_owned())?;
    let token = std::env::var(TOKEN_ENV)
        .map_err(|_| "Concordia question bridge token is unavailable".to_owned())?;
    let mut stream = TcpStream::connect(endpoint)
        .map_err(|_| "Concordia turn is no longer running".to_owned())?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(120)))
        .ok();
    let body = json!({"token":token,"questions":args["questions"]}).to_string();
    stream
        .write_all(body.as_bytes())
        .map_err(|_| "Concordia turn closed the question bridge".to_owned())?;
    stream
        .write_all(b"\n")
        .map_err(|_| "Concordia turn closed the question bridge".to_owned())?;
    let mut response = Vec::new();
    BufReader::new(stream)
        .take(MAX_REQUEST_BYTES as u64 + 1)
        .read_until(b'\n', &mut response)
        .map_err(|_| "invalid response from Concordia turn".to_owned())?;
    if response.len() > MAX_REQUEST_BYTES {
        return Err("question bridge response is too large".into());
    }
    let value: Value = serde_json::from_slice(&response)
        .map_err(|_| "invalid response from Concordia turn".to_owned())?;
    Ok(value.to_string())
}

pub fn handle_line(line: &str) -> Option<String> {
    if line.len() > MAX_REQUEST_BYTES {
        return Some(error(Value::Null, -32600, "request too large"));
    }
    let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
        return Some(error(Value::Null, -32700, "parse error"));
    };
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = value.get("method").and_then(Value::as_str) else {
        return None;
    };
    Some(match method {
        "initialize" => reply(
            id,
            json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"concordia-ask","version":crate::version::VERSION}}),
        ),
        "ping" => reply(id, json!({})),
        "tools/list" => reply(id, json!({"tools":[tool()]})),
        "tools/call" => {
            let params = value.get("params").cloned().unwrap_or(Value::Null);
            if params.get("name").and_then(Value::as_str) != Some("ask_user") {
                error(id, -32601, "unknown tool")
            } else {
                match forward(params.get("arguments").unwrap_or(&Value::Null)) {
                    Ok(text) => reply(id, json!({"content":[{"type":"text","text":text}]})),
                    Err(message) => reply(
                        id,
                        json!({"isError":true,"content":[{"type":"text","text":message}]}),
                    ),
                }
            }
        }
        _ => error(id, -32601, "method not found"),
    })
}

pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> std::io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = input.read_until(b'\n', &mut line)?;
        if n == 0 {
            return Ok(());
        }
        if let Some(response) = handle_line(std::str::from_utf8(&line).unwrap_or("")) {
            output.write_all(response.as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exposes_only_ask_user() {
        let v: Value = handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(v["result"]["tools"].as_array().unwrap().len(), 1);
        assert_eq!(v["result"]["tools"][0]["name"], "ask_user");
    }
    #[test]
    fn rejects_oversized_input() {
        assert!(handle_line(&"x".repeat(MAX_REQUEST_BYTES + 1))
            .unwrap()
            .contains("request too large"));
    }
}
