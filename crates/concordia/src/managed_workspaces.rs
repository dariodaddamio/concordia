//! Explicit Git worktree creation. No automatic integration, deletion or filesystem sandbox claim.
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_RECORDS: usize = 64;

#[path = "managed_lifecycle.rs"]
pub mod lifecycle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Creating,
    Ready,
    ReconcileRequired,
    Retained,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: String,
    pub revision: u64,
    pub owner: PathBuf,
    pub owner_identity: String,
    pub common_git: PathBuf,
    pub common_identity: String,
    pub base: String,
    pub branch: String,
    pub execution: PathBuf,
    pub execution_identity: Option<String>,
    pub phase: Phase,
    pub note: String,
    #[serde(default)]
    pub operation: Option<lifecycle::Operation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub version: u8,
    pub revision: u64,
    pub records: Vec<Record>,
}

fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
fn object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn regular(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() || crate::workspaces::linked(&meta) => {
            Err("managed registry must use regular owned files".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn root_path(root: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let canonical = root.canonicalize().map_err(|error| error.to_string())?;
    crate::workspaces::directory_id(&canonical)?;
    Ok(canonical)
}

pub fn load(root: &Path) -> Result<Registry, String> {
    let path = root.join("managed-workspaces.json");
    regular(&path)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Registry {
                version: 1,
                revision: 0,
                records: Vec::new(),
            })
        }
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("managed registry exceeds 1 MiB; original kept".into());
    }
    let registry: Registry = serde_json::from_slice(&bytes)
        .map_err(|error| format!("managed registry unavailable: {error}; original kept"))?;
    let mut seen = std::collections::BTreeSet::new();
    let owned = root
        .canonicalize()
        .map_err(|error| error.to_string())?
        .join("managed-worktrees");
    if !(1..=2).contains(&registry.version)
        || registry.records.len() > MAX_RECORDS
        || registry.records.iter().any(|record| {
            !uuid(&record.id)
                || !seen.insert(&record.id)
                || record.revision == 0
                || !object_id(&record.base)
                || record.branch != format!("concordia/work-{}", record.id)
                || record.execution != owned.join(&record.id)
                || !record.owner.is_absolute()
                || !record.common_git.is_absolute()
                || record.note.len() > 4096
                || record
                    .operation
                    .as_ref()
                    .is_some_and(|operation| !operation.valid())
                || [&record.owner, &record.common_git, &record.execution]
                    .iter()
                    .any(|path| path_text(path).is_err())
                || !crate::workspaces::stable_id(&record.owner_identity)
                || !crate::workspaces::stable_id(&record.common_identity)
                || record
                    .execution_identity
                    .as_ref()
                    .is_some_and(|id| !crate::workspaces::stable_id(id))
                || record.phase == Phase::Ready && record.execution_identity.is_none()
        })
    {
        return Err("invalid managed registry identity/version; original kept".into());
    }
    Ok(registry)
}

fn mutate<T>(
    root: &Path,
    operation: impl FnOnce(&mut Registry) -> Result<T, String>,
) -> Result<T, String> {
    let root = root_path(root)?;
    let lock = root.join("managed-workspaces.lock");
    regular(&lock)?;
    crate::state_io::with_lock(&lock, || {
        let mut registry = load(&root).map_err(std::io::Error::other)?;
        let result = operation(&mut registry).map_err(std::io::Error::other)?;
        save(&root, &mut registry).map_err(std::io::Error::other)?;
        Ok(result)
    })
    .map_err(|error| error.to_string())
}

fn save(root: &Path, registry: &mut Registry) -> Result<(), String> {
    registry.version = 2;
    registry.revision = registry
        .revision
        .checked_add(1)
        .ok_or("managed revision overflow")?;
    let bytes = serde_json::to_vec(registry).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("managed registry is full; original kept".into());
    }
    crate::state_io::write_atomic(&root.join("managed-workspaces.json"), &bytes)
        .map_err(|error| error.to_string())
}

struct Git<'a> {
    args: Vec<String>,
    cancel: &'a AtomicBool,
    deadline: Instant,
    confirmed_exit: std::cell::Cell<bool>,
}

