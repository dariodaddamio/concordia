//! The shared bounded, locked goal store. Goal text is immutable and completion is monotonic.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Goal {
    pub id: String,
    pub text: String,
    pub done: bool,
    pub added_at: String,
    pub closed_at: Option<String>,
}

fn goals_path(root: &Path) -> PathBuf {
    root.join("goals.json")
}

pub fn load_goals(root: &Path) -> Vec<Goal> {
    read_goals(root).unwrap_or_default()
}

const MAX_GOALS_BYTES: usize = 2 * 1024 * 1024;

pub fn read_goals(root: &Path) -> std::io::Result<Vec<Goal>> {
    use std::io::Read;
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
    let goals: Vec<Goal> = serde_json::from_slice(&bytes).map_err(|_| {
        std::io::Error::other("existing goals file is unreadable; it was kept unchanged")
    })?;
    let mut ids = std::collections::BTreeSet::new();
    if goals.iter().any(|goal| {
        goal.id.is_empty()
            || goal.id.len() > 128
            || !ids.insert(goal.id.as_str())
            || goal.added_at.len() > 64
            || goal.closed_at.as_ref().is_some_and(|at| at.len() > 64)
    }) {
        return Err(std::io::Error::other(
            "invalid goal identities or metadata bounds; original kept",
        ));
    }
    Ok(goals)
}

/// Temp file plus rename, so a crash never leaves half a file.
pub fn save_goals(root: &Path, goals: &[Goal]) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    crate::state_io::with_lock(&root.join("goals.lock"), || {
        let mut saved = read_goals(root)?;
        for goal in goals {
            if let Some(existing) = saved.iter_mut().find(|g| g.id == goal.id) {
                // Goal text is immutable and completion is monotonic. A stale pane
                // cannot undo completion recorded by another pane.
                if goal.done && !existing.done {
                    existing.done = true;
                    existing.closed_at = goal.closed_at.clone();
                }
            } else {
                saved.push(goal.clone());
            }
        }
        let json = serde_json::to_vec_pretty(&saved).map_err(std::io::Error::other)?;
        if json.len() > MAX_GOALS_BYTES {
            return Err(std::io::Error::other(
                "goals exceed the 2 MiB save limit; original file kept",
            ));
        }
        crate::state_io::write_atomic(&goals_path(root), &json)
    })
}
