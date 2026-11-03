//! An opt-in local transport probe. Fake processes only; not an inference benchmark.
use concordia::provider::{run_provider_turn, ProviderTurnRequest};
use concordia::store::{ProviderKind, TrustMode};
use std::time::Instant;

#[tokio::test]
#[ignore = "local direct-process vs wrapper timing probe; no real provider"]
async fn fake_process_transport_latency() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = dir.path().join("stream.jsonl");
    std::fs::write(&fixture, concat!(
        "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"id\":\"reply\",\"text\":\"probe\"}}\n",
        "{\"type\":\"turn.completed\"}\n"
    )).unwrap();
    let fake = env!("CARGO_BIN_EXE_concordia-fake-provider");
    let values = [
        ("CONCORDIA_CODEX_BIN", std::ffi::OsString::from(fake)),
        ("CONCORDIA_STATE_DIR", dir.path().as_os_str().to_owned()),
        ("FAKE_PROVIDER_MODE", "stream".into()),
        ("FAKE_PROVIDER_STREAM", fixture.as_os_str().to_owned()),
    ];
    struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }
    let _restore = Restore(
        values
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect(),
    );
    for (key, value) in values {
        std::env::set_var(key, value);
    }
    let req = ProviderTurnRequest {
        provider: ProviderKind::Codex,
        images: &[],
        prompt: "probe",
        cwd: dir.path(),
        home_path: Some(dir.path()),
        model: None,
        native_session_id: None,
        trust: TrustMode::Workspace,
        context_prefix: None,
        local_time: None,
        compaction: Default::default(),
    };
    let mut direct = Vec::new();
    let mut wrapped = Vec::new();
    // Alternate measurements so neither path gets every warm-cache advantage.
    for _ in 0..12 {
        let start = Instant::now();
        let mut cmd = tokio::process::Command::new(fake);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.as_std_mut().creation_flags(0x0800_0000);
        }
        let output = cmd
            .args(["exec", "--json", "probe"])
            .current_dir(dir.path())
            .stdin(std::process::Stdio::null())
            .output()
            .await
            .unwrap();
        direct.push(start.elapsed().as_micros());
        assert!(output.status.success());
        let start = Instant::now();
        let output = run_provider_turn(&req, None).await.unwrap();
        wrapped.push(start.elapsed().as_micros());
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(output.text.trim(), "probe");
    }
    direct.sort_unstable();
    wrapped.sort_unstable();
    eprintln!("FAKE ONLY: direct median={}us, wrapper median={}us, p90={}us/{}us; no native provider/model latency measured", direct[6], wrapped[6], direct[10], wrapped[10]);
}
