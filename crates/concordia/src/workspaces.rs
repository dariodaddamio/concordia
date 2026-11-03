//! Cooperative scheduler reservations. These are never a native filesystem sandbox.
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const REGISTRY_BYTES: u64 = 2 * 1024 * 1024;
const MAX_RECORDS: usize = 4096;
pub const MAX_DECLARATIONS: usize = 64;
const SCOPE_BYTES: usize = 8 * 1024;

pub(crate) fn word(input: &str) -> Result<Option<(String, usize)>, String> {
    let mut quoted = false;
    let mut value = String::new();
    let mut started = false;
    for (offset, character) in input.char_indices() {
        if !started && character.is_whitespace() {
            continue;
        }
        started = true;
        if character == '"' {
            quoted = !quoted;
            continue;
        }
        if !quoted && character.is_whitespace() {
            return Ok(Some((value, offset)));
        }
        value.push(character);
    }
    if quoted {
        return Err("unclosed quoted workspace path".into());
    }
    Ok(started.then_some((value, input.len())))
}

pub fn parse_declarations(
    input: &str,
    needs_task: bool,
) -> Result<(Vec<Declaration>, String), String> {
    let mut declarations = Vec::new();
    let mut remaining = input;
    while let Some((token, consumed)) = word(remaining)? {
        remaining = &remaining[consumed..];
        if token == "--" && needs_task {
            if declarations.is_empty() {
                return Err("a scoped task needs at least one declared path".into());
            }
            let task = remaining.trim_start();
            if task.is_empty() {
                return Err("workspace-scoped task is empty".into());
            }
            return Ok((declarations, task.into()));
        }
        let (access, path) = if let Some(path) = token.strip_prefix("read:") {
            (Access::Read, path)
        } else if let Some(path) = token.strip_prefix("write:") {
            (Access::Write, path)
        } else {
            return Err("use read:PATH or write:PATH; quote paths containing spaces".into());
        };
        if path.is_empty() || declarations.len() >= MAX_DECLARATIONS {
            return Err("declare 1..64 explicit nonempty paths".into());
        }
        let path = PathBuf::from(path);
        if !relative_valid(&path) || path.is_absolute() {
            return Err("workspace declarations require safe relative paths".into());
        }
        declarations.push(Declaration { path, access });
    }
    if needs_task {
        return Err("use -- before the scoped task text".into());
    }
    if declarations.is_empty() {
        return Err("no workspace paths declared; use scope default for the whole folder".into());
    }
    Ok((declarations, String::new()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub path: PathBuf,
    pub access: Access,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub version: u8,
    pub owner: PathBuf,
    pub execution: PathBuf,
    pub directory_id: String,
    #[serde(default)]
    pub managed: Option<ManagedBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedBinding {
    pub id: String,
    pub revision: u64,
    pub owner_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub relative: PathBuf,
    pub key: String,
    pub access: Access,
    pub subtree: bool,
    pub node_id: Option<String>,
    pub parent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub binding: Binding,
    pub claims: Vec<Claim>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Held,
    Released,
    ReconcileRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reservation {
    pub run: String,
    pub conversation: String,
    pub attempt: String,
    pub request: Request,
    pub phase: Phase,
    pub confirmed_exit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub version: u8,
    pub revision: u64,
    pub reservations: Vec<Reservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    Reserved,
    Conflict { run: String, path: PathBuf },
    ReconcileRequired,
}

pub fn path_key(path: &Path) -> String {
    #[cfg(windows)]
    {
        path.to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .into()
    }
    #[cfg(not(windows))]
    {
        path.to_string_lossy().trim_end_matches('/').into()
    }
}

pub(crate) fn same_key(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CompareStringOrdinal(
                left: *const u16,
                left_len: i32,
                right: *const u16,
                right_len: i32,
                ignore_case: i32,
            ) -> i32;
        }
        let left: Vec<u16> = left.encode_utf16().collect();
        let right: Vec<u16> = right.encode_utf16().collect();
        let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
        else {
            return true; // Unknown comparison must not admit potentially overlapping writes.
        };
        // Both buffers remain alive for this call. Windows uses its ordinal uppercase table,
        // including for absent leaf names; Rust's Unicode lowercase is not path equivalence.
        matches!(
            unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) },
            0 | 2
        )
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

pub(crate) fn within_key(path: &str, parent: &str) -> bool {
    let mut parts = path.split('/');
    parent.split('/').all(|part| {
        parts
            .next()
            .is_some_and(|candidate| same_key(candidate, part))
    })
}

pub(crate) fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(crate) fn directory_id(path: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || linked(&metadata) {
        return Err("workspace directory is missing, replaced or linked".into());
    }
    node_id(path)
}

fn node_id(path: &Path) -> Result<String, String> {
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .access_mode(0x80)
            .share_mode(7)
            .custom_flags(0x02000000 | 0x00200000)
            .open(path)
            .map_err(|error| error.to_string())?
    };
    #[cfg(not(windows))]
    let file = File::open(path).map_err(|error| error.to_string())?;
    if linked(&file.metadata().map_err(|error| error.to_string())?) {
        return Err("workspace path became a link while opening".into());
    }
    crate::file_identity::of(&file)
}

pub(crate) fn relative_valid(path: &Path) -> bool {
    path.to_string_lossy().len() <= 1024
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
        && !path
            .to_string_lossy()
            .chars()
            .any(|ch| ch.is_control() || ch == ':')
        && path.components().all(|component| {
            let value = component.as_os_str().to_string_lossy();
            if value == "." {
                return true;
            }
            let stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
            !value.ends_with(['.', ' '])
                && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.as_bytes()[3].is_ascii_digit())
        })
        && !crate::workspace_paths::sensitive(path)
}

