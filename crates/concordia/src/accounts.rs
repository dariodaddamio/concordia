use crate::store::{list_stored_accounts, ProviderKind, StoredAccount};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AccountDef {
    pub provider: ProviderKind,
    pub account: String,
    pub home_path: Option<PathBuf>,
    pub label: String,
    pub custom: bool,
}

/// Serializes tests that point `profile_home()` at a temp dir via the environment.
#[cfg(test)]
pub(crate) static TEST_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn profile_home() -> PathBuf {
    std::env::var("CONCORDIA_CODEX_HOME_BASE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::test_guard::test_home())
}

pub fn normalize_account_id(id: &str) -> String {
    match id {
        "codex-a" | "codex-default" => "codex".into(),
        "codex-b" => "codex-2".into(),
        "claude-default" | "claude-a" => "claude".into(),
        "cursor-default" | "cursor-a" => "cursor".into(),
        other => other.to_string(),
    }
}

pub fn display_account_name(provider: ProviderKind, account: &str) -> String {
    let cap = match provider {
        ProviderKind::Codex => "Codex",
        ProviderKind::Claude => "Claude",
        ProviderKind::Cursor => "Cursor",
    };
    let id = normalize_account_id(account);
    if id == provider.as_str() {
        return cap.to_string();
    }
    if let Some(n) = id.strip_prefix(&format!("{}-", provider.as_str())) {
        if n.chars().all(|c| c.is_ascii_digit()) {
            return format!("{cap} {n}");
        }
    }
    id
}

/// A nickname changes display only, never account ids, credential homes or removal ownership.
pub fn set_nickname(root: &Path, account: &str, name: &str) -> anyhow::Result<()> {
    let id = normalize_account_id(account);
    anyhow::ensure!(get_account(root, &id).is_some(), "unknown account: {id}");
    let name = name.trim();
    anyhow::ensure!(
        name.chars().count() <= 48 && !name.chars().any(char::is_control),
        "nickname must be at most 48 characters without control characters"
    );
    crate::store::modify_store(root, |store| {
        if name.is_empty() {
            store.account_nicknames.remove(&id);
        } else {
            store.account_nicknames.insert(id, name.to_string());
        }
        Ok(())
    })
}

/// Whether `home` holds a usable login for `provider` (existence/shape check only).
pub fn is_logged_in(provider: ProviderKind, home: &Path) -> bool {
    crate::test_guard::check_not_real_state(home);
    dir_logged_in(provider, home)
}

fn dir_logged_in(provider: ProviderKind, home: &Path) -> bool {
    match provider {
        ProviderKind::Codex => home.join("auth.json").is_file(),
        ProviderKind::Claude => {
            let cred = if home.join(".credentials.json").is_file() {
                home.join(".credentials.json")
            } else {
                home.join("credentials.json")
            };
            // Existence and size only: whether a login is present never needs the token itself.
            fs::metadata(&cred)
                .map(|m| m.is_file() && m.len() > 20)
                .unwrap_or(false)
        }
        ProviderKind::Cursor => cursor_home_ready(home),
    }
}

fn cursor_home_ready(home: &Path) -> bool {
    for name in ["concordia-api-key", "api-key"] {
        let p = home.join(name);
        if p.is_file()
            && fs::read_to_string(&p)
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        {
            return true;
        }
    }
    let cfg = home.join("cli-config.json");
    if !cfg.is_file() {
        return false;
    }
    fs::read_to_string(&cfg)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|j| {
            j.get("authInfo")
                .and_then(|a| a.get("email"))
                .and_then(|e| e.as_str())
                .map(|e| !e.is_empty())
        })
        .unwrap_or(false)
}

pub fn builtin_accounts() -> Vec<AccountDef> {
    let home = profile_home();
    vec![
        AccountDef {
            provider: ProviderKind::Codex,
            account: "codex".into(),
            home_path: Some(
                std::env::var("AO_CODEX_HOME")
                    .or_else(|_| std::env::var("AO_CODEX_HOME_A"))
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| home.join(".codex")),
            ),
            label: "Codex".into(),
            custom: false,
        },
        AccountDef {
            provider: ProviderKind::Codex,
            account: "codex-2".into(),
            home_path: Some(
                std::env::var("AO_CODEX_HOME_2")
                    .or_else(|_| std::env::var("AO_CODEX_HOME_B"))
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| home.join(".codex-2")),
            ),
            label: "Codex 2".into(),
            custom: false,
        },
        AccountDef {
            provider: ProviderKind::Claude,
            account: "claude".into(),
            home_path: Some(
                std::env::var("AO_CLAUDE_CONFIG_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| home.join(".claude")),
            ),
            label: "Claude".into(),
            custom: false,
        },
        AccountDef {
            provider: ProviderKind::Cursor,
            account: "cursor".into(),
            home_path: Some(
                std::env::var("AO_CURSOR_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| home.join(".cursor")),
            ),
            label: "Cursor".into(),
            custom: false,
        },
    ]
}

