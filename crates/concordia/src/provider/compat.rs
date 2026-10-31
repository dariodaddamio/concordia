//! Provider compatibility: feature detection, not version pinning.
//!
//! Concordia never pins or updates the provider CLIs. For each one it reads `--version` and, once
//! per version (cached in `<state>/compat.json`), `--help`, and records which flags exist. Turn
//! planning then degrades instead of failing: stream-json falls back to plain text, plan mode to
//! ask. [`TESTED`] holds the versions the parsers and flag sets were last verified against;
//! `concordia doctor` warns (never fails) when the installed one differs.
//!
//! Detection never runs a turn. The runner is injectable so tests use fake output; the real one
//! runs `--version` / `--help` with a 5 s bound and kills the process tree on timeout.

use crate::provider::launch::{cursor_ps1_beside, is_batch_shim};
use crate::provider::turn::PlannedTurn;
use crate::store::{ProviderKind, TrustMode};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Versions each provider's parsers and flag sets were last verified against (version output
/// only; no turn was run to obtain them).
pub const TESTED: &[(ProviderKind, &[&str])] = &[
    (ProviderKind::Claude, &["2.1.301"]),
    (ProviderKind::Codex, &["0.163.0"]),
    (ProviderKind::Cursor, &["2026.10.14-0f9edde"]),
];

/// Bump when the help parsing below changes, so cached results are read again.
const PARSER_REV: u32 = 1;
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const OUTPUT_MAX: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tested {
    Same,
    Newer,
    Older,
    Unknown,
}

impl Tested {
    pub fn label(self) -> &'static str {
        match self {
            Tested::Same => "same",
            Tested::Newer => "newer",
            Tested::Older => "older",
            Tested::Unknown => "unknown",
        }
    }
}

/// Which flags a CLI's help text lists. When the help could not be read every flag is assumed
/// present (no degradation on a guess).
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
    /// False when this is the assume-everything fallback.
    pub known: bool,
}

