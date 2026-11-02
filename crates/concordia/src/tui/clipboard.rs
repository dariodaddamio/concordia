//! Native Windows text clipboard. No subprocess or shell startup on copy/cut/paste.
//! The bounded helper runner is retained for the asynchronous image/file pickers.
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
    pub(super) static FAIL_WRITES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Clipboard text without trimming or discarding line endings.
#[cfg(test)]
pub fn get_text() -> Result<String, String> {
    Ok(TEST_CLIPBOARD.with(|c| c.borrow().clone()))
}

#[cfg(test)]
pub fn set_text(text: &str) -> Result<(), String> {
    if FAIL_WRITES.get() {
        return Err("clipboard is busy; try again".into());
    }
    TEST_CLIPBOARD.with(|c| *c.borrow_mut() = text.to_string());
    Ok(())
}

#[cfg(all(not(test), not(windows)))]
pub use system::{get_text, set_text};

#[cfg(all(not(test), not(windows)))]
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
        cmd.args(["-NoProfile", "-NonInteractive", "-STA", "-Command", script]);
        hide_window(&mut cmd);
        cmd
    }

    /// Clipboard text without trimming or discarding line endings.
    pub fn get_text() -> Result<String, String> {
        let out = run_with_timeout(
            powershell("$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.Encoding]::UTF8; [Console]::Write((Get-Clipboard -Raw))"),
            None,
            CLIPBOARD_TIMEOUT,
        )
        .map_err(|e| format!("clipboard read failed: {e}"))?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    pub fn set_text(text: &str) -> Result<(), String> {
        run_with_timeout(
            powershell(
                "$ErrorActionPreference='Stop'; [Console]::InputEncoding=[Text.Encoding]::UTF8; $copyText=[Console]::In.ReadToEnd(); for ($attempt=0; $attempt -lt 5; $attempt++) { try { Set-Clipboard -Value $copyText -ErrorAction Stop; exit 0 } catch { if ($attempt -eq 4) { exit 1 }; Start-Sleep -Milliseconds 50 } }",
            ),
            Some(text.as_bytes()),
            CLIPBOARD_TIMEOUT,
        )
        .map(drop)
        .map_err(|e| format!("clipboard write failed: {e}"))
    }
}

#[cfg(all(windows, not(test)))]
pub use native::{get_text, has_image, set_text};

#[cfg(any(test, not(windows)))]
pub fn has_image() -> bool {
    false
}

#[cfg(all(windows, not(test)))]
mod native {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, HWND_MESSAGE,
    };
    const UNICODE_TEXT: u32 = 13;
    const MAX_BYTES: usize = 16 * 1024 * 1024;

    struct Open;
    impl Drop for Open {
        fn drop(&mut self) {
            // SAFETY: only constructed after this thread opens the clipboard.
            unsafe {
                CloseClipboard();
            }
        }
    }
    struct Owner(windows_sys::Win32::Foundation::HWND);
    impl Drop for Owner {
        fn drop(&mut self) {
            // SAFETY: the hidden message-only window belongs to this call/thread.
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }
    fn open(owner: windows_sys::Win32::Foundation::HWND) -> Result<Open, String> {
        for attempt in 0..4 {
            // SAFETY: owner is null for a read, or a live window on this thread.
            if unsafe { OpenClipboard(owner) } != 0 {
                return Ok(Open);
            }
            if attempt < 3 {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        Err("clipboard is busy; try again".into())
    }
    pub fn has_image() -> bool {
        // CF_BITMAP, CF_DIB and CF_DIBV5; no data is read by this inexpensive query.
        [2, 8, 17]
            .into_iter()
            .any(|format| unsafe { IsClipboardFormatAvailable(format) != 0 })
    }
    pub fn get_text() -> Result<String, String> {
        let _open = open(null_mut())?;
        // SAFETY: clipboard is held for the duration of the borrowed data/lock.
        unsafe {
            let data = GetClipboardData(UNICODE_TEXT);
            if data.is_null() {
                return Ok(String::new());
            }
            let bytes = GlobalSize(data);
            if bytes > MAX_BYTES || !bytes.is_multiple_of(2) {
                return Err("clipboard text exceeds preview limit or is invalid".into());
            }
            let ptr = GlobalLock(data) as *const u16;
            if ptr.is_null() {
                return Err("clipboard text could not be locked".into());
            }
            let wide = std::slice::from_raw_parts(ptr, bytes / 2);
            let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
            let text = String::from_utf16_lossy(&wide[..end]);
            GlobalUnlock(data);
            Ok(text)
        }
    }
    pub fn set_text(text: &str) -> Result<(), String> {
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let bytes = wide.len().saturating_mul(2);
        if bytes > MAX_BYTES {
            return Err("clipboard text exceeds 16 MiB".into());
        }
        // SAFETY: STATIC is a system window class; the window is never shown. A real
        // owner is required: EmptyClipboard with a null owner prevents SetClipboardData.
        unsafe {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                null_mut(),
                null_mut(),
                null(),
            );
            if hwnd.is_null() {
                return Err("clipboard owner could not be created".into());
            }
            let _owner = Owner(hwnd);
            let _open = open(hwnd)?;
            let data = GlobalAlloc(GMEM_MOVEABLE, bytes);
            if data.is_null() {
                return Err("clipboard allocation failed".into());
            }
            let ptr = GlobalLock(data) as *mut u16;
            if ptr.is_null() {
                GlobalFree(data);
                return Err("clipboard allocation could not be locked".into());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            GlobalUnlock(data);
            if EmptyClipboard() == 0 || SetClipboardData(UNICODE_TEXT, data).is_null() {
                GlobalFree(data);
                return Err("clipboard write failed; selection retained".into());
            }
            // Windows now owns data; do not free it.
            Ok(())
        }
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
