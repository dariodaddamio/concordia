//! Model catalogs from on-disk caches only — never invent model lists.

use crate::store::ProviderKind;
use serde_json::Value;
use std::fs;
use std::path::Path;

pub fn list_models(provider: ProviderKind, home: &Path) -> Vec<String> {
    match provider {
        ProviderKind::Codex => list_codex_models(home),
        ProviderKind::Claude => list_claude_models(home),
        ProviderKind::Cursor => list_cursor_models(home),
    }
}

fn list_codex_models(home: &Path) -> Vec<String> {
    let cache = home.join("models_cache.json");
    let Ok(raw) = fs::read_to_string(&cache) else {
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
            if !id.is_empty() && !out.iter().any(|x| x == id) {
                out.push(id.to_string());
            }
        } else if let Some(s) = item.as_str() {
            if !s.is_empty() && !out.iter().any(|x| x == s) {
                out.push(s.to_string());
            }
        }
    }
    out
}

fn list_claude_models(home: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for rel in [
        "cache/model-catalog.json",
        "cache/additionalModelOptionsCache.json",
    ] {
        let p = home.join(rel);
        let Ok(raw) = fs::read_to_string(&p) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        collect_model_ids(&v, &mut out);
    }
    out
}

fn list_cursor_models(home: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(raw) = fs::read_to_string(home.join("cli-config.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            if let Some(id) = v
                .get("model")
                .and_then(|m| m.get("modelId").or_else(|| m.get("displayModelId")))
                .and_then(|x| x.as_str())
            {
                out.push(id.to_string());
            }
            if out.is_empty() {
                out.push("auto".into());
            }
        }
    }
    if out.is_empty() {
        out.push("auto".into());
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
        assert!(m.contains(&"gpt-5.6-sol".into()));
        assert!(m.contains(&"o3".into()));
    }
}
