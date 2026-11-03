//! Historical file facts from verified native tool records. Referenced files are never opened.
use super::*;

const MAX_FACTS: usize = 64;
const CACHE_BYTES: u64 = 128 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    pub message: usize,
    pub source_byte_start: u64,
    pub sha256: String,
    pub path: PathBuf,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub version: u8,
    pub source: Summary,
    pub workspace: Option<PathBuf>,
    pub workspace_anchor_sha256: String,
    pub facts: Vec<Fact>,
    pub overflow: bool,
    pub limits: Vec<String>,
    pub incomplete: bool,
    pub unsupported_tool_records: usize,
    pub record_bytes_read: u64,
    pub unavailable: Vec<String>,
}

fn native_path(value: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let value = if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else if let Some(dos) = value.strip_prefix(r"\\?\") {
            if dos.as_bytes().get(1) != Some(&b':') {
                return None;
            }
            dos.into()
        } else {
            value.into()
        };
        let path = PathBuf::from(value);
        if path.components().any(|component| matches!(component, std::path::Component::Prefix(prefix) if !matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::UNC(_, _)))) { return None; }
        Some(path)
    }
    #[cfg(not(windows))]
    {
        Some(PathBuf::from(value))
    }
}

pub(super) fn normalize_workspace(value: &str) -> Option<PathBuf> {
    if value.len() > 4096 || value.chars().any(char::is_control) {
        return None;
    }
    let path = native_path(value)?;
    if !path.is_absolute()
        || crate::workspace_paths::sensitive(&path)
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    Some(path.components().collect())
}

fn reported_workspace(index: &SourceIndex, header: &[u8]) -> Option<PathBuf> {
    let mut workspace: Option<PathBuf> = None;
    for line in header.split(|byte| *byte == b'\n') {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let reported = match index.binding.provider {
            ProviderKind::Codex
                if value["type"] == "session_meta"
                    && value["payload"]["id"] == index.binding.session_id =>
            {
                value.pointer("/payload/cwd").and_then(Value::as_str)
            }
            ProviderKind::Claude if value["sessionId"] == index.binding.session_id => {
                value.get("cwd").and_then(Value::as_str)
            }
            _ => None,
        };
        if let Some(reported) = reported {
            let next = normalize_workspace(reported)?;
            if workspace.as_ref().is_some_and(|previous| {
                !crate::workspaces::same_key(
                    &crate::workspaces::path_key(previous),
                    &crate::workspaces::path_key(&next),
                )
            }) {
                return None;
            }
            workspace = Some(next);
        }
    }
    workspace
}

/// Lexical historical matching only. Neither existence nor a current symlink target is inferred.
pub fn relative_path(workspace: &Path, value: &str) -> Option<PathBuf> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return None;
    }
    let workspace = normalize_workspace(workspace.to_str()?)?;
    let path = native_path(value)?;
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    let target: PathBuf = if path.is_absolute() {
        path.components().collect()
    } else {
        workspace.join(path).components().collect()
    };
    if !crate::workspaces::within_key(
        &crate::workspaces::path_key(&target),
        &crate::workspaces::path_key(&workspace),
    ) {
        return None;
    }
    let relative: PathBuf = target
        .components()
        .skip(workspace.components().count())
        .collect();
    (!relative.as_os_str().is_empty() && crate::workspaces::relative_valid(&relative))
        .then_some(relative)
}

impl Page {
    pub(super) fn new(id: &str, index: &SourceIndex, header: &[u8]) -> Self {
        let workspace = reported_workspace(index, header);
        Self {
            version: 1,
            source: summary(id, index),
            workspace: workspace.clone(),
            workspace_anchor_sha256: index.anchor_sha256.clone(),
            facts: Vec::new(),
            overflow: false,
            limits: Vec::new(),
            incomplete: workspace.is_none()
                || index.truncated
                || index
                    .history
                    .as_ref()
                    .is_some_and(|page| page.next.is_some() || page.skipped_records > 0),
            unsupported_tool_records: 0,
            record_bytes_read: 0,
            unavailable: if workspace.is_none() {
                vec!["verified native header does not establish one supported absolute workspace; file matching unavailable".into()]
            } else {
                Vec::new()
            },
        }
    }

    fn limit(&mut self, reason: &str) {
        self.overflow = true;
        self.incomplete = true;
        if !self.limits.iter().any(|known| known == reason) {
            self.limits.push(reason.into());
        }
    }

    fn push(&mut self, index: &SourceIndex, position: usize, value: &str, evidence: &str) {
        let Some(workspace) = &self.workspace else {
            return;
        };
        let Some(path) = relative_path(workspace, value) else {
            return;
        };
        let span = &index.messages[position];
        let message = index.history.as_ref().map_or(0, |page| page.message_base) + position + 1;
        if self.facts.iter().any(|fact| {
            fact.message == message
                && crate::workspaces::same_key(
                    &crate::workspaces::path_key(&fact.path),
                    &crate::workspaces::path_key(&path),
                )
        }) {
            return;
        }
        if self.facts.len() >= MAX_FACTS {
            self.limit("64 file facts per source page");
            return;
        }
        self.facts.push(Fact {
            message,
            source_byte_start: span.start,
            sha256: span.sha256.clone(),
            path,
            evidence: evidence.into(),
        });
    }

