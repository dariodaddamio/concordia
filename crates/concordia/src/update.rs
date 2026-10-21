//! In-pane self-update: obtain a new binary (build from source, or download a release), verify
//! it runs, stage it into an unlocked install slot, flip the pointer, re-exec.

use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// The one file name `concordia` resolves to on PATH. It is always the newest installed build.
pub const PRIMARY_SLOT: &str = "concordia.exe";

#[derive(Debug, Clone)]
pub struct UpdateResult {
    pub install_path: PathBuf,
    pub built_from: PathBuf,
    pub log_tail: String,
    /// One-line human summary ("updated 0.1.0 -> 0.1.2"); empty for a source rebuild.
    pub summary: String,
}

pub fn is_concordia_binary_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "concordia"
        || lower == "concordia.exe"
        || lower.starts_with("concordia-") && lower.ends_with(".exe")
}

/// Where installed binaries and the `current` pointer live. `CONCORDIA_INSTALL_DIR` overrides the
/// default (`%LOCALAPPDATA%\concordia\bin`) for portable installs and for tests.
pub fn install_bin_dir() -> PathBuf {
    if let Ok(dir) = env::var("CONCORDIA_INSTALL_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("concordia")
        .join("bin")
}

/// Pointer file: one line = absolute path of the active install slot.
pub fn current_pointer_path() -> PathBuf {
    install_bin_dir().join("current")
}

/// Resolve the binary every launcher must use (pointer → CONCORDIA_EXE → newest slot).
pub fn resolve_active_exe() -> PathBuf {
    if let Ok(raw) = fs::read_to_string(current_pointer_path()) {
        let p = PathBuf::from(raw.trim());
        if p.is_file() {
            return p;
        }
    }
    if let Ok(p) = env::var("CONCORDIA_EXE") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return pb;
        }
    }
    let primary = install_bin_dir().join(PRIMARY_SLOT);
    if primary.is_file() {
        return primary;
    }
    env::current_exe().unwrap_or_else(|_| PathBuf::from("concordia.exe"))
}

/// Point every launcher at `path`. Written via a temp file + rename so a crash can never leave a
/// half-written pointer.
pub fn write_current_pointer(path: &Path) -> std::io::Result<()> {
    let bin = install_bin_dir();
    fs::create_dir_all(&bin)?;
    let tmp = bin.join(format!("current.tmp-{}", std::process::id()));
    fs::write(&tmp, path.to_string_lossy().as_bytes())?;
    fs::rename(&tmp, current_pointer_path())
}

/// Run `<exe> --version` and require an concordia version line. Called before an update is installed:
/// a corrupt, truncated or wrong-architecture binary must never become what every launcher runs.
pub fn verify_binary(exe: &Path) -> Result<String> {
    let line = crate::doctor::probe_version(exe)
        .map_err(|e| anyhow::anyhow!("{} does not run: {e}", exe.display()))?;
    if !line.starts_with("concordia ") {
        bail!(
            "{} is not an concordia binary (--version said {line:?})",
            exe.display()
        );
    }
    Ok(line)
}

// ---- background jobs ---------------------------------------------------------------------------
//
// A rebuild takes up to a minute and a release download depends on the network, so neither may
// run on the UI thread. A job reports progress through `JobControl::say`, can be cancelled, and
// delivers one result.

/// Shared between the job thread and the UI: a cancel flag and the latest progress line.
#[derive(Clone, Default)]
pub struct JobControl {
    cancel: Arc<AtomicBool>,
    status: Arc<Mutex<String>>,
}

impl JobControl {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn say(&self, line: &str) {
        if let Ok(mut s) = self.status.lock() {
            *s = line.trim().to_string();
        }
    }

    pub fn status(&self) -> String {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Update,
    Check,
}

pub enum JobResult {
    Update(Result<UpdateOutcome>),
    Check(Result<String>),
}

pub struct Job {
    pub kind: JobKind,
    pub started: Instant,
    ctl: JobControl,
    rx: mpsc::Receiver<JobResult>,
}

impl Job {
    /// The result, once the job has finished.
    pub fn poll(&self) -> Option<JobResult> {
        self.rx.try_recv().ok()
    }

