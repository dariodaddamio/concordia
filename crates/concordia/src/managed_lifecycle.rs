//! Explicit, revision-bound native Git integration and retirement. Preview never grants action.
use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Integrate,
    CherryPick,
    Retire,
    DeleteBranch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub action: Action,
    pub owner_head: String,
    pub worker_head: String,
    pub expected_tree: Option<String>,
    pub receipt: String,
    pub finished: bool,
    pub runs: Vec<String>,
    pub conversation: String,
    pub attempt: String,
}
impl Operation {
    pub(super) fn valid(&self) -> bool {
        object_id(&self.owner_head)
            && object_id(&self.worker_head)
            && self
                .expected_tree
                .as_ref()
                .is_none_or(|tree| object_id(tree))
            && self.receipt.len() == 64
            && self.receipt.bytes().all(|byte| byte.is_ascii_hexdigit())
            && (1..=2).contains(&self.runs.len())
            && self.runs.iter().all(|run| uuid(run))
            && uuid(&self.conversation)
            && uuid(&self.attempt)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Active,
    Paused,
    ReviewRequired,
    Retained,
    Retirable,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub action: Action,
    pub state: State,
    pub record: Record,
    pub owner_head: String,
    pub owner_branch: String,
    pub worker_head: String,
    pub owner_status: String,
    pub worker_status: String,
    pub changes: Vec<String>,
    pub conflicts: Vec<String>,
    pub expected_tree: Option<String>,
    pub unique_commits: u64,
    pub references: Vec<String>,
    pub reservations: Vec<String>,
    pub blockers: Vec<String>,
    pub receipt: String,
}

fn head(git: &Git<'_>, path: &Path, revision: &str) -> Result<String, String> {
    let value = git
        .run(path, &["rev-parse", "--verify", revision], false)?
        .trim()
        .to_owned();
    if !object_id(&value) {
        return Err("unsupported Git commit identity".into());
    }
    Ok(value)
}

fn clean(git: &Git<'_>, path: &Path) -> Result<String, String> {
    git.run(
        path,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignored=matching",
        ],
        false,
    )
}

fn edited(status: &str) -> bool {
    status.lines().any(|line| !line.starts_with("!! "))
}

fn idle_git(git: &Git<'_>, path: &Path) -> Result<(), String> {
    for marker in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
        "sequencer",
        "index.lock",
    ] {
        let marker = git.run(
            path,
            &["rev-parse", "--path-format=absolute", "--git-path", marker],
            false,
        )?;
        if Path::new(marker.trim()).exists() {
            return Err(
                "native Git operation/lock is already present; retain and reconcile".into(),
            );
        }
    }
    Ok(())
}

fn safe_tree(git: &Git<'_>, owner: &Path, revision: &str) -> Result<(), String> {
    let entries = git.run(owner, &["ls-tree", "-rz", revision], false)?;
    for entry in entries.split('\0').filter(|entry| !entry.is_empty()) {
        let (metadata, path) = entry
            .split_once('\t')
            .ok_or("invalid native tree metadata")?;
        if !(metadata.starts_with("100644 blob ") || metadata.starts_with("100755 blob "))
            || !crate::workspaces::relative_valid(Path::new(path))
            || Path::new(path).is_absolute()
        {
            return Err("integration tree contains linked, sensitive or unsupported paths; retain for native review".into());
        }
    }
    Ok(())
}

fn verify_owner(git: &Git<'_>, record: &Record) -> Result<(), String> {
    if record.owner.canonicalize().ok().as_ref() != Some(&record.owner)
        || crate::workspaces::directory_id(&record.owner)? != record.owner_identity
        || crate::workspaces::directory_id(&record.common_git)? != record.common_identity
        || git_path(git, &record.owner, "--show-toplevel")? != record.owner
        || git_path(git, &record.owner, "--git-common-dir")? != record.common_git
    {
        return Err("managed repository identity changed; retain workspace".into());
    }
    Ok(())
}

