//! Real turns on every provider with its cheapest model. Ignored by default; run with
//! `CONCORDIA_LIVE=1 cargo test -p concordia --test live_providers -- --ignored --test-threads=1`.
//! Optional filters: `CONCORDIA_LIVE_PROVIDERS=cursor,codex,claude`. Homes default to the
//! personal `~/.cursor`, `~/.codex` and `~/.claude`; override with `CONCORDIA_LIVE_<PROVIDER>_HOME`.
use concordia::{
    native_input::Mode,
    provider::{
        interaction::{InteractionOwner, NativeInteraction},
        turn::{run_provider_turn_with_transport, ProviderTurnRequest, TurnOutcome, TurnProgress},
    },
    store::{ProviderKind, TrustMode},
};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

const PROVIDERS: [ProviderKind; 4] = [
    ProviderKind::Cursor,
    ProviderKind::Codex,
    ProviderKind::Claude,
    ProviderKind::Grok,
];

fn cheap_model(provider: ProviderKind) -> String {
    let variable = format!("CONCORDIA_LIVE_{}_MODEL", provider.as_str().to_uppercase());
    if let Ok(model) = std::env::var(&variable) {
        if !model.trim().is_empty() {
            return model;
        }
    }
    match provider {
        ProviderKind::Cursor => "auto",
        ProviderKind::Codex => "gpt-5.6-luna",
        ProviderKind::Claude => "haiku",
        ProviderKind::Grok => "grok-build",
        ProviderKind::Opencode => "opencode",
    }
    .to_string()
}

fn mode(provider: ProviderKind) -> Mode {
    let variable = format!("CONCORDIA_LIVE_{}_MODE", provider.as_str().to_uppercase());
    if let Ok(value) = std::env::var(variable) {
        return match (provider, value.as_str()) {
            (_, "standard") => Mode::Standard,
            (ProviderKind::Codex, "appserver") => Mode::CodexAppServer,
            (ProviderKind::Claude, "stdio") => Mode::ClaudeStdio,
            (ProviderKind::Cursor, "acp") => Mode::CursorAcp,
            (ProviderKind::Grok, "acp") => Mode::GrokAcp,
            _ => panic!("invalid explicitly selected live transport"),
        };
    }
    match provider {
        ProviderKind::Cursor => Mode::CursorAcp,
        ProviderKind::Grok => Mode::GrokAcp,
        ProviderKind::Codex | ProviderKind::Claude => Mode::Standard,
        ProviderKind::Opencode => Mode::OpencodeAcp,
    }
}

fn selected() -> Vec<(ProviderKind, PathBuf)> {
    if std::env::var("CONCORDIA_LIVE").ok().as_deref() != Some("1") {
        eprintln!("skipped: set CONCORDIA_LIVE=1");
        return Vec::new();
    }
    let filter = std::env::var("CONCORDIA_LIVE_PROVIDERS").unwrap_or_default();
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .expect("home directory");
    PROVIDERS
        .into_iter()
        .filter(|p| filter.is_empty() || filter.split(',').any(|f| f.trim() == p.as_str()))
        .map(|p| {
            let var = format!("CONCORDIA_LIVE_{}_HOME", p.as_str().to_uppercase());
            let dir = std::env::var_os(var)
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(format!(".{}", p.as_str())));
            (p, dir)
        })
        .collect()
}

fn owner(provider: ProviderKind, cwd: &Path, home: &Path) -> InteractionOwner {
    let id = || uuid::Uuid::new_v4().to_string();
    InteractionOwner {
        conversation_id: id(),
        workspace: cwd.into(),
        execution_cwd: cwd.into(),
        run_id: id(),
        attempt_id: id(),
        provider,
        account: format!("live-{}", provider.as_str()),
        home: Some(home.into()),
    }
}

struct Seen {
    questions: usize,
    diffs: Vec<String>,
}

