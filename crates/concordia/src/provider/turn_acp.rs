//! Native Cursor image inputs over ACP with explicit sessions and real permission responses.
use super::*;
use crate::provider::interaction::InteractionConnection;
use crate::provider::CardStatus;
use serde_json::{json, Value};

struct QuestionListener(tokio::task::JoinHandle<()>);

impl Drop for QuestionListener {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct Connection {
    input: tokio::process::ChildStdin,
    output: BufReader<tokio::process::ChildStdout>,
    handle: Arc<RunningTurn>,
    progress: Option<mpsc::Sender<TurnProgress>>,
    out: TurnOutcome,
    session_id: Option<String>,
    interactions: InteractionConnection,
    prompting: bool,
    bridge_rx: tokio::sync::mpsc::UnboundedReceiver<crate::ask_bridge::BridgeRequest>,
}
impl Connection {
    async fn emit(&self, event: TurnProgress) {
        let _ = send_progress(&self.progress, event, &self.handle).await;
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
            anyhow::ensure!(!self.handle.cancel_flag.load(Ordering::SeqCst), "cancelled");
            let mut bytes = Vec::new();
            let next = tokio::select! {
                _=self.handle.wake.notified()=>anyhow::bail!("cancelled"),
                Some(request)=self.bridge_rx.recv()=>{
                    self.bridge_request(request).await?;
                    continue;
                }
                next=read_line_bounded(&mut self.output,&mut bytes)=>next,
            };
            let received = next?;
            anyhow::ensure!(received > 0, "Cursor ACP closed before replying");
            anyhow::ensure!(received <= LINE_MAX, "Cursor ACP response line too large");
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            if value.get("method").is_some() {
                if value.get("id").is_some() {
                    self.request(&value).await?;
                } else {
                    self.update(&value).await;
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
    async fn bridge_request(&mut self, request: crate::ask_bridge::BridgeRequest) -> Result<()> {
        let Some(session) = self.session_id.clone() else {
            let _ = request.answer.send(json!(crate::ask_bridge::DISMISSED));
            return Ok(());
        };
        let session = session.as_str();
        let bridge_id = format!("concordia-ask-user-{}", uuid::Uuid::new_v4());
        let Some(questions) = request.questions.as_array() else {
            let _ = request.answer.send(json!(crate::ask_bridge::DISMISSED));
            return Ok(());
        };
        let native = json!({"sessionId":session,"toolCallId":bridge_id,"prompt":"The agent needs a decision","questions":questions.iter().enumerate().map(|(i,q)| json!({"id":format!("question-{i}"),"prompt":q["question"],"options":q["options"].as_array().unwrap_or(&Vec::new()).iter().enumerate().map(|(j,o)| json!({"id":format!("option-{i}-{j}"),"label":o})).collect::<Vec<_>>(),"allowMultiple":q["multiple"].as_bool().unwrap_or(false)})).collect::<Vec<_>>() });
        let (native_request, rx) = self
            .interactions
            .request(
                session,
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
            ))
            .await;
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
                Ok(Some(request)) => request,
                Ok(None) => return Ok(()),
                Err(reason) => {
                    self.emit(TurnProgress::Notice(format!(
                        "Unsupported native request: {reason}"
                    )))
                    .await;
                    return self.write(json!({"jsonrpc":"2.0","id":value["id"],"result":{"outcome":{"outcome":"cancelled"}}})).await;
                }
            };
            if !send_progress(
                &self.progress,
                TurnProgress::Interaction(Box::new(request.clone())),
                &self.handle,
            )
            .await
            {
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
            ))
            .await;
            self.write(json!({"jsonrpc":"2.0","id":value["id"],"error":{"code":-32601,"message":"Client capability is not supported"}})).await
        }
    }
    async fn update(&mut self, value: &Value) {
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
                    append_visible_text(&mut self.out.text, text);
                    self.emit(TurnProgress::TextDelta(text.into())).await;
                }
            }
            "agent_thought_chunk" => {
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                    self.emit(TurnProgress::ThinkingDelta(text.into())).await;
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
                let mut card = StreamCard {
                    id,
                    title,
                    detail,
                    status,
                };
                if let Some(content) = update.get("content").or_else(|| update.get("rawInput")) {
                    crate::provider::edit_display::normalize_acp(&mut card, content);
                }
                crate::provider::stream_cards::upsert_card(&mut self.out.cards, card.clone());
                self.emit(TurnProgress::Card(card)).await;
            }
            _ => {}
        }
    }
}
/// Cursor keeps print-mode chats (`create-chat`, `--resume`) apart from ACP sessions, so a chat
/// started before ACP became the default cannot be loaded here. The caller continues it in print
/// mode; nothing was prompted and nothing was shown.
pub(super) const SESSION_NOT_IN_ACP: &str = "Cursor ACP has no session with this id";

