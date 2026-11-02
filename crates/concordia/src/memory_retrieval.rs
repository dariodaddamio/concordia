//! Bounded lexical retrieval and explicit source relationships. No persisted conversation text.
use super::*;

const CANDIDATES: usize = 8;
const HITS: usize = 12;

#[derive(Debug, Serialize)]
pub struct Hit {
    pub source: Summary,
    pub message: usize,
    pub role: String,
    pub byte_start: u64,
    pub sha256: String,
    pub preview: String,
    pub relationship: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub hits: Vec<Hit>,
    pub pages: Vec<Summary>,
    pub incomplete: bool,
    pub unavailable: Vec<String>,
}

fn contains_text(value: &Value, query: &str) -> Option<String> {
    match value {
        Value::String(text) if text.to_lowercase().contains(query) => Some(text.clone()),
        Value::Array(items) => items.iter().find_map(|v| contains_text(v, query)),
        Value::Object(fields) => fields
            .iter()
            .filter(|(key, _)| {
                matches!(
                    key.as_str(),
                    "text"
                        | "content"
                        | "message"
                        | "input"
                        | "output"
                        | "arguments"
                        | "params"
                        | "update"
                        | "payload"
                )
            })
            .find_map(|(_, v)| contains_text(v, query)),
        _ => None,
    }
}

/// Search a verified page once, using the same native revision/range checks as exact readback.
fn search_page(root: &Path, id: &str, query: &str, report: &mut Report) -> Result<(), String> {
    let index = load(root, id)?;
    let mut source = open_index_source(root, &index)?;
    let mut hits = Vec::new();
    for (n, span) in index.messages.iter().enumerate() {
        let bytes = read_at(&mut source, span.start, span.len)?;
        if digest(&bytes) != span.sha256 {
            return Err("native message changed since indexing".into());
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid native record")?;
        if let Some(text) = contains_text(&value, query) {
            if report.hits.len() + hits.len() >= HITS {
                report.incomplete = true;
                break;
            }
            // Preview is untrusted display text. Exact original remains available by pointer.
            let preview: String = crate::chat_cli::terminal_safe(&text)
                .chars()
                .take(240)
                .collect();
            hits.push(Hit {
                source: summary(id, &index),
                message: index.history.as_ref().map_or(0, |p| p.message_base) + n + 1,
                role: span.role.clone(),
                byte_start: span.start,
                sha256: span.sha256.clone(),
                preview,
                relationship: "lexical suggestion; not a confirmed relationship",
            });
        }
    }
    // All candidate hits are withheld if the source's identity/revision changed during the scan.
    let _ = open_index_source(root, &index)?;
    report.incomplete |= index.truncated
        || index
            .history
            .as_ref()
            .is_some_and(|p| p.next.is_some() || p.skipped_records > 0);
    report.hits.extend(hits);
    report.pages.push(summary(id, &index));
    Ok(())
}

/// Initial discovery searches at most eight recent native source windows. Explicit source ids
/// search any whole-history page; callers can advance its immutable cursor and search that page.
pub fn recall(
    root: &Path,
    query: &str,
    source: Option<&str>,
    account: Option<&str>,
    workspace: Option<&Path>,
) -> Result<Report, String> {
    if query.trim().is_empty() || query.len() > 512 {
        return Err("recall query must contain 1..512 UTF-8 bytes".into());
    }
    let query = query.to_lowercase();
    let mut report = Report {
        hits: Vec::new(),
        pages: Vec::new(),
        incomplete: false,
        unavailable: Vec::new(),
    };
    if let Some(id) = source {
        let index = load(root, id)?;
        if account.is_some_and(|a| a != index.binding.account) {
            return Err("recall source belongs to another account".into());
        }
        if workspace.is_some() {
            return Err("workspace filter applies to discovery, not an explicit source".into());
        }
        search_page(root, id, &query, &mut report)?;
        return Ok(report);
    }
    let scan = sessions::list_native_records_bounded(root, 1000, account, None);
    report.incomplete = scan.truncated;
    let workspace = workspace
        .map(fs::canonicalize)
        .transpose()
        .map_err(|_| "workspace is unavailable")?;
    let mut candidates = 0;
    for (session, _) in scan.records {
        if let Some(workspace) = &workspace {
            if session
                .cwd
                .as_ref()
                .and_then(|p| fs::canonicalize(p).ok())
                .as_ref()
                != Some(workspace)
            {
                continue;
            }
        }
        if candidates >= CANDIDATES {
            report.incomplete = true;
            break;
        }
        candidates += 1;
        // Cursor replay launches its native read-only client. Broad background discovery never
        // launches up to eight native clients; explicit indexing enables Cursor page retrieval.
        if session.provider == ProviderKind::Cursor {
            report.unavailable.push(format!(
                "{} / {}: index this Cursor session explicitly through ACP",
                session.account, session.session_id
            ));
            report.incomplete = true;
            continue;
        }
        let result = index_session(
            root,
            session.provider,
            &session.account,
            &session.session_id,
        )
        .and_then(|page| search_page(root, &page.id, &query, &mut report));
        if let Err(error) = result {
            report.incomplete = true;
            report.unavailable.push(format!(
                "{} / {}: {error}",
                session.account, session.session_id
            ));
        }
    }
    Ok(report)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub parent: String,
    pub child: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize)]
