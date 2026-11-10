use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Complete reads and writes fail visibly above this bound; original records are never truncated.
const STORE_BYTES: u64 = 32 * 1024 * 1024;

fn read_store_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(STORE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > STORE_BYTES {
        return Err(std::io::Error::other(
            "store exceeds the 32 MiB complete-read budget; original preserved; inspect it with a compatible tool",
        ));
    }
    Ok(bytes)
}

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
    pub worker_panel_open: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_action: Option<crate::file_open::FileAction>,
    #[serde(default = "default_trust")]
    pub trust: TrustMode,
    #[serde(default = "default_child_trust")]
    pub child_trust: ChildTrust,
    /// Accounts that may run as delegated workers (configured ceiling, never the orchestrator).
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
            worker_panel_open: None,
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

/// Checked read status for views that must distinguish missing records from unreadable state.
pub fn store_read_error(root: &Path) -> Option<String> {
    load_store_checked(root)
        .err()
        .map(|error| format!("{error:#}"))
}

pub fn load_store(root: &Path) -> OrchestratorStore {
    load_store_checked(root).unwrap_or_else(|_| fresh_store(root))
}

/// A mutation must not turn unreadable, corrupt or future-version state into defaults.
pub(crate) fn load_store_checked(root: &Path) -> Result<OrchestratorStore> {
    crate::test_guard::check_not_real_state(root);
    let path = store_path(root);
    let raw = match read_store_bytes(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(fresh_store(root)),
        Err(error) => return Err(error).context("store could not be read; original preserved"),
    };
    let value: serde_json::Value = serde_json::from_slice(&raw)
        .context("store is invalid; original preserved; inspect the saved store before retrying")?;
    let has_mode = value.pointer("/settings/trust").is_some();
    let mut store: OrchestratorStore =
        serde_json::from_value(value).context("store format is invalid; original preserved")?;
    if store.version != 1 {
        anyhow::bail!(
            "unsupported store version {}; original preserved; use a compatible Concordia build",
            store.version
        );
    }
    store.settings = sanitize_settings(store.settings);
    if !has_mode {
        store.settings.trust = crate::config::for_root(root).mode.default;
    }
    Ok(store)
}

/// Whether the orchestrator may propose delegations: the user's saved choice, else the config
/// default. A value saved in the store always wins.
pub fn delegation_protocol_on(root: &Path) -> bool {
    get_settings(root)
        .delegation_protocol
        .unwrap_or_else(|| crate::config::for_root(root).delegation.protocol)
}

