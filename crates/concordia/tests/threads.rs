//! Thread log, map and `concordia chat thread ...`, driven through the real binary. The conversation
//! comes from the fake provider; nothing here runs a real CLI.

use concordia::provider::{run_provider_turn, ProviderTurnRequest};
use concordia::store::{ProviderKind, TrustMode};
use concordia::threads::{ThreadEvent, ThreadLog, ThreadMeta};
use std::path::Path;
use std::process::{Command, Output};

const CONCORDIA: &str = env!("CARGO_BIN_EXE_concordia");
const FAKE: &str = env!("CARGO_BIN_EXE_concordia-fake-provider");

fn thread_cmd(state: &Path, args: &[&str]) -> Output {
    Command::new(CONCORDIA)
        .args(["chat", "thread"])
        .args(args)
        .env("CONCORDIA_STATE_DIR", state)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[tokio::test]
async fn a_recorded_conversation_can_be_listed_mapped_and_read() {
    let state = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/control-plane/codex-stream.jsonl");

    // One real turn through the runner against the fake provider.
    std::env::set_var("CONCORDIA_CODEX_BIN", FAKE);
    std::env::set_var("FAKE_PROVIDER_MODE", "stream");
    std::env::set_var("FAKE_PROVIDER_STREAM", &fixture);
    let outcome = run_provider_turn(
        &ProviderTurnRequest {
            images: &[],
            provider: ProviderKind::Codex,
            prompt: "say hello",
            cwd: work.path(),
            home_path: None,
            model: None,
            native_session_id: None,
            trust: TrustMode::Edit,
            context_prefix: None,
            local_time: None,
            compaction: Default::default(),
        },
        None,
    )
    .await
    .unwrap();
    for k in [
        "CONCORDIA_CODEX_BIN",
        "FAKE_PROVIDER_MODE",
        "FAKE_PROVIDER_STREAM",
    ] {
        std::env::remove_var(k);
    }
    assert_eq!(outcome.exit_code, Some(0));

    // Seed a current thread record for CLI read coverage.
    let id = "3f9a0000-1111-2222-3333-444444444444";
    let log = ThreadLog::open(state.path(), id);
    log.write_meta(&ThreadMeta {
        id: id.into(),
        started: concordia::threads::now_secs(),
        title: "say hello".into(),
        cwd: String::new(),
        account: Some("codex".into()),
    })
    .unwrap();
    let home = state.path().join("synthetic-codex-home");
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    concordia::store::upsert_stored_account(
        state.path(),
        concordia::store::StoredAccount {
            id: "synthetic-codex".into(),
            provider: ProviderKind::Codex,
            home_path: Some(home.to_string_lossy().into_owned()),
            label: "Synthetic Codex account".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    let session = "00000000-0000-4000-8000-00000000007b";
    let long = (0..5000)
        .map(|n| format!("long-{n} "))
        .collect::<String>();
    let native = [
        serde_json::json!({"type":"session_meta","payload":{"id":session}}),
        serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"say hello"}]}}),
        serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":outcome.text}]}}),
        serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":long}]}}),
    ];
    std::fs::write(
        home.join("sessions").join(format!("{session}.jsonl")),
        native
            .iter()
            .map(|row| format!("{row}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let binding = concordia::native_memory::Binding::new(
        ProviderKind::Codex,
        "synthetic-codex",
        &home,
        session,
    )
    .unwrap();
    let source = concordia::native_memory::index_bound(state.path(), &binding).unwrap();
    let scope = concordia::native_memory::references::historical_scope(state.path(), &source.id)
        .unwrap();
    let events = [
        ThreadEvent::with_text("user", "say hello").who("codex", "synthetic-codex", "m"),
        ThreadEvent::with_text("assistant", &outcome.text)
            .who("codex", "synthetic-codex", "m"),
        ThreadEvent::with_text("user", &long).who("codex", "synthetic-codex", "m"),
    ];
    for event in &events {
        let role = event.k.as_str();
        let body = event.text.as_deref().unwrap();
        let reference = concordia::native_memory::references::capture_text(
            state.path(),
            &scope,
            role,
            body,
        )
        .unwrap();
        log.append_referenced(event, reference).unwrap();
    }

    let list = thread_cmd(state.path(), &["list"]);
    assert!(list.status.success());
    assert!(text(&list).contains(id) && text(&list).contains("say hello"));
    let json = thread_cmd(state.path(), &["list", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&text(&json)).unwrap();
    assert_eq!(v[0]["id"], id);

    let map = thread_cmd(state.path(), &["map", "3f9a"]);
    assert!(
        map.status.success(),
        "{}",
        String::from_utf8_lossy(&map.stderr)
    );
    let m = text(&map);
    assert!(m.starts_with("[concordia map] thread 3f9a0000"));
    assert!(m.contains("goal: say hello"));
    assert!(m.contains(&format!("concordia chat thread read {id}")));
    assert!(m.chars().count() <= 1601, "{}", m.chars().count());

    let read = thread_cmd(
        state.path(),
        &["read", "3f9a", "--tail", "1", "--max-chars", "300"],
    );
    let r = text(&read);
    assert!(read.status.success());
    assert!(r.chars().count() <= 500, "{}", r.chars().count());
    assert!(r.contains("[truncated"));
    let grep = thread_cmd(state.path(), &["read", id, "--grep", "SAY HELLO"]);
    assert!(text(&grep).contains("#1 user"));
    let range = thread_cmd(state.path(), &["read", id, "--range", "1-1"]);
    assert_eq!(text(&range).lines().count(), 1);

}

#[test]
fn errors_use_exit_codes_1_and_2() {
    let state = tempfile::tempdir().unwrap();
    let a = ThreadLog::open(state.path(), "abc1");
    let b = ThreadLog::open(state.path(), "abc2");
    // Only distinct log ids are needed for prefix resolution; body-free metadata is sufficient.
    a.append(&ThreadEvent::new("note")).unwrap();
    b.append(&ThreadEvent::new("note")).unwrap();
    assert_eq!(
        thread_cmd(state.path(), &["map", "nope"]).status.code(),
        Some(1)
    );
    let amb = thread_cmd(state.path(), &["read", "abc"]);
    assert_eq!(amb.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&amb.stderr).contains("ambiguous"));
    assert_eq!(thread_cmd(state.path(), &[]).status.code(), Some(2));
    assert_eq!(thread_cmd(state.path(), &["bogus"]).status.code(), Some(2));
    assert_eq!(
        thread_cmd(state.path(), &["read", "abc1", "--range", "9-2"])
            .status
            .code(),
        Some(2)
    );
    // Empty state lists cleanly.
    let empty = tempfile::tempdir().unwrap();
    assert!(thread_cmd(empty.path(), &["list"]).status.success());
}
