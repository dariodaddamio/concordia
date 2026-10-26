//! Launch gate for projects that protect code regions.
//!
//! A project that carries `docs/quality/locked-code.json` asks its agents to respect LOCKED
//! regions. Installed hook files and capability flags do not prove that a provider client will
//! actually refuse an edit, so the gate treats enforcement as unverified until a receipt says a
//! live probe saw the denial. The receipt is bound to the provider, its isolated home and the
//! current policy revision, so switching any of them closes the gate again. Without a matching
//! receipt a write-capable turn drops to read-only planning and says why.
//!
//! The receipt is written by the Harness verifier, never by an agent. Runtime version binding is
//! not part of this check yet.

use crate::store::{ProviderKind, TrustMode};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const POLICY_FILE: &str = "docs/quality/locked-code.json";
const RECEIPT_DIR: &str = ".opifex/enforcement";
const PROVEN: &str = "denied-live-probe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// The project has no locked regions, or enforcement was verified for this provider and home.
    Open,
    /// Write-capable work must not start; the text says why.
    ReadOnly(String),
}

/// SHA-256 of the policy file with line endings normalized, so a checkout setting cannot change it.
pub fn policy_revision(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn project_root(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .find(|dir| dir.join(POLICY_FILE).is_file())
        .map(Path::to_path_buf)
}

fn home_key(home: Option<&Path>) -> Option<String> {
    home.map(|h| {
        let key = h.to_string_lossy().replace('\\', "/");
        let key = key.trim_end_matches('/').to_string();
        if cfg!(windows) {
            key.to_ascii_lowercase()
        } else {
            key
        }
    })
}

fn receipt_matches(
    root: &Path,
    provider: ProviderKind,
    home: Option<&Path>,
    revision: &str,
) -> bool {
    let path = root
        .join(RECEIPT_DIR)
        .join(format!("{}.json", provider.as_str()));
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    let recorded_home = value
        .get("home")
        .and_then(|h| h.as_str())
        .map(|h| home_key(Some(Path::new(h))).unwrap_or_default());
    value.get("provider").and_then(|v| v.as_str()) == Some(provider.as_str())
        && value.get("result").and_then(|v| v.as_str()) == Some(PROVEN)
        && value.get("policyRevision").and_then(|v| v.as_str()) == Some(revision)
        && recorded_home == home_key(home)
}

/// Decide whether `provider` may do write-capable work from `cwd`. Any read failure closes it.
pub fn gate(cwd: &Path, provider: ProviderKind, home: Option<&Path>) -> Gate {
    let Some(root) = project_root(cwd) else {
        return Gate::Open;
    };
    let revision = std::fs::read(root.join(POLICY_FILE))
        .map(|bytes| policy_revision(&bytes))
        .unwrap_or_default();
    if !revision.is_empty() && receipt_matches(&root, provider, home, &revision) {
        return Gate::Open;
    }
    Gate::ReadOnly(format!(
        "{}: locked-code enforcement is not verified for this home and policy, so this turn is read-only (plan). Run the Harness enforcement check to allow edits.",
        provider.as_str()
    ))
}

/// The trust a turn may run with, and the notice to show when it was lowered.
pub fn effective_trust(
    cwd: &Path,
    provider: ProviderKind,
    home: Option<&Path>,
    requested: TrustMode,
) -> (TrustMode, Option<String>) {
    if requested == TrustMode::Plan {
        return (TrustMode::Plan, None);
    }
    match gate(cwd, provider, home) {
        Gate::Open => (requested, None),
        Gate::ReadOnly(reason) => (TrustMode::Plan, Some(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{ProviderKind, TrustMode};
    use std::fs;
    use std::path::Path;

    const POLICY: &str = "{\"controls\":[]}\n";

    fn locked_project(dir: &Path) {
        fs::create_dir_all(dir.join("docs/quality")).unwrap();
        fs::write(dir.join("docs/quality/locked-code.json"), POLICY).unwrap();
    }

    fn receipt(dir: &Path, provider: &str, home: Option<&str>, revision: &str, result: &str) {
        fs::create_dir_all(dir.join(".opifex/enforcement")).unwrap();
        let body = serde_json::json!({
            "provider": provider,
            "home": home,
            "policyRevision": revision,
            "result": result,
        });
        fs::write(
            dir.join(format!(".opifex/enforcement/{provider}.json")),
            body.to_string(),
        )
        .unwrap();
    }

    fn revision() -> String {
        policy_revision(POLICY.as_bytes())
    }

    #[test]
    fn a_project_without_locked_regions_is_never_gated() {
        let dir = tempfile::tempdir().unwrap();
        let gate = gate(dir.path(), ProviderKind::Claude, None);
        assert_eq!(gate, Gate::Open);
    }

    #[test]
    fn a_locked_project_without_a_receipt_is_read_only_with_a_reason() {
        let dir = tempfile::tempdir().unwrap();
        locked_project(dir.path());
        match gate(dir.path(), ProviderKind::Claude, None) {
            Gate::ReadOnly(reason) => {
                assert!(reason.contains("claude"), "{reason}");
                assert!(reason.contains("not verified"), "{reason}");
            }
            Gate::Open => panic!("an unverified locked project must not be writable"),
        }
    }

    #[test]
    fn a_matching_receipt_opens_the_gate() {
        let dir = tempfile::tempdir().unwrap();
        locked_project(dir.path());
        receipt(dir.path(), "claude", None, &revision(), "denied-live-probe");
        assert_eq!(gate(dir.path(), ProviderKind::Claude, None), Gate::Open);
    }

    #[test]
    fn the_gate_finds_the_project_from_a_nested_directory() {
        let dir = tempfile::tempdir().unwrap();
        locked_project(dir.path());
        let nested = dir.path().join("crates/app/src");
        fs::create_dir_all(&nested).unwrap();
        assert!(matches!(
            gate(&nested, ProviderKind::Codex, None),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn a_receipt_for_another_provider_does_not_carry_over() {
        let dir = tempfile::tempdir().unwrap();
        locked_project(dir.path());
        receipt(dir.path(), "claude", None, &revision(), "denied-live-probe");
        assert!(matches!(
            gate(dir.path(), ProviderKind::Codex, None),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn a_receipt_for_another_home_does_not_carry_over() {
        let dir = tempfile::tempdir().unwrap();
        locked_project(dir.path());
        receipt(
            dir.path(),
            "claude",
            Some("C:/Users/dario/.claude-a"),
            &revision(),
            "denied-live-probe",
        );
        let other = Path::new("C:/Users/dario/.claude-b");
        assert!(matches!(
            gate(dir.path(), ProviderKind::Claude, Some(other)),
            Gate::ReadOnly(_)
        ));
        let same = Path::new("C:\\Users\\dario\\.claude-a");
        assert_eq!(
            gate(dir.path(), ProviderKind::Claude, Some(same)),
            Gate::Open,
            "slash style must not matter"
        );
    }

    #[test]
    fn a_stale_policy_revision_closes_the_gate_again() {
        let dir = tempfile::tempdir().unwrap();
        locked_project(dir.path());
        receipt(dir.path(), "claude", None, &revision(), "denied-live-probe");
        fs::write(
            dir.path().join("docs/quality/locked-code.json"),
            "{\"controls\":[{\"path\":\"a\"}]}\n",
        )
        .unwrap();
        assert!(matches!(
            gate(dir.path(), ProviderKind::Claude, None),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn a_receipt_that_did_not_prove_a_denial_is_not_verification() {
        let dir = tempfile::tempdir().unwrap();
        locked_project(dir.path());
        receipt(dir.path(), "claude", None, &revision(), "hooks-installed");
        assert!(matches!(
            gate(dir.path(), ProviderKind::Claude, None),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn a_corrupt_receipt_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        locked_project(dir.path());
        fs::create_dir_all(dir.path().join(".opifex/enforcement")).unwrap();
        fs::write(dir.path().join(".opifex/enforcement/claude.json"), "{nope").unwrap();
        assert!(matches!(
            gate(dir.path(), ProviderKind::Claude, None),
            Gate::ReadOnly(_)
        ));
    }

    #[test]
    fn line_endings_do_not_change_the_policy_revision() {
        assert_eq!(policy_revision(b"a\r\nb\r\n"), policy_revision(b"a\nb\n"));
    }

    #[test]
    fn write_capable_trust_drops_to_plan_with_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        locked_project(dir.path());
        for trust in [TrustMode::Ask, TrustMode::Workspace, TrustMode::Full] {
            let (effective, notice) =
                effective_trust(dir.path(), ProviderKind::Claude, None, trust);
            assert_eq!(effective, TrustMode::Plan, "{trust:?}");
            assert!(notice.unwrap().contains("read-only"));
        }
    }

    #[test]
    fn plan_trust_needs_no_notice_and_a_verified_project_keeps_its_trust() {
        let dir = tempfile::tempdir().unwrap();
        locked_project(dir.path());
        let (t, n) = effective_trust(dir.path(), ProviderKind::Claude, None, TrustMode::Plan);
        assert_eq!((t, n), (TrustMode::Plan, None));
        receipt(dir.path(), "claude", None, &revision(), "denied-live-probe");
        let (t, n) = effective_trust(dir.path(), ProviderKind::Claude, None, TrustMode::Full);
        assert_eq!((t, n), (TrustMode::Full, None));
    }
}
