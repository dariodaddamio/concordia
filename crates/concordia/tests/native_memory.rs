//! CLI/MCP native-pointer readback with synthetic provider sources; no provider processes.
use concordia::{
    native_memory,
    store::{ProviderKind, StoredAccount},
};
use serde_json::{json, Value};
use std::process::Command;
#[test]
fn cli_and_mcp_read_the_same_revision_and_reject_stale_sources() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("fixture-home");
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    let session = "11111111-2222-4333-8444-555555555555";
    let path = home.join("sessions").join(format!("{session}.jsonl"));
    let raw = json!({"type":"response_item","payload":{"role":"assistant","content":[{"type":"output_text","text":"exact original  reply"}]}}).to_string() + "\n";
    std::fs::write(
        &path,
        format!(
            "{}\n{raw}",
            json!({"type":"session_meta","payload":{"id":session}})
        ),
    )
    .unwrap();
    concordia::store::upsert_stored_account(
        dir.path(),
        StoredAccount {
            id: "fixture".into(),
            provider: ProviderKind::Codex,
            home_path: Some(home.to_string_lossy().into_owned()),
            label: "Fixture".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_concordia"))
        .env("CONCORDIA_STATE_DIR", dir.path())
        .args([
            "chat",
            "index",
            session,
            "--account",
            "fixture",
            "--provider",
            "codex",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: native_memory::Summary = serde_json::from_slice(&output.stdout).unwrap();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"native_read","arguments":{"id":summary.id,"message":1,"max_bytes":4096}}});
    let read = |request: &Value| -> Value {
        serde_json::from_str(
            &concordia::mcp::handle_line(dir.path(), &request.to_string()).unwrap(),
        )
        .unwrap()
    };
    let response = read(&request);
    assert_eq!(response["result"]["isError"], false);
    let excerpt: native_memory::Excerpt =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(excerpt.raw, raw);
    let mut invalid = request.clone();
    invalid["params"]["arguments"]["offset"] = json!("0");
    assert_eq!(read(&invalid)["result"]["isError"], true);
    std::fs::write(&path, "{}").unwrap();
    assert_eq!(read(&request)["result"]["isError"], true);
}
