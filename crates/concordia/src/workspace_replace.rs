//! User-reviewed literal replacements use the editor's compare-before-replace save path.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

const MAX_BATCH_BYTES: usize = 8 * 1024 * 1024;
const MAX_PREVIEW_BYTES: usize = 256 * 1024;

#[derive(Debug)]
struct File {
    path: PathBuf,
    before: String,
    after: String,
    before_hash: String,
    after_hash: String,
}
#[derive(Debug)]
pub struct Plan {
    pub id: String,
    pub root: PathBuf,
    pub conversation: String,
    pub generation: u64,
    pub preview: String,
    files: Vec<File>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileReceipt {
    pub path: PathBuf,
    pub before_hash: String,
    pub after_hash: String,
    pub status: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: u8,
    pub id: String,
    pub root: PathBuf,
    pub conversation: String,
    pub files: Vec<FileReceipt>,
    pub message: String,
}
impl Receipt {
    pub fn summary(&self) -> String {
        format!(
            "Replacement {}: {} of {} files saved. {}",
            self.id,
            self.files
                .iter()
                .filter(|file| file.status.starts_with("saved"))
                .count(),
            self.files.len(),
            self.message
        )
    }
}
fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

pub fn stage(
    summary: &SearchSummary,
    selected: &[usize],
    replacement: &str,
    cancel: &AtomicBool,
) -> Result<Plan, String> {
    if selected.is_empty()
        || selected.len() > 1000
        || replacement.len() > 4096
        || replacement.contains('\0')
    {
        return Err("Select 1–1,000 matches; replacement is limited to 4 KiB".into());
    }
    let mut grouped: BTreeMap<PathBuf, Vec<&SearchHit>> = BTreeMap::new();
    let mut unique = HashSet::new();
    for index in selected {
        if !unique.insert(*index) {
            return Err("Duplicate replacement selection".into());
        }
        let hit = summary
            .hits
            .get(*index)
            .ok_or("Selected search result is no longer available")?;
        grouped.entry(hit.path.clone()).or_default().push(hit);
    }
    if grouped.len() > 32 {
        return Err("Select at most 32 files per preview".into());
    }
    let mut plan = Plan {
        id: uuid::Uuid::new_v4().to_string(),
        root: summary.request.root.clone(),
        conversation: summary.request.conversation.clone(),
        generation: summary.request.generation,
        preview: String::new(),
        files: Vec::new(),
    };
    let mut bytes = 0usize;
    for (path, mut hits) in grouped {
        let verified = verify_hit(&summary.request, hits[0], cancel)?;
        let before = verified.text;
        let ranges = match_ranges(
            &before,
            &summary.request.query,
            summary.request.case_sensitive,
            cancel,
            Instant::now(),
        )
        .map_err(|status| format!("replacement verification stopped: {status:?}"))?;
        hits.sort_by_key(|hit| hit.range.start);
        let mut end = 0;
        for hit in &hits {
            if hit.hash != hits[0].hash
                || hit.range.start < end
                || hit.range.start >= hit.range.end
                || !before.is_char_boundary(hit.range.start)
                || !before.is_char_boundary(hit.range.end)
                || hit.range.end > before.len()
            {
                return Err("Selected match ranges overlap or have changed".into());
            }
            if !ranges.contains(&hit.range) {
                return Err("Selected match no longer agrees with its query".into());
            }
            end = hit.range.end;
        }
        let new_len = hits
            .iter()
            .try_fold(before.len(), |len, hit| {
                len.checked_sub(hit.range.len())
                    .and_then(|len| len.checked_add(replacement.len()))
            })
            .ok_or("Replacement size overflow")?;
        if new_len > MAX_FILE_BYTES {
            return Err("Replacement exceeds the 2 MiB file limit".into());
        }
        bytes = bytes.saturating_add(before.len()).saturating_add(new_len);
        if bytes > MAX_BATCH_BYTES {
            return Err("Replacement preview exceeds 8 MiB; select fewer files".into());
        }
        let mut after = before.clone();
        for hit in hits.iter().rev() {
            after.replace_range(hit.range.clone(), replacement);
        }
        if after.bytes().filter(|byte| *byte == b'\n').count() + 1 > MAX_LINES {
            return Err("Replacement exceeds 50,000 lines per file".into());
        }
        plan.preview
            .push_str(&format!("File: {}\n", path.display()));
        for hit in &hits {
            let (line, _, _) = location(&before, hit.range.start);
            plan.preview.push_str(&format!(
                "@@ line {}, bytes {}..{} @@\n-{}\n+{}\n",
                line,
                hit.range.start,
                hit.range.end,
                &before[hit.range.clone()],
                replacement.replace('\n', "\n+")
            ));
            if plan.preview.len() > MAX_PREVIEW_BYTES {
                return Err("Readable preview exceeds 256 KiB; select fewer matches".into());
            }
        }
        plan.files.push(File {
            path,
            before_hash: digest(&before),
            after_hash: digest(&after),
            before,
            after,
        });
    }
    Ok(plan)
}

fn receipt_path(state: &Path, id: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Replacement receipt needs its exact UUID")?;
    let directory = state.join("project-replacements");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    crate::workspaces::directory_id(&directory)?;
    Ok(directory.join(format!("{id}.json")))
}
fn persist(path: &Path, receipt: &Receipt) -> Result<(), String> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if !metadata.is_file() || crate::workspaces::linked(&metadata) {
            return Err("Replacement receipt must be a regular owned file".into());
        }
    }
    let bytes = serde_json::to_vec(receipt).map_err(|e| e.to_string())?;
    if bytes.len() > 128 * 1024 {
        return Err("Replacement receipt exceeds 128 KiB".into());
    }
    crate::state_io::write_atomic(path, &bytes).map_err(|e| e.to_string())
}

