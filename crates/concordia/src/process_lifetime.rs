//! Platform process lifetime ownership for provider children.

#[cfg(windows)]
mod windows {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    };
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_BASIC_LIMIT_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    static JOB: OnceLock<Option<usize>> = OnceLock::new();
    static ASSIGNMENT_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);

    fn job() -> Option<HANDLE> {
        JOB.get_or_init(|| unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                crate::logging::error(&format!(
                    "could not create Concordia process job: {}",
                    GetLastError()
                ));
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation = JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                ..std::mem::zeroed()
            };
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&mut info as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<std::ffi::c_void>(),
                std::mem::size_of_val(&info) as u32,
            ) == 0
            {
                crate::logging::error(&format!(
                    "could not configure Concordia process job: {}",
                    GetLastError()
                ));
                CloseHandle(handle);
                return None;
            }
            // Deliberately leak the one process-lifetime handle: closing it is the kill switch.
            Some(handle as usize)
        })
        .map(|handle| handle as HANDLE)
    }

    pub fn bind_to_concordia(child: &tokio::process::Child) {
        let Some(raw) = child.raw_handle() else {
            return;
        };
        bind_raw(raw as HANDLE);
    }

    pub fn bind_std_to_concordia(child: &std::process::Child) {
        use std::os::windows::io::AsRawHandle;
        bind_raw(child.as_raw_handle() as HANDLE);
    }

    pub(super) fn bind_raw(process: HANDLE) {
        let Some(job) = job() else { return };
        if unsafe { AssignProcessToJobObject(job, process) } == 0
            && !ASSIGNMENT_FAILURE_LOGGED.swap(true, Ordering::Relaxed)
        {
            crate::logging::error(&format!(
                "could not bind provider child to Concordia process job: {}",
                unsafe { GetLastError() }
            ));
        }
    }

    /// Nested beneath the app job; closing a probe owns its descendants even after
    /// its direct child exits and leaves inherited output pipes open.
    pub(super) struct CaptureJob(usize);

    impl CaptureJob {
        pub(super) fn new() -> Result<Self, String> {
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return Err(format!("could not create probe job: {}", GetLastError()));
                }
                let job = Self(handle as usize);
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    (&mut info as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&info) as u32,
                ) == 0
                {
                    return Err(format!("could not configure probe job: {}", GetLastError()));
                }
                Ok(job)
            }
        }

        pub(super) fn bind(&self, child: &std::process::Child) -> Result<(), String> {
            use std::os::windows::io::AsRawHandle;
            self.bind_raw(child.as_raw_handle() as HANDLE)
        }

        pub(super) fn bind_raw(&self, process: HANDLE) -> Result<(), String> {
            if unsafe { AssignProcessToJobObject(self.0 as HANDLE, process) } == 0 {
                return Err(format!("could not bind probe process job: {}", unsafe {
                    GetLastError()
                }));
            }
            Ok(())
        }
    }

    impl Drop for CaptureJob {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0 as HANDLE) };
        }
    }

    #[cfg(test)]
    pub fn job_handle_for_test() -> Option<HANDLE> {
        job()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::System::JobObjects::{
            IsProcessInJob, JobObjectExtendedLimitInformation, QueryInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        };

        #[test]
        fn job_object_is_created_once_with_kill_on_close() {
            let first = job_handle_for_test().expect("job object");
            let second = job_handle_for_test().expect("job object");
            assert_eq!(first, second);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            let ok = unsafe {
                QueryInformationJobObject(
                    first,
                    JobObjectExtendedLimitInformation,
                    (&mut info as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION)
                        .cast::<std::ffi::c_void>(),
                    std::mem::size_of_val(&info) as u32,
                    std::ptr::null_mut(),
                )
            };
            assert_ne!(ok, 0);
            assert_ne!(
                info.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                0
            );
        }

        #[tokio::test]
        async fn provider_child_is_assigned_to_the_concordia_job() {
            let job = job_handle_for_test().expect("job object");
            let mut child = crate::process_ui::tokio_command("cmd")
                .args(["/C", "ping", "127.0.0.1", "-n", "3"])
                .spawn()
                .expect("spawn test child");
            bind_to_concordia(&child);
            let mut in_job = 0;
            let ok =
                unsafe { IsProcessInJob(child.raw_handle().unwrap() as HANDLE, job, &mut in_job) };
            assert_ne!(ok, 0);
            assert_ne!(in_job, 0);
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}

#[cfg(windows)]
pub use windows::{bind_std_to_concordia, bind_to_concordia};

#[cfg(not(windows))]
pub fn bind_to_concordia(_child: &tokio::process::Child) {}

#[cfg(not(windows))]
pub fn bind_std_to_concordia(_child: &std::process::Child) {}

/// Scoped tree ownership for an auxiliary/build child. The launcher must use a
/// fresh process group on Unix; Windows uses a nested kill-on-close job.
pub(crate) struct ProcessTree {
    #[cfg(windows)]
    _job: windows::CaptureJob,
    #[cfg(unix)]
    pid: u32,
}

pub(crate) fn own_std_process(child: &std::process::Child) -> Result<ProcessTree, String> {
    bind_std_to_concordia(child);
    #[cfg(windows)]
    {
        let job = windows::CaptureJob::new()?;
        job.bind(child)?;
        Ok(ProcessTree { _job: job })
    }
    #[cfg(unix)]
    {
        Ok(ProcessTree { pid: child.id() })
    }
}

pub(crate) fn own_tokio_process(child: &tokio::process::Child) -> Result<ProcessTree, String> {
    bind_to_concordia(child);
    #[cfg(windows)]
    {
        let process = child.raw_handle().ok_or("process handle unavailable")?;
        let job = windows::CaptureJob::new()?;
        job.bind_raw(process.cast())?;
        Ok(ProcessTree { _job: job })
    }
    #[cfg(unix)]
    {
        Ok(ProcessTree {
            pid: child.id().ok_or("process ID unavailable")?,
        })
    }
}

pub(crate) fn own_pty_process(
    child: &(dyn portable_pty::Child + Send + Sync),
) -> Result<ProcessTree, String> {
    #[cfg(windows)]
    {
        let process = child
            .as_raw_handle()
            .ok_or("PTY process handle unavailable")?;
        windows::bind_raw(process.cast());
        let job = windows::CaptureJob::new()?;
        job.bind_raw(process.cast())?;
        Ok(ProcessTree { _job: job })
    }
    #[cfg(unix)]
    {
        Ok(ProcessTree {
            pid: child.process_id().ok_or("PTY process ID unavailable")?,
        })
    }
}

#[cfg(unix)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        kill_process_group(self.pid, libc::SIGKILL);
    }
}

