//! Native Cursor image inputs over ACP with explicit sessions and real permission responses.
use super::*;
use crate::provider::interaction::InteractionConnection;
use crate::provider::CardStatus;
use serde_json::{json, Value};
struct Connection {
    input: tokio::process::ChildStdin,
    output: BufReader<tokio::process::ChildStdout>,
    handle: Arc<RunningTurn>,
    progress: Option<mpsc::UnboundedSender<TurnProgress>>,
    out: TurnOutcome,
    bytes: usize,
    session_id: Option<String>,
    interactions: InteractionConnection,
    prompting: bool,
}
impl Connection {
    fn emit(&self, event: TurnProgress) {
        if let Some(tx) = &self.progress {
            let _ = tx.send(event);
        }
    }
    async fn write(&mut self, value: Value) -> Result<()> {
        self.input
            .write_all(format!("{value}\n").as_bytes())
            .await?;
        self.input.flush().await?;
        Ok(())
    }
    async fn rpc(&mut self, id: u64, method: &str, params: Value) -> Result<Value> {
        self.write(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        loop {
            let mut bytes = Vec::new();
            let next = tokio::select! {
                _=self.handle.wake.notified()=>anyhow::bail!("cancelled"),
                next=read_line_bounded(&mut self.output,&mut bytes)=>next,
            };
            let received = next?;
            anyhow::ensure!(received > 0, "Cursor ACP closed before replying");
            anyhow::ensure!(received <= LINE_MAX, "Cursor ACP response line too large");
            self.bytes += bytes.len();
            anyhow::ensure!(
                self.bytes <= 16 * 1024 * 1024,
                "Cursor ACP output bound exceeded"
            );
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            if value.get("method").is_some() {
                if value.get("id").is_some() {
                    self.request(&value).await?;
                } else {
                    self.update(&value);
                }
                continue;
            }
            if value["id"] == id {
                if let Some(error) = value.get("error") {
                    let message = error["message"]
                        .as_str()
                        .unwrap_or("Cursor ACP request failed");
                    let info = error.get("data").map(Value::to_string).unwrap_or_default();
                    anyhow::bail!(
                        "{} {}",
                        message.chars().take(2000).collect::<String>(),
                        info.chars().take(500).collect::<String>()
                    );
                }
                return value
                    .get("result")
                    .cloned()
                    .context("Cursor ACP response missing result");
            }
        }
    }
    async fn request(&mut self, value: &Value) -> Result<()> {
        let method = value["method"].as_str().unwrap_or("");
        let interactive = matches!(
            method,
            "session/request_permission" | "cursor/ask_question" | "cursor/create_plan"
        );
        if value
            .pointer("/params/sessionId")
            .and_then(Value::as_str)
            .is_some_and(|id| Some(id) != self.session_id.as_deref())
            || interactive
                && (method == "session/request_permission"
                    || value["params"].get("sessionId").is_some())
                && value
                    .pointer("/params/sessionId")
                    .and_then(Value::as_str)
                    .is_none()
        {
            self.emit(TurnProgress::Notice(
                "Native request rejected: session identity mismatch".into(),
            ));
            return self.write(json!({"jsonrpc":"2.0","id":value["id"],"error":{"code":-32602,"message":"Session identity mismatch"}})).await;
        }
        if interactive {
            if !self.prompting {
                return self.write(json!({"jsonrpc":"2.0","id":value["id"],"result":{"outcome":{"outcome":"cancelled"}}})).await;
            }
            let Some(session) = self.session_id.as_deref() else {
                return self.write(json!({"jsonrpc":"2.0","id":value["id"],"error":{"code":-32602,"message":"Session identity is unavailable"}})).await;
            };
            let (request, rx) = match self.interactions.request(
                session,
                value["id"].clone(),
                method.into(),
                value["params"].clone(),
            ) {
                Ok(request) => request,
                Err(reason) => {
                    self.emit(TurnProgress::Notice(format!(
                        "Unsupported native request: {reason}"
                    )));
                    return self.write(json!({"jsonrpc":"2.0","id":value["id"],"result":{"outcome":{"outcome":"cancelled"}}})).await;
                }
            };
            if !self.progress.as_ref().is_some_and(|tx| {
                tx.send(TurnProgress::Interaction(Box::new(request.clone())))
                    .is_ok()
            }) {
                // A closed/missing UI cannot leave a provider awaiting an invisible grant.
                request.respond(json!({"outcome":{"outcome":"cancelled"}}));
            }
            let answer = tokio::select! {
                _=self.handle.wake.notified()=>anyhow::bail!("cancelled"),
                answer=rx=>answer.unwrap_or_else(|_|json!({"outcome":{"outcome":"cancelled"}})),
            };
            request.invalidate();
            anyhow::ensure!(!self.handle.cancel_flag.load(Ordering::SeqCst), "cancelled");
            self.write(json!({"jsonrpc":"2.0","id":value["id"],"result":answer}))
                .await
        } else {
            self.emit(TurnProgress::Notice(
                "Unsupported native interaction method".into(),
            ));
            self.write(json!({"jsonrpc":"2.0","id":value["id"],"error":{"code":-32601,"message":"Client capability is not supported"}})).await
        }
    }
    fn update(&mut self, value: &Value) {
        // session/load replays the old transcript before its response. History belongs to
        // readback, not this turn's answer, thinking stream or active tool cards.
        if !self.prompting {
            return;
        }
        if value.pointer("/params/sessionId").and_then(Value::as_str) != self.session_id.as_deref()
        {
            return;
        }
        let Some(update) = value.pointer("/params/update") else {
            return;
        };
        match update["sessionUpdate"].as_str().unwrap_or("") {
            "agent_message_chunk" => {
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                    self.out.text.push_str(text);
                    self.emit(TurnProgress::TextDelta(text.into()));
                }
            }
            "agent_thought_chunk" => {
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                    self.emit(TurnProgress::ThinkingDelta(text.into()));
                }
            }
            "tool_call" | "tool_call_update" => {
                let status = match update["status"].as_str() {
                    Some("completed") => CardStatus::Done,
                    Some("failed") => CardStatus::Error,
                    _ => CardStatus::Running,
                };
                let id = update["toolCallId"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned();
                let prior = self.out.cards.iter().find(|c| c.id == id);
                let title = update["title"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| prior.map(|c| c.title.clone()))
                    .unwrap_or_else(|| "tool".into());
                let detail = update
                    .get("rawInput")
                    .filter(|v| !v.is_null())
                    .map(Value::to_string)
                    .or_else(|| prior.and_then(|c| c.detail.clone()));
                let card = StreamCard {
                    id,
                    title,
                    detail,
                    status,
                };
                crate::provider::stream_cards::upsert_card(&mut self.out.cards, card.clone());
                self.emit(TurnProgress::Card(card));
            }
            _ => {}
        }
    }
}
pub(super) async fn run(
    req: &ProviderTurnRequest<'_>,
    plan: &PlannedTurn,
    owner: Option<InteractionOwner>,
    started: Option<mpsc::UnboundedSender<Arc<RunningTurn>>>,
    progress: Option<mpsc::UnboundedSender<TurnProgress>>,
) -> Result<TurnOutcome> {
    anyhow::ensure!(
        req.images.len() <= crate::attachments::MAX_IMAGES,
        "at most four images per message"
    );
    let mut content = vec![json!({"type":"text","text":plan.prompt})];
    for image in req.images {
        content.push(image.acp().map_err(anyhow::Error::msg)?);
    }
    let mut command = Command::new(&plan.command);
    crate::process_ui::background(&mut command);
    command
        .arg("acp")
        .envs(plan.env.clone())
        .current_dir(&plan.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().context("could not start Cursor ACP")?;
    let handle = Arc::new(RunningTurn::new());
    handle.pid.store(child.id().unwrap_or(0), Ordering::SeqCst);
    let input = child.stdin.take().context("Cursor ACP stdin unavailable")?;
    let output = BufReader::new(
        child
            .stdout
            .take()
            .context("Cursor ACP stdout unavailable")?,
    );
    let stderr = child
        .stderr
        .take()
        .context("Cursor ACP stderr unavailable")?;
    let mut stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
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
    let mut conn = Connection {
        input,
        output,
        handle: handle.clone(),
        progress,
        out: TurnOutcome::default(),
        bytes: 0,
        session_id: req.native_session_id.map(str::to_owned),
        interactions: InteractionConnection::new(owner, handle.cancel_flag.clone()),
        prompting: false,
    };
    let result=async {
        let init=tokio::time::timeout(Duration::from_secs(12),conn.rpc(1,"initialize",json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"concordia","version":crate::version::VERSION}}))).await??;
        conn.interactions.initialized_cursor(&init);
        anyhow::ensure!(init["protocolVersion"] == 1, "Installed Cursor ACP did not negotiate protocol version 1");
        anyhow::ensure!(req.images.is_empty() || init.pointer("/agentCapabilities/promptCapabilities/image").and_then(Value::as_bool)==Some(true),"Installed Cursor ACP does not advertise image input. Images are retained; update the native CLI or select a compatible provider.");
        let (method,params)=if let Some(id)=req.native_session_id{
            anyhow::ensure!(init.pointer("/agentCapabilities/loadSession").and_then(Value::as_bool)==Some(true),"Cursor ACP cannot load this exact session; start a new conversation explicitly");
            ("session/load",json!({"sessionId":id,"cwd":plan.cwd,"mcpServers":[]}))
        }else{("session/new",json!({"cwd":plan.cwd,"mcpServers":[]}))};
        let session=tokio::time::timeout(Duration::from_secs(12),conn.rpc(2,method,params)).await??;
        anyhow::ensure!(!session.get("sessionId").is_some_and(|id| req.native_session_id.is_some_and(|expected| id.as_str()!=Some(expected))), "Cursor ACP loaded a different native session; prompt withheld");
        let sid=req.native_session_id.map(str::to_owned).or_else(||session["sessionId"].as_str().map(str::to_owned)).context("Cursor ACP returned no session id")?;
        conn.out.session_id=Some(sid.clone());conn.session_id=Some(sid.clone());conn.emit(TurnProgress::Session(sid.clone()));
        // Honor the final plan after locked-code enforcement, rather than the requested trust.
        let mode=if plan.argv.windows(2).any(|v|v==["--mode","plan"]){"plan"}else{"agent"};
        tokio::time::timeout(Duration::from_secs(12),conn.rpc(3,"session/set_mode",json!({"sessionId":sid,"modeId":mode}))).await??;
        if let Some(model)=req.model.filter(|m|!m.is_empty()&&*m!="auto"){
            tokio::time::timeout(Duration::from_secs(12),conn.rpc(4,"session/set_model",json!({"sessionId":sid,"modelId":model}))).await??;
        }
        conn.prompting = true;
        let result=conn.rpc(5,"session/prompt",json!({"sessionId":sid,"prompt":content})).await?;
        if result["stopReason"]=="cancelled"{conn.out.cancelled=true;}
        Ok::<(),anyhow::Error>(())
    }.await;
    conn.out.cancelled |= handle.cancel_flag.load(Ordering::SeqCst);
    if let Err(error) = result {
        if !conn.out.cancelled {
            let message = format!("{error:#}");
            conn.emit(TurnProgress::Error(message.clone()));
            conn.out.stderr_tail = message;
            conn.out.failed = true;
        }
    }
    kill_process_tree(handle.pid.load(Ordering::SeqCst));
    let mut reaped = false;
    if let Some(mut child) = handle.child.lock().await.take() {
        let _ = child.kill().await;
        reaped = tokio::time::timeout(REAP_TIMEOUT, child.wait())
            .await
            .is_ok_and(|r| r.is_ok());
    }
    if let Ok(Ok(tail)) = tokio::time::timeout(STDERR_JOIN_TIMEOUT, &mut stderr_task).await {
        if !tail.is_empty() {
            if !conn.out.stderr_tail.is_empty() {
                conn.out.stderr_tail.push('\n');
            }
            conn.out.stderr_tail.push_str(&tail);
        }
    } else {
        stderr_task.abort();
    }
    conn.out.exit_code = reaped.then_some(if conn.out.failed { 1 } else { 0 });
    Ok(conn.out)
}
