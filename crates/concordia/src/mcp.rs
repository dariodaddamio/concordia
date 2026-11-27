use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
mod http;
mod stdio;
use http::HttpSession;

pub const LATEST_PROTOCOL: &str = "2025-06-18";
const KNOWN_PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_RESULT_CHARS: usize = 20_000;
pub const MAX_PENDING_PROPOSALS: usize = 64;
const MAX_PROPOSAL_FILE: u64 = 16 * 1024;
const MAX_UPSTREAM_BYTES: usize = 2 * 1024 * 1024;
const MAX_UPSTREAM_FRAME: usize = 256 * 1024;

fn bounded_line<R: BufRead>(input: &mut R, limit: usize) -> std::io::Result<Option<Vec<u8>>> {
    let mut out = Vec::with_capacity(limit.min(4096));
    let mut oversized = false;
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok((oversized || !out.is_empty()).then_some(if oversized {
                Vec::new()
            } else {
                out
            }));
        }
        let take = chunk
            .iter()
            .position(|b| *b == b'\n')
            .map_or(chunk.len(), |i| i + 1);
        let ended = take < chunk.len() || chunk[take - 1] == b'\n';
        if out.len().saturating_add(take) > limit {
            oversized = true;
            out.clear();
        }
        if !oversized {
            out.extend_from_slice(&chunk[..take]);
        }
        input.consume(take);
        if ended {
            return Ok(Some(if oversized { Vec::new() } else { out }));
        }
    }
}

pub fn proposals_dir(root: &Path) -> PathBuf {
    root.join("proposals")
}
pub use crate::agent_records as records;
pub use records::{
    agents_dir, bind_agent_run, read_agent, record_path, update_agent, update_agent_run,
    write_agent, AgentRecord,
};
pub fn take_pending_records(root: &Path, limit: usize) -> Vec<AgentRecord> {
    let Ok(entries) = std::fs::read_dir(agents_dir(root)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .take(MAX_PENDING_PROPOSALS * 4)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                return None;
            }
            read_agent(root, path.file_stem()?.to_str()?).ok()
        })
        .take(limit.min(MAX_PENDING_PROPOSALS))
        .collect()
}

pub fn registered_tool_names() -> Vec<String> {
    vec!["agents".into()]
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub agent_id: String,
    pub to: String,
    pub task: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub from_thread: String,
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    #[serde(default)]
    pub parent_run_id: Option<String>,
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
        let Some(buf) = bounded_line(&mut input, MAX_REQUEST_BYTES)? else {
            return Ok(());
        };
        let too_large = buf.is_empty();
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
        "tools/list" => result_reply(id, json!({"tools": advertised_tools(root)})),
        "tools/call" => match call_agents(root, &params) {
            Ok(text) => result_reply(id, text_result(text, false)),
            Err(e) if e == "unknown tool" => match call_remote(root, &params) {
                Ok(value) => result_reply(id, redact_value(value)),
                Err(error) => result_reply(id, text_result(error, true)),
            },
            Err(e) => result_reply(id, text_result(e, true)),
        },
        _ => error_reply(id, -32601, "method not found"),
    })
}

