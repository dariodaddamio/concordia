#[cfg(test)]
use crate::provider::stream_cards::StreamCard;
use crate::{sessions, store::ProviderKind};
use serde_json::{json, Value};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const HELP: &str = "usage: concordia chat list [--account ID] [--provider codex|claude|cursor|grok] [--json] [--limit N]\n       concordia chat read SESSION --account ID [--provider codex|claude|cursor|grok] [--tail N] [--grep TEXT] [--max-chars N]\n       concordia chat search TEXT [--account ID] [--provider codex|claude|cursor|grok] [--json] [--limit N] (session IDs and previews/titles only)\n       concordia chat repair [--dry-run]\n       concordia chat thread list [--json] | map ID | read ID [--cursor TOKEN] [--tail N] [--grep TEXT] [--range A-B] [--max-chars N]";
const MAX_CHAT_OUTPUT_BYTES: usize = 5 * 1024 * 1024;

pub(crate) use crate::native_history::visible_user_history;
#[cfg(test)]
use crate::native_history::{
    read_history_file, HistoryBuffer, HISTORY_MESSAGES, HISTORY_NATIVE_BYTES, HISTORY_TEXT_BYTES,
};
pub use crate::native_history::{History, HistoryMessage};

pub fn read_history(
    root: &Path,
    account: &str,
    provider: ProviderKind,
    session_id: &str,
) -> Result<History, String> {
    let (session, path) = sessions::find_native_record(root, account, Some(provider), session_id)?
        .ok_or_else(|| "session not found in that registered account and provider".to_string())?;
    let mut history =
        crate::native_history::read_bound(root, account, provider, session_id, &path)?;
    history.title = session
        .preview
        .as_deref()
        .map(crate::text_safe::terminal_safe);
    Ok(history)
}

