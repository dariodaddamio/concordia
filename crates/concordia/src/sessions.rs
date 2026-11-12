//! Resume picker — list Codex / Claude / Cursor sessions from account homes.
//! Never uses resume --last; callers bind an explicit session id.

use crate::accounts::list_all_accounts;
use crate::store::ProviderKind;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const MAX_DISCOVERY_ENTRIES: usize = 20_000;
const MAX_DISCOVERED_FILES: usize = 5_000;
const MAX_DISCOVERED_ACCOUNTS: usize = 32;
// Native Codex setup (tools, instructions and environment) can exceed 100 KiB
// before the first user request. Keep reads bounded without hiding those chats.
const MAX_SUMMARY_BYTES: u64 = 1024 * 1024;
const MAX_SUMMARY_LINE_BYTES: usize = 256 * 1024;
const MAX_SUMMARY_LINES: usize = 200;
const MAX_SUMMARIZED_RECORDS: usize = 1000;

struct Discovery {
    files: Vec<(PathBuf, u128)>,
    truncated: bool,
    entries_scanned: usize,
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut out: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() && max_chars > 0 {
        out.pop();
        out.push('…');
    }
    out
}

fn read_summary_lines(file: File, max_lines: usize, first_line_bytes: usize) -> Vec<Vec<u8>> {
    let mut reader = BufReader::new(file.take(MAX_SUMMARY_BYTES));
    let mut lines = Vec::new();
    let mut first = true;
    while lines.len() < max_lines {
        let mut line = Vec::new();
        let limit = if first {
            first_line_bytes
        } else {
            MAX_SUMMARY_LINE_BYTES
        };
        first = false;
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) if line.len() <= limit => lines.push(line),
            Ok(_) => continue,
        }
    }
    lines
}

/// Ignore large setup/attachment fields while extracting only exact session metadata.
#[derive(serde::Deserialize)]
struct IdentityRecord {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    payload: Option<IdentityPayload>,
}

#[derive(serde::Deserialize)]
struct IdentityPayload {
    id: Option<String>,
    session_id: Option<String>,
    cwd: Option<String>,
    thread_source: Option<String>,
    source: Option<Value>,
}

pub(crate) fn identity_matches(
    provider: ProviderKind,
    bytes: &[u8],
    expected: &str,
) -> Result<bool, String> {
    let mut found = false;
    for line in bytes.split(|byte| *byte == b'\n') {
        let Ok(record) = serde_json::from_slice::<IdentityRecord>(line) else {
            continue;
        };
        let id = match provider {
            ProviderKind::Codex if record.kind.as_deref() == Some("session_meta") => record
                .payload
                .as_ref()
                .and_then(|payload| payload.id.as_deref().or(payload.session_id.as_deref())),
            ProviderKind::Claude => record.session_id.as_deref(),
            _ => None,
        };
        if let Some(id) = id {
            if id != expected {
                return Err(
                    "native source contains a conflicting session identity; original preserved"
                        .into(),
                );
            }
            found = true;
        }
    }
    Ok(found)
}

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub account: String,
    pub provider: ProviderKind,
    pub session_id: String,
    pub cwd: Option<String>,
    pub preview: Option<String>,
    pub modified_ms: u128,
    /// Real provider user turns; metadata, setup and command records do not count.
    pub user_messages: usize,
}

#[derive(Debug, Clone)]
pub struct PickerRow {
    pub id: String,
    pub label: String,
    pub detail: String,
}

/// Rows for the durable task view. The latest session is shown inline; the full chain is kept in
/// the task record and can be inspected without changing provider-native history.
pub fn tasks_to_picker_rows(tasks: &[crate::store::TaskRecord]) -> Vec<PickerRow> {
    tasks
        .iter()
        .filter_map(|task| {
            let latest = task.sessions.last()?;
            Some(PickerRow {
                id: format!("task:{}", task.id),
                label: task.title.clone(),
                detail: format!(
                    "{} / {} · {} session{} · {}",
                    latest.provider.as_str(),
                    latest.account,
                    task.sessions.len(),
                    if task.sessions.len() == 1 { "" } else { "s" },
                    task.state.as_str()
                ),
            })
        })
        .collect()
}

/// Current native sessions without a task ledger entry remain directly selectable.
/// This view never fabricates durable tasks or rewrites provider-owned files.
pub fn native_session_picker_rows(sessions: &[SessionSummary]) -> Vec<PickerRow> {
    sessions
        .iter()
        .map(|session| PickerRow {
            id: format!("resume:{}:{}", session.account, session.session_id),
            label: clean_preview(session.preview.as_deref(), 60),
            detail: format!(
                "{} / {} ? native session",
                session.provider.as_str(),
                session.account
            ),
        })
        .collect()
}

pub fn format_relative_time(modified_ms: u128, now_ms: u128) -> String {
    let d = now_ms.saturating_sub(modified_ms);
    if d < 45_000 {
        return "just now".into();
    }
    if d < 3_600_000 {
        return format!("{}m ago", (d / 60_000).max(1));
    }
    if d < 86_400_000 {
        return format!("{}h ago", (d / 3_600_000).max(1));
    }
    if d < 86_400_000 * 14 {
        return format!("{}d ago", (d / 86_400_000).max(1));
    }
    let secs = (modified_ms / 1000) as i64;
    // YYYY-MM-DD approximation via unix days is fine for old rows
    format!("t{secs}")
}

