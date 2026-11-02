//! Durable worker ownership and bounded failover. Recovery never starts a provider turn.
use super::*;
use std::fs::{File, OpenOptions};
use std::io::Read;

const LEDGER_BYTES: u64 = 8 * 1024 * 1024;

pub(super) struct Recovery {
    pub state_root: PathBuf,
    path: PathBuf,
    workspace: PathBuf,
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
            WorkerState::Tool,
            "unconfirmed action",
            0,
        );
        let job = d.get_mut(id).unwrap();
        job.spec.cwd = workspace.clone();
        job.spec.prompt = "original explicit task".into();
        job.spec.prefix = Some("conversation body must not persist".into());
        job.text = "native result must not persist".into();
        job.session_id = Some("exact-native-session".into());
        let run = job.run_id.clone();
        d.persist_recovery();
        let bytes = std::fs::read_to_string(recovery.join("workers.json")).unwrap();
        assert!(!bytes.contains("conversation body must not persist"));
        assert!(!bytes.contains("native result must not persist"));
        assert!(bytes.contains("original explicit task"));
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
        drop(restored);
        assert!(Delegates::default()
            .attach_recovery(&recovery, dir.path(), dir.path())
            .is_err());
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
}

#[derive(Serialize, Deserialize)]
struct Ledger {
    version: u32,
    owner: String,
    workspace: PathBuf,
    conversation: String,
    jobs: Vec<SavedJob>,
}

