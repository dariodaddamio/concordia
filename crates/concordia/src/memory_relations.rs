//! Explicit goal/source relationships. Native sources stay authoritative; no transcript copies.
use super::*;

const STORE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_RELATIONS: usize = 1024;
const MAX_EVENTS: usize = 4096;
const PAGE_SIZE: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub id: String,
    pub workspace: PathBuf,
    pub goal: String,
    pub source: String,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    relation: String,
    attached: bool,
    at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    version: u8,
    relations: Vec<Relation>,
    changes: Vec<Change>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: 1,
            relations: Vec::new(),
            changes: Vec::new(),
        }
    }
}

fn store_path(root: &Path) -> PathBuf {
    directory(root).join("relations.json")
}

fn relation_id(workspace: &Path, goal: &str, source: &str) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(workspace, goal, source)).map_err(|e| e.to_string())?;
    Ok(digest(&bytes))
}

fn load_store(root: &Path) -> Result<Store, String> {
    let file = match File::open(store_path(root)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(STORE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > STORE_BYTES {
        return Err("relationship store exceeds 2 MiB; original kept".into());
    }
    let store: Store =
        serde_json::from_slice(&bytes).map_err(|_| "invalid relationship store; original kept")?;
    if store.version != 1
        || store.relations.len() > MAX_RELATIONS
        || store.changes.len() > MAX_EVENTS
    {
        return Err("unsupported relationship version or bounds; original kept".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for relation in &store.relations {
        if !relation.workspace.is_absolute()
            || relation.goal.is_empty()
            || relation.goal.len() > 128
            || !valid_id(&relation.source)
            || relation.provenance != "user-confirmed"
            || relation.id != relation_id(&relation.workspace, &relation.goal, &relation.source)?
            || !ids.insert(relation.id.as_str())
        {
            return Err("invalid relationship identity; original kept".into());
        }
    }
    if store
        .changes
        .iter()
        .any(|change| !ids.contains(change.relation.as_str()))
    {
        return Err("relationship change references an unknown relation; original kept".into());
    }
    Ok(store)
}

fn active(store: &Store, id: &str) -> bool {
    store
        .changes
        .iter()
        .rev()
        .find(|change| change.relation == id)
        .is_some_and(|change| change.attached)
}

pub(super) fn ranking_sources(
    root: &Path,
    workspace: Option<&Path>,
    goal: &str,
) -> Result<Vec<Relation>, String> {
    let goals = crate::goals::read_goals(root).map_err(|error| error.to_string())?;
    if !goals.iter().any(|known| known.id == goal) {
        return Err("unknown goal id".into());
    }
    let store = load_store(root)?;
    // Goal links capture the canonical workspace. The current explicit context can arrive
    // through a Windows short path or directory alias; never resolve historical source paths.
    let canonical_workspace = workspace.and_then(|workspace| fs::canonicalize(workspace).ok());
    let mut rows: Vec<_> = store
        .relations
        .iter()
        .filter(|relation| {
            relation.goal == goal
                && active(&store, &relation.id)
                && workspace.is_none_or(|workspace| {
                    super::rank::same_workspace(workspace, &relation.workspace)
                        || canonical_workspace.as_ref().is_some_and(|canonical| {
                            super::rank::same_workspace(canonical, &relation.workspace)
                        })
                })
        })
        .cloned()
        .collect();
    rows.sort_by(|left, right| (&left.source, &left.id).cmp(&(&right.source, &right.id)));
    Ok(rows)
}

fn mutate(
    root: &Path,
    workspace: &Path,
    goal: &str,
    source: &str,
    attach: bool,
) -> Result<String, String> {
    if goal.is_empty() || goal.len() > 128 || !valid_id(source) {
        return Err("invalid goal or source identity".into());
    }
    let workspace = fs::canonicalize(workspace).map_err(|_| "workspace unavailable")?;
    if !workspace.is_dir() {
        return Err("workspace must be a directory".into());
    }
    if attach {
        let goals = crate::goals::read_goals(root).map_err(|e| e.to_string())?;
        if !goals.iter().any(|existing| existing.id == goal) {
            return Err("unknown goal id".into());
        }
    }
    let id = relation_id(&workspace, goal, source)?;
    fs::create_dir_all(directory(root)).map_err(|e| e.to_string())?;
    crate::state_io::with_lock(&directory(root).join("relations.lock"), || {
        let result = (|| -> Result<(), String> {
            let mut store = load_store(root)?;
            if attach {
                verify_source(root, source)?;
            }
            if active(&store, &id) == attach {
                return Ok(());
            }
            if !store.relations.iter().any(|relation| relation.id == id) {
                if store.relations.len() >= MAX_RELATIONS {
                    return Err("relationship store is full".into());
                }
                store.relations.push(Relation {
                    id: id.clone(),
                    workspace: workspace.clone(),
                    goal: goal.into(),
                    source: source.into(),
                    provenance: "user-confirmed".into(),
                });
            }
            if store.changes.len() >= MAX_EVENTS {
                return Err("relationship provenance budget is full; original kept".into());
            }
            store.changes.push(Change {
                relation: id.clone(),
                attached: attach,
                at_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis(),
            });
            let bytes = serde_json::to_vec(&store).map_err(|e| e.to_string())?;
            if bytes.len() as u64 > STORE_BYTES {
                return Err("relationship store exceeds 2 MiB; original kept".into());
            }
            crate::state_io::write_atomic(&store_path(root), &bytes).map_err(|e| e.to_string())
        })();
        result.map_err(std::io::Error::other)
    })
    .map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn attach(root: &Path, workspace: &Path, goal: &str, source: &str) -> Result<String, String> {
    mutate(root, workspace, goal, source, true)
}

pub fn detach(root: &Path, workspace: &Path, goal: &str, source: &str) -> Result<String, String> {
    mutate(root, workspace, goal, source, false)
}

/// Metadata reads do not replay native transports. Exact source validation is an explicit read.
pub fn list(
    root: &Path,
    workspace: Option<&Path>,
    goal: Option<&str>,
    offset: usize,
    revision: Option<&str>,
) -> Result<Value, String> {
    let workspace = workspace
        .map(fs::canonicalize)
        .transpose()
        .map_err(|_| "workspace unavailable")?;
    let store = load_store(root)?;
    let current_revision = digest(&serde_json::to_vec(&store).map_err(|e| e.to_string())?);
    check_revision(offset, revision, &current_revision)?;
    let rows: Vec<_> = store
        .relations
        .iter()
        .filter(|relation| {
            active(&store, &relation.id)
                && workspace
                    .as_ref()
                    .is_none_or(|workspace| workspace == &relation.workspace)
                && goal.is_none_or(|goal| goal == relation.goal)
        })
        .collect();
    if offset > rows.len() {
        return Err("relationship offset is outside this snapshot".into());
    }
    let mut end = offset;
    let mut result_bytes = 0;
    for row in rows.iter().skip(offset).take(PAGE_SIZE) {
        let bytes = serde_json::to_vec(row).map_err(|e| e.to_string())?.len();
        if result_bytes + bytes > 12_000 {
            break;
        }
        result_bytes += bytes;
        end += 1;
    }
    if end == offset && offset < rows.len() {
        return Err("relationship metadata row exceeds the response budget".into());
    }
    Ok(
        json!({"relations": &rows[offset..end], "next_offset": (end < rows.len()).then_some(end),
        "revision": current_revision,
        "source_status": "metadata only; use exact source readback to verify current availability"}),
    )
}

fn check_revision(offset: usize, revision: Option<&str>, current: &str) -> Result<(), String> {
    if (offset > 0 && revision.is_none()) || revision.is_some_and(|revision| revision != current) {
        return Err(
            "metadata snapshot changed or cursor lacks its revision; restart from offset zero"
                .into(),
        );
    }
    Ok(())
}

pub fn goals(root: &Path, offset: usize, revision: Option<&str>) -> Result<Value, String> {
    let goals = crate::goals::read_goals(root).map_err(|e| e.to_string())?;
    let current_revision = digest(&serde_json::to_vec(&goals).map_err(|e| e.to_string())?);
    check_revision(offset, revision, &current_revision)?;
    if offset > goals.len() {
        return Err("goal offset is outside this snapshot".into());
    }
    let end = (offset + PAGE_SIZE).min(goals.len());
    let rows: Vec<_> = goals[offset..end].iter().map(|goal| {
        let text = crate::chat_cli::terminal_safe(&goal.text);
        json!({"id": goal.id, "text": text.chars().take(240).collect::<String>(), "text_truncated": text.chars().count() > 240,
            "done":goal.done, "added_at":goal.added_at, "closed_at":goal.closed_at})
    }).collect();
    Ok(
        json!({"goals": rows, "next_offset": (end < goals.len()).then_some(end), "revision": current_revision}),
    )
}

pub fn command(root: &Path, args: &[String], workspace: &Path) -> Result<String, String> {
    let usage = "goals [OFFSET REVISION] | goal-sources GOAL [OFFSET REVISION] | goal-link GOAL SOURCE | goal-unlink GOAL SOURCE";
    let offset = |value: Option<&String>| {
        value
            .map(|value| value.parse::<usize>().map_err(|_| usage.to_string()))
            .transpose()
            .map(|value| value.unwrap_or(0))
    };
    let value = match args.first().map(String::as_str) {
        Some("goals") if matches!(args.len(), 1 | 3) => {
            goals(root, offset(args.get(1))?, args.get(2).map(String::as_str))?
        }
        Some("goal-sources") if matches!(args.len(), 2 | 4) => list(
            root,
            Some(workspace),
            Some(&args[1]),
            offset(args.get(2))?,
            args.get(3).map(String::as_str),
        )?,
        Some("goal-link" | "goal-unlink") if args.len() == 3 => {
            let id = mutate(root, workspace, &args[1], &args[2], args[0] == "goal-link")?;
            json!({"ok": true, "relation": id, "attached": args[0] == "goal-link"})
        }
        _ => return Err(usage.into()),
    };
    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
}
