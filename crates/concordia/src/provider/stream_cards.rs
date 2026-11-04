//! Clean-room Codex JSONL -> text / tool cards.
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardStatus {
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamCard {
    pub id: String,
    pub title: String,
    pub detail: Option<String>,
    pub status: CardStatus,
}

/// Token counts and cost a provider reported for one turn. Every field is optional: a provider
/// reports what it reports (Cursor reports none) and nothing here is ever guessed.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TurnUsage {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cached: Option<u64>,
    pub cost_usd: Option<f64>,
}

impl TurnUsage {
    /// Adapters emit terminal turn/result snapshots. Repeated notifications are not deltas.
    pub fn update_snapshot(&mut self, update: &Self) {
        if update.input.is_some() {
            self.input = update.input;
        }
        if update.output.is_some() {
            self.output = update.output;
        }
        if update.cached.is_some() {
            self.cached = update.cached;
        }
        if let Some(cost) = update
            .cost_usd
            .filter(|cost| cost.is_finite() && *cost >= 0.0)
        {
            self.cost_usd = Some(cost);
        }
    }
    pub fn is_empty(&self) -> bool {
        self.input.is_none()
            && self.output.is_none()
            && self.cached.is_none()
            && self.cost_usd.is_none()
    }

    /// Read a usage object tolerantly: missing or mistyped fields are simply absent.
    /// `cached_key` is the provider's name for cache-read tokens.
    pub fn from_json(usage: Option<&serde_json::Value>, cached_key: &str) -> TurnUsage {
        let num = |k: &str| usage.and_then(|u| u.get(k)).and_then(|v| v.as_u64());
        TurnUsage {
            input: num("input_tokens"),
            output: num("output_tokens"),
            cached: num(cached_key),
            cost_usd: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamPart {
    Text(String),
    /// A fragment of reply text that continues the previous one, with no break in between.
    TextDelta(String),
    Card(StreamCard),
    Error(String),
    Session(String),
    /// The model's reasoning, shown dimmed and never added to the reply or the context pack.
    Thinking(String),
    /// A fragment of reasoning that continues the previous one; a lone newline ends the block.
    ThinkingDelta(String),
    /// Token counts for the turn; shown on the token line, never reply text.
    Usage(TurnUsage),
}

pub fn parse_codex_stream_chunk(chunk: &str) -> Vec<StreamPart> {
    let mut out = Vec::new();
    for line in chunk.split(['\n', '\r']) {
        let t = line.trim();
        if !t.starts_with('{') {
            continue;
        }
        let Ok(ev) = serde_json::from_str::<serde_json::Value>(t) else {
            continue;
        };
        out.extend(classify_codex_event(&ev));
    }
    out
}

pub fn usage_exhausted(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "usage limit",
        "usage_limit",
        "usage has run out",
        "you are out of usage",
        "you're out of usage",
        "quota exceeded",
        "quota_exceeded",
        "insufficient_quota",
        "you've hit your limit",
        "you have hit your limit",
        "rate_limit_exceeded",
        "rate limit reached",
        "usage has been exhausted",
        "out of usage",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase))
        || [
            "usagelimitexceeded",
            "usagelimitreached",
            "quotaexceeded",
            "insufficientquota",
            "credit balance is too low",
        ]
        .iter()
        .any(|code| lower.replace(['_', '-'], "").contains(code))
}

/// The provider rejected the selected model because its servers are busy (Codex
/// `server_overloaded` / "Selected model is at capacity", Claude `overloaded_error`). Quota is
/// not the cause, so another model or account can take the request.
pub fn model_overloaded(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("model is at capacity")
        || lower.contains("overloaded_error")
        || lower.replace(['_', '-'], "").contains("serveroverloaded")
}

pub(super) fn capacity_error_text(message: &str, code: Option<&str>) -> String {
    let code = code.map(|code| {
        if code.eq_ignore_ascii_case("ContextWindowExceeded") {
            "context_window_exceeded"
        } else {
            code
        }
    });
    if let Some(code) = code
        .filter(|code| (usage_exhausted(code) || model_overloaded(code)) && !message.contains(code))
    {
        return format!("{code}: {message}");
    }
    match code {
        Some(
            code @ ("context_length_exceeded"
            | "context_window_exceeded"
            | "usage_limit_reached"
            | "usage_limit_exceeded"
            | "rate_limit_exceeded"
            | "quota_exceeded"
            | "insufficient_quota"),
        ) if !message.contains(code) => {
            format!("{code}: {message}")
        }
        _ => message.to_string(),
    }
}

fn native_error_code(ev: &serde_json::Value) -> Option<&str> {
    let codes: Vec<_> = [
        ev.get("code"),
        ev.pointer("/error/code"),
        ev.get("codexErrorInfo"),
        ev.pointer("/error/codexErrorInfo"),
    ]
    .into_iter()
    .flatten()
    .filter_map(|v| {
        v.as_str().or_else(|| {
            v.as_object()
                .and_then(|o| o.keys().next())
                .map(String::as_str)
        })
    })
    .collect();
    codes
        .iter()
        .copied()
        .find(|code| {
            usage_exhausted(code)
                || model_overloaded(code)
                || code.eq_ignore_ascii_case("ContextWindowExceeded")
        })
        .or_else(|| codes.first().copied())
}

pub fn classify_codex_event(ev: &serde_json::Value) -> Vec<StreamPart> {
    let typ = ev.get("type").and_then(|x| x.as_str()).unwrap_or("");
    if matches!(
        typ,
        "response.output_item.added" | "response.output_item.done"
    ) {
        if let Some(item) = ev.get("item") {
            if let Some(card) = web_search_card(typ, item, ev) {
                return vec![StreamPart::Card(card)];
            }
        }
    }
    if typ.starts_with("response.web_search_call.") {
        if let Some(card) = web_search_card(typ, ev, ev) {
            return vec![StreamPart::Card(card)];
        }
    }
    if typ == "thread.started" {
        if let Some(id) = ev.get("thread_id").and_then(|x| x.as_str()) {
            return vec![StreamPart::Session(id.to_string())];
        }
    }
    if typ == "error" {
        let code = native_error_code(ev);
        if let Some(msg) = ev
            .get("message")
            .or_else(|| ev.pointer("/error/message"))
            .or_else(|| ev.get("error").filter(|v| v.is_string()))
            .and_then(|x| x.as_str())
            .or(code)
        {
            return vec![StreamPart::Error(capacity_error_text(msg, code))];
        }
    }
    if typ == "turn.completed" {
        let usage = TurnUsage::from_json(ev.get("usage"), "cached_input_tokens");
        return if usage.is_empty() {
            Vec::new()
        } else {
            vec![StreamPart::Usage(usage)]
        };
    }
    if typ == "turn.failed" {
        let code = native_error_code(ev);
        if let Some(msg) = ev
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|x| x.as_str())
            .or(code)
        {
            return vec![StreamPart::Error(capacity_error_text(msg, code))];
        }
    }
    if typ.starts_with("item.") {
        return classify_item_event(typ, ev);
    }
    if typ == "response_item" {
        if let Some(payload) = ev.get("payload").filter(|p| p.is_object()) {
            return classify_payload(payload);
        }
    }
    if let Some(text) = pick_plain_text(ev) {
        return vec![StreamPart::Text(text)];
    }
    Vec::new()
}