pub(crate) fn read_history_page(
    root: &Path,
    account: &str,
    provider: ProviderKind,
    session_id: &str,
    before: crate::native_history::Anchor,
) -> Result<History, String> {
    let (session, path) = sessions::find_native_record(root, account, Some(provider), session_id)?
        .ok_or_else(|| "session not found in that registered account and provider".to_string())?;
    let mut history = crate::native_history::read_bound_page(
        root,
        account,
        provider,
        session_id,
        &path,
        Some(before),
    )?;
    history.title = session
        .preview
        .as_deref()
        .map(crate::text_safe::terminal_safe);
    Ok(history)
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
    if args.first().is_some_and(|operation| operation == "thread") {
        return crate::threads::run_chat_cli(root, &args[1..]);
    }
    if args
        .first()
        .is_some_and(|s| matches!(s.as_str(), "recall" | "file-facts"))
    {
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
    if operation == "repair" {
        return repair_chats(root, &args[1..]);
    }
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

fn short_title(title: &str) -> String {
    title
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join(" ")
}

fn repair_chats(root: &Path, args: &[String]) -> Result<String, String> {
    let dry_run = match args {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        _ => return Err("chat repair accepts only --dry-run".into()),
    };
    let diagnostics = crate::sessions::unopenable_tasks(root);
    let tasks = crate::store::list_tasks(root);
    let mut actions = Vec::new();
    for diagnostic in diagnostics {
        let task = tasks.iter().find(|task| task.id == diagnostic.task_id);
        let action = match task.and_then(|task| task.sessions.last()) {
            Some(session) => match crate::sessions::find_native_record(
                root,
                &session.account,
                Some(session.provider),
                &session.native_session_id,
            ) {
                Ok(Some((summary, _))) if summary.user_messages == 0 => "removed",
                Ok(Some(_)) if diagnostic.handoff_map_id.is_some() => "repaired",
                Ok(Some(_)) => "still unreadable",
                Ok(None) => "still unreadable",
                Err(_) => "still unreadable",
            },
            None => "still unreadable",
        };
        actions.push((diagnostic, action));
    }
    let changes: Vec<_> = actions
        .iter()
        .filter(|(_, action)| *action != "still unreadable")
        .collect();
    let mut output = String::new();
    if !dry_run && !changes.is_empty() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        let backup = crate::store::store_dir(root).join(format!("chat-repair-{stamp}"));
        std::fs::create_dir_all(backup.join("tasks")).map_err(|e| e.to_string())?;
        for (diagnostic, _) in &changes {
            let task = tasks
                .iter()
                .find(|task| task.id == diagnostic.task_id)
                .unwrap();
            let bytes = serde_json::to_vec_pretty(task).map_err(|e| e.to_string())?;
            std::fs::write(
                backup.join("tasks").join(format!("{}.json", task.id)),
                bytes,
            )
            .map_err(|e| e.to_string())?;
        }
        output.push_str(&format!("backup: {}\n", backup.display()));
        for (diagnostic, action) in &changes {
            if *action != "repaired" {
                continue;
            }
            let task = tasks
                .iter()
                .find(|task| task.id == diagnostic.task_id)
                .unwrap();
            let map_id = task.last_map.as_ref().unwrap().thread_id.clone();
            let thread_dir = root.join("threads");
            std::fs::create_dir_all(&thread_dir).map_err(|e| e.to_string())?;
            let log = crate::threads::ThreadLog::open(root, &map_id);
            let safe_map_id = log.id().to_owned();
            let log_path = thread_dir.join(format!("{safe_map_id}.jsonl"));
            if !log_path.exists() {
                std::fs::File::create_new(&log_path).map_err(|e| e.to_string())?;
            }
            log.write_meta(&crate::threads::ThreadMeta {
                id: safe_map_id,
                started: crate::threads::now_secs(),
                title: task.title.clone(),
                cwd: task.workspace.clone(),
                account: Some(diagnostic.account.clone()),
            })
            .map_err(|e| e.to_string())?;
        }
        crate::store::modify_store(root, |store| {
            for (diagnostic, action) in &changes {
                if *action == "removed" {
                    store.tasks.retain(|task| task.id != diagnostic.task_id);
                }
            }
            Ok(())
        })
        .map_err(|e| e.to_string())?;
    }
    for (diagnostic, action) in actions {
        let result = if dry_run && action != "still unreadable" {
            format!("would be {action}")
        } else if action == "still unreadable" {
            format!("still unreadable: {}", diagnostic.failure_detail())
        } else {
            action.to_owned()
        };
        output.push_str(&format!(
            "{} ({}/{}): {}\n",
            short_title(&diagnostic.title),
            diagnostic.provider.as_str(),
            diagnostic.account,
            result
        ));
    }
    if output.is_empty() {
        output.push_str("no damaged chats found\n");
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
    use crate::store::{record_task_session, TaskMapPointer};
    use std::fs;

    #[test]
    fn terminal_safe_strips_bidi_and_zero_width() {
        let spoofed = "invoice\u{202e}fdp.exe\u{200b}\u{2066}🙂";
        assert_eq!(terminal_safe(spoofed), "invoicefdp.exe🙂");
    }

    #[test]
    fn repair_dry_run_backup_and_second_run_are_safe() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("codex-home");
        let session_dir = home.join("sessions/2026/11/13");
        fs::create_dir_all(&session_dir).unwrap();
        let with_message = "11111111-1111-4111-8111-111111111111";
        let empty = "22222222-2222-4222-8222-222222222222";
        for (id, body) in [(with_message, Some("keep this")), (empty, None)] {
            let mut text = format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"session_id\":\"{id}\"}}}}\n"
            );
            if let Some(body) = body {
                text.push_str(&format!(
                    "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{body}\"}}]}}}}\n"
                ));
            }
            fs::write(
                session_dir.join(format!("rollout-2026-11-13T00-00-00-{id}.jsonl")),
                text,
            )
            .unwrap();
        }
        crate::store::upsert_stored_account(
            dir.path(),
            crate::store::StoredAccount {
                id: "fixture".into(),
                provider: ProviderKind::Codex,
                home_path: Some(home.to_string_lossy().into_owned()),
                label: "Fixture".into(),
                created_at: "0".into(),
            },
        )
        .unwrap();
        for (id, title, session) in [
            ("keep-task", "Keep task", with_message),
            ("empty-task", "Empty task", empty),
        ] {
            record_task_session(
                dir.path(),
                id,
                title,
                &home.to_string_lossy(),
                ProviderKind::Codex,
                "fixture",
                session,
                Some(TaskMapPointer {
                    version: 2,
                    thread_id: format!("map-{id}"),
                }),
            )
            .unwrap();
        }
        let before = fs::read(crate::store::store_path(dir.path())).unwrap();
        let dry = run(dir.path(), &["repair".into(), "--dry-run".into()]).unwrap();
        assert!(dry.contains("would be repaired"));
        assert!(dry.contains("would be removed"));
        assert_eq!(
            fs::read(crate::store::store_path(dir.path())).unwrap(),
            before
        );

        let repaired = run(dir.path(), &["repair".into()]).unwrap();
        assert!(repaired.contains("backup:"));
        let backup = fs::read_dir(crate::store::store_dir(dir.path()))
            .unwrap()
            .flatten()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("chat-repair-")
            })
            .unwrap();
        assert!(backup.path().join("tasks/keep-task.json").is_file());
        assert!(backup.path().join("tasks/empty-task.json").is_file());
        assert!(crate::store::task(dir.path(), "empty-task").is_none());
        assert!(crate::store::task(dir.path(), "keep-task")
            .unwrap()
            .last_map
            .is_some());
        assert!(dir.path().join("threads/map-keep-task.jsonl").is_file());
        assert!(dir.path().join("threads/map-keep-task.meta.json").is_file());
        let second = run(dir.path(), &["repair".into()]).unwrap();
        assert!(second.contains("still unreadable") || second.contains("no damaged chats"));
        assert_eq!(crate::store::list_tasks(dir.path()).len(), 1);
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
