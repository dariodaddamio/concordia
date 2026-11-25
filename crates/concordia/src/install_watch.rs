use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

#[cfg(windows)]
use std::time::Duration;

pub struct InstallWatcher {
    updates: Receiver<PathBuf>,
}

impl InstallWatcher {
    pub fn try_take(&self) -> Option<PathBuf> {
        self.updates.try_recv().ok()
    }
}

#[cfg(windows)]
pub fn start(path: PathBuf, wake: impl Fn() + Send + 'static) -> Option<InstallWatcher> {
    let install = std::fs::canonicalize(&path).ok()?;
    let running = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::canonicalize(p).ok())?;
    if install != running {
        return None;
    }
    let directory = install.parent()?.to_path_buf();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("concordia-install-watch".into())
        .spawn(move || {
            if let Some(initial) = signature(&install) {
                watch(directory, install, initial, tx, wake);
            }
        })
        .ok()?;
    Some(InstallWatcher { updates: rx })
}

#[cfg(not(windows))]
pub fn start(_path: PathBuf, _wake: impl Fn() + Send + 'static) -> Option<InstallWatcher> {
    None
}

#[cfg(windows)]
fn signature(path: &std::path::Path) -> Option<(u64, std::time::SystemTime, [u8; 32])> {
    use sha2::{Digest, Sha256};
    let metadata = std::fs::metadata(path).ok()?;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash).ok()?;
    Some((
        metadata.len(),
        metadata.modified().ok()?,
        hash.finalize().into(),
    ))
}

#[cfg(windows)]
fn watch(
    directory: PathBuf,
    install: PathBuf,
    mut previous: (u64, std::time::SystemTime, [u8; 32]),
    updates: mpsc::Sender<PathBuf>,
    wake: impl Fn() + Send + 'static,
) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    let mut directory_wide: Vec<u16> = directory.as_os_str().encode_wide().collect();
    directory_wide.push(0);
    let handle = unsafe {
        FindFirstChangeNotificationW(
            directory_wide.as_ptr(),
            0,
            FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_SIZE | FILE_NOTIFY_CHANGE_LAST_WRITE,
        )
    };
    if handle as isize == INVALID_HANDLE_VALUE {
        return;
    }

    loop {
        if unsafe { WaitForSingleObject(handle, INFINITE) } != WAIT_OBJECT_0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
        let Some(meta) = stable_meta(&install) else {
            let _ = unsafe { FindNextChangeNotification(handle) };
            continue;
        };
        if meta == (previous.0, previous.1) {
            if unsafe { FindNextChangeNotification(handle) } == 0 {
                break;
            }
            continue;
        }
        let Some(candidate) = signature(&install) else {
            let _ = unsafe { FindNextChangeNotification(handle) };
            continue;
        };
        if candidate.2 == previous.2 {
            previous = candidate;
        } else {
            previous = candidate;
            if updates.send(install.clone()).is_err() {
                break;
            }
            wake();
        }
        if unsafe { FindNextChangeNotification(handle) } == 0 {
            break;
        }
    }
    let _ = unsafe { FindCloseChangeNotification(handle) };
}

#[cfg(windows)]
fn stable_meta(path: &std::path::Path) -> Option<(u64, std::time::SystemTime)> {
    let read = || {
        let metadata = std::fs::metadata(path).ok()?;
        Some((metadata.len(), metadata.modified().ok()?))
    };
    let first = read()?;
    std::thread::sleep(Duration::from_millis(25));
    let second = read()?;
    (first == second).then_some(second)
}

#[cfg(windows)]
const FILE_NOTIFY_CHANGE_FILE_NAME: u32 = 1;
#[cfg(windows)]
const FILE_NOTIFY_CHANGE_SIZE: u32 = 8;
#[cfg(windows)]
const FILE_NOTIFY_CHANGE_LAST_WRITE: u32 = 16;
#[cfg(windows)]
const WAIT_OBJECT_0: u32 = 0;
#[cfg(windows)]
const INFINITE: u32 = u32::MAX;
#[cfg(windows)]
const INVALID_HANDLE_VALUE: isize = -1;

#[cfg(windows)]
extern "system" {
    fn FindFirstChangeNotificationW(
        path: *const u16,
        watch_subtree: i32,
        notify_filter: u32,
    ) -> windows_sys::Win32::Foundation::HANDLE;
    fn FindNextChangeNotification(handle: windows_sys::Win32::Foundation::HANDLE) -> i32;
    fn FindCloseChangeNotification(handle: windows_sys::Win32::Foundation::HANDLE) -> i32;
}
