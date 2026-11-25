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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeModel {
    pub id: String,
    pub display_name: String,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

impl NativeModel {
    fn named(id: String, display_name: String) -> Self {
        NativeModel {
            id,
            display_name,
            efforts: Vec::new(),
            default_effort: None,
        }
    }
}

pub fn list_models(provider: ProviderKind, home: &Path) -> Vec<String> {
    list_model_details(provider, home)
        .into_iter()
        .map(|model| model.id)
        .collect()
}

pub fn list_model_details(provider: ProviderKind, home: &Path) -> Vec<NativeModel> {
    match provider {
        ProviderKind::Codex => list_codex_models(home),
        ProviderKind::Claude => Vec::new(),
        ProviderKind::Cursor => list_cursor_models(home),
        ProviderKind::Grok => list_grok_models(home),
        ProviderKind::Opencode => {
            let _ = home;
            Vec::new()
        }
    }
}

fn list_grok_models(home: &Path) -> Vec<NativeModel> {
    let _ = home;
    Vec::new()
}

fn list_codex_models(home: &Path) -> Vec<NativeModel> {
    let Some(raw) = read_local_cache(home, "models_cache.json") else {
        return Vec::new();
    };
    #[derive(serde::Deserialize)]
    struct CodexModel {
        slug: String,
        display_name: String,
        visibility: String,
        #[serde(default)]
        supported_reasoning_levels: Vec<CodexEffort>,
        #[serde(default)]
        default_reasoning_level: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct CodexEffort {
        effort: String,
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
    let Ok(models) = serde_json::from_value::<Vec<CodexModel>>(Value::Array(models)) else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    models
        .into_iter()
        .filter(|model| {
            model.visibility == "list" && !model.slug.is_empty() && !model.display_name.is_empty()
        })
        .filter(|model| seen.insert(model.slug.clone()))
        .map(|model| {
            let efforts: Vec<String> = model
                .supported_reasoning_levels
                .into_iter()
                .map(|level| level.effort)
                .filter(|effort| valid_effort(effort))
                .collect();
            NativeModel {
                default_effort: model
                    .default_reasoning_level
                    .filter(|effort| efforts.contains(effort)),
                efforts,
                id: model.slug,
                display_name: model.display_name,
            }
        })
        .collect()
}

pub use crate::text_safe::valid_effort;

fn list_cursor_models(home: &Path) -> Vec<NativeModel> {
    let mut out = Vec::new();
    if let Some(raw) = read_local_cache(home, "cli-config.json") {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            if let Some(id) = v
                .get("model")
                .and_then(|m| m.get("modelId").or_else(|| m.get("displayModelId")))
                .and_then(|x| x.as_str())
            {
                out.push(NativeModel::named(id.to_string(), id.to_string()));
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
        assert!(m.iter().any(|model| model.id == "gpt-5.6-sol"));
        assert!(m.iter().any(|model| model.id == "o3"));
        assert!(!m.iter().any(|model| model.id == "hidden-native-model"));
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
    fn codex_models_carry_their_listed_efforts_and_default() {
        let d = tempdir().unwrap();
        fs::write(
            d.path().join("models_cache.json"),
            r#"{"models":[{"slug":"model-a","display_name":"Model A","visibility":"list","default_reasoning_level":"medium","supported_reasoning_levels":[{"effort":"low","description":"x"},{"effort":"medium","description":"y"},{"effort":"Bad Value","description":"z"}]},{"slug":"model-b","display_name":"Model B","visibility":"list","default_reasoning_level":"ultra"}]}"#,
        )
        .unwrap();
        let m = list_codex_models(d.path());
        assert_eq!(m[0].efforts, ["low", "medium"]);
        assert_eq!(m[0].default_effort.as_deref(), Some("medium"));
        assert!(m[1].efforts.is_empty());
        assert_eq!(m[1].default_effort, None);
    }

    #[test]
    fn grok_catalog_does_not_treat_a_configured_default_as_a_model_list() {
        let d = tempdir().unwrap();
        fs::write(
            d.path().join("config.toml"),
            "[models]\ndefault = \"grok-build\"\n",
        )
        .unwrap();
        assert!(list_models(ProviderKind::Grok, d.path()).is_empty());
    }
}
