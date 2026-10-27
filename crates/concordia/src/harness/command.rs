//! The `/harness` command: connect, verify, status, disconnect and skill, as plain functions
//! that return the lines to show. Running processes is injected, so every path is testable.

use super::connections::{self, Entry, Identity};
use super::verify::{self, Verdict};
use super::{expand, parse_manifest, resolve_program, Context, Manifest, RunOutcome};
use crate::store::ProviderKind;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Embedded so the skill installs from the installed executable, with no source checkout.
const SKILL: &str = include_str!("../../../../skills/concordia-harness-connect/SKILL.md");
const SKILL_NAME: &str = "concordia-harness-connect";
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
/// Marks a connection whose manifest comes from a program, not a file.
const COMMAND_PREFIX: &str = "command:";

type Run<'a> = &'a dyn Fn(&[String]) -> RunOutcome;

fn run_adapter(run: Run<'_>, env: &[String], argv: &[String]) -> RunOutcome {
    super::run::with_extra_env(env, || run(argv))
}

/// Where the command runs: the current pane's account and project.
pub struct Session<'a> {
    pub store: &'a Path,
    pub cwd: &'a Path,
    pub provider: ProviderKind,
    pub home: Option<&'a Path>,
    /// Every registered account, for `--all`.
    pub accounts: Vec<(ProviderKind, Option<PathBuf>)>,
}

