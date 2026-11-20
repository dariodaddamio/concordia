//! OpenCode uses the shared ACP transport. Its credentials and configuration remain provider-owned.

use crate::store::ProviderKind;
use std::path::{Path, PathBuf};

pub fn home_ready(home: &Path) -> bool {
    // Do not inspect auth.json. OpenCode documents it as a credential store.
    home.join("auth.json").is_file()
        || home.join("opencode.json").is_file()
        || home.join("opencode.jsonc").is_file()
}

pub fn inspect_model(home: &Path) -> Option<String> {
    crate::fs_safe::read_text_bounded(&home.join("opencode.json"), 1024 * 1024)
        .or_else(|| crate::fs_safe::read_text_bounded(&home.join("opencode.jsonc"), 1024 * 1024))
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("model")
                .and_then(|model| model.as_str())
                .map(str::to_owned)
        })
}

pub fn located(home: &Path) -> Option<PathBuf> {
    home_ready(home).then(|| home.to_path_buf())
}

pub fn provider() -> ProviderKind {
    ProviderKind::Opencode
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_checks_shape_without_reading_auth_contents() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("auth.json"), "not-json-secret").unwrap();
        assert!(home_ready(root.path()));
        assert_eq!(inspect_model(root.path()), None);
        std::fs::write(
            root.path().join("opencode.json"),
            r#"{"model":"deepseek/deepseek-chat"}"#,
        )
        .unwrap();
        assert_eq!(
            inspect_model(root.path()).as_deref(),
            Some("deepseek/deepseek-chat")
        );
    }
}