fn ask_server_config(endpoint: &str, token: &str, enabled: bool) -> Value {
    if !enabled || endpoint.is_empty() {
        return json!([]);
    }
    json!([{"name":"concordia-ask","command":std::env::current_exe().unwrap_or_default().to_string_lossy(),"args":["ask-mcp"],"env":[{"name":"CONCORDIA_ASK_ENDPOINT","value":endpoint},{"name":"CONCORDIA_ASK_TOKEN","value":token}]}])
}

/// `agent models` lists flat variants (`claude-opus-5-5-high-fast`, `cursor-grok-4.6-low`), while
/// ACP sessions offer one parameterized entry per base (`claude-opus-5-5[effort=medium,fast=false]`).
/// Map a flat id onto the session's entry for its base, changing only effort, fast and thinking.
fn acp_model_id(id: &str, available: &[String]) -> Option<String> {
    if id.contains('[') || available.iter().any(|m| m == id) {
        return Some(id.to_owned());
    }
    let flat = id.strip_prefix("cursor-").unwrap_or(id);
    let (base, params, rest) = available
        .iter()
        .filter_map(|m| {
            let (base, params) = m.split_once('[')?;
            let rest = flat.strip_prefix(base)?;
            (rest.is_empty() || rest.starts_with('-'))
                .then(|| (base, params.trim_end_matches(']'), rest))
        })
        .max_by_key(|(base, _, _)| base.len())?;
    let tokens: Vec<&str> = rest.split('-').filter(|t| !t.is_empty()).collect();
    let effort = tokens
        .iter()
        .find(|t| matches!(**t, "low" | "medium" | "high" | "xhigh" | "max"));
    let params: Vec<String> = params
        .split(',')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (key, value) = p.split_once('=').unwrap_or((p, ""));
            let value = match key {
                "fast" => tokens.contains(&"fast").to_string(),
                "thinking" => tokens.contains(&"thinking").to_string(),
                "effort" | "reasoning_effort" | "reasoning" => {
                    effort.map_or(value.to_owned(), |e| (*e).to_owned())
                }
                _ => value.to_owned(),
            };
            format!("{key}={value}")
        })
        .collect();
    Some(format!("{base}[{}]", params.join(",")))
}

