use concordia::provider::turn::{run_provider_turn_with_idle_timeout, TurnProgress};
use concordia::provider::{ProviderTurnRequest, RunningTurn};
use concordia::store::{ProviderKind, TrustMode};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};

const FAKE: &str = env!("CARGO_BIN_EXE_concordia-fake-provider");
static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn request<'a>(cwd: &'a std::path::Path) -> ProviderTurnRequest<'a> {
    ProviderTurnRequest {
        images: &[],
        provider: ProviderKind::Codex,
        prompt: "idle watchdog",
        cwd,
        home_path: None,
        model: None,
        native_session_id: None,
        trust: TrustMode::Workspace,
        context_prefix: None,
        local_time: None,
        compaction: Default::default(),
    }
}

async fn run(
    mode: &str,
    timeout: Duration,
) -> (
    tokio::task::JoinHandle<anyhow::Result<concordia::provider::TurnOutcome>>,
    mpsc::UnboundedReceiver<TurnProgress>,
    mpsc::UnboundedReceiver<Arc<RunningTurn>>,
) {
    std::env::remove_var("NO_COLOR");
    std::env::set_var("CONCORDIA_CODEX_BIN", FAKE);
    std::env::set_var("FAKE_PROVIDER_MODE", mode);
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let (progress_tx, progress_rx) = mpsc::unbounded_channel();
    let (started_tx, started_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let _dir = dir;
        let req = request(&cwd);
        run_provider_turn_with_idle_timeout(&req, Some(started_tx), Some(progress_tx), timeout)
            .await
    });
    (task, progress_rx, started_rx)
}

async fn cancel(
    task: tokio::task::JoinHandle<anyhow::Result<concordia::provider::TurnOutcome>>,
    handle: Arc<RunningTurn>,
) {
    handle.cancel();
    let outcome = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(outcome.cancelled, "outcome: {outcome:?}");
}

#[tokio::test]
async fn idle_fires_with_cumulative_silence_and_never_cancels() {
    let _guard = ENV_LOCK.lock().await;
    let (task, mut progress, mut started) = run("silent-hang", Duration::from_millis(150)).await;
    let handle = tokio::time::timeout(Duration::from_secs(1), started.recv())
        .await
        .unwrap()
        .unwrap();
    let mut idle = Vec::new();
    let deadline = tokio::time::sleep(Duration::from_millis(550));
    tokio::pin!(deadline);
    while !deadline.is_elapsed() {
        tokio::select! {
            Some(event) = progress.recv() => if let TurnProgress::Idle { silent_for, .. } = event { idle.push(silent_for); },
            _ = &mut deadline => break,
        }
    }
    assert!(idle.len() >= 2, "idle events: {idle:?}");
    assert!(idle[0] >= Duration::from_millis(150));
    assert!(
        idle.windows(2).all(|pair| pair[1] > pair[0]),
        "idle events: {idle:?}"
    );
    assert!(!task.is_finished());
    cancel(task, handle).await;
}

#[tokio::test]
async fn stdout_activity_prevents_idle() {
    let _guard = ENV_LOCK.lock().await;
    let (task, mut progress, mut started) = run("drip-stdout", Duration::from_secs(2)).await;
    let handle = tokio::time::timeout(Duration::from_secs(2), started.recv())
        .await
        .unwrap()
        .unwrap();
    let mut idle = 0;
    let deadline = tokio::time::sleep(Duration::from_millis(700));
    tokio::pin!(deadline);
    while !deadline.is_elapsed() {
        tokio::select! { Some(event) = progress.recv() => if matches!(event, TurnProgress::Idle { .. }) { idle += 1; }, _ = &mut deadline => break }
    }
    assert_eq!(idle, 0);
    cancel(task, handle).await;
}

#[tokio::test]
async fn stderr_activity_prevents_idle() {
    let _guard = ENV_LOCK.lock().await;
    let (task, mut progress, mut started) = run("drip-stderr", Duration::from_secs(2)).await;
    let handle = tokio::time::timeout(Duration::from_secs(2), started.recv())
        .await
        .unwrap()
        .unwrap();
    let mut idle = 0;
    let deadline = tokio::time::sleep(Duration::from_millis(700));
    tokio::pin!(deadline);
    while !deadline.is_elapsed() {
        tokio::select! { Some(event) = progress.recv() => if matches!(event, TurnProgress::Idle { .. }) { idle += 1; }, _ = &mut deadline => break }
    }
    assert_eq!(idle, 0);
    cancel(task, handle).await;
}

#[tokio::test]
async fn zero_timeout_disables_idle() {
    let _guard = ENV_LOCK.lock().await;
    let (task, mut progress, mut started) = run("hang", Duration::ZERO).await;
    let handle = tokio::time::timeout(Duration::from_secs(2), started.recv())
        .await
        .unwrap()
        .unwrap();
    let result = tokio::time::timeout(Duration::from_millis(300), async {
        let mut idle = 0;
        while let Some(event) = progress.recv().await {
            if matches!(event, TurnProgress::Idle { .. }) {
                idle += 1;
            }
        }
        idle
    })
    .await;
    assert!(result.is_err(), "progress unexpectedly closed");
    cancel(task, handle).await;
}