    pub fn cancel(&self) {
        self.ctl.cancel.store(true, Ordering::SeqCst);
    }

    pub fn status(&self) -> String {
        self.ctl.status()
    }

    pub fn is_cancelled(&self) -> bool {
        self.ctl.cancelled()
    }
}

pub fn spawn_job(
    kind: JobKind,
    work: impl FnOnce(&JobControl) -> JobResult + Send + 'static,
) -> Job {
    let ctl = JobControl::default();
    let (tx, rx) = mpsc::channel();
    let worker_ctl = ctl.clone();
    std::thread::spawn(move || {
        let _ = tx.send(work(&worker_ctl));
    });
    Job {
        kind,
        started: Instant::now(),
        ctl,
        rx,
    }
}

pub fn start_update(cwd: PathBuf) -> Job {
    spawn_job(JobKind::Update, move |ctl| {
        JobResult::Update(update_auto_ctl(&cwd, ctl))
    })
}

pub fn start_check(cwd: PathBuf) -> Job {
    spawn_job(JobKind::Check, move |_| {
        JobResult::Check(check_for_update(&cwd))
    })
}

/// Run `cmd`, relaying its latest output line as progress, and kill its whole process tree if the
/// job is cancelled. Returns `(success, exit code, last lines of output)`.
pub fn run_child_with_cancel(mut cmd: Command, ctl: &JobControl) -> Result<(bool, i32, String)> {
    use std::collections::VecDeque;
    use std::io::{BufRead, BufReader};
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().context("spawn build")?;
    let tail: Arc<Mutex<VecDeque<String>>> = Arc::default();
    let mut readers = Vec::new();
    for stream in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let (tail, ctl) = (Arc::clone(&tail), ctl.clone());
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(|l| l.ok()) {
                if !line.trim().is_empty() {
                    ctl.say(&line);
                    if let Ok(mut t) = tail.lock() {
                        if t.len() == 40 {
                            t.pop_front();
                        }
                        t.push_back(line);
                    }
                }
            }
        }));
    }
    let finish = |readers: Vec<std::thread::JoinHandle<()>>| {
        for r in readers {
            let _ = r.join();
        }
    };
    loop {
        if ctl.cancelled() {
            crate::provider::turn::kill_process_tree(child.id());
            let _ = child.kill();
            let _ = child.wait();
            finish(readers);
            bail!("cancelled");
        }
        match child.try_wait().context("wait for build")? {
            Some(status) => {
                finish(readers);
                let lines = tail
                    .lock()
                    .map(|t| t.iter().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                return Ok((
                    status.success(),
                    status.code().unwrap_or(-1),
                    tail_lines(&lines.join("\n"), 12),
                ));
            }
            None => std::thread::sleep(Duration::from_millis(40)),
        }
    }
}

/// What `/update` did.
#[derive(Debug, Clone)]
pub enum UpdateOutcome {
    /// A new build is installed; the caller should re-exec into it.
    Installed(UpdateResult),
    /// Nothing newer exists (message for the user).
    UpToDate(String),
}

const SOURCE_HINT_FILE: &str = "source";

fn is_source_root(dir: &Path) -> bool {
    dir.join("crates")
        .join("concordia")
        .join("Cargo.toml")
        .is_file()
}

/// For an exe built in a checkout (`<root>/target/{release,debug}/concordia.exe`), the checkout.
pub fn source_root_of_exe(exe: &Path) -> Option<PathBuf> {
    let target = exe.parent()?.parent()?;
    if target.file_name()? != "target" {
        return None;
    }
    let root = target.parent()?;
    is_source_root(root).then(|| root.to_path_buf())
}

/// Remember where this install was built from, so `/update` can rebuild it from any directory.
pub fn record_source_root(root: &Path) {
    let dir = install_bin_dir();
    if fs::create_dir_all(&dir).is_ok() {
        let _ = fs::write(
            dir.join(SOURCE_HINT_FILE),
            root.to_string_lossy().as_bytes(),
        );
    }
}

