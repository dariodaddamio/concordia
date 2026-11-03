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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate_draft: Option<String>,
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
    pub fn record_id(&self) -> String {
        self.directory
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }
    pub fn workers_directory(&self) -> PathBuf {
        self.directory.join("worker-runs")
    }
    pub fn save_files(&self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > 32 * 1024 * 1024 {
            return Err(io::Error::other("file recovery exceeds 32 MiB"));
        }
        crate::state_io::write_atomic(&self.directory.join("files.json"), bytes)
    }
    pub fn files(&self) -> io::Result<Vec<u8>> {
        use std::io::Read;
        let file = match File::open(self.directory.join("files.json")) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut bytes = Vec::new();
        file.take(32 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err(io::Error::other("file recovery exceeds 32 MiB"));
        }
        Ok(bytes)
    }
    pub fn save_turn(
        &self,
        checkpoint: Option<&super::turn_checkpoint::Checkpoint>,
    ) -> io::Result<()> {
        let bytes = serde_json::to_vec(&checkpoint)?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(io::Error::other("unfinished request exceeds 2 MiB"));
        }
        crate::state_io::write_atomic(&self.directory.join("turn.json"), &bytes)
    }

    pub fn turn(&self) -> io::Result<Option<super::turn_checkpoint::Checkpoint>> {
        read_turn(&self.directory.join("turn.json"))
    }

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
        self.save_with_alternate(draft, queue, pending, None)
    }

    pub fn save_with_alternate(
        &self,
        draft: &str,
        queue: &[String],
        pending: Option<&RecoveryEdit>,
        alternate: Option<&str>,
    ) -> io::Result<()> {
        let data = RecoveryData {
            owner_pid: std::process::id(),
            draft: draft.to_owned(),
            alternate_draft: alternate.map(str::to_owned),
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
        let unfinished = read_turn(&e.path().join("turn.json")).ok().flatten();
        let files = fs::metadata(e.path().join("files.json"))
            .ok()
            .filter(|m| m.len() > 2);
        let workers = fs::metadata(e.path().join("worker-runs/workers.json"))
            .ok()
            .is_some_and(|m| m.len() > 2);
        if data.draft.trim().is_empty()
            && data.queue.is_empty()
            && data.pending_edit.is_none()
            && unfinished.is_none()
            && files.is_none()
            && !workers
        {
            continue;
        }
        out.push(RecoveryChoice {
            id,
            draft: crate::attachments::Message::text(&if data.draft.trim().is_empty() {
                unfinished.map(|c| c.prompt).unwrap_or_else(|| {
                    if files.is_some() {
                        "Unsaved file workspace".into()
                    } else {
                        data.draft
                    }
                })
            } else {
                data.draft
            })
            .to_owned(),
            queued: data.queue.len(),
            updated_unix_ms: data.updated_unix_ms.max(
                files
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_millis()),
            ),
        });
    }
    out.sort_by_key(|choice| std::cmp::Reverse(choice.updated_unix_ms));
    Ok(out)
}

fn read_turn(path: &Path) -> io::Result<Option<super::turn_checkpoint::Checkpoint>> {
    use std::io::Read;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(io::Error::other(
            "unfinished-request record exceeds its limit",
        ));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
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