fn advertised_tools(root: &Path) -> Vec<Value> {
    let mut tools = vec![
        json!({"name":"agents","description":"Start an agent with role defaults; explicit provider/model win. Inspect and control a bounded worker: follow-up creates a new explicitly identified run, cancel is owned by the worker loop, and watch is bounded without retrying uncertain work.","inputSchema":{"type":"object","additionalProperties":false,"properties":{"action":{"enum":["list","start","status","result","followup","cancel","watch"]},"agent_id":{"type":"string"},"role":{"type":"string"},"account":{"type":"string"},"provider":{"type":"string"},"routine":{"type":"string"},"model":{"type":"string"},"effort":{"type":"string"},"task":{"type":"string"},"parent":{"type":"string"}},"required":["action"]}}),
    ];
    let selected = std::env::var("CONCORDIA_MCP_SERVER").ok();
    if selected.is_some() {
        tools.clear();
    }
    let project = proxy_project();
    let discovery_deadline = Instant::now() + Duration::from_secs(10);
    let Ok(registry) = crate::mcp_registry::load(root) else {
        return tools;
    };
    for (server, entry) in &registry.servers {
        if Instant::now() >= discovery_deadline {
            break;
        }
        if selected.as_deref().is_some_and(|name| name != server)
            || crate::mcp_registry::effective(&registry, &project, server).is_none()
        {
            continue;
        }
        if let Ok(values) = remote_sequence(
            &entry.transport,
            vec![
                json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":LATEST_PROTOCOL,"capabilities":{},"clientInfo":{"name":"concordia","version":crate::version::VERSION}}}),
                json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
            ],
        ) {
            let value = values.into_iter().last().unwrap_or(Value::Null);
            if let Some(remote) = value
                .get("result")
                .and_then(|v| v.get("tools"))
                .and_then(Value::as_array)
            {
                tools.extend(remote.iter().filter_map(|tool| {
                    let name = tool.get("name")?.as_str()?;
                    let description = tool.get("description").and_then(Value::as_str).unwrap_or("remote MCP tool");
                    Some(json!({"name":format!("{server}/{name}"),"description":crate::redact::redact(description).as_ref(),"inputSchema":redact_value(tool.get("inputSchema").cloned().unwrap_or_else(|| json!({"type":"object"})))}))
                }));
            }
        }
    }
    tools
}

fn proxy_project() -> PathBuf {
    let project = std::env::var_os("CONCORDIA_PROJECT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    project.canonicalize().unwrap_or(project)
}

fn call_remote(root: &Path, params: &Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "remote MCP call needs a tool name".to_string())?;
    let (server, tool) = name
        .split_once('/')
        .ok_or_else(|| "unknown tool".to_string())?;
    if std::env::var("CONCORDIA_MCP_SERVER")
        .ok()
        .as_deref()
        .is_some_and(|name| name != server)
    {
        return Err("remote MCP server does not match this proxy".into());
    }
    let registry =
        crate::mcp_registry::load(root).map_err(|_| "MCP registry cannot be read".to_string())?;
    let project = proxy_project();
    let entry = crate::mcp_registry::effective(&registry, &project, server)
        .ok_or_else(|| "remote MCP server is disabled for this project".to_string())?;
    let values = remote_sequence(
        &entry.transport,
        vec![
            json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":LATEST_PROTOCOL,"capabilities":{},"clientInfo":{"name":"concordia","version":crate::version::VERSION}}}),
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":params.get("arguments").cloned().unwrap_or_else(|| json!({}))}}),
        ],
    )?;
    let response = values.into_iter().last().unwrap_or(Value::Null);
    if let Some(error) = response.get("error") {
        return Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("remote MCP call failed")
            .to_owned());
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| "remote MCP response had no result".into())
}

fn remote_sequence(
    transport: &crate::mcp_registry::Transport,
    requests: Vec<Value>,
) -> Result<Vec<Value>, String> {
    if let crate::mcp_registry::Transport::Stdio { command, args, env } = transport {
        return stdio::sequence(command, args, env, requests);
    }
    if let crate::mcp_registry::Transport::Remote { url, headers } = transport {
        let mut session = HttpSession::new(url, headers)?;
        let mut values = Vec::new();
        for (index, request) in requests.into_iter().enumerate() {
            let value = session.request(&request)?;
            if index == 0 {
                let notification = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
                let _ = session.request(&notification)?;
            }
            if request.get("id").is_some() {
                values.push(value);
            }
        }
        return Ok(values);
    }
    Err("unsupported MCP transport".into())
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({"content":[{"type":"text","text":crate::redact::redact(&text).chars().take(MAX_RESULT_CHARS).collect::<String>()}],"isError":is_error})
}

