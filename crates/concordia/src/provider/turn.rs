//! Unified provider turns with tracked cancel (Ctrl+C).

use crate::provider::claude::{plan_claude_turn, ClaudeTurnRequest};
use crate::provider::codex::{
    self, apply_stream_parts, extract_stream_chunk, plan_turn_argv, TurnRequest as CodexReq,
};
use crate::provider::cursor::{plan_cursor_turn, CursorTurnRequest};
use crate::provider::enforcement;
use crate::provider::launch::prepare;
use crate::provider::stream_cards::{parse_codex_stream_chunk, StreamCard, StreamPart, TurnUsage};
use crate::provider::stream_json::{Flavor, StreamParser};
use crate::store::{ProviderKind, TrustMode};
use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex, Notify};

/// Lines of provider stderr kept for error reporting.
const STDERR_KEEP_LINES: usize = 12;
/// How long to wait for the provider to exit after it has been killed.
const REAP_TIMEOUT: Duration = Duration::from_secs(5);
/// After a cancel, how long taskkill gets to finish before we terminate the direct child.
const KILL_GRACE: Duration = Duration::from_millis(2000);
const STDERR_JOIN_TIMEOUT: Duration = Duration::from_millis(500);
/// A single provider output line is kept up to this size; the rest of it is discarded.
const LINE_MAX: usize = 1024 * 1024;
/// Raw stream kept for the Codex fallback parse.
const RAW_MAX: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ProviderTurnRequest<'a> {
    pub provider: ProviderKind,
    pub prompt: &'a str,
    pub cwd: &'a Path,
    pub home_path: Option<&'a Path>,
    pub model: Option<&'a str>,
    pub native_session_id: Option<&'a str>,
    pub trust: TrustMode,
    pub context_prefix: Option<&'a str>,
    /// When set, the prompt opens with this time and the relative times it mentions resolved
    /// (`prompt_time::header`). Every user and worker turn sets it; `None` sends the text as is.
    pub local_time: Option<crate::prompt_time::LocalTime>,
}

#[derive(Debug, Clone)]
pub struct PlannedTurn {
    pub command: String,
    pub argv: Vec<String>,
    /// The full prompt (context prefix included); always the last entry of `argv`.
    pub prompt: String,
    /// Session id this turn creates (Claude `--session-id`, Cursor `create-chat`); promoted to the
    /// conversation's session once the turn succeeds.
    pub new_session_id: Option<String>,
    pub env: Vec<(String, String)>,
    pub cwd: String,
    pub provider: ProviderKind,
}

#[derive(Debug, Default, Clone)]
pub struct TurnOutcome {
    /// A terminal failure event, independent of the process exit code.
    pub failed: bool,
    pub exit_code: Option<i32>,
    pub text: String,
    pub session_id: Option<String>,
    pub cancelled: bool,
    pub cards: Vec<StreamCard>,
    /// Last lines the provider wrote to stderr (why it failed, when it did).
    pub stderr_tail: String,
}

#[derive(Debug, Clone)]
pub enum TurnProgress {
    Session(String),
    Text(String),
    TextDelta(String),
    Card(StreamCard),
    Error(String),
    Thinking(String),
    ThinkingDelta(String),
    /// Token counts the provider reported for the turn (never reply text).
    Usage(TurnUsage),
    /// Concordia's own explanation of how it changed the turn (for example, read-only enforcement).
    Notice(String),
}

/// Why this turn will run read-only instead of with the trust it asked for, if it will.
pub fn enforcement_notice(req: &ProviderTurnRequest<'_>) -> Option<String> {
    enforcement::effective_trust(req.cwd, req.provider, req.home_path, req.trust).1
}

