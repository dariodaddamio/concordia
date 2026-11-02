//! Real turn runner, fake provider: argv fidelity, env isolation, streaming, failure and cancel
//! (including killing the whole process tree behind an npm-style `.cmd` shim).
//!
//! Environment variables are process-global, so every test takes `LOCK` and clears what it sets.

use concordia::provider::{
    run_provider_turn, run_provider_turn_with_progress, ProviderTurnRequest, RunningTurn,
    TurnProgress,
};
use concordia::store::{ProviderKind, TrustMode};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

static LOCK: Mutex<()> = Mutex::new(());

const FAKE: &str = env!("CARGO_BIN_EXE_concordia-fake-provider");

struct Env {
    _guard: MutexGuard<'static, ()>,
    dir: tempfile::TempDir,
    keys: Vec<&'static str>,
}

impl Env {
    fn new() -> Self {
        Env {
            _guard: LOCK.lock().unwrap_or_else(|e| e.into_inner()),
            dir: tempfile::tempdir().unwrap(),
            keys: Vec::new(),
        }
    }
    fn set(&mut self, k: &'static str, v: impl AsRef<std::ffi::OsStr>) {
        std::env::set_var(k, v);
        self.keys.push(k);
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    /// Point codex at the fake provider, directly or through an npm-style `.cmd` shim.
    fn use_codex(&mut self, via_cmd_shim: bool) {
        let bin = if via_cmd_shim {
            let shim = self.path("codex.cmd");
            // Mirrors npm's generated shim: forwards every argument with %*.
            std::fs::write(&shim, format!("@ECHO off\r\n\"{FAKE}\" %*\r\n")).unwrap();
            shim
        } else {
            PathBuf::from(FAKE)
        };
        self.set("CONCORDIA_CODEX_BIN", bin);
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        for k in &self.keys {
            std::env::remove_var(k);
        }
    }
}

fn unhex(s: &str) -> String {
    let bytes: Vec<u8> = (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
        .collect();
    String::from_utf8(bytes).unwrap()
}

#[derive(Debug, Default)]
struct Received {
    args: Vec<String>,
    env: Vec<(String, String)>,
    cwd: String,
    stdin: Option<String>,
}

fn read_received(path: &Path) -> Received {
    let mut r = Received::default();
    for line in std::fs::read_to_string(path)
        .expect("fake provider output")
        .lines()
    {
        if let Some(h) = line.strip_prefix("ARG:") {
            r.args.push(unhex(h));
        } else if let Some(rest) = line.strip_prefix("ENV:") {
            let (k, v) = rest.split_once('=').unwrap();
            r.env.push((k.to_string(), unhex(v)));
        } else if let Some(h) = line.strip_prefix("CWD:") {
            r.cwd = unhex(h);
        } else if let Some(h) = line.strip_prefix("STDIN:") {
            r.stdin = Some(unhex(h));
        }
    }
    r
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("control-plane")
        .join(name)
}

fn req<'a>(prompt: &'a str, cwd: &'a Path, home: Option<&'a Path>) -> ProviderTurnRequest<'a> {
    ProviderTurnRequest {
        images: &[],
        provider: ProviderKind::Codex,
        prompt,
        cwd,
        home_path: home,
        model: None,
        native_session_id: None,
        trust: TrustMode::Workspace,
        context_prefix: None,
        local_time: None,
        compaction: Default::default(),
    }
}

/// The prompt is always the final argv entry the runner passes.
fn prompt_arg(r: &Received) -> &str {
    r.args.last().expect("at least the prompt arg")
}

const NASTY: &str = "line one\nline two with \"double\" and 'single' quotes\n\
    percent %PATH% %1 caret ^ amp & pipe | redirect < > paren (x) bang !\n\
    unicode 日本語 émoji ✓\n  indented\ttab";

#[tokio::test]
async fn shell_start_arrives_while_provider_waits_for_completion_gate() {
    use concordia::provider::stream_cards::CardStatus;
    let mut env = Env::new();
    env.use_codex(false);
    env.set("FAKE_PROVIDER_MODE", "gated-stream");
    env.set("FAKE_PROVIDER_STREAM", fixture("codex-stream.jsonl"));
    let gate = env.path("release-stream");
    env.set("FAKE_PROVIDER_GATE", &gate);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let request = req("stream before completion", &cwd, None);
    let run = run_provider_turn_with_progress(&request, None, Some(tx));
    tokio::pin!(run);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            tokio::select! {
                result = &mut run => panic!("provider finished before gate release: {result:?}"),
                event = rx.recv() => match event.expect("progress stream open") {
                    TurnProgress::Card(card) if card.status == CardStatus::Running => break,
                    _ => {}
                }
            }
        }
    })
    .await
    .expect("running shell event must arrive before completion");
    assert!(!gate.exists());
    std::fs::write(gate, "release").unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outcome.exit_code, Some(0));
    assert!(outcome
        .cards
        .iter()
        .any(|card| card.status == CardStatus::Done));
}