fn recorded_source_root() -> Option<PathBuf> {
    let raw = fs::read_to_string(install_bin_dir().join(SOURCE_HINT_FILE)).ok()?;
    let root = PathBuf::from(raw.trim());
    is_source_root(&root).then_some(root)
}

/// The source checkout `/update` rebuilds: the one containing `cwd`, else the one this install
/// was built from.
pub fn source_root(cwd: &Path) -> Option<PathBuf> {
    crate::host::wezterm::find_project_root(cwd)
        .or_else(|| find_cargo_workspace(cwd))
        .or_else(recorded_source_root)
}

/// `/update`: rebuild from source when run inside a checkout, otherwise download the newest
/// published release (when a release feed is configured).
pub fn update_auto(cwd: &Path) -> Result<UpdateOutcome> {
    update_auto_ctl(cwd, &JobControl::default())
}

pub fn update_auto_ctl(cwd: &Path, ctl: &JobControl) -> Result<UpdateOutcome> {
    if source_root(cwd).is_some() {
        return perform_update_ctl(cwd, ctl).map(UpdateOutcome::Installed);
    }
    let current = crate::version::VERSION;
    let cfg = crate::release::config_from_env(current).context(
        "no source checkout here and no release feed configured (set CONCORDIA_UPDATE_REPO=owner/name)",
    )?;
    let fetch = crate::release::PowerShellFetcher;
    ctl.say("checking for a newer release");
    let found = crate::release::check(&cfg, current, &fetch)?;
    if ctl.cancelled() {
        bail!("cancelled");
    }
    match found {
        crate::release::Check::UpToDate { current, .. } => Ok(UpdateOutcome::UpToDate(format!(
            "already up to date ({current})"
        ))),
        crate::release::Check::Available(rel) => {
            ctl.say(&format!("downloading {}", rel.tag));
            crate::release::install_release(&cfg, &rel, current, &fetch)
                .map(UpdateOutcome::Installed)
        }
    }
}

/// `/update check`: say what `/update` would do, without installing anything.
pub fn check_for_update(cwd: &Path) -> Result<String> {
    if let Some(root) = source_root(cwd) {
        return Ok(format!(
            "/update rebuilds from the source checkout at {}",
            root.display()
        ));
    }
    let current = crate::version::VERSION;
    let cfg = crate::release::config_from_env(current).context(
        "no source checkout here and no release feed configured (set CONCORDIA_UPDATE_REPO=owner/name)",
    )?;
    Ok(
        match crate::release::check(&cfg, current, &crate::release::PowerShellFetcher)? {
            crate::release::Check::UpToDate { current, latest } => {
                format!("up to date: running {current}, newest release {latest}")
            }
            crate::release::Check::Available(rel) => {
                format!(
                    "update available: {} -> {} (run /update)",
                    current, rel.version
                )
            }
        },
    )
}

/// Build `concordia` release and copy into a slot that is not the currently running exe.
pub fn perform_update(cwd: &Path) -> Result<UpdateResult> {
    perform_update_ctl(cwd, &JobControl::default())
}

pub fn perform_update_ctl(cwd: &Path, ctl: &JobControl) -> Result<UpdateResult> {
    let root = source_root(cwd).context(
        "no source checkout found: run /update inside the repository once, or run `concordia install` from a build in it",
    )?;

    let cargo = which::which("cargo").context("cargo not on PATH")?;
    // Prefer default target/; if this process is that release exe (locked), build aside.
    let default_built = root.join("target").join("release").join("concordia.exe");
    let aside_dir = root.join("target").join("concordia-update");
    let aside_built = aside_dir.join("release").join("concordia.exe");
    let running_default = env::current_exe()
        .ok()
        .and_then(|p| fs::canonicalize(p).ok())
        .and_then(|cur| fs::canonicalize(&default_built).ok().map(|def| def == cur))
        .unwrap_or(false);

    let (built, log_tail) = if running_default {
        run_cargo_build(&cargo, &root, Some(&aside_dir), ctl)?
    } else {
        match run_cargo_build(&cargo, &root, None, ctl) {
            Ok(v) => v,
            Err(e) => {
                // Linker cannot overwrite a locked target/release/concordia.exe — retry aside.
                let msg = format!("{e:#}");
                if msg.to_ascii_lowercase().contains("access")
                    || msg.to_ascii_lowercase().contains("denied")
                    || msg.to_ascii_lowercase().contains("being used")
                {
                    run_cargo_build(&cargo, &root, Some(&aside_dir), ctl)?
                } else {
                    return Err(e);
                }
            }
        }
    };
    let built = if built.is_file() {
        built
    } else if aside_built.is_file() {
        aside_built
    } else {
        bail!("missing built binary at {}", default_built.display());
    };

    let install_path = install_binary(&built)?;
    record_source_root(&root);
    Ok(UpdateResult {
        install_path,
        built_from: built,
        log_tail,
        summary: String::new(),
    })
}

