//! OS-owned locks serialize Concordia writers without leaving stale ownership after a crash.
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::path::Path;

pub(crate) fn acquire(root: &Path, home: Option<&Path>, session: &str) -> Result<File> {
    let mut key = std::collections::hash_map::DefaultHasher::new();
    let home = home.map(|home| {
        home.canonicalize()
            .map(|path| {
                std::path::PathBuf::from(crate::file_links::normalized_local_path(
                    &path.to_string_lossy(),
                ))
            })
            .unwrap_or_else(|_| home.to_owned())
    });
    home.hash(&mut key);
    session.hash(&mut key);
    let dir = root.join("session-locks");
    std::fs::create_dir_all(&dir)?;
    crate::workspaces::directory_id(&dir).map_err(anyhow::Error::msg)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join(format!("{:016x}.lock", key.finish())))?;
    file.try_lock().context("this provider session is already running in another Concordia pane; wait for that turn to finish before retrying")?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_only_the_same_home_and_session_and_releases_on_drop() {
        let root = tempfile::tempdir().unwrap();
        let home = Path::new("synthetic-home");
        let guard = acquire(root.path(), Some(home), "session-a").unwrap();
        assert!(acquire(root.path(), Some(home), "session-a").is_err());
        assert!(acquire(root.path(), Some(home), "session-b").is_ok());
        assert!(acquire(root.path(), Some(Path::new("other-home")), "session-a").is_ok());
        drop(guard);
        assert!(acquire(root.path(), Some(home), "session-a").is_ok());
    }
}
