//! Thread log and map: an opt-in, append-only record of a conversation, and deterministic
//! pointer maps (no model, no network). See docs/architecture.md ("Thread log", "Map").

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[path = "thread_references.rs"]
mod references;
use crate::native_memory::references::{CaptureScope, Reference};

/// Per-event text cap, in characters.
pub const MAX_EVENT_TEXT: usize = 20_000;
/// Default map budget, in characters.
pub const MAP_CHARS: usize = 1600;
/// Default cap for `concordia thread read`.
pub const READ_CHARS: usize = 6000;
/// Maximum number of locally pinned native spans retained for one thread.
pub const MAX_PINNED_SPANS: usize = 128;
/// Maximum UTF-8 bytes retained by one thread's pin journal.
pub const MAX_PINNED_BYTES: usize = 256 * 1024;

// ---------------------------------------------------------------------------------------------
// Opt-in switch: the env var, the store setting, or `[threads] enabled` in `<state>/config.toml`.
// ---------------------------------------------------------------------------------------------
pub fn enabled(state_root: &Path) -> bool {
    if std::env::var("CONCORDIA_THREADS").is_ok_and(|v| v == "1") {
        return true;
    }
    crate::store::get_settings(state_root).thread_log
        || crate::config::for_root(state_root).threads.enabled
}

/// A new thread log for a conversation, or None when recording is off.
pub fn start(state_root: &Path) -> Option<ThreadLog> {
    enabled(state_root).then(|| ThreadLog::open(state_root, &uuid::Uuid::new_v4().to_string()))
}

