//! Real child processes against the fake adapter: timeout, no shell, output caps, and the
//! connect and verify flows end to end through `concordia::harness`.

use concordia::harness::run::run_command;
use concordia::harness::{command, parse_manifest, Context};
use concordia::provider::enforcement::{self, Gate};
use concordia::store::ProviderKind;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, Instant};

const FAKE: &str = env!("CARGO_BIN_EXE_concordia-fake-harness");

#[test]
fn shared_skills_keep_one_current_source_across_providers_and_future_accounts() {
    use concordia::harness::{connections, skills};
    let root = tempfile::tempdir().unwrap();
    let state_file = root.path().join("harness/connections.json");
    std::fs::create_dir_all(state_file.parent().unwrap()).unwrap();
    std::fs::write(&state_file, r#"{"version":1,"connections":[{"name":"legacy","version":null,"manifestPath":"legacy-manifest","manifestSha256":"legacy-sha","skill":null,"entries":[] }]}"#).unwrap();
    assert!(connections::shared_skill_sources(root.path())
        .unwrap()
        .is_empty());
    assert!(connections::load(root.path()).unwrap()[0]
        .all_account_projects
        .is_empty());
    let source = root.path().join("source");
    std::fs::create_dir_all(source.join("review")).unwrap();
    std::fs::write(
        source.join("review/SKILL.md"),
        "---\nname: review\ndescription: Synthetic review\n---\nFirst revision\n",
    )
    .unwrap();
    let homes: Vec<_> = (0..4)
        .map(|index| {
            let home = root.path().join(format!("account-{index}"));
            std::fs::create_dir(&home).unwrap();
            home
        })
        .collect();
    let session = command::Session {
        store: root.path(),
        cwd: root.path(),
        provider: ProviderKind::Codex,
        home: Some(&homes[0]),
        accounts: vec![
            (ProviderKind::Codex, Some(homes[0].clone())),
            (ProviderKind::Claude, Some(homes[1].clone())),
            (ProviderKind::Cursor, Some(homes[2].clone())),
        ],
    };
    let no_adapter = |_: &[String]| panic!("skill links must not run a harness adapter");
    let lines = command::dispatch(
        &session,
        &format!("skills \"{}\"", source.display()),
        &no_adapter,
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("shared skill source saved")),
        "{lines:?}"
    );
    for home in &homes[..3] {
        assert!(home.join("skills/review/SKILL.md").is_file(), "{lines:?}");
    }
    assert_eq!(
        connections::shared_skill_sources(root.path())
            .unwrap()
            .len(),
        1
    );
    let upgraded: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&state_file).unwrap()).unwrap();
    assert_eq!(upgraded["version"], 2);
    assert_eq!(upgraded["sharedSkillSources"].as_array().unwrap().len(), 1);
    std::fs::write(source.join("review/SKILL.md"), "Second revision\n").unwrap();
    for home in &homes[..3] {
        assert_eq!(
            std::fs::read_to_string(home.join("skills/review/SKILL.md")).unwrap(),
            "Second revision\n"
        );
    }
    assert!(skills::needs_setup(root.path(), Some(&homes[3])).unwrap());
    skills::reconcile(
        root.path(),
        Some(&homes[3]),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert!(!skills::needs_setup(root.path(), Some(&homes[3])).unwrap());
    std::fs::create_dir_all(homes[0].join("skills/existing")).unwrap();
    std::fs::write(homes[0].join("skills/existing/SKILL.md"), "Owner skill").unwrap();
    std::fs::create_dir_all(source.join("existing")).unwrap();
    std::fs::write(source.join("existing/SKILL.md"), "Shared skill").unwrap();
    let result = skills::install(
        &source,
        &homes[0],
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert!(result
        .iter()
        .any(|line| line.contains("kept existing skill")));
    assert_eq!(
        std::fs::read_to_string(homes[0].join("skills/existing/SKILL.md")).unwrap(),
        "Owner skill"
    );
    let invalid = command::dispatch(&session, "skills --bogus", &no_adapter);
    assert!(invalid[0].contains("unsupported setup option"));
    command::dispatch(
        &session,
        &format!("skills-remove \"{}\"", source.display()),
        &no_adapter,
    );
    assert!(connections::shared_skill_sources(root.path())
        .unwrap()
        .is_empty());
    for home in &homes[..3] {
        assert!(!home.join("skills/review").exists());
    }
    assert_eq!(
        std::fs::read_to_string(homes[0].join("skills/existing/SKILL.md")).unwrap(),
        "Owner skill"
    );
    assert!(source.join("review/SKILL.md").is_file());
    connections::share_skill_source(root.path(), &source).unwrap();
    std::fs::rename(&source, root.path().join("renamed-source")).unwrap();
    command::dispatch(&session, "skills-remove source", &no_adapter);
    assert!(connections::shared_skill_sources(root.path())
        .unwrap()
        .is_empty());
    assert_eq!(connections::load(root.path()).unwrap()[0].name, "legacy");
    let unsupported = r#"{"version":99,"connections":[]}"#;
    std::fs::write(&state_file, unsupported).unwrap();
    assert!(connections::load(root.path()).is_err());
    assert!(connections::share_skill_source(root.path(), root.path()).is_err());
    assert_eq!(std::fs::read_to_string(&state_file).unwrap(), unsupported);
}

#[test]
fn shared_setup_reaches_providers_and_later_accounts_without_replaying_failed_or_changed_sources() {
    use concordia::harness::{connections, shared};
    let readme = include_str!("../../../README.md");
    let table = readme
        .split("<!-- COMMANDS:START -->")
        .nth(1)
        .unwrap()
        .split("<!-- COMMANDS:END -->")
        .next()
        .unwrap();
    assert_eq!(
        table.replace("\r\n", "\n").trim(),
        concordia::commands::markdown_command_table().trim()
    );
    for line in [
        "/skills list extra",
        "/hooks catalog extra",
        "/hooks status extra",
    ] {
        assert!(
            matches!(
                concordia::commands::parse_slash(line).unwrap().0,
                concordia::commands::SlashCommand::Invalid(_)
            ),
            "{line}"
        );
    }
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let homes: Vec<_> = (0..4)
        .map(|index| {
            let home = root.path().join(format!("home-{index}"));
            std::fs::create_dir(&home).unwrap();
            home
        })
        .collect();
    let manifest = serde_json::json!({"name":"shared-fixture","providers":{
        "codex":{"connect":[FAKE,"connect","{provider}","{home}","{project}"]},
        "claude":{"connect":[FAKE,"connect","{provider}","{home}","{project}"]},
        "cursor":{"connect":[FAKE,"connect","{provider}","{home}","{project}"]}
    }});
    let source = project.join("harness.json");
    std::fs::write(&source, manifest.to_string()).unwrap();
    let session = command::Session {
        store: root.path(),
        cwd: &project,
        provider: ProviderKind::Codex,
        home: Some(&homes[0]),
        accounts: vec![
            (ProviderKind::Codex, Some(homes[0].clone())),
            (ProviderKind::Claude, Some(homes[1].clone())),
            (ProviderKind::Cursor, Some(homes[2].clone())),
        ],
    };
    let run = |args: &[String]| run_command(args, &project, Duration::from_secs(10));
    let lines = command::dispatch(&session, "connect harness.json", &run);
    assert!(
        lines.iter().any(|line| line.contains("shared setup saved")),
        "{lines:?}"
    );
    for home in &homes[..3] {
        assert!(home.join("harness-connected").is_file());
    }
    assert_eq!(connections::load(root.path()).unwrap()[0].entries.len(), 3);
    let result = shared::reconcile_account(
        root.path(),
        ProviderKind::Claude,
        Some(&homes[3]),
        &project,
        &run,
    )
    .unwrap();
    assert_eq!(result.len(), 1);
    assert!(homes[3].join("harness-connected").is_file());
    let never_run = |_: &[String]| panic!("already-connected setup must not execute again");
    assert!(shared::reconcile_account(
        root.path(),
        ProviderKind::Claude,
        Some(&homes[3]),
        &project,
        &never_run
    )
    .unwrap()
    .is_empty());
    let failed_home = root.path().join("failed-home");
    std::fs::create_dir(&failed_home).unwrap();
    let failed = |_: &[String]| concordia::harness::RunOutcome {
        exit: Some(1),
        stderr: "synthetic failure".into(),
        ..Default::default()
    };
    shared::reconcile_account(
        root.path(),
        ProviderKind::Cursor,
        Some(&failed_home),
        &project,
        &failed,
    )
    .unwrap();
    let stopped = shared::reconcile_account(
        root.path(),
        ProviderKind::Cursor,
        Some(&failed_home),
        &project,
        &never_run,
    )
    .unwrap();
    assert!(stopped[0].contains("failed previously"));
    std::fs::write(&source, format!("{}\n", manifest)).unwrap();
    assert!(shared::reconcile_account(
        root.path(),
        ProviderKind::Cursor,
        Some(&homes[3]),
        &project,
        &never_run
    )
    .is_err());
    let connection = connections::load(root.path()).unwrap().remove(0);
    let entry = connection
        .entries
        .iter()
        .find(|entry| entry.provider == "codex")
        .unwrap();
    connections::remove_action_target(
        root.path(),
        &connections::ActionTarget::from_connection(&connection, entry),
    )
    .unwrap();
    assert!(shared::reconcile_account(
        root.path(),
        ProviderKind::Cursor,
        Some(&homes[3]),
        &project,
        &never_run
    )
    .unwrap()
    .is_empty());
}

fn argv(parts: &[&str]) -> Vec<String> {
    let mut v = vec![FAKE.to_string()];
    v.extend(parts.iter().map(|s| s.to_string()));
    v
}

#[test]
fn a_hanging_command_is_killed_at_the_timeout() {
    let start = Instant::now();
    let out = run_command(&argv(&["hang"]), Path::new("."), Duration::from_millis(400));
    assert!(out.timed_out);
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "the timeout must be enforced"
    );
}