fn preview_with(
    root: &Path,
    owner: &Path,
    id: &str,
    action: Action,
    git: &Git<'_>,
) -> Result<Preview, String> {
    let record = load(root)?
        .records
        .into_iter()
        .find(|record| record.id == id)
        .ok_or("unknown managed workspace")?;
    if owner.canonicalize().ok().as_ref() != Some(&record.owner) {
        return Err("managed workspace belongs to another owner".into());
    }
    verify_owner(git, &record)?;
    let owner_head = head(git, &record.owner, "HEAD^{commit}")?;
    let owner_branch = git
        .run(&record.owner, &["symbolic-ref", "--quiet", "HEAD"], false)?
        .trim()
        .to_owned();
    let worker_head = head(
        git,
        &record.owner,
        &format!("refs/heads/{}^{{commit}}", record.branch),
    )?;
    let mut preview = Preview {
        action,
        state: State::Retained,
        record,
        owner_head,
        owner_branch,
        worker_head,
        owner_status: String::new(),
        worker_status: String::new(),
        changes: Vec::new(),
        conflicts: Vec::new(),
        expected_tree: None,
        unique_commits: 0,
        references: Vec::new(),
        reservations: Vec::new(),
        blockers: Vec::new(),
        receipt: String::new(),
    };
    if preview.owner_branch == format!("refs/heads/{}", preview.record.branch) {
        return Err("owner checked out the managed branch; action refused".into());
    }
    idle_git(git, &preview.record.owner)?;
    preview.owner_status = clean(git, &preview.record.owner)?;
    if edited(&preview.owner_status) {
        preview
            .blockers
            .push("owner has tracked or untracked edits; keep all work".into());
    }
    if action == Action::DeleteBranch {
        if preview.record.phase != Phase::Retired || preview.record.execution.exists() {
            preview
                .blockers
                .push("worktree must be explicitly retired before separate branch deletion".into());
        }
        if preview
            .record
            .operation
            .as_ref()
            .is_none_or(|operation| operation.worker_head != preview.worker_head)
        {
            preview.blockers.push(
                "managed branch changed after retirement; retain for native inspection".into(),
            );
        }
    } else {
        if preview.record.phase != Phase::Ready {
            preview
                .blockers
                .push("workspace is retained/uncertain; explicit reconciliation required".into());
        }
        verify_record(git, &preview.record)?;
        idle_git(git, &preview.record.execution)?;
        preview.worker_status = clean(git, &preview.record.execution)?;
        if edited(&preview.worker_status)
            || action == Action::Retire && !preview.worker_status.is_empty()
        {
            preview.blockers.push("worker has tracked, untracked or ignored files; commit/review them before lifecycle changes".into());
        }
    }
    preview.unique_commits = git
        .run(
            &preview.record.owner,
            &[
                "rev-list",
                "--count",
                &format!("{}..{}", preview.owner_head, preview.worker_head),
            ],
            false,
        )?
        .trim()
        .parse()
        .map_err(|_| "invalid native commit count")?;
    let registrations = git.run(
        &preview.record.owner,
        &["worktree", "list", "--porcelain", "-z"],
        false,
    )?;
    let expected_path =
        crate::workspaces::path_key(Path::new(path_text(&preview.record.execution)?));
    let mut target = false;
    for field in registrations.split('\0') {
        if let Some(path) = field.strip_prefix("worktree ") {
            target = crate::workspaces::same_key(
                &crate::workspaces::path_key(Path::new(path)),
                &expected_path,
            );
        }
        if action == Action::Retire && target && field.starts_with("locked") {
            preview
                .blockers
                .push("native worktree is locked; retain until explicitly unlocked".into());
        }
        if action == Action::DeleteBranch
            && field == format!("branch refs/heads/{}", preview.record.branch)
        {
            preview
                .blockers
                .push("managed branch is checked out in another worktree; retain it".into());
        }
    }
    preview.changes = git
        .run(
            &preview.record.owner,
            &[
                "diff",
                "--no-ext-diff",
                "--name-status",
                &preview.record.base,
                &preview.worker_head,
                "--",
            ],
            false,
        )?
        .lines()
        .map(str::to_owned)
        .collect();
    if preview.changes.len() > 512 {
        return Err("changed path budget exceeded; retain for native review".into());
    }
    let (references, blockers) = crate::delegate::managed_references(root, id)?;
    preview.references = references;
    preview.blockers.extend(blockers);
    let paths = if action == Action::DeleteBranch {
        vec![preview.record.owner.as_path()]
    } else {
        vec![
            preview.record.owner.as_path(),
            preview.record.execution.as_path(),
        ]
    };
    let scopes = paths
        .iter()
        .map(|path| crate::workspaces::Request::shared(path, &[]))
        .collect::<Result<Vec<_>, _>>()?;
    for reservation in crate::workspaces::load(root)?.reservations {
        if reservation.phase != crate::workspaces::Phase::Released
            && scopes.iter().any(|scope| {
                scope.claims.iter().any(|claim| {
                    reservation
                        .request
                        .claims
                        .iter()
                        .any(|other| crate::workspaces::intersects(claim, other))
                })
            })
        {
            preview
                .reservations
                .push(format!("run {} / {:?}", reservation.run, reservation.phase));
        }
    }
    if !preview.reservations.is_empty() {
        preview
            .blockers
            .push("active or uncertain workspace reservations remain".into());
    }
    match action {
        Action::Integrate | Action::CherryPick => {
            if preview.blockers.is_empty() {
                let drivers = git.run(
                    &preview.record.owner,
                    &[
                        "config",
                        "--name-only",
                        "--get-regexp",
                        "^merge\\..*\\.driver$",
                    ],
                    true,
                )?;
                if !drivers.trim().is_empty() {
                    return Err("custom native merge drivers are configured; review through native Git before integration".into());
                }
                let parent = if action == Action::CherryPick {
                    Some(head(
                        git,
                        &preview.record.owner,
                        &format!("{}^", preview.worker_head),
                    )?)
                } else {
                    None
                };
                if action == Action::CherryPick
                    && git
                        .run(
                            &preview.record.owner,
                            &["rev-list", "--parents", "-n", "1", &preview.worker_head],
                            false,
                        )?
                        .split_whitespace()
                        .count()
                        != 2
                {
                    return Err("pick requires a reviewed non-merge HEAD commit".into());
                }
                safe_tree(git, &preview.record.owner, &preview.worker_head)?;
                let mut argv = vec!["merge-tree", "--write-tree", "--name-only"];
                let base = parent
                    .as_ref()
                    .map(|parent| format!("--merge-base={parent}"));
                if let Some(base) = base.as_ref() {
                    argv.push(base);
                }
                argv.extend([preview.owner_head.as_str(), preview.worker_head.as_str()]);
                let tree = git.run(&preview.record.owner, &argv, true)?;
                let mut lines = tree.lines();
                let tree = lines
                    .next()
                    .filter(|tree| object_id(tree))
                    .ok_or("native merge preview unavailable; no integration authorized")?;
                preview.expected_tree = Some(tree.to_owned());
                preview.conflicts = lines
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect();
                if !preview.conflicts.is_empty() {
                    preview.blockers.push(
                        "native merge reports conflicts; resolve explicitly before a new preview"
                            .into(),
                    );
                }
                if preview.conflicts.is_empty() {
                    safe_tree(git, &preview.record.owner, tree)?;
                }
            }
        }
        Action::Retire | Action::DeleteBranch => {
            if preview.unique_commits != 0 {
                preview.blockers.push(
                    "managed branch has commits absent from the owner; retain until integrated"
                        .into(),
                );
            }
        }
    }
    preview.state = if preview.record.phase == Phase::Retained {
        State::Retained
    } else if !preview.reservations.is_empty() {
        State::Active
    } else if !preview.blockers.is_empty() {
        State::Paused
    } else if matches!(action, Action::Integrate | Action::CherryPick) {
        State::ReviewRequired
    } else {
        State::Retirable
    };
    let bytes = serde_json::to_vec(&preview).map_err(|error| error.to_string())?;
    preview.receipt = format!("{:x}", Sha256::digest(bytes));
    Ok(preview)
}

