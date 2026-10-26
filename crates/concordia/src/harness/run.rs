//! Run one adapter command: no shell, no stdin, capped output, hard timeout.

use super::RunOutcome;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
    let Some((program, args)) = argv.split_first() else {
        return RunOutcome {
            stderr: "could not start: empty command".into(),
            ..RunOutcome::default()
        };
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env("CONCORDIA_HARNESS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
    let out = child.stdout.take().map(drain);
    let err = child.stderr.take().map(drain);
    let deadline = Instant::now() + timeout;
    let (exit, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code(), false),
            Ok(None) if Instant::now() >= deadline => {
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
