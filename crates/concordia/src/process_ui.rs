//! Headless background children must not create Windows console windows.
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