#[tokio::test]
async fn streams_session_text_and_cards_through_the_real_runner() {
    let mut env = Env::new();
    env.use_codex(false);
    env.set("FAKE_PROVIDER_MODE", "stream");
    env.set("FAKE_PROVIDER_STREAM", fixture("codex-stream.jsonl"));
    let home = env.path("home");
    std::fs::create_dir_all(&home).unwrap();
    let out_file = env.path("received.txt");
    env.set("FAKE_PROVIDER_OUT", &out_file);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let outcome = run_provider_turn_with_progress(&req("hi", &cwd, Some(&home)), None, Some(tx))
        .await
        .unwrap();

    assert_eq!(outcome.exit_code, Some(0));
    assert!(!outcome.cancelled);
    assert_eq!(
        outcome.session_id.as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    assert!(outcome.text.contains("parity hello"), "{:?}", outcome.text);
    assert!(!outcome.cards.is_empty());

    // Progress events arrived live, in order, before completion.
    let mut saw_session = false;
    let mut saw_text = false;
    while let Ok(ev) = rx.try_recv() {
        match ev {
            TurnProgress::Session(_) => saw_session = true,
            TurnProgress::Text(_) => saw_text = true,
            _ => {}
        }
    }
    assert!(saw_session && saw_text);

    // The isolated home and cwd reached the child; no `--last`/`--continue` was passed.
    let got = read_received(&out_file);
    assert!(got
        .env
        .iter()
        .any(|(k, v)| k == "CODEX_HOME" && Path::new(v) == home));
    assert_eq!(
        Path::new(&got.cwd).canonicalize().unwrap(),
        cwd.canonicalize().unwrap()
    );
    assert!(got.args.iter().all(|a| a != "--last" && a != "--continue"));
    assert_eq!(&got.args[..2], ["exec", "--json"]);
}

#[tokio::test]
async fn nasty_prompt_survives_a_direct_exe_byte_for_byte() {
    let mut env = Env::new();
    env.use_codex(false);
    let out_file = env.path("received.txt");
    env.set("FAKE_PROVIDER_OUT", &out_file);
    env.set("FAKE_PROVIDER_MODE", "stream");
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();

    run_provider_turn(&req(NASTY, &cwd, None), None)
        .await
        .unwrap();
    assert_eq!(prompt_arg(&read_received(&out_file)), NASTY);
}

/// Run one turn behind the `.cmd` shim and return what the provider received.
async fn received_via_shim(prompt: &str) -> Received {
    let mut env = Env::new();
    env.use_codex(true);
    let out_file = env.path("received.txt");
    env.set("FAKE_PROVIDER_OUT", &out_file);
    env.set("FAKE_PROVIDER_READ_STDIN", "1");
    env.set("FAKE_PROVIDER_MODE", "stream");
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    run_provider_turn(&req(prompt, &cwd, None), None)
        .await
        .expect("turn must start behind a .cmd shim");
    read_received(&out_file)
}

#[tokio::test]
async fn a_multi_line_special_character_prompt_behind_a_cmd_shim_arrives_intact_over_stdin() {
    let got = received_via_shim(NASTY).await;
    assert_eq!(
        got.stdin.as_deref(),
        Some(NASTY),
        "prompt must arrive byte-for-byte"
    );
    assert_eq!(
        prompt_arg(&got),
        "-",
        "codex reads its prompt from stdin when given `-`"
    );
    assert!(!got.args.iter().any(|a| a.contains("line one")));
}

#[tokio::test]
async fn a_very_long_prompt_behind_a_cmd_shim_arrives_intact_over_stdin() {
    // cmd.exe caps a command line at 8191 chars; continuity packs can exceed that.
    let long = "word ".repeat(4000);
    let got = received_via_shim(&long).await;
    assert_eq!(got.stdin.as_deref(), Some(long.as_str()));
    assert_eq!(prompt_arg(&got), "-");
}

#[tokio::test]
async fn a_simple_prompt_behind_a_cmd_shim_keeps_the_proven_argv_path() {
    let got = received_via_shim("fix the bug in src/main.rs, please?").await;
    assert_eq!(
        got.stdin.as_deref(),
        Some(""),
        "nothing is piped for a plain prompt"
    );
    assert_eq!(prompt_arg(&got), "fix the bug in src/main.rs, please?");
}

#[tokio::test]
async fn a_prompt_that_looks_like_a_flag_is_never_passed_as_one() {
    let mut env = Env::new();
    env.use_codex(false);
    let out_file = env.path("received.txt");
    env.set("FAKE_PROVIDER_OUT", &out_file);
    env.set("FAKE_PROVIDER_READ_STDIN", "1");
    env.set("FAKE_PROVIDER_MODE", "stream");
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let flag = "--dangerously-bypass-approvals-and-sandbox";
    run_provider_turn(&req(flag, &cwd, None), None)
        .await
        .unwrap();
    let got = read_received(&out_file);
    assert!(!got.args.iter().any(|a| a == flag), "{:?}", got.args);
    assert_eq!(got.stdin.as_deref(), Some(flag));
}

#[tokio::test]
async fn provider_stderr_is_captured_so_failures_can_be_explained() {
    let mut env = Env::new();
    env.use_codex(false);
    env.set("FAKE_PROVIDER_MODE", "fail");
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let outcome = run_provider_turn(&req("hi", &cwd, None), None)
        .await
        .unwrap();
    assert_eq!(outcome.exit_code, Some(3));
    assert!(
        outcome.stderr_tail.contains("simulated failure"),
        "{:?}",
        outcome.stderr_tail
    );
}

#[tokio::test]
async fn a_provider_flooding_stderr_cannot_deadlock_the_turn() {
    let mut env = Env::new();
    env.use_codex(false);
    env.set("FAKE_PROVIDER_MODE", "flood");
    env.set("FAKE_PROVIDER_STREAM", fixture("codex-stream.jsonl"));
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        run_provider_turn(&req("hi", &cwd, None), None),
    )
    .await
    .expect("an unread stderr pipe would hang here")
    .unwrap();
    assert_eq!(outcome.exit_code, Some(0));
    assert!(outcome.text.contains("parity hello"));
    // Only the tail is kept, and it is bounded.
    assert!(outcome.stderr_tail.lines().count() <= 12);
    assert!(outcome.stderr_tail.contains("flood line"));
}

