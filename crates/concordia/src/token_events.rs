//! Immutable per-attempt accounting and bounded, revision-bound reads.
use super::*;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};

const PAGE_BYTES: usize = 32 * 1024 * 1024;
const PAGE_EVENTS: usize = 10_000;
const EVENT_BYTES: u64 = 16 * 1024;
const MAX_EVENTS: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Main,
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attribution {
    pub version: u8,
    pub origin: Origin,
    pub run: String,
    pub attempt: String,
    pub home_id: Option<String>,
    #[serde(default)]
    pub native_source: Option<String>,
    pub counter_source: String,
    pub outcome: String,
    pub input_known: bool,
    pub cached_known: bool,
    pub output_known: bool,
    pub complete: bool,
}

impl Attribution {
    /// The caller supplies the canonical home captured at launch, never a later login lookup.
    pub fn bound_home(home: Option<&Path>) -> Option<String> {
        home.map(|home| hash(home.to_string_lossy().as_bytes()))
    }
    pub fn new(
        origin: Origin,
        run: &str,
        attempt: &str,
        home: Option<&Path>,
        usage: &TurnUsage,
        outcome: &str,
        complete: bool,
    ) -> Self {
        let home_id = Self::bound_home(home);
        Self {
            version: 1,
            origin,
            run: run.into(),
            attempt: attempt.into(),
            home_id,
            native_source: None,
            counter_source: "native_terminal_snapshot".into(),
            outcome: outcome.into(),
            input_known: usage.input.is_some(),
            cached_known: usage.cached.is_some(),
            output_known: usage.output.is_some(),
            complete,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    event_offset: usize,
    events_revision: String,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub entries: Vec<Entry>,
    pub next: Option<Cursor>,
    pub incomplete: bool,
    pub skipped: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEvent {
    version: u8,
    entry: Entry,
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn directory(root: &Path) -> PathBuf {
    root.join("token-events")
}

fn require_owned(path: &Path, directory: bool) -> std::io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if metadata.file_type().is_symlink()
        || reparse
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(std::io::Error::other(
            "usage storage must use regular owned paths, without links",
        ));
    }
    Ok(())
}

fn event_key(entry: &Entry) -> std::io::Result<String> {
    let a = entry
        .attribution
        .as_ref()
        .ok_or_else(|| std::io::Error::other("missing usage attribution"))?;
    if a.version != 1
        || a.counter_source != "native_terminal_snapshot"
        || a.native_source.as_ref().is_some_and(|source| {
            source.len() != 64 || !source.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        || !uuid::Uuid::parse_str(&a.run).is_ok_and(|uuid| uuid.to_string() == a.run)
        || !uuid::Uuid::parse_str(&a.attempt).is_ok_and(|uuid| uuid.to_string() == a.attempt)
        || a.home_id
            .as_ref()
            .is_some_and(|id| id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
        || !matches!(
            a.outcome.as_str(),
            "succeeded" | "failed" | "cancelled" | "uncertain" | "interrupted"
        )
        || entry
            .cost_usd
            .is_some_and(|cost| !cost.is_finite() || cost < 0.0)
    {
        return Err(std::io::Error::other("invalid usage event attribution"));
    }
    Ok(hash(
        &serde_json::to_vec(&(a.origin, &a.run, &a.attempt)).map_err(std::io::Error::other)?,
    ))
}

fn revision(path: &Path) -> std::io::Result<String> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(format!(
            "{}:{:?}:{:?}",
            metadata.len(),
            metadata.created().ok(),
            metadata.modified().ok()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("missing".into()),
        Err(error) => Err(error),
    }
}

fn check_cancel(cancel: Option<&AtomicBool>) -> std::io::Result<()> {
    if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "usage read cancelled",
        ));
    }
    Ok(())
}

fn catalog(root: &Path, cancel: Option<&AtomicBool>) -> std::io::Result<(Vec<PathBuf>, String)> {
    require_owned(&directory(root), true)?;
    let entries = match fs::read_dir(directory(root)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), hash(&[])))
        }
        Err(error) => return Err(error),
    };
    let mut paths = Vec::new();
    for (index, entry) in entries.enumerate() {
        check_cancel(cancel)?;
        if index > MAX_EVENTS {
            return Err(std::io::Error::other(
                "usage event directory exceeds its record bound",
            ));
        }
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
            && path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| {
                    stem.len() == 64 && stem.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
        {
            require_owned(&path, false)?;
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(std::io::Error::other(
                    "usage event must be a regular owned file",
                ));
            }
            paths.push(path);
        }
    }
    paths.sort();
    let mut revisions = Vec::new();
    for path in &paths {
        check_cancel(cancel)?;
        revisions.push((
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            revision(path)?,
        ));
    }
    Ok((
        paths,
        hash(&serde_json::to_vec(&revisions).map_err(std::io::Error::other)?),
    ))
}

