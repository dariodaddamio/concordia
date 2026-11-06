//! Shared exclusions for automatic workspace discovery and content search.
use std::path::{Path, PathBuf};

/// Classify by path alone: never open credentials to decide whether they are sensitive.
pub fn sensitive(path: &Path) -> bool {
    let normalized = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if normalized.split('/').any(|part| part == ".docker") && normalized.ends_with("/config.json")
        || normalized.split('/').any(|part| part == ".config")
            && normalized.contains("/.config/gh/")
    {
        return true;
    }
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
                | ".pgpass"
                | ".kube"
                | ".git-credentials"
                | ".pypirc"
        ) || name == ".env"
            || name.starts_with(".env.")
            || name.starts_with(".codex")
            || name.starts_with(".claude")
            || name == ".cursor"
            || name.starts_with(".cursor-")
            || name.contains("secret")
            // SSH public keys are commonly stored beside their private key as
            // `id_*.pub`; only the private-key name belongs in this exclusion.
            || (name.starts_with("id_") && !name.ends_with(".pub"))
            || (name.starts_with("service-account") && name.ends_with(".json"))
    }) || path.extension().is_some_and(|e| {
        matches!(
            e.to_string_lossy().to_ascii_lowercase().as_str(),
            "pem" | "key" | "p12" | "pfx" | "tfstate" | "jks" | "ppk" | "kdbx" | "p8" | "keystore"
        )
    })
}

pub fn contained_file(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|_| "workspace is unavailable")?;
    let canonical = path.canonicalize().map_err(|_| "file is unavailable")?;
    if !canonical.starts_with(&root) {
        return Err("file is outside the workspace (outside workspace)".into());
    }
    if !canonical.is_file() {
        return Err("reference does not name a file".into());
    }
    Ok(canonical)
}

