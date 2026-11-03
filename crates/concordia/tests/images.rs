//! Real process transport with synthetic image bytes; never invokes a real provider.
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
        json!({}),
        json!({"question":true}),
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
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let answers = tokio::spawn(async move {
            let mut requests = Vec::new();
            while let Some(event) = rx.recv().await {
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
                                request.single_choices(&[choice]).unwrap(),
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
            if fixture["question"] == true {
                assert_eq!(
                    answer["result"],
                    json!({"answers":{"route":{"answers":["Direct"]}}})
                );
            } else {
                assert_eq!(answer["result"], json!({"decision":"decline"}));
            }
        }
    }
}
