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

/// Detect only an explicit structured quota signal. Error prose is not a signal.
pub fn structured_usage_limit(provider: ProviderKind, input: &str) -> bool {
    let signals: &[&str] = match provider {
        ProviderKind::Claude => &[
            "rate_limit_error",
            "rate_limit_exceeded",
            "usage_limit_reached",
            "quota_exceeded",
        ],
        ProviderKind::Codex => &[
            "UsageLimitExceeded",
            "usage_limit_reached",
            "quota_exceeded",
            "insufficient_quota",
        ],
        ProviderKind::Cursor | ProviderKind::Grok => &["rate_limit_exceeded", "quota_exceeded", "usage_limit_reached"],
        ProviderKind::Opencode => &["rate_limit", "rate_limit_exceeded", "quota_exceeded"],
    };
    input.lines().any(|line| {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { return false; };
        fn visit(value: &serde_json::Value, signals: &[&str]) -> bool {
            match value {
                serde_json::Value::Object(map) => map.iter().any(|(key, value)| {
                    (signals.iter().any(|signal| key.eq_ignore_ascii_case(signal))
                        || matches!(key.as_str(), "code" | "type" | "errorType" | "error_code")
                        && value.as_str().is_some_and(|text| signals.iter().any(|signal| text.eq_ignore_ascii_case(signal))))
                        || visit(value, signals)
                }),
                serde_json::Value::Array(values) => values.iter().any(|value| visit(value, signals)),
                _ => false,
            }
        }
        visit(&value, signals) || value.get("status").and_then(serde_json::Value::as_u64) == Some(429)
    }) || input.split_whitespace().any(|token| {
        let token = token.split_once('=').map_or(token, |(_, value)| value);
        signals.iter().any(|signal| token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_').eq_ignore_ascii_case(signal))
    })
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
        if structured_usage_limit(provider, &v.to_string()) {
            return FailureKind::Usage;
        }
        if text.contains("overload") || text.contains("capacity") {
            return FailureKind::Overloaded;
        }
        if v.get("type").and_then(|x| x.as_str()) == Some("error") || v.get("error").is_some() {
            return FailureKind::Other;
        }
    }
    if outcome.exit_code == Some(429) || structured_usage_limit(provider, &outcome.stderr_tail) {
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

    #[test]
    fn each_provider_limit_fixture_uses_a_structured_signal() {
        let fixtures = [
            (ProviderKind::Claude, r#"{"type":"error","error":{"type":"rate_limit_error"}}"#),
            (ProviderKind::Codex, r#"{"error":{"codexErrorInfo":{"UsageLimitExceeded":{}}}}"#),
            (ProviderKind::Cursor, r#"{"error":{"code":"rate_limit_exceeded"}}"#),
            (ProviderKind::Grok, r#"{"error":{"code":"quota_exceeded"}}"#),
            (ProviderKind::Opencode, r#"{"error":{"type":"rate_limit"}}"#),
        ];
        for (provider, fixture) in fixtures {
            let outcome = TurnOutcome { stderr_tail: fixture.into(), exit_code: Some(1), ..Default::default() };
            assert_eq!(classify_failure(provider, &outcome), FailureKind::Usage);
        }
        let ordinary = TurnOutcome { stderr_tail: r#"{"type":"error","message":"usage limit mentioned in prose"}"#.into(), exit_code: Some(1), ..Default::default() };
        assert_eq!(classify_failure(ProviderKind::Claude, &ordinary), FailureKind::Other);
    }
}
