//! Bounded transport and command stress checks. No provider or network calls.
use concordia::mcp;
use serde_json::{json, Value};
use std::io::{BufReader, Cursor};

fn token_entry() -> concordia::token_log::Entry {
    concordia::token_log::Entry {
        at: 1,
        provider: "codex".into(),
        account: "synthetic".into(),
        model: None,
        project: "fixture".repeat(300),
        chat: None,
        new_input: 1,
        cached_input: 2,
        output: 3,
        cost_usd: None,
        attribution: Some(concordia::token_log::Attribution::new(
            concordia::token_log::Origin::Main,
            &uuid::Uuid::new_v4().to_string(),
            &uuid::Uuid::new_v4().to_string(),
            Some(std::path::Path::new("fixture")),
            &concordia::provider::stream_cards::TurnUsage {
                input: Some(3),
                cached: Some(2),
                output: Some(3),
                cost_usd: None,
            },
            "succeeded",
            true,
        )),
    }
}

#[test]
fn concurrent_token_writers_never_interleave_or_lose_records() {
    let root = tempfile::tempdir().unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
    std::thread::scope(|scope| {
        for _ in 0..16 {
            let barrier = barrier.clone();
            let path = root.path();
            scope.spawn(move || {
                barrier.wait();
                for _ in 0..100 {
                    concordia::token_log::record(path, &token_entry()).unwrap();
                }
            });
        }
    });
    let entries = concordia::token_log::load(root.path());
    assert_eq!(entries.len(), 1600);
    assert_eq!(concordia::token_log::totals(&entries).total(), 9600);
}

#[test]
fn token_totals_cannot_overflow_on_provider_or_persisted_integer_extremes() {
    let mut entry = token_entry();
    entry.new_input = u64::MAX;
    entry.cached_input = u64::MAX;
    entry.output = u64::MAX;
    assert_eq!(entry.total(), u64::MAX);
    let totals = concordia::token_log::totals([&entry, &entry]);
    assert_eq!(totals.total(), u64::MAX);
    assert_eq!(totals.new_input, u64::MAX);
}

#[test]
fn an_immutable_event_does_not_need_a_shared_append_lock() {
    let root = tempfile::tempdir().unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.path().join("token-events.lock"))
        .unwrap();
    lock.lock().unwrap();
    let start = std::time::Instant::now();
    concordia::token_log::record(root.path(), &token_entry()).unwrap();
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(concordia::token_log::load(root.path()).len(), 1);
    drop(lock);
    concordia::token_log::record(root.path(), &token_entry()).unwrap();
    assert_eq!(concordia::token_log::load(root.path()).len(), 2);
}

