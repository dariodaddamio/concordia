//! Revision-checked pointers into native session files. Persist metadata, never chat bodies.
use crate::{accounts, sessions, store::ProviderKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const TAIL_BYTES: u64 = 8 * 1024 * 1024;
const HEAD_BYTES: u64 = 64 * 1024;
const LINE_BYTES: usize = 256 * 1024;
const MAX_MESSAGES: usize = 128;
const INDEX_BYTES: u64 = 256 * 1024;
const EXCERPT_BYTES: usize = 8192;

#[path = "cursor_history.rs"]
mod cursor_history;
#[path = "memory_files.rs"]
pub mod files;
#[path = "memory_rank.rs"]
pub mod rank;
#[path = "memory_relations.rs"]
pub mod relations;
#[path = "memory_retrieval.rs"]
pub mod retrieval;

enum Source {
    File(File),
    Replay(std::io::Cursor<Vec<u8>>),
}

impl Read for Source {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::File(file) => file.read(buffer),
            Self::Replay(replay) => replay.read(buffer),
        }
    }
}
impl Seek for Source {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        match self {
            Self::File(file) => file.seek(position),
            Self::Replay(replay) => replay.seek(position),
        }
    }
}
impl Source {
    fn len(&self) -> Result<u64, String> {
        match self {
            Self::File(file) => file.metadata().map(|m| m.len()).map_err(|e| e.to_string()),
            Self::Replay(replay) => Ok(replay.get_ref().len() as u64),
        }
    }
    fn identity(&self) -> Result<String, String> {
        match self {
            Self::File(file) => identity(file),
            Self::Replay(_) => Ok("cursor-acp-v1".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Binding {
    pub provider: ProviderKind,
    pub account: String,
    pub home: PathBuf,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub start: u64,
    pub len: usize,
    pub sha256: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceIndex {
    pub version: u32,
    pub binding: Binding,
    pub relative_file: PathBuf,
    pub file_identity: String,
    pub source_len: u64,
    pub anchor_len: u64,
    pub anchor_sha256: String,
    pub messages: Vec<Span>,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<HistoryPage>,
}

/// An immutable page in a forward scan. The cursor is this page's content-addressed index id.
/// Only byte ranges and hashes are persisted; appends never extend an in-flight snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryPage {
    pub start: u64,
    pub end: u64,
    pub next: Option<u64>,
    pub message_base: usize,
    pub skipped_records: usize,
    pub skip_prefix: bool,
    pub parent: Option<String>,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub home_id: String,
    pub file: PathBuf,
    pub provider: ProviderKind,
    pub account: String,
    pub session_id: String,
    pub messages: usize,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<HistorySummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_protocol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySummary {
    pub first_message: usize,
    pub scanned_through: u64,
    pub snapshot_bytes: u64,
    pub next_cursor: Option<String>,
    pub skipped_records: usize,
    pub complete: bool,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Excerpt {
    pub source: Summary,
    pub message: usize,
    pub role: String,
    pub source_byte_start: u64,
    pub source_byte_end: u64,
    pub sha256: String,
    /// Exact UTF-8 bytes from the native JSONL event, possibly one bounded chunk.
    pub raw: String,
    pub next_offset: Option<usize>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}
fn directory(root: &Path) -> PathBuf {
    root.join(".ao/native-memory")
}
fn normalize_home(home: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(home).map_err(|_| "native account home is unavailable".into())
}

impl Binding {
    pub fn new(
        provider: ProviderKind,
        account: &str,
        home: &Path,
        session_id: &str,
    ) -> Result<Self, String> {
        if account.is_empty()
            || account.len() > 128
            || session_id.is_empty()
            || session_id.len() > 128
            || account
                .chars()
                .chain(session_id.chars())
                .any(char::is_control)
        {
            return Err("invalid native source identity".into());
        }
        Ok(Self {
            provider,
            account: account.into(),
            home: normalize_home(home)?,
            session_id: session_id.into(),
        })
    }

    fn account_home(&self, root: &Path) -> Result<PathBuf, String> {
        let account = accounts::get_account(root, &self.account)
            .ok_or("native source account is unavailable")?;
        let home = account
            .home_path
            .as_ref()
            .ok_or("native source account has no explicit home")?;
        if account.provider != self.provider || normalize_home(home)? != normalize_home(&self.home)?
        {
            return Err("native source binding changed; refusing another account/home".into());
        }
        normalize_home(home)
    }

    fn verify(&self, root: &Path) -> Result<PathBuf, String> {
        let home = self.account_home(root)?;
        let source_root = fs::canonicalize(home.join(match self.provider {
            ProviderKind::Codex => "sessions",
            _ => "projects",
        }))
        .map_err(|_| "native session source directory is unavailable")?;
        if !source_root.starts_with(&home) {
            return Err("native session directory escapes the bound home".into());
        }
        Ok(source_root)
    }
}

fn open_source(path: &Path, allowed: &Path) -> Result<File, String> {
    let canonical = fs::canonicalize(path).map_err(|_| "native source unavailable or moved")?;
    if !canonical.starts_with(allowed)
        || canonical.extension().is_none_or(|e| e != "jsonl")
        || canonical
            .components()
            .any(|part| part.as_os_str() == "subagents")
    {
        return Err("source is outside the registered native session tree".into());
    }
    let file = File::open(&canonical).map_err(|_| "native source cannot be opened")?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file()
        || fs::canonicalize(path).map_err(|_| "native source moved while opening")? != canonical
    {
        return Err("native source changed while opening".into());
    }
    Ok(file)
}

fn read_at(file: &mut Source, offset: u64, len: usize) -> Result<Vec<u8>, String> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut bytes = vec![0; len];
    file.read_exact(&mut bytes)
        .map_err(|_| "native source was truncated or became unreadable")?;
    Ok(bytes)
}

use crate::file_identity::of as identity;

fn message_role(value: &Value, provider: ProviderKind, session: &str) -> Option<(String, bool)> {
    if provider == ProviderKind::Cursor {
        if value["method"] != "session/update"
            || value.pointer("/params/sessionId").and_then(Value::as_str) != Some(session)
        {
            return None;
        }
        let role = match value
            .pointer("/params/update/sessionUpdate")
            .and_then(Value::as_str)?
        {
            "user_message_chunk" => "user",
            "agent_message_chunk" => "assistant",
            "tool_call" | "tool_call_update" => "tool",
            _ => return None,
        };
        return Some((role.into(), false));
    }
    if let Some(id) = value.get("sessionId").and_then(Value::as_str) {
        if id != session {
            return None;
        }
    }
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
    let event_fallback = provider == ProviderKind::Codex && kind == "event_msg";
    let payload = if provider == ProviderKind::Codex {
        value.get("payload").unwrap_or(value)
    } else {
        value.get("message").unwrap_or(value)
    };
    if event_fallback {
        return match payload.get("type").and_then(Value::as_str) {
            Some("user_message") => Some(("user".into(), true)),
            Some("agent_message") => Some(("assistant".into(), true)),
            _ => None,
        };
    }
    if provider == ProviderKind::Codex && kind != "response_item" {
        return None;
    }
    if provider == ProviderKind::Claude && !matches!(kind, "user" | "assistant") {
        return None;
    }
    if provider == ProviderKind::Codex
        && matches!(
            payload.get("type").and_then(Value::as_str),
            Some(
                "function_call"
                    | "function_call_output"
                    | "custom_tool_call"
                    | "custom_tool_call_output"
            )
        )
    {
        return Some(("tool".into(), false));
    }
    let role = payload.get("role").and_then(Value::as_str).unwrap_or(kind);
    if !matches!(role, "user" | "assistant") {
        return None;
    }
    let content = payload.get("content")?;
    let has_text = content.as_str().is_some_and(|text| !text.is_empty())
        || content.as_array().is_some_and(|blocks| {
            blocks.iter().any(|b| {
                matches!(
                    b.get("type").and_then(Value::as_str),
                    Some("text" | "input_text" | "output_text") | None
                ) && b
                    .get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|t| !t.is_empty())
            })
        });
    let has_tool = content.as_array().is_some_and(|blocks| {
        blocks.iter().any(|block| {
            matches!(
                block.get("type").and_then(Value::as_str),
                Some("tool_use" | "tool_result")
            )
        })
    });
    if has_text {
        Some((role.into(), false))
    } else {
        has_tool.then(|| ("tool".into(), false))
    }
}

/// Display text from the same validated native Cursor replay used by exact pointers.
/// Text is returned in memory only; the caller applies its own transcript/display budget.
pub fn cursor_text(root: &Path, binding: &Binding) -> Result<Vec<(String, String)>, String> {
    if binding.provider != ProviderKind::Cursor {
        return Err("Cursor history requires a Cursor binding".into());
    }
    let source = cursor_history::open(root, binding)?;
    let Source::Replay(replay) = source else {
        return Err("invalid Cursor history transport".into());
    };
    let mut messages: Vec<(String, String)> = Vec::new();
    let mut previous = None;
    for raw in replay.get_ref().split(|b| *b == b'\n') {
        let Ok(value) = serde_json::from_slice::<Value>(raw) else {
            continue;
        };
        let Some((role, _)) = message_role(&value, ProviderKind::Cursor, &binding.session_id)
        else {
            continue;
        };
        if !matches!(role.as_str(), "user" | "assistant") {
            continue;
        }
        let Some(text) = value
            .pointer("/params/update/content/text")
            .and_then(Value::as_str)
        else {
            continue;
        };
        let key = value
            .pointer("/params/update/messageId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(|id| (role.clone(), id.to_owned()));
        if key.is_some() && key == previous {
            if let Some((_, content)) = messages.last_mut() {
                content.push_str(text);
            }
        } else {
            messages.push((role, text.into()));
        }
        previous = key;
    }
    Ok(messages)
}

/// Last native Codex context measurement, not cumulative billed usage. Unsupported formats and
/// measurements invalidated by model changes/compaction remain unknown. No provider process runs.
pub fn context_measurement(
    root: &Path,
    binding: &Binding,
    model: &str,
) -> Result<Option<crate::capacity::Measurement>, String> {
    if binding.provider != ProviderKind::Codex {
        return Ok(None);
    }
    let (mut file, index) = open_bound(root, binding)?;
    let start = index.source_len.saturating_sub(TAIL_BYTES);
    let bytes = read_at(&mut file, start, (index.source_len - start) as usize)?;
    let begin = if start > 0 {
        bytes
            .iter()
            .position(|b| *b == b'\n')
            .map_or(bytes.len(), |n| n + 1)
    } else {
        0
    };
    let mut measurement = None;
    let mut native_model = None;
    for raw in bytes[begin..].split(|b| *b == b'\n') {
        if raw.len() > LINE_BYTES {
            measurement = None;
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(raw) else {
            continue;
        };
        if v["type"] == "compacted" {
            measurement = None;
        }
        if v["type"] == "turn_context" {
            let next = v
                .pointer("/payload/model")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if native_model != next {
                measurement = None;
            }
            native_model = next;
        }
        if v["type"] == "event_msg" && v["payload"]["type"] == "token_count" {
            measurement = v
                .pointer("/payload/info/last_token_usage/total_tokens")
                .and_then(Value::as_u64)
                .zip(
                    v.pointer("/payload/info/model_context_window")
                        .and_then(Value::as_u64),
                )
                .filter(|(_, window)| *window > 0)
                .map(|(used, window)| crate::capacity::Measurement { used, window });
        }
    }
    if native_model.as_deref() != Some(model) || model == "?" {
        return Ok(None);
    }
    let (mut check, current) = open_bound(root, binding)?;
    if current.file_identity != index.file_identity
        || current.source_len != index.source_len
        || digest(&read_at(&mut check, start, bytes.len())?) != digest(&bytes)
    {
        return Err("native context source changed during measurement".into());
    }
    Ok(measurement)
}

/// Verify source identity and all retained record hashes without retaining conversation text.
pub fn verify_source(root: &Path, id: &str) -> Result<Summary, String> {
    let index = load(root, id)?;
    let mut source = open_index_source(root, &index)?;
    for span in &index.messages {
        let raw = read_at(&mut source, span.start, span.len)?;
        if digest(&raw) != span.sha256 {
            return Err("native source changed since indexing".into());
        }
    }
    if index.binding.provider != ProviderKind::Cursor {
        let _ = open_index_source(root, &index)?;
    }
    Ok(summary(id, &index))
}

/// Recover a known completed worker's display result from its immutable native pointer.
pub fn assistant_text(root: &Path, id: &str) -> Result<String, String> {
    let index = load(root, id)?;
    let mut source = open_index_source(root, &index)?;
    let mut text = String::new();
    for span in index.messages.iter().filter(|m| m.role == "assistant") {
        let raw = read_at(&mut source, span.start, span.len)?;
        if digest(&raw) != span.sha256 {
            return Err("worker source changed since completion".into());
        }
        let value: Value = serde_json::from_slice(&raw).map_err(|_| "invalid worker source")?;
        let payload = match index.binding.provider {
            ProviderKind::Codex => &value["payload"],
            ProviderKind::Claude => &value["message"],
            ProviderKind::Cursor => &value["params"]["update"],
        };
        let content = &payload["content"];
        let rendered = if let Some(blocks) = content.as_array() {
            blocks
                .iter()
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            content
                .as_str()
                .or_else(|| content["text"].as_str())
                .or_else(|| payload["message"].as_str())
                .unwrap_or("")
                .to_owned()
        };
        if text.len() + rendered.len() > 1024 * 1024 {
            return Err("worker result exceeds display budget; inspect exact source chunks".into());
        }
        text.push_str(&rendered);
        if index.binding.provider != ProviderKind::Cursor {
            text.push('\n');
        }
    }
    let _ = open_index_source(root, &index)?;
    if text.trim().is_empty() {
        return Err("no assistant result in the recorded source window".into());
    }
    let suffix = if index.truncated {
        "\n[Bounded native window; inspect older source pages for omitted history.]"
    } else {
        ""
    };
    Ok(format!("{}{suffix}", crate::chat_cli::terminal_safe(&text)))
}

pub fn index_session(
    root: &Path,
    provider: ProviderKind,
    account: &str,
    session_id: &str,
) -> Result<Summary, String> {
    let account_def = accounts::get_account(root, account).ok_or("unknown native account")?;
    if account_def.provider != provider {
        return Err("native account/provider mismatch".into());
    }
    let home = account_def.home_path.ok_or("native account has no home")?;
    index_bound(root, &Binding::new(provider, account, &home, session_id)?)
}

pub fn index_bound(root: &Path, binding: &Binding) -> Result<Summary, String> {
    let (mut file, mut index) = open_bound(root, binding)?;
    let source_len = index.source_len;
    let start = source_len.saturating_sub(TAIL_BYTES);
    let bytes = read_at(&mut file, start, (source_len - start) as usize)?;
    let skip = if start > 0 && read_at(&mut file, start - 1, 1)? != b"\n" {
        bytes
            .iter()
            .position(|b| *b == b'\n')
            .map_or(bytes.len(), |i| i + 1)
    } else {
        0
    };
    let mut offset = start + skip as u64;
    let mut messages = Vec::new();
    let mut fallback = Vec::new();
    let mut truncated = start > 0;
    for line in bytes[skip..].split_inclusive(|b| *b == b'\n') {
        if line.len() > LINE_BYTES {
            truncated = true;
            offset += line.len() as u64;
            continue;
        }
        if let Ok(value) = serde_json::from_slice::<Value>(line) {
            if let Some((role, is_fallback)) =
                message_role(&value, binding.provider, &binding.session_id)
            {
                let span = Span {
                    start: offset,
                    len: line.len(),
                    sha256: digest(line),
                    role,
                };
                if is_fallback {
                    fallback.push(span);
                } else {
                    messages.push(span);
                }
            }
        } else if !line.iter().all(u8::is_ascii_whitespace) {
            truncated = true;
        }
        offset += line.len() as u64;
    }
    if !messages.iter().any(|span| span.role != "tool") {
        messages.extend(fallback);
        messages.sort_by_key(|span| span.start);
    }
    if messages.len() > MAX_MESSAGES {
        truncated = true;
        messages.drain(..messages.len() - MAX_MESSAGES);
    }
    index.messages = messages;
    index.truncated = truncated;
    save_index(root, &mut file, index)
}

fn open_bound(root: &Path, binding: &Binding) -> Result<(Source, SourceIndex), String> {
    if binding.provider == ProviderKind::Cursor {
        let home = binding.account_home(root)?;
        let mut source = cursor_history::open(root, binding)?;
        let source_len = source.len()?;
        let anchor_len = source_len.min(HEAD_BYTES);
        let head = read_at(&mut source, 0, anchor_len as usize)?;
        return Ok((
            source,
            SourceIndex {
                version: 1,
                binding: Binding {
                    home,
                    ..binding.clone()
                },
                relative_file: "acp-history".into(),
                file_identity: "cursor-acp-v1".into(),
                source_len,
                anchor_len,
                anchor_sha256: digest(&head),
                messages: Vec::new(),
                truncated: false,
                history: None,
            },
        ));
    }
    let allowed = binding.verify(root)?;
    let (_, path) = sessions::find_native_record(
        root,
        &binding.account,
        Some(binding.provider),
        &binding.session_id,
    )?
    .ok_or("explicit native session was not found")?;
    let canonical = fs::canonicalize(&path).map_err(|_| "native source unavailable")?;
    let relative_file = canonical
        .strip_prefix(&allowed)
        .map_err(|_| "native source escapes its bound session tree")?
        .to_path_buf();
    let mut file = Source::File(open_source(&canonical, &allowed)?);
    let file_identity = file.identity()?;
    let source_len = file.len()?;
    let anchor_len = source_len.min(HEAD_BYTES);
    let head = read_at(&mut file, 0, anchor_len as usize)?;
    let owns_session = head
        .split(|b| *b == b'\n')
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .any(|v| {
            if binding.provider == ProviderKind::Codex {
                v.get("type").and_then(Value::as_str) == Some("session_meta")
                    && v.pointer("/payload/id").and_then(Value::as_str)
                        == Some(binding.session_id.as_str())
            } else {
                v.get("sessionId").and_then(Value::as_str) == Some(binding.session_id.as_str())
            }
        });
    if !owns_session {
        return Err(
            "native source does not confirm the exact session identity in its bounded header"
                .into(),
        );
    }
    let index = SourceIndex {
        version: 1,
        binding: binding.clone(),
        relative_file,
        file_identity,
        source_len,
        anchor_len,
        anchor_sha256: digest(&head),
        messages: Vec::new(),
        truncated: false,
        history: None,
    };
    Ok((file, index))
}

fn save_index(root: &Path, file: &mut Source, mut index: SourceIndex) -> Result<Summary, String> {
    let source_len = index.source_len;
    let anchor_len = index.anchor_len;
    let binding = &index.binding;
    index.binding.home = normalize_home(&binding.home)?;
    if file.identity()? != index.file_identity
        || file.len()? < source_len
        || digest(&read_at(file, 0, anchor_len as usize)?) != index.anchor_sha256
    {
        return Err("native source changed while indexing; retry explicitly".into());
    }
    let encoded = serde_json::to_vec(&index).map_err(|e| e.to_string())?;
    if encoded.len() as u64 > INDEX_BYTES {
        return Err("native pointer index exceeds its metadata budget".into());
    }
    let id = digest(&encoded);
    crate::state_io::write_atomic(&directory(root).join(format!("{id}.json")), &encoded)
        .map_err(|e| e.to_string())?;
    Ok(summary(&id, &index))
}

fn summary(id: &str, index: &SourceIndex) -> Summary {
    Summary {
        id: id.into(),
        home_id: digest(index.binding.home.to_string_lossy().as_bytes()),
        file: index.relative_file.clone(),
        provider: index.binding.provider,
        account: index.binding.account.clone(),
        session_id: index.binding.session_id.clone(),
        messages: index.messages.len(),
        truncated: index.truncated,
        history: index.history.as_ref().map(|page| HistorySummary {
            first_message: page.message_base + 1,
            scanned_through: page.end,
            snapshot_bytes: index.source_len,
            next_cursor: page.next.map(|_| id.to_owned()),
            skipped_records: page.skipped_records,
            complete: page.next.is_none() && page.skipped_records == 0,
            parent: page.parent.clone(),
        }),
        native_protocol: (index.binding.provider == ProviderKind::Cursor)
            .then(|| "acp/session.load".into()),
    }
}

/// Scan one bounded page from the beginning of a native history, or continue an immutable
/// cursor. Each call reads at most two 8 MiB pages plus the bounded identity header.
pub fn index_history(
    root: &Path,
    binding: &Binding,
    after: Option<&str>,
) -> Result<Summary, String> {
    let (mut file, mut index) = open_bound(root, binding)?;
    let mut start = 0;
    let mut message_base = 0;
    let mut skipped_records = 0;
    let mut skip_prefix = false;
    if let Some(cursor) = after {
        let previous = load(root, cursor)?;
        let page = previous
            .history
            .as_ref()
            .ok_or("cursor is not a whole-history page")?;
        if previous.binding != index.binding
            || previous.file_identity != index.file_identity
            || previous.relative_file != index.relative_file
        {
            return Err("whole-history cursor belongs to another native source".into());
        }
        if index.source_len < previous.source_len
            || digest(&read_at(&mut file, 0, previous.anchor_len as usize)?)
                != previous.anchor_sha256
            || digest(&read_at(
                &mut file,
                page.start,
                (page.end - page.start) as usize,
            )?) != page.sha256
        {
            return Err("whole-history cursor is stale: its source changed".into());
        }
        start = page
            .next
            .ok_or("whole-history snapshot already scanned; start a new index for appended data")?;
        message_base = page.message_base + previous.messages.len();
        if message_base > usize::MAX - MAX_MESSAGES - 1 {
            return Err("whole-history ordinal exceeds the supported range".into());
        }
        skipped_records = page.skipped_records;
        skip_prefix = page.skip_prefix;
        // Preserve the original header and length when the live log has appended new records.
        index.source_len = previous.source_len;
        index.anchor_len = previous.anchor_len;
        index.anchor_sha256 = previous.anchor_sha256;
    }
    let len = (index.source_len - start).min(TAIL_BYTES) as usize;
    let bytes = read_at(&mut file, start, len)?;
    let at_end = start + len as u64 == index.source_len;
    let mut consumed = 0;
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        if skip_prefix {
            consumed += line.len();
            skip_prefix = !line.ends_with(b"\n");
            continue;
        }
        if !line.ends_with(b"\n") && !at_end {
            if consumed == 0 {
                // A single oversized record must not trap the cursor at the same offset.
                skipped_records += 1;
                consumed += line.len();
                skip_prefix = true;
            }
            break;
        }
        if index.messages.len() == MAX_MESSAGES {
            break;
        }
        if let Ok(value) = serde_json::from_slice::<Value>(line) {
            if let Some((role, _)) = message_role(&value, binding.provider, &binding.session_id) {
                index.messages.push(Span {
                    start: start + consumed as u64,
                    len: line.len(),
                    sha256: digest(line),
                    role,
                });
            }
        } else if !line.iter().all(u8::is_ascii_whitespace) {
            skipped_records += 1;
        }
        consumed += line.len();
    }
    let end = start + consumed as u64;
    index.history = Some(HistoryPage {
        start,
        end,
        next: (end < index.source_len).then_some(end),
        message_base,
        skipped_records,
        skip_prefix,
        parent: after.map(str::to_owned),
        sha256: digest(&bytes[..consumed]),
    });
    index.truncated = end < index.source_len || skipped_records > 0;
    save_index(root, &mut file, index)
}

/// Expand an exact source pointer into whole-history pages without guessing a session or home.
pub fn history_from_pointer(root: &Path, id: &str) -> Result<Summary, String> {
    let index = load(root, id)?;
    index_history(root, &index.binding, index.history.as_ref().map(|_| id))
}

fn load(root: &Path, id: &str) -> Result<SourceIndex, String> {
    if !valid_id(id) {
        return Err("invalid native pointer id (expected 64 lowercase hex characters)".into());
    }
    let file = File::open(directory(root).join(format!("{id}.json")))
        .map_err(|_| "native pointer index is unavailable")?;
    let mut bytes = Vec::new();
    file.take(INDEX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > INDEX_BYTES || digest(&bytes) != id {
        return Err("native pointer index is corrupt or replaced".into());
    }
    let index: SourceIndex =
        serde_json::from_slice(&bytes).map_err(|_| "malformed native pointer index")?;
    if index.version != 1 || index.messages.len() > MAX_MESSAGES || index.anchor_len > HEAD_BYTES {
        return Err("unsupported native pointer index".into());
    }
    if let Some(page) = &index.history {
        if page.start > page.end
            || page.end > index.source_len
            || page.end - page.start > TAIL_BYTES
            || page.next != (page.end < index.source_len).then_some(page.end)
            || page.next.is_some_and(|next| next <= page.start)
            || page.message_base > usize::MAX - MAX_MESSAGES - 1
            || !valid_id(&page.sha256)
            || page.parent.as_ref().is_some_and(|id| !valid_id(id))
        {
            return Err("invalid whole-history page metadata".into());
        }
    }
    let record_limit = if index.history.is_some() {
        TAIL_BYTES as usize
    } else {
        LINE_BYTES
    };
    let mut previous_end = 0;
    let mut total_bytes = 0u64;
    for span in &index.messages {
        let end = span
            .start
            .checked_add(span.len as u64)
            .ok_or("native source range overflow")?;
        total_bytes = total_bytes
            .checked_add(span.len as u64)
            .ok_or("native source byte budget overflow")?;
        if span.len > record_limit
            || end > index.source_len
            || span.start < previous_end
            || total_bytes > TAIL_BYTES
            || !valid_id(&span.sha256)
            || index
                .history
                .as_ref()
                .is_some_and(|page| span.start < page.start || end > page.end)
        {
            return Err("invalid native source ranges or byte budget".into());
        }
        previous_end = end;
    }
    Ok(index)
}

pub fn describe(root: &Path, id: &str) -> Result<Value, String> {
    describe_page(root, id, None, 20)
}

pub fn describe_page(
    root: &Path,
    id: &str,
    from_message: Option<usize>,
    limit: usize,
) -> Result<Value, String> {
    if !(1..=50).contains(&limit) {
        return Err("native source page limit must be 1..50".into());
    }
    let index = load(root, id)?;
    let _file = open_index_source(root, &index)?;
    let first = index.history.as_ref().map_or(1, |p| p.message_base + 1);
    let start = from_message
        .unwrap_or(first)
        .checked_sub(first)
        .filter(|start| *start <= index.messages.len())
        .ok_or("message is outside this index page")?;
    let end = (start + limit).min(index.messages.len());
    Ok(
        json!({"source":summary(id, &index),"next_unlisted_message":(end < index.messages.len()).then_some(first+end),
        "messages":index.messages.iter().enumerate().skip(start).take(limit).map(|(i,m)|json!({"message":first+i,"role":m.role,"byte_start":m.start,"bytes":m.len,"sha256":m.sha256})).collect::<Vec<_>>()}),
    )
}

fn open_index_source(root: &Path, index: &SourceIndex) -> Result<Source, String> {
    if index.relative_file.is_absolute()
        || index
            .relative_file
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("unsafe native source path in pointer".into());
    }
    let mut file = if index.binding.provider == ProviderKind::Cursor {
        index.binding.account_home(root)?;
        if index.relative_file != Path::new("acp-history") {
            return Err("invalid Cursor protocol source".into());
        }
        cursor_history::open(root, &index.binding)?
    } else {
        let allowed = index.binding.verify(root)?;
        Source::File(open_source(&allowed.join(&index.relative_file), &allowed)?)
    };
    if file.identity()? != index.file_identity || file.len()? < index.source_len {
        return Err(
            "native source is stale: replaced or truncated; index this session again".into(),
        );
    }
    if digest(&read_at(&mut file, 0, index.anchor_len as usize)?) != index.anchor_sha256 {
        return Err("native source is stale: its original header changed".into());
    }
    Ok(file)
}

pub fn read(
    root: &Path,
    id: &str,
    message: usize,
    offset: usize,
    max_bytes: usize,
) -> Result<Excerpt, String> {
    if !(1..=EXCERPT_BYTES).contains(&max_bytes) {
        return Err("native excerpt max_bytes must be 1..8192".into());
    }
    let index = load(root, id)?;
    let mut file = open_index_source(root, &index)?;
    let first = index.history.as_ref().map_or(1, |p| p.message_base + 1);
    let span = message
        .checked_sub(first)
        .and_then(|i| index.messages.get(i))
        .ok_or("native message ordinal is outside the indexed range")?;
    let record_limit = if index.history.is_some() {
        TAIL_BYTES as usize
    } else {
        LINE_BYTES
    };
    if span.len > record_limit
        || span
            .start
            .checked_add(span.len as u64)
            .is_none_or(|end| end > index.source_len)
    {
        return Err("invalid native source range".into());
    }
    let bytes = read_at(&mut file, span.start, span.len)?;
    if digest(&bytes) != span.sha256 {
        return Err("native source is stale: indexed message changed or was compacted".into());
    }
    // Re-open files to detect replacement, but do not issue a second Cursor replay in one read.
    if index.binding.provider != ProviderKind::Cursor
        && open_index_source(root, &index)?.identity()? != index.file_identity
    {
        return Err("native source replaced during read".into());
    }
    let raw = std::str::from_utf8(&bytes).map_err(|_| "native source is no longer UTF-8")?;
    if offset >= raw.len() || !raw.is_char_boundary(offset) {
        return Err("native excerpt offset is outside the message or splits UTF-8".into());
    }
    let mut end = offset.saturating_add(max_bytes).min(raw.len());
    while end > offset && !raw.is_char_boundary(end) {
        end -= 1;
    }
    if end == offset {
        return Err("max_bytes is too small for the next UTF-8 character".into());
    }
    Ok(Excerpt {
        source: summary(id, &index),
        message,
        role: span.role.clone(),
        source_byte_start: span.start + offset as u64,
        source_byte_end: span.start + end as u64,
        sha256: span.sha256.clone(),
        raw: raw[offset..end].into(),
        next_offset: (end < raw.len()).then_some(end),
    })
}

/// CLI and MCP share the same validated, bounded operations.
pub fn command(root: &Path, args: &[String]) -> Result<String, String> {
    let Some(operation) = args.first().map(String::as_str) else {
        return Err("usage: chat index SESSION --account ID --provider codex|claude|cursor [--scope recent|whole --cursor INDEX] | chat excerpt INDEX [--message N --offset N --max-bytes N] | chat sources INDEX [--from-message N --limit N]".into());
    };
    let positional = args
        .get(1)
        .ok_or("native source command needs an exact session/index id")?;
    let mut options = std::collections::HashMap::new();
    let (pairs, remainder) = args[2..].as_chunks::<2>();
    for pair in pairs {
        if options.insert(pair[0].as_str(), pair[1].as_str()).is_some() {
            return Err("duplicate native source option".into());
        }
    }
    if !remainder.is_empty() {
        return Err("native source option needs a value".into());
    }
    let number = |key: &str, default: usize| -> Result<usize, String> {
        options
            .get(key)
            .map(|v| v.parse().map_err(|_| format!("invalid {key}")))
            .unwrap_or(Ok(default))
    };
    let allowed = match operation {
        "index" => &["--account", "--provider", "--scope", "--cursor"][..],
        "excerpt" => &["--message", "--offset", "--max-bytes"][..],
        "sources" => &["--from-message", "--limit"][..],
        _ => return Err("unknown native source operation".into()),
    };
    if options.keys().any(|key| !allowed.contains(key)) {
        return Err("unsupported native source option".into());
    }
    let value = match operation {
        "index" => {
            let provider = crate::commands::parse_provider(
                options
                    .get("--provider")
                    .ok_or("index requires --provider")?,
            )
            .ok_or("invalid native provider")?;
            let account = *options.get("--account").ok_or("index requires --account")?;
            let indexed = match options.get("--scope").copied().unwrap_or("recent") {
                "recent" if !options.contains_key("--cursor") => {
                    index_session(root, provider, account, positional)?
                }
                "whole" => {
                    let def =
                        accounts::get_account(root, account).ok_or("unknown native account")?;
                    if def.provider != provider {
                        return Err("native account/provider mismatch".into());
                    }
                    let binding = Binding::new(
                        provider,
                        account,
                        &def.home_path.ok_or("native account has no home")?,
                        positional,
                    )?;
                    index_history(root, &binding, options.get("--cursor").copied())?
                }
                _ => {
                    return Err(
                        "index --scope must be recent or whole; --cursor requires whole".into(),
                    )
                }
            };
            serde_json::to_value(indexed).map_err(|e| e.to_string())?
        }
        "sources" => describe_page(
            root,
            positional,
            options
                .get("--from-message")
                .map(|_| number("--from-message", 1))
                .transpose()?,
            number("--limit", 20)?,
        )?,
        "excerpt" => serde_json::to_value(read(
            root,
            positional,
            number("--message", 1)?,
            number("--offset", 0)?,
            number("--max-bytes", 4096)?,
        )?)
        .map_err(|e| e.to_string())?,
        _ => unreachable!(),
    };
    let output = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    if output.chars().count() > 16_000 {
        return Err(
            "native source result exceeds 16000 characters; request a smaller excerpt".into(),
        );
    }
    Ok(output + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn fixture(provider: ProviderKind, root: &Path, account: &str) -> (PathBuf, String) {
        let home = root.join(account);
        let folder = home.join(if provider == ProviderKind::Codex {
            "sessions"
        } else {
            "projects/fixture"
        });
        fs::create_dir_all(&folder).unwrap();
        let session = "11111111-2222-4333-8444-555555555555".to_string();
        let path = folder.join(format!("{session}.jsonl"));
        let header = if provider == ProviderKind::Codex {
            json!({"type":"session_meta","payload":{"id":session}})
        } else {
            json!({"type":"system","sessionId":session})
        };
        fs::write(&path, format!("{header}\n")).unwrap();
        crate::store::upsert_stored_account(
            root,
            crate::store::StoredAccount {
                id: account.into(),
                provider,
                home_path: Some(home.to_string_lossy().into_owned()),
                label: account.into(),
                created_at: "0".into(),
            },
        )
        .unwrap();
        (path, session)
    }
    fn message(provider: ProviderKind, text: &str) -> String {
        if provider == ProviderKind::Codex {
            json!({"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":text}]}}).to_string() + "\n"
        } else {
            json!({"type":"user","message":{"role":"user","content":text}}).to_string() + "\n"
        }
    }
    fn append(path: &Path, bytes: &[u8]) {
        fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
    #[test]
    fn native_context_uses_last_request_and_invalidates_after_compaction_or_model_change() {
        let dir = tempfile::tempdir().unwrap();
        let (path, session) = fixture(ProviderKind::Codex, dir.path(), "fixture");
        let binding = Binding::new(
            ProviderKind::Codex,
            "fixture",
            &dir.path().join("fixture"),
            &session,
        )
        .unwrap();
        let context =
            json!({"type":"turn_context","payload":{"model":"fixture-model"}}).to_string() + "\n";
        append(&path, context.as_bytes());
        let count = json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":9999999},"last_token_usage":{"total_tokens":80},"model_context_window":100}}}).to_string() + "\n";
        append(&path, count.as_bytes());
        assert_eq!(
            context_measurement(dir.path(), &binding, "fixture-model").unwrap(),
            Some(crate::capacity::Measurement {
                used: 80,
                window: 100
            })
        );
        assert_eq!(
            context_measurement(dir.path(), &binding, "other-model").unwrap(),
            None
        );
        append(&path, b"{\"type\":\"compacted\",\"payload\":{}}\n");
        assert_eq!(
            context_measurement(dir.path(), &binding, "fixture-model").unwrap(),
            None
        );
        append(&path, count.as_bytes());
        append(
            &path,
            b"{\"type\":\"turn_context\",\"payload\":{\"model\":\"other-model\"}}\n",
        );
        assert_eq!(
            context_measurement(dir.path(), &binding, "other-model").unwrap(),
            None
        );
    }

    #[test]
    fn retrieval_separates_exact_hits_from_verified_links_and_rejects_stale_sources() {
        for provider in [ProviderKind::Codex, ProviderKind::Claude] {
            let dir = tempfile::tempdir().unwrap();
            let (path, session) = fixture(provider, dir.path(), "fixture");
            let workspace = dir.path().join("historical-project");
            let header = if provider == ProviderKind::Codex {
                json!({"type":"session_meta","payload":{"id":session,"cwd":workspace}})
            } else {
                json!({"type":"system","sessionId":session,"cwd":workspace})
            };
            fs::write(&path, format!("{header}\n")).unwrap();
            let mut request: Value =
                serde_json::from_str(&message(provider, "distinctive needle 雪")).unwrap();
            request["timestamp"] = json!("2026-11-02T23:59:59.123-07:00");
            append(&path, format!("{request}\n").as_bytes());
            let paths: Vec<_> = ["src/雪 name.rs", ".env", ".git/index", "../escape.rs"]
                .into_iter()
                .map(str::to_owned)
                .chain(std::iter::once(
                    dir.path()
                        .join("other-project/same.rs")
                        .to_string_lossy()
                        .into_owned(),
                ))
                .chain((0..80).map(|n| format!("src/historical-{n}.rs")))
                .collect();
            let tool = if provider == ProviderKind::Codex {
                let patch = paths.iter().map(|path| format!("*** Update File: {path}\n@@\n-DO_NOT_COPY_NATIVE_BODY\n+synthetic replacement\n")).collect::<String>();
                json!({"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","input":format!("*** Begin Patch\n{patch}*** End Patch")}})
            } else {
                json!({"type":"assistant","message":{"role":"assistant","content":paths.iter().enumerate().map(|(n,path)| json!({"type":"tool_use","name":"Read","id":format!("fixture-{n}"),"input":{"file_path":path,"ignored":"DO_NOT_COPY_NATIVE_BODY"}})).collect::<Vec<_>>()}})
            };
            append(&path, format!("{tool}\n").as_bytes());
            let parent = index_session(dir.path(), provider, "fixture", &session).unwrap();
            let facts = files::read_page(dir.path(), &parent.id).unwrap();
            assert_eq!(facts.workspace.as_ref(), Some(&workspace));
            assert_eq!(facts.facts.len(), 64);
            assert!(facts.overflow && facts.incomplete);
            assert_eq!(facts.facts[0].path, Path::new("src/雪 name.rs"));
            assert!(
                !workspace.exists(),
                "historical workspace and referenced files are never opened"
            );
            assert!(facts
                .facts
                .iter()
                .all(|fact| !crate::workspace_paths::sensitive(&fact.path)
                    && !fact.path.is_absolute()));
            assert!(
                read(dir.path(), &parent.id, facts.facts[0].message, 0, 8192)
                    .unwrap()
                    .raw
                    .contains("雪 name.rs")
            );
            let listing = files::list(dir.path(), &parent.id, 0, None).unwrap();
            assert!(serde_json::to_vec_pretty(&listing).unwrap().len() <= 28_000);
            assert_eq!(listing["facts"].as_array().unwrap().len(), 16);
            assert_eq!(listing["next_offset"], 16);
            assert!(files::list(dir.path(), &parent.id, 16, None).is_err());
            let next =
                files::list(dir.path(), &parent.id, 16, listing["revision"].as_str()).unwrap();
            assert_eq!(next["facts"].as_array().unwrap().len(), 16);
            let cache_path = directory(dir.path())
                .join("file-facts")
                .join(format!("{}.json", parent.id));
            let fact_cache = fs::read(&cache_path).unwrap();
            assert!(!String::from_utf8_lossy(&fact_cache).contains("DO_NOT_COPY_NATIVE_BODY"));
            assert_eq!(
                index_session(dir.path(), provider, "fixture", &session)
                    .unwrap()
                    .id,
                parent.id,
                "derived facts must not change native pointer IDs"
            );
            let other_session = "22222222-3333-4444-8555-666666666666";
            let other_path = path.with_file_name(format!("{other_session}.jsonl"));
            let other_workspace = dir.path().join("other-project");
            let other_header = if provider == ProviderKind::Codex {
                json!({"type":"session_meta","payload":{"id":other_session,"cwd":other_workspace}})
            } else {
                json!({"type":"system","sessionId":other_session,"cwd":other_workspace})
            };
            fs::write(&other_path, format!("{other_header}\n")).unwrap();
            let mut collision: Value =
                serde_json::from_str(&message(provider, "needle keyword collision")).unwrap();
            collision["timestamp"] = json!("2026-11-03T06:59:59.123Z");
            append(&other_path, format!("{collision}\n{tool}\n").as_bytes());
            let other = index_session(dir.path(), provider, "fixture", other_session).unwrap();
            let goal = "synthetic-ranking-goal-v1";
            crate::goals::save_goals(
                dir.path(),
                &[crate::goals::Goal {
                    id: goal.into(),
                    text: "Synthetic retrieval context".into(),
                    done: false,
                    added_at: "0".into(),
                    closed_at: None,
                }],
            )
            .unwrap();
            relations::command(
                dir.path(),
                &["goal-link".into(), goal.into(), parent.id.clone()],
                dir.path(),
            )
            .unwrap();
            let ranked = |source, goal, file, from, workspace| {
                rank::recall(
                    dir.path(),
                    rank::Request {
                        text: "needle",
                        source,
                        account: Some("fixture"),
                        workspace,
                        goal,
                        file,
                        from,
                        at: Some("2026-11-03T06:59:59.123Z"),
                    },
                )
            };
            let goal_rank = ranked(None, Some(goal), None, None, None).unwrap();
            let scoped_goal = ranked(None, Some(goal), None, None, Some(dir.path())).unwrap();
            assert!(
                scoped_goal
                    .hits
                    .iter()
                    .any(|hit| hit.hit.source.id == parent.id
                        && hit
                            .reasons
                            .iter()
                            .any(|reason| reason.kind == "confirmed_goal_source")),
                "current workspace aliases must match canonical goal associations"
            );
            assert_eq!(
                goal_rank.hits[0].hit.source.id, parent.id,
                "confirmed association outranks keyword collisions"
            );
            assert!(goal_rank.hits[0]
                .reasons
                .iter()
                .any(|reason| reason.kind == "confirmed_goal_source"));
            assert!(goal_rank
                .hits
                .iter()
                .any(|hit| hit.hit.source.id == other.id && hit.score < 10_000));
            assert_eq!(
                serde_json::to_value(&goal_rank).unwrap(),
                serde_json::to_value(ranked(None, Some(goal), None, None, None).unwrap()).unwrap(),
                "same snapshot produces identical rank order and evidence"
            );
            assert_eq!(
                rank::timestamp_ms("2026-11-02T23:59:59.123-07:00"),
                rank::timestamp_ms("2026-11-03T06:59:59.123Z")
            );
            assert!(rank::timestamp_ms("2026-02-29T00:00:00Z").is_none());
            assert!(rank::timestamp_ms("2024-02-29T00:00:00+00:00").is_some());
            assert!(rank::timestamp_ms("2026-11-03T06:59:60Z").is_none());
            let file_rank = ranked(
                Some(&parent.id),
                None,
                Some("src/雪 name.rs"),
                None,
                Some(workspace.as_path()),
            )
            .unwrap();
            assert!(file_rank.hits.iter().any(|hit| hit
                .reasons
                .iter()
                .any(|reason| reason.kind == "exact_historical_file")));
            let different_root = ranked(
                Some(&other.id),
                None,
                Some("src/雪 name.rs"),
                None,
                Some(workspace.as_path()),
            )
            .unwrap();
            assert!(
                different_root.hits.iter().all(|hit| hit
                    .reasons
                    .iter()
                    .all(|reason| reason.kind != "exact_historical_file")),
                "same-named files in different workspaces cannot collide"
            );
            retrieval::link(dir.path(), &parent.id, &other.id, "user-confirmed").unwrap();
            let related = ranked(Some(&other.id), None, None, Some(&parent.id), None).unwrap();
            assert!(related.hits.iter().any(|hit| hit
                .reasons
                .iter()
                .any(|reason| reason.kind == "explicit_relationship")));
            retrieval::detach(dir.path(), &parent.id, &other.id).unwrap();
            let report = retrieval::recall(
                dir.path(),
                "needle",
                Some(&parent.id),
                Some("fixture"),
                None,
            )
            .unwrap();
            assert_eq!(report.hits.len(), 1);
            let hit = &report.hits[0];
            assert!(hit.relationship.starts_with("lexical suggestion"));
            assert!(read(dir.path(), &hit.source.id, hit.message, 0, 8192)
                .unwrap()
                .raw
                .contains("needle"));
            assert!(
                retrieval::recall(dir.path(), "needle", Some(&parent.id), Some("other"), None)
                    .is_err()
            );
            append(&path, message(provider, "later record").as_bytes());
            let child = index_session(dir.path(), provider, "fixture", &session).unwrap();
            retrieval::link(dir.path(), &parent.id, &child.id, "user-confirmed").unwrap();
            assert_eq!(
                retrieval::links(dir.path(), &parent.id).unwrap()["links"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            retrieval::detach(dir.path(), &parent.id, &child.id).unwrap();
            assert!(retrieval::links(dir.path(), &parent.id).unwrap()["links"]
                .as_array()
                .unwrap()
                .is_empty());
            let stored =
                fs::read_to_string(directory(dir.path()).join(format!("{}.json", parent.id)))
                    .unwrap();
            assert!(!stored.contains("needle"));
            let original_binding = load(dir.path(), &parent.id).unwrap().binding;
            let other_binding = load(dir.path(), &other.id).unwrap().binding;
            retrieval::observe_handoff(dir.path(), &original_binding, &other_binding, dir.path())
                .unwrap();
            let handoff = ranked(Some(&other.id), None, None, Some(&parent.id), None).unwrap();
            assert!(handoff.hits.iter().any(|hit| hit
                .reasons
                .iter()
                .any(|reason| reason.detail.starts_with("observed Concordia handoff"))));
            fs::write(&cache_path, b"{broken").unwrap();
            assert!(files::read_page(dir.path(), &parent.id).is_err());
            assert_eq!(fs::read(&cache_path).unwrap(), b"{broken");
            fs::write(&cache_path, &fact_cache).unwrap();
            let bytes = fs::read(&path).unwrap();
            fs::write(
                &path,
                String::from_utf8(bytes)
                    .unwrap()
                    .replace("needle", "edited"),
            )
            .unwrap();
            assert!(retrieval::recall(dir.path(), "needle", Some(&parent.id), None, None).is_err());
            assert!(
                files::read_page(dir.path(), &parent.id).is_err(),
                "stale source facts must be withheld"
            );
            let after_rewrite = ranked(None, Some(goal), None, None, None).unwrap();
            assert!(after_rewrite.incomplete);
            assert!(
                after_rewrite
                    .hits
                    .iter()
                    .all(|hit| hit.hit.source.id != parent.id),
                "ranking never bypasses stale source verification"
            );
        }
    }

    #[test]
    fn whole_history_pages_reach_every_record_and_keep_snapshot_ordinals_after_append() {
        for provider in [ProviderKind::Codex, ProviderKind::Claude] {
            let dir = tempfile::tempdir().unwrap();
            let (path, session) = fixture(provider, dir.path(), "fixture");
            let expected: Vec<_> = (0..400)
                .map(|i| message(provider, &format!("history token {i} 雪")))
                .collect();
            append(&path, expected.join("").as_bytes());
            let binding =
                Binding::new(provider, "fixture", &dir.path().join("fixture"), &session).unwrap();
            let mut page = index_history(dir.path(), &binding, None).unwrap();
            assert_eq!(page.history.as_ref().unwrap().first_message, 1);
            append(
                &path,
                message(provider, "appended after the snapshot").as_bytes(),
            );
            let mut ordinal = 1;
            loop {
                let listing = describe_page(dir.path(), &page.id, Some(ordinal), 50).unwrap();
                assert_eq!(listing["messages"][0]["message"], ordinal);
                assert_eq!(
                    listing["next_unlisted_message"].is_u64(),
                    page.messages > 50
                );
                let stored =
                    fs::read_to_string(directory(dir.path()).join(format!("{}.json", page.id)))
                        .unwrap();
                assert!(!stored.contains("history token"));
                for _ in 0..page.messages {
                    assert_eq!(
                        read(dir.path(), &page.id, ordinal, 0, 8192).unwrap().raw,
                        expected[ordinal - 1]
                    );
                    ordinal += 1;
                }
                if page.history.as_ref().unwrap().next_cursor.is_none() {
                    break;
                }
                page = index_history(dir.path(), &binding, Some(&page.id)).unwrap();
            }
            assert_eq!(ordinal, 401);
            assert!(page.history.as_ref().unwrap().complete);
            assert!(index_history(dir.path(), &binding, Some(&page.id)).is_err());
        }
    }

    #[test]
    fn whole_history_scan_crosses_the_old_tail_bound_and_reports_oversized_gaps() {
        let dir = tempfile::tempdir().unwrap();
        let (path, session) = fixture(ProviderKind::Codex, dir.path(), "fixture");
        append(
            &path,
            message(ProviderKind::Codex, "oldest original").as_bytes(),
        );
        append(
            &path,
            message(ProviderKind::Codex, &"x".repeat(TAIL_BYTES as usize + 1000)).as_bytes(),
        );
        append(
            &path,
            message(ProviderKind::Codex, "newest original").as_bytes(),
        );
        let binding = Binding::new(
            ProviderKind::Codex,
            "fixture",
            &dir.path().join("fixture"),
            &session,
        )
        .unwrap();
        let first = index_history(dir.path(), &binding, None).unwrap();
        assert!(read(dir.path(), &first.id, 1, 0, 8192)
            .unwrap()
            .raw
            .contains("oldest original"));
        let mut page = first;
        let mut count = 0;
        while page.history.as_ref().unwrap().next_cursor.is_some() {
            let previous = page.history.as_ref().unwrap().scanned_through;
            page = index_history(dir.path(), &binding, Some(&page.id)).unwrap();
            assert!(page.history.as_ref().unwrap().scanned_through > previous);
            count += 1;
            assert!(count < 5, "cursor must make bounded forward progress");
        }
        let history = page.history.as_ref().unwrap();
        assert_eq!(history.skipped_records, 1);
        assert!(
            !history.complete,
            "an oversized source event must not be called complete"
        );
        assert!(read(dir.path(), &page.id, history.first_message, 0, 8192)
            .unwrap()
            .raw
            .contains("newest original"));
    }

    #[test]
    fn history_cursors_reject_other_accounts_and_rewrites_past_the_identity_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let (path, session) = fixture(ProviderKind::Codex, dir.path(), "fixture");
        let (other, _) = fixture(ProviderKind::Codex, dir.path(), "other");
        for i in 0..200 {
            append(
                &path,
                message(ProviderKind::Codex, &format!("{i}:{}", "a".repeat(1000))).as_bytes(),
            );
        }
        let binding = Binding::new(
            ProviderKind::Codex,
            "fixture",
            &dir.path().join("fixture"),
            &session,
        )
        .unwrap();
        let first = index_history(dir.path(), &binding, None).unwrap();
        let second_binding = Binding::new(
            ProviderKind::Codex,
            "other",
            &dir.path().join("other"),
            &session,
        )
        .unwrap();
        append(
            &other,
            message(ProviderKind::Codex, "other source").as_bytes(),
        );
        assert!(index_history(dir.path(), &second_binding, Some(&first.id))
            .unwrap_err()
            .contains("another native source"));
        let index = load(dir.path(), &first.id).unwrap();
        let span = index.messages.last().unwrap();
        assert!(span.start > HEAD_BYTES);
        let mut file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(span.start + span.len as u64 - 8))
            .unwrap();
        file.write_all(b"z").unwrap();
        assert!(index_history(dir.path(), &binding, Some(&first.id))
            .unwrap_err()
            .contains("stale"));
        assert!(read(dir.path(), &first.id, 128, 0, 8192).is_err());
    }

    #[test]
    fn exact_native_readback_survives_append_and_never_copies_chat_bodies_into_the_index() {
        for provider in [ProviderKind::Codex, ProviderKind::Claude] {
            let dir = tempfile::tempdir().unwrap();
            let (path, session) = fixture(provider, dir.path(), "fixture-account");
            let raw = message(provider, "exact request with whitespace  and \nnew line");
            append(&path, raw.as_bytes());
            let indexed = index_session(dir.path(), provider, "fixture-account", &session).unwrap();
            let stored =
                fs::read(directory(dir.path()).join(format!("{}.json", indexed.id))).unwrap();
            assert!(!String::from_utf8(stored).unwrap().contains("exact request"));
            assert_eq!(read(dir.path(), &indexed.id, 1, 0, 8192).unwrap().raw, raw);
            append(&path, message(provider, "later").as_bytes());
            assert_eq!(read(dir.path(), &indexed.id, 1, 0, 8192).unwrap().raw, raw);
            assert_eq!(
                index_session(dir.path(), provider, "fixture-account", &session)
                    .unwrap()
                    .messages,
                2
            );
        }
    }
    #[test]
    fn same_session_id_in_two_account_homes_has_distinct_pointers_and_retargeting_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let (a, session) = fixture(ProviderKind::Codex, dir.path(), "first");
        let (b, _) = fixture(ProviderKind::Codex, dir.path(), "second");
        append(&a, message(ProviderKind::Codex, "one").as_bytes());
        append(&b, message(ProviderKind::Codex, "two").as_bytes());
        let first = index_session(dir.path(), ProviderKind::Codex, "first", &session).unwrap();
        let second = index_session(dir.path(), ProviderKind::Codex, "second", &session).unwrap();
        assert_ne!(first.id, second.id);
        let mut account = crate::store::list_stored_accounts(dir.path())
            .into_iter()
            .find(|a| a.id == "first")
            .unwrap();
        account.home_path = Some(dir.path().join("second").to_string_lossy().into_owned());
        crate::store::upsert_stored_account(dir.path(), account).unwrap();
        assert!(read(dir.path(), &first.id, 1, 0, 100)
            .unwrap_err()
            .contains("binding changed"));
    }
    #[test]
    fn replacement_truncation_rewrite_deletion_and_corrupt_index_are_explicitly_unavailable() {
        for case in ["replace", "truncate", "rewrite", "delete", "index"] {
            let dir = tempfile::tempdir().unwrap();
            let (path, session) = fixture(ProviderKind::Codex, dir.path(), "fixture-account");
            append(&path, message(ProviderKind::Codex, "original").as_bytes());
            let indexed =
                index_session(dir.path(), ProviderKind::Codex, "fixture-account", &session)
                    .unwrap();
            match case {
                "replace" => {
                    let original = fs::read(&path).unwrap();
                    fs::rename(&path, path.with_extension("old")).unwrap();
                    fs::write(&path, original).unwrap();
                }
                "truncate" => {
                    fs::OpenOptions::new()
                        .write(true)
                        .open(&path)
                        .unwrap()
                        .set_len(0)
                        .unwrap();
                }
                "rewrite" => {
                    let text = fs::read_to_string(&path)
                        .unwrap()
                        .replace("original", "modified");
                    fs::write(&path, text).unwrap();
                }
                "delete" => fs::remove_file(&path).unwrap(),
                "index" => fs::write(
                    directory(dir.path()).join(format!("{}.json", indexed.id)),
                    "{}",
                )
                .unwrap(),
                _ => unreachable!(),
            }
            assert!(read(dir.path(), &indexed.id, 1, 0, 8192).is_err(), "{case}");
        }
    }
    #[test]
    fn excerpts_are_chunked_without_guessing_or_splitting_unicode_and_reject_invalid_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let (path, session) = fixture(ProviderKind::Codex, dir.path(), "fixture-account");
        let raw = message(ProviderKind::Codex, &"\u{96ea}".repeat(5000));
        append(&path, raw.as_bytes());
        let indexed =
            index_session(dir.path(), ProviderKind::Codex, "fixture-account", &session).unwrap();
        let mut output = String::new();
        let mut offset = 0;
        loop {
            let chunk = read(dir.path(), &indexed.id, 1, offset, 1000).unwrap();
            output.push_str(&chunk.raw);
            let Some(next) = chunk.next_offset else { break };
            offset = next;
        }
        assert_eq!(output, raw);
        assert!(read(dir.path(), &indexed.id, 0, 0, 10).is_err());
        assert!(read(dir.path(), &indexed.id, 1, 0, 8193).is_err());
        assert!(read(dir.path(), "../auth", 1, 0, 10).is_err());
        assert!(command(
            dir.path(),
            &[
                "index".into(),
                session,
                "--account".into(),
                "fixture-account".into()
            ]
        )
        .is_err());
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;
    #[test]
    fn exact_range_rewrite_past_the_anchor_and_tool_results_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(home.join("sessions")).unwrap();
        let session = "11111111-2222-4333-8444-555555555555";
        let path = home.join("sessions").join(format!("{session}.jsonl"));
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
        let text = format!(
            "{}\n{}\n{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":session}}),
            json!({"type":"system","padding":"x".repeat(70_000)}),
            json!({"type":"response_item","payload":{"type":"function_call_output","output":"original tool result"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"original request"}})
        );
        fs::write(&path, &text).unwrap();
        let indexed = index_session(dir.path(), ProviderKind::Codex, "fixture", session).unwrap();
        assert_eq!(indexed.messages, 2);
        assert_eq!(
            read(dir.path(), &indexed.id, 1, 0, 4096).unwrap().role,
            "tool"
        );
        fs::write(
            &path,
            text.replace("original tool result", "modified tool result"),
        )
        .unwrap();
        assert!(read(dir.path(), &indexed.id, 1, 0, 4096)
            .unwrap_err()
            .contains("indexed message changed"));
        let binding = Binding::new(ProviderKind::Codex, "fixture", &home, session).unwrap();
        let (prefix, notices) = handoff_prefix(dir.path(), &[binding]);
        assert!(notices.is_empty());
        assert!(prefix.contains("chat excerpt"));
        assert!(
            !prefix.contains("modified tool result"),
            "handoff includes pointers rather than copied messages"
        );
    }
}