impl Caps {
    pub fn assume_all() -> Caps {
        Caps {
            stream_json: true,
            permission_mode: true,
            plan_mode: true,
            partial_messages: true,
            sandbox: true,
            known: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compat {
    /// First line of `--version`, as printed.
    pub version: String,
    pub tested: Tested,
    pub caps: Caps,
}

/// Runs `program args...` and returns its output (stdout then stderr).
pub type Runner<'a> = &'a dyn Fn(&Path, &[&str]) -> Result<String, String>;

// ---- versions -----------------------------------------------------------------------------

/// The version token of a `--version` line: `2.1.301 (Claude Code)` gives `2.1.301`,
/// `codex-cli 0.163.0` gives `0.163.0`.
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

fn assess_against(token: &str, tested: &[&str]) -> Tested {
    let Some(v) = numeric(token) else {
        return Tested::Unknown;
    };
    let known: Vec<Vec<u64>> = tested.iter().filter_map(|t| numeric(t)).collect();
    if known.is_empty() {
        return Tested::Unknown;
    }
    if known.contains(&v) {
        Tested::Same
    } else if known.iter().all(|t| &v > t) {
        Tested::Newer
    } else if known.iter().all(|t| &v < t) {
        Tested::Older
    } else {
        Tested::Unknown
    }
}

/// How `version_line` compares with what [`TESTED`] lists for the provider.
pub fn assess(provider: ProviderKind, version_line: &str) -> Tested {
    let Some(token) = version_token(version_line) else {
        return Tested::Unknown;
    };
    TESTED
        .iter()
        .find(|(p, _)| *p == provider)
        .map_or(Tested::Unknown, |(_, t)| assess_against(&token, t))
}

// ---- help parsing -------------------------------------------------------------------------

/// Flags from `--help` text; `None` when it does not look like help at all (then the caller
/// assumes everything is supported rather than degrading on a guess).
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

fn cache_key(provider: ProviderKind, version: &str) -> String {
    format!("{}@{version}@r{PARSER_REV}", provider.as_str())
}

fn load_cache(state_root: &Path) -> BTreeMap<String, Caps> {
    std::fs::read_to_string(cache_path(state_root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_cache(state_root: &Path, map: &BTreeMap<String, Caps>) {
    if let Ok(json) = serde_json::to_string_pretty(map) {
        let _ = std::fs::create_dir_all(state_root);
        let _ = std::fs::write(cache_path(state_root), json);
    }
}

// ---- detection ----------------------------------------------------------------------------

/// Version and capabilities of one CLI. `--version` always runs; `--help` only when this version
/// is not in the cache under `state_root`. `None` when the CLI cannot report a version.
pub fn detect(
    provider: ProviderKind,
    bin: &Path,
    runner: Runner<'_>,
    state_root: Option<&Path>,
) -> Option<Compat> {
    let out = runner(bin, &["--version"]).ok()?;
    let version = out
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?
        .to_string();
    let tested = assess(provider, &version);
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
                if let Some(root) = state_root {
                    cache.insert(key, c);
                    save_cache(root, &cache);
                }
                c
            }
            None => Caps::assume_all(),
        },
    };
    Some(Compat {
        version,
        tested,
        caps,
    })
}

// ---- the real runner ----------------------------------------------------------------------

/// `program args...` with a 5 s deadline; the whole process tree is killed on timeout. Arguments
/// are an array, never a shell string; a Cursor `.cmd` shim is bypassed the way a turn does.
pub fn run_cli(program: &Path, args: &[&str]) -> Result<String, String> {
    use std::process::{Command, Stdio};
    let mut cmd;
    match is_batch_shim(&program.to_string_lossy())
        .then(|| cursor_ps1_beside(program))
        .flatten()
    {
        Some(ps1) => {
            cmd = Command::new("powershell.exe");
            cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(ps1)
                .args(args)
                .env("CURSOR_INVOKED_AS", program.file_name().unwrap_or_default());
        }
        None => {
            cmd = Command::new(program);
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
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    fn drain<R: Read + Send + 'static>(r: Option<R>) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(r) = r {
                let _ = r.take(OUTPUT_MAX as u64).read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    }
    let (out, err) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() >= PROBE_TIMEOUT => {
                crate::provider::turn::kill_process_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
                return Err("timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut text = out.join().unwrap_or_default();
    text.push('\n');
    text.push_str(&err.join().unwrap_or_default());
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
    .filter_map(|p| crate::doctor::resolve_provider_bin(p).map(|b| (p, b)))
    .collect();
    start_detection(bins, state_root, Arc::new(run_cli))
}

// ---- what planning does with it -----------------------------------------------------------

static CAPS: Mutex<Vec<(&'static str, Caps)>> = Mutex::new(Vec::new());

/// Record what was detected so turn planning can degrade; also used by `concordia doctor`.
pub fn set_caps(provider: ProviderKind, caps: Caps) {
    if let Ok(mut v) = CAPS.lock() {
        v.retain(|(p, _)| *p != provider.as_str());
        v.push((provider.as_str(), caps));
    }
}

pub fn caps_for(provider: ProviderKind) -> Option<Caps> {
    CAPS.lock()
        .ok()?
        .iter()
        .find(|(p, _)| *p == provider.as_str())
        .map(|(_, c)| *c)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Degrade {
    /// Plain text instead of stream-json.
    NoStreamJson,
    /// Ask instead of plan.
    NoPlanMode,
    /// The permission flag is omitted.
    NoPermissionMode,
}

impl Degrade {
    pub fn notice(self, provider: ProviderKind) -> String {
        let p = provider.as_str();
        match self {
            Degrade::NoStreamJson => format!("{p}: no stream-json, showing text"),
            Degrade::NoPlanMode => format!("{p}: no plan mode, using ask"),
            Degrade::NoPermissionMode => {
                format!("{p}: no --permission-mode, running with its own defaults")
            }
        }
    }
}

fn set_value(argv: &mut [String], flag: &str, value: &str) -> bool {
    match argv.iter().position(|a| a == flag) {
        Some(i) if i + 1 < argv.len() => {
            argv[i + 1] = value.into();
            true
        }
        _ => false,
    }
}

fn remove_pair(argv: &mut Vec<String>, flag: &str, value: &str) -> bool {
    match argv.windows(2).position(|w| w[0] == flag && w[1] == value) {
        Some(i) => {
            argv.drain(i..i + 2);
            true
        }
        None => false,
    }
}

/// Rewrite a planned argv for a CLI that lacks a flag. The prompt (always last) is never
/// touched. Returns what was degraded.
pub fn degrade_argv(
    provider: ProviderKind,
    argv: &mut Vec<String>,
    trust: TrustMode,
    caps: &Caps,
) -> Vec<Degrade> {
    let mut out = Vec::new();
    if !caps.known {
        return out;
    }
    let prompt = argv.pop();
    match provider {
        ProviderKind::Claude | ProviderKind::Cursor if !caps.stream_json => {
            if set_value(argv, "--output-format", "text") {
                argv.retain(|a| a != "--verbose");
                out.push(Degrade::NoStreamJson);
            }
        }
        ProviderKind::Codex if !caps.stream_json => {
            let before = argv.len();
            argv.retain(|a| a != "--json");
            if argv.len() != before {
                out.push(Degrade::NoStreamJson);
            }
        }
        _ => {}
    }
    match provider {
        ProviderKind::Claude => {
            if !caps.permission_mode {
                let pos = argv.iter().position(|a| a == "--permission-mode");
                if let Some(i) = pos.filter(|i| i + 1 < argv.len()) {
                    argv.drain(i..i + 2);
                    out.push(Degrade::NoPermissionMode);
                }
            } else if trust == TrustMode::Plan
                && !caps.plan_mode
                && set_value(argv, "--permission-mode", "manual")
            {
                out.push(Degrade::NoPlanMode);
            }
        }
        ProviderKind::Cursor
            if trust == TrustMode::Plan
                && !caps.plan_mode
                && remove_pair(argv, "--mode", "plan") =>
        {
            out.push(Degrade::NoPlanMode);
        }
        _ => {}
    }
    // Partial output only exists with stream-json; without it the reply arrives whole (no notice).
    if provider == ProviderKind::Cursor && !(caps.partial_messages && caps.stream_json) {
        argv.retain(|a| a != "--stream-partial-output");
    }
    if let Some(prompt) = prompt {
        argv.push(prompt);
    }
    out
}

/// Notices already shown this launch, and ones waiting for the UI.
#[derive(Default)]
pub struct NoticeLog {
    seen: HashSet<(&'static str, Degrade)>,
    pending: Vec<String>,
}

impl NoticeLog {
    /// Queue the notice for each degradation not yet seen for this provider.
    pub fn note(&mut self, provider: ProviderKind, degraded: &[Degrade]) {
        for d in degraded {
            if self.seen.insert((provider.as_str(), *d)) {
                self.pending.push(d.notice(provider));
            }
        }
    }

    pub fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending)
    }
}

static NOTICES: Mutex<Option<NoticeLog>> = Mutex::new(None);

/// Dim system notices queued by planning (once per provider and degradation per launch).
pub fn take_notices() -> Vec<String> {
    NOTICES
        .lock()
        .ok()
        .and_then(|mut g| g.as_mut().map(NoticeLog::take))
        .unwrap_or_default()
}

/// Apply detected capabilities to a planned turn. No-op until detection has landed.
pub fn degrade_planned(plan: &mut PlannedTurn, trust: TrustMode) {
    let Some(caps) = caps_for(plan.provider) else {
        return;
    };
    let done = apply_caps(plan, trust, &caps);
    if done.is_empty() {
        return;
    }
    if let Ok(mut g) = NOTICES.lock() {
        g.get_or_insert_with(NoticeLog::default)
            .note(plan.provider, &done);
    }
}

/// Degrade `plan` for `caps`; returns what changed (no notice is queued).
pub fn apply_caps(plan: &mut PlannedTurn, trust: TrustMode, caps: &Caps) -> Vec<Degrade> {
    degrade_argv(plan.provider, &mut plan.argv, trust, caps)
}

/// The part of a doctor row that explains degradations (empty when nothing is missing).
pub fn degradation_summary(provider: ProviderKind, caps: &Caps) -> String {
    let mut argv = vec![
        "--output-format".to_string(),
        "stream-json".into(),
        "--permission-mode".into(),
        "plan".into(),
        "--mode".into(),
        "plan".into(),
        "--json".into(),
    ];
    degrade_argv(provider, &mut argv, TrustMode::Plan, caps)
        .into_iter()
        .map(|d| d.notice(provider))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const CLAUDE_HELP: &str = "Usage: claude [options]\n  --output-format <format>  Output format: \"text\" (default), or \"stream-json\"\n  --include-partial-messages  Include partial chunks\n  --permission-mode <mode>  Permission mode (choices: \"acceptEdits\", \"plan\")\n  --model <model>  Model\n";
    const CURSOR_HELP: &str = "Usage: agent [options]\n  --output-format <format>  text | json | stream-json\n  --stream-partial-output  Stream deltas\n  --mode <mode>  plan or ask (choices: \"plan\", \"ask\")\n  --sandbox <mode>  sandbox\n";
    const CODEX_EXEC_HELP: &str = "Run Codex non-interactively\n  --json  Print events as JSONL\n  -s, --sandbox <MODE>  sandbox policy\n  -m, --model <MODEL>  model\n";

    #[test]
    fn version_tokens_and_tested_comparison() {
        assert_eq!(
            version_token("2.1.301 (Claude Code)").as_deref(),
            Some("2.1.301")
        );
        assert_eq!(
            version_token("codex-cli 0.163.0").as_deref(),
            Some("0.163.0")
        );
        assert_eq!(
            version_token("2026.10.14-0f9edde").as_deref(),
            Some("2026.10.14-0f9edde")
        );
        assert_eq!(version_token("no version here"), None);
        assert_eq!(
            assess(ProviderKind::Claude, "2.1.301 (Claude Code)"),
            Tested::Same
        );
        assert_eq!(
            assess(ProviderKind::Claude, "2.2.0 (Claude Code)"),
            Tested::Newer
        );
        assert_eq!(assess(ProviderKind::Claude, "2.0.9"), Tested::Older);
        assert_eq!(
            assess(ProviderKind::Codex, "codex-cli 0.163.0"),
            Tested::Same
        );
        assert_eq!(
            assess(ProviderKind::Codex, "codex-cli 0.200.1"),
            Tested::Newer
        );
        // A different build of the same date counts as the tested one.
        assert_eq!(
            assess(ProviderKind::Cursor, "2026.10.14-abcdef0"),
            Tested::Same
        );
        assert_eq!(
            assess(ProviderKind::Cursor, "2026.11.02-abcdef0"),
            Tested::Newer
        );
        assert_eq!(assess(ProviderKind::Cursor, "garbage"), Tested::Unknown);
        assert_eq!(
            assess_against("1.5.0", &["1.0.0", "2.0.0"]),
            Tested::Unknown
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
        let run = fake(&calls, "2.1.301 (Claude Code)\n", CLAUDE_HELP);
        let bin = Path::new("claude");
        let a = detect(ProviderKind::Claude, bin, &run, Some(dir.path())).unwrap();
        assert_eq!(a.tested, Tested::Same);
        assert!(a.caps.stream_json && a.caps.known);
        assert_eq!(calls.borrow().len(), 2, "version and help");
        assert!(dir.path().join("compat.json").is_file());

        calls.borrow_mut().clear();
        let b = detect(ProviderKind::Claude, bin, &run, Some(dir.path())).unwrap();
        assert_eq!(b, a);
        assert_eq!(
            *calls.borrow(),
            vec![vec!["--version".to_string()]],
            "help is cached"
        );

        // A new CLI version is read again.
        calls.borrow_mut().clear();
        let old_help = CLAUDE_HELP.replace("stream-json", "ndjson");
        let run2 = fake(&calls, "2.9.0 (Claude Code)", &old_help);
        let c = detect(ProviderKind::Claude, bin, &run2, Some(dir.path())).unwrap();
        assert_eq!(c.tested, Tested::Newer);
        assert!(!c.caps.stream_json);
        assert_eq!(calls.borrow().len(), 2);
    }

    #[test]
    fn codex_help_is_read_from_exec_and_failures_assume_support() {
        let calls: FakeRun = RefCell::new(Vec::new());
        let run = fake(&calls, "codex-cli 0.163.0", CODEX_EXEC_HELP);
        let c = detect(ProviderKind::Codex, Path::new("codex"), &run, None).unwrap();
        assert!(c.caps.stream_json);
        assert_eq!(calls.borrow()[1], vec!["exec", "--help"]);

        let broken = |_: &Path, a: &[&str]| {
            if a == ["--version"] {
                Ok("2026.10.14-0f9edde".to_string())
            } else {
                Err("timed out".to_string())
            }
        };
        let c = detect(ProviderKind::Cursor, Path::new("agent"), &broken, None).unwrap();
        assert!(!c.caps.known && c.caps.stream_json && c.caps.plan_mode);
        let none = |_: &Path, _: &[&str]| Err("not found".to_string());
        assert!(detect(ProviderKind::Cursor, Path::new("agent"), &none, None).is_none());
    }

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn claude_without_stream_json_or_plan_degrades_and_keeps_the_prompt_last() {
        let mut caps = Caps::assume_all();
        caps.known = true;
        caps.stream_json = false;
        caps.plan_mode = false;
        let mut a = argv(&[
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            "plan",
            "--session-id",
            "x",
            "do it",
        ]);
        let d = degrade_argv(ProviderKind::Claude, &mut a, TrustMode::Plan, &caps);
        assert_eq!(d, vec![Degrade::NoStreamJson, Degrade::NoPlanMode]);
        assert_eq!(
            a,
            argv(&[
                "-p",
                "--output-format",
                "text",
                "--permission-mode",
                "manual",
                "--session-id",
                "x",
                "do it"
            ])
        );
        caps.permission_mode = false;
        let mut b = argv(&["-p", "--permission-mode", "acceptEdits", "hello"]);
        let d = degrade_argv(ProviderKind::Claude, &mut b, TrustMode::Workspace, &caps);
        assert_eq!(d, vec![Degrade::NoPermissionMode]);
        assert_eq!(b, argv(&["-p", "hello"]));
    }

    #[test]
    fn cursor_and_codex_degrade_and_full_support_changes_nothing() {
        let mut caps = Caps::assume_all();
        caps.known = true;
        let original = argv(&[
            "--print",
            "--output-format",
            "stream-json",
            "--mode",
            "plan",
            "p",
        ]);
        let mut same = original.clone();
        assert!(degrade_argv(ProviderKind::Cursor, &mut same, TrustMode::Plan, &caps).is_empty());
        assert_eq!(same, original);

        caps.plan_mode = false;
        caps.stream_json = false;
        let mut a = original.clone();
        let d = degrade_argv(ProviderKind::Cursor, &mut a, TrustMode::Plan, &caps);
        assert_eq!(d, vec![Degrade::NoStreamJson, Degrade::NoPlanMode]);
        assert_eq!(a, argv(&["--print", "--output-format", "text", "p"]));

        let mut c = argv(&["exec", "--json", "-s", "read-only", "p"]);
        let d = degrade_argv(ProviderKind::Codex, &mut c, TrustMode::Plan, &caps);
        assert_eq!(d, vec![Degrade::NoStreamJson]);
        assert_eq!(c, argv(&["exec", "-s", "read-only", "p"]));

        // Unknown capabilities (help unreadable) never degrade.
        let mut u = original.clone();
        let mut unknown = Caps::assume_all();
        unknown.stream_json = false;
        assert!(degrade_argv(ProviderKind::Cursor, &mut u, TrustMode::Plan, &unknown).is_empty());
    }

    #[test]
    fn a_notice_is_queued_once_per_provider_and_degradation() {
        let mut log = NoticeLog::default();
        log.note(ProviderKind::Claude, &[Degrade::NoStreamJson]);
        log.note(
            ProviderKind::Claude,
            &[Degrade::NoStreamJson, Degrade::NoPlanMode],
        );
        log.note(ProviderKind::Cursor, &[Degrade::NoStreamJson]);
        assert_eq!(
            log.take(),
            vec![
                "claude: no stream-json, showing text".to_string(),
                "claude: no plan mode, using ask".to_string(),
                "cursor: no stream-json, showing text".to_string(),
            ]
        );
        assert!(log.take().is_empty());
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
                "2.1.301 (Claude Code)".into()
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
        assert_eq!(c.tested, Tested::Same);
    }
}

#[cfg(test)]
mod planning_tests {
    use super::*;
    use crate::provider::turn::{plan_provider_turn, ProviderTurnRequest};

    #[test]
    fn a_planned_claude_turn_falls_back_to_text_and_ask() {
        let cwd = std::env::temp_dir();
        let req = ProviderTurnRequest {
            provider: ProviderKind::Claude,
            prompt: "hello",
            cwd: &cwd,
            home_path: None,
            model: None,
            native_session_id: None,
            trust: TrustMode::Plan,
            context_prefix: None,
            local_time: None,
        };
        let mut plan = plan_provider_turn(&req).unwrap();
        assert!(plan
            .argv
            .windows(2)
            .any(|w| w == ["--output-format", "stream-json"]));
        let caps = Caps {
            stream_json: false,
            plan_mode: false,
            ..Caps::assume_all()
        };
        let caps = Caps {
            known: true,
            ..caps
        };
        let done = apply_caps(&mut plan, TrustMode::Plan, &caps);
        assert_eq!(done, vec![Degrade::NoStreamJson, Degrade::NoPlanMode]);
        assert!(plan
            .argv
            .windows(2)
            .any(|w| w == ["--output-format", "text"]));
        assert!(plan
            .argv
            .windows(2)
            .any(|w| w == ["--permission-mode", "manual"]));
        assert!(!plan.argv.contains(&"--verbose".to_string()));
        assert_eq!(plan.argv.last().map(String::as_str), Some("hello"));
        assert_eq!(plan.prompt, "hello");
    }
}
