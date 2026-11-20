//! Run one adapter command: no shell, no stdin, capped output, hard timeout.

use super::RunOutcome;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

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
    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        return RunOutcome {
            timed_out: true,
            stderr: "cancelled before adapter startup".into(),
            ..Default::default()
        };
    }
    let Some((program, args)) = argv.split_first() else {
        return RunOutcome {
            stderr: "could not start: empty command".into(),
            ..RunOutcome::default()
        };
    };
    // Resolve as a shell would, so npm-style `.cmd` shims start; the stored name is unchanged.
    let executable = which::which(program).unwrap_or_else(|_| program.into());
    if cfg!(windows)
        && crate::provider::launch::is_batch_shim(&executable.to_string_lossy())
        && args.iter().any(|argument| {
            !argument.chars().all(|character| {
                character.is_ascii_alphanumeric() || " .,:;_/-?\\='".contains(character)
            })
        })
    {
        return RunOutcome {
            stderr: "Adapter arguments cannot pass safely through a batch launcher; select its native executable or a supported native bridge".into(),
            ..RunOutcome::default()
        };
    }
    let mut command = crate::process_ui::command(&executable);
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
    match crate::process_lifetime::capture_cancellable(command, timeout, MAX_OUTPUT, cancel) {
        Ok(output) => RunOutcome {
            exit: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            timed_out: false,
        },
        Err(error) => RunOutcome {
            timed_out: error.contains("timed out") || error.contains("cancelled"),
            stderr: error,
            ..RunOutcome::default()
        },
    }
}
