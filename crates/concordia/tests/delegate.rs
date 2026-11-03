//! Delegation end to end against the fake provider: a worker turn runs in the background with its
//! own fresh session, its result is labelled, a second task for the same worker queues behind it,
//! and cancelling kills the provider's whole process tree. Never runs a real provider.

use concordia::delegate::{
    result_block, use_prompt, Delegates, JobSpec, Origin, Submit, WorkerState,
};
use concordia::store::{ProviderKind, TrustMode};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

static LOCK: Mutex<()> = Mutex::new(());
const FAKE: &str = env!("CARGO_BIN_EXE_concordia-fake-provider");

struct Env {
    _guard: MutexGuard<'static, ()>,
    dir: tempfile::TempDir,
    keys: Vec<&'static str>,
}

impl Env {
    fn new() -> Self {
        let mut e = Env {
            _guard: LOCK.lock().unwrap_or_else(|e| e.into_inner()),
            dir: tempfile::tempdir().unwrap(),
            keys: Vec::new(),
        };
        e.set("CONCORDIA_CODEX_BIN", FAKE);
        e
    }
    fn set(&mut self, k: &'static str, v: impl AsRef<std::ffi::OsStr>) {
        std::env::set_var(k, v);
        self.keys.push(k);
    }
    fn path(&self, n: &str) -> PathBuf {
        self.dir.path().join(n)
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
    let b: Vec<u8> = (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
        .collect();
    String::from_utf8(b).unwrap()
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/control-plane")
        .join(name)
}

fn pid_alive(pid: u32) -> bool {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .expect("tasklist");
    String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
}

fn submit(d: &mut Delegates, account: &str, task: &str, cwd: &Path, trust: TrustMode) -> u32 {
    d.submit(
        Submit {
            account: account.into(),
            provider: ProviderKind::Codex,
            task: task.into(),
            origin: Origin::User,
            spec: JobSpec {
                model: None,
                home: None,
                cwd: cwd.to_path_buf(),
                prompt: task.into(),
                prefix: Some("[concordia prior thread - bounded]".into()),
                trust,
            },
        },
        Instant::now(),
    )
    .unwrap()
}

async fn until(d: &mut Delegates, secs: u64, mut f: impl FnMut(&Delegates) -> bool) -> Vec<u32> {
    let start = Instant::now();
    let mut finished = Vec::new();
    while start.elapsed() < Duration::from_secs(secs) {
        finished.extend(d.poll(Instant::now()));
        if f(d) {
            return finished;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    panic!("timed out");
}

#[tokio::test]
async fn a_delegate_runs_in_the_background_with_a_fresh_session_and_a_labelled_result() {
    let mut env = Env::new();
    env.set("FAKE_PROVIDER_MODE", "stream");
    env.set("FAKE_PROVIDER_STREAM", fixture("codex-stream.jsonl"));
    let out = env.path("received.txt");
    env.set("FAKE_PROVIDER_OUT", &out);
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();

    let mut d = Delegates::default();
    let id = submit(
        &mut d,
        "codex-w1",
        "summarise the repo",
        &cwd,
        TrustMode::Workspace,
    );
    assert!(
        d.get(id).unwrap().state.is_running() || d.get(id).unwrap().state == WorkerState::Starting
    );
    // The caller is not blocked: the same worker is busy and a second task queues behind it.
    let q = submit(
        &mut d,
        "codex-w1",
        "second task",
        &cwd,
        TrustMode::Workspace,
    );
    assert_eq!(d.get(q).unwrap().state, WorkerState::Queued);

    let finished = until(&mut d, 20, |d| d.get(id).unwrap().state.is_finished()).await;
    assert!(finished.contains(&id));
    let j = d.get(id).unwrap();
    assert_eq!(j.state, WorkerState::Done, "{:?}", j.error);
    assert!(
        j.final_text().contains("parity hello"),
        "{:?}",
        j.final_text()
    );
    assert!(j.tool_cards() > 0);
    assert_eq!(
        j.session_id.as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    let block = result_block(j, "Codex");
    assert!(
        block.contains("delegate result")
            && block.contains("untrusted")
            && block.contains("tool card")
    );
    assert!(use_prompt(j).contains("UNTRUSTED"));

    // What the provider received: a brand-new session (no resume, no --last), the bounded
    // context ahead of the task, and the workspace trust flags (never full).
    let rec = std::fs::read_to_string(&out).unwrap_or_default();
    let args: Vec<String> = rec
        .lines()
        .filter_map(|l| l.strip_prefix("ARG:"))
        .map(unhex)
        .collect();
    // The queued task ran next and overwrote the file; it is just as fresh.
    until(&mut d, 20, |d| d.get(q).unwrap().state.is_finished()).await;
    let rec2 = std::fs::read_to_string(&out).unwrap();
    let args2: Vec<String> = rec2
        .lines()
        .filter_map(|l| l.strip_prefix("ARG:"))
        .map(unhex)
        .collect();
    for a in [&args, &args2] {
        if a.is_empty() {
            continue;
        }
        assert!(
            a.iter()
                .all(|x| x != "resume" && x != "--last" && x != "--continue"),
            "{a:?}"
        );
        assert!(
            a.iter()
                .all(|x| x != "--dangerously-bypass-approvals-and-sandbox"),
            "{a:?}"
        );
        assert!(a
            .last()
            .unwrap()
            .contains("[concordia prior thread - bounded]"));
    }
    assert!(!args2.is_empty());
    assert_eq!(d.get(q).unwrap().state, WorkerState::Done);
}

#[tokio::test]
async fn explicit_worker_quota_failover_is_bounded_and_recovery_never_reexecutes() {
    let mut env = Env::new();
    env.set("CONCORDIA_CLAUDE_BIN", FAKE);
    env.set("FAKE_PROVIDER_MODE", "interaction");
    let journal = env.path("journal.jsonl");
    env.set("FAKE_PROVIDER_JOURNAL", &journal);
    env.set("CONCORDIA_CODEX_HOME_BASE", env.path("empty-profile"));
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    for (account, provider, marker) in [
        ("worker-one", ProviderKind::Codex, "auth.json"),
        ("worker-two", ProviderKind::Claude, ".credentials.json"),
    ] {
        let home = env.path(account);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(marker), "synthetic-login-existence-marker").unwrap();
        concordia::store::upsert_stored_account(
            env.dir.path(),
            concordia::store::StoredAccount {
                id: account.into(),
                provider,
                home_path: Some(home.display().to_string()),
                label: account.into(),
                created_at: "0".into(),
            },
        )
        .unwrap();
    }
    concordia::store::modify_store(env.dir.path(), |s| {
        s.settings.workers = vec!["worker-one".into(), "worker-two".into()];
        s.settings.delegate_quota_failover = true;
        Ok(())
    })
    .unwrap();
    let recovery = env.path("pane/worker-runs");
    let mut d = Delegates::default();
    d.attach_recovery(&recovery, &cwd, env.dir.path()).unwrap();
    let id = d
        .submit(
            Submit {
                account: "worker-one".into(),
                provider: ProviderKind::Codex,
                task: "bounded task [quota]".into(),
                origin: Origin::User,
                spec: JobSpec {
                    model: None,
                    home: Some(env.path("worker-one")),
                    cwd: cwd.clone(),
                    prompt: "bounded task [quota]".into(),
                    prefix: None,
                    trust: TrustMode::Ask,
                },
            },
            Instant::now(),
        )
        .unwrap();
    let run = d.get(id).unwrap().run_id.clone();
    until(&mut d, 20, |d| d.get(id).unwrap().state.is_finished()).await;
    assert_eq!(
        d.get(id).unwrap().state,
        WorkerState::Done,
        "{:?}",
        d.get(id).unwrap().error
    );
    assert_eq!(d.get(id).unwrap().account, "worker-two");
    assert_eq!(d.get(id).unwrap().trust, TrustMode::Ask);
    d.reserve_delivery(&[id]).unwrap();
    let before = std::fs::read_to_string(&journal).unwrap();
    assert_eq!(
        before
            .lines()
            .filter(|l| l.contains("\"kind\":\"start\""))
            .count(),
        2
    );
    drop(d);
    let mut restored = Delegates::default();
    restored
        .attach_recovery(&recovery, &cwd, env.dir.path())
        .unwrap();
    assert_eq!(restored.get(id).unwrap().run_id, run);
    assert!(restored.get(id).unwrap().collected);
    assert!(restored.get(id).unwrap().text.is_empty());
    restored.poll(Instant::now());
    assert_eq!(std::fs::read_to_string(journal).unwrap(), before);
}

#[tokio::test]
async fn a_failing_worker_reports_why() {
    let mut env = Env::new();
    env.set("FAKE_PROVIDER_MODE", "fail");
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let mut d = Delegates::default();
    let id = submit(&mut d, "codex-w1", "x", &cwd, TrustMode::Ask);
    until(&mut d, 20, |d| d.get(id).unwrap().state.is_finished()).await;
    let j = d.get(id).unwrap();
    assert_eq!(j.state, WorkerState::Failed);
    assert!(j.error.as_deref().unwrap().contains("code 3"));
    assert!(result_block(j, "Codex").contains("simulated failure"));
}

#[tokio::test]
async fn stopping_a_worker_kills_the_provider_process_and_drops_its_queue() {
    let mut env = Env::new();
    env.set("FAKE_PROVIDER_MODE", "hang");
    let pid_file = env.path("pid.txt");
    env.set("FAKE_PROVIDER_PID", &pid_file);
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();

    let mut d = Delegates::default();
    let id = submit(&mut d, "codex-w1", "long task", &cwd, TrustMode::Workspace);
    let q = submit(&mut d, "codex-w1", "queued", &cwd, TrustMode::Workspace);
    // Wait for the provider to be really running.
    let start = Instant::now();
    let pid: u32 = loop {
        d.poll(Instant::now());
        if let Ok(s) = std::fs::read_to_string(&pid_file) {
            if let Ok(p) = s.trim().parse() {
                break p;
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "provider never started"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    until(&mut d, 10, |d| {
        d.get(id).unwrap().state == WorkerState::Thinking
    })
    .await;
    assert!(pid_alive(pid));

    assert_eq!(d.cancel_account("codex-w1", Instant::now()), 2);
    assert_eq!(
        d.get(q).unwrap().state,
        WorkerState::Cancelled,
        "queued work is dropped"
    );
    until(&mut d, 15, |d| d.get(id).unwrap().state.is_finished()).await;
    assert_eq!(d.get(id).unwrap().state, WorkerState::Cancelled);
    assert!(
        d.get(id).unwrap().final_text().is_empty(),
        "no result from a cancelled worker"
    );

    let start = Instant::now();
    while pid_alive(pid) && start.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        !pid_alive(pid),
        "provider pid {pid} survived the stop (orphan)"
    );
    // The queued job never started a process of its own.
    assert!(!d.any_active());
}

#[tokio::test]
async fn cancelling_right_after_submit_still_stops_the_process() {
    let mut env = Env::new();
    env.set("FAKE_PROVIDER_MODE", "hang");
    let pid_file = env.path("pid.txt");
    env.set("FAKE_PROVIDER_PID", &pid_file);
    let cwd = env.path("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let mut d = Delegates::default();
    let id = submit(&mut d, "codex-w1", "t", &cwd, TrustMode::Workspace);
    // Before the runner has even reported its handle.
    assert!(d.cancel(id, Instant::now()));
    until(&mut d, 15, |d| d.get(id).unwrap().state.is_finished()).await;
    assert_eq!(d.get(id).unwrap().state, WorkerState::Cancelled);
    if let Ok(s) = std::fs::read_to_string(&pid_file) {
        if let Ok(pid) = s.trim().parse::<u32>() {
            let start = Instant::now();
            while pid_alive(pid) && start.elapsed() < Duration::from_secs(5) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            assert!(!pid_alive(pid), "orphaned provider {pid}");
        }
    }
}