fn resolve(root: &Path, declaration: &Declaration) -> Result<Claim, String> {
    if declaration.path.is_absolute() || !relative_valid(&declaration.path) {
        return Err(
            "workspace declarations require relative paths without traversal or sensitive entries"
                .into(),
        );
    }
    let relative: PathBuf = declaration
        .path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect();
    let mut candidate = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        candidate.push(component.as_os_str());
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) => {
                if linked(&metadata) {
                    return Err("workspace declaration crosses a symlink or junction".into());
                }
                if index + 1 < components.len() && !metadata.is_dir() {
                    return Err("workspace declaration has a non-directory parent".into());
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && index + 1 == components.len()
                    && declaration.access == Access::Write => {}
            Err(error) => return Err(format!("workspace declaration is unavailable: {error}")),
        }
    }
    let metadata = match fs::symlink_metadata(&candidate) {
        Ok(metadata) if linked(&metadata) => {
            return Err("workspace path became linked while resolving".into())
        }
        Ok(metadata) => Some(metadata),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && declaration.access == Access::Write =>
        {
            None
        }
        Err(error) => return Err(format!("workspace path became unavailable: {error}")),
    };
    let canonical = if metadata.is_some() {
        candidate
            .canonicalize()
            .map_err(|error| error.to_string())?
    } else {
        let parent = candidate
            .parent()
            .ok_or("workspace declaration has no parent")?
            .canonicalize()
            .map_err(|error| error.to_string())?;
        parent.join(
            candidate
                .file_name()
                .ok_or("workspace declaration has no filename")?,
        )
    };
    let root_key = path_key(root);
    let key = path_key(&canonical);
    if !within_key(&key, &root_key) {
        return Err("workspace declaration escapes its execution root".into());
    }
    let subtree = metadata.as_ref().is_some_and(|metadata| metadata.is_dir());
    if metadata
        .as_ref()
        .is_some_and(|metadata| !metadata.is_file() && !metadata.is_dir())
    {
        return Err("workspace declaration is not a regular file or directory".into());
    }
    let node_id = metadata
        .is_some()
        .then(|| node_id(&canonical))
        .transpose()?;
    let parent_id = directory_id(canonical.parent().unwrap_or(root))?;
    Ok(Claim {
        relative,
        key,
        access: declaration.access,
        subtree,
        node_id,
        parent_id,
    })
}

