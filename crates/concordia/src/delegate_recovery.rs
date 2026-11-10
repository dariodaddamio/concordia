//! Durable worker ownership and bounded failover. Recovery never starts a provider turn.
use super::*;
use std::fs::{File, OpenOptions};
use std::io::Read;

const LEDGER_BYTES: u64 = 8 * 1024 * 1024;

/// Bounded Concordia-owned metadata only; never scans native session histories.
pub(crate) fn managed_references(
    root: &Path,
    id: &str,
    retirement: bool,
) -> Result<(Vec<String>, Vec<String>), String> {
    use sha2::{Digest, Sha256};
    let recovery = root.join("pane-recovery");
    if !recovery.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut directories = vec![recovery];
    let mut files = Vec::new();
    let mut inspected_entries = 0usize;
    for _ in 0..2 {
        let mut next = Vec::new();
        for directory in directories {
            if crate::workspaces::linked(
                &std::fs::symlink_metadata(&directory).map_err(|error| error.to_string())?,
            ) {
                return Err("linked worker recovery directory; references unknown".into());
            }
            for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
                inspected_entries += 1;
                if inspected_entries > 4096 {
                    return Err("worker reference entry budget exceeded; references unknown, retain the workspace".into());
                }
                let entry = entry.map_err(|error| error.to_string())?;
                let metadata = entry.metadata().map_err(|error| error.to_string())?;
                if metadata.is_dir() {
                    if next.len() >= 256 {
                        return Err("worker reference directory budget exceeded".into());
                    }
                    next.push(entry.path());
                }
            }
        }
        directories = next;
    }
    for pane in directories {
        if crate::workspaces::linked(
            &std::fs::symlink_metadata(&pane).map_err(|error| error.to_string())?,
        ) {
            return Err("linked recovery pane; references unknown".into());
        }
        let worker = pane.join("worker-runs");
        if worker.exists()
            && crate::workspaces::linked(
                &std::fs::symlink_metadata(&worker).map_err(|error| error.to_string())?,
            )
        {
            return Err("linked worker ledger directory; references unknown".into());
        }
        let path = worker.join("workers.json");
        if path.exists() {
            files.push(path);
        }
    }
    files.sort();
    let mut total = 0;
    let mut references = Vec::new();
    let mut blockers = Vec::new();
    for file in files {
        let metadata = std::fs::symlink_metadata(&file).map_err(|error| error.to_string())?;
        if !metadata.is_file() || crate::workspaces::linked(&metadata) {
            return Err("worker ledger is not a regular file; references unknown".into());
        }
        let mut bytes = Vec::new();
        File::open(&file)
            .map_err(|error| error.to_string())?
            .take(LEDGER_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        total += bytes.len();
        if bytes.len() as u64 > LEDGER_BYTES || total > 64 * 1024 * 1024 {
            return Err("worker reference byte budget exceeded".into());
        }
        let ledger: Ledger = serde_json::from_slice(&bytes)
            .map_err(|_| "worker ledger unavailable; references unknown")?;
        if !(1..=9).contains(&ledger.version) {
            return Err("unknown worker ledger version; references retained".into());
        }
        let hash = format!("{:x}", Sha256::digest(&bytes));
        if let Some(graph) = &ledger.schedule {
            graph.validate()?;
        }
        for job in ledger.jobs {
            if job
                .workspace
                .as_ref()
                .and_then(|scope| scope.binding.managed.as_ref())
                .is_some_and(|managed| managed.id == id)
            {
                if let Some(graph) = &ledger.schedule {
                    for dependent in graph.tasks().iter().filter(|task| {
                        task.prerequisites()
                            .iter()
                            .any(|prerequisite| prerequisite.run_id == job.run_id)
                    }) {
                        references.push(format!(
                            "dependent run {} references managed prerequisite {} / {:?}",
                            dependent.run_id(),
                            job.run_id,
                            dependent.state()
                        ));
                        if retirement
                            && !matches!(
                                dependent.state(),
                                schedule::RunState::Succeeded
                                    | schedule::RunState::Failed
                                    | schedule::RunState::Cancelled
                            )
                        {
                            blockers.push(format!("dependency run {} still needs managed prerequisite {}; retain until it finishes or is removed explicitly", dependent.run_id(), job.run_id));
                        }
                    }
                }
                references.push(format!(
                    "run {} / {:?} / result {} / ledger {}",
                    job.run_id,
                    job.state,
                    if job.collected {
                        "returned"
                    } else {
                        "not returned"
                    },
                    hash
                ));
                if !matches!(
                    job.state,
                    WorkerState::Done | WorkerState::Failed | WorkerState::Cancelled
                ) {
                    blockers.push(format!(
                        "worker run {} is {:?}; inspect/reconcile before lifecycle changes",
                        job.run_id, job.state
                    ));
                }
            }
        }
    }
    references.sort();
    blockers.sort();
    Ok((references, blockers))
}

fn dependency_state(state: WorkerState) -> schedule::RunState {
    match state {
        WorkerState::Queued => schedule::RunState::Pending,
        WorkerState::Starting
        | WorkerState::Thinking
        | WorkerState::Tool
        | WorkerState::WaitingInput
        | WorkerState::Writing => schedule::RunState::Running,
        WorkerState::Done => schedule::RunState::Succeeded,
        WorkerState::Failed => schedule::RunState::Failed,
        WorkerState::Cancelled => schedule::RunState::Cancelled,
        WorkerState::Paused => schedule::RunState::PausedRecovery,
    }
}

