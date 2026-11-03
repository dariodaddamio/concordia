//! Explicit groups share the relationship lock/store; suggestions never write membership.
use super::*;

const GROUPS: usize = 128;
const MEMBERSHIPS: usize = 4096;
const RESPONSE_BYTES: usize = 28_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: String,
    pub workspace: PathBuf,
    pub title: String,
    pub seed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Membership {
    pub group: String,
    pub source: String,
    pub attached: bool,
    pub at_ms: u128,
}

fn group_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|parsed| parsed.to_string() == id)
}

pub(super) fn validate(store: &Store) -> Result<(), String> {
    if store.groups.len() > GROUPS
        || store.memberships.len() > MEMBERSHIPS
        || store.version == 1 && (!store.groups.is_empty() || !store.memberships.is_empty())
    {
        return Err("unsupported group version or bounds; original kept".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for group in &store.groups {
        if !group_id(&group.id)
            || !ids.insert(&group.id)
            || !group.workspace.is_absolute()
            || super::super::files::normalize_workspace(group.workspace.to_str().unwrap_or(""))
                .is_none()
            || group.workspace.to_string_lossy().len() > 4096
            || group.title.is_empty()
            || group.title.len() > 512
            || group.title.chars().any(char::is_control)
            || !valid_id(&group.seed)
        {
            return Err("invalid group identity; original kept".into());
        }
    }
    for member in &store.memberships {
        if !ids.contains(&member.group) || !valid_id(&member.source) {
            return Err("invalid group membership; original kept".into());
        }
    }
    Ok(())
}

/// Same main-repository identity used by landing groups, with a bounded worktree pointer read.
pub fn project_root(folder: &Path) -> PathBuf {
    let pointer = folder.join(".git");
    let main = fs::symlink_metadata(&pointer)
        .ok()
        .filter(|metadata| metadata.is_file() && !crate::workspaces::linked(metadata))
        .and_then(|_| File::open(&pointer).ok())
        .and_then(|file| {
            let mut text = String::new();
            file.take(4097).read_to_string(&mut text).ok()?;
            if text.len() > 4096 {
                return None;
            }
            let gitdir = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
            let gitdir = if gitdir.is_absolute() {
                gitdir
            } else {
                folder.join(gitdir)
            };
            let dot_git = gitdir.parent()?.parent()?;
            (dot_git.file_name()? == ".git").then(|| dot_git.parent().map(Path::to_path_buf))?
        });
    let root = main.as_deref().unwrap_or(folder);
    fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

fn member_active(store: &Store, group: &str, source: &str) -> bool {
    store
        .memberships
        .iter()
        .rev()
        .find(|member| member.group == group && member.source == source)
        .is_some_and(|member| member.attached)
}

fn write(
    root: &Path,
    edit: impl FnOnce(&mut Store) -> Result<String, String>,
) -> Result<String, String> {
    fs::create_dir_all(directory(root)).map_err(|error| error.to_string())?;
    crate::workspaces::directory_id(&directory(root))?;
    crate::state_io::with_lock(&directory(root).join("relations.lock"), || {
        let mut store = load_store(root).map_err(std::io::Error::other)?;
        let result = edit(&mut store).map_err(std::io::Error::other)?;
        store.version = 2;
        validate(&store).map_err(std::io::Error::other)?;
        let bytes = serde_json::to_vec(&store).map_err(std::io::Error::other)?;
        if bytes.len() as u64 > STORE_BYTES {
            return Err(std::io::Error::other(
                "relationship store exceeds 2 MiB; original kept",
            ));
        }
        crate::state_io::write_atomic(&store_path(root), &bytes)?;
        Ok(result)
    })
    .map_err(|error| error.to_string())
}

fn membership(store: &mut Store, group: &str, source: &str, attached: bool) -> Result<(), String> {
    if !valid_id(source) {
        return Err("membership needs an exact source ID".into());
    }
    if !store.groups.iter().any(|known| known.id == group) {
        return Err("unknown group ID".into());
    }
    if member_active(store, group, source) == attached {
        return Ok(());
    }
    if store.memberships.len() >= MEMBERSHIPS {
        return Err("group provenance budget is full; original kept".into());
    }
    store.memberships.push(Membership {
        group: group.into(),
        source: source.into(),
        attached,
        at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    });
    Ok(())
}

pub fn create(root: &Path, workspace: &Path, seed: &str) -> Result<String, String> {
    let workspace = fs::canonicalize(workspace).map_err(|_| "workspace unavailable")?;
    if !workspace.is_dir() {
        return Err("workspace must be a directory".into());
    }
    let workspace = project_root(&workspace);
    let source = verify_source(root, seed)?;
    let index = load(root, seed)?;
    let title = crate::session_titles::get(
        root,
        index.binding.provider,
        &index.binding.account,
        &index.binding.home,
        &index.binding.session_id,
    )
    .unwrap_or_else(|| format!("{} / {}", source.account, source.session_id));
    let title: String = crate::chat_cli::terminal_safe(&title)
        .chars()
        .take(100)
        .collect();
    let id = uuid::Uuid::new_v4().to_string();
    write(root, |store| {
        if store.groups.len() >= GROUPS {
            return Err("group store is full; no automatic pruning".into());
        }
        verify_source(root, seed)?;
        store.groups.push(Group {
            id: id.clone(),
            workspace,
            title,
            seed: seed.into(),
        });
        membership(store, &id, seed, true)?;
        Ok(id)
    })
}

pub fn set_member(
    root: &Path,
    group: &str,
    source: &str,
    attached: bool,
) -> Result<String, String> {
    write(root, |store| {
        if attached {
            verify_source(root, source)?;
        }
        membership(store, group, source, attached)?;
        Ok(group.into())
    })
}

fn page(rows: &[Value], offset: usize, revision: &str, extras: Value) -> Result<Value, String> {
    if offset > rows.len() {
        return Err("group offset is outside this snapshot".into());
    }
    let mut result = extras;
    result["revision"] = json!(revision);
    result["rows"] = json!([]);
    result["next_offset"] = Value::Null;
    let mut end = offset;
    for candidate in offset + 1..=rows.len().min(offset + 8) {
        result["rows"] = json!(&rows[offset..candidate]);
        result["next_offset"] = json!((candidate < rows.len()).then_some(candidate));
        if serde_json::to_vec_pretty(&result)
            .map_err(|error| error.to_string())?
            .len()
            > RESPONSE_BYTES
        {
            break;
        }
        end = candidate;
    }
    if end == offset && offset < rows.len() {
        return Err("group row exceeds display budget".into());
    }
    result["rows"] = json!(&rows[offset..end]);
    result["next_offset"] = json!((end < rows.len()).then_some(end));
    Ok(result)
}

pub fn list(
    root: &Path,
    workspace: &Path,
    offset: usize,
    revision: Option<&str>,
) -> Result<Value, String> {
    let store = load_store(root)?;
    let current = digest(&serde_json::to_vec(&store).map_err(|error| error.to_string())?);
    check_revision(offset, revision, &current)?;
    let workspace = project_root(workspace);
    let mut groups: Vec<_> = store
        .groups
        .iter()
        .filter(|group| super::super::rank::same_workspace(&group.workspace, &workspace))
        .collect();
    groups.sort_by(|left, right| left.id.cmp(&right.id));
    let rows: Vec<_> = groups.iter().map(|group| json!({"group":group,"members": store.memberships.iter()
        .filter(|member| member.group == group.id).map(|member| &member.source).collect::<std::collections::BTreeSet<_>>()
        .into_iter().filter(|source| member_active(&store, &group.id, source)).count()})).collect();
    page(
        &rows,
        offset,
        &current,
        json!({"kind":"confirmed groups","source_status":"metadata only; open group-members to check availability"}),
    )
}

pub fn members(
    root: &Path,
    group: &str,
    offset: usize,
    revision: Option<&str>,
) -> Result<Value, String> {
    let store = load_store(root)?;
    let known = store
        .groups
        .iter()
        .find(|known| known.id == group)
        .ok_or("unknown group ID")?;
    let current = digest(&serde_json::to_vec(&store).map_err(|error| error.to_string())?);
    check_revision(offset, revision, &current)?;
    let sources: Vec<_> = store
        .memberships
        .iter()
        .filter(|member| member.group == group)
        .map(|member| &member.source)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|source| member_active(&store, group, source))
        .collect();
    if offset > sources.len() {
        return Err("member offset is outside this snapshot".into());
    }
    // Verify only this response's bounded eight members; missing entries stay present.
    let mut rows = vec![Value::Null; offset];
    for source in sources.iter().skip(offset).take(8) {
        let status = if load(root, source)
            .is_ok_and(|index| index.binding.provider == ProviderKind::Cursor)
        {
            Err("Cursor native replay requires explicit source readback; not started by group discovery".into())
        } else {
            verify_source(root, source)
        };
        rows.push(match status {
            Ok(summary) => json!({"source":source,"status":"verified","native":summary,"provenance":"user-confirmed membership"}),
            Err(error) => json!({"source":source,"status":"unavailable","reason":error,"provenance":"user-confirmed membership"}),
        });
    }
    let mut result = page(
        &rows,
        offset,
        &current,
        json!({"kind":"confirmed membership","group":known}),
    )?;
    let end = offset + result["rows"].as_array().map_or(0, Vec::len);
    result["next_offset"] = json!((end < sources.len()).then_some(end));
    let latest = load_store(root)?;
    if digest(&serde_json::to_vec(&latest).map_err(|error| error.to_string())?) != current {
        return Err("groups changed during read; restart at offset zero".into());
    }
    Ok(result)
}

pub fn command(root: &Path, args: &[String], workspace: &Path) -> Result<String, String> {
    let usage = "groups [OFFSET REVISION] | group-create SOURCE | group-members GROUP [OFFSET REVISION] | group-link GROUP SOURCE | group-unlink GROUP SOURCE";
    let offset = |position: usize| {
        args.get(position)
            .map(|value| value.parse::<usize>().map_err(|_| usage.to_string()))
            .transpose()
            .map(|value| value.unwrap_or(0))
    };
    let value = match args.first().map(String::as_str) {
        Some("groups") if matches!(args.len(), 1 | 3) => {
            list(root, workspace, offset(1)?, args.get(2).map(String::as_str))?
        }
        Some("group-members") if matches!(args.len(), 2 | 4) => {
            members(root, &args[1], offset(2)?, args.get(3).map(String::as_str))?
        }
        Some("group-create") if args.len() == 2 => {
            json!({"created":true,"group":create(root, workspace, &args[1])?})
        }
        Some("group-link" | "group-unlink") if args.len() == 3 => {
            json!({"group":set_member(root, &args[1], &args[2], args[0] == "group-link")?,"attached":args[0] == "group-link"})
        }
        _ => return Err(usage.into()),
    };
    serde_json::to_string_pretty(&value).map_err(|error| error.to_string())
}
