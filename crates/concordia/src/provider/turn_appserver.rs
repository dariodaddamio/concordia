use super::*;
use crate::provider::{interaction::InteractionConnection, CardStatus};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const TOOL_MAP_MAX: usize = 4096;
const FRAME_MAX: usize = 64 * 1024 * 1024;
const TEXT_CHUNK_MAX: usize = 64 * 1024;

fn remember_delta(deltas: &mut HashSet<String>, id: String) -> bool {
    if deltas.contains(&id) || deltas.len() < TOOL_MAP_MAX {
        deltas.insert(id);
        true
    } else {
        false
    }
}

struct Connection {
    input: tokio::process::ChildStdin,
    output: mpsc::Receiver<Result<Value>>,
    handle: Arc<RunningTurn>,
    progress: Option<mpsc::Sender<TurnProgress>>,
    interactions: InteractionConnection,
    out: TurnOutcome,
    thread: Option<String>,
    turn: Option<String>,
    pending: HashMap<String, NativeRequest>,
    answers: mpsc::UnboundedReceiver<(Value, Option<Value>)>,
    answer_tx: mpsc::UnboundedSender<(Value, Option<Value>)>,
    waiters: tokio::task::JoinSet<()>,
    deltas: HashSet<String>,
    text_item: Option<String>,
    dropped_deltas: usize,
    buffered: Vec<Value>,
    artifacts: crate::artifacts::Collector,
    bridge_rx: tokio::sync::mpsc::UnboundedReceiver<crate::ask_bridge::BridgeRequest>,
    steers: mpsc::Receiver<SteerRequest>,
    steering: Option<(u64, SteerRequest)>,
    next_steer_id: u64,
}

struct QuestionListener(tokio::task::JoinHandle<()>);

