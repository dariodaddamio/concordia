//! Provider-independent continuity decisions. The TUI supplies facts and dispatches results.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffDecision {
    Stay,
    FreshSession,
    Rotate,
}

pub fn handoff_decision(
    context_exhausted: bool,
    automatic: bool,
    target_available: bool,
) -> HandoffDecision {
    if context_exhausted {
        HandoffDecision::FreshSession
    } else if automatic && target_available {
        HandoffDecision::Rotate
    } else {
        HandoffDecision::Stay
    }
}

pub fn automatic_rules_respect_same_provider_opt_in(
    automatic: bool,
    same_provider_opt_in: bool,
    terms_accepted: bool,
) -> bool {
    automatic && same_provider_opt_in && terms_accepted
}

pub fn threshold_triggers_fresh_mode(usage_percent: Option<u8>, threshold: u8) -> bool {
    usage_percent.is_some_and(|usage| usage >= threshold)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handoff_decision_table() {
        assert_eq!(handoff_decision(false, false, true), HandoffDecision::Stay);
        assert_eq!(handoff_decision(false, true, true), HandoffDecision::Rotate);
        assert_eq!(
            handoff_decision(true, true, true),
            HandoffDecision::FreshSession
        );
    }
    #[test]
    fn automatic_rules_respect_same_provider_opt_in() {
        assert!(!super::automatic_rules_respect_same_provider_opt_in(
            true, false, true
        ));
        assert!(!super::automatic_rules_respect_same_provider_opt_in(
            true, true, false
        ));
        assert!(super::automatic_rules_respect_same_provider_opt_in(
            true, true, true
        ));
    }
    #[test]
    fn threshold_triggers_fresh_mode() {
        assert!(!super::threshold_triggers_fresh_mode(None, 85));
        assert!(!super::threshold_triggers_fresh_mode(Some(84), 85));
        assert!(super::threshold_triggers_fresh_mode(Some(85), 85));
    }
}
