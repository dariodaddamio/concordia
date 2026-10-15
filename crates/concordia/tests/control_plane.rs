//! Control-plane fixtures: fail if argv construction, session parsing or usage reading drift.

use concordia::context::should_attempt_native_resume;
use concordia::provider::claude::{assert_safe_claude_argv, plan_claude_turn, ClaudeTurnRequest};
use concordia::provider::codex::{
    assert_safe_argv, extract_stream_chunk, plan_turn_argv, TurnRequest,
};
use concordia::provider::cursor::{assert_safe_cursor_argv, plan_cursor_turn, CursorTurnRequest};
use concordia::provider::turn::{plan_provider_turn, ProviderTurnRequest};
use concordia::store::{ProviderKind, TrustMode};
use concordia::usage::read_codex_usage;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("fixtures")
        .join("control-plane")
        .join(name)
}

#[test]
fn codex_workspace_argv_is_stable() {
    let cwd = PathBuf::from("C:\\proj");
    let home = PathBuf::from("C:\\homes\\a");
    let plan = plan_turn_argv(&TurnRequest {
        prompt: "hello world",
        cwd: &cwd,
        codex_home: Some(&home),
        model: Some("gpt-5.6-sol"),
        native_session_id: Some("sess-1"),
        trust: TrustMode::Workspace,
    });
    assert_eq!(
        plan.argv,
        vec![
            "exec",
            "--json",
            "-m",
            "gpt-5.6-sol",
            "--skip-git-repo-check",
            "--approve-for-me",
            "resume",
            "sess-1",
            "hello world",
        ]
    );
    assert!(!plan.argv.iter().any(|a| a == "-s"));
    assert!(!plan.argv.iter().any(|a| a == "-a"));
    assert!(!plan
        .argv
        .iter()
        .any(|a| a == "--dangerously-bypass-approvals-and-sandbox"));
}

#[test]
fn codex_full_and_ask_trust_flags() {
    let cwd = PathBuf::from(".");
    let full = plan_turn_argv(&TurnRequest {
        prompt: "hi",
        cwd: &cwd,
        codex_home: None,
        model: None,
        native_session_id: None,
        trust: TrustMode::Full,
    });
    assert!(full
        .argv
        .iter()
        .any(|a| a == "--dangerously-bypass-approvals-and-sandbox"));

    let ask = plan_turn_argv(&TurnRequest {
        prompt: "hi",
        cwd: &cwd,
        codex_home: None,
        model: None,
        native_session_id: None,
        trust: TrustMode::Ask,
    });
    assert!(ask
        .argv
        .windows(2)
        .any(|w| w[0] == "-s" && w[1] == "read-only"));
}

#[test]
fn refuses_last_and_continue() {
    assert!(assert_safe_argv(&["exec".into(), "--last".into()]).is_err());
    assert!(assert_safe_argv(&["exec".into(), "--continue".into()]).is_err());
    assert!(assert_safe_argv(&["exec".into(), "--json".into()]).is_ok());
}

