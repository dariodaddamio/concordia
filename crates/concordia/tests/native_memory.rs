//! CLI/MCP native-pointer readback with synthetic provider sources; no provider processes.
use concordia::{
    native_memory,
    store::{ProviderKind, StoredAccount},
};
use serde_json::{json, Value};
use std::process::Command;

#[test]
fn cursor_history_is_native_read_only_account_bound_and_revision_checked() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("cursor-home");
    let workspace = dir.path().join("workspace");
    let session = "11111111-2222-4333-8444-555555555555";
    let folder = home.join("chats/project").join(session);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(
        folder.join("meta.json"),
        json!({"cwd":workspace,"title":"Synthetic history"}).to_string(),
    )
    .unwrap();
    concordia::store::upsert_stored_account(
        dir.path(),
        StoredAccount {
            id: "cursor-fixture".into(),
            provider: ProviderKind::Cursor,
            home_path: Some(home.to_string_lossy().into_owned()),
            label: "Cursor fixture".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    let fixture_path = workspace.join("acp-fixture.json");
    let mut fixture = json!({"history":[{"id":"u1","role":"user","text":"exact  original\n雪"},{"id":"a1","role":"assistant","text":"original answer"}]});
    std::fs::write(&fixture_path, fixture.to_string()).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_concordia"))
            .env("CONCORDIA_STATE_DIR", dir.path())
            .env(
                "CONCORDIA_CODEX_HOME_BASE",
                dir.path().join("empty-profile"),
            )
            .env(
                "CONCORDIA_AGENT_BIN",
                env!("CARGO_BIN_EXE_concordia-fake-provider"),
            )
            .env("CURSOR_API_KEY", "synthetic-parent-key")
            .env("CURSOR_AUTH_TOKEN", "synthetic-parent-token")
            .args(args)
            .output()
            .unwrap()
    };
    let indexed = run(&[
        "chat",
        "index",
        session,
        "--account",
        "cursor-fixture",
        "--provider",
        "cursor",
        "--scope",
        "whole",
    ]);
    assert!(
        indexed.status.success(),
        "{}",
        String::from_utf8_lossy(&indexed.stderr)
    );
    let summary: native_memory::Summary = serde_json::from_slice(&indexed.stdout).unwrap();
    assert_eq!(summary.messages, 2);
    assert_eq!(summary.native_protocol.as_deref(), Some("acp/session.load"));
    assert!(summary.history.as_ref().unwrap().complete);
    let read = run(&[
        "chat",
        "excerpt",
        &summary.id,
        "--message",
        "1",
        "--max-bytes",
        "4096",
    ]);
    assert!(
        read.status.success(),
        "{}",
        String::from_utf8_lossy(&read.stderr)
    );
    let excerpt: native_memory::Excerpt = serde_json::from_slice(&read.stdout).unwrap();
    let original: Value = serde_json::from_str(&excerpt.raw).unwrap();
    assert_eq!(
        original["params"]["update"]["content"]["text"],
        "exact  original\n雪"
    );
    let env: Value =
        serde_json::from_slice(&std::fs::read(workspace.join("acp-environment.json")).unwrap())
            .unwrap();
    assert_eq!(
        std::fs::canonicalize(env["config_dir"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(&home).unwrap()
    );
    assert_eq!(env["api_key_present"], false);
    assert_eq!(env["auth_token_present"], false);
    let display = run(&[
        "chat",
        "read",
        session,
        "--account",
        "cursor-fixture",
        "--provider",
        "cursor",
    ]);
    assert!(
        display.status.success(),
        "{}",
        String::from_utf8_lossy(&display.stderr)
    );
    let display = String::from_utf8(display.stdout).unwrap();
    assert!(display.contains("user: exact  original\n雪"));
    assert!(display.contains("assistant: original answer"));
    let calls = std::fs::read_to_string(workspace.join("acp-calls.jsonl")).unwrap();
    for call in calls
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
    {
        assert!(matches!(
            call["method"].as_str(),
            Some("initialize" | "session/load")
        ));
    }
    let stored = std::fs::read_to_string(
        dir.path()
            .join(".ao/native-memory")
            .join(format!("{}.json", summary.id)),
    )
    .unwrap();
    assert!(!stored.contains("original answer"));
    fixture["history"][0]["text"] = json!("rewritten history");
    std::fs::write(&fixture_path, fixture.to_string()).unwrap();
    assert!(!run(&["chat", "excerpt", &summary.id, "--message", "1"])
        .status
        .success());
    fixture["load_session"] = json!(false);
    std::fs::write(&fixture_path, fixture.to_string()).unwrap();
    assert!(!run(&[
        "chat",
        "index",
        session,
        "--account",
        "cursor-fixture",
        "--provider",
        "cursor"
    ])
    .status
    .success());
    fixture["load_session"] = json!(true);
    fixture["wrong_history_session"] = json!(true);
    std::fs::write(&fixture_path, fixture.to_string()).unwrap();
    assert!(!run(&[
        "chat",
        "index",
        session,
        "--account",
        "cursor-fixture",
        "--provider",
        "cursor"
    ])
    .status
    .success());
    fixture["wrong_history_session"] = json!(false);
    fixture["history_tool_request"] = json!(true);
    std::fs::write(&fixture_path, fixture.to_string()).unwrap();
    assert!(!run(&[
        "chat",
        "index",
        session,
        "--account",
        "cursor-fixture",
        "--provider",
        "cursor"
    ])
    .status
    .success());
}
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
    let history = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"native_read","arguments":{"id":summary.id,"history":true}}});
    let history_response = read(&history);
    assert_eq!(history_response["result"]["isError"], false);
    let whole: native_memory::Summary = serde_json::from_str(
        history_response["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(whole.history.as_ref().unwrap().complete);
    assert_eq!(
        native_memory::read(dir.path(), &whole.id, 1, 0, 4096)
            .unwrap()
            .raw,
        raw
    );
    let listed = Command::new(env!("CARGO_BIN_EXE_concordia"))
        .env("CONCORDIA_STATE_DIR", dir.path())
        .args([
            "chat",
            "sources",
            &whole.id,
            "--from-message",
            "1",
            "--limit",
            "1",
        ])
        .output()
        .unwrap();
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let listed: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(listed["messages"][0]["message"], 1);
    assert_eq!(listed["messages"].as_array().unwrap().len(), 1);
    let mut invalid = request.clone();
    invalid["params"]["arguments"]["offset"] = json!("0");
    assert_eq!(read(&invalid)["result"]["isError"], true);

    // Goal associations share the old goal store and never copy native conversation bodies.
    let goals: Vec<_> = (0..12)
        .map(|index| concordia::goals::Goal {
            id: format!("g-{index}"),
            text: format!("Goal fixture {index}"),
            done: false,
            added_at: "now".into(),
            closed_at: None,
        })
        .collect();
    concordia::goals::save_goals(dir.path(), &goals).unwrap();
    let goal_bytes = std::fs::read(dir.path().join("goals.json")).unwrap();
    let relation =
        native_memory::relations::attach(dir.path(), dir.path(), "g-0", &summary.id).unwrap();
    let relation_path = dir.path().join(".ao/native-memory/relations.json");
    let relation_bytes = std::fs::read(&relation_path).unwrap();
    assert_eq!(
        native_memory::relations::attach(dir.path(), dir.path(), "g-0", &summary.id).unwrap(),
        relation
    );
    assert_eq!(std::fs::read(&relation_path).unwrap(), relation_bytes);
    assert_eq!(
        std::fs::read(dir.path().join("goals.json")).unwrap(),
        goal_bytes
    );
    assert!(!String::from_utf8_lossy(&relation_bytes).contains("exact original"));
    assert!(!String::from_utf8_lossy(&relation_bytes).contains("Goal fixture"));
    std::thread::scope(|scope| {
        for goal in &goals[1..] {
            let id = &summary.id;
            let root = dir.path();
            scope
                .spawn(move || native_memory::relations::attach(root, root, &goal.id, id).unwrap());
        }
    });
    let page = native_memory::relations::list(dir.path(), Some(dir.path()), None, 0, None).unwrap();
    assert_eq!(page["relations"].as_array().unwrap().len(), 10);
    assert_eq!(page["next_offset"], 10);
    let revision = page["revision"].as_str().unwrap();
    let tail =
        native_memory::relations::list(dir.path(), Some(dir.path()), None, 10, Some(revision))
            .unwrap();
    assert_eq!(tail["relations"].as_array().unwrap().len(), 2);
    assert!(native_memory::relations::list(dir.path(), Some(dir.path()), None, 10, None).is_err());
    native_memory::relations::detach(dir.path(), dir.path(), "g-0", &summary.id).unwrap();
    assert!(
        native_memory::relations::list(dir.path(), Some(dir.path()), None, 10, Some(revision))
            .is_err()
    );
    assert!(std::fs::read_to_string(&relation_path)
        .unwrap()
        .contains(&relation));
    assert_eq!(
        std::fs::read(dir.path().join("goals.json")).unwrap(),
        goal_bytes
    );
    let goal_request = json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"goals","arguments":{}}});
    assert_eq!(read(&goal_request)["result"]["isError"], false);
    let mut invalid_goal = goal_request.clone();
    invalid_goal["params"]["arguments"]["offset"] = json!(1);
    assert_eq!(read(&invalid_goal)["result"]["isError"], true);
    let cli_goals = Command::new(env!("CARGO_BIN_EXE_concordia"))
        .env("CONCORDIA_STATE_DIR", dir.path())
        .current_dir(dir.path())
        .args(["chat", "goal-sources", "g-1"])
        .output()
        .unwrap();
    assert!(
        cli_goals.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_goals.stderr)
    );
    assert!(String::from_utf8_lossy(&cli_goals.stdout).contains(&summary.id));
    let second_home = dir.path().join("second-fixture-home");
    std::fs::create_dir_all(second_home.join("sessions")).unwrap();
    std::fs::copy(
        &path,
        second_home
            .join("sessions")
            .join(format!("{session}.jsonl")),
    )
    .unwrap();
    concordia::store::upsert_stored_account(
        dir.path(),
        StoredAccount {
            id: "second-fixture".into(),
            provider: ProviderKind::Codex,
            home_path: Some(second_home.display().to_string()),
            label: "Second fixture".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    let second =
        native_memory::index_session(dir.path(), ProviderKind::Codex, "second-fixture", session)
            .unwrap();
    assert_ne!(
        second.id, summary.id,
        "equal native session ids in different homes must remain distinct"
    );
    native_memory::relations::attach(dir.path(), dir.path(), "g-1", &second.id).unwrap();
    let paired =
        native_memory::relations::list(dir.path(), Some(dir.path()), Some("g-1"), 0, None).unwrap();
    assert_eq!(paired["relations"].as_array().unwrap().len(), 2);
    native_memory::relations::detach(dir.path(), dir.path(), "g-1", &summary.id).unwrap();
    let retained =
        native_memory::relations::list(dir.path(), Some(dir.path()), Some("g-1"), 0, None).unwrap();
    assert_eq!(retained["relations"][0]["source"], second.id);
    let before_corruption = std::fs::read(&relation_path).unwrap();
    std::fs::write(
        &relation_path,
        br#"{"version":99,"relations":[],"changes":[]}"#,
    )
    .unwrap();
    let corrupt_bytes = std::fs::read(&relation_path).unwrap();
    assert!(native_memory::relations::attach(dir.path(), dir.path(), "g-1", &summary.id).is_err());
    assert_eq!(std::fs::read(&relation_path).unwrap(), corrupt_bytes);
    std::fs::write(&relation_path, &before_corruption).unwrap();
    std::fs::write(&path, "{}").unwrap();
    assert_eq!(read(&request)["result"]["isError"], true);
    assert!(native_memory::relations::attach(dir.path(), dir.path(), "g-0", &summary.id).is_err());
    assert_eq!(std::fs::read(&relation_path).unwrap(), before_corruption);
}
