pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    command_with_flags(program, 0)
}

pub fn command_with_flags(
    program: impl AsRef<std::ffi::OsStr>,
    extra_flags: u32,
) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    hide_std(&mut command, extra_flags);
    command
}

pub fn tokio_command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    tokio_command_with_flags(program, 0)
}

pub fn tokio_command_with_flags(
    program: impl AsRef<std::ffi::OsStr>,
    extra_flags: u32,
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(program);
    hide_tokio(&mut command, extra_flags);
    command
}

pub fn visible_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    std::process::Command::new(program)
}

pub fn visible_tokio_command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    tokio::process::Command::new(program)
}

pub(crate) fn background(command: &mut tokio::process::Command) {
    hide_tokio(command, 0);
}

pub(crate) fn background_std(command: &mut std::process::Command) {
    hide_std(command, 0);
}

fn hide_tokio(command: &mut tokio::process::Command, extra: u32) {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW | extra);
    #[cfg(not(windows))]
    let _ = (command, extra);
}

fn hide_std(command: &mut std::process::Command, extra: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW | extra);
    }
    #[cfg(not(windows))]
    let _ = (command, extra);
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    const ALLOWED: &[(&str, &str)] = &[
        (
            "cursor_usage.rs",
            "portable_pty CommandBuilder: ConPTY child, no creation-flag hook",
        ),
        (
            "local_terminal.rs",
            "portable_pty CommandBuilder: ConPTY child, no creation-flag hook",
        ),
    ];

    fn scan(dir: &Path, hits: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan(&path, hits);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if name == "process_ui.rs" || ALLOWED.iter().any(|(f, _)| *f == name) {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for (n, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                if code.contains("Command::new(")
                    || code.contains("CommandBuilder::new(")
                    || code.contains("Command::from(")
                {
                    hits.push(format!("{}:{}", path.display(), n + 1));
                }
            }
        }
    }

    #[test]
    fn spawns_go_through_the_helper() {
        let mut hits = Vec::new();
        scan(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut hits,
        );
        assert!(
            hits.is_empty(),
            "build child processes with crate::process_ui, not Command::new directly: {hits:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn console_probe() {
        if std::env::var_os("CONCORDIA_CONSOLE_PROBE").is_none() {
            return;
        }
        extern "system" {
            fn GetConsoleWindow() -> *mut std::ffi::c_void;
        }
        let none = unsafe { GetConsoleWindow() }.is_null();
        println!("CONSOLE_WINDOW_NONE={none}");
    }

    #[cfg(windows)]
    #[test]
    fn helper_child_has_no_console_window() {
        let out = super::command(std::env::current_exe().unwrap())
            .args(["--exact", "process_ui::tests::console_probe", "--nocapture"])
            .env("CONCORDIA_CONSOLE_PROBE", "1")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("CONSOLE_WINDOW_NONE=true"), "{text}");
    }
}
