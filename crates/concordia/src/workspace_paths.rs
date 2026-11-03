//! Shared exclusions for automatic workspace discovery and content search.
use std::path::{Path, PathBuf};

/// Classify by path alone: never open credentials to decide whether they are sensitive.
pub fn sensitive(path: &Path) -> bool {
    path.components().any(|c| {
        let name = c.as_os_str().to_string_lossy().to_ascii_lowercase();
        matches!(
            name.as_str(),
            ".git"
                | ".ssh"
                | ".aws"
                | ".azure"
                | ".gnupg"
                | "auth.json"
                | "credentials.json"
                | ".credentials.json"
                | "credentials"
                | "concordia-api-key"
                | ".npmrc"
                | ".netrc"
        ) || name == ".env"
            || name.starts_with(".env.")
            || name.starts_with(".codex")
            || name.starts_with(".claude")
            || name == ".cursor"
            || name.starts_with(".cursor-")
    }) || path.extension().is_some_and(|e| {
        matches!(
            e.to_string_lossy().to_ascii_lowercase().as_str(),
            "pem" | "key" | "p12" | "pfx"
        )
    })
}

pub fn discoverable(path: &Path, directory: bool) -> bool {
    if sensitive(path) {
        return false;
    }
    !directory
        || !path.file_name().is_some_and(|n| {
            matches!(
                n.to_string_lossy().to_ascii_lowercase().as_str(),
                "target"
                    | "node_modules"
                    | "dist"
                    | "build"
                    | ".venv"
                    | "venv"
                    | "appdata"
                    | ".cache"
            )
        })
}

/// Revalidate at use. Canonical aliases/links cannot bypass root containment or exclusions.
pub fn regular_file(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "file is outside the search root")?;
    if sensitive(path) {
        return Err("file is excluded by workspace policy".into());
    }
    let mut component = root.to_path_buf();
    for part in relative.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err("invalid workspace path".into());
        }
        component.push(part.as_os_str());
        let metadata = std::fs::symlink_metadata(&component).map_err(|_| "file is unavailable")?;
        if metadata.file_type().is_symlink() {
            return Err("workspace links are excluded".into());
        }
        if !discoverable(&component, metadata.is_dir()) {
            return Err("file is excluded by workspace policy".into());
        }
    }
    let canonical = path.canonicalize().map_err(|_| "file is unavailable")?;
    if canonical != path || !canonical.starts_with(root) || sensitive(&canonical) {
        return Err("workspace file identity changed".into());
    }
    if !std::fs::symlink_metadata(&canonical)
        .map_err(|_| "file is unavailable")?
        .is_file()
    {
        return Err("workspace entry is not a regular file".into());
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_discovery_excludes_provider_homes_credentials_and_build_outputs() {
        for path in [
            "src/.env",
            ".env.local",
            "nested/.codex-3/sessions/log.jsonl",
            ".cursor/config.json",
            "nested/.aws/config",
            "private/auth.json",
            "cert.KEY",
        ] {
            assert!(sensitive(Path::new(path)), "{path}");
        }
        assert!(!discoverable(Path::new("src/target"), true));
        assert!(discoverable(Path::new("src/target.rs"), false));
        assert!(!sensitive(Path::new("src/config.rs")));
    }
}
