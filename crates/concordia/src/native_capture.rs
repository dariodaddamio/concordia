//! Larger capture snapshots are separate from the small interactive history index.
use super::*;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::io::{BufRead, BufReader};

const TURN_BYTES: u64 = 256 * 1024 * 1024;
const RECORD_BYTES: usize = 64 * 1024 * 1024;
const RECORDS: usize = 8192;
const META_BYTES: u64 = 2 * 1024 * 1024;
const CHUNK: usize = 64 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tool {
    id: String,
    call: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    span: Span,
    tools: Vec<Tool>,
    text_unavailable: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    start: u64,
    bytes: usize,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    historical: bool,
    source: SourceIndex,
    start: u64,
    checkpoint: Checkpoint,
    records: Vec<Record>,
}

/// Only protocol metadata and explicit plaintext fields are materialized. In particular,
/// image/PDF data, tool arguments/results and encrypted payloads are streamed past, not cloned.
struct Filter {
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for Filter {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, decoder: D) -> Result<Value, D::Error> {
        if self.depth > 64 {
            return Err(serde::de::Error::custom(
                "native projection nesting exceeds its budget",
            ));
        }
        decoder.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Filter {
    type Value = Value;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a bounded native projection")
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E: serde::de::Error>(self, _v: bool) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_i64<E: serde::de::Error>(self, _v: i64) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_u64<E: serde::de::Error>(self, _v: u64) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_f64<E: serde::de::Error>(self, _v: f64) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(if value.len() <= TEXT_BYTES {
            Value::String(value.to_owned())
        } else {
            Value::Bool(true)
        })
    }
    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Value, E> {
        Ok(if value.len() <= TEXT_BYTES {
            Value::String(value)
        } else {
            Value::Bool(true)
        })
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = seq.next_element_seed(Filter {
            depth: self.depth + 1,
        })? {
            if values.len() == RECORDS {
                return Err(serde::de::Error::custom(
                    "native projection array exceeds its budget",
                ));
            }
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if key.len() > 4096 || values.contains_key(&key) {
                return Err(serde::de::Error::custom(
                    "invalid or duplicate native protocol field",
                ));
            }
            if key == "encrypted_content" {
                map.next_value::<IgnoredAny>()?;
                values.insert(key, Value::Bool(true));
            } else if matches!(
                key.as_str(),
                "type"
                    | "role"
                    | "payload"
                    | "message"
                    | "content"
                    | "text"
                    | "params"
                    | "update"
                    | "sessionId"
                    | "method"
                    | "sessionUpdate"
                    | "call_id"
                    | "toolCallId"
                    | "id"
                    | "tool_use_id"
            ) {
                let value = map.next_value_seed(Filter {
                    depth: self.depth + 1,
                })?;
                values.insert(key, value);
            } else {
                map.next_value::<IgnoredAny>()?;
            }
            if values.len() > 64 {
                return Err(serde::de::Error::custom(
                    "native projection object exceeds its budget",
                ));
            }
        }
        Ok(Value::Object(values))
    }
}

fn project(raw: &[u8]) -> Result<Value, String> {
    std::str::from_utf8(raw).map_err(|_| "native capture record is not UTF-8")?;
    let mut decoder = serde_json::Deserializer::from_slice(raw);
    let value = Filter { depth: 0 }
        .deserialize(&mut decoder)
        .map_err(|_| "malformed or over-budget native capture record")?;
    decoder
        .end()
        .map_err(|_| "native capture record has trailing data")?;
    Ok(value)
}

fn record(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, String> {
    let mut line = Vec::new();
    loop {
        let buffer = reader.fill_buf().map_err(|e| e.to_string())?;
        if buffer.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err("native capture ends in an unfinished source record".into())
            };
        }
        let count = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |end| end + 1);
        if line.len().saturating_add(count) > RECORD_BYTES {
            return Err("native capture record exceeds its 64 MiB budget".into());
        }
        let complete = buffer[count - 1] == b'\n';
        line.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if complete {
            return Ok(Some(line));
        }
    }
}

fn checkpoint(file: &mut Source, end: u64) -> Result<Checkpoint, String> {
    let start = end.saturating_sub(CHUNK as u64);
    let bytes = (end - start) as usize;
    Ok(Checkpoint {
        start,
        bytes,
        sha256: digest(&read_at(file, start, bytes)?),
    })
}