impl<'a> Git<'a> {
    fn new(cancel: &'a AtomicBool) -> Result<Self, String> {
        let executable = which::which("git")
            .map_err(|_| "Git is unavailable; managed worktrees are unsupported")?;
        #[cfg(windows)]
        if !executable
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        {
            return Err("managed worktrees require a native Git executable".into());
        }
        let program = executable
            .to_str()
            .ok_or("Git executable has ambiguous Unicode identity")?
            .to_owned();
        Ok(Self {
            args: vec![
                program,
                "--no-pager".into(),
                "--no-optional-locks".into(),
                "-c".into(),
                "core.fsmonitor=false".into(),
                "-c".into(),
                "submodule.recurse=false".into(),
                "-c".into(),
                "gc.auto=0".into(),
                "-c".into(),
                "maintenance.auto=false".into(),
            ],
            cancel,
            deadline: Instant::now() + Duration::from_secs(60),
            confirmed_exit: std::cell::Cell::new(true),
        })
    }
    fn run(&self, cwd: &Path, args: &[&str], allow_missing: bool) -> Result<String, String> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err("managed Git operation cancelled; inspect any recorded creation".into());
        }
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("managed Git operation reached its 60-second deadline".into());
        }
        let mut argv = self.args.clone();
        argv.extend(args.iter().map(|arg| (*arg).to_owned()));
        let output =
            crate::harness::run::run_command_cancellable(&argv, cwd, remaining, self.cancel);
        self.confirmed_exit.set(output.exit.is_some());
        if output.timed_out || self.cancel.load(Ordering::SeqCst) {
            return Err("managed Git command interrupted; inspect recorded identities before reconciliation".into());
        }
        if output.stdout.len() >= 64 * 1024
            || output.stderr.len() >= 64 * 1024
            || output.stdout.contains('\u{fffd}')
        {
            return Err(
                "managed Git metadata exceeded its bounded UTF-8 read; original work kept".into(),
            );
        }
        if output.exit != Some(0) && !(allow_missing && output.exit == Some(1)) {
            let detail: String = output
                .stderr
                .chars()
                .filter(|ch| !ch.is_control() || *ch == '\n')
                .take(1200)
                .collect();
            return Err(format!("Git refused managed workspace operation: {detail}"));
        }
        Ok(output.stdout)
    }
    fn protect_checkout(&mut self, owner: &Path, root: &Path) -> Result<(), String> {
        let hooks = root.join("managed-empty-hooks");
        fs::create_dir_all(&hooks).map_err(|error| error.to_string())?;
        crate::workspaces::directory_id(&hooks)?;
        if fs::read_dir(&hooks)
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
        {
            return Err("managed empty hooks directory changed; checkout refused".into());
        }
        self.args.extend([
            "-c".into(),
            format!("core.hooksPath={}", path_text(&hooks)?),
        ]);
        // Read only configuration key names; never inspect credential/configuration values.
        let keys = self.run(
            owner,
            &[
                "config",
                "--name-only",
                "--get-regexp",
                "^filter\\..*\\.(clean|smudge|process|required)$",
            ],
            true,
        )?;
        let mut filters = std::collections::BTreeSet::new();
        for key in keys.lines() {
            let filter = key
                .strip_prefix("filter.")
                .and_then(|key| key.rsplit_once('.').map(|(name, _)| name))
                .ok_or("invalid Git filter key")?;
            if filter.is_empty()
                || filter.len() > 128
                || !filter
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            {
                return Err("unsupported Git filter identity; checkout refused".into());
            }
            filters.insert(filter);
            if filters.len() > 32 {
                return Err("Git filter configuration exceeds 32-filter bound".into());
            }
        }
        for filter in filters {
            for (field, value) in [
                ("clean", ""),
                ("smudge", ""),
                ("process", ""),
                ("required", "false"),
            ] {
                self.args
                    .extend(["-c".into(), format!("filter.{filter}.{field}={value}")]);
            }
        }
        Ok(())
    }
}

fn path_text(path: &Path) -> Result<&str, String> {
    let text = path
        .to_str()
        .filter(|text| text.len() <= 4096 && !text.chars().any(char::is_control))
        .ok_or("managed path has ambiguous Unicode identity")?;
    // Git for Windows cannot create worktrees through Rust's verbatim DOS prefix.
    // Keep canonical metadata unchanged and pass the equivalent DOS path as one argument.
    #[cfg(windows)]
    if let Some(dos) = text.strip_prefix(r"\\?\") {
        if dos.as_bytes().get(1) == Some(&b':') {
            return Ok(dos);
        }
        return Err("managed Git creation on verbatim network paths is unsupported".into());
    }
    Ok(text)
}
fn git_path(git: &Git<'_>, cwd: &Path, arg: &str) -> Result<PathBuf, String> {
    let text = git.run(cwd, &["rev-parse", "--path-format=absolute", arg], false)?;
    let path = PathBuf::from(text.trim_end_matches(['\r', '\n']));
    if !path.is_absolute() {
        return Err("Git returned a relative repository identity".into());
    }
    path.canonicalize().map_err(|error| error.to_string())
}