struct Handoff {
    parent: Binding,
    child: Binding,
    workspace: PathBuf,
}

/// Record an observed source transition without reading history on the UI thread. This records
/// Concordia's handoff, never an inferred native parent. Readback verifies both ends on demand.
pub fn observe_handoff(
    root: &Path,
    parent: &Binding,
    child: &Binding,
    workspace: &Path,
) -> Result<(), String> {
    if parent == child {
        return Ok(());
    }
    parent.account_home(root)?;
    child.account_home(root)?;
    let event = Handoff {
        parent: parent.clone(),
        child: child.clone(),
        workspace: fs::canonicalize(workspace).map_err(|e| e.to_string())?,
    };
    let bytes = serde_json::to_vec(&event).map_err(|e| e.to_string())?;
    let path = directory(root)
        .join("handoffs")
        .join(format!("{}.json", digest(&bytes)));
    crate::state_io::write_atomic(&path, &bytes).map_err(|e| e.to_string())
}

fn link_path(root: &Path, parent: &str, child: &str) -> Result<PathBuf, String> {
    if !valid_id(parent) || !valid_id(child) || parent == child {
        return Err("link needs two distinct source ids".into());
    }
    Ok(directory(root)
        .join("lineage")
        .join(format!("{}-{}.json", parent, child)))
}

/// A link means an explicit user confirmation or an observed Concordia context handoff. Both
/// ends must still pass native revision verification. It does not claim provider-native ancestry.
pub fn link(root: &Path, parent: &str, child: &str, reason: &str) -> Result<(), String> {
    if !matches!(reason, "user-confirmed" | "context-handoff" | "worker") {
        return Err("unsupported link reason".into());
    }
    let path = link_path(root, parent, child)?;
    for id in [parent, child] {
        let index = load(root, id)?;
        let _ = open_index_source(root, &index)?;
    }
    let bytes = serde_json::to_vec(&Link {
        parent: parent.into(),
        child: child.into(),
        reason: reason.into(),
    })
    .map_err(|e| e.to_string())?;
    crate::state_io::write_atomic(&path, &bytes).map_err(|e| e.to_string())
}

