//! Small deterministic titles, separate from optional transcript logging and provider files.
use crate::store::{store_dir, ProviderKind};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_TITLE_CHARS: usize = 80;
const MAX_RECORD_BYTES: u64 = 32 * 1024;

fn clean(raw: &str) -> String {
    let mut plain = String::new();
    let mut chars = raw.chars().take(64 * 1024).peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
        } else {
            plain.push(c);
        }
    }
    let safe: String = plain.chars().filter(|c| !c.is_control() || c.is_whitespace())
        .filter(|c| !matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}'))
        .collect();
    let text = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= MAX_TITLE_CHARS {
        return text;
    }
    let mut title: String = text.chars().take(MAX_TITLE_CHARS - 1).collect();
    title.push('…');
    title
}

/// First substantive request, without inference or a provider call.
pub fn title_from_prompt(prompt: &str) -> String {
    if prompt.trim_start().starts_with("# AGENTS.md") {
        return String::new();
    }
    // Bound work even when a pasted prompt is enormous.
    let mut text: String = crate::prompt_time::strip_header(prompt)
        .chars()
        .take(64 * 1024)
        .collect();
    for tag in [
        "environment_context",
        "task-notification",
        "system-reminder",
        "system",
        "developer",
        "instructions",
        "context",
        "concordia_context",
    ] {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        while let Some(start) = text.find(&open) {
            let Some(end) = text[start + open.len()..].find(&close) else {
                text.truncate(start);
                break;
            };
            text.replace_range(start..start + open.len() + end + close.len(), " ");
        }
    }
    let request = text
        .lines()
        .map(str::trim)
        .find(|line| {
            !line.is_empty()
                && !line.starts_with('<')
                && !line.starts_with("[concordia map]")
                && !line.starts_with("more:")
                && !line.starts_with("asked:")
                && !line.starts_with("# AGENTS.md")
                && !line.starts_with("```")
        })
        .unwrap_or("");
    clean(request.trim_start_matches(['#', ' ', '\t']))
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Identity {
    provider: ProviderKind,
    account: String,
    home: String,
    session_id: String,
}

impl Identity {
    fn new(provider: ProviderKind, account: &str, home: &Path, session_id: &str) -> Option<Self> {
        if account.is_empty() || session_id.is_empty() || home.as_os_str().is_empty() {
            return None;
        }
        let normalized_path: PathBuf = home.components().collect();
        let mut normalized = normalized_path.to_string_lossy().replace('\\', "/");
        // Windows homes are case insensitive; preserve case on case-sensitive hosts.
        if cfg!(windows) {
            normalized.make_ascii_lowercase();
        }
        while normalized.ends_with('/') && normalized.len() > 1 {
            normalized.pop();
        }
        Some(Self {
            provider,
            account: account.into(),
            home: normalized,
            session_id: session_id.into(),
        })
    }

    fn path(&self, root: &Path) -> PathBuf {
        // Stable filename hash is only an index. Reads also verify every identity field.
        let encoded = serde_json::to_vec(self).expect("string identity serializes");
        let hash = encoded.iter().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        });
        store_dir(root)
            .join("session-titles")
            .join(format!("{hash:016x}.json"))
    }
}

#[derive(Serialize, Deserialize)]
struct Record {
    identity: Identity,
    title: String,
}

