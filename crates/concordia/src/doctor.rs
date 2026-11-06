//! `concordia doctor` / `/doctor`: explain why something is not working, without touching anything.
//!
//! Every probe is injected through [`Env`] so each branch is unit tested. Nothing here reads
//! credential contents; account status uses the same existence/shape check as `/use`.

use crate::accounts::{is_logged_in, list_all_accounts};
use crate::provider::compat;
use crate::provider::launch::is_batch_shim;
use crate::store::{load_store, store_path, ProviderKind};
use crate::version::version_line;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub level: Level,
    pub name: &'static str,
    pub detail: String,
}

/// The one concrete action shown below every warning or failure in the doctor report.
pub fn next_action(row: &Check) -> Option<String> {
    if row.level == Level::Ok {
        return None;
    }
    let action = match row.name {
        "codex" if row.detail.contains("not found") => "npm install -g @openai/codex".into(),
        "claude" if row.detail.contains("not found") => {
            "irm https://claude.ai/install.ps1 | iex".into()
        }
        "cursor" if row.detail.contains("not found") => {
            "irm 'https://cursor.com/install?win32=true' | iex".into()
        }
        "codex" | "claude" | "cursor" if row.detail.contains("not on PATH") => row
            .detail
            .split_once("append with ")
            .map(|(_, action)| action.to_string())
            .unwrap_or_else(|| "append the provider CLI directory to PATH".into()),
        "account" if row.detail.contains("not signed in") || row.detail.contains("missing") => {
            "/use to sign in again (or choose + add account)".into()
        }
        "accounts" => "/use to add an account".into(),
        "path" => "concordia install".into(),
        "wezterm" => "winget install --id wez.wezterm -e".into(),
        "clipboard" => "install or restore powershell.exe on PATH".into(),
        "install" => "re-run scripts\\install-dev.ps1".into(),
        "store" => "move the corrupt store aside, then restart Concordia".into(),
        "state" if row.detail.contains("not owner-only") => "concordia doctor --fix-acl".into(),
        "state" => "choose a writable Concordia state directory".into(),
        "config" => "/config to review the setting".into(),
        "commands" => "rename or remove the conflicting custom command".into(),
        "providers" => "install at least one provider CLI, then retry the operation".into(),
        name if matches!(name, "codex" | "claude" | "cursor")
            && row.detail.contains("(tested: ") =>
        {
            let tested = compat::TESTED
                .iter()
                .find(|(provider, _)| provider.as_str() == name)
                .and_then(|(_, versions)| versions.first())
                .copied()
                .unwrap_or("the verified release");
            format!(
                "works; Concordia was tested with {tested}. Update the CLI if something misbehaves"
            )
        }
        "binary" if row.detail.contains("this process is") => {
            "close this terminal and open a new one".into()
        }
        _ => "review the diagnostic details and address the reported condition".into(),
    };
    Some(action)
}

fn check(level: Level, name: &'static str, detail: impl Into<String>) -> Check {
    Check {
        level,
        name,
        detail: detail.into(),
    }
}

/// A `--version` probe for one CLI.
pub type VersionProbe<'a> = &'a dyn Fn(&Path) -> Result<String, String>;

pub struct Env<'a> {
    pub state_root: PathBuf,
    pub running_exe: Option<PathBuf>,
    pub active_exe: PathBuf,
    /// Raw contents of the install pointer file, if present.
    pub pointer: Option<String>,
    /// Resolve a provider CLI the way a turn would (override env first, then PATH).
    pub provider_bin:
        &'a dyn Fn(ProviderKind) -> Result<crate::provider::Located, crate::provider::NotFound>,
    pub which: &'a dyn Fn(&str) -> Option<PathBuf>,
    pub wezterm_bin: &'a dyn Fn() -> Option<PathBuf>,
    /// `--version` of a CLI; `None` skips probing (fast mode for the TUI).
    pub version_of: Option<VersionProbe<'a>>,
    pub in_wezterm: bool,
    pub source_checkout: Option<PathBuf>,
    /// `owner/name` of the configured release feed, if any.
    pub release_repo: Option<String>,
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

pub fn run(env: &Env<'_>) -> Vec<Check> {
    let mut out = vec![check(Level::Ok, "build", version_line())];
    out.push(check_binary(env));
    out.push(check_pointer(env));
    out.push(check_path_resolution(env));
    out.push(check_state_dir(&env.state_root));
    out.push(check_store(&env.state_root));
    out.push(check_log(&env.state_root));
    out.push(check_config(&env.state_root));
    out.push(check_user_commands(&env.state_root));
    out.extend(check_providers(env));
    out.extend(check_accounts(&env.state_root));
    out.push(check_memory());
    for (warn, name, detail) in
        crate::integration::doctor_rows(&env.state_root, &crate::update::resolve_active_exe())
    {
        out.push(check(
            if warn { Level::Warn } else { Level::Ok },
            name,
            detail,
        ));
    }
    out.push(match (env.wezterm_bin)() {
        Some(p) => check(
            Level::Ok,
            "wezterm",
            format!(
                "{}{}",
                p.display(),
                if env.in_wezterm {
                    " (this pane is in WezTerm)"
                } else {
                    ""
                }
            ),
        ),
        None => check(
            Level::Warn,
            "wezterm",
            "not found; /split and login windows fall back to a plain PowerShell console",
        ),
    });
    out.push(match (env.which)("powershell.exe") {
        Some(_) => check(
            Level::Ok,
            "clipboard",
            "powershell.exe found (copy/paste available)",
        ),
        None => check(
            Level::Warn,
            "clipboard",
            "powershell.exe not on PATH; copy and paste will not work",
        ),
    });
    out.push(match &env.source_checkout {
        Some(p) => check(
            Level::Ok,
            "update",
            format!(
                "/update rebuilds from the source checkout at {}",
                p.display()
            ),
        ),
        None => match &env.release_repo {
            Some(repo) => check(
                Level::Ok,
                "update",
                format!("/update downloads newer releases from github.com/{repo}"),
            ),
            None => check(
                Level::Warn,
                "update",
                "no source checkout and no release feed (set CONCORDIA_UPDATE_REPO=owner/name); /update has nothing to update from",
            ),
        },
    });
    out
}

