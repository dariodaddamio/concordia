//! The real `concordia` binary, driven as a user would from a shell.

use std::path::PathBuf;
use std::process::Command;

const CONCORDIA: &str = env!("CARGO_BIN_EXE_concordia");

fn pointer_state() -> (bool, Option<String>, Option<std::time::SystemTime>) {
    let p: PathBuf = concordia::update::current_pointer_path();
    let content = std::fs::read_to_string(&p).ok();
    let mtime = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
    (p.exists(), content, mtime)
}

#[test]
fn version_is_read_only_and_never_repoints_the_install() {
    // Running any copy of concordia (a debug build, an old slot) with --version must not change which
    // binary every launcher uses. This is the stale-build bug the pointer exists to prevent.
    let before = pointer_state();
    let out = Command::new(CONCORDIA).arg("--version").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("concordia "), "{text}");
    assert!(text.contains(concordia::version::BUILD_STAMP), "{text}");
    assert_eq!(
        pointer_state(),
        before,
        "--version changed the install pointer"
    );
}

#[test]
fn help_lists_the_surface_users_need() {
    let out = Command::new(CONCORDIA).arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for needle in [
        "/use [add]",
        "/copy",
        "/doctor",
        "concordia doctor",
        "Connect:",
        "Mouse:",
    ] {
        assert!(text.contains(needle), "help missing {needle}:\n{text}");
    }
}

#[test]
fn an_unknown_argument_exits_2_with_a_hint() {
    let out = Command::new(CONCORDIA).arg("--bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--help"));
}

#[test]
fn doctor_reports_and_its_exit_code_matches_its_findings() {
    let state = tempfile::tempdir().unwrap();
    let out = Command::new(CONCORDIA)
        .arg("doctor")
        .env("CONCORDIA_STATE_DIR", state.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("concordia doctor"), "{text}");
    assert!(text.contains("build"), "{text}");
    assert!(text.contains("state"), "{text}");
    // Exit 1 exactly when a FAIL line is present.
    let has_fail = text.lines().any(|l| l.starts_with("FAIL"));
    assert_eq!(
        out.status.code(),
        Some(if has_fail { 1 } else { 0 }),
        "{text}"
    );
    // Pointing at a scratch dir must not have touched real state: only doctor's probe file
    // (already removed) could ever land there.
    let leftovers: Vec<_> = std::fs::read_dir(state.path()).unwrap().flatten().collect();
    assert!(
        leftovers.is_empty(),
        "doctor left files behind: {leftovers:?}"
    );
}

#[test]
fn doctor_flags_an_unreadable_store_with_a_nonzero_exit() {
    let state = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(state.path().join(".ao")).unwrap();
    std::fs::write(state.path().join(".ao").join("store.json"), "{ broken").unwrap();
    let out = Command::new(CONCORDIA)
        .arg("doctor")
        .env("CONCORDIA_STATE_DIR", state.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("unreadable"), "{text}");
    // doctor is read-only: the broken file is still exactly as the user left it.
    assert_eq!(
        std::fs::read_to_string(state.path().join(".ao").join("store.json")).unwrap(),
        "{ broken"
    );
}
