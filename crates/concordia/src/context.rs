//! Cross-provider / cross-account continuity — bounded packs, never `--last`.
//!
//! Contract (mock-first, from P0 docs + PLAN):
//! - Same provider + same home + explicit session id → native resume, **no pack**.
//! - Otherwise → new native session; the pack is the thread *map* (`threads::build_map`,
//!   at most `MAP_CHARS`), never a transcript dump; never pass a foreign session id into
//!   another home/provider.

use crate::store::ProviderKind;
use crate::threads;

/// Character budget for the context pack (a char budget, not a tokenizer).
pub const MAX_CONTEXT_CHARS: usize = 6000;

/// Rough estimate only (no live tokenizer / model). ~4 chars per token.
pub fn estimate_tokens(chars: usize) -> usize {
    chars.div_ceil(4)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackRole {
    User,
    Assistant,
    /// A tool/command card; `text` is `threads::encode_card(title, detail, status)`.
    Card,
}

#[derive(Debug, Clone)]
pub struct PackMessage {
    pub role: PackRole,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ContextPack {
    pub text: String,
    pub labeled: String,
    pub from_provider: ProviderKind,
    pub to_provider: ProviderKind,
    pub pack_chars: usize,
    pub est_tokens: usize,
    pub message_count: usize,
}

#[derive(Debug, Clone)]
pub struct ContinuityDecision {
    /// True only when same provider + same home + non-empty session id.
    pub native_resume: bool,
    /// Session id to pass to the provider — **None** unless native_resume.
    pub session_id: Option<String>,
    /// Pack prepended to the next user prompt — **None** when native_resume or empty history.
    pub pack: Option<ContextPack>,
    pub pack_chars: usize,
    pub est_tokens: usize,
    pub reason: &'static str,
}

#[derive(Debug, Clone)]
pub struct ContinuityRequest<'a> {
    pub from_provider: ProviderKind,
    pub to_provider: ProviderKind,
    pub from_home: Option<&'a str>,
    pub to_home: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub messages: &'a [PackMessage],
    /// Id of the stored thread log, when recording is on (the map's `more:` line points at it).
    pub thread_id: Option<&'a str>,
    /// When false, decide switch policy only (no pack body) — e.g. account bind before next turn.
    pub include_pack_if_needed: bool,
}

pub fn should_attempt_native_resume(
    from_provider: ProviderKind,
    to_provider: ProviderKind,
    from_home: Option<&str>,
    to_home: Option<&str>,
    session_id: Option<&str>,
) -> bool {
    let Some(sid) = session_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let _ = sid;
    if from_provider != to_provider {
        return false;
    }
    match (from_home, to_home) {
        (Some(a), Some(b)) => normalize_home(a) == normalize_home(b),
        _ => false,
    }
}

/// Single planner used by TUI + tests — omit pack when native resume applies.
pub fn plan_continuity(req: &ContinuityRequest<'_>) -> ContinuityDecision {
    plan_continuity_capped(req, MAX_CONTEXT_CHARS)
}

/// Same planner with a caller-chosen (smaller) pack budget, e.g. for delegated workers.
pub fn plan_continuity_capped(req: &ContinuityRequest<'_>, max_chars: usize) -> ContinuityDecision {
    let native = should_attempt_native_resume(
        req.from_provider,
        req.to_provider,
        req.from_home,
        req.to_home,
        req.session_id,
    );
    if native {
        return ContinuityDecision {
            native_resume: true,
            session_id: req
                .session_id
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string()),
            pack: None,
            pack_chars: 0,
            est_tokens: 0,
            reason: "same provider+home+explicit session — native resume; pack omitted",
        };
    }

    let reason = continuity_reason(req);
    let pack = if req.include_pack_if_needed {
        build_context_pack(
            req.messages,
            req.from_provider,
            req.to_provider,
            max_chars.min(MAX_CONTEXT_CHARS),
            req.thread_id,
        )
    } else {
        None
    };
    let pack_chars = pack.as_ref().map(|p| p.pack_chars).unwrap_or(0);
    let est_tokens = pack.as_ref().map(|p| p.est_tokens).unwrap_or(0);
    ContinuityDecision {
        native_resume: false,
        session_id: None, // never leak foreign / cross-home session ids
        pack,
        pack_chars,
        est_tokens,
        reason,
    }
}

/// The reason for an ordinary new chat; it is not worth a notice.
pub const NEW_SESSION: &str = "New session.";