fn check_binary(env: &Env<'_>) -> Check {
    let Some(running) = &env.running_exe else {
        return check(
            Level::Warn,
            "binary",
            "could not determine the running executable",
        );
    };
    if same_file(running, &env.active_exe) {
        check(
            Level::Ok,
            "binary",
            format!("{} (the active build)", running.display()),
        )
    } else {
        check(
            Level::Warn,
            "binary",
            format!(
                "this process is {} but launchers use {}; close old panes and open a new tab",
                running.display(),
                env.active_exe.display()
            ),
        )
    }
}

/// What typing `concordia` actually runs. A stale copy earlier on PATH than the active build is the
/// classic reason a new version "does not show up".
fn check_path_resolution(env: &Env<'_>) -> Check {
    let Some(found) = (env.which)("concordia") else {
        return check(
            Level::Warn,
            "path",
            "`concordia` is not on PATH; run `concordia install`, then open a new terminal",
        );
    };
    if same_file(&found, &env.active_exe) {
        return check(
            Level::Ok,
            "path",
            format!("typing `concordia` runs {}", found.display()),
        );
    }
    let version = env
        .version_of
        .and_then(|probe| probe(&found).ok())
        .map(|v| format!(" ({v})"))
        .unwrap_or_default();
    check(
        Level::Warn,
        "path",
        format!(
            "typing `concordia` runs {}{version}, not the active build {}; run `concordia install` and open a new terminal",
            found.display(),
            env.active_exe.display()
        ),
    )
}

fn check_pointer(env: &Env<'_>) -> Check {
    match &env.pointer {
        None => check(
            Level::Warn,
            "install",
            "no install pointer yet; run scripts\\install-dev.ps1 so every launcher finds the same build",
        ),
        Some(raw) => {
            let target = PathBuf::from(raw.trim());
            if target.is_file() {
                check(Level::Ok, "install", format!("pointer -> {}", target.display()))
            } else {
                check(
                    Level::Fail,
                    "install",
                    format!(
                        "pointer -> {} but that file is missing; re-run scripts\\install-dev.ps1",
                        target.display()
                    ),
                )
            }
        }
    }
}

fn check_state_dir(root: &Path) -> Check {
    if let Err(e) = std::fs::create_dir_all(root) {
        return check(
            Level::Fail,
            "state",
            format!("cannot create {}: {e}", root.display()),
        );
    }
    let probe = root.join(format!(".doctor-{}", std::process::id()));
    let warning = crate::store::outside_profile_warning(root);
    let acl_status = crate::state_security::is_owner_only(root);
    let acl_warning = acl_status.as_ref().ok().filter(|ok| !**ok);
    let acl_unknown = acl_status.is_err();
    match std::fs::write(&probe, b"ok").and_then(|_| std::fs::remove_file(&probe)) {
        Ok(()) => check(
            if warning.is_some() || acl_warning.is_some() || acl_unknown { Level::Warn } else { Level::Ok },
            "state",
            warning.unwrap_or_else(|| if acl_unknown {
                format!("{}: could not read permissions", root.display())
            } else if acl_warning.is_some() {
                format!("{} is writable; state directory is not owner-only; run icacls \"{}\" /inheritance:r /grant:r \"%USERNAME%\":F SYSTEM:F", root.display(), root.display())
            } else { format!("{} is writable", root.display()) }),
        ),
        Err(e) => check(
            Level::Fail,
            "state",
            format!("{} is not writable: {e}", root.display()),
        ),
    }
}

/// Where the event log is, so a crash report has somewhere to point.
fn check_log(root: &Path) -> Check {
    let path = crate::logging::log_path(root);
    match std::fs::metadata(&path) {
        Ok(m) => check(
            Level::Ok,
            "log",
            format!("{} ({} KB)", path.display(), m.len().div_ceil(1024)),
        ),
        Err(_) => check(
            Level::Ok,
            "log",
            format!("{} (nothing logged yet)", path.display()),
        ),
    }
}

/// `<state>/config.toml`: never a failure (a bad file falls back to defaults), but a warning row.
fn check_config(root: &Path) -> Check {
    let l = crate::config::Config::load(root);
    if !l.exists {
        return check(
            Level::Ok,
            "config",
            format!("{} not present (defaults)", l.path.display()),
        );
    }
    match l.warnings.first() {
        None => check(
            Level::Ok,
            "config",
            format!("{} (0 warnings)", l.path.display()),
        ),
        Some(first) => check(
            Level::Warn,
            "config",
            format!(
                "{} ({} warning(s)): {first}",
                l.path.display(),
                l.warnings.len()
            ),
        ),
    }
}

