//! Streaming JSON events from Claude Code (`--output-format stream-json --verbose`) and Cursor
//! (`--output-format stream-json`) -> text, thinking and tool cards.
//!
//! Both formats require JSON objects. Unknown structured events are ignored; invalid protocol
//! output is reported as an error and is never promoted into an assistant reply.

use super::stream_cards::{CardStatus, StreamCard, StreamPart, TurnUsage};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const TOOL_MAP_MAX: usize = 4096;

/// Compare Cursor's repeated whole messages without keeping a second copy of the stream.
#[derive(Default)]
struct SegmentFingerprint {
    hash: Sha256,
    trimmed: Sha256,
    started: bool,
}

impl SegmentFingerprint {
    fn push(&mut self, text: &str) {
        let text = if self.started {
            text
        } else {
            text.trim_start()
        };
        if text.is_empty() {
            return;
        }
        self.started = true;
        let trimmed = text.trim_end();
        if !trimmed.is_empty() {
            self.trimmed = self.hash.clone();
            self.trimmed.update(trimmed.as_bytes());
        }
        self.hash.update(text.as_bytes());
    }

    fn matches(&self, text: &str) -> bool {
        self.trimmed.clone().finalize() == Sha256::digest(text.trim().as_bytes())
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Claude,
    Cursor,
}

/// Stateful because a tool result names only the call id; the title and detail come from the call.
pub struct StreamParser {
    flavor: Flavor,
    tools: HashMap<String, (String, Option<String>)>,
    partial_tools: HashMap<u64, (String, String, bool)>,
    dropped_tools: usize,
    dropped_partial_tools: usize,
    saw_text: bool,
    /// Cursor is mid-way through a run of stamped text (see `parse_line`).
    streaming_text: bool,
    /// The last run of stamped text, to recognise Cursor's unstamped repeat of it.
    segment: SegmentFingerprint,
    /// Tool paths inside this folder are shown relative to it.
    cwd: Option<PathBuf>,
    invalid_output: bool,
}

fn remember_tool<K, V>(map: &mut HashMap<K, V>, key: K, value: V) -> bool
where
    K: std::hash::Hash + Eq,
{
    if map.contains_key(&key) || map.len() < TOOL_MAP_MAX {
        map.insert(key, value);
        true
    } else {
        false
    }
}

impl StreamParser {
    pub fn new(flavor: Flavor) -> Self {
        Self {
            flavor,
            tools: HashMap::new(),
            partial_tools: HashMap::new(),
            dropped_tools: 0,
            dropped_partial_tools: 0,
            saw_text: false,
            streaming_text: false,
            segment: SegmentFingerprint::default(),
            cwd: None,
            invalid_output: false,
        }
    }

    pub fn with_cwd(mut self, cwd: &Path) -> Self {
        self.cwd = Some(cwd.to_path_buf());
        self
    }

    pub fn parse_line(&mut self, line: &str) -> Vec<StreamPart> {
        let t = line.trim();
        if t.is_empty() {
            return Vec::new();
        }
        let Ok(ev) = serde_json::from_str::<Value>(t) else {
            self.invalid_output = true;
            return vec![StreamPart::Error("Provider returned invalid structured JSON output; plain-text downgrade is disabled.".into())];
        };
        if !ev.is_object() {
            self.invalid_output = true;
            return vec![StreamPart::Error(
                "Provider returned a non-object JSON event; current structured output is required."
                    .into(),
            )];
        }
        let typ = ev.get("type").and_then(Value::as_str).unwrap_or("");
        let mut out = Vec::new();
        if typ != "assistant" {
            self.end_segment(&mut out);
        }
        if let Some(id) = ev.get("session_id").and_then(Value::as_str) {
            if !id.is_empty() && matches!(typ, "system" | "result") {
                out.push(StreamPart::Session(id.to_string()));
            }
        }
        match typ {
            // Cursor stamps text with `timestamp_ms` as it is written: fragments with
            // `--stream-partial-output`, whole messages without it. A run of stamped text is one
            // segment; the whole copy that may follow repeats it and is dropped if so. Before a
            // tool call that copy is stamped too, so it is recognised by matching the segment.
            "assistant" if self.flavor == Flavor::Cursor && ev.get("timestamp_ms").is_some() => {
                let whole = Self::block_texts(&ev).concat();
                if self.streaming_text && whole.len() > 1 && self.segment.matches(&whole) {
                    return out;
                }
                for t in Self::block_texts(&ev) {
                    if !self.streaming_text {
                        self.streaming_text = true;
                        self.segment.clear();
                    }
                    self.saw_text = true;
                    self.segment.push(&t);
                    out.push(StreamPart::TextDelta(t));
                }
            }
            "assistant" if self.flavor == Flavor::Cursor => {
                self.end_segment(&mut out);
                let repeat = Self::block_texts(&ev).concat();
                if !self.segment.matches(&repeat) {
                    self.content_blocks(&ev, &mut out);
                }
                self.segment.clear();
            }
            "assistant" => self.content_blocks(&ev, &mut out),
            "user" => self.tool_results(&ev, &mut out),
            "stream_event" if self.flavor == Flavor::Claude => {
                self.claude_stream_event(&ev, &mut out)
            }
            "thinking" => match ev.get("subtype").and_then(Value::as_str) {
                Some("delta") => {
                    if let Some(t) = ev.get("text").and_then(Value::as_str) {
                        out.push(StreamPart::ThinkingDelta(t.to_string()));
                    }
                }
                Some("completed") => out.push(StreamPart::ThinkingDelta("\n".into())),
                _ => {
                    if let Some(t) = ev.get("text").and_then(Value::as_str) {
                        out.push(StreamPart::Thinking(t.to_string()));
                    }
                }
            },
            "tool_call" if self.flavor == Flavor::Cursor => self.cursor_tool_call(&ev, &mut out),
            "result" => self.result(&ev, &mut out),
            "error" => out.extend(super::stream_cards::classify_codex_event(&ev)),
            _ => {}
        }
        out
    }