#[tokio::test]
async fn a_failing_provider_reports_its_exit_code_not_a_panic() {
    let mut env = Env::new();
    env.use_codex(false);
    env.set("FAKE_PROVIDER_MODE", "fail");
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let outcome = run_provider_turn(&req("hi", &cwd, None), None)
        .await
        .unwrap();
    assert_eq!(outcome.exit_code, Some(3));
    assert!(!outcome.cancelled);
}

#[tokio::test]
async fn a_missing_provider_binary_is_a_clear_error() {
    let mut env = Env::new();
    env.set("CONCORDIA_CODEX_BIN", env.path("definitely-not-here.exe"));
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let err = run_provider_turn(&req("hi", &cwd, None), None)
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("spawn"), "{err:#}");
}

// ---- cancel ----------------------------------------------------------------------------------

fn pid_alive(pid: u32) -> bool {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .expect("tasklist");
    String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
}

async fn wait_for_file(path: &Path, secs: u64) -> String {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if let Ok(s) = std::fs::read_to_string(path) {
            if !s.trim().is_empty() {
                return s;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {}", path.display());
}

async fn cancel_scenario(via_cmd_shim: bool, mode: &str) {
    let mut env = Env::new();
    env.use_codex(via_cmd_shim);
    env.set("FAKE_PROVIDER_MODE", mode);
    let pid_file = env.path("pid.txt");
    env.set("FAKE_PROVIDER_PID", &pid_file);
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();

    let (htx, mut hrx) = mpsc::unbounded_channel::<Arc<RunningTurn>>();
    let cwd2 = cwd.clone();
    let task =
        tokio::spawn(async move { run_provider_turn(&req("hi", &cwd2, None), Some(htx)).await });

    let handle = tokio::time::timeout(Duration::from_secs(10), hrx.recv())
        .await
        .expect("handle")
        .expect("handle sent");
    let pid: u32 = wait_for_file(&pid_file, 15).await.trim().parse().unwrap();
    assert!(pid_alive(pid), "provider should be running before cancel");
    if mode == "eof-hang" {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    handle.cancel();
    let outcome = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("runner must return promptly after cancel")
        .unwrap()
        .unwrap();
    assert!(outcome.cancelled);

    // The provider process itself must be gone, not just the shim in front of it.
    let start = Instant::now();
    while pid_alive(pid) && start.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        !pid_alive(pid),
        "provider pid {pid} survived cancel (via_cmd_shim={via_cmd_shim})"
    );
}

#[tokio::test]
async fn cancel_stops_a_direct_provider() {
    cancel_scenario(false, "hang").await;
}

#[tokio::test]
async fn cancel_stops_the_provider_behind_a_cmd_shim_not_just_the_shim() {
    cancel_scenario(true, "hang").await;
}

#[cfg(windows)]
#[tokio::test]
async fn cancel_after_stdout_eof_is_reported_as_cancellation() {
    cancel_scenario(false, "eof-hang").await;
}

async fn stream_after_odd_output(mode: &'static str) -> concordia::provider::TurnOutcome {
    let mut env = Env::new();
    env.use_codex(false);
    env.set("FAKE_PROVIDER_MODE", mode);
    env.set("FAKE_PROVIDER_STREAM", fixture("codex-stream.jsonl"));
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    tokio::time::timeout(
        Duration::from_secs(30),
        run_provider_turn(&req("hi", &cwd, None), None),
    )
    .await
    .expect("turn must not hang")
    .unwrap()
}

#[tokio::test]
async fn invalid_utf8_from_the_provider_does_not_truncate_the_reply() {
    let outcome = stream_after_odd_output("badutf8").await;
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(
        outcome.session_id.as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    assert!(outcome.text.contains("parity hello"), "{:?}", outcome.text);
}

#[tokio::test]
async fn a_multi_megabyte_line_is_bounded_and_the_rest_of_the_stream_still_parses() {
    let outcome = stream_after_odd_output("longline").await;
    assert_eq!(outcome.exit_code, Some(0));
    assert!(
        outcome.text.contains("parity hello"),
        "stream after the long line was lost"
    );
    assert!(
        outcome.text.len() < 2 * 1024 * 1024,
        "reply text ballooned to {} bytes",
        outcome.text.len()
    );
}

// ---- session continuity for Claude and Cursor ---------------------------------------------------

fn req_for<'a>(
    provider: ProviderKind,
    prompt: &'a str,
    cwd: &'a Path,
    session: Option<&'a str>,
) -> ProviderTurnRequest<'a> {
    ProviderTurnRequest {
        images: &[],
        provider,
        prompt,
        cwd,
        home_path: None,
        model: None,
        native_session_id: session,
        trust: TrustMode::Workspace,
        context_prefix: None,
        local_time: None,
        compaction: Default::default(),
    }
}

/// Value following `flag` in the recorded argv.
fn arg_after<'a>(r: &'a Received, flag: &str) -> Option<&'a str> {
    r.args
        .windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

