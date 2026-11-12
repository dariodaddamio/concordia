//! `concordia mcp`: a minimal stdio MCP server over Concordia's own data (docs/mcp.md).
//!
//! JSON-RPC 2.0, one message per line. Data reads and bounded task/goal proposals; goal mutation
//! happens only after explicit local UI review. No network, no process
//! spawning. The handler is a pure function of (state folder, request line).

use crate::threads;
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

/// Newest protocol revision implemented.
pub const LATEST_PROTOCOL: &str = "2025-06-18";
/// Revisions this server answers as-is (the surface used here is identical across them).
const KNOWN_PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
/// A request line larger than this is refused.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
/// Cap for any tool result text, in characters.
pub const MAX_RESULT_CHARS: usize = 20_000;
const DEFAULT_READ_CHARS: usize = threads::READ_CHARS;
/// Pending proposal files beyond this are refused (a runaway agent cannot fill the folder).
pub const MAX_PENDING_PROPOSALS: usize = 10;
const MAX_PROPOSAL_FILE: u64 = 16 * 1024;

#[derive(Debug, Clone)]
pub struct Scope {
    pub account: String,
    pub workspace: PathBuf,
    unrestricted: bool,
}

impl Scope {
    fn from_env() -> Result<Self, String> {
        let raw = std::env::var(crate::integration::SCOPE_ENV)
            .map_err(|_| "MCP server is unscoped; history reads are disabled".to_string())?;
        let value: Value = serde_json::from_str(&raw)
            .map_err(|_| "MCP server scope is malformed; history reads are disabled".to_string())?;
        let account = value
            .get("account")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("MCP server scope has no account; history reads are disabled")?;
        let workspace = value
            .get("workspace")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("MCP server scope has no workspace; history reads are disabled")?;
        let workspace = std::fs::canonicalize(workspace)
            .map_err(|_| "MCP server scope workspace is unavailable; history reads are disabled")?;
        if !workspace.is_dir() {
            return Err(
                "MCP server scope workspace is not a directory; history reads are disabled".into(),
            );
        }
        Ok(Self {
            account: account.into(),
            workspace,
            unrestricted: false,
        })
    }
}

pub fn proposals_dir(state_root: &Path) -> PathBuf {
    state_root.join("proposals")
}

// ---------------------------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------------------------

/// Serve until the input closes. One reply line per request; notifications get none.
pub fn serve<R: BufRead, W: Write>(
    mut input: R,
    mut output: W,
    state_root: &Path,
) -> std::io::Result<()> {
    let scope = Scope::from_env();
    loop {
        let mut buf = Vec::new();
        let n = (&mut input)
            .take(MAX_REQUEST_BYTES as u64 + 1)
            .read_until(b'\n', &mut buf)?;
        if n == 0 {
            return Ok(());
        }
        let oversized = buf.len() > MAX_REQUEST_BYTES;
        if oversized && buf.last() != Some(&b'\n') {
            // Discard the rest of this line without holding it in memory.
            loop {
                let mut sink = Vec::new();
                let m = (&mut input).take(8192).read_until(b'\n', &mut sink)?;
                if m == 0 || sink.last() == Some(&b'\n') {
                    break;
                }
            }
        }
        let reply = if oversized {
            Some(error_reply(Value::Null, -32600, "request too large"))
        } else {
            match std::str::from_utf8(&buf) {
                Ok(line) if line.trim().is_empty() => None,
                Ok(line) => handle_line_scoped(state_root, line, scope.as_ref().ok()),
                Err(_) => Some(error_reply(Value::Null, -32700, "parse error: not UTF-8")),
            }
        };
        if let Some(r) = reply {
            output.write_all(r.as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// JSON-RPC handler (pure)
// ---------------------------------------------------------------------------------------------

fn error_reply(id: Value, code: i64, message: &str) -> String {
    let message = crate::redact::redact(message);
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.as_ref()}}).to_string()
}

fn result_reply(id: Value, result: Value) -> String {
    json!({"jsonrpc":"2.0","id":id,"result":result}).to_string()
}

/// One request line in, the reply line out (None for notifications).
pub fn handle_line(state_root: &Path, line: &str) -> Option<String> {
    handle_line_scoped(state_root, line, None)
}

fn handle_line_scoped(state_root: &Path, line: &str, scope: Option<&Scope>) -> Option<String> {
    if line.len() > MAX_REQUEST_BYTES {
        return Some(error_reply(Value::Null, -32600, "request too large"));
    }
    let msg: Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(_) => return Some(error_reply(Value::Null, -32700, "parse error")),
    };
    let Some(obj) = msg.as_object() else {
        // Batches are not supported (removed from the protocol in 2025-06-18).
        return Some(error_reply(
            Value::Null,
            -32600,
            "invalid request: expected one JSON object",
        ));
    };
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // A response or garbage from the client: nothing to answer.
        return if obj.contains_key("id")
            && !obj.contains_key("result")
            && !obj.contains_key("error")
        {
            Some(error_reply(
                obj["id"].clone(),
                -32600,
                "invalid request: no method",
            ))
        } else {
            None
        };
    };
    let Some(id) = obj
        .get("id")
        .cloned()
        .filter(|i| i.is_string() || i.is_number())
    else {
        return None; // notification (initialized, cancelled, ...): never answered
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => result_reply(id, initialize(&params)),
        "ping" => result_reply(id, json!({})),
        "tools/list" => result_reply(id, json!({"tools": tool_list()})),
        "tools/call" => match call_tool(state_root, &params, scope) {
            Ok(r) => result_reply(id, r),
            Err(m) => error_reply(id, -32602, &m),
        },
        other => error_reply(id, -32601, &format!("method not found: {other}")),
    })
}

fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = match asked {
        Some(v) if KNOWN_PROTOCOLS.contains(&v) => v,
        _ => LATEST_PROTOCOL,
    };
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {}},
        "serverInfo": {"name": "concordia", "version": crate::version::VERSION},
    })
}

/// Tool list. Descriptions and schemas are kept terse: they cost tokens on every turn.
fn tool_list() -> Value {
    json!([{
        "name": "agents",
        "description": "List accounts/models, start an agent, or read its status/result.",
        "inputSchema": {"type":"object","additionalProperties":false,"properties":{
            "action":{"enum":["list","start","status","result"]},
            "agent_id":{"type":"string"}, "account":{"type":"string"},
            "provider":{"type":"string"}, "model":{"type":"string"},
            "task":{"type":"string"}, "parent":{"type":"string"}
        },"required":["action"]}
    }])
}

fn text_result(text: String, is_error: bool) -> Value {
    let redacted = crate::redact::redact(&text);
    if redacted.chars().count() > MAX_RESULT_CHARS && serde_json::from_str::<Value>(&text).is_ok() {
        return json!({"content":[{"type":"text","text":"Structured result exceeds the response budget; request a smaller page or narrow the query."}],"isError":true});
    }
    let changed = redacted.as_ref() != text;
    let mut result = json!({"content":[{"type":"text","text":cap(redacted.as_ref(), MAX_RESULT_CHARS)}],"isError":is_error});
    if changed {
        result["redacted"] = json!(true);
        result["content"].as_array_mut().unwrap().push(
            json!({"type":"text","text":"Some stored text was redacted before returning it."}),
        );
    }
    result
}