fn open(root: &Path, binding: &Binding) -> Result<(Source, SourceIndex), String> {
    if let Ok(source) = open_bound(root, binding) {
        return Ok(source);
    }
    // Claude can begin a source with one attachment-bearing user record larger than the
    // ordinary index's identity header. Prove that exact first record without expanding
    // the interactive reader's limits or accepting a path-only session guess.
    if binding.provider != ProviderKind::Claude {
        return open_bound(root, binding);
    }
    let allowed = binding.verify(root)?;
    let (_, path) = sessions::find_native_record(
        root,
        &binding.account,
        Some(binding.provider),
        &binding.session_id,
    )?
    .ok_or("explicit native session was not found")?;
    let canonical = fs::canonicalize(path).map_err(|_| "native source unavailable")?;
    let relative_file = canonical
        .strip_prefix(&allowed)
        .map_err(|_| "native source escapes its bound session tree")?
        .to_path_buf();
    let mut file = Source::File(open_source(&canonical, &allowed)?);
    let identity = file.identity()?;
    let source_len = file.len()?;
    let anchor_len = source_len.min(HEAD_BYTES);
    let head = read_at(&mut file, 0, anchor_len as usize)?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let line = record(&mut BufReader::with_capacity(
        CHUNK,
        (&mut file).take(RECORD_BYTES as u64),
    ))?
    .ok_or("native source has no identity record")?;
    let metadata = project(&line)?;
    if metadata["sessionId"].as_str() != Some(binding.session_id.as_str()) {
        return Err("native source does not confirm its exact session identity".into());
    }
    let source = SourceIndex {
        version: 1,
        binding: Binding {
            home: normalize_home(&binding.home)?,
            ..binding.clone()
        },
        relative_file,
        file_identity: identity,
        source_len,
        anchor_len,
        anchor_sha256: digest(&head),
        messages: Vec::new(),
        truncated: false,
        history: None,
    };
    let _ = open_index_source(root, &source)?;
    Ok((file, source))
}

fn manifest_dir(root: &Path, create: bool) -> Result<PathBuf, String> {
    let root = crate::update::resolved_directory(root).ok_or("native state root is unavailable")?;
    let mut path = root;
    for part in [".ao", "native-memory", "captures"] {
        let child = path.join(part);
        if create && !child.exists() {
            fs::create_dir(&child).map_err(|e| e.to_string())?;
        }
        let resolved = crate::update::resolved_directory(&child)
            .ok_or("native capture directory is unavailable or linked")?;
        if resolved.parent() != Some(path.as_path()) {
            return Err("native capture directory escapes its state root".into());
        }
        path = resolved;
    }
    Ok(path)
}

