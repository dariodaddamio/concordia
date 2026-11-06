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
    if !metadata.is_file() || metadata.file_type().is_symlink() {
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
        ProviderKind::Claude => list_claude_models(home),
        ProviderKind::Cursor => list_cursor_models(home),
    }
}

fn list_codex_models(home: &Path) -> Vec<(String, String)> {
    let Some(raw) = read_local_cache(home, "models_cache.json") else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let arr = v
        .as_array()
        .cloned()
        .or_else(|| v.get("models").and_then(|m| m.as_array()).cloned())
        .unwrap_or_default();
    for item in arr {
        if let Some(id) = item
            .get("slug")
            .or_else(|| item.get("id"))
            .or_else(|| item.get("model"))
            .and_then(|x| x.as_str())
        {
            if !id.is_empty() && !out.iter().any(|x: &(String, String)| x.0 == id) {
                let name = item
                    .get("display_name")
                    .and_then(|x| x.as_str())
                    .filter(|name| !name.is_empty())
                    .unwrap_or(id)
                    .to_string();
                out.push((id.to_string(), name));
            }
        } else if let Some(s) = item.as_str() {
            if !s.is_empty() && !out.iter().any(|x: &(String, String)| x.0 == s) {
                out.push((s.to_string(), s.to_string()));
            }
        }
    }
    out
}

fn list_claude_models(home: &Path) -> Vec<(String, String)> {
    let mut ids = Vec::new();
    for rel in [
        "cache/model-catalog.json",
        "cache/additionalModelOptionsCache.json",
    ] {
        let Some(raw) = read_local_cache(home, rel) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        collect_model_ids(&v, &mut ids);
    }
    ids.into_iter().map(|id| (id.clone(), id)).collect()
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
            if out.is_empty() {
                out.push(("auto".into(), "auto".into()));
            }
        }
    }
    if out.is_empty() {
        out.push(("auto".into(), "auto".into()));
    }
    out
}

fn collect_model_ids(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Array(arr) => {
            for item in arr {
                collect_model_ids(item, out);
            }
        }
        Value::Object(map) => {
            if let Some(id) = map
                .get("id")
                .or_else(|| map.get("model"))
                .or_else(|| map.get("value"))
                .and_then(|x| x.as_str())
            {
                if !id.is_empty() && !out.iter().any(|x| x == id) {
                    out.push(id.to_string());
                }
            }
            for (k, child) in map {
                if k == "models" || k == "options" || k == "items" {
                    collect_model_ids(child, out);
                }
            }
        }
        _ => {}
    }
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
            r#"[{"slug":"gpt-5.6-sol"},{"slug":"o3"}]"#,
        )
        .unwrap();
        let m = list_codex_models(d.path());
        assert!(m.iter().any(|(id, _)| id == "gpt-5.6-sol"));
        assert!(m.iter().any(|(id, _)| id == "o3"));
    }
}