fn cap(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}\n[truncated]", &s[..i]),
        None => s.to_string(),
    }
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn uint_arg(args: &Value, key: &str) -> Option<usize> {
    args.get(key).and_then(Value::as_u64).map(|n| n as usize)
}

fn tool_goals(root: &Path, args: &Value) -> Result<String, String> {
    if args.as_object().is_some_and(|args| {
        args.keys().any(|key| {
            !matches!(
                key.as_str(),
                "action" | "goal" | "workspace" | "offset" | "revision" | "text" | "request_id"
            )
        })
    }) || [
        "action",
        "goal",
        "workspace",
        "revision",
        "text",
        "request_id",
    ]
    .iter()
    .any(|key| args.get(*key).is_some_and(|value| !value.is_string()))
    {
        return Err(
            "invalid goal tool arguments; requester identity cannot be supplied by the client"
                .into(),
        );
    }
    let action = str_arg(args, "action").unwrap_or("read");
    if action != "read" {
        let request =
            str_arg(args, "request_id").ok_or("goal proposals/status need request_id UUID")?;
        if action == "status" {
            if ["goal", "workspace", "offset", "revision", "text"]
                .iter()
                .any(|key| args.get(*key).is_some())
            {
                return Err("goal status takes only action and request_id".into());
            }
            return crate::goals::changes::status(root, request);
        }
        let revision = str_arg(args, "revision")
            .ok_or("read current goals first and supply their revision")?;
        if args.get("workspace").is_some() || args.get("offset").is_some() {
            return Err("goal proposals do not take source/cursor arguments".into());
        }
        let proposal = match action {
            "propose_add" if args.get("goal").is_none() => crate::goals::changes::propose_add(
                root,
                request,
                revision,
                str_arg(args, "text").ok_or("goal add proposal needs text")?,
            )?,
            "propose_close" if args.get("text").is_none() => crate::goals::changes::propose_close(
                root,
                request,
                revision,
                str_arg(args, "goal").ok_or("goal close proposal needs exact goal id")?,
            )?,
            _ => return Err("unsupported goal action or incompatible arguments".into()),
        };
        if let Some(status) = crate::goals::changes::existing_status(root, &proposal)? {
            return Ok(status);
        }
        record_pending(
            root,
            request,
            &serde_json::to_vec(&proposal).map_err(|_| "goal proposal serialization failed")?,
        )?;
        return Ok(json!({"status":"proposal_recorded","request_id":request,"goal":proposal.goal_id(),"revision":proposal.observed_revision,"approval":"local user must review and approve; no goal changed","requesting_actor":"unverified MCP client"}).to_string());
    }
    if args.get("text").is_some() || args.get("request_id").is_some() {
        return Err("goal reads do not take proposal fields".into());
    }
    if args.get("offset").is_some_and(|offset| !offset.is_u64())
        || args
            .get("revision")
            .is_some_and(|revision| !revision.is_string())
    {
        return Err("goal cursor needs a nonnegative offset and string revision".into());
    }
    let offset = uint_arg(args, "offset").unwrap_or(0);
    let revision = str_arg(args, "revision");
    let value = if let Some(goal) = str_arg(args, "goal") {
        if goal.is_empty() || goal.len() > 128 {
            return Err("invalid goal id".into());
        }
        let workspace =
            str_arg(args, "workspace").ok_or("goal/source reads need the exact workspace")?;
        crate::native_memory::relations::list(
            root,
            Some(Path::new(workspace)),
            Some(goal),
            offset,
            revision,
        )?
    } else {
        if args.get("goal").is_some() || args.get("workspace").is_some() {
            return Err("workspace is used only with a goal id".into());
        }
        crate::native_memory::relations::goals(root, offset, revision)?
    };
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

fn tool_memory(root: &Path, args: &Value, scope: &Scope) -> Result<String, String> {
    let object = args.as_object().ok_or("memory needs an object")?;
    let action = str_arg(args, "action").ok_or("memory needs an action")?;
    let allowed: &[&str] = match action {
        "recall" => &["action", "query", "workspace", "source", "account"],
        "ranked" => &[
            "action",
            "query",
            "workspace",
            "source",
            "account",
            "goal",
            "file",
        ],
        "files" => &["action", "source", "offset", "revision"],
        "groups" => &["action", "workspace", "offset", "revision"],
        "members" => &["action", "group", "offset", "revision"],
        _ => return Err("unsupported read-only memory action".into()),
    };
    for (key, value) in object {
        if !allowed.contains(&key.as_str())
            || if key == "offset" {
                !value.is_u64()
            } else {
                !value.is_string()
            }
        {
            return Err("unsupported or mistyped memory argument".into());
        }
    }
    let required = |key| {
        str_arg(args, key)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("memory {action} needs {key}"))
    };
    let offset = uint_arg(args, "offset").unwrap_or(0);
    let revision = str_arg(args, "revision");
    if !scope.unrestricted && matches!(action, "files" | "groups" | "members") {
        return Err("this memory read is unavailable through scoped MCP".into());
    }
    if !scope.unrestricted {
        if let Some(account) = str_arg(args, "account") {
            if account != scope.account {
                return Err("MCP scope rejects account outside the registered scope".into());
            }
        }
    }
    if !scope.unrestricted {
        if let Some(workspace) = str_arg(args, "workspace") {
            let requested =
                std::fs::canonicalize(workspace).map_err(|_| "workspace is unavailable")?;
            if !requested.starts_with(&scope.workspace) {
                return Err("MCP scope rejects workspace outside the registered scope".into());
            }
        }
    }
    let value = match action {
        "recall" => serde_json::to_value(crate::native_memory::retrieval::recall(
            root,
            required("query")?,
            str_arg(args, "source"),
            if scope.unrestricted {
                str_arg(args, "account")
            } else {
                Some(&scope.account)
            },
            if scope.unrestricted {
                str_arg(args, "workspace").map(Path::new)
            } else {
                Some(&scope.workspace)
            },
        )?)
        .map_err(|error| error.to_string())?,
        "ranked" => serde_json::to_value(crate::native_memory::rank::recall(
            root,
            crate::native_memory::rank::Request {
                text: required("query")?,
                source: str_arg(args, "source"),
                account: if scope.unrestricted {
                    str_arg(args, "account")
                } else {
                    Some(&scope.account)
                },
                workspace: if scope.unrestricted {
                    str_arg(args, "workspace").map(Path::new)
                } else {
                    Some(&scope.workspace)
                },
                goal: str_arg(args, "goal"),
                file: str_arg(args, "file"),
                from: None,
                at: None,
            },
        )?)
        .map_err(|error| error.to_string())?,
        "groups" => crate::native_memory::relations::groups::list(
            root,
            Path::new(required("workspace")?),
            offset,
            revision,
        )?,
        "members" => crate::native_memory::relations::groups::members(
            root,
            required("group")?,
            offset,
            revision,
        )?,
        "files" => {
            let mut argv = vec!["file-facts".into(), required("source")?.to_owned()];
            if offset != 0 || revision.is_some() {
                argv.extend([
                    offset.to_string(),
                    revision.ok_or("file cursor requires revision")?.to_owned(),
                ]);
            }
            crate::native_memory::files::command(root, &argv)?
        }
        _ => unreachable!(),
    };
    let text = serde_json::to_string(&value).map_err(|error| error.to_string())?;
    if text.chars().count() > MAX_RESULT_CHARS {
        return Err("memory result exceeds response budget; narrow the query with source, account or workspace".into());
    }
    Ok(text)
}