pub fn detach(root: &Path, parent: &str, child: &str) -> Result<(), String> {
    let path = link_path(root, parent, child)?;
    let p = load(root, parent)?.binding;
    let c = load(root, child)?.binding;
    if let Ok(entries) = fs::read_dir(directory(root).join("handoffs")) {
        for entry in entries.take(512).flatten() {
            let path = entry.path();
            let event = read_handoff(&path)?;
            if event.parent == p && event.child == c {
                fs::remove_file(path).map_err(|e| e.to_string())?;
            }
        }
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn read_handoff(path: &Path) -> Result<Handoff, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(16385).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16384 {
        return Err("handoff metadata exceeds budget".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid handoff metadata".into())
}

pub fn links(root: &Path, id: &str) -> Result<Value, String> {
    let index = load(root, id)?;
    let _ = open_index_source(root, &index)?;
    let mut rows = Vec::new();
    let mut incomplete = false;
    if let Ok(entries) = fs::read_dir(directory(root).join("lineage")) {
        for (n, entry) in entries.enumerate() {
            if n >= 512 {
                incomplete = true;
                break;
            }
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !name.starts_with(id) && !name.ends_with(&format!("{id}.json")) {
                continue;
            }
            let bytes = File::open(path)
                .and_then(|f| {
                    let mut b = Vec::new();
                    f.take(4096).read_to_end(&mut b)?;
                    Ok(b)
                })
                .map_err(|e| e.to_string())?;
            let edge: Link =
                serde_json::from_slice(&bytes).map_err(|_| "invalid lineage record")?;
            if edge.parent != id && edge.child != id {
                return Err("lineage identity mismatch".into());
            }
            let other = if edge.parent == id {
                &edge.child
            } else {
                &edge.parent
            };
            let available = load(root, other)
                .and_then(|i| open_index_source(root, &i))
                .is_ok();
            rows.push(json!({"link":edge,"source_available":available}));
        }
    }
    if let Ok(entries) = fs::read_dir(directory(root).join("handoffs")) {
        for (n, entry) in entries.enumerate() {
            if n >= 512 {
                incomplete = true;
                break;
            }
            let Ok(entry) = entry else { continue };
            let event = read_handoff(&entry.path())?;
            if event.parent != index.binding && event.child != index.binding {
                continue;
            }
            let other = if event.parent == index.binding {
                &event.child
            } else {
                &event.parent
            };
            let other = index_bound(root, other);
            rows.push(json!({"reason":"observed-context-handoff","parent":event.parent,"child":event.child,"workspace":event.workspace,"other_source":other.as_ref().ok(),"source_available":other.is_ok()}));
            if rows.len() >= 24 {
                incomplete = true;
                break;
            }
        }
    }
    Ok(json!({"source":id,"links":rows,"incomplete":incomplete}))
}

pub fn command(root: &Path, args: &[String]) -> Result<String, String> {
    let usage = "chat recall TEXT [--source INDEX] [--account ID] [--workspace DIR] | chat lineage INDEX | chat link PARENT CHILD | chat detach PARENT CHILD";
    let value = match args.first().map(String::as_str) {
        Some("recall") => {
            let query = args.get(1).ok_or(usage)?;
            let mut source = None;
            let mut account = None;
            let mut workspace = None;
            let mut seen = std::collections::HashSet::new();
            for pair in args[2..].chunks(2) {
                if pair.len() != 2 || !seen.insert(pair[0].as_str()) {
                    return Err(usage.into());
                }
                match pair[0].as_str() {
                    "--source" => source = Some(pair[1].as_str()),
                    "--account" => account = Some(pair[1].as_str()),
                    "--workspace" => workspace = Some(Path::new(&pair[1])),
                    _ => return Err(usage.into()),
                }
            }
            serde_json::to_value(recall(root, query, source, account, workspace)?)
                .map_err(|e| e.to_string())?
        }
        Some("link" | "detach") if args.len() == 3 => {
            if args[0] == "link" {
                link(root, &args[1], &args[2], "user-confirmed")?;
            } else {
                detach(root, &args[1], &args[2])?;
            }
            json!({"ok":true})
        }
        Some("lineage") if args.len() == 2 => links(root, &args[1])?,
        _ => return Err(usage.into()),
    };
    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
}