pub fn clean_preview(raw: Option<&str>, max: usize) -> String {
    let Some(raw) = raw else {
        return "untitled session".into();
    };
    let mut t = raw.trim().to_string();
    if t.contains("pasted_content") {
        if let Some(rest) = t.split('#').nth(1) {
            let heading = rest
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .trim_start_matches([' ', '\t']);
            if !heading.is_empty() {
                t = heading.to_string();
            }
        }
    }
    if let Some(stripped) = t.strip_prefix('<') {
        if let Some(end) = stripped.find('>') {
            t = stripped[end + 1..].trim().to_string();
        }
    }
    let collapsed: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() || collapsed.starts_with('<') {
        return "untitled session".into();
    }
    if collapsed.chars().count() > max {
        collapsed.chars().take(max).collect()
    } else {
        collapsed
    }
}

fn normalize_path(p: &str) -> String {
    p.replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn basename(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string())
}

fn mtime_ms(path: &Path) -> u128 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn walk_jsonl(
    root: &Path,
    keep: &dyn Fn(&Path, &str) -> bool,
    file_limit: usize,
    entry_limit: usize,
) -> Discovery {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    let mut visited = 0;
    let mut truncated = false;
    'walk: while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > entry_limit || found.len() >= file_limit {
                truncated = true;
                break 'walk;
            }
            let path = entry.path();
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if ft.is_dir() {
                if name == "subagents" {
                    continue;
                }
                pending.push(path);
            } else if ft.is_file() && name.ends_with(".jsonl") && keep(&path, &name) {
                found.push((path.clone(), mtime_ms(&path)));
            }
        }
    }
    Discovery {
        files: found,
        truncated,
        entries_scanned: visited.min(entry_limit),
    }
}

fn session_id_from_filename(path: &Path) -> String {
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let re_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(cap) = extract_uuid(&re_name) {
        return cap;
    }
    name
}

fn extract_uuid(s: &str) -> Option<String> {
    // 8-4-4-4-12 hex
    for (i, _) in s.char_indices() {
        let Some(slice) = s.get(i..i.saturating_add(36)) else {
            continue;
        };
        if is_uuid(slice) {
            return Some(slice.to_string());
        }
    }
    None
}