fn redact_value(mut value: Value) -> Value {
    match &mut value {
        Value::String(text) => *text = crate::redact::redact(text).into_owned(),
        Value::Array(values) => {
            for value in values {
                *value = redact_value(value.take());
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                *value = redact_value(value.take());
            }
        }
        _ => {}
    }
    value
}
fn arg<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("agents needs {name}"))
}
fn caller_context(args: &Value) -> Result<(String, String, PathBuf), String> {
    let parent = std::env::var("CONCORDIA_PARENT_CONVERSATION")
        .map_err(|_| "agent start requires a verified parent conversation")?;
    let parent_run = std::env::var("CONCORDIA_PARENT_RUN")
        .map_err(|_| "agent start requires a verified parent run")?;
    if parent.is_empty() || uuid::Uuid::parse_str(&parent_run).is_err() {
        return Err("agent start requires a valid parent conversation and run".into());
    }
    if args
        .get("parent")
        .and_then(Value::as_str)
        .is_some_and(|requested| requested != parent)
    {
        return Err("agent start parent does not match the verified caller".into());
    }
    let workspace = std::env::var_os("CONCORDIA_PROJECT_DIR")
        .map(PathBuf::from)
        .ok_or("agent start requires a verified workspace")?
        .canonicalize()
        .map_err(|_| "agent workspace is unavailable")?;
    Ok((parent, parent_run, workspace))
}
fn call_agents(root: &Path, params: &Value) -> Result<String, String> {
    if std::env::var_os("CONCORDIA_MCP_SERVER").is_some() {
        return Err("unknown tool".into());
    }
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
            let (parent, _, _) = caller_context(&args)?;
            let id = crate::grok_bot::start(root, routine, task, &parent)?;
            Ok(json!({"agent_id":id,"status":"waiting"}).to_string())
        }
        "start" => {
            let (parent, parent_run, workspace) = caller_context(&args)?;
            let role_name = args
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("general");
            let role = crate::store::get_settings(root)
                .roles
                .get(role_name)
                .cloned()
                .ok_or_else(|| format!("unknown agent role: {role_name}"))?;
            let account = args
                .get("account")
                .and_then(Value::as_str)
                .or(role.account.as_deref())
                .ok_or_else(|| "agents needs account or a role account".to_string())?;
            let task = arg(&args, "task")?;
            let connected = crate::accounts::get_account(root, account)
                .ok_or_else(|| format!("unknown connected account: {account}"))?;
            let selected_provider = args
                .get("provider")
                .and_then(Value::as_str)
                .or(role.provider.as_deref());
            if let Some(provider) = selected_provider {
                if provider != connected.provider.as_str() {
                    return Err(format!(
                        "agent provider override {provider} does not match connected account {account} ({})",
                        connected.provider.as_str()
                    ));
                }
            }
            let id = uuid::Uuid::new_v4().to_string();
            let record = AgentRecord {
                agent_id: id.clone(),
                account: account.into(),
                provider: selected_provider
                    .unwrap_or(connected.provider.as_str())
                    .into(),
                role: role_name.into(),
                model: args
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or(role.model.clone()),
                effort: args
                    .get("effort")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or(role.effort.clone()),
                task: task.into(),
                parent: parent.clone(),
                workspace: Some(workspace.clone()),
                parent_run_id: Some(parent_run.clone()),
                run_id: None,
                status: "queued".into(),
                result: None,
                activity: vec!["delegation requested".into()],
            };
            write_agent(root, &record)?;
            if let Err(error) = write_proposal(
                root,
                &Proposal {
                    agent_id: id.clone(),
                    to: account.into(),
                    task: task.into(),
                    role: role_name.into(),
                    model: args
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or(role.model),
                    effort: args
                        .get("effort")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or(role.effort),
                    from_thread: record.parent.clone(),
                    workspace: Some(workspace),
                    parent_run_id: Some(parent_run),
                },
            ) {
                records::update_agent_exact(
                    root,
                    &id,
                    "failed",
                    Some(error.clone()),
                    "request publication failed",
                )?;
                return Err(error);
            }
            Ok(json!({"agent_id":id,"status":"queued","role":role_name}).to_string())
        }
        "status" | "result" | "watch" => {
            let id = arg(&args, "agent_id")?;
            let (parent, parent_run, workspace) = caller_context(&args)?;
            let record = if action == "watch" {
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    let current = read_agent(root, id)?;
                    if current.parent != parent
                        || current.parent_run_id.as_deref() != Some(parent_run.as_str())
                        || current.workspace.as_ref() != Some(&workspace)
                    {
                        return Err("agent read owner mismatch".into());
                    }
                    if matches!(
                        current.status.as_str(),
                        "finished"
                            | "success"
                            | "quota"
                            | "crashed"
                            | "cancelled"
                            | "uncertain"
                            | "failed"
                    ) || Instant::now() >= deadline
                    {
                        break current;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            } else {
                read_agent(root, id)?
            };
            if record.parent != parent
                || record.parent_run_id.as_deref() != Some(parent_run.as_str())
                || record.workspace.as_ref() != Some(&workspace)
            {
                return Err("agent read owner mismatch".into());
            }
            Ok(agent_view(root, record, action == "result")?.to_string())
        }
        "cancel" => {
            let id = arg(&args, "agent_id")?;
            let _lock = records::ControlLock::acquire(root, id)?;
            let record = read_agent(root, id)?;
            let (parent, parent_run, workspace) = caller_context(&args)?;
            if record.parent != parent
                || record.parent_run_id.as_deref() != Some(parent_run.as_str())
                || record.workspace.as_ref() != Some(&workspace)
            {
                return Err("agent cancellation owner mismatch".into());
            }
            if matches!(
                record.status.as_str(),
                "finished" | "success" | "quota" | "crashed" | "cancelled" | "uncertain" | "failed"
            ) {
                return Err("agent is already terminal; no action was replayed".into());
            }
            records::write_control(
                root,
                &records::AgentControl {
                    agent_id: id.into(),
                    action: "cancel".into(),
                    task: None,
                },
            )?;
            Ok(
                json!({"agent_id":id,"accepted":true,"status":"cancellation_requested"})
                    .to_string(),
            )
        }
        "followup" => {
            let source = read_agent(root, arg(&args, "agent_id")?)?;
            let task = arg(&args, "task")?;
            let (parent, parent_run, workspace) = caller_context(&args)?;
            if source.parent != parent
                || source.parent_run_id.as_deref() != Some(parent_run.as_str())
                || source.workspace.as_ref() != Some(&workspace)
            {
                return Err("agent follow-up owner mismatch".into());
            }
            let id = uuid::Uuid::new_v4().to_string();
            let record = AgentRecord {
                agent_id: id.clone(),
                account: source.account.clone(),
                provider: source.provider.clone(),
                role: source.role.clone(),
                model: source.model.clone(),
                effort: source.effort.clone(),
                task: task.into(),
                parent: parent.clone(),
                workspace: Some(workspace.clone()),
                parent_run_id: Some(parent_run.clone()),
                run_id: None,
                status: "queued".into(),
                result: None,
                activity: vec![format!("follow-up of {} requested", source.agent_id)],
            };
            write_agent(root, &record)?;
            if let Err(error) = write_proposal(
                root,
                &Proposal {
                    agent_id: id.clone(),
                    to: record.account.clone(),
                    task: record.task.clone(),
                    role: record.role.clone(),
                    model: record.model.clone(),
                    effort: record.effort.clone(),
                    from_thread: parent,
                    workspace: Some(workspace),
                    parent_run_id: Some(parent_run),
                },
            ) {
                records::update_agent_exact(
                    root,
                    &id,
                    "failed",
                    Some(error.clone()),
                    "follow-up publication failed",
                )?;
                return Err(error);
            }
            Ok(json!({"agent_id":id,"status":"queued","followup_of":source.agent_id}).to_string())
        }
        _ => Err(
            "agents action must be list, start, status, result, followup, cancel or watch".into(),
        ),
    }
}

