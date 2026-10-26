//! Streaming JSON events from Claude Code (`--output-format stream-json --verbose`) and Cursor
//! (`--output-format stream-json`) -> text, thinking and tool cards.
//!
//! Both formats are line-delimited JSON. The parser is tolerant on purpose: an unknown event is
//! ignored, and a line that is not JSON at all is passed through as plain text, so a CLI that
//! rejects the flag or prints a notice still shows something.

use super::stream_cards::{CardStatus, StreamCard, StreamPart, TurnUsage};
use serde_json::Value;
use std::collections::HashMap;

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
    saw_text: bool,
}

impl StreamParser {
    pub fn new(flavor: Flavor) -> Self {
        Self {
            flavor,
            tools: HashMap::new(),
            partial_tools: HashMap::new(),
            saw_text: false,
        }
    }

    pub fn parse_line(&mut self, line: &str) -> Vec<StreamPart> {
        let t = line.trim();
        if t.is_empty() {
            return Vec::new();
        }
        let Ok(ev) = serde_json::from_str::<Value>(t) else {
            return vec![StreamPart::Text(t.to_string())];
        };
        if !ev.is_object() {
            return vec![StreamPart::Text(t.to_string())];
        }
        let typ = ev.get("type").and_then(Value::as_str).unwrap_or("");
        let mut out = Vec::new();
        if let Some(id) = ev.get("session_id").and_then(Value::as_str) {
            if !id.is_empty() && matches!(typ, "system" | "result") {
                out.push(StreamPart::Session(id.to_string()));
            }
        }
        match typ {
            "assistant" => self.content_blocks(&ev, &mut out),
            "user" => self.tool_results(&ev, &mut out),
            "stream_event" if self.flavor == Flavor::Claude => {
                self.claude_stream_event(&ev, &mut out)
            }
            "thinking" => {
                if let Some(t) = ev.get("text").and_then(Value::as_str) {
                    out.push(StreamPart::Thinking(t.to_string()));
                }
            }
            "tool_call" if self.flavor == Flavor::Cursor => self.cursor_tool_call(&ev, &mut out),
            "result" => self.result(&ev, &mut out),
            _ => {}
        }
        out
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
                    .map(format_provider_value);
                self.partial_tools
                    .insert(index, (id.clone(), String::new(), false));
                self.tools
                    .insert(id.clone(), (name.clone(), detail.clone()));
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
                    self.tools
                        .insert(id.clone(), (title.clone(), detail.clone()));
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
                    let detail = b.get("input").and_then(tool_input_detail);
                    self.tools
                        .insert(id.to_string(), (name.to_string(), detail.clone()));
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
        let Some((kind, body)) = call.iter().next() else {
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
            .or_else(|| self.tools.get(&id).and_then(|(_, d)| d.clone()));
        self.tools
            .insert(id.clone(), (title.clone(), detail.clone()));
        let done = ev.get("subtype").and_then(Value::as_str) == Some("completed");
        if done {
            if let Some(result) = body.get("result") {
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

    fn result(&mut self, ev: &Value, out: &mut Vec<StreamPart>) {
        let mut usage = TurnUsage::from_json(ev.get("usage"), "cache_read_input_tokens");
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
        if text.is_empty() {
            return;
        }
        if is_error {
            out.push(StreamPart::Error(text.to_string()));
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
    fn plain_text_and_unknown_events_are_tolerated() {
        let parts = parse(
            Flavor::Cursor,
            &[
                "just a notice",
                r#"{"type":"something_new","x":1}"#,
                "",
                "[1,2]",
            ],
        );
        assert!(matches!(&parts[0], StreamPart::Text(t) if t == "just a notice"));
        assert!(matches!(&parts[1], StreamPart::Text(t) if t == "[1,2]"));
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
        assert!(cards
            .iter()
            .all(|c| c.id == "call-雪" && c.status == CardStatus::Running));
        assert!(cards
            .last()
            .unwrap()
            .detail
            .as_deref()
            .unwrap()
            .contains("echo 雪"));
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
                input: Some(120),
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
}