fn is_uuid(s: &str) -> bool {
    if s.len() != 36 {
        return false;
    }
    let b = s.as_bytes();
    for (i, c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *c != b'-' {
                    return false;
                }
            }
            _ => {
                if !c.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

fn discover_codex(home: &Path, file_limit: usize, entry_limit: usize) -> Discovery {
    let sessions = home.join("sessions");
    if !sessions.is_dir() {
        return Discovery {
            files: Vec::new(),
            truncated: false,
            entries_scanned: 0,
        };
    }
    walk_jsonl(&sessions, &|_, _| true, file_limit, entry_limit)
}

fn discover_claude(home: &Path, file_limit: usize, entry_limit: usize) -> Discovery {
    let projects = home.join("projects");
    if !projects.is_dir() {
        return Discovery {
            files: Vec::new(),
            truncated: false,
            entries_scanned: 0,
        };
    }
    walk_jsonl(
        &projects,
        &|path, name| {
            if name.starts_with("agent-") {
                return false;
            }
            let p = path.to_string_lossy();
            !p.contains("subagents")
        },
        file_limit,
        entry_limit,
    )
}

fn discover_cursor(home: &Path, file_limit: usize, entry_limit: usize) -> Discovery {
    let chats = home.join("chats");
    if !chats.is_dir() {
        return Discovery {
            files: Vec::new(),
            truncated: false,
            entries_scanned: 0,
        };
    }
    let mut found = Vec::new();
    let mut visited = 0;
    let mut truncated = false;
    let Ok(projects) = fs::read_dir(&chats) else {
        return Discovery {
            files: found,
            truncated,
            entries_scanned: visited,
        };
    };
    'projects: for project in projects.flatten() {
        visited += 1;
        if visited > entry_limit {
            truncated = true;
            break;
        }
        let Ok(ft) = project.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let Ok(chat_ids) = fs::read_dir(project.path()) else {
            continue;
        };
        for chat in chat_ids.flatten() {
            visited += 1;
            if visited > entry_limit || found.len() >= file_limit {
                truncated = true;
                break 'projects;
            }
            let dir = chat.path();
            let meta = dir.join("meta.json");
            if meta.is_file() {
                found.push((dir, mtime_ms(&meta)));
            }
        }
    }
    Discovery {
        files: found,
        truncated,
        entries_scanned: visited.min(entry_limit),
    }
}

fn summarize_codex(account: &str, path: &Path, modified_ms: u128) -> Option<SessionSummary> {
    summarize_codex_with_child(account, path, modified_ms).map(|(summary, _)| summary)
}

fn summarize_codex_with_child(
    account: &str,
    path: &Path,
    modified_ms: u128,
) -> Option<(SessionSummary, bool)> {
    let mut session_id = session_id_from_filename(path);
    let mut cwd: Option<String> = None;
    let mut preview: Option<String> = None;
    let mut user_messages = 0;
    let mut child = false;
    let file = File::open(path).ok()?;
    for line in read_summary_lines(file, 80, MAX_SUMMARY_BYTES as usize) {
        if let Ok(record) = serde_json::from_slice::<IdentityRecord>(&line) {
            if record.kind.as_deref() == Some("session_meta") {
                if let Some(metadata) = record.payload {
                    if let Some(id) = metadata.id.or(metadata.session_id) {
                        session_id = id;
                    }
                    cwd = metadata.cwd.or(cwd);
                    child |= metadata.thread_source.as_deref() == Some("subagent")
                        || metadata.source.as_ref().is_some_and(|source| {
                            source == "subagent" || source.get("subagent").is_some()
                        });
                }
                continue;
            }
        }
        if line.len() > MAX_SUMMARY_LINE_BYTES {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if ty == "session_meta" {
            if let Some(p) = v.get("payload") {
                if let Some(id) = p
                    .get("id")
                    .or_else(|| p.get("session_id"))
                    .and_then(|x| x.as_str())
                {
                    session_id = id.to_string();
                }
                if let Some(c) = p.get("cwd").and_then(|x| x.as_str()) {
                    cwd = Some(c.to_string());
                }
            }
            continue;
        }
        if ty == "response_item" {
            if let Some(p) = v.get("payload") {
                if p.get("type").and_then(|t| t.as_str()) == Some("message")
                    && p.get("role").and_then(|t| t.as_str()) == Some("user")
                {
                    if let Some(arr) = p.get("content").and_then(|c| c.as_array()) {
                        let joined = arr
                            .iter()
                            .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join(" ");
                        let title = crate::session_titles::title_from_prompt(&joined);
                        if (!title.is_empty() && !title.starts_with('/'))
                            || arr.iter().any(|item| {
                                matches!(
                                    item.get("type").and_then(Value::as_str),
                                    Some("input_image" | "image")
                                )
                            })
                        {
                            user_messages += 1;
                        }
                        if preview.is_none() && !title.is_empty() && !title.starts_with('/') {
                            preview = Some(bounded_text(&title, 512));
                        }
                    }
                }
            }
        }
    }
    Some((
        SessionSummary {
            account: account.to_string(),
            provider: ProviderKind::Codex,
            session_id,
            cwd,
            preview,
            modified_ms,
            user_messages,
        },
        child,
    ))
}

fn summarize_claude(account: &str, path: &Path, modified_ms: u128) -> Option<SessionSummary> {
    let mut session_id = session_id_from_filename(path);
    let mut cwd = decode_claude_project_cwd(path);
    let mut preview: Option<String> = None;
    let mut user_messages = 0;
    let file = File::open(path).ok()?;
    for line in read_summary_lines(file, MAX_SUMMARY_LINES, MAX_SUMMARY_LINE_BYTES) {
        let Ok(v) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if let Some(id) = v.get("sessionId").and_then(|x| x.as_str()) {
            session_id = id.to_string();
        }
        if let Some(c) = v.get("cwd").and_then(|x| x.as_str()) {
            cwd = Some(c.to_string());
        }
        if v.get("type").and_then(|t| t.as_str()) == Some("user") {
            if v.get("isMeta").and_then(|m| m.as_bool()).unwrap_or(false) {
                continue;
            }
            if let Some(text) = claude_user_text(&v) {
                let title = crate::session_titles::title_from_prompt(&text);
                if !title.is_empty() && !title.starts_with('/') {
                    user_messages += 1;
                }
                if preview.is_none() && !title.is_empty() && !title.starts_with('/') {
                    preview = Some(title);
                }
            }
        }
    }
    Some(SessionSummary {
        account: account.to_string(),
        provider: ProviderKind::Claude,
        session_id,
        cwd,
        preview: preview.map(|s| bounded_text(&s, 512)),
        modified_ms,
        user_messages,
    })
}

fn claude_user_text(v: &Value) -> Option<String> {
    let content = v
        .get("message")
        .and_then(|m| m.get("content"))
        .or_else(|| v.get("content"))?;
    let text = if let Some(s) = content.as_str() {
        s.trim().to_string()
    } else {
        content
            .as_array()?
            .iter()
            .filter_map(|c| {
                c.as_str().map(|s| s.to_string()).or_else(|| {
                    c.get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                })
            })
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string()
    };
    if text.is_empty()
        || text.starts_with("<local-command")
        || text.starts_with("<command-name>")
        || text.starts_with("<command-message>")
    {
        return None;
    }
    Some(text)
}

pub fn decode_claude_project_cwd(session_path: &Path) -> Option<String> {
    let parts: Vec<_> = session_path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let i = parts
        .iter()
        .rposition(|p| p.eq_ignore_ascii_case("projects"))?;
    let encoded = parts.get(i + 1)?;
    if encoded.is_empty() || encoded.contains('.') {
        return None;
    }
    // C--Users-alex-dev → C:\Users\alex\dev
    let bytes = encoded.as_bytes();
    if bytes.len() >= 4 && bytes[1] == b'-' && bytes[2] == b'-' {
        let drive = encoded.chars().next()?;
        let rest = &encoded[3..];
        return Some(format!("{drive}:\\{}", rest.replace('-', "\\")));
    }
    None
}

fn read_file_capped(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    let file = File::open(path)?;
    let mut text = String::new();
    file.take(max_bytes).read_to_string(&mut text)?;
    Ok(text)
}

fn summarize_cursor(account: &str, dir: &Path, modified_ms: u128) -> Option<SessionSummary> {
    let meta_path = dir.join("meta.json");
    let raw = read_file_capped(&meta_path, 32 * 1024).ok()?;
    let meta: Value = serde_json::from_str(&raw).ok()?;
    let session_id = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cwd = meta.get("cwd").and_then(|c| c.as_str()).map(str::to_string);
    let mut preview = meta
        .get("title")
        .and_then(|t| t.as_str())
        .map(|s| bounded_text(s, 512));
    let hist_path = dir.join("prompt_history.json");
    let mut user_messages = 0;
    if hist_path.is_file() {
        if let Ok(hist) = read_file_capped(&hist_path, 32 * 1024) {
            if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&hist) {
                for item in &items {
                    if let Some(t) = item.as_str().map(crate::session_titles::title_from_prompt) {
                        if !t.is_empty() && !t.starts_with('/') {
                            user_messages += 1;
                            if preview.as_ref().is_none_or(|preview| preview.len() < 3) {
                                preview = Some(bounded_text(&t, 512));
                            }
                        }
                    }
                }
            }
        }
    }
    let updated = meta
        .get("updatedAtMs")
        .and_then(|x| x.as_u64())
        .or_else(|| meta.get("createdAtMs").and_then(|x| x.as_u64()))
        .map(|u| u as u128)
        .unwrap_or(modified_ms);
    Some(SessionSummary {
        account: account.to_string(),
        provider: ProviderKind::Cursor,
        session_id,
        cwd,
        preview,
        modified_ms: updated,
        user_messages,
    })
}

/// Native transcript locations are discovered from registered account homes, never constructed
/// from a caller-provided session path. Cursor records currently contain metadata only.
pub struct NativeRecordScan {
    pub records: Vec<(SessionSummary, PathBuf)>,
    pub truncated: bool,
}

fn summarize_one(
    account: &str,
    provider: ProviderKind,
    path: &Path,
    ms: u128,
) -> Option<(SessionSummary, PathBuf)> {
    let summary = match provider {
        ProviderKind::Codex => summarize_codex(account, path, ms),
        ProviderKind::Claude => summarize_claude(account, path, ms),
        ProviderKind::Cursor => summarize_cursor(account, path, ms),
    }?;
    Some((summary, path.to_path_buf()))
}

pub fn list_native_records_bounded(
    state_root: &Path,
    limit: usize,
    account: Option<&str>,
    provider: Option<ProviderKind>,
) -> NativeRecordScan {
    scan_native_records(state_root, limit, account, provider, true)
}

fn scan_native_records(
    state_root: &Path,
    limit: usize,
    account: Option<&str>,
    provider: Option<ProviderKind>,
    include_child_agents: bool,
) -> NativeRecordScan {
    let limit = limit.clamp(1, 1000);
    let accounts: Vec<_> = list_all_accounts(state_root)
        .into_iter()
        .filter(|a| account.is_none_or(|id| a.account == id))
        .filter(|a| provider.is_none_or(|p| a.provider == p))
        .collect();
    let mut records = Vec::new();
    let mut candidates: Vec<(String, ProviderKind, PathBuf, PathBuf, u128)> = Vec::new();
    let mut entries_left = MAX_DISCOVERY_ENTRIES;
    let mut files_left = MAX_DISCOVERED_FILES;
    let mut truncated = accounts.len() > MAX_DISCOVERED_ACCOUNTS;
    for a in accounts.iter().take(MAX_DISCOVERED_ACCOUNTS) {
        if entries_left == 0 || files_left == 0 {
            truncated = true;
            break;
        }
        let Some(home) = a.home_path.as_ref() else {
            continue;
        };
        let file_limit = files_left.min(MAX_DISCOVERED_FILES);
        let discovery = match a.provider {
            ProviderKind::Codex => discover_codex(home, file_limit, entries_left),
            ProviderKind::Claude => discover_claude(home, file_limit, entries_left),
            ProviderKind::Cursor => discover_cursor(home, file_limit, entries_left),
        };
        let files_seen = discovery.files.len();
        let cut = discovery.truncated;
        let visited = discovery.entries_scanned;
        entries_left = entries_left.saturating_sub(visited);
        files_left = files_left.saturating_sub(files_seen);
        truncated |= cut;
        candidates.extend(
            discovery
                .files
                .into_iter()
                .map(|(path, ms)| (a.account.clone(), a.provider, home.clone(), path, ms)),
        );
    }
    candidates.sort_by_key(|(_, _, _, _, ms)| std::cmp::Reverse(*ms));
    for (index, (account, provider, home, path, ms)) in candidates.iter().enumerate() {
        if records.len() >= limit {
            break;
        }
        if index >= MAX_SUMMARIZED_RECORDS {
            truncated = true;
            break;
        }
        let summarized = if *provider == ProviderKind::Codex && !include_child_agents {
            summarize_codex_with_child(account, path, *ms)
                .filter(|(_, child)| !*child)
                .map(|(summary, _)| (summary, path.clone()))
        } else {
            summarize_one(account, *provider, path, *ms)
        };
        if let Some(mut record) = summarized {
            if !include_child_agents
                && records
                    .iter()
                    .any(|(known, _): &(SessionSummary, PathBuf)| {
                        known.provider == record.0.provider
                            && known.account == record.0.account
                            && known.session_id == record.0.session_id
                            && known.cwd == record.0.cwd
                    })
            {
                continue;
            }
            if let Some(title) = crate::session_titles::get(
                state_root,
                *provider,
                account,
                home,
                &record.0.session_id,
            ) {
                record.0.preview = Some(title);
            }
            // Home's limit counts user chats, not empty artifacts that will be
            // discarded afterward. Exact source lookup still includes both.
            if !include_child_agents && record.0.user_messages == 0 {
                continue;
            }
            records.push(record);
        }
    }
    NativeRecordScan { records, truncated }
}

/// Native artifacts whose complete bounded content contains no real user turn.
/// These are report candidates only; provider-owned files are never deleted here.
pub fn list_empty_native_records(
    state_root: &Path,
    limit: usize,
) -> Vec<(SessionSummary, PathBuf)> {
    list_native_records_bounded(state_root, limit, None, None)
        .records
        .into_iter()
        .filter(|(session, path)| {
            session.user_messages == 0 && definitely_empty_native_record(session.provider, path)
        })
        .collect()
}

/// Summary absence is not deletion proof. Retain every incomplete, unrecognized or
/// meaningful record; a bounded prefix must never authorize removal of a whole chat.
fn definitely_empty_native_record(provider: ProviderKind, path: &Path) -> bool {
    fn complete_json(path: &Path) -> Option<Vec<u8>> {
        let metadata = fs::symlink_metadata(path).ok()?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return None;
            }
        }
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_SUMMARY_BYTES
        {
            return None;
        }
        let mut bytes = Vec::new();
        File::open(path)
            .ok()?
            .take(MAX_SUMMARY_BYTES + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        (bytes.len() as u64 == metadata.len()
            && fs::metadata(path).ok()?.modified().ok()? == metadata.modified().ok()?)
        .then_some(bytes)
    }
    if provider == ProviderKind::Cursor {
        if crate::fs_safe::resolved_directory(path).is_none() {
            return false;
        }
        let Ok(entries) = fs::read_dir(path) else {
            return false;
        };
        for (index, entry) in entries.enumerate() {
            let Ok(entry) = entry else { return false };
            if index >= 2
                || !matches!(
                    entry.file_name().to_str(),
                    Some("meta.json" | "prompt_history.json")
                )
            {
                return false;
            }
        }
        let Some(meta) = complete_json(&path.join("meta.json")) else {
            return false;
        };
        let Ok(meta) = serde_json::from_slice::<Value>(&meta) else {
            return false;
        };
        if !meta.is_object()
            || meta
                .get("title")
                .and_then(Value::as_str)
                .is_some_and(|title| !title.trim().is_empty())
        {
            return false;
        }
        let history = path.join("prompt_history.json");
        if !history.exists() {
            return true;
        }
        let Some(bytes) = complete_json(&history) else {
            return false;
        };
        return serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|value| value.as_array().cloned())
            .is_some_and(|items| {
                items
                    .iter()
                    .all(|item| item.as_str().is_some_and(|text| text.trim().is_empty()))
            });
    }
    let Some(bytes) = complete_json(path) else {
        return false;
    };
    bytes.split(|byte| *byte == b'\n').all(|line| {
        if line.iter().all(u8::is_ascii_whitespace) {
            return true;
        }
        if line.len() > MAX_SUMMARY_LINE_BYTES {
            return false;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return false;
        };
        value.as_object().is_some_and(|object| object.is_empty())
            || (provider == ProviderKind::Codex
                && value.get("type").and_then(Value::as_str) == Some("session_meta"))
            || (provider == ProviderKind::Claude
                && value.get("type").and_then(Value::as_str) == Some("system")
                && value.get("subtype").and_then(Value::as_str) == Some("init"))
    })
}

