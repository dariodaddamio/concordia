//! Launch gate for projects that protect code regions.
//!
//! A project that carries `docs/quality/locked-code.json` asks its agents to respect LOCKED
//! regions. Installed hook files and capability flags do not prove that a provider client will
//! actually refuse an edit, so the gate treats enforcement as unverified until a receipt says a
//! live probe saw the denial. The receipt is bound to the provider, its isolated home and the
//! current policy revision, so switching any of them closes the gate again. Without a matching
//! receipt a write-capable turn drops to read-only planning and says why.
//!
//! Receipts live in Concordia's own state directory, keyed by project, never inside the project,
//! so an agent that can only write the project cannot forge one. The first time the gate sees a
//! policy file it records that the project was locked, so deleting the file does not unlock it.
//! A provider home inside the project is not isolated and never opens the gate. The receipt is
//! written by the Harness verifier, never by an agent. Receipts are also bound to the connected
//! harness manifest identity, including a current hash for file manifests.

use crate::store::{ProviderKind, TrustMode};
use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub(crate) const POLICY_FILE: &str = "docs/quality/locked-code.json";
const STORE_DIR: &str = "enforcement";
const LOCKED_MARKER: &str = "locked";
const PROVEN: &str = "denied-live-probe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// The project has no locked regions, or enforcement was verified for this provider and home.
    Open,
    /// Write-capable work must not start; the text says why.
    ReadOnly(String),
}

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// SHA-256 of the policy file with line endings normalized, so a checkout setting cannot change it.
pub fn policy_revision(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    sha_hex(text.as_bytes())
}