/// Trust flags are global options and must precede the `acp` subcommand. Plan mode is set per
/// session over the protocol instead.
fn acp_argv(print_argv: &[String]) -> Vec<String> {
    let mut argv = Vec::new();
    if planned_options(print_argv).any(|(flag, _)| flag == "--force") {
        argv.push("--force".into());
    }
    if let Some((flag, Some(value))) =
        planned_options(print_argv).find(|(flag, _)| *flag == "--sandbox")
    {
        argv.extend([flag.into(), value.into()]);
    }
    argv.push("acp".into());
    argv
}

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
        "at most four images per message"
    );
    let mut content = vec![json!({"type":"text","text":plan.prompt})];
    for image in req.images {
        content.push(image.acp().map_err(anyhow::Error::msg)?);
    }
    let token = uuid::Uuid::new_v4().to_string();
    let bridge_token = token.clone();
    let (bridge_tx, bridge_rx) = tokio::sync::mpsc::unbounded_channel();
    let (endpoint, bridge_task) =
        crate::ask_bridge::spawn_listener(token.clone(), bridge_tx).await?;
    let _question_listener = QuestionListener(bridge_task);
    let mut env = plan.env.clone();
    if !endpoint.is_empty() {
        env.push(("CONCORDIA_ASK_ENDPOINT".into(), endpoint.clone()));
        env.push(("CONCORDIA_ASK_TOKEN".into(), token));
    }
    let launch = crate::provider::launch::prepare(&PlannedTurn {
        argv: acp_argv(&plan.argv),
        prompt: "acp".into(),
        ..plan.clone()
    })?;
    let mut command = crate::process_ui::tokio_command(&launch.program);
    crate::process_ui::background(&mut command);
    #[cfg(unix)]
    command.process_group(0);
    for key in &plan.env_remove {
        command.env_remove(key);
    }
    command
        .args(&launch.args)
        .envs(env)
        .envs(launch.env)
        .current_dir(&plan.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if handle.cancel_flag.load(Ordering::SeqCst) {
        return Ok(TurnOutcome {
            cancelled: true,
            ..Default::default()
        });
    }
    let mut child = command.spawn().context("could not start Cursor ACP")?;
    let _process_tree =
        crate::process_lifetime::own_tokio_process(&child).map_err(anyhow::Error::msg)?;
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
    let mut stderr_task = tokio::spawn(read_stderr_tail(stderr, || {}));
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
        session_id: req.native_session_id.map(str::to_owned),
        interactions: InteractionConnection::new(owner, handle.cancel_flag.clone()),
        prompting: false,
        bridge_rx,
    };
    let result=async {
        let init=tokio::time::timeout(Duration::from_secs(12),conn.rpc(1,"initialize",json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"concordia","version":crate::version::VERSION}}))).await??;
        conn.interactions.initialized_cursor(&init);
        anyhow::ensure!(init["protocolVersion"] == 1, "Installed Cursor ACP did not negotiate protocol version 1");
        anyhow::ensure!(req.images.is_empty() || init.pointer("/agentCapabilities/promptCapabilities/image").and_then(Value::as_bool)==Some(true),"Installed Cursor ACP does not advertise image input. Images are retained; update the native CLI or select a compatible provider.");
        let mcp_servers = ask_server_config(&endpoint, &bridge_token, !endpoint.is_empty());
        let (method,params)=if let Some(id)=req.native_session_id{
            anyhow::ensure!(init.pointer("/agentCapabilities/loadSession").and_then(Value::as_bool)==Some(true),"Cursor ACP cannot load this exact session; start a new conversation explicitly");
            ("session/load",json!({"sessionId":id,"cwd":plan.cwd,"mcpServers":mcp_servers}))
        }else{("session/new",json!({"cwd":plan.cwd,"mcpServers":mcp_servers}))};
        let session=match tokio::time::timeout(Duration::from_secs(12),conn.rpc(2,method,params)).await?{
            Err(error) if req.native_session_id.is_some() && format!("{error:#}").contains("not found") => anyhow::bail!(SESSION_NOT_IN_ACP),
            other => other?,
        };
        anyhow::ensure!(!session.get("sessionId").is_some_and(|id| req.native_session_id.is_some_and(|expected| id.as_str()!=Some(expected))), "Cursor ACP loaded a different native session; prompt withheld");
        let sid=req.native_session_id.map(str::to_owned).or_else(||session["sessionId"].as_str().map(str::to_owned)).context("Cursor ACP returned no session id")?;
        conn.out.session_id=Some(sid.clone());conn.session_id=Some(sid.clone());conn.emit(TurnProgress::Session(sid.clone())).await;
        // Honor the final plan after locked-code enforcement, rather than the requested trust.
        let mode=if planned_options(&plan.argv).any(|(flag,value)|flag=="--mode"&&value==Some("plan")){"plan"}else{"agent"};
        tokio::time::timeout(Duration::from_secs(12),conn.rpc(3,"session/set_mode",json!({"sessionId":sid,"modeId":mode}))).await??;
        if let Some(model)=req.model.filter(|m|!m.is_empty()&&*m!="auto"){
            let available:Vec<String>=session.pointer("/models/availableModels").and_then(Value::as_array).into_iter().flatten()
                .filter_map(|m|m["modelId"].as_str().map(str::to_owned)).collect();
            let model=acp_model_id(model,&available).unwrap_or_else(||model.to_owned());
            tokio::time::timeout(Duration::from_secs(12),conn.rpc(4,"session/set_model",json!({"sessionId":sid,"modelId":model}))).await??;
        }
        conn.prompting = true;
        let result=conn.rpc(5,"session/prompt",json!({"sessionId":sid,"prompt":content})).await?;
        if result["stopReason"]=="cancelled"{conn.out.cancelled=true;}
        Ok::<(),anyhow::Error>(())
    }.await;
    conn.out.cancelled |= handle.cancel_flag.load(Ordering::SeqCst);
    if let Err(error) = result {
        let message = format!("{error:#}");
        if message == SESSION_NOT_IN_ACP {
            conn.out.stderr_tail = message;
            conn.out.failed = true;
        } else if !conn.out.cancelled {
            conn.emit(TurnProgress::Error(message.clone())).await;
            conn.out.stderr_tail = message;
            conn.out.failed = true;
        }
    }
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