#[test]
fn a_timed_out_adapter_does_not_leave_a_running_descendant() {
    let marker = tempfile::NamedTempFile::new().unwrap();
    let path = marker.path().to_path_buf();
    std::fs::remove_file(&path).unwrap();
    let out = run_command(
        &argv(&["tree-hang", &path.to_string_lossy()]),
        Path::new("."),
        Duration::from_millis(300),
    );
    assert!(out.timed_out);
    std::thread::sleep(Duration::from_millis(2500));
    assert!(
        std::fs::read_to_string(path).ok().as_deref() != Some("survived"),
        "descendant survived"
    );
}

#[test]
fn adapter_environment_is_allowlisted_and_manifest_env_is_forwarded() {
    std::env::set_var("ANTHROPIC_API_KEY", "secret-test-value");
    std::env::set_var("HARNESS_TEST_EXTRA", "extra-test-value");
    let out = run_command(
        &argv(&[
            "env-check",
            "ANTHROPIC_API_KEY",
            "HARNESS_TEST_EXTRA",
            "PATH",
        ]),
        Path::new("."),
        Duration::from_secs(5),
    );
    assert!(!out.stdout.contains("secret-test-value"));
    assert!(out.stdout.lines().any(|l| l.starts_with("PATH=")));
    let manifest = parse_manifest(
        r#"{"name":"x","providers":{"claude":{"connect":["h"],"env":["HARNESS_TEST_EXTRA"]}}}"#,
        Path::new("."),
    )
    .unwrap();
    assert_eq!(manifest.providers["claude"].env, ["HARNESS_TEST_EXTRA"]);
    std::env::remove_var("ANTHROPIC_API_KEY");
    std::env::remove_var("HARNESS_TEST_EXTRA");
}