/// Err is a protocol error (unknown tool, bad params shape); tool failures are `isError` results.
fn call_tool(state_root: &Path, params: &Value, scope: Option<&Scope>) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("tools/call needs a tool name")?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !args.is_object() {
        return Err("arguments must be an object".into());
    }
    let out = match name {
        "agents" => tool_agents(state_root, &args, scope),
        other => return Err(format!("unknown tool: {other}")),
    };
    Ok(match out {
        Ok(t) => text_result(t, false),
        Err(e) => text_result(e, true),
    })
}

fn tool_agents(state_root: &Path, args: &Value, scope: Option<&Scope>) -> Result<String, String> {
    let action = str_arg(args, "action").ok_or("agents needs action")?;
    match action {
        "list" => {
            let accounts = crate::accounts::list_all_accounts(state_root)
                .into_iter()
                .filter(|account| {
                    scope.is_none_or(|scope| {
                        scope.unrestricted || account.account == scope.account
                    })
                })
                .map(|account| {
                    let models = account
                        .home_path
                        .as_deref()
                        .map(|home| crate::catalog::list_models(account.provider, home))
                        .unwrap_or_default();
                    let usage = account.home_path.as_deref().and_then(|home| {
                        crate::usage::cached_usage_view(
                            state_root,
                            account.provider,
                            &account.account,
                            home,
                        )
                    });
                    json!({
                        "account": account.account,
                        "provider": account.provider.as_str(),
                        "models": models,
                        "usage": usage.map(|u| json!({"fresh":u.fresh,"observed_at":u.observed_at,"windows":u.windows})).unwrap_or(Value::Null)
                    })
                })
                .collect::<Vec<_>>();
            serde_json::to_string(&json!({"accounts": accounts})).map_err(|e| e.to_string())
        }
        "start" => {
            let account = str_arg(args, "account").ok_or("agents start needs account")?;
            let task = str_arg(args, "task").ok_or("agents start needs task")?;
            let connected = crate::accounts::get_account(state_root, account)
                .ok_or_else(|| format!("unknown connected account: {account}"))?;
            if let Some(provider) = str_arg(args, "provider") {
                if provider != connected.provider.as_str() {
                    return Err(format!(
                        "account {account} belongs to {}, not {provider}",
                        connected.provider.as_str()
                    ));
                }
            }
            if str_arg(args, "model").is_some_and(|model| model.is_empty()) {
                return Err("model must not be empty".into());
            }
            if connected.home_path.is_none() {
                return Err(format!("unknown connected account: {account}"));
            }
            tool_delegate(state_root, &json!({
                "to": account,
                "task": task,
                "from_thread": str_arg(args, "parent").unwrap_or("")
            }))
        }
        "status" | "result" => {
            let id = str_arg(args, "agent_id").ok_or("agents status needs agent_id")?;
            let path = proposals_dir(state_root).join(format!("{id}.json"));
            if path.is_file() {
                Ok(json!({"agent_id":id,"status":"queued","result":Value::Null}).to_string())
            } else {
                Err(format!("unknown delegated agent: {id}"))
            }
        }
        _ => Err("agents action must be list, start, status or result".into()),
    }
}

fn tool_feed(root: &Path, args: &Value, scope: &Scope) -> Result<String, String> {
    let cursor = uint_arg(args, "cursor").unwrap_or(0);
    let limit = uint_arg(args, "limit").unwrap_or(50).min(200);
    let pane = str_arg(args, "pane");
    let mut events = crate::feed::read(root, 0, pane).map_err(|e| e.to_string())?;
    if !scope.unrestricted {
        events.retain(|e| e.account.as_deref() == Some(scope.account.as_str()));
    }
    let page: Vec<_> = events.iter().skip(cursor).take(limit).collect();
    let next = if cursor + page.len() < events.len() {
        Some(cursor + page.len())
    } else {
        None
    };
    let value = json!({"events": page, "next_cursor": next, "account": scope.account});
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

fn tool_native_read(root: &Path, args: &Value) -> Result<String, String> {
    let object = args
        .as_object()
        .ok_or("native_read arguments must be an object")?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "id" | "message" | "offset" | "max_bytes" | "from_message" | "limit" | "history"
        )
    }) {
        return Err("unsupported native_read argument".into());
    }
    let id = str_arg(args, "id").ok_or("native_read requires id")?;
    if let Some(history) = args.get("history") {
        if history.as_bool() != Some(true) || object.len() != 2 {
            return Err("history must be true without excerpt/range options".into());
        }
        return serde_json::to_string(&crate::native_memory::history_from_pointer(root, id)?)
            .map_err(|e| e.to_string());
    }
    let mut command = vec![
        if object.contains_key("message") {
            "excerpt"
        } else {
            "sources"
        }
        .to_string(),
        id.to_string(),
    ];
    for (key, option) in [
        ("message", "--message"),
        ("offset", "--offset"),
        ("max_bytes", "--max-bytes"),
        ("from_message", "--from-message"),
        ("limit", "--limit"),
    ] {
        if let Some(value) = args.get(key) {
            let number = value
                .as_u64()
                .ok_or_else(|| format!("{key} must be a nonnegative integer"))?;
            command.extend([option.to_string(), number.to_string()]);
        }
    }
    crate::native_memory::command(root, &command)
}

fn normalized_scope_path(path: &Path) -> Option<String> {
    let path = std::fs::canonicalize(path).ok()?;
    let mut value = path.to_string_lossy().replace('/', "\\");
    while value.len() > 3 && value.ends_with('\\') {
        value.pop();
    }
    #[cfg(windows)]
    {
        if let Some(stripped) = value.strip_prefix(r"\\?\") {
            value = stripped.to_owned();
        }
        value.make_ascii_lowercase();
    }
    Some(value)
}

fn in_workspace(root: &Path, path: &str, scope: &Scope) -> bool {
    let Some(candidate) = normalized_scope_path(Path::new(path)) else {
        return false;
    };
    if normalized_scope_path(&scope.workspace).is_some_and(|workspace| {
        candidate == workspace
            || candidate
                .strip_prefix(&workspace)
                .is_some_and(|rest| rest.starts_with('\\'))
    }) {
        return true;
    }
    let Ok(registry) = crate::managed_workspaces::load(root) else {
        return false;
    };
    registry.records.iter().any(|record| {
        normalized_scope_path(&record.owner)
            .zip(normalized_scope_path(&scope.workspace))
            .is_some_and(|(owner, workspace)| owner == workspace)
            && normalized_scope_path(&record.execution).is_some_and(|worktree| {
                candidate == worktree
                    || candidate
                        .strip_prefix(&worktree)
                        .is_some_and(|rest| rest.starts_with('\\'))
            })
    })
}

