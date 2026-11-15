//! The single always-on MCP surface: account discovery and delegated agents.
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

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
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok((!out.is_empty()).then_some(out));
        }
        let take = chunk
            .iter()
            .position(|b| *b == b'\n')
            .map_or(chunk.len(), |i| i + 1);
        if out.len().saturating_add(take) > limit {
            input.consume(take);
            return Ok(Some(Vec::new()));
        }
        out.extend_from_slice(&chunk[..take]);
        input.consume(take);
        if take < chunk.len() || out.last() == Some(&b'\n') {
            return Ok(Some(out));
        }
    }
}

pub fn proposals_dir(root: &Path) -> PathBuf {
    root.join("proposals")
}
pub mod records;
pub use records::{
    agents_dir, read_agent, record_path, update_agent, update_matching, write_agent, AgentRecord,
};
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

/// Names returned by this server's `tools/list` response. Callers use this registry when
/// describing a session, so the standing context cannot drift from the registered surface.
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
        json!({"name":"agents","description":"Start an agent with role (orchestrator, implementer, reviewer, re-reviewer, general) defaults; explicit provider/model win, or read status/result.","inputSchema":{"type":"object","additionalProperties":false,"properties":{"action":{"enum":["list","start","status","result"]},"agent_id":{"type":"string"},"role":{"type":"string"},"account":{"type":"string"},"provider":{"type":"string"},"routine":{"type":"string"},"model":{"type":"string"},"effort":{"type":"string"},"task":{"type":"string"},"parent":{"type":"string"}},"required":["action"]}}),
    ];
    let Ok(registry) = crate::mcp_registry::load(root) else {
        return tools;
    };
    for (server, entry) in registry.servers {
        if !entry.enabled {
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

fn call_remote(root: &Path, params: &Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "remote MCP call needs a tool name".to_string())?;
    let (server, tool) = name
        .split_once('/')
        .ok_or_else(|| "unknown tool".to_string())?;
    let registry =
        crate::mcp_registry::load(root).map_err(|_| "MCP registry cannot be read".to_string())?;
    // The caller cannot choose the policy scope. The proxy process binds its project at
    // launch; absent an authenticated binding, use its own working directory.
    let project = std::env::var_os("CONCORDIA_PROJECT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
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

fn remote_request(
    transport: &crate::mcp_registry::Transport,
    request: Value,
) -> Result<Value, String> {
    if let crate::mcp_registry::Transport::Stdio { command, args, env } = transport {
        let mut child = crate::process_ui::command(command);
        child.args(args);
        for (key, reference) in env {
            let value = crate::mcp_registry::resolve_secret(reference).ok_or_else(|| {
                "MCP credential is not available; sign in before using this server".to_string()
            })?;
            crate::redact::register_runtime_secret(value.clone());
            child.env(key, value);
        }
        let mut child = child
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|_| "MCP stdio server could not start".to_string())?;
        let input = serde_json::to_string(&request)
            .map_err(|_| "MCP request could not be encoded".to_string())?;
        use std::io::Write as _;
        use std::io::{BufRead, BufReader};
        child
            .stdin
            .take()
            .ok_or_else(|| "MCP stdio input unavailable".to_string())?
            .write_all(format!("{input}\n").as_bytes())
            .map_err(|_| "MCP stdio request failed".to_string())?;
        let mut line = Vec::new();
        BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| "MCP stdio output unavailable".to_string())?,
        )
        .read_until(b'\n', &mut line)
        .map_err(|_| "MCP stdio response failed".to_string())?;
        if line.len() > MAX_UPSTREAM_FRAME {
            return Err("MCP stdio response exceeded the frame limit".into());
        }
        let _ = child.kill();
        return serde_json::from_slice(&line)
            .map_err(|_| "MCP stdio response was not JSON".to_string());
    }
    let (url, headers) = match transport {
        crate::mcp_registry::Transport::Remote { url, headers } => (url, headers),
        _ => unreachable!(),
    };
    if url.starts_with("https://") {
        #[cfg(windows)]
        {
            return remote_https_request(url, headers, request);
        }
        #[cfg(not(windows))]
        {
            return Err("HTTPS MCP proxy requires Windows WinHTTP".into());
        }
    }
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| "remote MCP proxy requires an http URL".to_string())?;
    let (host_path, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
    let (host, port) = host_path
        .split_once(':')
        .map_or((host_path, 80), |(h, p)| (h, p.parse().unwrap_or(80)));
    let address = (host, port)
        .to_socket_addrs()
        .map_err(|_| "remote MCP host could not be resolved".to_string())?
        .next()
        .ok_or_else(|| "remote MCP host could not be resolved".to_string())?;
    let mut stream =
        TcpStream::connect(address).map_err(|_| "remote MCP connection failed".to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(10))).ok();
    let mut request_headers = String::new();
    for (key, reference) in headers {
        let crate::mcp_registry::SecretRef::Credential { .. } = reference else {
            return Err("remote MCP headers must use Windows Credential Manager".into());
        };
        if let Some(value) = crate::mcp_registry::resolve_secret(reference) {
            crate::redact::register_runtime_secret(value.clone());
            request_headers.push_str(&format!("{key}: {value}\r\n"));
        } else {
            return Err("remote MCP credential is not available; sign in through Windows Credential Manager".into());
        }
    }
    let body = serde_json::to_string(&request)
        .map_err(|_| "remote MCP request could not be encoded".to_string())?;
    let wire = format!("POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: {LATEST_PROTOCOL}\r\nContent-Length: {}\r\nConnection: close\r\n{request_headers}\r\n{body}", body.len());
    stream
        .write_all(wire.as_bytes())
        .map_err(|_| "remote MCP request failed".to_string())?;
    let mut response = Vec::new();
    stream
        .take((MAX_UPSTREAM_BYTES + 1) as u64)
        .read_to_end(&mut response)
        .map_err(|_| "remote MCP response failed".to_string())?;
    if response.len() > MAX_UPSTREAM_BYTES {
        return Err("remote MCP response exceeded the size limit".into());
    }
    let text = String::from_utf8_lossy(&response);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "remote MCP response was malformed".to_string())?;
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return Err("remote MCP server rejected the request".into());
    }
    serde_json::from_str(body).map_err(|_| "remote MCP response was not JSON".to_string())
}