#[derive(Serialize, Deserialize)]
struct SavedJob {
    id: u32,
    run_id: String,
    conversation: String,
    account: String,
    provider: ProviderKind,
    task: String,
    spec: JobSpec,
    state: WorkerState,
    origin: Origin,
    source: Option<String>,
    native_session: Option<String>,
    collected: bool,
    attempts: Vec<String>,
    attempt_records: Vec<Attempt>,
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
        let owner;
        let mut conversation = self.conversation_id.clone();
        if bytes.is_empty() {
            owner = uuid::Uuid::new_v4().to_string();
        } else {
            let ledger: Ledger = serde_json::from_slice(&bytes)
                .map_err(|_| "invalid worker ledger; original kept")?;
            if ledger.version != 1
                || ledger.workspace != workspace
                || ledger.jobs.len() > 64
                || uuid::Uuid::parse_str(&ledger.owner).is_err()
                || uuid::Uuid::parse_str(&ledger.conversation).is_err()
            {
                return Err("worker ledger identity/version mismatch; original kept".into());
            }
            owner = ledger.owner;
            conversation = ledger.conversation;
            let mut ids = std::collections::HashSet::new();
            let mut runs = std::collections::HashSet::new();
            for saved in ledger.jobs {
                if saved.id == 0
                    || saved.id == u32::MAX
                    || !ids.insert(saved.id)
                    || !runs.insert(saved.run_id.clone())
                    || uuid::Uuid::parse_str(&saved.run_id).is_err()
                    || uuid::Uuid::parse_str(&saved.conversation).is_err()
                    || saved.spec.prompt.len() > 1024 * 1024
                    || saved.task.len() > 32768
                    || saved.attempts.len() > 4
                    || saved.attempt_records.len() > 4
                    || std::fs::canonicalize(&saved.spec.cwd).ok().as_ref() != Some(&workspace)
                {
                    return Err(
                        "invalid worker run identity or request bounds; original kept".into(),
                    );
                }
                let now = Instant::now();
                let state = if saved.state.is_finished() {
                    saved.state
                } else {
                    WorkerState::Paused
                };
                jobs.push(Job {
                    run_id:saved.run_id, conversation_id:saved.conversation, source_id:saved.source,
                    attempts:saved.attempts, attempt_records:saved.attempt_records, id:saved.id, account:saved.account, provider:saved.provider,
                    task:saved.task, model:saved.spec.model.clone(), trust:saved.spec.trust, spec:saved.spec,
                    state, origin:saved.origin, collected:saved.collected,
                    created:now, started:None, finished:Some(now),
                    snippet:"Recovered metadata; inspect the native session before retrying or returning a result".into(),
                    cards:Vec::new(),text:String::new(),thinking:String::new(),session_id:saved.native_session,
                    exit_code:None,error:Some("Recovery is paused. No process or tool action was replayed. /delegate recover ID inspects its source; /delegate retry ID reconciled explicitly starts a fresh task.".into()),
                    cancel:None,cancel_requested:false,announced:true,
                });
            }
        }
        self.next_id = jobs.iter().map(|j| j.id).max().unwrap_or(0) + 1;
        self.jobs = jobs;
        self.conversation_id = conversation;
        self.recovery = Some(Recovery {
            state_root: state_root.into(),
            path,
            workspace,
            owner,
            _lock: lock,
            last: Vec::new(),
        });
        self.save_recovery()?;
        Ok(self.jobs.len())
    }

    pub(super) fn save_recovery(&mut self) -> Result<(), String> {
        let Some(recovery) = self.recovery.as_mut() else {
            return Ok(());
        };
        if self.jobs.is_empty() && recovery.last.is_empty() && !recovery.path.exists() {
            return Ok(());
        }
        let jobs = self
            .jobs
            .iter()
            .map(|j| SavedJob {
                id: j.id,
                run_id: j.run_id.clone(),
                conversation: j.conversation_id.clone(),
                account: j.account.clone(),
                provider: j.provider,
                task: j.task.clone(),
                spec: j.spec.clone(),
                state: j.state,
                origin: j.origin,
                source: j.source_id.clone(),
                native_session: j.session_id.clone(),
                collected: j.collected,
                attempts: j.attempts.clone(),
                attempt_records: j.attempt_records.clone(),
            })
            .collect();
        let bytes = serde_json::to_vec(&Ledger {
            version: 1,
            owner: recovery.owner.clone(),
            workspace: recovery.workspace.clone(),
            conversation: self.conversation_id.clone(),
            jobs,
        })
        .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > LEDGER_BYTES || self.jobs.len() > 64 {
            return Err("worker recovery exceeds its budget".into());
        }
        if bytes != recovery.last {
            crate::state_io::write_atomic(&recovery.path, &bytes).map_err(|e| e.to_string())?;
            recovery.last = bytes;
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
        // A confirmed terminal quota error before any work is safe to reroute. Any tool or partial
        // answer, cancellation, or unconfirmed exit requires reconciliation instead.
        if job.cancel_requested
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
        let current_identity = job
            .spec
            .home
            .as_ref()
            .and_then(|h| identity_key(job.provider, h));
        let orchestrator =
            crate::store::get_use_selection(&root).map(|s| normalize_account_id(&s.account));
        let selection = settings
            .workers
            .iter()
            .filter(|a| {
                !job.attempts.contains(a)
                    && self.running_for(a).is_none()
                    && orchestrator.as_ref() != Some(a)
            })
            .find_map(|account| {
                let a = get_account(&root, account)?;
                let h = a.home_path.as_ref()?;
                if !is_logged_in(a.provider, h)
                    || job.spec.home.as_ref().is_some_and(|old| {
                        std::fs::canonicalize(old).ok() == std::fs::canonicalize(h).ok()
                    })
                    || (current_identity.is_some()
                        && identity_key(a.provider, h) == current_identity)
                {
                    return None;
                }
                let model = settings.worker_models.get(account).cloned();
                if model
                    .as_ref()
                    .is_some_and(|m| !crate::catalog::list_models(a.provider, h).contains(m))
                {
                    return None;
                }
                Some((a, model))
            });
        let Some((target, model)) = selection else {
            return false;
        };
        if let Some(job) = self.get_mut(id) {
            job.attempts.push(target.account.clone());
            job.attempt_records.push(Attempt {
                account: target.account.clone(),
                provider: target.provider,
                home: target.home_path.clone(),
                model: model.clone(),
                native_session: None,
                source: None,
            });
            job.account = target.account;
            job.provider = target.provider;
            job.model = model.clone();
            job.spec.home = target.home_path;
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