    pub fn invalid_output(&self) -> bool {
        self.invalid_output
    }

    fn claude_stream_event(&mut self, ev: &Value, out: &mut Vec<StreamPart>) {
        let Some(event) = ev.get("event") else { return };
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "content_block_start" => {
                let Some(block) = event.get("content_block") else {
                    return;
                };
                if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                    return;
                }
                let Some(index) = event.get("index").and_then(Value::as_u64) else {
                    return;
                };
                let Some(id) = block.get("id").and_then(Value::as_str) else {
                    return;
                };
                let Some(name) = block.get("name").and_then(Value::as_str) else {
                    return;
                };
                let (id, name) = (id.to_string(), name.to_string());
                let detail = block
                    .get("input")
                    .filter(|v| !v.is_null() && *v != &Value::Object(Default::default()))
                    .and_then(|i| self.input_detail(i));
                if !remember_tool(
                    &mut self.partial_tools,
                    index,
                    (id.clone(), String::new(), false),
                ) {
                    self.dropped_partial_tools += 1;
                }
                if !remember_tool(&mut self.tools, id.clone(), (name.clone(), detail.clone())) {
                    self.dropped_tools += 1;
                }
                out.push(StreamPart::Card(StreamCard {
                    id,
                    title: name,
                    detail,
                    status: CardStatus::Running,
                }));
            }
            "content_block_delta" => {
                if event.pointer("/delta/type").and_then(Value::as_str) != Some("input_json_delta")
                {
                    return;
                }
                let Some(index) = event.get("index").and_then(Value::as_u64) else {
                    return;
                };
                let Some((id, partial, truncated)) = self.partial_tools.get_mut(&index) else {
                    return;
                };
                if let Some(chunk) = event.pointer("/delta/partial_json").and_then(Value::as_str) {
                    let remaining = (256usize * 1024).saturating_sub(partial.len());
                    if remaining == 0 && chunk.is_empty() {
                        return;
                    }
                    let mut end = chunk.len().min(remaining);
                    while !chunk.is_char_boundary(end) {
                        end -= 1;
                    }
                    partial.push_str(&chunk[..end]);
                    *truncated |= end < chunk.len();
                    let title = self
                        .tools
                        .get(id)
                        .map(|(title, _)| title.clone())
                        .unwrap_or_else(|| "tool".into());
                    let detail = Some(format!(
                        "Partial input: {partial}{}",
                        if *truncated {
                            "\n[input limited to 256 KiB]"
                        } else {
                            ""
                        }
                    ));
                    if !remember_tool(&mut self.tools, id.clone(), (title.clone(), detail.clone()))
                    {
                        self.dropped_tools += 1;
                    }
                    out.push(StreamPart::Card(StreamCard {
                        id: id.clone(),
                        title,
                        detail,
                        status: CardStatus::Running,
                    }));
                }
            }
            _ => {}
        }
    }

    /// Close a run of stamped Cursor text so the next text starts on its own line.
    fn end_segment(&mut self, out: &mut Vec<StreamPart>) {
        if std::mem::take(&mut self.streaming_text) {
            out.push(StreamPart::TextDelta("\n".into()));
        }
    }

    /// The text blocks of an `assistant` event, whitespace included (a delta may be just a space).
    fn block_texts(ev: &Value) -> Vec<String> {
        ev.pointer("/message/content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn content_blocks(&mut self, ev: &Value, out: &mut Vec<StreamPart>) {
        let Some(blocks) = ev.pointer("/message/content").and_then(Value::as_array) else {
            return;
        };
        for b in blocks {
            match b.get("type").and_then(Value::as_str).unwrap_or("") {
                "text" => {
                    if let Some(t) = b.get("text").and_then(Value::as_str) {
                        if !t.trim().is_empty() {
                            self.saw_text = true;
                            out.push(StreamPart::Text(t.to_string()));
                        }
                    }
                }
                "thinking" => {
                    if let Some(t) = b.get("thinking").and_then(Value::as_str) {
                        if !t.trim().is_empty() {
                            out.push(StreamPart::Thinking(t.to_string()));
                        }
                    }
                }
                "tool_use" => {
                    let id = b.get("id").and_then(Value::as_str).unwrap_or("tool");
                    let name = b.get("name").and_then(Value::as_str).unwrap_or("tool");
                    let detail = b.get("input").and_then(|i| self.input_detail(i));
                    if !remember_tool(
                        &mut self.tools,
                        id.to_string(),
                        (name.to_string(), detail.clone()),
                    ) {
                        self.dropped_tools += 1;
                    }
                    out.push(StreamPart::Card(StreamCard {
                        id: id.to_string(),
                        title: name.to_string(),
                        detail,
                        status: CardStatus::Running,
                    }));
                }
                _ => {}
            }
        }
    }

    fn tool_results(&mut self, ev: &Value, out: &mut Vec<StreamPart>) {
        let Some(blocks) = ev.pointer("/message/content").and_then(Value::as_array) else {
            return;
        };
        for b in blocks {
            if b.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let Some(id) = b.get("tool_use_id").and_then(Value::as_str) else {
                continue;
            };
            let (title, detail) = self
                .tools
                .get(id)
                .cloned()
                .unwrap_or_else(|| ("tool".into(), None));
            let output = b
                .get("content")
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.to_string())
                })
                .unwrap_or_default();
            let detail = if output.is_empty() {
                detail
            } else {
                Some(format!("{}\n{}", detail.unwrap_or_default(), output))
            };
            let failed = b.get("is_error").and_then(Value::as_bool) == Some(true);
            out.push(StreamPart::Card(StreamCard {
                id: id.to_string(),
                title,
                detail,
                status: if failed {
                    CardStatus::Error
                } else {
                    CardStatus::Done
                },
            }));
        }
    }

    /// Cursor: `{"type":"tool_call","subtype":"started|completed","call_id":..,
    /// "tool_call":{"readToolCall":{"args":{"path":..}}}}`.
    fn cursor_tool_call(&mut self, ev: &Value, out: &mut Vec<StreamPart>) {
        let id = ev
            .get("call_id")
            .and_then(Value::as_str)
            .unwrap_or("tool")
            .to_string();
        let Some(call) = ev.get("tool_call").and_then(Value::as_object) else {
            return;
        };
        // The call sits beside bookkeeping fields (`startedAtMs`, `toolCallId`, ...).
        let Some((kind, body)) = call.iter().find(|(k, _)| k.ends_with("ToolCall")) else {
            return;
        };
        let title = kind
            .strip_suffix("ToolCall")
            .unwrap_or(kind)
            .to_ascii_lowercase();
        let title = if title.is_empty() {
            "tool".into()
        } else {
            title
        };
        let mut detail = body
            .get("args")
            .and_then(tool_input_detail)
            .map(|d| self.relative(&d))
            .or_else(|| self.tools.get(&id).and_then(|(_, d)| d.clone()));
        let done = ev.get("subtype").and_then(Value::as_str) == Some("completed");
        if done {
            self.tools.remove(&id);
        } else if !remember_tool(&mut self.tools, id.clone(), (title.clone(), detail.clone())) {
            self.dropped_tools += 1;
        }
        if done {
            if let Some(summary) = self.cursor_result_summary(kind, body) {
                detail = Some(summary);
            } else if let Some(result) = body.get("result") {
                let output = result
                    .pointer("/success/output")
                    .and_then(Value::as_str)
                    .or_else(|| result.get("output").and_then(Value::as_str))
                    .map(str::to_string)
                    .unwrap_or_else(|| result.to_string());
                if !output.is_empty() && output != "{}" && output != "{\"success\":{}}" {
                    detail = Some(format!("{}\n{output}", detail.unwrap_or_default()));
                }
            }
        }
        let failed = done
            && [
                body.pointer("/result/error"),
                body.pointer("/result/failure"),
            ]
            .into_iter()
            .flatten()
            .any(|value| !value.is_null());
        out.push(StreamPart::Card(StreamCard {
            id,
            title,
            detail,
            status: if failed {
                CardStatus::Error
            } else if done {
                CardStatus::Done
            } else {
                CardStatus::Running
            },
        }));
    }

    /// What a finished edit, read or glob shows instead of its raw result: the diff, the line
    /// count, or the matched files. `None` keeps the generic output for other tools.
    fn cursor_result_summary(&self, kind: &str, body: &Value) -> Option<String> {
        let success = body.pointer("/result/success")?;
        let arg = |key: &str| {
            body.pointer(&format!("/args/{key}"))
                .and_then(Value::as_str)
        };
        match kind {
            "editToolCall" => success
                .get("diffString")
                .and_then(Value::as_str)
                .map(|diff| self.relative(diff)),
            "readToolCall" => {
                let lines = success.get("totalLines").and_then(Value::as_u64)?;
                Some(format!("{} · {lines} lines", self.relative(arg("path")?)))
            }
            "globToolCall" => {
                let files: Vec<String> = success
                    .get("files")
                    .and_then(Value::as_array)?
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|f| self.relative(f))
                    .collect();
                Some(format!("{}\n{}", arg("globPattern")?, files.join("\n")))
            }
            _ => None,
        }
    }

    /// `text` with this session's folder removed from the front of any path inside it.
    /// A tool's input as card detail, with file paths inside the session folder made relative.
    fn input_detail(&self, input: &Value) -> Option<String> {
        let mut input = input.clone();
        if let Some(map) = input.as_object_mut() {
            for key in ["file_path", "path", "notebook_path"] {
                if let Some(Value::String(path)) = map.get_mut(key) {
                    *path = self.relative(path);
                }
            }
        }
        tool_input_detail(&input)
    }

    fn relative(&self, text: &str) -> String {
        let Some(cwd) = self.cwd.as_ref().and_then(|c| c.to_str()) else {
            return text.to_string();
        };
        let cwd = cwd.trim_end_matches(['\\', '/']);
        if cwd.is_empty() {
            return text.to_string();
        }
        text.replace(&format!("{cwd}\\"), "")
            .replace(&format!("{}/", cwd.replace('\\', "/")), "")
    }

    fn result(&mut self, ev: &Value, out: &mut Vec<StreamPart>) {
        let mut usage = TurnUsage::from_json(ev.get("usage"), "cache_read_input_tokens");
        if self.flavor == Flavor::Claude {
            if let (Some(input), Some(written)) = (
                usage.input,
                ev.pointer("/usage/cache_creation_input_tokens")
                    .and_then(Value::as_u64),
            ) {
                usage.input = Some(input.saturating_add(written));
            }
        }
        if self.flavor == Flavor::Cursor && usage.is_empty() {
            // Cursor names its counts in camelCase (`inputTokens`, `cacheReadTokens`).
            let num = |k: &str| ev.pointer(&format!("/usage/{k}")).and_then(Value::as_u64);
            usage.input = num("inputTokens");
            usage.output = num("outputTokens");
            usage.cached = num("cacheReadTokens");
        }
        usage.cost_usd = ev.get("total_cost_usd").and_then(Value::as_f64);
        if !usage.is_empty() {
            out.push(StreamPart::Usage(usage));
        }
        let text = ev
            .get("result")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let is_error = ev.get("is_error").and_then(Value::as_bool) == Some(true)
            || ev
                .get("subtype")
                .and_then(Value::as_str)
                .is_some_and(|s| s.starts_with("error"));
        if text.is_empty() && is_error {
            if let Some(errors) = ev.get("errors").and_then(Value::as_array) {
                let text = errors
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    out.push(StreamPart::Error(text));
                }
            }
        }
        if text.is_empty() {
            return;
        }
        if is_error {
            out.push(StreamPart::Error(super::stream_cards::capacity_error_text(
                text,
                ev.pointer("/error/code")
                    .or_else(|| ev.get("error_code"))
                    .and_then(Value::as_str),
            )));
        } else if !self.saw_text {
            // The final result repeats the assistant text; use it only if no text streamed.
            out.push(StreamPart::Text(text.to_string()));
        }
    }
}

