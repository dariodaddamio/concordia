//! Real process transport with synthetic image bytes; never invokes a real provider.
use base64::Engine as _;
use concordia::{
    attachments,
    provider::{
        self,
        turn::{ProviderTurnRequest, TurnProgress},
    },
    store::{ProviderKind, TrustMode},
};
use serde_json::{json, Value};
const FAKE: &str = env!("CARGO_BIN_EXE_concordia-fake-provider");
struct Env(Vec<(String, Option<std::ffi::OsString>)>);
impl Env {
    fn set(&mut self, name: &str, value: impl AsRef<std::ffi::OsStr>) {
        self.0.push((name.into(), std::env::var_os(name)));
        std::env::set_var(name, value);
    }
}
impl Drop for Env {
    fn drop(&mut self) {
        for (name, value) in self.0.drain(..).rev() {
            if let Some(value) = value {
                std::env::set_var(name, value)
            } else {
                std::env::remove_var(name)
            }
        }
    }
}

#[tokio::test]
async fn all_three_image_transports_preserve_pixels_and_cursor_negotiates_and_answers_native_permissions(
) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let path = root.join("sample.png");
    std::fs::write(&path, b"\x89PNG\r\n\x1a\nsynthetic image pixels").unwrap();
    let image = attachments::import(root, &path).unwrap();
    let document_path = root.join("notes.md");
    std::fs::write(&document_path, "Attached document bytes\r\n").unwrap();
    let document = attachments::Document::import(root, &document_path).unwrap();
    let payload = attachments::Message {
        text: "Inspect these pixels".into(),
        attachments: vec![
            attachments::Attachment::Image(image.clone()),
            attachments::Attachment::TextDocument(document),
        ],
        ..Default::default()
    }
    .prompt_text()
    .unwrap();
    let images = vec![image.clone()];
    let mut env = Env(vec![]);
    for variable in [
        "CONCORDIA_CODEX_BIN",
        "CONCORDIA_CLAUDE_BIN",
        "CONCORDIA_AGENT_BIN",
    ] {
        env.set(variable, FAKE);
    }
    env.set("CONCORDIA_STATE_DIR", root.join("state"));
    env.set("FAKE_PROVIDER_OUT", root.join("launch.txt"));
    env.set("FAKE_PROVIDER_READ_STDIN", "1");
    let stream = root.join("stream.jsonl");
    std::fs::write(&stream, "{\"type\":\"turn.completed\"}\n").unwrap();
    env.set("FAKE_PROVIDER_STREAM", &stream);
    for kind in [ProviderKind::Codex, ProviderKind::Claude] {
        let request = ProviderTurnRequest {
            images: &images,
            provider: kind,
            prompt: &payload,
            cwd: root,
            home_path: Some(root),
            model: None,
            native_session_id: None,
            trust: TrustMode::Plan,
            context_prefix: None,
            local_time: None,
            compaction: Default::default(),
        };
        let outcome = provider::run_provider_turn(&request, None).await.unwrap();
        assert_eq!(outcome.exit_code, Some(0));
        let journal = std::fs::read_to_string(root.join("launch.txt")).unwrap();
        let hex = |s: &str| {
            s.as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        if kind == ProviderKind::Codex {
            assert!(journal.contains(&format!("ARG:{}", hex(&payload))));
            assert!(journal.contains(&format!("ARG:{}", hex("--image"))));
            assert!(journal.contains(&hex(&image.path.to_string_lossy())));
        } else {
            let raw = journal
                .lines()
                .find_map(|l| l.strip_prefix("STDIN:"))
                .unwrap();
            let bytes: Vec<u8> = (0..raw.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&raw[i..i + 2], 16).unwrap())
                .collect();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["message"]["content"][0]["text"], payload);
            assert_eq!(
                value["message"]["content"][1]["source"]["data"],
                image.data().unwrap()
            );
            assert!(!journal.contains(&format!("ARG:{}", hex("Inspect these pixels"))));
        }
    }
    for (fixture, expected_failed) in [
        (json!({"permission":true}), false),
        (json!({"image":false}), true),
        (json!({"quota":true}), true),
    ] {
        let work = tempfile::tempdir().unwrap();
        std::fs::write(work.path().join("acp-fixture.json"), fixture.to_string()).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let answers = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let TurnProgress::Interaction(request) = event {
                    request.respond(json!({"outcome":{"outcome":"selected","optionId":"yes"}}));
                }
            }
        });
        let request = ProviderTurnRequest {
            images: &images,
            provider: ProviderKind::Cursor,
            prompt: &payload,
            cwd: work.path(),
            home_path: Some(root),
            model: Some("auto"),
            native_session_id: Some("exact-image-session"),
            trust: TrustMode::Plan,
            context_prefix: None,
            local_time: None,
            compaction: Default::default(),
        };
        let out = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            provider::run_provider_turn_with_progress(&request, None, Some(tx)),
        )
        .await
        .unwrap()
        .unwrap();
        answers.await.unwrap();
        assert_eq!(out.failed, expected_failed, "{out:?}");
        let calls = std::fs::read_to_string(work.path().join("acp-calls.jsonl")).unwrap();
        let values: Vec<Value> = calls
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        if fixture["image"] == false {
            assert!(!calls.contains("session/prompt"));
        } else {
            let prompt = values
                .iter()
                .find(|v| v["method"] == "session/prompt")
                .unwrap();
            assert_eq!(prompt["params"]["prompt"][1]["data"], image.data().unwrap());
            assert_eq!(prompt["params"]["prompt"][0]["text"], payload);
            assert!(values.iter().any(|v| v["method"] == "session/load"
                && v["params"]["sessionId"] == "exact-image-session"));
            assert!(values
                .iter()
                .any(|v| v["method"] == "session/set_mode" && v["params"]["modeId"] == "plan"));
            if !expected_failed {
                assert_eq!(out.text, "Native image received");
                assert!(values
                    .iter()
                    .any(|v| v["id"] == 100 && v["result"]["outcome"]["optionId"] == "yes"));
            }
        }
    }
    env.set("FAKE_PROVIDER_NATIVE_CODEX", "1");
    for fixture in [
        json!({"artifact":true}),
        json!({"question":true}),
        json!({"question":true,"freeform":true}),
        json!({"resolved":true}),
        json!({"wrong_thread":true}),
        json!({"wrong_turn":true}),
        json!({"closed":true}),
    ] {
        use concordia::provider::interaction::InteractionOwner;
        let work = tempfile::tempdir().unwrap();
        std::fs::write(
            work.path().join("appserver-fixture.json"),
            fixture.to_string(),
        )
        .unwrap();
        let owner = InteractionOwner {
            conversation_id: uuid::Uuid::new_v4().to_string(),
            run_id: uuid::Uuid::new_v4().to_string(),
            attempt_id: uuid::Uuid::new_v4().to_string(),
            workspace: work.path().to_owned(),
            execution_cwd: work.path().to_owned(),
            provider: ProviderKind::Codex,
            account: "fixture-codex".into(),
            home: Some(root.to_owned()),
        };
        let owner_for_answers = owner
            .validate_for(ProviderKind::Codex, work.path(), Some(root))
            .unwrap();
        let resolved = fixture["resolved"] == true;
        let artifact = fixture["artifact"] == true;
        let conversation = owner.conversation_id.clone();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let answers = tokio::spawn(async move {
            let mut requests = Vec::new();
            let mut outputs = Vec::new();
            while let Some(event) = rx.recv().await {
                if let TurnProgress::Artifact(record) = &event {
                    assert_eq!(record.owner, owner_for_answers);
                    assert_eq!(record.native_session, "exact-native-thread");
                    assert_eq!(record.native_turn.as_deref(), Some("native-turn"));
                    assert_eq!(record.event_id, "native-output-event");
                    assert!(record.open_current().unwrap().ends_with("native-output.md"));
                    outputs.push(record.id.clone());
                }
                if let TurnProgress::Interaction(request) = event {
                    assert_eq!(
                        request.envelope().native_turn_id.as_deref(),
                        Some("native-turn")
                    );
                    if !resolved {
                        let choice = if request.method.contains("requestUserInput") {
                            0
                        } else {
                            2
                        };
                        request
                            .respond_owned(
                                &owner_for_answers,
                                if request.allows_text(0) {
                                    request
                                        .choices_with_text(
                                            &[0],
                                            &[Some("Synthetic free reply".into())],
                                        )
                                        .unwrap()
                                } else {
                                    request.single_choices(&[choice]).unwrap()
                                },
                            )
                            .unwrap();
                    }
                    requests.push(request);
                }
            }
            if resolved {
                assert!(requests.iter().all(|request| request.was_resolved()
                    && !request.is_active()
                    && !request.was_answered()));
            }
            assert_eq!(
                outputs.len(),
                usize::from(artifact),
                "repeated native events must deduplicate"
            );
            requests.len()
        });
        let request = ProviderTurnRequest {
            images: &images,
            provider: ProviderKind::Codex,
            prompt: "Inspect native fixture",
            cwd: work.path(),
            home_path: Some(root),
            model: Some("fixture-model"),
            native_session_id: Some("exact-native-thread"),
            trust: TrustMode::Workspace,
            context_prefix: None,
            local_time: None,
            compaction: Default::default(),
        };
        let out = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            provider::turn::run_provider_turn_with_transport(
                &request,
                owner,
                concordia::native_input::Mode::CodexAppServer,
                None,
                Some(tx),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        let offered = answers.await.unwrap();
        let records = concordia::artifacts::read(&root.join("state"), &conversation).unwrap();
        assert_eq!(records.len(), usize::from(artifact));
        let expected_failed = fixture["wrong_thread"] == true || fixture["closed"] == true;
        assert_eq!(out.failed, expected_failed, "{fixture}: {out:?}");
        if !expected_failed {
            assert_eq!(out.text, "Native approval handled");
        }
        let calls = std::fs::read_to_string(work.path().join("appserver-calls.jsonl")).unwrap();
        let values: Vec<Value> = calls
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        if fixture["wrong_thread"] == true {
            assert!(!calls.contains("turn/start"));
            assert_eq!(offered, 0);
            continue;
        }
        let turn = values
            .iter()
            .find(|value| value["method"] == "turn/start")
            .unwrap();
        assert_eq!(turn["params"]["threadId"], "exact-native-thread");
        assert_eq!(turn["params"]["sandboxPolicy"]["type"], "workspaceWrite");
        assert_eq!(
            turn["params"]["input"][1]["url"],
            format!("data:image/png;base64,{}", image.data().unwrap())
        );
        if fixture["wrong_turn"] == true {
            assert_eq!(offered, 0);
            assert!(values
                .iter()
                .any(|value| value["id"] == 100 && value.get("error").is_some()));
        } else if fixture["resolved"] == true {
            assert!(!values.iter().any(|value| value["id"] == 100));
        } else if fixture["closed"] != true {
            assert_eq!(offered, 1);
            let answer = values.iter().find(|value| value["id"] == 100).unwrap();
            if fixture["freeform"] == true {
                assert_eq!(
                    answer["result"],
                    json!({"answers":{"route":{"answers":["user_note: Synthetic free reply"]}}})
                );
            } else if fixture["question"] == true {
                assert_eq!(
                    answer["result"],
                    json!({"answers":{"route":{"answers":["Direct"]}}})
                );
            } else {
                assert_eq!(answer["result"], json!({"decision":"decline"}));
            }
        }
    }

    // Cursor reports successful file paths on completed structured events. Exercise the real
    // stream runner and the shared catalog, including duplicate and foreign/failed reports.
    env.set("CONCORDIA_CURSOR_BIN", FAKE);
    for (reported_session, failed, expected) in [
        ("cursor-output-session", false, 1),
        ("foreign-session", false, 0),
        ("", false, 0),
        ("cursor-output-session", true, 0),
    ] {
        let work = tempfile::tempdir().unwrap();
        std::fs::write(work.path().join("output.md"), "Synthetic output\n").unwrap();
        let conversation = uuid::Uuid::new_v4().to_string();
        let owner = provider::interaction::InteractionOwner {
            conversation_id: conversation.clone(),
            workspace: work.path().into(),
            execution_cwd: work.path().into(),
            run_id: uuid::Uuid::new_v4().to_string(),
            attempt_id: uuid::Uuid::new_v4().to_string(),
            provider: ProviderKind::Cursor,
            account: "cursor-fixture".into(),
            home: Some(root.into()),
        };
        let mut event = json!({"type":"tool_call","subtype":"completed","session_id":reported_session,
            "call_id":"file-output\nrevision","tool_call":{"writeToolCall":{"args":{"path":"output.md"},
            "result":{"success":{"path":"output.md"}}}}});
        if failed {
            event["tool_call"]["writeToolCall"]["result"]["error"] = json!("Write failed");
        }
        std::fs::write(&stream, format!("{event}\n{event}\n")).unwrap();
        let request = ProviderTurnRequest {
            images: &[],
            provider: ProviderKind::Cursor,
            prompt: "Produce the requested file",
            cwd: work.path(),
            home_path: Some(root),
            model: None,
            native_session_id: Some("cursor-output-session"),
            trust: TrustMode::Plan,
            context_prefix: None,
            local_time: None,
            compaction: Default::default(),
        };
        let outcome = provider::turn::run_provider_turn_with_owner(&request, owner, None, None)
            .await
            .unwrap();
        assert_eq!(outcome.exit_code, Some(0));
        let records = concordia::artifacts::read(&root.join("state"), &conversation).unwrap();
        assert_eq!(records.len(), expected);
        if let Some(record) = records.first() {
            assert_eq!(record.native_session, "cursor-output-session");
            assert_eq!(record.kind, "update");
            assert!(!record.event_id.contains('\n'));
            assert_eq!(
                record.open_current().unwrap(),
                work.path().join("output.md").canonicalize().unwrap()
            );
            std::fs::remove_file(work.path().join("output.md")).unwrap();
            assert!(record.open_current().is_err());
        }
    }

    env.set("CONCORDIA_CLAUDE_BIN", FAKE);
    for (failed, path, expected) in [
        (false, "output.md", 1),
        (true, "output.md", 0),
        (false, "foreign.md", 0),
    ] {
        let work = tempfile::tempdir().unwrap();
        std::fs::write(work.path().join("output.md"), "Synthetic Claude output\n").unwrap();
        let conversation = uuid::Uuid::new_v4().to_string();
        let owner = provider::interaction::InteractionOwner {
            conversation_id: conversation.clone(),
            workspace: work.path().into(),
            execution_cwd: work.path().into(),
            run_id: uuid::Uuid::new_v4().to_string(),
            attempt_id: uuid::Uuid::new_v4().to_string(),
            provider: ProviderKind::Claude,
            account: "claude-artifacts".into(),
            home: Some(root.into()),
        };
        let call = json!({"type":"assistant","session_id":"claude-output-session","message":{"content":[
            {"type":"tool_use","id":"write-output","name":"Write","input":{"file_path":"output.md","content":"Synthetic Claude output"}}]}});
        let result = json!({"type":"user","session_id":"claude-output-session","message":{"content":[
            {"type":"tool_result","tool_use_id":"write-output","is_error":failed,"content":"Completed"}]},
            "tool_use_result":{"filePath":path}});
        std::fs::write(&stream, format!("{call}\n{result}\n{result}\n")).unwrap();
        let request = ProviderTurnRequest {
            images: &[],
            provider: ProviderKind::Claude,
            prompt: "Produce the requested file",
            cwd: work.path(),
            home_path: Some(root),
            model: None,
            native_session_id: Some("claude-output-session"),
            trust: TrustMode::Plan,
            context_prefix: None,
            local_time: None,
            compaction: Default::default(),
        };
        let outcome = provider::turn::run_provider_turn_with_owner(&request, owner, None, None)
            .await
            .unwrap();
        assert_eq!(outcome.exit_code, Some(0));
        let records = concordia::artifacts::read(&root.join("state"), &conversation).unwrap();
        assert_eq!(records.len(), expected);
        if let Some(record) = records.first() {
            assert_eq!(record.kind, "update");
            assert!(record.open_current().is_ok());
        }
    }
    env.set("FAKE_PROVIDER_NATIVE_CLAUDE", "1");
    for fixture in [
        json!({}),
        json!({"allow":true}),
        json!({"cancel":true}),
        json!({"duplicate":true}),
        json!({"fragmented":true}),
        json!({"questions":true}),
        json!({"questions":true,"four_questions":true}),
        json!({"questions":true,"cancel":true}),
        json!({"questions":true,"duplicate":true}),
        json!({"questions":true,"malformed":true}),
        json!({"unknown":true}),
        json!({"model_only":true}),
        json!({"bad_init":true}),
        json!({"closed":true}),
    ] {
        let work = tempfile::tempdir().unwrap();
        std::fs::write(work.path().join("claude-fixture.json"), fixture.to_string()).unwrap();
        let owner = provider::interaction::InteractionOwner {
            conversation_id: uuid::Uuid::new_v4().to_string(),
            workspace: work.path().into(),
            execution_cwd: work.path().into(),
            run_id: uuid::Uuid::new_v4().to_string(),
            attempt_id: uuid::Uuid::new_v4().to_string(),
            provider: ProviderKind::Claude,
            account: "claude-fixture".into(),
            home: Some(root.into()),
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let answer_owner = owner
            .validate_for(ProviderKind::Claude, work.path(), Some(root))
            .unwrap();
        let answer_fixture = fixture.clone();
        let answers = tokio::spawn(async move {
            let mut offered = 0;
            while let Some(event) = rx.recv().await {
                if let TurnProgress::Interaction(request) = event {
                    offered += 1;
                    if answer_fixture["cancel"] == true || answer_fixture["closed"] == true {
                        // The process must continue reading cancellation/EOF while review is pending.
                        continue;
                    }
                    let answer = if answer_fixture["four_questions"] == true {
                        request
                            .choices_with_selections(
                                &[vec![0], vec![0, 1, 2, 3], vec![], vec![0]],
                                &[
                                    None,
                                    None,
                                    Some("free-form answer".into()),
                                    Some("option note".into()),
                                ],
                            )
                            .unwrap()
                    } else if answer_fixture["questions"] == true {
                        json!({"outcome":{"outcome":"answered","answers":[
                            {"questionId":"Choose a route","selectedOptionIds":["Direct"]},
                            {"questionId":"Choose colors","selectedOptionIds":["Red","Blue"]}
                        ]}})
                    } else {
                        request
                            .single_choices(&[usize::from(answer_fixture["allow"] != true)])
                            .unwrap()
                    };
                    request.respond_owned(&answer_owner, answer).unwrap();
                }
            }
            offered
        });
        let request = ProviderTurnRequest {
            images: &images,
            provider: ProviderKind::Claude,
            prompt: "Inspect the synthetic fixture",
            cwd: work.path(),
            home_path: Some(root),
            model: Some("fixture-claude"),
            native_session_id: Some("exact-claude-session"),
            trust: TrustMode::Ask,
            context_prefix: None,
            local_time: None,
            compaction: Default::default(),
        };
        let out = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            provider::turn::run_provider_turn_with_transport(
                &request,
                owner,
                concordia::native_input::Mode::ClaudeStdio,
                None,
                Some(tx),
            ),
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "Claude fixture timed out: {fixture}; journal: {}",
                std::fs::read_to_string(work.path().join("claude-calls.jsonl")).unwrap_or_default()
            )
        })
        .unwrap();
        let offered = answers.await.unwrap();
        let calls = std::fs::read_to_string(work.path().join("claude-calls.jsonl")).unwrap();
        let values: Vec<Value> = calls
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            out.failed,
            fixture["bad_init"] == true || fixture["closed"] == true,
            "{fixture}: {out:?}"
        );
        if fixture["bad_init"] == true {
            assert!(!values.iter().any(|value| value["type"] == "user"));
            assert_eq!(offered, 0);
        } else if fixture["model_only"] == true {
            assert_eq!(offered, 0);
        } else if fixture["unknown"] == true {
            assert_eq!(offered, 0);
            let response = values
                .iter()
                .find(|value| value["type"] == "control_response")
                .unwrap();
            assert_eq!(response["response"]["subtype"], "error");
        } else if fixture["questions"] == true && fixture["malformed"] == true {
            assert_eq!(offered, 0);
            let response = values
                .iter()
                .find(|value| value["type"] == "control_response")
                .unwrap();
            assert_eq!(response["response"]["response"]["behavior"], "deny");
            assert!(response["response"]["response"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("multiSelect")));
        } else {
            assert_eq!(offered, 1);
            let user = values.iter().find(|value| value["type"] == "user").unwrap();
            assert_eq!(
                user["message"]["content"][1]["source"]["data"],
                image.data().unwrap()
            );
            let responses: Vec<_> = values
                .iter()
                .filter(|value| value["type"] == "control_response")
                .collect();
            if fixture["cancel"] == true || fixture["closed"] == true {
                assert!(
                    responses.is_empty(),
                    "late grant after cancellation/EOF: {calls}"
                );
            } else {
                assert_eq!(responses.len(), 1, "duplicate response: {calls}");
                assert_eq!(
                    responses[0]["response"]["response"]["behavior"],
                    if fixture["allow"] == true || fixture["questions"] == true {
                        "allow"
                    } else {
                        "deny"
                    }
                );
                if fixture["allow"] == true {
                    assert_eq!(
                        responses[0]["response"]["response"]["updatedInput"]["file_path"],
                        "synthetic-output.md"
                    );
                }
                if fixture["questions"] == true {
                    assert_eq!(responses[0]["response"]["response"]["behavior"], "allow");
                    let updated = &responses[0]["response"]["response"]["updatedInput"];
                    if fixture["four_questions"] == true {
                        assert_eq!(
                            updated["questions"],
                            json!([
                                {"question":"Choose a route","header":"Route","multiSelect":false,
                                 "options":[{"label":"Direct","description":"Use direct routing"},{"label":"Review","description":"Review first"}]},
                                {"question":"Choose colors","header":"Colors","multiSelect":true,
                                 "options":[{"label":"Red","description":"Use red"},{"label":"Blue","description":"Use blue"},{"label":"Green","description":"Use green"},{"label":"Yellow","description":"Use yellow"}]},
                                {"question":"Add context","header":"Context","multiSelect":false,
                                 "options":[{"label":"Keep","description":"Keep context"},{"label":"Drop","description":"Drop context"}]},
                                {"question":"Route note","header":"Note","multiSelect":true,
                                 "options":[{"label":"One","description":"First"},{"label":"Two","description":"Second"}]}
                            ])
                        );
                        assert_eq!(updated["answers"]["Choose a route"], "Direct");
                        assert_eq!(
                            updated["answers"]["Choose colors"],
                            "Red, Blue, Green, Yellow"
                        );
                        assert_eq!(updated["answers"]["Add context"], "free-form answer");
                        assert_eq!(updated["answers"]["Route note"], "One, option note");
                    } else {
                        assert_eq!(updated["questions"][0]["question"], "Choose a route");
                        assert_eq!(updated["questions"][1]["question"], "Choose colors");
                        assert_eq!(updated["answers"]["Choose a route"], "Direct");
                        assert_eq!(updated["answers"]["Choose colors"], "Red, Blue");
                    }
                }
            }
        }
    }

    concordia::store::upsert_stored_account(
        &root.join("state"),
        concordia::store::StoredAccount {
            id: "claude-fixture".into(),
            provider: ProviderKind::Claude,
            home_path: Some(root.to_string_lossy().into_owned()),
            label: "Synthetic Claude fixture".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    for (fixture, known) in [
        (json!({"context_used":800}), true),
        (json!({"context_model":"foreign-model"}), false),
    ] {
        let work = tempfile::tempdir().unwrap();
        std::fs::write(work.path().join("claude-fixture.json"), fixture.to_string()).unwrap();
        let session = uuid::Uuid::new_v4().to_string();
        let binding = concordia::native_memory::Binding::new(
            ProviderKind::Claude,
            "claude-fixture",
            root,
            &session,
        )
        .unwrap();
        concordia::capacity::set(&root.join("state"), &binding, "rollover 80").unwrap();
        let owner = provider::interaction::InteractionOwner {
            conversation_id: uuid::Uuid::new_v4().to_string(),
            workspace: work.path().into(),
            execution_cwd: work.path().into(),
            run_id: uuid::Uuid::new_v4().to_string(),
            attempt_id: uuid::Uuid::new_v4().to_string(),
            provider: ProviderKind::Claude,
            account: "claude-fixture".into(),
            home: Some(root.into()),
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let answers = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let TurnProgress::Interaction(request) = event {
                    let owner = request.owner().unwrap().clone();
                    request
                        .respond_owned(&owner, request.single_choices(&[1]).unwrap())
                        .unwrap();
                }
            }
        });
        let request = ProviderTurnRequest {
            images: &[],
            provider: ProviderKind::Claude,
            prompt: "Synthetic context fixture",
            cwd: work.path(),
            home_path: Some(root),
            model: Some("fixture-claude"),
            native_session_id: Some(&session),
            trust: TrustMode::Ask,
            context_prefix: None,
            local_time: None,
            compaction: Default::default(),
        };
        let outcome = provider::turn::run_provider_turn_with_transport(
            &request,
            owner,
            concordia::native_input::Mode::ClaudeStdio,
            None,
            Some(tx),
        )
        .await
        .unwrap();
        answers.await.unwrap();
        assert!(!outcome.failed, "{outcome:?}");
        let observation = concordia::native_memory::context_observation(
            &root.join("state"),
            &binding,
            "fixture-claude",
        )
        .unwrap();
        assert_eq!(observation.measurement.is_some(), known, "{observation:?}");
        assert!(observation.native_turn_id.is_none());
        if known {
            let evidence = observation.control.clone().unwrap();
            assert_eq!(evidence.response_sha256.len(), 64);
            uuid::Uuid::parse_str(&evidence.connection_id).unwrap();
            uuid::Uuid::parse_str(&evidence.request_id).unwrap();
            assert!(observation.source.is_some());
            assert!(concordia::capacity::Policy {
                rollover_percent: Some(80),
                compaction: Default::default()
            }
            .rollover(observation.measurement));
            let state = root.join("state");
            concordia::capacity::set(&state, &binding, "compact automatic-off").unwrap();
            assert!(concordia::native_memory::context_observation(
                &state,
                &binding,
                "fixture-claude"
            )
            .unwrap()
            .measurement
            .is_none());
            concordia::capacity::set(&state, &binding, "compact native").unwrap();
            let cache = std::fs::read_dir(state.join(".ao/context-observations"))
                .unwrap()
                .flatten()
                .map(|entry| entry.path())
                .find(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "json")
                })
                .unwrap();
            let original = std::fs::read(&cache).unwrap();
            let mut future: Value = serde_json::from_slice(&original).unwrap();
            future["version"] = json!(255);
            std::fs::write(&cache, future.to_string()).unwrap();
            assert!(concordia::native_memory::context_observation(
                &state,
                &binding,
                "fixture-claude"
            )
            .unwrap()
            .measurement
            .is_none());
            assert!(concordia::capacity::claude_capacity::save(
                &state,
                &binding,
                "fixture-claude",
                observation.source.clone().unwrap(),
                observation.measurement.unwrap(),
                evidence,
                &Default::default()
            )
            .is_err());
            assert_eq!(std::fs::read_to_string(&cache).unwrap(), future.to_string());
            std::fs::write(&cache, original).unwrap();
            let source = root
                .join("projects/fake-provider")
                .join(format!("{session}.jsonl"));
            use std::io::Write;
            writeln!(std::fs::OpenOptions::new()
                .append(true)
                .open(source)
                .unwrap())
            .unwrap();
            let stale = concordia::native_memory::context_observation(
                &root.join("state"),
                &binding,
                "fixture-claude",
            )
            .unwrap();
            assert!(stale.measurement.is_none());
        }
        let journal = std::fs::read_to_string(work.path().join("claude-calls.jsonl")).unwrap();
        let values: Vec<Value> = journal
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            values
                .iter()
                .filter(|value| value["type"] == "user")
                .count(),
            1
        );
        assert_eq!(
            values
                .iter()
                .filter(|value| value["request"]["subtype"] == "get_context_usage")
                .count(),
            1
        );
    }

    // PDF transport is exercised through the same fake process boundary as images. The bytes
    // are synthetic but have a PDF signature; this proves ordering, hashing and rejection before
    // child spawn without invoking a PDF engine or a real provider.
    let pdf_path = root.join("brief.pdf");
    let original_pdf = b"%PDF-1.7\nsynthetic pdf bytes\n%%EOF\n".to_vec();
    std::fs::write(&pdf_path, &original_pdf).unwrap();
    let pdf = attachments::PdfDocument::import(root, &pdf_path).unwrap();
    // Pasted absolute PDF paths use the same bounded grammar as other attachments.
    assert_eq!(
        attachments::pasted_paths(r#"C:\folder\document.pdf"#).unwrap(),
        vec![std::path::PathBuf::from(r"C:\folder\document.pdf")]
    );
    // Import snapshots bytes, so changing the source after import cannot alter the payload.
    std::fs::write(&pdf_path, b"%PDF-1.7\nsource changed after import\n%%EOF\n").unwrap();
    let expected_pdf = base64::engine::general_purpose::STANDARD.encode(&original_pdf);
    assert_eq!(pdf.data().unwrap(), expected_pdf);
    let encrypted_path = root.join("encrypted.pdf");
    std::fs::write(&encrypted_path, b"%PDF-1.7\n/Encrypt 7 0 R\n%%EOF\n").unwrap();
    let encrypted = attachments::PdfDocument::import(root, &encrypted_path).unwrap_err();
    assert!(encrypted.to_ascii_lowercase().contains("encrypt"));
    let mixed = attachments::Message {
        text: "Inspect ordered attachments".into(),
        attachments: vec![
            attachments::Attachment::Image(image.clone()),
            attachments::Attachment::PdfDocument(pdf.clone()),
            attachments::Attachment::TextDocument(
                attachments::Document::import(root, &document_path).unwrap(),
            ),
        ],
        ..Default::default()
    };
    assert!(mixed.has_pdfs());
    let claude_content = mixed.claude_content("Inspect ordered attachments").unwrap();
    assert_eq!(claude_content[0]["text"], "Inspect ordered attachments");
    assert_eq!(claude_content[1]["type"], "image");
    assert_eq!(claude_content[2]["type"], "document");
    assert_eq!(claude_content[2]["source"]["media_type"], "application/pdf");
    assert_eq!(claude_content[3]["type"], "text");
    assert!(claude_content[3]["text"]
        .as_str()
        .unwrap()
        .contains("Attached document bytes\r\n"));
    assert_eq!(pdf.data().unwrap(), expected_pdf);
    assert_eq!(pdf.claude().unwrap()["source"]["data"], expected_pdf);

    let encoded = mixed.encode();
    let pdf_images = mixed.images();
    env.set("FAKE_PROVIDER_MODE", "stream");
    let pdf_request = ProviderTurnRequest {
        images: &pdf_images,
        provider: ProviderKind::Claude,
        prompt: &encoded,
        cwd: root,
        home_path: Some(root),
        model: Some("fixture-claude"),
        native_session_id: None,
        trust: TrustMode::Plan,
        context_prefix: None,
        local_time: None,
        compaction: Default::default(),
    };
    let pdf_outcome = provider::run_provider_turn(&pdf_request, None)
        .await
        .unwrap();
    assert_eq!(pdf_outcome.exit_code, Some(0));
    let pdf_journal = std::fs::read_to_string(root.join("launch.txt")).unwrap();
    let stdin_hex = pdf_journal
        .lines()
        .find_map(|line| line.strip_prefix("STDIN:"))
        .unwrap();
    let stdin = (0..stdin_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&stdin_hex[i..i + 2], 16).unwrap())
        .collect::<Vec<_>>();
    let envelope: Value = serde_json::from_slice(&stdin).unwrap();
    assert!(envelope.to_string().contains(&expected_pdf));

    let stdio_work = tempfile::tempdir().unwrap();
    std::fs::write(stdio_work.path().join("claude-fixture.json"), "{}").unwrap();
    let owner = concordia::provider::interaction::InteractionOwner {
        conversation_id: uuid::Uuid::new_v4().to_string(),
        workspace: stdio_work.path().into(),
        execution_cwd: stdio_work.path().into(),
        run_id: uuid::Uuid::new_v4().to_string(),
        attempt_id: uuid::Uuid::new_v4().to_string(),
        provider: ProviderKind::Claude,
        account: "pdf-claude".into(),
        home: Some(root.into()),
    };
    let owner_for_answer = owner
        .validate_for(ProviderKind::Claude, stdio_work.path(), Some(root))
        .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let answerer = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let TurnProgress::Interaction(request) = event {
                request
                    .respond_owned(&owner_for_answer, request.single_choices(&[1]).unwrap())
                    .unwrap();
            }
        }
    });
    let stdio_request = ProviderTurnRequest {
        images: &pdf_images,
        provider: ProviderKind::Claude,
        prompt: &encoded,
        cwd: stdio_work.path(),
        home_path: Some(root),
        model: Some("fixture-claude"),
        native_session_id: Some("pdf-stdio-session"),
        trust: TrustMode::Ask,
        context_prefix: None,
        local_time: None,
        compaction: Default::default(),
    };
    let stdio_outcome = provider::turn::run_provider_turn_with_transport(
        &stdio_request,
        owner,
        concordia::native_input::Mode::ClaudeStdio,
        None,
        Some(tx),
    )
    .await
    .unwrap();
    answerer.await.unwrap();
    assert!(!stdio_outcome.failed, "{stdio_outcome:?}");
    let stdio_journal =
        std::fs::read_to_string(stdio_work.path().join("claude-calls.jsonl")).unwrap();
    assert!(stdio_journal.contains(&expected_pdf));

    for provider in [ProviderKind::Codex, ProviderKind::Cursor] {
        let _ = std::fs::remove_file(root.join("launch.txt"));
        let unsupported = ProviderTurnRequest {
            images: &pdf_images,
            provider,
            prompt: &encoded,
            cwd: root,
            home_path: Some(root),
            model: Some("fixture-model"),
            native_session_id: None,
            trust: TrustMode::Plan,
            context_prefix: None,
            local_time: None,
            compaction: Default::default(),
        };
        let outcome = provider::run_provider_turn(&unsupported, None).await;
        assert!(outcome.is_err() || outcome.unwrap().failed);
        assert!(
            !root.join("launch.txt").exists(),
            "unsupported PDF child spawned for {provider:?}"
        );
    }

    // A changed PDF is rejected before launching the fake process; the original envelope stays
    // the draft/queue value and no provider child is spawned.
    std::fs::write(&pdf.path, b"%PDF-1.7\ntampered\n%%EOF\n").unwrap();
    let _ = std::fs::remove_file(root.join("launch.txt"));
    let tampered = provider::run_provider_turn(&pdf_request, None).await;
    assert!(tampered.is_err() || tampered.unwrap().failed);
    assert!(!root.join("launch.txt").exists());
}
