//! Windows-only console inheritance coverage in an isolated integration-test process.

use concordia::provider::{run_provider_turn, ProviderTurnRequest};
use concordia::store::{ProviderKind, TrustMode};
use std::time::{Duration, Instant};

const FAKE: &str = env!("CARGO_BIN_EXE_concordia-fake-provider");

#[cfg(windows)]
#[tokio::test]
async fn hidden_provider_grandchild_has_no_visible_console() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let out = dir.path().join("grandchild.txt");
    let stream = dir.path().join("stream.jsonl");
    std::fs::write(
        &stream,
        "{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok\"}\n",
    )
    .unwrap();
    std::env::set_var("CONCORDIA_CODEX_BIN", FAKE);
    std::env::set_var("FAKE_PROVIDER_MODE", "stream");
    std::env::set_var("FAKE_PROVIDER_STREAM", &stream);
    std::env::set_var("FAKE_PROVIDER_GRANDCHILD_OUT", &out);
    let req = ProviderTurnRequest {
        images: &[],
        provider: ProviderKind::Codex,
        prompt: "grandchild probe",
        cwd: &cwd,
        home_path: None,
        model: None,
        native_session_id: None,
        trust: TrustMode::Workspace,
        context_prefix: None,
        local_time: None,
        compaction: Default::default(),
    };
    run_provider_turn(&req, None).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !out.exists() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        std::fs::read_to_string(&out).unwrap().trim(),
        "GRANDCHILD_VISIBLE=0"
    );
    for key in [
        "CONCORDIA_CODEX_BIN",
        "FAKE_PROVIDER_MODE",
        "FAKE_PROVIDER_STREAM",
        "FAKE_PROVIDER_GRANDCHILD_OUT",
    ] {
        std::env::remove_var(key);
    }
}
