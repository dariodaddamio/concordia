//! Explicit, bounded access to native CLI chats. No session resume, context transfer or inference.
use crate::provider::stream_cards::{
    classify_codex_event, format_card_line, upsert_card, StreamCard, StreamPart,
};
use crate::{sessions, store::ProviderKind};
use serde_json::{json, Value};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const HELP: &str = "usage: concordia chat list [--account ID] [--provider codex|claude|cursor] [--json] [--limit N]\n       concordia chat read SESSION --account ID [--provider codex|claude|cursor] [--tail N] [--grep TEXT] [--max-chars N]\n       concordia chat search TEXT [--account ID] [--provider codex|claude|cursor] [--json] [--limit N] (session IDs and previews/titles only)";
const MAX_CHAT_OUTPUT_BYTES: usize = 5 * 1024 * 1024;

/// Read-only restoration data. Loading it never dispatches a provider turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryMessage {
    pub role: String,
    pub text: String,
    /// Original native tool identity and reported input/output, for display only.
    pub card: Option<StreamCard>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    pub title: Option<String>,
    pub messages: Vec<HistoryMessage>,
    pub truncated: bool,
}

const HISTORY_MESSAGES: usize = 200;
const HISTORY_TEXT_BYTES: usize = 1024 * 1024;
const HISTORY_NATIVE_BYTES: u64 = 8 * 1024 * 1024;

fn bound_history_card(card: &mut StreamCard) -> bool {
    const LIMIT: usize = 64 * 1024;
    const NOTICE: &str = "\n[Saved-history detail truncated; original native source retained.]\n";
    let Some(detail) = &mut card.detail else {
        return false;
    };
    if detail.len() <= LIMIT {
        return false;
    }
    let side = (LIMIT - NOTICE.len()) / 2;
    let mut head = side;
    while !detail.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = detail.len() - side;
    while !detail.is_char_boundary(tail) {
        tail += 1;
    }
    *detail = format!("{}{NOTICE}{}", &detail[..head], &detail[tail..]);
    true
}

/// Read a bounded scrollback from an explicitly bound account and native session.
/// File discovery and I/O belong on a worker thread, outside the render loop.
pub fn read_history(
    root: &Path,
    account: &str,
    provider: ProviderKind,
    session_id: &str,
) -> Result<History, String> {
    let (session, path) = sessions::find_native_record(root, account, Some(provider), session_id)?
        .ok_or_else(|| "session not found in that registered account and provider".to_string())?;
    let mut history = if provider == ProviderKind::Cursor {
        let home = crate::accounts::get_account(root, account)
            .and_then(|a| a.home_path)
            .ok_or("Cursor account home is unavailable")?;
        let binding = crate::native_memory::Binding::new(provider, account, &home, session_id)?;
        let mut buffer = HistoryBuffer::new(false);
        for (role, text) in crate::native_memory::cursor_text(root, &binding)? {
            buffer.push(&role, &text);
        }
        buffer.finish()
    } else {
        read_history_file(&path, provider)?
    };
    history.title = session.preview.map(|s| terminal_safe(&s));
    Ok(history)
}

