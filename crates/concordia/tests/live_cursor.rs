//! Real Cursor turns over ACP on the Auto model. Ignored by default because they spend the user's
//! Cursor usage; run with `CONCORDIA_LIVE_CURSOR=1 cargo test -p concordia --test live_cursor
//! -- --ignored --test-threads=1`. `CONCORDIA_LIVE_CURSOR_HOME` picks the account home
//! (default `~/.cursor`).
use concordia::{
    attachments,
    native_input::Mode,
    provider::{
        interaction::{InteractionOwner, NativeInteraction},
        turn::{run_provider_turn_with_transport, ProviderTurnRequest, TurnOutcome, TurnProgress},
    },
    store::{ProviderKind, TrustMode},
};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

fn live() -> Option<PathBuf> {
    if std::env::var("CONCORDIA_LIVE_CURSOR").ok().as_deref() != Some("1") {
        eprintln!("skipped: set CONCORDIA_LIVE_CURSOR=1");
        return None;
    }
    Some(
        std::env::var_os("CONCORDIA_LIVE_CURSOR_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs_home().join(".cursor")),
    )
}

fn dirs_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .expect("home directory")
}

fn owner(cwd: &Path, home: &Path) -> InteractionOwner {
    let id = || uuid::Uuid::new_v4().to_string();
    InteractionOwner {
        conversation_id: id(),
        workspace: cwd.into(),
        execution_cwd: cwd.into(),
        run_id: id(),
        attempt_id: id(),
        provider: ProviderKind::Cursor,
        account: "live-cursor".into(),
        home: Some(home.into()),
    }
}

/// Runs one turn, answering every question with its first option and every permission with
/// the first offered option. Returns the outcome and the interaction kinds that arrived.
async fn turn(
    home: &Path,
    cwd: &Path,
    prompt: &str,
    images: &[attachments::Image],
    session: Option<&str>,
) -> (TurnOutcome, Vec<String>) {
    turn_with(home, cwd, prompt, images, session, Mode::CursorAcp).await
}

async fn turn_with(
    home: &Path,
    cwd: &Path,
    prompt: &str,
    images: &[attachments::Image],
    session: Option<&str>,
    mode: Mode,
) -> (TurnOutcome, Vec<String>) {
    let (tx, mut rx) = mpsc::channel(64);
    let seen = tokio::spawn(async move {
        let mut kinds = Vec::new();
        while let Some(event) = rx.recv().await {
            if let TurnProgress::Session(id) = &event {
                kinds.push(format!("session:{id}"));
            }
            if let TurnProgress::Interaction(request) = event {
                match request.kind() {
                    NativeInteraction::Questions(questions) => {
                        kinds.push(format!("questions:{}", questions.len()));
                        let picks = vec![0; questions.len()];
                        request.respond(request.single_choices(&picks).expect("answer"));
                    }
                    NativeInteraction::Permission { options, .. } => {
                        kinds.push("permission".into());
                        let first = options.first().map(|o| o.id.clone()).unwrap_or_default();
                        request.respond(serde_json::json!({
                            "outcome": {"outcome": "selected", "optionId": first}
                        }));
                    }
                    other => kinds.push(format!("{other:?}")),
                }
            }
        }
        kinds
    });
    let model = std::env::var("CONCORDIA_LIVE_MODEL").unwrap_or_else(|_| "auto".into());
    let req = ProviderTurnRequest {
        images,
        provider: ProviderKind::Cursor,
        prompt,
        cwd,
        home_path: Some(home),
        model: Some(&model),
        native_session_id: session,
        trust: match std::env::var("CONCORDIA_LIVE_TRUST").as_deref() {
            Ok("ask") => TrustMode::Ask,
            _ => TrustMode::Plan,
        },
        context_prefix: None,
        local_time: None,
        compaction: Default::default(),
    };
    let out = run_provider_turn_with_transport(&req, owner(cwd, home), mode, None, Some(tx))
        .await
        .expect("turn");
    (out, seen.await.unwrap())
}

#[tokio::test]
#[ignore = "spends real Cursor usage"]
async fn text_turn_then_resume_the_same_session() {
    let Some(home) = live() else { return };
    let dir = tempfile::tempdir().unwrap();
    let (first, _) = turn(
        &home,
        dir.path(),
        "Reply with exactly the word PONG and nothing else.",
        &[],
        None,
    )
    .await;
    assert!(!first.failed, "{}", first.stderr_tail);
    assert!(first.text.contains("PONG"), "{:?}", first.text);
    let session = first.session_id.clone().expect("session id");
    let (second, _) = turn(
        &home,
        dir.path(),
        "What single word did you reply with last time? Reply with that word only.",
        &[],
        Some(&session),
    )
    .await;
    assert!(!second.failed, "{}", second.stderr_tail);
    assert!(second.text.contains("PONG"), "{:?}", second.text);
}

#[tokio::test]
#[ignore = "spends real Cursor usage"]
async fn a_print_mode_chat_continues_after_acp_became_the_default() {
    let Some(home) = live() else { return };
    // Print mode stops at Cursor's workspace-trust prompt in a fresh temp folder.
    let cwd = std::env::current_dir().unwrap();
    let dir = cwd.as_path();
    let (first, events) = turn_with(
        &home,
        dir,
        "Reply with exactly the word ZEBRA and nothing else.",
        &[],
        None,
        Mode::Standard,
    )
    .await;
    assert!(!first.failed, "{}", first.stderr_tail);
    let session = first
        .session_id
        .clone()
        .or_else(|| {
            events
                .iter()
                .find_map(|e| e.strip_prefix("session:").map(str::to_owned))
        })
        .unwrap_or_else(|| {
            panic!(
                "no chat id: {:?} {events:?} {}",
                first.text, first.stderr_tail
            )
        });
    let (second, _) = turn(
        &home,
        dir,
        "What single word did you reply with last time? Reply with that word only.",
        &[],
        Some(&session),
    )
    .await;
    assert!(!second.failed, "{}", second.stderr_tail);
    assert!(second.text.contains("ZEBRA"), "{:?}", second.text);
}

#[tokio::test]
#[ignore = "spends real Cursor usage"]
async fn image_reaches_cursor() {
    let Some(home) = live() else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("red.png");
    image::RgbImage::from_pixel(64, 64, image::Rgb([220, 20, 20]))
        .save(&path)
        .unwrap();
    let image = attachments::import(dir.path(), &path).unwrap();
    let (out, _) = turn(
        &home,
        dir.path(),
        "What colour is the attached image? Answer with one lowercase word.",
        &[image],
        None,
    )
    .await;
    assert!(!out.failed, "{}", out.stderr_tail);
    assert!(out.text.to_lowercase().contains("red"), "{:?}", out.text);
}

#[tokio::test]
#[ignore = "spends real Cursor usage"]
async fn question_tool_reaches_the_popup_broker() {
    let Some(home) = live() else { return };
    let dir = tempfile::tempdir().unwrap();
    let (out, kinds) = turn(
        &home,
        dir.path(),
        "Use your ask-question tool (not plain text) to ask me one multiple-choice question: \
         'Pick a letter' with options A and B. After I answer, reply with only the letter I picked.",
        &[],
        None,
    )
    .await;
    assert!(!out.failed, "{}", out.stderr_tail);
    assert!(
        kinds.iter().any(|k| k.starts_with("questions:")),
        "{kinds:?} {:?}",
        out.text
    );
    assert!(out.text.contains('A'), "{:?}", out.text);
}