/// Install a verified binary as `<bin>\concordia.exe` (what `concordia` on PATH runs) and flip the pointer.
///
/// A running exe cannot be overwritten on Windows, but it *can* be renamed. So the new build is
/// staged next to the old one, the old file is moved aside, and the new one takes its name. A
/// pane still running the old build is untouched and keeps working; every new launch gets the new
/// build. This is what stops `concordia` from silently resolving to a stale copy.
pub fn install_binary(built: &Path) -> Result<PathBuf> {
    // Never install something that does not run.
    verify_binary(built)?;
    let built_len = fs::metadata(built)?.len();

    let bin_dir = install_bin_dir();
    fs::create_dir_all(&bin_dir).with_context(|| format!("mkdir {}", bin_dir.display()))?;
    let dest = bin_dir.join(PRIMARY_SLOT);

    swap_in(built, &dest, built_len)?;
    sweep_leftovers(&bin_dir);
    persist_concordia_exe_env(&dest);
    Ok(dest)
}

fn unique_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos}", std::process::id())
}

/// Replace `dest` with a copy of `built`, tolerating `dest` being a running executable.
fn swap_in(built: &Path, dest: &Path, built_len: u64) -> Result<()> {
    let dir = dest.parent().context("install path has no parent")?;
    let tag = unique_tag();
    let staged = dir.join(format!("{PRIMARY_SLOT}.new-{tag}"));
    let aside = dir.join(format!("{PRIMARY_SLOT}.old-{tag}"));

    fs::copy(built, &staged).with_context(|| format!("copy to {}", staged.display()))?;
    // A short copy (disk full, antivirus) must never become the active build.
    if fs::metadata(&staged).map(|m| m.len()).unwrap_or(0) != built_len {
        let _ = fs::remove_file(&staged);
        bail!("copy to {} was incomplete", staged.display());
    }

    let had_old = dest.exists();
    if had_old {
        if let Err(e) = fs::rename(dest, &aside) {
            let _ = fs::remove_file(&staged);
            bail!("cannot move the old {} aside: {e}", dest.display());
        }
    }
    if let Err(e) = fs::rename(&staged, dest) {
        // Put the old one back so the install is never left without an concordia.exe.
        if had_old {
            let _ = fs::rename(&aside, dest);
        }
        let _ = fs::remove_file(&staged);
        bail!("cannot install {}: {e}", dest.display());
    }
    Ok(())
}