fn agent_view(root: &Path, record: AgentRecord, result_only: bool) -> Result<Value, String> {
    let metadata = records::read_agent_metadata(root, &record.agent_id)?;
    let outcome = metadata.reason.as_deref().unwrap_or("unknown");
    let base = json!({"agent_id":record.agent_id,"status":record.status,"outcome":outcome,"result":record.result,"activity":record.activity,"last_event":metadata.last_event,"heartbeat_unix":metadata.heartbeat_unix,"run_id":record.run_id,"process_id":metadata.process_id,"native_session":metadata.native_session});
    Ok(if result_only {
        base
    } else {
        json!({"agent_id":record.agent_id,"account":record.account,"provider":record.provider,"status":record.status,"outcome":outcome,"run_id":record.run_id,"process_id":metadata.process_id,"native_session":metadata.native_session,"result":record.result,"activity":record.activity,"last_event":metadata.last_event,"heartbeat_unix":metadata.heartbeat_unix})
    })
}
fn write_proposal(root: &Path, proposal: &Proposal) -> Result<(), String> {
    let dir = proposals_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::fs_safe::resolved_directory(&dir)
        .ok_or("proposal directory is linked or unavailable")?;
    if std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .take(MAX_PENDING_PROPOSALS)
        .count()
        >= MAX_PENDING_PROPOSALS
    {
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
    let dir = proposals_dir(root);
    let Some(owned) = crate::fs_safe::resolved_directory(&dir) else {
        return out;
    };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten().take(limit.min(MAX_PENDING_PROPOSALS)) {
        let p = e.path();
        if !crate::fs_safe::regular_owned_file(&p, &owned) {
            continue;
        }
        if let Some(text) = crate::fs_safe::read_text_bounded(&p, MAX_PROPOSAL_FILE as usize) {
            if let Ok(v) = serde_json::from_str(&text) {
                let _ = std::fs::remove_file(p);
                out.push(v);
            }
        }
    }
    out
}

pub fn take_pending_for(
    root: &Path,
    limit: usize,
    parent: &str,
    workspace: &Path,
    parent_run: &str,
) -> Vec<Proposal> {
    if limit == 0 {
        return Vec::new();
    }
    let Ok(workspace) = workspace.canonicalize() else {
        return Vec::new();
    };
    let directory = proposals_dir(root);
    let Some(owned) = crate::fs_safe::resolved_directory(&directory) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten().take(MAX_PENDING_PROPOSALS * 4) {
        let path = entry.path();
        if !crate::fs_safe::regular_owned_file(&path, &owned) {
            continue;
        }
        let Some(raw) = crate::fs_safe::read_text_bounded(&path, MAX_PROPOSAL_FILE as usize) else {
            continue;
        };
        let Ok(proposal) = serde_json::from_str::<Proposal>(&raw) else {
            continue;
        };
        if proposal.from_thread != parent
            || proposal.workspace.as_ref() != Some(&workspace)
            || proposal.parent_run_id.as_deref() != Some(parent_run)
        {
            continue;
        }
        let Ok(record) = records::read_agent(root, &proposal.agent_id) else {
            continue;
        };
        if record.parent != parent
            || record.workspace.as_ref() != Some(&workspace)
            || record.parent_run_id.as_deref() != Some(parent_run)
            || path.file_name().and_then(|name| name.to_str())
                != Some(format!("{}.json", proposal.agent_id).as_str())
        {
            continue;
        }
        if record.status != "queued" {
            continue;
        }
        if records::peek_control(root, &proposal.agent_id)
            .is_some_and(|control| control.action == "cancel")
        {
            if records::update_agent_exact(
                root,
                &proposal.agent_id,
                "cancelled",
                None,
                "queued cancellation accepted before launch",
            )
            .is_ok()
            {
                let _ = records::ack_control(root, &proposal.agent_id);
                let _ = std::fs::remove_file(&path);
            }
            continue;
        }
        let claim = path.with_extension(format!("claim-{}", uuid::Uuid::new_v4()));
        if std::fs::rename(&path, &claim).is_err() {
            continue;
        }
        let _ = std::fs::remove_file(claim);
        out.push(proposal);
        if out.len() == limit {
            break;
        }
    }
    out
}
pub fn has_proposals(root: &Path) -> bool {
    std::fs::read_dir(proposals_dir(root)).is_ok_and(|mut r| r.next().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    #[test]
    fn agent_record_storage_redacts_every_user_supplied_field() {
        let root = tempfile::tempdir().unwrap();
        let secret = "agent-record-secret-20261112";
        crate::redact::register_runtime_secret(secret);
        let record = AgentRecord {
            agent_id: "agent-record-redaction".into(),
            account: secret.into(),
            provider: "synthetic".into(),
            role: "general".into(),
            model: Some(secret.into()),
            effort: Some(secret.into()),
            task: secret.into(),
            parent: secret.into(),
            workspace: None,
            parent_run_id: None,
            run_id: None,
            status: "queued".into(),
            result: Some(secret.into()),
            activity: vec![secret.into()],
        };

        write_agent(root.path(), &record).unwrap();
        let raw =
            std::fs::read_to_string(record_path(root.path(), &record.agent_id).unwrap()).unwrap();
        assert!(!raw.contains(secret));
        let stored = read_agent(root.path(), &record.agent_id).unwrap();
        assert_ne!(stored.account, secret);
        assert_ne!(stored.model.as_deref(), Some(secret));
        assert_ne!(stored.task, secret);
        assert_ne!(stored.parent, secret);
        assert_ne!(stored.result.as_deref(), Some(secret));
        assert_ne!(stored.activity[0], secret);
    }

    #[test]
    fn tool_schema_names_role_defaults_and_override_fields() {
        let root = tempfile::tempdir().unwrap();
        let line = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string();
        let reply = handle_line(root.path(), &line).unwrap();
        assert!(reply.contains("role"));
        assert!(reply.contains("explicit provider/model"));
    }

    #[cfg(windows)]
    #[test]
    fn remote_tool_result_is_unwrapped_and_credential_manager_header_is_used() {
        use std::ffi::OsStr;
        use std::iter::once;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::Security::Credentials::*;

        let target_name = "ConcordiaSyntheticMcpReview";
        let target: Vec<u16> = OsStr::new(target_name)
            .encode_wide()
            .chain(once(0))
            .collect();
        let mut blob = b"synthetic-token".to_vec();
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_ptr() as *mut u16,
            Comment: std::ptr::null_mut(),
            LastWritten: FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_SESSION,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: std::ptr::null_mut(),
            UserName: std::ptr::null_mut(),
        };
        assert_ne!(unsafe { CredWriteW(&credential, 0) }, 0);

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    let count = stream.read(&mut chunk).unwrap();
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                    let Some(header_end) = bytes
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                        .map(|index| index + 4)
                    else {
                        continue;
                    };
                    let head = String::from_utf8_lossy(&bytes[..header_end]);
                    let content_length = head
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("Content-Length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if bytes.len() >= header_end + content_length {
                        break;
                    }
                }
                let wire = String::from_utf8(bytes).unwrap();
                assert!(wire.contains("synthetic-token"));
                use std::io::Write;
                if wire.contains("notifications/initialized") {
                    write!(
                        stream,
                        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .unwrap();
                    continue;
                }
                let body = if wire.contains("\"method\":\"initialize\"") {
                    format!(
                        r#"{{"jsonrpc":"2.0","id":0,"result":{{"protocolVersion":"{LATEST_PROTOCOL}","capabilities":{{}},"serverInfo":{{"name":"synthetic","version":"0"}}}}}}"#
                    )
                } else {
                    r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"synthetic result"}]}}"#.into()
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let root = tempfile::tempdir().unwrap();
        let mut registry = crate::mcp_registry::Registry {
            servers: std::collections::BTreeMap::new(),
            projects: std::collections::BTreeMap::new(),
        };
        registry.servers.insert(
            "synthetic".into(),
            crate::mcp_registry::Server {
                enabled: true,
                transport: crate::mcp_registry::Transport::Remote {
                    url: format!("http://{address}/mcp"),
                    headers: std::collections::BTreeMap::from([(
                        String::from("Authorization"),
                        crate::mcp_registry::SecretRef::Credential {
                            credential: target_name.into(),
                        },
                    )]),
                },
            },
        );
        crate::mcp_registry::save(root.path(), &registry).unwrap();
        let line = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"synthetic/tool","arguments":{}}}"#;
        let reply: Value = serde_json::from_str(&handle_line(root.path(), line).unwrap()).unwrap();
        assert_eq!(reply["result"]["content"][0]["text"], "synthetic result");
        thread.join().unwrap();
        unsafe {
            CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0);
        }
    }
}
