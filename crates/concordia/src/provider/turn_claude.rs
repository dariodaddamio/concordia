//! Claude Agent SDK stdio control protocol.
//!
//! This module is deliberately limited to the documented native control frames.  It does not
//! treat MCP callbacks, tool results, or model prose as approval evidence.  The coordinator owns
//! process lifetime and routes parsed permission requests through `InteractionConnection`.
use super::*;
use crate::provider::interaction::InteractionConnection;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};

/// The installed, source-matched Claude Code CLI version qualified for this bridge.
pub(crate) const SUPPORTED_VERSION: &str = crate::capacity::claude_capacity::CLI_VERSION;

const MAX_ID: usize = 512;
const MAX_TOOL: usize = 256;
const MAX_INPUT: usize = 256 * 1024;
const DEFERRED_TASK_LIMIT: usize = 256;
const RUN_END_CEILING: Duration = Duration::from_secs(600);

fn native_task_card(
    identity: (&str, Option<&str>, Option<&str>),
    title: &str,
    status: crate::provider::stream_cards::CardStatus,
    detail: Option<String>,
    out: &mut TurnOutcome,
    progress: &Option<tokio::sync::mpsc::UnboundedSender<TurnProgress>>,
) {
    let id = format!(
        "native-agent:claude:{}",
        json!([identity.1.unwrap_or(""), identity.0, identity.2])
    );
    let card = crate::provider::stream_cards::StreamCard {
        id,
        title: format!("agent {}", identity.0.chars().take(12).collect::<String>()),
        detail: Some(
            format!(
                "Task: {title}{}",
                detail
                    .map(|detail| format!("\nResult: {detail}"))
                    .unwrap_or_default()
            )
            .chars()
            .take(4096)
            .collect(),
        ),
        status,
    };
    emit_progress(&[StreamPart::Card(card.clone())], progress);
    crate::provider::stream_cards::upsert_card(&mut out.cards, card);
}

fn clean_task_text(value: Option<&Value>, fallback: &str, limit: usize) -> String {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(|text| {
            text.chars()
                .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
                .take(limit)
                .collect::<String>()
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

#[derive(Debug, Clone)]
pub(crate) struct PermissionRequest {
    pub request_id: String,
    pub tool_name: String,
    pub tool_use_id: String,
    pub title: Option<String>,
    pub input: Value,
}

struct PendingPermission {
    request_id: String,
    input: Value,
    question: bool,
    native: crate::provider::interaction::NativeRequest,
    rx: tokio::sync::oneshot::Receiver<Value>,
}

fn bounded_id(value: Option<&Value>) -> Option<String> {
    let value = value?.as_str()?;
    (!value.is_empty() && value.len() <= MAX_ID && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

/// Parse only a top-level native `can_use_tool` control request.
pub(crate) fn permission_request(value: &Value) -> Result<Option<PermissionRequest>, String> {
    if value.get("type").and_then(Value::as_str) != Some("control_request") {
        return Ok(None);
    }
    let request_id =
        bounded_id(value.get("request_id")).ok_or("Claude control request identity is invalid")?;
    let request = value
        .get("request")
        .filter(|v| v.is_object())
        .ok_or("Claude control request body is invalid")?;
    if request.get("subtype").and_then(Value::as_str) != Some("can_use_tool") {
        return Ok(None);
    }
    let tool_name = request
        .get("tool_name")
        .and_then(Value::as_str)
        .filter(|name| {
            !name.is_empty() && name.len() <= MAX_TOOL && !name.chars().any(char::is_control)
        })
        .ok_or("Claude permission tool name is invalid")?
        .to_owned();
    let tool_use_id = bounded_id(request.get("tool_use_id"))
        .ok_or("Claude permission tool-use identity is invalid")?;
    let input = request
        .get("input")
        .filter(|v| v.is_object())
        .cloned()
        .ok_or("Claude permission input is invalid")?;
    if input.to_string().len() > MAX_INPUT {
        return Err("Claude permission input exceeds its byte budget".into());
    }
    let title = request
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| {
            !title.is_empty() && title.len() <= MAX_TOOL && !title.chars().any(char::is_control)
        })
        .map(str::to_owned);
    Ok(Some(PermissionRequest {
        request_id,
        tool_name,
        tool_use_id,
        title,
        input,
    }))
}

/// Control cancellation identifies a pending permission request and must never be answered.
pub(crate) fn cancelled_request_id(value: &Value) -> Option<&str> {
    (value.get("type").and_then(Value::as_str) == Some("control_cancel_request"))
        .then(|| value.get("request_id").and_then(Value::as_str))
        .flatten()
        .filter(|id| !id.is_empty() && id.len() <= MAX_ID && !id.chars().any(char::is_control))
}

/// Build the native SDK response after the broker has produced an at-most-once decision.
pub(crate) fn permission_response(
    request_id: &str,
    input: &Value,
    allow: bool,
    deny_message: &str,
) -> Value {
    let response = if allow {
        json!({"behavior":"allow", "updatedInput": input})
    } else {
        json!({"behavior":"deny", "message": deny_message})
    };
    json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": response
        }
    })
}

pub(crate) fn initialize_request(request_id: &str) -> Value {
    json!({
        "type": "control_request",
        "request_id": request_id,
        "request": {"subtype": "initialize"}
    })
}

pub(crate) fn user_message(prompt: &str) -> Value {
    json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{"type": "text", "text": prompt}]
        },
        "parent_tool_use_id": null
    })
}