/// `<state>/commands/*.md`: count, plus a warning for skipped files and built-in name clashes.
fn check_user_commands(root: &Path) -> Check {
    let s = crate::user_commands::scan(root);
    let mut detail = format!("{} custom command(s)", s.commands.len());
    let mut level = Level::Ok;
    if !s.clashes.is_empty() {
        level = Level::Warn;
        detail.push_str(&format!(
            "; {} clash(es) with built-in commands (built-in wins): {}",
            s.clashes.len(),
            s.clashes
                .iter()
                .map(|n| format!("/{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(first) = s.warnings.first() {
        level = Level::Warn;
        detail.push_str(&format!("; {} skipped: {first}", s.warnings.len()));
    }
    check(level, "commands", detail)
}

fn check_store(root: &Path) -> Check {
    let path = store_path(root);
    match std::fs::read_to_string(&path) {
        Err(_) => check(Level::Ok, "store", "no store yet (fresh install)"),
        Ok(raw) => match serde_json::from_str::<crate::store::OrchestratorStore>(&raw) {
            Ok(_) => {
                let s = load_store(root);
                check(
                    Level::Ok,
                    "store",
                    format!(
                        "{} account(s) registered, trust {}{}",
                        s.accounts.len(),
                        s.settings.trust.as_str(),
                        if s.settings.trust == crate::store::TrustMode::Full {
                            " (provider approvals and sandboxing are bypassed; /trust to change)"
                        } else {
                            ""
                        }
                    ),
                )
            }
            Err(e) => check(
                Level::Fail,
                "store",
                format!(
                    "{} is unreadable ({e}); concordia starts fresh and keeps a .corrupt copy on the next save",
                    path.display()
                ),
            ),
        },
    }
}

fn check_providers(env: &Env<'_>) -> Vec<Check> {
    let mut rows = Vec::new();
    let mut found = 0;
    for (name, kind) in [
        ("codex", ProviderKind::Codex),
        ("claude", ProviderKind::Claude),
        ("cursor", ProviderKind::Cursor),
    ] {
        match (env.provider_bin)(kind) {
            Err(missing) => rows.push(check(
                Level::Warn,
                name_static(name),
                format!(
                    "{name} not found; looked in: {}; install it or set {}",
                    missing
                        .searched
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    provider_variable(kind)
                ),
            )),
            Ok(located) => {
                let path = &located.path;
                found += 1;
                let mut level = if located.source == crate::provider::LocateSource::Fallback {
                    Level::Warn
                } else {
                    Level::Ok
                };
                let version = match env.version_of {
                    Some(probe) => match probe(path) {
                        Ok(v) => {
                            // Newer, older or unknown is a warning, never a failure.
                            let t = compat::assess(kind, &v);
                            if t != compat::Tested::Same {
                                level = Level::Warn;
                            }
                            let degraded = compat::caps_for(kind)
                                .map(|c| compat::degradation_summary(kind, &c))
                                .filter(|s| !s.is_empty())
                                .map(|s| format!(" · {s}"))
                                .unwrap_or_default();
                            format!(" · {v} (tested: {}){degraded}", t.label())
                        }
                        Err(e) => format!(" · version check failed: {e}"),
                    },
                    None => String::new(),
                };
                let shim = if is_batch_shim(&path.to_string_lossy()) {
                    " · .cmd shim (multi-line and special-character prompts go over stdin)"
                } else {
                    ""
                };
                rows.push(check(
                    level,
                    name_static(name),
                    format_provider_cli(name, &Ok(located.clone())) + &format!("{version}{shim}"),
                ));
            }
        }
    }
    if found == 0 {
        rows.push(check(
            Level::Fail,
            "providers",
            "no provider CLI found; install codex, claude or agent",
        ));
    }
    rows
}

fn provider_variable(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Codex => "CONCORDIA_CODEX_BIN",
        ProviderKind::Claude => "CONCORDIA_CLAUDE_BIN",
        ProviderKind::Cursor => "CONCORDIA_AGENT_BIN",
    }
}

/// Idle memory of this process (working set), as a row.
fn check_memory() -> Check {
    match crate::sysstats::self_memory_bytes() {
        Some(b) => check(
            Level::Ok,
            "memory",
            format!("{} in use by this process", crate::sysstats::format_mem(b)),
        ),
        None => check(Level::Ok, "memory", "not available on this platform"),
    }
}

fn name_static(n: &str) -> &'static str {
    match n {
        "codex" => "codex",
        "claude" => "claude",
        _ => "cursor",
    }
}

fn check_accounts(root: &Path) -> Vec<Check> {
    let mut rows = Vec::new();
    let mut ready = 0;
    for a in list_all_accounts(root) {
        let Some(home) = &a.home_path else { continue };
        let ok = is_logged_in(a.provider, home);
        if ok {
            ready += 1;
            rows.push(check(
                Level::Ok,
                "account",
                format!("{} ({}) logged in · {}", a.label, a.account, home.display()),
            ));
        } else if a.custom {
            // An concordia-created account is the user's intent; say why it is not usable yet.
            let why = if home.is_dir() {
                "not signed in yet"
            } else {
                "home folder is missing"
            };
            rows.push(check(
                Level::Warn,
                "account",
                format!("{} ({}) {why} · {}", a.label, a.account, home.display()),
            ));
        }
    }
    for group in crate::accounts::duplicate_groups(root) {
        let names: Vec<_> = group.iter().map(|a| a.label.as_str()).collect();
        rows.push(check(
            Level::Warn,
            "duplicate",
            format!(
                "{} are the same login; /use remove one of them",
                names.join(" and ")
            ),
        ));
    }
    if ready == 0 {
        rows.push(check(
            Level::Warn,
            "accounts",
            "no logged-in accounts; type /use add to connect one",
        ));
    }
    rows
}

pub fn format_report(checks: &[Check]) -> String {
    let mut lines = vec![format!("concordia doctor · {}", version_line())];
    for c in checks {
        let tag = match c.level {
            Level::Ok => "ok  ",
            Level::Warn => "warn",
            Level::Fail => "FAIL",
        };
        lines.push(format!("{tag}  {:<9} {}", c.name, c.detail));
        if let Some(action) = next_action(c) {
            lines.push(format!("          next: {action}"));
        }
    }
    let (w, f) = summary(checks);
    lines.push(if f > 0 {
        format!("{f} problem(s), {w} warning(s)")
    } else if w > 0 {
        format!("no problems, {w} warning(s)")
    } else {
        "all good".to_string()
    });
    lines.join("\n")
}

/// Fast, local provider health used by the first-launch panel. It resolves the same binaries as
/// doctor but deliberately does not run `--version` or contact a provider.
pub fn fast_provider_status() -> Vec<(ProviderKind, bool)> {
    [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Cursor,
    ]
    .into_iter()
    .map(|kind| (kind, resolve_provider_bin(kind).is_ok()))
    .collect()
}

pub fn summary(checks: &[Check]) -> (usize, usize) {
    let w = checks.iter().filter(|c| c.level == Level::Warn).count();
    let f = checks.iter().filter(|c| c.level == Level::Fail).count();
    (w, f)
}

/// The real environment. `probe_versions` runs each CLI's `--version` (about a second each).
pub fn real_env<'a>(
    provider_bin: &'a dyn Fn(
        ProviderKind,
    ) -> Result<crate::provider::Located, crate::provider::NotFound>,
    which: &'a dyn Fn(&str) -> Option<PathBuf>,
    version_of: Option<VersionProbe<'a>>,
) -> Env<'a> {
    let cwd = std::env::current_dir().unwrap_or_default();
    Env {
        state_root: crate::store::default_store_root(),
        running_exe: std::env::current_exe().ok(),
        active_exe: crate::update::resolve_active_exe(),
        pointer: std::fs::read_to_string(crate::update::current_pointer_path()).ok(),
        provider_bin,
        which,
        wezterm_bin: &|| crate::host::wezterm::find_wezterm(),
        version_of,
        in_wezterm: std::env::var_os("WEZTERM_PANE").is_some(),
        source_checkout: crate::update::source_root(&cwd),
        release_repo: crate::release::config_from_env(crate::version::VERSION).map(|c| c.repo),
    }
}

