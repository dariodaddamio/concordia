//! Live worker requests keep their exact launch authority; recovery never stores grants/channels.
use super::*;
use crate::provider::interaction::{InteractionOwner, NativeEnvelope, NativeRequest};

const PER_RUN: usize = 8;
// Reserve the remaining eight app slots for the foreground connection.
const ALL_WORKERS: usize = 24;
const BYTES_PER_RUN: usize = 256 * 1024;

#[derive(Default)]
pub(super) struct State {
    owner: Option<InteractionOwner>,
    connection: Option<String>,
    pending: Vec<Arc<NativeRequest>>,
    uncertain: bool,
    prior: Option<WorkerState>,
}

impl State {
    fn refresh(&mut self) {
        self.pending.retain(|request| {
            if request.is_active() {
                true
            } else {
                self.uncertain |= !request.was_answered() && !request.was_resolved();
                false
            }
        });
    }
    pub(super) fn clear(&mut self) {
        for request in self.pending.drain(..) {
            request.invalidate();
        }
    }
    pub(super) fn unresolved(&self) -> bool {
        self.uncertain
            || self
                .pending
                .iter()
                .any(|request| !request.was_answered() && !request.was_resolved())
    }
    pub(super) fn observed(&self) -> bool {
        self.connection.is_some()
    }
}

impl Job {
    pub fn native_inputs(&self) -> Vec<Arc<NativeRequest>> {
        self.input
            .pending
            .iter()
            .filter(|request| {
                self.native_owner()
                    .is_some_and(|owner| request.is_owned_by(owner))
            })
            .cloned()
            .collect()
    }
    pub fn native_owner(&self) -> Option<&InteractionOwner> {
        let owner = self.input.owner.as_ref()?;
        (self.state.is_running()
            && !self.cancel_requested
            && owner.run_id == self.run_id
            && owner.conversation_id == self.conversation_id
            && owner.account == self.account
            && owner.provider == self.provider
            && self
                .attempt_records
                .last()
                .and_then(|attempt| attempt.id.as_deref())
                == Some(owner.attempt_id.as_str()))
        .then_some(owner)
    }
    pub(super) fn refresh_native_input(&mut self) {
        self.input.refresh();
        if self.state.is_running() {
            if self.native_inputs().is_empty() {
                if self.state == WorkerState::WaitingInput {
                    self.state = self.input.prior.take().unwrap_or(WorkerState::Thinking);
                }
            } else {
                if self.state != WorkerState::WaitingInput {
                    self.input.prior = Some(self.state);
                }
                self.state = WorkerState::WaitingInput;
                self.snippet = format!(
                    "{} native request(s) waiting; /delegate input {} reviews this attempt",
                    self.input.pending.len(),
                    self.id
                );
            }
        }
    }
    pub(super) fn bind_native_input(&mut self) -> Result<Option<InteractionOwner>, String> {
        self.input.clear();
        self.input = State::default();
        // Low-level callers without a registered home retain their legacy noninteractive path.
        if self.spec.home.is_none() {
            return Ok(None);
        }
        let owner = InteractionOwner {
            conversation_id: self.conversation_id.clone(),
            run_id: self.run_id.clone(),
            attempt_id: self
                .attempt_records
                .last()
                .and_then(|attempt| attempt.id.clone())
                .ok_or("worker launch has no attempt identity")?,
            account: self.account.clone(),
            provider: self.provider,
            home: self.spec.home.clone(),
            workspace: self
                .workspace
                .as_ref()
                .map(|request| request.binding.owner.clone())
                .unwrap_or_else(|| self.spec.cwd.clone()),
            execution_cwd: self.spec.cwd.clone(),
        }
        .validate_for(self.provider, &self.spec.cwd, self.spec.home.as_deref())?;
        self.input.owner = Some(owner.clone());
        Ok(Some(owner))
    }
}

impl Delegates {
    pub(super) fn accept_native_input(&mut self, id: u32, request: Box<NativeRequest>) {
        for job in &mut self.jobs {
            job.refresh_native_input();
        }
        let total: usize = self.jobs.iter().map(|job| job.native_inputs().len()).sum();
        let Some(job) = self.get_mut(id) else {
            request.invalidate();
            return;
        };
        let valid = job
            .native_owner()
            .is_some_and(|owner| request.is_owned_by(owner))
            && job
                .input
                .connection
                .as_ref()
                .is_none_or(|connection| *connection == request.envelope().connection_id)
            && job
                .session_id
                .as_ref()
                .is_some_and(|session| *session == request.envelope().native_session_id);
        if !valid {
            request.invalidate();
            job.snippet = "Native worker request withheld: launch/session identity mismatch".into();
            return;
        }
        if job
            .input
            .pending
            .iter()
            .any(|old| old.envelope() == request.envelope())
        {
            return;
        }
        let bytes = job
            .input
            .pending
            .iter()
            .map(|request| {
                serde_json::to_vec(&request.params).map_or(BYTES_PER_RUN + 1, |bytes| bytes.len())
            })
            .sum::<usize>();
        if total >= ALL_WORKERS
            || job.input.pending.len() >= PER_RUN
            || bytes.saturating_add(
                serde_json::to_vec(&request.params).map_or(BYTES_PER_RUN + 1, |bytes| bytes.len()),
            ) > BYTES_PER_RUN
        {
            request.cancel();
            job.snippet = "Native worker request declined: pending input budget reached".into();
            return;
        }
        job.input.connection = Some(request.envelope().connection_id.clone());
        job.input.pending.push(Arc::from(request));
        job.refresh_native_input();
    }

    pub fn answer_native_input(
        &mut self,
        id: u32,
        envelope: &NativeEnvelope,
        answer: serde_json::Value,
    ) -> Result<(), String> {
        let job = self.get_mut(id).ok_or("worker no longer exists")?;
        let owner = job
            .native_owner()
            .ok_or("worker attempt is no longer active")?
            .clone();
        let request = job
            .input
            .pending
            .iter()
            .find(|request| request.envelope() == envelope)
            .ok_or("native request is no longer pending")?;
        request.respond_owned(&owner, answer)?;
        job.refresh_native_input();
        self.persist_recovery();
        Ok(())
    }
}
