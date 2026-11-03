//! Native requests stay bound to the owning run, launch and live protocol connection.
use crate::store::ProviderKind;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::oneshot;

const REQUEST_BYTES: usize = 256 * 1024;
const REQUESTS_PER_CONNECTION: usize = 1024;
const PENDING_REQUESTS: usize = 8;
type ResponseChannel = Mutex<Option<oneshot::Sender<Value>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionOwner {
    pub conversation_id: String,
    pub workspace: PathBuf,
    pub execution_cwd: PathBuf,
    pub run_id: String,
    pub attempt_id: String,
    pub provider: ProviderKind,
    pub account: String,
    pub home: Option<PathBuf>,
}

impl InteractionOwner {
    /// Resolve identities before launch; a missing home is never an implicit account match.
    pub fn validate_for(
        &self,
        provider: ProviderKind,
        cwd: &Path,
        home: Option<&Path>,
    ) -> Result<Self, String> {
        for id in [&self.conversation_id, &self.run_id, &self.attempt_id] {
            uuid::Uuid::parse_str(id)
                .map_err(|_| "native interaction owner needs UUID identities")?;
        }
        if self.provider != provider || self.account.trim().is_empty() || self.account.len() > 256 {
            return Err("native interaction provider/account mismatch".into());
        }
        let canonical = |path: &Path| {
            if !path.is_dir() {
                return Err("native interaction owner path is not a directory".to_owned());
            }
            std::fs::canonicalize(path)
                .map_err(|_| "native interaction owner path is unavailable".to_owned())
        };
        let owner_home = self
            .home
            .as_deref()
            .ok_or("native interaction home is missing")?;
        let requested_home = home.ok_or("native interaction launch home is missing")?;
        let execution_cwd = canonical(&self.execution_cwd)?;
        let owner_home = canonical(owner_home)?;
        if execution_cwd != canonical(cwd)? || owner_home != canonical(requested_home)? {
            return Err("native interaction workspace/home mismatch".into());
        }
        Ok(Self {
            workspace: canonical(&self.workspace)?,
            execution_cwd,
            home: Some(owner_home),
            ..self.clone()
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Supported,
    Unsupported,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityEvidence {
    pub support: Support,
    pub transport: String,
    pub cli_version: Option<String>,
    pub protocol_version: Option<u64>,
    pub connection_id: Option<String>,
    pub source: String,
    pub observed_at_unix_ms: Option<u64>,
}

impl Default for CapabilityEvidence {
    fn default() -> Self {
        Self {
            support: Support::Unknown,
            transport: "unknown".into(),
            cli_version: None,
            protocol_version: None,
            connection_id: None,
            source: "no verified native evidence".into(),
            observed_at_unix_ms: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InteractionCapabilities {
    pub permissions: CapabilityEvidence,
    pub questions: CapabilityEvidence,
    pub plans: CapabilityEvidence,
    pub context_capacity: CapabilityEvidence,
    pub compaction_control: CapabilityEvidence,
    pub images: CapabilityEvidence,
    pub load_session: CapabilityEvidence,
}

impl InteractionCapabilities {
    /// A help/version cache does not establish a runtime interaction protocol.
    pub fn unknown_for_version(version: Option<&str>) -> Self {
        let mut out = Self::default();
        for evidence in [
            &mut out.permissions,
            &mut out.questions,
            &mut out.plans,
            &mut out.context_capacity,
            &mut out.compaction_control,
            &mut out.images,
            &mut out.load_session,
        ] {
            evidence.cli_version = version.map(str::to_owned);
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionScope {
    Once,
    Always,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOption {
    pub id: String,
    pub label: String,
    pub native_kind: Option<String>,
    pub scope: DecisionScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeQuestion {
    pub id: String,
    pub prompt: String,
    pub options: Vec<NativeOption>,
    pub multiple: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeInteraction {
    Permission {
        title: String,
        options: Vec<NativeOption>,
    },
    Questions(Vec<NativeQuestion>),
    /// Reserved contract: no adapter advertises or parses plan approval in this slice.
    PlanApproval {
        title: String,
        options: Vec<NativeOption>,
    },
}

fn text(value: &Value, name: &str) -> Result<String, String> {
    value[name]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 4096)
        .map(str::to_owned)
        .ok_or_else(|| format!("native request has an invalid {name}"))
}

fn options(value: &Value, permission: bool) -> Result<Vec<NativeOption>, String> {
    let options = value.as_array().ok_or("native request needs options")?;
    let max = if permission { 32 } else { 9 };
    if options.is_empty() || options.len() > max {
        return Err("native request option bound exceeded".into());
    }
    let mut ids = HashSet::new();
    options
        .iter()
        .map(|option| {
            let id = text(option, if permission { "optionId" } else { "id" })?;
            if !ids.insert(id.clone()) {
                return Err("native request has duplicate option identities".into());
            }
            let native_kind = option["kind"].as_str().map(str::to_owned);
            let scope = match native_kind.as_deref() {
                Some("allow_once" | "reject_once") => DecisionScope::Once,
                Some("allow_always" | "reject_always") => DecisionScope::Always,
                _ => DecisionScope::Unknown,
            };
            Ok(NativeOption {
                id,
                label: text(option, if permission { "name" } else { "label" })?,
                native_kind,
                scope,
            })
        })
        .collect()
}

impl NativeInteraction {
    fn parse(method: &str, params: &Value) -> Result<Self, String> {
        match method {
            "session/request_permission" => Ok(Self::Permission {
                title: params
                    .pointer("/toolCall/title")
                    .and_then(Value::as_str)
                    .unwrap_or("Allow this tool action?")
                    .to_owned(),
                options: options(&params["options"], true)?,
            }),
            "cursor/ask_question" => {
                let questions = params["questions"]
                    .as_array()
                    .ok_or("native request needs questions")?;
                if questions.is_empty() || questions.len() > 3 {
                    return Err("native question bound exceeded".into());
                }
                let mut ids = HashSet::new();
                let questions = questions
                    .iter()
                    .map(|question| {
                        let id = text(question, "id")?;
                        if !ids.insert(id.clone()) {
                            return Err("native request has duplicate question identities".into());
                        }
                        let multiple = match question.get("allowMultiple") {
                            None => false,
                            Some(value) => value
                                .as_bool()
                                .ok_or("native question has invalid selection mode")?,
                        };
                        Ok(NativeQuestion {
                            id,
                            prompt: text(question, "prompt")?,
                            options: options(&question["options"], false)?,
                            multiple,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(Self::Questions(questions))
            }
            _ => Err("native interaction method is unsupported".into()),
        }
    }

    fn accepts(&self, value: &Value) -> bool {
        if value.as_object().is_none_or(|object| object.len() != 1) {
            return false;
        }
        let Some(outcome) = value.get("outcome") else {
            return false;
        };
        if outcome["outcome"] == "cancelled" {
            return outcome.as_object().is_some_and(|object| object.len() == 1);
        }
        if outcome.as_object().is_none_or(|object| object.len() != 2) {
            return false;
        }
        match self {
            Self::Permission { options, .. } | Self::PlanApproval { options, .. } => {
                outcome["outcome"] == "selected"
                    && outcome["optionId"]
                        .as_str()
                        .is_some_and(|id| options.iter().any(|o| o.id == id))
            }
            Self::Questions(questions) => {
                let Some(answers) = outcome["answers"].as_array() else {
                    return false;
                };
                if outcome["outcome"] != "answered" || answers.len() != questions.len() {
                    return false;
                }
                let mut answered = HashSet::new();
                answers.iter().all(|answer| {
                    if answer.as_object().is_none_or(|object| object.len() != 2) {
                        return false;
                    }
                    let Some(id) = answer["questionId"].as_str() else {
                        return false;
                    };
                    let Some(question) = questions.iter().find(|q| q.id == id) else {
                        return false;
                    };
                    let Some(selected) = answer["selectedOptionIds"].as_array() else {
                        return false;
                    };
                    if !answered.insert(id)
                        || selected.is_empty()
                        || (!question.multiple && selected.len() != 1)
                    {
                        return false;
                    }
                    let mut unique = HashSet::new();
                    selected.iter().all(|option| {
                        option.as_str().is_some_and(|id| {
                            unique.insert(id) && question.options.iter().any(|o| o.id == id)
                        })
                    })
                })
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeEnvelope {
    /// Standalone low-level callers have no UI conversation authority.
    pub owner: Option<InteractionOwner>,
    pub native_session_id: String,
    pub connection_id: String,
    pub request_id: Value,
}

#[derive(Debug, Clone)]
pub struct NativeRequest {
    pub method: String,
    pub params: Value,
    kind: NativeInteraction,
    envelope: NativeEnvelope,
    capability: CapabilityEvidence,
    active: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    response: Arc<ResponseChannel>,
}

impl NativeRequest {
    pub fn owner(&self) -> Option<&InteractionOwner> {
        self.envelope.owner.as_ref()
    }

    pub fn envelope(&self) -> &NativeEnvelope {
        &self.envelope
    }

    pub fn kind(&self) -> &NativeInteraction {
        &self.kind
    }

    pub fn capability(&self) -> &CapabilityEvidence {
        &self.capability
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
            && !self.cancelled.load(Ordering::SeqCst)
            && self
                .response
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_some()
    }

    pub fn is_owned_by(&self, owner: &InteractionOwner) -> bool {
        self.envelope.owner.as_ref() == Some(owner) && self.is_active()
    }

    /// Reject a wrong owner without consuming the rightful owner's response channel.
    pub fn respond_owned(&self, owner: &InteractionOwner, value: Value) -> Result<(), String> {
        if self.envelope.owner.as_ref() != Some(owner) {
            return Err("native interaction owner mismatch".into());
        }
        self.send(value)
    }

    /// Standalone callers can answer their request; owned UI requests require respond_owned.
    pub fn respond(&self, value: Value) {
        if self.envelope.owner.is_none()
            || value.pointer("/outcome/outcome") == Some(&json!("cancelled"))
        {
            let _ = self.send(value);
        }
    }

    fn send(&self, value: Value) -> Result<(), String> {
        let mut response = self.response.lock().unwrap_or_else(|e| e.into_inner());
        if !self.active.load(Ordering::SeqCst) || self.cancelled.load(Ordering::SeqCst) {
            response.take();
            return Err("native interaction is no longer active".into());
        }
        if !self.kind.accepts(&value)
            || serde_json::to_vec(&value).map_or(true, |v| v.len() > REQUEST_BYTES)
        {
            return Err("native response does not match the offered request".into());
        }
        response
            .take()
            .ok_or("native interaction already answered")?
            .send(value)
            .map_err(|_| "native interaction receiver is closed".into())
    }

    pub fn invalidate(&self) {
        self.response
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
    }
}

/// Kept only by the live transport. Dropping it invalidates every cloned request.
pub(crate) struct InteractionConnection {
    owner: Option<InteractionOwner>,
    id: String,
    active: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    seen: HashSet<String>,
    pending: Vec<(usize, Weak<ResponseChannel>)>,
    pub capabilities: InteractionCapabilities,
}

impl InteractionConnection {
    pub fn new(owner: Option<InteractionOwner>, cancelled: Arc<AtomicBool>) -> Self {
        Self {
            owner,
            id: uuid::Uuid::new_v4().to_string(),
            active: Arc::new(AtomicBool::new(true)),
            cancelled,
            seen: HashSet::new(),
            pending: Vec::new(),
            capabilities: InteractionCapabilities::default(),
        }
    }

    pub fn initialized_cursor(&mut self, value: &Value) {
        let base = CapabilityEvidence {
            support: Support::Unknown,
            transport: "cursor-acp".into(),
            cli_version: value
                .pointer("/agentInfo/version")
                .and_then(Value::as_str)
                .filter(|s| s.len() <= 256)
                .map(str::to_owned),
            protocol_version: value["protocolVersion"].as_u64(),
            connection_id: Some(self.id.clone()),
            source: "native initialize response".into(),
            observed_at_unix_ms: observed_at(),
        };
        let capability = |value: Option<bool>| CapabilityEvidence {
            support: match value {
                Some(true) => Support::Supported,
                Some(false) => Support::Unsupported,
                None => Support::Unknown,
            },
            ..base.clone()
        };
        self.capabilities.images = capability(
            value
                .pointer("/agentCapabilities/promptCapabilities/image")
                .and_then(Value::as_bool),
        );
        self.capabilities.load_session = capability(
            value
                .pointer("/agentCapabilities/loadSession")
                .and_then(Value::as_bool),
        );
    }

    pub fn request(
        &mut self,
        session: &str,
        request_id: Value,
        method: String,
        params: Value,
    ) -> Result<(NativeRequest, oneshot::Receiver<Value>), String> {
        if session.is_empty() || session.len() > 4096 || self.cancelled.load(Ordering::SeqCst) {
            return Err("native interaction session is unavailable".into());
        }
        if params["sessionId"].as_str() != Some(session) {
            return Err("native interaction session identity mismatch".into());
        }
        let valid_id = match &request_id {
            Value::String(s) => !s.is_empty() && s.len() <= 512,
            Value::Number(n) => n.is_i64() || n.is_u64(),
            _ => false,
        };
        if !valid_id || self.seen.len() >= REQUESTS_PER_CONNECTION {
            return Err("native interaction request identity/budget is invalid".into());
        }
        let bytes = serde_json::to_vec(&params)
            .map_err(|_| "invalid native interaction data")?
            .len();
        self.pending.retain(|(_, response)| {
            response.upgrade().is_some_and(|response| {
                response.lock().unwrap_or_else(|e| e.into_inner()).is_some()
            })
        });
        if bytes > REQUEST_BYTES
            || self.pending.len() >= PENDING_REQUESTS
            || self.pending.iter().map(|(bytes, _)| bytes).sum::<usize>() + bytes > REQUEST_BYTES
        {
            return Err("native interaction request exceeds its byte budget".into());
        }
        let kind = NativeInteraction::parse(&method, &params)?;
        if !self.seen.insert(request_id.to_string()) {
            return Err("native interaction request was already observed".into());
        }
        let capability = CapabilityEvidence {
            support: Support::Supported,
            transport: "cursor-acp".into(),
            cli_version: self.capabilities.images.cli_version.clone(),
            protocol_version: self.capabilities.images.protocol_version,
            connection_id: Some(self.id.clone()),
            source: "validated native request on this connection".into(),
            observed_at_unix_ms: observed_at(),
        };
        match &kind {
            NativeInteraction::Permission { .. } => {
                self.capabilities.permissions = capability.clone()
            }
            NativeInteraction::Questions(_) => self.capabilities.questions = capability.clone(),
            NativeInteraction::PlanApproval { .. } => self.capabilities.plans = capability.clone(),
        }
        let (tx, rx) = oneshot::channel();
        let response = Arc::new(Mutex::new(Some(tx)));
        self.pending.push((bytes, Arc::downgrade(&response)));
        Ok((
            NativeRequest {
                method,
                params,
                kind,
                envelope: NativeEnvelope {
                    owner: self.owner.clone(),
                    native_session_id: session.into(),
                    connection_id: self.id.clone(),
                    request_id,
                },
                capability,
                active: self.active.clone(),
                cancelled: self.cancelled.clone(),
                response,
            },
            rx,
        ))
    }
}

impl Drop for InteractionConnection {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}

fn observed_at() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> InteractionOwner {
        InteractionOwner {
            conversation_id: uuid::Uuid::new_v4().to_string(),
            workspace: PathBuf::from("synthetic-workspace"),
            execution_cwd: PathBuf::from("synthetic-execution"),
            run_id: uuid::Uuid::new_v4().to_string(),
            attempt_id: uuid::Uuid::new_v4().to_string(),
            provider: ProviderKind::Cursor,
            account: "synthetic-account".into(),
            home: Some(PathBuf::from("synthetic-home")),
        }
    }

    fn permission() -> Value {
        json!({"sessionId":"native-session","toolCall":{"title":"Read fixture?"},
            "options":[{"optionId":"once","name":"Allow once","kind":"allow_once"},
                {"optionId":"always","name":"Allow always","kind":"allow_always"}]})
    }

    fn selected(id: &str) -> Value {
        json!({"outcome":{"outcome":"selected","optionId":id}})
    }

    #[test]
    fn exact_owner_and_options_are_required_before_single_response() {
        let owner = owner();
        let mut scope =
            InteractionConnection::new(Some(owner.clone()), Arc::new(AtomicBool::new(false)));
        let (request, mut rx) = scope
            .request(
                "native-session",
                json!(7),
                "session/request_permission".into(),
                permission(),
            )
            .unwrap();
        for wrong in [
            InteractionOwner {
                run_id: uuid::Uuid::new_v4().to_string(),
                ..owner.clone()
            },
            InteractionOwner {
                attempt_id: uuid::Uuid::new_v4().to_string(),
                ..owner.clone()
            },
            InteractionOwner {
                home: Some("other-home".into()),
                ..owner.clone()
            },
            InteractionOwner {
                conversation_id: uuid::Uuid::new_v4().to_string(),
                ..owner.clone()
            },
        ] {
            assert!(request.respond_owned(&wrong, selected("once")).is_err());
        }
        assert!(request
            .respond_owned(&owner, selected("invented-grant"))
            .is_err());
        request.respond(selected("always"));
        assert!(rx.try_recv().is_err());
        let NativeInteraction::Permission { options, .. } = request.kind() else {
            panic!("permission expected")
        };
        assert_eq!(options[0].scope, DecisionScope::Once);
        assert_eq!(options[1].scope, DecisionScope::Always);
        request.respond_owned(&owner, selected("once")).unwrap();
        assert_eq!(rx.try_recv().unwrap(), selected("once"));
        assert!(request.respond_owned(&owner, selected("always")).is_err());
    }

    #[test]
    fn cancel_disconnect_and_session_changes_never_reuse_requests() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut scope = InteractionConnection::new(None, cancelled.clone());
        let (request, mut rx) = scope
            .request(
                "native-session",
                json!(1),
                "session/request_permission".into(),
                permission(),
            )
            .unwrap();
        assert!(scope
            .request(
                "different-session",
                json!(2),
                "session/request_permission".into(),
                permission()
            )
            .is_err());
        assert!(scope
            .request(
                "native-session",
                json!(1),
                "session/request_permission".into(),
                permission()
            )
            .is_err());
        cancelled.store(true, Ordering::SeqCst);
        request.respond(selected("once"));
        assert!(!request.is_active());
        assert!(rx.try_recv().is_err());
        cancelled.store(false, Ordering::SeqCst);
        let (request, mut rx) = scope
            .request(
                "native-session",
                json!(3),
                "session/request_permission".into(),
                permission(),
            )
            .unwrap();
        drop(scope);
        request.respond(selected("once"));
        assert!(!request.is_active());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn connections_namespace_ids_and_out_of_order_answers_stay_separate() {
        let mut first = InteractionConnection::new(None, Arc::new(AtomicBool::new(false)));
        let mut second = InteractionConnection::new(None, Arc::new(AtomicBool::new(false)));
        let (a, mut a_rx) = first
            .request(
                "native-session",
                json!(1),
                "session/request_permission".into(),
                permission(),
            )
            .unwrap();
        let (b, mut b_rx) = second
            .request(
                "native-session",
                json!(1),
                "session/request_permission".into(),
                permission(),
            )
            .unwrap();
        assert_ne!(a.envelope().connection_id, b.envelope().connection_id);
        b.respond(selected("always"));
        assert_eq!(b_rx.try_recv().unwrap(), selected("always"));
        assert!(a_rx.try_recv().is_err());
        a.respond(selected("once"));
        assert_eq!(a_rx.try_recv().unwrap(), selected("once"));
    }

    #[test]
    fn malformed_unknown_and_extra_scope_responses_fail_closed() {
        let mut scope = InteractionConnection::new(None, Arc::new(AtomicBool::new(false)));
        assert!(scope
            .request(
                "native-session",
                json!(1),
                "invented/approve".into(),
                permission()
            )
            .is_err());
        let mut duplicate = permission();
        duplicate["options"][1]["optionId"] = json!("once");
        assert!(scope
            .request(
                "native-session",
                json!(2),
                "session/request_permission".into(),
                duplicate
            )
            .is_err());
        let (request, mut rx) = scope
            .request(
                "native-session",
                json!(3),
                "session/request_permission".into(),
                permission(),
            )
            .unwrap();
        request
            .respond(json!({"outcome":{"outcome":"selected","optionId":"once","scope":"always"}}));
        assert!(rx.try_recv().is_err());
        request.respond(selected("once"));
        assert_eq!(rx.try_recv().unwrap(), selected("once"));
    }

    #[test]
    fn questions_preserve_ids_and_reject_duplicate_or_foreign_answers() {
        let mut scope = InteractionConnection::new(None, Arc::new(AtomicBool::new(false)));
        let params = json!({"sessionId":"native-session","questions":[{"id":"question","prompt":"Choose?",
            "options":[{"id":"a","label":"First"},{"id":"b","label":"Second"}],"allowMultiple":true}]});
        let (request, mut rx) = scope
            .request(
                "native-session",
                json!(1),
                "cursor/ask_question".into(),
                params,
            )
            .unwrap();
        for selected in [json!(["a", "a"]), json!(["foreign"]), json!([])] {
            request.respond(json!({"outcome":{"outcome":"answered","answers":[{"questionId":"question","selectedOptionIds":selected}]}}));
            assert!(rx.try_recv().is_err());
        }
        let answer = json!({"outcome":{"outcome":"answered","answers":[{"questionId":"question","selectedOptionIds":["b","a"]}]}});
        request.respond(answer.clone());
        assert_eq!(rx.try_recv().unwrap(), answer);
    }

    #[test]
    fn capability_cache_and_negotiation_do_not_invent_protocol_support() {
        let unknown = InteractionCapabilities::unknown_for_version(Some("synthetic-version"));
        assert_eq!(unknown.permissions.support, Support::Unknown);
        assert_eq!(unknown.context_capacity.support, Support::Unknown);
        assert_eq!(
            unknown.permissions.cli_version.as_deref(),
            Some("synthetic-version")
        );
        let mut scope = InteractionConnection::new(None, Arc::new(AtomicBool::new(false)));
        scope.initialized_cursor(&json!({"protocolVersion":1,"agentCapabilities":{"promptCapabilities":{"image":true},"loadSession":false}}));
        assert_eq!(scope.capabilities.images.support, Support::Supported);
        assert_eq!(
            scope.capabilities.load_session.support,
            Support::Unsupported
        );
        assert_eq!(scope.capabilities.permissions.support, Support::Unknown);
        assert_eq!(scope.capabilities.plans.support, Support::Unknown);
        assert!(scope.capabilities.images.observed_at_unix_ms.is_some());
    }

    #[test]
    fn pending_request_count_and_aggregate_bytes_are_bounded_and_released() {
        let mut scope = InteractionConnection::new(None, Arc::new(AtomicBool::new(false)));
        let mut pending = Vec::new();
        for id in 0..PENDING_REQUESTS {
            pending.push(
                scope
                    .request(
                        "native-session",
                        json!(id),
                        "session/request_permission".into(),
                        permission(),
                    )
                    .unwrap(),
            );
        }
        assert!(scope
            .request(
                "native-session",
                json!(99),
                "session/request_permission".into(),
                permission()
            )
            .is_err());
        pending[0].0.respond(selected("once"));
        let replacement = scope
            .request(
                "native-session",
                json!(99),
                "session/request_permission".into(),
                permission(),
            )
            .unwrap();
        drop(replacement);
        drop(pending);
        let mut large = permission();
        large["toolCall"]["title"] = json!("x".repeat(REQUEST_BYTES / 2));
        let retained = scope
            .request(
                "native-session",
                json!(100),
                "session/request_permission".into(),
                large.clone(),
            )
            .unwrap();
        assert!(scope
            .request(
                "native-session",
                json!(101),
                "session/request_permission".into(),
                large
            )
            .is_err());
        retained.0.invalidate();
        assert!(scope
            .request(
                "native-session",
                json!(101),
                "session/request_permission".into(),
                permission()
            )
            .is_ok());
    }

    #[test]
    fn launch_validation_canonicalizes_home_and_rejects_wrong_execution() {
        let workspace = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let mut owner = owner();
        owner.workspace = workspace.path().into();
        owner.execution_cwd = workspace.path().into();
        owner.home = Some(home.path().into());
        let valid = owner
            .validate_for(ProviderKind::Cursor, workspace.path(), Some(home.path()))
            .unwrap();
        assert_eq!(
            valid.home,
            Some(std::fs::canonicalize(home.path()).unwrap())
        );
        assert!(owner
            .validate_for(ProviderKind::Cursor, other.path(), Some(home.path()))
            .is_err());
        assert!(owner
            .validate_for(ProviderKind::Cursor, workspace.path(), Some(other.path()))
            .is_err());
        assert!(owner
            .validate_for(ProviderKind::Codex, workspace.path(), Some(home.path()))
            .is_err());
        assert!(owner
            .validate_for(ProviderKind::Cursor, workspace.path(), None)
            .is_err());
    }
}