/// Translate the broker's one-shot ACP-shaped selection into the Claude wire decision.
pub(crate) fn allow_from_broker(value: &Value) -> bool {
    value.pointer("/outcome/optionId").and_then(Value::as_str) == Some("allow")
}

fn question_answered(value: &Value) -> bool {
    value.pointer("/outcome/outcome").and_then(Value::as_str) == Some("answered")
}

fn bounded_question_text(value: Option<&Value>, field: &str) -> Result<String, String> {
    let text = value
        .and_then(Value::as_str)
        .filter(|text| {
            !text.is_empty() && text.len() <= 4096 && !text.chars().any(char::is_control)
        })
        .ok_or_else(|| format!("Claude question has invalid {field}"))?;
    Ok(text.to_owned())
}

/// Normalize Claude's AskUserQuestion payload into the existing bounded Questions broker shape.
/// The original input remains in the pending request and is used to build updatedInput later.
fn claude_question_params(request: &PermissionRequest, session: &str) -> Result<Value, String> {
    let questions = request
        .input
        .get("questions")
        .and_then(Value::as_array)
        .filter(|questions| (1..=4).contains(&questions.len()))
        .ok_or("Claude question count must be between 1 and 4")?;
    let mut question_ids = HashSet::new();
    let mut normalized = Vec::with_capacity(questions.len());
    for question in questions {
        let id = bounded_question_text(question.get("question"), "question")?;
        if !question_ids.insert(id.clone()) {
            return Err("Claude questions must have unique question text".into());
        }
        let multiple = question
            .get("multiSelect")
            .and_then(Value::as_bool)
            .ok_or("Claude question multiSelect must be boolean")?;
        let options = question
            .get("options")
            .and_then(Value::as_array)
            .filter(|options| (2..=4).contains(&options.len()))
            .ok_or("Claude question options must contain between 2 and 4 choices")?;
        let mut labels = HashSet::new();
        let mut choices = Vec::with_capacity(options.len());
        for option in options {
            let label = bounded_question_text(option.get("label"), "option label")?;
            if !labels.insert(label.clone()) {
                return Err("Claude question options must have unique labels".into());
            }
            let description = match option.get("description") {
                None | Some(Value::Null) => None,
                Some(value) => Some(bounded_question_text(Some(value), "option description")?),
            };
            let display = description
                .as_deref()
                .filter(|description| !description.is_empty())
                .map_or_else(
                    || label.clone(),
                    |description| format!("{label} · {description}"),
                );
            choices.push(json!({"id": label, "label": display}));
        }
        normalized.push(json!({
            "id": id,
            "prompt": bounded_question_text(question.get("question"), "question")?,
            "options": choices,
            "allowMultiple": multiple,
        }));
    }
    Ok(json!({
        "sessionId": session,
        "toolCall": {
            "title": request.title.as_deref().unwrap_or("Answer Claude's questions"),
            "toolCallId": request.tool_use_id,
            "rawInput": request.input,
        },
        "questions": normalized,
    }))
}