fn continuity_reason(req: &ContinuityRequest<'_>) -> &'static str {
    if req
        .session_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        return NEW_SESSION;
    }
    if req.from_provider != req.to_provider {
        return "New session: you switched provider, so tool and approval state does not carry over.";
    }
    match (req.from_home, req.to_home) {
        (Some(a), Some(b)) if normalize_home(a) != normalize_home(b) => {
            "New session: this account cannot read the other account's history, so only the chat map carries over."
        }
        _ => "New session: Concordia could not confirm this is the same account.",
    }
}

/// The pack is the deterministic thread map (no transcript), at most
/// `min(max_chars, MAP_CHARS)` characters. `thread_id` is Some when the log is stored.
pub fn build_context_pack(
    messages: &[PackMessage],
    from: ProviderKind,
    to: ProviderKind,
    max_chars: usize,
    thread_id: Option<&str>,
) -> Option<ContextPack> {
    build_context_pack_with_workspace(messages, from, to, max_chars, thread_id, None)
}

pub fn build_context_pack_with_workspace(
    messages: &[PackMessage],
    from: ProviderKind,
    to: ProviderKind,
    max_chars: usize,
    thread_id: Option<&str>,
    workspace: Option<&crate::workspace_state::WorkspaceState>,
) -> Option<ContextPack> {
    let count = messages
        .iter()
        .filter(|m| m.role != PackRole::Card && !m.text.trim().is_empty())
        .count();
    if count == 0 {
        return None;
    }
    let mut events = threads::events_from_messages(messages, from.as_str());
    let mut sw = threads::ThreadEvent::new("switch");
    sw.t = 0;
    sw.provider = Some(to.as_str().to_string());
    events.push(sw);
    let budget = max_chars.min(threads::MAP_CHARS);
    let text = threads::build_map_v2_with_workspace(
        &events,
        thread_id.unwrap_or(""),
        budget,
        thread_id.is_some(),
        workspace,
    );
    let labeled = text.lines().next().unwrap_or("").to_string();
    let pack_chars = text.chars().count();
    Some(ContextPack {
        text,
        labeled,
        from_provider: from,
        to_provider: to,
        pack_chars,
        est_tokens: estimate_tokens(pack_chars),
        message_count: count,
    })
}

/// Assert pack never exceeds budget (fixture gate).
pub fn assert_pack_within_budget(pack: &ContextPack, max_chars: usize) -> Result<(), String> {
    if pack.pack_chars > max_chars {
        return Err(format!(
            "pack {} chars exceeds budget {} ",
            pack.pack_chars, max_chars
        ));
    }
    Ok(())
}