struct Loaded {
    manifest: Manifest,
    path: String,
    sha: String,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sha_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn home_label(home: Option<&str>) -> String {
    home.map_or("default home".to_string(), str::to_string)
}

pub fn help_lines() -> Vec<String> {
    [
        "/harness: connect any harness to the providers Concordia launches",
        "  /harness connect <folder|harness.json|program> [--all]   wire it into this account (or every account)",
        "  /harness status                                          what is connected and verified",
        "  /harness verify [name] [provider] [--all]                run its live probe; opens a locked project only on a real denial",
        "  /harness disconnect <name>                               forget it and withdraw the receipts it wrote",
        "  /harness skill                                           install the connect skill into this account's home",
        "  /harness tools [name|harness/name]                      list or run a declared harness tool",
        "A harness is described by harness.json (see docs/harness.md). Commands run without a shell.",
    ]
    .iter()
    .map(|l| l.to_string())
    .collect()
}

/// Split on spaces, keeping a "quoted path" together.
fn split_args(arg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote = false;
    for c in arg.chars() {
        match c {
            '"' => quote = !quote,
            c if c.is_whitespace() && !quote => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn load_manifest(target: &str, cwd: &Path, run: Run<'_>) -> Result<Loaded, String> {
    let given = Path::new(target);
    let path = if given.is_absolute() {
        given.to_path_buf()
    } else {
        cwd.join(given)
    };
    let looks_like_path =
        target.contains('/') || target.contains('\\') || target.starts_with('.') || path.exists();
    if looks_like_path {
        let file = if path.is_dir() {
            path.join("harness.json")
        } else {
            path
        };
        let meta =
            fs::metadata(&file).map_err(|_| format!("no harness.json at {}", file.display()))?;
        if meta.len() > MAX_MANIFEST_BYTES {
            return Err("that manifest is too large to be a harness.json".into());
        }
        let text = fs::read_to_string(&file)
            .map_err(|e| format!("could not read {}: {e}", file.display()))?;
        let dir = file.parent().unwrap_or(cwd);
        let manifest = parse_manifest(&text, dir)?;
        return Ok(Loaded {
            sha: sha_hex(&text),
            path: file.to_string_lossy().into_owned(),
            manifest,
        });
    }
    // A bare program name: ask the program for its manifest.
    let outcome = run(&[target.to_string(), "concordia-manifest".to_string()]);
    if outcome.timed_out || outcome.exit != Some(0) {
        return Err(format!(
            "`{target} concordia-manifest` did not print a manifest ({})",
            if outcome.timed_out {
                "timed out".to_string()
            } else {
                outcome.stderr.trim().to_string()
            }
        ));
    }
    let manifest = parse_manifest(&outcome.stdout, cwd)?;
    Ok(Loaded {
        sha: sha_hex(&outcome.stdout),
        path: format!("{COMMAND_PREFIX}{target}"),
        manifest,
    })
}

fn identity<'a>(
    manifest: &'a Manifest,
    path: &'a str,
    sha: &'a str,
    skill: &'a Option<String>,
) -> Identity<'a> {
    Identity {
        name: &manifest.name,
        version: manifest.version.as_deref(),
        manifest_path: path,
        manifest_sha256: sha,
        skill: skill.as_deref(),
    }
}

fn entry_for(ctx: &Context<'_>, connect_ok: bool, note: String) -> Entry {
    Entry {
        provider: ctx.provider.as_str().to_string(),
        home: ctx.home.map(|h| h.to_string_lossy().into_owned()),
        project: ctx.project.to_string_lossy().into_owned(),
        connect_ok,
        connect_note: note,
        verify: "not-run".into(),
        verify_note: String::new(),
        at: now(),
    }
}

/// Run the manifest's connect command for one provider and home, and record the result.
pub fn connect_in(
    store: &Path,
    manifest: &Manifest,
    manifest_path: &Path,
    sha: &str,
    ctx: &Context<'_>,
    run: Run<'_>,
) -> Entry {
    let provider = ctx.provider.as_str();
    let outcome = manifest
        .providers
        .get(provider)
        .ok_or_else(|| format!("{} does not declare {provider}", manifest.name))
        .and_then(|commands| expand(&commands.connect, ctx))
        .map(|mut argv| {
            argv[0] = resolve_program(&argv[0], &manifest.dir)
                .to_string_lossy()
                .into_owned();
            run_adapter(run, &manifest.providers[provider].env, &argv)
        });
    let entry = match outcome {
        Err(why) => entry_for(ctx, false, why),
        Ok(o) if o.timed_out => entry_for(ctx, false, "the connect command timed out".into()),
        Ok(o) if o.exit == Some(0) => entry_for(ctx, true, "ok".into()),
        Ok(o) => {
            let code = o
                .exit
                .map_or("no exit code".to_string(), |c| format!("exit {c}"));
            let detail = o.stderr.trim().lines().last().unwrap_or("").to_string();
            entry_for(ctx, false, format!("{code}: {detail}"))
        }
    };
    let skill = manifest
        .skill
        .as_ref()
        .map(|s| s.to_string_lossy().into_owned());
    let manifest_path_text = manifest_path.to_string_lossy().into_owned();
    let who = identity(manifest, &manifest_path_text, sha, &skill);
    let mut entry = entry;
    if let Err(e) = connections::record(store, &who, entry.clone()) {
        entry.connect_note = format!("{}; {e}", entry.connect_note);
    }
    entry
}

/// Run the manifest's verify command; a receipt is written only for a real, matching denial.
pub fn verify_in(
    store: &Path,
    manifest: &Manifest,
    manifest_path: &Path,
    sha: &str,
    ctx: &Context<'_>,
    run: Run<'_>,
) -> Verdict {
    let env = manifest
        .providers
        .get(ctx.provider.as_str())
        .map_or(&[][..], |commands| commands.env.as_slice());
    let verdict = verify::verify_in(store, manifest, manifest_path, sha, ctx, &|argv| {
        run_adapter(run, env, argv)
    });
    let (state, note) = match &verdict {
        Verdict::Verified => ("verified", String::new()),
        Verdict::Unverified(why) => ("unverified", why.clone()),
        Verdict::NothingToVerify(why) => ("not-run", why.clone()),
    };
    let home_text = ctx.home.map(|h| h.to_string_lossy().into_owned());
    let project_text = ctx.project.to_string_lossy().into_owned();
    let has_entry = connections::load(store).ok().is_some_and(|all| {
        all.iter().any(|c| {
            c.name == manifest.name
                && c.entries.iter().any(|e| {
                    e.provider == ctx.provider.as_str()
                        && e.home == home_text
                        && e.project == project_text
                })
        })
    });
    if !has_entry {
        let manifest_path_text = manifest_path.to_string_lossy().into_owned();
        let skill = manifest
            .skill
            .as_ref()
            .map(|s| s.to_string_lossy().into_owned());
        let who = identity(manifest, &manifest_path_text, sha, &skill);
        let _ = connections::record(store, &who, entry_for(ctx, true, "ok".into()));
    }
    let _ = connections::set_verify(
        store,
        &manifest.name,
        ctx.provider.as_str(),
        home_text.as_deref(),
        &project_text,
        state,
        &note,
        now(),
    );
    verdict
}

pub fn status_lines(store: &Path) -> Vec<String> {
    let all = match connections::load(store) {
        Ok(all) => all,
        Err(e) => return vec![e],
    };
    if all.is_empty() {
        return vec!["no harness connected. /harness connect <folder> (or /harness help)".into()];
    }
    let mut lines = Vec::new();
    for c in all {
        let manifest_current = if c.manifest_path.starts_with(COMMAND_PREFIX) {
            true
        } else {
            fs::read_to_string(&c.manifest_path)
                .ok()
                .is_some_and(|text| sha_hex(&text) == c.manifest_sha256)
        };
        lines.push(format!(
            "{} {}  ({})",
            c.name,
            c.version.as_deref().unwrap_or(""),
            c.manifest_path
        ));
        for e in c.entries {
            let connect = if e.connect_ok {
                "connected".to_string()
            } else {
                format!("connect failed: {}", e.connect_note)
            };
            let live = manifest_current
                && e.home.as_deref().is_none_or(|h| Path::new(h).is_dir())
                && Path::new(&e.project)
                    .join(crate::provider::enforcement::POLICY_FILE)
                    .is_file()
                && match e.provider.as_str() {
                    "codex" => {
                        crate::provider::enforcement::gate_in(
                            store,
                            Path::new(&e.project),
                            ProviderKind::Codex,
                            e.home.as_deref().map(Path::new),
                        ) == crate::provider::enforcement::Gate::Open
                    }
                    "claude" => {
                        crate::provider::enforcement::gate_in(
                            store,
                            Path::new(&e.project),
                            ProviderKind::Claude,
                            e.home.as_deref().map(Path::new),
                        ) == crate::provider::enforcement::Gate::Open
                    }
                    "cursor" => {
                        crate::provider::enforcement::gate_in(
                            store,
                            Path::new(&e.project),
                            ProviderKind::Cursor,
                            e.home.as_deref().map(Path::new),
                        ) == crate::provider::enforcement::Gate::Open
                    }
                    _ => false,
                };
            let receipt =
                crate::provider::enforcement::project_root(Path::new(&e.project)).map(|root| {
                    crate::provider::enforcement::slot(store, &root)
                        .join(format!("{}.json", e.provider))
                });
            let verify = match (e.verify.as_str(), live) {
                ("verified", true) => "verified".to_string(),
                ("verified", false) => {
                    if receipt.as_ref().is_some_and(|path| path.is_file()) {
                        "unverified: receipt is stale (the policy, manifest, or binding changed)"
                            .to_string()
                    } else {
                        "unverified: receipt is missing".to_string()
                    }
                }
                ("unverified", _) => format!("unverified: {}", e.verify_note),
                _ => "not verified".to_string(),
            };
            lines.push(format!(
                "  {}  {}  {}  {}  {connect}  {verify}",
                c.name,
                e.provider,
                home_label(e.home.as_deref()),
                e.project
            ));
        }
    }
    lines
}

fn targets(session: &Session<'_>, all: bool) -> Vec<(ProviderKind, Option<PathBuf>)> {
    if all && !session.accounts.is_empty() {
        session.accounts.clone()
    } else {
        vec![(session.provider, session.home.map(Path::to_path_buf))]
    }
}

fn connect_cmd(session: &Session<'_>, args: &[String], run: Run<'_>) -> Vec<String> {
    let all = args.iter().any(|a| a == "--all");
    let Some(target) = args.iter().find(|a| !a.starts_with("--")) else {
        return vec!["usage: /harness connect <folder|harness.json|program> [--all]".into()];
    };
    let loaded = match load_manifest(target, session.cwd, run) {
        Ok(l) => l,
        Err(e) => return vec![e],
    };
    let mut lines = vec![format!(
        "harness {} {}",
        loaded.manifest.name,
        loaded.manifest.version.as_deref().unwrap_or("")
    )];
    for (provider, home) in targets(session, all) {
        if !loaded.manifest.providers.contains_key(provider.as_str()) {
            lines.push(format!(
                "skipped {p}: {n} does not declare {p}",
                p = provider.as_str(),
                n = loaded.manifest.name
            ));
            continue;
        }
        let ctx = Context {
            provider,
            home: home.as_deref(),
            project: session.cwd,
        };
        let entry = connect_in(
            session.store,
            &loaded.manifest,
            Path::new(&loaded.path),
            &loaded.sha,
            &ctx,
            run,
        );
        lines.push(if entry.connect_ok {
            format!(
                "connected {} · {}",
                provider.as_str(),
                home_label(entry.home.as_deref())
            )
        } else {
            format!("failed {}: {}", provider.as_str(), entry.connect_note)
        });
    }
    lines.push(
        "next: /harness verify (a locked-code project stays read-only until a real probe passes)"
            .into(),
    );
    lines
}

fn verify_cmd(session: &Session<'_>, args: &[String], run: Run<'_>) -> Vec<String> {
    let all = args.iter().any(|a| a == "--all");
    let mut provider_filter = None;
    let mut name = None;
    for a in args.iter().filter(|a| !a.starts_with("--")) {
        match a.to_lowercase().as_str() {
            "codex" => provider_filter = Some(ProviderKind::Codex),
            "claude" => provider_filter = Some(ProviderKind::Claude),
            "cursor" => provider_filter = Some(ProviderKind::Cursor),
            _ => name = Some(a.clone()),
        }
    }
    let connections = match connections::load(session.store) {
        Ok(c) => c,
        Err(e) => return vec![e],
    };
    let chosen = match (&name, connections.len()) {
        (_, 0) => return vec!["no harness connected. /harness connect <folder> first".into()],
        (Some(n), _) => match connections.iter().find(|c| &c.name == n) {
            Some(c) => c.clone(),
            None => return vec![format!("no harness named {n}. /harness status lists them")],
        },
        (None, 1) => connections[0].clone(),
        (None, _) => {
            let names: Vec<_> = connections.iter().map(|c| c.name.as_str()).collect();
            return vec![format!(
                "several harnesses are connected; name one: {}",
                names.join(", ")
            )];
        }
    };
    let loaded = match reload(&chosen.manifest_path, session.cwd, run) {
        Ok(l) => l,
        Err(e) => return vec![e],
    };
    if loaded.sha != chosen.manifest_sha256 {
        return vec![format!(
            "{}: unverified; the manifest changed since connect; reconnect before verifying",
            chosen.name
        )];
    }
    let mut targets = targets(session, all);
    if let Some(p) = provider_filter {
        targets.retain(|(tp, _)| *tp == p);
        if targets.is_empty() {
            return vec![format!(
                "no {} account to verify; use --all or switch with /use",
                p.as_str()
            )];
        }
    }
    let mut lines = Vec::new();
    for (provider, home) in targets {
        let project = session.cwd.to_string_lossy().into_owned();
        let home_text = home.as_ref().map(|h| h.to_string_lossy().into_owned());
        let connected = chosen.entries.iter().any(|e| {
            e.provider == provider.as_str()
                && e.home == home_text
                && e.project == project
                && e.connect_ok
        });
        if !connected {
            lines.push(format!(
                "{}: connect it first (/harness connect {})",
                provider.as_str(),
                chosen.manifest_path
            ));
            continue;
        }
        let ctx = Context {
            provider,
            home: home.as_deref(),
            project: session.cwd,
        };
        lines.push(
            match verify_in(
                session.store,
                &loaded.manifest,
                Path::new(&loaded.path),
                &loaded.sha,
                &ctx,
                run,
            ) {
                Verdict::Verified => format!(
                    "verified {} · {} (the launch gate now accepts it)",
                    provider.as_str(),
                    home_label(home_text.as_deref())
                ),
                Verdict::Unverified(why) => format!("unverified {}: {why}", provider.as_str()),
                Verdict::NothingToVerify(why) => format!("{}: {why}", provider.as_str()),
            },
        );
    }
    lines
}

fn reload(stored: &str, cwd: &Path, run: Run<'_>) -> Result<Loaded, String> {
    match stored.strip_prefix(COMMAND_PREFIX) {
        Some(program) => load_manifest(program, cwd, run),
        None => load_manifest(stored, cwd, run),
    }
}

/// Delete every receipt that `name` wrote. Receipts from other harnesses are left alone.
fn withdraw_receipts(store: &Path, name: &str) -> usize {
    let Ok(projects) = fs::read_dir(store.join("enforcement")) else {
        return 0;
    };
    let mut removed = 0;
    for project in projects.flatten() {
        let Ok(files) = fs::read_dir(project.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            let wrote = fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .is_some_and(|v| v.get("harness").and_then(|h| h.as_str()) == Some(name));
            if wrote && fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

fn disconnect_cmd(session: &Session<'_>, args: &[String]) -> Vec<String> {
    let Some(name) = args.first() else {
        return vec!["usage: /harness disconnect <name>".into()];
    };
    match connections::remove(session.store, name) {
        Err(e) => vec![e],
        Ok(false) => vec![format!(
            "no harness named {name}. /harness status lists them"
        )],
        Ok(true) => {
            let withdrawn = withdraw_receipts(session.store, name);
            vec![format!(
                "disconnected {name}; withdrew {withdrawn} receipt(s). Changes it made in provider homes stay; use its own uninstall to remove them."
            )]
        }
    }
}

/// Install the connect skill as one folder under the account's home. No other file is touched.
fn skill_cmd(session: &Session<'_>) -> Vec<String> {
    let Some(home) = session.home else {
        return vec!["this account has no isolated home, so the skill cannot be installed".into()];
    };
    let skills = home.join("skills");
    let folder = skills.join(SKILL_NAME);
    for path in [&skills, &folder] {
        if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return vec![format!(
                "refusing to write through a link: {}",
                path.display()
            )];
        }
    }
    match fs::create_dir_all(&folder).and_then(|()| fs::write(folder.join("SKILL.md"), SKILL)) {
        Ok(()) => vec![format!(
            "installed the {SKILL_NAME} skill in {}",
            folder.display()
        )],
        Err(e) => vec![format!("could not install the skill: {e}")],
    }
}

fn tools_cmd(session: &Session<'_>, args: &[String], run: Run<'_>) -> Vec<String> {
    let connections = match connections::load(session.store) {
        Ok(c) => c,
        Err(e) => return vec![e],
    };
    if connections.is_empty() {
        return vec!["no harness connected. /harness connect <folder> first".into()];
    }
    let mut rows: Vec<(String, super::Tool, Manifest)> = Vec::new();
    let mut errors = Vec::new();
    for connection in connections {
        match reload(&connection.manifest_path, session.cwd, run) {
            Ok(loaded) => rows.extend(
                loaded
                    .manifest
                    .tools
                    .iter()
                    .cloned()
                    .map(|tool| (connection.name.clone(), tool, loaded.manifest.clone())),
            ),
            Err(e) => errors.push(format!("{}: {e}", connection.name)),
        }
    }
    let Some(requested) = args.first() else {
        if rows.is_empty() {
            errors.push("connected harnesses declare no tools".into());
        }
        return rows
            .into_iter()
            .map(|(h, t, _)| format!("{h}/{}  {}", t.name, t.description))
            .chain(errors)
            .collect();
    };
    let (wanted_harness, wanted_tool) = requested
        .split_once('/')
        .map_or((None, requested.as_str()), |(h, t)| (Some(h), t));
    let Some((harness, tool, manifest)) = rows
        .into_iter()
        .find(|(h, t, _)| wanted_tool == t.name && wanted_harness.is_none_or(|w| w == h))
    else {
        return vec![format!("no declared harness tool `{requested}`")];
    };
    let ctx = Context {
        provider: session.provider,
        home: session.home,
        project: session.cwd,
    };
    let mut argv = match expand(&tool.argv, &ctx) {
        Ok(a) => a,
        Err(e) => return vec![format!("{}: {e}", tool.name)],
    };
    argv[0] = resolve_program(&argv[0], &manifest.dir)
        .to_string_lossy()
        .into_owned();
    let env = manifest
        .providers
        .get(session.provider.as_str())
        .map_or(&[][..], |commands| commands.env.as_slice());
    let outcome = run_adapter(run, env, &argv);
    if outcome.timed_out {
        return vec![format!("tool {harness}/{} timed out", tool.name)];
    }
    let mut out = vec![format!(
        "tool {harness}/{} exited {}",
        tool.name,
        outcome
            .exit
            .map_or("without a code".into(), |c| c.to_string())
    )];
    if !outcome.stdout.trim().is_empty() {
        out.push(outcome.stdout.trim_end().into());
    }
    if !outcome.stderr.trim().is_empty() {
        out.push(format!("stderr: {}", outcome.stderr.trim_end()));
    }
    out
}

/// Handle `/harness <arg>` and return the lines to show.
pub fn dispatch(session: &Session<'_>, arg: &str, run: Run<'_>) -> Vec<String> {
    let args = split_args(arg);
    let Some((sub, rest)) = args.split_first() else {
        return help_lines();
    };
    match sub.to_lowercase().as_str() {
        "help" | "?" => help_lines(),
        "status" | "list" => status_lines(session.store),
        "connect" => connect_cmd(session, rest, run),
        "verify" => verify_cmd(session, rest, run),
        "disconnect" | "remove" => disconnect_cmd(session, rest),
        "skill" => skill_cmd(session),
        "tools" => tools_cmd(session, rest, run),
        other => vec![format!(
            "unknown /harness subcommand `{other}`. /harness help lists them"
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::RunOutcome;
    use crate::provider::enforcement::{self, Gate};
    use std::cell::RefCell;
    use std::fs;

    const POLICY: &str = "{\"controls\":[]}\n";

    struct World {
        store: tempfile::TempDir,
        project: tempfile::TempDir,
        home: tempfile::TempDir,
        tool: tempfile::TempDir,
    }

    fn world() -> World {
        let w = World {
            store: tempfile::tempdir().unwrap(),
            project: tempfile::tempdir().unwrap(),
            home: tempfile::tempdir().unwrap(),
            tool: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(w.project.path().join("docs/quality")).unwrap();
        fs::write(
            w.project.path().join("docs/quality/locked-code.json"),
            POLICY,
        )
        .unwrap();
        fs::write(
            w.tool.path().join("harness.json"),
            r#"{"name":"mine","version":"1.0","providers":{
                "claude":{"connect":["h","connect","{provider}","{home}"],"verify":["h","verify","{provider}","{home}","{project}"]},
                "codex":{"connect":["h","connect","{provider}","{home}"]}},
                "tools":[{"name":"notes","description":"query notes","argv":["h","notes","{project}"]}]}"#,
        )
        .unwrap();
        w
    }

    fn session<'a>(w: &'a World, provider: ProviderKind) -> Session<'a> {
        Session {
            store: w.store.path(),
            cwd: w.project.path(),
            provider,
            home: Some(w.home.path()),
            accounts: Vec::new(),
        }
    }

    fn ok() -> RunOutcome {
        RunOutcome {
            exit: Some(0),
            ..RunOutcome::default()
        }
    }

    fn text(lines: &[String]) -> String {
        lines.join("\n")
    }

    #[test]
    fn help_lists_every_subcommand() {
        let w = world();
        let out = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "",
            &|_: &[String]| ok(),
        ));
        for word in ["connect", "status", "verify", "disconnect", "skill"] {
            assert!(out.contains(word), "{out}");
        }
        assert_eq!(
            out,
            text(&dispatch(
                &session(&w, ProviderKind::Claude),
                "help",
                &|_: &[String]| ok()
            ))
        );
    }

    #[test]
    fn status_with_nothing_connected_says_so() {
        let w = world();
        let out = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "status",
            &|_: &[String]| ok(),
        ));
        assert!(out.contains("no harness connected"), "{out}");
    }