/// A short, readable summary of a tool's input: the command, path, pattern or query.
fn tool_input_detail(input: &Value) -> Option<String> {
    (!input.is_null()).then(|| format_provider_value(input))
}

fn format_provider_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(flavor: Flavor, lines: &[&str]) -> Vec<StreamPart> {
        let mut p = StreamParser::new(flavor);
        lines.iter().flat_map(|l| p.parse_line(l)).collect()
    }

    #[test]
    fn claude_events_become_session_thinking_cards_and_text() {
        let parts = parse(
            Flavor::Claude,
            &[
                r#"{"type":"system","subtype":"init","session_id":"s-1"}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"need to look at the tests"}]}}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]}}"#,
                r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"All green."}]}}"#,
                r#"{"type":"result","subtype":"success","is_error":false,"result":"All green.","session_id":"s-1"}"#,
            ],
        );
        assert!(matches!(&parts[0], StreamPart::Session(s) if s == "s-1"));
        assert!(matches!(&parts[1], StreamPart::Thinking(t) if t.contains("tests")));
        let StreamPart::Card(running) = &parts[2] else {
            panic!("{parts:?}")
        };
        assert_eq!(running.title, "Bash");
        assert!(running.detail.as_deref().unwrap().contains("cargo test"));
        assert_eq!(running.status, CardStatus::Running);
        let StreamPart::Card(done) = &parts[3] else {
            panic!("{parts:?}")
        };
        assert_eq!((done.id.as_str(), done.status), ("t1", CardStatus::Done));
        assert_eq!(done.title, "Bash", "the result keeps the call's title");
        assert!(matches!(&parts[4], StreamPart::Text(t) if t == "All green."));
        // The result repeats the text: it must not be shown twice.
        assert_eq!(
            parts
                .iter()
                .filter(|p| matches!(p, StreamPart::Text(_)))
                .count(),
            1
        );
    }

    #[test]
    fn claude_result_is_used_when_no_text_streamed_and_errors_are_errors() {
        let parts = parse(
            Flavor::Claude,
            &[r#"{"type":"result","subtype":"success","is_error":false,"result":"done"}"#],
        );
        assert!(matches!(&parts[0], StreamPart::Text(t) if t == "done"));
        let parts = parse(
            Flavor::Claude,
            &[
                r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"Credit balance is too low"}"#,
            ],
        );
        assert!(matches!(&parts[0], StreamPart::Error(e) if e.contains("Credit")));
    }

    #[test]
    fn a_failed_tool_result_is_an_error_card() {
        let parts = parse(
            Flavor::Claude,
            &[
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"a","name":"Read","input":{"file_path":"x.rs"}}]}}"#,
                r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"a","is_error":true,"content":"no"}]}}"#,
            ],
        );
        let StreamPart::Card(c) = &parts[1] else {
            panic!()
        };
        assert_eq!(c.status, CardStatus::Error);
        assert!(c.detail.as_deref().unwrap().contains("x.rs"));
        assert!(c.detail.as_deref().unwrap().ends_with("\nno"));
    }

    #[test]
    fn cursor_tool_calls_track_started_and_completed() {
        let parts = parse(
            Flavor::Cursor,
            &[
                r#"{"type":"system","subtype":"init","session_id":"c-9"}"#,
                r#"{"type":"tool_call","subtype":"started","call_id":"k1","tool_call":{"readToolCall":{"args":{"path":"src/a.rs"}}}}"#,
                r#"{"type":"tool_call","subtype":"completed","call_id":"k1","tool_call":{"readToolCall":{"args":{"path":"src/a.rs"},"result":{"success":{}}}}}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Read it."}]}}"#,
            ],
        );
        assert!(matches!(&parts[0], StreamPart::Session(s) if s == "c-9"));
        let (StreamPart::Card(a), StreamPart::Card(b)) = (&parts[1], &parts[2]) else {
            panic!("{parts:?}")
        };
        assert_eq!((a.title.as_str(), a.status), ("read", CardStatus::Running));
        assert_eq!(b.status, CardStatus::Done);
        assert!(b.detail.as_deref().unwrap().contains("src/a.rs"));
        assert!(matches!(&parts[3], StreamPart::Text(t) if t == "Read it."));
    }

    #[test]
    fn invalid_protocol_is_an_error_and_unknown_structured_events_are_ignored() {
        let parts = parse(
            Flavor::Cursor,
            &[
                "just a notice",
                r#"{"type":"something_new","x":1}"#,
                "",
                "[1,2]",
            ],
        );
        assert!(matches!(&parts[0], StreamPart::Error(_)));
        assert!(matches!(&parts[1], StreamPart::Error(_)));
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn claude_streamed_tool_json_deltas_accumulate_under_one_call_id() {
        let parts = parse(
            Flavor::Claude,
            &[
                r#"{"type":"stream_event","event":{"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"call-雪","name":"Bash","input":{}}}}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"command\":\"echo "}}}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"雪\"}"}}}"#,
            ],
        );
        let cards = parts
            .iter()
            .filter_map(|p| match p {
                StreamPart::Card(c) => Some(c),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(cards.len(), 3);
        assert!(
            cards
                .iter()
                .all(|c| c.id == "call-雪" && c.status == CardStatus::Running)
        );
        assert!(
            cards
                .last()
                .unwrap()
                .detail
                .as_deref()
                .unwrap()
                .contains("echo 雪")
        );
    }

    #[test]
    fn claude_keeps_structured_mcp_arguments_results_and_errors() {
        let parts = parse(
            Flavor::Claude,
            &[
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"mcp-1","name":"mcp__wiki__search","input":{"query":"雪","limit":2}}]}}"#,
                r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"mcp-1","is_error":true,"content":[{"type":"text","text":"offline"},{"type":"text","text":"retry later"}]}]}}"#,
            ],
        );
        let StreamPart::Card(call) = &parts[0] else {
            panic!("{parts:?}")
        };
        assert_eq!(call.status, CardStatus::Running);
        assert!(call.detail.as_deref().unwrap().contains("雪"));
        let StreamPart::Card(result) = &parts[1] else {
            panic!("{parts:?}")
        };
        assert_eq!(result.id, "mcp-1");
        assert_eq!(result.status, CardStatus::Error);
        assert!(result.detail.as_deref().unwrap().contains("offline"));
        assert!(result.detail.as_deref().unwrap().contains("retry later"));
    }

    #[test]
    fn cursor_keeps_full_edit_args_and_structured_result() {
        let parts = parse(
            Flavor::Cursor,
            &[
                r#"{"type":"tool_call","subtype":"started","call_id":"edit-1","tool_call":{"editToolCall":{"args":{"path":"src/雪.rs","old_string":"old","new_string":"new","patch":"@@ -1 +1 @@\n-old\n+new"}}}}"#,
                r#"{"type":"tool_call","subtype":"completed","call_id":"edit-1","tool_call":{"editToolCall":{"args":{"path":"src/雪.rs","old_string":"old","new_string":"new","patch":"@@ -1 +1 @@\n-old\n+new"},"result":{"success":{"output":{"lines_changed":1}}}}}}"#,
            ],
        );
        let (StreamPart::Card(start), StreamPart::Card(done)) = (&parts[0], &parts[1]) else {
            panic!("{parts:?}")
        };
        assert_eq!(start.status, CardStatus::Running);
        assert_eq!(done.status, CardStatus::Done);
        let detail = done.detail.as_deref().unwrap();
        for expected in ["src/雪.rs", "old", "new", "@@ -1 +1 @@", "lines_changed"] {
            assert!(detail.contains(expected), "missing {expected}: {detail}");
        }
    }

    #[test]
    fn malformed_claude_partial_events_and_null_cursor_errors_are_safe() {
        let parts = parse(
            Flavor::Claude,
            &[
                r#"{"type":"stream_event","event":{"type":"content_block_start","content_block":{"type":"tool_use","id":"missing-index","name":"Bash","input":{}}}}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"ignored"}}}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","name":"Bash","input":{}}}}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"no-name","input":{}}}}"#,
            ],
        );
        assert!(
            parts.is_empty(),
            "invalid metadata must not merge into a guessed call"
        );

        let parts = parse(
            Flavor::Cursor,
            &[
                r#"{"type":"tool_call","subtype":"completed","call_id":"null-error","tool_call":{"runToolCall":{"args":{"command":"true"},"result":{"error":null,"failure":null,"success":{"output":"ok"}}}}}"#,
            ],
        );
        let StreamPart::Card(card) = &parts[0] else {
            panic!("{parts:?}")
        };
        assert_eq!(card.status, CardStatus::Done);
        assert!(card.detail.as_deref().unwrap().contains("ok"));
    }

    #[test]
    fn claude_partial_input_has_a_utf8_safe_visible_limit() {
        let mut parser = StreamParser::new(Flavor::Claude);
        let start = serde_json::json!({"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"bounded","name":"Bash","input":{}}}}).to_string();
        assert!(parser.parse_line(&start).len() == 1);
        let exact = serde_json::json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"a".repeat(256 * 1024)}}}).to_string();
        let _ = parser.parse_line(&exact);
        let overflow = serde_json::json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"雪"}}}).to_string();
        let parts = parser.parse_line(&overflow);
        let StreamPart::Card(card) = &parts[0] else {
            panic!("{parts:?}")
        };
        let detail = card.detail.as_deref().unwrap();
        assert!(detail.ends_with("[input limited to 256 KiB]"));
        assert!(detail.len() <= 256 * 1024 + 64);
    }

    #[test]
    fn tool_maps_are_capped() {
        let mut parser = StreamParser::new(Flavor::Claude);
        for n in 0..5000 {
            let line = serde_json::json!({
                "type": "assistant",
                "message": {"content": [{"type": "tool_use", "id": format!("tool-{n}"), "name": "Read", "input": {}}]}
            });
            parser.parse_line(&line.to_string());
        }
        assert!(parser.tools.len() <= TOOL_MAP_MAX);
        let parts = parser.parse_line(
            &serde_json::json!({
                "type": "assistant",
                "message": {"content": [{"type": "tool_use", "id": "new-tool", "name": "Read", "input": {}}]}
            })
            .to_string(),
        );
        assert!(matches!(parts.first(), Some(StreamPart::Card(card)) if card.id == "new-tool"));
    }
}