fn save(root: &Path, snapshot: &Manifest) -> Result<String, String> {
    let _ = open_index_source(root, &snapshot.source)?;
    let bytes = serde_json::to_vec(snapshot).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > META_BYTES {
        return Err("native capture metadata exceeds its 2 MiB budget".into());
    }
    let id = digest(&bytes);
    let dir = manifest_dir(root, true)?;
    let path = dir.join(format!("{id}.json"));
    if path.exists() && !crate::update::regular_owned_file(&path, &dir) {
        return Err("native capture metadata is not an owned regular file".into());
    }
    crate::state_io::write_atomic(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(id)
}

fn load_manifest(root: &Path, id: &str) -> Result<Manifest, String> {
    if !valid_id(id) {
        return Err("invalid native capture manifest identity".into());
    }
    let dir = manifest_dir(root, false)?;
    let path = dir.join(format!("{id}.json"));
    if !crate::update::regular_owned_file(&path, &dir) {
        return Err("native capture metadata is unavailable or linked".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "native capture metadata is unavailable")?
        .take(META_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > META_BYTES || digest(&bytes) != id {
        return Err("native capture metadata is corrupt or replaced".into());
    }
    let snapshot: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| "malformed native capture metadata")?;
    let source = &snapshot.source;
    if snapshot.version != 2
        || source.version != 1
        || !source.messages.is_empty()
        || source.history.is_some()
        || source.anchor_len > HEAD_BYTES
        || !valid_id(&source.anchor_sha256)
        || snapshot.start > source.source_len
        || source.source_len - snapshot.start > TURN_BYTES
        || snapshot.records.len() > RECORDS
        || snapshot.checkpoint.bytes > CHUNK
        || snapshot
            .checkpoint
            .start
            .checked_add(snapshot.checkpoint.bytes as u64)
            != Some(source.source_len)
        || !valid_id(&snapshot.checkpoint.sha256)
    {
        return Err("invalid native capture metadata bounds".into());
    }
    let mut end = snapshot.start;
    for record in &snapshot.records {
        let span = &record.span;
        if span.start < end
            || span.len == 0
            || span.len > RECORD_BYTES
            || !valid_id(&span.sha256)
            || !matches!(span.role.as_str(), "user" | "assistant" | "tool")
            || record.tools.len() > 64
            || record.tools.iter().any(|tool| {
                tool.id.is_empty() || tool.id.len() > 256 || tool.id.chars().any(char::is_control)
            })
        {
            return Err("invalid native capture record metadata".into());
        }
        end = span
            .start
            .checked_add(span.len as u64)
            .filter(|end| *end <= source.source_len)
            .ok_or("native capture record range overflow")?;
    }
    Ok(snapshot)
}

pub(super) fn begin(root: &Path, binding: &Binding) -> Result<CaptureScope, String> {
    let (mut file, source) = open(root, binding)?;
    if source.source_len > 0 && read_at(&mut file, source.source_len - 1, 1)? != b"\n" {
        return Err("native source has an unfinished record at its capture watermark".into());
    }
    let snapshot = Manifest {
        version: 2,
        historical: false,
        start: source.source_len,
        checkpoint: checkpoint(&mut file, source.source_len)?,
        source,
        records: Vec::new(),
    };
    let before = save(root, &snapshot)?;
    Ok(CaptureScope {
        version: 2,
        binding: snapshot.source.binding,
        before: Some(before),
        watermark: snapshot.start,
        sealed: None,
    })
}

pub(super) fn fresh(root: &Path, binding: &Binding) -> Result<CaptureScope, String> {
    let (_, source) = open(root, binding)?;
    Ok(CaptureScope {
        version: 2,
        binding: source.binding,
        before: None,
        watermark: 0,
        sealed: None,
    })
}

fn tools(value: &Value, provider: ProviderKind) -> Vec<Tool> {
    if value.get("encrypted_content").is_some()
        || value["payload"].get("encrypted_content").is_some()
        || value["params"]["update"].get("encrypted_content").is_some()
    {
        return Vec::new();
    }
    if provider == ProviderKind::Claude {
        let Some(blocks) = value["message"]["content"].as_array() else {
            return Vec::new();
        };
        if blocks.iter().any(|block| {
            matches!(
                block["type"].as_str(),
                Some("thinking" | "redacted_thinking" | "reasoning")
            ) || block.get("encrypted_content").is_some()
        }) {
            return Vec::new();
        }
        return blocks
            .iter()
            .filter_map(|block| {
                let (id, call) = if block["type"] == "tool_use" {
                    (block["id"].as_str()?, true)
                } else if block["type"] == "tool_result" {
                    (block["tool_use_id"].as_str()?, false)
                } else {
                    return None;
                };
                (!id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)).then(
                    || Tool {
                        id: id.into(),
                        call,
                    },
                )
            })
            .collect();
    }
    let ids = match provider {
        ProviderKind::Codex => value["payload"]["call_id"]
            .as_str()
            .into_iter()
            .collect::<Vec<_>>(),
        ProviderKind::Cursor | ProviderKind::Grok | ProviderKind::Opencode => value["params"]
            ["update"]["toolCallId"]
            .as_str()
            .into_iter()
            .collect(),
        ProviderKind::Claude => {
            unreachable!("provider tool records handled above")
        }
    };
    ids.into_iter()
        .filter_map(|id| {
            if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
                return None;
            }
            tool_kind(value, provider, id).map(|call| Tool {
                id: id.into(),
                call,
            })
        })
        .collect()
}

fn unavailable_role(value: &Value, provider: ProviderKind, session: &str) -> Option<String> {
    if value
        .get("sessionId")
        .and_then(Value::as_str)
        .is_some_and(|id| id != session)
    {
        return None;
    }
    let payload = match provider {
        ProviderKind::Codex => &value["payload"],
        ProviderKind::Claude => &value["message"],
        ProviderKind::Cursor | ProviderKind::Grok | ProviderKind::Opencode => {
            &value["params"]["update"]
        }
    };
    let role = match provider {
        ProviderKind::Codex if value["type"] == "response_item" => payload["role"].as_str()?,
        ProviderKind::Claude if matches!(value["type"].as_str(), Some("user" | "assistant")) => {
            payload["role"]
                .as_str()
                .or_else(|| value["type"].as_str())?
        }
        ProviderKind::Cursor if value["params"]["sessionId"] == session => {
            match payload["sessionUpdate"].as_str()? {
                "user_message_chunk" => "user",
                "agent_message_chunk" => "assistant",
                _ => return None,
            }
        }
        _ => return None,
    };
    if !role_valid(role) {
        return None;
    }
    let content = &payload["content"];
    let oversized = content == &Value::Bool(true)
        || content.as_array().is_some_and(|blocks| {
            blocks.iter().any(|block| {
                matches!(
                    block["type"].as_str(),
                    Some("text" | "input_text" | "output_text")
                ) && block["text"] == Value::Bool(true)
            })
        })
        || (provider == ProviderKind::Cursor
            && content["type"] == "text"
            && content["text"] == Value::Bool(true));
    oversized.then(|| role.into())
}

