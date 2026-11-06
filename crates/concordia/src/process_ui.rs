//! Headless background children must not create Windows console windows.
/// Construct a hidden std child process.
pub(crate) fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    background_std(&mut command);
    command
}

/// Construct a hidden Tokio child process.
pub(crate) fn tokio_command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(program);
    background(&mut command);
    command
}

pub(crate) fn background(command: &mut tokio::process::Command) {
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW; preserves pipes and cancellation.
    #[cfg(not(windows))]
    let _ = command;
}

pub(crate) fn background_std(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}
