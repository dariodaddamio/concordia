//! Exact add/close proposals and write-ahead receipts over the one shared goal store.
use super::*;
use sha2::{Digest, Sha256};
use std::io::Read;

const RECEIPT_BYTES: u64 = 16 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    GoalChange,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Add { text: String },
    Close { goal: String, text: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub kind: Kind,
    pub version: u8,
    pub request_id: String,
    pub observed_revision: String,
    pub change: Action,
}
fn uuid(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id)
}
fn text_valid(text: &str) -> bool {
    !text.trim().is_empty()
        && text.len() <= 2048
        && text.chars().count() <= 512
        && !text
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
}
impl Proposal {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || !uuid(&self.request_id)
            || self.observed_revision.len() != 64
            || !self
                .observed_revision
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("invalid goal proposal version/request/revision".into());
        }
        let text = match &self.change {
            Action::Add { text } => text,
            Action::Close { goal, text } => {
                if goal.is_empty() || goal.len() > 128 || goal.chars().any(char::is_control) {
                    return Err("invalid goal id".into());
                }
                text
            }
        };
        if !text_valid(text) {
            return Err(
                "goal proposal needs at most 512 characters / 2048 bytes of readable text".into(),
            );
        }
        Ok(())
    }
    pub fn hash(&self) -> Result<String, String> {
        Ok(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(self).map_err(|_| "goal proposal serialization failed")?
            )
        ))
    }
    pub fn goal_id(&self) -> String {
        match &self.change {
            Action::Add { .. } => format!("g-{}", self.request_id),
            Action::Close { goal, .. } => goal.clone(),
        }
    }
    pub fn description(&self) -> String {
        let (action, text) = match &self.change {
            Action::Add { text } => ("ADD", text),
            Action::Close { text, .. } => ("CLOSE", text),
        };
        format!("MCP proposes {action} goal; requesting account/session are unverified.\nGoal: {}\n{}\nObserved revision: {}\nRequest: {}", self.goal_id(), crate::chat_cli::terminal_safe(text), self.observed_revision, self.request_id)
    }
}
fn observed(root: &Path, request: &str, expected: &str) -> Result<Vec<Goal>, String> {
    if !uuid(request) {
        return Err("request_id must be a canonical UUID".into());
    }
    let goals = read_goals(root).map_err(|error| error.to_string())?;
    if revision(&goals)? != expected {
        return Err("goals changed; read the current goal revision before proposing".into());
    }
    Ok(goals)
}
pub fn propose_add(
    root: &Path,
    request: &str,
    expected: &str,
    text: &str,
) -> Result<Proposal, String> {
    let proposal = Proposal {
        kind: Kind::GoalChange,
        version: 1,
        request_id: request.into(),
        observed_revision: expected.into(),
        change: Action::Add { text: text.into() },
    };
    proposal.validate()?;
    if let Some(receipt) = read_receipt(root, request)? {
        if receipt.proposal != proposal {
            return Err("goal request id was already used for another proposal".into());
        }
        return Ok(proposal);
    }
    observed(root, request, expected)?;
    Ok(proposal)
}
pub fn propose_close(
    root: &Path,
    request: &str,
    expected: &str,
    id: &str,
) -> Result<Proposal, String> {
    if let Some(receipt) = read_receipt(root, request)? {
        if receipt.proposal.observed_revision != expected
            || !matches!(&receipt.proposal.change, Action::Close { goal, .. } if goal == id)
        {
            return Err("goal request id was already used for another proposal".into());
        }
        return Ok(receipt.proposal);
    }
    let goals = observed(root, request, expected)?;
    let goal = goals
        .iter()
        .find(|goal| goal.id == id)
        .ok_or("unknown goal id")?;
    let proposal = Proposal {
        kind: Kind::GoalChange,
        version: 1,
        request_id: request.into(),
        observed_revision: expected.into(),
        change: Action::Close {
            goal: id.into(),
            text: goal.text.clone(),
        },
    };
    proposal.validate()?;
    Ok(proposal)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Prepared,
    Applied,
    AlreadyComplete,
    Declined,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u8,
    proposal: Proposal,
    proposal_sha256: String,
    status: Status,
}
fn receipt_path(root: &Path, request: &str) -> Result<PathBuf, String> {
    if !uuid(request) {
        return Err("invalid goal request UUID".into());
    }
    Ok(root
        .join(".ao/goal-changes")
        .join(format!("{request}.json")))
}
fn read_receipt(root: &Path, request: &str) -> Result<Option<Receipt>, String> {
    let path = receipt_path(root, request)?;
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.is_file() || crate::workspaces::linked(&metadata) => {
            return Err("goal receipt must remain a regular owned file".into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("goal receipt metadata unavailable: {error}")),
        Ok(_) => {}
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "goal receipt unavailable")?
        .take(RECEIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "goal receipt unreadable")?;
    if bytes.len() as u64 > RECEIPT_BYTES {
        return Err("goal receipt exceeds 16 KiB; original kept".into());
    }
    let receipt: Receipt =
        serde_json::from_slice(&bytes).map_err(|_| "invalid goal receipt; original kept")?;
    receipt.proposal.validate()?;
    if receipt.version != 1
        || receipt.proposal.request_id != request
        || receipt.proposal_sha256 != receipt.proposal.hash()?
    {
        return Err("goal receipt identity/hash mismatch; original kept".into());
    }
    Ok(Some(receipt))
}
fn write_receipt(root: &Path, receipt: &Receipt) -> Result<(), String> {
    let path = receipt_path(root, &receipt.proposal.request_id)?;
    let directory = path.parent().ok_or("goal receipt directory unavailable")?;
    std::fs::create_dir_all(directory).map_err(|_| "goal receipt directory unavailable")?;
    crate::workspaces::directory_id(directory)?;
    if !path.exists()
        && std::fs::read_dir(directory)
            .map_err(|_| "goal receipts unavailable")?
            .take(257)
            .count()
            >= 256
    {
        return Err(
            "goal receipt budget reached; original goals kept, no automatic pruning".into(),
        );
    }
    let bytes = serde_json::to_vec(receipt).map_err(|_| "goal receipt serialization failed")?;
    if bytes.len() as u64 > RECEIPT_BYTES {
        return Err("goal receipt exceeds 16 KiB; original goals kept".into());
    }
    crate::state_io::write_atomic(&path, &bytes)
        .map_err(|_| "goal receipt could not be saved".into())
}
fn matches_goal(proposal: &Proposal, goals: &[Goal]) -> bool {
    let Some(goal) = goals.iter().find(|goal| goal.id == proposal.goal_id()) else {
        return false;
    };
    match &proposal.change {
        Action::Add { text } => {
            goal.text == *text
                && goal
                    .origin
                    .as_ref()
                    .and_then(|origin| origin.request_id.as_deref())
                    == Some(proposal.request_id.as_str())
        }
        Action::Close { text, .. } => {
            goal.done
                && goal.text == *text
                && goal
                    .closed_origin
                    .as_ref()
                    .and_then(|origin| origin.request_id.as_deref())
                    == Some(proposal.request_id.as_str())
        }
    }
}
pub fn status(root: &Path, request: &str) -> Result<String, String> {
    let receipt = read_receipt(root, request)?.ok_or("goal request receipt not found")?;
    let goals = read_goals(root).map_err(|error| error.to_string())?;
    serde_json::to_string(&serde_json::json!({"request_id":request,"goal":receipt.proposal.goal_id(),"status":receipt.status,"proposal_sha256":receipt.proposal_sha256,"current_goal_matches_request":matches_goal(&receipt.proposal,&goals),"requesting_actor":"unverified MCP client; no native account/session binding","revision":revision(&goals)?})).map_err(|_| "goal receipt output failed".into())
}
/// A duplicate tool call can inspect its outcome, but can never reapply an uncertain request.
pub fn existing_status(root: &Path, proposal: &Proposal) -> Result<Option<String>, String> {
    let Some(receipt) = read_receipt(root, &proposal.request_id)? else {
        return Ok(None);
    };
    if receipt.proposal != *proposal {
        return Err("goal request id was already used for a different proposal".into());
    }
    status(root, &proposal.request_id).map(Some)
}
pub fn decline(root: &Path, proposal: &Proposal) -> Result<String, String> {
    proposal.validate()?;
    std::fs::create_dir_all(root).map_err(|_| "goal state directory unavailable")?;
    with_goal_lock(root, || {
        if let Some(status) = existing_status(root, proposal).map_err(std::io::Error::other)? {
            return Ok(status);
        }
        write_receipt(
            root,
            &Receipt {
                version: 1,
                proposal: proposal.clone(),
                proposal_sha256: proposal.hash().map_err(std::io::Error::other)?,
                status: Status::Declined,
            },
        )
        .map_err(std::io::Error::other)?;
        Ok(format!(
            "Declined goal proposal {}. Goals unchanged.",
            proposal.request_id
        ))
    })
    .map_err(|error| error.to_string())
}
/// Called only by explicit local review, never by the MCP request handler.
pub fn apply(root: &Path, proposal: &Proposal) -> Result<String, String> {
    proposal.validate()?;
    std::fs::create_dir_all(root).map_err(|_| "goal state directory unavailable")?;
    with_goal_lock(root, || {
        if let Some(receipt) = read_receipt(root, &proposal.request_id).map_err(std::io::Error::other)? {
            if receipt.proposal != *proposal { return Err(std::io::Error::other("goal request id was already used for a different proposal")); }
            return match receipt.status {
                Status::Applied | Status::AlreadyComplete => Ok(format!("Goal request {} already resolved; no change replayed", proposal.request_id)),
                Status::Prepared => Err(std::io::Error::other("goal request has an unconfirmed receipt; inspect status, never replay")),
                Status::Declined => Err(std::io::Error::other("goal request was declined; a new proposal needs a new request id")),
            };
        }
        let mut goals = read_goals(root)?;
        let mut receipt = Receipt { version: 1, proposal: proposal.clone(), proposal_sha256: proposal.hash().map_err(std::io::Error::other)?, status: Status::Prepared };
        if let Action::Close { goal, text } = &proposal.change {
            if let Some(existing) = goals.iter().find(|existing| existing.id == *goal && existing.done && existing.text == *text) {
                let id = existing.id.clone();
                receipt.status = Status::AlreadyComplete;
                write_receipt(root, &receipt).map_err(std::io::Error::other)?;
                return Ok(format!("Goal {id} was already complete; original close attribution preserved"));
            }
        }
        if revision(&goals).map_err(std::io::Error::other)? != proposal.observed_revision { return Err(std::io::Error::other("goals changed since this proposal; read and propose again, no change applied")); }
        let attribution = Some(Attribution { source: ChangeSource::McpProposalApprovedLocally, request_id: Some(proposal.request_id.clone()) });
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(std::io::Error::other)?.as_secs().to_string();
        match &proposal.change {
            Action::Add { text } => {
                let id = proposal.goal_id();
                if goals.iter().any(|goal| goal.id == id) { return Err(std::io::Error::other("goal id already exists; original kept")); }
                goals.push(Goal { id, text: text.clone(), done: false, added_at: now, closed_at: None, origin: attribution, closed_origin: None, ..Default::default() });
            }
            Action::Close { goal, text } => {
                let existing = goals.iter_mut().find(|existing| existing.id == *goal && existing.text == *text).ok_or_else(|| std::io::Error::other("observed goal identity/text changed; original kept"))?;
                existing.done = true;
                existing.closed_at = Some(now);
                existing.closed_origin = attribution;
            }
        }
        // Check the entire prospective store before recording an uncertain operation.
        encode_goals(&goals)?;
        write_receipt(root, &receipt).map_err(std::io::Error::other)?;
        write_goals(root, &goals)?;
        receipt.status = Status::Applied;
        if write_receipt(root, &receipt).is_err() { return Ok(format!("Goal {} saved with local approval; receipt {} remains unconfirmed. Inspect its status; do not replay.", proposal.goal_id(), proposal.request_id)); }
        Ok(format!("Goal {} {} after explicit local approval of MCP proposal {} (requesting account/session unverified)", proposal.goal_id(), if matches!(proposal.change, Action::Add { .. }) { "added" } else { "closed" }, proposal.request_id))
    }).map_err(|error| error.to_string())
}
