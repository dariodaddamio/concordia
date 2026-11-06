//! In-pane self-update: obtain a new binary (build from source, or download a release), verify
//! it runs, stage it into an unlocked install slot, flip the pointer, re-exec.

use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// The one file name `concordia` resolves to on PATH. It is always the newest installed build.
pub const PRIMARY_SLOT: &str = "concordia.exe";

/// Hash bytes, rather than trusting an executable's name, size or reported version.
pub(crate) fn file_sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut bytes = [0_u8; 65536];
    loop {
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub(crate) fn regular_owned_file(path: &Path, root: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    let linked = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        linked || metadata.file_attributes() & 0x400 != 0
    };
    metadata.is_file()
        && !linked
        && fs::canonicalize(path)
            .ok()
            .is_some_and(|path| path.parent() == Some(root))
}

pub(crate) fn resolved_directory(path: &Path) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(path).ok()?;
    let linked = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        linked || metadata.file_attributes() & 0x400 != 0
    };
    (metadata.is_dir() && !linked)
        .then(|| fs::canonicalize(path).ok())
        .flatten()
}

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
    Check(Result<UpdatePreview>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogSection {
    pub kind: String,
    pub stamp: String,
    pub bullets: Vec<String>,
}

/// One line of the `/update` screen; the screen styles and wraps each kind its own way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewLine {
    /// `Installed  0.0.1 · build 2026-10-30a`: a dim label and its value.
    Field(&'static str, String),
    /// `What's new`, `You're on the newest build (...)`.
    Title(String),
    /// A changelog group: `Fixed`, `Changed`.
    Kind(String),
    Bullet(String),
    Text(String),
    Blank,
    /// The keys that act on this screen.
    Hint(String),
}

impl PreviewLine {
    /// The line as plain text, as the screen draws it before wrapping.
    pub fn plain(&self) -> String {
        match self {
            PreviewLine::Field(label, value) => format!("{label:<10} {value}"),
            PreviewLine::Title(t) | PreviewLine::Text(t) | PreviewLine::Hint(t) => t.clone(),
            PreviewLine::Kind(k) => format!(" {k}"),
            PreviewLine::Bullet(b) => format!(" · {b}"),
            PreviewLine::Blank => String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePreview {
    pub lines: Vec<PreviewLine>,
    pub history: Vec<PreviewLine>,
    pub history_expanded: bool,
    pub install_available: bool,
    pub release_url: Option<String>,
}

impl UpdatePreview {
    pub fn visible_lines(&self) -> Vec<PreviewLine> {
        let mut lines = self.lines.clone();
        if self.history_expanded {
            lines.push(PreviewLine::Title("Earlier updates".into()));
            lines.extend(self.history.clone());
        }
        lines
    }

    fn message(text: &str) -> Self {
        UpdatePreview {
            history: Vec::new(),
            history_expanded: false,
            lines: vec![
                PreviewLine::Text(text.into()),
                PreviewLine::Blank,
                PreviewLine::Hint("Esc close".into()),
            ],
            install_available: false,
            release_url: None,
        }
    }

    /// Every line as plain text (tests and the transcript fallback).
    pub fn plain_lines(&self) -> Vec<String> {
        self.lines.iter().map(PreviewLine::plain).collect()
    }
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
    spawn_job(JobKind::Check, move |ctl| {
        JobResult::Check(check_for_update_ctl(&cwd, ctl))
    })
}

/// Run `cmd`, relaying its latest output line as progress, and kill its whole process tree if the
/// job is cancelled. Returns `(success, exit code, last lines of output)`.
pub fn run_child_with_cancel(cmd: Command, ctl: &JobControl) -> Result<(bool, i32, String)> {
    run_child_with_format(cmd, ctl, false)
}

fn run_child_with_format(
    mut cmd: Command,
    ctl: &JobControl,
    cargo_json: bool,
) -> Result<(bool, i32, String)> {
    crate::process_ui::background_std(&mut cmd);
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
                    if cargo_json {
                        match serde_json::from_str::<serde_json::Value>(&line) {
                            Ok(record) => {
                                if let Some(message) = record
                                    .pointer("/message/rendered")
                                    .and_then(|value| value.as_str())
                                {
                                    ctl.say(message);
                                } else if record["reason"] == "build-finished"
                                    && record["success"] == true
                                {
                                    ctl.say("Release build finished");
                                }
                            }
                            Err(_) => ctl.say(&line),
                        }
                    } else {
                        ctl.say(&line);
                    }
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
const CHANNEL_FILE: &str = "channel";

fn install_channel() -> Result<Option<String>> {
    let path = install_bin_dir().join(CHANNEL_FILE);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("cannot inspect the install channel"),
    };
    let linked = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        linked || metadata.file_attributes() & 0x400 != 0
    };
    anyhow::ensure!(
        metadata.is_file() && !linked && metadata.len() <= 16,
        "invalid install channel file; reinstall Concordia to repair it"
    );
    let channel = fs::read_to_string(path).context("cannot read the install channel")?;
    anyhow::ensure!(
        matches!(channel.trim(), "source" | "release"),
        "invalid install channel; reinstall Concordia to repair it"
    );
    Ok(Some(channel.trim().to_owned()))
}

fn record_install_channel(channel: &str) -> Result<()> {
    // Validate an existing owned marker before overwriting it; never follow a link.
    install_channel()?;
    let dir = install_bin_dir();
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(CHANNEL_FILE), format!("{channel}\n"))?;
    Ok(())
}

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
        let _ = record_install_channel("source");
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
    let channel = install_channel().ok()?;
    if channel.as_deref() == Some("release") {
        return None;
    }
    crate::host::wezterm::find_project_root(cwd)
        .filter(|root| is_source_root(root))
        .or_else(|| find_cargo_workspace(cwd).filter(|root| is_source_root(root)))
        .or_else(recorded_source_root)
}

/// `/update`: rebuild from source when run inside a checkout, otherwise download the newest
/// published release (when a release feed is configured).
pub fn update_auto(cwd: &Path) -> Result<UpdateOutcome> {
    update_auto_ctl(cwd, &JobControl::default())
}

pub fn update_auto_ctl(cwd: &Path, ctl: &JobControl) -> Result<UpdateOutcome> {
    install_channel()?;
    if source_root(cwd).is_some() {
        return perform_update_ctl(cwd, ctl).map(UpdateOutcome::Installed);
    }
    let current = crate::version::VERSION;
    let cfg = crate::release::config_from_env(current).context(
        "invalid release repository override; CONCORDIA_UPDATE_REPO must be owner/name (unset it to use the official feed)",
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
            ctl.say(&format!("Preparing {} update", rel.tag));
            crate::release::install_release(&cfg, &rel, current, &fetch)
                .map(UpdateOutcome::Installed)
        }
    }
}