pub(super) fn record_once(root: &Path, entry: &Entry) -> std::io::Result<()> {
    let key = event_key(entry)?;
    let bytes = serde_json::to_vec(&StoredEvent {
        version: 1,
        entry: entry.clone(),
    })
    .map_err(std::io::Error::other)?;
    if bytes.len() as u64 > EVENT_BYTES {
        return Err(std::io::Error::other("usage event exceeds 16 KiB"));
    }
    require_owned(&directory(root), true)?;
    fs::create_dir_all(directory(root))?;
    let path = directory(root).join(format!("{key}.json"));
    require_owned(&path, false)?;
    match File::open(&path) {
        Ok(file) => {
            let mut old = Vec::new();
            file.take(EVENT_BYTES + 1).read_to_end(&mut old)?;
            if old == bytes {
                return Ok(());
            }
            return Err(std::io::Error::other(
                "usage event already exists with different counters; original kept",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    match crate::state_io::write_atomic_new(&path, &bytes) {
        Ok(()) => Ok(()),
        Err(error) => {
            // A concurrent writer may have published the same immutable event between
            // cataloging and publication. Treat that idempotent race as success.
            let mut existing = Vec::new();
            match File::open(&path) {
                Ok(file) => {
                    file.take(EVENT_BYTES + 1).read_to_end(&mut existing)?;
                    if existing == bytes {
                        Ok(())
                    } else {
                        Err(error)
                    }
                }
                Err(_) => Err(error),
            }
        }
    }
}

pub fn load_page(root: &Path, cursor: Option<&Cursor>) -> std::io::Result<Report> {
    load_internal(root, cursor, None)
}

pub fn load_attempt(
    root: &Path,
    origin: Origin,
    run: &str,
    attempt: &str,
) -> std::io::Result<Option<Entry>> {
    if !uuid::Uuid::parse_str(run).is_ok_and(|id| id.to_string() == run)
        || !uuid::Uuid::parse_str(attempt).is_ok_and(|id| id.to_string() == attempt)
    {
        return Err(std::io::Error::other("invalid attempt identity"));
    }
    let key = hash(&serde_json::to_vec(&(origin, run, attempt)).map_err(std::io::Error::other)?);
    require_owned(&directory(root), true)?;
    let path = directory(root).join(format!("{key}.json"));
    require_owned(&path, false)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(EVENT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > EVENT_BYTES {
        return Err(std::io::Error::other("attempt record exceeds its bound"));
    }
    let stored: StoredEvent = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
    if stored.version != 1 || event_key(&stored.entry)? != key {
        return Err(std::io::Error::other("attempt record identity mismatch"));
    }
    Ok(Some(stored.entry))
}

pub fn load_page_with_cancel(
    root: &Path,
    cursor: Option<&Cursor>,
    cancel: &AtomicBool,
) -> std::io::Result<Report> {
    load_internal(root, cursor, Some(cancel))
}

fn load_internal(
    root: &Path,
    cursor: Option<&Cursor>,
    cancel: Option<&AtomicBool>,
) -> std::io::Result<Report> {
    check_cancel(cancel)?;
    let (paths, events_revision) = catalog(root, cancel)?;
    if cursor.is_some_and(|cursor| {
        cursor.events_revision != events_revision
    }) {
        return Err(std::io::Error::other(
            "usage snapshot changed; restart from the first page",
        ));
    }
    let mut position = cursor.cloned().unwrap_or(Cursor {
        event_offset: 0,
        events_revision: events_revision.clone(),
    });
    let mut entries = Vec::new();
    let mut skipped = 0;
    let mut consumed_bytes = 0;
    let mut records = 0;
    if position.event_offset > paths.len() {
        return Err(std::io::Error::other(
            "usage cursor is outside this snapshot",
        ));
    }
    {
        for path in paths.iter().skip(position.event_offset) {
            check_cancel(cancel)?;
            if records >= PAGE_EVENTS || consumed_bytes + EVENT_BYTES as usize + 1 > PAGE_BYTES {
                break;
            }
            let mut bytes = Vec::new();
            File::open(path)?
                .take(EVENT_BYTES + 1)
                .read_to_end(&mut bytes)?;
            consumed_bytes += bytes.len();
            records += 1;
            position.event_offset += 1;
            let parsed: Option<StoredEvent> = (bytes.len() as u64 <= EVENT_BYTES)
                .then(|| serde_json::from_slice(&bytes).ok())
                .flatten();
            match parsed {
                Some(event)
                    if event.version == 1
                        && event_key(&event.entry).ok().as_deref()
                            == path.file_stem().and_then(|name| name.to_str()) =>
                {
                    entries.push(event.entry)
                }
                _ => skipped += 1,
            }
        }
    }
    if catalog(root, cancel)?.1 != events_revision {
        return Err(std::io::Error::other(
            "usage snapshot changed during read; restart from the first page",
        ));
    }
    let has_more = position.event_offset < paths.len();
    Ok(Report {
        entries,
        next: has_more.then_some(position),
        incomplete: has_more || skipped > 0,
        skipped,
    })
}