#[cfg(test)]
mod tests {
    use super::{acp_argv, acp_model_id, ask_server_config};

    #[test]
    fn question_server_is_registered_only_when_enabled() {
        assert!(ask_server_config("", "token", false)
            .as_array()
            .unwrap()
            .is_empty());
        let enabled = ask_server_config("127.0.0.1:1234", "token", true);
        assert_eq!(enabled[0]["name"], "concordia-ask");
        assert_eq!(enabled[0]["env"][1]["value"], "token");
    }

    #[test]
    fn flat_cli_model_ids_map_onto_the_sessions_parameterized_variants() {
        let available: Vec<String> = [
            "default[]",
            "grok-4.7[context=256k,reasoning_effort=high,fast=true]",
            "grok-4.6[effort=high,fast=true]",
            "composer-2.5[fast=true]",
            "claude-opus-5-5[context=300k,effort=medium,fast=false]",
            "claude-opus-5[thinking=true,context=300k,effort=high,fast=false]",
            "gpt-5.6-sol[context=272k,reasoning=medium,fast=false]",
        ]
        .map(String::from)
        .into();
        let map = |id| acp_model_id(id, &available).unwrap();
        assert_eq!(
            map("claude-opus-5-5-high-fast"),
            "claude-opus-5-5[context=300k,effort=high,fast=true]"
        );
        assert_eq!(
            map("claude-opus-5-5-medium"),
            "claude-opus-5-5[context=300k,effort=medium,fast=false]"
        );
        assert_eq!(
            map("claude-opus-5-high"),
            "claude-opus-5[thinking=false,context=300k,effort=high,fast=false]"
        );
        assert_eq!(
            map("claude-opus-5-thinking-max"),
            "claude-opus-5[thinking=true,context=300k,effort=max,fast=false]"
        );
        assert_eq!(
            map("cursor-grok-4.6-low"),
            "grok-4.6[effort=low,fast=false]"
        );
        assert_eq!(
            map("grok-4.7-xhigh-fast"),
            "grok-4.7[context=256k,reasoning_effort=xhigh,fast=true]"
        );
        assert_eq!(map("composer-2.5-fast"), "composer-2.5[fast=true]");
        assert_eq!(
            map("gpt-5.6-sol-high"),
            "gpt-5.6-sol[context=272k,reasoning=high,fast=false]"
        );
        assert_eq!(map("grok-4.6[effort=low]"), "grok-4.6[effort=low]");
        assert_eq!(acp_model_id("unknown-model", &available), None);
    }

    #[test]
    fn trust_flags_precede_the_acp_subcommand() {
        let print = |args: &[&str]| args.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(
            acp_argv(&print(&["--print", "--force", "hi"])),
            ["--force", "acp"]
        );
        assert_eq!(
            acp_argv(&print(&["--print", "--mode", "plan", "hi"])),
            ["acp"]
        );
    }
}
