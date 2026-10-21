//! Resume picker — list Codex / Claude / Cursor sessions from account homes.
//! Never uses resume --last; callers bind an explicit session id.

use crate::accounts::{display_account_name, list_all_accounts};
use crate::store::ProviderKind;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_DISCOVERY_ENTRIES: usize = 20_000;
const MAX_DISCOVERED_FILES: usize = 5_000;
const MAX_DISCOVERED_ACCOUNTS: usize = 32;
const MAX_SUMMARY_BYTES: u64 = 64 * 1024;
const MAX_SUMMARY_LINE_BYTES: usize = 32 * 1024;
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

fn read_summary_lines(file: File, max_lines: usize) -> Vec<Vec<u8>> {
    let mut reader = BufReader::new(file.take(MAX_SUMMARY_BYTES));
    let mut lines = Vec::new();
    while lines.len() < max_lines {
        let mut line = Vec::new();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) if line.len() <= MAX_SUMMARY_LINE_BYTES => lines.push(line),
            Ok(_) => continue,
        }
    }
    lines
}

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub account: String,
    pub provider: ProviderKind,
    pub session_id: String,
    pub cwd: Option<String>,
    pub preview: Option<String>,
    pub modified_ms: u128,
}

#[derive(Debug, Clone)]
pub struct PickerRow {
    pub id: String,
    pub label: String,
    pub detail: String,
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

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
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
    // rollout-<uuid>.jsonl
    if let Some(idx) = name.rfind('-') {
        let maybe = &name[idx + 1..];
        if maybe.len() >= 32 {
            // full uuid might still have dashes in stem
        }
    }
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
    let mut session_id = session_id_from_filename(path);
    let mut cwd: Option<String> = None;
    let mut preview: Option<String> = None;
    let file = File::open(path).ok()?;
    for line in read_summary_lines(file, 80) {
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
        if preview.is_none() && ty == "response_item" {
            if let Some(p) = v.get("payload") {
                if p.get("type").and_then(|t| t.as_str()) == Some("message")
                    && p.get("role").and_then(|t| t.as_str()) == Some("user")
                {
                    if let Some(arr) = p.get("content").and_then(|c| c.as_array()) {
                        let text: String = arr
                            .iter()
                            .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join(" ")
                            .trim()
                            .to_string();
                        if !text.is_empty() {
                            preview = Some(bounded_text(&text, 512));
                        }
                    }
                }
            }
        }
    }
    Some(SessionSummary {
        account: account.to_string(),
        provider: ProviderKind::Codex,
        session_id,
        cwd,
        preview,
        modified_ms,
    })
}