fn git_text(root: &Path, args: &[&str], ctl: &JobControl) -> Result<String> {
    let mut cmd = crate::process_ui::command("git");
    cmd.arg("-C").arg(root).args(args);
    let (ok, _, output) = run_child_with_cancel(cmd, ctl)?;
    if !ok {
        bail!("git {} failed", args.join(" "));
    }
    Ok(output.trim().to_string())
}

fn source_version(root: &Path) -> Result<(String, String)> {
    let cargo = fs::read_to_string(root.join("crates/concordia/Cargo.toml"))?;
    let version = cargo
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("version = \"")?
                .split_once('"')
                .map(|x| x.0.to_string())
        })
        .context("source Cargo.toml has no package version")?;
    let source = fs::read_to_string(root.join("crates/concordia/src/version.rs"))?;
    let stamp = source
        .lines()
        .find_map(|line| {
            line.split_once("BUILD_STAMP: &str = \"")?
                .1
                .split_once('"')
                .map(|x| x.0.to_string())
        })
        .context("source version.rs has no BUILD_STAMP")?;
    Ok((version, stamp))
}

pub fn changelog_sections(text: &str) -> Vec<ChangelogSection> {
    let mut out = Vec::new();
    let mut current: Option<ChangelogSection> = None;
    let mut bullet: Option<usize> = None;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("### ") {
            out.extend(current.take());
            bullet = None;
            // Only sections stamped with a build (`### Fixed (`2026-10-30a`)`) shipped in one.
            let Some((kind, raw)) = rest.split_once("(`") else {
                continue;
            };
            let Some(stamp) = raw.strip_suffix("`)") else {
                continue;
            };
            current = Some(ChangelogSection {
                kind: kind.trim().to_string(),
                stamp: stamp.to_string(),
                bullets: Vec::new(),
            });
        } else if line.starts_with("## ") {
            out.extend(current.take());
            bullet = None;
        } else if let Some(text) = line.strip_prefix("- ") {
            if let Some(section) = current.as_mut() {
                section.bullets.push(crate::release::plain_markdown(text));
                bullet = Some(section.bullets.len() - 1);
            }
        } else if line.starts_with("  ") && !line.trim().is_empty() {
            if let (Some(section), Some(index)) = (current.as_mut(), bullet) {
                section.bullets[index].push(' ');
                section.bullets[index].push_str(&crate::release::plain_markdown(line.trim()));
            }
        }
    }
    out.extend(current);
    out
}

