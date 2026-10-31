//! The tokens screen behind `/usage` (`t`): what went through Concordia, and where.

use super::token_line::compact;
use super::{App, PickerKind};
use crate::token_log::{breakdown, load, totals, Entry, Totals};
use crate::update::{PreviewLine, UpdatePreview};
use std::path::Path;

const DAY: u64 = 24 * 60 * 60;
/// Chats beyond this many are left out of the per-chat list.
const TOP_CHATS: usize = 8;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Total tokens in the last 24 hours, for the landing page.
pub(super) fn last_day_total(state_root: &Path) -> u64 {
    let since = now().saturating_sub(DAY);
    totals(load(state_root).iter().filter(|e| e.at >= since)).total()
}

fn summary(t: &Totals) -> String {
    let turns = if t.turns == 1 { "turn" } else { "turns" };
    format!("{} tokens · {} {turns}", compact(t.total()), t.turns)
}

fn share(part: u64, whole: u64) -> String {
    let percent = (part * 100 + whole / 2).checked_div(whole).unwrap_or(0);
    format!("{percent}%")
}

/// A breakdown's title and how it names an entry's group.
type Section<'a> = (&'a str, Box<dyn Fn(&Entry) -> String>);

/// The screen's lines for `entries` as of `now`; `chat_title` names a chat by account and id.
pub fn lines(
    entries: &[Entry],
    now: u64,
    chat_title: impl Fn(&Entry) -> Option<String>,
) -> Vec<PreviewLine> {
    if entries.is_empty() {
        return vec![
            PreviewLine::Title("No turns recorded yet.".into()),
            PreviewLine::Blank,
            PreviewLine::Hint("Esc close".into()),
        ];
    }
    let week: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.at >= now.saturating_sub(7 * DAY))
        .collect();
    let day = totals(entries.iter().filter(|e| e.at >= now.saturating_sub(DAY)));
    let mut out = vec![
        PreviewLine::Field("24 hours", summary(&day)),
        PreviewLine::Field("7 days", summary(&totals(week.iter().copied()))),
        PreviewLine::Field("All time", summary(&totals(entries))),
        PreviewLine::Blank,
    ];
    let w = totals(week.iter().copied());
    if w.turns > 0 {
        out.push(PreviewLine::Title("Where they went, last 7 days".into()));
        for (label, n) in [
            ("Re-read from the cache", w.cached_input),
            ("New input", w.new_input),
            ("Output", w.output),
        ] {
            out.push(PreviewLine::Bullet(format!(
                "{label:<24}{:>7} · {}",
                compact(n),
                share(n, w.total())
            )));
        }
        let sections: [Section; 4] = [
            ("By provider", Box::new(|e: &Entry| e.provider.clone())),
            (
                "By model",
                Box::new(|e: &Entry| e.model.clone().unwrap_or_else(|| "not reported".into())),
            ),
            ("By account", Box::new(|e: &Entry| e.account.clone())),
            (
                "By project",
                Box::new(|e: &Entry| {
                    Path::new(&e.project)
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| e.project.clone())
                }),
            ),
        ];
        for (title, key) in sections {
            out.push(PreviewLine::Blank);
            out.push(PreviewLine::Kind(format!("{title}, last 7 days")));
            for (name, t) in breakdown(week.iter().copied(), key) {
                out.push(PreviewLine::Bullet(format!("{name:<24}{}", summary(&t))));
            }
        }
        out.push(PreviewLine::Blank);
        out.push(PreviewLine::Kind("Largest chats, last 7 days".into()));
        let by_chat = breakdown(week.iter().copied(), |e| {
            e.chat.clone().unwrap_or_else(|| "(no session id)".into())
        });
        for (id, t) in by_chat.into_iter().take(TOP_CHATS) {
            let sample = week.iter().find(|e| e.chat.as_deref() == Some(id.as_str()));
            let name = sample
                .and_then(|e| chat_title(e))
                .unwrap_or_else(|| id.chars().take(24).collect());
            out.push(PreviewLine::Bullet(format!("{}  {}", summary(&t), name)));
        }
        if let Some(cost) = w.cost_usd {
            out.push(PreviewLine::Blank);
            out.push(PreviewLine::Text(format!(
                "Reported cost, last 7 days: ${cost:.2} (providers that report it)"
            )));
        }
    }
    out.push(PreviewLine::Blank);
    out.push(PreviewLine::Hint("Esc close".into()));
    out
}

pub(super) fn open(app: &mut App) {
    let entries = load(&app.state_root);
    let root = app.state_root.clone();
    let lines = lines(&entries, now(), |e| {
        let provider = crate::commands::parse_provider(&e.provider)?;
        let home = crate::accounts::get_account(&root, &e.account)?.home_path?;
        crate::session_titles::get(&root, provider, &e.account, &home, e.chat.as_deref()?)
    });
    app.update_preview = Some(UpdatePreview {
        lines,
        install_available: false,
        release_url: None,
    });
    app.show_picker(PickerKind::Tokens, "tokens".into(), Vec::new(), false);
    app.footer_left = "↑↓ scroll · Esc close".into();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(provider: &str, at: u64, parts: (u64, u64, u64), chat: &str) -> Entry {
        Entry {
            at,
            provider: provider.into(),
            account: "a".into(),
            model: Some("m".into()),
            project: "C:/work/concordia".into(),
            chat: Some(chat.into()),
            new_input: parts.0,
            cached_input: parts.1,
            output: parts.2,
            cost_usd: None,
        }
    }

    #[test]
    fn the_screen_totals_by_period_and_splits_where_tokens_went() {
        let now = 100 * DAY;
        let entries = [
            entry("codex", now - 60, (1000, 3000, 0), "c1"),
            entry("cursor", now - 2 * DAY, (0, 0, 1000), "c2"),
            entry("cursor", now - 30 * DAY, (5000, 0, 0), "old"),
        ];
        let text: Vec<String> = lines(&entries, now, |_| None)
            .iter()
            .map(PreviewLine::plain)
            .collect();
        assert_eq!(text[0], "24 hours   4k tokens · 1 turn");
        assert_eq!(text[1], "7 days     5k tokens · 2 turns");
        assert_eq!(text[2], "All time   10k tokens · 3 turns");
        assert!(text
            .iter()
            .any(|l| l.contains("Re-read from the cache") && l.ends_with("60%")));
        assert!(text
            .iter()
            .any(|l| l.contains("Output") && l.ends_with("20%")));
        assert!(text.contains(&" · codex                   4k tokens · 1 turn".to_string()));
        assert!(text.iter().any(|l| l.contains("concordia")));
        assert!(!text.iter().any(|l| l.contains("old")), "older than 7 days");
    }

    #[test]
    fn an_empty_log_says_so() {
        let text: Vec<String> = lines(&[], 0, |_| None)
            .iter()
            .map(PreviewLine::plain)
            .collect();
        assert_eq!(text[0], "No turns recorded yet.");
    }
}
