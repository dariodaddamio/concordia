//! Body-free projections into one explicitly owned native turn's appended source records.
//! Full-text logs and unfinished request recovery are deliberately outside this module.
use super::*;

const TEXT_BYTES: usize = 1024 * 1024;

#[path = "native_capture.rs"]
mod capture;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CaptureScope {
    version: u32,
    binding: Binding,
    before: Option<String>,
    watermark: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sealed: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    version: u32,
    binding: Binding,
    index: String,
    role: String,
    messages: Vec<usize>,
    start: usize,
    bytes: usize,
    sha256: String,
    projection: Projection,
    anchor: Option<Segment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Segment {
    start: usize,
    bytes: usize,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum Projection {
    PlainText,
    RawJsonRecords { call_id: String },
}

impl CaptureScope {
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
}

impl Reference {
    pub(crate) fn current_format(&self) -> bool {
        self.version == 2
    }
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn role(&self) -> &str {
        &self.role
    }
    pub fn native_call_id(&self) -> Option<&str> {
        match &self.projection {
            Projection::PlainText => None,
            Projection::RawJsonRecords { call_id } => Some(call_id),
        }
    }
}

/// Bind an explicitly observed NEW native session, after the provider creates its source.
/// The caller must have captured `native_session_id=None` BEFORE dispatch and received this
/// exact session id from that launch. Missing history/resume never authorizes this constructor.
pub fn fresh_scope(root: &Path, binding: &Binding) -> Result<CaptureScope, String> {
    capture::fresh(root, binding)
}

/// Capture before dispatch, while the exact existing session has no turn in flight.
pub fn begin_scope(root: &Path, binding: &Binding) -> Result<CaptureScope, String> {
    capture::begin(root, binding)
}

fn role_valid(role: &str) -> bool {
    matches!(role, "user" | "assistant")
}

/// Only documented plaintext message fields are projected; reasoning/tool blocks are excluded.
fn plaintext(value: &Value, provider: ProviderKind) -> Result<String, String> {
    let payload = match provider {
        ProviderKind::Codex => &value["payload"],
        ProviderKind::Claude => &value["message"],
        ProviderKind::Cursor | ProviderKind::Grok | ProviderKind::Opencode => {
            &value["params"]["update"]
        }
    };
    if payload.get("encrypted_content").is_some()
        || matches!(
            payload["type"].as_str(),
            Some("reasoning" | "thinking" | "redacted_thinking")
        )
    {
        return Err("native reasoning is not a plaintext message projection".into());
    }
    if provider == ProviderKind::Codex && value["type"] == "event_msg" {
        return payload["message"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "native event has no supported plaintext".into());
    }
    let content = &payload["content"];
    let result = if let Some(text) = content.as_str() {
        text.to_owned()
    } else if provider == ProviderKind::Cursor {
        if content["type"] != "text" {
            return Err("Cursor native record is not plaintext".into());
        }
        content["text"]
            .as_str()
            .ok_or("Cursor native plaintext is unavailable")?
            .to_owned()
    } else if let Some(blocks) = content.as_array() {
        blocks
            .iter()
            .filter(|block| {
                matches!(
                    block["type"].as_str(),
                    Some("text" | "input_text" | "output_text")
                )
            })
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        return Err("native record has no supported plaintext projection".into());
    };
    if result.len() > TEXT_BYTES {
        return Err("native plaintext exceeds the capture budget".into());
    }
    Ok(result)
}

/// Freeze a completed turn before another request may append to the same native session.
pub fn seal_scope(root: &Path, scope: &CaptureScope) -> Result<CaptureScope, String> {
    require_current_scope(scope)?;
    capture::seal(root, scope)
}

fn require_current_scope(scope: &CaptureScope) -> Result<(), String> {
    if scope.version != 2 {
        return Err("unsupported native capture scope format; original source retained; start a new explicit capture instead of replaying this scope".into());
    }
    Ok(())
}

/// Capture a UNIQUE exact plaintext segment without persisting the caller's expected body.
pub fn capture_text(
    root: &Path,
    scope: &CaptureScope,
    role: &str,
    expected: &str,
) -> Result<Reference, String> {
    if !role_valid(role) || expected.is_empty() || expected.len() > TEXT_BYTES {
        return Err("unsupported native capture role or text budget".into());
    }
    require_current_scope(scope)?;
    capture::text(root, scope, role, expected)
}

/// Bind the complete planned native input first, then retain a caller-defined UTF-8 segment.
/// The caller supplies structural offsets, never a guessed occurrence of the composer text.
pub fn capture_projected_text(
    root: &Path,
    scope: &CaptureScope,
    role: &str,
    full_expected: &str,
    projection: &std::ops::Range<usize>,
) -> Result<Reference, String> {
    let projected = full_expected
        .get(projection.clone())
        .ok_or("native input projection is outside its UTF-8 boundaries")?;
    if projected.is_empty() && role != "user" {
        return Err("only an anchored user input may have an empty composer projection".into());
    }
    let mut reference = capture_text(root, scope, role, full_expected)?;
    reference.start = reference
        .start
        .checked_add(projection.start)
        .ok_or("native input projection offset overflow")?;
    reference.bytes = projected.len();
    reference.sha256 = digest(projected.as_bytes());
    Ok(reference)
}

fn tool_kind(value: &Value, provider: ProviderKind, call_id: &str) -> Option<bool> {
    if value.get("encrypted_content").is_some()
        || value["payload"].get("encrypted_content").is_some()
        || value["params"]["update"].get("encrypted_content").is_some()
    {
        return None;
    }
    match provider {
        ProviderKind::Codex => {
            let payload = &value["payload"];
            if value["type"] != "response_item" || payload["call_id"] != call_id {
                return None;
            }
            match payload["type"].as_str()? {
                "function_call" | "custom_tool_call" => Some(true),
                "function_call_output" | "custom_tool_call_output" => Some(false),
                _ => None,
            }
        }
        ProviderKind::Claude => {
            let blocks = value["message"]["content"].as_array()?;
            // Whole raw records containing private/encrypted reasoning are never projected.
            if blocks.iter().any(|block| {
                matches!(
                    block["type"].as_str(),
                    Some("thinking" | "redacted_thinking" | "reasoning")
                ) || block.get("encrypted_content").is_some()
            }) {
                return None;
            }
            let matches: Vec<bool> = blocks
                .iter()
                .filter_map(|block| match block["type"].as_str()? {
                    "tool_use" if block["id"] == call_id => Some(true),
                    "tool_result" if block["tool_use_id"] == call_id => Some(false),
                    _ => None,
                })
                .collect();
            (matches.len() == 1).then(|| matches[0])
        }
        ProviderKind::Cursor | ProviderKind::Grok | ProviderKind::Opencode => {
            let update = &value["params"]["update"];
            if update["toolCallId"] != call_id {
                return None;
            }
            match update["sessionUpdate"].as_str()? {
                "tool_call" => Some(true),
                "tool_call_update" => Some(false),
                _ => None,
            }
        }
    }
}

/// Exact scoped native tool JSONL records, without interpreting success or recreating UI cards.
pub fn capture_tool(root: &Path, scope: &CaptureScope, call_id: &str) -> Result<Reference, String> {
    if call_id.is_empty() || call_id.len() > 256 || call_id.chars().any(char::is_control) {
        return Err("invalid native tool capture identity".into());
    }
    require_current_scope(scope)?;
    capture::tool(root, scope, call_id)
}

/// Reconstruct only the originally verified UTF-8 segment; no stale/foreign fallback.
pub fn hydrate_text(root: &Path, reference: &Reference) -> Result<String, String> {
    if !reference.current_format() {
        return Err("unsupported native reference format; original source retained; re-index and explicitly select the original source again".into());
    }
    if reference.bytes > TEXT_BYTES || !valid_id(&reference.sha256) {
        return Err("unsupported native text reference".into());
    }
    let text = capture::hydrate(root, reference)?;
    match &reference.anchor {
        Some(anchor) if matches!(reference.projection, Projection::PlainText) => {
            let end = anchor
                .start
                .checked_add(anchor.bytes)
                .ok_or("native input anchor overflow")?;
            let full = text
                .get(anchor.start..end)
                .ok_or("native input anchor is outside UTF-8 boundaries")?;
            if anchor.bytes == 0
                || anchor.bytes > TEXT_BYTES
                || !valid_id(&anchor.sha256)
                || digest(full.as_bytes()) != anchor.sha256
                || reference.start < anchor.start
                || reference
                    .start
                    .checked_add(reference.bytes)
                    .is_none_or(|projected_end| projected_end > end)
                || (reference.bytes == 0 && reference.role != "user")
            {
                return Err("native input anchor or projection changed".into());
            }
        }
        None if matches!(reference.projection, Projection::RawJsonRecords { .. })
            && reference.bytes > 0 => {}
        _ => return Err("native reference has no valid input anchor".into()),
    }
    let end = reference
        .start
        .checked_add(reference.bytes)
        .ok_or("native text projection overflow")?;
    let projected = text
        .get(reference.start..end)
        .ok_or("native text projection no longer has valid UTF-8 boundaries")?;
    if digest(projected.as_bytes()) != reference.sha256 {
        return Err("native text projection changed".into());
    }
    Ok(projected.to_owned())
}

/// Explicit historical read intent, pinned to an existing immutable native source index.
/// This is never a replacement for pre-dispatch/fresh-session ownership checks.
pub fn historical_scope(root: &Path, source_index: &str) -> Result<CaptureScope, String> {
    capture::historical_scope(root, source_index)
}

#[cfg(test)]
pub(super) fn fixture_proof() {
    capture::fixture_proof();
    {
        let dir = tempfile::tempdir().unwrap();
        let (path, session) =
            super::tests::fixture(ProviderKind::Codex, dir.path(), "sealed-fixture");
        let binding = Binding::new(
            ProviderKind::Codex,
            "sealed-fixture",
            &dir.path().join("sealed-fixture"),
            &session,
        )
        .unwrap();
        let scope = begin_scope(dir.path(), &binding).unwrap();
        let body = super::tests::message(ProviderKind::Codex, "same sealed request");
        super::tests::append(&path, body.as_bytes());
        let sealed = seal_scope(dir.path(), &scope).unwrap();
        super::tests::append(&path, body.as_bytes());
        assert!(capture_text(dir.path(), &scope, "user", "same sealed request").is_err());
        let reference = capture_text(dir.path(), &sealed, "user", "same sealed request").unwrap();
        assert_eq!(
            hydrate_text(dir.path(), &reference).unwrap(),
            "same sealed request"
        );
    }
    for provider in [ProviderKind::Codex, ProviderKind::Claude] {
        let dir = tempfile::tempdir().unwrap();
        let (path, session) = super::tests::fixture(provider, dir.path(), "reference-fixture");
        let binding = Binding::new(
            provider,
            "reference-fixture",
            &dir.path().join("reference-fixture"),
            &session,
        )
        .unwrap();
        super::tests::append(
            &path,
            super::tests::message(provider, "same old request").as_bytes(),
        );
        let scope = begin_scope(dir.path(), &binding).unwrap();
        assert!(capture_text(dir.path(), &scope, "user", "same old request").is_err());
        super::tests::append(
            &path,
            super::tests::message(provider, "prefix\nexact 日本語\nbody\nsuffix").as_bytes(),
        );
        let reference = capture_text(dir.path(), &scope, "user", "exact 日本語\nbody").unwrap();
        assert_eq!(
            hydrate_text(dir.path(), &reference).unwrap(),
            "exact 日本語\nbody"
        );
        let serialized = serde_json::to_string(&reference).unwrap();
        assert!(!serialized.contains("exact") && !serialized.contains("日本語"));
        assert!(!serde_json::to_string(&scope)
            .unwrap()
            .contains("same old request"));
        assert_eq!(reference.binding(), &binding);
        assert_eq!(reference.role(), "user");
        assert!(reference.native_call_id().is_none());
        let projected_scope = begin_scope(dir.path(), &binding).unwrap();
        let full = "prefix ok\nORIGINAL ok\nsuffix";
        super::tests::append(&path, super::tests::message(provider, full).as_bytes());
        assert!(capture_text(dir.path(), &projected_scope, "user", "ok").is_err());
        let start = "prefix ok\nORIGINAL ".len();
        let projected = capture_projected_text(
            dir.path(),
            &projected_scope,
            "user",
            full,
            &(start..start + 2),
        )
        .unwrap();
        assert_eq!(hydrate_text(dir.path(), &projected).unwrap(), "ok");
        let empty =
            capture_projected_text(dir.path(), &projected_scope, "user", full, &(start..start))
                .unwrap();
        assert_eq!(hydrate_text(dir.path(), &empty).unwrap(), "");
        assert!(capture_projected_text(
            dir.path(),
            &projected_scope,
            "user",
            full,
            &(0..full.len() + 1)
        )
        .is_err());
        assert!(
            capture_projected_text(dir.path(), &projected_scope, "user", "前ok", &(1..2)).is_err()
        );
        assert!(capture_text(
            dir.path(),
            &projected_scope,
            "user",
            &"x".repeat(TEXT_BYTES + 1)
        )
        .is_err());
        assert!(capture_text(dir.path(), &projected_scope, "tool", "ok").is_err());
        let tools_scope = begin_scope(dir.path(), &binding).unwrap();
        let tool_input = if provider == ProviderKind::Codex {
            json!({"type":"response_item","payload":{"type":"function_call","call_id":"synthetic-call","name":"read_file","arguments":"synthetic"}})
        } else {
            json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"synthetic-call","name":"read_file","input":{"path":"synthetic"}}]}})
        };
        let tool_output = if provider == ProviderKind::Codex {
            json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"synthetic-call","output":"synthetic result"}})
        } else {
            json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"synthetic-call","content":"synthetic result"}]}})
        };
        super::tests::append(&path, format!("{tool_input}\n{tool_output}\n").as_bytes());
        let tool_reference = capture_tool(dir.path(), &tools_scope, "synthetic-call").unwrap();
        assert_eq!(tool_reference.role(), "tool");
        assert_eq!(tool_reference.native_call_id(), Some("synthetic-call"));
        let tool_raw = format!("{tool_input}\n{tool_output}\n");
        assert_eq!(hydrate_text(dir.path(), &tool_reference).unwrap(), tool_raw);
        assert!(!serde_json::to_string(&tool_reference)
            .unwrap()
            .contains("synthetic result"));
        assert!(capture_tool(dir.path(), &tools_scope, "another-call").is_err());
        let log = crate::threads::ThreadLog::open(dir.path(), "reference-tool-proof");
        let event = crate::threads::ThreadEvent::card(
            "Read",
            Some("synthetic pretty detail"),
            "done",
        )
        .who(provider.as_str(), "reference-fixture", "fixture-model");
        log.append_referenced(&event, tool_reference).unwrap();
        let journal = fs::read_to_string(
            crate::threads::threads_dir(dir.path()).join("reference-tool-proof.jsonl"),
        )
        .unwrap();
        assert!(
            !journal.contains("synthetic result")
                && !journal.contains("synthetic pretty detail")
                && !journal.contains("read_file")
        );
        let restored = log.read_all().unwrap();
        assert!(restored[0]
            .card
            .as_ref()
            .unwrap()
            .detail
            .as_ref()
            .unwrap()
            .starts_with("Native tool records (source data; not replayed):\n"));
        assert!(restored[0]
            .card
            .as_ref()
            .unwrap()
            .detail
            .as_ref()
            .unwrap()
            .contains(&tool_raw));
        super::tests::append(
            &path,
            super::tests::message(provider, "later append").as_bytes(),
        );
        assert_eq!(
            hydrate_text(dir.path(), &reference).unwrap(),
            "exact 日本語\nbody"
        );
        let duplicate = begin_scope(dir.path(), &binding).unwrap();
        super::tests::append(&path, super::tests::message(provider, "aaa").as_bytes());
        assert!(capture_text(dir.path(), &duplicate, "user", "aa").is_err());
        let original = fs::read_to_string(&path).unwrap();
        fs::write(&path, original.replace("exact 日本語", "other 日本語")).unwrap();
        assert!(hydrate_text(dir.path(), &reference).is_err());
        let mut wrong = reference.clone();
        wrong.binding.account = "another-account".into();
        assert!(hydrate_text(dir.path(), &wrong).is_err());
    }
}
