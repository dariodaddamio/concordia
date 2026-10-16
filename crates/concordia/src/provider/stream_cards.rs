//! Clean-room Codex JSONL -> text / tool cards.

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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnUsage {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cached: Option<u64>,
    pub cost_usd: Option<f64>,
}

impl TurnUsage {
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
    Card(StreamCard),
    Error(String),
    Session(String),
    /// The model's reasoning, shown dimmed and never added to the reply or the context pack.
    Thinking(String),
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

pub fn classify_codex_event(ev: &serde_json::Value) -> Vec<StreamPart> {
    let typ = ev.get("type").and_then(|x| x.as_str()).unwrap_or("");
    if typ == "thread.started" {
        if let Some(id) = ev.get("thread_id").and_then(|x| x.as_str()) {
            return vec![StreamPart::Session(id.to_string())];
        }
    }
    if typ == "error" {
        if let Some(msg) = ev.get("message").and_then(|x| x.as_str()) {
            return vec![StreamPart::Error(msg.to_string())];
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
        if let Some(msg) = ev
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|x| x.as_str())
        {
            return vec![StreamPart::Error(msg.to_string())];
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
        let detail_raw = item
            .get("command")
            .and_then(|x| x.as_str())
            .or_else(|| item.get("input").and_then(|x| x.as_str()))
            .or_else(|| item.get("arguments").and_then(|x| x.as_str()))
            .or_else(|| item.get("path").and_then(|x| x.as_str()))
            .or_else(|| item.get("text").and_then(|x| x.as_str()))
            .unwrap_or("");
        let detail = compact_detail(detail_raw, 72);
        let status = card_status_from_type(typ);
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

fn classify_payload(p: &serde_json::Value) -> Vec<StreamPart> {
    let p_type = p.get("type").and_then(|x| x.as_str()).unwrap_or("");
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
        let raw = p
            .get("arguments")
            .and_then(|x| x.as_str())
            .or_else(|| p.get("input").and_then(|x| x.as_str()))
            .unwrap_or("");
        let status = match p.get("status").and_then(|x| x.as_str()) {
            Some("completed") => CardStatus::Done,
            Some("failed") => CardStatus::Error,
            _ => CardStatus::Running,
        };
        let detail = compact_detail(&shellish_detail(raw, &name), 72);
        return vec![StreamPart::Card(StreamCard {
            id,
            title: name,
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
        let out_text = content_to_text(p.get("output"))
            .or_else(|| p.get("output").and_then(|x| x.as_str()).map(str::to_string))
            .unwrap_or_default();
        let detail = compact_detail(&out_text, 72);
        return vec![StreamPart::Card(StreamCard {
            id,
            title: "result".into(),
            detail: if detail.is_empty() {
                None
            } else {
                Some(detail)
            },
            status: CardStatus::Done,
        })];
    }
    Vec::new()
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

fn shellish_detail(raw: &str, name: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    if name == "shell_command" || name == "shell" {
        if let Ok(j) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(cmd) = j.get("command").and_then(|x| x.as_str()) {
                return cmd.to_string();
            }
        }
    }
    raw.to_string()
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

pub fn upsert_card(cards: &mut Vec<StreamCard>, next: StreamCard) {
    if let Some(prev) = cards.iter_mut().find(|c| c.id == next.id) {
        if !next.title.is_empty() {
            prev.title = next.title;
        }
        if let Some(d) = next.detail {
            if !d.is_empty() {
                prev.detail = Some(d);
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
        Some(d) if !d.is_empty() => format!("│ {mark} {}  {d}", card.title),
        _ => format!("│ {mark} {}", card.title),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
