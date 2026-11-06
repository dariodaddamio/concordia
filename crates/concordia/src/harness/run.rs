//! Run one adapter command: no shell, no stdin, capped output, hard timeout.

use super::RunOutcome;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

thread_local! {
    static EXTRA_ENV: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub(crate) fn with_extra_env<T>(names: &[String], f: impl FnOnce() -> T) -> T {
    EXTRA_ENV.with(|slot| {
        let old = slot.replace(names.to_vec());
        let result = f();
        slot.replace(old);
        result
    })
}

/// How long an adapter command may run before it is killed.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_OUTPUT: usize = 64 * 1024;
/// After the child ends, how long to wait for its output pipes (a grandchild may hold them open).
const DRAIN_WAIT: Duration = Duration::from_secs(2);

/// Read a pipe to the end, keeping only the first `MAX_OUTPUT` bytes so a noisy child never blocks.
fn drain(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0u8; 8192];
        while let Ok(n) = pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let room = MAX_OUTPUT.saturating_sub(kept.len());
            kept.extend_from_slice(&chunk[..n.min(room)]);
        }
        let _ = tx.send(String::from_utf8_lossy(&kept).into_owned());
    });
    rx
}

/// Run `argv` (program first) in `cwd`. A program that cannot start is reported in `stderr`.
pub fn run_command(argv: &[String], cwd: &Path, timeout: Duration) -> RunOutcome {
    run_command_cancellable(
        argv,
        cwd,
        timeout,
        &std::sync::atomic::AtomicBool::new(false),
    )
}

/// The caller retains cancellation ownership; cancellation kills this child's tracked tree.
pub fn run_command_cancellable(
    argv: &[String],
    cwd: &Path,
    timeout: Duration,
    cancel: &std::sync::atomic::AtomicBool,
) -> RunOutcome {
    let Some((program, args)) = argv.split_first() else {
        return RunOutcome {
            stderr: "could not start: empty command".into(),
            ..RunOutcome::default()
        };
    };
    // Resolve as a shell would, so npm-style `.cmd` shims start; the stored name is unchanged.
    let mut command =
        crate::process_ui::command(which::which(program).unwrap_or_else(|_| program.into()));
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("CONCORDIA_HARNESS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in [
        "PATH",
        "SystemRoot",
        "SYSTEMROOT",
        "ComSpec",
        "TEMP",
        "TMP",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "LANG",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    EXTRA_ENV.with(|slot| {
        for name in slot.borrow().iter() {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
    });
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            return RunOutcome {
                stderr: format!("could not start {program}: {e}"),
                ..RunOutcome::default()
            }
        }
    };
    #[cfg(windows)]
    let job = WindowsJob::assign(&child);
    let out = child.stdout.take().map(drain);
    let err = child.stderr.take().map(drain);
    let deadline = Instant::now() + timeout;
    let (exit, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code(), false),
            Ok(None)
                if Instant::now() >= deadline
                    || cancel.load(std::sync::atomic::Ordering::SeqCst) =>
            {
                #[cfg(windows)]
                {
                    if let Some(job) = &job {
                        job.terminate();
                    } else {
                        use std::os::windows::process::CommandExt;
                        let _ = crate::process_ui::command("taskkill.exe")
                            .args(["/PID", &child.id().to_string(), "/T", "/F"])
                            .creation_flags(0x0800_0000)
                            .status();
                    }
                }
                #[cfg(unix)]
                {
                    let _ = crate::process_ui::command("kill")
                        .args(["-TERM", &format!("-{}", child.id())])
                        .status();
                }
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break (None, false),
        }
    };
    let collect = |rx: Option<mpsc::Receiver<String>>| {
        rx.and_then(|rx| rx.recv_timeout(DRAIN_WAIT).ok())
            .unwrap_or_default()
    };
    RunOutcome {
        exit,
        stdout: collect(out),
        stderr: collect(err),
        timed_out,
    }
}

#[cfg(windows)]
struct WindowsJob(*mut std::ffi::c_void);

#[cfg(windows)]
impl WindowsJob {
    fn assign(child: &std::process::Child) -> Option<Self> {
        use std::os::windows::io::AsRawHandle;
        #[link(name = "kernel32")]
        extern "system" {
            fn CreateJobObjectW(
                a: *mut std::ffi::c_void,
                name: *const u16,
            ) -> *mut std::ffi::c_void;
            fn AssignProcessToJobObject(
                job: *mut std::ffi::c_void,
                process: *mut std::ffi::c_void,
            ) -> i32;
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        let job = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
        if job.is_null() || unsafe { AssignProcessToJobObject(job, child.as_raw_handle()) } == 0 {
            if !job.is_null() {
                unsafe {
                    CloseHandle(job);
                }
            }
            None
        } else {
            Some(Self(job))
        }
    }

    fn terminate(&self) {
        #[link(name = "kernel32")]
        extern "system" {
            fn TerminateJobObject(job: *mut std::ffi::c_void, code: u32) -> i32;
        }
        unsafe {
            let _ = TerminateJobObject(self.0, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        #[link(name = "kernel32")]
        extern "system" {
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
