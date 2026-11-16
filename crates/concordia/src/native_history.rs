//! Explicit, bounded access to native CLI chats. No session resume, context transfer or inference.
use crate::provider::stream_cards::{
    classify_codex_event, format_card_line, upsert_card, StreamCard, StreamPart,
};
use crate::store::ProviderKind;
use crate::text_safe::terminal_safe;
use serde_json::Value;
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

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

pub(crate) const HISTORY_MESSAGES: usize = 200;
pub(crate) const HISTORY_TEXT_BYTES: usize = 1024 * 1024;
pub(crate) const HISTORY_NATIVE_BYTES: u64 = 8 * 1024 * 1024;

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
pub fn read_bound(
    root: &Path,
    account: &str,
    provider: ProviderKind,
    session_id: &str,
    path: &Path,
) -> Result<History, String> {
    let history = if provider == ProviderKind::Cursor {
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
        read_history_file(path, provider)?
    };
    Ok(history)
}

/// Confirm a bound native record can be opened without parsing its transcript. Listing
/// surfaces (/resume, /doctor) call this for every chat; the full read happens on open.
pub fn probe_bound(
    root: &Path,
    account: &str,
    provider: ProviderKind,
    session_id: &str,
    path: &Path,
) -> Result<(), String> {
    if provider == ProviderKind::Cursor {
        let home = crate::accounts::get_account(root, account)
            .and_then(|a| a.home_path)
            .ok_or("Cursor account home is unavailable")?;
        crate::native_memory::Binding::new(provider, account, &home, session_id)?;
        return Ok(());
    }
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("native record is not a file".into());
    }
    Ok(())
}

pub(crate) fn read_history_file(path: &Path, provider: ProviderKind) -> Result<History, String> {
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

pub(crate) struct HistoryBuffer {
    messages: std::collections::VecDeque<HistoryMessage>,
    bytes: usize,
    truncated: bool,
}

impl HistoryBuffer {
    pub(crate) fn new(truncated: bool) -> Self {
        Self {
            messages: std::collections::VecDeque::new(),
            bytes: 0,
            truncated,
        }
    }

    pub(crate) fn push(&mut self, role: &str, text: &str) {
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

    pub(crate) fn push_card(&mut self, mut card: StreamCard) {
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

    pub(crate) fn finish(self) -> History {
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
            "concordia_standing_context",
        ]
        .into_iter()
        .find(|tag| text.starts_with(&format!("<{tag}>")));
        let Some(tag) = wrapper else {
            // ACP transcripts keep Concordia's standing context as plain text before the request.
            const START: &str = "You run inside Concordia, which hosts native provider CLIs.";
            const END: &str = "Do not ask the user to do work Concordia can do.";
            if text.starts_with(START) {
                if let Some(index) = text.find(END) {
                    text = &text[index + END.len()..];
                    continue;
                }
            }
            break;
        };
        let end = format!("</{tag}>");
        let Some(index) = text.find(&end) else {
            return "";
        };
        text = &text[index + end.len()..];
    }
    if text.starts_with("# AGENTS.md instructions") {
        ""
    } else {
        crate::prompt_time::strip_header(text)
    }
}