/// The sections that `pick` keeps, as kind headings each followed by their bullets once.
fn grouped(sections: &[ChangelogSection], pick: impl Fn(&str) -> bool) -> Vec<PreviewLine> {
    let mut lines = Vec::new();
    for kind in ["Added", "Changed", "Fixed", "Removed", "Security"] {
        let mut bullets: Vec<&String> = Vec::new();
        for section in sections.iter().filter(|s| s.kind == kind && pick(&s.stamp)) {
            for b in &section.bullets {
                if !bullets.contains(&b) {
                    bullets.push(b);
                }
            }
        }
        if !bullets.is_empty() {
            lines.push(PreviewLine::Kind(kind.into()));
            lines.extend(bullets.into_iter().map(|b| PreviewLine::Bullet(b.clone())));
        }
    }
    lines
}

fn source_preview(root: &Path, ctl: &JobControl) -> Result<UpdatePreview> {
    source_preview_for(
        root,
        crate::version::VERSION,
        crate::version::BUILD_STAMP,
        ctl,
    )
}

fn source_preview_for(
    root: &Path,
    installed_version: &str,
    installed: &str,
    ctl: &JobControl,
) -> Result<UpdatePreview> {
    let (version, stamp) = source_version(root)?;
    let branch = git_text(root, &["rev-parse", "--abbrev-ref", "HEAD"], ctl)?;
    let status = git_text(root, &["status", "--porcelain"], ctl)?;
    let state = match status.lines().count() {
        0 => "clean".to_string(),
        1 => "1 uncommitted file".to_string(),
        n => format!("{n} uncommitted files"),
    };
    let sections = fs::read_to_string(root.join("CHANGELOG.md"))
        .map(|s| changelog_sections(&s))
        .unwrap_or_default();
    let from = PreviewLine::Field("From", format!("source checkout · {branch} · {state}"));
    let mut lines = Vec::new();
    // A checkout behind the running build is shown but never installed by a keypress.
    let older = stamp.as_str() < installed;
    let install_available = stamp.as_str() > installed;
    if install_available || older {
        let note = if older {
            " · older than this build"
        } else {
            ""
        };
        lines.push(PreviewLine::Field(
            "Installed",
            format!("{installed_version} · build {installed}"),
        ));
        lines.push(PreviewLine::Field(
            "Available",
            format!("{version} · build {stamp}{note}"),
        ));
        lines.push(from.clone());
        lines.push(PreviewLine::Blank);
    }
    if older {
        lines.push(PreviewLine::Text(
            "The checkout is behind this build. Pull or switch branch, then run /update again."
                .into(),
        ));
        lines.push(PreviewLine::Blank);
    } else if install_available {
        lines.push(PreviewLine::Text(INSTALL_BUILD.into()));
        lines.push(PreviewLine::Blank);
        let news = grouped(&sections, |s| s > installed && s <= stamp.as_str());
        if !news.is_empty() {
            lines.push(PreviewLine::Title("What's new".into()));
            lines.extend(news);
            lines.push(PreviewLine::Blank);
        }
    } else {
        lines.push(PreviewLine::Title(format!(
            "You're on the newest build ({installed})."
        )));
        lines.push(from);
        lines.push(PreviewLine::Blank);
        let current = grouped(&sections, |s| s == installed);
        if !current.is_empty() {
            lines.push(PreviewLine::Title("What's in this build".into()));
            lines.extend(current);
            lines.push(PreviewLine::Blank);
        }
    }
    let history = builds(&sections, |s| {
        if install_available {
            s <= installed
        } else {
            s < installed
        }
    });
    lines.push(PreviewLine::Hint(
        if history.is_empty() {
            "Esc close"
        } else {
            "H history · Esc close"
        }
        .into(),
    ));
    Ok(UpdatePreview {
        history,
        history_expanded: false,
        lines,
        install_available,
        release_url: None,
    })
}

/// Every build that `include` keeps, newest first, each with its changes: the whole changelog
/// on this screen, formatted like the newest entries.
fn builds(sections: &[ChangelogSection], include: impl Fn(&str) -> bool) -> Vec<PreviewLine> {
    let mut stamps: Vec<&str> = sections
        .iter()
        .map(|s| s.stamp.as_str())
        .filter(|s| include(s))
        .collect();
    stamps.sort_unstable_by(|a, b| b.cmp(a));
    stamps.dedup();
    let mut lines = Vec::new();
    for stamp in stamps {
        lines.push(PreviewLine::Title(format!("Build {stamp}")));
        lines.extend(grouped(sections, |s| s == stamp));
        lines.push(PreviewLine::Blank);
    }
    lines
}

