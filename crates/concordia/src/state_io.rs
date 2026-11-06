//! Small cross-process transactions for append logs and independently saved state.
use std::fs::{OpenOptions, TryLockError};
use std::io;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

/// Keep a stable lock file beside data that may be atomically renamed. A stalled
/// writer produces a visible error after two seconds instead of hanging the UI.
pub(crate) fn with_lock<T>(path: &Path, write: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    crate::test_guard::check_not_real_state(path);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match file.try_lock() {
            Ok(()) => return write(),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    crate::test_guard::check_not_real_state(path);
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(
        "{name}.tmp-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let write = || -> io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
        f.sync_all()?;
        drop(f);
        // Another pane may hold the destination open for a moment (Windows): try a few times.
        let mut last = Ok(());
        for _ in 0..3 {
            match std::fs::rename(&tmp, path) {
                Ok(()) => return Ok(()),
                Err(e) => last = Err(e),
            }
        }
        last
    };
    let r = write();
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_syncs_before_rename() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        write_atomic(&path, b"complete payload").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"complete payload");
        assert!(!dir.path().read_dir().unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")
        }));
    }
}
