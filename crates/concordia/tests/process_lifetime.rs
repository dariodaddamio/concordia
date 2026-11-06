//! Process-lifetime ownership integration coverage.

#[cfg(windows)]
#[test]
fn provider_child_dies_when_concordia_is_killed() {
    use std::io::BufRead;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    const FAKE: &str = env!("CARGO_BIN_EXE_concordia-fake-provider");
    const PROBE: &str = env!("CARGO_BIN_EXE_concordia-lifetime-probe");

    let mut probe = concordia::process_ui::command(PROBE)
        .arg(FAKE)
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn lifetime probe");
    let stdout = probe.stdout.take().expect("probe stdout");
    let mut lines = std::io::BufReader::new(stdout).lines();
    let child_pid = lines
        .next()
        .expect("probe should print child PID")
        .expect("read child PID")
        .trim()
        .parse::<u32>()
        .expect("child PID should be numeric");
    let startup_deadline = Instant::now() + Duration::from_secs(5);
    while !process_alive(child_pid) && Instant::now() < startup_deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        process_alive(child_pid),
        "provider child {child_pid} did not become alive"
    );

    terminate_process(probe.id()).expect("terminate probe");
    let _ = probe.wait();

    let deadline = Instant::now() + Duration::from_secs(5);
    while process_alive(child_pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !process_alive(child_pid),
        "provider child {child_pid} survived probe crash"
    );
}

#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    let output = concordia::process_ui::command("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .expect("tasklist");
    if String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\"")) {
        return true;
    }

    // Some managed Windows runners deny tasklist enumeration. Querying the specific process
    // keeps the same PID-based check usable there without broad process inspection.
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    const STILL_ACTIVE: u32 = 259;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut exit_code = 0;
        let result = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        result != 0 && exit_code == STILL_ACTIVE
    }
}

#[cfg(windows)]
fn terminate_process(pid: u32) -> Result<(), String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return Err(format!("OpenProcess failed for {pid}"));
        }
        let result = TerminateProcess(handle, 1);
        CloseHandle(handle);
        if result == 0 {
            Err(format!("TerminateProcess failed for {pid}"))
        } else {
            Ok(())
        }
    }
}