/// Capture a small auxiliary command with one deadline for process exit and both
/// pipes. Output overflow is an error, never a successfully truncated response.
pub(crate) fn capture(
    command: std::process::Command,
    timeout: std::time::Duration,
    output_limit: usize,
) -> Result<std::process::Output, String> {
    capture_with_input(command, timeout, output_limit, None)
}

/// Own stdin delivery and both output pipes under the same process-tree deadline.
pub(crate) fn capture_with_input(
    command: std::process::Command,
    timeout: std::time::Duration,
    output_limit: usize,
    input: Option<Vec<u8>>,
) -> Result<std::process::Output, String> {
    capture_controlled(command, timeout, output_limit, input, None)
}

pub(crate) fn capture_cancellable(
    command: std::process::Command,
    timeout: std::time::Duration,
    output_limit: usize,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<std::process::Output, String> {
    capture_controlled(command, timeout, output_limit, None, Some(cancel))
}

fn capture_controlled(
    mut command: std::process::Command,
    timeout: std::time::Duration,
    output_limit: usize,
    input: Option<Vec<u8>>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<std::process::Output, String> {
    use std::io::{Read, Write};
    use std::process::{Child, Stdio};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    };
    use std::time::{Duration, Instant};

    struct OwnedChild {
        child: Child,
        tree: Option<ProcessTree>,
    }
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            drop(self.tree.take());
            let _ = self.child.kill();
            // Never replace the caller's deadline with an unbounded wait.
            let until = Instant::now() + Duration::from_millis(200);
            while self.child.try_wait().is_ok_and(|status| status.is_none())
                && Instant::now() < until
            {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    let started = Instant::now();
    if cancel.is_some_and(|cancel| cancel.load(Ordering::SeqCst)) {
        return Err("command cancelled before startup".into());
    }
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut owned = OwnedChild {
        child: command.spawn().map_err(|e| e.to_string())?,
        tree: None,
    };
    owned.tree = Some(own_std_process(&owned.child)?);
    let total = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel();
    let stdout = owned
        .child
        .stdout
        .take()
        .ok_or("probe stdout unavailable")?;
    let stderr = owned
        .child
        .stderr
        .take()
        .ok_or("probe stderr unavailable")?;
    let mut readers = Vec::new();
    let mut input_done = input.is_none();
    if let Some(input) = input {
        let mut stdin = owned.child.stdin.take().ok_or("probe stdin unavailable")?;
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            let result = stdin
                .write_all(&input)
                .and_then(|_| stdin.flush())
                .map(|_| Vec::new())
                .map_err(|error| error.to_string());
            drop(stdin);
            let _ = tx.send((2, result));
        }));
    }
    for (index, mut pipe) in [
        (0, Box::new(stdout) as Box<dyn Read + Send>),
        (1, Box::new(stderr) as Box<dyn Read + Send>),
    ] {
        let (tx, total) = (tx.clone(), total.clone());
        readers.push(std::thread::spawn(move || {
            let result = (|| {
                let mut bytes = Vec::new();
                let mut chunk = [0; 4096];
                loop {
                    let count = pipe.read(&mut chunk).map_err(|e| e.to_string())?;
                    if count == 0 {
                        return Ok(bytes);
                    }
                    if total
                        .fetch_add(count, Ordering::Relaxed)
                        .saturating_add(count)
                        > output_limit
                    {
                        return Err(format!("probe output exceeds {output_limit} bytes"));
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                }
            })();
            let _ = tx.send((index, result));
        }));
    }
    drop(tx);
    let mut output: [Option<Vec<u8>>; 2] = [None, None];
    let mut status = None;
    loop {
        if cancel.is_some_and(|cancel| cancel.load(Ordering::SeqCst)) {
            return Err("command cancelled".into());
        }
        while let Ok((index, result)) = rx.try_recv() {
            if index == 2 {
                result?;
                input_done = true;
            } else {
                output[index] = Some(result?);
            }
        }
        if status.is_none() {
            status = owned.child.try_wait().map_err(|e| e.to_string())?;
        }
        if let (true, Some(status), [Some(stdout), Some(stderr)]) =
            (input_done, status, &mut output)
        {
            for reader in readers {
                let _ = reader.join();
            }
            return Ok(std::process::Output {
                status,
                stdout: std::mem::take(stdout),
                stderr: std::mem::take(stderr),
            });
        }
        if started.elapsed() >= timeout {
            return Err(format!("timed out after {}s", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(unix)]
pub fn kill_process_group(pid: u32, signal: i32) {
    if pid != 0 {
        unsafe {
            libc::killpg(pid as libc::pid_t, signal);
        }
    }
}
