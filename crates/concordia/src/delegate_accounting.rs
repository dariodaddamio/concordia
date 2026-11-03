//! Attempt usage is durable metadata; recovery may reconcile accounting, never native execution.
use super::*;
use crate::token_log::{Attribution, Entry, Origin as UsageOrigin};

pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn pending(job: &Job) -> bool {
    job.attempt_records.iter().any(|attempt| {
        attempt.id.is_some()
            && attempt.started_at.is_some()
            && attempt.outcome.is_some()
            && !attempt.accounted
    })
}

impl Delegates {
    pub(super) fn finish_worker_attempt(&mut self, id: u32, outcome: &TurnOutcome) {
        if let Some(job) = self.get_mut(id) {
            if let Some(attempt) = job.attempt_records.last_mut() {
                if attempt.started_at.is_some() && attempt.outcome.is_none() {
                    let (status, complete) = crate::token_log::attempt_outcome(
                        outcome.exit_code,
                        outcome.cancelled,
                        outcome.failed,
                    );
                    attempt.outcome = Some(status.into());
                    attempt.complete = complete;
                    attempt.ended_at = Some(now());
                    if outcome.session_id.is_some() {
                        attempt.native_session = outcome.session_id.clone();
                    }
                }
            }
        }
        if let Err(error) = self.save_recovery() {
            self.recovery_error = Some(format!("attempt usage checkpoint failed: {error}"));
        }
        self.flush_accounting();
        self.persist_recovery();
    }

    pub(super) fn reconcile_interrupted_accounting(&mut self) {
        for job in &mut self.jobs {
            for attempt in &mut job.attempt_records {
                if attempt.id.is_some() && attempt.started_at.is_some() {
                    // Reconcile the immutable event again after a crash/lost acknowledgement.
                    attempt.accounted = false;
                }
                if attempt.id.is_some() && attempt.started_at.is_some() && attempt.outcome.is_none()
                {
                    attempt.outcome = Some("interrupted".into());
                    attempt.complete = false;
                    attempt.ended_at = Some(now());
                }
            }
        }
    }

    pub(super) fn flush_accounting(&mut self) {
        let Some(root) = self
            .recovery
            .as_ref()
            .map(|recovery| recovery.state_root.clone())
        else {
            return;
        };
        for job in &mut self.jobs {
            for attempt in &mut job.attempt_records {
                let (Some(id), Some(_), Some(at), Some(outcome)) = (
                    &attempt.id,
                    attempt.started_at,
                    attempt.ended_at,
                    &attempt.outcome,
                ) else {
                    continue;
                };
                if attempt.accounted {
                    continue;
                }
                let (new_input, cached_input, output) =
                    crate::token_log::split(attempt.provider, &attempt.usage);
                let mut attribution = Attribution::new(
                    UsageOrigin::Worker,
                    &job.run_id,
                    id,
                    None,
                    &attempt.usage,
                    outcome,
                    attempt.complete,
                );
                attribution.home_id = attempt.home_id.clone();
                attribution.native_source = attempt.source.clone();
                let entry = Entry {
                    at,
                    provider: attempt.provider.as_str().into(),
                    account: attempt.account.clone(),
                    model: attempt.model.clone(),
                    project: job.spec.cwd.to_string_lossy().into_owned(),
                    chat: attempt.native_session.clone(),
                    new_input,
                    cached_input,
                    output,
                    cost_usd: attempt.usage.cost_usd,
                    attribution: Some(attribution),
                };
                match crate::token_log::record(&root, &entry) {
                    Ok(()) => attempt.accounted = true,
                    Err(error) => {
                        self.recovery_error = Some(format!(
                            "usage could not be recorded; attempt metadata retained: {error}"
                        ))
                    }
                }
            }
        }
    }
}

impl Job {
    pub fn usage_summary(&self) -> String {
        if self.attempt_records.is_empty() {
            return "Usage: legacy attribution unavailable".into();
        }
        self.attempt_records
            .iter()
            .map(|attempt| {
                let counter = |value: Option<u64>| {
                    value
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".into())
                };
                format!(
                    "{} attempt {}: input {}, cached {}, output {}, cost {}; {}",
                    attempt.account,
                    attempt.id.as_deref().unwrap_or("legacy/unknown"),
                    counter(attempt.usage.input),
                    counter(attempt.usage.cached),
                    counter(attempt.usage.output),
                    attempt
                        .usage
                        .cost_usd
                        .map(|cost| format!("reported ${cost:.4}"))
                        .unwrap_or_else(|| "unknown".into()),
                    attempt.outcome.as_deref().unwrap_or("not finalized")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
