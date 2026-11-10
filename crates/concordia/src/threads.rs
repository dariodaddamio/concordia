//! Thread log and map: an opt-in, append-only record of a conversation, and deterministic
//! pointer maps (no model, no network). See docs/architecture.md ("Thread log", "Map").

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[path = "thread_references.rs"]
mod references;
use crate::context_types::{PackMessage, PackRole};
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
const JOURNAL_PAGE_BYTES: u64 = 8 * 1024 * 1024;
const JOURNAL_RECORD_BYTES: usize = 1024 * 1024;
const JOURNAL_PAGE_EVENTS: usize = 256;
const JOURNAL_READ_BYTES: usize = 32 * 1024 * 1024;
const JOURNAL_SCAN_BYTES: u64 = 256 * 1024 * 1024;
const JOURNAL_ANCHOR_BYTES: u64 = 64 * 1024;

/// One finite journal page; cursors retain exact file identity and a fixed source snapshot.
pub struct ThreadPage {
    pub events: Vec<ThreadEvent>,
    pub first_ordinal: usize,
    pub ordinals: Vec<usize>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct JournalCursor {
    identity: String,
    length: u64,
    offset: u64,
    ordinal: usize,
    anchor_len: u64,
    anchor_sha256: String,
}

#[derive(Serialize, Deserialize)]
struct JournalCount {
    identity: String,
    length: u64,
    records: usize,
    modified_ns: u128,
    anchor_len: u64,
    anchor_sha256: String,
}

fn journal_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn journal_anchor(file: &mut std::fs::File, length: u64) -> std::io::Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(length).read_to_end(&mut bytes)?;
    Ok(journal_digest(&bytes))
}

fn modified_ns(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |time| time.as_nanos())
}

// ---------------------------------------------------------------------------------------------
// The environment variable is an explicit enable override; normal configuration is
// `[threads] logging = "on"`.
// ---------------------------------------------------------------------------------------------
pub fn enabled(state_root: &Path) -> bool {
    if std::env::var("CONCORDIA_THREADS").is_ok_and(|v| v == "1") {
        return true;
    }
    crate::store::get_settings(state_root).thread_log
        || crate::config::for_root(state_root).threads.logging
}

