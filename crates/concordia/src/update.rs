use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

pub const PRIMARY_SLOT: &str = "concordia.exe";

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

pub(crate) use crate::fs_safe::{regular_owned_file, resolved_directory};

#[derive(Debug, Clone)]
pub struct UpdateResult {
    pub install_path: PathBuf,
    pub built_from: PathBuf,
    pub log_tail: String,
    pub summary: String,
}

pub fn is_concordia_binary_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "concordia"
        || lower == "concordia.exe"
        || lower.starts_with("concordia-") && lower.ends_with(".exe")
}

pub fn install_bin_dir() -> PathBuf {
    if let Ok(dir) = env::var("CONCORDIA_INSTALL_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    default_install_bin_dir()
}

fn default_install_bin_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("concordia")
        .join("bin")
}

pub fn current_pointer_path() -> PathBuf {
    install_bin_dir().join("current")
}

pub use crate::fs_safe::resolve_active_exe;

pub fn write_current_pointer(path: &Path) -> std::io::Result<()> {
    let bin = install_bin_dir();
    fs::create_dir_all(&bin)?;
    crate::state_io::write_atomic(&current_pointer_path(), path.to_string_lossy().as_bytes())
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStage {
    Preparing,
    Building,
    Downloading,
    Verifying,
    Installing,
    Finishing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobDiagnostic {
    pub severity: String,
    pub message: String,
    pub code: Option<String>,
    pub source: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JobSnapshot {
    pub stage: Option<UpdateStage>,
    pub download: bool,
    pub status: String,
    pub detail: String,
    pub diagnostics: Vec<JobDiagnostic>,
    pub warning_count: usize,
    pub error_count: usize,
}

#[derive(Clone, Default)]
pub struct JobControl {
    cancel: Arc<AtomicBool>,
    snapshot: Arc<Mutex<JobSnapshot>>,
}

impl JobControl {
    pub(crate) fn cancel_flag(&self) -> &AtomicBool {
        &self.cancel
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn say(&self, line: &str) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.status = bounded_redacted(line.trim(), 512);
            snapshot.detail.clear();
        }
    }

    pub fn stage(&self, stage: UpdateStage, status: &str, detail: impl AsRef<str>) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.stage = Some(stage);
            if stage == UpdateStage::Downloading {
                snapshot.download = true;
            }
            if stage == UpdateStage::Building {
                snapshot.download = false;
            }
            snapshot.status = bounded_redacted(status, 512);
            snapshot.detail = bounded_redacted(detail.as_ref(), 4096);
        }
    }

    fn progress(&self, status: &str, detail: &str) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.status = bounded_redacted(status, 512);
            snapshot.detail = bounded_redacted(detail, 4096);
        }
    }

    fn diagnostic(&self, record: &serde_json::Value) {
        let Some(message) = record.pointer("/message") else {
            return;
        };
        let severity = message["level"].as_str().unwrap_or("warning").to_owned();
        let rendered = message["rendered"].as_str().unwrap_or_default();
        let concise = message["message"].as_str().unwrap_or(rendered).trim();
        let code = message
            .pointer("/code/code")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let source = message["spans"]
            .as_array()
            .and_then(|spans| {
                spans
                    .iter()
                    .find(|span| span["is_primary"] == true)
                    .or_else(|| spans.first())
            })
            .and_then(|span| span["file_name"].as_str())
            .map(str::to_owned);
        let diagnostic = JobDiagnostic {
            severity: bounded_redacted(&severity, 32),
            message: bounded_redacted(concise, 512),
            code: code.map(|value| bounded_redacted(&value, 64)),
            source: source.map(|value| bounded_redacted(&value, 512)),
            detail: bounded_redacted(rendered, 4096),
        };
        if !matches!(diagnostic.severity.as_str(), "warning" | "error") {
            return;
        }
        if let Ok(mut snapshot) = self.snapshot.lock() {
            match diagnostic.severity.as_str() {
                "error" => snapshot.error_count = snapshot.error_count.saturating_add(1).min(9999),
                _ => snapshot.warning_count = snapshot.warning_count.saturating_add(1).min(9999),
            }
            if snapshot.diagnostics.len() < 32 {
                snapshot.diagnostics.push(diagnostic);
            }
        }
    }

    pub fn status(&self) -> String {
        self.snapshot
            .lock()
            .map(|s| s.status.clone())
            .unwrap_or_default()
    }

    pub fn snapshot(&self) -> JobSnapshot {
        self.snapshot.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn bounded_redacted(value: &str, limit: usize) -> String {
    let safe = crate::redact::redact(value);
    safe.chars().take(limit).collect()
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewLine {
    Field(&'static str, String),
    Title(String),
    Kind(String),
    Bullet(String),
    Text(String),
    Blank,
    Hint(String),
}

impl PreviewLine {
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
            lines: vec![PreviewLine::Text(text.into())],
            install_available: false,
            release_url: None,
        }
    }

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
    pub fn poll(&self) -> Option<JobResult> {
        self.rx.try_recv().ok()
    }

    pub fn cancel(&self) {
        self.ctl.cancel.store(true, Ordering::SeqCst);
    }

    pub fn status(&self) -> String {
        self.ctl.status()
    }

    pub fn snapshot(&self) -> JobSnapshot {
        self.ctl.snapshot()
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
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().context("spawn build")?;
    if cargo_json {
        lower_process_priority(&child)?;
    }
    let mut tree = match crate::process_lifetime::own_std_process(&child) {
        Ok(tree) => Some(tree),
        Err(error) => {
            let _ = child.kill();
            bail!("cannot own build process: {error}");
        }
    };
    let read_failed = Arc::new(AtomicBool::new(false));
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
        let (tail, ctl, read_failed) = (Arc::clone(&tail), ctl.clone(), Arc::clone(&read_failed));
        readers.push(std::thread::spawn(move || {
            let mut reader = BufReader::new(stream);
            loop {
                let mut bytes = Vec::new();
                match reader
                    .by_ref()
                    .take(1024 * 1024 + 1)
                    .read_until(b'\n', &mut bytes)
                {
                    Ok(0) => break,
                    Ok(_) if bytes.len() <= 1024 * 1024 => {}
                    _ => {
                        read_failed.store(true, Ordering::Relaxed);
                        break;
                    }
                }
                let line = String::from_utf8_lossy(&bytes).trim_end().to_owned();
                if !line.trim().is_empty() {
                    if cargo_json {
                        match serde_json::from_str::<serde_json::Value>(&line) {
                            Ok(record) => match record["reason"].as_str() {
                                Some("compiler-message") => ctl.diagnostic(&record),
                                Some("compiler-artifact") => {
                                    let target = record
                                        .pointer("/target/name")
                                        .and_then(|value| value.as_str())
                                        .unwrap_or("target");
                                    let fresh = record["fresh"].as_bool().unwrap_or(false);
                                    ctl.progress(
                                        &if fresh {
                                            format!("Ready: {target}")
                                        } else {
                                            format!("Built: {target}")
                                        },
                                        "Preparing the verified build",
                                    );
                                }
                                Some("build-finished") if record["success"] == true => {
                                    ctl.progress("Build ready", "Preparing the verified build");
                                }
                                _ => {}
                            },
                            Err(_) => {}
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
        if ctl.cancelled() || read_failed.load(Ordering::Relaxed) {
            drop(tree.take());
            let _ = child.kill();
            finish(readers);
            if read_failed.load(Ordering::Relaxed) {
                bail!("build output could not be read or exceeded the 1 MiB line limit");
            }
            bail!("cancelled");
        }
        match child.try_wait().context("wait for build")? {
            Some(status) => {
                drop(tree.take());
                finish(readers);
                if read_failed.load(Ordering::Relaxed) {
                    bail!("build output could not be read or exceeded the 1 MiB line limit");
                }
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

#[cfg(windows)]
fn lower_process_priority(child: &std::process::Child) -> Result<()> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS, PROCESS_SET_INFORMATION,
    };

    let process = unsafe { OpenProcess(PROCESS_SET_INFORMATION, 0, child.id()) };
    if process.is_null() {
        bail!("could not open source build process for priority control");
    }
    let changed = unsafe { SetPriorityClass(process, BELOW_NORMAL_PRIORITY_CLASS) } != 0;
    unsafe { CloseHandle(process) };
    anyhow::ensure!(changed, "could not lower source build priority");
    Ok(())
}

#[cfg(not(windows))]
fn lower_process_priority(_child: &std::process::Child) -> Result<()> {
    Ok(())
}

#[derive(Debug, Clone)]
pub enum UpdateOutcome {
    Installed(UpdateResult),
    UpToDate(String),
}

const SOURCE_HINT_FILE: &str = "source";
const CHANNEL_FILE: &str = "channel";
const START_SHORTCUT_REMOVED: &str = "shortcut-start-removed";
const DESKTOP_SHORTCUT_REMOVED: &str = "shortcut-desktop-removed";

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
    install_channel()?;
    let dir = install_bin_dir();
    fs::create_dir_all(&dir)?;
    crate::state_io::write_atomic(&dir.join(CHANNEL_FILE), format!("{channel}\n").as_bytes())?;
    Ok(())
}

fn is_source_root(dir: &Path) -> bool {
    dir.join("crates")
        .join("concordia")
        .join("Cargo.toml")
        .is_file()
}

pub fn source_root_of_exe(exe: &Path) -> Option<PathBuf> {
    let target = exe.parent()?.parent()?;
    if target.file_name()? != "target" {
        return None;
    }
    let root = target.parent()?;
    is_source_root(root).then(|| root.to_path_buf())
}

pub fn record_source_root(root: &Path) {
    let dir = install_bin_dir();
    if fs::create_dir_all(&dir).is_ok() {
        let _ = crate::state_io::write_atomic(
            &dir.join(SOURCE_HINT_FILE),
            root.to_string_lossy().as_bytes(),
        );
        let _ = record_install_channel("source");
    }
}

fn recorded_source_root() -> Option<PathBuf> {
    let mut raw = String::new();
    fs::File::open(install_bin_dir().join(SOURCE_HINT_FILE))
        .ok()?
        .take(32769)
        .read_to_string(&mut raw)
        .ok()?;
    if raw.len() > 32768 {
        return None;
    }
    let root = PathBuf::from(raw.trim());
    is_source_root(&root).then_some(root)
}

pub fn source_root(cwd: &Path) -> Option<PathBuf> {
    let channel = install_channel().ok()?;
    if channel.as_deref() == Some("release") {
        return None;
    }
    find_cargo_workspace(cwd)
        .filter(|root| is_source_root(root))
        .or_else(recorded_source_root)
}

pub fn update_auto(cwd: &Path) -> Result<UpdateOutcome> {
    update_auto_ctl(cwd, &JobControl::default())
}

pub fn update_auto_ctl(cwd: &Path, ctl: &JobControl) -> Result<UpdateOutcome> {
    ctl.stage(
        UpdateStage::Preparing,
        "Preparing update",
        "Checking the selected update channel",
    );
    install_channel()?;
    if source_root(cwd).is_some() {
        return perform_update_ctl(cwd, ctl).map(UpdateOutcome::Installed);
    }
    let current = crate::version::VERSION;
    let cfg = crate::release::config_from_env(current).context(
        "invalid release repository override; CONCORDIA_UPDATE_REPO must be owner/name (unset it to use the official feed)",
    )?;
    let fetch = crate::release::PowerShellFetcher::controlled(ctl);
    ctl.stage(
        UpdateStage::Preparing,
        "Checking for a newer release",
        "Reading the configured release feed",
    );
    let found = crate::release::check(&cfg, current, &fetch)?;
    if ctl.cancelled() {
        bail!("cancelled");
    }
    match found {
        crate::release::Check::UpToDate { current, .. } => Ok(UpdateOutcome::UpToDate(format!(
            "already up to date ({current})"
        ))),
        crate::release::Check::Available(rel) => {
            ctl.stage(
                UpdateStage::Downloading,
                &format!("Downloading {}", rel.tag),
                "Fetching the checksum and executable",
            );
            let result =
                crate::release::install_release_controlled(&cfg, &rel, current, &fetch, Some(ctl))?;
            ctl.stage(
                UpdateStage::Finishing,
                "Finishing update",
                "Release verified and installed",
            );
            sync_owned_shortcuts()?;
            Ok(UpdateOutcome::Installed(result))
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
    Ok(UpdatePreview {
        history,
        history_expanded: false,
        lines,
        install_available,
        release_url: None,
    })
}

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

const INSTALL_BUILD: &str = "Concordia builds the update (about a minute), then restarts this tab.";
const INSTALL_RELEASE: &str = "Concordia downloads and checks the update, then restarts this tab.";

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
        match crate::release::check(
            &cfg,
            current,
            &crate::release::PowerShellFetcher::controlled(ctl),
        )? {
            crate::release::Check::UpToDate { current, .. } => UpdatePreview {
                history: Vec::new(),
                history_expanded: false,
                lines: vec![
                    PreviewLine::Title(format!("You're on the newest release ({current}).")),
                    from,
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

pub fn perform_update_ctl(cwd: &Path, ctl: &JobControl) -> Result<UpdateResult> {
    ctl.stage(
        UpdateStage::Preparing,
        "Preparing source update",
        "Locating and checking the source checkout",
    );
    install_channel()?;
    let root = source_root(cwd).context(
        "no source checkout found: run /update inside the repository once, or run `concordia install` from a build in it",
    )?;

    let expected_source = source_version(&root)?;
    let cargo = which::which("cargo").context("cargo not on PATH")?;
    if cargo_process_is_running() {
        bail!("another Cargo process is running; Concordia will try again later")
    }
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
    ctl.stage(
        UpdateStage::Verifying,
        "Verifying source build",
        "Checking the reported version and executable identity",
    );
    anyhow::ensure!(
        built.is_file(),
        "Cargo-reported executable is missing: {}",
        built.display()
    );
    anyhow::ensure!(
        source_version(&root)? == expected_source,
        "source build identity changed during update; retry the build"
    );
    let reported = verify_binary(&built)?;
    let stamp_prefix = format!("concordia {} ({}", expected_source.0, expected_source.1);
    anyhow::ensure!(
        reported == format!("{stamp_prefix})")
            || reported.starts_with(&format!("{stamp_prefix}, ")),
        "rebuilt executable does not match the selected source version; existing install kept"
    );

    anyhow::ensure!(!ctl.cancelled(), "cancelled");
    ctl.stage(
        UpdateStage::Installing,
        "Installing update",
        "Staging the verified executable and switching the active pointer",
    );
    let install_path = install_binary(&built)?;
    sync_owned_shortcuts()?;
    record_source_root(&root);
    ctl.stage(
        UpdateStage::Finishing,
        "Finishing update",
        "Refreshing owned shortcuts and source identity",
    );
    Ok(UpdateResult {
        install_path,
        built_from: built,
        log_tail,
        summary: String::new(),
    })
}

pub fn install_binary(built: &Path) -> Result<PathBuf> {
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

pub fn cleanup_obsolete_installs() {
    #[cfg(not(test))]
    sweep_leftovers(&install_bin_dir());
}

pub struct InstallReport {
    pub installed: PathBuf,
    pub path_added: bool,
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
    let path_added = !explicit_dir && ensure_on_user_path(&install_bin_dir())?;
    sync_owned_shortcuts()?;
    Ok(InstallReport {
        installed,
        path_added,
    })
}

pub fn shortcut_path(desktop: bool) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let root = if crate::test_guard::is_enabled() {
            shortcut_home().join(if desktop {
                "Desktop"
            } else {
                "AppData/Roaming/Microsoft/Windows/Start Menu/Programs"
            })
        } else if desktop {
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

#[cfg(windows)]
fn shortcut_home() -> PathBuf {
    if crate::test_guard::is_enabled() {
        env::var_os("CONCORDIA_TEST_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(crate::test_guard::test_home)
    } else {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutSpec {
    pub target: PathBuf,
    pub args: String,
    pub workdir: PathBuf,
    pub icon: PathBuf,
}

pub fn shortcut_spec(concordia_exe: &Path, home: &Path) -> ShortcutSpec {
    ShortcutSpec {
        target: concordia_exe.to_path_buf(),
        args: String::new(),
        workdir: home.to_path_buf(),
        icon: concordia_exe.to_path_buf(),
    }
}

pub fn install_shortcut(desktop: bool) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let path = shortcut_path(desktop)?;
        let active = resolve_active_exe();
        let target = if active.is_file() {
            active
        } else {
            env::current_exe().context("cannot locate the running executable")?
        };
        let home = shortcut_home();
        let spec = shortcut_spec(&target, &home);
        write_shortcut(&path, &spec)?;
        fs::remove_file(shortcut_removed_marker(desktop)).ok();
        Ok(path)
    }
    #[cfg(not(windows))]
    {
        let _ = desktop;
        bail!("shortcuts are supported on Windows only")
    }
}

#[cfg(windows)]
fn write_shortcut(path: &Path, spec: &ShortcutSpec) -> Result<()> {
    let parent = path.parent().context("shortcut has no parent")?;
    fs::create_dir_all(parent)?;
    let script = "$s=(New-Object -ComObject WScript.Shell).CreateShortcut($env:CONCORDIA_SHORTCUT);$s.TargetPath=$env:CONCORDIA_TARGET;$s.Arguments=$env:CONCORDIA_ARGS;$s.WorkingDirectory=$env:CONCORDIA_WORKDIR;$s.IconLocation=$env:CONCORDIA_ICON;$s.Save()";
    let mut command = crate::process_ui::command("powershell.exe");
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .env("CONCORDIA_SHORTCUT", path)
        .env("CONCORDIA_TARGET", &spec.target)
        .env("CONCORDIA_ARGS", &spec.args)
        .env("CONCORDIA_WORKDIR", &spec.workdir)
        .env("CONCORDIA_ICON", &spec.icon)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output = crate::process_lifetime::capture(command, Duration::from_secs(15), 64 * 1024)
        .map_err(anyhow::Error::msg)
        .context("create Concordia shortcut")?;
    anyhow::ensure!(
        output.status.success(),
        "PowerShell could not create the Concordia shortcut: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

fn shortcut_removed_marker(desktop: bool) -> PathBuf {
    install_bin_dir().join(if desktop {
        DESKTOP_SHORTCUT_REMOVED
    } else {
        START_SHORTCUT_REMOVED
    })
}

#[cfg(windows)]
fn read_shortcut(path: &Path) -> Result<Option<ShortcutSpec>> {
    if !path.is_file() {
        return Ok(None);
    }
    let script = "$s=(New-Object -ComObject WScript.Shell).CreateShortcut($env:CONCORDIA_SHORTCUT); [Console]::Out.WriteLine((ConvertTo-Json -Compress @{target=$s.TargetPath;args=$s.Arguments;workdir=$s.WorkingDirectory;icon=$s.IconLocation}))";
    let mut command = crate::process_ui::command("powershell.exe");
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .env("CONCORDIA_SHORTCUT", path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = crate::process_lifetime::capture(command, Duration::from_secs(15), 64 * 1024)
        .map_err(anyhow::Error::msg)
        .context("read Concordia shortcut")?;
    anyhow::ensure!(
        output.status.success(),
        "PowerShell could not read the Concordia shortcut: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("invalid Concordia shortcut data")?;
    Ok(Some(ShortcutSpec {
        target: PathBuf::from(value["target"].as_str().unwrap_or_default()),
        args: value["args"].as_str().unwrap_or_default().to_owned(),
        workdir: PathBuf::from(value["workdir"].as_str().unwrap_or_default()),
        icon: PathBuf::from(
            value["icon"]
                .as_str()
                .unwrap_or_default()
                .split_once(',')
                .map_or(value["icon"].as_str().unwrap_or_default(), |(path, _)| path),
        ),
    }))
}

pub fn owned_shortcut(desktop: bool) -> Result<Option<ShortcutSpec>> {
    #[cfg(windows)]
    {
        read_shortcut(&shortcut_path(desktop)?)
    }
    #[cfg(not(windows))]
    {
        let _ = desktop;
        Ok(None)
    }
}

#[cfg(windows)]
fn shortcut_matches(actual: &ShortcutSpec, expected: &ShortcutSpec) -> bool {
    let same_path = |left: &Path, right: &Path| {
        left.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .eq_ignore_ascii_case(right.to_string_lossy().trim_end_matches(['\\', '/']))
    };
    same_path(&actual.target, &expected.target)
        && actual.args == expected.args
        && same_path(&actual.workdir, &expected.workdir)
        && same_path(&actual.icon, &expected.icon)
}

pub fn sync_owned_shortcuts() -> Result<()> {
    #[cfg(windows)]
    {
        let active = resolve_active_exe();
        let target = if active.is_file() {
            active
        } else {
            env::current_exe().context("cannot locate the running executable")?
        };
        let home = shortcut_home();
        let expected = shortcut_spec(&target, &home);
        for desktop in [false, true] {
            if shortcut_removed_marker(desktop).is_file() {
                continue;
            }
            let path = shortcut_path(desktop)?;
            let Some(actual) = read_shortcut(&path)? else {
                if desktop {
                    continue;
                }
                write_shortcut(&path, &expected)?;
                continue;
            };
            if !shortcut_matches(&actual, &expected) {
                write_shortcut(&path, &expected)?;
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

pub fn remove_shortcut(desktop: bool) -> Result<PathBuf> {
    let path = shortcut_path(desktop)?;
    if path.is_file() {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    }
    fs::create_dir_all(install_bin_dir())?;
    crate::state_io::write_atomic(&shortcut_removed_marker(desktop), b"removed\n")?;
    Ok(path)
}

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
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = crate::process_lifetime::capture(cmd, Duration::from_secs(15), 64 * 1024)
        .map_err(anyhow::Error::msg)
        .context("run powershell.exe")?;
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

pub fn ensure_on_user_path(dir: &Path) -> Result<bool> {
    let existing = user_path_ps(
        "(Get-Item 'HKCU:\\Environment').GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)",
        &[],
    )?;
    let Some(updated) = path_with_dir(&existing, dir) else {
        return Ok(false);
    };
    user_path_ps(
        "Set-ItemProperty -Path 'HKCU:\\Environment' -Name Path -Value $env:CONCORDIA_NEW_PATH -Type ExpandString; \
         [Environment]::SetEnvironmentVariable('CONCORDIA_PATH_REFRESH', [string](Get-Date).Ticks, 'User'); \
         [Environment]::SetEnvironmentVariable('CONCORDIA_PATH_REFRESH', $null, 'User')",
        &[("CONCORDIA_NEW_PATH", &updated)],
    )?;
    Ok(true)
}

pub fn reexec_into(new_exe: &Path, cwd: &Path) -> Result<i32> {
    persist_concordia_exe_env(new_exe);
    let mut command = crate::process_ui::visible_command(new_exe);
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

pub fn relaunch_window(new_exe: &Path, cwd: &Path) -> Result<()> {
    persist_concordia_exe_env(new_exe);
    crate::process_ui::command(new_exe)
        .current_dir(cwd)
        .env("CONCORDIA_EXE", new_exe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn {}", new_exe.display()))?;
    Ok(())
}

fn run_cargo_build(
    cargo: &Path,
    root: &Path,
    target_dir: Option<&Path>,
    ctl: &JobControl,
) -> Result<(PathBuf, String)> {
    ctl.stage(
        UpdateStage::Building,
        "Building update",
        "Cargo is compiling Concordia",
    );
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
    let inherited_jobs = std::env::var("CARGO_BUILD_JOBS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(2);
    cmd.env("CARGO_BUILD_JOBS", inherited_jobs.min(2).max(1).to_string());
    if let Some(dir) = target_dir {
        cmd.arg("--target-dir").arg(dir);
    }
    let (ok, code, log_tail) = run_child_with_format(cmd, ctl, true)?;
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
    if !ok {
        if ctl.snapshot().error_count > 0 {
            bail!("cargo build failed ({code}); see compiler errors in build details");
        }
        bail!(
            "cargo build failed ({code})\n{}",
            bounded_redacted(&display, 4096)
        );
    }
    let built = cargo_executable(root, &log_tail)?;
    Ok((built, display))
}

fn cargo_process_is_running() -> bool {
    #[cfg(windows)]
    {
        let ancestors = {
            let procs = crate::sysstats::process_pairs();
            let mut set = std::collections::HashSet::new();
            let mut pid = std::process::id();
            set.insert(pid);
            for _ in 0..64 {
                let Some(parent) = procs
                    .iter()
                    .find_map(|&(child, parent)| (child == pid).then_some(parent))
                else {
                    break;
                };
                if parent == 0 || !set.insert(parent) {
                    break;
                }
                pid = parent;
            }
            set
        };
        return crate::process_ui::command("tasklist")
            .args(["/FI", "IMAGENAME eq cargo.exe", "/FO", "CSV", "/NH"])
            .output()
            .map(|output| {
                String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                    let line = line.trim_start();
                    if !line.starts_with("\"cargo.exe\"") {
                        return false;
                    }
                    let pid = line
                        .split(',')
                        .nth(1)
                        .and_then(|field| field.trim_matches('"').parse::<u32>().ok());
                    pid.is_none_or(|pid| !ancestors.contains(&pid))
                })
            })
            .unwrap_or(true);
    }
    #[cfg(not(windows))]
    {
        let me = std::process::id();
        crate::process_ui::command("pgrep")
            .args(["-x", "cargo"])
            .output()
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .filter_map(|pid| pid.parse::<u32>().ok())
                    .any(|pid| pid != me && !is_ancestor_of(me, pid))
            })
            .unwrap_or(true)
    }
}

#[cfg(not(windows))]
fn is_ancestor_of(child: u32, candidate: u32) -> bool {
    let mut pid = child;
    for _ in 0..64 {
        let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
            return false;
        };
        let Some(parent) = status.lines().find_map(|line| {
            line.strip_prefix("PPid:")
                .and_then(|value| value.trim().parse::<u32>().ok())
        }) else {
            return false;
        };
        if parent == candidate {
            return true;
        }
        if parent == 0 || parent == pid {
            return false;
        }
        pid = parent;
    }
    false
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
    if env::var("CONCORDIA_INSTALL_DIR").is_ok_and(|d| !d.trim().is_empty()) {
        return;
    }
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
        env::set_var("CONCORDIA_INSTALL_DIR", sandbox.path());
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
        assert!(path_with_dir(&added, dir).is_none());
        assert!(path_with_dir(r"C:\X;c:\USERS\ME\appdata\local\CONCORDIA\bin\", dir).is_none());
        assert_eq!(path_with_dir("", dir).unwrap(), dir.to_string_lossy());
        let kept = path_with_dir(r"%USERPROFILE%\bin;C:\Tools", dir).unwrap();
        assert!(kept.contains(r"%USERPROFILE%\bin"));
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

        fs::write(
            elsewhere.path().join("Cargo.toml"),
            "[workspace]\nmembers=[]\n",
        )
        .unwrap();
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

        fs::remove_dir_all(checkout.path().join("crates")).unwrap();
        assert!(source_root(elsewhere.path()).is_none());
        std::env::remove_var("CONCORDIA_INSTALL_DIR");
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
        assert!(default_install_bin_dir().ends_with(Path::new("concordia").join("bin")));
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

        ctl.stage(
            UpdateStage::Building,
            "Building Concordia",
            "Using Cargo's cache",
        );
        let warning = serde_json::json!({
            "reason":"compiler-message",
            "message":{"level":"warning","message":"unused variable: fixture",
                "code":{"code":"unused_variables"},
                "spans":[{"file_name":"src/fixture.rs","is_primary":true}],
                "rendered":"warning: unused variable: fixture\n --> src/fixture.rs:2:5\n"}
        });
        let script = format!("Write-Output '{}'", warning);
        assert!(run_child_with_format(ps(&script), &ctl, true).unwrap().0);
        let snapshot = ctl.snapshot();
        assert_eq!(snapshot.stage, Some(UpdateStage::Building));
        assert_eq!(snapshot.status, "Building Concordia");
        assert_eq!(snapshot.warning_count, 1);
        assert_eq!(snapshot.error_count, 0);
        assert_eq!(
            snapshot.diagnostics[0].source.as_deref(),
            Some("src/fixture.rs")
        );
        assert!(snapshot.diagnostics[0]
            .detail
            .contains(" --> src/fixture.rs:2:5"));
        ctl.stage(UpdateStage::Downloading, "Downloading", "");
        ctl.stage(UpdateStage::Verifying, "Verifying", "");
        assert!(
            ctl.snapshot().download,
            "later stages retain the actual update route"
        );
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

        use crate::release::Fetcher;
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/delayed", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let ctl = JobControl::default();
        let c2 = ctl.clone();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let peer = std::thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(40) {
                if let Ok((mut stream, _)) = listener.accept() {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(20)))
                        .unwrap();
                    let mut byte = [0];
                    let received = stream.read(&mut byte).unwrap_or(0) > 0;
                    let cancelled_at = Instant::now();
                    c2.cancel.store(true, Ordering::SeqCst);
                    let _ = finished_rx.recv_timeout(Duration::from_secs(5));
                    return (received, Some(cancelled_at));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            c2.cancel.store(true, Ordering::SeqCst);
            (false, None)
        });
        let error = crate::release::PowerShellFetcher::controlled(&ctl)
            .get_text(&url)
            .unwrap_err();
        let returned_at = Instant::now();
        let _ = finished_tx.send(());
        let (received, cancelled_at) = peer.join().unwrap();
        assert!(received, "local HTTP request never started");
        assert!(format!("{error:#}").contains("cancelled"), "{error:#}");
        let cancelled_at = cancelled_at.expect("request was cancelled");
        assert!(
            returned_at.saturating_duration_since(cancelled_at) < Duration::from_secs(5),
            "the fetch kept running after cancellation"
        );
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
            ]
        );
    }
}
