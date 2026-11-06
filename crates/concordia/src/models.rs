//! Deterministic model discovery with explicit, bounded provider refreshes.

use crate::accounts::{get_account, list_all_accounts};
use crate::catalog;
use crate::store::{default_store_root, ProviderKind};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

const REFRESH_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub display_name: String,
    pub family: String,
    pub source: ModelSource,
    #[serde(default)]
    pub deprecated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelSource {
    ListedByCli,
    Cached,
    Catalog,
    Typed,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModelCache {
    fetched_at: String,
    models: Vec<ModelEntry>,
}

pub fn available(provider: ProviderKind, account: &str) -> Vec<ModelEntry> {
    let root = default_store_root();
    available_at(&root, provider, account)
}

fn available_at(root: &Path, provider: ProviderKind, account: &str) -> Vec<ModelEntry> {
    let mut out = Vec::new();
    let mut ids = HashSet::new();
    if let Some(cache) = read_refresh_cache(root, provider, account) {
        merge(&mut out, &mut ids, cache);
    }
    for candidate in list_all_accounts(root)
        .into_iter()
        .filter(|a| a.provider == provider)
    {
        if let Some(home) = candidate.home_path {
            for (id, display_name) in catalog::list_model_details(provider, &home) {
                add(
                    &mut out,
                    &mut ids,
                    entry_named(id, display_name, ModelSource::Cached),
                );
            }
        }
    }
    for item in catalog_entries(provider) {
        add(&mut out, &mut ids, item);
    }
    out
}

/// Run the provider's supported list command, if one exists, and update the account cache.
/// The caller is bounded even when a provider process hangs.
pub fn refresh(provider: ProviderKind, account: &str) -> Vec<ModelEntry> {
    let root = default_store_root();
    let Some(def) = get_account(&root, account) else {
        return Vec::new();
    };
    let Some(home) = def.home_path else {
        return Vec::new();
    };
    let Some(program) = list_program(provider) else {
        return available_at(&root, provider, account);
    };
    let home = home.clone();
    let Some(result) = run_bounded(
        move || {
            let result = crate::process_ui::command(program)
                .arg("--list-models")
                .current_dir(home)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| parse_list_output(&String::from_utf8_lossy(&o.stdout)));
            result
        },
        REFRESH_TIMEOUT,
    ) else {
        return available_at(&root, provider, account);
    };
    let Some(models) = result else {
        return available_at(&root, provider, account);
    };
    let _ = write_refresh_cache(&root, provider, account, &models);
    available_at(&root, provider, account)
}

fn run_bounded<F, T>(job: F, timeout: Duration) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(job());
    });
    rx.recv_timeout(timeout).ok()
}

fn list_program(provider: ProviderKind) -> Option<&'static str> {
    match provider {
        ProviderKind::Cursor => Some("agent"),
        ProviderKind::Codex | ProviderKind::Claude => None,
    }
}

fn parse_list_output(raw: &str) -> Vec<ModelEntry> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|id| entry(id.to_string(), ModelSource::ListedByCli))
        .collect()
}

fn entry(id: String, source: ModelSource) -> ModelEntry {
    entry_named(id.clone(), id, source)
}

fn entry_named(id: String, display_name: String, source: ModelSource) -> ModelEntry {
    ModelEntry {
        display_name,
        family: family(&id),
        id,
        source,
        deprecated: false,
    }
}

fn family(id: &str) -> String {
    id.split(['-', '/']).next().unwrap_or(id).to_string()
}

