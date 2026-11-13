//! Deterministic model discovery with explicit, bounded provider refreshes.

use crate::accounts::get_account;
use crate::catalog;
use crate::store::{default_store_root, ProviderKind};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

const REFRESH_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CACHE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub display_name: String,
    pub family: String,
    #[serde(default)]
    pub tier: ModelTier,
    pub source: ModelSource,
    /// Reasoning efforts the provider lists for this model, in effort order.
    #[serde(default)]
    pub efforts: Vec<ModelEffort>,
    /// The effort the provider applies when none is chosen, when it reports one.
    #[serde(default)]
    pub default_effort: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelEffort {
    pub name: String,
    /// The provider's own model id for this effort, when it encodes effort in the model id
    /// (Cursor's `agent models` lists `model-high`, `model-low`, ...). `None` means the effort is
    /// passed separately from the model.
    #[serde(default)]
    pub model_id: Option<String>,
}

impl ModelEntry {
    pub fn effort_names(&self) -> Vec<&str> {
        self.efforts.iter().map(|e| e.name.as_str()).collect()
    }

    /// The model id a turn runs with for `effort`: the provider's own variant id when it has one.
    pub fn id_for_effort(&self, effort: Option<&str>) -> &str {
        effort
            .and_then(|effort| self.efforts.iter().find(|e| e.name == effort))
            .and_then(|e| e.model_id.as_deref())
            .unwrap_or(&self.id)
    }
}