/// Delete files left by earlier swaps. A moved-aside exe that a pane is still running cannot be
/// deleted yet; it is simply retried on the next install. A staged copy is only removed once it
/// is clearly abandoned, so a concurrent install is never disturbed.
fn sweep_leftovers(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let abandoned = if name.starts_with(&format!("{PRIMARY_SLOT}.old-")) {
            true
        } else if name.starts_with(&format!("{PRIMARY_SLOT}.new-")) {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > std::time::Duration::from_secs(600))
        } else {
            false
        };
        if abandoned {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// `concordia install`: put *this* executable where `concordia` on PATH finds it, make sure that directory is
/// on the user's PATH, and point every launcher at it. The one install story for a downloaded exe
/// and for a source build alike.
pub struct InstallReport {
    pub installed: PathBuf,
    pub path_added: bool,
    /// An install from before the rename (`%LOCALAPPDATA%\orc\bin`) that was taken off PATH.
    pub legacy_retired: Option<PathBuf>,
    /// Settings and registered accounts copied over from the pre-rename state folder.
    pub state_adopted: bool,
}

pub fn install_self() -> Result<InstallReport> {
    let me = env::current_exe().context("cannot locate the running executable")?;
    let installed = install_binary(&me)?;
    if let Some(root) = source_root_of_exe(&me) {
        record_source_root(&root);
    }
    let explicit_dir = env::var("CONCORDIA_INSTALL_DIR").is_ok_and(|d| !d.trim().is_empty());
    let state_adopted = !explicit_dir
        && crate::store::adopt_legacy_state(&crate::store::default_store_root()).unwrap_or(false);
    let (path_added, legacy_retired) = if explicit_dir {
        (false, None)
    } else {
        (
            ensure_on_user_path(&install_bin_dir())?,
            retire_legacy_install().unwrap_or(None),
        )
    };
    Ok(InstallReport {
        installed,
        path_added,
        legacy_retired,
        state_adopted,
    })
}

/// This product was called `orc` before it was renamed. Its install lived in
/// `%LOCALAPPDATA%\orc\bin`; left on PATH it would keep answering to `orc` with a stale build.
pub fn legacy_install_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("orc").join("bin"))
}

/// `existing` with `dir` removed (same comparison as `path_with_dir`), or `None` if absent.
pub fn path_without_dir(existing: &str, dir: &Path) -> Option<String> {
    let norm = |p: &str| p.trim().trim_end_matches(['\\', '/']).to_ascii_lowercase();
    let want = norm(&dir.to_string_lossy());
    let parts: Vec<&str> = existing.split(';').collect();
    if !parts.iter().any(|e| norm(e) == want) {
        return None;
    }
    Some(
        parts
            .into_iter()
            .filter(|e| norm(e) != want && !e.trim().is_empty())
            .collect::<Vec<_>>()
            .join(";"),
    )
}

/// Take the pre-rename install off the user PATH. Its files are left alone (they hold the old
/// binary and the homes of accounts created by it); the user can delete them when ready.
pub fn retire_legacy_install() -> Result<Option<PathBuf>> {
    let Some(old) = legacy_install_dir().filter(|d| d.is_dir()) else {
        return Ok(None);
    };
    let existing = user_path_ps(
        "(Get-Item 'HKCU:\\Environment').GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)",
        &[],
    )?;
    if let Some(updated) = path_without_dir(&existing, &old) {
        user_path_ps(
            "Set-ItemProperty -Path 'HKCU:\\Environment' -Name Path -Value $env:CONCORDIA_NEW_PATH -Type ExpandString; \
             [Environment]::SetEnvironmentVariable('CONCORDIA_PATH_REFRESH', [string](Get-Date).Ticks, 'User'); \
             [Environment]::SetEnvironmentVariable('CONCORDIA_PATH_REFRESH', $null, 'User')",
            &[("CONCORDIA_NEW_PATH", &updated)],
        )?;
    }
    Ok(Some(old))
}

/// `existing` with `dir` added at the front, or `None` when it is already present (compared
/// case-insensitively, ignoring a trailing backslash, as Windows does).
pub fn path_with_dir(existing: &str, dir: &Path) -> Option<String> {
    let norm = |p: &str| p.trim().trim_end_matches(['\\', '/']).to_ascii_lowercase();
    let want = norm(&dir.to_string_lossy());
    if existing.split(';').any(|e| norm(e) == want) {
        return None;
    }
    let dir = dir.to_string_lossy();
    Some(if existing.trim().is_empty() {
        dir.into_owned()
    } else {
        format!("{dir};{existing}")
    })
}