fn verify_record(git: &Git<'_>, record: &Record) -> Result<(), String> {
    if record.owner.canonicalize().ok().as_ref() != Some(&record.owner)
        || crate::workspaces::directory_id(&record.owner)? != record.owner_identity
        || crate::workspaces::directory_id(&record.common_git)? != record.common_identity
        || git_path(git, &record.owner, "--git-common-dir")? != record.common_git
        || record.execution.canonicalize().ok().as_ref() != Some(&record.execution)
        || git_path(git, &record.execution, "--show-toplevel")? != record.execution
        || git_path(git, &record.execution, "--git-common-dir")? != record.common_git
        || git
            .run(
                &record.execution,
                &["symbolic-ref", "--quiet", "HEAD"],
                false,
            )?
            .trim()
            != format!("refs/heads/{}", record.branch)
    {
        return Err("managed workspace ownership or repository binding changed".into());
    }
    git.run(
        &record.execution,
        &["merge-base", "--is-ancestor", &record.base, "HEAD"],
        false,
    )?;
    if let Some(identity) = &record.execution_identity {
        if crate::workspaces::directory_id(&record.execution)? != *identity {
            return Err("managed execution directory was replaced".into());
        }
    }
    Ok(())
}

/// Read-only revalidation for worker admission. Working edits are retained; head/branch ownership remains explicit.
pub fn bound(root: &Path, owner: &Path, id: &str) -> Result<Record, String> {
    let registry = load(root)?;
    let record = registry
        .records
        .into_iter()
        .find(|record| record.id == id)
        .ok_or("unknown managed workspace")?;
    if record.phase != Phase::Ready || owner.canonicalize().ok().as_ref() != Some(&record.owner) {
        return Err("managed workspace is not ready for this owner".into());
    }
    if crate::workspaces::directory_id(&record.owner)? != record.owner_identity
        || crate::workspaces::directory_id(&record.common_git)? != record.common_identity
        || record.execution.canonicalize().ok().as_ref() != Some(&record.execution)
        || Some(crate::workspaces::directory_id(&record.execution)?) != record.execution_identity
    {
        return Err("managed workspace directory identity changed".into());
    }
    Ok(record)
}

pub fn verified(root: &Path, owner: &Path, id: &str) -> Result<Record, String> {
    verified_cancellable(root, owner, id, &AtomicBool::new(false))
}

pub fn verified_cancellable(
    root: &Path,
    owner: &Path,
    id: &str,
    cancel: &AtomicBool,
) -> Result<Record, String> {
    let record = bound(root, owner, id)?;
    verify_record(&Git::new(cancel)?, &record)?;
    Ok(record)
}