fn discover_numbered_homes(
    provider: ProviderKind,
    prefix: &str,
    skip_ids: &[&str],
) -> Vec<AccountDef> {
    let home = profile_home();
    let Ok(entries) = fs::read_dir(&home) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == prefix {
            continue;
        }
        let pattern = format!("{prefix}-");
        let Some(suffix) = name.strip_prefix(&pattern) else {
            continue;
        };
        if !suffix.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let id = if provider == ProviderKind::Codex && suffix == "2" {
            "codex-2".to_string()
        } else {
            format!("{}-{}", provider.as_str(), suffix)
        };
        if skip_ids.contains(&id.as_str()) {
            continue;
        }
        let home_path = entry.path();
        if !dir_logged_in(provider, &home_path) {
            continue;
        }
        let label = format!(
            "{}{}",
            match provider {
                ProviderKind::Codex => "Codex",
                ProviderKind::Claude => "Claude",
                ProviderKind::Cursor => "Cursor",
            },
            suffix
        );
        found.push(AccountDef {
            provider,
            account: id,
            home_path: Some(home_path),
            label,
            custom: false,
        });
    }
    found.sort_by(|a, b| a.account.cmp(&b.account));
    found
}

pub fn list_all_accounts(state_root: &Path) -> Vec<AccountDef> {
    let mut by_id: HashMap<String, AccountDef> = HashMap::new();
    for a in builtin_accounts() {
        by_id.insert(a.account.clone(), a);
    }
    for a in discover_numbered_homes(ProviderKind::Codex, ".codex", &["codex-2"]) {
        by_id.entry(a.account.clone()).or_insert(a);
    }
    for a in discover_numbered_homes(ProviderKind::Claude, ".claude", &[]) {
        by_id.entry(a.account.clone()).or_insert(a);
    }
    for a in discover_numbered_homes(ProviderKind::Cursor, ".cursor", &[]) {
        by_id.entry(a.account.clone()).or_insert(a);
    }
    for s in list_stored_accounts(state_root) {
        merge_stored(&mut by_id, s);
    }
    let store = crate::store::load_store(state_root);
    let hidden = store.hidden_accounts;
    let mut out: Vec<_> = by_id
        .into_values()
        .filter(|a| !hidden.contains(&a.account))
        .collect();
    for a in &mut out {
        if !a.custom {
            a.label = display_account_name(a.provider, &a.account);
        }
        if let Some(name) = store.account_nicknames.get(&a.account) {
            a.label = format!("{name} ({})", display_account_name(a.provider, &a.account));
        }
    }
    out.sort_by(|a, b| {
        a.provider
            .as_str()
            .cmp(b.provider.as_str())
            .then_with(|| a.account.cmp(&b.account))
    });
    out
}

/// Who is signed in to this account's home: the email (or provider account id) it reports,
/// lower-cased. `None` when not logged in or the home does not say.
pub fn identity_key(provider: ProviderKind, home: &Path) -> Option<String> {
    let id = crate::identity::inspect_provider(provider, home);
    if !id.logged_in {
        return None;
    }
    id.login
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty())
}

/// Groups of logged-in accounts of one provider that are the *same login* (two slots signed in
/// to one subscription). Each group has at least two accounts, ordered built-in/older first.
pub fn duplicate_groups(state_root: &Path) -> Vec<Vec<AccountDef>> {
    let mut by_key: std::collections::BTreeMap<(String, String), Vec<AccountDef>> =
        std::collections::BTreeMap::new();
    for a in accounts_with_auth(state_root) {
        let Some(home) = a.home_path.clone() else {
            continue;
        };
        if let Some(key) = identity_key(a.provider, &home) {
            by_key
                .entry((a.provider.as_str().to_string(), key))
                .or_default()
                .push(a);
        }
    }
    by_key
        .into_values()
        .filter(|g| g.len() > 1)
        .map(|mut g| {
            g.sort_by(|a, b| {
                a.custom
                    .cmp(&b.custom)
                    .then_with(|| a.account.cmp(&b.account))
            });
            g
        })
        .collect()
}

/// The existing account that `id` duplicates, if any (never `id` itself).
pub fn duplicate_of(state_root: &Path, id: &str) -> Option<AccountDef> {
    let want = normalize_account_id(id);
    duplicate_groups(state_root)
        .into_iter()
        .find(|g| g.iter().any(|a| a.account == want))
        .and_then(|g| g.into_iter().find(|a| a.account != want))
}

fn merge_stored(by_id: &mut HashMap<String, AccountDef>, s: StoredAccount) {
    let id = normalize_account_id(&s.id);
    by_id.insert(
        id.clone(),
        AccountDef {
            provider: s.provider,
            account: id,
            home_path: s.home_path.map(PathBuf::from),
            label: s.label,
            custom: true,
        },
    );
}

pub fn get_account(state_root: &Path, id: &str) -> Option<AccountDef> {
    let want = normalize_account_id(id);
    list_all_accounts(state_root)
        .into_iter()
        .find(|a| a.account == want)
}

pub fn accounts_with_auth(state_root: &Path) -> Vec<AccountDef> {
    list_all_accounts(state_root)
        .into_iter()
        .filter(|a| {
            a.home_path
                .as_ref()
                .map(|h| dir_logged_in(a.provider, h))
                .unwrap_or(false)
        })
        .collect()
}
