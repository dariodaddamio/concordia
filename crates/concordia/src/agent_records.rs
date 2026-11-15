use std::io::Read;
use std::path::{Path, PathBuf};
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
            if let Some((end, _)) = activity.char_indices().nth(256) { activity.truncate(end); }
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
        record.status = status.into();
        record.result = result;
        record.activity.push(activity.into());
        Ok(())
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
        record.status = status.into();
        record.result = result;
        record.activity.push(activity.into());
        Ok(())
    })
}