fn classify_item_event(typ: &str, ev: &serde_json::Value) -> Vec<StreamPart> {
    let item = ev.get("item").filter(|i| i.is_object()).unwrap_or(ev);
    let item_type = item.get("type").and_then(|x| x.as_str()).unwrap_or("");
    if let Some(card) = web_search_card(typ, item, ev) {
        return vec![StreamPart::Card(card)];
    }
    if item_type == "collab_tool_call" {
        return classify_collab_tool_call(item);
    }
    let id = item
        .get("id")
        .or_else(|| item.get("call_id"))
        .or_else(|| ev.get("id"))
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{item_type}-{typ}"));

    if item_type == "reasoning" {
        let text = pick_plain_text(item)
            .or_else(|| content_to_text(item.get("summary")))
            .unwrap_or_default();
        return if text.trim().is_empty() {
            Vec::new()
        } else {
            vec![StreamPart::Thinking(text)]
        };
    }

    if item_type == "agent_message" || item_type == "message" {
        if let Some(text) = pick_plain_text(item).or_else(|| pick_plain_text(ev)) {
            return vec![StreamPart::Text(text)];
        }
    }

    let looks_tool = item_type_is_tool(item_type)
        || item.get("command").and_then(|x| x.as_str()).is_some()
        || item.get("name").and_then(|x| x.as_str()).is_some();
    if looks_tool {
        let title = item
            .get("name")
            .and_then(|x| x.as_str())
            .map(str::to_string)
            .or_else(|| {
                item.get("command")
                    .and_then(|x| x.as_str())
                    .map(|_| "shell".to_string())
            })
            .unwrap_or_else(|| item_type.replace('_', " "));
        let mut detail = [
            "command",
            "path",
            "input",
            "arguments",
            "content",
            "diff",
            "changes",
            "result",
        ]
        .into_iter()
        .filter_map(|key| {
            item.get(key)
                .filter(|value| !value.is_null())
                .map(|value| format!("{key}: {}", format_provider_value(value)))
        })
        .collect::<Vec<_>>()
        .join("\n");
        let title = if item_type == "file_change" {
            if let Some(rendered) = item.get("changes").and_then(super::edit_display::changes) {
                detail = rendered;
            }
            "Edit".to_string()
        } else {
            title
        };
        if let Some(output) = item.get("aggregated_output").or_else(|| item.get("output")) {
            let output = format_provider_value(output);
            if !output.is_empty() {
                detail.push_str("\nOutput: ");
                detail.push_str(&output);
            }
        }
        if let Some(server) = item.get("server").and_then(|v| v.as_str()) {
            let tool = item.get("tool").and_then(|v| v.as_str());
            detail = format!(
                "MCP server: {server}{}\n{detail}",
                tool.map(|t| format!("\nMCP tool: {t}")).unwrap_or_default()
            );
        }
        if let Some(error) = item.get("error") {
            let error = format_provider_value(error);
            if !error.is_empty() {
                detail.push_str("\nError: ");
                detail.push_str(&error);
            }
        }
        let status = if item
            .get("exit_code")
            .and_then(|v| v.as_i64())
            .is_some_and(|code| code != 0)
            || item.get("error").is_some_and(|error| !error.is_null())
            || item
                .get("status")
                .and_then(|v| v.as_str())
                .is_some_and(|s| matches!(s, "failed" | "error" | "cancelled"))
        {
            CardStatus::Error
        } else {
            card_status_from_type(typ)
        };
        let title = if let (Some(server), Some(tool)) = (
            item.get("server").and_then(|v| v.as_str()),
            item.get("tool").and_then(|v| v.as_str()),
        ) {
            format!("{server}/{tool}")
        } else {
            title
        };
        let card = StreamCard {
            id,
            title: if title.is_empty() {
                "tool".into()
            } else {
                title
            },
            detail: if detail.is_empty() {
                None
            } else {
                Some(detail)
            },
            status,
        };
        return vec![StreamPart::Card(card)];
    }

    if let Some(text) = pick_plain_text(item).or_else(|| pick_plain_text(ev)) {
        if !typ_is_errorish(typ) {
            return vec![StreamPart::Text(text)];
        }
    }
    if typ_is_errorish(typ) {
        let msg = pick_plain_text(ev).unwrap_or_else(|| typ.to_string());
        return vec![StreamPart::Error(msg)];
    }
    Vec::new()
}

