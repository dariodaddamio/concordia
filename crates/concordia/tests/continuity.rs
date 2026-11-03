//! Continuity fixture gates — foreign session leak, pack budget, omit-on-native.

use concordia::context::{
    assert_pack_within_budget, plan_continuity, ContinuityRequest, PackMessage, PackRole,
    MAX_CONTEXT_CHARS,
};
use concordia::provider::turn::{plan_provider_turn, ProviderTurnRequest};
use concordia::store::{ProviderKind, TrustMode};
use std::path::{Path, PathBuf};

#[test]
fn foreign_session_id_never_in_cross_home_argv() {
    let cwd = PathBuf::from(".");
    let decision = plan_continuity(&ContinuityRequest {
        from_provider: ProviderKind::Codex,
        to_provider: ProviderKind::Codex,
        from_home: Some(r"C:\homes\.codex"),
        to_home: Some(r"C:\homes\.codex-2"),
        session_id: Some("foreign-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
        messages: &[PackMessage {
            role: PackRole::User,
            text: "prior".into(),
        }],
        thread_id: None,
        include_pack_if_needed: true,
    });
    assert!(!decision.native_resume);
    assert!(decision.session_id.is_none());

    let plan = plan_provider_turn(&ProviderTurnRequest {
        images: &[],
        provider: ProviderKind::Codex,
        prompt: "next",
        cwd: &cwd,
        home_path: Some(Path::new(r"C:\homes\.codex-2")),
        model: None,
        native_session_id: decision.session_id.as_deref(),
        trust: TrustMode::Workspace,
        context_prefix: decision.pack.as_ref().map(|p| p.text.as_str()),
        local_time: None,
        compaction: Default::default(),
    })
    .unwrap();
    assert!(
        !plan.argv.iter().any(|a| a.contains("foreign-aaaaaaaa")),
        "argv leaked foreign session: {:?}",
        plan.argv
    );
    assert!(!plan.argv.iter().any(|a| a == "resume"));
}

#[test]
fn native_resume_omits_pack_and_keeps_session_in_argv() {
    let cwd = PathBuf::from(".");
    let sid = "11111111-2222-3333-4444-555555555555";
    let decision = plan_continuity(&ContinuityRequest {
        from_provider: ProviderKind::Codex,
        to_provider: ProviderKind::Codex,
        from_home: Some(r"C:\homes\.codex"),
        to_home: Some(r"C:\homes\.codex"),
        session_id: Some(sid),
        messages: &[PackMessage {
            role: PackRole::User,
            text: "should not be packed".into(),
        }],
        thread_id: None,
        include_pack_if_needed: true,
    });
    assert!(decision.native_resume);
    assert!(decision.pack.is_none());
    assert_eq!(decision.session_id.as_deref(), Some(sid));

    let plan = plan_provider_turn(&ProviderTurnRequest {
        images: &[],
        provider: ProviderKind::Codex,
        prompt: "continue",
        cwd: &cwd,
        home_path: Some(Path::new(r"C:\homes\.codex")),
        model: None,
        native_session_id: decision.session_id.as_deref(),
        trust: TrustMode::Workspace,
        context_prefix: None,
        local_time: None,
        compaction: Default::default(),
    })
    .unwrap();
    assert!(plan
        .argv
        .windows(2)
        .any(|w| w[0] == "resume" && w[1] == sid));
}

#[test]
fn oversized_history_pack_stays_in_budget() {
    let mut msgs = Vec::new();
    for i in 0..200 {
        msgs.push(PackMessage {
            role: if i % 2 == 0 {
                PackRole::User
            } else {
                PackRole::Assistant
            },
            text: format!("block-{i}-{}", "z".repeat(80)),
        });
    }
    let decision = plan_continuity(&ContinuityRequest {
        from_provider: ProviderKind::Codex,
        to_provider: ProviderKind::Claude,
        from_home: Some(r"C:\a\.codex"),
        to_home: Some(r"C:\a\.claude"),
        session_id: Some("any"),
        messages: &msgs,
        thread_id: None,
        include_pack_if_needed: true,
    });
    let pack = decision
        .pack
        .expect("pack required for cross-provider with history");
    assert_pack_within_budget(&pack, MAX_CONTEXT_CHARS).unwrap();
    assert!(decision.est_tokens > 0);
    assert!(decision.session_id.is_none());
}