pub fn plan_provider_turn(req: &ProviderTurnRequest<'_>) -> Result<PlannedTurn> {
    // Locked-code projects without verified enforcement never get a write-capable turn. This runs
    // before any provider process (including Cursor's `create-chat`) starts.
    let (trust, lowered) =
        enforcement::effective_trust(req.cwd, req.provider, req.home_path, req.trust);
    let req = &ProviderTurnRequest {
        trust,
        ..req.clone()
    };
    let mut plan = plan_provider_turn_raw(req)?;
    // A CLI that lacks a flag (found by `provider::compat`) gets the older, plainer argv.
    crate::provider::compat::degrade_planned(&mut plan, req.trust);
    // That fallback can drop the plan switch (Claude to manual, Cursor without --mode plan).
    // A turn the gate made read-only must still be read-only, or it does not start.
    if lowered.is_some() {
        enforcement::ensure_read_only(plan.provider, &plan.argv)?;
    }
    Ok(plan)
}

fn plan_provider_turn_raw(req: &ProviderTurnRequest<'_>) -> Result<PlannedTurn> {
    let mut prompt = match req.context_prefix {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}\n{}", req.prompt),
        _ => req.prompt.to_string(),
    };
    if let Some(now) = &req.local_time {
        // Relative times are read from what the user wrote, not from the context pack.
        prompt = format!("{}\n{prompt}", crate::prompt_time::header(req.prompt, now));
    }
    match req.provider {
        ProviderKind::Codex => {
            let plan = plan_turn_argv(&CodexReq {
                prompt: &prompt,
                cwd: req.cwd,
                codex_home: req.home_path,
                model: req.model,
                native_session_id: req.native_session_id,
                trust: req.trust,
            });
            codex::assert_safe_argv(&plan.argv)?;
            let mut env = Vec::new();
            if let Some(home) = req.home_path {
                env.push(("CODEX_HOME".into(), home.to_string_lossy().into_owned()));
            }
            Ok(PlannedTurn {
                command: plan.command,
                argv: plan.argv,
                env,
                cwd: plan.cwd,
                prompt: prompt.clone(),
                new_session_id: None,
                provider: ProviderKind::Codex,
            })
        }
        ProviderKind::Claude => {
            let plan = plan_claude_turn(&ClaudeTurnRequest {
                prompt: &prompt,
                cwd: req.cwd,
                config_dir: req.home_path,
                model: req.model,
                native_session_id: req.native_session_id,
                trust: req.trust,
            })?;
            Ok(PlannedTurn {
                command: plan.command,
                argv: plan.argv,
                env: plan.env,
                cwd: plan.cwd,
                prompt: prompt.clone(),
                new_session_id: plan.new_session_id,
                provider: ProviderKind::Claude,
            })
        }
        ProviderKind::Cursor => {
            let plan = plan_cursor_turn(&CursorTurnRequest {
                prompt: &prompt,
                cwd: req.cwd,
                home_path: req.home_path,
                model: req.model,
                native_session_id: req.native_session_id,
                trust: req.trust,
            })?;
            Ok(PlannedTurn {
                command: plan.command,
                argv: plan.argv,
                env: plan.env,
                cwd: plan.cwd,
                prompt: prompt.clone(),
                new_session_id: None,
                provider: ProviderKind::Cursor,
            })
        }
    }
}

pub struct RunningTurn {
    pub cancel_flag: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    /// Pid of the spawned process (0 until spawn); used to kill the whole tree.
    pid: Arc<AtomicU32>,
    wake: Arc<Notify>,
}

impl RunningTurn {
    fn new() -> Self {
        RunningTurn {
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child: Arc::new(Mutex::new(None)),
            pid: Arc::new(AtomicU32::new(0)),
            wake: Arc::new(Notify::new()),
        }
    }

