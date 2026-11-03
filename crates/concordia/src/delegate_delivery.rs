//! Explicit cross-worktree dependency acceptance. It never integrates or executes native tools.
use super::*;
use crate::managed_workspaces::lifecycle::DependencySnapshot;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    ResultOnly,
    Integrated,
}
impl Mode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "result-only" => Ok(Self::ResultOnly),
            "integrated" => Ok(Self::Integrated),
            _ => Err("choose result-only or integrated".into()),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::ResultOnly => "result-only",
            Self::Integrated => "integrated",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parent {
    pub run: String,
    pub attempt: String,
    pub account: String,
    pub provider: ProviderKind,
    pub home: PathBuf,
    pub session: String,
    pub source: String,
    pub managed: String,
    pub execution: PathBuf,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub conversation: String,
    pub child: String,
    pub owner: PathBuf,
    pub parents: Vec<Parent>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: u8,
    pub request: Request,
    pub snapshots: Vec<DependencySnapshot>,
    pub mode: Mode,
    pub hash: String,
}
#[derive(Debug, Clone)]
pub struct Review {
    pub request: Request,
    pub snapshots: Vec<DependencySnapshot>,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
impl Review {
    pub fn receipt(&self, mode: Mode) -> Result<Receipt, String> {
        if mode == Mode::Integrated
            && self
                .snapshots
                .iter()
                .any(|snapshot| !snapshot.integration_verified)
        {
            return Err("integration_required: an exact finished managed integration receipt and matching clean owner tree are missing; integrate explicitly or accept result-only".into());
        }
        let mut receipt = Receipt {
            version: 1,
            request: self.request.clone(),
            snapshots: self.snapshots.clone(),
            mode,
            hash: String::new(),
        };
        receipt.hash = digest(
            &serde_json::to_vec(&receipt).map_err(|_| "dependency receipt serialization failed")?,
        );
        Ok(receipt)
    }
    pub fn describe(&self, job: u32) -> Result<String, String> {
        let mut text = format!("Cross-worktree dependency review for #{job}\nChild run {}\nConversation {}\nOwner {}\nThese are observed current heads, not proof that the prerequisite authored each commit. Completion grants no integration.\n", self.request.child, self.request.conversation, self.request.owner.display());
        for (parent, snapshot) in self.request.parents.iter().zip(&self.snapshots) {
            text.push_str(&format!("\nPrerequisite run {} / attempt {}\nNative source {} · {} / {} / {}\nManaged {} revision {}\nBase {}\nOwner head {}\nWorktree head {}\nWorktree {}\nOwner status (first 4096 characters):\n{}\nWorker status (first 4096 characters):\n{}\nExisting integration verified: {}\n", parent.run, parent.attempt, parent.source, parent.provider.as_str(), parent.account, parent.session, snapshot.managed, snapshot.revision, snapshot.base, snapshot.owner_head, snapshot.worker_head, snapshot.execution.display(), snapshot.owner_status_excerpt, snapshot.worker_status_excerpt, snapshot.integration_verified));
        }
        text.push_str(&format!("\nAccept results only (edits are NOT delivered): /delegate delivery {job} result-only {}", self.receipt(Mode::ResultOnly)?.hash));
        match self.receipt(Mode::Integrated) {
            Ok(receipt) => text.push_str(&format!(
                "\nAccept already integrated tree: /delegate delivery {job} integrated {}",
                receipt.hash
            )),
            Err(reason) => text.push_str(&format!("\n{reason}")),
        }
        Ok(text)
    }
}
impl Receipt {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.request.parents.is_empty()
            || self.request.parents.len() > 8
            || self.request.parents.len() != self.snapshots.len()
            || !self.request.owner.is_absolute()
            || self.request.owner.to_string_lossy().len() > 4096
        {
            return Err("invalid dependency delivery receipt bounds".into());
        }
        for id in [&self.request.conversation, &self.request.child] {
            if uuid::Uuid::parse_str(id).is_err() {
                return Err("invalid dependency delivery identity".into());
            }
        }
        let mut runs = std::collections::BTreeSet::new();
        for (parent, snapshot) in self.request.parents.iter().zip(&self.snapshots) {
            if uuid::Uuid::parse_str(&parent.run).is_err()
                || !runs.insert(parent.run.as_str())
                || parent.run == self.request.child
                || uuid::Uuid::parse_str(&parent.attempt).is_err()
                || uuid::Uuid::parse_str(&parent.managed).is_err()
                || parent.managed != snapshot.managed
                || parent.execution != snapshot.execution
                || self.request.owner != snapshot.owner
                || parent.source.len() != 64
                || !parent.source.bytes().all(|byte| byte.is_ascii_hexdigit())
                || parent.account.len() > 256
                || parent.session.len() > 256
                || parent.home.to_string_lossy().len() > 4096
                || !snapshot.common_git.is_absolute()
                || snapshot.common_git.to_string_lossy().len() > 4096
                || [
                    &snapshot.base,
                    &snapshot.owner_head,
                    &snapshot.worker_head,
                    &snapshot.owner_tree,
                ]
                .iter()
                .any(|id| {
                    !matches!(id.len(), 40 | 64) || !id.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                || [
                    &snapshot.owner_status_sha256,
                    &snapshot.worker_status_sha256,
                ]
                .iter()
                .any(|hash| hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
                || snapshot.owner_status_excerpt.len() > 16384
                || snapshot.worker_status_excerpt.len() > 16384
            {
                return Err("invalid dependency parent/workspace receipt".into());
            }
        }
        let expected = Review {
            request: self.request.clone(),
            snapshots: self.snapshots.clone(),
        }
        .receipt(self.mode)?;
        if self.hash != expected.hash {
            return Err("dependency delivery receipt hash mismatch".into());
        }
        Ok(())
    }
}

pub fn review(root: &Path, request: &Request, cancel: &AtomicBool) -> Result<Review, String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut snapshots = Vec::new();
    for parent in &request.parents {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            return Err("dependency review cancelled".into());
        }
        let source = crate::native_memory::verify_source(root, &parent.source)?;
        if source.provider != parent.provider
            || source.account != parent.account
            || source.session_id != parent.session
            || source.home_id != digest(parent.home.to_string_lossy().as_bytes())
        {
            return Err(
                "prerequisite native source belongs to another account/home/session".into(),
            );
        }
        let snapshot = crate::managed_workspaces::lifecycle::dependency_snapshot(
            root,
            &request.owner,
            &parent.managed,
            cancel,
            deadline,
        )?;
        if snapshot.execution != parent.execution {
            return Err("prerequisite managed execution path changed".into());
        }
        snapshots.push(snapshot);
    }
    let review = Review {
        request: request.clone(),
        snapshots,
    };
    review.receipt(Mode::ResultOnly)?.validate()?;
    Ok(review)
}

impl Delegates {
    pub fn delivery_request(&self, id: u32) -> Result<Request, String> {
        if self
            .get(id)
            .is_none_or(|job| job.state != WorkerState::Queued)
        {
            return Err("delivery review requires an exact queued task".into());
        }
        self.delivery_input(id)
    }
    pub(super) fn delivery_input(&self, id: u32) -> Result<Request, String> {
        let job = self.get(id).ok_or("unknown downstream job")?;
        if !matches!(job.state, WorkerState::Queued | WorkerState::Starting)
            || job.cancel_requested
            || job.conversation_id != self.conversation_id
        {
            return Err(
                "delivery review requires the exact queued downstream task in this conversation"
                    .into(),
            );
        }
        let owner = self
            .recovery
            .as_ref()
            .ok_or("durable dependency owner unavailable")?;
        if job.spec.cwd.canonicalize().ok().as_ref() != Some(&owner.workspace) {
            return Err("downstream task must use its owner's workspace".into());
        }
        let task = self
            .schedule
            .tasks()
            .iter()
            .find(|task| task.run_id() == job.run_id)
            .ok_or("task has no dependency receipt")?;
        let mut parents = Vec::new();
        for prerequisite in task.prerequisites() {
            let parent = self
                .jobs
                .iter()
                .find(|parent| parent.run_id == prerequisite.run_id)
                .ok_or("prerequisite result unavailable")?;
            if parent.spec.cwd.canonicalize().ok().as_ref() == Some(&owner.workspace) {
                continue;
            }
            if parent.state != WorkerState::Done || parent.conversation_id != self.conversation_id {
                return Err("wait for the exact prerequisite run to finish successfully".into());
            }
            let binding = &parent
                .workspace
                .as_ref()
                .ok_or("prerequisite has no workspace ownership")?
                .binding;
            let managed = binding
                .managed
                .as_ref()
                .ok_or("external unmanaged prerequisites are unsupported")?;
            if binding.owner != owner.workspace || binding.execution != parent.spec.cwd {
                return Err("prerequisite managed owner changed".into());
            }
            parents.push(Parent {
                run: parent.run_id.clone(),
                attempt: parent
                    .attempt_records
                    .last()
                    .and_then(|attempt| attempt.id.clone())
                    .ok_or("prerequisite attempt identity unavailable")?,
                account: parent.account.clone(),
                provider: parent.provider,
                home: parent
                    .spec
                    .home
                    .as_ref()
                    .and_then(|home| home.canonicalize().ok())
                    .ok_or("prerequisite native home unavailable")?,
                session: parent
                    .session_id
                    .clone()
                    .ok_or("prerequisite native session unavailable")?,
                source: parent
                    .source_id
                    .clone()
                    .ok_or("prerequisite native source unavailable")?,
                managed: managed.id.clone(),
                execution: binding.execution.clone(),
            });
        }
        if parents.is_empty() {
            return Err("no cross-worktree prerequisites need delivery acceptance".into());
        }
        Ok(Request {
            conversation: self.conversation_id.clone(),
            child: job.run_id.clone(),
            owner: owner.workspace.clone(),
            parents,
        })
    }
    pub fn accept_delivery(&mut self, id: u32, receipt: Receipt) -> Result<(), String> {
        receipt.validate()?;
        if self.delivery_request(id)? != receipt.request {
            return Err("dependency request changed during review; nothing accepted".into());
        }
        let before = self.schedule.clone();
        let child = receipt.request.child.clone();
        self.schedule.accept_delivery(&child, receipt)?;
        if let Err(error) = self.save_recovery() {
            self.schedule = before;
            return Err(error);
        }
        self.refresh_dependency_status();
        Ok(())
    }
}