fn snapshot(root: &Path, scope: &CaptureScope) -> Result<(String, Manifest), String> {
    if scope.version != 2 {
        return Err("unsupported native capture version".into());
    }
    if let Some(id) = &scope.sealed {
        let snapshot = load_manifest(root, id)?;
        if snapshot.source.binding != scope.binding || snapshot.start != scope.watermark {
            return Err("sealed native capture binding changed".into());
        }
        let _ = open_index_source(root, &snapshot.source)?;
        return Ok((id.clone(), snapshot));
    }
    let (mut file, source) = open(root, &scope.binding)?;
    if let Some(id) = &scope.before {
        let before = load_manifest(root, id)?;
        if before.source.binding != scope.binding
            || before.source.source_len != scope.watermark
            || before.source.file_identity != source.file_identity
            || before.source.relative_file != source.relative_file
            || digest(&read_at(
                &mut file,
                before.checkpoint.start,
                before.checkpoint.bytes,
            )?) != before.checkpoint.sha256
        {
            return Err("native source changed across its capture watermark".into());
        }
        let _ = open_index_source(root, &before.source)?;
    } else if scope.watermark != 0 {
        return Err("invalid native capture watermark".into());
    }
    scan(root, scope, file, source)
}

fn scan(
    root: &Path,
    scope: &CaptureScope,
    mut file: Source,
    source: SourceIndex,
) -> Result<(String, Manifest), String> {
    let delta = source
        .source_len
        .checked_sub(scope.watermark)
        .filter(|delta| *delta > 0 && *delta <= TURN_BYTES)
        .ok_or("native capture is missing, stale or exceeds its 256 MiB turn budget")?;
    file.seek(SeekFrom::Start(scope.watermark))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::with_capacity(CHUNK, (&mut file).take(delta));
    let mut offset = scope.watermark;
    let mut records = Vec::new();
    let mut event_records = Vec::new();
    while let Some(raw) = record(&mut reader)? {
        let value = project(&raw)?;
        let unavailable =
            unavailable_role(&value, scope.binding.provider, &scope.binding.session_id);
        if let Some((role, is_event_message)) = unavailable
            .as_ref()
            .map(|role| (role.clone(), false))
            .or_else(|| message_role(&value, scope.binding.provider, &scope.binding.session_id))
        {
            let tools = tools(&value, scope.binding.provider);
            if tools.len() > 64 {
                return Err("native record has too many tool identities".into());
            }
            let record = Record {
                span: Span {
                    start: offset,
                    len: raw.len(),
                    sha256: digest(&raw),
                    role,
                },
                tools,
                text_unavailable: unavailable.is_some(),
            };
            if is_event_message {
                event_records.push(record);
            } else {
                records.push(record);
            }
            if records.len() + event_records.len() > RECORDS {
                return Err("native capture exceeds its 8192-record budget".into());
            }
        }
        offset += raw.len() as u64;
    }
    drop(reader);
    if offset != source.source_len {
        return Err("native capture source length changed during scan".into());
    }
    if !records.iter().any(|record| record.span.role != "tool") {
        records.extend(event_records);
        records.sort_by_key(|record| record.span.start);
    }
    let snapshot = Manifest {
        version: 2,
        historical: false,
        start: scope.watermark,
        checkpoint: checkpoint(&mut file, source.source_len)?,
        source,
        records,
    };
    let id = save(root, &snapshot)?;
    Ok((id, snapshot))
}

pub(super) fn seal(root: &Path, scope: &CaptureScope) -> Result<CaptureScope, String> {
    let (id, _) = snapshot(root, scope)?;
    Ok(CaptureScope {
        sealed: Some(id),
        ..scope.clone()
    })
}