/// What Enter does, said once at the top of the screen so a long changelog cannot push it out
/// of view.
const INSTALL_BUILD: &str = "Press Enter to install it. Concordia builds it (about a minute), then \
     restarts on the new build by itself. There is nothing else to do. Esc closes without updating.";
const INSTALL_RELEASE: &str = "Press Enter to install it. Concordia downloads and checks it, then \
     restarts on the new release by itself. There is nothing else to do. Esc closes without updating.";

/// `/update`: what an update would install, without installing anything.
pub fn check_for_update(cwd: &Path) -> Result<UpdatePreview> {
    check_for_update_ctl(cwd, &JobControl::default())
}

fn check_for_update_ctl(cwd: &Path, ctl: &JobControl) -> Result<UpdatePreview> {
    cleanup_obsolete_installs();
    install_channel()?;
    if let Some(root) = source_root(cwd) {
        return source_preview(&root, ctl);
    }
    let current = crate::version::VERSION;
    let Some(cfg) = crate::release::config_from_env(current) else {
        return Ok(UpdatePreview::message(
            "invalid release repository override; unset CONCORDIA_UPDATE_REPO to use the official feed",
        ));
    };
    let from = PreviewLine::Field(
        "From",
        format!("github.com/{} · checksum verified before install", cfg.repo),
    );
    Ok(
        match crate::release::check(&cfg, current, &crate::release::PowerShellFetcher)? {
            crate::release::Check::UpToDate { current, .. } => UpdatePreview {
                history: Vec::new(),
                history_expanded: false,
                lines: vec![
                    PreviewLine::Title(format!("You're on the newest release ({current}).")),
                    from,
                    PreviewLine::Blank,
                    PreviewLine::Hint("Esc close".into()),
                ],
                install_available: false,
                release_url: None,
            },
            crate::release::Check::Available(rel) => {
                let mut lines = vec![
                    PreviewLine::Field("Installed", current.to_string()),
                    PreviewLine::Field(
                        "Available",
                        format!(
                            "{} · released {}",
                            rel.version,
                            crate::release::published_date(&rel.published_at)
                        ),
                    ),
                    from,
                    PreviewLine::Blank,
                    PreviewLine::Text(INSTALL_RELEASE.into()),
                    PreviewLine::Blank,
                ];
                let notes = crate::release::clean_notes(&rel.notes);
                if !notes.is_empty() {
                    lines.push(PreviewLine::Title("What's new".into()));
                    for line in notes.lines() {
                        lines.push(match line.trim_start().strip_prefix(['-', '*']) {
                            Some(b) => PreviewLine::Bullet(b.trim().to_string()),
                            None if line.trim().is_empty() => PreviewLine::Blank,
                            None => PreviewLine::Text(line.trim().to_string()),
                        });
                    }
                    lines.push(PreviewLine::Blank);
                }
                lines.push(PreviewLine::Hint("o open release page · Esc close".into()));
                UpdatePreview {
                    history: Vec::new(),
                    history_expanded: false,
                    lines,
                    install_available: true,
                    release_url: Some(rel.html_url),
                }
            }
        },
    )
}

/// Build `concordia` release and copy into a slot that is not the currently running exe.
pub fn perform_update(cwd: &Path) -> Result<UpdateResult> {
    perform_update_ctl(cwd, &JobControl::default())
}

pub fn perform_update_ctl(cwd: &Path, ctl: &JobControl) -> Result<UpdateResult> {
    install_channel()?;
    let root = source_root(cwd).context(
        "no source checkout found: run /update inside the repository once, or run `concordia install` from a build in it",
    )?;

    let expected_source = source_version(&root)?;
    let cargo = which::which("cargo").context("cargo not on PATH")?;
    // Prefer default target/; if this process is that release exe (locked), build aside.
    let default_built = root.join("target").join("release").join("concordia.exe");
    let aside_dir = root.join("target").join("concordia-update");
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
    anyhow::ensure!(
        built.is_file(),
        "Cargo-reported executable is missing: {}",
        built.display()
    );
    anyhow::ensure!(
        source_version(&root)? == expected_source,
        "source build identity changed during update; retry the build"
    );
    let expected_version = format!("concordia {} ({})", expected_source.0, expected_source.1);
    anyhow::ensure!(
        verify_binary(&built)? == expected_version,
        "rebuilt executable does not match the selected source version; existing install kept"
    );

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
    let expected_version = verify_binary(built)?;
    let built_len = fs::metadata(built)?.len();

    let bin_dir = install_bin_dir();
    fs::create_dir_all(&bin_dir).with_context(|| format!("mkdir {}", bin_dir.display()))?;
    let dest = bin_dir.join(PRIMARY_SLOT);
    let digest = file_sha256(built)?;
    let same_bytes = resolved_directory(&bin_dir).is_some_and(|root| {
        regular_owned_file(&dest, &root)
            && fs::metadata(&dest)
                .ok()
                .is_some_and(|m| m.len() == built_len)
            && file_sha256(&dest).ok().as_deref() == Some(digest.as_str())
    });
    if !same_bytes {
        swap_in(built, &dest, built_len, &expected_version, &digest)?;
    }
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

/// Rename may briefly fail while Windows AV or a just-exited verifier releases its image.
/// Retry only the documented sharing/lock errors; all other failures remain immediate.
fn rename_with_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error)
                if cfg!(windows)
                    && matches!(error.raw_os_error(), Some(32 | 33))
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
}