/// Runs one turn, answering questions with their first option and permissions with the first
/// offered option.
async fn turn(
    provider: ProviderKind,
    home: &Path,
    cwd: &Path,
    prompt: &str,
    trust: TrustMode,
) -> (TurnOutcome, Seen) {
    let (tx, mut rx) = mpsc::channel(64);
    let seen = tokio::spawn(async move {
        let mut seen = Seen {
            questions: 0,
            diffs: Vec::new(),
        };
        while let Some(event) = rx.recv().await {
            match event {
                TurnProgress::TextDelta(_) | TurnProgress::Text(_) => {}
                TurnProgress::Card(card) => {
                    if let Some(detail) = card.detail.filter(|d| d.contains("\n@@")) {
                        seen.diffs.push(detail);
                    }
                }
                TurnProgress::Interaction(request) => match request.kind() {
                    NativeInteraction::Questions(questions) => {
                        seen.questions += 1;
                        let picks = vec![0; questions.len()];
                        request.respond(request.single_choices(&picks).expect("answer"));
                    }
                    NativeInteraction::Permission { options, .. } => {
                        let first = options.first().map(|o| o.id.clone()).unwrap_or_default();
                        request.respond(serde_json::json!({
                            "outcome": {"outcome": "selected", "optionId": first}
                        }));
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        seen
    });
    let model = cheap_model(provider);
    let req = ProviderTurnRequest {
        images: &[],
        provider,
        prompt,
        cwd,
        home_path: Some(home),
        model: Some(&model),
        native_session_id: None,
        trust,
        context_prefix: None,
        local_time: None,
        compaction: Default::default(),
    };
    let out = run_provider_turn_with_transport(
        &req,
        owner(provider, cwd, home),
        mode(provider),
        None,
        Some(tx),
    )
    .await
    .expect("turn");
    (out, seen.await.unwrap())
}

#[tokio::test]
#[ignore = "spends real provider usage"]
async fn every_provider_streams_and_finishes() {
    for (provider, home) in selected() {
        let dir = tempfile::tempdir().unwrap();
        let (out, _) = turn(
            provider,
            &home,
            dir.path(),
            "Reply with exactly OK. Do not run tools.",
            TrustMode::Plan,
        )
        .await;
        std::fs::write(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target").join(format!("roadmap-live-stream-{}.json",provider.as_str())),
            serde_json::to_vec_pretty(&serde_json::json!({"provider":provider.as_str(),"model":cheap_model(provider),"mode":format!("{:?}",mode(provider)),"failed":out.failed,"exit_code":out.exit_code,"reply":out.text,"stderr":out.stderr_tail})).unwrap(),
        ).unwrap();
        assert!(
            !out.failed && out.exit_code == Some(0),
            "{provider:?}: exit={:?}; reply={:?}; stderr={}",
            out.exit_code,
            out.text,
            out.stderr_tail
        );
        assert!(out.text.contains("OK"), "{provider:?}: {:?}", out.text);
    }
}

#[tokio::test]
#[ignore = "spends real provider usage"]
async fn every_provider_asks_through_the_popup() {
    for (provider, home) in selected() {
        let dir = tempfile::tempdir().unwrap();
        let (out, seen) = turn(
            provider,
            &home,
            dir.path(),
            "Use your tool for asking the user a question (ask_user, AskUserQuestion, \
             request_user_input or your ask-question tool; not plain text) to ask one \
             multiple-choice question 'Pick a letter' with options A and B. After the answer, \
             reply with only the letter that was picked.",
            TrustMode::Ask,
        )
        .await;
        let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("live-question-{}.json", provider.as_str()));
        std::fs::write(
            artifact,
            serde_json::to_vec_pretty(&serde_json::json!({
                "provider": provider.as_str(),
                "model": cheap_model(provider),
                "failed": out.failed,
                "exit_code": out.exit_code,
                "reply": out.text,
                "stderr": out.stderr_tail,
                "questions": seen.questions,
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(
            !out.failed,
            "{provider:?}: reply={:?}; stderr={}",
            out.text, out.stderr_tail
        );
        assert!(
            seen.questions > 0,
            "{provider:?} asked no popup question: {:?}",
            out.text
        );
        assert!(out.text.contains('A'), "{provider:?}: {:?}", out.text);
    }
}

#[tokio::test]
#[ignore = "spends real provider usage"]
async fn every_provider_reports_line_diffs_for_edits() {
    for (provider, home) in selected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("note.txt"), "alpha\nbeta\ngamma\n").unwrap();
        let (out, seen) = turn(
            provider,
            &home,
            dir.path(),
            "In note.txt, replace the line 'beta' with 'delta' using your file edit tool. \
             Change nothing else and reply with only DONE.",
            TrustMode::Full,
        )
        .await;
        assert!(!out.failed, "{provider:?}: {}", out.stderr_tail);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("note.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "alpha\ndelta\ngamma\n",
            "{provider:?} did not make the edit"
        );
        assert!(
            seen.diffs
                .iter()
                .any(|d| d.contains("-beta") && d.contains("+delta")),
            "{provider:?} reported no line diff: {:?}",
            seen.diffs
        );
    }
}