/// Convert broker selections to the official Claude answer shape. Claude accepts a string for
/// each answer; multi-select labels are joined with comma-space as documented by the SDK demo.
fn claude_question_answer(input: &Value, answer: &Value) -> Result<Value, String> {
    let questions = input
        .get("questions")
        .and_then(Value::as_array)
        .ok_or("Claude question input disappeared")?;
    let selected = answer
        .pointer("/outcome/answers")
        .and_then(Value::as_array)
        .ok_or("Claude question answer is malformed")?;
    if selected.len() != questions.len() {
        return Err("Claude question answer count changed".into());
    }
    let mut answers = serde_json::Map::new();
    for item in selected {
        let id = item
            .get("questionId")
            .and_then(Value::as_str)
            .ok_or("Claude question answer has no question identity")?;
        let question = questions
            .iter()
            .find(|question| question.get("question").and_then(Value::as_str) == Some(id))
            .ok_or("Claude question answer has a foreign question identity")?;
        let options = question
            .get("options")
            .and_then(Value::as_array)
            .ok_or("Claude question options disappeared")?;
        let labels: Vec<String> = item
            .get("selectedOptionIds")
            .and_then(Value::as_array)
            .ok_or("Claude question answer has no selections")?
            .iter()
            .map(|selection| {
                let label = selection
                    .as_str()
                    .ok_or("Claude question selection is not text")?;
                if options
                    .iter()
                    .any(|option| option.get("label").and_then(Value::as_str) == Some(label))
                {
                    return Ok(label.to_owned());
                }
                if let Some(note) = label.strip_prefix("user_note: ") {
                    if !crate::provider::interaction::valid_reply(note) {
                        return Err(
                            "Claude question free-text answer exceeds its bound".to_string()
                        );
                    }
                    return Ok(note.to_owned());
                }
                if !options
                    .iter()
                    .any(|option| option.get("label").and_then(Value::as_str) == Some(label))
                {
                    return Err("Claude question answer has a foreign option".to_string());
                }
                Ok(label.to_owned())
            })
            .collect::<Result<_, String>>()?;
        if labels.is_empty() {
            return Err("Claude question answer is empty".into());
        }
        answers.insert(id.to_owned(), Value::String(labels.join(", ")));
    }
    let mut updated = input.clone();
    updated["answers"] = Value::Object(answers);
    Ok(updated)
}

async fn write_frame(input: &mut tokio::process::ChildStdin, value: &Value) -> anyhow::Result<()> {
    let bytes = format!("{value}\n");
    tokio::time::timeout(Duration::from_secs(10), async {
        input.write_all(bytes.as_bytes()).await?;
        input.flush().await
    })
    .await
    .context("Claude control write timed out")??;
    Ok(())
}

