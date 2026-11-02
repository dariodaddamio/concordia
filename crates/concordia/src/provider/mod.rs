pub mod claude;
pub mod codex;
pub mod compat;
pub mod cursor;
pub mod enforcement;
pub mod launch;
pub(crate) mod npm;
mod session_guard;
pub mod stream_cards;
pub mod stream_json;
pub mod turn;

pub use stream_cards::{format_card_line, upsert_card, CardStatus, StreamCard};
pub use turn::{
    plan_provider_turn, run_provider_turn, run_provider_turn_with_progress, spawn_and_cancel_mock,
    PlannedTurn, ProviderTurnRequest, RunningTurn, TurnOutcome, TurnProgress,
};

/// Explicit provider CLI path from the environment (custom installs, and the fake provider in
/// tests). Empty values are ignored.
pub fn bin_override(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}
