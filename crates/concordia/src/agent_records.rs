use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
const MAX_RECORD_BYTES: u64 = 256 * 1024;

pub fn agents_dir(root: &Path) -> PathBuf {
    root.join("agents")
}
fn valid_agent_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
pub fn record_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_agent_id(id) {
        return Err("invalid delegated agent id".into());
    }
    Ok(agents_dir(root).join(format!("{id}.json")))
}
pub fn control_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    Ok(record_path(root, id)?.with_extension("control.json"))
}

fn metadata_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    Ok(record_path(root, id)?.with_extension("meta.json"))
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AgentMetadata {
    pub reason: Option<String>,
    pub last_event: Option<String>,
    pub heartbeat_unix: Option<u64>,
    pub native_session: Option<String>,
    pub process_id: Option<u32>,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn read_agent_metadata(root: &Path, id: &str) -> Result<AgentMetadata, String> {
    let path = metadata_path(root, id)?;
    if !path.exists() {
        return Ok(AgentMetadata::default());
    }
    let directory = crate::fs_safe::resolved_directory(&agents_dir(root))
        .ok_or("delegated agent metadata directory is linked or unavailable")?;
    if !crate::fs_safe::regular_owned_file(&path, &directory) {
        return Err("delegated agent metadata is not a regular owned file".into());
    }
    let text = crate::fs_safe::read_text_bounded(&path, MAX_RECORD_BYTES as usize)
        .ok_or("delegated agent metadata is missing, oversized, or unreadable")?;
    serde_json::from_str(&text).map_err(|_| "delegated agent metadata is damaged".into())
}

pub fn update_agent_runtime(
    root: &Path,
    id: &str,
    process_id: Option<u32>,
    native_session: Option<&str>,
    event: &str,
) -> Result<(), String> {
    let mut metadata = read_agent_metadata(root, id)?;
    metadata.last_event = Some(event.into());
    metadata.heartbeat_unix = Some(now_unix());
    if let Some(session) = native_session {
        metadata.native_session = Some(session.into());
    }
    if let Some(pid) = process_id {
        metadata.process_id = Some(pid);
    }
    write_agent_metadata(root, id, &metadata)
}

pub fn update_agent_outcome(
    root: &Path,
    id: &str,
    reason: &str,
    native_session: Option<&str>,
    event: &str,
) -> Result<(), String> {
    let mut metadata = read_agent_metadata(root, id)?;
    if !metadata.reason.as_deref().is_some_and(is_terminal_reason) {
        metadata.reason = Some(reason.into());
    }
    metadata.last_event = Some(event.into());
    metadata.heartbeat_unix = Some(now_unix());
    if let Some(session) = native_session {
        metadata.native_session = Some(session.into());
    }
    write_agent_metadata(root, id, &metadata)
}

fn is_terminal_status(status: &str) -> bool {
    matches!(
        status,
        "finished" | "quota" | "crashed" | "cancelled" | "uncertain"
    )
}

fn is_terminal_reason(reason: &str) -> bool {
    matches!(
        reason,
        "success" | "quota" | "crash" | "cancelled" | "unknown"
    )
}

fn write_agent_metadata(root: &Path, id: &str, metadata: &AgentMetadata) -> Result<(), String> {
    let dir = crate::fs_safe::resolved_directory(&agents_dir(root))
        .ok_or("delegated agent directory is linked or unavailable")?;
    let path = metadata_path(root, id)?;
    if path.exists() && !crate::fs_safe::regular_owned_file(&path, &dir) {
        return Err("delegated agent metadata is not a regular owned file".into());
    }
    let bytes =
        crate::redact::redact_bytes(&serde_json::to_vec(metadata).map_err(|e| e.to_string())?);
    crate::state_io::write_atomic(&path, &bytes).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentControl {
    pub agent_id: String,
    pub action: String,
    #[serde(default)]
    pub task: Option<String>,
}

pub fn write_control(root: &Path, control: &AgentControl) -> Result<(), String> {
    let dir = crate::fs_safe::resolved_directory(&agents_dir(root))
        .ok_or("delegated agent directory is linked or unavailable")?;
    let path = control_path(root, &control.agent_id)?;
    if path.exists() && !crate::fs_safe::regular_owned_file(&path, &dir) {
        return Err("delegated agent control is not a regular owned file".into());
    }
    let bytes =
        crate::redact::redact_bytes(&serde_json::to_vec(control).map_err(|e| e.to_string())?);
    crate::state_io::write_atomic(&path, &bytes).map_err(|e| e.to_string())
}

pub fn peek_control(root: &Path, id: &str) -> Option<AgentControl> {
    let path = control_path(root, id).ok()?;
    let dir = crate::fs_safe::resolved_directory(&agents_dir(root))?;
    if !crate::fs_safe::regular_owned_file(&path, &dir) {
        return None;
    }
    let text = crate::fs_safe::read_text_bounded(&path, 8 * 1024)?;
    let control = serde_json::from_str(&text).ok()?;
    Some(control)
}

pub fn ack_control(root: &Path, id: &str) -> Result<(), String> {
    let path = control_path(root, id)?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Small per-agent lock spanning the cancellation decision and owner handoff.
/// Directory creation is atomic on the state volume; stale locks are reclaimed only after
/// a bounded age so a crashed owner cannot deadlock recovery forever.
pub struct ControlLock {
    path: PathBuf,
    token: String,
}

impl ControlLock {
    pub fn acquire(root: &Path, id: &str) -> Result<Self, String> {
        // Validate through the same bounded policy used by the primary record before deriving
        // any lock path. Abandoned locks fail closed; no age or PID heuristic may steal one.
        let _ = record_path(root, id)?;
        let dir = crate::fs_safe::resolved_directory(&agents_dir(root))
            .ok_or("delegated agent directory is linked or unavailable")?;
        let path = dir.join(format!(".{id}.control.lock"));
        let token = uuid::Uuid::new_v4().to_string();
        for _ in 0..40 {
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    let owner = path.join("owner");
                    if std::fs::write(&owner, token.as_bytes()).is_err() {
                        let _ = std::fs::remove_dir_all(&path);
                        return Err("agent control lock owner record unavailable".into());
                    }
                    return Ok(Self { path, token });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(format!("agent control lock unavailable: {error}")),
            }
        }
        Err("agent control lock busy or abandoned; recover the owner lock before retrying".into())
    }
}

impl Drop for ControlLock {
    fn drop(&mut self) {
        let owner = self.path.join("owner");
        let matches = std::fs::read_to_string(&owner).is_ok_and(|token| token == self.token);
        if matches {
            let _ = std::fs::remove_file(owner);
            let _ = std::fs::remove_dir(&self.path);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentRecord {
    pub agent_id: String,
    pub account: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    pub task: String,
    #[serde(default)]
    pub parent: String,
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    #[serde(default)]
    pub parent_run_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    pub status: String,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub activity: Vec<String>,
}
pub fn read_agent(root: &Path, id: &str) -> Result<AgentRecord, String> {
    if !valid_agent_id(id) {
        return Err(format!("unknown delegated agent: {id}"));
    }
    let path = record_path(root, id)?;
    let directory = crate::fs_safe::resolved_directory(&agents_dir(root))
        .ok_or("delegated agent directory is linked or unavailable")?;
    if !crate::fs_safe::regular_owned_file(&path, &directory) {
        return Err(format!("delegated agent record is invalid: {id}"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| format!("unknown delegated agent: {id}"))?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err("delegated agent record exceeds 256 KiB".into());
    }
    let record: AgentRecord = serde_json::from_slice(&bytes)
        .map_err(|_| format!("delegated agent record is invalid: {id}"))?;
    if record.agent_id != id {
        return Err(format!("delegated agent record identity mismatch: {id}"));
    }
    Ok(record)
}
pub fn write_agent(root: &Path, record: &AgentRecord) -> Result<(), String> {
    if !valid_agent_id(&record.agent_id) {
        return Err("invalid delegated agent id".into());
    }
    std::fs::create_dir_all(agents_dir(root)).map_err(|e| e.to_string())?;
    let directory = crate::fs_safe::resolved_directory(&agents_dir(root))
        .ok_or("delegated agent directory is linked or unavailable")?;
    let path = record_path(root, &record.agent_id)?;
    if path.exists() && !crate::fs_safe::regular_owned_file(&path, &directory) {
        return Err("delegated agent record is not a regular owned file".into());
    }
    let bytes = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    let bytes = crate::redact::redact_bytes(&bytes);
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err("delegated agent record exceeds 256 KiB".into());
    }
    crate::state_io::write_atomic(&path, &bytes).map_err(|e| e.to_string())
}

fn modify_agent<T>(
    root: &Path,
    id: &str,
    change: impl FnOnce(&mut AgentRecord) -> Result<T, String>,
) -> Result<T, String> {
    let lock = record_path(root, id)?.with_extension("lock");
    crate::state_io::with_lock(&lock, || {
        let mut record = read_agent(root, id).map_err(std::io::Error::other)?;
        let result = change(&mut record).map_err(std::io::Error::other)?;
        if let Some(text) = &mut record.result {
            if let Some((end, _)) = text.char_indices().nth(20_000) {
                text.truncate(end);
                text.push_str("\n[Result truncated; inspect the worker's native source.]");
            }
        }
        let excess = record.activity.len().saturating_sub(32);
        record.activity.drain(..excess);
        for activity in &mut record.activity {
            if let Some((end, _)) = activity.char_indices().nth(256) {
                activity.truncate(end);
            }
        }
        write_agent(root, &record).map_err(std::io::Error::other)?;
        Ok(result)
    })
    .map_err(|error| error.to_string())
}
pub fn update_agent_exact(
    root: &Path,
    id: &str,
    status: &str,
    result: Option<String>,
    activity: &str,
) -> Result<(), String> {
    modify_agent(root, id, |record| {
        if is_terminal_status(&record.status) {
            return Ok(());
        }
        record.status = status.into();
        record.result = result;
        record.activity.push(activity.into());
        Ok(())
    })
    .and_then(|result| {
        let mut metadata = read_agent_metadata(root, id)?;
        if let Some(reason) = match status {
            "finished" => Some("success"),
            "cancelled" => Some("cancelled"),
            "failed" => Some("unknown"),
            _ => None,
        } {
            metadata.reason = Some(reason.into());
        }
        metadata.last_event = Some(activity.into());
        metadata.heartbeat_unix = Some(now_unix());
        write_agent_metadata(root, id, &metadata)?;
        Ok(result)
    })
}
pub fn update_agent(root: &Path, id: &str, status: &str, result: Option<String>, activity: &str) {
    if let Err(error) = update_agent_exact(root, id, status, result, activity) {
        crate::logging::warn(&format!("agent status not saved: {error}"));
    }
}

pub fn bind_agent_run(
    root: &Path,
    id: &str,
    parent: &str,
    workspace: &Path,
    parent_run: &str,
    worker_run: &str,
) -> Result<(), String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "agent workspace unavailable")?;
    modify_agent(root, id, |record| {
        if record.parent != parent
            || record.workspace.as_ref() != Some(&workspace)
            || record.parent_run_id.as_deref() != Some(parent_run)
            || record
                .run_id
                .as_deref()
                .is_some_and(|run| run != worker_run)
        {
            return Err("delegated agent owner mismatch; start withheld".into());
        }
        record.run_id = Some(worker_run.into());
        record.status = "running".into();
        record
            .activity
            .push("accepted by owning conversation".into());
        Ok(())
    })
    .and_then(|result| {
        let mut metadata = read_agent_metadata(root, id)?;
        metadata.last_event = Some("accepted by owning conversation".into());
        metadata.heartbeat_unix = Some(now_unix());
        write_agent_metadata(root, id, &metadata)?;
        Ok(result)
    })
}

pub fn update_agent_run(
    root: &Path,
    id: &str,
    expected_run: &str,
    status: &str,
    result: Option<String>,
    activity: &str,
) -> Result<(), String> {
    modify_agent(root, id, |record| {
        if record.run_id.as_deref() != Some(expected_run) {
            return Err("delegated agent completion belongs to another run".into());
        }
        if is_terminal_status(&record.status) {
            return Ok(());
        }
        record.status = status.into();
        record.result = result;
        record.activity.push(activity.into());
        Ok(())
    })
    .and_then(|result| {
        let reason = match status {
            "finished" => Some("success"),
            "cancelled" => Some("cancelled"),
            "failed" => Some("unknown"),
            _ => None,
        };
        let mut metadata = read_agent_metadata(root, id)?;
        if let Some(reason) = reason {
            if !metadata.reason.as_deref().is_some_and(is_terminal_reason) {
                metadata.reason = Some(reason.into());
            }
        }
        metadata.last_event = Some(activity.into());
        metadata.heartbeat_unix = Some(now_unix());
        write_agent_metadata(root, id, &metadata)?;
        Ok(result)
    })
}
