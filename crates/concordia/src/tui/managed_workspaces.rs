use super::*;
use crate::managed_workspaces as managed;
use std::sync::atomic::{AtomicBool, Ordering};

type Pending = (
    String,
    Arc<AtomicBool>,
    std::sync::mpsc::Receiver<Result<String, String>>,
);

fn lifecycle_action(name: &str) -> Result<managed::lifecycle::Action, String> {
    use managed::lifecycle::Action;
    match name {
        "integrate" => Ok(Action::Integrate),
        "pick" => Ok(Action::CherryPick),
        "retire" => Ok(Action::Retire),
        "delete-branch" => Ok(Action::DeleteBranch),
        _ => Err("choose integrate, pick, retire or delete-branch".into()),
    }
}

fn review(
    root: &Path,
    owner: &Path,
    id: &str,
    action: &str,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let preview = managed::lifecycle::preview(root, owner, id, lifecycle_action(action)?, cancel)?;
    let configured_checks = repository_checks(&preview.record.owner);
    Ok(format!("Managed workspace review: {:?} / {:?}\nOwner: {}\nWorktree: {}\nBase: {}\nOwner head: {} ({})\nWorker head: {}\nUnique worker commits: {}\nChanged paths:\n{}\nPath object identities (old/new/path):\n{}\nOwner status:\n{}\nWorker status:\n{}\nConflicts:\n{}\nWorker/result references:\n{}\nReference hash: {}\nRepository-configured checks (not run by Concordia):\n{}\nReservations:\n{}\nBlockers:\n{}\nReceipt: {}\nExact integration action: /worktree {action} {id} {}\nReview the provider's change view and the user's Git/editor before accepting. PR readiness remains in the user's Git workflow. No action is automatic. External Git/processes remain outside cooperative scheduler enforcement.",preview.action,preview.state,preview.record.owner.display(),preview.record.execution.display(),preview.record.base,preview.owner_head,preview.owner_branch,preview.worker_head,preview.unique_commits,preview.changes.join("\n"),preview.path_objects.join("\n"),preview.owner_status,preview.worker_status,preview.conflicts.join("\n"),preview.references.join("\n"),preview.reference_hash,configured_checks,preview.reservations.join("\n"),preview.blockers.join("\n"),preview.receipt,preview.receipt))
}

fn finished_summary(action: &str, id: &str, json: &str, owner: &Path) -> String {
    let value: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let field = |name: &str| {
        value
            .get(name)
            .and_then(|item| item.as_str())
            .unwrap_or("unavailable")
    };
    format!(
        "Managed {action} finished for {id}.\nOwner head before: {}\nOwner head after: {}\nReviewed worker head: {}\nReference hash: {}\nRepository-configured checks (not run by Concordia):\n{}\nChecks: {}\nPR readiness: {}\nInspect the saved workspace record for exact provenance.",
        field("owner_before"),
        field("owner_after"),
        field("worker_head"),
        field("reference_hash"),
        repository_checks(owner),
        field("checks"),
        field("pr_readiness"),
    )
}

