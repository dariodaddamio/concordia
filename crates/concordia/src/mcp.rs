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
                Ok(line) => handle_line(state_root, line),
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
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}).to_string()
}

fn result_reply(id: Value, result: Value) -> String {
    json!({"jsonrpc":"2.0","id":id,"result":result}).to_string()
}

/// One request line in, the reply line out (None for notifications).
pub fn handle_line(state_root: &Path, line: &str) -> Option<String> {
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
        "tools/call" => match call_tool(state_root, &params) {
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
    let id = json!({"type":"string","description":"thread id or unique prefix"});
    json!([
        {
            "name": "threads",
            "description": "List recent Concordia chat threads (id, age, title).",
            "inputSchema": {"type":"object","properties":{"limit":{"type":"integer"}}}
        },
        {
            "name": "thread_map",
            "description": "Short map of one thread: ask, files, commands, errors, last reply.",
            "inputSchema": {"type":"object","properties":{"id":id},"required":["id"]}
        },
        {
            "name": "thread_read",
            "description": "Read parts of a thread. Prefer tail or grep over the whole text.",
            "inputSchema": {"type":"object","properties":{
                "id":id,
                "tail":{"type":"integer"},
                "grep":{"type":"string"},
                "range":{"type":"string","description":"A-B"},
                "max_chars":{"type":"integer"}
            },"required":["id"]}
        },
        {
            "name": "native_read",
            "description": "Read exact chunks/ranges. history scans the next bounded whole-history page.",
            "inputSchema": {"type":"object","properties":{"id":{"type":"string"},"message":{"type":"integer"},"offset":{"type":"integer"},"max_bytes":{"type":"integer"},"from_message":{"type":"integer"},"limit":{"type":"integer"},"history":{"type":"boolean"}},"required":["id"]}
        },
        {
            "name": "usage",
            "description": "Last cached usage limits per account (no live check).",
            "inputSchema": {"type":"object","properties":{}}
        },
        {
            "name":"memory",
            "description":"Search original chats, file facts or confirmed groups. Read-only; use native_read for exact excerpts.",
            "inputSchema":{"type":"object","additionalProperties":false,"properties":{
                "action":{"enum":["recall","ranked","files","groups","members"]},
                "query":{"type":"string"},"workspace":{"type":"string"},"source":{"type":"string"},
                "account":{"type":"string"},"goal":{"type":"string"},"file":{"type":"string"},
                "group":{"type":"string"},"offset":{"type":"integer","minimum":0},"revision":{"type":"string"}
            },"required":["action"]}
        },
        {
            "name": "goals",
            "description": "Read goals/source links or propose add/close for local review. Proposals need request_id UUID and current revision.",
            "inputSchema": {"type":"object","additionalProperties":false,"properties":{
                "action":{"enum":["read","propose_add","propose_close","status"]},
                "goal":{"type":"string"},"workspace":{"type":"string"},
                "text":{"type":"string","maxLength":512},"request_id":{"type":"string"},
                "offset":{"type":"integer","minimum":0},"revision":{"type":"string"}
            }}
        },
        {
            "name": "delegate",
            "description": "Propose a task for another account. Only records a proposal; the user must approve it.",
            "inputSchema": {"type":"object","properties":{
                "to":{"type":"string","description":"worker account id"},
                "task":{"type":"string","description":"complete, self-contained task"},
                "from_thread":{"type":"string"}
            },"required":["to","task"]}
        },
        {
            "name":"integrations",
            "description":"Discover connected integration metadata and namespaced tools. Read-only; never executes tools or adapter commands.",
            "inputSchema":{"type":"object","additionalProperties":false,"properties":{
                "id":{"type":"string"},"offset":{"type":"integer","minimum":0},"revision":{"type":"string"}
            }}
        }
    ])
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({"content":[{"type":"text","text":cap(&text, MAX_RESULT_CHARS)}],"isError":is_error})
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

fn tool_memory(root: &Path, args: &Value) -> Result<String, String> {
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
    let value = match action {
        "recall" => serde_json::to_value(crate::native_memory::retrieval::recall(
            root,
            required("query")?,
            str_arg(args, "source"),
            str_arg(args, "account"),
            str_arg(args, "workspace").map(Path::new),
        )?)
        .map_err(|error| error.to_string())?,
        "ranked" => serde_json::to_value(crate::native_memory::rank::recall(
            root,
            crate::native_memory::rank::Request {
                text: required("query")?,
                source: str_arg(args, "source"),
                account: str_arg(args, "account"),
                workspace: str_arg(args, "workspace").map(Path::new),
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
fn call_tool(state_root: &Path, params: &Value) -> Result<Value, String> {
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
        "threads" => Ok(tool_threads(state_root, uint_arg(&args, "limit"))),
        "thread_map" => tool_thread_map(state_root, &args),
        "thread_read" => tool_thread_read(state_root, &args),
        "native_read" => tool_native_read(state_root, &args),
        "usage" => Ok(tool_usage(state_root)),
        "memory" => tool_memory(state_root, &args),
        "goals" => tool_goals(state_root, &args),
        "delegate" => tool_delegate(state_root, &args),
        "integrations" => {
            if args.as_object().is_some_and(|args| {
                args.keys()
                    .any(|key| !matches!(key.as_str(), "id" | "offset" | "revision"))
            }) || args.get("offset").is_some_and(|value| !value.is_u64())
                || args.get("id").is_some_and(|value| !value.is_string())
                || args.get("revision").is_some_and(|value| !value.is_string())
            {
                Err("invalid integration discovery arguments".into())
            } else {
                crate::harness::catalog::tool(
                    state_root,
                    str_arg(&args, "id"),
                    uint_arg(&args, "offset").unwrap_or(0),
                    str_arg(&args, "revision"),
                )
            }
        }
        other => return Err(format!("unknown tool: {other}")),
    };
    Ok(match out {
        Ok(t) => text_result(t, false),
        Err(e) => text_result(e, true),
    })
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

fn tool_threads(state_root: &Path, limit: Option<usize>) -> String {
    let limit = limit.unwrap_or(10).clamp(1, 30);
    let metas = threads::list(state_root);
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

fn resolve(state_root: &Path, args: &Value) -> Result<(String, Vec<threads::ThreadEvent>), String> {
    let prefix = str_arg(args, "id").ok_or("missing id")?;
    let id = threads::resolve_prefix(state_root, prefix).map_err(|e| e.to_string())?;
    let events = threads::ThreadLog::open(state_root, &id)
        .read_all()
        .map_err(|e| format!("{e:#}"))?;
    Ok((id, events))
}

fn tool_thread_map(state_root: &Path, args: &Value) -> Result<String, String> {
    let (id, events) = resolve(state_root, args)?;
    let cap = crate::config::for_root(state_root).threads.map_chars;
    Ok(threads::build_map(&events, &id, cap, true))
}

fn tool_thread_read(state_root: &Path, args: &Value) -> Result<String, String> {
    let (_, events) = resolve(state_root, args)?;
    let range = match str_arg(args, "range") {
        Some(r) => Some(threads::parse_range(r).map_err(|e| e.to_string())?),
        None => None,
    };
    let opts = threads::ReadOpts {
        tail: uint_arg(args, "tail"),
        grep: str_arg(args, "grep")
            .filter(|g| !g.is_empty())
            .map(str::to_string),
        range,
        max_chars: uint_arg(args, "max_chars")
            .unwrap_or(DEFAULT_READ_CHARS)
            .clamp(200, MAX_RESULT_CHARS),
    };
    Ok(threads::read_events_text(&events, &opts))
}

/// Cached readings only. Never starts a refresh or infers an account from an unbound legacy file.
fn tool_usage(state_root: &Path) -> String {
    let dir = state_root.join("usage-cache");
    let mut rows: Vec<String> = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    files.sort();
    for path in files {
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(account) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let now = threads::now_secs();
        let fetched_at = v.get("fetched_at").and_then(Value::as_u64);
        let (freshness, age) = match fetched_at.filter(|at| *at <= now) {
            Some(at) => {
                let seconds = now.saturating_sub(at);
                let freshness = if seconds < crate::usage::USAGE_TTL_SECS {
                    "recent cache"
                } else {
                    "stale cache"
                };
                (freshness, threads::age_label(at))
            }
            None => ("cache age unknown", "age unknown".to_string()),
        };
        let identity = v.get("identity");
        let bound = identity.and_then(|i| {
            if i.get("home")?.as_str()?.is_empty() {
                return None;
            }
            Some(format!(
                "{} account {}",
                i.get("provider")?.as_str()?,
                i.get("account")?.as_str()?
            ))
        });
        let who = bound.unwrap_or_else(|| format!("identity unknown (legacy cache {account})"));
        let windows: Vec<String> = v
            .get("windows")
            .and_then(Value::as_array)
            .map(|ws| {
                ws.iter()
                    .filter_map(|w| {
                        let label = w.get("label")?.as_str()?;
                        let used = w.get("used_pct")?.as_f64()?.clamp(0.0, 100.0);
                        let reset = w.get("resets").and_then(Value::as_str);
                        if reset.is_some_and(|r| r.starts_with("already reset")) {
                            return Some(format!(
                                "{label} reset since report (current usage unknown)"
                            ));
                        }
                        let resets = reset.map(|r| format!(", resets {r}")).unwrap_or_default();
                        Some(format!(
                            "{label} {:.0}% left ({:.0}% used{resets})",
                            100.0 - used,
                            used
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if windows.is_empty() {
            rows.push(format!(
                "{who} [{freshness}, read {age}]: no limits reported"
            ));
        } else {
            rows.push(format!(
                "{who} [{freshness}, read {age}]: {}",
                windows.join("; ")
            ));
        }
    }
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
    if !crate::store::delegation_protocol_on(state_root) {
        return Err("delegation is off; the user can turn it on with /delegate protocol on".into());
    }
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
    Ok("proposal recorded; the user must approve it in Concordia".into())
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

#[cfg(test)]
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
        let id = "3f9a0000-aaaa-bbbb-cccc-111122223333";
        let log = ThreadLog::open(root, id);
        log.write_meta(&ThreadMeta {
            id: id.into(),
            started: threads::now_secs(),
            title: "fix the login redirect".into(),
            cwd: String::new(),
            account: Some("claude-2".into()),
        })
        .unwrap();
        for ev in [
            ThreadEvent::with_text("user", "fix the login redirect"),
            ThreadEvent::with_text(
                "assistant",
                "Done: the redirect now keeps the query string.",
            ),
            ThreadEvent::with_text("user", "add a test for it"),
        ] {
            log.append(&ev).unwrap();
        }
        id.into()
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

    #[test]
    fn usage_reads_only_the_cache() {
        let d = tempfile::tempdir().unwrap();
        let (m, err) = tool(d.path(), "usage", json!({}));
        assert!(!err && m.contains("no cached usage"));
        let dir = d.path().join("usage-cache");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("claude-2.json"),
            json!({"fetched_at": threads::now_secs(), "windows":[
                {"label":"5h","used_pct":41.0,"resets":"3h"},
                {"label":"7d","used_pct":10.0,"resets":null}]})
            .to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("broken.json"), "{").unwrap();
        let (m, _) = tool(d.path(), "usage", json!({}));
        assert!(
            m.contains("identity unknown (legacy cache claude-2) [recent cache"),
            "{m}"
        );
        assert!(m.contains("5h 59% left (41% used, resets 3h)") && m.contains("7d 90% left"));
        assert!(!m.contains("broken"));
    }

    #[test]
    fn usage_labels_bound_identity_freshness_and_expired_windows_without_refreshing() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("usage-cache");
        std::fs::create_dir_all(&dir).unwrap();
        let now = threads::now_secs();
        std::fs::write(
            dir.join("recent.json"),
            json!({
                "fetched_at": now,
                "identity":{"account":"claude-personal","provider":"claude","home":"/synthetic/claude-personal"},
                "windows":[{"label":"5h","used_pct":88.0,"resets":"already reset (chat to refresh)"}]
            }).to_string(),
        ).unwrap();
        std::fs::write(
            dir.join("old.json"),
            json!({
                "fetched_at": now.saturating_sub(crate::usage::USAGE_TTL_SECS + 60),
                "identity":{"account":"cursor-work","provider":"cursor","home":"/synthetic/cursor-work"},
                "windows":[{"label":"Included","used_pct":27.0,"resets":null}]
            }).to_string(),
        ).unwrap();
        std::fs::write(
            dir.join("legacy.json"),
            json!({"windows":[{"label":"7d","used_pct":4.0,"resets":null}]}).to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.join("future.json"),
            json!({
                "fetched_at": now.saturating_add(60),
                "identity":{"account":"codex-test","provider":"codex","home":"/synthetic/codex-test"},
                "windows":[{"label":"7d","used_pct":12.0,"resets":null}]
            }).to_string(),
        ).unwrap();

        let (out, err) = tool(d.path(), "usage", json!({}));
        assert!(!err);
        assert!(
            out.contains("claude account claude-personal [recent cache, read now]"),
            "{out}"
        );
        assert!(
            out.contains("5h reset since report (current usage unknown)"),
            "{out}"
        );
        assert!(!out.contains("5h 12% left"), "{out}");
        assert!(
            out.contains("cursor account cursor-work [stale cache"),
            "{out}"
        );
        assert!(
            out.contains("identity unknown (legacy cache legacy) [cache age unknown"),
            "{out}"
        );
        assert!(
            out.contains("codex account codex-test [cache age unknown"),
            "{out}"
        );
        assert!(out.contains("7d 96% left"), "{out}");
        assert!(out.contains("7d 88% left"), "{out}");
        assert!(
            std::fs::read_dir(dir).unwrap().count() == 4,
            "MCP usage must not refresh or write cache data"
        );
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
        let log = ThreadLog::open(d.path(), id);
        for _ in 0..30 {
            log.append(&ThreadEvent::with_text("assistant", &"z".repeat(5000)))
                .unwrap();
        }
        let (t, _) = tool(
            d.path(),
            "thread_read",
            json!({"id":"aaaa","max_chars":999_999}),
        );
        assert!(t.chars().count() <= MAX_RESULT_CHARS + 40);
    }
}
