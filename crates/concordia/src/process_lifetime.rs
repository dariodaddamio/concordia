//! Platform process lifetime ownership for provider children.

#[cfg(windows)]
mod windows {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    };
    use windows_sys::Win32::Foundation::{GetLastError, HANDLE};
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
                return None;
            }
            // Deliberately leak the one process-lifetime handle: closing it is the kill switch.
            Some(handle as usize)
        })
        .map(|handle| handle as HANDLE)
    }

    pub fn bind_to_concordia(child: &tokio::process::Child) {
        let Some(job) = job() else { return };
        let process = child
            .raw_handle()
            .expect("spawned child has a process handle") as HANDLE;
        if unsafe { AssignProcessToJobObject(job, process) } == 0
            && !ASSIGNMENT_FAILURE_LOGGED.swap(true, Ordering::Relaxed)
        {
            crate::logging::error(&format!(
                "could not bind provider child to Concordia process job: {}",
                unsafe { GetLastError() }
            ));
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
pub use windows::bind_to_concordia;

#[cfg(not(windows))]
pub fn bind_to_concordia(_child: &tokio::process::Child) {}

#[cfg(unix)]
pub fn kill_process_group(pid: u32, signal: i32) {
    if pid != 0 {
        unsafe {
            libc::killpg(pid as libc::pid_t, signal);
        }
    }
}
