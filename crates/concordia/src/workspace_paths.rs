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
        // Exercise the supported Git CLI against only this synthetic committed repository.
        let repository = fixture.path().join("repository");
        let managed_state = fixture.path().join("managed-state");
        let empty_hooks = fixture.path().join("empty-hooks");
        std::fs::create_dir_all(repository.join("src")).unwrap();
        std::fs::create_dir(&empty_hooks).unwrap();
        std::fs::write(repository.join("src/input.txt"), "committed fixture\n").unwrap();
        std::fs::write(repository.join(".gitattributes"), "*.txt filter=fixture\n").unwrap();
        let git = |cwd: &Path, args: &[&str]| {
            let mut command = std::process::Command::new(which::which("git").unwrap());
            command
                .current_dir(cwd)
                .args([
                    "-c",
                    &format!("core.hooksPath={}", empty_hooks.display()),
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "-c",
                    "commit.gpgSign=false",
                ])
                .args(args);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "synthetic Git command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap()
        };
        git(&repository, &["init", "-b", "main"]);
        git(&repository, &["config", "core.autocrlf", "false"]);
        git(
            &repository,
            &["add", "--", "src/input.txt", ".gitattributes"],
        );
        git(&repository, &["commit", "-m", "synthetic base"]);
        git(
            &repository,
            &[
                "config",
                "filter.fixture.process",
                "nonexistent-concordia-fixture-filter-must-not-run",
            ],
        );
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let record =
            crate::managed_workspaces::create(&managed_state, &repository, "HEAD", &cancel)
                .unwrap();
        assert_eq!(record.phase, crate::managed_workspaces::Phase::Ready);
        assert_eq!(
            std::fs::read_to_string(record.execution.join("src/input.txt")).unwrap(),
            "committed fixture\n"
        );
        assert!(
            git(&repository, &["config", "--get", "filter.fixture.process"])
                .contains("must-not-run"),
            "creation never rewrites source configuration"
        );
        let managed_request =
            Request::managed(&managed_state, &repository, &record.id, &[]).unwrap();
        assert_ne!(
            managed_request.binding.owner,
            managed_request.binding.execution
        );
        let managed_run = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            workspaces::reserve(
                &managed_state,
                &managed_request,
                &managed_run,
                &conversation,
                &attempt
            )
            .unwrap(),
            Admission::Reserved
        );
        assert!(
            Request::shared(&repository, &[]).unwrap().binding.execution
                != managed_request.binding.execution
        );
        workspaces::finish(&managed_state, &managed_run, &attempt, true).unwrap();
        let mut registry = crate::managed_workspaces::load(&managed_state).unwrap();
        registry.records[0].phase = crate::managed_workspaces::Phase::Creating;
        registry.records[0].execution_identity = None;
        std::fs::write(
            managed_state.join("managed-workspaces.json"),
            serde_json::to_vec(&registry).unwrap(),
        )
        .unwrap();
        assert!(crate::managed_workspaces::bound(&managed_state, &repository, &record.id).is_err());
        let reconciled =
            crate::managed_workspaces::reconcile(&managed_state, &repository, &record.id, &cancel)
                .unwrap();
        assert_eq!(reconciled.phase, crate::managed_workspaces::Phase::Ready);
        assert!(
            workspaces::reserve(
                &managed_state,
                &managed_request,
                &uuid::Uuid::new_v4().to_string(),
                &conversation,
                &attempt
            )
            .is_err(),
            "changed registry revision invalidates approval"
        );
        assert_eq!(
            git(&repository, &["worktree", "list", "--porcelain"])
                .lines()
                .filter(|line| line.starts_with("worktree "))
                .count(),
            2,
            "reconciliation never creates another worktree"
        );
        std::fs::write(repository.join("unsaved.txt"), "retain this local edit").unwrap();
        assert!(
            crate::managed_workspaces::create(&managed_state, &repository, "HEAD", &cancel)
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(repository.join("unsaved.txt")).unwrap(),
            "retain this local edit"
        );
        cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(
            crate::managed_workspaces::create(&managed_state, &repository, "HEAD", &cancel)
                .is_err()
        );
        assert_eq!(
            crate::managed_workspaces::load(&managed_state)
                .unwrap()
                .records
                .len(),
            1
        );
    }
}
