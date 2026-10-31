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
    assert_eq!(out.exit, Some(0));
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
    assert!(
        missing.stderr.contains("could not start"),
        "{}",
        missing.stderr
    );
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
