//! What the user has connected, kept in Concordia's state directory (never in a project).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// One provider, isolated home and project a harness was connected for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub provider: String,
    pub home: Option<String>,
    pub project: String,
    pub connect_ok: bool,
    pub connect_note: String,
    /// `verified`, `unverified` or `not-run`.
    pub verify: String,
    pub verify_note: String,
    /// Seconds since the epoch of the last connect or verify.
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub name: String,
    pub version: Option<String>,
    pub manifest_path: String,
    pub manifest_sha256: String,
    pub skill: Option<String>,
    pub entries: Vec<Entry>,
    #[serde(default)]
    pub has_pinned_decisions: bool,
}

/// Who the entry belongs to; refreshes the stored record when the manifest changed.
#[derive(Debug, Clone, Copy)]
pub struct Identity<'a> {
    pub name: &'a str,
    pub version: Option<&'a str>,
    pub manifest_path: &'a str,
    pub manifest_sha256: &'a str,
    pub skill: Option<&'a str>,
}

/// The connected provider target whose receipt is being checked.
pub struct ReceiptTarget<'a> {
    pub provider: &'a str,
    pub project: &'a Path,
    pub home: Option<&'a Path>,
}

/// The identity returned by a command-backed manifest refresh.
pub struct RefreshedIdentity {
    pub name: String,
    pub version: Option<String>,
    pub manifest_sha256: String,
}

/// Native callback used to refresh a command-backed adapter manifest.
pub type RefreshCommand<'a> = dyn Fn(&str) -> Option<RefreshedIdentity> + 'a;

/// Identity recorded in a protected receipt.
pub struct ReceiptIdentity<'a> {
    pub name: &'a str,
    pub version: Option<&'a str>,
    pub manifest_path: &'a str,
    pub manifest_sha256: &'a str,
}

/// Immutable identity captured while a catalog row is reviewed.  Mutating commands must
/// validate this target immediately before invoking an adapter; a later catalog refresh may
/// never silently retarget an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionTarget {
    pub name: String,
    pub manifest_path: String,
    pub manifest_sha256: String,
    pub provider: String,
    pub home: Option<String>,
    pub project: String,
}

