//! Metadata-only local file outputs reported by verified structured native events.
//! A report is not a claim that current disk bytes were authored by the provider.
use crate::provider::{
    interaction::InteractionOwner,
    stream_cards::{CardStatus, StreamCard},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_RECORDS: usize = 256;
const MAX_BYTES: usize = 2 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: String,
    pub owner: InteractionOwner,
    pub native_session: String,
    pub native_turn: Option<String>,
    pub event_id: String,
    pub source_revision: String,
    pub reported_path: String,
    pub local_path: Option<PathBuf>,
    pub kind: String,
    pub execution_directory_id: String,
}
impl Record {
    /// Explicit bounded historical source inspection; never reads current file contents.
    pub fn inspect_edit(&self, root: &Path) -> Result<Option<StreamCard>, String> {
        if self.owner.provider != crate::store::ProviderKind::Codex {
            return Ok(None);
        }
        let patches = crate::native_memory::historical_codex_patches(root, self)?;
        let mut reported = read(root, &self.owner.conversation_id)?
            .into_iter()
            .filter(|record| {
                record.owner == self.owner
                    && record.event_id == self.event_id
                    && record.source_revision == self.source_revision
            })
            .map(|record| record.reported_path)
            .collect::<Vec<_>>();
        if reported.is_empty() {
            reported.push(self.reported_path.clone());
        }
        let patches = patches
            .into_iter()
            .filter(|patch| {
                crate::provider::edit_display::matches_reported_paths(
                    patch,
                    &self.owner.execution_cwd,
                    &reported,
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        if patches.len() != 1 {
            return Err("reported file event has no unique matching native patch batch".into());
        }
        let patches = patches.into_iter().collect::<Vec<_>>();
        let mut cards = [StreamCard {
            id: format!("artifact:{}", self.id),
            title: "Edit".into(),
            status: CardStatus::Done,
            detail: Some(format!(
                "File: {}\nProvider reported a change; line diff unavailable.",
                self.reported_path
            )),
        }];
        if crate::provider::edit_display::enrich_codex(
            &mut cards,
            &patches,
            &self.owner.execution_cwd,
        )
        .is_empty()
        {
            return Ok(None);
        }
        let mut card = cards.into_iter().next().unwrap();
        card.detail = Some(format!(
            "{}\n\nVerified patch input from this completed native attempt.\n\n{}",
            card.detail.as_deref().unwrap_or_default(),
            self.card().detail.as_deref().unwrap_or_default()
        ));
        Ok(Some(card))
    }
    pub fn card(&self) -> StreamCard {
        let target = self
            .local_path
            .as_ref()
            .map(|path| path.to_string_lossy().replace('\\', "/"));
        let name = Path::new(&self.reported_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let path = target
            .map(|target| format!("[Open current file](<{target}>)"))
            .unwrap_or_else(|| {
                "Open unavailable: reported path is outside the execution directory or excluded"
                    .into()
            });
        StreamCard { id: format!("artifact:{}", self.id), title: format!("File output · {name}"), status: CardStatus::Done,
            detail: Some(format!("Provider reported {} · {}\n{}\nSource: {} / {}\nNative session: {}\nNative turn: {}\nRun: {}\nAttempt: {}\nEvent: {}\nSource revision: {}\nCurrent file bytes are unverified; opening reads the current disk version.", self.kind, self.reported_path, path, self.owner.provider.as_str(), self.owner.account, self.native_session, self.native_turn.as_deref().unwrap_or("not reported"), self.owner.run_id, self.owner.attempt_id, self.event_id, self.source_revision)) }
    }
    pub fn open_current(&self) -> Result<PathBuf, String> {
        let path = self
            .local_path
            .as_deref()
            .ok_or("This reported path has no allowed local-file action")?;
        if crate::workspaces::directory_id(&self.owner.execution_cwd)?
            != self.execution_directory_id
        {
            return Err("Reported file execution directory was replaced".into());
        }
        let lexical = local_root(&self.owner.execution_cwd);
        let relative = path
            .strip_prefix(&lexical)
            .map_err(|_| "Reported file is outside its execution directory")?;
        let root = self
            .owner
            .execution_cwd
            .canonicalize()
            .map_err(|_| "Output execution directory is unavailable")?;
        crate::workspace_paths::regular_file(&root, &root.join(relative))
            .map_err(|error| format!("Reported file unavailable or changed: {error}"))
    }
}
fn local_root(path: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    PathBuf::from(value.strip_prefix(r"\\?\").unwrap_or(&value))
}
fn identity(
    owner: &InteractionOwner,
    session: &str,
    turn: Option<&str>,
    event: &str,
    revision: &str,
    path: &str,
) -> String {
    let bytes = serde_json::to_vec(&(owner, session, turn, event, revision, path))
        .expect("artifact metadata serializes");
    format!("{:x}", Sha256::digest(bytes))
}
fn local_path(owner: &InteractionOwner, reported: &str) -> Option<PathBuf> {
    if reported.is_empty()
        || reported.len() > 4096
        || reported.chars().any(char::is_control)
        || reported.starts_with(r"\\")
        || reported.contains(['<', '>', '"', '|'])
    {
        return None;
    }
    let path = Path::new(reported);
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return None;
    }
    let root = local_root(&owner.execution_cwd);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let relative = path.strip_prefix(&root).ok()?;
    if relative
        .components()
        .any(|part| part.as_os_str().to_string_lossy().contains(':'))
    {
        return None;
    }
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || crate::workspace_paths::sensitive(&path)
    {
        return None;
    }
    Some(path)
}
/// Only completed Codex file-change items. Prose, tool arguments, unknown providers and failed
/// requests never become output records. The native event hash is not a disk content hash.
pub fn from_item(
    owner: &InteractionOwner,
    session: &str,
    turn: Option<&str>,
    item: &Value,
) -> Vec<Record> {
    if [&owner.conversation_id, &owner.run_id, &owner.attempt_id]
        .iter()
        .any(|id| uuid::Uuid::parse_str(id).is_err())
        || session.chars().any(char::is_control)
        || turn.is_some_and(|turn| {
            turn.is_empty() || turn.len() > 512 || turn.chars().any(char::is_control)
        })
    {
        return Vec::new();
    }
    let Ok(execution_directory_id) = crate::workspaces::directory_id(&owner.execution_cwd) else {
        return Vec::new();
    };
    if owner.provider != crate::store::ProviderKind::Codex
        || session.is_empty()
        || session.len() > 512
        || item.to_string().len() > MAX_BYTES
        || !matches!(item["type"].as_str(), Some("file_change" | "fileChange"))
        || !matches!(item["status"].as_str(), Some("completed" | "applied"))
        || item.get("error").is_some_and(|error| !error.is_null())
    {
        return Vec::new();
    }
    let Some(event) = item["id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control))
    else {
        return Vec::new();
    };
    let Some(changes) = item["changes"]
        .as_array()
        .filter(|changes| changes.len() <= 32)
    else {
        return Vec::new();
    };
    let revision = format!("{:x}", Sha256::digest(item.to_string().as_bytes()));
    changes
        .iter()
        .filter_map(|change| {
            let path = change["path"].as_str().filter(|path| {
                !path.is_empty() && path.len() <= 4096 && !path.chars().any(char::is_control)
            })?;
            let kind = change["kind"]
                .as_str()
                .or_else(|| change["kind"]["type"].as_str())?;
            if !matches!(kind, "add" | "update" | "delete") {
                return None;
            }
            Some(Record {
                id: identity(owner, session, turn, event, &revision, path),
                owner: owner.clone(),
                native_session: session.into(),
                native_turn: turn.map(str::to_owned),
                event_id: event.into(),
                source_revision: revision.clone(),
                reported_path: path.into(),
                local_path: local_path(owner, path),
                kind: kind.into(),
                execution_directory_id: execution_directory_id.clone(),
            })
        })
        .collect()
}

/// Convert Cursor's documented completed write/edit tool report into the shared artifact
/// registry. Cursor emits the request and result in one `tool_call` event, so `call_id` is the
/// native event identity and the successful result path is the provider-owned local-path report.
/// A started call, failed result, prose path, or a result without an explicit success path is not
/// evidence that a file was produced.
pub fn from_cursor_event(
    owner: &InteractionOwner,
    session: &str,
    turn: Option<&str>,
    event: &Value,
) -> Vec<Record> {
    if owner.provider != crate::store::ProviderKind::Cursor
        || event.get("type").and_then(Value::as_str) != Some("tool_call")
        || event.get("subtype").and_then(Value::as_str) != Some("completed")
        || event.to_string().len() > MAX_BYTES
    {
        return Vec::new();
    }
    let Some(native_event_id) = event
        .get("call_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 512)
    else {
        return Vec::new();
    };
    let Some((kind, call)) = event
        .get("tool_call")
        .and_then(Value::as_object)
        .and_then(|calls| {
            calls
                .iter()
                .find(|(name, _)| matches!(name.as_str(), "writeToolCall" | "editToolCall"))
        })
    else {
        return Vec::new();
    };
    if event.get("session_id").and_then(Value::as_str) != Some(session)
        || call.get("result").is_some_and(|result| {
            result.get("error").is_some_and(|error| !error.is_null())
                || result
                    .get("failure")
                    .is_some_and(|failure| !failure.is_null())
        })
    {
        return Vec::new();
    }
    let Some(success) = call.pointer("/result/success").filter(|v| v.is_object()) else {
        return Vec::new();
    };
    // A success object is required to carry a concrete path. Do not fall back to prose or the
    // request arguments: the completed native result is the provenance-bearing report.
    let Some(path) = success.get("path").and_then(Value::as_str).filter(|path| {
        !path.is_empty() && path.len() <= 4096 && !path.chars().any(char::is_control)
    }) else {
        return Vec::new();
    };
    let kind = match kind.as_str() {
        "writeToolCall" => "update",
        "editToolCall" => "update",
        _ => return Vec::new(),
    };
    let Ok(execution_directory_id) = crate::workspaces::directory_id(&owner.execution_cwd) else {
        return Vec::new();
    };
    if [&owner.conversation_id, &owner.run_id, &owner.attempt_id]
        .iter()
        .any(|id| uuid::Uuid::parse_str(id).is_err())
        || session.is_empty()
        || session.len() > 512
        || session.chars().any(char::is_control)
        || turn.is_some_and(|turn| {
            turn.is_empty() || turn.len() > 512 || turn.chars().any(char::is_control)
        })
    {
        return Vec::new();
    }
    let revision = format!("{:x}", Sha256::digest(event.to_string().as_bytes()));
    // Some native call IDs contain line breaks. Keep their identity in a visibly escaped
    // representation rather than injecting control characters into provenance cards.
    let event_id = if native_event_id.chars().any(char::is_control) {
        format!(
            "json:{}",
            serde_json::to_string(native_event_id).unwrap_or_default()
        )
    } else {
        native_event_id.to_owned()
    };
    vec![Record {
        id: identity(owner, session, turn, &event_id, &revision, path),
        owner: owner.clone(),
        native_session: session.into(),
        native_turn: turn.map(str::to_owned),
        event_id,
        source_revision: revision,
        reported_path: path.into(),
        local_path: local_path(owner, path),
        kind: kind.into(),
        execution_directory_id,
    }]
}
/// Track Claude's built-in Write/Edit tool use until the matching successful user tool result.
/// Request arguments alone are not output evidence; the result must carry native tool-result
/// metadata (`filePath`/`path` or `structuredPatch`) and the exact `tool_use_id` correlation.
pub fn claude_tool_event(
    pending: &mut HashMap<String, (String, String)>,
    owner: &InteractionOwner,
    session: &str,
    turn: Option<&str>,
    event: &Value,
) -> Option<Record> {
    if owner.provider != crate::store::ProviderKind::Claude
        || event.to_string().len() > MAX_BYTES
        || session.is_empty()
        || session.len() > 512
        || session.chars().any(char::is_control)
        || [&owner.conversation_id, &owner.run_id, &owner.attempt_id]
            .iter()
            .any(|id| uuid::Uuid::parse_str(id).is_err())
        || turn.is_some_and(|turn| {
            turn.is_empty() || turn.len() > 512 || turn.chars().any(char::is_control)
        })
        || event.get("session_id").and_then(Value::as_str) != Some(session)
    {
        return None;
    }
    let kind = event.get("type").and_then(Value::as_str)?;
    if kind == "assistant" {
        let blocks = event.pointer("/message/content")?.as_array()?;
        for block in blocks {
            if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let name = block.get("name").and_then(Value::as_str)?;
            if !matches!(name, "Write" | "Edit") {
                continue;
            }
            let id = block.get("id").and_then(Value::as_str).filter(|id| {
                !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control)
            })?;
            let path = block
                .pointer("/input/file_path")
                .and_then(Value::as_str)
                .filter(|path| {
                    !path.is_empty() && path.len() <= 4096 && !path.chars().any(char::is_control)
                })?;
            if pending.len() < 128 {
                let next = (name.into(), path.into());
                pending
                    .entry(id.into())
                    .and_modify(|previous| {
                        if previous != &next {
                            *previous = (String::new(), String::new());
                        }
                    })
                    .or_insert(next);
            }
        }
        return None;
    }
    if kind != "user" {
        return None;
    }
    let blocks = event.pointer("/message/content")?.as_array()?;
    for block in blocks {
        if block.get("type").and_then(Value::as_str) != Some("tool_result") {
            continue;
        }
        let id = block.get("tool_use_id").and_then(Value::as_str)?;
        let (tool, requested_path) = pending.remove(id)?;
        if block.get("is_error").is_some_and(|error| error != false)
            || !matches!(tool.as_str(), "Write" | "Edit")
            || blocks
                .iter()
                .filter(|block| block["type"] == "tool_result")
                .count()
                != 1
        {
            return None;
        }
        let metadata = event
            .get("tool_use_result")
            .filter(|value| value.is_object())?;
        let result_path = metadata
            .get("filePath")
            .or_else(|| metadata.get("file_path"))
            .or_else(|| metadata.get("path"))
            .and_then(Value::as_str);
        let has_patch = metadata
            .get("structuredPatch")
            .and_then(Value::as_array)
            .is_some_and(|patch| !patch.is_empty());
        if result_path.is_none() && !has_patch {
            return None;
        }
        let path = result_path.unwrap_or(&requested_path);
        if path != requested_path
            && !local_path(owner, path)
                .zip(local_path(owner, &requested_path))
                .is_some_and(|(reported, requested)| reported == requested)
        {
            return None;
        }
        let revision = format!("{:x}", Sha256::digest(event.to_string().as_bytes()));
        let execution_directory_id = crate::workspaces::directory_id(&owner.execution_cwd).ok()?;
        return Some(Record {
            id: identity(owner, session, turn, id, &revision, path),
            owner: owner.clone(),
            native_session: session.into(),
            native_turn: turn.map(str::to_owned),
            event_id: id.into(),
            source_revision: revision,
            reported_path: path.into(),
            local_path: local_path(owner, path),
            kind: "update".into(),
            execution_directory_id,
        });
    }
    None
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    version: u8,
    records: Vec<Record>,
}
fn regular(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err("Artifact metadata must not be linked".into());
                }
            }
            if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_BYTES as u64 {
                return Err("Artifact metadata is not a bounded regular file".into());
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
fn path(root: &Path, conversation: &str, create: bool) -> Result<PathBuf, String> {
    if uuid::Uuid::parse_str(conversation).is_err() {
        return Err("Artifact conversation identity is invalid".into());
    }
    let dir = root.join("artifact-index");
    if create {
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    }
    if dir.exists() {
        crate::workspaces::directory_id(&dir)?;
    }
    Ok(dir.join(format!("{conversation}.json")))
}
pub fn read(root: &Path, conversation: &str) -> Result<Vec<Record>, String> {
    let path = path(root, conversation, false)?;
    regular(&path)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let opened = file.metadata().map_err(|error| error.to_string())?;
    if !opened.is_file() || opened.len() > MAX_BYTES as u64 {
        return Err("Artifact metadata changed while opening".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if opened.file_attributes() & 0x400 != 0 {
            return Err("Artifact metadata changed to a link".into());
        }
    }
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("Artifact metadata exceeds its byte limit".into());
    }
    let index: Index = serde_json::from_slice(&bytes)
        .map_err(|_| "Unknown or invalid artifact metadata retained")?;
    if index.version != 1
        || index.records.len() > MAX_RECORDS
        || index.records.iter().any(|record| {
            record.owner.conversation_id != conversation
                || record.id
                    != identity(
                        &record.owner,
                        &record.native_session,
                        record.native_turn.as_deref(),
                        &record.event_id,
                        &record.source_revision,
                        &record.reported_path,
                    )
                || record.local_path != local_path(&record.owner, &record.reported_path)
                || record.id.len() != 64
                || record.source_revision.len() != 64
        })
    {
        return Err("Artifact metadata identity/version mismatch".into());
    }
    Ok(index.records)
}
pub fn save(root: &Path, record: &Record) -> Result<bool, String> {
    let path = path(root, &record.owner.conversation_id, true)?;
    let lock = path.with_extension("lock");
    regular(&lock)?;
    crate::state_io::with_lock(&lock, || {
        let mut records =
            read(root, &record.owner.conversation_id).map_err(std::io::Error::other)?;
        if records.iter().any(|old| old.id == record.id) {
            return Ok(false);
        }
        records.push(record.clone());
        if records.len() > MAX_RECORDS {
            records.drain(..records.len() - MAX_RECORDS);
        }
        let bytes = serde_json::to_vec(&Index {
            version: 1,
            records,
        })
        .map_err(std::io::Error::other)?;
        if bytes.len() > MAX_BYTES {
            return Err(std::io::Error::other(
                "Artifact metadata byte limit reached",
            ));
        }
        regular(&path).map_err(std::io::Error::other)?;
        crate::state_io::write_atomic(&path, &bytes)?;
        Ok(true)
    })
    .map_err(|error| error.to_string())
}

#[derive(Default)]
pub(crate) struct Collector {
    count: usize,
    seen: std::collections::HashSet<String>,
    limited: bool,
    claude_pending: HashMap<String, (String, String)>,
}
impl Collector {
    fn report_records(
        &mut self,
        records: impl IntoIterator<Item = Record>,
        progress: &Option<tokio::sync::mpsc::UnboundedSender<crate::provider::turn::TurnProgress>>,
    ) {
        for record in records {
            if self.count >= 32 {
                if !self.limited {
                    if let Some(tx) = progress {
                        let _ = tx.send(crate::provider::turn::TurnProgress::Notice("File output catalog reached its 32 records per attempt limit; native source events remain authoritative".into()));
                    }
                    self.limited = true;
                }
                return;
            }
            if !self.seen.insert(record.id.clone()) {
                continue;
            }
            self.count += 1;
            match save(&crate::store::default_store_root(), &record) {
                Ok(true) => {
                    if let Some(tx) = progress {
                        let _ = tx.send(crate::provider::turn::TurnProgress::Artifact(Box::new(
                            record,
                        )));
                    }
                }
                Ok(false) => {}
                Err(error) => {
                    if let Some(tx) = progress {
                        let _ = tx.send(crate::provider::turn::TurnProgress::Notice(format!(
                            "File output metadata was not retained: {error}"
                        )));
                    }
                }
            }
        }
    }

    pub fn report(
        &mut self,
        owner: Option<&InteractionOwner>,
        session: Option<&str>,
        turn: Option<&str>,
        item: &Value,
        progress: &Option<tokio::sync::mpsc::UnboundedSender<crate::provider::turn::TurnProgress>>,
    ) {
        let (Some(owner), Some(session)) = (owner, session) else {
            return;
        };
        self.report_records(from_item(owner, session, turn, item), progress);
    }

    /// Record a completed Cursor write/edit report using the same bounded registry and
    /// persistence path as Codex file-change items.
    pub fn report_cursor(
        &mut self,
        owner: Option<&InteractionOwner>,
        session: Option<&str>,
        turn: Option<&str>,
        event: &Value,
        progress: &Option<tokio::sync::mpsc::UnboundedSender<crate::provider::turn::TurnProgress>>,
    ) {
        let (Some(owner), Some(session)) = (owner, session) else {
            return;
        };
        self.report_records(from_cursor_event(owner, session, turn, event), progress);
    }

    pub fn report_claude(
        &mut self,
        owner: Option<&InteractionOwner>,
        session: Option<&str>,
        turn: Option<&str>,
        event: &Value,
        progress: &Option<tokio::sync::mpsc::UnboundedSender<crate::provider::turn::TurnProgress>>,
    ) {
        let (Some(owner), Some(session)) = (owner, session) else {
            return;
        };
        if let Some(record) =
            claude_tool_event(&mut self.claude_pending, owner, session, turn, event)
        {
            self.report_records([record], progress);
        }
    }
}
