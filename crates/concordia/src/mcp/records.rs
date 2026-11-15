use std::path::{Path, PathBuf};

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
pub fn record_path(root: &Path, id: &str) -> PathBuf {
    if !valid_agent_id(id) {
        return agents_dir(root).join("__invalid__.json");
    }
    agents_dir(root).join(format!("{id}.json"))
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
    let bytes = std::fs::read(record_path(root, id))
        .map_err(|_| format!("unknown delegated agent: {id}"))?;
    serde_json::from_slice(&bytes).map_err(|_| format!("delegated agent record is invalid: {id}"))
}
pub fn write_agent(root: &Path, record: &AgentRecord) -> Result<(), String> {
    if !valid_agent_id(&record.agent_id) {
        return Err("invalid delegated agent id".into());
    }
    std::fs::create_dir_all(agents_dir(root)).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    crate::state_io::write_atomic(
        &record_path(root, &record.agent_id),
        &crate::redact::redact_bytes(&bytes),
    )
    .map_err(|e| e.to_string())
}
pub fn update_agent(root: &Path, id: &str, status: &str, result: Option<String>, activity: &str) {
    if let Ok(mut record) = read_agent(root, id) {
        record.status = status.into();
        record.result = result;
        record.activity.push(activity.into());
        let _ = write_agent(root, &record);
    }
}
pub fn update_matching(
    root: &Path,
    account: &str,
    task: &str,
    status: &str,
    result: Option<String>,
) {
    let Ok(entries) = std::fs::read_dir(agents_dir(root)) else {
        return;
    };
    let matches = entries
        .flatten()
        .filter_map(|entry| {
            let bytes = std::fs::read(entry.path()).ok()?;
            let record = serde_json::from_slice::<AgentRecord>(&bytes).ok()?;
            (record.account == account && record.task == task && record.status == "running")
                .then_some(record.agent_id)
        })
        .collect::<Vec<_>>();
    // Completion is keyed by the exact run identity. A legacy text-only match is safe only
    // when it identifies one record; never let one result complete multiple runs.
    if matches.len() == 1 {
        update_agent(root, &matches[0], status, result, "delegated run finished");
    }
}