fn ensure_thread_scope(root: &Path, args: &Value, scope: &Scope) -> Result<(), String> {
    if scope.unrestricted {
        return Ok(());
    }
    let prefix = str_arg(args, "id").ok_or("missing id")?;
    let id = threads::resolve_prefix(root, prefix).map_err(|e| e.to_string())?;
    let meta = threads::list(root)
        .into_iter()
        .find(|m| m.id == id)
        .ok_or("thread is outside the registered MCP scope")?;
    if meta.account.as_deref() != Some(scope.account.as_str())
        || !in_workspace(root, &meta.cwd, scope)
    {
        return Err("MCP scope rejects this thread".into());
    }
    Ok(())
}

fn ensure_native_scope(root: &Path, args: &Value, scope: &Scope) -> Result<(), String> {
    if scope.unrestricted {
        return Ok(());
    }
    let id = str_arg(args, "id").ok_or("native_read requires id")?;
    let summary = crate::native_memory::verify_source(root, id)?;
    if summary.account != scope.account {
        return Err("MCP scope rejects this native session account".into());
    }
    let record = crate::sessions::find_native_record(
        root,
        &summary.account,
        Some(summary.provider),
        &summary.session_id,
    )?
    .ok_or("native session is outside the registered MCP scope")?;
    if record.0.account != scope.account
        || !record
            .0
            .cwd
            .as_deref()
            .is_some_and(|cwd| in_workspace(root, cwd, scope))
    {
        return Err("MCP scope rejects this native session workspace".into());
    }
    Ok(())
}

