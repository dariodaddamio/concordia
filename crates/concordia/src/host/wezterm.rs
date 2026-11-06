use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn find_wezterm() -> Option<PathBuf> {
    if let Some(path) = env::var_os("CONCORDIA_WEZTERM_BIN").filter(|p| !p.is_empty()) {
        return Some(path.into());
    }
    if let Ok(p) = which::which_in("wezterm", env::var_os("PATH"), "") {
        if p.parent()
            .is_some_and(|parent| env::current_dir().ok().as_deref() != Some(parent))
        {
            return Some(p);
        }
    }
    let mut candidates = Vec::new();
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local)
                .join("Programs")
                .join("WezTerm")
                .join("wezterm.exe"),
        );
    }
    if let Some(pf) = env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(pf).join("WezTerm").join("wezterm.exe"));
    }
    candidates.into_iter().find(|p| p.exists())
}

/// Prefer this process's exe (Rust), never the npm `concordia.ps1` shim (opens in Notepad).
pub fn find_concordia_exe() -> PathBuf {
    // Splits should match the active install pointer / CONCORDIA_EXE, not a stale sibling.
    let active = crate::update::resolve_active_exe();
    if active.is_file() {
        return active;
    }
    // After /update, stay on the slot this pane is already running (concordia-next / concordia-a / …).
    if let Ok(exe) = env::current_exe() {
        if exe
            .file_name()
            .and_then(|n| n.to_str())
            .map(crate::update::is_concordia_binary_name)
            .unwrap_or(false)
        {
            return exe;
        }
    }
    if let Some(root) =
        find_project_root(&env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    {
        let release = root.join("target").join("release").join("concordia.exe");
        if release.is_file() {
            return release;
        }
        let debug = root.join("target").join("debug").join("concordia.exe");
        if debug.is_file() {
            return debug;
        }
    }
    which::which("concordia.exe")
        .or_else(|_| which::which("concordia"))
        .unwrap_or_else(|_| PathBuf::from("concordia.exe"))
}

fn concordia_pane_argv() -> Vec<String> {
    vec![
        find_concordia_exe().to_string_lossy().into_owned(),
        "--terminal".into(),
    ]
}

pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        let cfg = cur
            .join("experiments")
            .join("terminal")
            .join("wezterm")
            .join("wezterm.lua");
        if cfg.is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// The WezTerm config, compiled into the binary so splits and the launcher work for an install
/// that has no source checkout. It lives in the repository at experiments/terminal/wezterm/.
const EMBEDDED_CONFIG: &str = include_str!("../../../../experiments/terminal/wezterm/wezterm.lua");

/// Write the embedded config into `dir` (replacing an older copy) and return its path.
pub fn ensure_config(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("wezterm.lua");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(EMBEDDED_CONFIG) {
        std::fs::write(&path, EMBEDDED_CONFIG)?;
    }
    Ok(path)
}

/// The config in Concordia's state folder, written on demand.
pub fn config_path() -> Result<PathBuf, String> {
    ensure_config(&crate::store::default_store_root())
        .map_err(|e| format!("could not write the WezTerm config: {e}"))
}

/// A WezTerm window whose first pane runs Concordia, using the bundled config.
pub fn launch_command(wez: &Path, config: &Path, exe: &Path, cwd: &Path) -> Command {
    let mut cmd = crate::process_ui::command(wez);
    cmd.env("WEZTERM_CONFIG_FILE", config)
        .env("CONCORDIA_EXE", exe)
        .arg("start")
        .arg("--cwd")
        .arg(cwd)
        .arg("--")
        .arg(exe)
        .arg("--terminal");
    cmd
}

/// `concordia wezterm`: open WezTerm running Concordia with the bundled keys (Ctrl+Shift+D/E
/// split, Ctrl+Shift+T new tab, each pane running its own Concordia).
pub fn launch_window() -> Result<(), String> {
    let wez = find_wezterm()
        .ok_or_else(|| "WezTerm was not found (install it from wezterm.org)".to_string())?;
    let config = config_path()?;
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    launch_command(&wez, &config, &find_concordia_exe(), &cwd)
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not start WezTerm: {e}"))
}

/// Split the current WezTerm pane and spawn another Concordia (requires WEZTERM_PANE).
pub fn split_concordia_pane(vertical: bool) -> Result<(), String> {
    let pane_id = env::var("WEZTERM_PANE")
        .map_err(|_| "split needs WezTerm (WEZTERM_PANE unset) - run `concordia wezterm`, then Ctrl+Shift+D/E".to_string())?;
    let wez = find_wezterm().ok_or_else(|| "wezterm not found".to_string())?;
    let config = config_path()?;
    let dir = if vertical { "--bottom" } else { "--right" };
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut cmd = crate::process_ui::command(wez);
    cmd.env("WEZTERM_CONFIG_FILE", &config);
    let concordia = find_concordia_exe();
    cmd.env("CONCORDIA_EXE", &concordia);
    cmd.args([
        "cli",
        "split-pane",
        "--pane-id",
        &pane_id,
        dir,
        "--percent",
        "50",
        "--cwd",
    ]);
    cmd.arg(&cwd);
    cmd.arg("--");
    for a in concordia_pane_argv() {
        cmd.arg(a);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Err(if !err.trim().is_empty() {
            err.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            "split-pane failed".into()
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_config_is_written_once_and_refreshed_when_it_changes() {
        let d = tempfile::tempdir().unwrap();
        let p = ensure_config(d.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), EMBEDDED_CONFIG);
        assert!(
            EMBEDDED_CONFIG.contains("config.keys") && EMBEDDED_CONFIG.contains("SplitHorizontal")
        );
        // An older or edited copy is replaced; an identical one is left alone.
        std::fs::write(&p, "-- stale").unwrap();
        ensure_config(d.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), EMBEDDED_CONFIG);
        let before = std::fs::metadata(&p).unwrap().modified().unwrap();
        ensure_config(d.path()).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn the_launcher_starts_wezterm_with_the_bundled_config_and_this_exe() {
        let cmd = launch_command(
            Path::new(r"C:\wez\wezterm.exe"),
            Path::new(r"C:\state\wezterm.lua"),
            Path::new(r"C:\bin\concordia.exe"),
            Path::new(r"C:\work"),
        );
        assert_eq!(cmd.get_program(), r"C:\wez\wezterm.exe");
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "start",
                "--cwd",
                r"C:\work",
                "--",
                r"C:\bin\concordia.exe",
                "--terminal"
            ]
        );
        let envs: Vec<_> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert!(envs.contains(&(
            "WEZTERM_CONFIG_FILE".into(),
            Some(r"C:\state\wezterm.lua".into())
        )));
        assert!(envs.contains(&("CONCORDIA_EXE".into(), Some(r"C:\bin\concordia.exe".into()))));
    }
}