    fn fields(
        &mut self,
        index: &SourceIndex,
        position: usize,
        value: &Value,
        depth: usize,
        remaining: &mut usize,
    ) {
        if depth > 8 {
            self.limit("tool path schema depth 8");
            return;
        }
        if *remaining == 0 {
            self.limit("512 tool path nodes per record");
            return;
        }
        *remaining -= 1;
        match value {
            Value::Array(values) => {
                for value in values {
                    if *remaining == 0 {
                        self.limit("512 tool path nodes per record");
                        break;
                    }
                    self.fields(index, position, value, depth + 1, remaining);
                }
            }
            Value::Object(fields) => {
                for (key, value) in fields {
                    if *remaining == 0 {
                        self.limit("512 tool path nodes per record");
                        break;
                    }
                    *remaining -= 1;
                    if matches!(
                        key.as_str(),
                        "path" | "file_path" | "target_file" | "notebook_path"
                    ) {
                        if let Some(path) = value.as_str() {
                            self.push(index, position, path, "path in a native tool request; execution/current disk state unverified");
                        }
                    } else if key == "paths" {
                        if let Some(values) = value.as_array() {
                            for value in values {
                                if *remaining == 0 {
                                    self.limit("512 tool path nodes per record");
                                    break;
                                }
                                *remaining -= 1;
                                if let Some(path) = value.as_str() {
                                    self.push(index, position, path, "path in a native tool request; execution/current disk state unverified");
                                }
                            }
                        }
                    } else if matches!(key.as_str(), "edits" | "input" | "arguments") {
                        self.fields(index, position, value, depth + 1, remaining);
                    }
                }
            }
            _ => {}
        }
    }

    fn patch(&mut self, index: &SourceIndex, position: usize, patch: &str) {
        let mut remaining = 512;
        for line in patch.lines() {
            for prefix in [
                "*** Add File: ",
                "*** Update File: ",
                "*** Delete File: ",
                "*** Move to: ",
            ] {
                if let Some(path) = line.strip_prefix(prefix) {
                    if remaining == 0 {
                        self.limit("512 patch path mentions per record");
                        return;
                    }
                    remaining -= 1;
                    self.push(
                        index,
                        position,
                        path,
                        "path in a native patch request; execution/current disk state unverified",
                    );
                }
            }
        }
    }

