use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Codex,
    Claude,
    Cursor,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Cursor => "cursor",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustMode {
    /// Read-only planning: the provider analyses and proposes, it does not edit.
    Plan,
    Ask,
    Workspace,
    Full,
}

impl TrustMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Ask => "ask",
            Self::Workspace => "workspace",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatLaunchRecord {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub cwd: String,
    pub provider: ProviderKind,
    pub account: String,
    pub home_path: Option<String>,
    pub pane_id: Option<String>,
    pub native_session_id: Option<String>,
    pub mode: String,
    pub host: String,
    pub last_agent: ProviderKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UseSelection {
    pub provider: ProviderKind,
    pub account: String,
    pub home_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAccount {
    pub id: String,
    pub provider: ProviderKind,
    pub home_path: Option<String>,
    pub label: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestratorSettings {
    #[serde(default = "default_trust")]
    pub trust: TrustMode,
    #[serde(default = "default_child_trust")]
    pub child_trust: ChildTrust,
    /// Accounts that may run as delegated workers (max 3, never the orchestrator).
    #[serde(default)]
    pub workers: Vec<String>,
    /// Opt-in: let an orchestrator reply propose a delegation (still needs a yes from the user).
    /// `None` = never set by the user: `[delegation] protocol` in config.toml decides (see
    /// [`delegation_protocol_on`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation_protocol: Option<bool>,
    /// Opt-in: record conversations to `<state>/threads` (stores prompts). Read via `threads::enabled`.
    #[serde(default)]
    pub thread_log: bool,
}

fn default_trust() -> TrustMode {
    TrustMode::Workspace
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChildTrust {
    Ask,
    Workspace,
    Full,
    Inherit,
}

fn default_child_trust() -> ChildTrust {
    ChildTrust::Inherit
}

impl Default for OrchestratorSettings {
    fn default() -> Self {
        Self {
            trust: TrustMode::Workspace,
            child_trust: ChildTrust::Inherit,
            workers: Vec::new(),
            delegation_protocol: None,
            thread_log: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestratorStore {
    pub version: u32,
    #[serde(default)]
    pub selection: Option<UseSelection>,
    #[serde(default)]
    pub chats: Vec<ChatLaunchRecord>,
    #[serde(default)]
    pub accounts: Vec<StoredAccount>,
    /// Account ids the user removed from view. Used for homes Concordia does not own
    /// (`~/.codex` and friends): they are hidden, never deleted.
    #[serde(default)]
    pub hidden_accounts: Vec<String>,
    #[serde(default)]
    pub settings: OrchestratorSettings,
}

impl Default for OrchestratorStore {
    fn default() -> Self {
        Self {
            version: 1,
            selection: None,
            chats: Vec::new(),
            accounts: Vec::new(),
            hidden_accounts: Vec::new(),
            settings: OrchestratorSettings::default(),
        }
    }
}

pub fn default_store_root() -> PathBuf {
    if let Ok(dir) = std::env::var("CONCORDIA_STATE_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("concordia")
}

/// State directory of the product before it was renamed (`%LOCALAPPDATA%\orc`).
pub fn legacy_state_root() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("orc"))
}

/// Carry the pre-rename `store.json` over to `root`, once. Registered accounts keep working
/// because their home paths are absolute and the homes stay where they were. The old file is
/// copied, never moved or deleted. Returns whether anything was copied.
pub fn adopt_state_from(legacy_root: &Path, root: &Path) -> Result<bool> {
    let new_store = store_path(root);
    let old_store = store_path(legacy_root);
    if new_store.exists() || !old_store.is_file() {
        return Ok(false);
    }
    let dir = store_dir(root);
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    fs::copy(&old_store, &new_store).with_context(|| format!("copy {}", old_store.display()))?;
    Ok(true)
}

/// First run after the rename: adopt the old state unless an explicit state dir is in use.
pub fn adopt_legacy_state(root: &Path) -> Result<bool> {
    if std::env::var("CONCORDIA_STATE_DIR").is_ok_and(|d| !d.trim().is_empty()) {
        return Ok(false);
    }
    match legacy_state_root() {
        Some(legacy) if legacy != root => adopt_state_from(&legacy, root),
        _ => Ok(false),
    }
}

pub fn store_dir(root: &Path) -> PathBuf {
    root.join(".ao")
}

pub fn store_path(root: &Path) -> PathBuf {
    store_dir(root).join("store.json")
}

fn sanitize_settings(settings: OrchestratorSettings) -> OrchestratorSettings {
    match settings.trust {
        TrustMode::Plan | TrustMode::Ask | TrustMode::Workspace | TrustMode::Full => {}
    }
    match settings.child_trust {
        ChildTrust::Ask | ChildTrust::Workspace | ChildTrust::Full | ChildTrust::Inherit => {}
    }
    settings
}

/// A store with nothing saved yet: the permission mode comes from `[mode] default` in config.toml.
fn fresh_store(root: &Path) -> OrchestratorStore {
    let mut store = OrchestratorStore::default();
    store.settings.trust = crate::config::for_root(root).mode.default;
    store
}

pub fn load_store(root: &Path) -> OrchestratorStore {
    let path = store_path(root);
    let Ok(raw) = fs::read_to_string(&path) else {
        return fresh_store(root);
    };
    let parsed: Result<OrchestratorStore, _> = serde_json::from_str(&raw);
    match parsed {
        Ok(mut store) if store.version == 1 => {
            store.settings = sanitize_settings(store.settings);
            // A store written before a mode was ever saved: config default, not the legacy one.
            let has_mode = serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .is_some_and(|v| v.pointer("/settings/trust").is_some());
            if !has_mode {
                store.settings.trust = crate::config::for_root(root).mode.default;
            }
            store
        }
        _ => fresh_store(root),
    }
}

/// Whether the orchestrator may propose delegations: the user's saved choice, else the config
/// default. A value saved in the store always wins.
pub fn delegation_protocol_on(root: &Path) -> bool {
    get_settings(root)
        .delegation_protocol
        .unwrap_or_else(|| crate::config::for_root(root).delegation.protocol)
}

/// Write the store atomically (temp file + rename) so a crash cannot leave half a file. An
/// existing file that no longer parses is copied aside first rather than silently destroyed.
pub fn save_store(root: &Path, store: &OrchestratorStore) -> Result<()> {
    let dir = store_dir(root);
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = store_path(root);
    if let Ok(raw) = fs::read_to_string(&path) {
        if serde_json::from_str::<OrchestratorStore>(&raw).is_err() {
            let backup = dir.join(format!(
                "store.json.corrupt-{}",
                chrono_now().replace('.', "-")
            ));
            let _ = fs::copy(&path, backup);
        }
    }
    let json = serde_json::to_string_pretty(store).context("serialize store")?;
    let tmp = dir.join(format!("store.json.tmp-{}", std::process::id()));
    fs::write(&tmp, json).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}

const LOCK_STALE: std::time::Duration = std::time::Duration::from_secs(10);
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Removes the lock file when dropped, including on panic.
struct StoreLock(PathBuf);

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Several concordia panes share one store, so a read-modify-write must not interleave with another
/// pane's or an update would be lost. A lock file taken with `create_new` serializes them; a
/// lock left behind by a crashed process is reclaimed after `LOCK_STALE`.
fn lock_store(root: &Path) -> Result<StoreLock> {
    let dir = store_dir(root);
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join("store.lock");
    let start = std::time::Instant::now();
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => return Ok(StoreLock(path)),
            // On Windows a lock file that is mid-delete (another process just released it)
            // reports "access denied" rather than "already exists"; both mean "try again".
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::PermissionDenied
                ) =>
            {
                let stale = fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age > LOCK_STALE);
                if stale {
                    let _ = fs::remove_file(&path);
                    continue;
                }
                if start.elapsed() > LOCK_WAIT {
                    anyhow::bail!(
                        "state is locked by another concordia process ({})",
                        path.display()
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            Err(e) => return Err(e).with_context(|| format!("lock {}", path.display())),
        }
    }
}

/// Load, change and save the store under the lock.
pub fn modify_store<T>(
    root: &Path,
    f: impl FnOnce(&mut OrchestratorStore) -> Result<T>,
) -> Result<T> {
    // Do not let threads in this process race each other through the cross-process
    // lock's deadline. A busy writer can otherwise repeatedly reacquire the file while
    // another local writer times out, even though every individual update is brief.
    static LOCAL_WRITER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _local = LOCAL_WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let _lock = lock_store(root)?;
    let mut store = load_store(root);
    let out = f(&mut store)?;
    save_store(root, &store)?;
    Ok(out)
}

pub fn get_settings(root: &Path) -> OrchestratorSettings {
    load_store(root).settings
}

pub fn set_trust_mode(root: &Path, trust: TrustMode) -> Result<OrchestratorSettings> {
    modify_store(root, |store| {
        store.settings.trust = trust;
        store.settings = sanitize_settings(store.settings.clone());
        Ok(store.settings.clone())
    })
}

/// Replace the worker list (validation lives in `delegate`).
pub fn set_workers(root: &Path, workers: Vec<String>) -> Result<()> {
    modify_store(root, |store| {
        store.settings.workers = workers;
        Ok(())
    })
}

pub fn set_delegation_protocol(root: &Path, on: bool) -> Result<()> {
    modify_store(root, |store| {
        store.settings.delegation_protocol = Some(on);
        Ok(())
    })
}

pub fn get_use_selection(root: &Path) -> Option<UseSelection> {
    load_store(root).selection
}

pub fn set_use_selection(root: &Path, selection: UseSelection) -> Result<()> {
    modify_store(root, |store| {
        store.selection = Some(selection);
        Ok(())
    })
}

pub fn hidden_accounts(root: &Path) -> Vec<String> {
    load_store(root).hidden_accounts
}

pub fn hide_account(root: &Path, id: &str) -> Result<()> {
    modify_store(root, |store| {
        if !store.hidden_accounts.iter().any(|h| h == id) {
            store.hidden_accounts.push(id.to_string());
        }
        Ok(())
    })
}

/// Drop an orc-registered account (and any hidden flag for it). Returns whether it was registered.
pub fn remove_stored_account(root: &Path, id: &str) -> Result<bool> {
    modify_store(root, |store| {
        let before = store.accounts.len();
        store.accounts.retain(|a| a.id != id);
        store.hidden_accounts.retain(|h| h != id);
        Ok(store.accounts.len() != before)
    })
}

pub fn list_stored_accounts(root: &Path) -> Vec<StoredAccount> {
    load_store(root).accounts
}

/// Insert or replace an concordia-owned account (keyed by id).
pub fn upsert_stored_account(root: &Path, account: StoredAccount) -> Result<()> {
    modify_store(root, |store| {
        store.accounts.retain(|a| a.id != account.id);
        store.accounts.push(account);
        Ok(())
    })
}

pub fn ensure_default_selection(root: &Path) -> Result<()> {
    let _ = adopt_legacy_state(root);
    if get_use_selection(root).is_some() {
        return Ok(());
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let codex_home = std::env::var("CODEX_HOME")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    set_use_selection(
        root,
        UseSelection {
            provider: ProviderKind::Codex,
            account: "codex".into(),
            home_path: Some(codex_home.to_string_lossy().into_owned()),
            model: None,
            updated_at: chrono_now(),
        },
    )
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}Z", dur.as_secs(), dur.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows keeps a just-deleted file visible for a moment while Defender or the indexer
    /// holds a handle. Product code retries through that state; tests must not race it.
    fn eventually(mut cond: impl FnMut() -> bool) -> bool {
        for _ in 0..80 {
            if cond() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        cond()
    }

    fn acct(id: &str) -> StoredAccount {
        StoredAccount {
            id: id.into(),
            provider: ProviderKind::Codex,
            home_path: Some(format!("C:\\homes\\{id}")),
            label: id.into(),
            created_at: "0".into(),
        }
    }

    #[test]
    fn workers_and_protocol_persist_and_default_off() {
        let dir = tempfile::tempdir().unwrap();
        let s = get_settings(dir.path());
        assert!(s.workers.is_empty() && s.delegation_protocol.is_none());
        assert!(!delegation_protocol_on(dir.path()));
        set_workers(dir.path(), vec!["codex-2".into()]).unwrap();
        set_delegation_protocol(dir.path(), true).unwrap();
        set_trust_mode(dir.path(), TrustMode::Ask).unwrap();
        let s = get_settings(dir.path());
        assert_eq!(s.workers, ["codex-2"]);
        assert_eq!(s.delegation_protocol, Some(true));
        assert_eq!(s.trust, TrustMode::Ask);
        // An older store without the new keys still loads.
        fs::write(
            store_path(dir.path()),
            r#"{"version":1,"settings":{"trust":"ask","childTrust":"inherit"}}"#,
        )
        .unwrap();
        assert!(get_settings(dir.path()).workers.is_empty());
    }

    #[test]
    fn the_pre_rename_store_is_adopted_once_and_never_overwritten() {
        let legacy = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        // Nothing to adopt yet.
        assert!(!adopt_state_from(legacy.path(), root.path()).unwrap());

        upsert_stored_account(legacy.path(), acct("old-account")).unwrap();
        assert!(adopt_state_from(legacy.path(), root.path()).unwrap());
        let adopted = load_store(root.path());
        assert_eq!(adopted.accounts.len(), 1);
        assert_eq!(adopted.accounts[0].id, "old-account");
        // The old file is left in place.
        assert!(store_path(legacy.path()).is_file());

        // Once a store exists in the new root, the old one is never copied over it.
        upsert_stored_account(root.path(), acct("new-account")).unwrap();
        upsert_stored_account(legacy.path(), acct("changed-later")).unwrap();
        assert!(!adopt_state_from(legacy.path(), root.path()).unwrap());
        let ids: Vec<_> = load_store(root.path())
            .accounts
            .into_iter()
            .map(|a| a.id)
            .collect();
        assert!(
            ids.contains(&"new-account".to_string()) && !ids.contains(&"changed-later".to_string())
        );
    }

    #[test]
    fn concurrent_writers_never_lose_each_others_updates() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let threads: Vec<_> = (0..6)
            .map(|t| {
                let root = root.clone();
                std::thread::spawn(move || {
                    for i in 0..15 {
                        upsert_stored_account(&root, acct(&format!("t{t}-a{i}"))).unwrap();
                        set_trust_mode(&root, TrustMode::Ask).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let store = load_store(&root);
        assert_eq!(store.accounts.len(), 6 * 15, "lost updates");
        assert_eq!(store.settings.trust, TrustMode::Ask);
    }

    #[test]
    fn a_save_leaves_no_temp_or_lock_files_behind() {
        let dir = tempfile::tempdir().unwrap();
        upsert_stored_account(dir.path(), acct("a")).unwrap();
        let names = || -> Vec<String> {
            fs::read_dir(store_dir(dir.path()))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        };
        assert!(eventually(|| names() == ["store.json"]), "{:?}", names());
    }

    #[test]
    fn an_unreadable_store_is_backed_up_not_silently_destroyed() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(store_dir(dir.path())).unwrap();
        fs::write(store_path(dir.path()), "{ this is not json").unwrap();
        upsert_stored_account(dir.path(), acct("fresh")).unwrap();
        let backups: Vec<_> = fs::read_dir(store_dir(dir.path()))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("store.json.corrupt-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read_to_string(backups[0].path()).unwrap(),
            "{ this is not json"
        );
        assert_eq!(load_store(dir.path()).accounts.len(), 1);
    }

    #[test]
    fn a_stale_lock_from_a_crashed_process_is_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(store_dir(dir.path())).unwrap();
        let lock = store_dir(dir.path()).join("store.lock");
        fs::write(&lock, "").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        upsert_stored_account(dir.path(), acct("a")).unwrap();
        assert_eq!(load_store(dir.path()).accounts.len(), 1);
        assert!(eventually(|| !lock.exists()), "stale lock was not released");
    }

    #[test]
    fn a_failing_update_does_not_write_and_releases_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        upsert_stored_account(dir.path(), acct("keep")).unwrap();
        let r: Result<()> = modify_store(dir.path(), |s| {
            s.accounts.clear();
            anyhow::bail!("nope")
        });
        assert!(r.is_err());
        assert_eq!(load_store(dir.path()).accounts.len(), 1);
        let lock = store_dir(dir.path()).join("store.lock");
        assert!(eventually(|| !lock.exists()), "lock was not released");
        // ...and it really is free: the next update goes straight through.
        upsert_stored_account(dir.path(), acct("next")).unwrap();
    }
}
