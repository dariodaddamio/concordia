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

pub(super) struct Loaded {
    pub manifest: Manifest,
    pub path: String,
    pub sha: String,
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

fn manifest_revision_current(path: &Path, expected: &str) -> bool {
    if path.to_string_lossy().starts_with(COMMAND_PREFIX) {
        return true;
    }
    load_local_manifest_for_action(&path.to_string_lossy(), Path::new("."))
        .is_ok_and(|(_, revision, _)| revision == expected)
}

fn home_label(home: Option<&str>) -> String {
    home.map_or("default home".to_string(), str::to_string)
}

pub fn help_lines() -> Vec<String> {
    [
        "/harness: connect any harness to the providers Concordia launches",
        "  /harness connect <folder|harness.json|program> [--current]   connect all accounts; new accounts inherit this project setup",
        "  /harness status                                          what is connected and verified",
        "  /harness verify [name] [provider] [--current]             run its live probe across accounts; native trust is separate",
        "  /harness disconnect <name>                               forget it and withdraw the receipts it wrote",
        "  /harness skill [--current]                               install the Concordia connection skill across accounts",
        "  /skills install [PATH] [--current]                       share a skill folder/collection; default ~/.agents/skills",
        "  /skills remove PATH                                     stop sharing and remove matching links across accounts",
        "  /harness tools [name|harness/name]                      list or run a declared harness tool",
        "A harness is described by harness.json (see docs/harness.md). Commands run without a shell.",
    ]
    .iter()
    .map(|l| l.to_string())
    .collect()
}

/// Explain setup choices before any command can mutate receipts, links or provider homes.
/// Discovery is deliberately shallow and bounded: a guided command is an entry point, not a
/// filesystem indexer.
pub fn guided_lines(session: &Session<'_>, subcommand: &str) -> Vec<String> {
    let mut lines = match subcommand {
        "connect" => vec![
            "Connect a local Harness so Concordia can expose its declared tools to provider agents.".into(),
            "Choose one detected Harness checkout or enter a path to harness.json; connecting writes a receipt and shares this project setup.".into(),
        ],
        "verify" => vec![
            "Verify a connected Harness by running its declared probe for the selected provider.".into(),
            "Choose a connected Harness and provider; verification updates only its receipt.".into(),
        ],
        "disconnect" => vec![
            "Disconnect a Harness and withdraw the receipts Concordia created for it.".into(),
            "Choose a connected Harness; provider homes and the Harness files are not deleted.".into(),
        ],
        "skill" => vec![
            "Install Concordia's Harness connection skill so provider agents understand the connection boundary.".into(),
            "Choose the current account or all configured accounts; this writes only the embedded skill file.".into(),
        ],
        "skills" => vec![
            "Share a local skill collection with configured provider accounts.".into(),
            "Choose a detected skill folder; Concordia validates SKILL.md files before creating links.".into(),
        ],
        "tools" => vec![
            "Inspect or run a tool declared by a connected Harness.".into(),
            "Choose a connected Harness and one of its declared tools; running it uses its argument array without a shell.".into(),
        ],
        _ => return help_lines(),
    };
    lines.push("Detected choices:".into());
    match subcommand {
        "connect" => detected_harnesses(session, &mut lines),
        "skills" => detected_skills(session, &mut lines),
        "verify" | "disconnect" | "tools" => match connections::load(session.store) {
            Ok(entries) if !entries.is_empty() => {
                for entry in entries.into_iter().take(32) {
                        let target = entry.entries.first().map(|target| {
                            format!("{} · {}", target.provider, target.project)
                        }).unwrap_or_else(|| "no provider target recorded".into());
                        lines.push(format!("  {} · {target}", entry.name));
                }
            }
            _ => lines.push("  none connected yet; use /harness connect to add one".into()),
        },
        "skill" => lines.push("  current account or all configured accounts".into()),
        _ => {}
    }
    lines.extend([
        "Selection is validated and a review of the files, receipts and provider accounts changed appears before confirmation.".into(),
        "After confirmation, the panel reports exactly what was connected and shows the matching undo command.".into(),
        "Esc cancels without changing provider homes or existing receipts.".into(),
    ]);
    lines
}

fn detected_harnesses(session: &Session<'_>, lines: &mut Vec<String>) {
    let mut found = 0;
    for root in session.cwd.ancestors().take(8) {
        let path = root.join("harness.json");
        if path.is_file() {
            lines.push(format!(
                "  {} · connects the Harness tools declared by this checkout",
                path.display()
            ));
            found += 1;
        }
    }
    for name in [".brain", "brain", "brain-vault"] {
        let path = session.cwd.join(name);
        if path.is_dir() {
            lines.push(format!(
                "  {} · local memory source available to a Harness",
                path.display()
            ));
            found += 1;
        }
    }
    if found == 0 {
        lines.push(
            "  none found under the current checkout; enter a Harness path to continue".into(),
        );
    }
}

fn detected_skills(session: &Session<'_>, lines: &mut Vec<String>) {
    let mut found = 0;
    for root in session.cwd.ancestors().take(8) {
        let path = root.join(".agents").join("skills");
        if path.is_dir() {
            lines.push(format!(
                "  {} · shares the SKILL.md folders found here",
                path.display()
            ));
            found += 1;
        }
    }
    if found == 0 {
        lines.push("  no skill folder found under the current checkout; enter a folder containing SKILL.md".into());
    }
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
        let (manifest, sha, path) = load_local_manifest_for_action(&file.to_string_lossy(), cwd)?;
        return Ok(Loaded {
            sha,
            path,
            manifest,
        });
    }
    // Harness has a Concordia-owned adapter contract. Other programs retain the generic
    // external `concordia-manifest` integration.
    if super::adapter::is_harness_program(target) {
        let (manifest, text) = super::adapter::build(target, cwd, run)?;
        return Ok(Loaded {
            sha: sha_hex(&text),
            path: format!("{COMMAND_PREFIX}{target}"),
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
    if !manifest_revision_current(manifest_path, sha) {
        return entry_for(
            ctx,
            false,
            "manifest revision changed; refresh and reconnect explicitly".into(),
        );
    }
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
        Ok(o) if o.exit == Some(0) => {
            let report = serde_json::from_str::<serde_json::Value>(&o.stdout).ok();
            let note = match report.as_ref().and_then(|value| value["trust"].as_str()) {
                Some(trust @ ("trusted" | "untrusted" | "unknown" | "n/a")) =>
                    format!("configuration connected; reported native trust: {trust}; enforcement not verified"),
                _ => "ok".into(),
            };
            entry_for(ctx, true, note)
        }
        Ok(o) => {
            let code = o
                .exit
                .map_or("no exit code".to_string(), |c| format!("exit {c}"));
            let detail = crate::sessions::clean_preview(
                Some(if o.stderr.trim().is_empty() {
                    &o.stdout
                } else {
                    &o.stderr
                }),
                512,
            );
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
    if let Err(e) = connections::record_with_pins(
        store,
        &who,
        entry.clone(),
        !manifest.pinned_decisions.is_empty(),
    ) {
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
    if !manifest_revision_current(manifest_path, sha) {
        return Verdict::Unverified(
            "manifest revision changed; refresh and reconnect explicitly".into(),
        );
    }
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
        let _ = connections::record_with_pins(
            store,
            &who,
            entry_for(ctx, true, "ok".into()),
            !manifest.pinned_decisions.is_empty(),
        );
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

/// Reviewed reconnect: the saved binding and manifest revision must still be the one shown to
/// the user immediately before the adapter is called.
pub fn reconnect_reviewed(
    store: &Path,
    target: &connections::ActionTarget,
    manifest: &Manifest,
    run: Run<'_>,
) -> Result<Entry, String> {
    connections::check_action_target(store, target)?;
    if manifest.name != target.name || target.manifest_sha256.is_empty() {
        return Err("integration identity changed; refresh and reselect".into());
    }
    let provider = match target.provider.as_str() {
        "codex" => ProviderKind::Codex,
        "claude" => ProviderKind::Claude,
        "cursor" => ProviderKind::Cursor,
        _ => return Err("unsupported provider binding".into()),
    };
    let project = Path::new(&target.project);
    let home = target.home.as_deref().map(Path::new);
    Ok(connect_in(
        store,
        manifest,
        Path::new(&target.manifest_path),
        &target.manifest_sha256,
        &Context {
            provider,
            home,
            project,
        },
        run,
    ))
}

/// Reviewed verification with the same revision/binding guard as reconnect.
pub fn verify_reviewed(
    store: &Path,
    target: &connections::ActionTarget,
    manifest: &Manifest,
    run: Run<'_>,
) -> Result<Verdict, String> {
    connections::check_action_target(store, target)?;
    if manifest.name != target.name {
        return Err("integration identity changed; refresh and reselect".into());
    }
    let provider = match target.provider.as_str() {
        "codex" => ProviderKind::Codex,
        "claude" => ProviderKind::Claude,
        "cursor" => ProviderKind::Cursor,
        _ => return Err("unsupported provider binding".into()),
    };
    Ok(verify_in(
        store,
        manifest,
        Path::new(&target.manifest_path),
        &target.manifest_sha256,
        &Context {
            provider,
            home: target.home.as_deref().map(Path::new),
            project: Path::new(&target.project),
        },
        run,
    ))
}

pub fn status_lines(store: &Path) -> Vec<String> {
    let mut lines = match connections::shared_skill_sources(store) {
        Ok(sources) => sources
            .into_iter()
            .map(|source| format!("shared skills: {source} · all accounts, all projects"))
            .collect::<Vec<_>>(),
        Err(error) => return vec![error],
    };
    let all = match connections::load(store) {
        Ok(all) => all,
        Err(e) => return vec![e],
    };
    if all.is_empty() {
        if lines.is_empty() {
            lines
                .push("no setup connected. /skills install [PATH] or /harness connect PATH".into());
        }
        return lines;
    }
    for c in all {
        for project in &c.all_account_projects {
            lines.push(format!(
                "{} · shared project: {project} · later accounts inherit setup",
                c.name
            ));
        }
        let manifest_current = if c.manifest_path.starts_with(COMMAND_PREFIX) {
            true
        } else {
            crate::fs_safe::read_text_bounded(
                Path::new(&c.manifest_path),
                MAX_MANIFEST_BYTES as usize,
            )
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
                if e.connect_note == "ok" {
                    "connected".to_string()
                } else {
                    format!("connected: {}", e.connect_note)
                }
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
    let all = !args.iter().any(|a| a == "--current");
    let Some(target) = args.iter().find(|a| !a.starts_with("--")) else {
        return vec!["usage: /harness connect <folder|harness.json|program> [--current]".into()];
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
                "connected {} · {} · {}",
                provider.as_str(),
                home_label(entry.home.as_deref()),
                entry.connect_note
            )
        } else {
            format!("failed {}: {}", provider.as_str(), entry.connect_note)
        });
    }
    if all {
        if let Err(error) =
            connections::share_project(session.store, &loaded.manifest.name, session.cwd)
        {
            lines.push(format!("shared setup was not saved: {error}"));
        } else {
            lines.push("shared setup saved for this project; later accounts connect before their first turn".into());
        }
    }
    lines.push(
        "next: /harness verify (a locked-code project stays read-only until a real probe passes)"
            .into(),
    );
    lines
}

fn verify_cmd(session: &Session<'_>, args: &[String], run: Run<'_>) -> Vec<String> {
    let all = !args.iter().any(|a| a == "--current");
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

pub(super) fn reload(stored: &str, cwd: &Path, run: Run<'_>) -> Result<Loaded, String> {
    match stored.strip_prefix(COMMAND_PREFIX) {
        Some(program) => load_manifest(program, cwd, run),
        None => load_manifest(stored, cwd, run),
    }
}

/// Load a reviewed manifest without executing a connect/verify action. F10 uses this after the
/// user confirms a target; command-backed manifests are refreshed through their declared
/// read-only manifest query.
pub fn load_manifest_for_action(
    stored: &str,
    cwd: &Path,
    run: Run<'_>,
) -> Result<(Manifest, String), String> {
    let loaded = reload(stored, cwd, run)?;
    Ok((loaded.manifest, loaded.sha))
}

/// Read an explicitly selected local manifest for F10. This deliberately rejects bare programs
/// and command-backed manifests: entering a path is discovery only, and adapter execution waits
/// for the separate reviewed connect confirmation.
pub fn load_local_manifest_for_action(
    selection: &str,
    cwd: &Path,
) -> Result<(Manifest, String, String), String> {
    let given = Path::new(selection);
    let path = if given.is_absolute() {
        given.to_path_buf()
    } else {
        cwd.join(given)
    };
    let file = if path.is_dir() {
        path.join("harness.json")
    } else {
        path
    };
    let metadata = fs::symlink_metadata(&file).map_err(|_| {
        format!(
            "manifest path is unavailable: {} (select a harness.json file or its directory)",
            file.display()
        )
    })?;
    if !metadata.is_file()
        || super::super::workspaces::linked(&metadata)
        || metadata.len() > MAX_MANIFEST_BYTES
    {
        return Err("manifest must be a regular local file of at most 256 KiB".into());
    }
    let canonical = file
        .canonicalize()
        .map_err(|_| "manifest path is unavailable")?;
    if crate::workspace_paths::sensitive(&file) || crate::workspace_paths::sensitive(&canonical) {
        return Err("manifest path is excluded by workspace policy".into());
    }
    use std::io::Read;
    let mut text = String::new();
    fs::File::open(&canonical)
        .map_err(|_| "manifest is not readable")?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| "manifest is not bounded UTF-8")?;
    if text.len() as u64 > MAX_MANIFEST_BYTES
        || file.canonicalize().ok().as_ref() != Some(&canonical)
    {
        return Err("manifest changed or exceeds 256 KiB; reselect it".into());
    }
    let dir = canonical.parent().unwrap_or(cwd);
    let manifest = parse_manifest(&text, dir)?;
    Ok((
        manifest,
        sha_hex(&text),
        canonical.to_string_lossy().into_owned(),
    ))
}

/// Reviewed first connection for F10. Unlike reconnect, this target is not yet in the receipt
/// store; the manifest revision is checked immediately before the declared connect argv runs.
pub fn connect_new_reviewed(
    store: &Path,
    target: &connections::ActionTarget,
    manifest: &Manifest,
    run: Run<'_>,
) -> Result<Entry, String> {
    if manifest.name != target.name || target.manifest_path.starts_with("command:") {
        return Err("new F10 connections require an explicit local manifest".into());
    }
    if !manifest_revision_current(Path::new(&target.manifest_path), &target.manifest_sha256) {
        return Err("manifest revision changed; refresh and reselect".into());
    }
    if connections::load(store)?.iter().any(|existing| {
        existing.name == target.name
            && (existing.manifest_path != target.manifest_path
                || existing.manifest_sha256 != target.manifest_sha256)
    }) {
        return Err("an integration with this name already has a different manifest; select its existing connection first".into());
    }
    let provider = match target.provider.as_str() {
        "codex" => ProviderKind::Codex,
        "claude" => ProviderKind::Claude,
        "cursor" => ProviderKind::Cursor,
        _ => return Err("unsupported provider binding".into()),
    };
    Ok(connect_in(
        store,
        manifest,
        Path::new(&target.manifest_path),
        &target.manifest_sha256,
        &Context {
            provider,
            home: target.home.as_deref().map(Path::new),
            project: Path::new(&target.project),
        },
        run,
    ))
}

/// Delete every receipt that `name` wrote. Receipts from other harnesses are left alone.
fn withdraw_receipts(store: &Path, name: &str) -> usize {
    let Ok(projects) = fs::read_dir(store.join("enforcement")) else {
        return 0;
    };
    let mut removed = 0;
    for project in projects.take(4096).flatten() {
        for provider in ["codex", "claude", "cursor"] {
            let path = project.path().join(format!("{provider}.json"));
            if crate::provider::enforcement::has_reparse_component(store, &path) {
                continue;
            }
            let wrote = crate::fs_safe::read_text_bounded(&path, 64 * 1024)
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .is_some_and(|v| v.get("harness").and_then(|h| h.as_str()) == Some(name));
            if wrote && fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

fn withdraw_receipt_target(store: &Path, target: &connections::ActionTarget) -> usize {
    let Some(root) = crate::provider::enforcement::project_root(Path::new(&target.project)) else {
        return 0;
    };
    let path =
        crate::provider::enforcement::slot(store, &root).join(format!("{}.json", target.provider));
    if !matches!(target.provider.as_str(), "codex" | "claude" | "cursor")
        || crate::provider::enforcement::has_reparse_component(store, &path)
    {
        return 0;
    }
    let wrote = crate::fs_safe::read_text_bounded(&path, 64 * 1024)
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .is_some_and(|value| {
            value.get("harness").and_then(|h| h.as_str()) == Some(target.name.as_str())
                && value
                    .get("home")
                    .and_then(|home| home.as_str())
                    .map(|home| crate::provider::enforcement::path_key(Path::new(home)))
                    == target
                        .home
                        .as_deref()
                        .map(|home| crate::provider::enforcement::path_key(Path::new(home)))
                && value
                    .get("harnessManifestSha256")
                    .and_then(|revision| revision.as_str())
                    == Some(target.manifest_sha256.as_str())
        });
    if wrote && fs::remove_file(path).is_ok() {
        1
    } else {
        0
    }
}

pub fn disconnect_cmd(session: &Session<'_>, args: &[String]) -> Vec<String> {
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

/// Commit a reviewed disconnect after rechecking the immutable catalog target.
/// The caller supplies confirmation separately; this function performs no implicit prompt.
pub fn disconnect_reviewed(
    store: &Path,
    target: &connections::ActionTarget,
) -> Result<String, String> {
    match connections::remove_action_target(store, target)? {
        true => {
            let withdrawn = withdraw_receipt_target(store, target);
            Ok(format!(
                "disconnected {}; withdrew {withdrawn} receipt(s); provider homes were not uninstalled",
                target.name
            ))
        }
        false => Ok(format!("{} was already disconnected", target.name)),
    }
}

/// Install the connect skill as one folder under the account's home. No other file is touched.
fn skill_cmd(session: &Session<'_>, args: &[String]) -> Vec<String> {
    targets(session, !args.iter().any(|arg| arg == "--current"))
        .into_iter()
        .flat_map(|(provider, home)| {
            let label = home_label(home.as_deref().and_then(Path::to_str));
            install_connect_skill(home.as_deref())
                .into_iter()
                .map(move |line| format!("{} · {label}: {line}", provider.as_str()))
        })
        .collect()
}

fn install_connect_skill(home: Option<&Path>) -> Vec<String> {
    let Some(home) = home else {
        return vec!["this account has no isolated home, so the skill cannot be installed".into()];
    };
    let skills = home.join("skills");
    let folder = skills.join(SKILL_NAME);
    for path in [&skills, &folder] {
        if fs::symlink_metadata(path).is_ok_and(|m| crate::workspaces::linked(&m)) {
            return vec![format!(
                "refusing to write through a link: {}",
                path.display()
            )];
        }
    }
    match fs::create_dir_all(&folder).and_then(|()| {
        let path = folder.join("SKILL.md");
        if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file() || crate::workspaces::linked(&m))
        {
            return Err(std::io::Error::other(
                "skill destination is linked or not a regular file",
            ));
        }
        crate::state_io::write_atomic(&path, SKILL.as_bytes())
    }) {
        Ok(()) => vec![format!(
            "installed the {SKILL_NAME} skill in {}",
            folder.display()
        )],
        Err(e) => vec![format!("could not install the skill: {e}")],
    }
}

pub fn tools_cmd(session: &Session<'_>, args: &[String], run: Run<'_>) -> Vec<String> {
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
            Ok(loaded)
                if loaded.sha == connection.manifest_sha256
                    && loaded.manifest.name == connection.name =>
            {
                rows.extend(
                    loaded
                        .manifest
                        .tools
                        .iter()
                        .cloned()
                        .map(|tool| (connection.name.clone(), tool, loaded.manifest.clone())),
                )
            }
            Ok(_) => errors.push(format!(
                "{}: manifest changed; reconnect before listing or running its tools",
                connection.name
            )),
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
    dispatch_cancellable(
        session,
        arg,
        run,
        &std::sync::atomic::AtomicBool::new(false),
    )
}

pub fn dispatch_cancellable(
    session: &Session<'_>,
    arg: &str,
    run: Run<'_>,
    cancel: &std::sync::atomic::AtomicBool,
) -> Vec<String> {
    let args = split_args(arg);
    let Some((sub, rest)) = args.split_first() else {
        return help_lines();
    };
    if rest.is_empty()
        && matches!(
            sub.as_str(),
            "connect" | "verify" | "disconnect" | "remove" | "skill" | "skills" | "tools"
        )
    {
        return guided_lines(session, if sub == "remove" { "disconnect" } else { sub });
    }
    if matches!(sub.as_str(), "connect" | "verify" | "skill" | "skills") {
        if let Some(flag) = rest
            .iter()
            .find(|arg| arg.starts_with("--") && !matches!(arg.as_str(), "--all" | "--current"))
        {
            return vec![format!(
                "unsupported setup option: {flag}; use --current to limit setup to this account"
            )];
        }
        if sub == "skill" && rest.iter().any(|arg| !arg.starts_with("--")) {
            return vec![
                "usage: /harness skill [--current]; installs the Concordia connection skill".into(),
            ];
        }
        if matches!(sub.as_str(), "connect" | "skills")
            && rest.iter().filter(|arg| !arg.starts_with("--")).count() > 1
        {
            return vec![
                "usage: /harness connect PATH [--current]; quote a path containing spaces".into(),
            ];
        }
    }
    match sub.to_lowercase().as_str() {
        "help" | "?" => help_lines(),
        "status" | "list" => status_lines(session.store),
        "connect" => connect_cmd(session, rest, run),
        "verify" => verify_cmd(session, rest, run),
        "disconnect" | "remove" => disconnect_cmd(session, rest),
        "skill" => skill_cmd(session, rest),
        "skills-remove" => {
            if rest.len() != 1 || rest[0].starts_with("--") {
                return vec!["usage: /skills remove PATH; stops sharing and removes matching links across accounts".into()];
            }
            let path = session.cwd.join(&rest[0]);
            let source = fs::canonicalize(&path).unwrap_or(path);
            if let Err(error) = connections::unshare_skill_source(session.store, &source) {
                return vec![error];
            }
            let mut lines = vec![format!("stopped sharing skill source {}", source.display())];
            for (provider, home) in targets(session, true) {
                if let Some(home) = home {
                    match super::skills::remove(&source, &home) {
                        Ok(result) => lines.extend(result),
                        Err(error) => lines.push(format!(
                            "{} · {}: {error}; existing links retained for inspection",
                            provider.as_str(),
                            home.display()
                        )),
                    }
                }
            }
            lines
        }
        "skills" => {
            let source = rest
                .iter()
                .find(|arg| !arg.starts_with("--"))
                .map(|arg| session.cwd.join(arg))
                .unwrap_or_else(|| crate::test_guard::test_home().join(".agents/skills"));
            let source = match super::skills::validate_source(&source) {
                Ok(source) => source,
                Err(error) => return vec![format!("skill source unavailable: {error}")],
            };
            let all = !rest.iter().any(|arg| arg == "--current");
            let mut lines = Vec::new();
            for (provider, home) in targets(session, all) {
                let Some(home) = home else {
                    lines.push(format!("{}: no provider home", provider.as_str()));
                    continue;
                };
                match super::skills::install(&source, &home, cancel) {
                    Ok(result) => {
                        lines.extend(result.into_iter().map(|line| {
                            format!("{} · {}: {line}", provider.as_str(), home.display())
                        }))
                    }
                    Err(error) => lines.push(format!(
                        "{} · {}: {error}",
                        provider.as_str(),
                        home.display()
                    )),
                }
                if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                    return lines;
                }
            }
            if all {
                match connections::share_skill_source(session.store, &source) {
                    Ok(()) => lines.push(
                        "shared skill source saved; later accounts inherit it across projects"
                            .into(),
                    ),
                    Err(error) => lines.push(format!("shared skill source was not saved: {error}")),
                }
            }
            lines
        }
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
    fn argument_commands_without_arguments_open_guides() {
        let w = world();
        for command in [
            "connect",
            "verify",
            "disconnect",
            "remove",
            "skill",
            "skills",
            "tools",
        ] {
            let out = text(&dispatch(
                &session(&w, ProviderKind::Claude),
                command,
                &|_: &[String]| ok(),
            ));
            assert!(
                out.contains("Detected choices:")
                    && out.contains("Selection is validated")
                    && out.contains("Esc cancels"),
                "{command}: {out}"
            );
        }
    }

    #[test]
    fn connect_guide_detects_harness_checkout_and_explains_its_effect() {
        let w = world();
        fs::copy(
            w.tool.path().join("harness.json"),
            w.project.path().join("harness.json"),
        )
        .unwrap();
        let out = text(&dispatch(
            &session(&w, ProviderKind::Claude),
            "connect",
            &|_: &[String]| ok(),
        ));
        assert!(
            out.contains("harness.json") && out.contains("Harness tools"),
            "{out}"
        );
        assert!(
            out.contains("receipt") && out.contains("undo command"),
            "{out}"
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
        assert!(out.contains("no setup connected"), "{out}");
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
        assert!(status.contains("no setup connected"), "{status}");
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
