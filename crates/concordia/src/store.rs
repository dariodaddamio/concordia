use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Routing overrides have a separately versioned bounded transaction, preserving config.toml.
pub fn route_policy(root: &Path) -> Result<crate::routing::Policies, String> {
    crate::routing::load_policies(root)
}

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
    /// Security ordering: a larger rank grants strictly more provider capability.
    pub fn rank(self) -> u8 {
        match self {
            Self::Plan => 0,
            Self::Ask => 1,
            Self::Workspace => 2,
            Self::Full => 3,
        }
    }

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

/// A user task is deliberately separate from provider-native sessions.  Native histories remain
/// owned by their provider; this is only the bounded Concordia link between them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskSession {
    pub provider: ProviderKind,
    pub account: String,
    pub native_session_id: String,
    pub workspace: String,
    pub linked_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Active,
    Paused,
    Done,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Done => "done",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskMapPointer {
    pub version: u32,
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    pub title: String,
    pub workspace: String,
    pub created_at: String,
    pub updated_at: String,
    pub sessions: Vec<TaskSession>,
    pub state: TaskState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_map: Option<TaskMapPointer>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_action: Option<crate::file_open::FileAction>,
    #[serde(default = "default_trust")]
    pub trust: TrustMode,
    #[serde(default = "default_child_trust")]
    pub child_trust: ChildTrust,
    /// Accounts that may run as delegated workers (max 3, never the orchestrator).
    #[serde(default)]
    pub workers: Vec<String>,
    /// Per-worker native model ids. Missing entry means the provider's own default.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub worker_models: std::collections::BTreeMap<String, String>,
    /// Explicit opt-in: return worker batches to their owning conversation.
    #[serde(default)]
    pub delegate_auto_return: bool,
    #[serde(default)]
    pub delegate_quota_failover: bool,
    /// Opt-in: let an orchestrator reply propose a delegation (still needs a yes from the user).
    /// `None` = never set by the user: `[delegation] protocol` in config.toml decides (see
    /// [`delegation_protocol_on`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation_protocol: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_automatic: Option<bool>,
    #[serde(default)]
    pub same_provider_automatic: bool,
    #[serde(default = "default_handoff_cap")]
    pub automatic_handoff_cap: usize,
    #[serde(default)]
    pub accepted_terms_providers: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub automatic_handoffs_today: std::collections::BTreeMap<String, u8>,
    /// Opt-in: record conversations to `<state>/threads` (stores prompts). Read via `threads::enabled`.
    #[serde(default)]
    pub thread_log: bool,
}

fn default_trust() -> TrustMode {
    TrustMode::Workspace
}

fn default_handoff_cap() -> usize {
    3
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
            file_action: None,
            trust: TrustMode::Workspace,
            child_trust: ChildTrust::Inherit,
            workers: Vec::new(),
            worker_models: Default::default(),
            delegate_auto_return: false,
            delegate_quota_failover: false,
            delegation_protocol: None,
            continuation_automatic: None,
            same_provider_automatic: false,
            automatic_handoff_cap: 3,
            accepted_terms_providers: Default::default(),
            automatic_handoffs_today: Default::default(),
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
    /// Ordered task records. Missing in older stores and lazily populated as sessions are used.
    #[serde(default)]
    pub tasks: Vec<TaskRecord>,
    #[serde(default)]
    pub accounts: Vec<StoredAccount>,
    /// Account ids the user removed from view. Used for homes Concordia does not own
    /// (`~/.codex` and friends): they are hidden, never deleted.
    #[serde(default)]
    pub hidden_accounts: Vec<String>,
    /// User-facing names only; account ids, native homes and routing remain unchanged.
    #[serde(default)]
    pub account_nicknames: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub settings: OrchestratorSettings,
}

impl Default for OrchestratorStore {
    fn default() -> Self {
        Self {
            version: 1,
            selection: None,
            chats: Vec::new(),
            tasks: Vec::new(),
            accounts: Vec::new(),
            hidden_accounts: Vec::new(),
            account_nicknames: Default::default(),
            settings: OrchestratorSettings::default(),
        }
    }
}

pub fn default_store_root() -> PathBuf {
    if let Ok(dir) = std::env::var("CONCORDIA_STATE_DIR") {
        PathBuf::from(dir)
    } else if crate::test_guard::is_enabled() {
        crate::test_guard::test_state_root()
    } else {
        dirs::data_local_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
            .join("concordia")
    }
}