#[cfg(test)]
mod usage_tests {
    use super::*;

    fn usage_of(parts: &[StreamPart]) -> Vec<TurnUsage> {
        parts
            .iter()
            .filter_map(|p| match p {
                StreamPart::Usage(u) => Some(u.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn claude_result_reports_usage_and_cost_without_becoming_text() {
        let mut p = StreamParser::new(Flavor::Claude);
        let parts = p.parse_line(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"","total_cost_usd":0.0123,"usage":{"input_tokens":120,"output_tokens":45,"cache_read_input_tokens":900,"cache_creation_input_tokens":7}}"#,
        );
        assert_eq!(
            usage_of(&parts),
            vec![TurnUsage {
                input: Some(127),
                output: Some(45),
                cached: Some(900),
                cost_usd: Some(0.0123)
            }]
        );
        assert!(!parts.iter().any(|p| matches!(p, StreamPart::Text(_))));
    }

    #[test]
    fn missing_or_odd_usage_fields_are_tolerated() {
        let mut p = StreamParser::new(Flavor::Claude);
        let parts = p.parse_line(r#"{"type":"result","result":"ok","usage":{"output_tokens":3}}"#);
        assert_eq!(
            usage_of(&parts),
            vec![TurnUsage {
                output: Some(3),
                ..Default::default()
            }]
        );
        for line in [
            r#"{"type":"result","result":"ok"}"#,
            r#"{"type":"result","result":"ok","usage":"lots"}"#,
            r#"{"type":"result","result":"ok","usage":{"input_tokens":"x"}}"#,
        ] {
            assert!(usage_of(&p.parse_line(line)).is_empty(), "{line}");
        }
        let mut c = StreamParser::new(Flavor::Cursor);
        assert!(usage_of(&c.parse_line(r#"{"type":"result","result":"ok"}"#)).is_empty());
    }

    #[test]
    fn claude_tool_paths_inside_the_session_folder_are_shown_relative() {
        let mut p = StreamParser::new(Flavor::Claude).with_cwd(Path::new(r"C:\work\calc"));
        let parts = p.parse_line(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"C:\\work\\calc\\calc.py"}}]}}"#,
        );
        let StreamPart::Card(card) = &parts[0] else {
            panic!("{parts:?}");
        };
        let line = crate::provider::stream_cards::format_card_line(card);
        assert!(line.contains("calc.py") && !line.contains("work"), "{line}");
    }

    /// A recorded Cursor turn, parsed as a turn in `C:\work\calc` would be.
    fn cursor_fixture(text: &str) -> Vec<StreamPart> {
        let mut p = StreamParser::new(Flavor::Cursor).with_cwd(Path::new(r"C:\work\calc"));
        text.lines().flat_map(|l| p.parse_line(l)).collect()
    }

    /// The last state of each card, in the order the cards first appeared.
    fn final_cards(parts: &[StreamPart]) -> Vec<StreamCard> {
        let mut cards: Vec<StreamCard> = Vec::new();
        for part in parts {
            if let StreamPart::Card(card) = part {
                match cards.iter_mut().find(|c| c.id == card.id) {
                    Some(seen) => *seen = card.clone(),
                    None => cards.push(card.clone()),
                }
            }
        }
        cards
    }

    #[test]
    fn recorded_cursor_read_turn_names_tools_and_joins_thinking() {
        let parts = cursor_fixture(include_str!("fixtures/cursor-read-turn.jsonl"));
        let cards = final_cards(&parts);
        let titles: Vec<&str> = cards.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["glob", "getmcptools", "read"]);
        assert!(cards.iter().all(|c| c.status == CardStatus::Done));
        assert_eq!(cards[2].detail.as_deref(), Some("calc.py · 7 lines"));
        assert!(
            cards[0]
                .detail
                .as_deref()
                .is_some_and(|d| d.starts_with("**/calc.py\n"))
        );

        let first_card = parts
            .iter()
            .position(|p| matches!(p, StreamPart::Card(_)))
            .unwrap();
        assert!(parts[..first_card].iter().any(
            |p| matches!(p, StreamPart::TextDelta(t) if t.starts_with("I'll read `calc.py`"))
        ));
        let mut outcome = crate::provider::codex::StreamOutcome::default();
        crate::provider::codex::apply_stream_parts(parts.clone(), &mut outcome);
        let lines: Vec<&str> = outcome.text.lines().collect();
        assert_eq!(lines.len(), 2, "{:?}", outcome.text);
        assert!(lines[0].starts_with("I'll read `calc.py`"));
        assert!(lines[1].starts_with("`calc.py` defines"));

        let thinking: String = parts
            .iter()
            .filter_map(|p| match p {
                StreamPart::ThinkingDelta(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            thinking.lines().next(),
            Some("I need to explain what calc.py does in one sentence.")
        );
        // Cursor reports its counts in camelCase; they used to be dropped.
        let usage: Vec<_> = parts
            .iter()
            .filter_map(|p| match p {
                StreamPart::Usage(u) => Some(u.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].input, Some(12350));
        assert_eq!(usage[0].cached, Some(36864));
        assert_eq!(usage[0].output, Some(224));
    }

    #[test]
    fn a_stamped_whole_copy_before_a_tool_is_not_shown_twice() {
        // Shape seen live on 2026-10-30: streamed fragments, then the same text whole and
        // stamped, then a tool call.
        let parts = cursor_fixture(concat!(
            r#"{"type":"assistant","timestamp_ms":1,"message":{"content":[{"type":"text","text":"Checking the"}]}}"#,
            "\n",
            r#"{"type":"assistant","timestamp_ms":2,"message":{"content":[{"type":"text","text":" project."}]}}"#,
            "\n",
            r#"{"type":"assistant","timestamp_ms":3,"message":{"content":[{"type":"text","text":"Checking the project."}]}}"#,
            "\n",
            r#"{"type":"tool_call","subtype":"started","call_id":"g1","tool_call":{"globToolCall":{"args":{"globPattern":"*"}}}}"#,
        ));
        let mut outcome = crate::provider::codex::StreamOutcome::default();
        crate::provider::codex::apply_stream_parts(parts, &mut outcome);
        assert_eq!(outcome.text, "Checking the project.\n");
    }

    #[test]
    fn recorded_cursor_edit_turn_streams_the_reply_once_and_carries_the_diff() {
        let parts = cursor_fixture(include_str!("fixtures/cursor-edit-turn.jsonl"));
        let mut outcome = crate::provider::codex::StreamOutcome::default();
        crate::provider::codex::apply_stream_parts(parts.clone(), &mut outcome);
        assert_eq!(
            outcome
                .text
                .matches("`divide` now raises `ValueError`")
                .count(),
            1,
            "{:?}",
            outcome.text
        );
        assert!(
            parts
                .iter()
                .any(|p| matches!(p, StreamPart::TextDelta(t) if t == "divide"))
        );

        let cards = final_cards(&parts);
        let edit = cards.iter().find(|c| c.title == "edit").unwrap();
        let diff = edit.detail.as_deref().unwrap();
        assert!(diff.lines().any(|l| l == "+    if b == 0:"), "{diff}");
        assert!(diff.starts_with("--- a/calc.py\n"), "{diff}");
    }
}