fn selected(
    file: &mut Source,
    manifest: &Manifest,
    ordinal: usize,
) -> Result<(Vec<u8>, Value), String> {
    let record = ordinal
        .checked_sub(1)
        .and_then(|i| manifest.records.get(i))
        .ok_or("native capture record ordinal is unavailable")?;
    let bytes = read_at(file, record.span.start, record.span.len)?;
    if digest(&bytes) != record.span.sha256 {
        return Err("native captured source record changed".into());
    }
    let value = project(&bytes)?;
    if unavailable_role(
        &value,
        manifest.source.binding.provider,
        &manifest.source.binding.session_id,
    )
    .or_else(|| {
        message_role(
            &value,
            manifest.source.binding.provider,
            &manifest.source.binding.session_id,
        )
        .map(|(role, _)| role)
    }) != Some(record.span.role.clone())
    {
        return Err("native captured record role changed".into());
    }
    Ok((bytes, value))
}

struct Search<'a> {
    needle: &'a [u8],
    prefix: Vec<usize>,
    matched: usize,
    position: usize,
    found: Option<usize>,
}
impl<'a> Search<'a> {
    fn new(needle: &'a str) -> Self {
        let needle = needle.as_bytes();
        let mut prefix = vec![0; needle.len()];
        for i in 1..needle.len() {
            let mut matched = prefix[i - 1];
            while matched > 0 && needle[i] != needle[matched] {
                matched = prefix[matched - 1];
            }
            if needle[i] == needle[matched] {
                matched += 1;
            }
            prefix[i] = matched;
        }
        Self {
            needle,
            prefix,
            matched: 0,
            position: 0,
            found: None,
        }
    }
    fn push(&mut self, text: &[u8]) -> Result<(), String> {
        for &byte in text {
            while self.matched > 0 && byte != self.needle[self.matched] {
                self.matched = self.prefix[self.matched - 1];
            }
            if byte == self.needle[self.matched] {
                self.matched += 1;
            }
            self.position += 1;
            if self.matched == self.needle.len() {
                if self
                    .found
                    .replace(self.position - self.needle.len())
                    .is_some()
                {
                    return Err("exact native plaintext is ambiguous in the capture window".into());
                }
                self.matched = self.prefix[self.matched - 1];
            }
        }
        Ok(())
    }
}

pub(super) fn text(
    root: &Path,
    scope: &CaptureScope,
    role: &str,
    expected: &str,
) -> Result<Reference, String> {
    let (id, manifest) = snapshot(root, scope)?;
    let mut file = open_index_source(root, &manifest.source)?;
    // Streaming KMP finds overlapping and cross-record matches without repeatedly copying
    // a growing transcript or retaining any historical plaintext in the manifest.
    let mut search = Search::new(expected);
    let mut records: Vec<(usize, usize, usize)> = Vec::new();
    let mut seen_text = false;
    for (i, record) in manifest
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.span.role == role)
    {
        if record.text_unavailable {
            return Err(
                "native plaintext exceeds its projection budget; uniqueness cannot be established"
                    .into(),
            );
        }
        let (_, value) = selected(&mut file, &manifest, i + 1)?;
        let plain = plaintext(&value, manifest.source.binding.provider)?;
        if seen_text && manifest.source.binding.provider != ProviderKind::Cursor {
            if manifest.historical {
                search.position += 1;
            } else {
                search.push(b"\n")?;
            }
        }
        // A historical snapshot contains separate turns. Exact matching may not invent
        // one request by joining two unrelated native messages/chunks across that history.
        if manifest.historical {
            search.matched = 0;
        }
        let start = search.position;
        search.push(plain.as_bytes())?;
        records.push((i + 1, start, plain.len()));
        seen_text |= !plain.is_empty();
    }
    let found = search
        .found
        .ok_or("exact native plaintext is unavailable")?;
    let end = found + expected.len();
    let mut first = records
        .iter()
        .position(|(_, start, bytes)| start + bytes > found)
        .ok_or("native text match has no record")?;
    if records[first].1 > found {
        first = first
            .checked_sub(1)
            .ok_or("native match has no preceding separator record")?;
        while first > 0 && records[first].2 == 0 {
            first -= 1;
        }
    }
    let last = if manifest.historical {
        first
    } else {
        records
            .iter()
            .rposition(|(_, start, _)| *start <= end)
            .ok_or("native text match has no ending record")?
    };
    if manifest.historical
        && (found < records[first].1 || end > records[first].1 + records[first].2)
    {
        return Err("historical plaintext match crosses native record boundaries".into());
    }
    let messages = records[first..=last]
        .iter()
        .map(|(ordinal, _, _)| *ordinal)
        .collect();
    let start = found - records[first].1;
    if manifest.source.binding.provider != ProviderKind::Cursor {
        let _ = open_index_source(root, &manifest.source)?;
    }
    let reference = Reference {
        version: 2,
        binding: manifest.source.binding.clone(),
        index: id,
        role: role.into(),
        messages,
        start,
        bytes: expected.len(),
        sha256: digest(expected.as_bytes()),
        projection: Projection::PlainText,
        anchor: Some(Segment {
            start,
            bytes: expected.len(),
            sha256: digest(expected.as_bytes()),
        }),
    };
    if super::hydrate_text(root, &reference)? != expected {
        return Err("native plaintext projection accounting changed".into());
    }
    Ok(reference)
}