#[test]
fn extracts_session_and_text_from_jsonl_fixture() {
    let raw = std::fs::read_to_string(fixture("codex-stream.jsonl")).expect("fixture");
    let out = extract_stream_chunk(&raw);
    assert_eq!(
        out.session_id.as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    assert!(out.text.contains("parity hello"));
    assert!(
        out.cards
            .iter()
            .any(|c| { c.title == "shell" || c.detail.as_deref().unwrap_or("").contains("npm") }),
        "expected command card, got {:?}",
        out.cards
    );
    assert!(
        out.cards
            .iter()
            .any(|c| c.status == concordia::provider::CardStatus::Done),
        "expected completed card"
    );
}

#[test]
fn usage_reads_rate_limits_fixture() {
    let tmp = tempfile_dir();
    let codex_home = tmp.path().to_path_buf();
    let tmp_dir = codex_home.join("tmp");
    std::fs::create_dir_all(&tmp_dir).unwrap();
    std::fs::copy(fixture("rate_limits.json"), tmp_dir.join("rate.json")).unwrap();
    let snap = read_codex_usage(&codex_home);
    assert!(snap.summary.contains("5h 26% left"), "got {}", snap.summary);
    assert!(snap.summary.contains("7d 70% left"), "got {}", snap.summary);
}

#[test]
fn usage_reads_session_jsonl_when_tmp_empty() {
    let tmp = tempfile_dir();
    let codex_home = tmp.path().to_path_buf();
    let sess_dir = codex_home
        .join("sessions")
        .join("2026")
        .join("10")
        .join("15");
    std::fs::create_dir_all(&sess_dir).unwrap();
    std::fs::create_dir_all(codex_home.join("tmp")).unwrap();
    std::fs::copy(
        fixture("session-rate-limits.jsonl"),
        sess_dir.join("rollout-test.jsonl"),
    )
    .unwrap();
    let snap = read_codex_usage(&codex_home);
    assert!(snap.summary.contains("5h 76% left"), "got {}", snap.summary);
    assert!(snap.summary.contains("7d 44% left"), "got {}", snap.summary);
    assert_eq!(snap.primary_pct, Some(24.0));
}

fn tempfile_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

#[test]
fn claude_workspace_argv_and_resume() {
    let cwd = PathBuf::from(".");
    let home = PathBuf::from(r"C:\homes\claude");
    let plan = plan_claude_turn(&ClaudeTurnRequest {
        prompt: "hi",
        cwd: &cwd,
        config_dir: Some(&home),
        model: Some("sonnet"),
        native_session_id: Some("sess-claude"),
        trust: TrustMode::Workspace,
    })
    .unwrap();
    assert!(plan
        .argv
        .windows(2)
        .any(|w| w[0] == "--permission-mode" && w[1] == "acceptEdits"));
    assert!(plan
        .argv
        .windows(2)
        .any(|w| w[0] == "--resume" && w[1] == "sess-claude"));
    assert!(plan
        .env
        .iter()
        .any(|(k, v)| k == "CLAUDE_CONFIG_DIR" && v.contains("claude")));
    assert!(assert_safe_claude_argv(&["-p".into(), "--continue".into()], false).is_err());
}

#[test]
fn cursor_print_argv_requires_explicit_resume() {
    let cwd = PathBuf::from(".");
    let plan = plan_cursor_turn(&CursorTurnRequest {
        prompt: "hi",
        cwd: &cwd,
        home_path: None,
        model: None,
        native_session_id: Some("chat-1"),
        trust: TrustMode::Workspace,
    })
    .unwrap();
    assert!(plan.argv.contains(&"--print".into()));
    assert!(plan
        .argv
        .windows(2)
        .any(|w| w[0] == "--resume" && w[1] == "chat-1"));
    assert!(assert_safe_cursor_argv(&["--print".into(), "--continue".into()], false).is_err());
    assert!(assert_safe_cursor_argv(&["resume".into()], false).is_err());
}

#[test]
fn unified_planner_never_emits_last() {
    let cwd = PathBuf::from(".");
    for provider in [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Cursor,
    ] {
        let plan = plan_provider_turn(&ProviderTurnRequest {
            provider,
            prompt: "x",
            cwd: &cwd,
            home_path: None,
            model: None,
            native_session_id: Some("id-1"),
            trust: TrustMode::Workspace,
            context_prefix: None,
        })
        .unwrap();
        assert!(!plan.argv.iter().any(|a| a == "--last" || a == "--continue"));
    }
}

#[test]
fn cross_account_continuity_policy() {
    assert!(!should_attempt_native_resume(
        ProviderKind::Codex,
        ProviderKind::Codex,
        Some(r"C:\a\.codex"),
        Some(r"C:\a\.codex-2"),
        Some("sess"),
    ));
    assert!(should_attempt_native_resume(
        ProviderKind::Codex,
        ProviderKind::Codex,
        Some(r"C:\a\.codex"),
        Some(r"C:\a\.codex"),
        Some("sess"),
    ));
}

#[tokio::test]
async fn cancel_kills_mock_child() {
    let ok = concordia::provider::spawn_and_cancel_mock()
        .await
        .expect("cancel mock");
    assert!(ok);
}

#[test]
fn claude_first_turn_names_a_session_and_later_turns_resume_it() {
    let cwd = PathBuf::from(".");
    let mk = |sid: Option<&str>| {
        plan_claude_turn(&ClaudeTurnRequest {
            prompt: "hi",
            cwd: &cwd,
            config_dir: None,
            model: None,
            native_session_id: sid,
            trust: TrustMode::Workspace,
        })
        .unwrap()
    };
    let first = mk(None);
    let id = first
        .new_session_id
        .clone()
        .expect("first turn names a session");
    assert!(first
        .argv
        .windows(2)
        .any(|w| w[0] == "--session-id" && w[1] == id));
    assert!(!first.argv.iter().any(|a| a == "--resume"));
    // Every conversation gets its own id.
    assert_ne!(mk(None).new_session_id.unwrap(), id);

    let later = mk(Some(&id));
    assert!(later.new_session_id.is_none());
    assert!(later
        .argv
        .windows(2)
        .any(|w| w[0] == "--resume" && w[1] == id));
    assert!(!later.argv.iter().any(|a| a == "--session-id"));
}