pub fn preview(
    root: &Path,
    owner: &Path,
    id: &str,
    action: Action,
    cancel: &AtomicBool,
) -> Result<Preview, String> {
    let mut git = Git::new(cancel)?;
    git.protect_checkout(owner, &root_path(root)?)?;
    preview_with(root, owner, id, action, &git)
}

pub fn execute(
    root: &Path,
    owner: &Path,
    id: &str,
    action: Action,
    receipt: &str,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if receipt.len() != 64 || !receipt.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("action needs the exact preview receipt".into());
    }
    let root = root_path(root)?;
    let lock = root.join("managed-workspaces.lock");
    regular(&lock)?;
    crate::state_io::with_lock(&lock, || {
        let mut git = Git::new(cancel).map_err(std::io::Error::other)?;
        git.protect_checkout(owner,&root).map_err(std::io::Error::other)?;
        let before = preview_with(&root,owner,id,action,&git).map_err(std::io::Error::other)?;
        let paths = if action == Action::DeleteBranch { vec![before.record.owner.as_path()] } else { vec![before.record.owner.as_path(),before.record.execution.as_path()] };
        crate::workspaces::with_workspace_guard(&root,&paths,|guard| {
            let current = preview_with(&root,owner,id,action,&git)?;
            if current.receipt != receipt || before.receipt != receipt { return Err("workspace review changed; obtain and review a new preview".into()); }
            if !current.blockers.is_empty() { return Err(format!("workspace retained: {}", current.blockers.join("; "))); }
            let mut registry = load(&root)?;
            let index = registry.records.iter().position(|record|record.id==id).ok_or("managed record disappeared")?;
            registry.records[index].phase=Phase::Retained;
            let runs=paths.iter().map(|_|uuid::Uuid::new_v4().to_string()).collect::<Vec<_>>();
            let conversation=uuid::Uuid::new_v4().to_string();
            let attempt=uuid::Uuid::new_v4().to_string();
            registry.records[index].operation=Some(Operation { action, owner_head:current.owner_head.clone(), worker_head:current.worker_head.clone(), expected_tree:current.expected_tree.clone(), receipt:current.receipt.clone(), finished:false, runs:runs.clone(), conversation:conversation.clone(), attempt:attempt.clone() });
            registry.records[index].revision=registry.records[index].revision.checked_add(1).ok_or("managed revision overflow")?;
            registry.records[index].note=format!("Lifecycle {:?} reserved for owner {} and worker {}. Interrupted actions require explicit reconciliation; never replay automatically.",action,current.owner_head,current.worker_head);
            save(&root,&mut registry)?;
            guard.reserve(&runs,&conversation,&attempt)?;
            let result = match action {
                Action::Integrate => git.run(&current.record.owner,&["merge","--no-edit","--no-gpg-sign","--no-autostash","--no-overwrite-ignore","--no-stat","--",&current.worker_head],false),
                Action::CherryPick => git.run(&current.record.owner,&["cherry-pick","--no-gpg-sign","-x",&current.worker_head],false),
                Action::Retire => git.run(&current.record.owner,&["worktree","remove","--",path_text(&current.record.execution)?],false),
                Action::DeleteBranch => git.run(&current.record.owner,&["branch","-d","--",&current.record.branch],false),
            };
            let result=result.and_then(|output| {
                if let Some(expected)=&current.expected_tree {
                    if head(&git,&current.record.owner,"HEAD^{tree}")? != *expected { return Err("integration result differs from the reviewed native tree; retain for inspection".into()); }
                }
                if action==Action::Retire && current.record.execution.exists() { return Err("native Git reported retirement but the path remains; retain for inspection".into()); }
                Ok(output)
            });
            let confirmed=git.confirmed_exit.get();
            registry.records[index].revision=registry.records[index].revision.checked_add(1).ok_or("managed revision overflow")?;
            registry.records[index].phase=if result.is_ok() { match action { Action::Integrate | Action::CherryPick=>Phase::Ready, _=>Phase::Retired } } else { Phase::Retained };
            registry.records[index].operation.as_mut().unwrap().finished=result.is_ok();
            registry.records[index].note=match &result {
                Ok(_) => format!("Explicit {:?} finished for reviewed owner {} / worker {}. Branch deletion is separate; historical run metadata retained.",action,current.owner_head,current.worker_head),
                Err(error) => format!("Lifecycle {:?} retained after interruption/error: {}", action,error.chars().take(2000).collect::<String>()),
            };
            save(&root,&mut registry)?;
            guard.finish(&runs,&attempt,confirmed)?;
            result?;
            Ok(json!({"action":action,"workspace":id,"status":"finished","owner_before":current.owner_head,"worker_head":current.worker_head,"record":registry.records[index]}).to_string())
        }).map_err(std::io::Error::other)
    }).map_err(|error|error.to_string())
}