pub(super) fn tool(root: &Path, scope: &CaptureScope, call_id: &str) -> Result<Reference, String> {
    let (id, manifest) = snapshot(root, scope)?;
    let mut calls = 0;
    let mut outputs = 0;
    let mut messages = Vec::new();
    for (i, record) in manifest.records.iter().enumerate() {
        for tool in record.tools.iter().filter(|tool| tool.id == call_id) {
            if tool.call {
                calls += 1;
            } else {
                outputs += 1;
            }
            messages.push(i + 1);
        }
    }
    if calls > 1
        || (manifest.source.binding.provider != ProviderKind::Cursor && outputs > 1)
        || messages.is_empty()
        || messages.len() > MAX_MESSAGES
    {
        return Err("native tool capture is unavailable or ambiguous".into());
    }
    let raw = join(root, &manifest, &messages, "tool", Some(call_id))?;
    Ok(Reference {
        version: 2,
        binding: manifest.source.binding.clone(),
        index: id,
        role: "tool".into(),
        messages,
        start: 0,
        bytes: raw.len(),
        sha256: digest(raw.as_bytes()),
        projection: Projection::RawJsonRecords {
            call_id: call_id.into(),
        },
        anchor: None,
    })
}

fn join(
    root: &Path,
    manifest: &Manifest,
    messages: &[usize],
    role: &str,
    call_id: Option<&str>,
) -> Result<String, String> {
    if messages.is_empty() || messages.len() > RECORDS {
        return Err("invalid native selected record count".into());
    }
    let mut text = String::new();
    let mut file = open_index_source(root, &manifest.source)?;
    let mut previous = 0;
    for &ordinal in messages {
        if ordinal <= previous {
            return Err("native selected records are not ordered".into());
        }
        previous = ordinal;
        let record = ordinal
            .checked_sub(1)
            .and_then(|i| manifest.records.get(i))
            .ok_or("native selected record is unavailable")?;
        if call_id.is_some() && record.span.len > TEXT_BYTES {
            return Err("native raw tool record exceeds its projection budget".into());
        }
        let (raw, value) = selected(&mut file, manifest, ordinal)?;
        let rendered = if let Some(call_id) = call_id {
            tool_kind(&value, manifest.source.binding.provider, call_id)
                .ok_or("native selected tool identity changed")?;
            std::str::from_utf8(&raw)
                .map_err(|_| "native tool record is not UTF-8")?
                .to_owned()
        } else {
            if record.span.role != role {
                return Err("native selected plaintext role changed".into());
            }
            plaintext(&value, manifest.source.binding.provider)?
        };
        let separator = call_id.is_none()
            && !text.is_empty()
            && manifest.source.binding.provider != ProviderKind::Cursor;
        let budget = if call_id.is_some() {
            TEXT_BYTES
        } else {
            TEXT_BYTES * 3
        };
        if text
            .len()
            .saturating_add(rendered.len())
            .saturating_add(usize::from(separator))
            > budget
        {
            return Err("native selected plaintext exceeds its hydration budget".into());
        }
        if separator {
            text.push('\n');
        }
        text.push_str(&rendered);
    }
    if manifest.source.binding.provider != ProviderKind::Cursor {
        let _ = open_index_source(root, &manifest.source)?;
    }
    Ok(text)
}

