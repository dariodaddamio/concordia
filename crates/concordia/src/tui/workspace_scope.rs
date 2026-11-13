//! Main-turn scope ownership and explicit inspection. Native permissions remain authoritative.
use super::*;
use crate::workspaces::{self, Admission, Declaration, Request};

#[derive(Default)]
pub(super) struct State {
    defaults: Vec<Declaration>,
    task: Option<(String, Request)>,
    admission_failed: bool,
    active: Option<(
        crate::provider::interaction::InteractionOwner,
        Arc<std::sync::atomic::AtomicBool>,
    )>,
}

impl State {
    pub(super) fn snapshot(&self, run: &str) -> Option<Request> {
        self.task
            .as_ref()
            .filter(|(id, _)| id == run)
            .map(|(_, request)| request.clone())
    }
}

impl App {
    pub(super) fn bind_main_scope(&mut self) -> Result<(), String> {
        let run = self.continuation.run_id();
        if self.scopes.task.as_ref().is_none_or(|(id, _)| id != run) {
            let request = Request::shared(&self.cwd, &self.scopes.defaults)?;
            self.scopes.task = Some((run.into(), request));
        }
        Ok(())
    }

    pub(super) fn restore_main_scope(
        &mut self,
        run: &str,
        request: Option<Request>,
    ) -> Result<(), String> {
        self.scopes.active = None;
        self.scopes.task = None;
        if let Some(request) = request {
            if !request.structurally_valid()
                || self.cwd.canonicalize().ok().as_ref() != Some(&request.binding.owner)
            {
                return Err("recovered task scope does not match its owner workspace".into());
            }
            workspaces::recover(&self.state_root, run, &request)?;
            self.scopes.task = Some((run.into(), request));
        }
        Ok(())
    }

    pub(super) fn reserve_main_scope(
        &mut self,
        owner: &crate::provider::interaction::InteractionOwner,
    ) -> Result<(), String> {
        self.scopes.admission_failed = true;
        let request = self
            .scopes
            .snapshot(&owner.run_id)
            .ok_or("main turn has no approved workspace scope")?;
        if owner.workspace != request.binding.owner
            || owner.execution_cwd != request.binding.execution
        {
            return Err(
                "main execution cwd changed since scope approval; request kept paused".into(),
            );
        }
        let request =
            workspaces::for_attempt(&self.state_root, &request, &owner.run_id, &owner.attempt_id)?;
        let admission = if request.whole_workspace_write() && request.binding.managed.is_none() {
            workspaces::reserve_interactive(
                &self.state_root,
                &request,
                &owner.run_id,
                &owner.conversation_id,
                &owner.attempt_id,
            )?
        } else {
            workspaces::reserve(
                &self.state_root,
                &request,
                &owner.run_id,
                &owner.conversation_id,
                &owner.attempt_id,
            )?
        };
        match admission {
            Admission::Reserved => {
                self.scopes.task = Some((owner.run_id.clone(), request));
                self.scopes.active = Some((owner.clone(), Arc::new(std::sync::atomic::AtomicBool::new(false))));
                if !self.save_turn_checkpoint(false) {
                    let _ = workspaces::finish(&self.state_root, &owner.run_id, &owner.attempt_id, true);
                    self.scopes.active = None;
                    return Err("workspace admission was not durably checkpointed; provider was not started".into());
                }
                self.scopes.admission_failed = false;
                Ok(())
            }
            Admission::Conflict { run, path } => Err(format!("workspace path {} is reserved by run {run}; request remains paused", path.display())),
            Admission::ReconcileRequired => Err("old native process completion is uncertain; inspect it, then /scope stopped acknowledges it has stopped before explicit continuation".into()),
        }
    }

    pub(super) fn finish_main_scope(&mut self, outcome: &TurnOutcome) -> bool {
        let admission_failed = std::mem::take(&mut self.scopes.admission_failed);
        let Some((owner, invoked)) = self.scopes.active.take() else {
            return !admission_failed;
        };
        let not_started = !invoked.load(std::sync::atomic::Ordering::SeqCst);
        let confirmed = not_started
            || outcome.exit_code.is_some()
                && (outcome.cancelled
                    || !outcome
                        .cards
                        .iter()
                        .chain(&self.live_cards)
                        .any(|card| card.status == CardStatus::Running));
        match workspaces::finish(
            &self.state_root,
            &owner.run_id,
            &owner.attempt_id,
            confirmed,
        ) {
            Ok(()) => {
                if !confirmed {
                    self.push_system(if outcome.exit_code.is_some() {
                        "Native process exited before all tool completions were confirmed; workspace reservation retained. Inspect unfinished tool effects and any remaining child processes before /scope stopped and explicit continuation."
                    } else {
                        "Native process exit is uncertain; workspace reservation retained. Inspect the old process before /scope stopped and explicit continuation."
                    });
                }
                confirmed
            }
            Err(error) => {
                self.push_system(format!("workspace reservation could not be released: {error}; automatic continuation paused"));
                false
            }
        }
    }

    pub(super) fn scope_invocation_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.scopes
            .active
            .as_ref()
            .map(|(_, flag)| flag.clone())
            .unwrap_or_else(|| Arc::new(std::sync::atomic::AtomicBool::new(false)))
    }

}