/// Effort words the providers use, lowest first. Used only to order efforts and to recognise an
/// effort word in a Cursor model id; which efforts a model supports always comes from the provider.
const EFFORT_ORDER: [&str; 8] = [
    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    Fast,
    #[default]
    Standard,
    Frontier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelSource {
    ListedByCli,
    Cached,
    Typed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelIdentity {
    provider: ProviderKind,
    account: String,
    home: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelCache {
    version: u8,
    identity: ModelIdentity,
    fetched_at: u64,
    models: Vec<ModelEntry>,
}

fn selected_identity(root: &Path, provider: ProviderKind, account: &str) -> Option<ModelIdentity> {
    let selected = get_account(root, account)?;
    if selected.provider != provider {
        return None;
    }
    let home = selected.home_path?.canonicalize().ok()?;
    home.is_dir().then_some(ModelIdentity {
        provider,
        account: selected.account,
        home,
    })
}

pub fn available(provider: ProviderKind, account: &str) -> Vec<ModelEntry> {
    let root = default_store_root();
    available_at(&root, provider, account)
}

pub(crate) fn available_at(root: &Path, provider: ProviderKind, account: &str) -> Vec<ModelEntry> {
    let Some(identity) = selected_identity(root, provider, account) else {
        return Vec::new();
    };
    // The provider's own list for this account wins; without one, the provider's local metadata.
    if let Some(cache) = read_refresh_cache(root, &identity) {
        return cache.models;
    }
    let mut out = Vec::new();
    let mut ids = HashSet::new();
    for model in catalog::list_model_details(provider, &identity.home) {
        add(&mut out, &mut ids, native_entry(model));
    }
    out
}

pub(crate) fn find_at(
    root: &Path,
    provider: ProviderKind,
    account: &str,
    model_id: &str,
) -> Option<ModelEntry> {
    available_at(root, provider, account)
        .into_iter()
        .find(|model| {
            model.id == model_id
                || model
                    .efforts
                    .iter()
                    .any(|effort| effort.model_id.as_deref() == Some(model_id))
        })
}

fn native_entry(model: catalog::NativeModel) -> ModelEntry {
    let mut entry = entry_named(model.id, model.display_name, ModelSource::Cached);
    entry.efforts = model
        .efforts
        .into_iter()
        .map(|name| ModelEffort {
            name,
            model_id: None,
        })
        .collect();
    entry.default_effort = model.default_effort;
    entry
}

/// Current native metadata captured for one physical account home.
pub struct RefreshedModels {
    pub home: PathBuf,
    pub models: Vec<ModelEntry>,
}

/// Read current native metadata without a model request. Failure never masquerades as refresh.
pub fn refresh(provider: ProviderKind, account: &str) -> Result<RefreshedModels, String> {
    let root = default_store_root();
    refresh_at(&root, provider, account)
}

pub(crate) fn refresh_at(
    root: &Path,
    provider: ProviderKind,
    account: &str,
) -> Result<RefreshedModels, String> {
    let identity = selected_identity(&root, provider, account)
        .ok_or("selected model account/provider/home is unavailable")?;
    let models = match provider {
        ProviderKind::Claude => list_claude(&identity.home)?,
        ProviderKind::Cursor => list_cursor(&identity.home)?,
        ProviderKind::Codex => {
            let models = catalog::list_model_details(provider, &identity.home)
                .into_iter()
                .map(native_entry)
                .collect::<Vec<_>>();
            if models.is_empty() {
                return Err(
                    "Codex native model metadata is unavailable; no listing was invented".into(),
                );
            }
            return Ok(RefreshedModels {
                home: identity.home,
                models,
            });
        }
        ProviderKind::Grok => {
            let models = catalog::list_model_details(provider, &identity.home)
                .into_iter()
                .map(native_entry)
                .collect::<Vec<_>>();
            if models.is_empty() {
                return Err(
                    "Grok Build model metadata is unavailable; no listing was invented".into(),
                );
            }
            models
        }
        ProviderKind::Opencode => {
            let models = catalog::list_model_details(provider, &identity.home)
                .into_iter()
                .map(native_entry)
                .collect::<Vec<_>>();
            if models.is_empty() {
                return Err(
                    "opencode model metadata is unavailable; no listing was invented".into(),
                );
            }
            models
        }
    };
    write_refresh_cache(&root, &identity, &models)
        .map_err(|error| format!("model metadata cache was not saved: {error}"))?;
    Ok(RefreshedModels {
        home: identity.home,
        models,
    })
}

/// The one account/home-bound read path for routing, dependencies and model controls.
pub(crate) fn ids_at(
    root: &Path,
    provider: ProviderKind,
    account: &str,
    home: &Path,
) -> Vec<String> {
    let Some(identity) = selected_identity(root, provider, account) else {
        return Vec::new();
    };
    if home.canonicalize().ok().as_ref() != Some(&identity.home) {
        return Vec::new();
    }
    available_at(root, provider, account)
        .into_iter()
        .map(|model| model.id)
        .collect()
}

/// Refresh only when the account has no listed catalog yet or it is older than a day.
pub fn needs_refresh(provider: ProviderKind, account: &str) -> bool {
    needs_refresh_at(&default_store_root(), provider, account)
}

pub(crate) fn needs_refresh_at(root: &Path, provider: ProviderKind, account: &str) -> bool {
    if provider == ProviderKind::Codex {
        return false;
    }
    let Some(identity) = selected_identity(&root, provider, account) else {
        return false;
    };
    read_refresh_cache(&root, &identity)
        .is_none_or(|cache| unix_now().saturating_sub(cache.fetched_at) > 24 * 3600)
}

/// `agent models` lists every model the signed-in plan allows; it makes no model call.
fn list_cursor(home: &Path) -> Result<Vec<ModelEntry>, String> {
    let auth =
        crate::provider::cursor::account_auth(Some(home)).map_err(|error| error.to_string())?;
    let plan = crate::provider::PlannedTurn {
        command: crate::provider::cursor::find_agent_bin()?,
        argv: vec!["models".into()],
        env: auth.env,
        env_remove: auth.env_remove,
        cwd: home.to_string_lossy().into_owned(),
        prompt: "models".into(),
        new_session_id: None,
        provider: ProviderKind::Cursor,
    };
    let launch = crate::provider::launch::prepare(&plan).map_err(|error| error.to_string())?;
    let mut command = crate::process_ui::command(&launch.program);
    command
        .args(&launch.args)
        .envs(launch.env)
        .env("NO_COLOR", "1")
        .env_remove("CURSOR_API_KEY")
        .env_remove("CURSOR_AUTH_TOKEN")
        .current_dir(home);
    command.envs(plan.env);
    let output = crate::process_lifetime::capture(command, REFRESH_TIMEOUT, 1024 * 1024)?;
    if !output.status.success() {
        return Err(format!(
            "Cursor model listing failed (exit {:?})",
            output.status.code()
        ));
    }
    let models = parse_list_output(&String::from_utf8_lossy(&output.stdout));
    if models.is_empty() {
        return Err(
            "Cursor returned no model metadata; previous observations were not refreshed".into(),
        );
    }
    Ok(models)
}

/// The current SDK exposes supportedModels through initialize. No user frame is ever sent.
fn list_claude(home: &Path) -> Result<Vec<ModelEntry>, String> {
    let program = crate::provider::claude::find_claude_bin()?;
    if crate::provider::launch::is_batch_shim(&program) {
        return Err("Claude model initialization requires its current native executable".into());
    }
    let version = crate::provider::compat::run_cli(Path::new(&program), &["--version"])?;
    let required = crate::capacity::claude_capacity::CLI_VERSION;
    if crate::provider::compat::version_token(&version).as_deref() != Some(required) {
        return Err(format!(
            "Claude model metadata requires current CLI {required}"
        ));
    }
    let request = uuid::Uuid::new_v4().to_string();
    let input = format!("{}\n", serde_json::json!({"type":"control_request","request_id":request,"request":{"subtype":"initialize"}})).into_bytes();
    let mut command = crate::process_ui::command(&program);
    command
        .args([
            "--bare",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--setting-sources",
            "",
            "-p",
            "--verbose",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--permission-mode",
            "manual",
            "--permission-prompts",
            "host",
            "--permission-prompt-tool",
            "stdio",
        ])
        .env("CLAUDE_CONFIG_DIR", home)
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .current_dir(home);
    let output = crate::process_lifetime::capture_with_input(
        command,
        REFRESH_TIMEOUT,
        1024 * 1024,
        Some(input),
    )?;
    if !output.status.success() {
        return Err(format!(
            "Claude model initialization failed (exit {:?})",
            output.status.code()
        ));
    }
    parse_claude_models(&output.stdout, &request)
}

fn parse_claude_models(bytes: &[u8], request: &str) -> Result<Vec<ModelEntry>, String> {
    let mut observed = None;
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let value: serde_json::Value =
            serde_json::from_slice(line).map_err(|_| "Claude model metadata was not valid JSON")?;
        if value["type"] == "control_request" {
            return Err("Claude requested an action during metadata-only initialization; no permission was granted".into());
        }
        if value["type"] != "control_response" || value["response"]["request_id"] != request {
            continue;
        }
        if observed.is_some() {
            return Err("Claude returned duplicate model initialization responses".into());
        }
        let payload = &value["response"]["response"];
        if value["response"]["subtype"] != "success"
            || payload["claude_code_version"] != crate::capacity::claude_capacity::CLI_VERSION
        {
            return Err("Claude model initialization response/version mismatch".into());
        }
        let models = payload["models"]
            .as_array()
            .filter(|models| !models.is_empty() && models.len() <= 4096)
            .ok_or("Claude returned no bounded native model list")?;
        let mut entries = Vec::new();
        let mut ids = HashSet::new();
        for model in models {
            let id = model["value"]
                .as_str()
                .filter(|id| {
                    !id.is_empty()
                        && id.len() <= 256
                        && !id.chars().any(|c| c.is_whitespace() || c.is_control())
                })
                .ok_or("Claude returned an invalid native model value")?;
            let name = model["displayName"]
                .as_str()
                .filter(|name| {
                    !name.is_empty() && name.len() <= 512 && !name.chars().any(char::is_control)
                })
                .ok_or("Claude returned an invalid model display name")?;
            let mut entry = entry_named(id.into(), name.into(), ModelSource::ListedByCli);
            // `supportedEffortLevels` lists the efforts this model accepts; `supportsEffort:
            // false` means it takes none.
            if model["supportsEffort"].as_bool() != Some(false) {
                entry.efforts = model["supportedEffortLevels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_str)
                    .filter(|effort| catalog::valid_effort(effort))
                    .take(16)
                    .map(|name| ModelEffort {
                        name: name.to_owned(),
                        model_id: None,
                    })
                    .collect();
            }
            add(&mut entries, &mut ids, entry);
        }
        observed = Some(entries);
    }
    observed.ok_or_else(|| "Claude did not answer the exact metadata initialization request".into())
}

/// Lines look like `claude-opus-5-5-high - Claude Opus 5.5 1M High`; headers and notes have no
/// ` - ` separator or contain spaces in the id part. Variants that differ only by an effort word
/// in the id become one model whose efforts point at those variant ids.
fn parse_list_output(raw: &str) -> Vec<ModelEntry> {
    group_effort_variants(parse_list_lines(raw))
}

fn parse_list_lines(raw: &str) -> Vec<ModelEntry> {
    raw.lines()
        .filter_map(|line| {
            let (id, name) = line.trim().split_once(" - ")?;
            let id = id.trim();
            let name = name
                .trim()
                .trim_end_matches(" (default)")
                .chars()
                .filter(|c| !c.is_control() && !matches!(c, '\u{200b}'..='\u{200f}'))
                .collect::<String>();
            let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
            (!id.is_empty() && !id.contains(char::is_whitespace) && id.len() <= 120)
                .then(|| entry_named(id.into(), name, ModelSource::ListedByCli))
        })
        .collect()
}

/// `(id without its effort word, effort)` when exactly one `-`-separated id token is an effort.
fn split_effort(id: &str) -> Option<(String, &'static str)> {
    let tokens: Vec<&str> = id.split('-').collect();
    let mut found = tokens
        .iter()
        .enumerate()
        .skip(1)
        .filter_map(|(i, t)| EFFORT_ORDER.iter().find(|e| **e == *t).map(|e| (i, *e)));
    let (index, effort) = found.next()?;
    if found.next().is_some() {
        return None;
    }
    let base = tokens
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != index)
        .map(|(_, t)| *t)
        .collect::<Vec<_>>()
        .join("-");
    Some((base, effort))
}

/// The display name without the effort word the provider appended (`... 1M High`).
fn name_without_effort(name: &str, effort: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    match words.iter().rposition(|w| w.eq_ignore_ascii_case(effort)) {
        Some(at) if words.len() > 1 => words
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != at)
            .map(|(_, w)| *w)
            .collect::<Vec<_>>()
            .join(" "),
        _ => name.to_owned(),
    }
}

fn effort_rank(effort: &str) -> usize {
    EFFORT_ORDER
        .iter()
        .position(|e| *e == effort)
        .unwrap_or(EFFORT_ORDER.len())
}

fn group_effort_variants(entries: Vec<ModelEntry>) -> Vec<ModelEntry> {
    let mut out: Vec<ModelEntry> = Vec::new();
    for entry in entries {
        let Some((base, effort)) = split_effort(&entry.id) else {
            match out.iter_mut().find(|m| m.id == entry.id) {
                // The plain id arrived after its variants: keep its own name.
                Some(existing) => existing.display_name = entry.display_name,
                None => out.push(entry),
            }
            continue;
        };
        let variant = ModelEffort {
            name: effort.to_owned(),
            model_id: Some(entry.id.clone()),
        };
        match out.iter_mut().find(|m| m.id == base) {
            Some(group) => {
                if !group.efforts.iter().any(|e| e.name == effort) {
                    group.efforts.push(variant);
                }
            }
            None => {
                let name = name_without_effort(&entry.display_name, effort);
                let mut group = entry_named(base, name, entry.source);
                group.efforts.push(variant);
                out.push(group);
            }
        }
    }
    for group in &mut out {
        group.efforts.sort_by_key(|e| effort_rank(&e.name));
    }
    out
}

fn entry_named(id: String, display_name: String, source: ModelSource) -> ModelEntry {
    ModelEntry {
        display_name,
        family: family(&id),
        tier: catalog_tier(&id),
        id,
        source,
        efforts: Vec::new(),
        default_effort: None,
    }
}

fn family(id: &str) -> String {
    id.split(['-', '/']).next().unwrap_or(id).to_string()
}

pub fn catalog_tier(id: &str) -> ModelTier {
    match id.to_ascii_lowercase().as_str() {
        id if id.contains("opus") || id.contains("5.6") || id.contains("frontier") => {
            ModelTier::Frontier
        }
        id if id.contains("haiku") || id == "auto" || id.contains("fast") => ModelTier::Fast,
        _ => ModelTier::Standard,
    }
}

fn cache_path(root: &Path, identity: &ModelIdentity) -> PathBuf {
    use sha2::{Digest, Sha256};
    root.join("cache").join(format!(
        "models-{}-{}-{:x}.json",
        identity.provider.as_str(),
        safe(&identity.account),
        Sha256::digest(identity.home.to_string_lossy().as_bytes())
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

fn read_refresh_cache(root: &Path, identity: &ModelIdentity) -> Option<ModelCache> {
    let path = cache_path(root, identity);
    let parent = crate::fs_safe::resolved_directory(path.parent()?)?;
    if !crate::fs_safe::regular_owned_file(&path, &parent) {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_CACHE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        return None;
    }
    let cache = serde_json::from_slice::<ModelCache>(&bytes).ok()?;
    (cache.version == 1
        && cache.identity == *identity
        && cache.fetched_at <= unix_now()
        && cache
            .models
            .iter()
            .all(|model| model.source == ModelSource::ListedByCli))
    .then_some(cache)
}

fn write_refresh_cache(
    root: &Path,
    identity: &ModelIdentity,
    models: &[ModelEntry],
) -> std::io::Result<()> {
    if selected_identity(root, identity.provider, &identity.account).as_ref() != Some(identity) {
        return Err(std::io::Error::other(
            "model-list account/home changed while refreshing",
        ));
    }
    let path = cache_path(root, identity);
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("model cache path is unavailable"))?;
    fs::create_dir_all(parent)?;
    let parent = crate::fs_safe::resolved_directory(parent)
        .ok_or_else(|| std::io::Error::other("model cache directory must not be linked"))?;
    match fs::symlink_metadata(&path) {
        Ok(_) if !crate::fs_safe::regular_owned_file(&path, &parent) => {
            return Err(std::io::Error::other(
                "model cache must be a regular owned file",
            ))
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let cache = ModelCache {
        version: 1,
        identity: identity.clone(),
        fetched_at: unix_now(),
        models: models.to_vec(),
    };
    let bytes = serde_json::to_vec_pretty(&cache)?;
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        return Err(std::io::Error::other("model cache exceeds size limit"));
    }
    crate::state_io::write_atomic(&path, &bytes)
}

fn add(out: &mut Vec<ModelEntry>, ids: &mut HashSet<String>, item: ModelEntry) {
    if ids.insert(item.id.clone()) {
        out.push(item);
    }
}
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn absent_provider_data_never_creates_model_choices() {
        let root = tempdir().unwrap();
        for provider in [
            ProviderKind::Codex,
            ProviderKind::Claude,
            ProviderKind::Cursor,
        ] {
            assert!(available_at(root.path(), provider, "missing-fixture-account").is_empty());
            assert!(catalog::list_model_details(provider, root.path()).is_empty());
        }
    }

    #[test]
    fn native_codex_ids_are_preserved_without_a_built_in_catalog() {
        let home = tempdir().unwrap();
        fs::write(
            home.path().join("models_cache.json"),
            r#"{"models":[{"slug":"gpt-5.6-luna","display_name":"gpt-5.6-luna","visibility":"list"},{"slug":"native-current-id","display_name":"native-current-id","visibility":"list"}]}"#,
        )
        .unwrap();
        assert_eq!(
            catalog::list_models(ProviderKind::Codex, home.path()),
            ["gpt-5.6-luna", "native-current-id"]
        );
    }

    #[test]
    fn available_reads_only_selected_home_and_bound_list_cache_without_duplicates() {
        let root = tempdir().unwrap();
        let home = root.path().join("selected-home");
        let other_home = root.path().join("other-home");
        for (account, path) in [("claude-fixture", &home), ("other-fixture", &other_home)] {
            fs::create_dir_all(path.join("cache")).unwrap();
            crate::store::upsert_stored_account(
                root.path(),
                crate::store::StoredAccount {
                    id: account.into(),
                    provider: ProviderKind::Claude,
                    label: "fixture".into(),
                    home_path: Some(path.to_string_lossy().into_owned()),
                    created_at: "0".into(),
                },
            )
            .unwrap();
        }
        let response = serde_json::json!({"type":"control_response","response":{"subtype":"success","request_id":"fixture-models","response":{
            "claude_code_version":crate::capacity::claude_capacity::CLI_VERSION,
            "models":[{"value":"opus","displayName":"Native Opus","resolvedModel":"not-a-selectable-alias","supportsEffort":true,"supportedEffortLevels":["low","high"]},{"value":"haiku","displayName":"Native Haiku","supportsEffort":false,"supportedEffortLevels":["low"]}]
        }}});
        let listed =
            parse_claude_models(response.to_string().as_bytes(), "fixture-models").unwrap();
        assert!(parse_claude_models(response.to_string().as_bytes(), "wrong-request").is_err());
        let mut wrong_version = response.clone();
        wrong_version["response"]["response"]["claude_code_version"] = serde_json::json!("2.1.318");
        assert!(
            parse_claude_models(wrong_version.to_string().as_bytes(), "fixture-models").is_err()
        );
        assert!(!listed
            .iter()
            .any(|model| model.id == "not-a-selectable-alias"));
        assert_eq!(listed[0].effort_names(), ["low", "high"]);
        assert!(listed[1].efforts.is_empty());
        let other = selected_identity(root.path(), ProviderKind::Claude, "other-fixture").unwrap();
        write_refresh_cache(
            root.path(),
            &other,
            &[entry_named(
                "other-account-only".into(),
                "Other".into(),
                ModelSource::ListedByCli,
            )],
        )
        .unwrap();
        let identity =
            selected_identity(root.path(), ProviderKind::Claude, "claude-fixture").unwrap();
        write_refresh_cache(root.path(), &identity, &listed).unwrap();
        let models = available_at(root.path(), ProviderKind::Claude, "claude-fixture");
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["opus", "haiku"]
        );
        assert!(!models.iter().any(|model| model.id == "other-account-only"));
        assert!(available_at(root.path(), ProviderKind::Codex, "claude-fixture").is_empty());
        // Rebinding this registration must not reuse its former home's listed choices.
        crate::store::upsert_stored_account(
            root.path(),
            crate::store::StoredAccount {
                id: "claude-fixture".into(),
                provider: ProviderKind::Claude,
                label: "fixture".into(),
                home_path: Some(other_home.to_string_lossy().into_owned()),
                created_at: "0".into(),
            },
        )
        .unwrap();
        // Neither the old home's list nor another account's list authorizes this binding.
        assert!(available_at(root.path(), ProviderKind::Claude, "claude-fixture").is_empty());
        let rebound_identity =
            selected_identity(root.path(), ProviderKind::Claude, "claude-fixture").unwrap();
        let mut refreshed = response.clone();
        refreshed["response"]["response"]["models"] = serde_json::json!([
            {"value":"other-account-only","displayName":"Current rebound home"}
        ]);
        let rebound_models =
            parse_claude_models(refreshed.to_string().as_bytes(), "fixture-models").unwrap();
        write_refresh_cache(root.path(), &rebound_identity, &rebound_models).unwrap();
        let rebound = available_at(root.path(), ProviderKind::Claude, "claude-fixture");
        assert_eq!(
            rebound
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["other-account-only"]
        );
    }

    #[test]
    fn cursor_models_output_lists_every_plan_model_with_names() {
        let raw = "Available models\n\nauto - Auto (default)\nclaude-opus-5-5-high - Claude Opus 5.5 1M High\nclaude-opus-5-5-low - Claude Opus 5.5 1M Low\ngrok-4.7-low-fast - Grok 4.7  Low Fast\u{200b}\u{200b}\ncursor-grok-4.6-high - Grok 4.6\n\nTip: use --model <id>\n";
        let models = parse_list_output(raw);
        let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            ["auto", "claude-opus-5-5", "grok-4.7-fast", "cursor-grok-4.6"]
        );
        assert_eq!(models[0].display_name, "Auto");
        assert!(models[0].efforts.is_empty());
        assert_eq!(models[1].display_name, "Claude Opus 5.5 1M");
        assert_eq!(models[1].effort_names(), ["low", "high"]);
        assert_eq!(models[1].id_for_effort(Some("low")), "claude-opus-5-5-low");
        assert_eq!(models[1].id_for_effort(None), "claude-opus-5-5");
        assert_eq!(models[2].display_name, "Grok 4.7 Fast");
        assert_eq!(models[2].id_for_effort(Some("low")), "grok-4.7-low-fast");
        assert!(models.iter().all(|m| m.source == ModelSource::ListedByCli));
    }

    #[test]
    fn refresh_without_a_provider_list_command_is_non_blocking() {
        let start = std::time::Instant::now();
        let _ = refresh(ProviderKind::Codex, "missing-fixture-account");
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
