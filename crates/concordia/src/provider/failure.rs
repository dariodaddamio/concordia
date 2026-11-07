use super::turn::TurnOutcome;
use crate::store::ProviderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Usage,
    Context,
    Overloaded,
    Other,
    None,
}

pub fn classify_failure(provider: ProviderKind, outcome: &TurnOutcome) -> FailureKind {
    let mut structured = Vec::new();
    for line in outcome.stderr_tail.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            structured.push(v);
        }
    }
    for v in &structured {
        let text = [
            v.get("code"),
            v.pointer("/error/code"),
            v.get("type"),
            v.pointer("/error/type"),
        ]
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
        if text.contains("context") || text.contains("context_window") {
            return FailureKind::Context;
        }
        if text.contains("quota") || text.contains("usage") || text.contains("rate_limit") {
            return FailureKind::Usage;
        }
        if text.contains("overload") || text.contains("capacity") {
            return FailureKind::Overloaded;
        }
        if v.get("type").and_then(|x| x.as_str()) == Some("error") || v.get("error").is_some() {
            return FailureKind::Other;
        }
    }
    if outcome.exit_code == Some(429) {
        return FailureKind::Usage;
    }
    let fallback = classify_failure_text(provider, &outcome.stderr_tail, outcome.exit_code);
    if fallback != FailureKind::None {
        fallback
    } else if outcome.exit_code.is_some_and(|code| code != 0) {
        FailureKind::Other
    } else {
        FailureKind::None
    }
}

pub fn classify_failure_text(
    provider: ProviderKind,
    input: &str,
    exit_code: Option<i32>,
) -> FailureKind {
    let text = input.to_ascii_lowercase();
    let kind = if exit_code == Some(429) || crate::provider::stream_cards::usage_exhausted(&text) {
        FailureKind::Usage
    } else if crate::provider::stream_cards::model_overloaded(&text) {
        FailureKind::Overloaded
    } else if text.contains("context_length_exceeded")
        || text.contains("context_window_exceeded")
        || text.contains("maximum context length exceeded")
    {
        FailureKind::Context
    } else {
        FailureKind::None
    };
    let _ = provider;
    kind
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structured_codes_win_over_provider_wording() {
        let outcome = TurnOutcome {
            stderr_tail:
                r#"{"error":{"code":"context_window_exceeded"},"message":"temporary issue"}"#.into(),
            exit_code: Some(1),
            ..Default::default()
        };
        assert_eq!(
            classify_failure(ProviderKind::Codex, &outcome),
            FailureKind::Context
        );
        assert_eq!(
            classify_failure_text(ProviderKind::Claude, "quota exceeded", Some(1)),
            FailureKind::Usage
        );
        assert_eq!(
            classify_failure_text(ProviderKind::Cursor, "unrelated", Some(0)),
            FailureKind::None
        );
    }
}