#[test]
fn arguments_reach_the_program_literally_with_no_shell() {
    let out = run_command(
        &argv(&["echo-args", "a;b", "$(whoami)", "x && y", "`id`", "*"]),
        Path::new("."),
        Duration::from_secs(20),
    );
    assert_eq!(out.exit, Some(0));
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines, ["a;b", "$(whoami)", "x && y", "`id`", "*"]);
}

#[test]
fn output_is_capped_and_a_missing_program_is_reported_not_panicked() {
    let out = run_command(&argv(&["flood"]), Path::new("."), Duration::from_secs(20));
    assert_eq!(out.exit, None);
    assert!(out.stderr.contains("output exceeds"), "{}", out.stderr);
    assert!(
        out.stdout.len() <= 70_000,
        "stdout was {} bytes",
        out.stdout.len()
    );
    let missing = run_command(
        &["definitely-not-a-real-program-xyz".to_string()],
        Path::new("."),
        Duration::from_secs(5),
    );
    assert_eq!(missing.exit, None);
    assert!(!missing.stderr.trim().is_empty(), "{}", missing.stderr);
}

fn manifest_text() -> String {
    let fake = FAKE.replace('\\', "/");
    format!(
        r#"{{"name":"fake","version":"1.0","providers":{{"claude":{{
            "connect":["{fake}","connect","{{provider}}","{{home}}","{{project}}"],
            "verify":["{fake}","verify","{{provider}}","{{home}}","{{project}}"]}}}}}}"#
    )
}