/// Write an already checked store atomically (temp file + rename). Normal mutations must use
/// `modify_store`, which refuses invalid existing state before running the mutation. If the
/// existing file becomes malformed before this final write, preserve its exact bytes in a backup.
pub fn save_store(root: &Path, store: &OrchestratorStore) -> Result<()> {
    crate::test_guard::check_not_real_state(root);
    let dir = store_dir(root);
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = store_path(root);
    match read_store_bytes(&path) {
        Ok(raw) => match serde_json::from_slice::<OrchestratorStore>(&raw) {
            Ok(existing) if existing.version != 1 => anyhow::bail!(
                "unsupported existing store version {}; original preserved",
                existing.version
            ),
            Ok(_) => {}
            Err(_) => {
                let backup = dir.join(format!("store.json.corrupt-{}", uuid::Uuid::new_v4()));
                crate::state_io::write_atomic_new(&backup, &raw).context(
                    "could not preserve corrupt store backup; original was not overwritten",
                )?;
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).context("could not inspect existing store; original preserved")
        }
    }
    let json = serde_json::to_string_pretty(store).context("serialize store")?;
    if json.len() as u64 > STORE_BYTES {
        anyhow::bail!("store exceeds the 32 MiB write budget; original preserved");
    }
    crate::state_io::write_atomic(&path, json.as_bytes())
        .with_context(|| format!("replace {}; original preserved on failure", path.display()))
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
    fs::create_dir_all(store_dir(root))?;
    let mutate = || -> Result<T> {
        let mut store = load_store_checked(root)?;
        let out = f(&mut store)?;
        save_store(root, &store)?;
        Ok(out)
    };
    // Keep the lock inode stable: OS ownership ends on close or process exit.
    // An old timestamp never authorizes taking another writer's lock.
    crate::state_io::with_lock_file(&store_dir(root).join("store.lock"), |file, created_new| {
        const MARKER: &[u8] = b"concordia-store-lock-v1\n";
        if created_new {
            file.write_all(MARKER)?;
            file.flush()?;
        } else {
            let mut marker = Vec::new();
            file.take(MARKER.len() as u64 + 1)
                .read_to_end(&mut marker)?;
            if marker != MARKER {
                return Err(std::io::Error::other(
                        "state is locked by an older Concordia writer or an unrecognized lock; original preserved; let that writer finish and retry",
                    ));
            }
        }
        Ok(mutate())
    })?
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
    fn native_sessions_have_direct_resume_rows() {
        let sessions = vec![crate::sessions::SessionSummary {
            account: "legacy".into(),
            provider: ProviderKind::Claude,
            session_id: "native-1".into(),
            cwd: Some("C:\\work".into()),
            preview: Some("Old request".into()),
            modified_ms: 1,
            user_messages: 1,
        }];
        let rows = crate::sessions::native_session_picker_rows(&sessions);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "resume:legacy:native-1");
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
    fn a_save_leaves_only_the_store_and_stable_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        upsert_stored_account(dir.path(), acct("a")).unwrap();
        let names = || -> Vec<String> {
            fs::read_dir(store_dir(dir.path()))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        };
        assert!(
            eventually(|| names()
                .iter()
                .all(|name| matches!(name.as_str(), "store.json" | "store.lock"))
                && names().len() == 2),
            "{:?}",
            names()
        );
    }

    #[test]
    fn an_unreadable_store_rejects_mutation_and_preserves_original_bytes() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(store_dir(dir.path())).unwrap();
        let original = b"{ this is not json";
        fs::write(store_path(dir.path()), original).unwrap();
        let mut mutation_ran = false;
        let error = modify_store(dir.path(), |store| {
            mutation_ran = true;
            store.accounts.push(acct("fresh"));
            Ok(())
        })
        .unwrap_err();
        assert!(error.to_string().contains("original preserved"));
        assert!(!mutation_ran, "invalid state must reject before mutation");
        assert_eq!(fs::read(store_path(dir.path())).unwrap(), original);
        let names: Vec<_> = fs::read_dir(store_dir(dir.path()))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2);
        assert!(
            names
                .iter()
                .all(|name| matches!(name.as_str(), "store.json" | "store.lock")),
            "no replacement or backup was needed"
        );
    }

    #[test]
    fn an_old_unmarked_lock_blocks_writes_until_the_legacy_owner_removes_it() {
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
        let error = upsert_stored_account(dir.path(), acct("a")).unwrap_err();
        assert!(error.to_string().contains("older Concordia writer"));
        assert_eq!(fs::read(&lock).unwrap(), b"");
        assert!(!store_path(dir.path()).exists());
        // Simulate only this synthetic legacy owner releasing its fixture pathname.
        fs::remove_file(&lock).unwrap();
        upsert_stored_account(dir.path(), acct("a")).unwrap();
        assert_eq!(load_store(dir.path()).accounts.len(), 1);
        assert!(lock.exists(), "the stable lock file must remain");
        assert_eq!(fs::read(&lock).unwrap(), b"concordia-store-lock-v1\n");
        assert!(fs::File::options()
            .read(true)
            .write(true)
            .open(&lock)
            .unwrap()
            .try_lock()
            .is_ok());
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
        assert!(
            fs::File::options()
                .read(true)
                .write(true)
                .open(&lock)
                .unwrap()
                .try_lock()
                .is_ok(),
            "lock ownership was not released"
        );
        // ...and it really is free: the next update goes straight through.
        upsert_stored_account(dir.path(), acct("next")).unwrap();
    }
}