pub fn outside_profile_warning(root: &Path) -> Option<String> {
    if crate::test_guard::is_enabled() {
        return None;
    }
    let profile = if cfg!(windows) {
        std::env::var_os("USERPROFILE")
    } else {
        std::env::var_os("HOME")
    };
    let profile = profile.map(PathBuf::from)?.canonicalize().ok()?;
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    (!root.starts_with(profile)).then(|| {
        format!(
            "state directory is outside your user profile: {}",
            root.display()
        )
    })
}

#[cfg(test)]
mod state_hygiene_tests {
    #[test]
    fn state_dir_outside_profile_warns() {
        // The test guard intentionally exempts isolated test roots from this user-facing warning.
        let root = tempfile::tempdir().unwrap();
        assert!(super::outside_profile_warning(root.path()).is_none());
    }
}

/// Ensure the product-owned state root has restrictive permissions when it is first created.
pub fn ensure_state_root(root: &Path) -> std::io::Result<()> {
    if root.exists() {
        return Ok(());
    }
    fs::create_dir_all(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    if let Err(error) = crate::state_security::set_owner_only(root) {
        eprintln!("warning: could not restrict state directory permissions: {error}");
    }
    Ok(())
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
    ensure_state_root(root).with_context(|| format!("create {}", root.display()))?;
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
    crate::test_guard::check_not_real_state(root);
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
    crate::test_guard::check_not_real_state(root);
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

/// Create or update the task containing a native session.  Repeated observations of the same
/// session are idempotent, while a new provider/account session is appended in order.
#[allow(clippy::too_many_arguments)]
pub fn record_task_session(
    root: &Path,
    task_id: &str,
    title: &str,
    workspace: &str,
    provider: ProviderKind,
    account: &str,
    native_session_id: &str,
    map: Option<TaskMapPointer>,
) -> Result<()> {
    if task_id.trim().is_empty() || native_session_id.trim().is_empty() {
        anyhow::bail!("task and native session ids must not be empty");
    }
    modify_store(root, |store| {
        let now = chrono_now();
        let task = if let Some(task) = store.tasks.iter_mut().find(|task| task.id == task_id) {
            task
        } else {
            store.tasks.push(TaskRecord {
                id: task_id.to_owned(),
                title: crate::session_titles::title_from_prompt(title),
                workspace: workspace.to_owned(),
                created_at: now.clone(),
                updated_at: now.clone(),
                sessions: Vec::new(),
                state: TaskState::Active,
                last_map: None,
            });
            store.tasks.last_mut().expect("task was just inserted")
        };
        if !task.sessions.iter().any(|session| {
            session.provider == provider
                && session.account == account
                && session.native_session_id == native_session_id
        }) {
            task.sessions.push(TaskSession {
                provider,
                account: account.to_owned(),
                native_session_id: native_session_id.to_owned(),
                workspace: workspace.to_owned(),
                linked_at: now.clone(),
            });
        }
        task.updated_at = now;
        if let Some(map) = map {
            task.last_map = Some(map);
        }
        Ok(())
    })
}

pub fn list_tasks(root: &Path) -> Vec<TaskRecord> {
    let mut tasks = load_store(root).tasks;
    tasks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    tasks
}

pub fn task(root: &Path, id: &str) -> Option<TaskRecord> {
    load_store(root)
        .tasks
        .into_iter()
        .find(|task| task.id == id)
}
pub fn set_file_action(root: &Path, action: crate::file_open::FileAction) -> Result<()> {
    modify_store(root, |store| {
        store.settings.file_action = Some(action);
        Ok(())
    })
}

pub fn set_continuation_automatic(root: &Path, automatic: bool) -> Result<()> {
    modify_store(root, |store| {
        store.settings.continuation_automatic = Some(automatic);
        Ok(())
    })
}

pub fn same_provider_automatic(root: &Path) -> bool {
    get_settings(root).same_provider_automatic
        || crate::config::for_root(root)
            .continuation
            .same_provider_automatic
}

pub fn accept_same_provider_terms(root: &Path, provider: ProviderKind) -> Result<()> {
    modify_store(root, |store| {
        store
            .settings
            .accepted_terms_providers
            .insert(provider.as_str().into());
        store.settings.same_provider_automatic = true;
        Ok(())
    })
}

pub fn withdraw_same_provider_terms(root: &Path, provider: ProviderKind) -> Result<()> {
    modify_store(root, |store| {
        store
            .settings
            .accepted_terms_providers
            .remove(provider.as_str());
        if store.settings.accepted_terms_providers.is_empty() {
            store.settings.same_provider_automatic = false;
        }
        Ok(())
    })
}

pub fn same_provider_terms_accepted(root: &Path, provider: ProviderKind) -> bool {
    get_settings(root)
        .accepted_terms_providers
        .contains(provider.as_str())
}

pub fn automatic_handoff_allowed(root: &Path, account: &str, day: &str) -> bool {
    let settings = get_settings(root);
    let cap = crate::config::for_root(root)
        .continuation
        .automatic_handoff_cap
        .max(settings.automatic_handoff_cap);
    settings
        .automatic_handoffs_today
        .get(&format!("{day}:{account}"))
        .copied()
        .unwrap_or(0)
        < cap as u8
}

pub fn record_automatic_handoff(root: &Path, account: &str, day: &str) -> Result<()> {
    modify_store(root, |store| {
        let key = format!("{day}:{account}");
        let count = store
            .settings
            .automatic_handoffs_today
            .entry(key)
            .or_default();
        *count = count.saturating_add(1);
        Ok(())
    })
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
    let home = crate::accounts::profile_home();
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
    fn handoff_appends_session_to_same_task() {
        let dir = tempfile::tempdir().unwrap();
        record_task_session(
            dir.path(),
            "task-1",
            "Fix parser",
            "C:\\work",
            ProviderKind::Codex,
            "a",
            "s1",
            None,
        )
        .unwrap();
        record_task_session(
            dir.path(),
            "task-1",
            "Fix parser",
            "C:\\work",
            ProviderKind::Claude,
            "b",
            "s2",
            Some(TaskMapPointer {
                version: 2,
                thread_id: "thread-1".into(),
            }),
        )
        .unwrap();
        let task = task(dir.path(), "task-1").unwrap();
        assert_eq!(task.sessions.len(), 2);
        assert_eq!(task.sessions[1].provider, ProviderKind::Claude);
        assert_eq!(task.last_map.unwrap().version, 2);
    }

    #[test]
    fn resume_lists_tasks_with_session_chains() {
        let dir = tempfile::tempdir().unwrap();
        record_task_session(
            dir.path(),
            "task-1",
            "Review",
            "C:\\work",
            ProviderKind::Cursor,
            "c",
            "s1",
            None,
        )
        .unwrap();
        record_task_session(
            dir.path(),
            "task-1",
            "Review",
            "C:\\work",
            ProviderKind::Codex,
            "d",
            "s2",
            None,
        )
        .unwrap();
        assert_eq!(list_tasks(dir.path())[0].sessions.len(), 2);
    }

    #[test]
    fn legacy_sessions_appear_as_single_session_tasks() {
        let sessions = vec![crate::sessions::SessionSummary {
            account: "legacy".into(),
            provider: ProviderKind::Claude,
            session_id: "native-1".into(),
            cwd: Some("C:\\work".into()),
            preview: Some("Old request".into()),
            modified_ms: 1,
            user_messages: 1,
        }];
        let tasks = crate::sessions::legacy_session_tasks(&sessions);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].sessions.len(), 1);
        assert!(tasks[0].id.starts_with("legacy-"));
    }

    #[test]
    fn task_survives_app_restart() {
        let dir = tempfile::tempdir().unwrap();
        record_task_session(
            dir.path(),
            "task-1",
            "Persist me",
            "C:\\work",
            ProviderKind::Codex,
            "a",
            "s1",
            None,
        )
        .unwrap();
        assert_eq!(load_store(dir.path()).tasks[0].title, "Persist me");
        assert_eq!(
            list_tasks(dir.path())[0].sessions[0].native_session_id,
            "s1"
        );
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
    fn same_provider_auto_handoff_off_by_default() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!same_provider_automatic(dir.path()));
        assert!(!same_provider_terms_accepted(
            dir.path(),
            ProviderKind::Codex
        ));
    }

    #[test]
    fn enabling_requires_terms_notice_once() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!same_provider_automatic(dir.path()));
        accept_same_provider_terms(dir.path(), ProviderKind::Claude).unwrap();
        assert!(same_provider_automatic(dir.path()));
        assert!(same_provider_terms_accepted(
            dir.path(),
            ProviderKind::Claude
        ));
        accept_same_provider_terms(dir.path(), ProviderKind::Claude).unwrap();
    }

    #[test]
    fn per_account_cap_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let day = "2026-11-06";
        for _ in 0..3 {
            assert!(automatic_handoff_allowed(dir.path(), "codex-2", day));
            record_automatic_handoff(dir.path(), "codex-2", day).unwrap();
        }
        assert!(!automatic_handoff_allowed(dir.path(), "codex-2", day));
        assert!(automatic_handoff_allowed(dir.path(), "codex-3", day));
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