    /// Cancel the turn: mark it, wake the reader, and kill the provider **and everything it
    /// started**. On Windows the CLI is usually a `.cmd` shim in front of node, so killing only
    /// the direct child would leave the real agent running (and spending tokens).
    pub fn cancel(&self) {
        if self.cancel_flag.swap(true, Ordering::SeqCst) {
            return;
        }
        self.wake.notify_one();
        let pid = self.pid.load(Ordering::SeqCst);
        if cfg!(windows) && pid != 0 {
            // taskkill walks the tree from the *live* parent. Terminating the shim ourselves
            // first would orphan its children before taskkill can find them, so the direct kill
            // is only a fallback, applied by the runner after a grace period.
            kill_process_tree(pid);
        } else if let Ok(mut guard) = self.child.try_lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.start_kill();
            }
        }
    }
}

/// Kill `pid` and all its descendants. Fire-and-forget; no-op for pid 0.
pub(crate) fn kill_process_tree(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    #[cfg(not(windows))]
    {
        // Direct child only; Linux/macOS are not a supported target yet.
        let _ = pid;
    }
}

/// Run a provider turn. If `on_started` is Some, sends the cancel handle as soon as the child spawns.
/// If `on_progress` is Some, streams Codex text/cards as they arrive.
pub async fn run_provider_turn(
    req: &ProviderTurnRequest<'_>,
    on_started: Option<mpsc::UnboundedSender<Arc<RunningTurn>>>,
) -> Result<TurnOutcome> {
    run_provider_turn_with_progress(req, on_started, None).await
}

