//! Startup handoff failure coverage for the provider process runner.

use concordia::provider::{run_provider_turn, ProviderTurnRequest};
use concordia::store::{ProviderKind, TrustMode};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const FAKE: &str = env!("CARGO_BIN_EXE_concordia-fake-provider");

#[tokio::test]
async fn closed_started_receiver_cancels_the_provider() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let pid_file = dir.path().join("provider.pid");
    std::env::set_var("CONCORDIA_CODEX_BIN", FAKE);
    std::env::set_var("FAKE_PROVIDER_MODE", "hang");
    std::env::set_var("FAKE_PROVIDER_PID", &pid_file);

    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);
    let req = ProviderTurnRequest {
        provider: ProviderKind::Codex,
        prompt: "cancel when startup handoff is unavailable",
        cwd: &cwd,
        home_path: None,
        model: None,
        native_session_id: None,
        trust: TrustMode::Workspace,
        context_prefix: None,
        local_time: None,
    };
    let outcome = tokio::time::timeout(Duration::from_secs(10), run_provider_turn(&req, Some(tx)))
        .await
        .expect("runner should stop after the startup receiver is gone")
        .expect("cancellation should be a normal outcome");

    std::env::remove_var("CONCORDIA_CODEX_BIN");
    std::env::remove_var("FAKE_PROVIDER_MODE");
    std::env::remove_var("FAKE_PROVIDER_PID");
    assert!(outcome.cancelled, "outcome: {outcome:?}");

    // The runner may cancel before the fake process has time to write its marker.
    // If it did start far enough to publish a PID, ensure that process is reaped.
    if let Ok(pid_text) = std::fs::read_to_string(&pid_file) {
        let pid = pid_text.trim().parse::<u32>().expect("fake provider PID");
        let start = Instant::now();
        while process_alive(pid) && start.elapsed() < Duration::from_secs(5) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            !process_alive(pid),
            "provider PID {pid} survived startup cancellation"
        );
    }
}

fn process_alive(pid: u32) -> bool {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .expect("tasklist");
    String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
}
