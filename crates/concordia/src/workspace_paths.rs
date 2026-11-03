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
        use crate::workspaces::{self, Access, Admission, Declaration, Phase, Request};
        let fixture = tempfile::tempdir().unwrap();
        let workspace = fixture.path().join("project");
        let state = fixture.path().join("state");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        let file = workspace.join("src/a.txt");
        std::fs::write(&file, "before").unwrap();
        let conversation = uuid::Uuid::new_v4().to_string();
        let run = uuid::Uuid::new_v4().to_string();
        let attempt = uuid::Uuid::new_v4().to_string();
        let writer = Request::shared(
            &workspace,
            &[Declaration {
                path: "src".into(),
                access: Access::Write,
            }],
        )
        .unwrap();
        assert_eq!(
            workspaces::reserve(&state, &writer, &run, &conversation, &attempt).unwrap(),
            Admission::Reserved
        );
        let reader = Request::shared(
            &workspace,
            &[Declaration {
                path: "src/a.txt".into(),
                access: Access::Read,
            }],
        )
        .unwrap();
        let other_run = uuid::Uuid::new_v4().to_string();
        let other_attempt = uuid::Uuid::new_v4().to_string();
        assert!(
            matches!(workspaces::reserve(&state, &reader, &other_run, &conversation, &other_attempt).unwrap(), Admission::Conflict { run: owner, .. } if owner == run)
        );
        let disjoint = Request::shared(
            &workspace,
            &[Declaration {
                path: "other.txt".into(),
                access: Access::Write,
            }],
        )
        .unwrap();
        let disjoint_run = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            workspaces::reserve(
                &state,
                &disjoint,
                &disjoint_run,
                &conversation,
                &other_attempt
            )
            .unwrap(),
            Admission::Reserved
        );
        workspaces::finish(&state, &run, &attempt, false).unwrap();
        assert_eq!(
            workspaces::load(&state).unwrap().reservations[0].phase,
            Phase::ReconcileRequired
        );
        assert!(
            workspaces::with_write_guard(&state, &file, || crate::file_document::save(
                &file, "before", "after"
            ))
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "before");
        workspaces::acknowledge_stopped(&state, &run, &conversation).unwrap();
        assert_eq!(
            workspaces::reserve(&state, &reader, &other_run, &conversation, &other_attempt)
                .unwrap(),
            Admission::Reserved
        );
        let read_run = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            workspaces::reserve(&state, &reader, &read_run, &conversation, &attempt).unwrap(),
            Admission::Reserved
        );
        workspaces::finish(&state, &other_run, &other_attempt, true).unwrap();
        workspaces::finish(&state, &read_run, &attempt, true).unwrap();
        workspaces::with_write_guard(&state, &file, || {
            crate::file_document::save(&file, "before", "after")
        })
        .unwrap();
        assert!(
            reader.revalidate().is_err(),
            "atomic replacement changes the declared read file identity"
        );
        for path in [
            "../outside",
            ".git/index",
            "auth.json",
            "src/a.txt:stream",
            "NUL",
            "src/trailing.",
            "missing/child.txt",
        ] {
            assert!(
                Request::shared(
                    &workspace,
                    &[Declaration {
                        path: path.into(),
                        access: Access::Write
                    }]
                )
                .is_err(),
                "{path}"
            );
        }
        let (declarations, task) = workspaces::parse_declarations(
            "write:\"src/a b.txt\" read:src -- Preserve  exact\nTASK",
            true,
        )
        .unwrap();
        assert_eq!(declarations[0].path, Path::new("src/a b.txt"));
        assert_eq!(task, "Preserve  exact\nTASK");
        let outside = fixture.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("a.txt"), "synthetic outside-workspace data").unwrap();
        let alias = workspace.join("escaped");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let result = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:CONCORDIA_RELIABILITY_JUNCTION_LINK -Target $env:CONCORDIA_RELIABILITY_JUNCTION_TARGET | Out-Null"])
                .env("CONCORDIA_RELIABILITY_JUNCTION_LINK", &alias)
                .env("CONCORDIA_RELIABILITY_JUNCTION_TARGET", &outside)
                .creation_flags(0x08000000).output().unwrap();
            assert!(
                result.status.success(),
                "junction fixture failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &alias).unwrap();
        #[cfg(any(windows, unix))]
        assert!(Request::shared(
            &workspace,
            &[Declaration {
                path: "escaped/a.txt".into(),
                access: Access::Write
            }]
        )
        .is_err());
        #[cfg(windows)]
        {
            let declare = |path: &str| {
                Request::shared(
                    &workspace,
                    &[Declaration {
                        path: path.into(),
                        access: Access::Write,
                    }],
                )
                .unwrap()
            };
            let missing = declare("ÜNEW.txt");
            let alias = declare("ünew.txt");
            let case_run = uuid::Uuid::new_v4().to_string();
            assert_eq!(
                workspaces::reserve(&state, &missing, &case_run, &conversation, &attempt).unwrap(),
                Admission::Reserved
            );
            assert!(matches!(
                workspaces::reserve(
                    &state,
                    &alias,
                    &uuid::Uuid::new_v4().to_string(),
                    &conversation,
                    &attempt
                )
                .unwrap(),
                Admission::Conflict { .. }
            ));
            workspaces::finish(&state, &case_run, &attempt, true).unwrap();
        }
        let hardlink = workspace.join("hardlink.txt");
        std::fs::hard_link(&file, &hardlink).unwrap();
        let link_request = Request::shared(
            &workspace,
            &[Declaration {
                path: "hardlink.txt".into(),
                access: Access::Write,
            }],
        )
        .unwrap();
        let fresh_reader = Request::shared(
            &workspace,
            &[Declaration {
                path: "src/a.txt".into(),
                access: Access::Read,
            }],
        )
        .unwrap();
        let link_run = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            workspaces::reserve(&state, &link_request, &link_run, &conversation, &attempt).unwrap(),
            Admission::Reserved
        );
        assert!(matches!(
            workspaces::reserve(
                &state,
                &fresh_reader,
                &uuid::Uuid::new_v4().to_string(),
                &conversation,
                &attempt
            )
            .unwrap(),
            Admission::Conflict { .. }
        ));
        workspaces::finish(&state, &link_run, &attempt, true).unwrap();
        let corrupted = state.join("workspace-registry.json");
        std::fs::write(&corrupted, b"{invalid").unwrap();
        assert!(workspaces::reserve(&state, &writer, &run, &conversation, &attempt).is_err());
        assert_eq!(std::fs::read(corrupted).unwrap(), b"{invalid");
    }
}