fn remote_sequence(
    transport: &crate::mcp_registry::Transport,
    requests: Vec<Value>,
) -> Result<Vec<Value>, String> {
    if let crate::mcp_registry::Transport::Stdio { command, args, env } = transport {
        let mut child = crate::process_ui::command(command);
        child.args(args);
        for (key, reference) in env {
            let value = crate::mcp_registry::resolve_secret(reference).ok_or_else(|| {
                "MCP credential is not available; sign in before using this server".to_string()
            })?;
            crate::redact::register_runtime_secret(value.clone());
            child.env(key, value);
        }
        let mut child = child
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|_| "MCP stdio server could not start".to_string())?;
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| "MCP stdio input unavailable".to_string())?;
        let mut output = std::io::BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| "MCP stdio output unavailable".to_string())?,
        );
        let mut values = Vec::with_capacity(requests.len());
        for (index, request) in requests.into_iter().enumerate() {
            input
                .write_all(
                    serde_json::to_string(&request)
                        .map_err(|_| "MCP request could not be encoded".to_string())?
                        .as_bytes(),
                )
                .map_err(|_| "MCP stdio request failed".to_string())?;
            input
                .write_all(b"\n")
                .map_err(|_| "MCP stdio request failed".to_string())?;
            input
                .flush()
                .map_err(|_| "MCP stdio request failed".to_string())?;
            if index == 0 {
                input
                    .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
                    .map_err(|_| "MCP stdio notification failed".to_string())?;
                input
                    .flush()
                    .map_err(|_| "MCP stdio notification failed".to_string())?;
            }
            loop {
                let mut line = Vec::new();
                output
                    .read_until(b'\n', &mut line)
                    .map_err(|_| "MCP stdio response failed".to_string())?;
                if line.len() > MAX_UPSTREAM_FRAME {
                    return Err("MCP stdio response exceeded the frame limit".into());
                }
                let value: Value = serde_json::from_slice(&line)
                    .map_err(|_| "MCP stdio response was not JSON".to_string())?;
                if value.get("id") == request.get("id") {
                    values.push(value);
                    break;
                }
            }
        }
        let _ = child.kill();
        return Ok(values);
    }
    requests
        .into_iter()
        .map(|request| remote_request(transport, request))
        .collect()
}

