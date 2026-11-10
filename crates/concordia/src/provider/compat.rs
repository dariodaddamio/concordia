//! Read-only provider interface observation and required-capability validation.
//!
//! Concordia never installs or updates the provider CLIs. For each one it reads `--version` and, once
//! per binary revision (cached in `<state>/version-probes.json`), `--help`, and records which flags exist. Turn
//! planning preserves its current structured/native controls and rejects known missing requirements.
//! [`OBSERVED_INTERFACES`] records read-only installed interface observations, not live qualification.
//!
//! Detection never runs a turn. The runner is injectable so tests use fake output; the real one
//! runs `--version` / `--help` with a 5 s bound and kills the process tree on timeout.

use crate::provider::launch::{cursor_ps1_beside, is_batch_shim};
use crate::provider::turn::PlannedTurn;
use crate::store::{ProviderKind, TrustMode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Installed interfaces observed through bounded --version/--help on 2026-11-08.
/// This table is diagnostic metadata, never proof of model-turn or approval parity.
pub const OBSERVED_INTERFACES: &[(ProviderKind, &[&str])] = &[
    (ProviderKind::Claude, &["2.1.319"]),
    (ProviderKind::Codex, &["0.166.0"]),
    (ProviderKind::Cursor, &["2026.11.05-86047df"]),
];

/// Bump when the help parsing below changes, so cached results are read again.
const PARSER_REV: u32 = 2;
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const OUTPUT_MAX: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionComparison {
    Same,
    Newer,
    Older,
    Unknown,
}

impl VersionComparison {
    pub fn label(self) -> &'static str {
        match self {
            VersionComparison::Same => "same",
            VersionComparison::Newer => "newer",
            VersionComparison::Older => "older",
            VersionComparison::Unknown => "unknown",
        }
    }
}

/// Which flags native help declares. Unknown observations grant no capability evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Caps {
    /// `--output-format stream-json` (Codex: `--json`).
    pub stream_json: bool,
    /// `--permission-mode` (Claude).
    pub permission_mode: bool,
    /// A read-only planning mode (Claude `--permission-mode plan`, Cursor `--mode plan`).
    pub plan_mode: bool,
    /// `--include-partial-messages` / `--stream-partial-output`.
    pub partial_messages: bool,
    /// `-s/--sandbox`.
    pub sandbox: bool,
    /// False when native help was unavailable or malformed.
    pub known: bool,
}