/// Resolve a provider CLI exactly as a turn would: the `CONCORDIA_*_BIN` override, else PATH.
pub fn resolve_provider_bin(
    kind: ProviderKind,
) -> Result<crate::provider::Located, crate::provider::NotFound> {
    crate::provider::locate_real(kind)
}

/// Format one provider CLI lookup without touching the environment or probing a process.
pub fn format_provider_cli(
    name: &str,
    result: &Result<crate::provider::Located, crate::provider::NotFound>,
) -> String {
    match result {
        Ok(located) => {
            let source = match located.source {
                crate::provider::LocateSource::Override => "override",
                crate::provider::LocateSource::Path => "PATH",
                crate::provider::LocateSource::Fallback => "standard location",
            };
            let mut line = format!("{name}: {} ({source})", located.path.display());
            if located.source == crate::provider::LocateSource::Fallback {
                if let Some(dir) = located.path.parent() {
                    line.push_str(&format!("; not on PATH; append with [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path','User') + ';{}', 'User')", dir.display()));
                }
            }
            line
        }
        Err(missing) => format!(
            "{name}: missing; searched: {}",
            missing
                .searched
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

pub fn format_not_found_turn_error(
    kind: ProviderKind,
    missing: &crate::provider::NotFound,
) -> String {
    let name = match kind {
        ProviderKind::Codex => "Codex",
        ProviderKind::Claude => "Claude Code",
        ProviderKind::Cursor => "Cursor agent",
    };
    format!(
        "{name} CLI was not found. Concordia looked in: {}. Run `concordia doctor`.",
        missing
            .searched
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// `<program> --version` with a deadline, first non-empty line.
pub fn probe_version(program: &Path) -> Result<String, String> {
    use std::io::Read;
    use std::process::Stdio;
    let native = crate::provider::npm::codex(&program.to_string_lossy());
    let mut command =
        crate::process_ui::command(native.as_ref().map_or(program, |n| n.program.as_path()));
    if let Some(native) = native {
        command.envs(native.env);
    }
    crate::process_ui::background_std(&mut command);
    let mut child = command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(25))
            }
            Ok(None) => {
                let _ = child.kill();
                return Err("timed out".into());
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut text = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut text);
    }
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "no output".into())
}

/// Run the three version probes in parallel and return the report. For the CLI.
pub fn run_cli_report() -> (String, bool) {
    let provider_bin = resolve_provider_bin;
    let which = |n: &str| which::which(n).ok();
    // Probe once per provider concurrently so the slowest CLI sets the pace, not the sum.
    let probes: Vec<_> = [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Cursor,
    ]
    .into_iter()
    .filter_map(|k| resolve_provider_bin(k).ok().map(|l| l.path))
    // Also probe whatever typing `concordia` finds, so a stale copy can be named with its version.
    .chain(which::which("concordia").ok())
    .map(|p| {
        let q = p.clone();
        (p, std::thread::spawn(move || probe_version(&q)))
    })
    .collect();
    let results: Vec<(PathBuf, Result<String, String>)> = probes
        .into_iter()
        .map(|(p, h)| (p, h.join().unwrap_or_else(|_| Err("probe panicked".into()))))
        .collect();
    // Doctor is read-only: use existing caches, but do not create or refresh Concordia state.
    let detections: Vec<_> = [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Cursor,
    ]
    .into_iter()
    .filter_map(|k| resolve_provider_bin(k).ok().map(|l| (k, l.path)))
    .map(|(k, p)| {
        let state_root = crate::store::default_store_root();
        let h = std::thread::spawn(move || {
            compat::detect_with_mode(
                k,
                &p,
                &compat::run_cli,
                Some(&state_root),
                compat::CacheMode::ReadOnly,
            )
        });
        (k, h)
    })
    .collect();
    for (k, h) in detections {
        if let Ok(Some(c)) = h.join() {
            compat::set_compat(k, &c);
        }
    }
    let version_of = |p: &Path| {
        results
            .iter()
            .find(|(q, _)| q == p)
            .map(|(_, r)| r.clone())
            .unwrap_or_else(|| Err("not probed".into()))
    };
    let checks = run(&real_env(&provider_bin, &which, Some(&version_of)));
    let (_, fails) = summary(&checks);
    (format_report(&checks), fails == 0)
}

/// Fast report for the TUI (no subprocess probes, so it cannot stall the event loop).
pub fn run_fast_report() -> String {
    let provider_bin = resolve_provider_bin;
    let which = |n: &str| which::which(n).ok();
    format_report(&run(&real_env(&provider_bin, &which, None)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoredAccount;
    use std::fs;

    struct Fx {
        _guard: std::sync::MutexGuard<'static, ()>,
        base: tempfile::TempDir,
        state: tempfile::TempDir,
        exe: tempfile::TempDir,
    }

    impl Fx {
        fn new() -> Self {
            let guard = crate::accounts::TEST_ENV
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let base = tempfile::tempdir().unwrap();
            std::env::set_var("CONCORDIA_CODEX_HOME_BASE", base.path());
            for k in [
                "AO_CODEX_HOME",
                "AO_CODEX_HOME_A",
                "AO_CODEX_HOME_2",
                "AO_CODEX_HOME_B",
                "AO_CLAUDE_CONFIG_DIR",
                "AO_CURSOR_HOME",
            ] {
                std::env::remove_var(k);
            }
            Fx {
                _guard: guard,
                base,
                state: tempfile::tempdir().unwrap(),
                exe: tempfile::tempdir().unwrap(),
            }
        }
        fn exe_file(&self, name: &str) -> PathBuf {
            let p = self.exe.path().join(name);
            fs::write(&p, "").unwrap();
            p
        }
    }

    impl Drop for Fx {
        fn drop(&mut self) {
            std::env::remove_var("CONCORDIA_CODEX_HOME_BASE");
        }
    }

    fn find<'a>(checks: &'a [Check], name: &str) -> Vec<&'a Check> {
        checks.iter().filter(|c| c.name == name).collect()
    }

    fn env<'a>(
        fx: &Fx,
        provider_bin: &'a dyn Fn(
            ProviderKind,
        )
            -> Result<crate::provider::Located, crate::provider::NotFound>,
        which: &'a dyn Fn(&str) -> Option<PathBuf>,
    ) -> Env<'a> {
        let active = fx.exe_file("concordia-a.exe");
        Env {
            state_root: fx.state.path().to_path_buf(),
            running_exe: Some(active.clone()),
            pointer: Some(active.to_string_lossy().into_owned()),
            active_exe: active,
            provider_bin,
            which,
            wezterm_bin: &|| None,
            version_of: None,
            in_wezterm: false,
            source_checkout: None,
            release_repo: None,
        }
    }

    #[test]
    fn a_healthy_machine_reports_ok_and_warns_only_about_optional_things() {
        let fx = Fx::new();
        let codex = fx.exe_file("codex.exe");
        fs::create_dir_all(fx.base.path().join(".codex")).unwrap();
        fs::write(fx.base.path().join(".codex").join("auth.json"), "{}").unwrap();
        let pb = move |k: ProviderKind| {
            if k == ProviderKind::Codex {
                {
                    Ok(crate::provider::Located {
                        path: codex.clone(),
                        source: crate::provider::LocateSource::Override,
                    })
                }
            } else {
                Err(crate::provider::NotFound { searched: vec![] })
            }
        };
        let wz = fx.exe_file("wezterm.exe");
        let ps = fx.exe_file("powershell.exe");
        let which = move |n: &str| match n {
            "wezterm" => Some(wz.clone()),
            "powershell.exe" => Some(ps.clone()),
            _ => None,
        };
        let mut e = env(&fx, &pb, &which);
        e.source_checkout = Some(PathBuf::from("C:\\src\\concordia"));
        let checks = run(&e);
        assert_eq!(summary(&checks).1, 0, "{}", format_report(&checks));
        assert!(find(&checks, "binary")[0].detail.contains("active build"));
        assert_eq!(find(&checks, "codex")[0].level, Level::Ok);
        // claude/cursor are simply not installed: a warning, not a failure.
        assert_eq!(find(&checks, "claude")[0].level, Level::Warn);
        assert!(find(&checks, "account")
            .iter()
            .any(|c| c.detail.contains("logged in")));
    }

    #[test]
    fn config_and_commands_rows_warn_but_never_fail() {
        let fx = Fx::new();
        let _cfg = crate::config::env_guard();
        let pb = |_: ProviderKind| Err(crate::provider::NotFound { searched: vec![] });
        let which = |_: &str| None;
        let checks = run(&env(&fx, &pb, &which));
        assert!(find(&checks, "config")[0].detail.contains("defaults"));
        assert_eq!(find(&checks, "config")[0].level, Level::Ok);
        assert!(find(&checks, "commands")[0].detail.starts_with("0 custom"));

        fs::write(fx.state.path().join("config.toml"), "[mode\n").unwrap();
        let cmds = fx.state.path().join("commands");
        fs::create_dir_all(&cmds).unwrap();
        fs::write(cmds.join("doctor.md"), "x").unwrap();
        fs::write(cmds.join("ok.md"), "y").unwrap();
        let before = summary(&checks).1;
        let checks = run(&env(&fx, &pb, &which));
        let c = find(&checks, "config")[0];
        assert_eq!(c.level, Level::Warn);
        assert!(c.detail.contains("line 1") && c.detail.contains("1 warning"));
        let k = find(&checks, "commands")[0];
        assert_eq!(k.level, Level::Warn);
        assert!(k.detail.contains("1 custom") && k.detail.contains("/doctor"));
        assert_eq!(summary(&checks).1, before, "warnings must not add failures");
    }

    #[test]
    fn no_provider_cli_at_all_is_a_failure() {
        let fx = Fx::new();
        let pb = |_: ProviderKind| Err(crate::provider::NotFound { searched: vec![] });
        let which = |_: &str| None;
        let checks = run(&env(&fx, &pb, &which));
        assert_eq!(find(&checks, "providers")[0].level, Level::Fail);
        assert!(summary(&checks).1 >= 1);
    }

    #[test]
    fn a_stale_pane_is_called_out_with_the_fix() {
        let fx = Fx::new();
        let pb = |_: ProviderKind| Err(crate::provider::NotFound { searched: vec![] });
        let which = |_: &str| None;
        let mut e = env(&fx, &pb, &which);
        e.running_exe = Some(fx.exe_file("concordia-old.exe"));
        let checks = run(&e);
        let b = find(&checks, "binary")[0];
        assert_eq!(b.level, Level::Warn);
        assert!(b.detail.contains("open a new tab"));
    }

    #[test]
    fn a_dangling_install_pointer_is_a_failure_and_a_missing_one_a_warning() {
        let fx = Fx::new();
        let pb = |_: ProviderKind| Err(crate::provider::NotFound { searched: vec![] });
        let which = |_: &str| None;
        let mut e = env(&fx, &pb, &which);
        e.pointer = Some("C:\\definitely\\missing\\concordia.exe".into());
        assert_eq!(find(&run(&e), "install")[0].level, Level::Fail);
        e.pointer = None;
        assert_eq!(find(&run(&e), "install")[0].level, Level::Warn);
    }

    #[test]
    fn a_corrupt_store_is_reported_and_a_good_one_summarised() {
        let fx = Fx::new();
        let pb = |_: ProviderKind| Err(crate::provider::NotFound { searched: vec![] });
        let which = |_: &str| None;
        fs::create_dir_all(crate::store::store_dir(fx.state.path())).unwrap();
        fs::write(store_path(fx.state.path()), "{ nope").unwrap();
        let c = run(&env(&fx, &pb, &which));
        assert_eq!(find(&c, "store")[0].level, Level::Fail);
        assert!(find(&c, "store")[0].detail.contains("unreadable"));

        fs::write(store_path(fx.state.path()), "{\"version\":1}").unwrap();
        let c = run(&env(&fx, &pb, &which));
        assert_eq!(find(&c, "store")[0].level, Level::Ok);
    }

    #[test]
    fn an_unfinished_concordia_account_says_why_it_is_not_usable() {
        let fx = Fx::new();
        let pb = |_: ProviderKind| Err(crate::provider::NotFound { searched: vec![] });
        let which = |_: &str| None;
        let home = fx.state.path().join("homes").join("codex-3");
        fs::create_dir_all(&home).unwrap();
        crate::store::upsert_stored_account(
            fx.state.path(),
            StoredAccount {
                id: "codex-3".into(),
                provider: ProviderKind::Codex,
                home_path: Some(home.to_string_lossy().into_owned()),
                label: "Codex 3".into(),
                created_at: "0".into(),
            },
        )
        .unwrap();
        let c = run(&env(&fx, &pb, &which));
        assert!(find(&c, "account")
            .iter()
            .any(|c| c.detail.contains("not signed in yet")));
        assert_eq!(find(&c, "accounts")[0].level, Level::Warn); // none logged in
        fs::remove_dir_all(&home).unwrap();
        let c = run(&env(&fx, &pb, &which));
        assert!(find(&c, "account")
            .iter()
            .any(|c| c.detail.contains("home folder is missing")));
    }

    #[test]
    fn provider_rows_show_versions_and_flag_cmd_shims() {
        let fx = Fx::new();
        let shim = fx.exe_file("codex.cmd");
        let pb = move |k: ProviderKind| {
            if k == ProviderKind::Codex {
                {
                    Ok(crate::provider::Located {
                        path: shim.clone(),
                        source: crate::provider::LocateSource::Override,
                    })
                }
            } else {
                Err(crate::provider::NotFound { searched: vec![] })
            }
        };
        let which = |_: &str| None;
        let probe = |_: &Path| Ok("codex-cli 9.9.9".to_string());
        let mut e = env(&fx, &pb, &which);
        e.version_of = Some(&probe);
        let c = run(&e);
        let d = &find(&c, "codex")[0].detail;
        assert!(d.contains("codex-cli 9.9.9"), "{d}");
        assert!(d.contains(".cmd shim"), "{d}");

        let failing = |_: &Path| Err("timed out".to_string());
        e.version_of = Some(&failing);
        assert!(find(&run(&e), "codex")[0]
            .detail
            .contains("version check failed: timed out"));
    }

    #[test]
    fn doctor_check_providers_warns_and_shows_path_fix_for_fallback() {
        let fx = Fx::new();
        let folder = fx.exe_file("fallback").parent().unwrap().to_path_buf();
        let binary = folder.join("codex.exe");
        fs::write(&binary, "").unwrap();
        let path = binary.clone();
        let pb = move |k: ProviderKind| {
            if k == ProviderKind::Codex {
                Ok(crate::provider::Located {
                    path: path.clone(),
                    source: crate::provider::LocateSource::Fallback,
                })
            } else {
                Err(crate::provider::NotFound { searched: vec![] })
            }
        };
        let which = |_: &str| None;
        let checks = run(&env(&fx, &pb, &which));
        let row = find(&checks, "codex")[0];
        assert_eq!(row.level, Level::Warn);
        assert!(row.detail.contains("not on PATH"));
        assert!(row.detail.contains("SetEnvironmentVariable"));
        assert!(row.detail.contains(&folder.display().to_string()));
    }

    #[test]
    fn version_rows_say_tested_same_and_warn_when_it_differs() {
        let fx = Fx::new();
        let exe = fx.exe_file("claude.exe");
        let pb = move |k: ProviderKind| {
            if k == ProviderKind::Claude {
                {
                    Ok(crate::provider::Located {
                        path: exe.clone(),
                        source: crate::provider::LocateSource::Override,
                    })
                }
            } else {
                Err(crate::provider::NotFound { searched: vec![] })
            }
        };
        let which = |_: &str| None;
        let same = |_: &Path| Ok("2.1.301 (Claude Code)".to_string());
        let mut e = env(&fx, &pb, &which);
        e.version_of = Some(&same);
        let row = &find(&run(&e), "claude")[0].clone();
        assert!(
            row.detail.contains("2.1.301 (Claude Code) (tested: same)"),
            "{}",
            row.detail
        );
        assert_eq!(row.level, Level::Ok);

        let newer = |_: &Path| Ok("9.0.0 (Claude Code)".to_string());
        e.version_of = Some(&newer);
        let row = &find(&run(&e), "claude")[0].clone();
        assert!(row.detail.contains("(tested: newer)"), "{}", row.detail);
        assert_eq!(row.level, Level::Warn);

        let odd = |_: &Path| Ok("something odd".to_string());
        e.version_of = Some(&odd);
        let row = &find(&run(&e), "claude")[0].clone();
        assert!(row.detail.contains("(tested: unknown)"));
        assert_eq!(row.level, Level::Warn, "unknown versions warn, never fail");
    }

    #[cfg(windows)]
    #[test]
    fn the_report_includes_idle_memory() {
        let fx = Fx::new();
        let pb = |_: ProviderKind| Err(crate::provider::NotFound { searched: vec![] });
        let which = |_: &str| None;
        let c = run(&env(&fx, &pb, &which));
        let m = &find(&c, "memory")[0];
        assert_eq!(m.level, Level::Ok);
        assert!(m.detail.contains(" MB"), "{}", m.detail);
    }

    #[test]
    fn the_update_row_explains_which_update_mode_applies() {
        let fx = Fx::new();
        let pb = |_: ProviderKind| Err(crate::provider::NotFound { searched: vec![] });
        let which = |_: &str| None;
        let mut e = env(&fx, &pb, &which);
        // Neither a checkout nor a feed: a warning with the fix.
        let row = |e: &Env<'_>| find(&run(e), "update")[0].clone();
        let r = row(&e);
        assert_eq!(r.level, Level::Warn);
        assert!(r.detail.contains("CONCORDIA_UPDATE_REPO"), "{}", r.detail);
        // A release feed makes /update meaningful.
        e.release_repo = Some("me/concordia".into());
        let r = row(&e);
        assert_eq!(r.level, Level::Ok);
        assert!(r.detail.contains("github.com/me/concordia"), "{}", r.detail);
        // A source checkout wins.
        e.source_checkout = Some(PathBuf::from(r"C:\src\concordia"));
        assert!(row(&e).detail.contains("rebuilds from the source checkout"));
    }

    #[test]
    fn typing_concordia_is_checked_against_the_active_build() {
        let fx = Fx::new();
        let pb = |_: ProviderKind| Err(crate::provider::NotFound { searched: vec![] });
        let active = fx.exe_file("concordia-a.exe");

        // Resolves to the active build: fine.
        let a = active.clone();
        let which_ok = move |n: &str| (n == "concordia").then(|| a.clone());
        let c = run(&env(&fx, &pb, &which_ok));
        assert_eq!(find(&c, "path")[0].level, Level::Ok);

        // Resolves to a different, stale file: a warning naming both and the fix.
        let stale = fx.exe_file("stale-concordia.exe");
        let s2 = stale.clone();
        let which_stale = move |n: &str| (n == "concordia").then(|| s2.clone());
        let probe = |_: &Path| Ok("concordia 0.0.0 (ancient)".to_string());
        let mut e = env(&fx, &pb, &which_stale);
        e.version_of = Some(&probe);
        let c = run(&e);
        let row = find(&c, "path")[0];
        assert_eq!(row.level, Level::Warn);
        assert!(row.detail.contains("stale-concordia.exe"), "{}", row.detail);
        assert!(
            row.detail.contains("concordia 0.0.0 (ancient)"),
            "{}",
            row.detail
        );
        assert!(row.detail.contains("concordia install"), "{}", row.detail);

        // Not on PATH at all.
        let none = |_: &str| None;
        let c = run(&env(&fx, &pb, &none));
        assert_eq!(find(&c, "path")[0].level, Level::Warn);
        assert!(find(&c, "path")[0].detail.contains("not on PATH"));
    }

    #[test]
    fn the_report_is_readable_and_summarises() {
        let checks = vec![
            check(Level::Ok, "build", "concordia"),
            check(Level::Warn, "wezterm", "missing"),
            check(Level::Fail, "install", "broken"),
        ];
        let r = format_report(&checks);
        assert!(r.contains("ok    build"));
        assert!(r.contains("warn  wezterm"));
        assert!(r.contains("FAIL  install"));
        assert!(r.ends_with("1 problem(s), 1 warning(s)"));
        assert!(format_report(&checks[..1]).ends_with("all good"));
        assert!(format_report(&checks[..2]).ends_with("no problems, 1 warning(s)"));
    }

    #[test]
    fn doctor_rows_have_one_next_action_each() {
        let rows = [
            check(Level::Warn, "codex", "not found"),
            check(Level::Warn, "claude", "not found"),
            check(Level::Warn, "cursor", "not found"),
            check(
                Level::Warn,
                "codex",
                "not on PATH; append with PATH command",
            ),
            check(Level::Warn, "account", "not signed in"),
            check(Level::Warn, "accounts", "none"),
            check(Level::Warn, "path", "not on PATH"),
            check(Level::Warn, "wezterm", "missing"),
            check(Level::Fail, "install", "broken"),
            check(Level::Warn, "claude", "claude: installed (tested: older)"),
            check(
                Level::Warn,
                "binary",
                "this process is old but launchers use new; close old panes and open a new tab",
            ),
        ];
        for row in rows {
            let action = next_action(&row).expect("missing action");
            assert!(!action.contains("concordia doctor"), "{action}");
            let rendered = format_report(std::slice::from_ref(&row));
            assert_eq!(rendered.matches("next: ").count(), 1, "{rendered}");
        }
        assert!(next_action(&check(Level::Ok, "build", "fine")).is_none());
    }

    #[test]
    fn probe_version_reads_the_first_line_and_times_out_cleanly() {
        // The test harness itself answers `--version`.
        let exe = std::env::current_exe().unwrap();
        // cargo's libtest binary prints nothing useful for --version, so just require it not to hang.
        let _ = probe_version(&exe);
        assert!(probe_version(Path::new("C:\\definitely\\not\\here.exe")).is_err());
    }

    #[test]
    fn doctor_cli_lines_cover_path_fallback_and_missing() {
        let path = PathBuf::from("C:\\bin\\claude.exe");
        let line = format_provider_cli(
            "Claude Code",
            &Ok(crate::provider::Located {
                path: path.clone(),
                source: crate::provider::LocateSource::Path,
            }),
        );
        assert!(line.contains("PATH"));
        let line = format_provider_cli(
            "Claude Code",
            &Ok(crate::provider::Located {
                path: path.clone(),
                source: crate::provider::LocateSource::Fallback,
            }),
        );
        assert!(
            line.contains("not on PATH")
                && line.contains("SetEnvironmentVariable")
                && line.contains("C:\\bin")
        );
        let line = format_provider_cli(
            "Claude Code",
            &Err(crate::provider::NotFound {
                searched: vec![PathBuf::from("C:\\one"), PathBuf::from("C:\\two")],
            }),
        );
        assert!(line.contains("missing") && line.contains("C:\\one") && line.contains("C:\\two"));
    }

    #[test]
    fn not_found_turn_error_lists_locations_and_points_to_doctor() {
        let error = format_not_found_turn_error(
            ProviderKind::Cursor,
            &crate::provider::NotFound {
                searched: vec![PathBuf::from("C:\\cursor")],
            },
        );
        assert!(
            error.contains("Cursor agent")
                && error.contains("C:\\cursor")
                && error.contains("concordia doctor")
        );
    }
}
