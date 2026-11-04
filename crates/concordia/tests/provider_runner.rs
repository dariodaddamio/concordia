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
        images: &[],
        provider: ProviderKind::Codex,
        prompt: "cancel when startup handoff is unavailable",
        cwd: &cwd,
        home_path: None,
        model: None,
        native_session_id: None,
        trust: TrustMode::Workspace,
        context_prefix: None,
        local_time: None,
        compaction: Default::default(),
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

    // A resumed Claude result can include earlier session cost; never count it as this attempt.
    let stream = dir.path().join("claude-usage.jsonl");
    std::fs::write(&stream, "{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"done\",\"total_cost_usd\":1.25,\"usage\":{\"input_tokens\":120,\"cache_read_input_tokens\":900,\"cache_creation_input_tokens\":7,\"output_tokens\":45}}\n").unwrap();
    std::env::set_var("CONCORDIA_CLAUDE_BIN", FAKE);
    std::env::set_var("FAKE_PROVIDER_MODE", "stream");
    std::env::set_var("FAKE_PROVIDER_STREAM", &stream);
    std::env::set_var("CONCORDIA_STATE_DIR", dir.path());
    for session in [None, Some("11111111-2222-4333-8444-555555555555")] {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let req = ProviderTurnRequest {
            provider: ProviderKind::Claude,
            native_session_id: session,
            prompt: "synthetic accounting turn",
            ..req.clone()
        };
        let outcome =
            concordia::provider::turn::run_provider_turn_with_progress(&req, None, Some(tx))
                .await
                .unwrap();
        assert_eq!(outcome.exit_code, Some(0));
        let usage = std::iter::from_fn(|| rx.try_recv().ok())
            .find_map(|progress| {
                if let concordia::provider::turn::TurnProgress::Usage(usage) = progress {
                    Some(usage)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(usage.input, Some(127));
        assert_eq!(
            usage.cost_usd,
            if session.is_none() { Some(1.25) } else { None }
        );
    }
    if cfg!(windows) {
        // Exercise the actual spawn/attachment path through a stock npm wrapper. The
        // fake executable stands in for Node and records the untouched argv prefix.
        let npm = dir.path().join("npm fixture % & λ");
        let package = npm.join("node_modules/@openai/codex");
        std::fs::create_dir_all(package.join("bin")).unwrap();
        std::fs::copy(FAKE, npm.join("node.exe")).unwrap();
        let shim = npm.join("codex.cmd");
        std::fs::write(&shim, "@ECHO off\nGOTO start\n:find_dp0\nSET dp0=%~dp0\nEXIT /b\n:start\nSETLOCAL\nCALL :find_dp0\n\nIF EXIST \"%dp0%\\node.exe\" (\n  SET \"_prog=%dp0%\\node.exe\"\n) ELSE (\n  SET \"_prog=node\"\n  SET PATHEXT=%PATHEXT:;.JS;=;%\n)\n\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*").unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@openai/codex","bin":{"codex":"bin/codex.js"}}"#,
        )
        .unwrap();
        std::fs::write(
            package.join("bin/codex.js"),
            "synthetic updated bridge marker",
        )
        .unwrap();
        let png = dir.path().join("image % & λ.png");
        image::RgbaImage::new(2, 2).save(&png).unwrap();
        let image = concordia::attachments::import(dir.path(), &png).unwrap();
        let capture = dir.path().join("npm-launch.txt");
        let home = dir.path().join("fixture-home");
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("CONCORDIA_CODEX_BIN", &shim);
        std::env::set_var("FAKE_PROVIDER_OUT", &capture);
        let images = [image];
        let req = ProviderTurnRequest {
            images: &images,
            home_path: Some(&home),
            prompt: "image prompt %PATH% & \"quoted\"\nλ",
            ..req.clone()
        };
        let (tx, mut rx) = mpsc::unbounded_channel();
        let outcome = run_provider_turn(&req, Some(tx)).await.unwrap();
        assert_eq!(outcome.exit_code, Some(0));
        assert!(rx.try_recv().is_ok(), "actual child startup is reported");
        let received = std::fs::read_to_string(&capture).unwrap();
        let arg = |text: &str| {
            format!(
                "ARG:{}",
                text.bytes().map(|b| format!("{b:02x}")).collect::<String>()
            )
        };
        assert!(
            received.contains(&arg(&npm
                .join("node_modules")
                .join("@openai/codex")
                .join("bin/codex.js")
                .to_string_lossy())),
            "{received}"
        );
        assert!(received.contains(&arg("--image")), "{received}");
        assert!(
            received.contains(&arg(&images[0].path.to_string_lossy())),
            "{received}"
        );
        std::fs::remove_file(&capture).unwrap();
        std::fs::remove_file(&images[0].path).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        assert!(run_provider_turn(&req, Some(tx)).await.is_err());
        assert!(rx.try_recv().is_err(), "preflight failure reports no child");
        assert!(!capture.exists(), "invalid image never spawned a process");
        std::env::remove_var("CONCORDIA_CODEX_BIN");
        std::env::remove_var("FAKE_PROVIDER_OUT");
    }
    for key in [
        "CONCORDIA_CLAUDE_BIN",
        "FAKE_PROVIDER_MODE",
        "FAKE_PROVIDER_STREAM",
        "CONCORDIA_STATE_DIR",
    ] {
        std::env::remove_var(key);
    }
}

fn process_alive(pid: u32) -> bool {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .expect("tasklist");
    String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
}