impl ActionTarget {
    pub fn from_connection(connection: &Connection, entry: &Entry) -> Self {
        Self {
            name: connection.name.clone(),
            manifest_path: connection.manifest_path.clone(),
            manifest_sha256: connection.manifest_sha256.clone(),
            provider: entry.provider.clone(),
            home: entry.home.clone(),
            project: entry.project.clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    connections: Vec<Connection>,
}

fn file(store: &Path) -> PathBuf {
    store.join("harness").join("connections.json")
}

fn owned_directory(store: &Path, create: bool) -> Result<(), String> {
    let dir = store.join("harness");
    match fs::symlink_metadata(&dir) {
        Ok(meta) if !meta.is_dir() || crate::workspaces::linked(&meta) => {
            Err("harness state directory must not be linked".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if create {
                fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
            }
            Ok(())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn transaction<T>(store: &Path, mutate: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    owned_directory(store, true)?;
    let lock = store.join("harness/connections.lock");
    match fs::symlink_metadata(&lock) {
        Ok(meta) if !meta.is_file() || crate::workspaces::linked(&meta) => {
            return Err("harness state lock must not be linked".into())
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.to_string()),
        _ => {}
    }
    crate::state_io::with_lock(&lock, || mutate().map_err(std::io::Error::other))
        .map_err(|error| error.to_string())
}

/// Whether a receipt's recorded adapter identity is still the connected identity.
///
/// File manifests are re-hashed at the point of use; a stored version alone is not
/// sufficient because an adapter can change without updating its version string.
pub fn identity_matches(
    store: &Path,
    target: &ReceiptTarget<'_>,
    receipt: &ReceiptIdentity<'_>,
    refresh_command: &RefreshCommand<'_>,
) -> bool {
    let Ok(all) = load(store) else {
        return false;
    };
    let Some(connection) = all.iter().find(|c| c.name == receipt.name) else {
        return false;
    };
    let project_key = path_key(target.project);
    let home_key = target.home.map(path_key);
    if connection.version.as_deref() != receipt.version
        || connection.manifest_path != receipt.manifest_path
        || connection.manifest_sha256 != receipt.manifest_sha256
        || !connection.entries.iter().any(|entry| {
            entry.connect_ok
                && entry.provider == target.provider
                && path_key(Path::new(&entry.project)) == project_key
                && entry.home.as_deref().map(Path::new).map(path_key) == home_key
        })
    {
        return false;
    }
    if receipt.manifest_path.starts_with("command:") {
        let Some(program) = receipt.manifest_path.strip_prefix("command:") else {
            return false;
        };
        return refresh_command(program).is_some_and(|current| {
            current.name == receipt.name
                && current.version.as_deref() == receipt.version
                && current.manifest_sha256 == receipt.manifest_sha256
        });
    }
    let Ok(text) = fs::read_to_string(receipt.manifest_path) else {
        return false;
    };
    let digest = Sha256::digest(text.as_bytes());
    let current = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    current == receipt.manifest_sha256
}

fn path_key(path: &Path) -> String {
    let key = path.to_string_lossy().replace('\\', "/");
    let key = key.trim_end_matches('/').to_string();
    if cfg!(windows) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

/// Every connection. Missing file: none. A damaged file is an error, so it is never overwritten.
pub fn load(store: &Path) -> Result<Vec<Connection>, String> {
    use std::io::Read;
    owned_directory(store, false)?;
    let path = file(store);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.is_file() || crate::workspaces::linked(&metadata) => {
            return Err("harness connections must be a regular owned file".into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("harness connections metadata unavailable: {error}")),
        Ok(_) => {}
    }
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("harness connections are unreadable ({e})")),
    };
    if !file
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && !crate::workspaces::linked(&metadata))
    {
        return Err("harness connections must be a regular file".into());
    }
    let mut text = String::new();
    file.take(2 * 1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|_| "harness connections are unreadable")?;
    if text.len() > 2 * 1024 * 1024 {
        return Err("harness connections exceed the 2 MiB read bound; original retained".into());
    }
    serde_json::from_str::<File>(&text)
        .and_then(|f| {
            if f.version != 1
                || f.connections.len() > 512
                || f.connections
                    .iter()
                    .any(|connection| connection.entries.len() > 256)
            {
                return Err(serde::de::Error::custom(
                    "unsupported connection version or metadata bounds",
                ));
            }
            Ok(f.connections)
        })
        .map_err(|e| {
            format!(
                "harness connections are unreadable ({e}); fix or delete {}",
                path.display()
            )
        })
}

/// Re-read the saved connection and refuse a mutation when any reviewed identity changed.
pub fn check_action_target(store: &Path, target: &ActionTarget) -> Result<(), String> {
    let all = load(store)?;
    let connection = all
        .iter()
        .find(|connection| connection.name == target.name)
        .ok_or_else(|| {
            "integration changed: connection no longer exists; refresh and reselect".to_string()
        })?;
    if connection.manifest_path != target.manifest_path
        || connection.manifest_sha256 != target.manifest_sha256
    {
        return Err("integration revision changed; refresh and reselect before acting".into());
    }
    if !connection.entries.iter().any(|entry| {
        entry.provider == target.provider
            && entry.home == target.home
            && entry.project == target.project
    }) {
        return Err("integration binding changed; refresh and reselect before acting".into());
    }
    Ok(())
}

/// Check a reviewed target and remove only that integration. This is intentionally separate
/// from native uninstall: it withdraws Concordia receipts only.
pub fn remove_action_target(store: &Path, target: &ActionTarget) -> Result<bool, String> {
    transaction(store, || remove_action_target_locked(store, target))
}

fn remove_action_target_locked(store: &Path, target: &ActionTarget) -> Result<bool, String> {
    check_action_target(store, target)?;
    let mut all = load(store)?;
    let Some(connection) = all
        .iter_mut()
        .find(|connection| connection.name == target.name)
    else {
        return Ok(false);
    };
    let before = connection.entries.len();
    connection.entries.retain(|entry| {
        !(entry.provider == target.provider
            && entry.home == target.home
            && entry.project == target.project)
    });
    let changed = connection.entries.len() != before;
    if connection.entries.is_empty() {
        all.retain(|item| item.name != target.name);
    }
    if changed {
        save_unlocked(store, all)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn save_unlocked(store: &Path, connections: Vec<Connection>) -> Result<(), String> {
    if connections.len() > 512
        || connections
            .iter()
            .any(|connection| connection.entries.len() > 256)
    {
        return Err("harness connection metadata bounds reached; original retained".into());
    }
    let path = file(store);
    let body = serde_json::to_vec_pretty(&File {
        version: 1,
        connections,
    })
    .map_err(|e| e.to_string())?;
    if body.len() > 2 * 1024 * 1024 {
        return Err("harness connections exceed the 2 MiB save bound; original retained".into());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("could not save harness connections: {e}"))?;
    }
    crate::state_io::write_atomic(&path, &body)
        .map_err(|e| format!("could not save harness connections: {e}"))
}

fn same_target(a: &Entry, b: &Entry) -> bool {
    a.provider == b.provider && a.home == b.home && a.project == b.project
}

/// Store `entry` under `identity`, replacing an earlier entry for the same provider, home and project.
pub fn record(store: &Path, identity: &Identity<'_>, entry: Entry) -> Result<(), String> {
    let pins = !identity.manifest_path.starts_with("command:")
        && super::pinned::manifest_at(Path::new(identity.manifest_path), identity.manifest_sha256)
            .is_ok_and(|m| !m.pinned_decisions.is_empty());
    record_with_pins(store, identity, entry, pins)
}

pub fn record_with_pins(
    store: &Path,
    identity: &Identity<'_>,
    entry: Entry,
    has_pins: bool,
) -> Result<(), String> {
    transaction(store, || {
        record_with_pins_unlocked(store, identity, entry, has_pins)
    })
}

fn record_with_pins_unlocked(
    store: &Path,
    identity: &Identity<'_>,
    entry: Entry,
    has_pins: bool,
) -> Result<(), String> {
    if has_pins {
        if identity.manifest_path.starts_with("command:") {
            return Err(
                "pinned decisions require an explicitly connected file-backed manifest".into(),
            );
        }
        let manifest = super::pinned::manifest_at(
            Path::new(identity.manifest_path),
            identity.manifest_sha256,
        )?;
        if manifest.pinned_decisions.is_empty() {
            return Err("pinned decisions changed during connection".into());
        }
    }
    let mut all = load(store)?;
    let at = match all.iter().position(|c| c.name == identity.name) {
        Some(at) => at,
        None => {
            all.push(Connection {
                name: identity.name.into(),
                version: None,
                manifest_path: String::new(),
                manifest_sha256: String::new(),
                skill: None,
                entries: Vec::new(),
                has_pinned_decisions: false,
            });
            all.len() - 1
        }
    };
    let connection = &mut all[at];
    connection.version = identity.version.map(str::to_string);
    connection.manifest_path = identity.manifest_path.into();
    connection.manifest_sha256 = identity.manifest_sha256.into();
    connection.skill = identity.skill.map(str::to_string);
    connection.has_pinned_decisions = has_pins;
    connection.entries.retain(|e| !same_target(e, &entry));
    connection.entries.push(entry);
    save_unlocked(store, all)
}

/// Update the verify result of one existing entry.
#[allow(clippy::too_many_arguments)]
pub fn set_verify(
    store: &Path,
    name: &str,
    provider: &str,
    home: Option<&str>,
    project: &str,
    state: &str,
    note: &str,
    at: u64,
) -> Result<(), String> {
    transaction(store, || {
        set_verify_unlocked(store, name, provider, home, project, state, note, at)
    })
}

#[allow(clippy::too_many_arguments)]
fn set_verify_unlocked(
    store: &Path,
    name: &str,
    provider: &str,
    home: Option<&str>,
    project: &str,
    state: &str,
    note: &str,
    at: u64,
) -> Result<(), String> {
    let mut all = load(store)?;
    if let Some(e) = all
        .iter_mut()
        .filter(|c| c.name == name)
        .flat_map(|c| c.entries.iter_mut())
        .find(|e| e.provider == provider && e.home.as_deref() == home && e.project == project)
    {
        e.verify = state.into();
        e.verify_note = note.into();
        e.at = at;
    }
    save_unlocked(store, all)
}

/// Forget a connection. Returns whether it existed.
pub fn remove(store: &Path, name: &str) -> Result<bool, String> {
    transaction(store, || remove_unlocked(store, name))
}

fn remove_unlocked(store: &Path, name: &str) -> Result<bool, String> {
    let mut all = load(store)?;
    let before = all.len();
    all.retain(|c| c.name != name);
    let removed = all.len() != before;
    if removed {
        save_unlocked(store, all)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(provider: &str, home: Option<&str>, connected: bool) -> Entry {
        Entry {
            provider: provider.into(),
            home: home.map(str::to_string),
            project: "/work/app".into(),
            connect_ok: connected,
            connect_note: if connected {
                "ok".into()
            } else {
                "exit 3".into()
            },
            verify: "not-run".into(),
            verify_note: String::new(),
            at: 100,
        }
    }

    fn identity() -> Identity<'static> {
        Identity {
            name: "h1",
            version: Some("1.0"),
            manifest_path: "/opt/h/harness.json",
            manifest_sha256: "abc",
            skill: None,
        }
    }

    #[test]
    fn nothing_stored_means_an_empty_list() {
        let store = tempfile::tempdir().unwrap();
        assert!(load(store.path()).unwrap().is_empty());
        let manifest = store.path().join("harness.json");
        let text = serde_json::json!({"name":"catalog-fixture","providers":{"codex":{"connect":["must-never-run"]}},"tools":(0..12).map(|number| serde_json::json!({"name":format!("tool-{number}"),"description":"Untrusted manifest description","argv":["must-never-run"]})).collect::<Vec<_>>()}).to_string();
        fs::write(&manifest, &text).unwrap();
        let sha = format!("{:x}", Sha256::digest(text.as_bytes()));
        let manifest_path = manifest.display().to_string();
        record(
            store.path(),
            &Identity {
                name: "catalog-fixture",
                version: None,
                manifest_path: &manifest_path,
                manifest_sha256: &sha,
                skill: None,
            },
            entry("codex", None, true),
        )
        .unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let catalog = super::super::catalog::inspect(store.path(), &cancel).unwrap();
        assert_eq!(catalog.integrations.len(), 1);
        assert_eq!(catalog.integrations[0].tools.len(), 12);
        assert!(catalog.integrations[0].tools[0].name.ends_with("::tool-0"));
        assert!(catalog.integrations[0].status.contains("revision verified"));
        let id = &catalog.integrations[0].id;
        let page: serde_json::Value = serde_json::from_str(
            &super::super::catalog::tool(store.path(), Some(id), 0, None).unwrap(),
        )
        .unwrap();
        assert_eq!(page["tools"].as_array().unwrap().len(), 8);
        assert_eq!(page["next_offset"], 8);
        assert!(super::super::catalog::tool(store.path(), Some(id), 8, None).is_err());
        assert!(
            super::super::catalog::tool(store.path(), Some(id), 8, Some("wrong-revision")).is_err()
        );
        let last: serde_json::Value = serde_json::from_str(
            &super::super::catalog::tool(store.path(), Some(id), 8, Some(&sha)).unwrap(),
        )
        .unwrap();
        assert_eq!(last["tools"].as_array().unwrap().len(), 4);
        fs::write(&manifest, "changed after connection").unwrap();
        let stale = super::super::catalog::inspect(store.path(), &cancel).unwrap();
        assert!(stale.integrations[0].tools.is_empty());
        assert!(stale.integrations[0].status.contains("revision changed"));
        record(
            store.path(),
            &Identity {
                name: "command-fixture",
                version: None,
                manifest_path: "command:must-never-run",
                manifest_sha256: "unverified",
                skill: None,
            },
            entry("codex", None, true),
        )
        .unwrap();
        let command = super::super::catalog::inspect(store.path(), &cancel).unwrap();
        assert!(command
            .integrations
            .iter()
            .any(|integration| integration.name == "command-fixture"
                && integration.tools.is_empty()
                && integration.status.contains("never executes")));
        assert!(!super::super::catalog::rows(&command).is_empty());
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(super::super::catalog::inspect(store.path(), &cancel).is_err());
        fs::write(file(store.path()), vec![b' '; 2 * 1024 * 1024 + 1]).unwrap();
        assert!(load(store.path()).unwrap_err().contains("2 MiB"));
    }

    #[test]
    fn an_entry_round_trips_and_replaces_the_same_provider_home_and_project() {
        let store = tempfile::tempdir().unwrap();
        record(
            store.path(),
            &identity(),
            entry("claude", Some("/h/1"), false),
        )
        .unwrap();
        record(
            store.path(),
            &identity(),
            entry("claude", Some("/h/1"), true),
        )
        .unwrap();
        record(store.path(), &identity(), entry("codex", None, true)).unwrap();
        let all = load(store.path()).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "h1");
        assert_eq!(
            all[0].entries.len(),
            2,
            "same provider, home and project is replaced"
        );
        let claude = all[0]
            .entries
            .iter()
            .find(|e| e.provider == "claude")
            .unwrap();
        assert!(claude.connect_ok);
    }

    #[test]
    fn a_new_manifest_hash_updates_the_record_but_keeps_other_entries() {
        let store = tempfile::tempdir().unwrap();
        record(
            store.path(),
            &identity(),
            entry("claude", Some("/h/1"), true),
        )
        .unwrap();
        let newer = Identity {
            manifest_sha256: "def",
            version: Some("1.1"),
            ..identity()
        };
        record(store.path(), &newer, entry("codex", None, true)).unwrap();
        let all = load(store.path()).unwrap();
        assert_eq!(all[0].manifest_sha256, "def");
        assert_eq!(all[0].version.as_deref(), Some("1.1"));
        assert_eq!(all[0].entries.len(), 2);
    }

    #[test]
    fn remove_reports_whether_anything_was_there() {
        let store = tempfile::tempdir().unwrap();
        record(store.path(), &identity(), entry("claude", None, true)).unwrap();
        record(
            store.path(),
            &identity(),
            entry("codex", Some("/h/2"), true),
        )
        .unwrap();
        let target = ActionTarget {
            name: "h1".into(),
            manifest_path: identity().manifest_path.into(),
            manifest_sha256: "abc".into(),
            provider: "claude".into(),
            home: None,
            project: "/work/app".into(),
        };
        let stale = ActionTarget {
            manifest_sha256: "changed".into(),
            ..target.clone()
        };
        assert!(remove_action_target(store.path(), &stale).is_err());
        assert!(remove_action_target(store.path(), &target).unwrap());
        assert_eq!(load(store.path()).unwrap()[0].entries.len(), 1);
        assert!(remove(store.path(), "h1").unwrap());
        assert!(!remove(store.path(), "h1").unwrap());
        assert!(load(store.path()).unwrap().is_empty());
    }

    #[test]
    fn a_corrupt_file_is_an_error_and_is_never_overwritten() {
        let store = tempfile::tempdir().unwrap();
        let path = store.path().join("harness/connections.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{broken").unwrap();
        assert!(load(store.path()).unwrap_err().contains("unreadable"));
        assert!(record(store.path(), &identity(), entry("claude", None, true)).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
    }

    #[test]
    fn a_command_identity_is_refreshed_instead_of_trusting_a_stale_connection() {
        let store = tempfile::tempdir().unwrap();
        record(
            store.path(),
            &Identity {
                name: "h1",
                version: Some("1.0"),
                manifest_path: "command:adapter",
                manifest_sha256: "abc",
                skill: None,
            },
            entry("claude", Some("/h/1"), true),
        )
        .unwrap();
        let current = |program: &str| {
            assert_eq!(program, "adapter");
            Some(RefreshedIdentity {
                name: "h1".into(),
                version: Some("1.0".into()),
                manifest_sha256: "new-hash".into(),
            })
        };
        assert!(!identity_matches(
            store.path(),
            &ReceiptTarget {
                provider: "claude",
                project: Path::new("/work/app"),
                home: Some(Path::new("/h/1")),
            },
            &ReceiptIdentity {
                name: "h1",
                version: Some("1.0"),
                manifest_path: "command:adapter",
                manifest_sha256: "abc",
            },
            &current,
        ));
    }

    #[test]
    fn set_verify_updates_only_the_matching_entry() {
        let store = tempfile::tempdir().unwrap();
        record(
            store.path(),
            &identity(),
            entry("claude", Some("/h/1"), true),
        )
        .unwrap();
        record(store.path(), &identity(), entry("codex", None, true)).unwrap();
        set_verify(
            store.path(),
            "h1",
            "claude",
            Some("/h/1"),
            "/work/app",
            "verified",
            "proved",
            200,
        )
        .unwrap();
        let all = load(store.path()).unwrap();
        let by = |p: &str| {
            all[0]
                .entries
                .iter()
                .find(|e| e.provider == p)
                .unwrap()
                .clone()
        };
        assert_eq!(by("claude").verify, "verified");
        assert_eq!(by("claude").at, 200);
        assert_eq!(by("codex").verify, "not-run");
    }
}