/// A path as a comparable string: forward slashes, no trailing slash, lowercase on Windows.
pub(crate) fn path_key(path: &Path) -> String {
    let key = path.to_string_lossy().replace('\\', "/");
    let key = key.trim_end_matches('/').to_string();
    if cfg!(windows) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

/// The directory name that holds this project's receipts and lock marker in the store.
pub fn project_key(root: &Path) -> String {
    sha_hex(path_key(root).as_bytes())
}

pub(crate) fn project_root(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .find(|dir| dir.join(POLICY_FILE).is_file())
        .map(Path::to_path_buf)
}

pub(crate) fn slot(store: &Path, root: &Path) -> PathBuf {
    store.join(STORE_DIR).join(project_key(root))
}

/// Refuse state writes through symlinks/junctions, including an existing slot itself.
pub fn has_reparse_component(base: &Path, target: &Path) -> bool {
    let Ok(relative) = target.strip_prefix(base) else {
        return true;
    };
    let mut current = base.to_path_buf();
    if std::fs::symlink_metadata(&current).is_ok_and(|m| is_reparse(&m)) {
        return true;
    }
    for component in relative.components() {
        current.push(component);
        if std::fs::symlink_metadata(&current).is_ok_and(|m| is_reparse(&m)) {
            return true;
        }
    }
    false
}

fn is_reparse(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

/// Whether the project was seen locked before, from `cwd` or any parent.
fn was_locked(store: &Path, cwd: &Path) -> bool {
    cwd.ancestors()
        .any(|dir| slot(store, dir).join(LOCKED_MARKER).is_file())
}

/// Best effort: a failed write only means a later deletion would not be remembered.
fn remember_locked(slot: &Path, revision: &str) {
    let Some(state_dir) = slot.parent() else {
        return;
    };
    if has_reparse_component(state_dir, slot) {
        return;
    }
    if std::fs::create_dir_all(slot).is_ok() {
        let _ = std::fs::write(slot.join(LOCKED_MARKER), revision);
    }
}

pub(crate) fn is_inside(path: &Path, root: &Path) -> bool {
    let (path, root) = (path_key(path), path_key(root));
    path == root || path.starts_with(&format!("{root}/"))
}

fn receipt_matches(
    store: &Path,
    root: &Path,
    slot: &Path,
    provider: ProviderKind,
    home: Option<&Path>,
    revision: &str,
) -> bool {
    let Ok(text) = std::fs::read_to_string(slot.join(format!("{}.json", provider.as_str()))) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    let recorded_home = value
        .get("home")
        .and_then(|h| h.as_str())
        .map(|h| path_key(Path::new(h)));
    let harness = value.get("harness").and_then(|v| v.as_str());
    let manifest_path = value.get("harnessManifestPath").and_then(|v| v.as_str());
    let manifest_sha = value.get("harnessManifestSha256").and_then(|v| v.as_str());
    let harness_version = match value.get("harnessVersion") {
        Some(value) if value.is_null() => Some(None),
        Some(value) => value.as_str().map(Some),
        None => None,
    };
    value.get("provider").and_then(|v| v.as_str()) == Some(provider.as_str())
        && value.get("result").and_then(|v| v.as_str()) == Some(PROVEN)
        && value.get("policyRevision").and_then(|v| v.as_str()) == Some(revision)
        && recorded_home == home.map(path_key)
        && harness.is_some_and(|name| {
            manifest_path.is_some_and(|path| {
                manifest_sha.is_some_and(|sha| {
                    harness_version.is_some_and(|version| {
                        crate::harness::connections::identity_matches(
                            store,
                            &crate::harness::connections::ReceiptTarget {
                                provider: provider.as_str(),
                                project: root,
                                home,
                            },
                            &crate::harness::connections::ReceiptIdentity {
                                name,
                                version,
                                manifest_path: path,
                                manifest_sha256: sha,
                            },
                            &|program| refresh_command_identity(program, root),
                        )
                    })
                })
            })
        })
}

fn refresh_command_identity(
    program: &str,
    cwd: &Path,
) -> Option<crate::harness::connections::RefreshedIdentity> {
    if crate::harness::adapter::is_harness_program(program) {
        let (manifest, text) = crate::harness::adapter::build(program, cwd, &|argv| {
            crate::harness::run::run_command(argv, cwd, crate::harness::run::DEFAULT_TIMEOUT)
        })
        .ok()?;
        let sha = sha_hex(text.as_bytes());
        return Some(crate::harness::connections::RefreshedIdentity {
            name: manifest.name,
            version: manifest.version,
            manifest_sha256: sha,
        });
    }
    let outcome = crate::harness::run::run_command(
        &[program.to_string(), "concordia-manifest".into()],
        cwd,
        crate::harness::run::DEFAULT_TIMEOUT,
    );
    if outcome.timed_out || outcome.exit != Some(0) {
        return None;
    }
    let manifest = crate::harness::parse_manifest(&outcome.stdout, cwd).ok()?;
    let sha = sha_hex(outcome.stdout.as_bytes());
    Some(crate::harness::connections::RefreshedIdentity {
        name: manifest.name,
        version: manifest.version,
        manifest_sha256: sha,
    })
}

/// Where receipts live: Concordia's state directory (a throwaway one under test).
fn store_root() -> PathBuf {
    #[cfg(not(test))]
    {
        crate::store::default_store_root()
    }
    #[cfg(test)]
    {
        static DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        DIR.get_or_init(|| tempfile::tempdir().expect("test store"))
            .path()
            .to_path_buf()
    }
}

/// Decide whether `provider` may do write-capable work from `cwd`. Any read failure closes it.
pub fn gate(cwd: &Path, provider: ProviderKind, home: Option<&Path>) -> Gate {
    gate_in(&store_root(), cwd, provider, home)
}

/// [`gate`] against an explicit state directory.
pub fn gate_in(store: &Path, cwd: &Path, provider: ProviderKind, home: Option<&Path>) -> Gate {
    let name = provider.as_str();
    let Some(root) = project_root(cwd) else {
        if was_locked(store, cwd) {
            return Gate::ReadOnly(format!(
                "{name}: the locked-code policy file is missing, but this project was locked before, so this turn is read-only (plan). Restore docs/quality/locked-code.json."
            ));
        }
        return Gate::Open;
    };
    let revision = std::fs::read(root.join(POLICY_FILE))
        .map(|bytes| policy_revision(&bytes))
        .unwrap_or_default();
    let slot = slot(store, &root);
    remember_locked(&slot, &revision);
    if home.is_some_and(|h| is_inside(h, &root)) {
        return Gate::ReadOnly(format!(
            "{name}: the provider home is inside the project, so it is not isolated and this turn is read-only (plan)."
        ));
    }
    if !revision.is_empty() && receipt_matches(store, &root, &slot, provider, home, &revision) {
        return Gate::Open;
    }
    Gate::ReadOnly(format!(
        "{name}: locked-code enforcement is not verified for this home and policy, so this turn is read-only (plan). Run the Harness enforcement check to allow edits."
    ))
}

/// The trust a turn may run with, and the notice to show when it was lowered.
pub fn effective_trust(
    cwd: &Path,
    provider: ProviderKind,
    home: Option<&Path>,
    requested: TrustMode,
) -> (TrustMode, Option<String>) {
    effective_trust_in(&store_root(), cwd, provider, home, requested)
}

/// [`effective_trust`] against an explicit state directory.
pub fn effective_trust_in(
    store: &Path,
    cwd: &Path,
    provider: ProviderKind,
    home: Option<&Path>,
    requested: TrustMode,
) -> (TrustMode, Option<String>) {
    if requested == TrustMode::Plan {
        return (TrustMode::Plan, None);
    }
    match gate_in(store, cwd, provider, home) {
        Gate::Open => (requested, None),
        Gate::ReadOnly(reason) => (TrustMode::Plan, Some(reason)),
    }
}

/// Whether a final command line carries the provider's read-only switch. Capability fallbacks
/// can drop it, so this is checked after planning, not assumed from the requested trust.
pub fn is_read_only_argv(provider: ProviderKind, argv: &[String]) -> bool {
    let pair = |flag: &[&str], value: &str| {
        argv.windows(2)
            .any(|w| flag.contains(&w[0].as_str()) && w[1] == value)
    };
    match provider {
        ProviderKind::Claude => pair(&["--permission-mode"], "plan"),
        ProviderKind::Cursor => pair(&["--mode"], "plan"),
        ProviderKind::Codex => pair(&["-s", "--sandbox"], "read-only"),
    }
}

/// Refuse to start a turn the gate made read-only if the command line no longer is.
pub fn ensure_read_only(provider: ProviderKind, argv: &[String]) -> Result<()> {
    if is_read_only_argv(provider, argv) {
        return Ok(());
    }
    bail!(
        "{}: this CLI cannot run read-only here, so Concordia will not start a write-capable turn in a locked-code project. Update the CLI or verify enforcement first.",
        provider.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::connections;
    use crate::store::{ProviderKind, TrustMode};
    use std::fs;
    use std::path::Path;

    const POLICY: &str = "{\"controls\":[]}\n";

    fn locked_project(dir: &Path) {
        fs::create_dir_all(dir.join("docs/quality")).unwrap();
        fs::write(dir.join("docs/quality/locked-code.json"), POLICY).unwrap();
    }

    /// Write a receipt where the verifier would: in Concordia's own store, outside the project.
    fn receipt(
        store: &Path,
        root: &Path,
        provider: &str,
        home: Option<&str>,
        revision: &str,
        result: &str,
    ) {
        let dir = store.join("enforcement").join(project_key(root));
        fs::create_dir_all(&dir).unwrap();
        let manifest_path = root.join("harness.json");
        fs::write(&manifest_path, "{}").unwrap();
        let manifest_path_text = manifest_path.to_string_lossy().into_owned();
        let manifest_sha256 = sha_hex(b"{}");
        let body = serde_json::json!({
            "provider": provider,
            "home": home,
            "policyRevision": revision,
            "result": result,
            "harness": "h1",
            "harnessVersion": "1.0",
            "harnessManifestPath": manifest_path_text,
            "harnessManifestSha256": manifest_sha256,
        });
        fs::write(dir.join(format!("{provider}.json")), body.to_string()).unwrap();
        connections::record(
            store,
            &connections::Identity {
                name: "h1",
                version: Some("1.0"),
                manifest_path: &manifest_path_text,
                manifest_sha256: &manifest_sha256,
                skill: None,
            },
            connections::Entry {
                provider: provider.into(),
                home: home.map(str::to_string),
                project: root.to_string_lossy().into_owned(),
                connect_ok: true,
                connect_note: "ok".into(),
                verify: "verified".into(),
                verify_note: String::new(),
                at: 0,
            },
        )
        .unwrap();
    }

    fn revision() -> String {
        policy_revision(POLICY.as_bytes())
    }

    fn dirs2() -> (tempfile::TempDir, tempfile::TempDir) {
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap())
    }

    #[test]
    fn a_project_without_locked_regions_is_never_gated() {
        let (store, dir) = dirs2();
        let got = gate_in(store.path(), dir.path(), ProviderKind::Claude, None);
        assert_eq!(got, Gate::Open);
    }

    #[test]
    fn a_locked_project_without_a_receipt_is_read_only_with_a_reason() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        match gate_in(store.path(), dir.path(), ProviderKind::Claude, None) {
            Gate::ReadOnly(reason) => {
                assert!(reason.contains("claude"), "{reason}");
                assert!(reason.contains("not verified"), "{reason}");
            }
            Gate::Open => panic!("an unverified locked project must not be writable"),
        }
    }

    #[test]
    fn a_matching_receipt_opens_the_gate() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        receipt(
            store.path(),
            dir.path(),
            "claude",
            None,
            &revision(),
            "denied-live-probe",
        );
        assert_eq!(
            gate_in(store.path(), dir.path(), ProviderKind::Claude, None),
            Gate::Open
        );
    }

    #[test]
    fn a_changed_connected_harness_identity_closes_the_gate() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        receipt(
            store.path(),
            dir.path(),
            "claude",
            None,
            &revision(),
            "denied-live-probe",
        );
        assert_eq!(
            gate_in(store.path(), dir.path(), ProviderKind::Claude, None),
            Gate::Open
        );
        let manifest_path = dir.path().join("harness.json");
        let manifest_path_text = manifest_path.to_string_lossy().into_owned();
        connections::record(
            store.path(),
            &connections::Identity {
                name: "h1",
                version: Some("2.0"),
                manifest_path: &manifest_path_text,
                manifest_sha256: "new-manifest",
                skill: None,
            },
            connections::Entry {
                provider: "claude".into(),
                home: None,
                project: dir.path().to_string_lossy().into_owned(),
                connect_ok: true,
                connect_note: "ok".into(),
                verify: "verified".into(),
                verify_note: String::new(),
                at: 1,
            },
        )
        .unwrap();
        assert!(matches!(
            gate_in(store.path(), dir.path(), ProviderKind::Claude, None),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn a_receipt_written_inside_the_project_is_ignored() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        let forged = dir.path().join(".harness/enforcement");
        fs::create_dir_all(&forged).unwrap();
        let body = serde_json::json!({
            "provider": "claude", "home": null,
            "policyRevision": revision(), "result": "denied-live-probe",
        });
        fs::write(forged.join("claude.json"), body.to_string()).unwrap();
        assert!(matches!(
            gate_in(store.path(), dir.path(), ProviderKind::Claude, None),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn deleting_the_policy_file_does_not_open_a_project_that_was_locked() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        assert!(matches!(
            gate_in(store.path(), dir.path(), ProviderKind::Claude, None),
            Gate::ReadOnly(_)
        ));
        fs::remove_file(dir.path().join("docs/quality/locked-code.json")).unwrap();
        match gate_in(store.path(), dir.path(), ProviderKind::Claude, None) {
            Gate::ReadOnly(reason) => assert!(reason.contains("missing"), "{reason}"),
            Gate::Open => panic!("removing the policy file must not unlock the project"),
        }
    }

    #[test]
    fn the_gate_finds_the_project_from_a_nested_directory() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        let nested = dir.path().join("crates/app/src");
        fs::create_dir_all(&nested).unwrap();
        assert!(matches!(
            gate_in(store.path(), &nested, ProviderKind::Codex, None),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn a_receipt_for_another_provider_does_not_carry_over() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        receipt(
            store.path(),
            dir.path(),
            "claude",
            None,
            &revision(),
            "denied-live-probe",
        );
        assert!(matches!(
            gate_in(store.path(), dir.path(), ProviderKind::Codex, None),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn a_receipt_for_another_home_does_not_carry_over() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        receipt(
            store.path(),
            dir.path(),
            "claude",
            Some("C:/Users/fixture-user/.claude-a"),
            &revision(),
            "denied-live-probe",
        );
        let other = Path::new("C:/Users/fixture-user/.claude-b");
        assert!(matches!(
            gate_in(store.path(), dir.path(), ProviderKind::Claude, Some(other)),
            Gate::ReadOnly(_)
        ));
        let same = Path::new("C:\\Users\\fixture-user\\.claude-a");
        assert_eq!(
            gate_in(store.path(), dir.path(), ProviderKind::Claude, Some(same)),
            Gate::Open,
            "slash style must not matter"
        );
    }

    #[test]
    fn a_home_inside_the_project_is_not_isolated_even_with_a_receipt() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        let home = dir.path().join(".claude-home");
        receipt(
            store.path(),
            dir.path(),
            "claude",
            Some(&home.to_string_lossy()),
            &revision(),
            "denied-live-probe",
        );
        match gate_in(store.path(), dir.path(), ProviderKind::Claude, Some(&home)) {
            Gate::ReadOnly(reason) => assert!(reason.contains("isolated"), "{reason}"),
            Gate::Open => panic!("a home inside the project must not count as isolated"),
        }
        assert!(matches!(
            gate_in(
                store.path(),
                dir.path(),
                ProviderKind::Claude,
                Some(dir.path())
            ),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn a_stale_policy_revision_closes_the_gate_again() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        receipt(
            store.path(),
            dir.path(),
            "claude",
            None,
            &revision(),
            "denied-live-probe",
        );
        fs::write(
            dir.path().join("docs/quality/locked-code.json"),
            "{\"controls\":[{\"path\":\"a\"}]}\n",
        )
        .unwrap();
        assert!(matches!(
            gate_in(store.path(), dir.path(), ProviderKind::Claude, None),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn a_receipt_that_did_not_prove_a_denial_is_not_verification() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        receipt(
            store.path(),
            dir.path(),
            "claude",
            None,
            &revision(),
            "hooks-installed",
        );
        assert!(matches!(
            gate_in(store.path(), dir.path(), ProviderKind::Claude, None),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn a_corrupt_receipt_fails_closed() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        let slot = store
            .path()
            .join("enforcement")
            .join(project_key(dir.path()));
        fs::create_dir_all(&slot).unwrap();
        fs::write(slot.join("claude.json"), "{nope").unwrap();
        assert!(matches!(
            gate_in(store.path(), dir.path(), ProviderKind::Claude, None),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn line_endings_do_not_change_the_policy_revision() {
        assert_eq!(policy_revision(b"a\r\nb\r\n"), policy_revision(b"a\nb\n"));
    }

    #[test]
    fn write_capable_trust_drops_to_plan_with_a_notice() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        for trust in [TrustMode::Ask, TrustMode::Workspace, TrustMode::Full] {
            let (effective, notice) =
                effective_trust_in(store.path(), dir.path(), ProviderKind::Claude, None, trust);
            assert_eq!(effective, TrustMode::Plan, "{trust:?}");
            assert!(notice.unwrap().contains("read-only"));
        }
    }

    #[test]
    fn plan_trust_needs_no_notice_and_a_verified_project_keeps_its_trust() {
        let (store, dir) = dirs2();
        locked_project(dir.path());
        let (t, n) = effective_trust_in(
            store.path(),
            dir.path(),
            ProviderKind::Claude,
            None,
            TrustMode::Plan,
        );
        assert_eq!((t, n), (TrustMode::Plan, None));
        receipt(
            store.path(),
            dir.path(),
            "claude",
            None,
            &revision(),
            "denied-live-probe",
        );
        let (t, n) = effective_trust_in(
            store.path(),
            dir.path(),
            ProviderKind::Claude,
            None,
            TrustMode::Full,
        );
        assert_eq!((t, n), (TrustMode::Full, None));
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn only_a_provable_read_only_command_line_passes() {
        use ProviderKind::{Claude, Codex, Cursor};
        assert!(is_read_only_argv(
            Claude,
            &argv(&["-p", "--permission-mode", "plan", "hi"])
        ));
        assert!(!is_read_only_argv(
            Claude,
            &argv(&["-p", "--permission-mode", "manual", "hi"])
        ));
        assert!(!is_read_only_argv(Claude, &argv(&["-p", "hi"])));
        assert!(is_read_only_argv(Cursor, &argv(&["--mode", "plan", "hi"])));
        assert!(!is_read_only_argv(
            Cursor,
            &argv(&["--output-format", "text", "hi"])
        ));
        assert!(is_read_only_argv(
            Codex,
            &argv(&["exec", "-s", "read-only", "hi"])
        ));
        assert!(is_read_only_argv(
            Codex,
            &argv(&["exec", "--sandbox", "read-only", "hi"])
        ));
        assert!(!is_read_only_argv(
            Codex,
            &argv(&["exec", "-s", "workspace-write", "hi"])
        ));
    }

    #[test]
    fn ensure_read_only_refuses_a_weaker_command_line_with_a_clear_error() {
        let err = ensure_read_only(ProviderKind::Cursor, &argv(&["hi"])).unwrap_err();
        let text = err.to_string();
        assert!(
            text.contains("cursor") && text.contains("read-only"),
            "{text}"
        );
        assert!(ensure_read_only(ProviderKind::Cursor, &argv(&["--mode", "plan", "hi"])).is_ok());
    }
}
