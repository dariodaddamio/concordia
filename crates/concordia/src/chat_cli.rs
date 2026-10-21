//! Explicit, bounded access to native CLI chats. No session resume, context transfer or inference.
use crate::{sessions, store::ProviderKind};
use serde_json::{json, Value};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const HELP: &str = "usage: concordia chat list [--account ID] [--provider codex|claude|cursor] [--json] [--limit N]\n       concordia chat read SESSION --account ID [--provider codex|claude|cursor] [--tail N] [--grep TEXT] [--max-chars N]\n       concordia chat search TEXT [--account ID] [--provider codex|claude|cursor] [--json] [--limit N] (session IDs and previews/titles only)";
const MAX_CHAT_OUTPUT_BYTES: usize = 5 * 1024 * 1024;

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
        if session.provider == ProviderKind::Cursor {
            return Err("Cursor exposes chat metadata here; its native transcript format is not supported for reading".into());
        }
        let (messages, truncated) = read_messages(&path, session.provider)?;
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
            if let Some(text) = text.filter(|s| !s.is_empty()) {
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

fn terminal_safe(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(*c, '\n' | '\t'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_chat_text_handles_both_formats_and_never_emits_terminal_escapes() {
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
    }
}