    pub(super) fn observe(&mut self, index: &SourceIndex, position: usize, value: &Value) {
        if self.facts.len() == MAX_FACTS && self.overflow {
            return;
        }
        match index.binding.provider {
            ProviderKind::Codex
                if value["type"] == "response_item"
                    && matches!(
                        value["payload"]["type"].as_str(),
                        Some("function_call" | "custom_tool_call")
                    ) =>
            {
                let payload = &value["payload"];
                let name = payload["name"].as_str().unwrap_or("");
                if matches!(name, "apply_patch" | "functions.apply_patch") {
                    if let Some(patch) = payload.get("input").and_then(Value::as_str) {
                        self.patch(index, position, patch);
                    }
                    if let Some(arguments) = payload.get("arguments").and_then(Value::as_str) {
                        if let Ok(arguments) = serde_json::from_str::<Value>(arguments) {
                            if let Some(patch) = arguments.get("patch").and_then(Value::as_str) {
                                self.patch(index, position, patch);
                            }
                        } else {
                            self.patch(index, position, arguments);
                        }
                    }
                } else if matches!(
                    name,
                    "read_file"
                        | "write_file"
                        | "edit_file"
                        | "functions.read_file"
                        | "functions.write_file"
                        | "functions.edit_file"
                ) {
                    if let Some(arguments) = payload.get("arguments") {
                        let decoded = arguments
                            .as_str()
                            .and_then(|text| serde_json::from_str::<Value>(text).ok());
                        self.fields(
                            index,
                            position,
                            decoded.as_ref().unwrap_or(arguments),
                            0,
                            &mut 512,
                        );
                    }
                } else {
                    self.unsupported_tool_records += 1;
                }
            }
            ProviderKind::Claude if value["type"] == "assistant" => {
                let mut remaining = 512;
                if let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) {
                    for block in blocks {
                        if remaining == 0 {
                            self.limit("512 tool path nodes per record");
                            break;
                        }
                        remaining -= 1;
                        if block["type"] != "tool_use" {
                            continue;
                        }
                        if matches!(
                            block["name"].as_str(),
                            Some("Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit")
                        ) {
                            self.fields(index, position, &block["input"], 0, &mut remaining);
                        } else {
                            self.unsupported_tool_records += 1;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Rebuild from verified source bytes before publication; a cache is never evidence by itself.
pub fn read_page(root: &Path, id: &str) -> Result<Page, String> {
    let index = load(root, id)?;
    if index.binding.provider == ProviderKind::Cursor {
        return Err("Cursor file facts are unavailable: its current history adapter does not establish a verified workspace header".into());
    }
    let mut source = open_index_source(root, &index)?;
    let header = read_at(&mut source, 0, index.anchor_len as usize)?;
    let mut page = Page::new(id, &index, &header);
    for (position, span) in index.messages.iter().enumerate() {
        let bytes = read_at(&mut source, span.start, span.len)?;
        page.record_bytes_read += bytes.len() as u64;
        if digest(&bytes) != span.sha256 {
            return Err("native message changed; file facts withheld".into());
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| "invalid native record; file facts withheld")?;
        page.observe(&index, position, &value);
    }
    if page.unsupported_tool_records > 0 {
        page.incomplete = true;
    }
    let _ = open_index_source(root, &index)?;
    save_page(root, id, &page)?;
    Ok(page)
}

fn save_page(root: &Path, id: &str, page: &Page) -> Result<(), String> {
    let directory = directory(root).join("file-facts");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    crate::workspaces::directory_id(&directory)?;
    let lock = directory.join(format!("{id}.lock"));
    regular(&lock)?;
    crate::state_io::with_lock(&lock, || {
        save_locked(&directory.join(format!("{id}.json")), id, page).map_err(std::io::Error::other)
    })
    .map_err(|error| error.to_string())
}

fn regular(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || crate::workspaces::linked(&metadata) => {
            Err("file-fact metadata must remain regular owned files".into())
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn save_locked(path: &Path, id: &str, page: &Page) -> Result<(), String> {
    let current = serde_json::to_vec(page).map_err(|error| error.to_string())?;
    if current.len() as u64 > CACHE_BYTES {
        return Err("file facts exceed their 128 KiB page budget".into());
    }
    if regular(path)? {
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|error| error.to_string())?
            .take(CACHE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 > CACHE_BYTES {
            return Err("file-fact metadata exceeds 128 KiB; original kept".into());
        }
        let previous: Page = serde_json::from_slice(&bytes)
            .map_err(|_| "file-fact metadata is invalid; original kept")?;
        if previous.version != 1 || previous.source.id != id {
            return Err("unsupported file-fact metadata; original kept".into());
        }
        if serde_json::to_vec(&previous).map_err(|error| error.to_string())? != current {
            return Err(
                "file-fact metadata differs for this immutable source page; original kept".into(),
            );
        }
        return Ok(());
    }
    crate::state_io::write_atomic(path, &current).map_err(|error| error.to_string())
}

pub fn list(root: &Path, id: &str, offset: usize, revision: Option<&str>) -> Result<Value, String> {
    let page = read_page(root, id)?;
    let current = digest(&serde_json::to_vec(&page).map_err(|error| error.to_string())?);
    if offset > page.facts.len()
        || offset > 0 && revision.is_none()
        || revision.is_some_and(|revision| revision != current)
    {
        return Err(
            "file-fact snapshot changed or cursor lacks its revision; restart at offset zero"
                .into(),
        );
    }
    let mut result = json!({"source": page.source, "workspace": page.workspace, "workspace_anchor_sha256": page.workspace_anchor_sha256,
        "facts": [], "revision": current, "next_offset": null, "incomplete": page.incomplete, "limits":page.limits,
        "unsupported_tool_records":page.unsupported_tool_records, "record_bytes_read":page.record_bytes_read, "unavailable":page.unavailable,
        "meaning":"historical paths in verified native tool requests; referenced files were not opened and current disk state is unverified"});
    let bytes = serde_json::to_vec_pretty(&result)
        .map_err(|error| error.to_string())?
        .len();
    if bytes > 28_000 {
        return Err("file-fact response metadata exceeds its display budget".into());
    }
    let mut end = offset;
    for candidate in offset + 1..=page.facts.len().min(offset + 16) {
        result["facts"] = serde_json::to_value(&page.facts[offset..candidate])
            .map_err(|error| error.to_string())?;
        result["next_offset"] = json!((candidate < page.facts.len()).then_some(candidate));
        let length = serde_json::to_vec_pretty(&result)
            .map_err(|error| error.to_string())?
            .len();
        if length > 28_000 {
            break;
        }
        end = candidate;
    }
    if end == offset && offset < page.facts.len() {
        return Err("file-fact row exceeds its display budget".into());
    }
    result["facts"] =
        serde_json::to_value(&page.facts[offset..end]).map_err(|error| error.to_string())?;
    result["next_offset"] = json!((end < page.facts.len()).then_some(end));
    Ok(result)
}

pub fn command(root: &Path, args: &[String]) -> Result<Value, String> {
    let usage = "file-facts SOURCE [OFFSET REVISION]";
    if !matches!(args.len(), 2 | 4) || args[0] != "file-facts" {
        return Err(usage.into());
    }
    let offset = args
        .get(2)
        .map(|offset| offset.parse::<usize>().map_err(|_| usage.to_string()))
        .transpose()?
        .unwrap_or(0);
    list(root, &args[1], offset, args.get(3).map(String::as_str))
}
