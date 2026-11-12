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

    // Seed the legacy format explicitly to preserve CLI read compatibility coverage.
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
    let mut events = vec![ThreadEvent::with_text("user", "say hello")];
    for c in &outcome.cards {
        events.push(ThreadEvent::card(&c.title, c.detail.as_deref(), "done"));
    }
    events.push(ThreadEvent::with_text("assistant", &outcome.text).who("codex", "codex", "m"));
    events.push(ThreadEvent::with_text("user", &"long ".repeat(5000)));
    let legacy = events
        .iter()
        .map(|ev| serde_json::to_string(ev).unwrap() + "\n")
        .collect::<String>();
    std::fs::write(
        concordia::threads::threads_dir(state.path()).join(format!("{id}.jsonl")),
        legacy,
    )
    .unwrap();

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
    assert!(m.contains("asked: say hello"));
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

    // The existing CLI migrates explicitly selected legacy rows without a provider turn.
    let home = state.path().join("migration-native-home");
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    concordia::store::upsert_stored_account(
        state.path(),
        concordia::store::StoredAccount {
            id: "codex".into(),
            provider: ProviderKind::Codex,
            home_path: Some(home.to_string_lossy().into_owned()),
            label: "Synthetic migration native source".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    let session = "00000000-0000-4000-8000-00000000007b";
    let native = [
        serde_json::json!({"type":"session_meta","payload":{"id":session}}),
        serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"say hello"}]}}),
        serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":outcome.text}]}}),
    ];
    std::fs::write(
        home.join("sessions").join(format!("{session}.jsonl")),
        native
            .iter()
            .map(|row| format!("{row}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let binding =
        concordia::native_memory::Binding::new(ProviderKind::Codex, "codex", &home, session)
            .unwrap();
    let source = concordia::native_memory::index_bound(state.path(), &binding)
        .unwrap()
        .id;
    let path = concordia::threads::threads_dir(state.path()).join(format!("{id}.jsonl"));
    let original = std::fs::read(&path).unwrap();
    let preview = thread_cmd(
        state.path(),
        &["migrate", "preview", id, "--source", &source],
    );
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let plan: serde_json::Value = serde_json::from_str(&text(&preview)).unwrap();
    assert!(plan["convertible_rows"].as_u64().unwrap() >= 1);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let plan_id = plan["plan"].as_str().unwrap();
    let applied = thread_cmd(state.path(), &["migrate", "apply", plan_id]);
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert!(text(&applied).contains("\"status\":\"applied\""));
    let again = thread_cmd(state.path(), &["migrate", "apply", plan_id]);
    assert!(again.status.success());
    assert!(text(&thread_cmd(state.path(), &["read", id, "--range", "1-1"])).contains("say hello"));
    assert_eq!(
        std::fs::read(
            state
                .path()
                .join(".ao/legacy-migrations")
                .join(plan_id)
                .join("original.jsonl")
        )
        .unwrap(),
        original
    );
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