/// Report empty native artifacts without racing a provider-owned writer.
/// The historical command spelling is retained; deletion requires provider-owned quiescence
/// that Concordia cannot establish through the supported source-reading interfaces.
pub fn prune_empty_cli(state_root: &Path) -> Result<String, (String, i32)> {
    let rows = list_empty_native_records(state_root, MAX_SUMMARIZED_RECORDS);
    let mut out = if rows.is_empty() {
        String::from("No provably empty sessions found.\n")
    } else {
        String::from("Provably empty native session artifacts (retained):\n")
    };
    for (session, path) in rows {
        out.push_str(&format!(
            "- {} / {} / {}\n  {}\n",
            session.provider.as_str(),
            session.account,
            session.session_id,
            path.display()
        ));
    }
    out.push_str("No native sessions were removed. Concordia cannot prove a provider has stopped writing these sources. Use the provider's own session management for cleanup.\n");
    Ok(out)
}

/// Find one native session by exact id inside one registered account home. If the bounded scan
/// cannot prove the result is unique/absent, return an explicit incomplete-scan error.
pub fn find_native_record(
    state_root: &Path,
    account: &str,
    provider: Option<ProviderKind>,
    session_id: &str,
) -> Result<Option<(SessionSummary, PathBuf)>, String> {
    let accounts: Vec<_> = list_all_accounts(state_root)
        .into_iter()
        .filter(|a| a.account == account && provider.is_none_or(|p| a.provider == p))
        .collect();
    let [account_def] = accounts.as_slice() else {
        if accounts.is_empty() {
            return Ok(None);
        }
        return Err("account id is ambiguous; specify --provider".into());
    };
    let Some(home) = account_def.home_path.as_ref() else {
        return Ok(None);
    };
    let discovery = match account_def.provider {
        ProviderKind::Codex => discover_codex(home, MAX_DISCOVERED_FILES, MAX_DISCOVERY_ENTRIES),
        ProviderKind::Claude => discover_claude(home, MAX_DISCOVERED_FILES, MAX_DISCOVERY_ENTRIES),
        ProviderKind::Cursor => discover_cursor(home, MAX_DISCOVERED_FILES, MAX_DISCOVERY_ENTRIES),
    };
    if discovery.truncated {
        return Err(
            "native chat scan bound reached; could not verify this session is unique".into(),
        );
    }
    let uuid_id = is_uuid(session_id);
    let mut matches = Vec::new();
    let mut summarized = 0;
    for (path, ms) in discovery.files {
        let filename_id = match account_def.provider {
            ProviderKind::Codex | ProviderKind::Claude => session_id_from_filename(&path),
            ProviderKind::Cursor => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        if uuid_id && filename_id != session_id {
            continue;
        }
        summarized += 1;
        if summarized > MAX_SUMMARIZED_RECORDS {
            return Err(
                "native chat scan bound reached; could not verify this session is unique".into(),
            );
        }
        if let Some(mut record) =
            summarize_one(&account_def.account, account_def.provider, &path, ms)
        {
            if record.0.session_id == session_id {
                if account_def.provider != ProviderKind::Cursor {
                    let mut header = Vec::new();
                    File::open(&path)
                        .map_err(|error| error.to_string())?
                        .take(MAX_SUMMARY_BYTES)
                        .read_to_end(&mut header)
                        .map_err(|error| error.to_string())?;
                    // Absence may be an oversized attachment record validated by the capture
                    // reader; a contradictory explicit identity can never be accepted.
                    identity_matches(account_def.provider, &header, session_id)?;
                }
                if let Some(title) = crate::session_titles::get(
                    state_root,
                    account_def.provider,
                    account,
                    home,
                    session_id,
                ) {
                    record.0.preview = Some(title);
                }
                matches.push(record);
                if matches.len() > 1 {
                    break;
                }
            }
        }
    }
    if matches.len() > 1 {
        return Err("session id is ambiguous in that account; specify --provider".into());
    }
    Ok(matches.pop())
}

pub fn list_recent_sessions(state_root: &Path, limit: usize) -> Vec<SessionSummary> {
    scan_native_records(state_root, limit.clamp(1, 100), None, None, false)
        .records
        .into_iter()
        .filter(|(session, _)| session.user_messages > 0)
        .map(|(session, _)| session)
        .collect()
}

pub fn count_sessions_here(sessions: &[SessionSummary], prefer_cwd: Option<&Path>) -> usize {
    let Some(prefer) = prefer_cwd else {
        return 0;
    };
    let prefer_s = prefer.to_string_lossy();
    let norm = normalize_path(&prefer_s);
    let folder = basename(&prefer_s);
    sessions
        .iter()
        .filter(|s| {
            let Some(cwd) = s.cwd.as_deref() else {
                return false;
            };
            normalize_path(cwd) == norm || basename(cwd) == folder
        })
        .count()
}

pub fn trust_picker_rows(current: crate::store::TrustMode) -> Vec<PickerRow> {
    use crate::store::TrustMode;
    let cur = |m: TrustMode| if current == m { " · current" } else { "" };
    vec![
        PickerRow {
            id: "trust:plan".into(),
            label: format!("Plan{}", cur(TrustMode::Plan)),
            detail: "Read and plan; no file edits.".into(),
        },
        PickerRow {
            id: "trust:ask".into(),
            label: format!("Ask{}", cur(TrustMode::Ask)),
            detail: "Provider approval prompts may pause work.".into(),
        },
        PickerRow {
            id: "trust:workspace".into(),
            label: format!("Workspace{}", cur(TrustMode::Workspace)),
            detail: "Allow workspace edits; provider rules apply.".into(),
        },
        PickerRow {
            id: "trust:full".into(),
            label: format!("Full access{}", cur(TrustMode::Full)),
            detail: "Use with care: bypass provider permissions.".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_times() {
        let now = 1_000_000_000u128;
        assert_eq!(format_relative_time(now - 30_000, now), "just now");
        assert_eq!(format_relative_time(now - 2 * 3_600_000, now), "2h ago");
    }

    #[test]
    fn preview_clean() {
        assert_eq!(
            clean_preview(Some("  fix the picker  "), 56),
            "fix the picker"
        );
    }

    #[test]
    fn claude_cwd_decode() {
        let p = PathBuf::from(r"C:\Users\x\.claude\projects\C--Users-alex-dev\abc.jsonl");
        assert_eq!(
            decode_claude_project_cwd(&p).as_deref(),
            Some(r"C:\Users\alex\dev")
        );
    }

    #[test]
    fn native_summary_fields_and_single_line_reads_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic-session.jsonl");
        let meta = serde_json::json!({
            "type":"session_meta",
            "payload":{"id":"synthetic-session","cwd":"c".repeat(2000)}
        });
        let user = serde_json::json!({
            "type":"response_item",
            "payload":{"type":"message","role":"user","content":[{"text":"p".repeat(2000)}]}
        });
        std::fs::write(&path, format!("{meta}\n{user}\n")).unwrap();
        let summary = summarize_codex("synthetic", &path, 1).unwrap();
        assert_eq!(summary.preview.as_ref().unwrap().chars().count(), 80);
        assert_eq!(summary.cwd.as_ref().unwrap().chars().count(), 2000);

        std::fs::write(&path, format!("{}\n{meta}\n", "x".repeat(3 * 1024 * 1024))).unwrap();
        let summary = summarize_codex("synthetic", &path, 1).unwrap();
        assert!(
            summary.preview.is_none(),
            "metadata after the byte cap is not read"
        );
    }

    #[test]
    fn native_discovery_reports_its_file_bound() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("one.jsonl"), "{}\n").unwrap();
        std::fs::write(dir.path().join("two.jsonl"), "{}\n").unwrap();
        let found = walk_jsonl(dir.path(), &|_, _| true, 1, 100);
        assert_eq!(found.files.len(), 1);
        assert!(found.truncated);
    }

    #[test]
    fn codex_summary_skips_injected_setup_before_request() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let lines: Vec<String> = [
            "# AGENTS.md instructions\nRead project instructions",
            "<environment_context>\nsetup only\n</environment_context>",
            "Repair resumed history",
        ]
        .iter()
        .map(|text| {
            serde_json::json!({"type":"response_item","payload":{
                "type":"message","role":"user","content":[{"text":text}]
            }})
            .to_string()
        })
        .collect();
        fs::write(&path, lines.join("\n")).unwrap();
        assert_eq!(
            summarize_codex("synthetic", &path, 1)
                .unwrap()
                .preview
                .as_deref(),
            Some("Repair resumed history")
        );
    }

    #[test]
    fn native_artifacts_without_a_user_message_are_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let codex = dir.path().join("codex.jsonl");
        let claude = dir.path().join("claude.jsonl");
        let cursor = dir.path().join("cursor");
        fs::create_dir(&cursor).unwrap();

        fs::write(
            &codex,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"c\"}}\n",
        )
        .unwrap();
        fs::write(&claude, "{\"type\":\"system\",\"sessionId\":\"a\"}\n").unwrap();
        fs::write(cursor.join("meta.json"), "{\"id\":\"u\"}").unwrap();
        fs::write(cursor.join("prompt_history.json"), "[\"/help\"]").unwrap();
        assert_eq!(summarize_codex("a", &codex, 1).unwrap().user_messages, 0);
        assert_eq!(summarize_claude("a", &claude, 1).unwrap().user_messages, 0);
        assert_eq!(summarize_cursor("a", &cursor, 1).unwrap().user_messages, 0);

        fs::write(&codex, concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"c\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"text\":\"real request\"}]}}\n"
        )).unwrap();
        fs::write(
            &claude,
            concat!(
            "{\"type\":\"system\",\"sessionId\":\"a\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"real request\"}}\n"
        ),
        )
        .unwrap();
        fs::write(
            cursor.join("prompt_history.json"),
            "[\"/help\",\"real request\"]",
        )
        .unwrap();
        assert_eq!(summarize_codex("a", &codex, 1).unwrap().user_messages, 1);
        assert_eq!(summarize_claude("a", &claude, 1).unwrap().user_messages, 1);
        assert_eq!(summarize_cursor("a", &cursor, 1).unwrap().user_messages, 1);

        fs::write(&codex, concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"c\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"text\":\"/help\"}]}}\n"
        )).unwrap();
        fs::write(&claude, "{\"type\":\"user\",\"message\":{\"content\":\"<local-command-name>/help</local-command-name>\"}}\n").unwrap();
        fs::write(cursor.join("prompt_history.json"), "[\"/help\"]").unwrap();
        assert_eq!(summarize_codex("a", &codex, 1).unwrap().user_messages, 0);
        assert_eq!(summarize_claude("a", &claude, 1).unwrap().user_messages, 0);
        assert_eq!(summarize_cursor("a", &cursor, 1).unwrap().user_messages, 0);
    }

    #[test]
    fn codex_injected_context_is_not_a_title() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("codex.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"synthetic-codex\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"text\":\"<recommended_plugins>\\nsynthetic plugin\\n</recommended_plugins>\\n<environment_context>\\nsynthetic setup\\n</environment_context>\"}]}}\n"
            ),
        )
        .unwrap();
        let summary = summarize_codex("synthetic", &path, 1).unwrap();
        assert_eq!(summary.preview, None);
        assert_eq!(summary.user_messages, 0);
    }

    #[test]
    fn injected_only_session_counts_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let codex = dir.path().join("codex.jsonl");
        let claude = dir.path().join("claude.jsonl");
        let cursor = dir.path().join("cursor");
        fs::create_dir(&cursor).unwrap();
        fs::write(&codex, concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"c\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"text\":\"<environment_context>synthetic</environment_context>\"}]}}\n"
        )).unwrap();
        fs::write(&claude, "{\"type\":\"user\",\"message\":{\"content\":\"<user_instructions>synthetic</user_instructions>\"}}\n").unwrap();
        fs::write(cursor.join("meta.json"), "{}\n").unwrap();
        fs::write(
            cursor.join("prompt_history.json"),
            "[\"<system-reminder>synthetic</system-reminder>\"]",
        )
        .unwrap();
        assert_eq!(summarize_codex("a", &codex, 1).unwrap().user_messages, 0);
        assert_eq!(summarize_claude("a", &claude, 1).unwrap().user_messages, 0);
        assert_eq!(summarize_cursor("a", &cursor, 1).unwrap().user_messages, 0);
    }

    #[test]
    fn real_text_after_injected_block_becomes_title() {
        let dir = tempfile::tempdir().unwrap();
        let codex = dir.path().join("codex.jsonl");
        let claude = dir.path().join("claude.jsonl");
        let cursor = dir.path().join("cursor");
        fs::create_dir(&cursor).unwrap();
        let request =
            "<recommended_plugins>synthetic</recommended_plugins>\n\nFix the synthetic login";
        fs::write(&codex, format!("{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"text\":{request:?}}}]}}}}\n")).unwrap();
        fs::write(
            &claude,
            format!("{{\"type\":\"user\",\"message\":{{\"content\":{request:?}}}}}\n"),
        )
        .unwrap();
        fs::write(cursor.join("meta.json"), "{}\n").unwrap();
        fs::write(cursor.join("prompt_history.json"), format!("[{request:?}]")).unwrap();
        for summary in [
            summarize_codex("a", &codex, 1).unwrap(),
            summarize_claude("a", &claude, 1).unwrap(),
            summarize_cursor("a", &cursor, 1).unwrap(),
        ] {
            assert_eq!(summary.preview.as_deref(), Some("Fix the synthetic login"));
            assert_eq!(summary.user_messages, 1);
        }
    }

    #[test]
    fn empty_native_report_preserves_all_sources() {
        let dir = tempfile::tempdir().unwrap();
        let providers = [
            (ProviderKind::Codex, "codex", "sessions"),
            (
                ProviderKind::Claude,
                "claude",
                "projects/C--Users-synthetic-dev",
            ),
            (ProviderKind::Cursor, "cursor", "chats/project"),
        ];
        for (provider, account, relative) in providers {
            let home = dir.path().join(format!("home-{account}"));
            let base = home.join(relative);
            fs::create_dir_all(&base).unwrap();
            let empty = if provider == ProviderKind::Cursor {
                base.join("empty")
            } else {
                base.join("empty.jsonl")
            };
            let real = if provider == ProviderKind::Cursor {
                base.join("real")
            } else {
                base.join("real.jsonl")
            };
            if provider == ProviderKind::Cursor {
                fs::create_dir_all(&empty).unwrap();
                fs::create_dir_all(&real).unwrap();
                fs::write(empty.join("meta.json"), "{} ").unwrap();
                fs::write(real.join("meta.json"), "{} ").unwrap();
                fs::write(real.join("prompt_history.json"), "[\"keep me\"]").unwrap();
            } else {
                fs::write(&empty, "{}\n").unwrap();
                let content = if provider == ProviderKind::Codex {
                    "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"text\":\"keep me\"}]}}\n"
                } else {
                    "{\"type\":\"user\",\"message\":{\"content\":\"keep me\"}}\n"
                };
                fs::write(&real, content).unwrap();
            }
            crate::store::upsert_stored_account(
                dir.path(),
                crate::store::StoredAccount {
                    id: account.into(),
                    provider,
                    home_path: Some(home.to_string_lossy().into_owned()),
                    label: account.into(),
                    created_at: "0".into(),
                },
            )
            .unwrap();
        }
        let rows = list_empty_native_records(dir.path(), 10);
        assert_eq!(rows.len(), 3);
        assert!(rows
            .iter()
            .all(|(_, path)| path.to_string_lossy().contains("empty")));
        let report = prune_empty_cli(dir.path()).unwrap();
        assert!(report.contains("No native sessions were removed"));
        assert!(report.contains("provider's own session management"));
        assert_eq!(list_empty_native_records(dir.path(), 10).len(), 3);
        assert_eq!(
            list_native_records_bounded(dir.path(), 10, None, None)
                .records
                .into_iter()
                .filter(|(session, _)| session.user_messages > 0)
                .count(),
            3
        );
    }

    #[test]
    fn saved_titles_overlay_listing_and_exact_lookup_without_native_edits() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("synthetic-home");
        fs::create_dir_all(home.join("sessions")).unwrap();
        let path = home.join("sessions/rollout-synthetic-session.jsonl");
        let native = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"synthetic-session\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"text\":\"Native request\"}]}}\n"
        );
        fs::write(&path, native).unwrap();
        crate::store::upsert_stored_account(
            dir.path(),
            crate::store::StoredAccount {
                id: "synthetic-title-account".into(),
                provider: ProviderKind::Codex,
                home_path: Some(home.to_string_lossy().into_owned()),
                label: "Synthetic".into(),
                created_at: "0".into(),
            },
        )
        .unwrap();
        crate::session_titles::set_if_absent(
            dir.path(),
            ProviderKind::Codex,
            "synthetic-title-account",
            &home,
            "synthetic-session",
            "Saved automatic title",
        )
        .unwrap();
        let listing = list_native_records_bounded(
            dir.path(),
            10,
            Some("synthetic-title-account"),
            Some(ProviderKind::Codex),
        );
        assert_eq!(listing.records.len(), 1);
        assert_eq!(
            listing.records[0].0.preview.as_deref(),
            Some("Saved automatic title")
        );
        let found = find_native_record(
            dir.path(),
            "synthetic-title-account",
            Some(ProviderKind::Codex),
            "synthetic-session",
        )
        .unwrap()
        .unwrap();
        assert_eq!(found.0.preview.as_deref(), Some("Saved automatic title"));
        assert_eq!(fs::read_to_string(&path).unwrap(), native);
        // Native agents can inherit the entire parent's opening history while
        // living in the same sessions directory. Home must list user threads,
        // while exact source lookup must still find the child's original file.
        let child = home.join("sessions/rollout-child.jsonl");
        fs::write(&child, concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"synthetic-child\",\"thread_source\":\"subagent\",\"source\":{\"subagent\":{}},\"parent_thread_id\":\"synthetic-session\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"text\":\"Native request\"}]}}\n"
        )).unwrap();
        let metadata = serde_json::json!({"type":"session_meta","payload":{
            "id":"large-child", "thread_source":"subagent", "base_instructions":"x".repeat(40 * 1024)
        }});
        fs::write(
            home.join("sessions/rollout-large-child.jsonl"),
            format!("{metadata}\n"),
        )
        .unwrap();
        fs::write(home.join("sessions/rollout-copy.jsonl"), native).unwrap();
        fs::write(home.join("sessions/rollout-fork.jsonl"), native.replace(
            "\"id\":\"synthetic-session\"",
            "\"id\":\"independent-fork\",\"forked_from_id\":\"synthetic-session\",\"thread_source\":\"user\"",
        )).unwrap();
        for _ in 0..2 {
            let recent = scan_native_records(
                dir.path(),
                10,
                Some("synthetic-title-account"),
                Some(ProviderKind::Codex),
                false,
            );
            let mut ids: Vec<_> = recent
                .records
                .iter()
                .map(|(summary, _)| summary.session_id.as_str())
                .collect();
            ids.sort_unstable();
            assert_eq!(
                ids,
                ["independent-fork", "synthetic-session"],
                "refreshes deduplicate the same native identity and preserve user forks"
            );
        }
        let child_source = find_native_record(
            dir.path(),
            "synthetic-title-account",
            Some(ProviderKind::Codex),
            "synthetic-child",
        )
        .unwrap()
        .unwrap();
        assert_eq!(child_source.1, child);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            native,
            "Home filtering never edits provider history"
        );
    }
}
