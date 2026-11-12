//! The single always-on MCP surface: account discovery and delegated agents.
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

pub const LATEST_PROTOCOL: &str = "2025-06-18";
const KNOWN_PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_RESULT_CHARS: usize = 20_000;
pub const MAX_PENDING_PROPOSALS: usize = 64;
const MAX_PROPOSAL_FILE: u64 = 16 * 1024;

pub fn proposals_dir(root: &Path) -> PathBuf {
    root.join("proposals")
}
pub fn agents_dir(root: &Path) -> PathBuf {
    root.join("agents")
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentRecord {
    pub agent_id: String,
    pub account: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    pub task: String,
    #[serde(default)]
    pub parent: String,
    pub status: String,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub activity: Vec<String>,
}
pub fn record_path(root: &Path, id: &str) -> PathBuf {
    agents_dir(root).join(format!("{id}.json"))
}
pub fn read_agent(root: &Path, id: &str) -> Result<AgentRecord, String> {
    let bytes = std::fs::read(record_path(root, id))
        .map_err(|_| format!("unknown delegated agent: {id}"))?;
    serde_json::from_slice(&bytes).map_err(|_| format!("delegated agent record is invalid: {id}"))
}
pub fn take_pending_records(root: &Path, limit: usize) -> Vec<AgentRecord> {
    let Ok(entries) = std::fs::read_dir(agents_dir(root)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .take(limit)
        .filter_map(|entry| {
            std::fs::read(entry.path())
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        })
        .collect()
}
pub fn write_agent(root: &Path, record: &AgentRecord) -> Result<(), String> {
    std::fs::create_dir_all(agents_dir(root)).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    crate::state_io::write_atomic(
        &record_path(root, &record.agent_id),
        &crate::redact::redact_bytes(&bytes),
    )
    .map_err(|e| e.to_string())
}
pub fn update_agent(root: &Path, id: &str, status: &str, result: Option<String>, activity: &str) {
    if let Ok(mut record) = read_agent(root, id) {
        record.status = status.into();
        record.result = result;
        record.activity.push(activity.into());
        let _ = write_agent(root, &record);
    }
}
pub fn update_matching(
    root: &Path,
    account: &str,
    task: &str,
    status: &str,
    result: Option<String>,
) {
    let Ok(entries) = std::fs::read_dir(agents_dir(root)) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<AgentRecord>(&bytes) else {
            continue;
        };
        if record.account == account && record.task == task && record.status == "running" {
            update_agent(
                root,
                &record.agent_id,
                status,
                result.clone(),
                "delegated run finished",
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub agent_id: String,
    pub to: String,
    pub task: String,
    #[serde(default)]
    pub from_thread: String,
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(untagged)]
pub enum PendingProposal {
    Delegate(Proposal),
}

fn error_reply(id: Value, code: i64, message: &str) -> String {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":crate::redact::redact(message).as_ref()}}).to_string()
}
fn result_reply(id: Value, result: Value) -> String {
    json!({"jsonrpc":"2.0","id":id,"result":result}).to_string()
}
pub fn serve<R: BufRead, W: Write>(
    mut input: R,
    mut output: W,
    root: &Path,
) -> std::io::Result<()> {
    loop {
        let mut buf = Vec::new();
        let n = (&mut input)
            .take(MAX_REQUEST_BYTES as u64 + 1)
            .read_until(b'\n', &mut buf)?;
        if n == 0 {
            return Ok(());
        }
        let too_large = buf.len() > MAX_REQUEST_BYTES;
        if too_large && !buf.ends_with(b"\n") {
            let mut discard = [0u8; 8192];
            loop {
                let read = input.read(&mut discard)?;
                if read == 0 || discard[..read].contains(&b'\n') {
                    break;
                }
            }
        }
        let reply = if too_large {
            Some(error_reply(Value::Null, -32600, "request too large"))
        } else if buf.iter().all(u8::is_ascii_whitespace) {
            None
        } else {
            std::str::from_utf8(&buf)
                .ok()
                .and_then(|line| handle_line(root, line))
        };
        if let Some(reply) = reply {
            output.write_all(reply.as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}
pub fn handle_line(root: &Path, line: &str) -> Option<String> {
    let msg: Value = serde_json::from_str(line.trim()).ok()?;
    let id = msg
        .get("id")
        .cloned()
        .filter(|v| v.is_string() || v.is_number())?;
    let method = msg.get("method").and_then(Value::as_str)?;
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => result_reply(
            id,
            json!({"protocolVersion":params.get("protocolVersion").and_then(Value::as_str).filter(|v| KNOWN_PROTOCOLS.contains(v)).unwrap_or(LATEST_PROTOCOL),"capabilities":{"tools":{}},"serverInfo":{"name":"concordia","version":crate::version::VERSION}}),
        ),
        "ping" => result_reply(id, json!({})),
        "tools/list" => result_reply(
            id,
            json!({"tools":[{"name":"agents","description":"List accounts/models, start an agent, or read its status/result.","inputSchema":{"type":"object","additionalProperties":false,"properties":{"action":{"enum":["list","start","status","result"]},"agent_id":{"type":"string"},"account":{"type":"string"},"provider":{"type":"string"},"routine":{"type":"string"},"model":{"type":"string"},"task":{"type":"string"},"parent":{"type":"string"}},"required":["action"]}}]}),
        ),
        "tools/call" => match call_agents(root, &params) {
            Ok(text) => result_reply(id, text_result(text, false)),
            Err(e) => result_reply(id, text_result(e, true)),
        },
        _ => error_reply(id, -32601, "method not found"),
    })
}
fn text_result(text: String, is_error: bool) -> Value {
    json!({"content":[{"type":"text","text":crate::redact::redact(&text).chars().take(MAX_RESULT_CHARS).collect::<String>()}],"isError":is_error})
}
fn arg<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("agents needs {name}"))
}
fn call_agents(root: &Path, params: &Value) -> Result<String, String> {
    if params.get("name").and_then(Value::as_str) != Some("agents") {
        return Err("unknown tool".into());
    }
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let action = arg(&args, "action")?;
    match action {
        "list" => {
            let accounts = crate::accounts::list_all_accounts(root).into_iter().map(|a| { let models = a.home_path.as_deref().map(|h| crate::catalog::list_models(a.provider, h)).unwrap_or_default(); let usage = a.home_path.as_deref().and_then(|h| crate::usage::cached_usage_view(root, a.provider, &a.account, h)); json!({"account":a.account,"provider":a.provider.as_str(),"models":models,"usage":usage.map(|u| json!({"fresh":u.fresh,"observed_at":u.observed_at,"windows":u.windows}))}) }).collect::<Vec<_>>();
            Ok(json!({"accounts":accounts}).to_string())
        }
        "start" if args.get("provider").and_then(Value::as_str) == Some("grok-bot") => {
            let routine = arg(&args, "routine")?;
            let task = arg(&args, "task")?;
            let id = crate::grok_bot::start(
                root,
                routine,
                task,
                args.get("parent")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )?;
            Ok(json!({"agent_id":id,"status":"waiting"}).to_string())
        }
        "start" => {
            let account = arg(&args, "account")?;
            let task = arg(&args, "task")?;
            let connected = crate::accounts::get_account(root, account)
                .ok_or_else(|| format!("unknown connected account: {account}"))?;
            let id = uuid::Uuid::new_v4().to_string();
            let record = AgentRecord {
                agent_id: id.clone(),
                account: account.into(),
                provider: connected.provider.as_str().into(),
                model: args.get("model").and_then(Value::as_str).map(str::to_owned),
                task: task.into(),
                parent: args
                    .get("parent")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                status: "queued".into(),
                result: None,
                activity: vec!["delegation requested".into()],
            };
            write_agent(root, &record)?;
            write_proposal(
                root,
                &Proposal {
                    agent_id: id.clone(),
                    to: account.into(),
                    task: task.into(),
                    from_thread: record.parent.clone(),
                },
            )?;
            Ok(json!({"agent_id":id,"status":"queued"}).to_string())
        }
        "status" | "result" => Ok(serde_json::to_string(&read_agent(
            root,
            arg(&args, "agent_id")?,
        )?)
        .map_err(|e| e.to_string())?),
        _ => Err("agents action must be list, start, status or result".into()),
    }
}
fn write_proposal(root: &Path, proposal: &Proposal) -> Result<(), String> {
    let dir = proposals_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if std::fs::read_dir(&dir).map_err(|e| e.to_string())?.count() >= MAX_PENDING_PROPOSALS {
        return Err("too many delegated agents are waiting".into());
    }
    let bytes = serde_json::to_vec(proposal).map_err(|e| e.to_string())?;
    let bytes = crate::redact::redact_bytes(&bytes);
    if bytes.len() as u64 > MAX_PROPOSAL_FILE {
        return Err("delegation record is too large".into());
    }
    crate::state_io::write_atomic(&dir.join(format!("{}.json", proposal.agent_id)), &bytes)
        .map_err(|e| e.to_string())
}
pub fn take_proposals(root: &Path) -> Vec<Proposal> {
    take_pending(root, MAX_PENDING_PROPOSALS)
        .into_iter()
        .map(|p| match p {
            PendingProposal::Delegate(p) => p,
        })
        .collect()
}
pub fn take_pending(root: &Path, limit: usize) -> Vec<PendingProposal> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(proposals_dir(root)) else {
        return out;
    };
    for e in rd.flatten().take(limit) {
        let p = e.path();
        if let Ok(bytes) = std::fs::read(&p) {
            if let Ok(v) = serde_json::from_slice(&bytes) {
                let _ = std::fs::remove_file(p);
                out.push(v);
            }
        }
    }
    out
}
pub fn has_proposals(root: &Path) -> bool {
    std::fs::read_dir(proposals_dir(root)).is_ok_and(|mut r| r.next().is_some())
}