/// Replace `dest` with a copy of `built`, tolerating `dest` being a running executable.
fn swap_in(
    built: &Path,
    dest: &Path,
    built_len: u64,
    expected_version: &str,
    expected_hash: &str,
) -> Result<()> {
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

    if !verify_binary(&staged).is_ok_and(|version| version == expected_version)
        || file_sha256(&staged).ok().as_deref() != Some(expected_hash)
    {
        let _ = fs::remove_file(&staged);
        bail!("staged executable changed version or failed verification; existing install kept");
    }
    let had_old = dest.exists();
    if had_old {
        if let Err(e) = rename_with_retry(dest, &aside) {
            let _ = fs::remove_file(&staged);
            bail!("cannot move the old {} aside: {e}", dest.display());
        }
    }
    if let Err(e) = rename_with_retry(&staged, dest) {
        // Put the old one back so the install is never left without an concordia.exe.
        if had_old {
            let _ = rename_with_retry(&aside, dest);
        }
        let _ = fs::remove_file(&staged);
        bail!("cannot install {}: {e}", dest.display());
    }
    if had_old {
        let _ = record_swap_artifact(&aside);
    }
    Ok(())
}

fn record_swap_artifact(path: &Path) -> Result<()> {
    use std::io::Write;
    let record = serde_json::json!({"owner":"concordia-install-artifact-v1", "sha256":file_sha256(path)?, "bytes":fs::metadata(path)?.len()});
    let receipt = path.with_file_name(format!(
        "{}.receipt.json",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(receipt)?;
    output.write_all(serde_json::to_string(&record)?.as_bytes())?;
    output.sync_all()?;
    Ok(())
}

/// Clean only receipt-owned swap files, retaining the latest rollback and live Windows images.
/// Unknown files, links and in-progress copies are never removed by a name prefix alone.
fn sweep_leftovers(dir: &Path) {
    let Some(root) = resolved_directory(dir) else {
        return;
    };
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    let current = env::current_exe()
        .ok()
        .and_then(|path| fs::canonicalize(path).ok());
    let active = fs::read_to_string(root.join("current"))
        .ok()
        .and_then(|path| fs::canonicalize(path.trim()).ok());
    let mut artifacts = entries
        .take(64)
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !regular_owned_file(&path, &root) {
                return None;
            }
            let name = entry.file_name();
            let (pid, nanos) = name
                .to_str()?
                .strip_prefix("concordia.exe.old-")?
                .split_once('-')?;
            if pid.parse::<u32>().is_err() {
                return None;
            }
            let order = nanos.parse::<u128>().ok()?;
            let receipt = path.with_file_name(format!("{}.receipt.json", name.to_string_lossy()));
            if !regular_owned_file(&receipt, &root) || fs::metadata(&receipt).ok()?.len() > 4096 {
                return None;
            }
            let record: serde_json::Value =
                serde_json::from_slice(&fs::read(&receipt).ok()?).ok()?;
            if record["owner"] != "concordia-install-artifact-v1"
                || record["bytes"].as_u64()? != entry.metadata().ok()?.len()
            {
                return None;
            }
            Some((order, path, receipt, record["sha256"].as_str()?.to_owned()))
        })
        .collect::<Vec<_>>();
    artifacts.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, path, receipt, hash) in artifacts.into_iter().skip(1).take(4) {
        if current.as_ref() == Some(&path)
            || active.as_ref() == Some(&path)
            || file_sha256(&path).ok().as_deref() != Some(hash.as_str())
        {
            continue;
        }
        if fs::remove_file(&path).is_ok() {
            let _ = fs::remove_file(receipt);
        }
    }
}