/// Codex exec's documented CollabToolCall reports tool and child lifetimes separately.
/// Completing spawn_agent/wait does not by itself mean that a child has completed.
pub(super) fn classify_collab_tool_call(item: &serde_json::Value) -> Vec<StreamPart> {
    let states = item.get("agents_states").and_then(|v| v.as_object());
    let mut ids: Vec<&str> = item
        .get("receiver_thread_ids")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .filter(|id| !id.is_empty())
        .collect();
    if let Some(states) = states {
        ids.extend(
            states
                .keys()
                .map(String::as_str)
                .filter(|id| !id.is_empty()),
        );
    }
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .map(|id| {
            let state = states.and_then(|s| s.get(id));
            let status = state
                .and_then(|s| s.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let mut detail = Vec::new();
            if let Some(prompt) = item.get("prompt").and_then(|v| v.as_str()) {
                if !prompt.is_empty() {
                    // JSON quoting keeps a multiline task on one logical detail line, so
                    // later state-only updates can preserve the complete original task.
                    detail.push(format!(
                        "Task: {}",
                        serde_json::to_string(prompt).unwrap_or_default()
                    ));
                }
            }
            if let Some(sender) = item.get("sender_thread_id").and_then(|v| v.as_str()) {
                detail.push(format!("Sender: {sender}"));
            }
            detail.push(format!("Child: {id}"));
            if let Some(tool) = item.get("tool").and_then(|v| v.as_str()) {
                detail.push(format!("Tool: {tool}"));
            }
            detail.push(format!("State: {status}"));
            if let Some(message) = state
                .and_then(|s| s.get("message"))
                .and_then(|v| v.as_str())
            {
                detail.push(format!("Message: {message}"));
            }
            StreamPart::Card(StreamCard {
                id: format!("native-agent:{id}"),
                title: format!("agent {}", id.chars().take(12).collect::<String>()),
                detail: Some(detail.join("\n")),
                status: match status {
                    "completed" | "shutdown" => CardStatus::Done,
                    "errored" | "interrupted" | "not_found" => CardStatus::Error,
                    _ => CardStatus::Running,
                },
            })
        })
        .collect()
}

fn classify_payload(p: &serde_json::Value) -> Vec<StreamPart> {
    let p_type = p.get("type").and_then(|x| x.as_str()).unwrap_or("");
    if let Some(card) = web_search_card("response_item", p, p) {
        return vec![StreamPart::Card(card)];
    }
    if p_type == "message" && p.get("role").and_then(|x| x.as_str()) == Some("assistant") {
        if let Some(text) = content_to_text(p.get("content")) {
            return vec![StreamPart::Text(text)];
        }
    }
    if p_type == "function_call" || p_type == "custom_tool_call" {
        let name = p
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("tool")
            .to_string();
        let id = p
            .get("call_id")
            .or_else(|| p.get("id"))
            .and_then(|x| x.as_str())
            .unwrap_or(name.as_str())
            .to_string();
        let raw = p.get("arguments").or_else(|| p.get("input"));
        let status = match p.get("status").and_then(|x| x.as_str()) {
            Some("completed") => CardStatus::Done,
            Some("failed") => CardStatus::Error,
            _ => CardStatus::Running,
        };
        let detail = raw.map(format_provider_value).unwrap_or_default();
        let title = if matches!(name.as_str(), "exec" | "functions.exec") {
            wrapper_tool_title(&detail).unwrap_or(name)
        } else {
            name
        };
        return vec![StreamPart::Card(StreamCard {
            id,
            title,
            detail: if detail.is_empty() {
                None
            } else {
                Some(detail)
            },
            status,
        })];
    }
    if p_type == "function_call_output" || p_type == "custom_tool_call_output" {
        let id = p
            .get("call_id")
            .or_else(|| p.get("id"))
            .and_then(|x| x.as_str())
            .unwrap_or("out")
            .to_string();
        let out_text = p
            .get("output")
            .map(format_provider_value)
            .unwrap_or_default();
        let detail = if let Some(error) = p.get("error").filter(|value| !value.is_null()) {
            format!(
                "Output: {out_text}\nError: {}",
                format_provider_value(error)
            )
        } else {
            out_text
        };
        let failed = p.get("is_error").and_then(|v| v.as_bool()) == Some(true)
            || p.get("error").is_some_and(|value| !value.is_null())
            || p.get("status")
                .and_then(|v| v.as_str())
                .is_some_and(|status| matches!(status, "failed" | "error" | "cancelled"));
        return vec![StreamPart::Card(StreamCard {
            id,
            title: "result".into(),
            detail: if detail.is_empty() {
                None
            } else {
                Some(detail)
            },
            status: if failed {
                CardStatus::Error
            } else {
                CardStatus::Done
            },
        })];
    }
    Vec::new()
}

/// Describe statically named nested calls without evaluating JavaScript or inventing child IDs.
/// Quoted strings and comments are skipped so command output cannot masquerade as a tool call.
fn wrapper_tool_title(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut quote = None;
    let mut labels = Vec::new();
    while i < bytes.len().min(128 * 1024) {
        if let Some(delimiter) = quote {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == delimiter {
                quote = None;
            }
            i += 1;
            continue;
        }
        if matches!(bytes[i], b'\'' | b'"' | b'`') {
            quote = Some(bytes[i]);
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            i += 2;
            while i + 1 < bytes.len() && !bytes[i..].starts_with(b"*/") {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i..].starts_with(b"tools.")
            && (i == 0
                || !(bytes[i - 1].is_ascii_alphanumeric()
                    || matches!(bytes[i - 1], b'_' | b'$' | b'.')))
        {
            let start = i + 6;
            i = start;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let end = i;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if bytes.get(i) == Some(&b'(') && end > start {
                let name = &input[start..end];
                let label = match name {
                    "web__run" => "Browse web".into(),
                    "write_stdin" => "Read command output".into(),
                    _ => tool_label(name),
                };
                if !labels.contains(&label) {
                    labels.push(label);
                }
            }
            continue;
        }
        i += 1;
    }
    if labels.is_empty() {
        None
    } else {
        Some(labels.into_iter().take(4).collect::<Vec<_>>().join(" + "))
    }
}

fn web_search_card(typ: &str, item: &Value, event: &Value) -> Option<StreamCard> {
    let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
    if !matches!(kind, "web_search" | "web_search_call" | "webSearch")
        && !typ.starts_with("response.web_search_call.")
    {
        return None;
    }
    // A search without a native id cannot be safely merged with another search.
    let id = item
        .get("id")
        .or_else(|| item.get("call_id"))
        .or_else(|| event.get("item_id"))
        .and_then(Value::as_str)?;
    let action = item.get("action").unwrap_or(&Value::Null);
    let action_type = action
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("search");
    let title = match action_type {
        "open_page" | "openPage" => "Read web page",
        "find_in_page" | "findInPage" => "Find in web page",
        _ => "Search web",
    };
    let mut lines = Vec::new();
    let queries = item.get("queries").or_else(|| action.get("queries"));
    if let Some(queries) = queries.and_then(Value::as_array) {
        for query in queries.iter().filter_map(Value::as_str).take(16) {
            lines.push(format!("Query: {}", compact_detail(query, 1000)));
        }
    } else if let Some(query) = item
        .get("query")
        .or_else(|| action.get("query"))
        .and_then(Value::as_str)
    {
        if !query.is_empty() {
            lines.push(format!("Query: {}", compact_detail(query, 1000)));
        }
    }
    for key in ["url", "pattern"] {
        if let Some(value) = action.get(key).and_then(Value::as_str) {
            lines.push(format!(
                "{}: {}",
                if key == "url" { "URL" } else { "Find" },
                compact_detail(value, 4096)
            ));
        }
    }
    if let Some(results) = item
        .get("results")
        .or_else(|| item.get("sources"))
        .and_then(Value::as_array)
    {
        for source in results.iter().take(32) {
            if let Some(url) = source
                .get("url")
                .and_then(Value::as_str)
                .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
            {
                let title = source.get("title").and_then(Value::as_str).unwrap_or(url);
                lines.push(format!(
                    "Source: {}\n{}",
                    compact_detail(title, 300),
                    compact_detail(url, 4096)
                ));
            }
        }
    }
    // Keep structured source details available even when a provider adds fields
    // beyond the compact display contract. No text is promoted into the reply.
    if !typ.starts_with("response.web_search_call.") {
        lines.push(format!(
            "Native search details:\n{}",
            compact_detail(&format_provider_value(item), 64 * 1024)
        ));
    }
    let status = match item.get("status").and_then(Value::as_str) {
        Some("failed" | "error" | "cancelled" | "incomplete") => CardStatus::Error,
        Some("completed" | "done") => CardStatus::Done,
        Some("in_progress" | "searching" | "pending" | "running") => CardStatus::Running,
        _ if typ.ends_with(".failed") || typ.ends_with(".incomplete") => CardStatus::Error,
        _ if typ.ends_with(".completed") || typ.ends_with(".done") || typ == "response_item" => {
            CardStatus::Done
        }
        _ => CardStatus::Running,
    };
    Some(StreamCard {
        id: id.into(),
        title: title.into(),
        detail: (!lines.is_empty()).then(|| lines.join("\n")),
        status,
    })
}

fn item_type_is_tool(item_type: &str) -> bool {
    regex_lite_tool(item_type)
}

fn regex_lite_tool(item_type: &str) -> bool {
    let lower = item_type.to_ascii_lowercase();
    [
        "command",
        "shell",
        "exec",
        "tool",
        "function",
        "file",
        "mcp",
        "web_search",
        "search",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

fn card_status_from_type(typ: &str) -> CardStatus {
    let lower = typ.to_ascii_lowercase();
    if lower.contains("complet") {
        CardStatus::Done
    } else if lower.contains("fail") || lower.contains("error") {
        CardStatus::Error
    } else if lower.contains("start") || lower.contains("in_progress") {
        CardStatus::Running
    } else {
        CardStatus::Done
    }
}

fn typ_is_errorish(typ: &str) -> bool {
    let lower = typ.to_ascii_lowercase();
    lower.contains("error") || lower.contains("fail")
}

fn format_provider_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
    }
}

fn content_to_text(content: Option<&serde_json::Value>) -> Option<String> {
    let content = content?;
    if let Some(s) = content.as_str() {
        let t = s.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    let arr = content.as_array()?;
    let text = arr
        .iter()
        .filter_map(|c| {
            if let Some(s) = c.as_str() {
                return Some(s.to_string());
            }
            c.get("text").and_then(|x| x.as_str()).map(str::to_string)
        })
        .collect::<Vec<_>>()
        .join(" ");
    let t = text.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn pick_plain_text(ev: &serde_json::Value) -> Option<String> {
    if let Some(s) = ev.get("text").and_then(|x| x.as_str()) {
        let t = s.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    if let Some(s) = ev.get("message").and_then(|x| x.as_str()) {
        let t = s.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    if let Some(item) = ev.get("item") {
        if let Some(s) = item.get("text").and_then(|x| x.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
        if let Some(s) = item.get("content").and_then(|x| x.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
        if let Some(content) = item.get("content") {
            if let Some(t) = content_to_text(Some(content)) {
                return Some(t);
            }
        }
    }
    content_to_text(ev.get("content"))
}

pub fn compact_detail(raw: &str, max: usize) -> String {
    let t = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        return String::new();
    }
    if t.chars().count() <= max {
        return t;
    }
    let truncated: String = t.chars().take(max.saturating_sub(1)).collect();
    format!("{truncated}…")
}

pub fn upsert_card(cards: &mut Vec<StreamCard>, mut next: StreamCard) {
    super::edit_display::normalize(&mut next);
    if let Some(prev) = cards.iter_mut().find(|c| c.id == next.id) {
        let is_result_update = next.title == "result";
        let search_heartbeat = next.title == "Search web"
            && next.detail.is_none()
            && matches!(prev.title.as_str(), "Read web page" | "Find in web page");
        if !next.title.is_empty()
            && (!is_result_update || prev.title.is_empty())
            && !search_heartbeat
        {
            prev.title = next.title;
        }
        if let Some(d) = next.detail {
            if !d.is_empty() {
                if is_result_update {
                    match prev.detail.take() {
                        Some(existing) if existing == d => prev.detail = Some(existing),
                        Some(existing) if existing.ends_with(&format!("\nOutput: {d}")) => {
                            prev.detail = Some(existing)
                        }
                        Some(input) if !input.is_empty() => {
                            prev.detail = Some(format!("{input}\nOutput: {d}"))
                        }
                        _ => prev.detail = Some(d),
                    }
                } else if matches!(
                    tool_label(&prev.title).as_str(),
                    "Search web" | "Read web page" | "Find in web page"
                ) && !d.lines().any(|line| line.starts_with("Query: "))
                {
                    let query = prev
                        .detail
                        .as_deref()
                        .map(|old| {
                            old.lines()
                                .filter(|line| line.starts_with("Query: "))
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default();
                    prev.detail = Some(if query.is_empty() {
                        d
                    } else {
                        format!("{query}\n{d}")
                    });
                } else if prev.id.starts_with("native-agent:") && !d.starts_with("Task: ") {
                    let task = prev
                        .detail
                        .as_deref()
                        .and_then(|old| old.lines().find(|line| line.starts_with("Task: ")));
                    prev.detail = Some(match task {
                        Some(task) => format!("{task}\n{d}"),
                        None => d,
                    });
                } else {
                    prev.detail = Some(d);
                }
            }
        }
        prev.status = next.status;
    } else {
        cards.push(next);
    }
}

pub fn format_card_line(card: &StreamCard) -> String {
    let mark = match card.status {
        CardStatus::Running => "◉",
        CardStatus::Error => "✗",
        CardStatus::Done => "✓",
    };
    match &card.detail {
        Some(d) if !d.is_empty() => {
            format!("│ {mark} {}  {}", tool_label(&card.title), card_preview(d))
        }
        _ => format!("│ {mark} {}", tool_label(&card.title)),
    }
}

pub fn tool_label(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "shell" | "shell_command" | "command_execution" | "exec_command" | "bash"
        | "shelltoolcall" => "Run command".into(),
        "read" | "read_file" | "readtoolcall" => "Read file".into(),
        "edit" | "apply_patch" | "str_replace" | "edittoolcall" => "Edit file".into(),
        "write" | "write_file" | "writetoolcall" => "Write file".into(),
        "grep" | "rg" | "glob" | "search" | "searchtoolcall" => "Search files".into(),
        "web_search" | "web_search_call" | "websearch" | "web search" | "search web"
        | "search the web" => "Search web".into(),
        "web.run" | "web__run" | "functions.web__run" => "Browse web".into(),
        "webfetch" | "fetch" => "Read web page".into(),
        "spawn_agent" => "Start agent".into(),
        "wait_agent" | "wait" => "Wait for agents".into(),
        _ => name
            .strip_prefix("mcp__")
            .map(|s| s.replace("__", " / "))
            .unwrap_or_else(|| name.to_owned()),
    }
}

pub fn tool_icon(name: &str) -> &'static str {
    let label = tool_label(name).to_ascii_lowercase();
    if label.contains("edit") || label.contains("write") || label.contains("patch") {
        "✎"
    } else if label.contains("search") || label.contains("grep") || label.contains("glob") {
        "⌕"
    } else if label.contains("read") || label.contains("file") {
        "▤"
    } else if label.contains("agent") || label.contains("task") {
        "⑂"
    } else {
        "›"
    }
}

/// Compact presentation only: the original arguments and output remain on the card.
fn card_preview(detail: &str) -> String {
    let reported = detail
        .split_once("\nNative tool input and output:")
        .map_or(detail, |(reported, _)| reported);
    let reported = reported
        .split_once("\nOutput: ")
        .map_or(reported, |(reported, _)| reported);
    if reported.lines().any(|line| line.starts_with("@@")) {
        let added = reported
            .lines()
            .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
            .count();
        let removed = reported
            .lines()
            .filter(|line| line.starts_with('-') && !line.starts_with("---"))
            .count();
        let paths = reported
            .lines()
            .filter_map(|line| line.strip_prefix("File: "))
            .collect::<Vec<_>>()
            .join(", ");
        return format!("{paths}  +{added} -{removed}").trim().to_owned();
    }
    let first = detail
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    let raw = first.strip_prefix("command: ").unwrap_or(first);
    let mut command = raw.to_owned();
    // Some providers deliver tool arguments as a JSON string rather than an object.
    let input = detail
        .split_once("\nOutput: ")
        .map_or(detail, |(input, _)| input);
    let arguments = input
        .strip_prefix("arguments: ")
        .or_else(|| input.strip_prefix("input: "))
        .unwrap_or(input);
    if arguments.trim_start().starts_with(['{', '"']) {
        // Only the leading JSON value: the tool's output may follow it (Claude's cards).
        let first = serde_json::Deserializer::from_str(arguments)
            .into_iter::<serde_json::Value>()
            .next()
            .and_then(Result::ok);
        if let Some(value) = first {
            let value = match value.as_str() {
                Some(s) => serde_json::from_str(s).unwrap_or(value.clone()),
                None => value,
            };
            // The argument that names what the tool acted on.
            if let Some(subject) = [
                "cmd",
                "command",
                "file_path",
                "path",
                "pattern",
                "query",
                "url",
                "description",
            ]
            .iter()
            .find_map(|key| value.get(key).and_then(|v| v.as_str()))
            {
                command = subject.into();
            }
        }
    }
    let lower = command.to_ascii_lowercase();
    // Hide a recognized launcher only; this never changes the executed command.
    if lower.contains("powershell") || lower.contains("pwsh") {
        if let Some(index) = lower.find(" -command ").or_else(|| lower.find(" -c ")) {
            let rest = &command[index..];
            let content = rest
                .trim_start()
                .split_once(' ')
                .map(|(_, c)| c.trim())
                .unwrap_or(rest);
            command = content
                .strip_prefix('"')
                .and_then(|c| c.strip_suffix('"'))
                .unwrap_or(content)
                .to_owned();
        }
    }
    let output_lines = detail
        .split_once("\nOutput: ")
        .map(|(_, output)| output.lines().count());
    let summary = compact_detail(&command, 96);
    match output_lines {
        Some(1) => format!("{summary} · 1 output line"),
        Some(n) if n > 0 => format!("{summary} · {n} output lines"),
        _ => summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_shell_rows_show_the_command_and_preserve_full_output() {
        let detail = "command: \"C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe\" -NoProfile -Command \"cargo test -p demo\"\nOutput: first\nsecond";
        let card = StreamCard {
            id: "run".into(),
            title: "shell".into(),
            detail: Some(detail.into()),
            status: CardStatus::Done,
        };
        let row = format_card_line(&card);
        assert!(row.contains("cargo test -p demo"));
        assert!(row.contains("2 output lines"));
        assert!(!row.contains("WindowsPowerShell"));
        assert_eq!(card.detail.as_deref(), Some(detail));
        assert_eq!(
            card_preview("arguments: {\"cmd\":\"rg --files\"}"),
            "rg --files"
        );
        assert_eq!(
            card_preview("{\"command\":\"git status\"}\nOutput: clean"),
            "git status · 1 output line"
        );
    }

    // Synthetic fixtures following codex-rs/exec/src/exec_events.rs CollabToolCall.
    fn collab_cards(item: serde_json::Value) -> Vec<StreamCard> {
        classify_codex_event(&serde_json::json!({"type":"item.completed", "item":item}))
            .into_iter()
            .filter_map(|part| match part {
                StreamPart::Card(card) => Some(card),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn native_agent_spawn_wait_updates_preserve_multiline_task_and_child_status() {
        let mut cards = collab_cards(serde_json::json!({
            "type":"collab_tool_call", "id":"spawn-1", "tool":"spawn_agent",
            "sender_thread_id":"parent", "receiver_thread_ids":["child-a"],
            "prompt":"Review parser\nReturn evidence", "status":"completed", "agents_states":{}
        }));
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id, "native-agent:child-a");
        assert_eq!(cards[0].status, CardStatus::Running);
        for card in collab_cards(serde_json::json!({
            "type":"collab_tool_call", "id":"wait-2", "tool":"wait",
            "sender_thread_id":"parent", "receiver_thread_ids":["child-a"],
            "status":"completed",
            "agents_states":{"child-a":{"status":"completed","message":"Parser verified"}}
        })) {
            upsert_card(&mut cards, card);
        }
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].status, CardStatus::Done);
        let detail = cards[0].detail.as_deref().unwrap();
        assert!(detail.contains("Review parser\\nReturn evidence"));
        assert!(detail.contains("Sender: parent"));
        assert!(detail.contains("Child: child-a"));
        assert!(detail.contains("State: completed"));
        assert!(detail.contains("Parser verified"));
        assert!(!detail.contains("State: unknown"));
    }

    #[test]
    fn native_agent_multiple_receivers_unknown_states_and_failures_remain_distinct() {
        let cards = collab_cards(serde_json::json!({
            "type":"collab_tool_call", "tool":"wait", "status":"completed",
            "receiver_thread_ids":["a","a","b","c","d"],
            "agents_states":{
                "a":{"status":"running"}, "b":{"status":"future_state"},
                "c":{"status":"errored","message":"Worker failed"},
                "e":{"status":"interrupted"}, "f":{"status":"not_found"},
                "g":{"status":"shutdown"}, "h":{"status":"pending_init"}
            }
        }));
        assert_eq!(cards.len(), 8);
        let status = |id: &str| {
            cards
                .iter()
                .find(|c| c.id == format!("native-agent:{id}"))
                .unwrap()
                .status
        };
        for id in ["a", "b", "d", "h"] {
            assert_eq!(status(id), CardStatus::Running);
        }
        for id in ["c", "e", "f"] {
            assert_eq!(status(id), CardStatus::Error);
        }
        assert_eq!(status("g"), CardStatus::Done);
        assert!(cards
            .iter()
            .find(|c| c.id == "native-agent:c")
            .unwrap()
            .detail
            .as_deref()
            .unwrap()
            .contains("Worker failed"));
    }

    #[test]
    fn native_agent_without_child_identity_does_not_fabricate_an_agent() {
        assert!(collab_cards(serde_json::json!({
            "type":"collab_tool_call", "tool":"spawn_agent", "status":"failed",
            "receiver_thread_ids":[], "agents_states":{}
        }))
        .is_empty());
    }

    #[test]
    fn parses_agent_text_and_tool_cards() {
        let chunk = [
            r#"{"type":"thread.started","thread_id":"thr-1"}"#,
            r#"{"type":"item.started","item":{"type":"command_execution","id":"c1","command":"npm test","status":"in_progress"}}"#,
            r#"{"type":"item.completed","item":{"type":"command_execution","id":"c1","command":"npm test","status":"completed"}}"#,
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"all green"}}"#,
            r#"{"type":"error","message":"spend cap"}"#,
        ]
        .join("\n");
        let parts = parse_codex_stream_chunk(&chunk);
        assert!(parts
            .iter()
            .any(|p| matches!(p, StreamPart::Session(id) if id == "thr-1")));
        assert!(parts.iter().any(|p| matches!(
            p,
            StreamPart::Card(c) if c.title == "shell" || c.detail.as_deref().unwrap_or("").contains("npm")
        )));
        assert!(parts
            .iter()
            .any(|p| matches!(p, StreamPart::Text(t) if t == "all green")));
        assert!(parts
            .iter()
            .any(|p| matches!(p, StreamPart::Error(t) if t.contains("spend"))));
        // Codex exec and Responses/native history use distinct web-search envelopes.
        let web_events = [
            r#"{"type":"item.started","item":{"type":"web_search","id":"web-1","query":"Rust clipboard","action":{"type":"search","query":"Rust clipboard"}}}"#,
            r#"{"type":"item.completed","item":{"type":"web_search","id":"web-1","action":{"type":"search"},"results":[{"title":"Rust docs","url":"https://doc.rust-lang.org/"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"web_search_call","id":"web-2","status":"completed","action":{"type":"open_page","url":"https://doc.rust-lang.org/"}}}"#,
            r#"{"type":"response.output_item.added","item":{"type":"web_search_call","id":"web-3","action":{"type":"search","queries":["native search"]}}}"#,
            r#"{"type":"response.web_search_call.completed","item_id":"web-3"}"#,
            r#"{"type":"item.completed","item":{"type":"web_search","id":"web-4","status":"failed","query":"failed query","error":"search unavailable"}}"#,
        ].join("\n");
        let web = parse_codex_stream_chunk(&web_events);
        let mut cards = Vec::new();
        for part in web {
            match part {
                StreamPart::Card(card) => upsert_card(&mut cards, card),
                _ => panic!("search emitted a non-tool part"),
            }
        }
        assert_eq!(cards.len(), 4);
        assert_eq!(cards[0].status, CardStatus::Done);
        let detail = cards[0].detail.as_deref().unwrap();
        assert!(detail.contains("Query: Rust clipboard"));
        assert!(detail.contains("Source: Rust docs\nhttps://doc.rust-lang.org/"));
        assert!(format_card_line(&cards[0]).contains("Rust clipboard"));
        assert_eq!(cards[1].title, "Read web page");
        assert!(cards[1]
            .detail
            .as_deref()
            .unwrap()
            .contains("https://doc.rust-lang.org/"));
        assert_eq!(cards[2].status, CardStatus::Done);
        assert!(cards[2]
            .detail
            .as_deref()
            .unwrap()
            .contains("native search"));
        assert_eq!(cards[3].status, CardStatus::Error);
        assert!(cards[3]
            .detail
            .as_deref()
            .unwrap()
            .contains("search unavailable"));
        // Observed native account codex-2 uses a custom exec wrapper containing mixed calls.
        let mixed = classify_payload(
            &serde_json::json!({"type":"custom_tool_call", "call_id":"mixed-web", "name":"exec", "input":"text(await tools.web__run({search_query:[{q:\"Rust clipboard\"}]})); text(await tools.exec_command({cmd:\"git status\"}));"}),
        );
        let StreamPart::Card(mixed) = &mixed[0] else {
            panic!("missing wrapper card")
        };
        assert_eq!(mixed.id, "mixed-web");
        assert_eq!(mixed.title, "Browse web + Run command");
        assert!(mixed.detail.as_deref().unwrap().contains("Rust clipboard"));
        assert_eq!(
            wrapper_tool_title("// tools.web__run({})\ntext('tools.exec_command({})');"),
            None
        );
    }

    #[test]
    fn upserts_by_id() {
        let mut cards = Vec::new();
        upsert_card(
            &mut cards,
            StreamCard {
                id: "a".into(),
                title: "exec".into(),
                detail: Some("npm".into()),
                status: CardStatus::Running,
            },
        );
        upsert_card(
            &mut cards,
            StreamCard {
                id: "a".into(),
                title: "exec".into(),
                detail: None,
                status: CardStatus::Done,
            },
        );
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].status, CardStatus::Done);
        assert_eq!(cards[0].detail.as_deref(), Some("npm"));
    }

    #[test]
    fn output_update_keeps_the_call_title_and_arguments() {
        let mut cards = Vec::new();
        upsert_card(
            &mut cards,
            StreamCard {
                id: "call-1".into(),
                title: "shell_command".into(),
                detail: Some("{\"command\":\"git status\"}".into()),
                status: CardStatus::Running,
            },
        );
        upsert_card(
            &mut cards,
            StreamCard {
                id: "call-1".into(),
                title: "result".into(),
                detail: Some("working tree clean".into()),
                status: CardStatus::Done,
            },
        );
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "shell_command");
        assert_eq!(cards[0].status, CardStatus::Done);
        let detail = cards[0].detail.as_deref().unwrap();
        assert!(detail.contains("git status") && detail.contains("working tree clean"));
    }

    #[test]
    fn replaying_the_same_output_does_not_duplicate_it() {
        let input = StreamCard {
            id: "call-1".into(),
            title: "shell_command".into(),
            detail: Some("git status --short".into()),
            status: CardStatus::Running,
        };
        let output = StreamCard {
            id: "call-1".into(),
            title: "result".into(),
            detail: Some("working tree clean".into()),
            status: CardStatus::Done,
        };
        let mut cards = Vec::new();
        upsert_card(&mut cards, input);
        upsert_card(&mut cards, output.clone());
        let once = cards[0].detail.clone();
        upsert_card(&mut cards, output);
        assert_eq!(cards[0].detail, once);
        assert_eq!(
            cards[0]
                .detail
                .as_deref()
                .unwrap()
                .matches("Output:")
                .count(),
            1
        );
    }

    #[test]
    fn codex_turn_completed_reports_usage_and_tolerates_gaps() {
        let parts = parse_codex_stream_chunk(
            r#"{"type":"turn.completed","usage":{"input_tokens":2000,"cached_input_tokens":1500,"output_tokens":80}}"#,
        );
        assert_eq!(
            parts,
            vec![StreamPart::Usage(TurnUsage {
                input: Some(2000),
                output: Some(80),
                cached: Some(1500),
                cost_usd: None
            })]
        );
        let partial =
            parse_codex_stream_chunk(r#"{"type":"turn.completed","usage":{"output_tokens":9}}"#);
        assert!(
            matches!(&partial[0], StreamPart::Usage(u) if u.output == Some(9) && u.input.is_none())
        );
        assert!(parse_codex_stream_chunk(r#"{"type":"turn.completed"}"#).is_empty());
        assert!(parse_codex_stream_chunk(r#"{"type":"turn.completed","usage":[1]}"#).is_empty());
    }

    #[test]
    fn codex_keeps_command_output_and_mcp_data_with_call_status() {
        let parts = parse_codex_stream_chunk(&[
            r#"{"type":"item.started","item":{"type":"command_execution","id":"cmd-1","command":"git status --short","aggregated_output":"working…"}}"#,
            r#"{"type":"item.completed","item":{"type":"mcp_tool_call","id":"mcp-1","server":"docs","tool":"search","arguments":{"query":"雪"},"result":{"matches":[1]},"error":{"code":7,"message":"offline"}}}"#,
        ].join("\n"));
        let StreamPart::Card(command) = &parts[0] else {
            panic!("{parts:?}")
        };
        assert_eq!(command.status, CardStatus::Running);
        assert!(command
            .detail
            .as_deref()
            .unwrap()
            .contains("git status --short\nOutput: working…"));
        let StreamPart::Card(mcp) = &parts[1] else {
            panic!("{parts:?}")
        };
        assert_eq!(mcp.id, "mcp-1");
        assert_eq!(mcp.title, "docs/search");
        assert_eq!(mcp.status, CardStatus::Error);
        let detail = mcp.detail.as_deref().unwrap();
        assert!(detail.contains("MCP server: docs") && detail.contains("MCP tool: search"));
        assert!(detail.contains("雪") && detail.contains("offline"));
    }

    #[test]
    fn codex_file_changes_keep_provider_paths_and_diff_content() {
        let parts = parse_codex_stream_chunk(
            r#"{"type":"item.completed","item":{"type":"file_change","id":"edit-1","changes":[{"path":"src/雪.rs","kind":"update","old":"old line","new":"new line","diff":"@@ -1 +1 @@\n-old line\n+new line"}]}}"#,
        );
        let StreamPart::Card(card) = &parts[0] else {
            panic!("{parts:?}")
        };
        let detail = card.detail.as_deref().unwrap();
        assert_eq!(card.title, "Edit");
        let row = format_card_line(card);
        assert!(row.contains("+1 -1"), "{row}");
        assert!(row.contains("src/"), "{row}");
        for expected in ["src/雪.rs", "old line", "new line", "@@ -1 +1 @@"] {
            assert!(detail.contains(expected), "missing {expected}: {detail}");
        }
        let mut edits = Vec::new();
        let input = serde_json::json!({"file_path":"src/parser.rs","old_string":"one\ntwo","new_string":"three"}).to_string();
        upsert_card(
            &mut edits,
            StreamCard {
                id: "replacement".into(),
                title: "Edit".into(),
                detail: Some(input.clone()),
                status: CardStatus::Running,
            },
        );
        assert!(format_card_line(&edits[0]).contains("src/parser.rs  +1 -2"));
        let rendered = edits[0].detail.as_deref().unwrap();
        assert!(rendered.contains("-one\n-two\n+three"));
        assert!(rendered.contains(&input));

        // Real exec file-change events provide paths/kinds, not a fictional `diff` field.
        let parts = parse_codex_stream_chunk(
            r#"{"type":"item.completed","item":{"type":"file_change","id":"path-only","changes":[{"path":"src/parser space.rs","kind":"update"},{"path":"src/new.rs","kind":"add"},{"path":"src/gone.rs","kind":"delete"}]}}"#,
        );
        let StreamPart::Card(card) = &parts[0] else {
            panic!("{parts:?}")
        };
        let mut cards = vec![card.clone()];
        let patch = "*** Begin Patch\n*** Update File: src/parser space.rs\n@@\n-old source\n+new source\n*** Add File: src/new.rs\n+new file\n*** Delete File: src/gone.rs\n*** End Patch\n";
        let updated = super::super::edit_display::enrich_codex(
            &mut cards,
            &[patch.into()],
            std::path::Path::new("."),
        );
        assert_eq!(updated.len(), 1);
        let detail = updated[0].detail.as_deref().unwrap();
        assert!(
            detail.contains("File: src/parser space.rs\n@@\n-old source\n+new source"),
            "{detail}"
        );
        assert!(detail.contains("File: src/new.rs\n+new file"), "{detail}");
        assert!(
            detail
                .contains("File: src/gone.rs\nProvider reported a change; line diff unavailable."),
            "delete input contains no old file body; do not invent removed lines: {detail}"
        );
        assert_eq!(
            super::super::edit_display::enrich_codex(
                &mut cards,
                &[patch.into()],
                std::path::Path::new("."),
            )
            .len(),
            0,
            "repeated enrichment must not duplicate hunks"
        );
    }

    #[test]
    fn codex_uses_exit_and_error_fields_without_treating_null_as_failure() {
        let parts = parse_codex_stream_chunk(&[
            r#"{"type":"item.completed","item":{"type":"command_execution","id":"zero","command":"true","exit_code":0,"error":null}}"#,
            r#"{"type":"item.completed","item":{"type":"command_execution","id":"bad","command":"false","exit_code":1,"error":{"message":"failed"}}}"#,
            "{malformed",
        ]
        .join("\n"));
        let (StreamPart::Card(ok), StreamPart::Card(failed)) = (&parts[0], &parts[1]) else {
            panic!("{parts:?}")
        };
        assert_eq!(ok.status, CardStatus::Done);
        assert_eq!(failed.status, CardStatus::Error);
        assert!(failed.detail.as_deref().unwrap().contains("failed"));
        assert_eq!(parts.len(), 2, "malformed Codex JSON is ignored");
    }

    #[test]
    fn codex_response_item_output_updates_the_matching_function_call() {
        let parts = parse_codex_stream_chunk(&[
            r#"{"type":"response_item","payload":{"type":"function_call","call_id":"fc-1","name":"shell_command","arguments":{"command":"git status --short","workdir":"C:/repo"}}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"fc-1","output":{"stdout":"clean","exit_code":0}}}"#,
        ].join("\n"));
        let mut cards = Vec::new();
        for part in parts {
            if let StreamPart::Card(card) = part {
                upsert_card(&mut cards, card);
            }
        }
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id, "fc-1");
        assert_eq!(cards[0].title, "shell_command");
        assert_eq!(cards[0].status, CardStatus::Done);
        let detail = cards[0].detail.as_deref().unwrap();
        for expected in ["git status --short", "C:/repo", "clean", "exit_code"] {
            assert!(detail.contains(expected), "missing {expected}: {detail}");
        }
    }
}
