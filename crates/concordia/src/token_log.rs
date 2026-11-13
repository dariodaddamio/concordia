//! Per-attempt token usage, so totals and breakdowns cost no provider
//! call. Providers count differently: Codex's input includes the cached part, Claude's and
//! Cursor's do not. Entries store the same three parts for every provider.

use crate::provider::stream_cards::TurnUsage;
use crate::store::ProviderKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
#[path = "token_events.rs"]
mod events;
pub use events::{
    load_attempt, load_page, load_page_with_cancel, Attribution, Cursor, Origin, Report,
};

pub fn attempt_outcome(exit: Option<i32>, cancelled: bool, failed: bool) -> (&'static str, bool) {
    let status = if cancelled {
        "cancelled"
    } else if exit.is_none() {
        "uncertain"
    } else if failed || exit != Some(0) {
        "failed"
    } else {
        "succeeded"
    };
    (status, exit.is_some())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Seconds since the epoch when the turn finished.
    pub at: u64,
    pub provider: String,
    pub account: String,
    pub model: Option<String>,
    /// The folder the turn ran in.
    pub project: String,
    /// The provider's session id, when it reported one.
    pub chat: Option<String>,
    /// Input the provider had to read fresh.
    pub new_input: u64,
    /// Input served from the provider's prompt cache (re-read context).
    pub cached_input: u64,
    pub output: u64,
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<Attribution>,
}

impl Entry {
    pub fn total(&self) -> u64 {
        self.new_input
            .saturating_add(self.cached_input)
            .saturating_add(self.output)
    }
}

/// The three parts of a turn's tokens, the same way for every provider.
pub fn split(provider: ProviderKind, usage: &TurnUsage) -> (u64, u64, u64) {
    let input = usage.input.unwrap_or(0);
    let cached = usage.cached.unwrap_or(0);
    let new_input = match provider {
        ProviderKind::Codex => input.saturating_sub(cached),
        ProviderKind::Claude
        | ProviderKind::Cursor
        | ProviderKind::Grok
        | ProviderKind::Opencode => input,
    };
    (new_input, cached, usage.output.unwrap_or(0))
}

/// Record one attributed turn.
pub fn record(state_root: &Path, entry: &Entry) -> std::io::Result<()> {
    if entry.attribution.is_none() {
        return Err(std::io::Error::other(
            "usage entry requires account and run attribution",
        ));
    }
    events::record_once(state_root, entry)
}

/// Compatibility convenience for the first bounded page. Use `load_page` for coverage/cursors.
pub fn load(state_root: &Path) -> Vec<Entry> {
    load_page(state_root, None)
        .map(|report| report.entries)
        .unwrap_or_default()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Totals {
    pub turns: u64,
    pub new_input: u64,
    pub cached_input: u64,
    pub output: u64,
    /// Summed only over turns that reported a cost.
    pub cost_usd: Option<f64>,
    pub reported_cost_events: u64,
    pub incomplete_events: u64,
}

impl Totals {
    pub fn add(&mut self, e: &Entry) {
        self.turns = self.turns.saturating_add(1);
        self.new_input = self.new_input.saturating_add(e.new_input);
        self.cached_input = self.cached_input.saturating_add(e.cached_input);
        self.output = self.output.saturating_add(e.output);
        if let Some(c) = e.cost_usd {
            self.cost_usd = Some(self.cost_usd.unwrap_or(0.0) + c);
            self.reported_cost_events = self.reported_cost_events.saturating_add(1);
        }
        if e.attribution.as_ref().is_none_or(|a| {
            !a.input_known
                || !a.cached_known
                || !a.output_known
                || !a.complete
                || a.home_id.is_none()
        }) {
            self.incomplete_events = self.incomplete_events.saturating_add(1);
        }
    }

    pub fn total(&self) -> u64 {
        self.new_input
            .saturating_add(self.cached_input)
            .saturating_add(self.output)
    }
}

pub fn totals<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Totals {
    let mut t = Totals::default();
    for e in entries {
        t.add(e);
    }
    t
}

/// Totals per key, largest first.
pub fn breakdown<'a>(
    entries: impl IntoIterator<Item = &'a Entry>,
    key: impl Fn(&Entry) -> String,
) -> Vec<(String, Totals)> {
    let mut groups: BTreeMap<String, Totals> = BTreeMap::new();
    for e in entries {
        groups.entry(key(e)).or_default().add(e);
    }
    let mut out: Vec<_> = groups.into_iter().collect();
    out.sort_by(|a, b| b.1.total().cmp(&a.1.total()).then_with(|| a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(provider: &str, project: &str, at: u64, parts: (u64, u64, u64)) -> Entry {
        Entry {
            at,
            provider: provider.into(),
            account: "a".into(),
            model: None,
            project: project.into(),
            chat: None,
            new_input: parts.0,
            cached_input: parts.1,
            output: parts.2,
            cost_usd: None,
            attribution: None,
        }
    }

    #[test]
    fn codex_input_includes_its_cached_part_and_the_others_do_not() {
        let usage = TurnUsage {
            input: Some(2000),
            cached: Some(1500),
            output: Some(100),
            cost_usd: None,
        };
        assert_eq!(split(ProviderKind::Codex, &usage), (500, 1500, 100));
        assert_eq!(split(ProviderKind::Claude, &usage), (2000, 1500, 100));
        assert_eq!(split(ProviderKind::Cursor, &usage), (2000, 1500, 100));
    }

    #[test]
    fn breakdowns_sum_per_key_largest_first() {
        let entries = [
            entry("codex", "a", 1, (10, 0, 0)),
            entry("cursor", "b", 1, (5, 5, 5)),
            entry("codex", "b", 1, (1, 1, 1)),
        ];
        let by = breakdown(&entries, |e| e.provider.clone());
        assert_eq!(by[0].0, "cursor");
        assert_eq!(by[0].1.total(), 15);
        assert_eq!(by[1].1.total(), 13);
        assert_eq!(by[1].1.turns, 2);
        assert_eq!(totals(&entries).cached_input, 6);
    }
}