/// All files preflight before the first save. The scheduler lock prevents new cooperative owners.
/// A partial/uncertain save is retained in the receipt; this function never retries or rolls back.
pub fn apply(
    state: &Path,
    plan: Plan,
    blocked: &[PathBuf],
    cancel: &AtomicBool,
) -> Result<Receipt, String> {
    let path = receipt_path(state, &plan.id)?;
    if path.exists() {
        return Err(
            "This replacement plan already has a receipt; reconcile it instead of retrying".into(),
        );
    }
    if std::fs::read_dir(path.parent().ok_or("Receipt directory missing")?)
        .map_err(|e| e.to_string())?
        .take(129)
        .count()
        >= 128
    {
        return Err("Replacement receipt store is full; existing receipts retained".into());
    }
    let mut receipt = Receipt {
        version: 1,
        id: plan.id.clone(),
        root: plan.root.clone(),
        conversation: plan.conversation.clone(),
        files: plan
            .files
            .iter()
            .map(|file| FileReceipt {
                path: file.path.clone(),
                before_hash: file.before_hash.clone(),
                after_hash: file.after_hash.clone(),
                status: "unstarted".into(),
            })
            .collect(),
        message: "Prepared; no files saved".into(),
    };
    crate::workspaces::with_workspace_guard(state, &[&plan.root], |_| {
        if path.exists() {
            return Err(
                "This replacement plan already has a receipt; reconcile it instead of retrying"
                    .into(),
            );
        }
        for file in &plan.files {
            if cancel.load(Ordering::Relaxed) {
                return Err("Replacement cancelled before writing".into());
            }
            workspace_paths::regular_file(&plan.root, &file.path)?;
            if blocked.iter().any(|path| {
                crate::workspaces::same_key(
                    &crate::workspaces::path_key(path),
                    &crate::workspaces::path_key(&file.path),
                )
            }) {
                return Err(format!(
                    "Dirty/saving file pane blocks {}; save/discard it and preview again",
                    file.path.display()
                ));
            }
            let metadata = std::fs::metadata(&file.path).map_err(|e| e.to_string())?;
            if metadata.permissions().readonly() {
                return Err(format!("{} is read-only", file.path.display()));
            }
            let bytes = read_file(
                &plan.root,
                &file.path,
                cancel,
                Instant::now(),
                MAX_FILE_BYTES + 1,
            )?;
            if bytes != file.before.as_bytes() {
                return Err(format!(
                    "{} changed; preview again before replacing",
                    file.path.display()
                ));
            }
        }
        persist(&path, &receipt)?;
        for (index, file) in plan.files.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                receipt.message = "Cancelled; remaining files untouched".into();
                persist(&path, &receipt)?;
                return Ok(receipt);
            }
            // Write-ahead state makes a crash between save and receipt update reconcilable.
            receipt.files[index].status = "saving; outcome unconfirmed".into();
            persist(&path, &receipt)?;
            let result = workspace_paths::regular_file(&plan.root, &file.path)
                .and_then(|_| crate::file_document::save(&file.path, &file.before, &file.after));
            match result {
                Ok(()) => receipt.files[index].status = "saved".into(),
                Err(error) => {
                    receipt.files[index].status =
                        "failed or uncertain; inspect retained save artifacts".into();
                    receipt.message = error;
                    persist(&path, &receipt)?;
                    return Ok(receipt);
                }
            }
            receipt.message = "Applying selected files serially".into();
            persist(&path, &receipt)?;
        }
        receipt.message = "Complete; no automatic rollback or replay".into();
        persist(&path, &receipt)?;
        Ok(receipt)
    })
}