fn user_path_ps(script: &str, env: &[(&str, &str)]) -> Result<String> {
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().context("run powershell.exe")?;
    if !out.status.success() {
        bail!(
            "{}",
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .next()
                .unwrap_or("powershell failed")
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Add `dir` to the *user* PATH if missing. The registry value is read and written unexpanded
/// (REG_EXPAND_SZ), so entries such as `%USERPROFILE%\bin` survive; `setx` would truncate PATH at
/// 1024 characters. Returns whether anything changed.
pub fn ensure_on_user_path(dir: &Path) -> Result<bool> {
    let existing = user_path_ps(
        "(Get-Item 'HKCU:\\Environment').GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)",
        &[],
    )?;
    let Some(updated) = path_with_dir(&existing, dir) else {
        return Ok(false);
    };
    // Setting CONCORDIA_EXE through .NET also broadcasts the environment change to running programs.
    user_path_ps(
        "Set-ItemProperty -Path 'HKCU:\\Environment' -Name Path -Value $env:CONCORDIA_NEW_PATH -Type ExpandString; \
         [Environment]::SetEnvironmentVariable('CONCORDIA_PATH_REFRESH', [string](Get-Date).Ticks, 'User'); \
         [Environment]::SetEnvironmentVariable('CONCORDIA_PATH_REFRESH', $null, 'User')",
        &[("CONCORDIA_NEW_PATH", &updated)],
    )?;
    Ok(true)
}

/// Leave TUI callers: run the new binary in this console and wait for it.
///
/// Returns the new process's exit code. This process must stay alive until the child exits: if it
/// returned the console to the shell early, the shell and the new Concordia would both read the
/// keyboard and swallow each other's keystrokes.
pub fn reexec_into(new_exe: &Path, cwd: &Path) -> Result<i32> {
    persist_concordia_exe_env(new_exe);
    let status = Command::new(new_exe)
        .current_dir(cwd)
        .env("CONCORDIA_EXE", new_exe)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("spawn {}", new_exe.display()))?;
    Ok(status.code().unwrap_or(1))
}

fn run_cargo_build(
    cargo: &Path,
    root: &Path,
    target_dir: Option<&Path>,
    ctl: &JobControl,
) -> Result<(PathBuf, String)> {
    let mut cmd = Command::new(cargo);
    cmd.args(["build", "-p", "concordia", "--release"])
        .current_dir(root);
    if let Some(dir) = target_dir {
        cmd.arg("--target-dir").arg(dir);
    }
    let (ok, code, log_tail) = run_child_with_cancel(cmd, ctl)?;
    if !ok {
        bail!("cargo build failed ({code})\n{log_tail}");
    }
    let built = match target_dir {
        Some(dir) => dir.join("release").join("concordia.exe"),
        None => root.join("target").join("release").join("concordia.exe"),
    };
    Ok((built, log_tail))
}

fn persist_concordia_exe_env(path: &Path) {
    let _ = write_current_pointer(path);
    let s = path.to_string_lossy().into_owned();
    env::set_var("CONCORDIA_EXE", &s);
    // An explicit install dir (portable install, tests) must not touch the user's environment.
    if env::var("CONCORDIA_INSTALL_DIR").is_ok_and(|d| !d.trim().is_empty()) {
        return;
    }
    // Best-effort User env so new WezTerm panes / shells pick it up.
    let _ = Command::new("setx")
        .args(["CONCORDIA_EXE", &s])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn find_cargo_workspace(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        let manifest = cur.join("Cargo.toml");
        if manifest.is_file() {
            if let Ok(raw) = fs::read_to_string(&manifest) {
                if raw.contains("[workspace]") || cur.join("crates").join("concordia").is_dir() {
                    return Some(cur);
                }
            }
        }
        if !cur.pop() {
            return None;
        }
    }
}

fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<_> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn reexec_waits_for_the_child_and_returns_its_exit_code() {
        let _guard = crate::accounts::TEST_ENV
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let sandbox = tempfile::tempdir().unwrap();
        struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (key, value) in &self.0 {
                    match value {
                        Some(value) => env::set_var(key, value),
                        None => env::remove_var(key),
                    }
                }
            }
        }
        let _restore = Restore(
            ["CONCORDIA_INSTALL_DIR", "CONCORDIA_EXE"]
                .into_iter()
                .map(|key| (key, env::var_os(key)))
                .collect(),
        );
        // Re-exec persists its path. A test must never write the real pointer or User env.
        env::set_var("CONCORDIA_INSTALL_DIR", sandbox.path());
        // `where.exe` with no arguments fails with exit code 2. Returning at all proves we waited
        // (an early return would hand the console to the shell while the child still reads keys).
        let exe = which::which("where.exe").unwrap();
        let code = reexec_into(&exe, &std::env::temp_dir()).unwrap();
        assert_eq!(code, 2);
        assert_eq!(current_pointer_path().parent(), Some(sandbox.path()));
        assert_eq!(
            fs::read_to_string(current_pointer_path()).unwrap(),
            exe.to_string_lossy()
        );
    }

    use super::*;

    #[test]
    fn path_additions_are_idempotent_and_windows_flavoured() {
        let dir = Path::new(r"C:\Users\me\AppData\Local\concordia\bin");
        let added = path_with_dir(r"C:\Windows;C:\Tools", dir).unwrap();
        assert!(added.starts_with(r"C:\Users\me\AppData\Local\concordia\bin;"));
        assert!(added.ends_with(r"C:\Windows;C:\Tools"));
        // Already present, in any case or with a trailing slash: nothing to do.
        assert!(path_with_dir(&added, dir).is_none());
        assert!(path_with_dir(r"C:\X;c:\USERS\ME\appdata\local\CONCORDIA\bin\", dir).is_none());
        // Empty PATH, and unexpanded variables are left alone.
        assert_eq!(path_with_dir("", dir).unwrap(), dir.to_string_lossy());
        let kept = path_with_dir(r"%USERPROFILE%\bin;C:\Tools", dir).unwrap();
        assert!(kept.contains(r"%USERPROFILE%\bin"));
        // A look-alike directory is not a match.
        assert!(path_with_dir(r"C:\Users\me\AppData\Local\concordia\bin2", dir).is_some());
    }

    fn fake_checkout(dir: &Path) {
        let c = dir.join("crates").join("concordia");
        fs::create_dir_all(&c).unwrap();
        fs::write(c.join("Cargo.toml"), "").unwrap();
    }

    #[test]
    fn a_build_in_a_checkout_names_its_source_root() {
        let d = tempfile::tempdir().unwrap();
        fake_checkout(d.path());
        let exe = d
            .path()
            .join("target")
            .join("release")
            .join("concordia.exe");
        assert_eq!(source_root_of_exe(&exe).as_deref(), Some(d.path()));
        let dbg = d.path().join("target").join("debug").join("concordia.exe");
        assert_eq!(source_root_of_exe(&dbg).as_deref(), Some(d.path()));
        // An installed copy, or a build outside a checkout, has no source root.
        assert!(source_root_of_exe(&d.path().join("bin").join("concordia.exe")).is_none());
        let other = tempfile::tempdir().unwrap();
        let loose = other
            .path()
            .join("target")
            .join("release")
            .join("concordia.exe");
        assert!(source_root_of_exe(&loose).is_none());
    }

    #[test]
    fn update_finds_the_recorded_checkout_from_any_directory() {
        let _guard = crate::accounts::TEST_ENV
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let install = tempfile::tempdir().unwrap();
        std::env::set_var("CONCORDIA_INSTALL_DIR", install.path());
        let checkout = tempfile::tempdir().unwrap();
        fake_checkout(checkout.path());
        let elsewhere = tempfile::tempdir().unwrap();

        assert!(
            source_root(elsewhere.path()).is_none(),
            "nothing recorded yet"
        );
        record_source_root(checkout.path());
        assert_eq!(
            source_root(elsewhere.path()).as_deref(),
            Some(checkout.path())
        );

        // A record pointing at something that is no longer a checkout is ignored.
        fs::remove_dir_all(checkout.path().join("crates")).unwrap();
        assert!(source_root(elsewhere.path()).is_none());
        std::env::remove_var("CONCORDIA_INSTALL_DIR");
    }

    #[test]
    fn removing_a_directory_from_path_is_exact_and_keeps_the_rest() {
        let old = Path::new(r"C:\Users\me\AppData\Local\orc\bin");
        let path = r"C:\Windows;C:\USERS\ME\appdata\local\ORC\bin\;%USERPROFILE%\bin;C:\Users\me\AppData\Local\orc\bin2";
        let got = path_without_dir(path, old).unwrap();
        assert_eq!(
            got,
            r"C:\Windows;%USERPROFILE%\bin;C:\Users\me\AppData\Local\orc\bin2"
        );
        // Not present: nothing to do.
        assert!(path_without_dir(r"C:\Windows;C:\Tools", old).is_none());
    }

    #[test]
    fn sweeping_removes_abandoned_swap_files_and_nothing_else() {
        let d = tempfile::tempdir().unwrap();
        let w = |n: &str| fs::write(d.path().join(n), b"x").unwrap();
        w("concordia.exe");
        w("concordia.exe.old-1-2");
        w("concordia.exe.new-fresh");
        w("current");
        w("notes.txt");
        // A staged copy older than the cutoff is abandoned.
        w("concordia.exe.new-ancient");
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(d.path().join("concordia.exe.new-ancient"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        sweep_leftovers(d.path());
        let mut left: Vec<String> = fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "concordia.exe",
                "concordia.exe.new-fresh",
                "current",
                "notes.txt"
            ]
        );
    }

    #[test]
    fn recognizes_concordia_binary_names() {
        assert!(is_concordia_binary_name("concordia.exe"));
        assert!(is_concordia_binary_name("concordia-next.exe"));
        assert!(is_concordia_binary_name("concordia-a.exe"));
        assert!(!is_concordia_binary_name("notepad.exe"));
    }

    #[test]
    fn install_bin_dir_ends_with_concordia_bin() {
        let p = install_bin_dir();
        assert!(
            p.ends_with(Path::new("concordia").join("bin"))
                || p.to_string_lossy().contains("concordia")
        );
    }

    fn ps(script: &str) -> Command {
        let mut c = Command::new("powershell.exe");
        c.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        c
    }

    #[test]
    fn a_child_reports_its_latest_line_as_progress_and_returns_its_status() {
        let ctl = JobControl::default();
        let (ok, code, tail) = run_child_with_cancel(
            ps("Write-Output 'Compiling a'; Write-Output 'Compiling b'; exit 0"),
            &ctl,
        )
        .unwrap();
        assert!(ok && code == 0);
        assert!(tail.contains("Compiling b"), "{tail}");
        assert!(ctl.status().starts_with("Compiling"), "{}", ctl.status());
        let (ok, code, _) = run_child_with_cancel(ps("exit 7"), &ctl).unwrap();
        assert!(!ok && code == 7);
    }

    #[test]
    fn cancelling_stops_a_long_build_promptly() {
        let ctl = JobControl::default();
        let c2 = ctl.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            c2.cancel.store(true, Ordering::SeqCst);
        });
        let start = Instant::now();
        let err = run_child_with_cancel(ps("Start-Sleep -Seconds 60"), &ctl).unwrap_err();
        assert!(err.to_string().contains("cancelled"), "{err}");
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "{:?}",
            start.elapsed()
        );
        t.join().unwrap();
    }

    #[test]
    fn a_job_runs_off_the_calling_thread_reports_progress_and_delivers_one_result() {
        let job = spawn_job(JobKind::Check, |ctl| {
            ctl.say("working");
            while !ctl.cancelled() {
                std::thread::sleep(Duration::from_millis(10));
            }
            JobResult::Check(Ok("stopped".into()))
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while job.status().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            job.poll().is_none(),
            "still running; the caller was not blocked"
        );
        assert_eq!(job.status(), "working");
        job.cancel();
        assert!(job.is_cancelled());
        let start = Instant::now();
        let result = loop {
            if let Some(r) = job.poll() {
                break r;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(matches!(result, JobResult::Check(Ok(m)) if m == "stopped"));
        assert!(job.poll().is_none(), "exactly one result");
    }
}