fn repository_checks(owner: &Path) -> String {
    let workflows = owner.join(".github").join("workflows");
    let mut checks = Vec::new();
    let Ok(entries) = std::fs::read_dir(workflows) else {
        return "none discovered in repository configuration".into();
    };
    for entry in entries.flatten().take(32) {
        let path = entry.path();
        if !path.is_file()
            || !matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("yml" | "yaml")
            )
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        let relative = path
            .strip_prefix(owner)
            .unwrap_or(&path)
            .display()
            .to_string();
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            let Some(value) = trimmed
                .strip_prefix("- run:")
                .or_else(|| trimmed.strip_prefix("run:"))
            else {
                continue;
            };
            let value = value.trim();
            let indent = line.len() - trimmed.len();
            if matches!(value, "|" | ">" | "|-" | ">-") {
                for next in lines[index + 1..].iter().take(16) {
                    let body = next.trim();
                    let deeper = next.len() - next.trim_start().len() > indent;
                    if body.is_empty() {
                        continue;
                    }
                    if !deeper {
                        break;
                    }
                    if body.len() <= 512 {
                        checks.push(format!("{relative}: {body}"));
                    }
                }
            } else if !value.is_empty() && value.len() <= 512 {
                checks.push(format!("{relative}: {value}"));
            }
        }
    }
    if checks.is_empty() {
        "none discovered in repository configuration".into()
    } else {
        checks.join("\n")
    }
}
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
    let mut description=format!("Managed workspace {} revision {}: {:?}\nOwner: {}\nExecution: {}\nGit common directory: {}\nBase: {}\nBranch: {}\n{}\nWorktrees organize edits; native filesystem confinement is unverified. No integration or retirement is automatic.", record.id, record.revision, record.phase, record.owner.display(), record.execution.display(), record.common_git.display(), record.base, record.branch, record.note);
    if let Some(operation) = &record.operation {
        description.push_str(&format!(
            "\nLifecycle reservation runs: {}\nConversation: {}\nAttempt: {}",
            operation.runs.join(", "),
            operation.conversation,
            operation.attempt
        ));
    }
    description
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
            [] | ["list"]
                | ["create", _]
                | ["inspect", _]
                | ["reconcile", _]
                | ["stopped", _]
                | [
                    "review",
                    _,
                    "integrate" | "pick" | "retire" | "delete-branch"
                ]
                | ["integrate" | "pick" | "retire" | "delete-branch", _, _]
        ) {
            self.push_system("usage: /worktree list|create BASE|inspect ID|stopped ID|reconcile ID|review ID ACTION|ACTION ID RECEIPT|cancel. Actions: integrate, pick, retire, delete-branch. Review never grants execution; changed state requires a fresh receipt.");
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
                ["stopped", id] => managed::lifecycle::stopped(&root,&owner,id),
                ["review", id, action] => review(&root,&owner,id,action,&flag),
                [action @ ("integrate" | "pick" | "retire" | "delete-branch"), id, receipt] => lifecycle_action(action).and_then(|kind| managed::lifecycle::execute(&root,&owner,id,kind,receipt,&flag)).map(|json| finished_summary(action, id, &json, &owner)),
                ["create", base] => managed::create(&root, &owner, base, &flag).map(|record| describe(&record)),
                ["reconcile", id] => managed::reconcile(&root, &owner, id, &flag).map(|record| describe(&record)),
                ["inspect", id] => managed::load(&root).and_then(|registry| {
                    let record = registry.records.into_iter().find(|record| record.id == *id).ok_or("unknown managed workspace")?;
                    if owner.canonicalize().ok().as_ref() != Some(&record.owner) { return Err("managed workspace belongs to another owner".into()); }
                    let check = match record.phase {
                        managed::Phase::Ready => managed::verified_cancellable(&root, &owner, id, &flag).map(|_| "Current Git binding verified".into()).unwrap_or_else(|error| format!("Current binding unavailable: {error}; dispatch must pause")),
                        managed::Phase::Retired => "Retired by explicit action; saved provenance remains. No new dispatch is allowed.".into(),
                        _ if record.operation.is_some() => "Lifecycle action is retained/uncertain; inspect current heads before explicit reconciliation. No action was replayed.".into(),
                        _ => "Creation is uncertain/retained; inspect before explicit reconciliation. No mutation was replayed.".into(),
                    };
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

    pub(super) fn poll_managed_workspaces(&mut self) -> bool {
        let Some((conversation, cancel, rx)) = &self.managed_workspaces.pending else {
            return false;
        };
        let current = conversation == self.deleg.d.conversation_id();
        if !current {
            cancel.store(true, Ordering::SeqCst);
        }
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(_) => Err("managed workspace operation stopped; inspect any reserved creation before reconciliation".into()),
        };
        self.managed_workspaces.pending = None;
        if current {
            self.push_system(result.unwrap_or_else(|error| error));
        }
        current
    }
}