pub(super) struct Recovery {
    pub state_root: PathBuf,
    path: PathBuf,
    pub(super) workspace: PathBuf,
    owner: String,
    _lock: File,
    last: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crashed_workers_restore_paused_with_one_owner_and_no_stored_result_or_context() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let recovery = dir.path().join("pane/worker-runs");
        let mut d = Delegates::default();
        d.attach_recovery(&recovery, &workspace, dir.path())
            .unwrap();
        let mut competitor = Delegates::default();
        assert!(competitor
            .attach_recovery(&recovery, &workspace, dir.path())
            .is_err());
        let id = d.push_test_job(
            "synthetic",
            ProviderKind::Codex,
            WorkerState::WaitingInput,
            "unconfirmed action",
            0,
        );
        let job = d.get_mut(id).unwrap();
        job.reference_storage = false; // An existing pre-reference row must remain body-based.
        job.spec.cwd = workspace.clone();
        job.spec.prompt = "original explicit task".into();
        job.spec.prefix = Some("conversation body must not persist".into());
        job.text = "native result must not persist".into();
        job.session_id = Some("exact-native-session".into());
        let mut attempt = Attempt::new("synthetic", ProviderKind::Codex, None, None);
        attempt.started_at = Some(accounting::now());
        attempt.usage.input = Some(123);
        job.attempt_records.push(attempt);
        let run = job.run_id.clone();
        d.persist_recovery();
        let bytes = std::fs::read_to_string(recovery.join("workers.json")).unwrap();
        assert!(!bytes.contains("conversation body must not persist"));
        assert!(!bytes.contains("native result must not persist"));
        assert!(bytes.contains("original explicit task"));
        assert!(!bytes.contains("reference_storage"));
        drop(d);
        let mut restored = Delegates::default();
        assert_eq!(
            restored
                .attach_recovery(&recovery, &workspace, dir.path())
                .unwrap(),
            1
        );
        assert_eq!(restored.get(id).unwrap().run_id, run);
        assert_eq!(restored.get(id).unwrap().state, WorkerState::Paused);
        assert!(!restored.any_active());
        assert!(restored.poll(Instant::now()).is_empty());
        let usage = crate::token_log::load(dir.path());
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].new_input, 123);
        assert_eq!(
            usage[0].attribution.as_ref().unwrap().outcome,
            "interrupted"
        );
        assert!(!usage[0].attribution.as_ref().unwrap().output_known);
        assert_eq!(crate::token_log::totals(&usage).incomplete_events, 1);
        assert!(restored
            .restore_result(id, "partial output".into())
            .is_err());
        assert!(restored
            .recovery_source(id)
            .unwrap_err()
            .contains("exact-native-session"));
        restored.reserve_delivery(&[id]).unwrap();
        drop(restored);
        let mut restored = Delegates::default();
        restored
            .attach_recovery(&recovery, &workspace, dir.path())
            .unwrap();
        assert!(restored.get(id).unwrap().collected);
        assert_eq!(crate::token_log::load(dir.path()).len(), 1);
        drop(restored);
        assert!(Delegates::default()
            .attach_recovery(&recovery, dir.path(), dir.path())
            .is_err());

        // New, durably returned plain-text jobs retain only exact native references.
        let home = dir.path().join("native-home");
        std::fs::create_dir_all(home.join("sessions")).unwrap();
        crate::store::upsert_stored_account(
            dir.path(),
            crate::store::StoredAccount {
                id: "synthetic".into(),
                provider: ProviderKind::Codex,
                home_path: Some(home.to_string_lossy().into_owned()),
                label: "Synthetic".into(),
                created_at: "0".into(),
            },
        )
        .unwrap();
        let session = "worker-reference-native";
        let native = home.join("sessions/worker-reference.jsonl");
        let original = "NATIVE_WORKER_REFERENCE_TASK_83";
        let full_expected = format!("[Verified worker prefix]\n{original}");
        let records = [
            serde_json::json!({"type":"session_meta","payload":{"id":session,"cwd":workspace}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":full_expected}]}}),
        ];
        let native_bytes = records
            .iter()
            .map(|record| record.to_string() + "\n")
            .collect::<String>();
        std::fs::write(&native, &native_bytes).unwrap();
        let binding =
            crate::native_memory::Binding::new(ProviderKind::Codex, "synthetic", &home, session)
                .unwrap();
        let scope = crate::native_memory::references::fresh_scope(dir.path(), &binding).unwrap();
        let projection = full_expected.len() - original.len()..full_expected.len();
        let reference = TaskReferences::capture(
            dir.path(),
            &scope,
            &full_expected,
            &projection,
            original,
            original,
        )
        .unwrap();
        let mut d = Delegates::default();
        d.attach_recovery(&recovery, &workspace, dir.path())
            .unwrap();
        let complete = d.push_test_job(
            "synthetic",
            ProviderKind::Codex,
            WorkerState::Done,
            "complete",
            0,
        );
        let job = d.get_mut(complete).unwrap();
        job.spec.cwd = workspace.clone();
        job.spec.home = Some(home.clone());
        job.spec.prompt = original.into();
        job.task = original.into();
        job.session_id = Some(session.into());
        job.task_reference = Some(reference);
        d.save_recovery().unwrap();
        assert!(
            std::fs::read_to_string(recovery.join("workers.json"))
                .unwrap()
                .contains(original),
            "uncollected requests remain protected recovery"
        );
        d.reserve_delivery(&[complete]).unwrap();
        let stored = std::fs::read_to_string(recovery.join("workers.json")).unwrap();
        assert!(
            !stored.contains(original),
            "returned native-backed task bodies are absent"
        );
        assert!(
            stored.contains("task_reference") && stored.contains("original explicit task"),
            "legacy bodies remain untouched"
        );
        drop(d);
        let mut d = Delegates::default();
        d.attach_recovery(&recovery, &workspace, dir.path())
            .unwrap();
        assert_eq!(
            d.recovered_task(complete).unwrap(),
            ("synthetic".into(), original.into())
        );
        assert_eq!(d.get(complete).unwrap().task, original);
        assert!(
            !std::fs::read_to_string(recovery.join("workers.json"))
                .unwrap()
                .contains(original),
            "hydration never copies text back to owned storage"
        );
        drop(d);
        std::fs::write(
            &native,
            native_bytes.replace(original, "REPLACED_WORKER_REFERENCE_TASK"),
        )
        .unwrap();
        let mut d = Delegates::default();
        d.attach_recovery(&recovery, &workspace, dir.path())
            .unwrap();
        assert!(d
            .recovered_task(complete)
            .unwrap_err()
            .contains("retry is disabled"));
        assert!(d.get(complete).unwrap().task.contains("unavailable"));
        assert!(d.get(complete).unwrap().spec.prompt.is_empty());
        assert!(!std::fs::read_to_string(recovery.join("workers.json"))
            .unwrap()
            .contains(original));
        drop(d);
        for (provider, account, extension) in [
            (ProviderKind::Codex, "synthetic-image", "png"),
            (ProviderKind::Claude, "synthetic-document", "pdf"),
        ] {
            let home = dir.path().join(account);
            let folder = home.join(if provider == ProviderKind::Codex {
                "sessions"
            } else {
                "projects/fixture"
            });
            std::fs::create_dir_all(&folder).unwrap();
            crate::store::upsert_stored_account(
                dir.path(),
                crate::store::StoredAccount {
                    id: account.into(),
                    provider,
                    home_path: Some(home.to_string_lossy().into_owned()),
                    label: "Synthetic".into(),
                    created_at: "0".into(),
                },
            )
            .unwrap();
            let session = if provider == ProviderKind::Codex {
                "worker-image-native"
            } else {
                "worker-document-native"
            };
            let native = folder.join(format!("{session}.jsonl"));
            let source = dir.path().join(format!("snapshot.{extension}"));
            let attachment = if provider == ProviderKind::Codex {
                image::RgbaImage::from_pixel(2, 2, image::Rgba([20, 40, 60, 255]))
                    .save(&source)
                    .unwrap();
                crate::attachments::Attachment::Image(
                    crate::attachments::import(dir.path(), &source).unwrap(),
                )
            } else {
                std::fs::write(
                    &source,
                    b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n%%EOF\n",
                )
                .unwrap();
                crate::attachments::Attachment::PdfDocument(
                    crate::attachments::PdfDocument::import(dir.path(), &source).unwrap(),
                )
            };
            let snapshot = match &attachment {
                crate::attachments::Attachment::Image(image) => image.path.clone(),
                crate::attachments::Attachment::PdfDocument(document) => document.path.clone(),
                _ => unreachable!(),
            };
            let message = crate::attachments::Message {
                text: "ok".into(),
                attachments: vec![attachment],
                invalid: None,
            };
            let original = message.encode();
            let raw = if message.has_pdfs() {
                message.text.clone()
            } else {
                message.prompt_text().unwrap()
            };
            let full =
                format!("Worker prefix already mentions ok; source must be anchored.\n{raw}");
            let projection = full.len() - raw.len()..full.len() - raw.len() + message.text.len();
            let header = if provider == ProviderKind::Codex {
                serde_json::json!({"type":"session_meta","payload":{"id":session}})
            } else {
                serde_json::json!({"type":"system","sessionId":session})
            };
            let event = if provider == ProviderKind::Codex {
                serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":full}]}})
            } else {
                serde_json::json!({"type":"user","message":{"role":"user","content":message.claude_content(&full).unwrap()}})
            };
            std::fs::write(&native, format!("{header}\n{event}\n")).unwrap();
            let binding =
                crate::native_memory::Binding::new(provider, account, &home, session).unwrap();
            let scope =
                crate::native_memory::references::fresh_scope(dir.path(), &binding).unwrap();
            assert!(
                crate::native_memory::references::capture_text(dir.path(), &scope, "user", "ok")
                    .is_err(),
                "short text alone is ambiguous"
            );
            let refs = TaskReferences::capture(
                dir.path(),
                &scope,
                &full,
                &projection,
                &original,
                &cap_chars(original.trim(), TASK_MAX_CHARS),
            )
            .unwrap();
            let mut d = Delegates::default();
            let recovery = dir.path().join(format!("pane-{extension}"));
            d.attach_recovery(&recovery, &workspace, dir.path())
                .unwrap();
            let id = d.push_test_job(account, provider, WorkerState::Done, "done", 0);
            let job = d.get_mut(id).unwrap();
            job.spec.cwd = workspace.clone();
            job.spec.home = Some(home.clone());
            job.spec.prompt = original.clone();
            job.task = cap_chars(original.trim(), TASK_MAX_CHARS);
            job.session_id = Some(session.into());
            job.task_reference = Some(refs);
            d.reserve_delivery(&[id]).unwrap();
            let bytes = std::fs::read_to_string(recovery.join("workers.json")).unwrap();
            let saved: serde_json::Value = serde_json::from_str(&bytes).unwrap();
            assert_eq!(saved["jobs"][0]["spec"]["prompt"], "");
            assert_eq!(saved["jobs"][0]["task"], "");
            assert!(
                !bytes.contains("Worker prefix already mentions") && !bytes.contains("\\\"ok\\\"")
            );
            drop(d);
            let mut d = Delegates::default();
            d.attach_recovery(&recovery, &workspace, dir.path())
                .unwrap();
            assert_eq!(
                d.recovered_task(id).unwrap(),
                (account.into(), original.clone())
            );
            assert_eq!(
                d.get(id).unwrap().task,
                cap_chars(original.trim(), TASK_MAX_CHARS)
            );
            drop(d);
            std::fs::write(&snapshot, b"changed snapshot").unwrap();
            let mut d = Delegates::default();
            d.attach_recovery(&recovery, &workspace, dir.path())
                .unwrap();
            assert!(d
                .recovered_task(id)
                .unwrap_err()
                .contains("retry is disabled"));
        }
        let mut forged: serde_json::Value = serde_json::from_str(&stored).unwrap();
        let row = forged["jobs"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["id"] == complete)
            .unwrap();
        row["task_reference"]["prompt"]["binding"]["account"] =
            serde_json::json!("foreign-account");
        let forged = serde_json::to_vec(&forged).unwrap();
        std::fs::write(recovery.join("workers.json"), &forged).unwrap();
        assert!(Delegates::default()
            .attach_recovery(&recovery, &workspace, dir.path())
            .is_err());
        assert_eq!(
            std::fs::read(recovery.join("workers.json")).unwrap(),
            forged,
            "foreign references never trigger migration or fallback"
        );
    }

    #[test]
    fn failed_recovery_write_prevents_dispatch_and_delivery() {
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("pane");
        let mut d = Delegates::default();
        d.attach_recovery(&recovery, dir.path(), dir.path())
            .unwrap();
        std::fs::create_dir(recovery.join("workers.json")).unwrap();
        let id = d.push_test_job(
            "synthetic",
            ProviderKind::Codex,
            WorkerState::Done,
            "done",
            0,
        );
        d.get_mut(id).unwrap().spec.cwd = dir.path().into();
        assert!(d.reserve_delivery(&[id]).is_err());
        assert!(!d.get(id).unwrap().collected);
        d.launch(id, Instant::now());
        assert_eq!(d.get(id).unwrap().state, WorkerState::Paused);
        assert!(!d.any_active());
    }

    #[test]
    fn dependency_proposals_persist_pause_and_never_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("pane");
        let mut d = Delegates::default();
        d.attach_recovery(&recovery, dir.path(), dir.path())
            .unwrap();
        let parent = d.push_test_job(
            "synthetic",
            ProviderKind::Codex,
            WorkerState::Done,
            "done",
            0,
        );
        d.get_mut(parent).unwrap().spec.cwd = dir.path().into();
        let run = d.get(parent).unwrap().run_id.clone();
        d.save_recovery().unwrap();
        let task_id = uuid::Uuid::new_v4().to_string();
        let task = schedule::Task::new(
            task_id.clone(),
            d.conversation_id.clone(),
            uuid::Uuid::new_v4().to_string(),
            vec![schedule::Prerequisite {
                task_id: run.clone(),
                run_id: run.clone(),
            }],
        )
        .unwrap();
        d.add_dependency_task(task).unwrap();
        assert_eq!(
            d.dependency_graph().readiness(&task_id).unwrap(),
            schedule::Readiness::Ready
        );
        assert!(d.set_dependencies(&run, Vec::new()).is_err());
        assert_eq!(d.jobs.len(), 1);
        // Viewer pruning cannot discard a successful prerequisite that is still referenced.
        d.jobs.clear();
        d.save_recovery().unwrap();
        assert_eq!(d.dependency_graph().tasks().len(), 2);
        drop(d);
        let mut restored = Delegates::default();
        assert_eq!(
            restored
                .attach_recovery(&recovery, dir.path(), dir.path())
                .unwrap(),
            0
        );
        assert_eq!(
            restored.dependency_graph().readiness(&task_id).unwrap(),
            schedule::Readiness::PausedRecovery
        );
        assert_eq!(
            restored.dependency_graph().tasks()[0].state(),
            schedule::RunState::Succeeded
        );
        assert!(restored.poll(Instant::now()).is_empty());
        assert!(!restored.any_active());
    }

    #[test]
    fn dependency_mutations_roll_back_on_write_failure_and_reject_foreign_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("pane");
        let mut d = Delegates::default();
        d.attach_recovery(&recovery, dir.path(), dir.path())
            .unwrap();
        let task = |conversation| {
            schedule::Task::new(
                uuid::Uuid::new_v4().to_string(),
                conversation,
                uuid::Uuid::new_v4().to_string(),
                Vec::new(),
            )
            .unwrap()
        };
        assert!(d
            .add_dependency_task(task(uuid::Uuid::new_v4().to_string()))
            .is_err());
        std::fs::create_dir(recovery.join("workers.json")).unwrap();
        let before = d.dependency_graph().clone();
        assert!(d
            .add_dependency_task(task(d.conversation_id.clone()))
            .is_err());
        assert_eq!(d.dependency_graph(), &before);
    }

    #[test]
    fn legacy_ledgers_migrate_without_replay_and_invalid_graphs_keep_original_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("pane");
        let mut d = Delegates::default();
        d.attach_recovery(&recovery, dir.path(), dir.path())
            .unwrap();
        let id = d.push_test_job(
            "synthetic",
            ProviderKind::Codex,
            WorkerState::Tool,
            "in progress",
            0,
        );
        d.get_mut(id).unwrap().spec.cwd = dir.path().into();
        d.save_recovery().unwrap();
        drop(d);
        let path = recovery.join("workers.json");
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        legacy["version"] = serde_json::json!(1);
        legacy.as_object_mut().unwrap().remove("schedule");
        for row in legacy["jobs"].as_array_mut().unwrap() {
            row.as_object_mut().unwrap().remove("reference_storage");
            row.as_object_mut().unwrap().remove("task_reference");
        }
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut restored = Delegates::default();
        restored
            .attach_recovery(&recovery, dir.path(), dir.path())
            .unwrap();
        assert_eq!(restored.get(id).unwrap().state, WorkerState::Paused);
        assert_eq!(
            restored.dependency_graph().tasks()[0].state(),
            schedule::RunState::PausedRecovery
        );
        drop(restored);
        let mut corrupt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(corrupt["version"], 8);
        corrupt["schedule"]["tasks"][0]["state"] = serde_json::json!("succeeded");
        let bytes = serde_json::to_vec(&corrupt).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(Delegates::default()
            .attach_recovery(&recovery, dir.path(), dir.path())
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        corrupt["version"] = serde_json::json!(99);
        let bytes = serde_json::to_vec(&corrupt).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(Delegates::default()
            .attach_recovery(&recovery, dir.path(), dir.path())
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[derive(Serialize, Deserialize)]
struct Ledger {
    version: u32,
    owner: String,
    workspace: PathBuf,
    conversation: String,
    jobs: Vec<SavedJob>,
    #[serde(default)]
    schedule: Option<schedule::Graph>,
}

#[derive(Serialize, Deserialize)]
struct SavedJob {
    id: u32,
    run_id: String,
    conversation: String,
    account: String,
    provider: ProviderKind,
    #[serde(default, skip_serializing_if = "is_false")]
    reference_storage: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task_reference: Option<TaskReferences>,
    task: String,
    spec: JobSpec,
    state: WorkerState,
    origin: Origin,
    source: Option<String>,
    #[serde(default)]
    routing: Option<crate::routing::Approved>,
    #[serde(default)]
    route_history: Vec<String>,
    #[serde(default)]
    workspace: Option<crate::workspaces::Request>,
    #[serde(default)]
    workspace_notice: Option<String>,
    native_session: Option<String>,
    collected: bool,
    attempts: Vec<String>,
    attempt_records: Vec<Attempt>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn task_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

impl TaskReferences {
    pub(super) fn capture(
        root: &Path,
        scope: &crate::native_memory::references::CaptureScope,
        full_expected: &str,
        projection: &std::ops::Range<usize>,
        original: &str,
        task: &str,
    ) -> Result<Self, String> {
        let message = crate::attachments::Message::decode(original);
        message.validate()?;
        if message.encode() != original
            || full_expected.get(projection.clone()) != Some(message.text.as_str())
        {
            return Err(
                "native worker input cannot exactly reconstruct its original envelope".into(),
            );
        }
        let prompt = crate::native_memory::references::capture_projected_text(
            root,
            scope,
            "user",
            full_expected,
            projection,
        )?;
        let task_projection = if task == cap_chars(original.trim(), TASK_MAX_CHARS) {
            TaskProjection::OriginalPrompt {
                chars: TASK_MAX_CHARS,
            }
        } else if task == cap_chars(message.display().trim(), TASK_MAX_CHARS) {
            TaskProjection::MessageDisplay {
                chars: TASK_MAX_CHARS,
            }
        } else {
            TaskProjection::Native(Box::new(crate::native_memory::references::capture_text(
                root, scope, "user", task,
            )?))
        };
        Ok(Self {
            prompt,
            attachments: message.attachments,
            prompt_sha256: task_hash(original),
            task: task_projection,
            task_sha256: task_hash(task),
        })
    }

    fn hydrate(&self, root: &Path) -> Result<(String, String), String> {
        let text = crate::native_memory::references::hydrate_text(root, &self.prompt)?;
        let message = crate::attachments::Message {
            text,
            attachments: self.attachments.clone(),
            invalid: None,
        };
        message.validate()?;
        for attachment in &message.attachments {
            match attachment {
                crate::attachments::Attachment::Image(image) => {
                    image.data()?;
                }
                crate::attachments::Attachment::TextDocument(document) => {
                    document.text()?;
                }
                crate::attachments::Attachment::PdfDocument(document) => {
                    document.data()?;
                }
            }
        }
        let original = message.encode();
        let task = match &self.task {
            TaskProjection::Native(reference) => {
                crate::native_memory::references::hydrate_text(root, reference)?
            }
            TaskProjection::OriginalPrompt { chars } => cap_chars(original.trim(), *chars),
            TaskProjection::MessageDisplay { chars } => cap_chars(message.display().trim(), *chars),
        };
        if original.len() > 1024 * 1024
            || original.trim().is_empty()
            || task.len() > 32768
            || task_hash(&original) != self.prompt_sha256
            || task_hash(&task) != self.task_sha256
        {
            return Err("native worker task projection changed or exceeds its bounds".into());
        }
        Ok((original, task))
    }
}

pub(super) fn reference_matches(
    reference: &TaskReferences,
    account: &str,
    provider: ProviderKind,
    home: Option<&Path>,
    session: Option<&str>,
) -> bool {
    let Some(home) = home else { return false };
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    let digest_valid =
        |digest: &str| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit());
    if reference.attachments.len() > crate::attachments::MAX_IMAGES
        || !digest_valid(&reference.prompt_sha256)
        || !digest_valid(&reference.task_sha256)
    {
        return false;
    }
    let task = match &reference.task {
        TaskProjection::Native(reference) => Some(reference.as_ref()),
        TaskProjection::OriginalPrompt { chars } | TaskProjection::MessageDisplay { chars } => {
            if *chars == 0 || *chars > TASK_MAX_CHARS {
                return false;
            }
            None
        }
    };
    std::iter::once(&reference.prompt)
        .chain(task)
        .all(|reference| {
            let binding = reference.binding();
            reference.role() == "user"
                && binding.account == account
                && binding.provider == provider
                && binding.home == home
                && Some(binding.session_id.as_str()) == session
        })
}

impl Delegates {
    /// The caller supplies its pane-owned directory, not a newest-session guess. A second OS lock
    /// protects worker ownership even if this module is used outside the TUI.
    pub fn attach_recovery(
        &mut self,
        directory: &Path,
        workspace: &Path,
        state_root: &Path,
    ) -> Result<usize, String> {
        if self.any_active() {
            return Err("finish or cancel workers before changing recovery ownership".into());
        }
        self.recovery.take();
        let workspace = std::fs::canonicalize(workspace).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("workers.lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock()
            .map_err(|_| "another process owns these workers")?;
        let path = directory.join("workers.json");
        let mut bytes = Vec::new();
        match File::open(&path) {
            Ok(file) => {
                file.take(LEDGER_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        if bytes.len() as u64 > LEDGER_BYTES {
            return Err("worker recovery exceeds 8 MiB".into());
        }
        let mut jobs = Vec::new();
        let mut graph = schedule::Graph::default();
        let owner;
        let mut conversation = self.conversation_id.clone();
        if bytes.is_empty() {
            owner = uuid::Uuid::new_v4().to_string();
        } else {
            let ledger: Ledger = serde_json::from_slice(&bytes)
                .map_err(|_| "invalid worker ledger; original kept")?;
            if !matches!(ledger.version, 1..=9)
                || ledger.workspace != workspace
                || ledger.jobs.len() > 64
                || uuid::Uuid::parse_str(&ledger.owner).is_err()
                || uuid::Uuid::parse_str(&ledger.conversation).is_err()
            {
                return Err("worker ledger identity/version mismatch; original kept".into());
            }
            owner = ledger.owner;
            conversation = ledger.conversation;
            if ledger.version >= 2 {
                graph = ledger
                    .schedule
                    .ok_or("worker dependency graph missing; original kept")?;
                graph.validate()?;
            } else if ledger.schedule.is_some() {
                return Err(
                    "legacy ledger contains unexpected dependency metadata; original kept".into(),
                );
            }
            let mut ids = std::collections::HashSet::new();
            let mut runs = std::collections::HashSet::new();
            for mut saved in ledger.jobs {
                if saved.id == 0
                    || saved.id == u32::MAX
                    || !ids.insert(saved.id)
                    || !runs.insert(saved.run_id.clone())
                    || uuid::Uuid::parse_str(&saved.run_id).is_err()
                    || uuid::Uuid::parse_str(&saved.conversation).is_err()
                    || saved.spec.prompt.len() > 1024 * 1024
                    || saved.task.len() > 32768
                    || (ledger.version < 9
                        && (saved.reference_storage || saved.task_reference.is_some()))
                    || saved.task_reference.as_ref().is_some_and(|reference| {
                        !saved.reference_storage
                            || saved.state != WorkerState::Done
                            || !saved.collected
                            || !saved.task.is_empty()
                            || !saved.spec.prompt.is_empty()
                            || !reference_matches(
                                reference,
                                &saved.account,
                                saved.provider,
                                saved.spec.home.as_deref(),
                                saved.native_session.as_deref(),
                            )
                    })
                    || saved.attempts.len() > 4
                    || saved.attempt_records.len() > 4
                    || saved.attempt_records.iter().any(|attempt| !attempt.valid())
                    || saved.route_history.len() > 4
                    || saved.workspace.as_ref().is_some_and(|request| {
                        !request.structurally_valid() || request.binding.owner != workspace
                    })
                    || saved
                        .workspace_notice
                        .as_ref()
                        .is_some_and(|notice| notice.len() > 4096)
                    || saved
                        .route_history
                        .iter()
                        .any(|report| report.len() > 32_000)
                    || saved
                        .routing
                        .as_ref()
                        .is_some_and(|routing| !routing.valid())
                    || std::fs::canonicalize(&saved.spec.cwd).ok().as_ref()
                        != Some(
                            saved
                                .workspace
                                .as_ref()
                                .map_or(&workspace, |request| &request.binding.execution),
                        )
                {
                    return Err(
                        "invalid worker run identity or request bounds; original kept".into(),
                    );
                }
                let run_state = dependency_state(saved.state);
                if ledger.version == 1 {
                    graph.observe_run(&saved.conversation, &saved.run_id, run_state)?;
                } else if !graph.matches_run(&saved.conversation, &saved.run_id, run_state) {
                    return Err(
                        "worker dependency receipt does not match its run; original kept".into(),
                    );
                }
                let now = Instant::now();
                let state = if saved.state.is_finished() {
                    saved.state
                } else {
                    WorkerState::Paused
                };
                let mut task_reference_unavailable = false;
                if let Some(reference) = &saved.task_reference {
                    let hydrated = reference.hydrate(state_root);
                    match hydrated {
                        Ok((prompt, task)) => {
                            saved.spec.prompt = prompt;
                            saved.task = task;
                        }
                        Err(_) => {
                            task_reference_unavailable = true;
                            saved.task = "Task unavailable: native source missing or stale".into();
                        }
                    }
                }
                jobs.push(Job {
                    input:input::State::default(),
                    run_id:saved.run_id, conversation_id:saved.conversation, source_id:saved.source, reference_storage:saved.reference_storage, task_reference:saved.task_reference, task_reference_unavailable, routing:saved.routing, route_history:saved.route_history,
                    workspace:saved.workspace, workspace_notice:saved.workspace_notice, workspace_next_check:None, workspace_invoked:None, preparation_cancel:Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    attempts:saved.attempts, attempt_records:saved.attempt_records, id:saved.id, account:saved.account, provider:saved.provider,
                    task:saved.task, model:saved.spec.model.clone(), trust:saved.spec.trust, spec:saved.spec,
                    state, origin:saved.origin, collected:saved.collected,
                    created:now, started:None, finished:Some(now),
                    snippet:"Recovered metadata; inspect the native session before retrying or returning a result".into(),
                    cards:Vec::new(),text:String::new(),thinking:String::new(),session_id:saved.native_session,
                    exit_code:None,error:Some(if task_reference_unavailable { "Original worker task is unavailable from its verified native source; retry is disabled." } else { "Recovery is paused. No process or tool action was replayed. /delegate recover ID inspects its source; /delegate retry ID reconciled explicitly starts a fresh task." }.into()),
                    cancel:None,cancel_requested:false,announced:true,
                });
            }
        }
        self.next_id = jobs.iter().map(|j| j.id).max().unwrap_or(0) + 1;
        self.jobs = jobs;
        graph.pause_interrupted();
        self.schedule = graph;
        self.conversation_id = conversation;
        self.recovery = Some(Recovery {
            state_root: state_root.into(),
            path,
            workspace,
            owner,
            _lock: lock,
            last: Vec::new(),
        });
        self.reconcile_interrupted_accounting();
        for job in &self.jobs {
            if let Some(request) = &job.workspace {
                crate::workspaces::recover(state_root, &job.run_id, request)?;
            }
        }
        self.save_recovery()?;
        self.flush_accounting();
        self.save_recovery()?;
        Ok(self.jobs.len())
    }

    pub(super) fn save_recovery(&mut self) -> Result<(), String> {
        // Build a validated snapshot before touching either durable state or the accepted graph.
        let mut graph = self.schedule.clone();
        graph.retain_receipts(&self.jobs.iter().map(|j| j.run_id.as_str()).collect());
        for job in &self.jobs {
            graph.observe_run(
                &job.conversation_id,
                &job.run_id,
                dependency_state(job.state),
            )?;
        }
        graph.validate()?;
        let Some(recovery) = self.recovery.as_mut() else {
            self.schedule = graph;
            return Ok(());
        };
        if self.jobs.is_empty()
            && graph.tasks().is_empty()
            && recovery.last.is_empty()
            && !recovery.path.exists()
        {
            return Ok(());
        }
        let jobs = self
            .jobs
            .iter()
            .map(|j| {
                let redact = j.reference_storage
                    && j.state == WorkerState::Done
                    && j.collected
                    && j.task_reference.is_some();
                let mut spec = j.spec.clone();
                if redact {
                    spec.prompt.clear();
                }
                SavedJob {
                    id: j.id,
                    run_id: j.run_id.clone(),
                    conversation: j.conversation_id.clone(),
                    account: j.account.clone(),
                    provider: j.provider,
                    reference_storage: j.reference_storage,
                    task_reference: redact.then(|| j.task_reference.clone()).flatten(),
                    task: if redact {
                        String::new()
                    } else {
                        j.task.clone()
                    },
                    spec,
                    state: j.state,
                    origin: j.origin,
                    source: j.source_id.clone(),
                    routing: j.routing.clone(),
                    route_history: j.route_history.clone(),
                    workspace: j.workspace.clone(),
                    workspace_notice: j.workspace_notice.clone(),
                    native_session: j.session_id.clone(),
                    collected: j.collected,
                    attempts: j.attempts.clone(),
                    attempt_records: j.attempt_records.clone(),
                }
            })
            .collect();
        let bytes = serde_json::to_vec(&Ledger {
            version: if self.jobs.iter().any(|job| job.reference_storage) {
                9
            } else {
                8
            },
            owner: recovery.owner.clone(),
            workspace: recovery.workspace.clone(),
            conversation: self.conversation_id.clone(),
            jobs,
            schedule: Some(graph.clone()),
        })
        .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > LEDGER_BYTES || self.jobs.len() > 64 {
            return Err("worker recovery exceeds its budget".into());
        }
        if bytes != recovery.last {
            crate::state_io::write_atomic(&recovery.path, &bytes).map_err(|e| e.to_string())?;
            recovery.last = bytes;
        }
        self.schedule = graph;
        Ok(())
    }

    /// Descriptive metadata only: readiness never authorizes or dispatches a provider turn.
    pub fn dependency_graph(&self) -> &schedule::Graph {
        &self.schedule
    }

    /// Register a proposed task without adding it to the provider execution queue.
    pub fn add_dependency_task(&mut self, task: schedule::Task) -> Result<(), String> {
        if self.recovery.is_none() {
            return Err("dependency metadata requires durable worker ownership".into());
        }
        if task.conversation_id() != self.conversation_id {
            return Err("dependency task belongs to another conversation".into());
        }
        let before = self.schedule.clone();
        self.schedule.insert(task)?;
        if let Err(error) = self.save_recovery() {
            self.schedule = before;
            return Err(error);
        }
        Ok(())
    }

    /// Update proposed edges atomically under the same owner lease as worker recovery.
    pub fn set_dependencies(
        &mut self,
        task_id: &str,
        prerequisites: Vec<schedule::Prerequisite>,
    ) -> Result<(), String> {
        if self.recovery.is_none() {
            return Err("dependency metadata requires durable worker ownership".into());
        }
        let task = self
            .schedule
            .tasks()
            .iter()
            .find(|task| task.task_id() == task_id)
            .ok_or("unknown dependency task")?;
        if self.jobs.iter().any(|job| job.run_id == task.run_id()) {
            return Err("existing worker submissions do not support dependencies; register a metadata-only proposal".into());
        }
        if task.conversation_id() != self.conversation_id {
            return Err("dependency task belongs to another conversation".into());
        }
        let before = self.schedule.clone();
        self.schedule.set_prerequisites(task_id, prerequisites)?;
        if let Err(error) = self.save_recovery() {
            self.schedule = before;
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn persist_recovery(&mut self) {
        if let Err(error) = self.save_recovery() {
            self.recovery_error = Some(error);
        }
    }

    pub fn recovery_error(&mut self) -> Option<String> {
        self.recovery_error.take()
    }
    pub fn recovery_ready(&self) -> bool {
        self.recovery.is_some()
    }

    pub fn new_conversation(&mut self) {
        self.conversation_id = uuid::Uuid::new_v4().to_string();
        self.persist_recovery();
    }

    pub fn reserve_delivery(&mut self, ids: &[u32]) -> Result<(), String> {
        let before: Vec<_> = self.jobs.iter().map(|j| j.collected).collect();
        for j in &mut self.jobs {
            if ids.contains(&j.id) {
                j.collected = true;
            }
        }
        if let Err(e) = self.save_recovery() {
            for (j, collected) in self.jobs.iter_mut().zip(before) {
                j.collected = collected;
            }
            return Err(e);
        }
        Ok(())
    }

    /// Explicit inspection reads a revision-bound pointer, never the latest assistant response.
    pub fn recovery_source(&self, id: u32) -> Result<(PathBuf, String), String> {
        let root = self
            .recovery
            .as_ref()
            .ok_or("worker recovery is unavailable")?
            .state_root
            .clone();
        let job = self.get(id).ok_or("unknown worker run")?;
        let source = job.source_id.as_ref().ok_or_else(|| format!("No verified result pointer for run {}. Native session {:?}. Original task:\n{}\nInspect the workspace/native session before /delegate retry {id} reconciled.", job.run_id, job.session_id, job.spec.prompt))?;
        Ok((root, source.clone()))
    }

    pub fn recovered_task(&self, id: u32) -> Result<(String, String), String> {
        let job = self.get(id).ok_or("unknown worker run")?;
        if job.task_reference_unavailable || job.spec.prompt.trim().is_empty() {
            return Err("Original worker task is unavailable from its verified native source; retry is disabled. Restore the source or provide a new explicit task.".into());
        }
        if job.state.is_running() || job.state == WorkerState::Queued {
            return Err("worker is still active".into());
        }
        Ok((job.account.clone(), job.spec.prompt.clone()))
    }

    pub fn restore_result(&mut self, id: u32, result: String) -> Result<(), String> {
        let job = self.get_mut(id).ok_or("unknown worker run")?;
        // An uncertain run remains paused even if its native source contains some assistant text.
        if job.state != WorkerState::Done {
            return Err(
                "run was not durably completed; inspect its source and reconcile before retry"
                    .into(),
            );
        }
        job.text = result;
        Ok(())
    }

    pub(super) fn worker_failover(&mut self, id: u32, out: &TurnOutcome, now: Instant) -> bool {
        let Some(root) = self.recovery.as_ref().map(|r| r.state_root.clone()) else {
            return false;
        };
        let settings = get_settings(&root);
        if !settings.delegate_quota_failover {
            return false;
        }
        let Some(job) = self.get(id) else {
            return false;
        };
        let Some(approved) = &job.routing else {
            return false;
        };
        if !approved.failover {
            return false;
        }
        // A confirmed terminal quota error before any work is safe to reroute. Any tool or partial
        // answer, cancellation, or unconfirmed exit requires reconciliation instead.
        if job.cancel_requested
            || job.spec.native_input != crate::native_input::Mode::Standard
            || job.input.observed()
            || out.cancelled
            || out.exit_code.is_none()
            || !(out.failed || out.exit_code != Some(0))
            || job.attempts.len() >= 4
            || !job.cards.is_empty()
            || !out.cards.is_empty()
            || !job.text.trim().is_empty()
            || !out.text.trim().is_empty()
            || !crate::provider::stream_cards::usage_exhausted(&format!(
                "{}\n{}",
                out.stderr_tail,
                job.error.as_deref().unwrap_or("")
            ))
        {
            return false;
        }
        let mut context = crate::routing::Context::new(
            crate::routing::Purpose::WorkerFailover,
            &job.account,
            job.provider,
            job.spec.home.clone(),
            job.model.clone(),
        );
        context.owner_account = approved.owner.clone();
        context.workers = Some(settings.workers.iter().cloned().collect());
        context
            .attempted_accounts
            .extend(job.attempts.iter().cloned());
        context
            .attempted_pairs
            .extend(job.attempt_records.iter().filter_map(|attempt| {
                attempt
                    .model
                    .as_ref()
                    .map(|model| (attempt.account.clone(), model.clone()))
            }));
        for target in &approved.targets {
            if self.jobs.iter().any(|other| {
                other.id != id && other.account == target.target.account && other.state.is_running()
            }) {
                context.running.insert(target.target.account.clone());
            }
        }
        let Ok(decision) = approved.evaluate(&root, &context) else {
            return false;
        };
        let report: String = decision.report().join("\n").chars().take(8000).collect();
        if let Some(job) = self.get_mut(id) {
            if job.route_history.len() < 4 {
                job.route_history.push(report);
            }
        }
        let Some(selected) = decision.selected() else {
            return false;
        };
        let Some(target) = get_account(&root, &selected.candidate.target.account) else {
            return false;
        };
        let target_home = target
            .home_path
            .as_ref()
            .and_then(|home| home.canonicalize().ok());
        if target_home.is_none() {
            return false;
        }
        let model = selected.candidate.target.model.clone();
        if let Some(job) = self.get_mut(id) {
            job.attempts.push(target.account.clone());
            job.attempt_records.push(Attempt::new(
                &target.account,
                target.provider,
                target_home.clone(),
                model.clone(),
            ));
            job.account = target.account;
            job.provider = target.provider;
            job.model = model.clone();
            job.spec.home = target_home;
            job.spec.model = model;
            job.session_id = None;
            job.source_id = None;
            job.error = None;
            job.exit_code = None;
            job.cancel = None;
            job.state = WorkerState::Queued;
            job.snippet = "Explicit quota exhaustion; moving to the next configured worker".into();
        }
        self.launch(id, now);
        true
    }
}