pub(super) fn hydrate(root: &Path, reference: &Reference) -> Result<String, String> {
    let manifest = load_manifest(root, &reference.index)?;
    if manifest.source.binding != reference.binding {
        return Err("native reference binding changed".into());
    }
    if manifest.historical
        && matches!(reference.projection, Projection::PlainText)
        && reference.messages.len() != 1
    {
        return Err("historical plaintext reference crosses native record boundaries".into());
    }
    match &reference.projection {
        Projection::PlainText if role_valid(&reference.role) => {
            join(root, &manifest, &reference.messages, &reference.role, None)
        }
        Projection::RawJsonRecords { call_id }
            if reference.role == "tool"
                && reference.start == 0
                && !call_id.is_empty()
                && call_id.len() <= 256
                && !call_id.chars().any(char::is_control)
                && reference.messages.len() <= MAX_MESSAGES =>
        {
            join(root, &manifest, &reference.messages, "tool", Some(call_id))
        }
        _ => Err("native reference projector and role disagree".into()),
    }
}

pub(super) fn historical_scope(root: &Path, source_index: &str) -> Result<CaptureScope, String> {
    let mut source = super::super::load(root, source_index)?;
    let _ = super::super::verify_source(root, source_index)?;
    let file = open_index_source(root, &source)?;
    source.messages.clear();
    source.history = None;
    let scope = CaptureScope {
        version: 2,
        binding: source.binding.clone(),
        before: None,
        watermark: 0,
        sealed: None,
    };
    let (_, mut manifest) = scan(root, &scope, file, source)?;
    manifest.historical = true;
    let id = save(root, &manifest)?;
    Ok(CaptureScope {
        sealed: Some(id),
        ..scope
    })
}