/// May also be called after a later startup, when older Windows images have exited.
pub fn cleanup_obsolete_installs() {
    #[cfg(not(test))]
    sweep_leftovers(&install_bin_dir());
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
    install_channel()?;
    let me = env::current_exe().context("cannot locate the running executable")?;
    let installed = install_binary(&me)?;
    if let Some(root) = source_root_of_exe(&me) {
        record_source_root(&root);
    } else {
        record_install_channel("release")?;
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

/// The per-user Start menu (or Desktop when explicitly requested) shortcut owned by Concordia.
pub fn shortcut_path(desktop: bool) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let root = if desktop {
            dirs::desktop_dir().context("cannot locate the Desktop")?
        } else {
            dirs::data_dir()
                .context("cannot locate the per-user application data directory")?
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
        };
        Ok(root.join("Concordia.lnk"))
    }
    #[cfg(not(windows))]
    {
        let _ = desktop;
        bail!("shortcuts are supported on Windows only")
    }
}

/// Create a native Windows shell shortcut without launching a visible helper console.
pub fn install_shortcut(desktop: bool) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let path = shortcut_path(desktop)?;
        let target = env::current_exe().context("cannot locate the running executable")?;
        let parent = path.parent().context("shortcut has no parent")?;
        fs::create_dir_all(parent)?;
        let script = "$s=(New-Object -ComObject WScript.Shell).CreateShortcut($env:CONCORDIA_SHORTCUT);$s.TargetPath=$env:CONCORDIA_TARGET;$s.WorkingDirectory=$env:CONCORDIA_WORKDIR;$s.IconLocation=$env:CONCORDIA_ICON;$s.Save()";
        let status = crate::process_ui::command("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                script,
            ])
            .env("CONCORDIA_SHORTCUT", &path)
            .env("CONCORDIA_TARGET", &target)
            .env("CONCORDIA_WORKDIR", parent)
            .env("CONCORDIA_ICON", &target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .context("create Concordia shortcut")?;
        anyhow::ensure!(
            status.success(),
            "PowerShell could not create the Concordia shortcut"
        );
        Ok(path)
    }
    #[cfg(not(windows))]
    {
        let _ = desktop;
        bail!("shortcuts are supported on Windows only")
    }
}

/// Remove only the exact shortcut path Concordia owns.
pub fn remove_shortcut(desktop: bool) -> Result<PathBuf> {
    let path = shortcut_path(desktop)?;
    if path.is_file() {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(path)
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
    let mut cmd = crate::process_ui::command("powershell.exe");
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
    reexec_into_mode(new_exe, cwd, false)
}

pub fn reexec_into_mode(new_exe: &Path, cwd: &Path, terminal: bool) -> Result<i32> {
    persist_concordia_exe_env(new_exe);
    let mut command = Command::new(new_exe); // visible-spawn: replacement owns the interactive console.
    if terminal {
        command.arg("--terminal");
    }
    let status = command
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
    let mut cmd = crate::process_ui::command(cargo);
    cmd.args([
        "build",
        "-p",
        "concordia",
        "--release",
        "--bin",
        "concordia",
        "--message-format=json",
    ])
    .current_dir(root);
    if let Some(dir) = target_dir {
        cmd.arg("--target-dir").arg(dir);
    }
    let (ok, code, log_tail) = run_child_with_format(cmd, ctl, true)?;
    if !ok {
        bail!("cargo build failed ({code})\n{log_tail}");
    }
    let built = cargo_executable(root, &log_tail)?;
    let display = log_tail
        .lines()
        .filter_map(
            |line| match serde_json::from_str::<serde_json::Value>(line) {
                Ok(record) => record
                    .pointer("/message/rendered")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned),
                Err(_) => Some(line.to_owned()),
            },
        )
        .collect::<Vec<_>>()
        .join("\n");
    Ok((built, display))
}

