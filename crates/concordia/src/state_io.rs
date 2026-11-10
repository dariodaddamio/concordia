//! Small cross-process transactions for append logs and independently saved state.
use std::fs::{OpenOptions, TryLockError};
use std::io;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

/// Keep a stable lock file beside data that may be atomically renamed. A stalled
/// writer produces a visible error after two seconds instead of hanging the UI.
pub(crate) fn with_lock<T>(path: &Path, write: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    with_lock_file(path, |_, _| write())
}

/// The callback can initialize or verify a body-free protocol marker while ownership is held.
pub(crate) fn with_lock_file<T>(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File, bool) -> io::Result<T>,
) -> io::Result<T> {
    crate::test_guard::check_not_real_state(path);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut options = OpenOptions::new();
    options.read(true).write(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Hold the pathname as well as the OS byte-range lock: older writers
        // must not unlink and recreate this inode while a transaction owns it.
        options.share_mode(0);
    }
    let (mut file, created_new) = loop {
        let opened = match options.clone().create_new(true).open(path) {
            Ok(file) => Ok((file, true)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                match options.open(path) {
                    // A legacy writer may remove its pathname between the two opens.
                    Err(error)
                        if error.kind() == io::ErrorKind::NotFound && Instant::now() < deadline =>
                    {
                        continue
                    }
                    result => result.map(|file| (file, false)),
                }
            }
            Err(error) => Err(error),
        };
        match opened {
            Ok(file) => break file,
            #[cfg(windows)]
            Err(error)
                if matches!(error.raw_os_error(), Some(32 | 33)) && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error),
        }
    };
    loop {
        match file.try_lock() {
            Ok(()) => return write(&mut file, created_new),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_mode(path, bytes, true)
}

/// Publish complete bytes only if the destination is still absent. A concurrent
/// creator wins; filesystems without hard-link support fail without replacing it.
pub(crate) fn write_atomic_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_mode(path, bytes, false)
}

fn write_atomic_mode(path: &Path, bytes: &[u8], replace: bool) -> io::Result<()> {
    crate::test_guard::check_not_real_state(path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!("{name}.tmp-{}", uuid::Uuid::new_v4()));
    let mut created = false;
    let r = (|| -> io::Result<()> {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        created = true;
        f.write_all(bytes)?;
        f.flush()?;
        f.sync_all()?;
        drop(f);
        if !replace {
            std::fs::hard_link(&tmp, path)?;
            // Publication succeeded. A retained staging link is harmless and
            // must not make the caller retry a completed native config write.
            let _ = std::fs::remove_file(&tmp);
            return Ok(());
        }
        // Another pane may hold the destination open for a moment (Windows): try a few times.
        let mut last = Ok(());
        for _ in 0..3 {
            match std::fs::rename(&tmp, path) {
                Ok(()) => return Ok(()),
                Err(e) => last = Err(e),
            }
        }
        last
    })();
    if r.is_err() && created {
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