pub(super) fn reconcile_retained(
    root: &Path,
    record: &Record,
    cancel: &AtomicBool,
) -> Result<Record, String> {
    let root = root_path(root)?;
    let lock = root.join("managed-workspaces.lock");
    regular(&lock)?;
    crate::state_io::with_lock(&lock, || {
    let paths=if record.execution.exists() { vec![record.owner.as_path(),record.execution.as_path()] } else { vec![record.owner.as_path()] };
    crate::workspaces::with_workspace_guard(&root,&paths,|_guard| {
    let mut git=Git::new(cancel)?;
    git.protect_checkout(&record.owner,&root)?;
    verify_owner(&git,record)?;
    idle_git(&git,&record.owner)?;
    if edited(&clean(&git,&record.owner)?) { return Err("owner has edits; retain before reconciliation".into()); }
    let (_,blockers)=crate::delegate::managed_references(&root,&record.id)?;
    if !blockers.is_empty() { return Err(blockers.join("; ")); }
    let operation=record.operation.as_ref().ok_or("missing lifecycle operation")?;
    let mut next=record.clone();
    if record.execution.exists() {
        verify_record(&git,record)?;
        idle_git(&git,&record.execution)?;
        if edited(&clean(&git,&record.execution)?) { return Err("worker edits remain; retain before reconciliation".into()); }
        next.phase=Phase::Ready;
    } else {
        if !matches!(operation.action,Action::Retire | Action::DeleteBranch) { return Err("integration workspace is missing; retained for native inspection".into()); }
        let unique=git.run(&record.owner,&["rev-list","--count",&format!("HEAD..{}",operation.worker_head)],false)?;
        if unique.trim()!="0" { return Err("reviewed commits remain unique; retirement cannot be reconciled".into()); }
        let list=git.run(&record.owner,&["worktree","list","--porcelain","-z"],false)?;
        let key=crate::workspaces::path_key(Path::new(path_text(&record.execution)?));
        if list.split('\0').filter_map(|line|line.strip_prefix("worktree ")).any(|path|crate::workspaces::same_key(&crate::workspaces::path_key(Path::new(path)),&key)) { return Err("native worktree registration remains; reconcile through native Git".into()); }
        next.phase=Phase::Retired;
    }
        let mut registry=load(&root)?;
        let saved=registry.records.iter_mut().find(|saved|**saved==*record).ok_or("lifecycle changed during reconciliation")?;
        next.revision=next.revision.checked_add(1).ok_or("managed revision overflow")?;
        next.note="Explicitly reconciled current native Git state; no merge, pick, removal or branch deletion was replayed. Inspect heads before a new action.".into();
        *saved=next.clone();
        save(&root,&mut registry)?;
        Ok(next.clone())
    }).map_err(std::io::Error::other)
    }).map_err(|error|error.to_string())
}