    #[test]
    fn tools_are_listed_and_run_with_expanded_context() {
        let w = world();
        let arg = format!("connect {}", w.tool.path().display());
        dispatch(
            &session(&w, ProviderKind::Claude),
            &arg,
            &|_: &[String]| ok(),
        );
        let listed = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "tools",
            &|_: &[String]| ok(),
        ));
        assert!(
            listed.contains("mine/notes") && listed.contains("query notes"),
            "{listed}"
        );
        let seen = RefCell::new(Vec::new());
        let output = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "tools notes",
            &|argv| {
                seen.borrow_mut().push(argv.to_vec());
                RunOutcome {
                    exit: Some(0),
                    stdout: "answer".into(),
                    ..RunOutcome::default()
                }
            },
        ));
        assert!(output.contains("answer"), "{output}");
        assert_eq!(
            seen.into_inner()[0],
            ["h", "notes", &w.project.path().to_string_lossy()]
        );
    }

    #[test]
    fn connect_runs_the_declared_command_for_this_account_and_records_it() {
        let w = world();
        let seen = RefCell::new(Vec::new());
        {
            let run = |argv: &[String]| {
                seen.borrow_mut().push(argv.to_vec());
                ok()
            };
            let arg = format!("connect {}", w.tool.path().display());
            let out = text(&dispatch(&session(&w, ProviderKind::Claude), &arg, &run));
            assert!(out.contains("connected") && out.contains("claude"), "{out}");
            let status = text(&dispatch(
                &session(&w, ProviderKind::Claude),
                "status",
                &run,
            ));
            assert!(
                status.contains("mine") && status.contains("not verified"),
                "{status}"
            );
        }
        let seen = seen.into_inner();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            &seen[0][1..],
            ["connect", "claude", &w.home.path().to_string_lossy()]
        );
    }

    #[test]
    fn a_failing_connect_is_reported_and_recorded_as_failed() {
        let w = world();
        let run = |_: &[String]| RunOutcome {
            exit: Some(3),
            stderr: "refused".into(),
            ..RunOutcome::default()
        };
        let arg = format!("connect {}", w.tool.path().display());
        let out = text(&dispatch(&session(&w, ProviderKind::Claude), &arg, &run));
        assert!(out.contains("failed") && out.contains("refused"), "{out}");
        let out = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "verify",
            &run,
        ));
        assert!(
            out.contains("connect"),
            "verify needs a successful connect first: {out}"
        );
    }

    #[test]
    fn a_provider_the_manifest_does_not_declare_is_skipped_not_failed() {
        let w = world();
        let arg = format!("connect {}", w.tool.path().display());
        let out = text(&dispatch(
            &session(&w, ProviderKind::Cursor),
            &arg,
            &|_: &[String]| ok(),
        ));
        assert!(out.contains("does not declare cursor"), "{out}");
    }

    #[test]
    fn connect_with_a_bad_target_explains_what_is_missing() {
        let w = world();
        let empty = tempfile::tempdir().unwrap();
        let arg = format!("connect {}", empty.path().display());
        let out = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            &arg,
            &|_: &[String]| ok(),
        ));
        assert!(out.contains("harness.json"), "{out}");
        let out = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "connect",
            &|_: &[String]| ok(),
        ));
        assert!(out.contains("usage"), "{out}");
    }

    #[test]
    fn a_program_name_is_asked_for_its_manifest() {
        let w = world();
        let manifest = fs::read_to_string(w.tool.path().join("harness.json")).unwrap();
        let seen = RefCell::new(Vec::new());
        let run = |argv: &[String]| {
            seen.borrow_mut().push(argv.to_vec());
            if argv.get(1).map(String::as_str) == Some("concordia-manifest") {
                RunOutcome {
                    exit: Some(0),
                    stdout: manifest.clone(),
                    ..RunOutcome::default()
                }
            } else {
                ok()
            }
        };
        let out = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "connect my-tool",
            &run,
        ));
        assert!(out.contains("connected"), "{out}");
        assert_eq!(seen.borrow()[0], ["my-tool", "concordia-manifest"]);
    }

    fn denial(w: &World) -> RunOutcome {
        let body = serde_json::json!({
            "result": "denied-live-probe",
            "provider": "claude",
            "home": w.home.path().to_string_lossy(),
            "policyRevision": enforcement::policy_revision(POLICY.as_bytes()),
        });
        RunOutcome {
            exit: Some(0),
            stdout: body.to_string(),
            ..RunOutcome::default()
        }
    }

    fn connect_claude(w: &World) {
        let arg = format!("connect {}", w.tool.path().display());
        dispatch(
            &session(w, ProviderKind::Claude),
            &arg,
            &|_: &[String]| ok(),
        );
    }

    #[test]
    fn verify_opens_the_gate_only_after_a_real_denial_and_status_shows_it() {
        let w = world();
        connect_claude(&w);
        let gate = || {
            enforcement::gate_in(
                w.store.path(),
                w.project.path(),
                ProviderKind::Claude,
                Some(w.home.path()),
            )
        };
        assert!(matches!(gate(), Gate::ReadOnly(_)));
        let proof = denial(&w);
        let out = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "verify",
            &|_: &[String]| proof.clone(),
        ));
        assert!(out.contains("verified"), "{out}");
        assert_eq!(gate(), Gate::Open);
        let status = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "status",
            &|_: &[String]| ok(),
        ));
        assert!(
            status.contains("verified") && !status.contains("not verified"),
            "{status}"
        );
    }

    #[test]
    fn a_changed_manifest_is_refused_without_replacing_the_receipt_and_disconnect_withdraws_it() {
        let w = world();
        connect_claude(&w);
        let proof = denial(&w);
        let verified = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "verify",
            &|_: &[String]| proof.clone(),
        ));
        assert!(verified.contains("verified"), "{verified}");
        let root = enforcement::project_root(w.project.path()).unwrap();
        let receipt = enforcement::slot(w.store.path(), &root).join("claude.json");
        let before = fs::read(&receipt).unwrap();

        let manifest_path = w.tool.path().join("harness.json");
        let original = fs::read_to_string(&manifest_path).unwrap();
        fs::write(&manifest_path, format!("{original}\n")).unwrap();
        let refused = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "verify",
            &|_: &[String]| proof.clone(),
        ));
        assert!(
            refused.contains("unverified") && refused.contains("reconnect"),
            "{refused}"
        );
        assert_eq!(fs::read(&receipt).unwrap(), before);

        let disconnected = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "disconnect mine",
            &|_: &[String]| ok(),
        ));
        assert!(
            disconnected.contains("withdrew 1 receipt"),
            "{disconnected}"
        );
        assert!(!receipt.exists());
    }

    #[test]
    fn status_reports_missing_receipts_and_stale_policy_receipts_as_unverified() {
        let w = world();
        connect_claude(&w);
        let proof = denial(&w);
        let verified = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "verify",
            &|_: &[String]| proof.clone(),
        ));
        assert!(verified.contains("verified"), "{verified}");
        let root = enforcement::project_root(w.project.path()).unwrap();
        let receipt = enforcement::slot(w.store.path(), &root).join("claude.json");

        fs::remove_file(&receipt).unwrap();
        let missing = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "status",
            &|_: &[String]| ok(),
        ));
        assert!(
            missing.contains("unverified: receipt is missing"),
            "{missing}"
        );
        assert!(!missing.contains("  verified"), "{missing}");

        let verified_again = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "verify",
            &|_: &[String]| proof.clone(),
        ));
        assert!(verified_again.contains("verified"), "{verified_again}");
        fs::write(
            w.project.path().join("docs/quality/locked-code.json"),
            "{\"controls\":[\"changed\"]}\n",
        )
        .unwrap();
        let stale = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "status",
            &|_: &[String]| ok(),
        ));
        assert!(stale.contains("unverified: receipt is stale"), "{stale}");
        assert!(!stale.contains("  verified"), "{stale}");
    }

    #[test]
    fn verify_refuses_reparse_point_state_without_writing_inside_or_outside_it() {
        let w = world();
        connect_claude(&w);
        let root = enforcement::project_root(w.project.path()).unwrap();
        let slot = enforcement::slot(w.store.path(), &root);
        fs::create_dir_all(slot.parent().unwrap()).unwrap();
        fs::create_dir_all(&slot).unwrap();
        assert!(!enforcement::has_reparse_component(w.store.path(), &slot));

        let outside = tempfile::tempdir().unwrap();
        let link_target = outside.path().join("redirected");
        fs::create_dir(&link_target).unwrap();
        fs::remove_dir(&slot).unwrap();
        if let Err(error) = std::os::windows::fs::symlink_dir(&link_target, &slot) {
            println!(
                "skipping reparse-point write test: could not create directory symlink: {error}"
            );
            return;
        }
        assert!(enforcement::has_reparse_component(w.store.path(), &slot));

        let out = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "verify",
            &|_: &[String]| denial(&w),
        ));
        assert!(
            out.contains("unverified") && out.contains("symlink or junction"),
            "{out}"
        );
        assert!(!link_target.join("claude.json").exists());
        assert!(!link_target.join("locked").exists());
    }

    #[test]
    fn verify_that_cannot_prove_it_says_why_and_leaves_the_project_read_only() {
        let w = world();
        connect_claude(&w);
        let run = |_: &[String]| RunOutcome {
            exit: Some(0),
            stdout: r#"{"result":"not-supported","reason":"no hooks in this client"}"#.into(),
            ..RunOutcome::default()
        };
        let out = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "verify",
            &run,
        ));
        assert!(
            out.contains("unverified") && out.contains("no hooks in this client"),
            "{out}"
        );
        assert!(matches!(
            enforcement::gate_in(
                w.store.path(),
                w.project.path(),
                ProviderKind::Claude,
                Some(w.home.path())
            ),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn disconnect_forgets_the_harness_and_withdraws_only_its_receipts() {
        let w = world();
        connect_claude(&w);
        let proof = denial(&w);
        dispatch(
            &session(&w, ProviderKind::Claude),
            "verify",
            &|_: &[String]| proof.clone(),
        );
        // A receipt written by some other harness for another provider must survive.
        let root = enforcement::project_root(w.project.path()).unwrap();
        let slot = enforcement::slot(w.store.path(), &root);
        fs::write(
            slot.join("codex.json"),
            r#"{"provider":"codex","home":null,"policyRevision":"x","result":"denied-live-probe","harness":"other"}"#,
        )
        .unwrap();
        let out = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "disconnect mine",
            &|_: &[String]| ok(),
        ));
        assert!(out.contains("disconnected"), "{out}");
        assert!(
            !slot.join("claude.json").exists(),
            "this harness's receipt is withdrawn"
        );
        assert!(
            slot.join("codex.json").exists(),
            "another harness's receipt stays"
        );
        let status = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "status",
            &|_: &[String]| ok(),
        ));
        assert!(status.contains("no harness connected"), "{status}");
        let again = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "disconnect mine",
            &|_: &[String]| ok(),
        ));
        assert!(again.contains("no harness named"), "{again}");
    }

    #[test]
    fn the_skill_installs_as_one_folder_and_touches_nothing_else() {
        let w = world();
        fs::write(w.home.path().join("auth.json"), "SECRET").unwrap();
        let out = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "skill",
            &|_: &[String]| ok(),
        ));
        assert!(out.contains("installed"), "{out}");
        let skill = w
            .home
            .path()
            .join("skills/concordia-harness-connect/SKILL.md");
        let body = fs::read_to_string(&skill).unwrap();
        assert!(body.contains("name: concordia-harness-connect"));
        let mut names: Vec<String> = fs::read_dir(w.home.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["auth.json", "skills"]);
        assert_eq!(
            fs::read_to_string(w.home.path().join("auth.json")).unwrap(),
            "SECRET"
        );
    }

    #[test]
    fn the_skill_needs_an_isolated_home() {
        let w = world();
        let mut s = session(&w, ProviderKind::Claude);
        s.home = None;
        let out = text(&dispatch(&s, "skill", &|_: &[String]| ok()));
        assert!(out.contains("isolated home"), "{out}");
    }

    #[test]
    fn all_accounts_are_connected_when_asked() {
        let w = world();
        let other = tempfile::tempdir().unwrap();
        let mut s = session(&w, ProviderKind::Claude);
        s.accounts = vec![
            (ProviderKind::Claude, Some(w.home.path().to_path_buf())),
            (ProviderKind::Codex, Some(other.path().to_path_buf())),
            (ProviderKind::Cursor, Some(other.path().to_path_buf())),
        ];
        let arg = format!("connect {} --all", w.tool.path().display());
        let out = text(&dispatch(&s, &arg, &|_: &[String]| ok()));
        assert_eq!(out.matches("connected").count(), 2, "{out}");
        assert!(out.contains("does not declare cursor"), "{out}");
    }

    #[test]
    fn an_unknown_subcommand_points_at_help() {
        let w = world();
        let out = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "frobnicate",
            &|_: &[String]| ok(),
        ));
        assert!(out.contains("/harness help"), "{out}");
    }
}