#[tokio::test]
async fn a_claude_conversation_names_its_session_then_resumes_it() {
    let mut env = Env::new();
    env.set("CONCORDIA_CLAUDE_BIN", FAKE);
    let out_file = env.path("received.txt");
    env.set("FAKE_PROVIDER_OUT", &out_file);
    env.set("FAKE_PROVIDER_MODE", "stream");
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();

    // Turn 1: no session yet, so one is chosen up front and reported back.
    let first = run_provider_turn(&req_for(ProviderKind::Claude, "hello", &cwd, None), None)
        .await
        .unwrap();
    let got = read_received(&out_file);
    let sid = arg_after(&got, "--session-id").expect("first turn must name its session");
    assert_eq!(sid.len(), 36, "a UUID: {sid}");
    assert!(arg_after(&got, "--resume").is_none());
    assert_eq!(first.session_id.as_deref(), Some(sid));

    // Turn 2: that id is resumed, and no new one is invented.
    let sid = sid.to_string();
    let second = run_provider_turn(
        &req_for(ProviderKind::Claude, "and again", &cwd, Some(&sid)),
        None,
    )
    .await
    .unwrap();
    let got = read_received(&out_file);
    assert_eq!(arg_after(&got, "--resume"), Some(sid.as_str()));
    assert!(arg_after(&got, "--session-id").is_none());
    assert_eq!(prompt_arg(&got), "and again");
    assert!(
        second.session_id.is_none(),
        "an existing session is not re-reported as new"
    );
}

