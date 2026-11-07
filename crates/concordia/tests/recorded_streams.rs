use concordia::{
    provider::{
        failure::{classify_failure, FailureKind},
        recording::Recorder,
    },
    store::ProviderKind,
};
use std::{fs, path::PathBuf};

#[test]
fn recording_is_off_by_default() {
    std::env::remove_var("CONCORDIA_RECORD_STREAMS");
    assert!(Recorder::from_env(ProviderKind::Codex).is_none());
}

#[test]
fn recorded_lines_are_redacted() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("CONCORDIA_RECORD_STREAMS", dir.path());
    concordia::redact::register_runtime_secret("fixture-secret");
    Recorder::from_env(ProviderKind::Claude)
        .unwrap()
        .line("stdout", "home=C:\\Users\\fixture key=fixture-secret");
    let raw = fs::read_to_string(
        fs::read_dir(dir.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path(),
    )
    .unwrap();
    assert!(!raw.contains("fixture-secret"));
    assert!(raw.contains("claude") && raw.contains("stdout"));
    std::env::remove_var("CONCORDIA_RECORD_STREAMS");
}

#[test]
fn classifier_handles_structured_limit_fixture() {
    let outcome = concordia::provider::TurnOutcome {
        stderr_tail: r#"{"type":"error","code":"usage_limit_reached"}"#.into(),
        exit_code: Some(1),
        ..Default::default()
    };
    assert_eq!(
        classify_failure(ProviderKind::Cursor, &outcome),
        FailureKind::Usage
    );
}

#[test]
fn synthetic_provider_stream_fixtures_replay_and_classify() {
    let cases = [
        ("codex", "Codex", FailureKind::None),
        ("claude", "Claude", FailureKind::None),
        ("cursor", "Cursor", FailureKind::None),
    ];
    for (provider, flavor, expected) in cases {
        for scenario in [
            "normal",
            "tool_use",
            "usage_limit",
            "auth_failure",
            "context_overflow",
        ] {
            let path = _fixture_root()
                .join(provider)
                .join("synthetic")
                .join(format!("{scenario}.jsonl"));
            let lines = fs::read_to_string(path).unwrap();
            let mut parts = Vec::new();
            if provider == "codex" {
                for line in lines.lines() {
                    parts.extend(concordia::provider::stream_cards::parse_codex_stream_chunk(
                        line,
                    ));
                }
            } else {
                let kind = if flavor == "Claude" {
                    concordia::provider::stream_json::Flavor::Claude
                } else {
                    concordia::provider::stream_json::Flavor::Cursor
                };
                let mut parser = concordia::provider::stream_json::StreamParser::new(kind);
                for line in lines.lines() {
                    parts.extend(parser.parse_line(line));
                }
            }
            assert!(!parts.is_empty(), "{provider}/{scenario} parsed no parts");
            let kind = match scenario {
                "usage_limit" => FailureKind::Usage,
                "context_overflow" => FailureKind::Context,
                "auth_failure" => FailureKind::Other,
                _ => expected,
            };
            let outcome = concordia::provider::TurnOutcome {
                stderr_tail: lines,
                exit_code: (kind != FailureKind::None).then_some(1),
                ..Default::default()
            };
            assert_eq!(
                classify_failure(
                    match provider {
                        "codex" => ProviderKind::Codex,
                        "claude" => ProviderKind::Claude,
                        _ => ProviderKind::Cursor,
                    },
                    &outcome
                ),
                kind,
                "{provider}/{scenario}"
            );
        }
    }
}

#[test]
fn synthetic_stream_mutations_do_not_panic_or_lose_final_text() {
    for (provider, flavor) in [
        ("codex", None),
        (
            "claude",
            Some(concordia::provider::stream_json::Flavor::Claude),
        ),
        (
            "cursor",
            Some(concordia::provider::stream_json::Flavor::Cursor),
        ),
    ] {
        let source = fs::read_to_string(
            _fixture_root()
                .join(provider)
                .join("synthetic/normal.jsonl"),
        )
        .unwrap();
        let mutations = [
            source.replacen('{', "", 1),
            format!("{source}{{\"type\":\"unknown.future\"}}\n"),
        ];
        for mutated in mutations {
            if let Some(kind) = flavor {
                let mut parser = concordia::provider::stream_json::StreamParser::new(kind);
                let _: Vec<_> = mutated
                    .lines()
                    .flat_map(|line| parser.parse_line(line))
                    .collect();
            } else {
                let _: Vec<_> =
                    concordia::provider::stream_cards::parse_codex_stream_chunk(&mutated);
            }
        }
    }
}

#[test]
fn structured_question_fixtures_cover_native_provider_envelopes() {
    let root = _fixture_root();
    let codex: serde_json::Value = serde_json::from_str(
        fs::read_to_string(root.join("codex/questions/native_request_user_input.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();
    assert_eq!(codex["method"], "item/tool/requestUserInput");
    assert_eq!(
        codex["params"]["questions"][0]["options"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let claude = fs::read_to_string(root.join("claude/questions/ask_user_question.jsonl")).unwrap();
    let mut parser = concordia::provider::stream_json::StreamParser::new(
        concordia::provider::stream_json::Flavor::Claude,
    );
    let parts: Vec<_> = claude
        .lines()
        .flat_map(|line| parser.parse_line(line))
        .collect();
    assert!(parts.iter().any(|part| match part {
        concordia::provider::stream_cards::StreamPart::Card(card) =>
            card.title == "AskUserQuestion"
                && card
                    .detail
                    .as_deref()
                    .is_some_and(|d| d.contains("Choose the route")),
        _ => false,
    }));

    let cursor: serde_json::Value = serde_json::from_str(
        fs::read_to_string(root.join("cursor/questions/ask_question.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();
    assert_eq!(cursor["method"], "cursor/ask_question");
    assert_eq!(
        cursor["params"]["questions"][0]["options"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[allow(dead_code)]
fn _fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/streams")
}