pub fn reserved_device_name(path: &Path) -> bool {
    let Some(name) = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
    else {
        return false;
    };
    let stem = name.split('.').next().unwrap_or_default();
    matches!(stem, "con" | "prn" | "aux" | "nul")
        || ((stem.starts_with("com") || stem.starts_with("lpt"))
            && stem[3..].parse::<u8>().is_ok_and(|n| (1..=9).contains(&n)))
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
    fn sensitive_matches_each_new_pattern() {
        for path in [
            "id_rsa",
            "id_ed25519",
            ".pgpass",
            ".kube/config",
            ".docker/config.json",
            ".git-credentials",
            ".pypirc",
            "state.tfstate",
            "x.jks",
            "x.ppk",
            "x.kdbx",
            "x.p8",
            "x.keystore",
            "service-account-prod.json",
            "my-secret-notes.txt",
        ] {
            assert!(sensitive(Path::new(path)), "{path}");
        }
        assert!(!sensitive(Path::new("id_rsa.pub")));
        assert!(!sensitive(Path::new("id_ed25519.pub")));
    }

    #[test]
    fn reserved_device_names_rejected_without_fs_calls() {
        for name in ["CON", "nul.txt", "Com1", "LPT9.log", "normal.txt"] {
            assert_eq!(reserved_device_name(Path::new(name)), name != "normal.txt");
        }
    }

    #[test]
    fn symlink_out_of_workspace_is_not_opened_or_saved() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        let outside = temp.path().join("outside.txt");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(&outside, "outside").unwrap();
        let link = root.join("notes.md");
        #[cfg(windows)]
        {
            let status = crate::process_ui::command("cmd")
                .args([
                    "/c",
                    "mklink",
                    "/J",
                    link.to_str().unwrap(),
                    outside.parent().unwrap().to_str().unwrap(),
                ])
                .status()
                .unwrap();
            if !status.success() {
                eprintln!("skipped: junction creation requires privilege");
                return;
            }
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(contained_file(&root, &link).is_err());
        assert!(crate::file_document::save_contained(&root, &link, "outside", "changed").is_err());
    }

    #[test]
    fn save_rechecks_containment() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        let outside = temp.path().join("file.txt");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(&outside, "before").unwrap();
        assert!(crate::file_document::save_contained(&root, &outside, "before", "after").is_err());
        assert_eq!(std::fs::read_to_string(outside).unwrap(), "before");
    }

    #[test]
    fn explicitly_typed_out_of_tree_path_still_opens() {
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("typed.txt");
        std::fs::write(&outside, "typed").unwrap();
        let candidate = crate::file_links::explicit_reference(outside.to_str().unwrap()).unwrap();
        assert!(crate::file_links::resolve_explicit(
            &candidate,
            temp.path().join("missing").as_path()
        )
        .is_ok());
    }
    #[test]
    fn automatic_discovery_excludes_provider_homes_credentials_and_build_outputs() {
        let _environment = crate::config::env_guard();
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
            let result = crate::process_ui::command("powershell.exe")
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
            let mut command = crate::process_ui::command(which::which("git").unwrap());
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
        use crate::managed_workspaces::lifecycle::{self, Action, State};
        let current_scope = Request::managed(&managed_state, &repository, &record.id, &[]).unwrap();
        let live_run = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            workspaces::reserve(
                &managed_state,
                &current_scope,
                &live_run,
                &conversation,
                &attempt
            )
            .unwrap(),
            Admission::Reserved
        );
        let active = lifecycle::preview(
            &managed_state,
            &repository,
            &record.id,
            Action::Retire,
            &cancel,
        )
        .unwrap();
        assert_eq!(active.state, State::Active);
        assert!(lifecycle::execute(
            &managed_state,
            &repository,
            &record.id,
            Action::Retire,
            &active.receipt,
            &cancel
        )
        .is_err());
        workspaces::finish(&managed_state, &live_run, &attempt, true).unwrap();
        let stale = lifecycle::preview(
            &managed_state,
            &repository,
            &record.id,
            Action::Retire,
            &cancel,
        )
        .unwrap();
        std::fs::write(record.execution.join("unsaved.md"), "preserve this edit").unwrap();
        assert!(lifecycle::execute(
            &managed_state,
            &repository,
            &record.id,
            Action::Retire,
            &stale.receipt,
            &cancel
        )
        .is_err());
        assert_eq!(
            std::fs::read_to_string(record.execution.join("unsaved.md")).unwrap(),
            "preserve this edit"
        );
        std::fs::remove_file(record.execution.join("unsaved.md")).unwrap();
        // A crashed native lifecycle action retains its owner scope until explicit stopped-process acknowledgement.
        let uncertain_run = uuid::Uuid::new_v4().to_string();
        let owner_scope = Request::shared(&repository, &[]).unwrap();
        assert_eq!(
            workspaces::reserve(
                &managed_state,
                &owner_scope,
                &uncertain_run,
                &conversation,
                &attempt
            )
            .unwrap(),
            Admission::Reserved
        );
        workspaces::finish(&managed_state, &uncertain_run, &attempt, false).unwrap();
        let mut registry = crate::managed_workspaces::load(&managed_state).unwrap();
        registry.records[0].phase = crate::managed_workspaces::Phase::Retained;
        registry.records[0].operation = Some(lifecycle::Operation {
            action: Action::Integrate,
            owner_head: record.base.clone(),
            worker_head: record.base.clone(),
            expected_tree: None,
            receipt: "ab".repeat(32),
            finished: false,
            runs: vec![uncertain_run.clone()],
            conversation: conversation.clone(),
            attempt: attempt.clone(),
        });
        std::fs::write(
            managed_state.join("managed-workspaces.json"),
            serde_json::to_vec(&registry).unwrap(),
        )
        .unwrap();
        assert!(crate::managed_workspaces::reconcile(
            &managed_state,
            &repository,
            &record.id,
            &cancel
        )
        .is_err());
        lifecycle::stopped(&managed_state, &repository, &record.id).unwrap();
        crate::managed_workspaces::reconcile(&managed_state, &repository, &record.id, &cancel)
            .unwrap();
        assert_eq!(git(&repository, &["rev-parse", "HEAD"]).trim(), record.base);
        let current_scope = Request::managed(&managed_state, &repository, &record.id, &[]).unwrap();
        let mut delegates = crate::delegate::Delegates::default();
        delegates
            .attach_recovery(
                &managed_state.join("pane-recovery/fixture/pane/worker-runs"),
                &repository,
                &managed_state,
            )
            .unwrap();
        let queued = delegates.push_test_job(
            "fixture",
            crate::store::ProviderKind::Codex,
            crate::delegate::WorkerState::Queued,
            "queued metadata only",
            0,
        );
        delegates.jobs.last_mut().unwrap().workspace = Some(current_scope.clone());
        delegates.mark_collected(&[]);
        assert!(delegates.recovery_error().is_none());
        let pending = lifecycle::preview(
            &managed_state,
            &repository,
            &record.id,
            Action::Retire,
            &cancel,
        )
        .unwrap();
        assert!(!pending.references.is_empty());
        assert!(pending
            .blockers
            .iter()
            .any(|reason| reason.contains("Queued")));
        delegates.cancel_all(std::time::Instant::now());
        delegates.mark_collected(&[queued]);
        drop(delegates);
        git(&repository, &["config", "user.name", "Fixture"]);
        git(
            &repository,
            &["config", "user.email", "fixture@example.invalid"],
        );
        git(&repository, &["config", "commit.gpgSign", "false"]);
        std::fs::write(repository.join(".git/info/exclude"), "build/\n").unwrap();
        for directory in [repository.join("build"), record.execution.join("build")] {
            std::fs::create_dir(&directory).unwrap();
            std::fs::write(directory.join("retain.log"), "ignored local artifact").unwrap();
        }
        std::fs::write(record.execution.join("worker.md"), "worker result\n").unwrap();
        git(&record.execution, &["add", "--", "worker.md"]);
        git(
            &record.execution,
            &["commit", "-m", "synthetic worker result"],
        );
        let unique = lifecycle::preview(
            &managed_state,
            &repository,
            &record.id,
            Action::Retire,
            &cancel,
        )
        .unwrap();
        assert_eq!(unique.unique_commits, 1);
        assert!(!unique.blockers.is_empty());
        let before_owner_change = lifecycle::preview(
            &managed_state,
            &repository,
            &record.id,
            Action::Integrate,
            &cancel,
        )
        .unwrap();
        std::fs::write(repository.join("owner.md"), "independent owner result\n").unwrap();
        git(&repository, &["add", "--", "owner.md"]);
        git(&repository, &["commit", "-m", "synthetic owner result"]);
        assert!(lifecycle::execute(
            &managed_state,
            &repository,
            &record.id,
            Action::Integrate,
            &before_owner_change.receipt,
            &cancel
        )
        .is_err());
        let pick = lifecycle::preview(
            &managed_state,
            &repository,
            &record.id,
            Action::CherryPick,
            &cancel,
        )
        .unwrap();
        assert!(pick.blockers.is_empty(), "{pick:?}");
        lifecycle::execute(
            &managed_state,
            &repository,
            &record.id,
            Action::CherryPick,
            &pick.receipt,
            &cancel,
        )
        .unwrap();
        assert!(repository.join("worker.md").exists());
        assert!(repository.join("owner.md").exists());
        let merge = lifecycle::preview(
            &managed_state,
            &repository,
            &record.id,
            Action::Integrate,
            &cancel,
        )
        .unwrap();
        assert!(merge.blockers.is_empty(), "{merge:?}");
        lifecycle::execute(
            &managed_state,
            &repository,
            &record.id,
            Action::Integrate,
            &merge.receipt,
            &cancel,
        )
        .unwrap();
        let ignored = lifecycle::preview(
            &managed_state,
            &repository,
            &record.id,
            Action::Retire,
            &cancel,
        )
        .unwrap();
        assert!(
            !ignored.blockers.is_empty(),
            "ignored worker artifacts must prevent removal"
        );
        assert!(lifecycle::execute(
            &managed_state,
            &repository,
            &record.id,
            Action::Retire,
            &ignored.receipt,
            &cancel
        )
        .is_err());
        std::fs::remove_file(record.execution.join("build/retain.log")).unwrap();
        std::fs::remove_dir(record.execution.join("build")).unwrap();
        let retire = lifecycle::preview(
            &managed_state,
            &repository,
            &record.id,
            Action::Retire,
            &cancel,
        )
        .unwrap();
        assert_eq!(retire.state, State::Retirable);
        lifecycle::execute(
            &managed_state,
            &repository,
            &record.id,
            Action::Retire,
            &retire.receipt,
            &cancel,
        )
        .unwrap();
        assert!(!record.execution.exists());
        assert!(
            repository.join("build/retain.log").exists(),
            "owner's ignored artifacts stay in place"
        );
        assert!(
            !git(&repository, &["branch", "--list", &record.branch]).is_empty(),
            "retirement must retain the branch"
        );
        let branch = lifecycle::preview(
            &managed_state,
            &repository,
            &record.id,
            Action::DeleteBranch,
            &cancel,
        )
        .unwrap();
        lifecycle::execute(
            &managed_state,
            &repository,
            &record.id,
            Action::DeleteBranch,
            &branch.receipt,
            &cancel,
        )
        .unwrap();
        assert!(git(&repository, &["branch", "--list", &record.branch]).is_empty());
        assert!(
            workspaces::load(&managed_state)
                .unwrap()
                .reservations
                .iter()
                .all(|claim| claim.phase == workspaces::Phase::Released),
            "confirmed lifecycle exits must release durable claims"
        );
        assert_eq!(
            crate::managed_workspaces::load(&managed_state)
                .unwrap()
                .records[0]
                .phase,
            crate::managed_workspaces::Phase::Retired
        );
        let conflict =
            crate::managed_workspaces::create(&managed_state, &repository, "HEAD", &cancel)
                .unwrap();
        std::fs::write(
            conflict.execution.join("worker.md"),
            "worker conflicting edit\n",
        )
        .unwrap();
        git(&conflict.execution, &["add", "--", "worker.md"]);
        git(
            &conflict.execution,
            &["commit", "-m", "synthetic conflicting worker"],
        );
        std::fs::write(repository.join("worker.md"), "owner conflicting edit\n").unwrap();
        git(&repository, &["add", "--", "worker.md"]);
        git(
            &repository,
            &["commit", "-m", "synthetic conflicting owner"],
        );
        let conflict = lifecycle::preview(
            &managed_state,
            &repository,
            &conflict.id,
            Action::Integrate,
            &cancel,
        )
        .unwrap();
        assert!(!conflict.conflicts.is_empty());
        assert!(lifecycle::execute(
            &managed_state,
            &repository,
            &conflict.record.id,
            Action::Integrate,
            &conflict.receipt,
            &cancel
        )
        .is_err());
        assert_eq!(
            std::fs::read_to_string(repository.join("worker.md")).unwrap(),
            "owner conflicting edit\n"
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
            2
        );
    }
}