fn normalize_home(p: &str) -> String {
    p.replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_home_allows_resume() {
        assert!(should_attempt_native_resume(
            ProviderKind::Codex,
            ProviderKind::Codex,
            Some(r"C:\Users\x\.codex"),
            Some(r"C:\Users\x\.codex"),
            Some("11111111-2222-3333-4444-555555555555"),
        ));
    }

    #[test]
    fn other_home_forces_new_session() {
        assert!(!should_attempt_native_resume(
            ProviderKind::Codex,
            ProviderKind::Codex,
            Some(r"C:\Users\x\.codex"),
            Some(r"C:\Users\x\.codex-2"),
            Some("11111111-2222-3333-4444-555555555555"),
        ));
    }

    #[test]
    fn native_omits_pack() {
        let msgs = [PackMessage {
            role: PackRole::User,
            text: "hello".into(),
        }];
        let d = plan_continuity(&ContinuityRequest {
            from_provider: ProviderKind::Codex,
            to_provider: ProviderKind::Codex,
            from_home: Some(r"C:\a\.codex"),
            to_home: Some(r"C:\a\.codex"),
            session_id: Some("sess"),
            messages: &msgs,
            thread_id: None,
            include_pack_if_needed: true,
        });
        assert!(d.native_resume);
        assert_eq!(d.session_id.as_deref(), Some("sess"));
        assert!(d.pack.is_none());
        assert_eq!(d.pack_chars, 0);
    }

    #[test]
    fn cross_home_clears_session_and_packs() {
        let msgs = [
            PackMessage {
                role: PackRole::User,
                text: "a".into(),
            },
            PackMessage {
                role: PackRole::Assistant,
                text: "b".into(),
            },
        ];
        let d = plan_continuity(&ContinuityRequest {
            from_provider: ProviderKind::Codex,
            to_provider: ProviderKind::Codex,
            from_home: Some(r"C:\a\.codex"),
            to_home: Some(r"C:\a\.codex-2"),
            session_id: Some("foreign-sess"),
            messages: &msgs,
            thread_id: None,
            include_pack_if_needed: true,
        });
        assert!(!d.native_resume);
        assert!(d.session_id.is_none(), "must not leak foreign session id");
        assert!(d.pack.is_some());
        assert!(d.pack_chars > 0);
        assert_pack_within_budget(d.pack.as_ref().unwrap(), threads::MAP_CHARS).unwrap();
    }

    #[test]
    fn pack_skips_system_and_respects_budget() {
        let mut msgs = vec![PackMessage {
            role: PackRole::User,
            text: "x".repeat(100),
        }];
        // many assistants
        for _ in 0..50 {
            msgs.push(PackMessage {
                role: PackRole::Assistant,
                text: "y".repeat(200),
            });
        }
        let pack = build_context_pack(&msgs, ProviderKind::Codex, ProviderKind::Claude, 500, None)
            .unwrap();
        assert!(pack.pack_chars <= 500);
        assert!(!pack.text.contains("system:"));
    }

    #[test]
    fn cross_provider_new_session() {
        let d = plan_continuity(&ContinuityRequest {
            from_provider: ProviderKind::Codex,
            to_provider: ProviderKind::Claude,
            from_home: Some(r"C:\a\.codex"),
            to_home: Some(r"C:\a\.claude"),
            session_id: Some("sess"),
            messages: &[],
            thread_id: None,
            include_pack_if_needed: true,
        });
        assert!(!d.native_resume);
        assert!(d.session_id.is_none());
        assert!(d.pack.is_none()); // empty history
    }

    #[test]
    fn continuity_reasons_are_user_facing() {
        let request = |from_provider, to_provider, from_home, to_home, session_id| {
            plan_continuity(&ContinuityRequest {
                from_provider,
                to_provider,
                from_home,
                to_home,
                session_id,
                messages: &[],
                thread_id: None,
                include_pack_if_needed: false,
            })
            .reason
        };
        assert_eq!(
            request(ProviderKind::Codex, ProviderKind::Codex, None, None, None),
            "New session."
        );
        assert_eq!(
            request(
                ProviderKind::Codex,
                ProviderKind::Claude,
                Some("codex"),
                Some("claude"),
                Some("session")
            ),
            "New session: you switched provider, so tool and approval state does not carry over."
        );
        assert_eq!(
            request(
                ProviderKind::Codex,
                ProviderKind::Codex,
                Some("one"),
                Some("two"),
                Some("session")
            ),
            "New session: this account cannot read the other account's history, so only the chat map carries over."
        );
        assert_eq!(
            request(
                ProviderKind::Codex,
                ProviderKind::Codex,
                None,
                Some("two"),
                Some("session")
            ),
            "New session: Concordia could not confirm this is the same account."
        );
    }

    fn long_history() -> Vec<PackMessage> {
        let mut v = Vec::new();
        for i in 0..40 {
            v.push(PackMessage {
                role: PackRole::User,
                text: format!("question {i} {}", "x".repeat(300)),
            });
            v.push(PackMessage {
                role: PackRole::Assistant,
                text: format!("answer {i} {}", "y".repeat(900)),
            });
        }
        v
    }

    #[test]
    fn same_account_resume_omits_the_pack() {
        let msgs = long_history();
        let d = plan_continuity(&ContinuityRequest {
            from_provider: ProviderKind::Claude,
            to_provider: ProviderKind::Claude,
            from_home: Some(r"C:\.claude"),
            to_home: Some(r"C:\.claude"),
            session_id: Some("sess"),
            messages: &msgs,
            thread_id: Some("tid"),
            include_pack_if_needed: true,
        });
        assert!(d.native_resume && d.pack.is_none());
    }

    #[test]
    fn a_switch_yields_a_map_under_budget_with_the_pointer() {
        let msgs = long_history();
        let d = plan_continuity(&ContinuityRequest {
            from_provider: ProviderKind::Claude,
            to_provider: ProviderKind::Codex,
            from_home: Some(r"C:\.claude"),
            to_home: Some(r"C:\.codex"),
            session_id: Some("sess"),
            messages: &msgs,
            thread_id: Some("3f9a1111-2222-3333-4444-555555555555"),
            include_pack_if_needed: true,
        });
        let p = d.pack.unwrap();
        assert!(p.pack_chars <= threads::MAP_CHARS, "{}", p.pack_chars);
        assert!(p
            .text
            .contains("concordia thread read 3f9a1111-2222-3333-4444-555555555555"));
        assert!(p.labeled.contains("tools/approvals NOT transferred"));
        assert!(!p.text.contains("question 3 "), "no transcript dump");
    }

    #[test]
    fn log_off_still_yields_a_map_from_memory() {
        let msgs = long_history();
        let p = build_context_pack(&msgs, ProviderKind::Claude, ProviderKind::Codex, 6000, None)
            .unwrap();
        assert!(p.text.contains("not stored"));
        assert!(p.text.contains("asked: question 0"));
        assert!(p.pack_chars <= threads::MAP_CHARS);
    }
}