pub async fn run_provider_turn_with_progress(
    req: &ProviderTurnRequest<'_>,
    on_started: Option<mpsc::UnboundedSender<Arc<RunningTurn>>>,
    on_progress: Option<mpsc::UnboundedSender<TurnProgress>>,
) -> Result<TurnOutcome> {
    let mut session_guard = req
        .native_session_id
        .map(|id| {
            super::session_guard::acquire(&crate::store::default_store_root(), req.home_path, id)
        })
        .transpose()?;
    let mut plan = plan_provider_turn(req)?;
    if let (Some(tx), Some(notice)) = (&on_progress, enforcement_notice(req)) {
        let _ = tx.send(TurnProgress::Notice(notice));
    }
    // Cursor has no way to name a session up front: ask the CLI for a chat id first, then resume
    // it explicitly. If that does not work the turn runs unbound, exactly as before.
    if plan.provider == ProviderKind::Cursor
        && req.native_session_id.is_none_or(|s| s.trim().is_empty())
    {
        if let Some(id) =
            crate::provider::cursor::create_chat(&plan.command, &plan.env, &plan.cwd).await
        {
            let at = plan.argv.len() - 1; // the prompt stays last
            plan.argv.insert(at, id.clone());
            plan.argv.insert(at, "--resume".into());
            plan.new_session_id = Some(id);
        }
    }
    let plain_output =
        super::compat::caps_for(plan.provider).is_some_and(|caps| caps.known && !caps.stream_json);
    let launch = prepare(&plan)?;
    let handle = Arc::new(RunningTurn::new());

    let mut cmd = Command::new(&launch.program);
    cmd.args(&launch.args)
        .current_dir(&plan.cwd)
        .stdin(if launch.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (k, v) in plan.env.iter().chain(launch.env.iter()) {
        cmd.env(k, v);
    }
    // Do not echo argv here: it contains the whole prompt.
    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawn {}", launch.program))?;
    handle.pid.store(child.id().unwrap_or(0), Ordering::SeqCst);
    // Counted in the footer stats (when on) until this turn ends.
    let _stats_guard = crate::sysstats::register(child.id().unwrap_or(0));
    let stdout = child.stdout.take().context("stdout")?;
    let stderr = child.stderr.take().context("stderr")?;
    if let (Some(prompt), Some(mut stdin)) = (launch.stdin.clone(), child.stdin.take()) {
        // Written off-thread so a provider that has not started reading cannot stall us.
        tokio::spawn(async move {
            let _ = stdin.write_all(prompt.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
    }
    // Always drain stderr: an unread pipe blocks a chatty child, and it is where failures go.
    let mut stderr_task = tokio::spawn(async move {
        let mut tail: VecDeque<String> = VecDeque::new();
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            if tail.len() == STDERR_KEEP_LINES {
                tail.pop_front();
            }
            tail.push_back(l);
        }
        tail.into_iter().collect::<Vec<_>>().join("\n")
    });
    {
        let mut slot = handle.child.lock().await;
        *slot = Some(child);
    }
    if let Some(tx) = on_started {
        if tx.send(handle.clone()).is_err() {
            // No caller can cancel or observe this child anymore. Stop it here so a
            // receiver that disappeared during startup cannot leave a provider running.
            handle.cancel();
        }
    }

    let mut reader = BufReader::new(stdout);
    let mut line_buf: Vec<u8> = Vec::new();
    let mut out = TurnOutcome::default();
    let mut raw = String::new();
    let flavor = match plan.provider {
        ProviderKind::Claude => Some(Flavor::Claude),
        ProviderKind::Cursor => Some(Flavor::Cursor),
        ProviderKind::Codex => None,
    };
    let mut json_parser = flavor.map(|f| StreamParser::new(f).with_cwd(req.cwd));
    loop {
        if handle.cancel_flag.load(Ordering::SeqCst) {
            out.cancelled = true;
            break;
        }
        // Wake on cancel even when the provider is silent or its pipe is held by an orphan.
        let next = tokio::select! {
            _ = handle.wake.notified() => {
                out.cancelled = true;
                break;
            }
            n = read_line_bounded(&mut reader, &mut line_buf) => {
                n.map(|n| (n > 0).then(|| decode_line(&line_buf)))
            }
        };
        match next {
            Ok(Some(line)) => {
                if let Some(failed) = terminal_failure(&line) {
                    out.failed = failed;
                }
                if raw.len() < RAW_MAX {
                    raw.push_str(&line);
                    raw.push('\n');
                }
                {
                    let parts = if plain_output {
                        vec![StreamPart::Text(line.clone())]
                    } else {
                        match json_parser.as_mut() {
                            Some(p) => p.parse_line(&line),
                            None => parse_codex_stream_chunk(&line),
                        }
                    };
                    emit_progress(&parts, &on_progress);
                    let mut chunk = codex::StreamOutcome::default();
                    apply_stream_parts(parts, &mut chunk);
                    if let Some(sid) = chunk.session_id {
                        if session_guard.is_none() {
                            // The CLI has already opened its own writer. Hold our guard for
                            // the remainder of the turn so another pane cannot resume it.
                            session_guard = super::session_guard::acquire(
                                &crate::store::default_store_root(),
                                req.home_path,
                                &sid,
                            )
                            .ok();
                        }
                        out.session_id = Some(sid);
                    }
                    if !chunk.text.is_empty() {
                        out.text.push_str(&chunk.text);
                    }
                    for card in chunk.cards {
                        crate::provider::stream_cards::upsert_card(&mut out.cards, card);
                    }
                }
            }
            Ok(None) | Err(_) => break,
        }
    }

    let child = handle.child.lock().await.take();
    if let Some(mut child) = child {
        if out.cancelled {
            // Idempotent with cancel(); covers the path where the reader noticed first.
            kill_process_tree(handle.pid.load(Ordering::SeqCst));
            match tokio::time::timeout(KILL_GRACE, child.wait()).await {
                Ok(Ok(status)) => out.exit_code = status.code(),
                _ => {
                    // Tree kill did not finish the job: terminate the direct child.
                    let _ = child.start_kill();
                    if let Ok(Ok(status)) = tokio::time::timeout(REAP_TIMEOUT, child.wait()).await {
                        out.exit_code = status.code();
                    }
                }
            }
        } else if let Ok(Ok(status)) = tokio::time::timeout(REAP_TIMEOUT, child.wait()).await {
            out.exit_code = status.code();
        }
    }
    // Stop can race with stdout EOF while we are waiting for process exit. It is still a
    // cancellation, even when the reader has already left its select loop.
    out.cancelled |= handle.cancel_flag.load(Ordering::SeqCst);
    // A conversation's first turn names its session; keep it only if the turn really ran, so a
    // failed first turn never leaves an id that does not exist to be resumed next time.
    if out.session_id.is_none() && !out.cancelled && out.exit_code == Some(0) {
        out.session_id = plan.new_session_id.clone();
    }
    out.stderr_tail = match tokio::time::timeout(STDERR_JOIN_TIMEOUT, &mut stderr_task).await {
        Ok(result) => result.unwrap_or_default(),
        Err(_) => {
            stderr_task.abort();
            String::new()
        }
    };
    if out.cancelled {
        if out.text.is_empty() {
            out.text = "cancelled".into();
        }
    } else if out.text.is_empty() && out.cards.is_empty() && plan.provider == ProviderKind::Codex {
        let chunk = extract_stream_chunk(&raw);
        out.text = chunk.text;
        out.cards = chunk.cards;
        if out.session_id.is_none() {
            out.session_id = chunk.session_id;
        }
    }
    Ok(out)
}

fn terminal_failure(line: &str) -> Option<bool> {
    let event: serde_json::Value = serde_json::from_str(line).ok()?;
    match event.get("type")?.as_str()? {
        "turn.failed" => Some(true),
        "turn.completed" => Some(false),
        "result" => Some(
            event.get("is_error").and_then(|v| v.as_bool()) == Some(true)
                || event
                    .get("subtype")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s.starts_with("error")),
        ),
        _ => None,
    }
}

/// Read one line into `buf` (cleared first), keeping at most `LINE_MAX` bytes and discarding the
/// remainder of an over-long line. Returns the bytes consumed; 0 means end of stream. Unlike
/// `lines()`, invalid UTF-8 is not an error (the caller decodes lossily) and memory is bounded.
async fn read_line_bounded<R: AsyncBufRead + Unpin>(
    r: &mut R,
    buf: &mut Vec<u8>,
) -> std::io::Result<usize> {
    buf.clear();
    let mut total = 0usize;
    loop {
        let available = r.fill_buf().await?;
        if available.is_empty() {
            return Ok(total);
        }
        let (used, done) = match available.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (available.len(), false),
        };
        if buf.len() < LINE_MAX {
            let take = (LINE_MAX - buf.len()).min(used);
            buf.extend_from_slice(&available[..take]);
        }
        total += used;
        r.consume(used);
        if done {
            return Ok(total);
        }
    }
}

/// Lossy UTF-8 without the line terminator; marks a line that hit `LINE_MAX`.
fn decode_line(buf: &[u8]) -> String {
    let mut line = String::from_utf8_lossy(buf)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    if buf.len() >= LINE_MAX {
        line.push_str(" ...[line truncated]");
    }
    line
}

fn emit_progress(parts: &[StreamPart], on_progress: &Option<mpsc::UnboundedSender<TurnProgress>>) {
    let Some(tx) = on_progress else {
        return;
    };
    for part in parts {
        let ev = match part {
            StreamPart::Session(id) => TurnProgress::Session(id.clone()),
            StreamPart::Text(t) => TurnProgress::Text(t.clone()),
            StreamPart::TextDelta(t) => TurnProgress::TextDelta(t.clone()),
            StreamPart::Card(c) => TurnProgress::Card(c.clone()),
            StreamPart::Error(e) => TurnProgress::Error(e.clone()),
            StreamPart::Thinking(t) => TurnProgress::Thinking(t.clone()),
            StreamPart::ThinkingDelta(t) => TurnProgress::ThinkingDelta(t.clone()),
            StreamPart::Usage(u) => TurnProgress::Usage(u.clone()),
        };
        let _ = tx.send(ev);
    }
}

/// Kill a mock long-running child — used by cancel tests.
pub async fn spawn_and_cancel_mock() -> Result<bool> {
    let handle = RunningTurn::new();
    let cancel_flag = handle.cancel_flag.clone();
    let child_slot = handle.child.clone();
    #[cfg(windows)]
    let child = Command::new("ping")
        .args(["-n", "30", "127.0.0.1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("spawn ping")?;
    #[cfg(not(windows))]
    let child = Command::new("sleep")
        .arg("30")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("spawn sleep")?;
    {
        let mut slot = child_slot.lock().await;
        *slot = Some(child);
    }
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    handle.cancel();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let mut slot = child_slot.lock().await;
    if let Some(mut child) = slot.take() {
        let status = child.try_wait().context("try_wait")?;
        if status.is_none() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }
    Ok(cancel_flag.load(Ordering::SeqCst))
}

#[cfg(test)]
mod reader_tests {
    use super::*;

    #[test]
    fn terminal_failures_are_distinct_from_recoverable_diagnostics() {
        assert_eq!(
            terminal_failure(r#"{"type":"turn.failed","error":{"message":"usage limit"}}"#),
            Some(true)
        );
        assert_eq!(
            terminal_failure(r#"{"type":"turn.completed"}"#),
            Some(false)
        );
        assert_eq!(
            terminal_failure(r#"{"type":"error","message":"reconnecting"}"#),
            None
        );
        assert_eq!(
            terminal_failure(
                r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#
            ),
            Some(true)
        );
        assert_eq!(
            terminal_failure(r#"{"type":"result","subtype":"success"}"#),
            Some(false)
        );
    }

    async fn lines_of(input: &[u8]) -> Vec<String> {
        let mut r = BufReader::new(input);
        let mut buf = Vec::new();
        let mut out = Vec::new();
        while read_line_bounded(&mut r, &mut buf).await.unwrap() > 0 {
            out.push(decode_line(&buf));
        }
        out
    }

    #[tokio::test]
    async fn splits_lines_handles_crlf_and_a_missing_final_newline() {
        assert_eq!(lines_of(b"a\nb\r\nc").await, ["a", "b", "c"]);
        assert_eq!(lines_of(b"").await, Vec::<String>::new());
        assert_eq!(lines_of(b"\n\n").await, ["", ""]);
    }

    #[tokio::test]
    async fn invalid_utf8_does_not_end_the_stream() {
        let got = lines_of(b"ok\n\xff\xfe bad\nafter\n").await;
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], "ok");
        assert!(got[1].contains('\u{fffd}') && got[1].ends_with("bad"));
        assert_eq!(got[2], "after", "lines after a bad byte must still arrive");
    }

    #[tokio::test]
    async fn an_overlong_line_is_truncated_and_the_next_line_is_intact() {
        let mut input = vec![b'x'; LINE_MAX * 3 + 17];
        input.extend_from_slice(b"\nnext\n");
        let got = lines_of(&input).await;
        assert_eq!(got.len(), 2);
        assert!(got[0].ends_with("...[line truncated]"));
        assert!(got[0].len() <= LINE_MAX + 32, "memory must stay bounded");
        assert_eq!(got[1], "next");
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;

    #[test]
    fn a_stamped_prompt_opens_with_the_time_then_the_pack_then_the_prompt() {
        let cwd = std::env::temp_dir();
        let now = crate::prompt_time::LocalTime {
            year: 2026,
            month: 10,
            day: 30,
            hour: 14,
            minute: 5,
            zone: "local",
        };
        let req = ProviderTurnRequest {
            provider: ProviderKind::Codex,
            prompt: "what broke yesterday?",
            cwd: &cwd,
            home_path: None,
            model: None,
            native_session_id: None,
            trust: TrustMode::Workspace,
            context_prefix: Some("<concordia_context>last week</concordia_context>"),
            local_time: Some(now),
        };
        let plan = plan_provider_turn(&req).unwrap();
        // "last week" in the pack is not the user's words, so it is not resolved.
        assert_eq!(
            plan.prompt,
            "[Fri Oct 30 2026, 2:05pm local]\n\"yesterday\" = Thu Oct 29 2026\n\
             <concordia_context>last week</concordia_context>\nwhat broke yesterday?"
        );
        assert_eq!(plan.argv.last(), Some(&plan.prompt));
        let plain = plan_provider_turn(&ProviderTurnRequest {
            local_time: None,
            context_prefix: None,
            ..req
        })
        .unwrap();
        assert_eq!(plain.prompt, "what broke yesterday?");
    }

    fn claude_argv(cwd: &Path, trust: TrustMode) -> Vec<String> {
        let req = ProviderTurnRequest {
            provider: ProviderKind::Claude,
            prompt: "edit it",
            cwd,
            home_path: None,
            model: None,
            native_session_id: None,
            trust,
            context_prefix: None,
            local_time: None,
        };
        plan_provider_turn(&req).unwrap().argv
    }

    fn locked(dir: &Path) {
        std::fs::create_dir_all(dir.join("docs/quality")).unwrap();
        std::fs::write(dir.join("docs/quality/locked-code.json"), "{}\n").unwrap();
    }

    #[test]
    fn an_unverified_locked_project_plans_a_full_trust_turn_as_read_only() {
        let dir = tempfile::tempdir().unwrap();
        locked(dir.path());
        let argv = claude_argv(dir.path(), TrustMode::Full);
        assert!(argv.windows(2).any(|w| w == ["--permission-mode", "plan"]));
        assert!(!argv.contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn a_project_without_locks_keeps_the_requested_trust() {
        let dir = tempfile::tempdir().unwrap();
        let argv = claude_argv(dir.path(), TrustMode::Full);
        assert!(argv.contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn the_notice_is_available_before_the_turn_starts() {
        let dir = tempfile::tempdir().unwrap();
        locked(dir.path());
        let req = ProviderTurnRequest {
            provider: ProviderKind::Cursor,
            prompt: "x",
            cwd: dir.path(),
            home_path: None,
            model: None,
            native_session_id: None,
            trust: TrustMode::Workspace,
            context_prefix: None,
            local_time: None,
        };
        let notice = enforcement_notice(&req).expect("a lowered turn must explain itself");
        assert!(notice.contains("cursor") && notice.contains("read-only"));
        let plain = tempfile::tempdir().unwrap();
        let req = ProviderTurnRequest {
            cwd: plain.path(),
            ..req
        };
        assert_eq!(enforcement_notice(&req), None);
    }

    #[test]
    fn a_locked_project_refuses_a_turn_whose_cli_cannot_run_read_only() {
        use crate::provider::compat::{caps_for, set_caps, Caps};
        let dir = tempfile::tempdir().unwrap();
        locked(dir.path());
        let before = caps_for(ProviderKind::Cursor);
        // A Cursor CLI without plan mode: the compatibility fallback would drop `--mode plan`.
        set_caps(
            ProviderKind::Cursor,
            Caps {
                known: true,
                plan_mode: false,
                ..Caps::assume_all()
            },
        );
        let req = ProviderTurnRequest {
            provider: ProviderKind::Cursor,
            prompt: "edit it",
            cwd: dir.path(),
            home_path: None,
            model: None,
            native_session_id: None,
            trust: TrustMode::Full,
            context_prefix: None,
            local_time: None,
        };
        let result = plan_provider_turn(&req);
        set_caps(
            ProviderKind::Cursor,
            before.unwrap_or_else(Caps::assume_all),
        );
        let text = result
            .expect_err("must refuse to plan a write-capable turn")
            .to_string();
        assert!(
            text.contains("cursor") && text.contains("read-only"),
            "{text}"
        );
    }
}