/// Run Claude's version-matched stdio control transport. Process ownership and cancellation stay
/// with the normal `RunningTurn`; this adapter only owns the protocol handshake and request loop.
pub(super) async fn run(
    req: &ProviderTurnRequest<'_>,
    plan: &PlannedTurn,
    owner: Option<crate::provider::interaction::InteractionOwner>,
    started: Option<tokio::sync::mpsc::UnboundedSender<std::sync::Arc<RunningTurn>>>,
    progress: Option<tokio::sync::mpsc::UnboundedSender<TurnProgress>>,
) -> anyhow::Result<TurnOutcome> {
    let _new_session_guard = if req.native_session_id.is_none() {
        plan.new_session_id
            .as_deref()
            .map(|session| {
                super::super::session_guard::acquire(
                    &crate::store::default_store_root(),
                    req.home_path,
                    session,
                )
            })
            .transpose()?
    } else {
        None
    };
    let mut message = user_message(&plan.prompt);
    if let Some(documents) = pdf_message(req)? {
        message["message"]["content"] = Value::Array(
            documents
                .claude_content(&plan.prompt)
                .map_err(anyhow::Error::msg)?,
        );
    } else {
        anyhow::ensure!(
            req.images.len() <= crate::attachments::MAX_IMAGES,
            "at most four images per message"
        );
        for image in req.images {
            message["message"]["content"]
                .as_array_mut()
                .expect("native content array")
                .push(image.claude().map_err(anyhow::Error::msg)?);
        }
    }
    let program = plan.command.clone();
    let version = tokio::task::spawn_blocking(move || {
        crate::provider::compat::run_cli(std::path::Path::new(&program), &["--version"])
    })
    .await?
    .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        crate::provider::compat::version_token(&version).as_deref() == Some(SUPPORTED_VERSION),
        "Claude native permission transport requires CLI {SUPPORTED_VERSION}"
    );
    let mut launch = crate::provider::launch::prepare(plan)?;
    anyhow::ensure!(
        !crate::provider::launch::is_batch_shim(&launch.program),
        "Claude native stdio requires a native executable; batch wrappers are unsupported"
    );
    // The stream protocol receives initialize and user frames over stdin; the normal argv prompt
    // must not be sent as a second, uncorrelated request.
    if launch.args.last() == Some(&plan.prompt) {
        launch.args.pop();
    } else {
        anyhow::ensure!(
            launch.stdin.is_some(),
            "Claude prompt transport is inconsistent"
        );
    }
    launch.args.extend([
        "--input-format".into(),
        "stream-json".into(),
        "--permission-prompt-tool".into(),
        "stdio".into(),
    ]);
    let mut command = tokio::process::Command::new(&launch.program);
    crate::process_ui::background(&mut command);
    command
        .args(&launch.args)
        .envs(plan.env.clone())
        .envs(launch.env.clone())
        // Ask the CLI for the SDK-host-only session_state_changed frames. This is the
        // versioned signal the official SDK uses to know whether a result owes a follow-up turn.
        .env("CLAUDE_CODE_SDK_READS_SESSION_STATE", "1")
        .current_dir(&plan.cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .context("could not start Claude native stdio")?;
    let handle = std::sync::Arc::new(RunningTurn::new());
    handle
        .pid
        .store(child.id().unwrap_or(0), std::sync::atomic::Ordering::SeqCst);
    let mut input = child.stdin.take().context("Claude stdin unavailable")?;
    let mut output =
        tokio::io::BufReader::new(child.stdout.take().context("Claude stdout unavailable")?);
    let stderr = child.stderr.take().context("Claude stderr unavailable")?;
    let mut stderr_task = tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stderr);
        let mut tail = VecDeque::new();
        let mut line = Vec::new();
        while read_line_bounded(&mut reader, &mut line)
            .await
            .is_ok_and(|n| n > 0)
        {
            if tail.len() == STDERR_KEEP_LINES {
                tail.pop_front();
            }
            tail.push_back(decode_line(&line));
        }
        tail.into_iter().collect::<Vec<_>>().join("\n")
    });
    *handle.child.lock().await = Some(child);
    if let Some(tx) = started {
        if tx.send(handle.clone()).is_err() {
            handle.cancel();
        }
    }
    let session = req.native_session_id.or(plan.new_session_id.as_deref());
    let mut out = TurnOutcome {
        session_id: session.map(str::to_owned),
        ..Default::default()
    };
    let artifact_owner = owner.clone();
    let mut artifacts = crate::artifacts::Collector::default();
    let mut interactions = InteractionConnection::new(owner, handle.cancel_flag.clone());
    let mut parser = crate::provider::stream_json::StreamParser::new(
        crate::provider::stream_json::Flavor::Claude,
    )
    .with_cwd(req.cwd);
    let mut initialized = false;
    let mut sent_prompt = false;
    let mut pending: HashMap<String, PendingPermission> = HashMap::new();
    let mut seen_native = HashSet::new();
    let mut poll = tokio::time::interval(std::time::Duration::from_millis(10));
    let mut stream_bytes = 0usize;
    let mut result_seen = false;
    let mut result_deadline = None;
    let mut session_state = None::<String>;
    let mut deferred_tasks = HashSet::new();
    let mut seen_deferred_tasks = HashSet::new();
    let mut deferred_task_titles = HashMap::new();
    let mut deferred_task_tools = HashMap::new();
    let mut deferred_work_seen = false;
    let initialize_id = uuid::Uuid::new_v4().to_string();
    let initialize_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let result: anyhow::Result<()> = async {
        write_frame(&mut input, &initialize_request(&initialize_id)).await?;
        let mut line = Vec::new();
        while !handle.cancel_flag.load(std::sync::atomic::Ordering::SeqCst) {
            line.clear();
            let n = {
            let read = read_line_bounded(&mut output, &mut line);
            tokio::pin!(read);
            loop { tokio::select! {
                biased;
                _ = handle.wake.notified() => anyhow::bail!("cancelled"),
                n = &mut read => break n?,
                _ = tokio::time::sleep_until(initialize_deadline), if !initialized => anyhow::bail!("Claude native initialize timed out before prompt dispatch"),
                _ = async {
                    if let Some(deadline) = result_deadline {
                        tokio::time::sleep_until(deadline).await;
                    }
                }, if result_seen && result_deadline.is_some() => anyhow::bail!("Claude native run-end ceiling expired with deferred work"),
                _ = poll.tick() => {
                    let mut finished = Vec::new();
                    for (id, request) in pending.iter_mut() {
                        match request.rx.try_recv() {
                            Ok(answer) => finished.push((id.clone(), answer)),
                            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => finished.push((id.clone(), json!({}))),
                            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
                        }
                    }
                    for (id, answer) in finished {
                        if let Some(request) = pending.remove(&id) {
                            anyhow::ensure!(!handle.cancel_flag.load(Ordering::SeqCst), "cancelled");
                            let allow = if request.question {
                                question_answered(&answer)
                            } else {
                                allow_from_broker(&answer)
                            };
                            request.native.invalidate();
                            let (allow, updated_input, message) = if allow && request.question {
                                match claude_question_answer(&request.input, &answer) {
                                    Ok(updated) => (true, updated, "Permission denied".to_owned()),
                                    Err(reason) => (false, request.input.clone(), reason),
                                }
                            } else {
                                (allow, request.input.clone(), "Permission denied".to_owned())
                            };
                            write_frame(&mut input, &permission_response(&request.request_id, &updated_input, allow, &message)).await?;
                        }
                    }
                }
            }} };
            if n == 0 {
                if result_seen
                    && deferred_tasks.is_empty()
                    && ((!deferred_work_seen && session_state.as_deref() != Some("running"))
                        || (deferred_work_seen && session_state.as_deref() == Some("idle")))
                    && pending.is_empty()
                {
                    break;
                }
                anyhow::bail!("Claude stdio closed before deferred work settled");
            }
            anyhow::ensure!(n <= LINE_MAX, "Claude native output line exceeds its bound");
            stream_bytes = stream_bytes.saturating_add(n);
            anyhow::ensure!(stream_bytes <= 16 * 1024 * 1024, "Claude native output exceeds its bound");
            let text = decode_line(&line);
            let value = match serde_json::from_str::<Value>(&text) { Ok(v) => v, Err(_) => continue };
            if value.get("session_id").and_then(Value::as_str).is_some_and(|reported| {
                session.is_some_and(|expected| reported != expected)
            }) {
                anyhow::bail!("Claude native event session identity mismatch");
            }
            if !initialized {
                if value.pointer("/response/request_id").and_then(Value::as_str) == Some(initialize_id.as_str()) {
                    anyhow::ensure!(value["type"] == "control_response" && value["response"]["subtype"] == "success"
                        && value["response"]["response"].is_object(), "Claude native initialize was rejected");
                    initialized = true;
                    interactions.initialized_claude(SUPPORTED_VERSION).map_err(anyhow::Error::msg)?;
                    write_frame(&mut input, &message).await?;
                    sent_prompt = true;
                }
                continue;
            }
            if value["type"] == "system" {
                match value["subtype"].as_str() {
                    Some("session_state_changed") => {
                        if let Some(state) = value["state"].as_str() {
                            if matches!(state, "running" | "idle") {
                                session_state = Some(state.to_owned());
                            }
                        }
                    }
                    Some("task_started") => {
                        let task_type = value["task_type"].as_str();
                        if matches!(task_type, Some("local_agent" | "local_workflow")) {
                            let task_id = bounded_id(value.get("task_id"))
                                .context("Claude task_started has invalid task identity")?;
                            anyhow::ensure!(
                                seen_deferred_tasks.len() < DEFERRED_TASK_LIMIT,
                                "Claude deferred task budget exceeded"
                            );
                            anyhow::ensure!(
                                seen_deferred_tasks.insert(task_id.to_owned()),
                                "Claude duplicate deferred task identity"
                            );
                            deferred_tasks.insert(task_id.clone());
                            let title = clean_task_text(
                                value.get("description"),
                                "Claude delegated task",
                                256,
                            );
                            deferred_task_titles.insert(task_id.clone(), title.clone());
                            let tool_use_id = if value.get("tool_use_id").is_some_and(|value| !value.is_null()) {
                                Some(
                                    bounded_id(value.get("tool_use_id"))
                                        .context("Claude task_started has invalid tool identity")?,
                                )
                            } else {
                                None
                            };
                            deferred_task_tools.insert(task_id.clone(), tool_use_id.clone());
                            deferred_work_seen = true;
                            native_task_card(
                                (&task_id, interactions.capabilities.permissions.connection_id.as_deref(), tool_use_id.as_deref()),
                                &title,
                                crate::provider::stream_cards::CardStatus::Running,
                                None,
                                &mut out,
                                &progress,
                            );
                        }
                    }
                    Some("task_notification") => {
                        if value["status"].as_str().is_some_and(|status| {
                            matches!(status, "completed" | "failed" | "stopped" | "killed")
                        }) {
                            if let Some(task_id) = bounded_id(value.get("task_id")) {
                                if seen_deferred_tasks.contains(&task_id) {
                                    deferred_tasks.remove(&task_id);
                                    let title = deferred_task_titles
                                        .get(&task_id).cloned()
                                        .unwrap_or_else(|| "Claude delegated task".into());
                                    let tool_use_id = deferred_task_tools.get(&task_id).cloned().flatten();
                                    let status = if value["status"].as_str() == Some("completed") {
                                        crate::provider::stream_cards::CardStatus::Done
                                    } else {
                                        crate::provider::stream_cards::CardStatus::Error
                                    };
                                    native_task_card(
                                        (&task_id, interactions.capabilities.permissions.connection_id.as_deref(), tool_use_id.as_deref()),
                                        &title,
                                        status,
                                        Some(clean_task_text(
                                            value.get("summary"),
                                            "Claude delegated task completed",
                                            4096,
                                        )),
                                        &mut out,
                                        &progress,
                                    );
                                }
                            }
                        }
                    }
                    Some("task_updated") => {
                        if value["patch"]["status"].as_str().is_some_and(|status| {
                            matches!(status, "completed" | "failed" | "stopped" | "killed")
                        }) {
                            if let Some(task_id) = bounded_id(value.get("task_id")) {
                                if seen_deferred_tasks.contains(&task_id) {
                                    deferred_tasks.remove(&task_id);
                                    let title = deferred_task_titles
                                        .get(&task_id).cloned()
                                        .unwrap_or_else(|| "Claude delegated task".into());
                                    let tool_use_id = deferred_task_tools.get(&task_id).cloned().flatten();
                                    let status = if value["patch"]["status"].as_str() == Some("completed") {
                                        crate::provider::stream_cards::CardStatus::Done
                                    } else {
                                        crate::provider::stream_cards::CardStatus::Error
                                    };
                                    native_task_card(
                                        (&task_id, interactions.capabilities.permissions.connection_id.as_deref(), tool_use_id.as_deref()),
                                        &title,
                                        status,
                                        None,
                                        &mut out,
                                        &progress,
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
                if result_seen
                    && deferred_tasks.is_empty()
                    && ((!deferred_work_seen && session_state.as_deref() != Some("running"))
                        || (deferred_work_seen && session_state.as_deref() == Some("idle")))
                    && pending.is_empty()
                {
                    break;
                }
                continue;
            }
            if let Some(cancel_id) = cancelled_request_id(&value) {
                if let Some(request) = pending.remove(cancel_id) {
                    request.native.invalidate();
                }
                continue;
            }
            if let Some(request) = permission_request(&value).map_err(anyhow::Error::msg)? {
                anyhow::ensure!(seen_native.len() < 1024, "Claude native request budget exceeded");
                if !seen_native.insert(request.request_id.clone()) {
                    continue;
                }
                if request.tool_name == "AskUserQuestion" {
                    let session = session.context("Claude native session identity is unavailable")?;
                    let params = match claude_question_params(&request, session) {
                        Ok(params) => params,
                        Err(reason) => {
                            write_frame(&mut input, &permission_response(&request.request_id, &request.input, false, &reason)).await?;
                            continue;
                        }
                    };
                    let (native, rx) = match interactions.request(session, json!(request.request_id), "claude/ask_question".into(), params) {
                        Ok(pair) => pair,
                        Err(reason) => {
                            write_frame(&mut input, &permission_response(&request.request_id, &request.input, false, &reason)).await?;
                            continue;
                        }
                    };
                    if !progress.as_ref().is_some_and(|tx| tx.send(TurnProgress::Interaction(Box::new(native.clone()))).is_ok()) { native.cancel(); }
                    if pending.insert(request.request_id.clone(), PendingPermission { request_id: request.request_id, input: request.input, question: true, native, rx }).is_some() {
                        anyhow::bail!("duplicate Claude native request identity");
                    }
                    continue;
                }
                let session = session.context("Claude native session identity is unavailable")?;
                let params = json!({"sessionId":session,"toolCall":{"title":request.title.as_deref().unwrap_or(&request.tool_name),"toolCallId":request.tool_use_id,"rawInput":request.input},"options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"},{"optionId":"deny","name":"Deny","kind":"reject_once"}]});
                let (native, rx) = interactions.request(session, json!(request.request_id), "claude/can_use_tool".into(), params).map_err(anyhow::Error::msg)?;
                if !progress.as_ref().is_some_and(|tx| tx.send(TurnProgress::Interaction(Box::new(native.clone()))).is_ok()) { native.cancel(); }
                if pending.insert(request.request_id.clone(), PendingPermission { request_id: request.request_id, input: request.input, question: false, native, rx }).is_some() {
                    anyhow::bail!("duplicate Claude native request identity");
                }
                continue;
            }
            if result_seen && matches!(value["type"].as_str(), Some("assistant" | "stream_event")) {
                result_seen = false;
                result_deadline = None;
                if session_state.is_some() {
                    session_state = Some("running".into());
                }
            }
            if value["type"] == "control_request" {
                let id = value["request_id"].as_str().context("native control request has no identity")?;
                anyhow::ensure!(seen_native.len() < 1024, "Claude native request budget exceeded");
                if seen_native.insert(id.into()) {
                    write_frame(&mut input, &json!({"type":"control_response","response":{
                        "subtype":"error","request_id":id,"error":"Unsupported native control request; no permission was granted"}})).await?;
                }
                continue;
            }
            artifacts.report_claude(artifact_owner.as_ref(), session, None, &value, &progress);
            for mut part in parser.parse_line(&text) {
                if req.native_session_id.is_some() {
                    if let StreamPart::Usage(usage) = &mut part { usage.cost_usd = None; }
                }
                if matches!(&part, StreamPart::Error(_)) { out.failed = true; }
                emit_progress(std::slice::from_ref(&part), &progress);
                let mut chunk = crate::provider::codex::StreamOutcome::default();
                crate::provider::codex::apply_stream_parts(vec![part], &mut chunk);
                out.text.push_str(&chunk.text);
                for card in chunk.cards {
                    crate::provider::stream_cards::upsert_card(&mut out.cards, card);
                }
                if chunk.session_id.is_some() && out.session_id.is_none() {
                    out.session_id = chunk.session_id;
                }
            }
            if value.get("type").and_then(Value::as_str) == Some("result") {
                anyhow::ensure!(pending.is_empty(), "Claude ended with unresolved native permissions; completion is uncertain");
                out.failed |= value["is_error"] == true || value["subtype"].as_str() != Some("success");
                result_seen = true;
                result_deadline.get_or_insert(tokio::time::Instant::now() + RUN_END_CEILING);
                if out.failed
                    || (deferred_tasks.is_empty()
                        && ((!deferred_work_seen && session_state.as_deref() != Some("running"))
                            || (deferred_work_seen && session_state.as_deref() == Some("idle")))
                        && pending.is_empty())
                {
                    break;
                }
            }
        }
        anyhow::ensure!(sent_prompt, "Claude native initialize did not complete");
        if result_seen && !out.failed {
            if let (Some(owner), Some(session)) = (artifact_owner.as_ref(), session) {
                if let Err(reason) = capture_context(&mut input, &mut output, owner, session,
                    interactions.capabilities.permissions.connection_id.as_deref().unwrap_or_default(), &handle, req).await {
                    if let Some(tx) = &progress { let _ = tx.send(TurnProgress::Notice(format!("Native context remains unknown: {reason}"))); }
                }
            }
        }
        Ok(())
    }.await;
    out.cancelled |= handle.cancel_flag.load(std::sync::atomic::Ordering::SeqCst);
    if let Err(error) = result {
        if !out.cancelled {
            out.failed = true;
            out.stderr_tail = format!("{error:#}");
        }
    }
    if out.cancelled || out.failed {
        for task_id in deferred_tasks.drain() {
            let title = deferred_task_titles
                .remove(&task_id)
                .unwrap_or_else(|| "Claude delegated task".into());
            let tool_use_id = deferred_task_tools.remove(&task_id).flatten();
            native_task_card(
                &task_id,
                &title,
                crate::provider::stream_cards::CardStatus::Error,
                Some("interrupted before delegated task completion".into()),
                &mut out,
                &progress,
                interactions
                    .capabilities
                    .permissions
                    .connection_id
                    .as_deref(),
                tool_use_id.as_deref(),
            );
        }
    }
    for (_, request) in pending.drain() {
        request.native.invalidate();
    }
    drop(interactions);
    let _ = input.shutdown().await;
    drop(input);
    let mut exit = None;
    if let Some(mut child) = handle.child.lock().await.take() {
        if !out.failed && !out.cancelled {
            if let Ok(Ok(status)) = tokio::time::timeout(Duration::from_secs(2), child.wait()).await
            {
                exit = Some(status);
            }
        }
        if exit.is_none() {
            kill_process_tree(handle.pid.load(Ordering::SeqCst));
            let _ = child.start_kill();
            if let Ok(Ok(status)) = tokio::time::timeout(REAP_TIMEOUT, child.wait()).await {
                exit = Some(status);
            }
        }
    }
    if let Ok(Ok(tail)) = tokio::time::timeout(STDERR_JOIN_TIMEOUT, &mut stderr_task).await {
        if !tail.is_empty() {
            out.stderr_tail = if out.stderr_tail.is_empty() {
                tail
            } else {
                format!("{}\n{tail}", out.stderr_tail)
            };
        }
    } else {
        stderr_task.abort();
    }
    out.failed |=
        exit.is_none() || exit.as_ref().is_some_and(|status| !status.success()) && !out.cancelled;
    out.exit_code = exit.map(|status| {
        if out.failed {
            1
        } else {
            status.code().unwrap_or(0)
        }
    });
    Ok(out)
}

/// Capture only after a completed turn on this same live connection. A newly resumed
/// process is not evidence that its prior context has already been loaded.
async fn capture_context(
    input: &mut tokio::process::ChildStdin,
    output: &mut tokio::io::BufReader<tokio::process::ChildStdout>,
    owner: &crate::provider::interaction::InteractionOwner,
    session: &str,
    connection: &str,
    handle: &RunningTurn,
    req: &ProviderTurnRequest<'_>,
) -> Result<(), String> {
    let root = crate::store::default_store_root();
    let home = owner
        .home
        .as_deref()
        .ok_or("native context home is unavailable")?;
    let binding =
        crate::native_memory::Binding::new(ProviderKind::Claude, &owner.account, home, session)?;
    if crate::capacity::load(&root, &binding)?
        .rollover_percent
        .is_none()
    {
        return Ok(());
    }
    let model = req
        .model
        .ok_or("choose an explicit model before enabling native context measurements")?;
    let before = tokio::task::spawn_blocking({
        let root = root.clone();
        let binding = binding.clone();
        move || crate::native_memory::control_checkpoint(&root, &binding)
    })
    .await
    .map_err(|error| error.to_string())??;
    let request_id = uuid::Uuid::new_v4().to_string();
    let connection_id = connection.to_owned();
    write_frame(
        input,
        &json!({"type":"control_request","request_id":request_id,
        "request":{"subtype":"get_context_usage"}}),
    )
    .await
    .map_err(|error| error.to_string())?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut total = 0usize;
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        let received = tokio::select! {
            _ = handle.wake.notified() => return Err("context capture cancelled".into()),
            received = tokio::time::timeout_at(deadline, read_line_bounded(output, &mut bytes)) =>
                received.map_err(|_| "native context capture timed out")?.map_err(|error| error.to_string())?,
        };
        total = total.saturating_add(received);
        if received == 0 || received > LINE_MAX || total > 2 * 1024 * 1024 {
            return Err("native context capture stream ended or exceeded its bound".into());
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "invalid native context response")?;
        if value
            .get("session_id")
            .and_then(Value::as_str)
            .is_some_and(|id| id != session)
        {
            return Err("native context response belongs to another session".into());
        }
        if value["type"] != "control_response" || value["response"]["request_id"] != request_id {
            if value["type"] == "control_request" {
                if let Some(id) = value["request_id"].as_str().filter(|id| {
                    !id.is_empty() && id.len() <= MAX_ID && !id.chars().any(char::is_control)
                }) {
                    write_frame(input, &json!({"type":"control_response","response":{
                        "subtype":"error","request_id":id,"error":"The turn has finished; no permission was granted"}})).await.map_err(|error| error.to_string())?;
                }
            }
            continue;
        }
        if value["response"]["subtype"] != "success" {
            return Err("native context request was rejected".into());
        }
        let payload = value
            .pointer("/response/response")
            .ok_or("native context response lacks usage")?;
        let response = serde_json::to_vec(payload).map_err(|error| error.to_string())?;
        let usage = crate::capacity::claude_capacity::read(
            &response,
            crate::capacity::claude_capacity::ContextBinding {
                provider_version: SUPPORTED_VERSION.into(),
                account: owner.account.clone(),
                home: binding.home.clone(),
                session_id: session.into(),
                control_request_id: request_id.clone(),
                connection_generation: connection_id.clone(),
                requested_model: model.into(),
            },
        )?;
        let after = tokio::task::spawn_blocking({
            let root = root.clone();
            let binding = binding.clone();
            move || crate::native_memory::control_checkpoint(&root, &binding)
        })
        .await
        .map_err(|error| error.to_string())??;
        if before != after {
            return Err("native source changed during context capture".into());
        }
        use sha2::{Digest, Sha256};
        return crate::capacity::claude_capacity::save(
            &root,
            &binding,
            model,
            after,
            crate::capacity::Measurement {
                used: usage.total_tokens,
                window: usage.effective_max_tokens,
            },
            crate::capacity::ControlEvidence {
                connection_id,
                request_id,
                response_sha256: format!("{:x}", Sha256::digest(&response)),
            },
            &req.compaction,
        );
    }
}