/// Explicit user declaration after inspecting a stopped native process. Never infers exit.
pub fn stopped(root: &Path, owner: &Path, id: &str) -> Result<String, String> {
    let root = root_path(root)?;
    let lock = root.join("managed-workspaces.lock");
    regular(&lock)?;
    crate::state_io::with_lock(&lock, || {
        let record=load(&root).map_err(std::io::Error::other)?.records.into_iter().find(|record|record.id==id).ok_or_else(||std::io::Error::other("unknown managed workspace"))?;
        if owner.canonicalize().ok().as_ref()!=Some(&record.owner) || crate::workspaces::directory_id(&record.owner).map_err(std::io::Error::other)?!=record.owner_identity { return Err(std::io::Error::other("lifecycle belongs to another owner")); }
        let operation=record.operation.ok_or_else(||std::io::Error::other("no lifecycle reservation to acknowledge"))?;
        let registry=crate::workspaces::load(&root).map_err(std::io::Error::other)?;
        for run in &operation.runs {
            let claim=registry.reservations.iter().find(|claim|claim.run==*run && claim.conversation==operation.conversation && claim.attempt==operation.attempt).ok_or_else(||std::io::Error::other("lifecycle claim identity is unavailable; retained"))?;
            if claim.request.binding.execution!=record.owner && claim.request.binding.execution!=record.execution { return Err(std::io::Error::other("lifecycle claim path changed; retained")); }
            if claim.phase!=crate::workspaces::Phase::Released {
                crate::workspaces::finish(&root,run,&operation.attempt,false).map_err(std::io::Error::other)?;
                crate::workspaces::acknowledge_stopped(&root,run,&operation.conversation).map_err(std::io::Error::other)?;
            }
        }
        Ok("Stopped lifecycle process acknowledged. No Git action was replayed; inspect and reconcile the workspace before continuing.".into())
    }).map_err(|error|error.to_string())
}
