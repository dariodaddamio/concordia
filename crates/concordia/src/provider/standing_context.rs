//! The short, deterministic identity supplied to every provider session.

use crate::store::ProviderKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State<'a> {
    pub provider: ProviderKind,
    pub account: &'a str,
    pub role: &'a str,
    pub tools: &'a [String],
    pub handoff_map: Option<&'a str>,
}

/// Return the Concordia tools exposed by the selected provider transport.
/// The MCP catalog and the ACP question bridge are separate registrations.
pub fn tools_for_transport(
    provider: ProviderKind,
    mode: crate::native_input::Mode,
) -> Vec<String> {
    if matches!(provider, ProviderKind::Claude | ProviderKind::Codex) {
        crate::mcp::registered_tool_names()
    } else if matches!(
        (provider, mode),
        (
            ProviderKind::Cursor,
            crate::native_input::Mode::CursorAcp
        ) | (ProviderKind::Grok, crate::native_input::Mode::GrokAcp)
            | (ProviderKind::Opencode, crate::native_input::Mode::OpencodeAcp)
    ) {
        vec!["ask_user".into()]
    } else {
        Vec::new()
    }
}

pub fn render(state: &State<'_>) -> String {
    let tools = if state.tools.is_empty() {
        "none".to_owned()
    } else {
        state.tools.join(", ")
    };
    let guidance = match (state.tools.iter().any(|tool| tool == "agents"), state.tools.iter().any(|tool| tool == "ask_user")) {
        (true, true) => " Use agents to delegate work and ask_user to ask the user to choose or clarify.",
        (true, false) => " Use agents to delegate work. Ask through the provider's native question surface when it provides one.",
        (false, true) => " Use ask_user to ask the user to choose or clarify.",
        (false, false) => " No Concordia tools are registered in this session.",
    };
    let handoff = state
        .handoff_map
        .filter(|map| !map.trim().is_empty())
        .map(|map| format!(" Continued-task handoff map: {}.", compact(map)))
        .unwrap_or_default();
    let text = format!(
        "You are running inside Concordia, a native Windows app that hosts provider CLIs. The user reads your replies in Concordia's window. This session is {provider}, account {account}, role {role}. Registered Concordia tools: {tools}.{guidance} Do not tell the user to perform work Concordia can do. Preserve the provider's native approvals and session identity.{handoff}",
        provider = state.provider.as_str(),
        account = compact(state.account),
        role = compact(state.role),
        tools = compact(&tools),
        guidance = guidance,
    );
    text.split_whitespace().take(149).collect::<Vec<_>>().join(" ")
}

pub fn tag(text: &str) -> String {
    if text.contains("<concordia_standing_context>") {
        return text.to_owned();
    }
    format!("<concordia_standing_context>{text}</concordia_standing_context>")
}

/// Return whether a completed provider compaction has not already been handled.
pub(crate) fn should_reinject_context(last: &mut Option<String>, id: &str) -> bool {
    if id.is_empty() || last.as_deref() == Some(id) {
        return false;
    }
    *last = Some(id.to_owned());
    true
}

pub fn extract(prefix: Option<&str>) -> (Option<&str>, String) {
    let Some(prefix) = prefix else { return (None, String::new()) };
    let Some(start) = prefix.find("<concordia_standing_context>") else {
        return (None, prefix.to_owned());
    };
    let content_start = start + "<concordia_standing_context>".len();
    let Some(end_rel) = prefix[content_start..].find("</concordia_standing_context>") else {
        return (None, prefix.to_owned());
    };
    let end = content_start + end_rel;
    let rest = format!("{}{}", &prefix[..start], &prefix[end + "</concordia_standing_context>".len()..]);
    (Some(&prefix[content_start..end]), rest)
}

fn compact(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(240)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_recorded_identity_tools_and_handoff_under_budget() {
        let tools = vec!["agents".into(), "ask_user".into()];
        let text = render(&State {
            provider: ProviderKind::Claude,
            account: "acct-a",
            role: "reviewer",
            tools: &tools,
            handoff_map: Some("source map 42"),
        });
        assert!(text.split_whitespace().count() < 150);
        assert!(text.contains("inside Concordia"));
        assert!(text.contains("agents, ask_user"));
        assert!(text.contains("source map 42"));
        assert_eq!(tag(&tag("identity")), tag("identity"));
    }
}