#[cfg(windows)]
fn remote_https_request(
    url: &str,
    headers: &std::collections::BTreeMap<String, crate::mcp_registry::SecretRef>,
    request: Value,
) -> Result<Value, String> {
    use std::iter::once;
    use windows_sys::Win32::Networking::WinHttp::*;
    let rest = url
        .strip_prefix("https://")
        .ok_or("invalid HTTPS MCP URL")?;
    let (hostport, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_owned()), |(h, p)| (h, format!("/{p}")));
    let mut parts = hostport.split(':');
    let host = parts.next().unwrap_or("");
    let port = parts.next().and_then(|p| p.parse().ok()).unwrap_or(443);
    let wide = |s: &str| s.encode_utf16().chain(once(0)).collect::<Vec<u16>>();
    let agent = wide("Concordia");
    let host_w = wide(host);
    let path_w = wide(&path);
    let verb = wide("POST");
    let session = unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    };
    if session.is_null() {
        return Err("HTTPS MCP session could not start".into());
    }
    let connection = unsafe { WinHttpConnect(session, host_w.as_ptr(), port, 0) };
    if connection.is_null() {
        unsafe {
            WinHttpCloseHandle(session);
        }
        return Err("HTTPS MCP connection failed".into());
    }
    let handle = unsafe {
        WinHttpOpenRequest(
            connection,
            verb.as_ptr(),
            path_w.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    };
    if handle.is_null() {
        unsafe {
            WinHttpCloseHandle(connection);
            WinHttpCloseHandle(session);
        }
        return Err("HTTPS MCP request could not start".into());
    }
    let mut header_text = String::from("Content-Type: application/json\r\n");
    for (key, reference) in headers {
        let value = crate::mcp_registry::resolve_secret(reference).ok_or(
            "remote MCP credential is not available; sign in through Windows Credential Manager",
        )?;
        crate::redact::register_runtime_secret(value.clone());
        header_text.push_str(&format!("{key}: {value}\r\n"));
    }
    let header_w = wide(&header_text);
    let body = serde_json::to_string(&request)
        .map_err(|_| "MCP request could not be encoded".to_string())?;
    let ok = unsafe {
        WinHttpSendRequest(
            handle,
            header_w.as_ptr(),
            u32::MAX,
            body.as_ptr() as _,
            body.len() as u32,
            body.len() as u32,
            0,
        ) != 0
            && WinHttpReceiveResponse(handle, std::ptr::null_mut()) != 0
    };
    let mut bytes = Vec::new();
    if ok {
        loop {
            let mut size = 0u32;
            unsafe {
                WinHttpQueryDataAvailable(handle, &mut size);
            }
            if size == 0 {
                break;
            }
            let mut chunk = vec![0u8; size as usize];
            let mut read = 0u32;
            if unsafe { WinHttpReadData(handle, chunk.as_mut_ptr() as _, size, &mut read) } == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..read as usize]);
        }
    }
    unsafe {
        WinHttpCloseHandle(handle);
        WinHttpCloseHandle(connection);
        WinHttpCloseHandle(session);
    }
    if !ok {
        return Err("HTTPS MCP request failed".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "HTTPS MCP response was not JSON".into())
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
            let id = uuid::Uuid::new_v4().to_string();
            let record = AgentRecord {
                agent_id: id.clone(),
                account: account.into(),
                provider: args
                    .get("provider")
                    .and_then(Value::as_str)
                    .or(role.provider.as_deref())
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
                },
            )?;
            Ok(json!({"agent_id":id,"status":"queued","role":role_name}).to_string())
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
            status: "queued".into(),
            result: Some(secret.into()),
            activity: vec![secret.into()],
        };

        write_agent(root.path(), &record).unwrap();
        let raw = std::fs::read_to_string(record_path(root.path(), &record.agent_id)).unwrap();
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
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = [0u8; 4096];
                let count = stream.read(&mut bytes).unwrap();
                let wire = String::from_utf8(bytes[..count].to_vec()).unwrap();
                assert!(wire.contains("synthetic-token"));
                let body = r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"synthetic result"}]}}"#;
                use std::io::Write;
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
