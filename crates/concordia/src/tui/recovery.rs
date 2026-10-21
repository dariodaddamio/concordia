//! Pane-owned draft and queue recovery. Sent history remains in the shared state root.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    collections::hash_map::DefaultHasher,
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const ROOT_DIR: &str = "pane-recovery";
const RECORD_FILE: &str = "recovery.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryData {
    pub owner_pid: u32,
    pub draft: String,
    pub queue: Vec<String>,
    #[serde(default)]
    pub pending_edit: Option<RecoveryEdit>,
    pub updated_unix_ms: u128,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryEdit {
    pub index: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryChoice {
    pub id: String,
    pub draft: String,
    pub queued: usize,
    pub updated_unix_ms: u128,
}

/// A held OS file lock proves this pane owns the record. Keep it alive for the pane lifetime.
pub struct RecoveryLease {
    directory: PathBuf,
    #[cfg(test)]
    id: String,
    _lock: File,
}

impl RecoveryLease {
    #[cfg(test)]
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn save(&self, draft: &str, queue: &[String]) -> io::Result<()> {
        self.save_with_edit(draft, queue, None)
    }
    pub fn save_with_edit(
        &self,
        draft: &str,
        queue: &[String],
        pending: Option<&RecoveryEdit>,
    ) -> io::Result<()> {
        let data = RecoveryData {
            owner_pid: std::process::id(),
            draft: draft.to_owned(),
            queue: queue.to_vec(),
            pending_edit: pending.cloned(),
            updated_unix_ms: now_ms(),
        };
        crate::tui::persist::write_atomic(
            &self.directory.join(RECORD_FILE),
            &serde_json::to_vec(&data)?,
        )
    }
    pub fn data(&self) -> io::Result<RecoveryData> {
        read_data(&self.directory.join(RECORD_FILE))
    }
}

/// Stable recovery root per canonical workspace, separate from global sent history.
pub fn recovery_root(state_root: &Path, workspace: &Path) -> io::Result<PathBuf> {
    let canonical = fs::canonicalize(workspace)?;
    let mut h = DefaultHasher::new();
    canonical.hash(&mut h);
    let root = state_root
        .join(ROOT_DIR)
        .join(format!("{:016x}", h.finish()));
    fs::create_dir_all(&root)?;
    Ok(root)
}

/// Create a fresh record, auto-claiming one abandoned record and returning several as choices.
pub fn startup_with_restore(
    root: &Path,
) -> io::Result<(RecoveryLease, Option<RecoveryData>, Vec<RecoveryChoice>)> {
    fs::create_dir_all(root)?;
    let choices = available(root)?;
    if choices.len() == 1 {
        if let Ok(lease) = claim(root, &choices[0].id) {
            if let Ok(data) = lease.data() {
                return Ok((lease, Some(data), Vec::new()));
            }
        }
        let lease = create(root)?;
        Ok((lease, None, available(root)?))
    } else {
        let lease = create(root)?;
        Ok((lease, None, choices))
    }
}

pub fn create(root: &Path) -> io::Result<RecoveryLease> {
    fs::create_dir_all(root)?;
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    loop {
        let id = format!(
            "{}-{}-{}",
            std::process::id(),
            now_ms(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        );
        let dir = root.join(&id);
        match fs::create_dir(&dir) {
            Ok(()) => return lock_dir(dir, id),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => std::thread::yield_now(),
            Err(e) => return Err(e),
        }
    }
}

pub fn claim(root: &Path, id: &str) -> io::Result<RecoveryLease> {
    let component = Path::new(id);
    if id.is_empty()
        || component.components().count() != 1
        || !matches!(
            component.components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid recovery record id",
        ));
    }
    let dir = root.join(id);
    if !dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "recovery record missing",
        ));
    }
    lock_dir(dir, id.to_owned())
}

fn lock_dir(directory: PathBuf, _id: String) -> io::Result<RecoveryLease> {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("owner.lock"))?;
    lock.try_lock()?;
    Ok(RecoveryLease {
        directory,
        #[cfg(test)]
        id: _id,
        _lock: lock,
    })
}

pub fn available(root: &Path) -> io::Result<Vec<RecoveryChoice>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    for e in fs::read_dir(root)? {
        let e = e?;
        if !e.file_type()?.is_dir() {
            continue;
        }
        let id = e.file_name().to_string_lossy().into_owned();
        let lock_path = e.path().join("owner.lock");
        let Ok(lock) = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
        else {
            continue;
        };
        if lock.try_lock().is_err() {
            continue;
        }
        let Ok(data) = read_data(&e.path().join(RECORD_FILE)) else {
            continue;
        };
        if data.draft.trim().is_empty() && data.queue.is_empty() && data.pending_edit.is_none() {
            continue;
        }
        out.push(RecoveryChoice {
            id,
            draft: data.draft,
            queued: data.queue.len(),
            updated_unix_ms: data.updated_unix_ms,
        });
    }
    out.sort_by_key(|choice| std::cmp::Reverse(choice.updated_unix_ms));
    Ok(out)
}

fn read_data(path: &Path) -> io::Result<RecoveryData> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;

/// Legacy shared draft and queue are left intact for explicit user import; they cannot safely be
/// assigned to any pane. Return their presence so startup can offer that import with a notice.
pub fn legacy_available(state_root: &Path) -> bool {
    crate::tui::persist::draft_path(state_root).exists()
        || crate::tui::persist::queue_path(state_root).exists()
}

/// Explicitly import legacy shared data without deleting its originals.
pub fn import_legacy(state_root: &Path, lease: &RecoveryLease) -> io::Result<()> {
    let current = match lease.data() {
        Ok(data) => Some(data),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if current.is_some_and(|current| {
        !current.draft.trim().is_empty()
            || !current.queue.is_empty()
            || current.pending_edit.is_some()
    }) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "current recovery record is not empty",
        ));
    }
    let draft = crate::tui::persist::load_draft(state_root).unwrap_or_default();
    let queue = crate::tui::persist::load_queue(state_root);
    lease.save(&draft, &queue)
}