fn read(path: &Path) -> Option<Record> {
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_RECORD_BYTES {
        return None;
    }
    let mut raw = String::new();
    file.take(MAX_RECORD_BYTES).read_to_string(&mut raw).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn get(
    root: &Path,
    provider: ProviderKind,
    account: &str,
    home: &Path,
    session_id: &str,
) -> Option<String> {
    let identity = Identity::new(provider, account, home, session_id)?;
    let record = read(&identity.path(root))?;
    if record.identity != identity {
        return None;
    }
    let title = clean(&record.title);
    (!title.is_empty()).then_some(title)
}

/// Atomically publish a complete record without replacing the first writer's title.
pub fn set_if_absent(
    root: &Path,
    provider: ProviderKind,
    account: &str,
    home: &Path,
    session_id: &str,
    title: &str,
) -> Result<(), String> {
    let identity = Identity::new(provider, account, home, session_id)
        .ok_or("title requires an explicit session identity")?;
    let title = clean(title);
    if title.is_empty() {
        return Err("session title is empty".into());
    }
    let path = identity.path(root);
    if path.exists() {
        return match read(&path) {
            Some(record) if record.identity == identity => Ok(()),
            _ => Err("session title record is invalid or belongs to another identity".into()),
        };
    }
    let bytes = serde_json::to_vec(&Record { identity, title }).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err("session title identity is too large".into());
    }
    let dir = path.parent().ok_or("session title directory is missing")?;
    fs::create_dir_all(dir).map_err(|e| format!("create session title directory: {e}"))?;
    let temporary = dir.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        match fs::hard_link(&temporary, &path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if get(root, provider, account, home, session_id).is_some() {
                    Ok(())
                } else {
                    Err("session title identity collision or invalid record".into())
                }
            }
            Err(e) => Err(format!("publish session title: {e}")),
        }
    })();
    let _ = fs::remove_file(temporary);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_strip_context_and_sanitize_unicode_controls() {
        assert_eq!(title_from_prompt("<environment_context>\nprivate setup\n</environment_context>\nFix the resume picker\nextra detail"), "Fix the resume picker");
        let title = title_from_prompt(&format!(
            "\u{1b}[31mFix\u{1b}[0m\t日本語\u{202e} {}",
            "🦀".repeat(100)
        ));
        assert!(title.starts_with("Fix 日本語 "));
        assert_eq!(title.chars().count(), 80);
        assert!(title.ends_with('…'));
        assert!(!title.contains('\u{202e}'));
        assert_eq!(title_from_prompt("<context>setup</context>"), "");
        assert_eq!(
            title_from_prompt("# AGENTS.md instructions\nRead PLAN.md"),
            ""
        );
    }

    #[test]
    fn persisted_title_survives_restart_and_first_title_wins() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("synthetic-home");
        set_if_absent(
            dir.path(),
            ProviderKind::Codex,
            "synthetic",
            &home,
            "session",
            "Fix resume",
        )
        .unwrap();
        set_if_absent(
            dir.path(),
            ProviderKind::Codex,
            "synthetic",
            &home,
            "session",
            "Unrelated follow-up",
        )
        .unwrap();
        assert_eq!(
            get(
                dir.path(),
                ProviderKind::Codex,
                "synthetic",
                &home,
                "session"
            )
            .as_deref(),
            Some("Fix resume")
        );
        let records: Vec<_> = fs::read_dir(store_dir(dir.path()).join("session-titles"))
            .unwrap()
            .collect();
        assert_eq!(records.len(), 1);
        assert!(get(
            dir.path(),
            ProviderKind::Claude,
            "synthetic",
            &home,
            "session"
        )
        .is_none());
        assert!(get(dir.path(), ProviderKind::Codex, "other", &home, "session").is_none());
        assert!(get(
            dir.path(),
            ProviderKind::Codex,
            "synthetic",
            &home.join("other"),
            "session"
        )
        .is_none());
        assert!(get(dir.path(), ProviderKind::Codex, "synthetic", &home, "other").is_none());
    }

    #[test]
    fn concurrent_writers_publish_one_complete_record() {
        let dir = tempfile::tempdir().unwrap();
        std::thread::scope(|scope| {
            for title in ["First title", "Second title", "Third title"] {
                let root = dir.path();
                scope.spawn(move || {
                    set_if_absent(
                        root,
                        ProviderKind::Codex,
                        "synthetic",
                        Path::new("synthetic-home"),
                        "session",
                        title,
                    )
                    .unwrap()
                });
            }
        });
        let title = get(
            dir.path(),
            ProviderKind::Codex,
            "synthetic",
            Path::new("synthetic-home"),
            "session",
        )
        .unwrap();
        assert!(["First title", "Second title", "Third title"].contains(&title.as_str()));
        assert_eq!(
            fs::read_dir(store_dir(dir.path()).join("session-titles"))
                .unwrap()
                .count(),
            1
        );
    }
}
