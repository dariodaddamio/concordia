//! The shared bounded, locked goal store. Goal text is immutable and completion is monotonic.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[path = "goal_changes.rs"]
pub mod changes;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeSource {
    UserInput,
    McpProposalApprovedLocally,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attribution {
    pub source: ChangeSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}
impl Attribution {
    pub fn user() -> Self {
        Self {
            source: ChangeSource::UserInput,
            request_id: None,
        }
    }
    fn valid(&self) -> bool {
        match self.source {
            ChangeSource::UserInput => self.request_id.is_none(),
            ChangeSource::McpProposalApprovedLocally => {
                self.request_id.as_ref().is_some_and(|id| {
                    uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == *id)
                })
            }
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Goal {
    pub id: String,
    pub text: String,
    pub done: bool,
    pub added_at: String,
    pub closed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Attribution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_origin: Option<Attribution>,
    #[serde(default, flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

fn goals_path(root: &Path) -> PathBuf {
    root.join("goals.json")
}

pub fn load_goals(root: &Path) -> Vec<Goal> {
    read_goals(root).unwrap_or_default()
}

const MAX_GOALS_BYTES: usize = 2 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    goals: Vec<Goal>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Saved {
    Current(Document),
    Legacy(Vec<Goal>),
}

pub fn read_goals(root: &Path) -> std::io::Result<Vec<Goal>> {
    use std::io::Read;
    match std::fs::symlink_metadata(goals_path(root)) {
        Ok(metadata) if !metadata.is_file() || crate::workspaces::linked(&metadata) => {
            return Err(std::io::Error::other(
                "goals must remain a regular owned file; original retained",
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    let file = match std::fs::File::open(goals_path(root)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_GOALS_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_GOALS_BYTES {
        return Err(std::io::Error::other(
            "goals file exceeds the 2 MiB read limit; it was kept unchanged",
        ));
    }
    let saved: Saved = serde_json::from_slice(&bytes).map_err(|_| {
        std::io::Error::other("existing goals file is unreadable; it was kept unchanged")
    })?;
    let goals = match saved {
        Saved::Legacy(goals) => goals,
        Saved::Current(document) if document.version == 2 => document.goals,
        _ => {
            return Err(std::io::Error::other(
                "unsupported goal store version; original retained",
            ))
        }
    };
    validate(&goals)?;
    Ok(goals)
}

pub fn revision(goals: &[Goal]) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(goals).map_err(|error| error.to_string())?)
    ))
}

fn validate(goals: &[Goal]) -> std::io::Result<()> {
    let mut ids = std::collections::BTreeSet::new();
    if goals.iter().any(|goal| {
        goal.id.is_empty()
            || goal.id.len() > 128
            || !ids.insert(goal.id.as_str())
            || goal.added_at.len() > 64
            || goal.closed_at.as_ref().is_some_and(|at| at.len() > 64)
            || goal.origin.as_ref().is_some_and(|origin| !origin.valid())
            || goal.extra.keys().any(|key| {
                matches!(
                    key.as_str(),
                    "id" | "text" | "done" | "added_at" | "closed_at" | "origin" | "closed_origin"
                )
            })
            || goal
                .closed_origin
                .as_ref()
                .is_some_and(|origin| !origin.valid())
    }) {
        return Err(std::io::Error::other(
            "invalid goal identities or metadata bounds; original kept",
        ));
    }
    Ok(())
}

fn encode_goals(goals: &[Goal]) -> std::io::Result<Vec<u8>> {
    validate(goals)?;
    let json = serde_json::to_vec_pretty(&Document {
        version: 2,
        goals: goals.to_vec(),
    })
    .map_err(std::io::Error::other)?;
    if json.len() > MAX_GOALS_BYTES {
        return Err(std::io::Error::other(
            "goals exceed the 2 MiB save limit; original file kept",
        ));
    }
    Ok(json)
}

fn write_goals(root: &Path, goals: &[Goal]) -> std::io::Result<()> {
    crate::state_io::write_atomic(&goals_path(root), &encode_goals(goals)?)
}

fn with_goal_lock<T>(
    root: &Path,
    operation: impl FnOnce() -> std::io::Result<T>,
) -> std::io::Result<T> {
    std::fs::create_dir_all(root)?;
    let lock = root.join("goals.lock");
    match std::fs::symlink_metadata(&lock) {
        Ok(metadata) if !metadata.is_file() || crate::workspaces::linked(&metadata) => {
            return Err(std::io::Error::other(
                "goal lock must remain a regular owned file",
            ))
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    crate::state_io::with_lock(&lock, operation)
}

/// Temp file plus rename, so a crash never leaves half a file.
pub fn save_goals(root: &Path, goals: &[Goal]) -> std::io::Result<()> {
    with_goal_lock(root, || {
        let mut saved = read_goals(root)?;
        for goal in goals {
            if let Some(existing) = saved.iter_mut().find(|g| g.id == goal.id) {
                // Goal text is immutable and completion is monotonic. A stale pane
                // cannot undo completion recorded by another pane.
                if goal.done && !existing.done {
                    existing.done = true;
                    existing.closed_at = goal.closed_at.clone();
                    existing.closed_origin = goal.closed_origin.clone();
                }
            } else {
                saved.push(goal.clone());
            }
        }
        write_goals(root, &saved)
    })
}