pub fn create(
    root: &Path,
    owner: &Path,
    base: &str,
    cancel: &AtomicBool,
) -> Result<Record, String> {
    if base.is_empty()
        || base.len() > 256
        || base.starts_with('-')
        || base.chars().any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err("create requires one explicit Git commit/ref base".into());
    }
    let root = root_path(root)?;
    let owner = owner.canonicalize().map_err(|error| error.to_string())?;
    path_text(&owner)?;
    let owner_identity = crate::workspaces::directory_id(&owner)?;
    let mut git = Git::new(cancel)?;
    if git_path(&git, &owner, "--show-toplevel")? != owner {
        return Err(
            "select the repository worktree root before creating a managed workspace".into(),
        );
    }
    git.protect_checkout(&owner, &root)?;
    if !git
        .run(
            &owner,
            &["status", "--porcelain=v1", "--untracked-files=all"],
            false,
        )?
        .is_empty()
    {
        return Err("managed creation requires a clean committed source workspace; no stash, copy or reset was performed".into());
    }
    let base = git
        .run(
            &owner,
            &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
            false,
        )?
        .trim()
        .to_owned();
    if !object_id(&base) {
        return Err("Git returned an unsupported base commit identity".into());
    }
    let entries = git.run(&owner, &["ls-tree", "-rz", &base], false)?;
    for entry in entries.split('\0').filter(|entry| !entry.is_empty()) {
        let (metadata, entry) = entry.split_once('\t').ok_or("invalid Git tree metadata")?;
        if !metadata.starts_with("100644 blob ") && !metadata.starts_with("100755 blob ") {
            return Err(
                "linked files/submodules in the base are unsupported for managed checkout".into(),
            );
        }
        let path = Path::new(entry);
        if path.is_absolute()
            || !crate::workspaces::relative_valid(path)
            || path
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(
                "base contains sensitive or unsupported paths; no worktree was created".into(),
            );
        }
    }
    let common_git = git_path(&git, &owner, "--git-common-dir")?;
    path_text(&common_git)?;
    let common_identity = crate::workspaces::directory_id(&common_git)?;
    let managed = root.join("managed-worktrees");
    fs::create_dir_all(&managed).map_err(|error| error.to_string())?;
    crate::workspaces::directory_id(&managed)?;
    if managed.canonicalize().ok().as_ref() != Some(&managed) {
        return Err("managed root crosses a linked directory".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let mut record = Record {
        revision: 1,
        owner,
        owner_identity,
        common_git,
        common_identity,
        base,
        branch: format!("concordia/work-{id}"),
        execution: managed.join(&id),
        execution_identity: None,
        id,
        phase: Phase::Creating,
        operation: None,
        note:
            "Creation reserved; native Git mutation may be incomplete. Never reissue automatically."
                .into(),
    };
    mutate(&root, |registry| {
        if registry.records.len() >= MAX_RECORDS {
            return Err(
                "managed registry reached 64 retained workspaces; original work kept".into(),
            );
        }
        if record.execution.exists() {
            return Err("managed path collision; existing path kept".into());
        }
        registry.records.push(record.clone());
        Ok(())
    })?;
    let created = (|| {
        if crate::workspaces::directory_id(&record.owner)? != record.owner_identity
            || git_path(&git, &record.owner, "--git-common-dir")? != record.common_git
            || !git
                .run(
                    &record.owner,
                    &["status", "--porcelain=v1", "--untracked-files=all"],
                    false,
                )?
                .is_empty()
        {
            return Err("source binding or clean base changed before creation".into());
        }
        git.run(
            &record.owner,
            &[
                "worktree",
                "add",
                "-b",
                &record.branch,
                "--",
                path_text(&record.execution)?,
                &record.base,
            ],
            false,
        )?;
        verify_record(&git, &record)?;
        if git
            .run(&record.execution, &["rev-parse", "--verify", "HEAD"], false)?
            .trim()
            != record.base
        {
            return Err("created worktree head differs from approved base".into());
        }
        record.execution_identity = Some(crate::workspaces::directory_id(&record.execution)?);
        Ok(())
    })();
    record.phase = if created.is_ok() {
        Phase::Ready
    } else {
        Phase::ReconcileRequired
    };
    record.revision = 2;
    record.note = created.as_ref().err().cloned().unwrap_or_else(|| "Created and verified. Git worktree isolation is organizational; native filesystem confinement is unverified. Hooks, checkout filters and recursive submodule execution were disabled for creation.".into());
    mutate(&root, |registry| {
        let saved = registry
            .records
            .iter_mut()
            .find(|saved| {
                saved.id == record.id && saved.revision == 1 && saved.phase == Phase::Creating
            })
            .ok_or("creation receipt changed; retain worktree for reconciliation")?;
        *saved = record.clone();
        Ok(())
    })
    .map_err(|error| {
        format!(
            "worktree {} retained; creation result could not be recorded: {error}",
            record.id
        )
    })?;
    created
        .map(|()| record.clone())
        .map_err(|error| format!("worktree {} retained for inspection: {error}", record.id))
}

/// Explicit read-only Git reconciliation; never reissues creation or repairs/deletes files.
pub fn reconcile(
    root: &Path,
    owner: &Path,
    id: &str,
    cancel: &AtomicBool,
) -> Result<Record, String> {
    let registry = load(root)?;
    let mut record = registry
        .records
        .into_iter()
        .find(|record| record.id == id)
        .ok_or("unknown managed workspace")?;
    if owner.canonicalize().ok().as_ref() != Some(&record.owner) {
        return Err("managed workspace belongs to another owner".into());
    }
    if record.phase == Phase::Retained && record.operation.is_some() {
        return lifecycle::reconcile_retained(root, &record, cancel);
    }
    if !matches!(record.phase, Phase::Creating | Phase::ReconcileRequired) {
        return Err("only uncertain creations require explicit reconciliation".into());
    }
    let mut git = Git::new(cancel)?;
    git.protect_checkout(&record.owner, &root_path(root)?)?;
    verify_record(&git, &record)?;
    if git
        .run(&record.execution, &["rev-parse", "--verify", "HEAD"], false)?
        .trim()
        != record.base
        || !git
            .run(
                &record.execution,
                &["status", "--porcelain=v1", "--untracked-files=all"],
                false,
            )?
            .is_empty()
    {
        return Err(
            "uncertain worktree changed or differs from its approved base; retained for inspection"
                .into(),
        );
    }
    let before = record.clone();
    record.execution_identity = Some(crate::workspaces::directory_id(&record.execution)?);
    record.phase = Phase::Ready;
    record.revision = record
        .revision
        .checked_add(1)
        .ok_or("managed revision overflow")?;
    record.note = "Explicitly reconciled exact base/branch/repository and clean worktree; no Git mutation was replayed. Filesystem confinement remains unverified.".into();
    mutate(root, |registry| {
        let saved = registry
            .records
            .iter_mut()
            .find(|saved| **saved == before)
            .ok_or("managed creation changed during reconciliation")?;
        *saved = record.clone();
        Ok(())
    })?;
    Ok(record)
}
