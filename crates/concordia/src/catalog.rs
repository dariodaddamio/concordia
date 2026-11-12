//! Model catalogs from on-disk caches only — never invent model lists.

use crate::store::ProviderKind;
use serde_json::Value;
use std::fs;
use std::path::Path;

fn read_local_cache(home: &Path, relative: &str) -> Option<String> {
    use std::io::Read;
    let root = home.canonicalize().ok()?;
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || crate::workspaces::linked(&metadata) {
        return None;
    }
    if !path.canonicalize().ok()?.starts_with(&root) {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .ok()?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 4 * 1024 * 1024 {
        return None;
    }
    String::from_utf8(bytes).ok()
}

pub fn list_models(provider: ProviderKind, home: &Path) -> Vec<String> {
    list_model_details(provider, home)
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

/// Read provider-owned model ids and their safe display names.  This deliberately selects only
/// model fields from the Codex cache; in particular, identity metadata is never inspected.
pub fn list_model_details(provider: ProviderKind, home: &Path) -> Vec<(String, String)> {
    match provider {
        ProviderKind::Codex => list_codex_models(home),
        ProviderKind::Claude => Vec::new(),
        ProviderKind::Cursor => list_cursor_models(home),
        ProviderKind::Grok => list_grok_models(home),
    }
}

fn list_grok_models(home: &Path) -> Vec<(String, String)> {
    read_local_cache(home, "config.toml")
        .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
        .and_then(|value| value.get("models").and_then(|v| v.get("default")).and_then(|v| v.as_str()).map(|id| vec![(id.to_owned(), id.to_owned())]))
        .unwrap_or_default()
}

fn list_codex_models(home: &Path) -> Vec<(String, String)> {
    let Some(raw) = read_local_cache(home, "models_cache.json") else {
        return Vec::new();
    };
    #[derive(serde::Deserialize)]
    struct NativeModel {
        slug: String,
        display_name: String,
        visibility: String,
    }
    let Ok(Value::Object(mut root)) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    let Some(Value::Array(models)) = root.remove("models") else {
        return Vec::new();
    };
    if models.iter().any(|model| !model.is_object()) {
        return Vec::new();
    }
    let Ok(models) = serde_json::from_value::<Vec<NativeModel>>(Value::Array(models)) else {
        // Only the current provider schema is supported; unknown metadata is unavailable.
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    models
        .into_iter()
        .filter(|model| {
            model.visibility == "list" && !model.slug.is_empty() && !model.display_name.is_empty()
        })
        .filter(|model| seen.insert(model.slug.clone()))
        .map(|model| (model.slug, model.display_name))
        .collect()
}

fn list_cursor_models(home: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(raw) = read_local_cache(home, "cli-config.json") {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            if let Some(id) = v
                .get("model")
                .and_then(|m| m.get("modelId").or_else(|| m.get("displayModelId")))
                .and_then(|x| x.as_str())
            {
                out.push((id.to_string(), id.to_string()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn codex_cache_slugs() {
        let d = tempdir().unwrap();
        fs::write(
            d.path().join("models_cache.json"),
            r#"{"models":[{"slug":"gpt-5.6-sol","display_name":"Sol","visibility":"list"},{"slug":"o3","display_name":"O3","visibility":"list"},{"slug":"hidden-native-model","display_name":"Hidden","visibility":"hide"}]}"#,
        )
        .unwrap();
        let m = list_codex_models(d.path());
        assert!(m.iter().any(|(id, _)| id == "gpt-5.6-sol"));
        assert!(m.iter().any(|(id, _)| id == "o3"));
        assert!(!m.iter().any(|(id, _)| id == "hidden-native-model"));
        for unsupported in [
            r#"[{"slug":"old-root-array","display_name":"Old","visibility":"list"}]"#,
            r#"{"models":[{"id":"id-alias","display_name":"Alias","visibility":"list"}]}"#,
            r#"{"models":["bare-string"]}"#,
            r#"{"models":[{"slug":"no-visibility","display_name":"Unknown"}]}"#,
        ] {
            fs::write(d.path().join("models_cache.json"), unsupported).unwrap();
            assert!(list_codex_models(d.path()).is_empty());
        }
    }

    #[test]
    fn grok_catalog_reads_only_the_configured_default_model() {
        let d = tempdir().unwrap();
        fs::write(d.path().join("config.toml"), "[models]\ndefault = \"grok-build\"\n").unwrap();
        assert_eq!(list_models(ProviderKind::Grok, d.path()), vec!["grok-build"]);
    }
}