#[cfg(test)]
pub(super) fn fixture_proof() {
    use crate::native_memory::tests::{append, fixture, message};
    use std::io::Write;
    let mut invalid_opaque = b"{\"type\":\"diagnostic\",\"opaque\":\"".to_vec();
    invalid_opaque.push(0xff);
    invalid_opaque.extend_from_slice(b"\"}\n");
    assert!(
        project(&invalid_opaque).is_err(),
        "ignored attachment/tool fields must still be valid UTF-8"
    );
    for provider in [ProviderKind::Codex, ProviderKind::Claude] {
        let dir = tempfile::tempdir().unwrap();
        let (path, session) = fixture(provider, dir.path(), "large-capture-fixture");
        let binding = Binding::new(
            provider,
            "large-capture-fixture",
            &dir.path().join("large-capture-fixture"),
            &session,
        )
        .unwrap();
        let scope = super::begin_scope(dir.path(), &binding).unwrap();
        // A large irrelevant record used to invalidate every later message in the turn.
        let padding = if provider == ProviderKind::Codex {
            json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"oversized-tool","output":"x".repeat(9 * 1024 * 1024)}})
        } else {
            json!({"type":"user","sessionId":session,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"oversized-tool","content":"x".repeat(9 * 1024 * 1024)}]}})
        };
        append(&path, format!("{padding}\n").as_bytes());
        for i in 0..160 {
            let (input, output) = if provider == ProviderKind::Codex {
                (
                    json!({"type":"response_item","payload":{"type":"function_call","call_id":format!("capture-tool-{i}"),"name":"fixture_read","arguments":"unchanged"}}),
                    json!({"type":"response_item","payload":{"type":"function_call_output","call_id":format!("capture-tool-{i}"),"output":"fixture tool result"}}),
                )
            } else {
                (
                    json!({"type":"assistant","sessionId":session,"message":{"role":"assistant","content":[{"type":"tool_use","id":format!("capture-tool-{i}"),"name":"fixture_read","input":{"opaque":"unchanged"}}]}}),
                    json!({"type":"user","sessionId":session,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":format!("capture-tool-{i}"),"content":"fixture tool result"}]}}),
                )
            };
            append(&path, format!("{input}\n{output}\n").as_bytes());
        }
        let full = "TIME\nCONTEXT\noriginal café\ntrailing  ";
        let image = if provider == ProviderKind::Codex {
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":full},{"type":"input_image","image_url":format!("data:image/png;base64,{}", "A".repeat(512 * 1024))}]}})
        } else {
            json!({"type":"user","sessionId":session,"message":{"role":"user","content":[{"type":"text","text":full},{"type":"image","source":{"type":"base64","data":"A".repeat(512 * 1024)}}]}})
        };
        append(&path, format!("{image}\n").as_bytes());
        for part in ["start-record", "middle", "end-record"] {
            let value = if provider == ProviderKind::Codex {
                json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":part}]}})
            } else {
                json!({"type":"assistant","sessionId":session,"message":{"role":"assistant","content":[{"type":"text","text":part}]}})
            };
            append(&path, format!("{value}\n").as_bytes());
        }
        let sealed = super::seal_scope(dir.path(), &scope).unwrap();
        let reference = super::capture_projected_text(
            dir.path(),
            &sealed,
            "user",
            full,
            &("TIME\nCONTEXT\n".len()..full.len()),
        )
        .unwrap();
        assert_eq!(
            super::hydrate_text(dir.path(), &reference).unwrap(),
            "original café\ntrailing  "
        );
        assert_eq!(reference.version, 2);
        let crossing =
            super::capture_text(dir.path(), &sealed, "assistant", "record\nmiddle\nend").unwrap();
        assert_eq!(
            super::hydrate_text(dir.path(), &crossing).unwrap(),
            "record\nmiddle\nend"
        );
        let boundary = super::capture_text(dir.path(), &sealed, "assistant", "\nmiddle").unwrap();
        assert_eq!(
            super::hydrate_text(dir.path(), &boundary).unwrap(),
            "\nmiddle"
        );
        let tool = super::capture_tool(dir.path(), &sealed, "capture-tool-159").unwrap();
        assert!(super::hydrate_text(dir.path(), &tool)
            .unwrap()
            .contains("fixture tool result"));
        assert!(
            super::capture_tool(dir.path(), &sealed, "oversized-tool").is_err(),
            "one over-budget raw output does not prevent other references"
        );
        let (id, manifest) = snapshot(dir.path(), &sealed).unwrap();
        assert!(manifest.records.len() > MAX_MESSAGES);
        assert!(manifest.source.source_len - manifest.start > TAIL_BYTES);
        let encoded = fs::read_to_string(
            manifest_dir(dir.path(), false)
                .unwrap()
                .join(format!("{id}.json")),
        )
        .unwrap();
        assert!(
            !encoded.contains("original café")
                && !encoded.contains("AAAAAA")
                && !encoded.contains("fixture tool result")
        );
        let summary = index_bound(dir.path(), &binding).unwrap();
        assert!(
            summary.messages <= MAX_MESSAGES,
            "interactive index budgets remain unchanged"
        );
        let historical = super::historical_scope(dir.path(), &summary.id).unwrap();
        assert!(
            super::capture_text(dir.path(), &historical, "assistant", "record\nmiddle\nend")
                .is_err(),
            "historical lookup never fabricates one message across separate records"
        );
        append(
            &path,
            message(provider, "append is outside selected historical revision").as_bytes(),
        );
        assert!(super::capture_text(
            dir.path(),
            &historical,
            "user",
            "append is outside selected historical revision"
        )
        .is_err());
        assert_eq!(
            super::hydrate_text(
                dir.path(),
                &super::capture_text(dir.path(), &historical, "user", "original café").unwrap()
            )
            .unwrap(),
            "original café"
        );
        // Unsupported historical formats are rejected without changing original sources.
        let original = fs::read(&path).unwrap();
        let mut old_scope = sealed.clone();
        old_scope.version = 1;
        assert!(super::capture_text(dir.path(), &old_scope, "user", full)
            .unwrap_err()
            .contains("unsupported native capture scope format"));
        let mut old_reference = reference.clone();
        old_reference.version = 1;
        assert!(super::hydrate_text(dir.path(), &old_reference)
            .unwrap_err()
            .contains("unsupported native reference format"));
        assert_eq!(fs::read(&path).unwrap(), original);
        // Mutate only the selected large record beyond the identity header.
        let selected_record = manifest
            .records
            .iter()
            .find(|record| record.span.role == "user")
            .unwrap();
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.seek(SeekFrom::Start(selected_record.span.start))
            .unwrap();
        let mut raw = vec![0; selected_record.span.len];
        file.read_exact(&mut raw).unwrap();
        let offset = raw
            .windows(b"original".len())
            .position(|window| window == b"original")
            .unwrap();
        file.seek(SeekFrom::Start(selected_record.span.start + offset as u64))
            .unwrap();
        file.write_all(b"O").unwrap();
        drop(file);
        assert!(super::hydrate_text(dir.path(), &reference).is_err());
        assert!(
            super::hydrate_text(dir.path(), &tool).is_ok(),
            "unrelated source data never substitutes for the selected tool record"
        );
        if provider == ProviderKind::Claude {
            // A real Claude source may start directly with an attachment-bearing user record.
            fs::write(&path, format!("{image}\n")).unwrap();
            let fresh = super::fresh_scope(dir.path(), &binding).unwrap();
            let reference = super::capture_text(dir.path(), &fresh, "user", full).unwrap();
            assert_eq!(super::hydrate_text(dir.path(), &reference).unwrap(), full);
        }
    }
}
