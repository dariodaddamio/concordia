//! What the user has connected, kept in Concordia's state directory (never in a project).

use serde::{Deserialize, Serialize};
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

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    connections: Vec<Connection>,
}

fn file(store: &Path) -> PathBuf {
    store.join("harness").join("connections.json")
}

/// Every connection. Missing file: none. A damaged file is an error, so it is never overwritten.
pub fn load(store: &Path) -> Result<Vec<Connection>, String> {
    let path = file(store);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("harness connections are unreadable ({e})")),
    };
    serde_json::from_str::<File>(&text)
        .map(|f| f.connections)
        .map_err(|e| {
            format!(
                "harness connections are unreadable ({e}); fix or delete {}",
                path.display()
            )
        })
}

fn save(store: &Path, connections: Vec<Connection>) -> Result<(), String> {
    let path = file(store);
    let body = serde_json::to_vec_pretty(&File {
        version: 1,
        connections,
    })
    .map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("could not save harness connections: {e}"))?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, body)
        .and_then(|()| fs::rename(&temporary, &path))
        .map_err(|e| format!("could not save harness connections: {e}"))
}

fn same_target(a: &Entry, b: &Entry) -> bool {
    a.provider == b.provider && a.home == b.home && a.project == b.project
}

/// Store `entry` under `identity`, replacing an earlier entry for the same provider, home and project.
pub fn record(store: &Path, identity: &Identity<'_>, entry: Entry) -> Result<(), String> {
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
            });
            all.len() - 1
        }
    };
    let connection = &mut all[at];
    connection.version = identity.version.map(str::to_string);
    connection.manifest_path = identity.manifest_path.into();
    connection.manifest_sha256 = identity.manifest_sha256.into();
    connection.skill = identity.skill.map(str::to_string);
    connection.entries.retain(|e| !same_target(e, &entry));
    connection.entries.push(entry);
    save(store, all)
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
    save(store, all)
}

/// Forget a connection. Returns whether it existed.
pub fn remove(store: &Path, name: &str) -> Result<bool, String> {
    let mut all = load(store)?;
    let before = all.len();
    all.retain(|c| c.name != name);
    let removed = all.len() != before;
    if removed {
        save(store, all)?;
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