impl Caps {
    pub fn unknown() -> Caps {
        Caps {
            stream_json: false,
            permission_mode: false,
            plan_mode: false,
            partial_messages: false,
            sandbox: false,
            known: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compat {
    /// First line of `--version`, as printed.
    pub version: String,
    pub comparison: VersionComparison,
    pub caps: Caps,
}

/// Runs `program args...` and returns its output (stdout then stderr).
pub type Runner<'a> = &'a dyn Fn(&Path, &[&str]) -> Result<String, String>;

// ---- versions -----------------------------------------------------------------------------

/// The version token of a `--version` line: `2.1.319 (Claude Code)` gives `2.1.319`,
/// `codex-cli 0.166.0` gives `0.166.0`.
pub fn version_token(line: &str) -> Option<String> {
    line.split_whitespace()
        .map(|t| t.trim_start_matches('v'))
        .find(|t| t.starts_with(|c: char| c.is_ascii_digit()) && t.contains('.'))
        .map(str::to_string)
}

/// `2026.10.14-0f9edde` gives `[2026, 10, 14]`.
fn numeric(v: &str) -> Option<Vec<u64>> {
    let head: String = v
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let parts: Option<Vec<u64>> = head
        .split('.')
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().ok())
        .collect();
    parts.filter(|p| !p.is_empty())
}

fn compare_against(token: &str, observed: &[&str]) -> VersionComparison {
    let Some(v) = numeric(token) else {
        return VersionComparison::Unknown;
    };
    let known: Vec<Vec<u64>> = observed.iter().filter_map(|t| numeric(t)).collect();
    if known.is_empty() {
        return VersionComparison::Unknown;
    }
    if observed.contains(&token) {
        VersionComparison::Same
    } else if known.iter().all(|t| &v > t) {
        VersionComparison::Newer
    } else if known.iter().all(|t| &v < t) {
        VersionComparison::Older
    } else {
        VersionComparison::Unknown
    }
}

/// How `version_line` compares with what [`OBSERVED_INTERFACES`] lists for the provider.
pub fn compare_version(provider: ProviderKind, version_line: &str) -> VersionComparison {
    let Some(token) = version_token(version_line) else {
        return VersionComparison::Unknown;
    };
    OBSERVED_INTERFACES
        .iter()
        .find(|(p, _)| *p == provider)
        .map_or(VersionComparison::Unknown, |(_, t)| {
            compare_against(&token, t)
        })
}

// ---- help parsing -------------------------------------------------------------------------

/// Flags from `--help` text; `None` when it does not look like help at all (then the caller
/// retains an explicitly unknown observation).
pub fn parse_help(provider: ProviderKind, help: &str) -> Option<Caps> {
    if help.lines().filter(|l| l.contains("--")).count() < 3 {
        return None;
    }
    let has = |s: &str| help.contains(s);
    let permission_mode = has("--permission-mode");
    let caps = match provider {
        ProviderKind::Claude => Caps {
            stream_json: has("--output-format") && has("stream-json"),
            permission_mode,
            plan_mode: permission_mode && has("\"plan\""),
            partial_messages: has("--include-partial-messages"),
            sandbox: has("--sandbox"),
            known: true,
        },
        ProviderKind::Codex => Caps {
            stream_json: has("--json"),
            permission_mode: false,
            // No planning mode; Concordia uses a read-only sandbox, which needs `--sandbox`.
            plan_mode: has("--sandbox"),
            partial_messages: false,
            sandbox: has("--sandbox"),
            known: true,
        },
        ProviderKind::Cursor => Caps {
            stream_json: has("--output-format") && has("stream-json"),
            permission_mode: false,
            plan_mode: has("--mode") && has("plan") || has("--plan"),
            partial_messages: has("--stream-partial-output"),
            sandbox: has("--sandbox"),
            known: true,
        },
    };
    Some(caps)
}

/// The help arguments for each provider (Codex lists its run flags under `exec`).
fn help_args(provider: ProviderKind) -> &'static [&'static str] {
    match provider {
        ProviderKind::Codex => &["exec", "--help"],
        _ => &["--help"],
    }
}

// ---- cache --------------------------------------------------------------------------------

fn cache_path(state_root: &Path) -> PathBuf {
    state_root.join("compat.json")
}

fn version_cache_path(state_root: &Path) -> PathBuf {
    state_root.join("version-probes.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VersionProbeCache {
    path: String,
    size: u64,
    modified_secs: u64,
    modified_nanos: u32,
    #[serde(default)]
    target: String,
    version: String,
}

fn binary_fingerprint(bin: &Path) -> Option<(String, u64, u64, u32)> {
    let path = std::fs::canonicalize(bin).ok()?;
    let metadata = std::fs::metadata(&path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some((
        path.to_string_lossy().into_owned(),
        metadata.len(),
        modified.as_secs(),
        modified.subsec_nanos(),
    ))
}

fn load_version_cache(state_root: &Path) -> BTreeMap<String, VersionProbeCache> {
    crate::fs_safe::read_text_bounded(&version_cache_path(state_root), 1024 * 1024)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_version_cache(state_root: &Path, entry: VersionProbeCache) {
    let path = version_cache_path(state_root);
    let lock = path.with_extension("lock");
    let _ = crate::state_io::with_lock(&lock, || {
        let mut cache = load_version_cache(state_root);
        cache.insert(entry.path.clone(), entry);
        let json = serde_json::to_vec_pretty(&cache).map_err(std::io::Error::other)?;
        crate::state_io::write_atomic(&path, &json)
    });
}

fn shim_target(provider: ProviderKind, bin: &Path) -> Option<String> {
    if !is_batch_shim(&bin.to_string_lossy()) {
        return Some("binary".into());
    }
    match provider {
        ProviderKind::Codex | ProviderKind::Claude => {
            let package = super::npm::package_for_shim(&bin.to_string_lossy(), provider)?;
            let manifest = package.join("package.json");
            let value: serde_json::Value =
                serde_json::from_str(&crate::fs_safe::read_text_bounded(&manifest, 128 * 1024)?)
                    .ok()?;
            let version = value["version"].as_str()?;
            let modified = std::fs::metadata(&manifest)
                .ok()?
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?;
            Some(format!(
                "npm:{}@{}:{}:{}",
                package.display(),
                version,
                modified.as_secs(),
                modified.subsec_nanos()
            ))
        }
        ProviderKind::Cursor => {
            let versions = bin.parent()?.join("versions");
            let newest = std::fs::read_dir(&versions)
                .ok()?
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().ok().is_some_and(|t| t.is_dir()))
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .max()?;
            let modified = std::fs::metadata(&versions)
                .ok()?
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?;
            Some(format!(
                "cursor:{}:{}:{}",
                newest,
                modified.as_secs(),
                modified.subsec_nanos()
            ))
        }
    }
}

fn cached_version(state_root: &Path, provider: ProviderKind, bin: &Path) -> Option<String> {
    let (path, size, modified_secs, modified_nanos) = binary_fingerprint(bin)?;
    let target = shim_target(provider, bin)?;
    let entry = load_version_cache(state_root).remove(&path)?;
    (entry.path == path
        && entry.size == size
        && entry.modified_secs == modified_secs
        && entry.modified_nanos == modified_nanos
        && entry.target == target)
        .then_some(entry.version)
}

/// Refresh the version cache after an explicit live probe, such as `concordia doctor`.
pub fn cache_version_probe(state_root: &Path, provider: ProviderKind, bin: &Path, version: &str) {
    let Some((path, size, modified_secs, modified_nanos)) = binary_fingerprint(bin) else {
        return;
    };
    let Some(target) = shim_target(provider, bin) else {
        return;
    };
    save_version_cache(
        state_root,
        VersionProbeCache {
            path,
            size,
            modified_secs,
            modified_nanos,
            target,
            version: version.to_string(),
        },
    );
}

fn cache_key(provider: ProviderKind, version: &str) -> String {
    format!("{}@{version}@r{PARSER_REV}", provider.as_str())
}

fn load_cache(state_root: &Path) -> BTreeMap<String, Caps> {
    crate::fs_safe::read_text_bounded(&cache_path(state_root), 1024 * 1024)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_cache(state_root: &Path, map: &BTreeMap<String, Caps>) {
    if let Ok(json) = serde_json::to_string_pretty(map) {
        let _ = std::fs::create_dir_all(state_root);
        if json.len() <= 1024 * 1024 {
            let _ = crate::state_io::write_atomic(&cache_path(state_root), json.as_bytes());
        }
    }
}

// ---- detection ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMode {
    Persist,
    ReadOnly,
}

/// Version and capabilities of one CLI. With a state root, a matching binary fingerprint avoids
/// both provider probes; `--help` is only run when this version is not in the help cache.
pub fn detect(
    provider: ProviderKind,
    bin: &Path,
    runner: Runner<'_>,
    state_root: Option<&Path>,
) -> Option<Compat> {
    detect_with_mode(provider, bin, runner, state_root, CacheMode::Persist)
}

pub fn detect_with_mode(
    provider: ProviderKind,
    bin: &Path,
    runner: Runner<'_>,
    state_root: Option<&Path>,
    cache_mode: CacheMode,
) -> Option<Compat> {
    let version = state_root
        .and_then(|root| cached_version(root, provider, bin))
        .or_else(|| {
            let out = runner(bin, &["--version"]).ok()?;
            let version = out
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())?
                .to_string();
            if cache_mode == CacheMode::Persist {
                if let Some(root) = state_root {
                    cache_version_probe(root, provider, bin, &version);
                }
            }
            Some(version)
        })?;
    let comparison = compare_version(provider, &version);
    let token = version_token(&version).unwrap_or_else(|| version.clone());
    let key = cache_key(provider, &token);
    let mut cache = state_root.map(load_cache).unwrap_or_default();
    let caps = match cache.get(&key) {
        Some(c) => *c,
        None => match runner(bin, help_args(provider))
            .ok()
            .and_then(|h| parse_help(provider, &h))
        {
            Some(c) => {
                if cache_mode == CacheMode::Persist {
                    if let Some(root) = state_root {
                        cache.insert(key, c);
                        save_cache(root, &cache);
                    }
                }
                c
            }
            None => Caps::unknown(),
        },
    };
    Some(Compat {
        version,
        comparison,
        caps,
    })
}

// ---- the real runner ----------------------------------------------------------------------

/// `program args...` with a 5 s deadline; the whole process tree is killed on timeout. Arguments
/// are an array, never a shell string; a Cursor `.cmd` shim is bypassed the way a turn does.
pub fn run_cli(program: &Path, args: &[&str]) -> Result<String, String> {
    use std::process::Stdio;
    let mut cmd;
    match is_batch_shim(&program.to_string_lossy())
        .then(|| cursor_ps1_beside(program))
        .flatten()
    {
        Some(ps1) => {
            cmd = crate::process_ui::command("powershell.exe");
            cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(ps1)
                .args(args)
                .env("CURSOR_INVOKED_AS", program.file_name().unwrap_or_default());
        }
        None => {
            let native = super::npm::codex(&program.to_string_lossy());
            cmd = crate::process_ui::command(
                native.as_ref().map_or(program, |n| n.program.as_path()),
            );
            if let Some(native) = native {
                cmd.envs(native.env);
            }
            cmd.args(args);
        }
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = crate::process_lifetime::capture(cmd, PROBE_TIMEOUT, OUTPUT_MAX)?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        return Err(format!(
            "CLI interface probe failed (exit {:?}): {}",
            out.status.code(),
            text.trim()
        ));
    }
    Ok(text)
}

/// Run detection for these CLIs on background threads (one each); results arrive on the channel
/// as each finishes. The UI polls it on tick, so nothing here blocks it.
type SharedRunner = Arc<dyn Fn(&Path, &[&str]) -> Result<String, String> + Send + Sync>;

pub fn start_detection(
    bins: Vec<(ProviderKind, PathBuf)>,
    state_root: PathBuf,
    runner: SharedRunner,
) -> Receiver<(ProviderKind, Compat)> {
    let (tx, rx) = channel();
    for (provider, bin) in bins {
        let (tx, runner, root) = (tx.clone(), runner.clone(), state_root.clone());
        let _ = std::thread::Builder::new()
            .name("concordia-compat".into())
            .spawn(move || {
                if let Some(c) = detect(provider, &bin, &*runner, Some(&root)) {
                    let _ = tx.send((provider, c));
                }
            });
    }
    rx
}

/// Detection for the CLIs a turn would use right now (the real runner).
pub fn start_real_detection(state_root: PathBuf) -> Receiver<(ProviderKind, Compat)> {
    let bins = [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Cursor,
    ]
    .into_iter()
    .filter_map(|p| {
        crate::doctor::resolve_provider_bin(p)
            .ok()
            .map(|l| (p, l.path))
    })
    .collect();
    start_detection(bins, state_root, Arc::new(run_cli))
}

// ---- what planning does with it -----------------------------------------------------------

static CAPS: Mutex<Vec<(&'static str, Caps)>> = Mutex::new(Vec::new());
static VERSIONS: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());

/// Record detected capabilities for explicit turn validation and `concordia doctor`.
pub fn set_caps(provider: ProviderKind, caps: Caps) {
    if let Ok(mut v) = CAPS.lock() {
        v.retain(|(p, _)| *p != provider.as_str());
        v.push((provider.as_str(), caps));
    }
}

/// Record the version alongside detected capabilities for actionable Plan refusals.
pub fn set_compat(provider: ProviderKind, compat: &Compat) {
    set_caps(provider, compat.caps);
    if let Ok(mut v) = VERSIONS.lock() {
        v.retain(|(p, _)| *p != provider.as_str());
        v.push((provider.as_str(), compat.version.clone()));
    }
}

pub(super) fn version_for(provider: ProviderKind) -> String {
    VERSIONS
        .lock()
        .ok()
        .and_then(|v| {
            v.iter()
                .find(|(p, _)| *p == provider.as_str())
                .map(|(_, s)| s.clone())
        })
        .unwrap_or_else(|| "unknown version".into())
}

fn plan_enforcement_error(provider: ProviderKind) -> String {
    let (cli, flag) = match provider {
        ProviderKind::Claude => ("Claude Code", "--permission-mode plan"),
        ProviderKind::Cursor => ("Cursor", "--mode plan"),
        ProviderKind::Codex => ("Codex", "--sandbox read-only"),
    };
    format!("{cli} {} can't enforce Plan mode (its help has no {flag}). Use a CLI that provides the current required interface.", version_for(provider))
}

fn ensure_plan_supported(
    provider: ProviderKind,
    trust: TrustMode,
    caps: &Caps,
) -> anyhow::Result<()> {
    if trust == TrustMode::Plan && caps.known && !caps.plan_mode {
        anyhow::bail!("{}", plan_enforcement_error(provider));
    }
    Ok(())
}

pub fn caps_for(provider: ProviderKind) -> Option<Caps> {
    CAPS.lock()
        .ok()?
        .iter()
        .find(|(p, _)| *p == provider.as_str())
        .map(|(_, c)| *c)
}

/// Validate the declared current interface without rewriting argv or weakening native controls.
fn validate_argv(
    provider: ProviderKind,
    argv: &[String],
    trust: TrustMode,
    caps: &Caps,
) -> anyhow::Result<()> {
    ensure_plan_supported(provider, trust, caps)?;
    if !caps.known {
        // Unknown is not supported evidence. Keep explicit native flags and let the
        // selected CLI validate them; never remove controls or substitute plain text.
        return Ok(());
    }
    for (flag, value) in super::turn::planned_options(argv) {
        let missing = match flag {
            "--json" => !caps.stream_json,
            "--output-format" if value == Some("stream-json") => !caps.stream_json,
            "--permission-mode" if provider == ProviderKind::Claude => !caps.permission_mode,
            "--stream-partial-output" | "--include-partial-messages" => !caps.partial_messages,
            "-s" | "--sandbox" => !caps.sandbox,
            _ => false,
        };
        anyhow::ensure!(!missing,
            "{} {} does not declare required {flag}; this current provider interface is unsupported",
            provider.as_str(), version_for(provider));
    }
    Ok(())
}

/// Check observations for the selected provider; absence remains explicitly unverified.
pub fn validate_planned(plan: &PlannedTurn, trust: TrustMode) -> anyhow::Result<()> {
    validate_caps(
        plan,
        trust,
        &caps_for(plan.provider).unwrap_or_else(Caps::unknown),
    )
}

pub fn validate_caps(plan: &PlannedTurn, trust: TrustMode, caps: &Caps) -> anyhow::Result<()> {
    validate_argv(plan.provider, &plan.argv, trust, caps)
}

pub fn support_summary(provider: ProviderKind, caps: &Caps) -> String {
    if !caps.known {
        return "CLI help unavailable; required interface unverified".into();
    }
    let mut missing = Vec::new();
    if !caps.stream_json {
        missing.push("structured JSON output");
    }
    if provider == ProviderKind::Claude && !caps.permission_mode {
        missing.push("explicit permission mode");
    }
    if provider == ProviderKind::Cursor && !caps.partial_messages {
        missing.push("partial streaming");
    }
    if !caps.plan_mode {
        missing.push("read-only Plan controls");
    }
    if missing.is_empty() {
        String::new()
    } else {
        format!(
            "unsupported current interface: missing {}",
            missing.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const CLAUDE_HELP: &str = "Usage: claude [options]\n  --output-format <format>  Output format: \"text\" (default), or \"stream-json\"\n  --include-partial-messages  Include partial chunks\n  --permission-mode <mode>  Permission mode (choices: \"acceptEdits\", \"plan\")\n  --model <model>  Model\n";
    const CURSOR_HELP: &str = "Usage: agent [options]\n  --output-format <format>  text | json | stream-json\n  --stream-partial-output  Stream deltas\n  --mode <mode>  plan or ask (choices: \"plan\", \"ask\")\n  --sandbox <mode>  sandbox\n";
    const CODEX_EXEC_HELP: &str = "Run Codex non-interactively\n  --json  Print events as JSONL\n  -s, --sandbox <MODE>  sandbox policy\n  -m, --model <MODEL>  model\n";

    #[test]
    fn version_tokens_and_read_only_interface_comparison() {
        assert_eq!(
            version_token("2.1.319 (Claude Code)").as_deref(),
            Some("2.1.319")
        );
        assert_eq!(
            version_token("codex-cli 0.166.0").as_deref(),
            Some("0.166.0")
        );
        assert_eq!(
            version_token("2026.11.05-86047df").as_deref(),
            Some("2026.11.05-86047df")
        );
        assert_eq!(version_token("no version here"), None);
        assert_eq!(
            compare_version(ProviderKind::Claude, "2.1.319 (Claude Code)"),
            VersionComparison::Same
        );
        assert_eq!(
            compare_version(ProviderKind::Claude, "2.2.0 (Claude Code)"),
            VersionComparison::Newer
        );
        assert_eq!(
            compare_version(ProviderKind::Claude, "2.0.9"),
            VersionComparison::Older
        );
        assert_eq!(
            compare_version(ProviderKind::Codex, "codex-cli 0.166.0"),
            VersionComparison::Same
        );
        assert_eq!(
            compare_version(ProviderKind::Codex, "codex-cli 0.200.1"),
            VersionComparison::Newer
        );
        // Another build on the same date is not the observed binary revision.
        assert_eq!(
            compare_version(ProviderKind::Cursor, "2026.11.05-abcdef0"),
            VersionComparison::Unknown
        );
        assert_eq!(
            compare_version(ProviderKind::Cursor, "2026.11.06-abcdef0"),
            VersionComparison::Newer
        );
        assert_eq!(
            compare_version(ProviderKind::Cursor, "garbage"),
            VersionComparison::Unknown
        );
        assert_eq!(
            compare_against("1.5.0", &["1.0.0", "2.0.0"]),
            VersionComparison::Unknown
        );
    }

    #[test]
    fn help_text_gives_capability_flags() {
        let c = parse_help(ProviderKind::Claude, CLAUDE_HELP).unwrap();
        assert!(c.stream_json && c.permission_mode && c.plan_mode && c.partial_messages);
        let no_plan = CLAUDE_HELP.replace("\"plan\"", "\"auto\"");
        let c = parse_help(ProviderKind::Claude, &no_plan).unwrap();
        assert!(c.permission_mode && !c.plan_mode);
        let c = parse_help(ProviderKind::Cursor, CURSOR_HELP).unwrap();
        assert!(c.stream_json && c.plan_mode && c.partial_messages && c.sandbox);
        let c = parse_help(ProviderKind::Codex, CODEX_EXEC_HELP).unwrap();
        assert!(c.stream_json && c.sandbox && !c.permission_mode);
        // Not help text at all: unknown, not "nothing is supported".
        assert!(parse_help(ProviderKind::Claude, "error: unrecognized").is_none());
    }

    type FakeRun = RefCell<Vec<Vec<String>>>;

    fn fake<'a>(
        calls: &'a FakeRun,
        version: &'a str,
        help: &'a str,
    ) -> impl Fn(&Path, &[&str]) -> Result<String, String> + 'a {
        move |_p, args| {
            calls
                .borrow_mut()
                .push(args.iter().map(|a| a.to_string()).collect());
            if args == ["--version"] {
                Ok(version.to_string())
            } else {
                Ok(help.to_string())
            }
        }
    }

    #[test]
    fn detect_parses_help_once_per_version_and_caches_it() {
        let dir = tempfile::tempdir().unwrap();
        let calls: FakeRun = RefCell::new(Vec::new());
        let run = fake(&calls, "2.1.319 (Claude Code)\n", CLAUDE_HELP);
        let bin = Path::new("claude");
        let a = detect(ProviderKind::Claude, bin, &run, Some(dir.path())).unwrap();
        assert_eq!(a.comparison, VersionComparison::Same);
        assert!(a.caps.stream_json && a.caps.known);
        assert_eq!(calls.borrow().len(), 2, "version and help");
        assert!(dir.path().join("compat.json").is_file());

        calls.borrow_mut().clear();
        let b = detect(ProviderKind::Claude, bin, &run, Some(dir.path())).unwrap();
        assert_eq!(b, a);
        assert_eq!(
            *calls.borrow(),
            vec![vec!["--version".to_string()]],
            "help is cached; a non-filesystem test path cannot fingerprint the binary"
        );

        // A new CLI version is read again.
        calls.borrow_mut().clear();
        let old_help = CLAUDE_HELP.replace("stream-json", "ndjson");
        let run2 = fake(&calls, "2.9.0 (Claude Code)", &old_help);
        let c = detect(ProviderKind::Claude, bin, &run2, Some(dir.path())).unwrap();
        assert_eq!(c.comparison, VersionComparison::Newer);
        assert!(!c.caps.stream_json);
        assert_eq!(calls.borrow().len(), 2);
    }

    #[test]
    fn doctor_detection_never_writes_caches() {
        let dir = tempfile::tempdir().unwrap();
        let calls: FakeRun = RefCell::new(Vec::new());
        let run = fake(&calls, "2.1.319 (Claude Code)\n", CLAUDE_HELP);

        let detected = detect_with_mode(
            ProviderKind::Claude,
            Path::new("claude"),
            &run,
            Some(dir.path()),
            CacheMode::ReadOnly,
        );

        assert!(detected.is_some());
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn version_probe_is_cached_by_path_size_and_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("fake-provider.exe");
        std::fs::write(&bin, b"a").unwrap();
        let calls = AtomicUsize::new(0);
        let run = |_: &Path, args: &[&str]| {
            if args == ["--version"] {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok("2.1.319 (Claude Code)".to_string())
            } else {
                Ok(CLAUDE_HELP.to_string())
            }
        };

        detect(ProviderKind::Claude, &bin, &run, Some(dir.path())).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        detect(ProviderKind::Claude, &bin, &run, Some(dir.path())).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        std::fs::write(&bin, b"rewritten-binary").unwrap();
        detect(ProviderKind::Claude, &bin, &run, Some(dir.path())).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn shim_cache_invalidates_when_npm_package_updates() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("codex.cmd");
        let package = dir.path().join("node_modules/@openai/codex");
        std::fs::create_dir_all(package.join("bin")).unwrap();
        std::fs::write(
            &shim,
            "@echo off\r\nnode_modules\\@openai\\codex\\bin\\codex.js %*\r\n",
        )
        .unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@openai/codex","version":"1.0.0","bin":{"codex":"bin/codex.js"}}"#,
        )
        .unwrap();
        std::fs::write(package.join("bin/codex.js"), "fixture").unwrap();
        let calls = AtomicUsize::new(0);
        let run = |_: &Path, args: &[&str]| {
            if args == ["--version"] {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok("codex-cli 0.166.0".into())
            } else {
                Ok(CODEX_EXEC_HELP.into())
            }
        };
        detect(ProviderKind::Codex, &shim, &run, Some(dir.path())).unwrap();
        detect(ProviderKind::Codex, &shim, &run, Some(dir.path())).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@openai/codex","version":"1.0.1","bin":{"codex":"bin/codex.js"}}"#,
        )
        .unwrap();
        detect(ProviderKind::Codex, &shim, &run, Some(dir.path())).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn shim_cache_invalidates_when_cursor_version_dir_changes() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("cursor-agent");
        let versions = install.join("versions");
        std::fs::create_dir_all(versions.join("2026.10.14-old")).unwrap();
        let shim = install.join("agent.cmd");
        std::fs::write(&shim, "cursor shim").unwrap();
        let calls = AtomicUsize::new(0);
        let run = |_: &Path, args: &[&str]| {
            if args == ["--version"] {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok("2026.11.05-86047df".into())
            } else {
                Ok(CURSOR_HELP.into())
            }
        };
        detect(ProviderKind::Cursor, &shim, &run, Some(dir.path())).unwrap();
        detect(ProviderKind::Cursor, &shim, &run, Some(dir.path())).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        std::thread::sleep(Duration::from_millis(20));
        std::fs::create_dir(versions.join("2026.11.01-new")).unwrap();
        detect(ProviderKind::Cursor, &shim, &run, Some(dir.path())).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn unknown_shim_target_is_never_cached() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("unknown.cmd");
        std::fs::write(&shim, "@echo off\r\nunknown target\r\n").unwrap();
        let calls = AtomicUsize::new(0);
        let run = |_: &Path, args: &[&str]| {
            if args == ["--version"] {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok("codex-cli 0.166.0".into())
            } else {
                Ok(CODEX_EXEC_HELP.into())
            }
        };
        detect(ProviderKind::Codex, &shim, &run, Some(dir.path())).unwrap();
        detect(ProviderKind::Codex, &shim, &run, Some(dir.path())).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn launch_detection_spawns_nothing_when_cache_is_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let bins = [
            (
                ProviderKind::Codex,
                "codex.exe",
                "codex-cli 0.166.0",
                CODEX_EXEC_HELP,
            ),
            (
                ProviderKind::Claude,
                "claude.exe",
                "2.1.319 (Claude Code)",
                CLAUDE_HELP,
            ),
            (
                ProviderKind::Cursor,
                "agent.exe",
                "2026.11.05-86047df",
                CURSOR_HELP,
            ),
        ];
        let paths: Vec<_> = bins
            .iter()
            .map(|(_, name, _, _)| {
                let path = dir.path().join(name);
                std::fs::write(&path, b"fake provider binary").unwrap();
                path
            })
            .collect();
        for ((provider, _, version, help), path) in bins.iter().zip(&paths) {
            let run = |_: &Path, args: &[&str]| {
                if args == ["--version"] {
                    Ok((*version).to_string())
                } else {
                    Ok((*help).to_string())
                }
            };
            detect(*provider, path, &run, Some(dir.path())).unwrap();
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let runner_calls = Arc::clone(&calls);
        let runner: SharedRunner = Arc::new(move |_, _| {
            runner_calls.fetch_add(1, Ordering::SeqCst);
            Err("fresh cache should not invoke runner".into())
        });
        let rx = start_detection(
            bins.iter()
                .zip(paths)
                .map(|((provider, _, _, _), path)| (*provider, path))
                .collect(),
            dir.path().to_path_buf(),
            runner,
        );
        for _ in 0..3 {
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn corrupt_version_cache_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("fake.exe");
        std::fs::write(&bin, b"fake").unwrap();
        std::fs::write(version_cache_path(dir.path()), b"not json").unwrap();
        let calls = AtomicUsize::new(0);
        let run = |_: &Path, args: &[&str]| {
            if args == ["--version"] {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok("2.1.319 (Claude Code)".into())
            } else {
                Ok(CLAUDE_HELP.into())
            }
        };
        assert!(detect(ProviderKind::Claude, &bin, &run, Some(dir.path())).is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn codex_help_is_read_from_exec_and_failures_remain_unknown() {
        let calls: FakeRun = RefCell::new(Vec::new());
        let run = fake(&calls, "codex-cli 0.166.0", CODEX_EXEC_HELP);
        let c = detect(ProviderKind::Codex, Path::new("codex"), &run, None).unwrap();
        assert!(c.caps.stream_json);
        assert_eq!(calls.borrow()[1], vec!["exec", "--help"]);

        let broken = |_: &Path, a: &[&str]| {
            if a == ["--version"] {
                Ok("2026.11.05-86047df".to_string())
            } else {
                Err("timed out".to_string())
            }
        };
        let c = detect(ProviderKind::Cursor, Path::new("agent"), &broken, None).unwrap();
        assert!(!c.caps.known && !c.caps.stream_json && !c.caps.plan_mode);
        let none = |_: &Path, _: &[&str]| Err("not found".to_string());
        assert!(detect(ProviderKind::Cursor, Path::new("agent"), &none, None).is_none());
    }

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn claude_missing_required_controls_refuses_without_changing_the_prompt() {
        let mut caps = parse_help(ProviderKind::Claude, CLAUDE_HELP).unwrap();
        caps.stream_json = false;
        let a = argv(&[
            "-p",
            "--output-format",
            "stream-json",
            "--permission-mode",
            "plan",
            "do it",
        ]);
        let original = a.clone();
        assert!(validate_argv(ProviderKind::Claude, &a, TrustMode::Ask, &caps).is_err());
        assert_eq!(a, original);
        caps.stream_json = true;
        caps.permission_mode = false;
        assert!(validate_argv(ProviderKind::Claude, &a, TrustMode::Workspace, &caps).is_err());
        assert_eq!(a, original);
    }

    #[test]
    fn cursor_and_codex_require_structured_output_and_unknown_stays_unverified() {
        for (provider, help, args) in [
            (
                ProviderKind::Cursor,
                CURSOR_HELP,
                vec![
                    "--output-format",
                    "stream-json",
                    "--stream-partial-output",
                    "p",
                ],
            ),
            (
                ProviderKind::Codex,
                CODEX_EXEC_HELP,
                vec!["exec", "--json", "-s", "read-only", "p"],
            ),
        ] {
            let original = argv(&args);
            let mut caps = parse_help(provider, help).unwrap();
            assert!(validate_argv(provider, &original, TrustMode::Plan, &caps).is_ok());
            caps.stream_json = false;
            assert!(validate_argv(provider, &original, TrustMode::Plan, &caps).is_err());
            let unknown = Caps::unknown();
            assert!(!unknown.known && !unknown.stream_json && !unknown.plan_mode);
            assert!(validate_argv(provider, &original, TrustMode::Plan, &unknown).is_ok());
            assert!(support_summary(provider, &unknown).contains("unverified"));
        }
    }

    #[test]
    fn missing_plan_controls_refuse_plan_and_missing_sandbox_also_refuses_ask() {
        let cases = [
            (
                ProviderKind::Claude,
                CLAUDE_HELP.replace("\"plan\"", "\"auto\""),
                "--permission-mode",
            ),
            (
                ProviderKind::Cursor,
                CURSOR_HELP.replace("plan", "review"),
                "--mode",
            ),
            (
                ProviderKind::Codex,
                CODEX_EXEC_HELP.replace("--sandbox", "--policy"),
                "-s",
            ),
        ];
        for (provider, help, flag) in cases {
            let caps = parse_help(provider, &help).unwrap();
            assert!(!caps.plan_mode, "fixture must omit plan support");
            let mut argv = argv(&[flag, "plan", "prompt"]);
            let err = validate_caps(
                &PlannedTurn {
                    command: "fixture".into(),
                    argv: argv.clone(),
                    prompt: "prompt".into(),
                    new_session_id: None,
                    env_remove: Vec::new(),
                    env: vec![],
                    cwd: ".".into(),
                    provider,
                },
                TrustMode::Plan,
                &caps,
            )
            .unwrap_err();
            assert!(err.to_string().contains("can't enforce Plan mode"));
            assert_eq!(
                validate_caps(
                    &PlannedTurn {
                        command: "fixture".into(),
                        argv: std::mem::take(&mut argv),
                        prompt: "prompt".into(),
                        new_session_id: None,
                        env_remove: Vec::new(),
                        env: vec![],
                        cwd: ".".into(),
                        provider,
                    },
                    TrustMode::Ask,
                    &caps,
                )
                .is_ok(),
                provider != ProviderKind::Codex
            );
        }
    }

    #[test]
    fn the_real_runner_bounds_and_reports_missing_programs() {
        assert!(run_cli(Path::new("definitely-not-a-real-cli-xyz"), &["--version"]).is_err());
    }

    #[test]
    fn detection_threads_report_through_the_channel() {
        let dir = tempfile::tempdir().unwrap();
        let runner: SharedRunner = Arc::new(|_, a| {
            Ok(if a == ["--version"] {
                "2.1.319 (Claude Code)".into()
            } else {
                CLAUDE_HELP.to_string()
            })
        });
        let rx = start_detection(
            vec![(ProviderKind::Claude, PathBuf::from("claude"))],
            dir.path().to_path_buf(),
            runner,
        );
        let (p, c) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(p, ProviderKind::Claude);
        assert_eq!(c.comparison, VersionComparison::Same);
    }
}

#[cfg(test)]
mod planning_tests {
    use super::*;
    use crate::provider::turn::{plan_provider_turn, ProviderTurnRequest};

    #[test]
    fn a_planned_claude_turn_keeps_plan_and_refuses_missing_plan_support() {
        let cwd = std::env::temp_dir();
        let req = ProviderTurnRequest {
            images: &[],
            provider: ProviderKind::Claude,
            prompt: "hello",
            cwd: &cwd,
            home_path: None,
            model: None,
            native_session_id: None,
            trust: TrustMode::Plan,
            context_prefix: None,
            local_time: None,
            compaction: Default::default(),
        };
        let plan = plan_provider_turn(&req).unwrap();
        assert!(plan
            .argv
            .windows(2)
            .any(|w| w == ["--output-format", "stream-json"]));
        let caps = Caps {
            stream_json: false,
            plan_mode: false,
            ..Caps::unknown()
        };
        let caps = Caps {
            known: true,
            ..caps
        };
        let done = validate_caps(&plan, TrustMode::Plan, &caps);
        assert!(done.is_err());
        assert!(plan
            .argv
            .windows(2)
            .any(|w| w == ["--output-format", "stream-json"]));
        assert!(plan
            .argv
            .windows(2)
            .any(|w| w == ["--permission-mode", "plan"]));
        assert!(plan.argv.contains(&"--verbose".to_string()));
        assert_eq!(plan.argv.last().map(String::as_str), Some("hello"));
        assert_eq!(plan.prompt, "hello");
    }
}