fn load_receipt(state: &Path, id: &str) -> Result<Receipt, String> {
    let path = receipt_path(state, id)?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || crate::workspaces::linked(&metadata) || metadata.len() > 128 * 1024 {
        return Err("Invalid replacement receipt; original retained".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .map_err(|e| e.to_string())?
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 128 * 1024 {
        return Err("Replacement receipt exceeds its budget".into());
    }
    let receipt: Receipt = serde_json::from_slice(&bytes)
        .map_err(|_| "Invalid replacement receipt; original retained")?;
    if receipt.version != 1 || receipt.id != id || receipt.files.len() > 32 {
        return Err("Replacement receipt version/identity/budget mismatch".into());
    }
    Ok(receipt)
}
/// Read only the explicitly selected receipt and compare hashes. Never resumes a saved operation.
pub fn reconcile(state: &Path, id: &str, cancel: &AtomicBool) -> Result<Receipt, String> {
    let mut receipt = load_receipt(state, id)?;
    for file in &mut receipt.files {
        file.status = match read_file(
            &receipt.root,
            &file.path,
            cancel,
            Instant::now(),
            MAX_FILE_BYTES + 1,
        )
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|text| digest(&text))
        {
            Some(hash) if hash == file.after_hash => "saved bytes present".into(),
            Some(hash) if hash == file.before_hash => "original bytes present".into(),
            Some(_) => "changed since preview; inspect manually".into(),
            None => "unavailable; inspect manually".into(),
        };
    }
    receipt.message = "Reconciled current hashes; no writes or replay".into();
    Ok(receipt)
}

pub fn list(
    state: &Path,
    root: &Path,
    cancel: &AtomicBool,
) -> Result<Vec<(String, String)>, String> {
    let directory = state.join("project-replacements");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    crate::workspaces::directory_id(&directory)?;
    let entries = std::fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .take(129)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if entries.len() > 128 {
        return Err("Receipt directory exceeds its 128-entry budget".into());
    }
    let mut rows = Vec::new();
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            return Err("Receipt inspection cancelled".into());
        }
        if entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        let id = entry
            .path()
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let receipt = load_receipt(state, &id)?;
        if receipt.root == root {
            rows.push((id, receipt.summary()));
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(rows)
}