fn catalog_entries(provider: ProviderKind) -> Vec<ModelEntry> {
    #[derive(Deserialize)]
    struct File {
        models: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        provider: ProviderKind,
        id: String,
        display_name: String,
        family: String,
        #[serde(default)]
        deprecated: bool,
    }
    toml::from_str::<File>(include_str!("../assets/models.toml"))
        .map(|f| {
            f.models
                .into_iter()
                .filter(|m| m.provider == provider)
                .map(|m| ModelEntry {
                    id: m.id,
                    display_name: m.display_name,
                    family: m.family,
                    source: ModelSource::Catalog,
                    deprecated: m.deprecated,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn cache_path(root: &Path, provider: ProviderKind, account: &str) -> PathBuf {
    root.join("cache").join(format!(
        "models-{}-{}.json",
        provider.as_str(),
        safe(account)
    ))
}

fn safe(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn read_refresh_cache(
    root: &Path,
    provider: ProviderKind,
    account: &str,
) -> Option<Vec<ModelEntry>> {
    serde_json::from_str::<ModelCache>(
        &fs::read_to_string(cache_path(root, provider, account)).ok()?,
    )
    .ok()
    .map(|c| c.models)
}

fn write_refresh_cache(
    root: &Path,
    provider: ProviderKind,
    account: &str,
    models: &[ModelEntry],
) -> std::io::Result<()> {
    let path = cache_path(root, provider, account);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let cache = ModelCache {
        fetched_at: now_stamp(),
        models: models.to_vec(),
    };
    fs::write(
        path,
        serde_json::to_vec_pretty(&cache).expect("model cache serialization"),
    )
}

fn merge(out: &mut Vec<ModelEntry>, ids: &mut HashSet<String>, items: Vec<ModelEntry>) {
    for item in items {
        add(out, ids, item);
    }
}
fn add(out: &mut Vec<ModelEntry>, ids: &mut HashSet<String>, item: ModelEntry) {
    if ids.insert(item.id.clone()) {
        out.push(item);
    }
}
fn now_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}Z", dur.as_secs(), dur.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn catalog_parses_and_covers_all_three_providers() {
        for provider in [
            ProviderKind::Codex,
            ProviderKind::Claude,
            ProviderKind::Cursor,
        ] {
            assert!(
                !catalog_entries(provider).is_empty(),
                "missing {provider:?}"
            );
        }
    }

    #[test]
    fn codex_catalog_lists_current_models() {
        let ids: Vec<_> = catalog_entries(ProviderKind::Codex)
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"]);
    }

    #[test]
    fn available_merges_list_cache_and_catalog_without_duplicates() {
        let d = tempdir().unwrap();
        let home = d.path().join("home");
        fs::create_dir_all(home.join("cache")).unwrap();
        fs::write(
            home.join("cache/model-catalog.json"),
            r#"[{"id":"cached-model"},{"id":"gpt-5"}]"#,
        )
        .unwrap();
        crate::store::upsert_stored_account(
            d.path(),
            crate::store::StoredAccount {
                id: "claude".into(),
                provider: ProviderKind::Claude,
                label: "fixture".into(),
                home_path: Some(home.to_str().unwrap().into()),
                created_at: "now".into(),
            },
        )
        .unwrap();
        fs::create_dir_all(d.path().join("cache")).unwrap();
        fs::write(d.path().join("cache/models-claude-claude.json"), r#"{"fetched_at":"now","models":[{"id":"listed","display_name":"Listed","family":"listed","source":"ListedByCli","deprecated":false},{"id":"gpt-5","display_name":"wrong","family":"wrong","source":"ListedByCli","deprecated":false}]}"#).unwrap();
        let models = available_at(d.path(), ProviderKind::Claude, "claude");
        assert_eq!(models.iter().filter(|m| m.id == "gpt-5").count(), 1);
        assert_eq!(models[0].id, "listed");
        assert!(models
            .iter()
            .any(|m| m.id == "cached-model" && m.source == ModelSource::Cached));
    }

    #[test]
    fn hung_list_command_times_out_without_blocking() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();

        let result = run_bounded(
            move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                finished_tx.send(()).unwrap();
                42
            },
            Duration::from_millis(50),
        );
        started_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("fake list runner did not start");
        assert!(result.is_none(), "bounded list call waited for the runner");

        release_tx.send(()).unwrap();
        finished_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("fake list runner did not finish after release");
    }

    #[test]
    fn refresh_without_a_provider_list_command_is_non_blocking() {
        let start = std::time::Instant::now();
        let _ = refresh(ProviderKind::Codex, "missing-fixture-account");
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
