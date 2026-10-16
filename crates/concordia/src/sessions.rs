//! Resume picker — list Codex / Claude / Cursor sessions from account homes.
//! Never uses resume --last; callers bind an explicit session id.

use crate::accounts::{display_account_name, list_all_accounts};
use crate::store::ProviderKind;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

fn walk_jsonl(root: &Path, keep: &dyn Fn(&Path, &str) -> bool) -> Vec<(PathBuf, u128)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
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
    found
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
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 36 <= bytes.len() {
        let slice = &s[i..i + 36];
        if is_uuid(slice) {
            return Some(slice.to_string());
        }
        i += 1;
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

fn discover_codex(home: &Path) -> Vec<(PathBuf, u128)> {
    let sessions = home.join("sessions");
    if !sessions.is_dir() {
        return Vec::new();
    }
    walk_jsonl(&sessions, &|_, _| true)
}

fn discover_claude(home: &Path) -> Vec<(PathBuf, u128)> {
    let projects = home.join("projects");
    if !projects.is_dir() {
        return Vec::new();
    }
    walk_jsonl(&projects, &|path, name| {
        if name.starts_with("agent-") {
            return false;
        }
        let p = path.to_string_lossy();
        !p.contains("subagents")
    })
}

fn discover_cursor(home: &Path) -> Vec<(PathBuf, u128)> {
    let chats = home.join("chats");
    if !chats.is_dir() {
        return Vec::new();
    }
    let mut found = Vec::new();
    let Ok(projects) = fs::read_dir(&chats) else {
        return found;
    };
    for project in projects.flatten() {
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
            let dir = chat.path();
            let meta = dir.join("meta.json");
            if meta.is_file() {
                found.push((dir, mtime_ms(&meta)));
            }
        }
    }
    found
}

fn summarize_codex(account: &str, path: &Path, modified_ms: u128) -> Option<SessionSummary> {
    let mut session_id = session_id_from_filename(path);
    let mut cwd: Option<String> = None;
    let mut preview: Option<String> = None;
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut n = 0usize;
    for line in reader.lines().map_while(Result::ok) {
        n += 1;
        if n > 80 && preview.is_some() {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
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
                            preview = Some(text);
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
    let reader = BufReader::new(file);
    let mut n = 0usize;
    for line in reader.lines().map_while(Result::ok) {
        n += 1;
        if n > 120 && (preview.is_some() || n > 200) {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
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
        preview,
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

fn summarize_cursor(account: &str, dir: &Path, modified_ms: u128) -> Option<SessionSummary> {
    let meta_path = dir.join("meta.json");
    let raw = fs::read_to_string(&meta_path).ok()?;
    let meta: Value = serde_json::from_str(&raw).ok()?;
    let session_id = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cwd = meta
        .get("cwd")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());
    let mut preview = meta
        .get("title")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());
    let hist_path = dir.join("prompt_history.json");
    if preview.as_ref().map(|p| p.len() < 3).unwrap_or(true) && hist_path.is_file() {
        if let Ok(hist) = fs::read_to_string(&hist_path) {
            if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&hist) {
                for item in &items {
                    if let Some(t) = item.as_str().map(str::trim) {
                        if !t.is_empty() && !t.starts_with('/') {
                            preview = Some(t.to_string());
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

pub fn list_recent_sessions(state_root: &Path, limit: usize) -> Vec<SessionSummary> {
    let limit = limit.clamp(1, 100);
    let accounts = list_all_accounts(state_root);
    let mut discovered: Vec<(String, ProviderKind, PathBuf, u128, &'static str)> = Vec::new();
    for a in &accounts {
        let Some(home) = a.home_path.as_ref() else {
            continue;
        };
        match a.provider {
            ProviderKind::Codex => {
                for (path, ms) in discover_codex(home) {
                    discovered.push((a.account.clone(), a.provider, path, ms, "codex"));
                }
            }
            ProviderKind::Claude => {
                for (path, ms) in discover_claude(home) {
                    discovered.push((a.account.clone(), a.provider, path, ms, "claude"));
                }
            }
            ProviderKind::Cursor => {
                for (path, ms) in discover_cursor(home) {
                    discovered.push((a.account.clone(), a.provider, path, ms, "cursor"));
                }
            }
        }
    }
    discovered.sort_by_key(|d| std::cmp::Reverse(d.3));
    let mut out = Vec::new();
    for (account, _provider, path, ms, kind) in discovered.into_iter().take(limit * 3) {
        let summary = match kind {
            "codex" => summarize_codex(&account, &path, ms),
            "claude" => summarize_claude(&account, &path, ms),
            _ => summarize_cursor(&account, &path, ms),
        };
        if let Some(s) = summary {
            out.push(s);
            if out.len() >= limit {
                break;
            }
        }
    }
    out
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
}
