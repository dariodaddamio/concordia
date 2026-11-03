//! Durable workspace admission through the existing worker executor.
use super::*;
use crate::workspaces::{self, Admission};

impl Delegates {
    pub(super) fn prepare_workspace(&mut self, id: u32, now: Instant) -> bool {
        let Some(root) = self
            .recovery
            .as_ref()
            .map(|recovery| recovery.state_root.clone())
        else {
            return true;
        };
        let Some(job) = self.get(id) else {
            return false;
        };
        if job
            .workspace_next_check
            .is_some_and(|deadline| now < deadline)
        {
            return false;
        }
        let Some(request) = job.workspace.clone() else {
            self.pause_dependency_job(
                id,
                "worker has no approved workspace binding; reconcile and submit a fresh task"
                    .into(),
                now,
            );
            return false;
        };
        let Some(attempt) = job
            .attempt_records
            .last()
            .and_then(|attempt| attempt.id.clone())
        else {
            return false;
        };
        let run = job.run_id.clone();
        let conversation = job.conversation_id.clone();
        let admission =
            workspaces::for_attempt(&root, &request, &run, &attempt).and_then(|request| {
                workspaces::reserve(&root, &request, &run, &conversation, &attempt)
                    .map(|admission| (request, admission))
            });
        match admission {
            Ok((request, Admission::Reserved)) => {
                if let Some(job) = self.get_mut(id) {
                    job.workspace = Some(request);
                    job.workspace_next_check = None;
                    if job
                        .workspace_invoked
                        .as_ref()
                        .is_none_or(|(id, _)| id != &attempt)
                    {
                        job.workspace_invoked = Some((
                            attempt.clone(),
                            Arc::new(std::sync::atomic::AtomicBool::new(false)),
                        ));
                    }
                    job.workspace_notice = Some("Scheduler reservation held; native filesystem access is governed by the provider".into());
                }
                true
            }
            Ok((_, Admission::Conflict { run, path })) => {
                if let Some(job) = self.get_mut(id) {
                    job.state = WorkerState::Queued;
                    job.workspace_next_check = Some(now + Duration::from_secs(1));
                    job.snippet = format!("Waiting for workspace owner {run}: {}", path.display());
                    job.workspace_notice = Some(job.snippet.clone());
                }
                false
            }
            Ok((_, Admission::ReconcileRequired)) => {
                self.pause_dependency_job(id, "workspace reservation requires stopped-process reconciliation; no provider started".into(), now);
                false
            }
            Err(error) => {
                self.pause_dependency_job(id, format!("workspace admission failed: {error}"), now);
                false
            }
        }
    }

    pub(super) fn finish_workspace(&mut self, id: u32, confirmed: bool, not_started: bool) -> bool {
        let Some(root) = self
            .recovery
            .as_ref()
            .map(|recovery| recovery.state_root.clone())
        else {
            return true;
        };
        let Some(job) = self.get(id) else {
            return false;
        };
        if job.workspace.is_none() {
            return true;
        }
        let Some(attempt) = job
            .attempt_records
            .last()
            .and_then(|attempt| attempt.id.clone())
        else {
            return false;
        };
        let not_started = not_started
            || job.workspace_invoked.as_ref().is_some_and(|(id, flag)| {
                id == &attempt && !flag.load(std::sync::atomic::Ordering::SeqCst)
            });
        let result = workspaces::finish(&root, &job.run_id, &attempt, confirmed || not_started);
        if let Some(job) = self.get_mut(id) {
            job.workspace_notice = Some(match &result {
                Ok(()) if not_started => {
                    "Scheduler reservation released; provider was not started".into()
                }
                Ok(()) if confirmed => {
                    "Scheduler reservation released after confirmed native exit".into()
                }
                Ok(()) => {
                    "Native exit is uncertain; scheduler reservation retained for reconciliation"
                        .into()
                }
                Err(error) => format!("Workspace reservation retained: {error}"),
            });
        }
        result.is_ok() && (confirmed || not_started)
    }

    pub fn acknowledge_workspace_stopped(&mut self, id: u32) -> Result<(), String> {
        let root = self
            .recovery
            .as_ref()
            .ok_or("durable workspace owner unavailable")?
            .state_root
            .clone();
        let job = self.get(id).ok_or("unknown worker job")?;
        if job.state.is_running() || job.state == WorkerState::Queued {
            return Err("stop the active/queued worker first".into());
        }
        workspaces::acknowledge_stopped(&root, &job.run_id, &job.conversation_id)?;
        if let Some(job) = self.get_mut(id) {
            job.workspace_notice = Some("User acknowledged the old process stopped; native process ownership remains unverified for retirement".into());
        }
        self.save_recovery()
    }
}

impl Job {
    pub fn workspace_summary(&self) -> String {
        let Some(request) = &self.workspace else {
            return "Workspace: legacy/unmanaged reservation unavailable".into();
        };
        format!("Execution workspace: {}\n{}\nDeclared scheduler scope: {}\nThese claims do not confine native tools or external editors.",
            request.binding.execution.display(), self.workspace_notice.as_deref().unwrap_or("Reservation not yet acquired"),
            request.describe())
    }
}
