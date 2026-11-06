//! Opt-in native Codex stdio transport, matched to the 0.166.0 generated protocol.
use super::*;
use crate::provider::{interaction::InteractionConnection, CardStatus};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const TOOL_MAP_MAX: usize = 4096;

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
    progress: Option<mpsc::UnboundedSender<TurnProgress>>,
    interactions: InteractionConnection,
    out: TurnOutcome,
    thread: Option<String>,
    turn: Option<String>,
    pending: HashMap<String, NativeRequest>,
    answers: mpsc::UnboundedReceiver<(Value, Option<Value>)>,
    answer_tx: mpsc::UnboundedSender<(Value, Option<Value>)>,
    waiters: tokio::task::JoinSet<()>,
    deltas: HashSet<String>,
    dropped_deltas: usize,
    buffered: Vec<Value>,
    artifacts: crate::artifacts::Collector,
}
impl Connection {
    fn emit(&self, event: TurnProgress) {
        if let Some(tx) = &self.progress {
            let _ = tx.send(event);
        }
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
                // No approvals during initialize/history loading. Events from turn/start may
                // precede its response, but cannot become actionable before its exact turn ID.
                if method == "turn/start" {
                    buffered_bytes += value.to_string().len();
                    anyhow::ensure!(
                        self.buffered.len() < 256 && buffered_bytes <= 2 * 1024 * 1024,
                        "Too many early native events"
                    );
                    self.buffered.push(value);
                } else if value.get("id").is_some() {
                    self.reject(&value["id"], "No active verified turn").await?;
                }
                continue;
            }
            if value["id"] == id {
                anyhow::ensure!(
                    value.get("error").is_none(),
                    "Codex app-server rejected {method}: {}",
                    value["error"]["message"]
                        .as_str()
                        .unwrap_or("native request failed")
                        .chars()
                        .take(1000)
                        .collect::<String>()
                );
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
    fn review_finished(&mut self, request: &NativeRequest, title: &str) {
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
        self.emit(TurnProgress::Card(card));
    }
    async fn event(&mut self, value: Value) -> Result<bool> {
        let method = value["method"].as_str().unwrap_or("");
        let params = &value["params"];
        let same_thread = params["threadId"].as_str() == self.thread.as_deref();
        let same_turn = params["turnId"].as_str() == self.turn.as_deref();
        if value.get("id").is_some() {
            if !same_thread || !same_turn || self.pending.len() >= 8 {
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
                )));
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
                Ok(request) => request,
                Err(reason) => {
                    self.emit(TurnProgress::Notice(format!(
                        "Native request unavailable: {reason}"
                    )));
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
            if !self.progress.as_ref().is_some_and(|tx| {
                tx.send(TurnProgress::Interaction(Box::new(request.clone())))
                    .is_ok()
            }) {
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
                self.review_finished(&request, "Native request cleared by provider");
            }
            return Ok(false);
        }
        if method == "turn/completed" {
            if params["turn"]["id"].as_str() != self.turn.as_deref() {
                return Ok(false);
            }
            for (_, request) in std::mem::take(&mut self.pending) {
                request.resolve_native();
                self.review_finished(&request, "Native request ended with turn");
            }
            match params["turn"]["status"].as_str() {
                Some("completed") => {}
                Some("interrupted") => self.out.cancelled = true,
                _ => {
                    self.out.failed = true;
                    let message = params["turn"]["error"]["message"]
                        .as_str()
                        .unwrap_or("Native Codex turn failed");
                    self.out.stderr_tail = message.chars().take(2000).collect();
                    self.emit(TurnProgress::Error(self.out.stderr_tail.clone()));
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
                    self.out.text.push_str(delta);
                    self.emit(TurnProgress::TextDelta(delta.into()));
                }
            }
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                if let Some(delta) = params["delta"].as_str() {
                    self.emit(TurnProgress::ThinkingDelta(delta.into()));
                }
            }
            "item/started" | "item/completed" => {
                self.item(&params["item"], method == "item/completed")
            }
            // Cumulative thread totals and the last model response are not full-turn totals.
            // Existing native-source accounting remains authoritative until a baseline is proven.
            _ => {}
        }
        Ok(false)
    }
    fn item(&mut self, item: &Value, completed: bool) {
        if completed {
            self.artifacts.report(
                self.interactions.owner(),
                self.thread.as_deref(),
                self.turn.as_deref(),
                item,
                &self.progress,
            );
        }
        let Some(id) = item["id"].as_str().filter(|id| !id.is_empty()) else {
            return;
        };
        let kind = item["type"].as_str().unwrap_or("");
        if kind == "agentMessage" {
            if completed && !self.deltas.contains(id) {
                if let Some(text) = item["text"].as_str() {
                    self.out.text.push_str(text);
                    self.emit(TurnProgress::TextDelta(text.into()));
                }
            }
            return;
        }
        if kind == "collabAgentToolCall" {
            let translated = json!({"receiver_thread_ids":item["receiverThreadIds"],"sender_thread_id":item["senderThreadId"],"agents_states":item["agentsStates"],"prompt":item["prompt"],"tool":item["tool"],"status":item["status"]});
            for part in crate::provider::stream_cards::classify_collab_tool_call(&translated) {
                if let StreamPart::Card(card) = part {
                    crate::provider::stream_cards::upsert_card(&mut self.out.cards, card.clone());
                    self.emit(TurnProgress::Card(card));
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
        self.emit(TurnProgress::Card(card));
    }
    async fn listen(&mut self) -> Result<()> {
        for value in std::mem::take(&mut self.buffered) {
            if self.event(value).await? {
                return Ok(());
            }
        }
        loop {
            tokio::select! {
                biased;
                _=self.handle.wake.notified()=>anyhow::bail!("cancelled"),
                next=self.output.recv()=> {if self.event(next.context("Codex app-server closed before turn completion; no fallback was started")??).await? {return Ok(());}},
                Some((id,answer))=self.answers.recv()=> {
                    if let Some(request)=self.pending.remove(&id.to_string()) {
                        if request.was_resolved() {continue;}
                        if let Some(answer)=answer {
                            let decision=answer.get("decision").map(Value::to_string).unwrap_or_else(||"answers".into());
                            self.write(json!({"jsonrpc":"2.0","id":id,"result":answer})).await?;
                            self.review_finished(&request,&format!("Native decision sent: {decision}"));
                        } else { self.reject(&id,"Native input was closed without an answer").await?; }
                    }
                }
            }
            while self.waiters.try_join_next().is_some() {}
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
    anyhow::ensure!(version.trim()=="codex-cli 0.166.0","Native Codex input supports CLI 0.166.0; installed version is unverified. No fallback was started");
    let read_only = plan.argv.windows(2).any(|args| args == ["-s", "read-only"]);
    let full = plan
        .argv
        .iter()
        .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox");
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
            plan.argv.iter().any(|arg| arg == "--approve-for-me"),
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
    let mut command = crate::process_ui::tokio_command(&program);
    crate::process_ui::background(&mut command);
    if let Some((_, script)) = bridge {
        command.arg(script);
    }
    command.arg("app-server");
    // Carry the process-local compaction overrides from the final enforcement plan.
    for args in plan.argv.windows(2).filter(|args| args[0] == "-c") {
        command.args(args);
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
    let mut child = command
        .spawn()
        .context("Could not start Codex app-server")?;
    let handle = Arc::new(RunningTurn::new());
    handle.pid.store(child.id().unwrap_or(0), Ordering::SeqCst);
    let input = child.stdin.take().context("Codex stdin unavailable")?;
    let stdout = child.stdout.take().context("Codex stdout unavailable")?;
    let (output_tx, output) = mpsc::channel(16);
    let output_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut total = 0usize;
        loop {
            let mut bytes = Vec::new();
            let next = async {
                let n = read_line_bounded(&mut reader, &mut bytes).await?;
                anyhow::ensure!(
                    n > 0,
                    "Codex app-server closed before turn completion; no fallback was started"
                );
                total = total.saturating_add(n);
                anyhow::ensure!(
                    n <= LINE_MAX && total <= 16 * 1024 * 1024,
                    "Codex app-server output bound exceeded"
                );
                serde_json::from_slice::<Value>(&bytes).context("Invalid Codex app-server JSON")
            }
            .await;
            let failed = next.is_err();
            if output_tx.send(next).await.is_err() || failed {
                break;
            }
        }
    });
    let stderr = child.stderr.take().context("Codex stderr unavailable")?;
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
    let (answer_tx, answers) = mpsc::unbounded_channel();
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
        dropped_deltas: 0,
        buffered: Vec::new(),
        artifacts: crate::artifacts::Collector::default(),
    };
    let result=async {
        let init=tokio::time::timeout(Duration::from_secs(12),conn.rpc(1,"initialize",json!({"clientInfo":{"name":"concordia","title":"Concordia","version":crate::version::VERSION},"capabilities":{"experimentalApi":true}}))).await??;
        anyhow::ensure!(init["codexHome"].as_str().and_then(|path|Path::new(path).canonicalize().ok()).as_ref()==Some(&home),"Codex app-server account home mismatch; prompt withheld");
        conn.interactions.initialized_codex("0.166.0");
        conn.write(json!({"jsonrpc":"2.0","method":"initialized","params":{}})).await?;
        let mut params=json!({"cwd":plan.cwd,"sandbox":sandbox,"approvalPolicy":approval,"approvalsReviewer":"user"});
        if let Some(model)=req.model {params["model"]=json!(model);}
        let method=if let Some(id)=req.native_session_id {params["threadId"]=json!(id);params["excludeTurns"]=json!(true);"thread/resume"} else {"thread/start"};
        let thread=tokio::time::timeout(Duration::from_secs(12),conn.rpc(2,method,params)).await??;
        let id=thread["thread"]["id"].as_str().filter(|id|!id.is_empty()&&id.len()<=4096).context("Native thread ID missing")?;
        anyhow::ensure!(req.native_session_id.is_none_or(|expected|expected==id),"Codex resumed a different thread; prompt withheld");
        anyhow::ensure!(thread["cwd"].as_str().and_then(|path|Path::new(path).canonicalize().ok())==Some(Path::new(&plan.cwd).canonicalize()?),"Codex thread workspace mismatch; prompt withheld");
        anyhow::ensure!(thread["approvalPolicy"]==approval && thread["approvalsReviewer"]=="user","Codex did not honor the requested approval policy; prompt withheld");
        conn.thread=Some(id.into());conn.out.session_id=Some(id.into());conn.emit(TurnProgress::Session(id.into()));
        let mut params=json!({"threadId":id,"input":content,"cwd":plan.cwd,"approvalPolicy":approval,"approvalsReviewer":"user","sandboxPolicy":policy});
        if let Some(model)=req.model {params["model"]=json!(model);}
        let turn=tokio::time::timeout(Duration::from_secs(12),conn.rpc(3,"turn/start",params)).await??;
        conn.turn=Some(turn["turn"]["id"].as_str().filter(|id|!id.is_empty()&&id.len()<=4096).context("Native turn ID missing")?.into());
        conn.listen().await
    }.await;
    conn.out.cancelled |= handle.cancel_flag.load(Ordering::SeqCst);
    if let Err(error) = result {
        if !conn.out.cancelled {
            conn.out.failed = true;
            conn.out.stderr_tail = format!("{error:#}");
            conn.emit(TurnProgress::Error(conn.out.stderr_tail.clone()));
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
        let _ = child.kill().await;
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
