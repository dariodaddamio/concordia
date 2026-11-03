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
    read: Option<(String, std::sync::mpsc::Receiver<Result<String, String>>)>,
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
        match workspaces::reserve(&self.state_root, &request, &owner.run_id, &owner.conversation_id, &owner.attempt_id)? {
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
                && !outcome
                    .cards
                    .iter()
                    .chain(&self.live_cards)
                    .any(|card| card.status == CardStatus::Running);
        match workspaces::finish(
            &self.state_root,
            &owner.run_id,
            &owner.attempt_id,
            confirmed,
        ) {
            Ok(()) => {
                if !confirmed {
                    self.push_system("Native process exit is uncertain; workspace reservation retained. Inspect the old process before /scope stopped and explicit continuation.");
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

    pub(super) fn scope_command(&mut self, argument: &str) {
        let result = if argument.is_empty() {
            Ok(match &self.scopes.task {
                Some((run, request)) => format!("Current run {run}: {}\n/scope set read:PATH write:PATH changes future turns; /scope default reserves the whole folder. /scope leases inspects owners. Claims coordinate scheduler admission and do not sandbox native tools.", request.describe()),
                None => "Unscoped turns reserve the whole execution folder. /scope set read:PATH write:PATH declares future scope; quote paths with spaces. /scope leases inspects owners. These claims do not sandbox native tools.".into(),
            })
        } else if argument == "default" {
            self.scopes.defaults.clear();
            Ok("Future main turns reserve the whole execution folder; current task scope is unchanged".into())
        } else if let Some(declarations) = argument.strip_prefix("set ") {
            workspaces::parse_declarations(declarations, false).and_then(|(declarations, _)| {
                let request = Request::shared(&self.cwd, &declarations)?;
                self.scopes.defaults = declarations;
                Ok(format!("Future main scope: {}. Current task scope is unchanged; native permissions remain authoritative.", request.describe()))
            })
        } else if argument == "stopped" || argument.starts_with("stopped ") {
            self.acknowledge_scope_stopped(argument.strip_prefix("stopped").unwrap_or("").trim())
        } else if argument == "leases" || argument.starts_with("leases ") {
            self.inspect_scope_leases(argument.strip_prefix("leases").unwrap_or("").trim());
            return;
        } else {
            Err(
                "usage: /scope [set read:PATH write:PATH|default|leases [OFFSET REVISION]|stopped]"
                    .into(),
            )
        };
        self.push_system(result.unwrap_or_else(|error| error));
    }

    fn acknowledge_scope_stopped(&mut self, requested: &str) -> Result<String, String> {
        if self.busy || self.scopes.active.is_some() {
            return Err(
                "stop or finish the active turn before acknowledging an old process".into(),
            );
        }
        let run = if requested.is_empty() {
            self.continuation.run_id()
        } else {
            requested
        };
        if !uuid::Uuid::parse_str(run).is_ok_and(|id| id.to_string() == run) {
            return Err("stopped-process acknowledgment needs the exact run UUID".into());
        }
        let registry = workspaces::load(&self.state_root)?;
        let entry = registry
            .reservations
            .iter()
            .find(|entry| entry.run == run)
            .ok_or("unknown workspace run")?;
        if self.cwd.canonicalize().ok().as_ref() != Some(&entry.request.binding.owner) {
            return Err("workspace run belongs to another owner workspace".into());
        }
        if entry.phase == workspaces::Phase::Held {
            let owned = self
                .scopes
                .task
                .as_ref()
                .is_some_and(|(id, request)| id == run && request.same_scope(&entry.request));
            if !owned {
                return Err(
                    "reservation is still held; recover its owning pane before reconciliation"
                        .into(),
                );
            }
            workspaces::recover(&self.state_root, run, &entry.request)?;
        }
        workspaces::acknowledge_stopped(&self.state_root, run, &entry.conversation)?;
        Ok("Stopped-process acknowledgment saved. No task was replayed; native process ownership remains unverified for retirement.".into())
    }

    fn inspect_scope_leases(&mut self, arguments: &str) {
        if self.scopes.read.is_some() {
            self.push_system("Workspace inspection is already running");
            return;
        }
        let parts: Vec<_> = arguments.split_whitespace().collect();
        let page = match parts.as_slice() {
            [] => Some((0, None)),
            [offset, revision] => offset
                .parse::<usize>()
                .ok()
                .zip(revision.parse::<u64>().ok())
                .map(|(offset, revision)| (offset, Some(revision))),
            _ => None,
        };
        let Some((offset, revision)) = page else {
            self.push_system("usage: /scope leases [OFFSET REVISION]");
            return;
        };
        let root = self.state_root.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.scopes.read = Some((self.deleg.d.conversation_id().into(), rx));
        std::thread::spawn(move || {
            let result = workspaces::load(&root).and_then(|registry| {
                if revision.is_some_and(|revision| revision != registry.revision) {
                    return Err("workspace reservations changed; restart the inspection".into());
                }
                if offset > registry.reservations.len() {
                    return Err("workspace reservation offset is outside this snapshot".into());
                }
                let mut lines = vec![format!(
                    "Workspace reservations revision {} (cooperative admission only)",
                    registry.revision
                )];
                let mut ordered: Vec<_> = registry.reservations.iter().collect();
                ordered.sort_by(|left, right| {
                    (left.phase == workspaces::Phase::Released, &left.run)
                        .cmp(&(right.phase == workspaces::Phase::Released, &right.run))
                });
                let mut shown = 0;
                let mut bytes = lines[0].len();
                for entry in ordered.into_iter().skip(offset).take(16) {
                    let cwd: String = entry
                        .request
                        .binding
                        .execution
                        .to_string_lossy()
                        .chars()
                        .take(180)
                        .collect();
                    let row = format!(
                        "run {} / attempt {}: {:?}; confirmed exit {}\ncwd {}{}\n{}",
                        entry.run,
                        entry.attempt,
                        entry.phase,
                        entry.confirmed_exit,
                        cwd,
                        if entry
                            .request
                            .binding
                            .execution
                            .to_string_lossy()
                            .chars()
                            .count()
                            > 180
                        {
                            "... (path display truncated)"
                        } else {
                            ""
                        },
                        entry.request.describe()
                    );
                    if bytes + row.len() > 30_000 {
                        break;
                    }
                    bytes += row.len();
                    shown += 1;
                    lines.push(row);
                }
                if offset + shown < registry.reservations.len() {
                    lines.push(format!(
                        "Partial list; /scope leases {} {}",
                        offset + shown,
                        registry.revision
                    ));
                }
                Ok(lines
                    .join("\n")
                    .chars()
                    .filter(|character| !character.is_control() || *character == '\n')
                    .collect())
            });
            let _ = tx.send(result);
        });
        self.push_system("Reading workspace reservations");
    }

    pub(super) fn poll_scope_inspection(&mut self) {
        let Some((conversation, receiver)) = &self.scopes.read else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("workspace inspection stopped".into()),
        };
        let current = conversation == self.deleg.d.conversation_id();
        self.scopes.read = None;
        if current {
            self.push_system(result.unwrap_or_else(|error| error));
        }
    }
}