#[tokio::test]
async fn a_failed_first_claude_turn_does_not_leave_a_session_to_resume() {
    let mut env = Env::new();
    env.set("CONCORDIA_CLAUDE_BIN", FAKE);
    env.set("FAKE_PROVIDER_MODE", "fail");
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let out = run_provider_turn(&req_for(ProviderKind::Claude, "hi", &cwd, None), None)
        .await
        .unwrap();
    assert_eq!(out.exit_code, Some(3));
    assert!(out.session_id.is_none(), "{:?}", out.session_id);
}

#[tokio::test]
async fn a_cursor_conversation_gets_a_created_chat_and_resumes_it() {
    let mut env = Env::new();
    env.set("CONCORDIA_AGENT_BIN", FAKE);
    let out_file = env.path("received.txt");
    env.set("FAKE_PROVIDER_OUT", &out_file);
    env.set("FAKE_PROVIDER_MODE", "stream");
    let id = "3f2a9c1e-7b44-4d0a-9e51-0a1b2c3d4e5f";
    env.set("FAKE_PROVIDER_CHAT_OUT", format!(r#"{{"chatId":"{id}"}}"#));
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();

    let first = run_provider_turn(&req_for(ProviderKind::Cursor, "hello", &cwd, None), None)
        .await
        .unwrap();
    let got = read_received(&out_file);
    assert_eq!(arg_after(&got, "--resume"), Some(id));
    assert_eq!(prompt_arg(&got), "hello", "the prompt stays last");
    assert_eq!(first.session_id.as_deref(), Some(id));

    // With a bound chat, no new one is created and it is resumed as given.
    let second = run_provider_turn(&req_for(ProviderKind::Cursor, "more", &cwd, Some(id)), None)
        .await
        .unwrap();
    let got = read_received(&out_file);
    assert_eq!(arg_after(&got, "--resume"), Some(id));
    assert!(second.session_id.is_none());
}

#[tokio::test]
async fn cursor_falls_back_to_an_unbound_turn_when_create_chat_is_unusable() {
    for (label, key, value) in [
        ("garbage output", "FAKE_PROVIDER_CHAT_OUT", "no id here"),
        ("a failing command", "FAKE_PROVIDER_CHAT_FAIL", "1"),
    ] {
        let mut env = Env::new();
        env.set("CONCORDIA_AGENT_BIN", FAKE);
        let out_file = env.path("received.txt");
        env.set("FAKE_PROVIDER_OUT", &out_file);
        env.set("FAKE_PROVIDER_MODE", "stream");
        env.set(key, value);
        let cwd = env.path("work");
        std::fs::create_dir_all(&cwd).unwrap();
        let out = run_provider_turn(&req_for(ProviderKind::Cursor, "hello", &cwd, None), None)
            .await
            .unwrap();
        let got = read_received(&out_file);
        assert!(
            arg_after(&got, "--resume").is_none(),
            "{label}: {:?}",
            got.args
        );
        assert_eq!(prompt_arg(&got), "hello", "{label}");
        assert!(out.session_id.is_none(), "{label}");
        assert_eq!(
            out.exit_code,
            Some(0),
            "{label}: the turn itself must still run"
        );
    }
}

#[tokio::test]
async fn a_claude_result_line_with_usage_reaches_progress_but_never_the_reply() {
    let mut env = Env::new();
    env.set("CONCORDIA_CLAUDE_BIN", FAKE);
    env.set("FAKE_PROVIDER_MODE", "stream");
    let stream = env.path("claude-stream.jsonl");
    std::fs::write(
        &stream,
        concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"synthetic reply"}]}}"#,
            "\n",
            r#"{"type":"result","subtype":"success","is_error":false,"result":"synthetic reply","total_cost_usd":0.5,"usage":{"input_tokens":1200,"output_tokens":34,"cache_read_input_tokens":800}}"#,
            "\n"
        ),
    )
    .unwrap();
    env.set("FAKE_PROVIDER_STREAM", &stream);
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let outcome = run_provider_turn_with_progress(
        &req_for(ProviderKind::Claude, "hi", &cwd, None),
        None,
        Some(tx),
    )
    .await
    .unwrap();
    assert_eq!(outcome.text.trim(), "synthetic reply");
    let mut usage = None;
    while let Ok(ev) = rx.try_recv() {
        if let TurnProgress::Usage(u) = ev {
            usage = Some(u);
        }
    }
    let u = usage.expect("usage progress event");
    assert_eq!(
        (u.input, u.output, u.cached),
        (Some(1200), Some(34), Some(800))
    );
    assert_eq!(u.cost_usd, Some(0.5));
}