fn summarize_claude(account: &str, path: &Path, modified_ms: u128) -> Option<SessionSummary> {
    let mut session_id = session_id_from_filename(path);
    let mut cwd = decode_claude_project_cwd(path);
    let mut preview: Option<String> = None;
    let file = File::open(path).ok()?;
    for line in read_summary_lines(file, MAX_SUMMARY_LINES) {
        let Ok(v) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if let Some(id) = v.get("sessionId").and_then(|x| x.as_str()) {
            session_id = id.to_string();
        }
        if let Some(c) = v.get("cwd").and_then(|x| x.as_str()) {
            cwd = Some(c.to_string());
        }
        if preview.is_none() && v.get("type").and_then(|t| t.as_str()) == Some("user") {
            if v.get("isMeta").and_then(|m| m.as_bool()).unwrap_or(false) {
                continue;
            }
            if let Some(text) = claude_user_text(&v) {
                preview = Some(text);
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
    if preview.as_ref().map(|p| p.len() < 3).unwrap_or(true) && hist_path.is_file() {
        if let Ok(hist) = read_file_capped(&hist_path, 32 * 1024) {
            if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&hist) {
                for item in &items {
                    if let Some(t) = item.as_str().map(str::trim) {
                        if !t.is_empty() && !t.starts_with('/') {
                            preview = Some(bounded_text(t, 512));
                            break;
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
    let limit = limit.clamp(1, 1000);
    let accounts: Vec<_> = list_all_accounts(state_root)
        .into_iter()
        .filter(|a| account.is_none_or(|id| a.account == id))
        .filter(|a| provider.is_none_or(|p| a.provider == p))
        .collect();
    let mut records = Vec::new();
    let mut candidates: Vec<(String, ProviderKind, PathBuf, u128)> = Vec::new();
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
                .map(|(path, ms)| (a.account.clone(), a.provider, path, ms)),
        );
    }
    candidates.sort_by_key(|(_, _, _, ms)| std::cmp::Reverse(*ms));
    for (index, (account, provider, path, ms)) in candidates.iter().enumerate() {
        if records.len() >= limit {
            break;
        }
        if index >= MAX_SUMMARIZED_RECORDS {
            truncated = true;
            break;
        }
        if let Some(record) = summarize_one(account, *provider, path, *ms) {
            records.push(record);
        }
    }
    NativeRecordScan { records, truncated }
}

pub fn list_native_records(state_root: &Path, limit: usize) -> Vec<(SessionSummary, PathBuf)> {
    list_native_records_bounded(state_root, limit, None, None).records
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
        if let Some(record) = summarize_one(&account_def.account, account_def.provider, &path, ms) {
            if record.0.session_id == session_id {
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
    list_native_records(state_root, limit.clamp(1, 100))
        .into_iter()
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

pub fn sessions_to_picker_rows(
    sessions: &[SessionSummary],
    prefer_account: Option<&str>,
    prefer_cwd: Option<&Path>,
) -> Vec<PickerRow> {
    let prefer_cwd_norm = prefer_cwd.map(|p| normalize_path(&p.to_string_lossy()));
    let prefer_folder = prefer_cwd.map(|p| basename(&p.to_string_lossy()));
    let mut sorted: Vec<&SessionSummary> = sessions.iter().collect();
    sorted.sort_by(|a, b| {
        if let Some(ref norm) = prefer_cwd_norm {
            let af = a
                .cwd
                .as_ref()
                .map(|c| normalize_path(c) == *norm)
                .unwrap_or(false);
            let bf = b
                .cwd
                .as_ref()
                .map(|c| normalize_path(c) == *norm)
                .unwrap_or(false);
            match (af, bf) {
                (true, false) => return std::cmp::Ordering::Less,
                (false, true) => return std::cmp::Ordering::Greater,
                _ => {}
            }
        } else if let Some(ref folder) = prefer_folder {
            let af = a
                .cwd
                .as_ref()
                .map(|c| basename(c) == *folder)
                .unwrap_or(false);
            let bf = b
                .cwd
                .as_ref()
                .map(|c| basename(c) == *folder)
                .unwrap_or(false);
            match (af, bf) {
                (true, false) => return std::cmp::Ordering::Less,
                (false, true) => return std::cmp::Ordering::Greater,
                _ => {}
            }
        }
        if let Some(pref) = prefer_account {
            let ap = a.account == pref;
            let bp = b.account == pref;
            match (ap, bp) {
                (true, false) => return std::cmp::Ordering::Less,
                (false, true) => return std::cmp::Ordering::Greater,
                _ => {}
            }
        }
        b.modified_ms.cmp(&a.modified_ms)
    });
    let now = now_ms();
    sorted
        .into_iter()
        .map(|s| {
            let when = format_relative_time(s.modified_ms, now);
            let folder = s
                .cwd
                .as_deref()
                .map(basename)
                .unwrap_or_else(|| "no folder".into());
            let account = display_account_name(s.provider, &s.account);
            let label = clean_preview(s.preview.as_deref(), 56);
            PickerRow {
                id: format!("resume:{}:{}", s.account, s.session_id),
                label,
                detail: format!("{account} · {when} · {folder}"),
            }
        })
        .collect()
}

pub fn trust_picker_rows(current: crate::store::TrustMode) -> Vec<PickerRow> {
    use crate::store::TrustMode;
    let cur = |m: TrustMode| if current == m { " · current" } else { "" };
    vec![
        PickerRow {
            id: "trust:plan".into(),
            label: format!("plan{}", cur(TrustMode::Plan)),
            detail: "read-only: analyse and propose, no edits".into(),
        },
        PickerRow {
            id: "trust:ask".into(),
            label: format!("ask{}", cur(TrustMode::Ask)),
            detail: "fail closed — may hang on trust dialogs".into(),
        },
        PickerRow {
            id: "trust:workspace".into(),
            label: format!("workspace{}", cur(TrustMode::Workspace)),
            detail: "cwd write + skip git-trust (recommended)".into(),
        },
        PickerRow {
            id: "trust:full".into(),
            label: format!("full{}", cur(TrustMode::Full)),
            detail: "skip all permissions — dangerous".into(),
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
        assert_eq!(summary.preview.as_ref().unwrap().chars().count(), 512);
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
}