fn cargo_executable(root: &Path, log: &str) -> Result<PathBuf> {
    let manifest = fs::canonicalize(root.join("crates/concordia/Cargo.toml"))?;
    log.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|record| {
            record["reason"] == "compiler-artifact"
                && record["target"]["name"] == "concordia"
                && record["target"]["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
                && record["profile"]["test"] == false
                && record["manifest_path"]
                    .as_str()
                    .and_then(|path| fs::canonicalize(path).ok())
                    .as_ref()
                    == Some(&manifest)
        })
        .filter_map(|record| record["executable"].as_str().map(PathBuf::from))
        .rfind(|path| path.is_absolute())
        .context("Cargo did not report the built Concordia executable; existing install kept")
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
    let _ = crate::process_ui::command("setx")
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
        let configured = other.path().join("custom-output/release/concordia.exe");
        let artifact = serde_json::json!({"reason":"compiler-artifact", "target":{"name":"concordia","kind":["bin"]},
            "profile":{"test":false},"manifest_path":d.path().join("crates/concordia/Cargo.toml"),"executable":configured});
        assert_eq!(
            cargo_executable(d.path(), &artifact.to_string()).unwrap(),
            configured
        );
        assert!(cargo_executable(d.path(), "Finished release build").is_err());
        let mut wrong = artifact.clone();
        wrong["target"]["kind"] = serde_json::json!(["lib"]);
        assert!(cargo_executable(d.path(), &wrong.to_string()).is_err());
        wrong = artifact.clone();
        wrong["manifest_path"] = serde_json::json!(other.path().join("Cargo.toml"));
        assert!(cargo_executable(d.path(), &wrong.to_string()).is_err());
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
        assert_eq!(install_channel().unwrap().as_deref(), Some("source"));
        assert_eq!(
            source_root(elsewhere.path()).as_deref(),
            Some(checkout.path())
        );

        // Other projects must not steal an installed application's source update.
        fs::write(
            elsewhere.path().join("Cargo.toml"),
            "[workspace]\nmembers=[]\n",
        )
        .unwrap();
        assert_eq!(
            source_root(elsewhere.path()).as_deref(),
            Some(checkout.path())
        );
        let terminal = elsewhere.path().join("experiments/terminal/wezterm");
        fs::create_dir_all(&terminal).unwrap();
        fs::write(terminal.join("wezterm.lua"), "-- unrelated fixture\n").unwrap();
        assert_eq!(
            source_root(elsewhere.path()).as_deref(),
            Some(checkout.path())
        );

        record_install_channel("release").unwrap();
        assert!(
            source_root(checkout.path()).is_none(),
            "release installs never require a checkout"
        );
        fs::write(install.path().join(CHANNEL_FILE), "unknown").unwrap();
        assert!(install_channel().is_err());
        assert!(update_auto(elsewhere.path())
            .unwrap_err()
            .to_string()
            .contains("invalid install channel"));
        fs::write(install.path().join(CHANNEL_FILE), "source\n").unwrap();

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
        // Names and age alone do not establish ownership of a staged/rollback executable.
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
                "concordia.exe.new-ancient",
                "concordia.exe.new-fresh",
                "concordia.exe.old-1-2",
                "current",
                "notes.txt"
            ]
        );

        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let from = d.path().join("held-source.bin");
            let to = d.path().join("held-destination.bin");
            fs::write(&from, b"retry payload").unwrap();
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let held = std::thread::spawn({
                let from = from.clone();
                move || {
                    let _file = fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .share_mode(3)
                        .open(from)
                        .unwrap();
                    ready_tx.send(()).unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            });
            ready_rx.recv().unwrap();
            let first = fs::rename(&from, &to).unwrap_err();
            assert!(matches!(first.raw_os_error(), Some(32 | 33)), "{first}");
            rename_with_retry(&from, &to).unwrap();
            held.join().unwrap();
            assert_eq!(fs::read(&to).unwrap(), b"retry payload");
        }
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
        let mut c = crate::process_ui::command("powershell.exe");
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
            JobResult::Check(Ok(UpdatePreview {
                history: Vec::new(),
                history_expanded: false,
                lines: vec![PreviewLine::Text("stopped".into())],
                install_available: false,
                release_url: None,
            }))
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
        assert!(matches!(result, JobResult::Check(Ok(m)) if m.plain_lines() == ["stopped"]));
        assert!(job.poll().is_none(), "exactly one result");
    }

    #[test]
    fn changelog_sections_group_only_stamped_builds_and_join_continuations() {
        let sections = changelog_sections("### Fixed (`2026-10-31a`)\n- first `code`\n  continued\n### Added\n- ignored\n### Changed (`2026-10-30b`)\n- second\n");
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].bullets, ["first code continued"]);
        assert_eq!(sections[1].stamp, "2026-10-30b");
        assert!(!sections.iter().any(|s| s.kind == "Added"));
    }

    #[test]
    fn source_checkout_identity_is_read_from_text_files() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let unknown = bin.join("concordia.exe.old-1-0");
        fs::write(&unknown, b"unowned").unwrap();
        for order in 1..=4 {
            let old = bin.join(format!("concordia.exe.old-1-{order}"));
            fs::write(&old, b"owned fixture bytes").unwrap();
            record_swap_artifact(&old).unwrap();
        }
        let active = bin.join("concordia.exe.old-1-1");
        fs::write(bin.join("current"), active.to_string_lossy().as_bytes()).unwrap();
        sweep_leftovers(&bin);
        assert!(unknown.is_file());
        assert!(active.is_file(), "current pointer target is retained");
        assert!(
            bin.join("concordia.exe.old-1-4").is_file(),
            "newest rollback is retained"
        );
        assert!(!bin.join("concordia.exe.old-1-2").exists());
        assert!(!bin.join("concordia.exe.old-1-3.receipt.json").exists());
        let crate_dir = dir.path().join("crates/concordia/src");
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(
            dir.path().join("crates/concordia/Cargo.toml"),
            "[package]\nversion = \"1.2.3\"\n",
        )
        .unwrap();
        fs::write(
            crate_dir.join("version.rs"),
            "pub const BUILD_STAMP: &str = \"2026-11-06a\";\n",
        )
        .unwrap();
        assert_eq!(
            source_version(dir.path()).unwrap(),
            ("1.2.3".into(), "2026-11-06a".into())
        );
    }

    /// A git checkout at build `stamp` with a changelog spanning two builds.
    fn checkout_at_build(stamp: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("crates/concordia/src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            dir.path().join("crates/concordia/Cargo.toml"),
            "[package]\nversion = \"0.0.1\"\n",
        )
        .unwrap();
        fs::write(
            src.join("version.rs"),
            format!("pub const BUILD_STAMP: &str = \"{stamp}\";\n"),
        )
        .unwrap();
        fs::write(
            dir.path().join("CHANGELOG.md"),
            "## [Unreleased]\n\n### Fixed (`2026-10-30b`)\n- Text before a tool shows once.\n\n\
             ### Changed (`2026-10-30b`)\n- The list has 12 `commands`,\n  each opening one screen.\n\n\
             ### Added (next build)\n- Not built yet.\n\n### Fixed (`2026-10-30a`)\n- Cursor tool names.\n",
        )
        .unwrap();
        let git = |args: &[&str]| {
            let ok = crate::process_ui::command("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "c",
        ]);
        dir
    }

    #[test]
    fn a_newer_checkout_lists_only_the_builds_after_the_installed_one() {
        let dir = checkout_at_build("2026-10-30b");
        let preview =
            source_preview_for(dir.path(), "0.0.1", "2026-10-30a", &JobControl::default()).unwrap();
        assert!(preview.install_available);
        assert_eq!(
            preview.plain_lines(),
            [
                "Installed  0.0.1 · build 2026-10-30a",
                "Available  0.0.1 · build 2026-10-30b",
                "From       source checkout · main · clean",
                "",
                INSTALL_BUILD,
                "",
                "What's new",
                " Changed",
                " · The list has 12 commands, each opening one screen.",
                " Fixed",
                " · Text before a tool shows once.",
                "",
                "H history · Esc close",
            ]
        );

        fs::write(dir.path().join("scratch.txt"), "x").unwrap();
        let dirty =
            source_preview_for(dir.path(), "0.0.1", "2026-10-30a", &JobControl::default()).unwrap();
        assert_eq!(
            dirty.plain_lines()[2],
            "From       source checkout · main · 1 uncommitted file"
        );
    }

    #[test]
    fn the_installed_build_keeps_earlier_history_out_of_the_default_page() {
        let dir = checkout_at_build("2026-10-30b");
        let preview =
            source_preview_for(dir.path(), "0.0.1", "2026-10-30b", &JobControl::default()).unwrap();
        assert!(!preview.install_available);
        assert_eq!(
            preview.plain_lines(),
            [
                "You're on the newest build (2026-10-30b).",
                "From       source checkout · main · clean",
                "",
                "What's in this build",
                " Changed",
                " · The list has 12 commands, each opening one screen.",
                " Fixed",
                " · Text before a tool shows once.",
                "",
                "H history · Esc close",
            ]
        );
    }

    #[test]
    fn an_older_checkout_says_so() {
        let dir = checkout_at_build("2026-10-30a");
        let preview =
            source_preview_for(dir.path(), "0.0.1", "2026-10-30b", &JobControl::default()).unwrap();
        assert!(
            !preview.install_available,
            "a downgrade is never one keypress away"
        );
        assert_eq!(
            preview.plain_lines(),
            [
                "Installed  0.0.1 · build 2026-10-30b",
                "Available  0.0.1 · build 2026-10-30a · older than this build",
                "From       source checkout · main · clean",
                "",
                "The checkout is behind this build. Pull or switch branch, then run /update again.",
                "",
                "H history \u{b7} Esc close",
            ]
        );
    }
}