#[test]
#[ignore = "subprocess helper; invoked by the concurrent-process test"]
fn token_writer_child() {
    let Some(root) = std::env::var_os("CONCORDIA_RELIABILITY_CHILD_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    assert!(root.join("reliability-owner").is_file());
    if let Ok(run) = std::env::var("CONCORDIA_RELIABILITY_SCOPE_RUN") {
        let conversation = std::env::var("CONCORDIA_RELIABILITY_SCOPE_CONVERSATION").unwrap();
        let attempt = std::env::var("CONCORDIA_RELIABILITY_SCOPE_ATTEMPT").unwrap();
        let request = concordia::workspaces::Request::shared(&root.join("workspace"), &[]).unwrap();
        let admission =
            concordia::workspaces::reserve(&root, &request, &run, &conversation, &attempt).unwrap();
        let result: &[u8] = match admission {
            concordia::workspaces::Admission::Reserved => b"reserved",
            concordia::workspaces::Admission::Conflict { .. } => b"conflict",
            other => panic!("unexpected reservation result: {other:?}"),
        };
        std::fs::write(root.join(format!("scope-result-{run}")), result).unwrap();
        return;
    }
    for _ in 0..100 {
        concordia::token_log::record(&root, &token_entry()).unwrap();
    }
}

#[test]
fn independent_processes_can_append_tokens_without_losing_records() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("reliability-owner"), b"synthetic fixture").unwrap();
    let mut children = Vec::new();
    for _ in 0..8 {
        children.push(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "token_writer_child"])
                .env("CONCORDIA_RELIABILITY_CHILD_ROOT", root.path())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    for child in &mut children {
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("writer child timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    for child in children {
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(concordia::token_log::load(root.path()).len(), 800);
    std::fs::create_dir(root.path().join("workspace")).unwrap();
    let conversation = uuid::Uuid::new_v4().to_string();
    let mut children = Vec::new();
    let mut runs = Vec::new();
    for _ in 0..2 {
        let run = uuid::Uuid::new_v4().to_string();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "token_writer_child"])
            .env("CONCORDIA_RELIABILITY_CHILD_ROOT", root.path())
            .env("CONCORDIA_RELIABILITY_SCOPE_RUN", &run)
            .env("CONCORDIA_RELIABILITY_SCOPE_CONVERSATION", &conversation)
            .env(
                "CONCORDIA_RELIABILITY_SCOPE_ATTEMPT",
                uuid::Uuid::new_v4().to_string(),
            )
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        runs.push(run);
        children.push(child);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    for child in &mut children {
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("workspace reservation child timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    for child in children {
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let results: Vec<_> = runs
        .iter()
        .map(|run| std::fs::read(root.path().join(format!("scope-result-{run}"))).unwrap())
        .collect();
    assert_eq!(
        results
            .iter()
            .filter(|result| result.as_slice() == b"reserved")
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| result.as_slice() == b"conflict")
            .count(),
        1
    );
    let registry = concordia::workspaces::load(root.path()).unwrap();
    assert_eq!(
        registry.reservations.len(),
        1,
        "conflicts do not create a second reservation"
    );
}

#[test]
fn mcp_recovers_after_oversized_invalid_utf8_and_deep_json_messages() {
    let root = tempfile::tempdir().unwrap();
    let mut input = vec![b'x'; mcp::MAX_REQUEST_BYTES * 16];
    input.extend_from_slice(b"\n\xff\xfe\n");
    input.extend_from_slice("[".repeat(300).as_bytes());
    input.extend_from_slice("]".repeat(300).as_bytes());
    input.extend_from_slice(b"\n\n");
    input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n");
    for i in 0..1000 {
        input.extend_from_slice(
            format!("{{\"jsonrpc\":\"2.0\",\"id\":{i},\"method\":\"ping\"}}\n").as_bytes(),
        );
    }
    let mut output = Vec::new();
    mcp::serve(
        BufReader::with_capacity(7, Cursor::new(input)),
        &mut output,
        root.path(),
    )
    .unwrap();
    let responses: Vec<Value> = output
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 1001);
    assert_eq!(responses[0]["error"]["code"], -32600);
    for (i, response) in responses[1..].iter().enumerate() {
        assert_eq!(response["id"], i);
        assert_eq!(response["result"], json!({}));
    }
    assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
}

#[test]
fn malformed_mcp_calls_never_write_proposals_or_panic() {
    let root = tempfile::tempdir().unwrap();
    for arguments in [
        json!(null),
        json!(false),
        json!(7),
        json!([]),
        json!("bad"),
        json!({"to":null,"task":[]}),
        json!({"to":"../outside","task":"synthetic"}),
    ] {
        for name in [
            "threads",
            "thread_map",
            "thread_read",
            "usage",
            "delegate",
            "missing",
        ] {
            let line = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name":name,"arguments":arguments}})
            .to_string();
            let result: Value =
                serde_json::from_str(&mcp::handle_line(root.path(), &line).unwrap()).unwrap();
            assert_eq!(result["id"], 1);
            assert!(result.get("error").is_some() || result.get("result").is_some());
        }
    }
    assert!(!mcp::proposals_dir(root.path()).exists());
}

#[test]
fn slash_argument_fuzz_never_turns_invalid_controls_into_actions() {
    use concordia::commands::{parse_slash, SlashCommand};
    for command in [
        "new", "resume", "copy", "doctor", "exit", "version", "stats",
    ] {
        for argument in [
            "extra",
            "\u{65e5}",
            "\"quoted\"",
            "&& echo unsafe",
            "../outside",
            "\u{1f600}",
        ] {
            for whitespace in [" ", "\t", "\n", "\r\n", "\u{2003}"] {
                let line = format!("/{command}{whitespace}{argument}");
                assert!(
                    matches!(parse_slash(&line), Some((SlashCommand::Invalid(_), _))),
                    "{line:?}"
                );
            }
        }
    }
}
