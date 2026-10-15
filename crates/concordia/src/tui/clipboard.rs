//! System clipboard via PowerShell (no extra dependencies). UTF-8 is forced in both directions:
//! Windows PowerShell 5.1 otherwise uses the OEM code page and mangles non-ASCII text.
//!
//! Every call has a deadline, because these run on the UI thread: a wedged PowerShell must turn
//! into an error message, never a frozen terminal.
//!
//! Under `cfg(test)` an in-memory slot stands in, so tests never touch the user's clipboard.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Run `cmd` (optionally feeding it `input` on stdin) and return its stdout, giving up and
/// killing it after `timeout`. Stdin is written and stdout read on their own threads so a
/// full pipe on either side cannot deadlock the wait.
pub fn run_with_timeout(
    mut cmd: Command,
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| format!("could not start: {e}"))?;

    if let (Some(data), Some(mut stdin)) = (input, child.stdin.take()) {
        let data = data.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
            // Dropping stdin closes it, which is the helper's EOF.
        });
    }
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().unwrap_or_default();
                return if status.success() {
                    Ok(out)
                } else {
                    Err("the helper reported an error".into())
                };
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}s", timeout.as_secs().max(1)));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[cfg(test)]
thread_local! {
    static TEST_CLIPBOARD: std::cell::RefCell<String> =
        const { std::cell::RefCell::new(String::new()) };
}

/// Clipboard text with `\r` removed.
#[cfg(test)]
pub fn get_text() -> Result<String, String> {
    Ok(TEST_CLIPBOARD.with(|c| c.borrow().clone()))
}

#[cfg(test)]
pub fn set_text(text: &str) -> Result<(), String> {
    TEST_CLIPBOARD.with(|c| *c.borrow_mut() = text.to_string());
    Ok(())
}

#[cfg(not(test))]
pub use system::{get_text, set_text};

#[cfg(not(test))]
mod system {
    use super::run_with_timeout;
    use std::process::Command;
    use std::time::Duration;

    /// How long the UI will wait for the clipboard helper.
    const CLIPBOARD_TIMEOUT: Duration = Duration::from_secs(8);

    #[cfg(windows)]
    fn hide_window(cmd: &mut Command) {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    #[cfg(not(windows))]
    fn hide_window(_cmd: &mut Command) {}

    fn powershell(script: &str) -> Command {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        hide_window(&mut cmd);
        cmd
    }

    /// Clipboard text with `\r` removed.
    pub fn get_text() -> Result<String, String> {
        let out = run_with_timeout(
            powershell("[Console]::OutputEncoding=[Text.Encoding]::UTF8; Get-Clipboard -Raw"),
            None,
            CLIPBOARD_TIMEOUT,
        )
        .map_err(|e| format!("clipboard read failed: {e}"))?;
        Ok(String::from_utf8_lossy(&out).replace('\r', ""))
    }

    pub fn set_text(text: &str) -> Result<(), String> {
        run_with_timeout(
            powershell(
                "[Console]::InputEncoding=[Text.Encoding]::UTF8; Set-Clipboard -Value ([Console]::In.ReadToEnd())",
            ),
            Some(text.as_bytes()),
            CLIPBOARD_TIMEOUT,
        )
        .map(drop)
        .map_err(|e| format!("clipboard write failed: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ps(script: &str) -> Command {
        let mut c = Command::new("powershell.exe");
        c.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        c
    }

    #[test]
    fn a_normal_helper_returns_its_output() {
        let out =
            run_with_timeout(ps("Write-Output hello"), None, Duration::from_secs(20)).unwrap();
        assert_eq!(String::from_utf8_lossy(&out).trim(), "hello");
    }

    #[test]
    fn stdin_reaches_the_helper_even_for_a_large_payload() {
        let big = "x".repeat(300_000); // larger than a pipe buffer in both directions
        let out = run_with_timeout(
            ps("[Console]::In.ReadToEnd().Length"),
            Some(big.as_bytes()),
            Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!(String::from_utf8_lossy(&out).trim(), "300000");
    }

    #[test]
    fn a_wedged_helper_becomes_a_prompt_error_not_a_freeze() {
        let start = Instant::now();
        let err = run_with_timeout(
            ps("Start-Sleep -Seconds 60"),
            None,
            Duration::from_millis(700),
        )
        .unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn a_failing_helper_is_an_error() {
        assert!(run_with_timeout(ps("exit 3"), None, Duration::from_secs(20)).is_err());
        assert!(run_with_timeout(
            Command::new("definitely-not-a-real-program"),
            None,
            Duration::from_secs(5)
        )
        .is_err());
    }
}