pub fn map_chars_default() -> usize {
    crate::config::threads_map_chars()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EventCard {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EventUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// One line of `<thread-id>.jsonl`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ThreadEvent {
    /// Unix seconds (0 when unknown, e.g. rebuilt from memory).
    #[serde(default)]
    pub t: u64,
    /// `user`, `assistant`, `card`, `switch`, `note`, `usage`.
    pub k: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<EventCard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<EventUsage>,
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Cut to at most `max` characters on a char boundary.
fn cap_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

impl ThreadEvent {
    pub fn new(k: &str) -> Self {
        ThreadEvent {
            t: now_secs(),
            k: k.to_string(),
            ..Default::default()
        }
    }

    pub fn with_text(k: &str, text: &str) -> Self {
        let mut e = Self::new(k);
        e.text = Some(cap_chars(text, MAX_EVENT_TEXT).to_string());
        e
    }

    pub fn who(mut self, provider: &str, account: &str, model: &str) -> Self {
        self.provider = Some(provider.to_string()).filter(|s| !s.is_empty());
        self.account = Some(account.to_string()).filter(|s| !s.is_empty());
        self.model = Some(model.to_string()).filter(|s| !s.is_empty() && s != "?");
        self
    }

    pub fn card(title: &str, detail: Option<&str>, status: &str) -> Self {
        let mut e = Self::new("card");
        e.card = Some(EventCard {
            title: crate::redact::redact(title).into_owned(),
            detail: detail
                .map(|d| cap_chars(&crate::redact::redact(d), MAX_EVENT_TEXT).to_string()),
            status: Some(status.to_string()),
        });
        e
    }

    fn text(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadMeta {
    pub id: String,
    /// Unix seconds.
    pub started: u64,
    pub title: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

pub fn threads_dir(state_root: &Path) -> PathBuf {
    state_root.join("threads")
}

fn safe_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// First prompt, collapsed to one line and cut to 60 chars.
pub fn title_from_prompt(prompt: &str) -> String {
    one_line(prompt, 60)
}

pub struct ThreadLog {
    id: String,
    dir: PathBuf,
}

/// A redacted copy of an exact native span. Pins are deliberately independent of provider
/// transcript files, so a map remains readable after native pruning or compaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinnedSpan {
    pub version: u32,
    pub ordinal: usize,
    pub label: String,
    pub role: String,
    pub provider: Option<String>,
    pub account: Option<String>,
    pub native_session_id: Option<String>,
    pub text: String,
}

impl ThreadLog {
    /// No I/O until the first write.
    pub fn open(state_root: &Path, thread_id: &str) -> ThreadLog {
        ThreadLog {
            id: safe_id(thread_id),
            dir: threads_dir(state_root),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    fn log_path(&self) -> PathBuf {
        self.dir.join(format!("{}.jsonl", self.id))
    }

    fn meta_path(&self) -> PathBuf {
        self.dir.join(format!("{}.meta.json", self.id))
    }

    fn pins_path(&self) -> PathBuf {
        self.dir.join(format!("{}.pins.jsonl", self.id))
    }

    /// Append structural metadata only. Conversation bodies require an exact native reference.
    pub fn append(&self, ev: &ThreadEvent) -> Result<()> {
        self.append_record(&references::Record::metadata(ev)?)
    }

    /// Verify an immutable native projection before persisting its body-free pointer.
    pub fn append_referenced(&self, ev: &ThreadEvent, reference: Reference) -> Result<()> {
        let record = references::Record::referenced(self.root(), ev, reference.clone())?;
        let ordinal = self.read_record_count().saturating_add(1);
        self.append_record(&record)?;
        let role = if ev.k == "card" {
            "tool"
        } else {
            ev.k.as_str()
        };
        let text = crate::native_memory::references::hydrate_text(self.root(), &reference)
            .map_err(|error| anyhow!("native span pin failed: {error}"))?;
        self.pin_span_with_reference(
            ordinal,
            role,
            &format!("{} turn {}", role, ordinal),
            &text,
            &reference,
        )
    }

    fn read_record_count(&self) -> usize {
        std::fs::read_to_string(self.log_path())
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    /// Pin a span after applying the same deterministic redaction used by the journal.
    pub fn pin_span(&self, ordinal: usize, label: &str, text: &str) -> Result<()> {
        self.pin_span_inner(PinnedSpan {
            version: 1,
            ordinal,
            label: cap_chars(label, 160).to_owned(),
            role: "native".into(),
            provider: None,
            account: None,
            native_session_id: None,
            text: cap_chars(&crate::redact::redact(text), MAX_PINNED_BYTES).to_owned(),
        })
    }

    fn pin_span_with_reference(
        &self,
        ordinal: usize,
        role: &str,
        label: &str,
        text: &str,
        reference: &Reference,
    ) -> Result<()> {
        let binding = reference.binding();
        self.pin_span_inner(PinnedSpan {
            version: 1,
            ordinal,
            label: cap_chars(label, 160).to_owned(),
            role: role.to_owned(),
            provider: Some(binding.provider.as_str().to_owned()),
            account: Some(binding.account.clone()),
            native_session_id: Some(binding.session_id.clone()),
            text: cap_chars(&crate::redact::redact(text), MAX_PINNED_BYTES).to_owned(),
        })
    }

    fn pin_span_inner(&self, span: PinnedSpan) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let lock = self.dir.join(format!("{}.pins.lock", self.id));
        crate::state_io::with_lock(&lock, || {
            let mut spans = self.read_pins_unchecked();
            spans.retain(|old| old.ordinal != span.ordinal);
            spans.push(span.clone());
            spans.sort_by_key(|old| old.ordinal);
            while spans.len() > MAX_PINNED_SPANS
                || serde_json::to_vec(&spans).map_or(true, |v| v.len() > MAX_PINNED_BYTES)
            {
                if spans.is_empty() {
                    break;
                }
                spans.remove(0);
            }
            let body = spans
                .iter()
                .map(serde_json::to_string)
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");
            let body = if body.is_empty() {
                body
            } else {
                format!("{body}\n")
            };
            std::fs::write(self.pins_path(), crate::redact::redact(&body).as_bytes())?;
            Ok(())
        })?;
        Ok(())
    }

    fn read_pins_unchecked(&self) -> Vec<PinnedSpan> {
        std::fs::read_to_string(self.pins_path())
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// Resolve a map ordinal from the local pin journal, falling back to the native-backed log.
    pub fn resolve_span(&self, ordinal: usize) -> Result<String> {
        if let Some(span) = self
            .read_pins_unchecked()
            .into_iter()
            .find(|s| s.ordinal == ordinal)
        {
            return Ok(span.text);
        }
        self.read_all()?
            .get(ordinal.saturating_sub(1))
            .map(|event| event.text().to_owned())
            .ok_or_else(|| anyhow!("thread span {ordinal} is unavailable"))
    }

    /// Record a source gap without persisting the original text or an arbitrary error body.
    pub fn append_unavailable(
        &self,
        ev: &ThreadEvent,
        scope: Option<&CaptureScope>,
        reason: &str,
    ) -> Result<()> {
        self.append_record(&references::Record::unavailable(ev, scope, reason)?)
    }

    fn root(&self) -> &Path {
        self.dir
            .parent()
            .expect("thread directory has a state root")
    }

    /// One bounded cross-process append transaction per journal record.
    fn append_record(&self, record: &references::Record) -> Result<()> {
        let mut line = crate::redact::redact(&serde_json::to_string(record)?).into_owned();
        line.push('\n');
        std::fs::create_dir_all(&self.dir)?;
        let root = crate::update::resolved_directory(&self.dir)
            .ok_or_else(|| anyhow!("thread directory is not regular"))?;
        let path = self.log_path();
        let lock = self.dir.join(format!("{}.lock", self.id));
        if [path.as_path(), lock.as_path()]
            .into_iter()
            .any(|path| path.exists() && !crate::update::regular_owned_file(path, &root))
        {
            bail!("thread journal or lock is not a regular owned file");
        }
        crate::state_io::with_lock(&lock, || {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
            f.write_all(line.as_bytes())?;
            f.flush()
        })?;
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<ThreadEvent>> {
        read_events(self.root(), &self.log_path())
    }

    pub fn write_meta(&self, meta: &ThreadMeta) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let root = crate::update::resolved_directory(&self.dir)
            .ok_or_else(|| anyhow!("thread directory is not regular"))?;
        if self.meta_path().exists() && !crate::update::regular_owned_file(&self.meta_path(), &root)
        {
            bail!("thread metadata is not a regular owned file");
        }
        let mut meta = meta.clone();
        meta.id = self.id.clone();
        meta.title = cap_chars(&meta.title, 160).to_owned();
        meta.cwd = cap_chars(&meta.cwd, 4096).to_owned();
        meta.account = meta
            .account
            .as_deref()
            .map(|a| cap_chars(a, 128).to_owned());
        let encoded = serde_json::to_string(&meta)?;
        std::fs::write(self.meta_path(), crate::redact::redact(&encoded).as_bytes())?;
        Ok(())
    }

    pub fn meta_exists(&self) -> bool {
        self.meta_path().exists()
    }

    pub fn read_meta(&self) -> Option<ThreadMeta> {
        let root = crate::update::resolved_directory(&self.dir)?;
        if !crate::update::regular_owned_file(&self.meta_path(), &root) {
            return None;
        }
        let s = std::fs::read_to_string(self.meta_path()).ok()?;
        serde_json::from_str(&s).ok()
    }

    pub fn delete(&self) -> Result<()> {
        for p in [self.log_path(), self.meta_path()] {
            match std::fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("remove {}", p.display())),
            }
        }
        Ok(())
    }
}

fn read_events(root: &Path, path: &Path) -> Result<Vec<ThreadEvent>> {
    let directory = path
        .parent()
        .and_then(crate::update::resolved_directory)
        .ok_or_else(|| anyhow!("thread directory unavailable"))?;
    if !crate::update::regular_owned_file(path, &directory) {
        bail!("thread journal is not a regular owned file");
    }
    let s = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut events = Vec::new();
    for (ordinal, line) in s.lines().enumerate() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("version").is_some() {
            let record: references::Record = serde_json::from_value(value)
                .with_context(|| format!("invalid referenced thread event {}", ordinal + 1))?;
            events.push(record.hydrate(root)?);
        } else if let Ok(event) = serde_json::from_value::<ThreadEvent>(value) {
            // Legacy body-bearing lines remain readable, without rewriting or migrating them.
            events.push(event);
        }
    }
    Ok(events)
}

/// Convert one explicitly selected legacy message using the existing referenced-record format.
pub(crate) fn referenced_legacy_line(
    root: &Path,
    event: &ThreadEvent,
    source: Reference,
) -> std::result::Result<String, String> {
    let record =
        references::Record::referenced(root, event, source).map_err(|error| error.to_string())?;
    serde_json::to_string(&record).map_err(|error| error.to_string())
}

fn thread_ids(state_root: &Path) -> Vec<String> {
    let mut ids = Vec::new();
    let Ok(rd) = std::fs::read_dir(threads_dir(state_root)) else {
        return ids;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(id) = name
            .strip_suffix(".jsonl")
            .filter(|id| !id.ends_with(".pins"))
        {
            ids.push(id.to_string());
        }
    }
    ids
}

/// Recorded threads, newest first. A log without a meta file still lists (title empty).
pub fn list(state_root: &Path) -> Vec<ThreadMeta> {
    let mut out: Vec<ThreadMeta> = thread_ids(state_root)
        .into_iter()
        .map(|id| {
            let log = ThreadLog::open(state_root, &id);
            log.read_meta().unwrap_or_else(|| ThreadMeta {
                started: std::fs::metadata(log.log_path())
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                id,
                title: String::new(),
                cwd: String::new(),
                account: None,
            })
        })
        .collect();
    out.sort_by(|a, b| b.started.cmp(&a.started).then_with(|| a.id.cmp(&b.id)));
    out
}

pub fn delete(state_root: &Path, id: &str) -> Result<()> {
    ThreadLog::open(state_root, id).delete()
}

/// Full id from an exact id or an unambiguous prefix.
pub fn resolve_prefix(state_root: &Path, prefix: &str) -> Result<String> {
    let prefix = prefix.trim();
    if prefix.is_empty() {
        bail!("empty thread id");
    }
    let ids = thread_ids(state_root);
    if ids.iter().any(|i| i == prefix) {
        return Ok(prefix.to_string());
    }
    let hits: Vec<&String> = ids.iter().filter(|i| i.starts_with(prefix)).collect();
    match hits.len() {
        0 => Err(anyhow!(
            "no thread matches '{prefix}' (try: concordia thread list)"
        )),
        1 => Ok(hits[0].clone()),
        n => Err(anyhow!(
            "'{prefix}' is ambiguous ({n} threads match); use more characters"
        )),
    }
}

// ---------------------------------------------------------------------------------------------
// Map
// ---------------------------------------------------------------------------------------------

fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    if max <= 3 {
        return cap_chars(&flat, max).to_string();
    }
    format!("{}...", cap_chars(&flat, max - 3))
}

/// Last `max` chars of `s` (whitespace collapsed), on a char boundary.
fn tail_chars(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = flat.chars().count();
    if n <= max {
        return flat;
    }
    if max <= 3 {
        return flat.chars().skip(n - max).collect();
    }
    format!(
        "...{}",
        flat.chars().skip(n - (max - 3)).collect::<String>()
    )
}

fn looks_like_path(d: &str) -> bool {
    let d = d.trim();
    if d.is_empty() || d.chars().count() > 200 || d.contains(char::is_whitespace) {
        return false;
    }
    d.contains('/')
        || d.contains('\\')
        || d.rsplit_once('.').is_some_and(|(a, b)| {
            !a.is_empty()
                && !b.is_empty()
                && b.len() <= 5
                && b.chars().all(|c| c.is_ascii_alphanumeric())
        })
}

fn is_file_tool(title: &str) -> bool {
    let t = title.to_ascii_lowercase();
    [
        "edit", "write", "read", "patch", "update", "create", "notebook",
    ]
    .iter()
    .any(|k| t.contains(k))
}

fn is_shell_tool(title: &str) -> bool {
    let t = title.to_ascii_lowercase();
    [
        "bash",
        "shell",
        "exec",
        "command",
        "powershell",
        "terminal",
        "run",
    ]
    .iter()
    .any(|k| t == *k || t.contains(k))
}

fn is_error_card(c: &EventCard) -> bool {
    matches!(c.status.as_deref(), Some("error") | Some("failed"))
}

fn with_more(items: &[String], max: usize, sep: &str) -> String {
    let shown: Vec<&str> = items.iter().take(max).map(String::as_str).collect();
    let mut s = shown.join(sep);
    if items.len() > max {
        s.push_str(&format!(" (+{} more)", items.len() - max));
    }
    s
}

/// Deterministic summary of a thread. See docs/architecture.md ("Map").
pub fn build_map(
    events: &[ThreadEvent],
    thread_id: &str,
    max_chars: usize,
    stored: bool,
) -> String {
    build_map_with(events, thread_id, max_chars, stored, None)
}

/// Version 2 handoff map: user intent is retained verbatim (bounded), while all provider
/// output is represented by stable, fetchable labels. No assistant/tool body is included.
pub fn build_map_v2(
    events: &[ThreadEvent],
    thread_id: &str,
    max_chars: usize,
    stored: bool,
) -> String {
    let id = if stored && !thread_id.is_empty() {
        thread_id
    } else {
        "(memory)"
    };
    let mut lines = vec![format!(
        "[concordia map] thread {id} - v2 - tools/approvals NOT transferred"
    )];
    let pointer = if stored && !thread_id.is_empty() {
        format!("concordia thread read {thread_id}")
    } else {
        "native_memory".to_owned()
    };
    let user_indices = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.k == "user" && !event.text().trim().is_empty())
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    for (index, event) in events.iter().enumerate() {
        let ordinal = index + 1;
        let line = match event.k.as_str() {
            "user" if !event.text().trim().is_empty() => {
                if user_indices.len() > 2
                    && Some(&index) != user_indices.first()
                    && Some(&index) != user_indices.last()
                {
                    lines.push(format!(
                        "user pointer: turn {ordinal} -> {pointer} --range {ordinal}-{ordinal}"
                    ));
                    continue;
                }
                if ordinal == 1 {
                    format!("asked: {}", one_line(event.text(), 240))
                } else {
                    format!("user turn {ordinal}: {}", one_line(event.text(), 240))
                }
            }
            "assistant" => format!(
                "assistant pointer: turn {ordinal} -> {pointer} --range {ordinal}-{ordinal}"
            ),
            "card" => {
                let label = event
                    .card
                    .as_ref()
                    .map(|c| one_line(&c.title, 80))
                    .unwrap_or_else(|| "tool".into());
                let detail = event
                    .card
                    .as_ref()
                    .and_then(|c| c.detail.as_deref())
                    .unwrap_or("");
                if looks_like_path(detail) {
                    format!("files: {detail}; tool pointer ({label}): turn {ordinal} -> {pointer} --range {ordinal}-{ordinal}")
                } else {
                    format!("tool pointer ({label}): turn {ordinal} -> {pointer} --range {ordinal}-{ordinal}")
                }
            }
            "switch" | "note" | "usage" => format!(
                "{} pointer: turn {ordinal} -> {pointer} --range {ordinal}-{ordinal}",
                event.k
            ),
            _ => continue,
        };
        lines.push(line);
    }
    if stored && !thread_id.is_empty() {
        lines.push(format!(
            "more: concordia thread read {thread_id} [--range A-B | --tail N | --grep TEXT]"
        ));
    } else {
        lines.push("more: native_memory fallback; thread log is off (not stored)".into());
    }
    let mut out = lines.join("\n");
    if out.chars().count() > max_chars {
        let essential = [
            lines.first().cloned().unwrap_or_default(),
            lines
                .iter()
                .find(|line| line.starts_with("asked:"))
                .cloned()
                .unwrap_or_default(),
            lines
                .iter()
                .rev()
                .find(|line| line.starts_with("user turn "))
                .cloned()
                .unwrap_or_default(),
            lines.last().cloned().unwrap_or_default(),
        ]
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
        out = cap_chars(&essential.join("\n"), max_chars).to_owned();
    }
    out
}

/// As [`build_map`]; `note` is an extra header fragment such as "tools/approvals NOT transferred".
pub fn build_map_with(
    events: &[ThreadEvent],
    thread_id: &str,
    max_chars: usize,
    stored: bool,
    note: Option<&str>,
) -> String {
    let users: Vec<&ThreadEvent> = events.iter().filter(|e| e.k == "user").collect();
    let msgs = events
        .iter()
        .filter(|e| e.k == "user" || e.k == "assistant")
        .count();
    let title = users
        .first()
        .map(|u| one_line(u.text(), 60))
        .unwrap_or_default();

    // Accounts (or providers) in order of appearance, consecutive duplicates collapsed.
    let mut trail: Vec<String> = Vec::new();
    for e in events {
        if let Some(name) = e.account.as_ref().or(e.provider.as_ref()) {
            if trail.last() != Some(name) {
                trail.push(name.clone());
            }
        }
    }
    let times: Vec<u64> = events.iter().map(|e| e.t).filter(|t| *t > 0).collect();
    let minutes = match (times.iter().min(), times.iter().max()) {
        (Some(a), Some(b)) => Some((b - a) / 60),
        _ => None,
    };

    let id_part = if stored && !thread_id.is_empty() {
        thread_id.to_string()
    } else {
        "(memory)".to_string()
    };
    let mut header = format!("[concordia map] thread {id_part}");
    if !title.is_empty() {
        header.push_str(&format!(" \"{title}\""));
    }
    if !trail.is_empty() {
        header.push_str(&format!(" - {}", trail.join(" -> ")));
    }
    header.push_str(&format!(" - {msgs} msgs"));
    if let Some(m) = minutes {
        header.push_str(&format!(" - {m} min"));
    }
    if let Some(n) = note {
        header.push_str(&format!(" - {n}"));
    }

    let more = if stored && !thread_id.is_empty() {
        format!("more: concordia thread read {thread_id} [--tail N | --grep TEXT | --range A-B]")
    } else {
        "more: full text is not stored (thread log is off)".to_string()
    };

    let asked = users.first().map(|u| one_line(u.text(), 160));
    let now = if users.len() > 1
        && users.first().map(|e| e.text().trim()) != users.last().map(|e| e.text().trim())
    {
        users.last().map(|u| one_line(u.text(), 160))
    } else {
        None
    };

    let mut files: Vec<String> = Vec::new();
    let mut ran: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for e in events {
        let Some(c) = &e.card else { continue };
        let detail = c.detail.as_deref().unwrap_or("").trim();
        if is_error_card(c) {
            let line = if detail.is_empty() {
                c.title.clone()
            } else {
                format!("{}: {detail}", c.title)
            };
            errors.push(one_line(&line, 80));
        }
        if detail.is_empty() {
            continue;
        }
        // Raw native records are source evidence, not the legacy command/path projection.
        // Never interpret JSON source records or unavailable markers as executed commands.
        if detail.starts_with(references::RAW_TOOL_LABEL)
            || detail.starts_with("[native source unavailable:")
        {
            continue;
        }
        if is_shell_tool(&c.title) {
            let cmd = one_line(detail, 60);
            if !ran.contains(&cmd) {
                ran.push(cmd);
            }
        } else if is_file_tool(&c.title)
            && looks_like_path(detail)
            && !files.iter().any(|f| f == detail)
        {
            files.push(detail.to_string());
        }
    }
    let mut seen_errors = std::collections::HashSet::new();
    errors.retain(|error| seen_errors.insert(error.clone()));
    errors.truncate(3);

    let last_reply = events
        .iter()
        .rev()
        .find(|e| e.k == "assistant" && !e.text().trim().is_empty())
        .map(|e| e.text().to_string());

    let files_line = (!files.is_empty()).then(|| format!("files: {}", with_more(&files, 12, ", ")));
    let ran_line = (!ran.is_empty()).then(|| format!("ran: {}", with_more(&ran, 8, "; ")));
    let err_line = (!errors.is_empty()).then(|| format!("errors: {}", errors.join(" | ")));

    let assemble = |ran: bool, files: bool, errs: bool, reply_max: usize, ctx_max: usize| {
        let mut lines: Vec<String> = vec![header.clone()];
        if let Some(a) = &asked {
            lines.push(format!("asked: {}", one_line(a, ctx_max)));
        }
        if let Some(n) = &now {
            lines.push(format!("now: {}", one_line(n, ctx_max)));
        }
        if files {
            lines.extend(files_line.clone());
        }
        if ran {
            lines.extend(ran_line.clone());
        }
        if errs {
            lines.extend(err_line.clone());
        }
        if reply_max > 0 {
            if let Some(r) = &last_reply {
                lines.push(format!("last reply: {}", tail_chars(r, reply_max)));
            }
        }
        lines.push(more.clone());
        lines.join("\n")
    };
    let fits = |s: &str| s.chars().count() <= max_chars;

    // Documented drop order: ran, files, errors, then shorten the last reply.
    let mut out = assemble(true, true, true, 500, 160);
    if !fits(&out) {
        out = assemble(false, true, true, 500, 160);
    }
    if !fits(&out) {
        out = assemble(false, false, true, 500, 160);
    }
    if !fits(&out) {
        out = assemble(false, false, false, 500, 160);
    }
    if !fits(&out) {
        // Shorten the reply tail to what is left.
        let base = assemble(false, false, false, 0, 160).chars().count();
        let room = max_chars.saturating_sub(base + "\nlast reply: ".len());
        if room >= 20 {
            out = assemble(false, false, false, room.min(500), 160);
        }
    }
    if !fits(&out) {
        out = assemble(false, false, false, 0, 160);
    }
    if !fits(&out) {
        out = assemble(false, false, false, 0, 60);
    }
    if !fits(&out) {
        // Only header and pointer are left; the pointer must survive, so shrink the header.
        let tail_len = more.chars().count() + 1;
        if max_chars > tail_len + 20 {
            let room = max_chars - tail_len;
            let h = cap_chars(&header, room).to_string();
            out = format!("{h}\n{more}");
        } else {
            out = cap_chars(&out, max_chars).to_string();
        }
    }
    out
}

/// Map events from the in-memory transcript (thread log off). `Card` messages carry
/// `title \x1f detail \x1f status` in their text.
pub fn events_from_messages(
    messages: &[crate::context::PackMessage],
    provider: &str,
) -> Vec<ThreadEvent> {
    use crate::context::PackRole;
    let mut out = Vec::new();
    for m in messages {
        let text = m.text.trim();
        match m.role {
            PackRole::User if !text.is_empty() => {
                out.push(ThreadEvent {
                    t: 0,
                    ..ThreadEvent::with_text("user", text)
                });
            }
            PackRole::Assistant if !text.is_empty() => {
                let mut e = ThreadEvent::with_text("assistant", text);
                e.t = 0;
                e.provider = Some(provider.to_string());
                out.push(e);
            }
            PackRole::Card => {
                let mut it = m.text.split('\u{1f}');
                let title = it.next().unwrap_or("");
                let detail = it.next().filter(|d| !d.is_empty());
                let status = it.next().unwrap_or("done");
                if !title.is_empty() {
                    let mut e = ThreadEvent::card(title, detail, status);
                    e.t = 0;
                    out.push(e);
                }
            }
            _ => {}
        }
    }
    out
}

/// Encode a card for a `PackRole::Card` message.
pub fn encode_card(title: &str, detail: Option<&str>, status: &str) -> String {
    format!("{title}\u{1f}{}\u{1f}{status}", detail.unwrap_or(""))
}

// ---------------------------------------------------------------------------------------------
// `concordia thread read`
// ---------------------------------------------------------------------------------------------

/// Plain-text rendering of one event for `thread read` and `--grep`.
pub fn render_event(idx: usize, e: &ThreadEvent) -> String {
    let who = e
        .account
        .as_deref()
        .or(e.provider.as_deref())
        .map(|a| format!(" {a}"))
        .unwrap_or_default();
    match (e.k.as_str(), &e.card) {
        ("card", Some(c)) => format!(
            "#{idx} card{who}: {} {} [{}]",
            c.title,
            c.detail.as_deref().unwrap_or(""),
            c.status.as_deref().unwrap_or("")
        ),
        _ => format!("#{idx} {}{who}: {}", e.k, e.text()),
    }
}

pub struct ReadOpts {
    pub tail: Option<usize>,
    pub grep: Option<String>,
    pub range: Option<(usize, usize)>,
    pub max_chars: usize,
}

/// Parse `A-B` (1-based, inclusive).
pub fn parse_range(s: &str) -> Result<(usize, usize)> {
    let (a, b) = s
        .split_once('-')
        .ok_or_else(|| anyhow!("--range wants A-B, e.g. 3-10"))?;
    let a: usize = a.trim().parse().map_err(|_| anyhow!("--range wants A-B"))?;
    let b: usize = b.trim().parse().map_err(|_| anyhow!("--range wants A-B"))?;
    if a == 0 || b < a {
        bail!("--range wants 1 <= A <= B");
    }
    Ok((a, b))
}

pub fn read_events_text(events: &[ThreadEvent], opts: &ReadOpts) -> String {
    let mut rows: Vec<(usize, &ThreadEvent)> = events
        .iter()
        .enumerate()
        .map(|(i, e)| (i + 1, e))
        .filter(|(_, e)| e.k != "usage")
        .collect();
    if let Some((a, b)) = opts.range {
        rows.retain(|(i, _)| *i >= a && *i <= b);
    }
    if let Some(g) = &opts.grep {
        let g = g.to_lowercase();
        rows.retain(|(i, e)| render_event(*i, e).to_lowercase().contains(&g));
    }
    if let Some(n) = opts.tail {
        let skip = rows.len().saturating_sub(n);
        rows.drain(..skip);
    }
    let mut out = String::new();
    let mut used = 0usize;
    for (i, e) in rows {
        let line = render_event(i, e);
        let add = line.chars().count() + 1;
        if used + add > opts.max_chars {
            let room = opts.max_chars.saturating_sub(used + 1);
            if room > 0 {
                out.push_str(cap_chars(&line, room));
                out.push('\n');
            }
            out.push_str("[truncated: raise --max-chars or narrow with --tail/--grep/--range]\n");
            return out;
        }
        out.push_str(&line);
        out.push('\n');
        used += add;
    }
    out
}

/// Human age: "5m", "3h", "2d".
pub fn age_label(started: u64) -> String {
    let secs = now_secs().saturating_sub(started);
    match secs {
        0..=59 => "now".to_string(),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

// ---------------------------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------------------------

/// `concordia thread ...`. Returns (stdout, exit code); errors go to the Err string (stderr).
pub fn run_cli(state_root: &Path, args: &[String]) -> std::result::Result<String, (String, i32)> {
    let usage = "usage: concordia thread list [--json] | map <id> | read <id> [--tail N] [--grep TEXT] [--range A-B] [--max-chars N]".to_string();
    let Some(sub) = args.first() else {
        return Err((usage, 2));
    };
    match sub.as_str() {
        "migrate" => crate::legacy_migration::run_cli(state_root, &args[1..]),
        "list" => {
            let json = args[1..].iter().any(|a| a == "--json");
            if let Some(bad) = args[1..].iter().find(|a| *a != "--json") {
                return Err((format!("unknown option {bad}\n{usage}"), 2));
            }
            let metas = list(state_root);
            if json {
                return Ok(serde_json::to_string_pretty(&metas).unwrap_or_default() + "\n");
            }
            let mut out = String::new();
            for m in &metas {
                out.push_str(&format!(
                    "{}  {:>4}  {}\n",
                    m.id,
                    age_label(m.started),
                    m.title
                ));
            }
            if out.is_empty() {
                out.push_str("no recorded threads (enable with CONCORDIA_THREADS=1)\n");
            }
            Ok(out)
        }
        "map" | "read" => {
            let Some(prefix) = args.get(1) else {
                return Err((usage, 2));
            };
            let mut opts = ReadOpts {
                tail: None,
                grep: None,
                range: None,
                max_chars: READ_CHARS,
            };
            let mut it = args[2..].iter();
            while let Some(a) = it.next() {
                let mut val = |name: &str| -> std::result::Result<String, (String, i32)> {
                    it.next()
                        .cloned()
                        .ok_or_else(|| (format!("{name} needs a value\n{usage}"), 2))
                };
                match (sub.as_str(), a.as_str()) {
                    ("read", "--tail") => {
                        opts.tail = Some(
                            val("--tail")?
                                .parse()
                                .map_err(|_| ("--tail wants a number".to_string(), 2))?,
                        )
                    }
                    ("read", "--grep") => opts.grep = Some(val("--grep")?),
                    ("read", "--range") => {
                        opts.range =
                            Some(parse_range(&val("--range")?).map_err(|e| (e.to_string(), 2))?)
                    }
                    (_, "--max-chars") => {
                        opts.max_chars = val("--max-chars")?
                            .parse()
                            .map_err(|_| ("--max-chars wants a number".to_string(), 2))?
                    }
                    _ => return Err((format!("unknown option {a}\n{usage}"), 2)),
                }
            }
            let id = resolve_prefix(state_root, prefix).map_err(|e| (e.to_string(), 1))?;
            let events = ThreadLog::open(state_root, &id)
                .read_all()
                .map_err(|e| (format!("{e:#}"), 1))?;
            if sub == "map" {
                let cap = opts.max_chars.min(MAP_CHARS);
                Ok(build_map(&events, &id, cap, true) + "\n")
            } else {
                Ok(read_events_text(&events, &opts))
            }
        }
        other => Err((format!("unknown thread command '{other}'\n{usage}"), 2)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(t: &str) -> ThreadEvent {
        ThreadEvent::with_text("user", t)
    }
    fn asst(t: &str) -> ThreadEvent {
        ThreadEvent::with_text("assistant", t).who("claude", "claude-1", "m")
    }

    #[test]
    fn empty_thread_still_has_header_and_pointer() {
        let m = build_map(&[], "abc", 1600, true);
        assert!(m.starts_with("[concordia map] thread abc"));
        assert!(m
            .lines()
            .last()
            .unwrap()
            .contains("concordia thread read abc"));
        assert!(m.chars().count() <= 1600);
    }

    #[test]
    fn unstored_map_says_so() {
        let m = build_map(&[user("hi")], "", 1600, false);
        assert!(m.contains("not stored"));
        assert!(!m.contains("thread read"));
    }

    #[test]
    fn layout_lines_and_dedup() {
        let mut ev = vec![user("fix the login redirect"), asst("looking")];
        for p in ["src/a.rs", "src/b.rs", "src/a.rs"] {
            ev.push(ThreadEvent::card("Edit", Some(p), "done"));
        }
        ev.push(ThreadEvent::card("Bash", Some("cargo test"), "done"));
        ev.push(ThreadEvent::card("Bash", Some("boom"), "error"));
        ev.push(user("now the logout"));
        ev.push(asst("done with it"));
        let m = build_map(&ev, "id1", 1600, true);
        assert!(m.contains("asked: fix the login redirect"), "{m}");
        assert!(m.contains("now: now the logout"), "{m}");
        assert!(m.contains("files: src/a.rs, src/b.rs\n"), "{m}");
        assert!(m.contains("ran: cargo test; boom"), "{m}");
        assert!(m.contains("errors: Bash: boom"), "{m}");
        assert!(m.contains("last reply: done with it"), "{m}");
        assert!(m.contains("4 msgs"), "{m}");
    }

    #[test]
    fn map_omits_repeated_request_field_and_nonadjacent_identical_errors() {
        let events = vec![
            user("same request"),
            ThreadEvent::card("shell", Some("error one"), "error"),
            ThreadEvent::card("shell", Some("error two"), "error"),
            ThreadEvent::card("shell", Some("error one"), "error"),
            user("same request"),
        ];
        let map = build_map(&events, "fixture", 1600, true);
        assert!(map.contains("asked: same request"));
        assert!(!map.contains("now:"));
        let errors = map
            .lines()
            .find(|line| line.starts_with("errors:"))
            .unwrap();
        assert_eq!(errors.matches("error one").count(), 1);
        assert_eq!(errors.matches("error two").count(), 1);
        assert!(map.contains("2 msgs"), "source events are retained");
    }

    #[test]
    fn files_cap_at_12_and_non_paths_are_ignored() {
        let mut ev = vec![user("go")];
        for i in 0..15 {
            ev.push(ThreadEvent::card(
                "Read",
                Some(&format!("src/f{i}.rs")),
                "done",
            ));
        }
        ev.push(ThreadEvent::card("Read", Some("not a path here"), "done"));
        let m = build_map(&ev, "id", 5000, true);
        assert!(m.contains("(+3 more)"), "{m}");
        assert!(!m.contains("not a path"));
    }

    #[test]
    fn budget_is_never_exceeded_and_drop_order_holds() {
        let mut ev = vec![user(&"q".repeat(500)), asst(&"r".repeat(5000))];
        for i in 0..30 {
            ev.push(ThreadEvent::card(
                "Edit",
                Some(&format!("src/dir/file{i}.rs")),
                "done",
            ));
            ev.push(ThreadEvent::card(
                "Bash",
                Some(&format!("cmd number {i}")),
                "error",
            ));
        }
        for max in [1600usize, 900, 500, 300, 200, 120, 60, 10, 0] {
            let m = build_map(&ev, "3f9a1111-2222-3333-4444-555555555555", max, true);
            assert!(
                m.chars().count() <= max,
                "max {max} got {}",
                m.chars().count()
            );
        }
        let m = build_map(&ev, "id", 1600, true);
        assert!(m.chars().count() <= 1600);
        // A tight budget drops ran first, keeps errors.
        let tight = build_map(&ev, "id", 700, true);
        assert!(!tight.contains("\nran:"), "{tight}");
        assert!(tight.contains("more: concordia thread read id"), "{tight}");
    }

    #[test]
    fn huge_and_unicode_text_cut_on_char_boundaries() {
        let s = "é日本語✓".repeat(30_000);
        let e = ThreadEvent::with_text("user", &s);
        assert_eq!(e.text().chars().count(), MAX_EVENT_TEXT);
        let ev = vec![e, asst(&s)];
        for max in [1600usize, 333, 101] {
            let m = build_map(&ev, "x", max, true);
            assert!(m.chars().count() <= max);
        }
    }

    #[test]
    fn note_goes_into_header() {
        let m = build_map_with(
            &[user("a")],
            "",
            1600,
            false,
            Some("tools/approvals NOT transferred"),
        );
        assert!(m
            .lines()
            .next()
            .unwrap()
            .contains("tools/approvals NOT transferred"));
    }

    #[test]
    fn map_v2_is_deterministic() {
        let events = vec![
            user("fix the login"),
            asst("implementation detail must not leak"),
        ];
        assert_eq!(
            build_map_v2(&events, "thread-1", 1600, true),
            build_map_v2(&events, "thread-1", 1600, true)
        );
    }

    #[test]
    fn map_v2_keeps_the_concordia_map_marker() {
        let map = build_map_v2(&[user("keep the handoff readable")], "thread-1", 1600, true);

        assert!(map.starts_with("[concordia map]"));
        assert!(map.starts_with("[concordia map] thread thread-1 - v2"));
        assert!(!map.starts_with("[concordia map v2]"));
    }

    #[test]
    fn map_never_contains_free_text_summaries() {
        let events = vec![
            user("typed decision: keep PKCE"),
            asst("SECRET_ASSISTANT_REPLY"),
        ];
        let map = build_map_v2(&events, "thread-1", 1600, true);
        assert!(map.contains("typed decision: keep PKCE"));
        assert!(!map.contains("SECRET_ASSISTANT_REPLY"));
        assert!(map.contains("assistant pointer"));
    }

    #[test]
    fn map_respects_size_cap() {
        let events = (0..40)
            .map(|n| {
                if n % 2 == 0 {
                    user(&format!("decision {n}"))
                } else {
                    asst("reply")
                }
            })
            .collect::<Vec<_>>();
        for cap in [0, 10, 200, 1600] {
            assert!(build_map_v2(&events, "thread-1", cap, true).chars().count() <= cap);
        }
    }

    #[test]
    fn pinned_span_survives_native_file_truncation() {
        let state = tempfile::tempdir().unwrap();
        let native = state.path().join("native.jsonl");
        std::fs::write(&native, "native source body").unwrap();
        let log = ThreadLog::open(state.path(), "pin-survival");
        log.pin_span(4, "assistant", "native source body").unwrap();
        std::fs::write(&native, "").unwrap();
        assert_eq!(log.resolve_span(4).unwrap(), "native source body");
    }

    #[test]
    fn pins_are_redacted() {
        let state = tempfile::tempdir().unwrap();
        let log = ThreadLog::open(state.path(), "pin-redaction");
        log.pin_span(1, "tool", "Authorization: Bearer sk-abcdefghijklmnop")
            .unwrap();
        let raw = std::fs::read_to_string(log.pins_path()).unwrap();
        assert!(!raw.contains("sk-abcdefghijklmnop"));
        assert!(raw.contains("[REDACTED:authorization:"));
    }

    #[test]
    fn pins_are_pruned_oldest_first() {
        let state = tempfile::tempdir().unwrap();
        let log = ThreadLog::open(state.path(), "pin-pruning");
        for ordinal in 1..=MAX_PINNED_SPANS + 2 {
            log.pin_span(ordinal, "assistant", &format!("body {ordinal}"))
                .unwrap();
        }
        let pins = log.read_pins_unchecked();
        assert_eq!(pins.len(), MAX_PINNED_SPANS);
        assert_eq!(pins.first().unwrap().ordinal, 3);
        assert_eq!(pins.last().unwrap().ordinal, MAX_PINNED_SPANS + 2);
    }

    #[test]
    fn log_roundtrip_list_resolve_delete() {
        let dir = tempfile::tempdir().unwrap();
        let a = ThreadLog::open(dir.path(), "aaaa1111");
        // Old logs remain readable byte for byte; only new appends use the V2 contract.
        std::fs::create_dir_all(&a.dir).unwrap();
        let legacy = format!(
            "{}\n{}\n",
            serde_json::to_string(&user("hello")).unwrap(),
            serde_json::to_string(&asst("hi")).unwrap()
        );
        std::fs::write(a.log_path(), &legacy).unwrap();
        assert!(a.append(&user("must not be copied")).is_err());
        assert!(a
            .append(&ThreadEvent::card("shell", Some("secret command"), "done"))
            .is_err());
        assert_eq!(std::fs::read_to_string(a.log_path()).unwrap(), legacy);
        a.write_meta(&ThreadMeta {
            id: "aaaa1111".into(),
            started: 100,
            title: "hello".into(),
            cwd: String::new(),
            account: None,
        })
        .unwrap();
        let b = ThreadLog::open(dir.path(), "aaaa2222");
        b.append(&ThreadEvent::new("switch").who("codex", "fixture", "model"))
            .unwrap();
        let unavailable = user("later sentinel body must never be persisted");
        b.append_unavailable(
            &unavailable,
            None,
            "arbitrary error sentinel must not persist",
        )
        .unwrap();
        let body_free = std::fs::read_to_string(b.log_path()).unwrap();
        assert!(body_free.contains("\"version\":2"));
        assert!(
            !body_free.contains("sentinel")
                && !body_free.contains("\"text\"")
                && !body_free.contains("\"detail\"")
        );
        assert_eq!(
            b.read_all().unwrap()[1].text(),
            "[native source unavailable: capture_failed]"
        );
        b.write_meta(&ThreadMeta {
            id: "aaaa2222".into(),
            started: 200,
            title: "later".into(),
            cwd: String::new(),
            account: None,
        })
        .unwrap();
        assert_eq!(a.read_all().unwrap().len(), 2);
        let l = list(dir.path());
        assert_eq!(l[0].id, "aaaa2222");
        assert!(resolve_prefix(dir.path(), "aaaa").is_err());
        assert!(resolve_prefix(dir.path(), "zz").is_err());
        assert_eq!(resolve_prefix(dir.path(), "aaaa1").unwrap(), "aaaa1111");

        // Exact native source ownership, successful hydration and stale-source failure.
        use crate::native_memory::{
            references::{begin_scope, capture_text},
            Binding,
        };
        use crate::store::{ProviderKind, StoredAccount};
        let home = dir.path().join("fixture");
        std::fs::create_dir_all(home.join("sessions")).unwrap();
        crate::store::upsert_stored_account(
            dir.path(),
            StoredAccount {
                id: "fixture".into(),
                provider: ProviderKind::Codex,
                home_path: Some(home.to_string_lossy().into_owned()),
                label: "fixture".into(),
                created_at: "0".into(),
            },
        )
        .unwrap();
        let session = "11111111-2222-4333-8444-555555555555";
        let native_path = home.join("sessions").join(format!("{session}.jsonl"));
        std::fs::write(
            &native_path,
            format!(
                "{}\n",
                serde_json::json!({"type":"session_meta","payload":{"id":session}})
            ),
        )
        .unwrap();
        let binding = Binding::new(ProviderKind::Codex, "fixture", &home, session).unwrap();
        let scope = begin_scope(dir.path(), &binding).unwrap();
        let body = "immutable native-only 日本語 sentinel";
        let native_record = format!(
            "{}\n",
            serde_json::json!({"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":body}]}})
        );
        std::fs::OpenOptions::new()
            .append(true)
            .open(&native_path)
            .unwrap()
            .write_all(native_record.as_bytes())
            .unwrap();
        let reference = capture_text(dir.path(), &scope, "user", body).unwrap();
        let event = user(body).who("codex", "fixture", "model");
        assert!(b
            .append_referenced(&user("wrong projection"), reference.clone())
            .is_err());
        assert!(b.append_referenced(&asst(body), reference.clone()).is_err());
        assert!(b
            .append_referenced(
                &user(body).who("codex", "different", "model"),
                reference.clone()
            )
            .is_err());
        b.append_referenced(&event, reference.clone()).unwrap();
        b.append_unavailable(&user("another private body"), Some(&scope), "missing")
            .unwrap();
        // Image-only prompts have empty local text but an explicitly captured native input.
        b.append_referenced(&user("").who("codex", "fixture", "model"), reference)
            .unwrap();
        let journal = std::fs::read_to_string(b.log_path()).unwrap();
        assert!(!journal.contains(body) && !journal.contains("another private body"));
        let hydrated = b.read_all().unwrap();
        assert_eq!(hydrated[2], event);
        assert_eq!(hydrated[3].account.as_deref(), Some("fixture"));
        assert_eq!(hydrated[4].text(), body);
        assert!(build_map(&hydrated, b.id(), 1600, true).contains(body));
        let original_native = std::fs::read_to_string(&native_path).unwrap();
        std::fs::write(
            &native_path,
            original_native.replace("immutable", "corrupted"),
        )
        .unwrap();
        assert_eq!(
            b.read_all().unwrap()[2].text(),
            "[native source unavailable: verification_failed]"
        );
        assert_eq!(std::fs::read_to_string(a.log_path()).unwrap(), legacy);
        delete(dir.path(), "aaaa1111").unwrap();
        assert_eq!(list(dir.path()).len(), 1);
        crate::legacy_migration::fixture_proof();
    }

    #[test]
    fn read_filters_and_caps() {
        let ev = vec![user("alpha"), asst("beta"), user("gamma"), asst("delta")];
        let o = |tail, grep: Option<&str>, range, max| ReadOpts {
            tail,
            grep: grep.map(String::from),
            range,
            max_chars: max,
        };
        assert_eq!(
            read_events_text(&ev, &o(Some(1), None, None, 6000))
                .lines()
                .count(),
            1
        );
        let g = read_events_text(&ev, &o(None, Some("GAM"), None, 6000));
        assert!(g.contains("gamma") && !g.contains("alpha"));
        let r = read_events_text(&ev, &o(None, None, Some((2, 3)), 6000));
        assert!(r.contains("beta") && r.contains("gamma") && !r.contains("delta"));
        let c = read_events_text(&ev, &o(None, None, None, 20));
        assert!(c.contains("[truncated"));
        assert!(parse_range("0-3").is_err() && parse_range("3-1").is_err());
    }

    #[test]
    fn memory_events_roundtrip() {
        use crate::context::{PackMessage, PackRole};
        let msgs = vec![
            PackMessage {
                role: PackRole::User,
                text: "hi".into(),
            },
            PackMessage {
                role: PackRole::Card,
                text: encode_card("Edit", Some("a/b.rs"), "done"),
            },
        ];
        let ev = events_from_messages(&msgs, "codex");
        assert_eq!(ev.len(), 2);
        assert!(build_map(&ev, "", 1600, false).contains("files: a/b.rs"));
    }
}