#[test]
fn connect_runs_the_adapter_and_records_the_result() {
    let store = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let manifest = parse_manifest(&manifest_text(), Path::new(".")).unwrap();
    let manifest_path = project.path().join("harness.json");
    std::fs::write(&manifest_path, manifest_text()).unwrap();
    let sha = Sha256::digest(manifest_text().as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let ctx = Context {
        provider: ProviderKind::Claude,
        home: Some(home.path()),
        project: project.path(),
    };
    let run = |a: &[String]| run_command(a, Path::new("."), Duration::from_secs(20));
    let entry = command::connect_in(store.path(), &manifest, &manifest_path, &sha, &ctx, &run);
    assert!(entry.connect_ok, "{}", entry.connect_note);
    let written = std::fs::read_to_string(home.path().join("harness-connected")).unwrap();
    assert!(written.starts_with("claude|"), "{written}");
    let lines = command::status_lines(store.path());
    assert!(
        lines
            .iter()
            .any(|l| l.contains("fake") && l.contains("claude") && l.contains("connected")),
        "{lines:?}"
    );
}

#[test]
fn verify_end_to_end_opens_the_gate_only_for_a_real_matching_report() {
    let store = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join("docs/quality")).unwrap();
    std::fs::write(
        project.path().join("docs/quality/locked-code.json"),
        "{\"controls\":[]}\n",
    )
    .unwrap();
    let manifest = parse_manifest(&manifest_text(), Path::new(".")).unwrap();
    let manifest_path = project.path().join("harness.json");
    std::fs::write(&manifest_path, manifest_text()).unwrap();
    let sha = Sha256::digest(manifest_text().as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let ctx = Context {
        provider: ProviderKind::Claude,
        home: Some(home.path()),
        project: project.path(),
    };
    let gate = || {
        enforcement::gate_in(
            store.path(),
            project.path(),
            ProviderKind::Claude,
            Some(home.path()),
        )
    };
    let run = |a: &[String]| run_command(a, Path::new("."), Duration::from_secs(20));
    assert!(matches!(gate(), Gate::ReadOnly(_)));
    let verdict = command::verify_in(store.path(), &manifest, &manifest_path, &sha, &ctx, &run);
    assert_eq!(verdict, concordia::harness::verify::Verdict::Verified);
    assert_eq!(gate(), Gate::Open);
    assert!(command::status_lines(store.path())
        .iter()
        .any(|l| l.contains("verified")));
}

/// npm installs Node CLIs on Windows as `name.cmd`; a program named without its extension must
/// still start, as `harness` does in a shell.
#[cfg(windows)]
#[test]
fn a_cmd_shim_starts_by_its_bare_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("fake-shim.cmd"),
        "@echo off\r\necho shim says %1\r\n",
    )
    .unwrap();
    let bare = dir.path().join("fake-shim").to_string_lossy().into_owned();
    let out = run_command(&[bare, "hello".into()], dir.path(), Duration::from_secs(20));
    assert_eq!(out.exit, Some(0), "{}", out.stderr);
    assert_eq!(out.stdout.trim(), "shim says hello");
}