/// A new thread log for a conversation, or None when recording is off.
pub fn start(state_root: &Path) -> Option<ThreadLog> {
    enabled(state_root).then(|| ThreadLog::open(state_root, &uuid::Uuid::new_v4().to_string()))
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
            .map(|_| ())
    }

    /// Verify an immutable native projection before persisting its body-free pointer.
    pub fn append_referenced(&self, ev: &ThreadEvent, reference: Reference) -> Result<()> {
        let record = references::Record::referenced(self.root(), ev, reference.clone())?;
        let ordinal = self.append_record(&record)?;
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
        let root = crate::update::resolved_directory(&self.dir)
            .ok_or_else(|| anyhow!("pin directory is not regular"))?;
        let path = self.pins_path();
        if [lock.as_path(), path.as_path()]
            .into_iter()
            .any(|path| path.exists() && !crate::update::regular_owned_file(path, &root))
        {
            bail!("pin journal or lock is not a regular owned file");
        }
        crate::state_io::with_lock(&lock, || {
            let mut spans = self.read_pins_unchecked()?;
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
            let mut body = Vec::new();
            for span in spans {
                let json = serde_json::to_vec(&span)?;
                body.extend(crate::redact::redact_bytes(&json));
                body.push(b'\n');
            }
            if body.len() > MAX_PINNED_BYTES {
                return Err(std::io::Error::other(
                    "redacted pin journal exceeds its byte budget; original preserved",
                ));
            }
            crate::state_io::write_atomic(&self.pins_path(), &body)?;
            Ok(())
        })?;
        Ok(())
    }

    fn read_pins_unchecked(&self) -> std::io::Result<Vec<PinnedSpan>> {
        let file = match std::fs::File::open(self.pins_path()) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(MAX_PINNED_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_PINNED_BYTES {
            return Err(std::io::Error::other(
                "pin journal exceeds its byte budget; original preserved",
            ));
        }
        let mut spans = Vec::new();
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let span = serde_json::from_slice(line).map_err(std::io::Error::other)?;
            spans.push(span);
            if spans.len() > MAX_PINNED_SPANS {
                return Err(std::io::Error::other(
                    "pin journal exceeds its record budget; original preserved",
                ));
            }
        }
        Ok(spans)
    }

    /// Record a source gap without persisting the original text or an arbitrary error body.
    pub fn append_unavailable(
        &self,
        ev: &ThreadEvent,
        scope: Option<&CaptureScope>,
        reason: &str,
    ) -> Result<()> {
        self.append_record(&references::Record::unavailable(ev, scope, reason)?)
            .map(|_| ())
    }

    fn root(&self) -> &Path {
        self.dir
            .parent()
            .expect("thread directory has a state root")
    }

    /// One bounded cross-process append transaction per journal record.
    fn append_record(&self, record: &references::Record) -> Result<usize> {
        let mut line = crate::redact::redact_bytes(&serde_json::to_vec(record)?);
        line.push(b'\n');
        if line.len() > JOURNAL_RECORD_BYTES {
            bail!("thread record exceeds its byte budget; journal preserved");
        }
        std::fs::create_dir_all(&self.dir)?;
        let root = crate::update::resolved_directory(&self.dir)
            .ok_or_else(|| anyhow!("thread directory is not regular"))?;
        let path = self.log_path();
        let lock = self.dir.join(format!("{}.lock", self.id));
        let count_path = self.dir.join(format!("{}.ordinal.json", self.id));
        if [
            path.as_path(),
            lock.as_path(),
            count_path.as_path(),
            self.pins_path().as_path(),
        ]
        .into_iter()
        .any(|path| path.exists() && !crate::update::regular_owned_file(path, &root))
        {
            bail!("thread journal or lock is not a regular owned file");
        }
        let ordinal = crate::state_io::with_lock(&lock, || {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .read(true)
                .append(true)
                .open(&path)?;
            let metadata = file.metadata()?;
            let length = metadata.len();
            let identity = crate::file_identity::of(&file).map_err(std::io::Error::other)?;
            // Derived body-free metadata avoids rescanning the journal after every append,
            // including when the asynchronous reference journal opens a fresh ThreadLog.
            let cached = std::fs::File::open(&count_path)
                .ok()
                .and_then(|file| serde_json::from_reader::<_, JournalCount>(file.take(4097)).ok());
            let mut offset = 0;
            let mut records = 0usize;
            if let Some(cached) = cached {
                if cached.identity == identity
                    && cached.length <= length
                    && cached.anchor_len <= JOURNAL_ANCHOR_BYTES.min(cached.length)
                    && (cached.length < length || cached.modified_ns == modified_ns(&metadata))
                    && journal_anchor(&mut file, cached.anchor_len)? == cached.anchor_sha256
                {
                    offset = cached.length;
                    records = cached.records;
                }
            }
            if length - offset > JOURNAL_SCAN_BYTES {
                return Err(std::io::Error::other(
                    "thread ordinal scan exceeds 256 MiB; journal preserved",
                ));
            }
            file.seek(SeekFrom::Start(offset))?;
            let mut remaining = (&mut file).take(length - offset);
            let mut buffer = [0u8; 64 * 1024];
            let mut last = None;
            loop {
                let read = remaining.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                records = records
                    .checked_add(buffer[..read].iter().filter(|byte| **byte == b'\n').count())
                    .ok_or_else(|| std::io::Error::other("thread ordinal overflow"))?;
                last = buffer[..read].last().copied();
            }
            // Preserve a legacy final record lacking a newline; add only its separator.
            if last.is_some_and(|byte| byte != b'\n') {
                records = records
                    .checked_add(1)
                    .ok_or_else(|| std::io::Error::other("thread ordinal overflow"))?;
                file.write_all(b"\n")?;
            }
            let ordinal = records
                .checked_add(1)
                .ok_or_else(|| std::io::Error::other("thread ordinal overflow"))?;
            file.write_all(&line)?;
            file.flush()?;
            let metadata = file.metadata()?;
            let count = JournalCount {
                identity,
                length: metadata.len(),
                records: ordinal,
                modified_ns: modified_ns(&metadata),
                anchor_len: metadata.len().min(JOURNAL_ANCHOR_BYTES),
                anchor_sha256: journal_anchor(&mut file, metadata.len().min(JOURNAL_ANCHOR_BYTES))?,
            };
            // A failed cache update never invalidates the already flushed journal append.
            if let Ok(bytes) = serde_json::to_vec(&count) {
                let _ = crate::state_io::write_atomic(&count_path, &bytes);
            }
            Ok(ordinal)
        })?;
        Ok(ordinal)
    }

    pub fn read_page(&self, cursor: Option<&str>) -> Result<ThreadPage> {
        read_event_page(self.root(), &self.log_path(), cursor)
    }

    /// A bounded map remains available even when a full legacy thread is too large to hydrate.
    pub fn read_map(&self, max_chars: usize) -> Result<String> {
        let page = self.read_page(None)?;
        let note = page.next_cursor.as_ref().map(|_| {
            "PARTIAL journal: first page only; follow thread read --cursor pages for later events"
        });
        Ok(build_map(
            &page.events,
            &self.id,
            max_chars,
            true,
            None,
            note,
        ))
    }

    fn read_tail_page(&self, after: Option<&str>, opts: &ReadOpts) -> Result<ThreadPage> {
        let count = opts.tail.unwrap_or(JOURNAL_PAGE_EVENTS);
        let grep = opts.grep.as_ref().map(|text| text.to_lowercase());
        let mut cursor = after.map(str::to_owned);
        let mut retained = std::collections::VecDeque::new();
        let mut bytes = 0usize;
        for _ in 0..(JOURNAL_SCAN_BYTES / JOURNAL_PAGE_BYTES) {
            let page = self.read_page(cursor.as_deref())?;
            for (ordinal, event) in page.ordinals.into_iter().zip(page.events) {
                if event.k == "usage"
                    || opts
                        .range
                        .is_some_and(|(start, end)| ordinal < start || ordinal > end)
                    || grep.as_ref().is_some_and(|query| {
                        !render_event(ordinal, &event).to_lowercase().contains(query)
                    })
                {
                    continue;
                }
                let size = serde_json::to_vec(&event)?.len();
                bytes += size;
                retained.push_back((ordinal, event, size));
                while retained.len() > count || bytes > JOURNAL_PAGE_BYTES as usize {
                    if let Some((_, _, size)) = retained.pop_front() {
                        bytes -= size;
                    }
                }
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        let first_ordinal = retained.front().map_or(1, |(ordinal, _, _)| *ordinal);
        let ordinals = retained.iter().map(|(ordinal, _, _)| *ordinal).collect();
        Ok(ThreadPage {
            ordinals,
            events: retained.into_iter().map(|(_, event, _)| event).collect(),
            first_ordinal,
            next_cursor: cursor,
        })
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
        let encoded = crate::redact::redact_bytes(&serde_json::to_vec(&meta)?);
        crate::state_io::write_atomic(&self.meta_path(), &encoded)?;
        Ok(())
    }

    pub fn read_meta(&self) -> Option<ThreadMeta> {
        let root = crate::update::resolved_directory(&self.dir)?;
        if !crate::update::regular_owned_file(&self.meta_path(), &root) {
            return None;
        }
        let mut bytes = Vec::new();
        std::fs::File::open(self.meta_path())
            .ok()?
            .take(16 * 1024 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > 16 * 1024 {
            return None;
        }
        serde_json::from_slice(&bytes).ok()
    }

    pub fn delete(&self) -> Result<()> {
        if !self.dir.try_exists()? {
            return Ok(());
        }
        let root = crate::update::resolved_directory(&self.dir)
            .ok_or_else(|| anyhow!("thread directory is not regular"))?;
        let journal_lock = self.dir.join(format!("{}.lock", self.id));
        let pins_lock = self.dir.join(format!("{}.pins.lock", self.id));
        let paths = [
            self.log_path(),
            self.meta_path(),
            self.pins_path(),
            self.dir.join(format!("{}.ordinal.json", self.id)),
        ];
        let verify = || -> std::io::Result<()> {
            for path in paths.iter().chain([&journal_lock, &pins_lock]) {
                match std::fs::symlink_metadata(path) {
                    Ok(_) if !crate::update::regular_owned_file(path, &root) => {
                        return Err(std::io::Error::other(
                            "thread deletion target or lock is not a regular owned file; records preserved",
                        ));
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        };
        verify()?;
        crate::state_io::with_lock(&journal_lock, || {
            crate::state_io::with_lock(&pins_lock, || {
                // Validate every exact app-owned target before removing the first one.
                // Stable lock pathnames remain; provider-owned sources are never targets.
                verify()?;
                for path in &paths {
                    match std::fs::remove_file(path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error),
                    }
                }
                Ok(())
            })
        })?;
        Ok(())
    }
}

fn open_journal(path: &Path) -> Result<std::fs::File> {
    let directory = path
        .parent()
        .and_then(crate::update::resolved_directory)
        .ok_or_else(|| anyhow!("thread directory unavailable"))?;
    if !crate::update::regular_owned_file(path, &directory) {
        bail!("thread journal is not a regular owned file");
    }
    std::fs::File::open(path).with_context(|| format!("read {}", path.display()))
}

fn read_event_page(root: &Path, path: &Path, after: Option<&str>) -> Result<ThreadPage> {
    let mut file = open_journal(path)?;
    let identity = crate::file_identity::of(&file).map_err(anyhow::Error::msg)?;
    let length = file.metadata()?.len();
    let mut cursor = if let Some(encoded) = after {
        if encoded.len() > 4096 || encoded.len() % 2 != 0 || !encoded.is_ascii() {
            bail!("invalid thread cursor");
        }
        let bytes = (0..encoded.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&encoded[at..at + 2], 16))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let cursor: JournalCursor = serde_json::from_slice(&bytes)?;
        if cursor.identity != identity
            || cursor.length > length
            || cursor.offset > cursor.length
            || cursor.anchor_len > JOURNAL_ANCHOR_BYTES.min(cursor.length)
            || cursor.ordinal == 0
            || cursor.ordinal > usize::MAX - JOURNAL_PAGE_EVENTS
            || journal_anchor(&mut file, cursor.anchor_len)? != cursor.anchor_sha256
        {
            bail!("thread cursor is stale or belongs to another journal; source preserved");
        }
        if cursor.offset > 0 {
            file.seek(SeekFrom::Start(cursor.offset - 1))?;
            let mut boundary = [0];
            file.read_exact(&mut boundary)?;
            if boundary != [b'\n'] {
                bail!("thread cursor splits a journal record");
            }
        }
        cursor
    } else {
        JournalCursor {
            identity,
            length,
            offset: 0,
            ordinal: 1,
            anchor_len: length.min(JOURNAL_ANCHOR_BYTES),
            anchor_sha256: journal_anchor(&mut file, length.min(JOURNAL_ANCHOR_BYTES))?,
        }
    };
    let first_ordinal = cursor.ordinal;
    file.seek(SeekFrom::Start(cursor.offset))?;
    let available = cursor.length - cursor.offset;
    let mut reader = BufReader::new(file.take(available.min(JOURNAL_PAGE_BYTES)));
    let mut events = Vec::new();
    let mut retained = 0usize;
    let mut scanned = 0u64;
    let mut records = 0usize;
    while cursor.offset < cursor.length && records < JOURNAL_PAGE_EVENTS {
        let mut line = Vec::new();
        let read = (&mut reader)
            .take(JOURNAL_RECORD_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        if line.len() > JOURNAL_RECORD_BYTES {
            bail!("thread record at byte {} exceeds 1 MiB; original journal preserved; inspect the exact source with the provider's native history tools", cursor.offset);
        }
        // Leave a record that crosses this page's byte boundary for the next page.
        if line.last() != Some(&b'\n') && cursor.offset + (read as u64) < cursor.length {
            if scanned == 0 {
                bail!("thread record exceeds its page budget; original preserved");
            }
            break;
        }
        let event = match serde_json::from_slice::<serde_json::Value>(&line) {
            Ok(value) if value.get("version").is_some() => {
                let record: references::Record =
                    serde_json::from_value(value).with_context(|| {
                        format!("invalid referenced thread event {}", cursor.ordinal)
                    })?;
                Some(record.hydrate(root)?)
            }
            Ok(value) => serde_json::from_value::<ThreadEvent>(value).ok(),
            Err(_) => None,
        };
        // Preserve physical ordinals even if a legacy record is malformed.
        let event = event.unwrap_or_else(|| {
            ThreadEvent::with_text(
                "note",
                "[journal event unavailable: invalid record; source preserved]",
            )
        });
        let size = serde_json::to_vec(&event)?.len();
        if retained.saturating_add(size) > JOURNAL_PAGE_BYTES as usize {
            if events.is_empty() {
                bail!("hydrated thread event exceeds its page budget; original preserved");
            }
            break;
        }
        retained += size;
        events.push(event);
        cursor.offset += read as u64;
        cursor.ordinal += 1;
        scanned += read as u64;
        records += 1;
    }
    let next_cursor = if cursor.offset < cursor.length {
        Some(
            serde_json::to_vec(&cursor)?
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        )
    } else {
        None
    };
    Ok(ThreadPage {
        events,
        first_ordinal,
        ordinals: (first_ordinal..cursor.ordinal).collect(),
        next_cursor,
    })
}

fn read_events(root: &Path, path: &Path) -> Result<Vec<ThreadEvent>> {
    let mut events = Vec::new();
    let mut cursor = None;
    let mut retained = 0usize;
    // Full callers get an explicit failure, never silently incomplete history.
    for _ in 0..(JOURNAL_SCAN_BYTES / JOURNAL_PAGE_BYTES) {
        let page = read_event_page(root, path, cursor.as_deref())?;
        for event in page.events {
            retained = retained.saturating_add(serde_json::to_vec(&event)?.len());
            if retained > JOURNAL_READ_BYTES || events.len() >= 4096 {
                bail!("thread read exceeds its 32 MiB/4096-event budget; journal preserved; use concordia thread read <id> and follow --cursor pages");
            }
            events.push(event);
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Ok(events);
        }
    }
    bail!("thread scan exceeds 256 MiB; journal preserved; use concordia thread read <id> and follow --cursor pages")
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

/// Canonical bounded map: verbatim user intent, exact source pointers, no assistant/tool body.
pub fn build_map(
    events: &[ThreadEvent],
    thread_id: &str,
    max_chars: usize,
    stored: bool,
    workspace: Option<&crate::workspace_state::WorkspaceState>,
    note: Option<&str>,
) -> String {
    let id = if stored && !thread_id.is_empty() {
        thread_id
    } else {
        "(memory)"
    };
    let mut lines = vec![format!(
        "[concordia map] thread {id} - v2 - tools/approvals NOT transferred"
    )];
    if let Some(note) = note {
        lines[0].push_str(&format!(" - {}", one_line(note, 240)));
    }
    if let Some(state) = workspace {
        lines.push(crate::redact::redact(&crate::workspace_state::summary(state)).into_owned());
    }
    let source_pointer = |ordinal: usize| {
        if stored && !thread_id.is_empty() {
            format!("concordia thread read {thread_id} --range {ordinal}-{ordinal}")
        } else {
            "attached native-source indexes (in-memory turn number only)".to_owned()
        }
    };
    let user_indices = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.k == "user" && !event.text().trim().is_empty())
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    for (index, event) in events.iter().enumerate() {
        let ordinal = index + 1;
        let pointer = source_pointer(ordinal);
        let line = match event.k.as_str() {
            "user" if !event.text().trim().is_empty() => {
                if user_indices.len() > 2
                    && Some(&index) != user_indices.first()
                    && Some(&index) != user_indices.last()
                {
                    lines.push(format!("user pointer: turn {ordinal} -> {pointer}"));
                    continue;
                }
                if user_indices.first() == Some(&index) {
                    format!("asked: {}", one_line(event.text(), 240))
                } else {
                    format!("user turn {ordinal}: {}", one_line(event.text(), 240))
                }
            }
            "assistant" => format!("assistant pointer: turn {ordinal} -> {pointer}"),
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
                    format!("files: {detail}; tool pointer ({label}): turn {ordinal} -> {pointer}")
                } else {
                    format!("tool pointer ({label}): turn {ordinal} -> {pointer}")
                }
            }
            "switch" | "note" | "usage" => {
                format!("{} pointer: turn {ordinal} -> {pointer}", event.k)
            }
            _ => continue,
        };
        lines.push(line);
    }
    if stored && !thread_id.is_empty() {
        lines.push(format!(
            "more: concordia thread read {thread_id} [--range A-B | --tail N | --grep TEXT]"
        ));
    } else {
        lines.push("more: concordia chat index SESSION --account ACCOUNT --provider PROVIDER; thread log is off (not stored)".into());
    }
    let mut out = lines.join("\n");
    if out.chars().count() > max_chars {
        let header = &lines[0];
        let footer = lines.last().expect("map always has a source instruction");
        let base = header.chars().count() + footer.chars().count() + 1;
        if base > max_chars {
            out = cap_chars(&format!("{header}\n{footer}"), max_chars).to_owned();
        } else {
            let intents = [
                lines.iter().find(|line| line.starts_with("asked:")),
                lines
                    .iter()
                    .rev()
                    .find(|line| line.starts_with("user turn ")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
            let mut compact = vec![header.clone()];
            let mut remaining = max_chars - base;
            for (index, line) in intents.iter().enumerate() {
                let share = remaining / (intents.len() - index);
                if share > 1 {
                    let text = cap_chars(line, share - 1).to_owned();
                    remaining -= text.chars().count() + 1;
                    compact.push(text);
                }
            }
            compact.push(footer.clone());
            out = compact.join("\n");
        }
    }
    out
}

/// Map events from the in-memory transcript (thread log off). `Card` messages carry
/// `title \x1f detail \x1f status` in their text.
pub fn events_from_messages(messages: &[PackMessage], provider: &str) -> Vec<ThreadEvent> {
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
    read_events_text_from(events, opts, 1, None)
}

fn read_events_text_from(
    events: &[ThreadEvent],
    opts: &ReadOpts,
    first: usize,
    ordinals: Option<&[usize]>,
) -> String {
    let mut rows: Vec<(usize, &ThreadEvent)> = events
        .iter()
        .enumerate()
        .map(|(i, e)| {
            (
                ordinals
                    .and_then(|ordinals| ordinals.get(i))
                    .copied()
                    .unwrap_or(i + first),
                e,
            )
        })
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
    let usage = "usage: concordia thread list [--json] | map <id> | read <id> [--cursor TOKEN] [--tail N] [--grep TEXT] [--range A-B] [--max-chars N]".to_string();
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
            let mut cursor = None;
            let mut it = args[2..].iter();
            while let Some(a) = it.next() {
                let mut val = |name: &str| -> std::result::Result<String, (String, i32)> {
                    it.next()
                        .cloned()
                        .ok_or_else(|| (format!("{name} needs a value\n{usage}"), 2))
                };
                match (sub.as_str(), a.as_str()) {
                    ("read", "--cursor") => cursor = Some(val("--cursor")?),
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
            let log = ThreadLog::open(state_root, &id);
            if sub == "map" {
                let cap = opts.max_chars.min(MAP_CHARS);
                log.read_map(cap)
                    .map(|map| map + "\n")
                    .map_err(|error| (format!("{error:#}"), 1))
            } else {
                if opts.max_chars > 1024 * 1024
                    || opts.tail.is_some_and(|tail| tail > JOURNAL_PAGE_EVENTS)
                {
                    return Err(("thread read accepts at most 1 MiB of display text and 256 tail events per page".into(), 2));
                }
                let page = if opts.tail.is_some() {
                    log.read_tail_page(cursor.as_deref(), &opts)
                } else {
                    log.read_page(cursor.as_deref())
                }
                .map_err(|e| (format!("{e:#}"), 1))?;
                let mut out = String::new();
                if cursor.is_some() || page.next_cursor.is_some() || opts.tail.is_some() {
                    out.push_str(&format!("[journal page starting at event {}; bounded journal selection; original journal retained]\n", page.first_ordinal));
                }
                out.push_str(&read_events_text_from(
                    &page.events,
                    &opts,
                    page.first_ordinal,
                    Some(&page.ordinals),
                ));
                if let Some(next) = page.next_cursor {
                    out.push_str(&format!(
                        "[next page: concordia thread read {id} --cursor {next}]\n"
                    ));
                }
                Ok(out)
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
        let m = build_map(&[], "abc", 1600, true, None, None);
        assert!(m.starts_with("[concordia map] thread abc"));
        assert!(m
            .lines()
            .last()
            .unwrap()
            .contains("concordia thread read abc"));
        assert!(m.chars().count() <= 1600);
    }

    #[test]
    fn thread_logging_off_writes_no_thread_files() {
        let root = tempfile::tempdir().unwrap();
        assert!(!enabled(root.path()));
        assert!(start(root.path()).is_none());
        assert!(!threads_dir(root.path()).exists());
    }

    #[test]
    fn unstored_map_says_so() {
        let m = build_map(&[user("hi")], "", 1600, false, None, None);
        assert!(m.contains("not stored"));
        assert!(!m.contains("thread read"));
    }

    #[test]
    fn map_layout_preserves_intent_metadata_and_exact_source_ordinals() {
        let mut ev = vec![user("fix the login redirect"), asst("looking")];
        for p in ["src/a.rs", "src/b.rs", "src/a.rs"] {
            ev.push(ThreadEvent::card("Edit", Some(p), "done"));
        }
        ev.push(ThreadEvent::card("Bash", Some("cargo test"), "done"));
        ev.push(ThreadEvent::card("Bash", Some("boom"), "error"));
        ev.push(user("now the logout"));
        ev.push(asst("done with it"));
        let m = build_map(&ev, "id1", 1600, true, None, None);
        assert!(m.contains("asked: fix the login redirect"), "{m}");
        assert!(m.contains("user turn 8: now the logout"), "{m}");
        assert!(m.contains("files: src/a.rs; tool pointer (Edit): turn 3"));
        assert!(m.contains("files: src/b.rs; tool pointer (Edit): turn 4"));
        assert!(m.contains("concordia thread read id1 --range 6-6"));
        assert!(m.contains("assistant pointer: turn 9"));
        assert!(!m.contains("cargo test") && !m.contains("boom"));
        assert!(!m.contains("looking") && !m.contains("done with it"));
    }

    #[test]
    fn map_preserves_repeated_user_ordinals_without_copying_error_bodies() {
        let events = vec![
            user("same request"),
            ThreadEvent::card("shell", Some("error one"), "error"),
            ThreadEvent::card("shell", Some("error two"), "error"),
            ThreadEvent::card("shell", Some("error one"), "error"),
            user("same request"),
        ];
        let map = build_map(&events, "fixture", 1600, true, None, None);
        assert!(map.contains("asked: same request"));
        assert!(map.contains("user turn 5: same request"));
        for ordinal in 2..=4 {
            assert!(map.contains(&format!(
                "concordia thread read fixture --range {ordinal}-{ordinal}"
            )));
        }
        assert!(!map.contains("error one") && !map.contains("error two"));
    }

    #[test]
    fn map_preserves_path_metadata_and_withholds_nonpath_tool_bodies() {
        let mut ev = vec![user("go")];
        for i in 0..15 {
            ev.push(ThreadEvent::card(
                "Read",
                Some(&format!("src/f{i}.rs")),
                "done",
            ));
        }
        ev.push(ThreadEvent::card("Read", Some("not a path here"), "done"));
        let m = build_map(&ev, "id", 5000, true, None, None);
        for index in 0..15 {
            assert!(m.contains(&format!("files: src/f{index}.rs;")), "{m}");
        }
        assert!(m.contains("tool pointer (Read): turn 17"));
        assert!(!m.contains("not a path"));
    }

    #[test]
    fn map_budget_preserves_source_instruction_and_withholds_bodies() {
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
            let m = build_map(
                &ev,
                "3f9a1111-2222-3333-4444-555555555555",
                max,
                true,
                None,
                None,
            );
            assert!(
                m.chars().count() <= max,
                "max {max} got {}",
                m.chars().count()
            );
        }
        let m = build_map(&ev, "id", 1600, true, None, None);
        assert!(m.chars().count() <= 1600);
        // Compact maps retain the exact source instruction without copying provider bodies.
        let tight = build_map(&ev, "id", 700, true, None, None);
        assert!(!tight.contains("cmd number") && !tight.contains(&"r".repeat(500)));
        assert!(tight.contains("more: concordia thread read id"), "{tight}");
    }

    #[test]
    fn huge_and_unicode_text_cut_on_char_boundaries() {
        let s = "é日本語✓".repeat(30_000);
        let e = ThreadEvent::with_text("user", &s);
        assert_eq!(e.text().chars().count(), MAX_EVENT_TEXT);
        let ev = vec![e, asst(&s)];
        for max in [1600usize, 333, 101] {
            let m = build_map(&ev, "x", max, true, None, None);
            assert!(m.chars().count() <= max);
        }
    }

    #[test]
    fn note_goes_into_header() {
        let m = build_map(
            &[user("a")],
            "",
            1600,
            false,
            None,
            Some("PARTIAL journal: first page only"),
        );
        assert!(m
            .lines()
            .next()
            .unwrap()
            .contains("PARTIAL journal: first page only"));
        assert!(m.contains("tools/approvals NOT transferred"));
    }

    #[test]
    fn map_is_deterministic() {
        let events = vec![
            user("fix the login"),
            asst("implementation detail must not leak"),
        ];
        assert_eq!(
            build_map(&events, "thread-1", 1600, true, None, None),
            build_map(&events, "thread-1", 1600, true, None, None)
        );
    }

    #[test]
    fn map_keeps_the_current_concordia_map_marker() {
        let map = build_map(
            &[user("keep the handoff readable")],
            "thread-1",
            1600,
            true,
            None,
            None,
        );

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
        let map = build_map(&events, "thread-1", 1600, true, None, None);
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
            assert!(
                build_map(&events, "thread-1", cap, true, None, None)
                    .chars()
                    .count()
                    <= cap
            );
        }
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
        let pins = log.read_pins_unchecked().unwrap();
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
        assert!(build_map(&hydrated, b.id(), 1600, true, None, None).contains(body));
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
        assert!(build_map(&ev, "", 1600, false, None, None).contains("files: a/b.rs"));
    }
}