/// Build a bounded handoff directory on a worker, while keeping provider originals in place.
pub fn handoff_prefix(root: &Path, bindings: &[Binding]) -> (String, Vec<String>) {
    let mut sections = Vec::new();
    let mut notices = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for binding in bindings.iter().rev().take(8) {
        let key = serde_json::to_string(&(binding.provider, &binding.home, &binding.session_id))
            .unwrap_or_default();
        if !seen.insert(key) {
            continue;
        }
        if seen.len() > 3 {
            break;
        }
        match index_bound(root, binding) {
            Ok(source) => sections.push(format!(
                "{}: {}/{}/{}; {} indexed records{}",
                source.id,
                source.provider.as_str(),
                source.account,
                source.session_id,
                source.messages,
                if source.truncated {
                    " (recent bounded window; older records omitted)"
                } else {
                    ""
                }
            )),
            Err(error) => notices.push(format!(
                "native source {}/{} unavailable: {error}",
                binding.provider.as_str(),
                binding.account
            )),
        }
    }
    if sections.is_empty() && notices.is_empty() {
        return (String::new(), notices);
    }
    let mut body = sections.join("\n");
    if !notices.is_empty() {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(
            &notices
                .iter()
                .map(|notice| format!("UNAVAILABLE: {notice}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    let instructions = if sections.is_empty() {
        "No exact source is available; do not invent earlier messages or assume tool completion. Reconcile the workspace or ask for missing information."
    } else {
        "Use concordia chat sources INDEX [--from-message N] to page message ordinals. Read exact UTF-8 JSONL chunks with concordia chat excerpt INDEX --message N [--offset BYTE --max-bytes 4096]; follow next_offset. For older history use chat index SESSION --account ACCOUNT --provider PROVIDER --scope whole, following history.next_cursor with --cursor INDEX. Check source identity before continuing. Do not invent unavailable history."
    };
    (format!("[native source pointers]\n{body}\n{instructions}\nNative tool/approval state is not transferred."), notices)
}