impl Drop for QuestionListener {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl Connection {
    async fn text_fragment(&mut self, id: &str, text: &str) {
        if self.text_item.as_deref() != Some(id) {
            if self.text_item.is_some() {
                append_visible_text(&mut self.out.text, "\n\n");
                self.emit(TurnProgress::TextBoundary).await;
            }
            self.text_item = Some(id.into());
        }
        append_visible_text(&mut self.out.text, text);
        let mut remaining = text;
        while !remaining.is_empty() && !self.handle.cancel_flag.load(Ordering::SeqCst) {
            let mut end = remaining.len().min(TEXT_CHUNK_MAX);
            while !remaining.is_char_boundary(end) {
                end -= 1;
            }
            self.emit(TurnProgress::TextDelta(remaining[..end].into()))
                .await;
            remaining = &remaining[end..];
        }
    }
    async fn emit(&self, event: TurnProgress) {
        let _ = send_progress(&self.progress, event, &self.handle).await;
    }
    async fn write(&mut self, value: Value) -> Result<()> {
        anyhow::ensure!(!self.handle.cancel_flag.load(Ordering::SeqCst), "cancelled");
        self.input
            .write_all(format!("{value}\n").as_bytes())
            .await?;
        self.input.flush().await?;
        Ok(())
    }
    async fn read(&mut self) -> Result<Value> {
        anyhow::ensure!(!self.handle.cancel_flag.load(Ordering::SeqCst), "cancelled");
        tokio::select! {
            _=self.handle.wake.notified()=>anyhow::bail!("cancelled"),
            result=self.output.recv()=>result.context("Codex app-server closed before turn completion; no fallback was started")?,
        }
    }
    async fn rpc(&mut self, id: u64, method: &str, params: Value) -> Result<Value> {
        self.write(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        let mut buffered_bytes = 0;
        loop {
            let value = self.read().await?;
            if value.get("method").is_some() {
                if method == "turn/start" {
                    buffered_bytes += value.to_string().len();
                    anyhow::ensure!(
                        self.buffered.len() < 256 && buffered_bytes <= FRAME_MAX,
                        "Too many early native events"
                    );
                    self.buffered.push(value);
                } else if value.get("id").is_some() {
                    self.reject(&value["id"], "No active verified turn").await?;
                }
                continue;
            }
            if value["id"] == id {
                if let Some(error) = value.get("error") {
                    let structured = serde_json::to_string(error)
                        .unwrap_or_else(|_| r#"{"code":"native_request_failed"}"#.into());
                    anyhow::bail!("Codex app-server rejected {method}: {structured}");
                }
                return value
                    .get("result")
                    .cloned()
                    .context("Native response missing result");
            }
        }
    }
    async fn reject(&mut self, id: &Value, message: &str) -> Result<()> {
        self.write(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":message}}))
            .await
    }
    async fn review_finished(&mut self, request: &NativeRequest, title: &str) {
        if !matches!(
            request.kind(),
            crate::provider::interaction::NativeInteraction::CodexPermission { .. }
        ) {
            return;
        }
        let card = StreamCard {
            id: format!(
                "native-review:{}:{}",
                request.envelope().connection_id,
                request.envelope().request_id
            ),
            title: title.into(),
            detail: None,
            status: CardStatus::Done,
        };
        crate::provider::stream_cards::upsert_card(&mut self.out.cards, card.clone());
        self.emit(TurnProgress::Card(card)).await;
    }
    async fn event(&mut self, value: Value) -> Result<bool> {
        if value.get("method").is_none() {
            if self
                .steering
                .as_ref()
                .is_some_and(|(id, _)| value["id"] == *id)
            {
                let (_, request) = self.steering.take().unwrap();
                let result = if let Some(error) = value.get("error") {
                    Err(format!(
                        "Codex refused steering: {}",
                        error["message"]
                            .as_str()
                            .unwrap_or("native request rejected")
                            .chars()
                            .take(240)
                            .collect::<String>()
                    ))
                } else if value["result"]["turnId"].as_str() == self.turn.as_deref() {
                    Ok(())
                } else {
                    Err("Steering was not confirmed for this turn; check native history before resending".into())
                };
                let _ = request.reply.send(result);
            }
            return Ok(false);
        }
        let method = value["method"].as_str().unwrap_or("");
        let params = &value["params"];
        let same_thread = params["threadId"].as_str() == self.thread.as_deref();
        let same_turn = params["turnId"].as_str() == self.turn.as_deref();
        if value.get("id").is_some() {
            if !same_thread || !same_turn {
                return self
                    .reject(
                        &value["id"],
                        "Native request does not belong to the active turn or exceeds its budget",
                    )
                    .await
                    .map(|_| false);
            }
            if !matches!(
                method,
                "item/commandExecution/requestApproval"
                    | "item/fileChange/requestApproval"
                    | "item/tool/requestUserInput"
            ) {
                self.emit(TurnProgress::Notice(format!(
                    "Unsupported native request: {method}; no permission granted"
                )))
                .await;
                return self
                    .reject(&value["id"], "Client capability is not supported")
                    .await
                    .map(|_| false);
            }
            let request = self.interactions.request(
                self.thread.as_deref().unwrap_or_default(),
                value["id"].clone(),
                method.into(),
                params.clone(),
            );
            let (request, rx) = match request {
                Ok(Some(request)) => request,
                Ok(None) => return Ok(false),
                Err(reason) => {
                    self.emit(TurnProgress::Notice(format!(
                        "Native request unavailable: {reason}"
                    )))
                    .await;
                    return self
                        .reject(&value["id"], "Native request cannot be represented safely")
                        .await
                        .map(|_| false);
                }
            };
            let id = value["id"].clone();
            let key = id.to_string();
            anyhow::ensure!(
                !self.pending.contains_key(&key),
                "Duplicate native request ID"
            );
            let tx = self.answer_tx.clone();
            self.waiters.spawn(async move {
                let _ = tx.send((id, rx.await.ok()));
            });
            if !send_progress(
                &self.progress,
                TurnProgress::Interaction(Box::new(request.clone())),
                &self.handle,
            )
            .await
            {
                request.cancel();
            }
            self.pending.insert(key, request);
            return Ok(false);
        }
        if !same_thread {
            return Ok(false);
        }
        if method == "serverRequest/resolved" {
            if let Some(request) = self.pending.remove(&params["requestId"].to_string()) {
                request.resolve_native();
                self.review_finished(&request, "Native request cleared by provider")
                    .await;
            }
            return Ok(false);
        }
        if method == "turn/completed" {
            if params["turn"]["id"].as_str() != self.turn.as_deref() {
                return Ok(false);
            }
            for (_, request) in std::mem::take(&mut self.pending) {
                request.resolve_native();
                self.review_finished(&request, "Native request ended with turn")
                    .await;
            }
            match params["turn"]["status"].as_str() {
                Some("completed") => {
                    self.out.terminal_outcome = TurnTerminalOutcome::Success;
                }
                Some("interrupted") => {
                    self.out.cancelled = true;
                    self.out.terminal_outcome = TurnTerminalOutcome::Cancelled;
                }
                _ => {
                    self.out.failed = true;
                    let error = &params["turn"]["error"];
                    let error_kind = error
                        .get("code")
                        .or_else(|| error.get("type"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    self.out.terminal_outcome = if matches!(
                        error_kind,
                        "quota_exceeded" | "rate_limit_exceeded" | "usage_limit_exceeded"
                    ) {
                        TurnTerminalOutcome::Quota
                    } else {
                        TurnTerminalOutcome::Crash
                    };
                    self.out.stderr_tail = serde_json::to_string(error)
                        .unwrap_or_else(|_| r#"{"type":"error"}"#.into())
                        .chars()
                        .take(2000)
                        .collect();
                    self.emit(TurnProgress::Error(self.out.stderr_tail.clone()))
                        .await;
                }
            }
            return Ok(true);
        }
        if !same_turn {
            return Ok(false);
        }
        match method {
            "item/agentMessage/delta" => {
                if let (Some(id), Some(delta)) =
                    (params["itemId"].as_str(), params["delta"].as_str())
                {
                    if !remember_delta(&mut self.deltas, id.into()) {
                        self.dropped_deltas += 1;
                    }
                    self.text_fragment(id, delta).await;
                }
            }
            "item/reasoning/summaryPartAdded" => {
                self.emit(TurnProgress::ThinkingDelta("\n".into())).await;
            }
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                if let Some(delta) = params["delta"].as_str() {
                    self.emit(TurnProgress::ThinkingDelta(delta.into())).await;
                }
            }
            "item/started" | "item/completed" => {
                self.item(&params["item"], method == "item/completed").await
            }
            _ => {}
        }
        Ok(false)
    }
    async fn item(&mut self, item: &Value, completed: bool) {
        if completed {
            self.artifacts
                .report(
                    self.interactions.owner(),
                    self.thread.as_deref(),
                    self.turn.as_deref(),
                    item,
                    &self.progress,
                    &self.handle,
                )
                .await;
        }
        let Some(id) = item["id"].as_str().filter(|id| !id.is_empty()) else {
            return;
        };
        let kind = item["type"].as_str().unwrap_or("");
        if kind == "agentMessage" {
            if completed && !self.deltas.contains(id) {
                if let Some(text) = item["text"].as_str() {
                    self.text_fragment(id, text).await;
                }
            }
            if completed && self.text_item.as_deref() == Some(id) {
                self.emit(TurnProgress::TextBoundary).await;
            }
            return;
        }
        if kind == "collabAgentToolCall" {
            let translated = json!({"receiver_thread_ids":item["receiverThreadIds"],"sender_thread_id":item["senderThreadId"],"agents_states":item["agentsStates"],"prompt":item["prompt"],"tool":item["tool"],"status":item["status"]});
            for part in crate::provider::stream_cards::classify_collab_tool_call(&translated) {
                if let StreamPart::Card(card) = part {
                    crate::provider::stream_cards::upsert_card(&mut self.out.cards, card.clone());
                    self.emit(TurnProgress::Card(card)).await;
                }
            }
            return;
        }
        let (title, detail) = match kind {
            "commandExecution" => (
                "Run command".into(),
                Some(format!(
                    "{}\nDirectory: {}\n{}",
                    item["command"].as_str().unwrap_or(""),
                    item["cwd"].as_str().unwrap_or(""),
                    item["aggregatedOutput"].as_str().unwrap_or("")
                )),
            ),
            "fileChange" => (
                "Edit".into(),
                crate::provider::edit_display::changes(&item["changes"]),
            ),
            "mcpToolCall" => (
                format!(
                    "{} / {}",
                    item["server"].as_str().unwrap_or("MCP"),
                    item["tool"].as_str().unwrap_or("tool")
                ),
                Some(item.to_string()),
            ),
            "webSearch" => ("Search the web".into(), Some(item.to_string())),
            "imageView" => ("View image".into(), Some(item.to_string())),
            _ => return,
        };
        let status = if matches!(item["status"].as_str(), Some("failed" | "declined"))
            || item["exitCode"].as_i64().is_some_and(|code| code != 0)
        {
            CardStatus::Error
        } else if completed {
            CardStatus::Done
        } else {
            CardStatus::Running
        };
        let card = StreamCard {
            id: id.into(),
            title,
            detail,
            status,
        };
        crate::provider::stream_cards::upsert_card(&mut self.out.cards, card.clone());
        self.emit(TurnProgress::Card(card)).await;
    }
    async fn bridge_request(&mut self, request: crate::ask_bridge::BridgeRequest) -> Result<()> {
        let (Some(session), Some(turn)) = (self.thread.clone(), self.turn.clone()) else {
            let _ = request.answer.send(json!(crate::ask_bridge::DISMISSED));
            return Ok(());
        };
        let Some(questions) = request.questions.as_array() else {
            let _ = request.answer.send(json!(crate::ask_bridge::DISMISSED));
            return Ok(());
        };
        let bridge_id = format!("concordia-ask-user-{}", uuid::Uuid::new_v4());
        let native = json!({
            "sessionId": session,
            "threadId": session,
            "turnId": turn,
            "toolCallId": bridge_id,
            "prompt": "The agent needs a decision",
            "questions": questions.iter().enumerate().map(|(i, question)| json!({
                "id": format!("question-{i}"),
                "prompt": question["question"],
                "options": question["options"].as_array().into_iter().flatten().enumerate().map(|(j, option)| json!({
                    "id": format!("option-{i}-{j}"),
                    "label": option,
                    "description": ""
                })).collect::<Vec<_>>(),
                "allowMultiple": question["multiple"].as_bool().unwrap_or(false)
            })).collect::<Vec<_>>()
        });
        let (native_request, rx) = self
            .interactions
            .request(
                &session,
                json!(bridge_id),
                "cursor/ask_question".into(),
                native,
            )
            .map_err(|e| anyhow::anyhow!(e))?
            .ok_or_else(|| anyhow::anyhow!("bridge request identity was reused"))?;
        if !send_progress(
            &self.progress,
            TurnProgress::Interaction(Box::new(native_request.clone())),
            &self.handle,
        )
        .await
        {
            native_request.cancel();
        }
        let answer = tokio::select! {
            _ = self.handle.wake.notified() => {
                native_request.invalidate();
                let _ = request.answer.send(json!(crate::ask_bridge::DISMISSED));
                anyhow::bail!("cancelled");
            }
            answer = rx => answer.unwrap_or_else(|_| json!({"outcome":{"outcome":"cancelled"}})),
        };
        native_request.invalidate();
        let text = crate::ask_bridge::answer_text(questions, &answer);
        let _ = request.answer.send(json!(text));
        Ok(())
    }
    async fn listen(&mut self) -> Result<()> {
        for value in std::mem::take(&mut self.buffered) {
            if self.event(value).await? {
                return Ok(());
            }
        }
        loop {
            anyhow::ensure!(!self.handle.cancel_flag.load(Ordering::SeqCst), "cancelled");
            tokio::select! {
                biased;
                _=self.handle.wake.notified()=>anyhow::bail!("cancelled"),
                Some(request)=self.steers.recv(), if self.steering.is_none()=> {
                    let id = self.next_steer_id;
                    self.next_steer_id += 1;
                    let frame = json!({"jsonrpc":"2.0","id":id,"method":"turn/steer","params":{
                        "threadId":self.thread,"expectedTurnId":self.turn,
                        "input":[{"type":"text","text":request.text,"text_elements":[]}]
                    }});
                    self.steering = Some((id, request));
                    self.write(frame).await?;
                }
                Some(request)=self.bridge_rx.recv()=> {
                    self.bridge_request(request).await?;
                }
                Some((id,answer))=self.answers.recv()=> {
                    if let Some(request)=self.pending.remove(&id.to_string()) {
                        if request.was_resolved() {continue;}
                        if let Some(answer)=answer {
                            let decision=answer.get("decision").map(Value::to_string).unwrap_or_else(||"answers".into());
                            self.write(json!({"jsonrpc":"2.0","id":id,"result":answer})).await?;
                            self.review_finished(&request,&format!("Native decision sent: {decision}")).await;
                        } else { self.reject(&id,"Native input was closed without an answer").await?; }
                    }
                }
                next=self.output.recv()=> {if self.event(next.context("Codex app-server closed before turn completion; no fallback was started")??).await? {return Ok(());}},
            }
            while self.waiters.try_join_next().is_some() {}
        }
    }
}

const QUALIFIED_VERSION: &str = "codex-cli 0.166.0";
const QUALIFIED_CLI_VERSION: &str = "0.166.0";
pub(super) const UNAVAILABLE: &str = "Codex native protocol unavailable before the prompt was sent";
pub(super) const INCOMPATIBLE: &str =
    "Codex app-server protocol is older than Concordia's qualified minimum";
pub(super) const UNKNOWN_COMPATIBILITY: &str =
    "Codex app-server initialize.userAgent was missing or unparseable";

pub(super) async fn run(
    req: &ProviderTurnRequest<'_>,
    plan: &PlannedTurn,
    owner: Option<InteractionOwner>,
    started: Option<mpsc::UnboundedSender<Arc<RunningTurn>>>,
    progress: Option<mpsc::Sender<TurnProgress>>,
    handle: Arc<RunningTurn>,
) -> Result<TurnOutcome> {
    anyhow::ensure!(
        req.images.len() <= crate::attachments::MAX_IMAGES,
        "At most four images per message"
    );
    let home = req
        .home_path
        .context("Native Codex input requires an isolated account home")?
        .canonicalize()?;
    let native = crate::provider::npm::codex(&plan.command);
    let bridge = native
        .is_none()
        .then(|| crate::provider::npm::node_bridge(&plan.command, ProviderKind::Codex))
        .flatten();
    let program = bridge.as_ref().map_or_else(
        || {
            native.as_ref().map_or_else(
                || plan.command.clone(),
                |n| n.program.to_string_lossy().into_owned(),
            )
        },
        |(node, _)| node.to_string_lossy().into_owned(),
    );
    anyhow::ensure!(
        !crate::provider::launch::is_batch_shim(&program),
        "Native Codex input requires the verified stock npm bridge or a native executable"
    );
    let version_program = program.clone();
    let version_prefix = bridge
        .as_ref()
        .map(|(_, script)| script.to_string_lossy().into_owned());
    let version = tokio::task::spawn_blocking(move || {
        let mut args = Vec::new();
        if let Some(script) = version_prefix.as_deref() {
            args.push(script);
        }
        args.push("--version");
        crate::provider::compat::run_cli(Path::new(&version_program), &args)
    })
    .await?
    .map_err(anyhow::Error::msg)?;
    if version.trim() != QUALIFIED_VERSION {
        crate::logging::info(&format!(
            "codex app-server: installed {} differs from the qualified {QUALIFIED_VERSION}",
            version.trim()
        ));
    }
    let read_only = planned_options(&plan.argv)
        .any(|(flag, value)| matches!(flag, "-s" | "--sandbox") && value == Some("read-only"));
    let full = planned_options(&plan.argv)
        .any(|(flag, _)| flag == "--dangerously-bypass-approvals-and-sandbox");
    let (sandbox, approval, policy) = if read_only {
        (
            "read-only",
            "never",
            json!({"type":"readOnly","networkAccess":false}),
        )
    } else if full {
        (
            "danger-full-access",
            "never",
            json!({"type":"dangerFullAccess"}),
        )
    } else {
        anyhow::ensure!(
            planned_options(&plan.argv).any(|(flag, _)| flag == "--approve-for-me"),
            "Final Codex plan has no verified sandbox policy; prompt withheld"
        );
        (
            "workspace-write",
            "on-request",
            json!({"type":"workspaceWrite","writableRoots":[plan.cwd],"networkAccess":false,"excludeTmpdirEnvVar":true,"excludeSlashTmp":true}),
        )
    };
    let mut content = vec![json!({"type":"text","text":plan.prompt})];
    for image in req.images {
        content.push(json!({"type":"image","url":format!("data:{};base64,{}",image.mime,image.data().map_err(anyhow::Error::msg)?)}));
    }
    let ask_token = uuid::Uuid::new_v4().to_string();
    let (ask_tx, bridge_rx) = tokio::sync::mpsc::unbounded_channel();
    let (ask_endpoint, ask_listener) =
        crate::ask_bridge::spawn_listener(ask_token.clone(), ask_tx).await?;
    let _question_listener = QuestionListener(ask_listener);
    let ask_command = std::env::current_exe()
        .context("Could not locate Concordia for the Codex ask MCP server")?
        .to_string_lossy()
        .into_owned();
    let mut command = crate::process_ui::tokio_command(&program);
    crate::process_ui::background(&mut command);
    #[cfg(unix)]
    command.process_group(0);
    if let Some((_, script)) = bridge {
        command.arg(script);
    }
    command.arg("app-server");
    for (flag, value) in planned_options(&plan.argv).filter(|(flag, _)| *flag == "-c") {
        if let Some(value) = value {
            command.args([flag, value]);
        }
    }
    let ask_name = toml::Value::String("concordia-ask".into());
    let ask_command = toml::Value::String(ask_command);
    let ask_endpoint = toml::Value::String(ask_endpoint);
    let ask_token = toml::Value::String(ask_token);
    for value in [
        format!("mcp_servers.{ask_name}.command={ask_command}"),
        format!("mcp_servers.{ask_name}.args=[\"ask-mcp\"]"),
        format!("mcp_servers.{ask_name}.env.CONCORDIA_ASK_ENDPOINT={ask_endpoint}"),
        format!("mcp_servers.{ask_name}.env.CONCORDIA_ASK_TOKEN={ask_token}"),
    ] {
        command.arg("-c").arg(value);
    }
    command.envs(plan.env.clone());
    if let Some(native) = native {
        command.envs(native.env);
    }
    command
        .current_dir(&plan.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if handle.cancel_flag.load(Ordering::SeqCst) {
        return Ok(TurnOutcome {
            cancelled: true,
            terminal_outcome: TurnTerminalOutcome::Cancelled,
            ..Default::default()
        });
    }
    let mut child = command
        .spawn()
        .context("Could not start Codex app-server")?;
    let _process_tree =
        crate::process_lifetime::own_tokio_process(&child).map_err(anyhow::Error::msg)?;
    handle.pid.store(child.id().unwrap_or(0), Ordering::SeqCst);
    let input = child.stdin.take().context("Codex stdin unavailable")?;
    let stdout = child.stdout.take().context("Codex stdout unavailable")?;
    let (output_tx, output) = mpsc::channel(1);
    let output_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut bytes = Vec::new();
            let next = async {
                let n = read_line_bounded_with_limit(&mut reader, &mut bytes, FRAME_MAX).await?;
                anyhow::ensure!(
                    n > 0,
                    "Codex app-server closed before turn completion; no fallback was started"
                );
                anyhow::ensure!(
                    n <= FRAME_MAX,
                    "Codex app-server event exceeds the 64 MiB transport frame limit; original native session is preserved"
                );
                if bytes.len() > LINE_MAX {
                    tokio::task::spawn_blocking(move || serde_json::from_slice::<Value>(&bytes))
                        .await?
                        .context("Invalid Codex app-server JSON")
                } else {
                    serde_json::from_slice::<Value>(&bytes).context("Invalid Codex app-server JSON")
                }
            }
            .await;
            let failed = next.is_err();
            if output_tx.send(next).await.is_err() || failed {
                break;
            }
        }
    });
    let stderr = child.stderr.take().context("Codex stderr unavailable")?;
    let mut stderr_task = tokio::spawn(read_stderr_tail(stderr, || {}));
    *handle.child.lock().await = Some(child);
    if let Some(tx) = started {
        if tx.send(handle.clone()).is_err() {
            handle.cancel();
        }
    }
    let (answer_tx, answers) = mpsc::unbounded_channel();
    let (steer_tx, steers) = mpsc::channel(1);
    let mut conn = Connection {
        input,
        output,
        handle: handle.clone(),
        progress,
        interactions: InteractionConnection::new(owner, handle.cancel_flag.clone()),
        out: TurnOutcome::default(),
        thread: None,
        turn: None,
        pending: HashMap::new(),
        answers,
        answer_tx,
        waiters: tokio::task::JoinSet::new(),
        deltas: HashSet::new(),
        text_item: None,
        dropped_deltas: 0,
        buffered: Vec::new(),
        artifacts: crate::artifacts::Collector::default(),
        bridge_rx,
        steers,
        steering: None,
        next_steer_id: 100,
    };
    let mut prompt_sent = false;
    let mut protocol_failure = true;
    let result=async {
        let init=tokio::time::timeout(Duration::from_secs(12),conn.rpc(1,"initialize",json!({"clientInfo":{"name":"concordia","title":"Concordia","version":crate::version::VERSION},"capabilities":{"experimentalApi":true}}))).await??;
        protocol_failure = false;
        anyhow::ensure!(init["codexHome"].as_str().and_then(|path|Path::new(path).canonicalize().ok()).as_ref()==Some(&home),"Codex app-server account home mismatch; prompt withheld");
        protocol_failure = true;
        let protocol_version = init["userAgent"]
            .as_str()
            .and_then(crate::provider::compat::version_token);
        let protocol_version = protocol_version.ok_or_else(|| {
            anyhow::anyhow!(
                "{UNKNOWN_COMPATIBILITY}; expected codex-cli <version> >= {QUALIFIED_CLI_VERSION}; the draft and session were preserved"
            )
        })?;
        anyhow::ensure!(
            crate::provider::compat::version_at_least(&protocol_version, QUALIFIED_CLI_VERSION),
            "{INCOMPATIBLE}: {protocol_version}; the draft and session were preserved"
        );
        conn.interactions.initialized_codex(&protocol_version);
        if let Some(progress) = conn.progress.as_ref() {
            let _ = progress
                .send(TurnProgress::Notice(format!(
                    "Codex app-server protocol {protocol_version}; Concordia's qualified schema is {QUALIFIED_CLI_VERSION}."
                )))
                .await;
        }
        conn.write(json!({"jsonrpc":"2.0","method":"initialized","params":{}})).await?;
        let mut params=json!({"cwd":plan.cwd,"sandbox":sandbox,"approvalPolicy":approval,"approvalsReviewer":"user"});
        if let Some(model)=req.model {params["model"]=json!(model);}
        let method=if let Some(id)=req.native_session_id {params["threadId"]=json!(id);params["excludeTurns"]=json!(true);"thread/resume"} else {"thread/start"};
        let thread=tokio::time::timeout(Duration::from_secs(12),conn.rpc(2,method,params)).await??;
        protocol_failure = false;
        let id=thread["thread"]["id"].as_str().filter(|id|!id.is_empty()&&id.len()<=4096).context("Native thread ID missing")?;
        anyhow::ensure!(req.native_session_id.is_none_or(|expected|expected==id),"Codex resumed a different thread; prompt withheld");
        anyhow::ensure!(thread["cwd"].as_str().and_then(|path|Path::new(path).canonicalize().ok())==Some(Path::new(&plan.cwd).canonicalize()?),"Codex thread workspace mismatch; prompt withheld");
        anyhow::ensure!(thread["approvalPolicy"]==approval && thread["approvalsReviewer"]=="user","Codex did not honor the requested approval policy; prompt withheld");
        conn.thread=Some(id.into());conn.out.session_id=Some(id.into());conn.emit(TurnProgress::Session(id.into())).await;
        let command = req.prompt.split_whitespace().next().unwrap_or("");
        if command == "/compact" && req.prompt.split_whitespace().count() == 1 {
            prompt_sent = true;
            tokio::time::timeout(Duration::from_secs(12), conn.rpc(4, "thread/compact/start", json!({"threadId": id}))).await??;
            conn.emit(TurnProgress::Notice("Codex compaction requested".into())).await;
            return Ok::<(), anyhow::Error>(());
        }
        if command == "/fork" && req.prompt.split_whitespace().count() == 1 {
            prompt_sent = true;
            let fork = tokio::time::timeout(Duration::from_secs(12), conn.rpc(4, "thread/fork", json!({"threadId": id, "cwd": plan.cwd, "sandbox": sandbox, "approvalPolicy": approval, "approvalsReviewer": "user", "excludeTurns": true}))).await??;
            let forked = fork["thread"]["id"].as_str().filter(|forked| !forked.is_empty() && forked.len() <= 4096).context("Codex fork returned no thread ID")?;
            anyhow::ensure!(forked != id, "Codex fork returned the original thread; nothing was changed");
            anyhow::ensure!(fork["cwd"].as_str().and_then(|path| Path::new(path).canonicalize().ok()) == Some(Path::new(&plan.cwd).canonicalize()?), "Codex fork workspace mismatch; the chat stays on the original thread");
            anyhow::ensure!(fork["approvalPolicy"] == approval && fork["approvalsReviewer"] == "user", "Codex fork did not honor the approval policy; the chat stays on the original thread");
            conn.thread = Some(forked.into());
            conn.out.session_id = Some(forked.into());
            conn.emit(TurnProgress::Session(forked.into())).await;
            conn.emit(TurnProgress::Notice("Forked into a new Codex thread; this chat continues there and the original thread is unchanged.".into())).await;
            return Ok::<(), anyhow::Error>(());
        }
        if command == "/review" && req.prompt.split_whitespace().count() == 1 {
            prompt_sent = true;
            let review = tokio::time::timeout(Duration::from_secs(12), conn.rpc(4, "review/start", json!({"threadId": id, "delivery": "inline", "target": {"type": "uncommittedChanges"}}))).await??;
            conn.turn = Some(review["turn"]["id"].as_str().filter(|turn| !turn.is_empty() && turn.len() <= 4096).context("Codex review returned no turn ID")?.into());
            return conn.listen().await;
        }
        let mut params=json!({"threadId":id,"input":content,"cwd":plan.cwd,"approvalPolicy":approval,"approvalsReviewer":"user","sandboxPolicy":policy});
        if let Some(model)=req.model {params["model"]=json!(model);}
        if let Some(effort)=req.effort.filter(|effort| !effort.trim().is_empty()) {
            params["effort"] = json!(effort);
        }
        prompt_sent = true;
        let turn=tokio::time::timeout(Duration::from_secs(12),conn.rpc(3,"turn/start",params)).await??;
        conn.turn=Some(turn["turn"]["id"].as_str().filter(|id|!id.is_empty()&&id.len()<=4096).context("Native turn ID missing")?.into());
        handle.set_steering(Some(steer_tx));
        conn.listen().await
    }.await;
    handle.set_steering(None);
    if let Some((_, request)) = conn.steering.take() {
        let _ = request.reply.send(Err("Steering was not confirmed before the connection ended; check native history before resending".into()));
    }
    conn.out.cancelled |= handle.cancel_flag.load(Ordering::SeqCst);
    if conn.out.cancelled {
        conn.out.terminal_outcome = TurnTerminalOutcome::Cancelled;
    }
    if let Err(error) = result {
        if !conn.out.cancelled && !prompt_sent && protocol_failure {
            conn.out.failed = true;
            conn.out.stderr_tail = format!("{UNAVAILABLE}: {error:#}");
        } else if !conn.out.cancelled {
            conn.out.failed = true;
            if conn.out.terminal_outcome == TurnTerminalOutcome::Unknown {
                conn.out.terminal_outcome = TurnTerminalOutcome::Crash;
            }
            conn.out.stderr_tail = format!("{error:#}");
            conn.emit(TurnProgress::Error(conn.out.stderr_tail.clone()))
                .await;
        }
    }
    for (_, request) in conn.pending.drain() {
        request.invalidate();
    }
    conn.waiters.abort_all();
    output_task.abort();
    kill_process_tree(handle.pid.load(Ordering::SeqCst));
    let mut reaped = false;
    if let Some(mut child) = handle.child.lock().await.take() {
        let _ = child.start_kill();
        reaped = tokio::time::timeout(REAP_TIMEOUT, child.wait())
            .await
            .is_ok_and(|r| r.is_ok());
    }
    if let Ok(Ok(tail)) = tokio::time::timeout(STDERR_JOIN_TIMEOUT, &mut stderr_task).await {
        if !tail.is_empty() {
            conn.out.stderr_tail.push('\n');
            conn.out.stderr_tail.push_str(&tail);
        }
    } else {
        stderr_task.abort();
    }
    conn.out.exit_code = reaped.then_some(if conn.out.failed { 1 } else { 0 });
    Ok(conn.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_ids_are_capped_and_new_events_remain_safe() {
        let mut deltas = HashSet::new();
        for n in 0..5000 {
            let _ = remember_delta(&mut deltas, format!("tool-{n}"));
        }
        assert!(deltas.len() <= TOOL_MAP_MAX);
        let _ = remember_delta(&mut deltas, "new-tool".into());
        assert!(deltas.len() <= TOOL_MAP_MAX);
    }
}