fn tool_threads(state_root: &Path, limit: Option<usize>, scope: &Scope) -> String {
    let limit = limit.unwrap_or(10).clamp(1, 30);
    let metas: Vec<_> = threads::list(state_root)
        .into_iter()
        .filter(|m| {
            scope.unrestricted
                || (m.account.as_deref() == Some(scope.account.as_str())
                    && in_workspace(state_root, &m.cwd, scope))
        })
        .collect();
    if metas.is_empty() {
        return "no recorded threads (the user enables recording with [threads] enabled = true)"
            .into();
    }
    metas
        .iter()
        .take(limit)
        .map(|m| {
            let title = if m.title.is_empty() {
                "(untitled)"
            } else {
                &m.title
            };
            format!("{}  {}  {}", m.id, threads::age_label(m.started), title)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tool_thread_map(state_root: &Path, args: &Value) -> Result<String, String> {
    let prefix = str_arg(args, "id").ok_or("missing id")?;
    let id = threads::resolve_prefix(state_root, prefix).map_err(|e| e.to_string())?;
    let cap = crate::config::for_root(state_root).threads.map_chars;
    threads::ThreadLog::open(state_root, &id)
        .read_map(cap)
        .map_err(|error| format!("{error:#}"))
}

fn tool_thread_read(state_root: &Path, args: &Value) -> Result<String, String> {
    let id = str_arg(args, "id").ok_or("missing id")?;
    let mut command = vec!["read".into(), id.into()];
    for key in ["cursor", "grep", "range"] {
        if let Some(value) = str_arg(args, key) {
            command.extend([format!("--{key}"), value.into()]);
        }
    }
    if let Some(tail) = uint_arg(args, "tail") {
        command.extend(["--tail".into(), tail.to_string()]);
    }
    command.extend([
        "--max-chars".into(),
        uint_arg(args, "max_chars")
            .unwrap_or(DEFAULT_READ_CHARS)
            .clamp(200, MAX_RESULT_CHARS)
            .to_string(),
    ]);
    threads::run_cli(state_root, &command).map_err(|(error, _)| error)
}

/// Cached readings only. Never starts a refresh or infers an account from an unbound legacy file.
fn tool_usage(state_root: &Path, scope: &Scope) -> String {
    let rows: Vec<String> = crate::accounts::list_all_accounts(state_root)
        .into_iter()
        .filter(|account| scope.unrestricted || account.account == scope.account)
        .filter_map(|account| {
            let home = account.home_path?;
            let cached = crate::usage::cached_usage_view(
                state_root,
                account.provider,
                &account.account,
                &home,
            )?;
            let freshness = if cached.fresh {
                "recent cache"
            } else {
                "stale cache"
            };
            let age = threads::age_label(cached.observed_at);
            let windows = if cached.windows.is_empty() {
                "no limits reported".into()
            } else {
                cached.windows.join("; ")
            };
            Some(format!(
                "{} account {} [{freshness}, read {age}]: {windows}",
                account.provider.as_str(),
                account.account
            ))
        })
        .collect();
    if rows.is_empty() {
        "no cached usage readings (cache only; Concordia does not refresh from MCP)".into()
    } else {
        rows.join("\n")
    }
}

/// A proposal as written by `delegate` and read by the TUI.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub to: String,
    pub task: String,
    #[serde(default)]
    pub from_thread: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(untagged)]
pub enum PendingProposal {
    Goal(crate::goals::changes::Proposal),
    Delegate(Proposal),
}

fn pending_proposal_files(state_root: &Path) -> Vec<PathBuf> {
    if std::fs::symlink_metadata(proposals_dir(state_root))
        .is_ok_and(|metadata| !metadata.is_dir() || crate::workspaces::linked(&metadata))
    {
        return Vec::new();
    }
    std::fs::read_dir(proposals_dir(state_root))
        .map(|rd| {
            rd.flatten()
                .take(256)
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
                .take(MAX_PENDING_PROPOSALS + 1)
                .collect()
        })
        .unwrap_or_default()
}

fn record_pending(state_root: &Path, request: &str, body: &[u8]) -> Result<(), String> {
    if !uuid::Uuid::parse_str(request).is_ok_and(|uuid| uuid.to_string() == request)
        || body.len() as u64 > MAX_PROPOSAL_FILE
    {
        return Err("invalid proposal request id or payload bound".into());
    }
    let dir = proposals_dir(state_root);
    std::fs::create_dir_all(&dir).map_err(|_| "proposal storage unavailable")?;
    crate::workspaces::directory_id(&dir)?;
    let path = dir.join(format!("{request}.json"));
    let lock = dir.join("proposals.lock");
    for file in [&path, &lock] {
        if std::fs::symlink_metadata(file)
            .is_ok_and(|metadata| !metadata.is_file() || crate::workspaces::linked(&metadata))
        {
            return Err("proposal storage must use regular owned files".into());
        }
    }
    crate::state_io::with_lock(&lock, || {
        if path.exists() {
            let mut existing = Vec::new();
            std::fs::File::open(&path)?
                .take(MAX_PROPOSAL_FILE + 1)
                .read_to_end(&mut existing)?;
            if existing == body {
                return Ok(());
            }
            return Err(std::io::Error::other(
                "request id already names another pending proposal",
            ));
        }
        if pending_proposal_files(state_root).len() >= MAX_PENDING_PROPOSALS
            || std::fs::read_dir(&dir)?.take(257).count() >= 256
        {
            return Err(std::io::Error::other(
                "too many proposals/retained invalid records; no automatic pruning",
            ));
        }
        crate::state_io::write_atomic(&path, body)
    })
    .map_err(|error| error.to_string())
}

fn tool_delegate(state_root: &Path, args: &Value) -> Result<String, String> {
    let to = str_arg(args, "to").unwrap_or("").trim();
    let task = str_arg(args, "task").unwrap_or("").trim();
    if to.is_empty() || task.is_empty() {
        return Err("delegate needs 'to' and 'task'".into());
    }
    if to.chars().count() > 64 {
        return Err("'to' is too long".into());
    }
    if task.chars().count() > crate::delegate::TASK_MAX_CHARS {
        return Err(format!(
            "task is longer than {} characters; make it shorter",
            crate::delegate::TASK_MAX_CHARS
        ));
    }
    let from: String = str_arg(args, "from_thread")
        .unwrap_or("")
        .trim()
        .chars()
        .take(64)
        .collect();
    if pending_proposal_files(state_root).len() >= MAX_PENDING_PROPOSALS {
        return Err("too many proposals are waiting for the user; try again later".into());
    }
    let dir = proposals_dir(state_root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot record proposal: {e}"))?;
    let id = uuid::Uuid::new_v4();
    let body = serde_json::to_string(&Proposal {
        to: to.to_string(),
        task: task.to_string(),
        from_thread: from,
    })
    .map_err(|e| e.to_string())?;
    record_pending(state_root, &id.to_string(), body.as_bytes())?;
    Ok(json!({"agent_id":id.to_string(),"status":"queued"}).to_string())
}

/// Legacy delegate-only access shares the same bounded proposal owner.
pub fn take_proposals(state_root: &Path) -> Vec<Proposal> {
    take_selected(state_root, MAX_PENDING_PROPOSALS, |proposal| {
        matches!(proposal, PendingProposal::Delegate(_))
    })
    .into_iter()
    .filter_map(|proposal| match proposal {
        PendingProposal::Delegate(proposal) => Some(proposal),
        _ => None,
    })
    .collect()
}

pub fn take_pending(state_root: &Path, delegates: bool, limit: usize) -> Vec<PendingProposal> {
    take_selected(state_root, limit.min(MAX_PENDING_PROPOSALS), |proposal| {
        delegates || matches!(proposal, PendingProposal::Goal(_))
    })
}

fn take_selected(
    state_root: &Path,
    limit: usize,
    accept: impl Fn(&PendingProposal) -> bool,
) -> Vec<PendingProposal> {
    let mut out = Vec::new();
    for path in pending_proposal_files(state_root)
        .into_iter()
        .take(MAX_PENDING_PROPOSALS)
    {
        if out.len() >= limit {
            break;
        }
        let small = std::fs::symlink_metadata(&path).is_ok_and(|metadata| {
            metadata.is_file()
                && !crate::workspaces::linked(&metadata)
                && metadata.len() <= MAX_PROPOSAL_FILE
        });
        let proposal = small
            .then(|| {
                let mut options = std::fs::OpenOptions::new();
                options.read(true);
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    options.custom_flags(0x0020_0000);
                }
                let file = options.open(&path).ok()?;
                if !file.metadata().is_ok_and(|metadata| {
                    metadata.is_file() && !crate::workspaces::linked(&metadata)
                }) {
                    return None;
                }
                let mut bytes = Vec::new();
                file.take(MAX_PROPOSAL_FILE + 1)
                    .read_to_end(&mut bytes)
                    .ok()?;
                if bytes.len() as u64 > MAX_PROPOSAL_FILE {
                    return None;
                }
                let proposal: PendingProposal = serde_json::from_slice(&bytes).ok()?;
                if matches!(&proposal, PendingProposal::Goal(goal) if goal.validate().is_err()) {
                    return None;
                }
                Some(proposal)
            })
            .flatten();
        if let Some(proposal) = proposal {
            if !accept(&proposal) {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                out.push(proposal);
            }
        } else {
            // Preserve invalid/future metadata without reading oversized bodies or following links.
            let _ = std::fs::rename(
                &path,
                path.with_extension(format!("invalid-{}", uuid::Uuid::new_v4())),
            );
        }
    }
    out
}

/// True when at least one proposal file is waiting (a cheap `read_dir`).
pub fn has_proposals(state_root: &Path) -> bool {
    !pending_proposal_files(state_root).is_empty()
}

// The former per-tool tests were removed with the legacy MCP surface.
#[cfg(any())]
mod tests {
    use super::*;
    use crate::threads::{ThreadEvent, ThreadLog, ThreadMeta};

    fn call(root: &Path, req: Value) -> Value {
        serde_json::from_str(&handle_line(root, &req.to_string()).unwrap()).unwrap()
    }

    fn rpc(id: u64, method: &str, params: Value) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
    }

    fn tool(root: &Path, name: &str, args: Value) -> (String, bool) {
        let r = call(
            root,
            rpc(1, "tools/call", json!({"name":name,"arguments":args})),
        );
        let res = &r["result"];
        (
            res["content"][0]["text"].as_str().unwrap().to_string(),
            res["isError"].as_bool().unwrap(),
        )
    }

    fn seed(root: &Path) -> String {
        seed_at(root, String::new())
    }

    fn seed_at(root: &Path, cwd: String) -> String {
        let id = "3f9a0000-aaaa-bbbb-cccc-111122223333";
        let log = ThreadLog::open(root, id);
        log.write_meta(&ThreadMeta {
            id: id.into(),
            started: threads::now_secs(),
            title: "fix the login redirect".into(),
            cwd,
            account: Some("claude-2".into()),
        })
        .unwrap();
        let legacy = [
            ThreadEvent::with_text("user", "fix the login redirect"),
            ThreadEvent::with_text(
                "assistant",
                "Done: the redirect now keeps the query string.",
            ),
            ThreadEvent::with_text("user", "add a test for it"),
        ]
        .iter()
        .map(|ev| serde_json::to_string(ev).unwrap() + "\n")
        .collect::<String>();
        // Deliberately exercise legacy body-bearing history, without a production body writer.
        std::fs::write(
            crate::threads::threads_dir(root).join(format!("{id}.jsonl")),
            legacy,
        )
        .unwrap();
        id.into()
    }

    fn managed_fixture(state: &Path, owner: &Path, execution: &Path) {
        let id = "11111111-1111-4111-8111-111111111111";
        let state = state.canonicalize().unwrap();
        let owner = owner.canonicalize().unwrap();
        std::fs::create_dir_all(state.join("managed-worktrees").join(id)).unwrap();
        let execution = execution.canonicalize().unwrap();
        let record = json!({
            "id": id,
            "revision": 1,
            "owner": owner,
            "owner_identity": "1",
            "common_git": owner,
            "common_identity": "1",
            "base": "0123456789012345678901234567890123456789",
            "branch": format!("concordia/work-{id}"),
            "execution": execution,
            "execution_identity": null,
            "phase": "creating",
            "note": "fixture",
            "operation": null
        });
        std::fs::write(
            state.join("managed-workspaces.json"),
            serde_json::to_vec(&json!({"version": 1, "revision": 1, "records": [record]})).unwrap(),
        )
        .unwrap();
    }

    fn scoped(root: &Path, account: &str, workspace: &Path, req: Value) -> Value {
        let scope = Scope {
            account: account.into(),
            workspace: workspace.to_path_buf(),
            unrestricted: false,
        };
        serde_json::from_str(&handle_line_scoped(root, &req.to_string(), Some(&scope)).unwrap())
            .unwrap()
    }

    #[test]
    fn scoped_server_cannot_read_another_workspace() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let id = seed(root.path());
        let req = rpc(
            1,
            "tools/call",
            json!({"name":"thread_read","arguments":{"id":id}}),
        );
        let response = scoped(root.path(), "claude-2", other.path(), req);
        assert!(response["error"]["message"].is_string());
    }

    #[test]
    fn scoped_server_cannot_read_another_account() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let id = seed(root.path());
        let response = scoped(
            root.path(),
            "codex-3",
            workspace.path(),
            rpc(
                1,
                "tools/call",
                json!({"name":"thread_read","arguments":{"id":id}}),
            ),
        );
        assert!(response["error"]["message"].is_string());
    }

    #[test]
    fn scoped_server_reads_its_managed_worktrees() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let worktree = root
            .path()
            .join("managed-worktrees/11111111-1111-4111-8111-111111111111");
        managed_fixture(root.path(), workspace.path(), &worktree);
        let id = seed_at(root.path(), worktree.to_string_lossy().into_owned());
        let scope = Scope {
            account: "claude-2".into(),
            workspace: workspace.path().to_path_buf(),
            unrestricted: false,
        };
        assert!(in_workspace(
            root.path(),
            &worktree.to_string_lossy(),
            &scope
        ));
        let request = rpc(
            1,
            "tools/call",
            json!({"name":"thread_read","arguments":{"id":id}}),
        );
        let response = scoped(root.path(), "claude-2", workspace.path(), request);
        assert!(
            response["result"]["content"][0]["text"].is_string(),
            "{response:?}"
        );

        let other_workspace = tempfile::tempdir().unwrap();
        let other_worktree = root
            .path()
            .join("managed-worktrees/22222222-2222-4222-8222-222222222222");
        let other_id = "22222222-2222-4222-8222-222222222222";
        std::fs::create_dir_all(&other_worktree).unwrap();
        let mut registry: crate::managed_workspaces::Registry =
            crate::managed_workspaces::load(root.path()).unwrap();
        registry.records.push(serde_json::from_value(json!({
            "id": other_id, "revision": 1, "owner": other_workspace.path(), "owner_identity": "1",
            "common_git": other_workspace.path(), "common_identity": "1", "base": "0123456789012345678901234567890123456789",
            "branch": format!("concordia/work-{other_id}"), "execution": other_worktree, "execution_identity": null,
            "phase": "creating", "note": "fixture", "operation": null
        })).unwrap());
        std::fs::write(
            root.path().join("managed-workspaces.json"),
            serde_json::to_vec(&registry).unwrap(),
        )
        .unwrap();
        let other_thread = seed_at(root.path(), other_worktree.to_string_lossy().into_owned());
        let rejected = scoped(
            root.path(),
            "claude-2",
            workspace.path(),
            rpc(
                1,
                "tools/call",
                json!({"name":"thread_read","arguments":{"id":other_thread}}),
            ),
        );
        assert!(rejected["error"]["message"].is_string());
    }

    #[test]
    fn scope_matches_extended_and_case_variant_paths() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let scope = Scope {
            account: "a".into(),
            workspace: workspace.path().to_path_buf(),
            unrestricted: false,
        };
        let plain = workspace.path().to_string_lossy().into_owned();
        let variant = if cfg!(windows) {
            format!(r"\\?\{}", plain.to_uppercase())
        } else {
            format!("{}{}", plain, std::path::MAIN_SEPARATOR)
        };
        assert!(in_workspace(root.path(), &variant, &scope));
    }

    #[test]
    fn unscoped_server_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let request = rpc(1, "tools/call", json!({"name":"threads","arguments":{}}));
        let response = serde_json::from_str::<Value>(
            &handle_line_scoped(root.path(), &request.to_string(), None).unwrap(),
        )
        .unwrap();
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unscoped"));
    }

    #[test]
    fn explicit_cross_scope_argument_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let response = scoped(
            root.path(),
            "claude-2",
            workspace.path(),
            rpc(
                1,
                "tools/call",
                json!({"name":"memory","arguments":{"action":"recall","query":"needle","account":"codex-3"}}),
            ),
        );
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("account"));
    }

    #[test]
    fn initialize_negotiates_the_protocol_version() {
        let d = tempfile::tempdir().unwrap();
        let r = call(
            d.path(),
            rpc(1, "initialize", json!({"protocolVersion":"2024-11-05"})),
        );
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(r["result"]["serverInfo"]["name"], "concordia");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        let r = call(
            d.path(),
            rpc(2, "initialize", json!({"protocolVersion":"2099-01-01"})),
        );
        assert_eq!(r["result"]["protocolVersion"], LATEST_PROTOCOL);
        let r = call(d.path(), rpc(3, "initialize", json!({})));
        assert_eq!(r["result"]["protocolVersion"], LATEST_PROTOCOL);
    }

    #[test]
    fn notifications_get_no_reply_and_ping_works() {
        let d = tempfile::tempdir().unwrap();
        let n = json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string();
        assert!(handle_line(d.path(), &n).is_none());
        let r = call(d.path(), rpc(7, "ping", Value::Null));
        assert_eq!(r["result"], json!({}));
    }

    #[test]
    fn tools_list_has_exactly_the_declared_tools_with_terse_schemas() {
        let d = tempfile::tempdir().unwrap();
        let r = call(d.path(), rpc(1, "tools/list", Value::Null));
        let tools = r["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "threads",
                "thread_map",
                "thread_read",
                "native_read",
                "usage",
                "feed",
                "memory",
                "goals",
                "delegate",
                "integrations"
            ]
        );
        let size = serde_json::to_string(tools).unwrap().len();
        assert!(
            size < 4000,
            "tool list costs tokens every turn: {size} bytes"
        );
        for t in tools {
            assert!(t["description"].as_str().unwrap().len() < 120);
            assert_eq!(t["inputSchema"]["type"], "object");
            if matches!(
                t["name"].as_str(),
                Some(
                    "threads"
                        | "thread_map"
                        | "thread_read"
                        | "native_read"
                        | "usage"
                        | "memory"
                        | "integrations"
                )
            ) {
                assert_eq!(
                    t["annotations"],
                    json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false})
                );
            } else if matches!(t["name"].as_str(), Some("goals" | "delegate")) {
                assert_ne!(
                    t["annotations"]["readOnlyHint"], true,
                    "proposal tools must not claim source-read-only behavior"
                );
            }
        }
        let (catalog, error) = tool(d.path(), "integrations", json!({}));
        assert!(!error, "{catalog}");
        assert!(catalog.contains("native_registration"));
        assert!(tool(d.path(), "integrations", json!({"execute":"anything"})).1);
        assert!(tool(d.path(), "integrations", json!({"offset":-1})).1);
        assert!(tool(d.path(), "memory", json!({"action":"group-link"})).1);
        assert!(
            tool(
                d.path(),
                "memory",
                json!({"action":"recall","query":"needle","offset":1})
            )
            .1
        );
        let (result, error) = tool(
            d.path(),
            "memory",
            json!({"action":"recall","query":"needle"}),
        );
        assert!(!error, "{result}");
        let report: Value = serde_json::from_str(&result).unwrap();
        assert!(report["hits"].is_array());
        assert!(report["incomplete"].is_boolean());
    }

    #[test]
    fn threads_map_and_read_reuse_the_thread_log() {
        let d = tempfile::tempdir().unwrap();
        let (none, err) = tool(d.path(), "threads", json!({}));
        assert!(!err && none.contains("no recorded threads"));
        let id = seed(d.path());
        let (list, _) = tool(d.path(), "threads", json!({}));
        assert!(list.contains(&id) && list.contains("fix the login redirect"));
        let (map, err) = tool(d.path(), "thread_map", json!({"id":"3f9a"}));
        assert!(!err && map.starts_with("[concordia map] thread 3f9a0000"));
        assert!(map.contains(&format!("concordia thread read {id}")));
        let (tail, _) = tool(d.path(), "thread_read", json!({"id":"3f9a","tail":1}));
        assert!(tail.contains("add a test") && !tail.contains("Done:"));
        let (g, _) = tool(
            d.path(),
            "thread_read",
            json!({"id":id,"grep":"query string"}),
        );
        assert!(g.contains("keeps the query string") && !g.contains("add a test"));
        let (rg, _) = tool(d.path(), "thread_read", json!({"id":id,"range":"1-1"}));
        assert!(rg.contains("#1 user") && !rg.contains("#2"));
        let (small, _) = tool(d.path(), "thread_read", json!({"id":id,"max_chars":1}));
        assert!(
            small.chars().count() < 500,
            "max_chars is clamped to a floor, not ignored"
        );
    }

    #[test]
    fn bad_ids_and_arguments_are_tool_errors_not_protocol_errors() {
        let d = tempfile::tempdir().unwrap();
        seed(d.path());
        let (m, err) = tool(d.path(), "thread_map", json!({"id":"zzzz"}));
        assert!(err && m.contains("no thread matches"));
        let (m, err) = tool(d.path(), "thread_read", json!({"id":"3f9a","range":"x"}));
        assert!(err && m.contains("--range"));
        let (_, err) = tool(d.path(), "thread_map", json!({}));
        assert!(err);
        let r = call(d.path(), rpc(1, "tools/call", json!({"name":"nope"})));
        assert_eq!(r["error"]["code"], -32602);
    }

    fn seeded_usage_cache(
        root: &Path,
        provider: crate::store::ProviderKind,
        account: &str,
        fetched_at: u64,
        windows: Value,
    ) -> PathBuf {
        let home = root.join("usage-fixture-homes").join(account);
        std::fs::create_dir_all(&home).unwrap();
        crate::store::upsert_stored_account(
            root,
            crate::store::StoredAccount {
                id: account.into(),
                provider,
                home_path: Some(home.to_string_lossy().into_owned()),
                label: account.into(),
                created_at: "fixture".into(),
            },
        )
        .unwrap();
        let mut fetcher = crate::usage::UsageFetcher::with_runner(
            root.into(),
            std::sync::Arc::new(|_| Ok("Current session: 1% used".into())),
        )
        .with_cursor_runner(std::sync::Arc::new(|_| {
            Ok("Usage ? Fixture\nIncluded 1% used".into())
        }))
        .with_codex_runner(std::sync::Arc::new(|_| {
            Ok(r#"{"rate_limits":{"primary":{"used_percent":1,"window_minutes":300}}}"#.into())
        }));
        fetcher.snapshot(provider, account, &home);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while crate::usage::peek_cached_usage(root, provider, account, &home).is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "synthetic usage fixture did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
            fetcher.poll();
        }
        let path = std::fs::read_dir(root.join("usage-cache"))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                std::fs::read(path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    .is_some_and(|value| value["identity"]["account"] == account)
            })
            .unwrap();
        let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["fetched_at"] = json!(fetched_at);
        value["windows"] = windows;
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        path
    }

    #[test]
    fn usage_reads_only_the_cache() {
        let d = tempfile::tempdir().unwrap();
        let (m, err) = tool(d.path(), "usage", json!({}));
        assert!(!err && m.contains("no cached usage"));
        seeded_usage_cache(
            d.path(),
            crate::store::ProviderKind::Claude,
            "claude-2",
            threads::now_secs(),
            json!([
            {"label":"5h","used_pct":41.0,"resets":"3h"},
            {"label":"7d","used_pct":10.0,"resets":null}]),
        );
        let dir = d.path().join("usage-cache");
        std::fs::write(dir.join("broken.json"), "{").unwrap();
        let before: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|entry| (entry.path(), std::fs::read(entry.path()).unwrap()))
            .collect();
        let (m, _) = tool(d.path(), "usage", json!({}));
        assert!(m.contains("claude account claude-2 [recent cache"), "{m}");
        assert!(
            m.contains("59% left")
                && m.contains("41% used")
                && m.contains("7d")
                && m.contains("90% left"),
            "{m}"
        );
        assert!(!m.contains("broken"));
        for (path, bytes) in before {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn usage_labels_bound_identity_freshness_and_expired_windows_without_refreshing() {
        let d = tempfile::tempdir().unwrap();
        let now = threads::now_secs();
        seeded_usage_cache(
            d.path(),
            crate::store::ProviderKind::Claude,
            "claude-personal",
            now,
            json!([{"label":"5h","used_pct":88.0,"resets":"already reset (chat to refresh)"}]),
        );
        seeded_usage_cache(
            d.path(),
            crate::store::ProviderKind::Cursor,
            "cursor-work",
            now.saturating_sub(crate::usage::USAGE_TTL_SECS + 60),
            json!([{"label":"Included","used_pct":27.0,"resets":null}]),
        );
        seeded_usage_cache(
            d.path(),
            crate::store::ProviderKind::Codex,
            "codex-test",
            now.saturating_add(60),
            json!([{"label":"7d","used_pct":12.0,"resets":null}]),
        );
        let dir = d.path().join("usage-cache");
        std::fs::write(
            dir.join("legacy.json"),
            json!({"windows":[{"label":"7d","used_pct":4.0,"resets":null}]}).to_string(),
        )
        .unwrap();
        let before: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|entry| (entry.path(), std::fs::read(entry.path()).unwrap()))
            .collect();
        let (out, err) = tool(d.path(), "usage", json!({}));
        assert!(!err);
        assert!(
            out.contains("claude account claude-personal [recent cache, read now]"),
            "{out}"
        );
        assert!(
            out.contains("current usage unavailable") && out.contains("last reading 88% used"),
            "{out}"
        );
        assert!(!out.contains("5h 12% left"), "{out}");
        assert!(
            out.contains("cursor account cursor-work [stale cache"),
            "{out}"
        );
        assert!(
            !out.contains("legacy")
                && !out.contains("codex-test")
                && !out.contains("7d 96% left")
                && !out.contains("7d 88% left"),
            "{out}"
        );
        assert!(out.contains("73% left"), "{out}");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            4,
            "MCP usage must not refresh or write cache data"
        );
        for (path, bytes) in before {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn delegate_and_goal_tools_only_write_reviewable_proposals() {
        let d = tempfile::tempdir().unwrap();
        let (m, err) = tool(d.path(), "delegate", json!({"to":"codex-2","task":"t"}));
        assert!(err && m.contains("delegation is off"));
        assert!(
            !proposals_dir(d.path()).exists(),
            "nothing is written while delegation is off"
        );
        crate::store::set_delegation_protocol(d.path(), true).unwrap();
        let (m, err) = tool(
            d.path(),
            "delegate",
            json!({"to":"codex-2","task":"run the tests","from_thread":"3f9a"}),
        );
        assert!(!err && m.contains("the user must approve"));
        let files: Vec<_> = std::fs::read_dir(proposals_dir(d.path()))
            .unwrap()
            .flatten()
            .filter(|file| {
                file.path()
                    .extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        assert_eq!(files.len(), 1);
        let p: Proposal =
            serde_json::from_str(&std::fs::read_to_string(files[0].path()).unwrap()).unwrap();
        assert_eq!(
            p,
            Proposal {
                to: "codex-2".into(),
                task: "run the tests".into(),
                from_thread: "3f9a".into()
            }
        );
        assert!(has_proposals(d.path()));
        assert_eq!(take_proposals(d.path()), vec![p]);
        assert!(!has_proposals(d.path()), "taken proposals are consumed");
        let (_, err) = tool(d.path(), "delegate", json!({"to":"x"}));
        assert!(err);
        let (m, err) = tool(
            d.path(),
            "delegate",
            json!({"to":"x","task":"y".repeat(5000)}),
        );
        assert!(err && m.contains("longer"));
        crate::store::set_delegation_protocol(d.path(), false).unwrap();
        let request = uuid::Uuid::new_v4().to_string();
        let revision = crate::goals::revision(&[]).unwrap();
        let arguments = json!({"action":"propose_add","request_id":request,"revision":revision,"text":"Goal proposed through MCP"});
        let (proposed, error) = tool(d.path(), "goals", arguments.clone());
        assert!(!error, "{proposed}");
        assert!(proposed.contains("proposal_recorded"));
        assert!(
            !d.path().join("goals.json").exists(),
            "MCP cannot mutate goals directly"
        );
        assert_eq!(pending_proposal_files(d.path()).len(), 1);
        assert!(!tool(d.path(), "goals", arguments.clone()).1);
        assert_eq!(
            pending_proposal_files(d.path()).len(),
            1,
            "same request does not duplicate pending work"
        );
        assert!(
            take_proposals(d.path()).is_empty(),
            "legacy delegate-only consumers leave goal proposals untouched"
        );
        let pending = take_pending(d.path(), false, 1);
        let [PendingProposal::Goal(goal)] = pending.as_slice() else {
            panic!("typed goal proposal expected");
        };
        assert!(crate::goals::read_goals(d.path()).unwrap().is_empty());
        let mut forged = arguments.clone();
        forged["actor"] = json!("verified native agent");
        assert!(tool(d.path(), "goals", forged).1);
        assert!(
            tool(
                d.path(),
                "goals",
                json!({"action":"apply","request_id":request,"revision":revision})
            )
            .1
        );
        crate::goals::changes::apply(d.path(), goal).unwrap();
        assert_eq!(crate::goals::read_goals(d.path()).unwrap().len(), 1);
        let (repeated, error) = tool(d.path(), "goals", arguments);
        assert!(!error && repeated.contains("applied"));
        assert!(
            pending_proposal_files(d.path()).is_empty(),
            "resolved request never requeues itself"
        );
        let (status, error) = tool(
            d.path(),
            "goals",
            json!({"action":"status","request_id":request}),
        );
        assert!(!error && status.contains("unverified MCP client"));
        let current = crate::goals::revision(&crate::goals::read_goals(d.path()).unwrap()).unwrap();
        assert!(tool(d.path(), "goals", json!({"action":"propose_add","request_id":uuid::Uuid::new_v4().to_string(),"revision":current,"text":"x".repeat(513)})).1);
    }

    #[test]
    fn too_many_pending_proposals_are_refused() {
        let d = tempfile::tempdir().unwrap();
        crate::store::set_delegation_protocol(d.path(), true).unwrap();
        for _ in 0..MAX_PENDING_PROPOSALS {
            assert!(!tool(d.path(), "delegate", json!({"to":"a","task":"b"})).1);
        }
        let (m, err) = tool(d.path(), "delegate", json!({"to":"a","task":"b"}));
        assert!(err && m.contains("too many"));
    }

    #[test]
    fn take_proposals_discards_junk_and_oversized_files() {
        let d = tempfile::tempdir().unwrap();
        let dir = proposals_dir(d.path());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.json"), "not json").unwrap();
        std::fs::write(dir.join("b.json"), "x".repeat(20_000)).unwrap();
        std::fs::write(dir.join("c.txt"), "ignored").unwrap();
        assert!(take_proposals(d.path()).is_empty());
        assert!(!dir.join("a.json").exists() && !dir.join("b.json").exists());
        assert!(dir.join("c.txt").exists());
    }

    #[test]
    fn protocol_errors_use_the_standard_codes() {
        let d = tempfile::tempdir().unwrap();
        let r = call(d.path(), rpc(5, "resources/list", Value::Null));
        assert_eq!(r["error"]["code"], -32601);
        assert_eq!(r["id"], 5);
        let r: Value = serde_json::from_str(&handle_line(d.path(), "{not json").unwrap()).unwrap();
        assert_eq!(r["error"]["code"], -32700);
        assert_eq!(r["id"], Value::Null);
        let r: Value = serde_json::from_str(&handle_line(d.path(), "[1,2]").unwrap()).unwrap();
        assert_eq!(r["error"]["code"], -32600);
        let big = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"ping","pad":"{}"}}"#,
            "x".repeat(MAX_REQUEST_BYTES)
        );
        let r: Value = serde_json::from_str(&handle_line(d.path(), &big).unwrap()).unwrap();
        assert_eq!(r["error"]["code"], -32600);
    }

    #[test]
    fn serve_answers_line_by_line_and_survives_an_oversized_line() {
        let d = tempfile::tempdir().unwrap();
        let mut input = String::new();
        input.push_str(&rpc(1, "ping", Value::Null).to_string());
        input.push('\n');
        input.push_str(&"y".repeat(MAX_REQUEST_BYTES * 3));
        input.push('\n');
        input.push_str(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string());
        input.push('\n');
        input.push_str(&rpc(2, "ping", Value::Null).to_string());
        input.push('\n');
        let mut out = Vec::new();
        serve(std::io::Cursor::new(input.into_bytes()), &mut out, d.path()).unwrap();
        let lines: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["id"], 1);
        assert_eq!(lines[1]["error"]["code"], -32600);
        assert_eq!(lines[2]["id"], 2);
    }

    #[test]
    fn results_are_capped() {
        let d = tempfile::tempdir().unwrap();
        let id = "aaaa1111-0000-0000-0000-000000000000";
        std::fs::create_dir_all(crate::threads::threads_dir(d.path())).unwrap();
        let legacy =
            (serde_json::to_string(&ThreadEvent::with_text("assistant", &"z".repeat(5000)))
                .unwrap()
                + "\n")
                .repeat(30);
        std::fs::write(
            crate::threads::threads_dir(d.path()).join(format!("{id}.jsonl")),
            legacy,
        )
        .unwrap();
        let (t, _) = tool(
            d.path(),
            "thread_read",
            json!({"id":"aaaa","max_chars":999_999}),
        );
        assert!(t.chars().count() <= MAX_RESULT_CHARS + 40);
    }

    #[test]
    fn mcp_results_are_flagged_when_redacted_on_the_fly() {
        let result = text_result("token=ghp_abcdefghijklmnop".into(), false);
        assert_eq!(result["redacted"], true);
        assert!(result["content"].as_array().unwrap().iter().any(|part| {
            part["text"]
                .as_str()
                .is_some_and(|text| text.contains("Some stored text was redacted"))
        }));
    }
}