fn read_history_file(path: &Path, provider: ProviderKind) -> Result<History, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    let start = size.saturating_sub(HISTORY_NATIVE_BYTES);
    let partial_first = if start > 0 {
        file.seek(SeekFrom::Start(start - 1))
            .map_err(|e| e.to_string())?;
        let mut previous = [0];
        file.read_exact(&mut previous).map_err(|e| e.to_string())?;
        previous[0] != b'\n'
    } else {
        false
    };
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(HISTORY_NATIVE_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    // Discard the incomplete first JSON line, including any split UTF-8 character.
    let begin = if partial_first {
        bytes
            .iter()
            .position(|b| *b == b'\n')
            .map_or(bytes.len(), |i| i + 1)
    } else {
        0
    };
    let mut primary = HistoryBuffer::new(start > 0);
    let mut fallback = HistoryBuffer::new(start > 0);
    let mut claude = crate::provider::stream_json::StreamParser::new(
        crate::provider::stream_json::Flavor::Claude,
    );
    for line in bytes[begin..].split(|b| *b == b'\n') {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let payload = match provider {
            ProviderKind::Codex => value.get("payload").unwrap_or(&value),
            _ => value.get("message").unwrap_or(&value),
        };
        let role = payload.get("role").and_then(Value::as_str).unwrap_or(kind);
        let structured_assistant = provider == ProviderKind::Claude
            && role == "assistant"
            && payload.get("content").is_some_and(Value::is_array);
        let activity = if provider == ProviderKind::Codex {
            let mut parts = classify_codex_event(&value);
            if kind == "response_item"
                && payload.get("type").and_then(Value::as_str) == Some("reasoning")
            {
                // Only provider-reported readable summaries; encrypted reasoning stays opaque.
                if let Some(summary) = payload.get("summary").and_then(Value::as_array) {
                    let text = summary
                        .iter()
                        .filter_map(|block| block.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !text.is_empty() {
                        parts.push(StreamPart::Thinking(text));
                    }
                }
            }
            if kind == "event_msg"
                && payload.get("type").and_then(Value::as_str) == Some("agent_reasoning")
            {
                if let Some(text) = payload.get("text").and_then(Value::as_str) {
                    parts.push(StreamPart::Thinking(text.to_owned()));
                }
            }
            parts
        } else {
            claude.parse_line(std::str::from_utf8(line).unwrap_or_default())
        };
        for part in activity {
            match part {
                StreamPart::Card(card) if reported_tool_id(&value, &card.id) => {
                    primary.push_card(card.clone());
                    fallback.push_card(card);
                }
                StreamPart::Thinking(text) => {
                    primary.push("thinking", &text);
                    fallback.push("thinking", &text);
                }
                StreamPart::Text(text) if structured_assistant => primary.push("assistant", &text),
                _ => {}
            }
        }
        if structured_assistant {
            continue;
        }
        if matches!(role, "user" | "assistant") {
            let content = payload.get("content");
            let text = content
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    content.and_then(Value::as_array).map(|blocks| {
                        blocks
                            .iter()
                            .filter_map(|b| {
                                let kind = b.get("type").and_then(Value::as_str).unwrap_or("text");
                                matches!(kind, "text" | "input_text" | "output_text")
                                    .then(|| b.get("text").and_then(Value::as_str))
                                    .flatten()
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                });
            if let Some(text) = text {
                primary.push(role, &text);
            }
        } else if kind == "event_msg" {
            let kind = payload.get("type").and_then(Value::as_str).unwrap_or("");
            if let Some(text) = payload.get("message").and_then(Value::as_str) {
                match kind {
                    "user_message" => fallback.push("user", text),
                    "agent_message" => fallback.push("assistant", text),
                    _ => {}
                }
            }
        }
    }
    Ok(if !primary
        .messages
        .iter()
        .any(|message| matches!(message.role.as_str(), "user" | "assistant"))
    {
        fallback
    } else {
        primary
    }
    .finish())
}

fn reported_tool_id(value: &Value, id: &str) -> bool {
    let payload = value.get("payload").unwrap_or(value);
    let item = value.get("item").unwrap_or(payload);
    [
        value.get("item_id"),
        payload.get("id"),
        payload.get("call_id"),
        item.get("id"),
        item.get("call_id"),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.as_str() == Some(id))
        || value
            .pointer("/message/content")
            .and_then(Value::as_array)
            .is_some_and(|blocks| {
                blocks.iter().any(|block| {
                    block
                        .get("id")
                        .or_else(|| block.get("tool_use_id"))
                        .and_then(Value::as_str)
                        == Some(id)
                })
            })
}

struct HistoryBuffer {
    messages: std::collections::VecDeque<HistoryMessage>,
    bytes: usize,
    truncated: bool,
}

impl HistoryBuffer {
    fn new(truncated: bool) -> Self {
        Self {
            messages: std::collections::VecDeque::new(),
            bytes: 0,
            truncated,
        }
    }

    fn push(&mut self, role: &str, text: &str) {
        let text = if role == "user" {
            visible_user_history(text)
        } else {
            text
        };
        let mut text = terminal_safe(text);
        if text.trim().is_empty() {
            return;
        }
        if role == "thinking"
            && self
                .messages
                .back()
                .is_some_and(|message| message.role == role && message.text == text)
        {
            return;
        }
        if text.len() > HISTORY_TEXT_BYTES {
            let mut start = text.len() - HISTORY_TEXT_BYTES;
            while !text.is_char_boundary(start) {
                start += 1;
            }
            text = text[start..].to_owned();
            self.truncated = true;
        }
        self.bytes += text.len();
        self.messages.push_back(HistoryMessage {
            role: role.into(),
            text,
            card: None,
        });
        self.enforce_bound();
    }

    fn push_card(&mut self, mut card: StreamCard) {
        card.title = terminal_safe(&card.title);
        card.detail = card.detail.as_deref().map(terminal_safe);
        self.truncated |= bound_history_card(&mut card);
        let start = self
            .messages
            .iter()
            .rposition(|message| message.role == "user")
            .unwrap_or(0);
        if let Some(index) = (start..self.messages.len()).find(|&index| {
            self.messages[index]
                .card
                .as_ref()
                .is_some_and(|old| old.id == card.id)
        }) {
            let message = &mut self.messages[index];
            self.bytes = self.bytes.saturating_sub(
                message.text.len()
                    + message
                        .card
                        .as_ref()
                        .and_then(|card| card.detail.as_ref())
                        .map_or(0, String::len),
            );
            let mut cards = vec![message.card.take().unwrap()];
            upsert_card(&mut cards, card);
            card = cards.remove(0);
            self.truncated |= bound_history_card(&mut card);
            message.text = format_card_line(&card);
            self.bytes += message.text.len() + card.detail.as_ref().map_or(0, String::len);
            message.card = Some(card);
        } else {
            let text = format_card_line(&card);
            self.bytes += text.len() + card.detail.as_ref().map_or(0, String::len);
            self.messages.push_back(HistoryMessage {
                role: "card".into(),
                text,
                card: Some(card),
            });
        }
        self.enforce_bound();
    }

    fn enforce_bound(&mut self) {
        while self.messages.len() > HISTORY_MESSAGES || self.bytes > HISTORY_TEXT_BYTES {
            if let Some(message) = self.messages.pop_front() {
                self.bytes -= message.text.len()
                    + message
                        .card
                        .as_ref()
                        .and_then(|card| card.detail.as_ref())
                        .map_or(0, String::len);
            }
            self.truncated = true;
        }
    }

    fn finish(self) -> History {
        History {
            title: None,
            messages: self.messages.into_iter().collect(),
            truncated: self.truncated,
        }
    }
}

pub(crate) fn visible_user_history(mut text: &str) -> &str {
    // Native transcripts include setup and background notices as user messages.
    // Keep any actual request following those wrappers.
    loop {
        text = text.trim_start();
        let wrapper = [
            "environment_context",
            "permissions instructions",
            "skills_instructions",
            "collaboration_mode",
            "task-notification",
            "system-reminder",
            "local-command-caveat",
        ]
        .into_iter()
        .find(|tag| text.starts_with(&format!("<{tag}>")));
        let Some(tag) = wrapper else { break };
        let end = format!("</{tag}>");
        let Some(index) = text.find(&end) else {
            return "";
        };
        text = &text[index + end.len()..];
    }
    if text.starts_with("# AGENTS.md instructions") {
        ""
    } else {
        // The time header Concordia put on the prompt was never part of what the user typed.
        crate::prompt_time::strip_header(text)
    }
}

fn bounded_field(value: &str, max_chars: usize) -> String {
    let safe = terminal_safe(value);
    let mut chars = safe.chars();
    let mut out: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() && max_chars > 0 {
        out.pop();
        out.push('…');
    }
    out
}

pub fn run(root: &Path, args: &[String]) -> Result<String, (String, i32)> {
    if args.first().is_some_and(|s| {
        matches!(
            s.as_str(),
            "recall" | "file-facts"
        )
    }) {
        return crate::native_memory::retrieval::command(root, args).map_err(|e| (e, 2));
    }
    if args
        .first()
        .is_some_and(|operation| matches!(operation.as_str(), "index" | "excerpt" | "sources"))
    {
        return crate::native_memory::command(root, args).map_err(|error| (error, 2));
    }
    run_inner(root, args)
        .map(|output| terminal_safe(&output))
        .map_err(|error| (terminal_safe(&format!("{error}\n{HELP}")), 2))
}

fn run_inner(root: &Path, args: &[String]) -> Result<String, String> {
    let operation = args.first().map(String::as_str).ok_or(HELP)?;
    if !matches!(operation, "list" | "read" | "search") {
        return Err("unknown chat operation".into());
    }
    let mut account = None;
    let mut provider = None;
    let mut limit = 30usize;
    let mut tail = 64usize;
    let mut grep = None;
    let mut max_chars = 40_000usize;
    let mut as_json = false;
    let mut positional = None;
    let mut seen = std::collections::HashSet::new();
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg.starts_with("--") {
            if !seen.insert(arg.as_str()) {
                return Err(format!("duplicate option {arg}"));
            }
            if arg == "--json" {
                as_json = true;
                i += 1;
                continue;
            }
            let value = args
                .get(i + 1)
                .filter(|v| !v.starts_with("--"))
                .ok_or_else(|| format!("{arg} needs a value"))?;
            let number = || {
                value
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("{arg} needs a positive integer"))
            };
            match arg.as_str() {
                "--account" => account = Some(value.as_str()),
                "--provider" => {
                    provider =
                        Some(crate::commands::parse_provider(value).ok_or("unknown provider")?)
                }
                "--limit" if operation != "read" => {
                    limit = number()?;
                    if limit > 1000 {
                        return Err("limit exceeds 1000".into());
                    }
                }
                "--tail" if operation == "read" => {
                    tail = number()?;
                    if tail > 1000 {
                        return Err("tail exceeds 1000".into());
                    }
                }
                "--max-chars" if operation == "read" => {
                    max_chars = number()?;
                    if max_chars > 100_000 {
                        return Err("max-chars exceeds 100000".into());
                    }
                }
                "--grep" if operation == "read" => grep = Some(value.as_str()),
                _ => return Err(format!("unknown or inapplicable option {arg}")),
            }
            i += 2;
        } else {
            if positional.replace(arg.as_str()).is_some() {
                return Err("too many positional arguments".into());
            }
            i += 1;
        }
    }
    if as_json && operation == "read" {
        return Err("--json is supported for list/search only".into());
    }
    if operation == "read" && (account.is_none() || positional.is_none()) {
        return Err("read requires a session ID and --account".into());
    }
    if operation == "search" && positional.is_none() {
        return Err("search needs text".into());
    }
    if operation == "list" && positional.is_some() {
        return Err("list accepts only named options".into());
    }
    if operation == "read" {
        let id = positional.ok_or("read needs an explicit session ID")?;
        let account = account.ok_or("read requires --account to bind chat ownership")?;
        let (session, path) = sessions::find_native_record(root, account, provider, id)?
            .ok_or_else(|| "session not found in that registered account".to_string())?;
        let (messages, truncated) = if session.provider == ProviderKind::Cursor {
            let history = read_history(root, account, session.provider, id)?;
            (
                history
                    .messages
                    .into_iter()
                    .map(|m| format!("{}: {}", m.role, m.text))
                    .collect(),
                history.truncated,
            )
        } else {
            read_messages(&path, session.provider)?
        };
        let messages: Vec<_> = messages
            .into_iter()
            .filter(|s| grep.is_none_or(|g| s.contains(g)))
            .collect();
        let body = messages
            .iter()
            .skip(messages.len().saturating_sub(tail))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n\n");
        let mut output = format!(
            "{} · {} · {}\n{}",
            session.provider.as_str(),
            account,
            id,
            body.chars().take(max_chars).collect::<String>()
        );
        if truncated || body.chars().count() > max_chars {
            output.push_str("\n[bounded output; transcript or display limit reached]");
        }
        return Ok(output + "\n");
    }
    let scan_limit = if operation == "search" { 1000 } else { limit };
    let scan = sessions::list_native_records_bounded(root, scan_limit, account, provider);
    if scan.truncated && as_json {
        return Err(
            "native chat scan bound reached; narrow the query with --account or --provider".into(),
        );
    }
    let mut records = scan.records;
    if operation == "search" {
        let query = positional.ok_or("search needs text")?.to_lowercase();
        records.retain(|(s, _)| {
            s.preview
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&query)
                || s.session_id.to_lowercase().contains(&query)
        });
    } else if positional.is_some() {
        return Err("list accepts only named options".into());
    }
    records.truncate(limit);
    if as_json {
        let mut rows: Vec<Value> = Vec::new();
        let mut encoded_bytes = 2usize;
        for (s, _) in &records {
            let row = json!({
                "account": s.account,
                "provider": s.provider.as_str(),
                "session_id": s.session_id,
                "cwd": s.cwd.as_deref().map(|v| bounded_field(v, 256)),
                "preview": s.preview.as_deref().map(|v| bounded_field(v, 512)),
                "modified_ms": s.modified_ms
            });
            let row_bytes = serde_json::to_vec(&row).map_err(|e| e.to_string())?.len();
            encoded_bytes = encoded_bytes.saturating_add(row_bytes + usize::from(!rows.is_empty()));
            if encoded_bytes > MAX_CHAT_OUTPUT_BYTES {
                return Err("chat JSON output exceeds the 5 MiB limit; narrow the query".into());
            }
            rows.push(row);
        }
        let output = serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())? + "\n";
        if output.len() > MAX_CHAT_OUTPUT_BYTES {
            return Err("chat JSON output exceeds the 5 MiB limit; narrow the query".into());
        }
        return Ok(output);
    }
    if records.is_empty() && !as_json {
        return Ok(if scan.truncated {
            "no matching native chats in the bounded scan; narrow with --account or --provider\n"
                .into()
        } else {
            "no matching native chats\n".into()
        });
    }
    let mut output = records
        .iter()
        .map(|(s, _)| {
            format!(
                "{} · {} · {} · {}",
                s.provider.as_str(),
                bounded_field(&s.account, 64),
                bounded_field(&s.session_id, 128),
                bounded_field(s.preview.as_deref().unwrap_or("(no preview)"), 512)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    if scan.truncated {
        output.push_str("[native chat scan bound reached; narrow with --account or --provider]\n");
    }
    Ok(output)
}

fn read_messages(path: &Path, provider: ProviderKind) -> Result<(Vec<String>, bool), String> {
    const CAP: u64 = 8 * 1024 * 1024;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    let start = size.saturating_sub(CAP);
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(CAP)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let begin = if start > 0 {
        bytes
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(bytes.len())
    } else {
        0
    };
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let truncated = start > 0;
    for line in bytes[begin..].split(|byte| *byte == b'\n') {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let payload = if provider == ProviderKind::Codex {
            value.get("payload").unwrap_or(&value)
        } else {
            value.get("message").unwrap_or(&value)
        };
        let role = payload.get("role").and_then(Value::as_str).unwrap_or(kind);
        if matches!(role, "user" | "assistant") {
            let content = payload.get("content");
            let text = content
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    content.and_then(Value::as_array).map(|blocks| {
                        blocks
                            .iter()
                            .filter_map(|b| b.get("text").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                });
            let text = text.map(|text| {
                if role == "user" {
                    visible_user_history(&text).to_owned()
                } else {
                    text
                }
            });
            if let Some(text) = text.filter(|s| !s.trim().is_empty()) {
                messages.push(format!("{role}: {}", terminal_safe(&text)));
            }
        } else if kind == "event_msg" {
            let typ = payload.get("type").and_then(Value::as_str).unwrap_or("");
            if matches!(typ, "user_message" | "agent_message") {
                if let Some(text) = payload.get("message").and_then(Value::as_str) {
                    events.push(format!(
                        "{}: {}",
                        if typ == "user_message" {
                            "user"
                        } else {
                            "assistant"
                        },
                        terminal_safe(text)
                    ));
                }
            }
        }
    }
    if messages.is_empty() {
        messages = events;
    }
    Ok((messages, truncated))
}

pub(crate) use crate::text_safe::terminal_safe;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_safe_strips_bidi_and_zero_width() {
        let spoofed = "invoice\u{202e}fdp.exe\u{200b}\u{2066}🙂";
        assert_eq!(terminal_safe(spoofed), "invoicefdp.exe🙂");
    }
    #[test]
    fn a_resumed_transcript_hides_the_time_header_on_user_prompts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.jsonl");
        let sent =
            "[Fri Oct 30 2026, 2:05pm local]\n\"yesterday\" = Thu Oct 29 2026\nWhat changed yesterday?";
        std::fs::write(
            &path,
            json!({"type":"user","message":{"role":"user","content":sent}}).to_string(),
        )
        .unwrap();
        let history = read_history_file(&path, ProviderKind::Claude).unwrap();
        assert_eq!(history.messages[0].text, "What changed yesterday?");
        assert_eq!(
            crate::session_titles::title_from_prompt(sent),
            "What changed yesterday?"
        );
    }

    #[test]
    fn structured_history_preserves_roles_unicode_and_excludes_setup_and_tool_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.jsonl");
        let rows = [
            json!({"type":"response_item","payload":{"role":"system","content":"hidden"}}),
            json!({"type":"response_item","payload":{"role":"developer","content":"hidden"}}),
            json!({"type":"response_item","payload":{"role":"user","content":"<environment_context>setup</environment_context>\nquestion 雪"}}),
            json!({"type":"response_item","payload":{"role":"assistant","content":[{"type":"output_text","text":"answer 🙂\u{1b}\u{7}"},{"type":"tool_result","text":"hidden"}]}}),
        ];
        std::fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let history = read_history_file(&path, ProviderKind::Codex).unwrap();
        assert_eq!(
            history.messages,
            vec![
                HistoryMessage {
                    role: "user".into(),
                    text: "question 雪".into(),
                    card: None,
                },
                HistoryMessage {
                    role: "assistant".into(),
                    text: "answer 🙂".into(),
                    card: None,
                },
            ]
        );
        assert!(!history.truncated);
        std::fs::write(
            &path,
            json!({"type":"assistant","message":{"content":[{"type":"text","text":"Claude 雪"}]}})
                .to_string(),
        )
        .unwrap();
        assert_eq!(
            read_history_file(&path, ProviderKind::Claude)
                .unwrap()
                .messages[0],
            HistoryMessage {
                role: "assistant".into(),
                text: "Claude 雪".into(),
                card: None,
            }
        );
        let rows = [
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":"inspect the project"}}),
            json!({"type":"event_msg","payload":{"type":"agent_reasoning","text":"Check the reported files first"}}),
            json!({"type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"Check the reported files first"}],"encrypted_content":"opaque-do-not-display"}}),
            json!({"type":"response_item","payload":{"type":"custom_tool_call","call_id":"native-call-one","name":"exec","input":"text(await tools.exec_command({cmd:\"git status\"}));"}}),
            json!({"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"native-call-one","output":"working tree clean"}}),
            json!({"type":"response_item","payload":{"type":"reasoning","encrypted_content":"opaque-only"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":"finished"}}),
        ];
        std::fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let history = read_history_file(&path, ProviderKind::Codex).unwrap();
        assert_eq!(
            history
                .messages
                .iter()
                .map(|message| message.role.as_str())
                .collect::<Vec<_>>(),
            vec!["user", "thinking", "card", "assistant"]
        );
        assert_eq!(history.messages[1].text, "Check the reported files first");
        let card = history.messages[2].card.as_ref().unwrap();
        assert_eq!(card.id, "native-call-one");
        assert_eq!(card.status, crate::provider::stream_cards::CardStatus::Done);
        assert!(card.detail.as_deref().unwrap().contains("git status"));
        assert!(card
            .detail
            .as_deref()
            .unwrap()
            .contains("working tree clean"));
        assert!(!format!("{history:?}").contains("opaque-"));

        let rows = [
            json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"Inspect actual input"},{"type":"text","text":"Opening file"},{"type":"tool_use","id":"claude-tool-one","name":"Read","input":{"file_path":"src/main.rs"}}]}}),
            json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"claude-tool-one","content":"fn main() {}"}]}}),
            json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Read complete"}]}}),
        ];
        std::fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let history = read_history_file(&path, ProviderKind::Claude).unwrap();
        assert_eq!(
            history
                .messages
                .iter()
                .map(|message| message.role.as_str())
                .collect::<Vec<_>>(),
            vec!["thinking", "assistant", "card", "assistant"]
        );
        let card = history.messages[2].card.as_ref().unwrap();
        assert_eq!(card.id, "claude-tool-one");
        assert!(card.detail.as_deref().unwrap().contains("fn main() {}"));
    }

    #[test]
    fn structured_history_returns_recent_messages_and_utf8_safe_text_tail() {
        let mut history = HistoryBuffer::new(false);
        for n in 0..HISTORY_MESSAGES + 3 {
            history.push("user", &n.to_string());
        }
        let history = history.finish();
        assert!(history.truncated);
        assert_eq!(history.messages.len(), HISTORY_MESSAGES);
        assert_eq!(history.messages[0].text, "3");
        let mut history = HistoryBuffer::new(false);
        history.push("user", "old");
        history.push("assistant", &"雪".repeat(HISTORY_TEXT_BYTES / 3 + 2));
        let history = history.finish();
        assert!(history.truncated);
        assert_eq!(history.messages.len(), 1);
        assert!(history.messages[0].text.len() <= HISTORY_TEXT_BYTES);
        assert!(history.messages[0].text.chars().all(|c| c == '雪'));
        let mut history = HistoryBuffer::new(false);
        history.push("user", "bounded request");
        history.push_card(StreamCard {
            id: "bounded-native-tool".into(),
            title: "read_file".into(),
            detail: Some("x".repeat(HISTORY_TEXT_BYTES + 1)),
            status: crate::provider::stream_cards::CardStatus::Done,
        });
        let history = history.finish();
        assert!(history.truncated);
        assert!(
            history
                .messages
                .iter()
                .any(|message| message.card.as_ref().is_some_and(|card| card
                    .detail
                    .as_deref()
                    .is_some_and(|detail| detail.contains("Saved-history detail truncated")))),
            "large tools retain an inspectable card instead of erasing the history"
        );
        assert!(
            history
                .messages
                .iter()
                .map(|message| message.text.len()
                    + message
                        .card
                        .as_ref()
                        .and_then(|card| card.detail.as_ref())
                        .map_or(0, String::len))
                .sum::<usize>()
                <= HISTORY_TEXT_BYTES
        );
    }

    #[test]
    fn native_byte_tail_discards_split_unicode_line_and_reports_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.jsonl");
        let mut bytes = "雪"
            .repeat(HISTORY_NATIVE_BYTES as usize / 3 + 1)
            .into_bytes();
        bytes.push(b'\n');
        bytes.extend_from_slice(
            json!({"type":"event_msg","payload":{"type":"agent_message","message":"recent 🙂"}})
                .to_string()
                .as_bytes(),
        );
        std::fs::write(&path, bytes).unwrap();
        let history = read_history_file(&path, ProviderKind::Codex).unwrap();
        assert!(history.truncated);
        assert_eq!(
            history.messages,
            vec![HistoryMessage {
                role: "assistant".into(),
                text: "recent 🙂".into(),
                card: None,
            }]
        );
    }

    #[test]
    fn structured_cursor_history_requires_an_explicit_registered_account() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            read_history(dir.path(), "account", ProviderKind::Cursor, "session")
                .unwrap_err()
                .contains("account")
        );
    }
    #[test]
    fn read_chat_text_handles_both_formats_and_never_emits_terminal_escapes() {
        for tag in [
            "task-notification",
            "system-reminder",
            "local-command-caveat",
        ] {
            assert_eq!(visible_user_history(&format!("<{tag}>notice</{tag}>")), "");
            assert_eq!(
                visible_user_history(&format!("<{tag}>notice</{tag}>\nreal question")),
                "real question"
            );
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.jsonl");
        std::fs::write(&path,concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"duplicate fallback\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"role\":\"user\",\"content\":[{\"text\":\"question 雪\"}]}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"role\":\"assistant\",\"content\":[{\"text\":\"answer \\u001b]0;bad\\u0007\"}]}}\n"
        )).unwrap();
        let (messages, truncated) = read_messages(&path, ProviderKind::Codex).unwrap();
        assert_eq!(messages.len(), 2);
        assert!(messages[0].contains("雪"));
        assert!(!messages[1].contains('\x1b'));
        assert!(!truncated);
        std::fs::write(&path,"{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Claude answer\"}]}}\n").unwrap();
        assert_eq!(
            read_messages(&path, ProviderKind::Claude).unwrap().0,
            ["assistant: Claude answer"]
        );
    }
    #[test]
    fn command_arguments_reject_ambiguous_or_inapplicable_inputs() {
        let dir = tempfile::tempdir().unwrap();
        for args in [
            vec!["read", "session"],
            vec!["list", "--tail", "2"],
            vec!["list", "--limit", "0"],
            vec!["list", "--json", "--json"],
            vec!["search"],
            vec!["list", "--account"],
        ] {
            assert!(
                run(
                    dir.path(),
                    &args.iter().map(|s| s.to_string()).collect::<Vec<_>>()
                )
                .is_err(),
                "{args:?}"
            );
        }
    }

    #[test]
    fn read_resolves_exact_session_in_requested_account_beyond_global_recent_window() {
        let state = tempfile::tempdir().unwrap();
        let home = state.path().join("synthetic-codex-home");
        let sessions_dir = home.join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        crate::store::upsert_stored_account(
            state.path(),
            crate::store::StoredAccount {
                id: "audit-account".into(),
                provider: ProviderKind::Codex,
                home_path: Some(home.to_string_lossy().into_owned()),
                label: "Synthetic account".into(),
                created_at: "synthetic".into(),
            },
        )
        .unwrap();
        let target = serde_json::json!({
            "type":"response_item",
            "payload":{"role":"assistant","content":[{"text":"old target answer"}]}
        });
        let target_id = "11111111-2222-4333-8444-555555555555";
        let meta = serde_json::json!({
            "type":"session_meta","payload":{"id":target_id,"cwd":"synthetic"}
        });
        std::fs::write(
            sessions_dir.join(format!("{target_id}.jsonl")),
            format!("{}\n{}\n", meta, target),
        )
        .unwrap();
        for n in 0..1001 {
            let meta = serde_json::json!({
                "type":"session_meta","payload":{"id":format!("new-{n}"),"cwd":"synthetic"}
            });
            std::fs::write(
                sessions_dir.join(format!("new-{n}.jsonl")),
                format!("{meta}\n"),
            )
            .unwrap();
        }
        let args = ["read", target_id, "--account", "audit-account"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let read = run(state.path(), &args).unwrap();
        assert!(read.contains("old target answer"), "{read}");
        assert!(!read.contains("new-"), "{read}");
        let history = read_history(
            state.path(),
            "audit-account",
            ProviderKind::Codex,
            target_id,
        )
        .unwrap();
        assert_eq!(
            history.messages,
            vec![HistoryMessage {
                role: "assistant".into(),
                text: "old target answer".into(),
                card: None,
            }]
        );
        assert!(read_history(
            state.path(),
            "other-account",
            ProviderKind::Codex,
            target_id
        )
        .is_err());
        assert!(read_history(
            state.path(),
            "audit-account",
            ProviderKind::Claude,
            target_id
        )
        .is_err());
        assert!(read_history(
            state.path(),
            "audit-account",
            ProviderKind::Codex,
            "missing-id"
        )
        .is_err());
    }
}