impl Request {
    pub fn whole_workspace_write(&self) -> bool {
        self.claims.len() == 1
            && self.claims[0].relative.as_os_str().is_empty()
            && self.claims[0].access == Access::Write
            && self.claims[0].subtree
    }
    pub fn describe(&self) -> String {
        let text = self
            .claims
            .iter()
            .map(|claim| {
                format!(
                    "{:?}:{}{}",
                    claim.access,
                    if claim.relative.as_os_str().is_empty() {
                        ".".into()
                    } else {
                        claim.relative.to_string_lossy()
                    },
                    if claim.subtree { "/" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        text.chars()
            .filter(|character| !character.is_control())
            .collect()
    }
    pub fn same_scope(&self, other: &Self) -> bool {
        self.binding == other.binding
            && self.claims.len() == other.claims.len()
            && self.claims.iter().zip(&other.claims).all(|(left, right)| {
                same_key(&left.key, &right.key)
                    && left.access == right.access
                    && left.subtree == right.subtree
            })
    }

    fn refreshed(&self, root: &Path) -> Result<Self, String> {
        let declarations: Vec<_> = self
            .claims
            .iter()
            .map(|claim| Declaration {
                path: claim.relative.clone(),
                access: claim.access,
            })
            .collect();
        let current = match &self.binding.managed {
            Some(managed) => Self::managed(root, &self.binding.owner, &managed.id, &declarations)?,
            None => Self::shared(&self.binding.execution, &declarations)?,
        };
        if !self.same_scope(&current) {
            return Err("workspace scope or directory binding changed between attempts".into());
        }
        Ok(current)
    }

    pub fn managed(
        root: &Path,
        owner: &Path,
        id: &str,
        declarations: &[Declaration],
    ) -> Result<Self, String> {
        let record = crate::managed_workspaces::bound(root, owner, id)?;
        let mut request = Self::shared(&record.execution, declarations)?;
        if Some(request.binding.directory_id.clone()) != record.execution_identity {
            return Err("managed execution directory changed during scope capture".into());
        }
        request.binding.version = 2;
        request.binding.owner = record.owner;
        request.binding.managed = Some(ManagedBinding {
            id: record.id,
            revision: record.revision,
            owner_identity: record.owner_identity,
        });
        Ok(request)
    }

    pub fn shared(workspace: &Path, declarations: &[Declaration]) -> Result<Self, String> {
        let execution = workspace
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if execution
            .to_str()
            .is_none_or(|path| path.len() > 4096 || path.chars().any(char::is_control))
        {
            return Err("workspace path has an ambiguous Unicode identity".into());
        }
        if crate::workspace_paths::sensitive(&execution) {
            return Err("workspace root is a sensitive/native-internal path".into());
        }
        let binding = Binding {
            version: 1,
            managed: None,
            owner: execution.clone(),
            directory_id: directory_id(&execution)?,
            execution,
        };
        let declarations = if declarations.is_empty() {
            vec![Declaration {
                path: PathBuf::from("."),
                access: Access::Write,
            }]
        } else {
            declarations.to_vec()
        };
        if declarations.len() > MAX_DECLARATIONS {
            return Err("at most 64 workspace declarations per task".into());
        }
        if declarations
            .iter()
            .map(|declaration| declaration.path.to_string_lossy().len() + 10)
            .sum::<usize>()
            > SCOPE_BYTES
        {
            return Err("workspace declarations exceed the 8 KiB task scope budget".into());
        }
        let mut claims = Vec::new();
        for declaration in &declarations {
            let claim = resolve(&binding.execution, declaration)?;
            if claims
                .iter()
                .any(|old: &Claim| same_key(&old.key, &claim.key) && old.access == claim.access)
            {
                return Err("duplicate workspace declaration".into());
            }
            claims.push(claim);
        }
        Ok(Self { binding, claims })
    }

    pub fn structurally_valid(&self) -> bool {
        let mut seen = std::collections::BTreeSet::new();
        matches!(
            (&self.binding.managed, self.binding.version),
            (None, 1) | (Some(_), 2)
        ) && self.binding.owner.is_absolute()
            && self.binding.execution.is_absolute()
            && self
                .binding
                .execution
                .to_str()
                .is_some_and(|path| path.len() <= 4096 && !path.chars().any(char::is_control))
            && match &self.binding.managed {
                None => self.binding.owner == self.binding.execution,
                Some(managed) => {
                    uuid_valid(&managed.id)
                        && managed.revision > 0
                        && stable_id(&managed.owner_identity)
                }
            }
            && stable_id(&self.binding.directory_id)
            && !self.claims.is_empty()
            && self.claims.len() <= MAX_DECLARATIONS
            && self
                .claims
                .iter()
                .map(|claim| claim.relative.to_string_lossy().len() + 10)
                .sum::<usize>()
                <= SCOPE_BYTES
            && self.claims.iter().all(|claim| {
                relative_valid(&claim.relative)
                    && seen.insert((&claim.key, claim.access == Access::Write))
                    && claim.node_id.as_ref().is_none_or(|id| stable_id(id))
                    && (claim.access != Access::Read || claim.node_id.is_some())
                    && stable_id(&claim.parent_id)
                    && same_key(
                        &claim.key,
                        &path_key(&self.binding.execution.join(&claim.relative)),
                    )
            })
    }

    pub fn revalidate(&self) -> Result<(), String> {
        if self.binding.managed.is_some() {
            return Err("managed scope requires registry-bound validation".into());
        }
        self.validate_paths()
    }

    fn revalidate_in(&self, root: &Path) -> Result<(), String> {
        if let Some(managed) = &self.binding.managed {
            let record = crate::managed_workspaces::bound(root, &self.binding.owner, &managed.id)?;
            if record.revision != managed.revision
                || record.owner_identity != managed.owner_identity
                || record.execution != self.binding.execution
                || record.execution_identity.as_ref() != Some(&self.binding.directory_id)
            {
                return Err("managed registry binding changed since task approval".into());
            }
        }
        self.validate_paths()
    }

    fn validate_paths(&self) -> Result<(), String> {
        if !self.structurally_valid()
            || self.binding.execution.canonicalize().ok().as_ref() != Some(&self.binding.execution)
            || directory_id(&self.binding.execution)? != self.binding.directory_id
        {
            return Err("workspace binding changed; reconcile before dispatch".into());
        }
        for claim in &self.claims {
            let current = resolve(
                &self.binding.execution,
                &Declaration {
                    path: claim.relative.clone(),
                    access: claim.access,
                },
            )?;
            if current != *claim {
                return Err("declared path identity or kind changed while waiting".into());
            }
        }
        Ok(())
    }
}

pub(crate) fn intersects(left: &Claim, right: &Claim) -> bool {
    (left.access == Access::Write || right.access == Access::Write)
        && (same_key(&left.key, &right.key)
            || left
                .node_id
                .as_ref()
                .is_some_and(|id| right.node_id.as_ref() == Some(id))
            || left.subtree && within_key(&right.key, &left.key)
            || right.subtree && within_key(&left.key, &right.key))
}

fn uuid_valid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

pub(crate) fn stable_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b':')
}

fn require_regular(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || linked(&metadata) => {
            Err("workspace registry must use regular owned files".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub fn load(root: &Path) -> Result<Registry, String> {
    let path = root.join("workspace-registry.json");
    require_regular(&path)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Registry {
                version: 1,
                revision: 0,
                reservations: Vec::new(),
            })
        }
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(REGISTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > REGISTRY_BYTES {
        return Err("workspace registry exceeds 2 MiB; original kept".into());
    }
    let registry: Registry = serde_json::from_slice(&bytes)
        .map_err(|error| format!("workspace registry unavailable: {error}; original kept"))?;
    let mut runs = std::collections::BTreeSet::new();
    if !matches!(registry.version, 1..=2)
        || registry.reservations.len() > MAX_RECORDS
        || registry.reservations.iter().any(|reservation| {
            !uuid_valid(&reservation.run)
                || !uuid_valid(&reservation.attempt)
                || !uuid_valid(&reservation.conversation)
                || !reservation.request.structurally_valid()
                || !runs.insert(&reservation.run)
                || reservation.confirmed_exit && reservation.phase != Phase::Released
        })
    {
        return Err("unsupported or invalid workspace registry; original kept".into());
    }
    Ok(registry)
}

fn mutate<T>(
    root: &Path,
    operation: impl FnOnce(&mut Registry) -> Result<T, String>,
) -> Result<T, String> {
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let lock = root.join("workspace-registry.lock");
    require_regular(&lock)?;
    crate::state_io::with_lock(&lock, || {
        let mut registry = load(root).map_err(std::io::Error::other)?;
        let before = serde_json::to_vec(&registry).map_err(std::io::Error::other)?;
        let result = operation(&mut registry).map_err(std::io::Error::other)?;
        if serde_json::to_vec(&registry).map_err(std::io::Error::other)? != before {
            registry.version = 2;
            registry.revision = registry
                .revision
                .checked_add(1)
                .ok_or_else(|| std::io::Error::other("workspace registry revision overflow"))?;
            let bytes = serde_json::to_vec(&registry).map_err(std::io::Error::other)?;
            if bytes.len() as u64 > REGISTRY_BYTES {
                return Err(std::io::Error::other(
                    "workspace registry is full; existing reservations kept",
                ));
            }
            crate::state_io::write_atomic(&root.join("workspace-registry.json"), &bytes)?;
        }
        Ok(result)
    })
    .map_err(|error| error.to_string())
}

pub fn reserve(
    root: &Path,
    request: &Request,
    run: &str,
    conversation: &str,
    attempt: &str,
) -> Result<Admission, String> {
    reserve_mode(root, request, run, conversation, attempt, false)
}

pub(crate) fn reserve_interactive(
    root: &Path,
    request: &Request,
    run: &str,
    conversation: &str,
    attempt: &str,
) -> Result<Admission, String> {
    reserve_mode(root, request, run, conversation, attempt, true)
}

fn reserve_mode(
    root: &Path,
    request: &Request,
    run: &str,
    conversation: &str,
    attempt: &str,
    advisory: bool,
) -> Result<Admission, String> {
    if !uuid_valid(run) || !uuid_valid(conversation) || !uuid_valid(attempt) {
        return Err("workspace reservation needs exact run/conversation/attempt UUIDs".into());
    }
    mutate(root, |registry| {
        request.revalidate_in(root)?;
        if let Some(existing) = registry
            .reservations
            .iter()
            .find(|reservation| reservation.run == run)
        {
            if (existing.request != *request
                && !(existing.phase == Phase::Released && existing.request.same_scope(request)))
                || existing.conversation != conversation
            {
                return Err("workspace reservation identity changed".into());
            }
            if existing.phase == Phase::ReconcileRequired {
                return Ok(Admission::ReconcileRequired);
            }
            if existing.phase == Phase::Held && existing.attempt != attempt {
                return Ok(Admission::ReconcileRequired);
            }
        }
        for other in registry.reservations.iter().filter(|reservation| {
            !advisory && reservation.run != run && reservation.phase != Phase::Released
        }) {
            for claim in &request.claims {
                if other
                    .request
                    .claims
                    .iter()
                    .any(|other| intersects(claim, other))
                {
                    return Ok(Admission::Conflict {
                        run: other.run.clone(),
                        path: claim.relative.clone(),
                    });
                }
            }
        }
        let reservation = Reservation {
            run: run.into(),
            conversation: conversation.into(),
            attempt: attempt.into(),
            request: request.clone(),
            phase: Phase::Held,
            confirmed_exit: false,
        };
        if let Some(existing) = registry
            .reservations
            .iter_mut()
            .find(|reservation| reservation.run == run)
        {
            if existing.phase == Phase::Released && existing.attempt == attempt {
                return Ok(Admission::ReconcileRequired);
            }
            *existing = reservation;
        } else {
            if registry.reservations.len() >= MAX_RECORDS {
                return Err(
                    "workspace registry reached its reservation bound; existing records kept"
                        .into(),
                );
            }
            if registry
                .reservations
                .iter()
                .filter(|reservation| reservation.phase != Phase::Released)
                .count()
                >= 64
            {
                return Err("workspace registry reached 64 active reservations; wait or reconcile existing owners".into());
            }
            registry.reservations.push(reservation);
        }
        Ok(Admission::Reserved)
    })
}

pub fn finish(root: &Path, run: &str, attempt: &str, confirmed_exit: bool) -> Result<(), String> {
    mutate(root, |registry| {
        let Some(reservation) = registry
            .reservations
            .iter_mut()
            .find(|reservation| reservation.run == run)
        else {
            return Ok(());
        };
        if reservation.attempt != attempt {
            return Err("late workspace completion belongs to another launch attempt".into());
        }
        if reservation.phase == Phase::Released {
            return Ok(());
        }
        reservation.phase = if confirmed_exit {
            Phase::Released
        } else {
            Phase::ReconcileRequired
        };
        reservation.confirmed_exit = confirmed_exit;
        Ok(())
    })
}

/// Explicit user reconciliation permits admission again; it does not prove process ownership for retirement.
pub fn acknowledge_stopped(root: &Path, run: &str, conversation: &str) -> Result<(), String> {
    mutate(root, |registry| {
        let reservation = registry
            .reservations
            .iter_mut()
            .find(|reservation| reservation.run == run && reservation.conversation == conversation)
            .ok_or("unknown owned workspace reservation")?;
        if reservation.phase != Phase::ReconcileRequired {
            return Err(
                "only uncertain reservations need explicit stopped-process reconciliation".into(),
            );
        }
        reservation.phase = Phase::Released;
        reservation.confirmed_exit = false;
        Ok(())
    })
}

pub fn writer(root: &Path, path: &Path) -> Result<Option<String>, String> {
    let registry = load(root)?;
    if registry
        .reservations
        .iter()
        .all(|reservation| reservation.phase == Phase::Released)
    {
        return Ok(None);
    }
    if !path.is_absolute() {
        return Err("workspace conflict lookup needs an absolute path".into());
    }
    let path = match path.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => path
            .parent()
            .ok_or("missing workspace file parent")?
            .canonicalize()
            .map_err(|error| error.to_string())?
            .join(path.file_name().ok_or("missing workspace filename")?),
        Err(error) => return Err(error.to_string()),
    };
    let node_id = match fs::symlink_metadata(&path) {
        Ok(_) => Some(node_id(&path)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let claim = Claim {
        relative: PathBuf::new(),
        key: path_key(&path),
        access: Access::Write,
        subtree: false,
        node_id,
        parent_id: String::new(),
    };
    Ok(registry
        .reservations
        .into_iter()
        .find(|reservation| {
            reservation.phase != Phase::Released
                && reservation
                    .request
                    .claims
                    .iter()
                    .any(|other| intersects(&claim, other))
        })
        .map(|reservation| reservation.run))
}

pub fn for_attempt(
    root: &Path,
    request: &Request,
    run: &str,
    attempt: &str,
) -> Result<Request, String> {
    let registry = load(root)?;
    if registry.reservations.iter().any(|reservation| {
        reservation.run == run
            && reservation.attempt != attempt
            && reservation.phase == Phase::Released
            && reservation.request.same_scope(request)
    }) {
        request.refreshed(root)
    } else {
        Ok(request.clone())
    }
}

pub fn recover(root: &Path, run: &str, request: &Request) -> Result<(), String> {
    mutate(root, |registry| {
        if let Some(reservation) = registry
            .reservations
            .iter_mut()
            .find(|reservation| reservation.run == run)
        {
            if !reservation.request.same_scope(request) {
                return Err("recovered workspace receipt does not match its exact run".into());
            }
            if reservation.phase == Phase::Held {
                reservation.phase = Phase::ReconcileRequired;
                reservation.confirmed_exit = false;
            }
        }
        Ok(())
    })
}

pub fn with_write_guard<T>(
    root: &Path,
    path: &Path,
    write: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let lock = root.join("workspace-registry.lock");
    require_regular(&lock)?;
    crate::state_io::with_lock(&lock, || {
        if let Some(run) = writer(root, path).map_err(std::io::Error::other)? {
            return Err(std::io::Error::other(format!(
                "file is reserved by run {run}; buffer retained"
            )));
        }
        write().map_err(std::io::Error::other)
    })
    .map_err(|error| error.to_string())
}

/// Hold the scheduler lock while a user-approved Git action touches whole worktrees.
/// This coordinates Concordia owners; it does not claim to constrain external Git/processes.
pub(crate) struct WorkspaceGuard<'a> {
    root: &'a Path,
    registry: Registry,
    requests: Vec<Request>,
}

impl WorkspaceGuard<'_> {
    fn save(&mut self) -> Result<(), String> {
        self.registry.version = 2;
        self.registry.revision = self
            .registry
            .revision
            .checked_add(1)
            .ok_or("workspace registry revision overflow")?;
        let bytes = serde_json::to_vec(&self.registry).map_err(|error| error.to_string())?;
        if bytes.len() as u64 > REGISTRY_BYTES {
            return Err("workspace registry is full; existing reservations kept".into());
        }
        crate::state_io::write_atomic(&self.root.join("workspace-registry.json"), &bytes)
            .map_err(|error| error.to_string())
    }
    /// The enclosing OS lock proves exclusive admission; persist claims before spawning Git.
    pub(crate) fn reserve(
        &mut self,
        runs: &[String],
        conversation: &str,
        attempt: &str,
    ) -> Result<(), String> {
        if runs.len() != self.requests.len()
            || !uuid_valid(conversation)
            || !uuid_valid(attempt)
            || runs.iter().any(|run| {
                !uuid_valid(run)
                    || self
                        .registry
                        .reservations
                        .iter()
                        .any(|known| known.run == *run)
            })
            || runs.iter().collect::<std::collections::BTreeSet<_>>().len() != runs.len()
            || self.registry.reservations.len() + runs.len() > MAX_RECORDS
            || self
                .registry
                .reservations
                .iter()
                .filter(|known| known.phase != Phase::Released)
                .count()
                + runs.len()
                > 64
        {
            return Err(
                "lifecycle reservation identity/capacity is invalid; Git was not started".into(),
            );
        }
        for (run, request) in runs.iter().zip(&self.requests) {
            request.revalidate()?;
            self.registry.reservations.push(Reservation {
                run: run.clone(),
                conversation: conversation.into(),
                attempt: attempt.into(),
                request: request.clone(),
                phase: Phase::Held,
                confirmed_exit: false,
            });
        }
        self.save()
    }
    pub(crate) fn finish(
        &mut self,
        runs: &[String],
        attempt: &str,
        confirmed: bool,
    ) -> Result<(), String> {
        for run in runs {
            let claim = self
                .registry
                .reservations
                .iter_mut()
                .find(|known| known.run == *run && known.attempt == attempt)
                .ok_or("lifecycle reservation changed; retain for reconciliation")?;
            claim.phase = if confirmed {
                Phase::Released
            } else {
                Phase::ReconcileRequired
            };
            claim.confirmed_exit = confirmed;
        }
        self.save()
    }
}

pub(crate) fn with_workspace_guard<T>(
    root: &Path,
    paths: &[&Path],
    action: impl FnOnce(&mut WorkspaceGuard<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let requests = paths
        .iter()
        .map(|path| Request::shared(path, &[]))
        .collect::<Result<Vec<_>, _>>()?;
    let lock = root.join("workspace-registry.lock");
    require_regular(&lock)?;
    crate::state_io::with_lock(&lock, || {
        let registry = load(root).map_err(std::io::Error::other)?;
        for request in &requests {
            request.revalidate().map_err(std::io::Error::other)?;
            if let Some(other) = registry.reservations.iter().find(|other| {
                other.phase != Phase::Released
                    && request.claims.iter().any(|claim| {
                        other
                            .request
                            .claims
                            .iter()
                            .any(|known| intersects(claim, known))
                    })
            }) {
                return Err(std::io::Error::other(format!(
                    "workspace is reserved by run {}; operation retained",
                    other.run
                )));
            }
        }
        action(&mut WorkspaceGuard {
            root,
            registry,
            requests,
        })
        .map_err(std::io::Error::other)
    })
    .map_err(|error| error.to_string())
}
