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
const RECORD_BYTES: usize = 32 * 1024 * 1024;

struct BoundedJson {
    bytes: Vec<u8>,
    limit: usize,
}

impl io::Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other(
                "recovery record exceeds its size limit; previous saved record kept",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn bounded_json(value: &impl Serialize, limit: usize) -> io::Result<Vec<u8>> {
    let mut output = BoundedJson {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut output, value).map_err(io::Error::other)?;
    Ok(output.bytes)
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<crate::attachments::Attachment>>,
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
        let safe = crate::redact::redact_bytes(bytes);
        crate::state_io::write_atomic(&self.directory.join("files.json"), &safe)
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
        let bytes = bounded_json(&checkpoint, 2 * 1024 * 1024)?;
        let safe = crate::redact::redact_bytes(&bytes);
        crate::state_io::write_atomic(&self.directory.join("turn.json"), &safe)
    }

    pub fn turn(&self) -> io::Result<Option<super::turn_checkpoint::Checkpoint>> {
        read_turn(&self.directory.join("turn.json"))
    }

    #[cfg(test)]
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn save(&self, draft: &str, queue: &[String]) -> io::Result<()> {
        self.save_with_alternate(draft, queue, None, None)
    }
    pub fn save_with_alternate(
        &self,
        draft: &str,
        queue: &[String],
        pending: Option<&RecoveryEdit>,
        alternate: Option<&str>,
    ) -> io::Result<()> {
        let path = self.directory.join(RECORD_FILE);
        if fs::metadata(&path).is_ok_and(|metadata| metadata.len() > RECORD_BYTES as u64) {
            return Err(io::Error::other(
                "oversized recovery original kept unchanged; use a fresh recovery record",
            ));
        }
        #[derive(Serialize)]
        struct Snapshot<'a> {
            owner_pid: u32,
            draft: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            alternate_draft: Option<&'a str>,
            queue: &'a [String],
            pending_edit: Option<&'a RecoveryEdit>,
            updated_unix_ms: u128,
        }
        let data = Snapshot {
            owner_pid: std::process::id(),
            draft,
            alternate_draft: alternate,
            queue,
            pending_edit: pending,
            updated_unix_ms: now_ms(),
        };
        let bytes = bounded_json(&data, RECORD_BYTES)?;
        let safe = crate::redact::redact_bytes(&bytes);
        if safe.len() > RECORD_BYTES {
            return Err(io::Error::other(
                "recovery record exceeds 32 MiB; previous saved record kept",
            ));
        }
        crate::state_io::write_atomic(&path, &safe)
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
    let choices = startup_choices(root)?;
    if choices.len() == 1 {
        if let Ok(lease) = claim(root, &choices[0].id) {
            if let Ok(data) = lease.data() {
                return Ok((lease, Some(data), Vec::new()));
            }
        }
        let lease = create(root)?;
        Ok((lease, None, startup_choices(root)?))
    } else {
        let lease = create(root)?;
        Ok((lease, None, choices))
    }
}

fn startup_choices(root: &Path) -> io::Result<Vec<RecoveryChoice>> {
    Ok(available(root)?
        .into_iter()
        .filter(|choice| {
            File::open(root.join(&choice.id).join("dismissed"))
                .ok()
                .and_then(|file| {
                    use std::io::Read;
                    let mut text = String::new();
                    file.take(40).read_to_string(&mut text).ok()?;
                    text.parse::<u128>().ok()
                })
                != Some(choice.updated_unix_ms)
        })
        .collect())
}

/// A dismissal suppresses startup offers for this exact saved version only.
/// Automatic restoration lists it without removing the original recovery data.
pub fn dismiss(root: &Path, choices: &[RecoveryChoice]) -> io::Result<()> {
    for choice in choices {
        let lease = claim(root, &choice.id)?;
        crate::state_io::write_atomic(
            &lease.directory.join("dismissed"),
            choice.updated_unix_ms.to_string().as_bytes(),
        )?;
    }
    Ok(())
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
        if id == "windows" {
            continue;
        }
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
        let data = match read_data(&e.path().join(RECORD_FILE)) {
            Ok(data) => data,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                out.push(RecoveryChoice {
                    id,
                    draft: format!("Saved recovery unavailable: {error}. Original kept unchanged."),
                    queued: 0,
                    updated_unix_ms: 0,
                });
                continue;
            }
        };
        let turn = read_turn(&e.path().join("turn.json"));
        let turn_unavailable = turn.is_err();
        let unfinished = turn.ok().flatten();
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
            && !turn_unavailable
            && files.is_none()
            && !workers
        {
            continue;
        }
        out.push(RecoveryChoice {
            id,
            draft: crate::attachments::Message::text(&if data.draft.trim().is_empty() {
                unfinished.map(|c| c.prompt).unwrap_or_else(|| {
                    if turn_unavailable {
                        "Unfinished-request recovery unavailable; original kept unchanged".into()
                    } else if files.is_some() {
                        "Unsaved file workspace".into()
                    } else {
                        data.draft
                    }
                })
            } else {
                data.draft
            })
            .chars()
            .take(256)
            .collect(),
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
    serde_json::from_slice(&read_record_bytes(path)?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn read_record_bytes(path: &Path) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let file = File::open(path)?;
    if file.metadata()?.len() > RECORD_BYTES as u64 {
        return Err(io::Error::other(
            "recovery record exceeds 32 MiB; original kept unchanged",
        ));
    }
    let mut bytes = Vec::new();
    file.take(RECORD_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > RECORD_BYTES {
        return Err(io::Error::other(
            "recovery record exceeds 32 MiB; original kept unchanged",
        ));
    }
    Ok(bytes)
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
