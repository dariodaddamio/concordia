//! One bounded background Git operation, tied to the requesting conversation.
use super::*;
use crate::managed_workspaces as managed;
use std::sync::atomic::{AtomicBool, Ordering};

type Pending = (
    String,
    Arc<AtomicBool>,
    std::sync::mpsc::Receiver<Result<String, String>>,
);
#[derive(Default)]
pub(super) struct State {
    pending: Option<Pending>,
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some((_, cancel, _)) = &self.pending {
            cancel.store(true, Ordering::SeqCst);
        }
    }
}

fn describe(record: &managed::Record) -> String {
    format!("Managed workspace {} revision {}: {:?}\nOwner: {}\nExecution: {}\nGit common directory: {}\nBase: {}\nBranch: {}\n{}\nWorktrees organize edits; native filesystem confinement is unverified. No integration or retirement is automatic.", record.id, record.revision, record.phase, record.owner.display(), record.execution.display(), record.common_git.display(), record.base, record.branch, record.note)
}

impl App {
    pub(super) fn managed_workspace_command(&mut self, argument: &str) {
        if argument == "cancel" {
            if let Some((_, cancel, _)) = &self.managed_workspaces.pending {
                cancel.store(true, Ordering::SeqCst);
                self.push_system("Cancelling the owned Git operation; any reserved creation remains inspectable and is never replayed automatically");
            } else {
                self.push_system("No managed Git operation is running");
            }
            return;
        }
        if self.managed_workspaces.pending.is_some() {
            self.push_system("A managed workspace operation is already running; /worktree cancel interrupts that operation");
            return;
        }
        let parts: Vec<_> = argument.split_whitespace().collect();
        if !matches!(
            parts.as_slice(),
            [] | ["list"] | ["create", _] | ["inspect", _] | ["reconcile", _]
        ) {
            self.push_system("usage: /worktree [list|create BASE|inspect ID|reconcile ID|cancel]. Creation requires a clean committed repository root; no uncommitted edits are copied.");
            return;
        }
        let command = argument.to_owned();
        let root = self.state_root.clone();
        let owner = self.cwd.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.managed_workspaces.pending = Some((self.deleg.d.conversation_id().into(), cancel, rx));
        std::thread::spawn(move || {
            let parts: Vec<_> = command.split_whitespace().collect();
            let result = match parts.as_slice() {
                ["create", base] => managed::create(&root, &owner, base, &flag).map(|record| describe(&record)),
                ["reconcile", id] => managed::reconcile(&root, &owner, id, &flag).map(|record| describe(&record)),
                ["inspect", id] => managed::load(&root).and_then(|registry| {
                    let record = registry.records.into_iter().find(|record| record.id == *id).ok_or("unknown managed workspace")?;
                    if owner.canonicalize().ok().as_ref() != Some(&record.owner) { return Err("managed workspace belongs to another owner".into()); }
                    let check = if record.phase == managed::Phase::Ready { managed::verified_cancellable(&root, &owner, id, &flag).map(|_| "Current Git binding verified".into()).unwrap_or_else(|error| format!("Current binding unavailable: {error}; dispatch must pause")) } else { "Creation is uncertain/retained; inspect before explicit reconciliation. No mutation was replayed.".into() };
                    Ok(format!("{}\n{check}", describe(&record)))
                }),
                _ => managed::load(&root).map(|registry| {
                    let owner = owner.canonicalize().ok();
                    let mut lines = vec![format!("Managed workspaces revision {} (up to 64 retained records)", registry.revision)];
                    for record in registry.records.iter().filter(|record| Some(&record.owner) == owner.as_ref()) {
                        lines.push(format!("{} {:?} / base {} /worktree inspect {}", record.id, record.phase, record.base, record.id));
                    }
                    if lines.len() == 1 { lines.push("No managed workspaces for this owner. /worktree create BASE explicitly creates one from a clean committed base.".into()); }
                    lines.join("\n")
                }),
            };
            let _ = tx.send(result.map(|text| {
                text.chars()
                    .filter(|ch| !ch.is_control() || *ch == '\n')
                    .collect()
            }));
        });
        self.push_system(
            "Managed workspace operation started; native provider turns were not requested",
        );
    }

    pub(super) fn poll_managed_workspaces(&mut self) {
        let Some((conversation, cancel, rx)) = &self.managed_workspaces.pending else {
            return;
        };
        let current = conversation == self.deleg.d.conversation_id();
        if !current {
            cancel.store(true, Ordering::SeqCst);
        }
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("managed workspace operation stopped; inspect any reserved creation before reconciliation".into()),
        };
        self.managed_workspaces.pending = None;
        if current {
            self.push_system(result.unwrap_or_else(|error| error));
        }
    }
}
