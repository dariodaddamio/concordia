//! The tokens screen behind `/usage` (`t`): what went through Concordia, and where.

use super::token_line::compact;
use super::{App, PickerKind};
use crate::token_log::{breakdown, totals, Entry, Totals};
use crate::update::{PreviewLine, UpdatePreview};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    screen: Option<u64>,
    next: Option<crate::token_log::Cursor>,
    jobs: Vec<ReadJob>,
}

#[derive(Clone, Copy)]
enum ReadKind {
    Screen,
    Landing,
}

struct ReadJob {
    generation: u64,
    conversation: String,
    kind: ReadKind,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<Loaded, String>>,
}

enum Loaded {
    Screen(Vec<PreviewLine>, Option<crate::token_log::Cursor>),
    Landing(u64, u64, bool),
}

const DAY: u64 = 24 * 60 * 60;
/// Chats beyond this many are left out of the per-chat list.
const TOP_CHATS: usize = 8;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Rolling 24-hour and all-time totals from the persistent Concordia log.
#[cfg(test)]
pub(super) fn landing_totals(state_root: &Path) -> (u64, u64) {
    let entries = crate::token_log::load(state_root);
    let since = now().saturating_sub(DAY);
    (
        totals(entries.iter().filter(|e| e.at >= since)).total(),
        totals(&entries).total(),
    )
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
        let sections: [Section; 6] = [
            (
                "By origin",
                Box::new(|e: &Entry| match e.attribution.as_ref().map(|a| a.origin) {
                    Some(crate::token_log::Origin::Main) => "main".into(),
                    Some(crate::token_log::Origin::Worker) => "worker".into(),
                    None => "legacy / unknown attribution".into(),
                }),
            ),
            (
                "By run",
                Box::new(|e: &Entry| {
                    e.attribution
                        .as_ref()
                        .map(|a| a.run.clone())
                        .unwrap_or_else(|| "legacy / unknown run".into())
                }),
            ),
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
            let groups = breakdown(week.iter().copied(), key);
            let omitted = groups.len().saturating_sub(64);
            for (name, t) in groups.into_iter().take(64) {
                out.push(PreviewLine::Bullet(format!("{name:<24}{}", summary(&t))));
            }
            if omitted > 0 {
                out.push(PreviewLine::Text(format!(
                    "{omitted} additional groups omitted from this view"
                )));
            }
        }
        out.push(PreviewLine::Blank);
        out.push(PreviewLine::Kind("Largest chats, last 7 days".into()));
        let by_chat = breakdown(week.iter().copied(), chat_key);
        for (id, t) in by_chat.into_iter().take(TOP_CHATS) {
            let sample = week.iter().find(|e| chat_key(e) == id);
            let name = sample.and_then(|e| chat_title(e)).unwrap_or_else(|| {
                sample
                    .and_then(|e| e.chat.as_deref())
                    .unwrap_or("no session id")
                    .chars()
                    .take(24)
                    .collect()
            });
            out.push(PreviewLine::Bullet(format!("{}  {}", summary(&t), name)));
        }
        if let Some(cost) = w.cost_usd {
            out.push(PreviewLine::Blank);
            out.push(PreviewLine::Text(format!(
                "Provider-reported cost estimate, last 7 days: ${cost:.2} ({} of {} turns)",
                w.reported_cost_events, w.turns
            )));
        } else {
            out.push(PreviewLine::Text(
                "Cost: unknown (no native estimate reported)".into(),
            ));
        }
        if w.incomplete_events > 0 {
            out.push(PreviewLine::Text(format!("Coverage: {} turns have legacy attribution, missing counters or an incomplete outcome. Totals include reported counters only.", w.incomplete_events)));
        }
        if week.iter().any(|entry| entry.provider == "claude") {
            out.push(PreviewLine::Text("Claude token counters cover the reported main loop; native subagent coverage is not inferred. Resumed-session cost is unavailable without a verified delta.".into()));
        }
    }
    out.push(PreviewLine::Blank);
    out.push(PreviewLine::Hint("Esc close".into()));
    out
}

fn chat_key(entry: &Entry) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}",
        entry.provider,
        entry.account,
        entry
            .attribution
            .as_ref()
            .and_then(|a| a.home_id.as_deref())
            .unwrap_or("legacy-home-unknown"),
        entry.chat.as_deref().unwrap_or("no-session")
    )
}

pub(super) fn close(state: &mut State) {
    state.screen = None;
    state.next = None;
    for job in &state.jobs {
        if matches!(job.kind, ReadKind::Screen) {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}

fn start(app: &mut App, cursor: Option<crate::token_log::Cursor>, kind: ReadKind) -> bool {
    if app.token_reads.jobs.len() >= 2 {
        return false;
    }
    app.token_reads.generation = app.token_reads.generation.wrapping_add(1);
    let generation = app.token_reads.generation;
    let root = app.state_root.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let job_cancel = cancel.clone();
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("concordia-usage-read".into())
        .spawn(move || {
            let result =
                crate::token_log::load_page_with_cancel(&root, cursor.as_ref(), &job_cancel)
                    .map_err(|error| error.to_string())
                    .map(|report| match kind {
                        ReadKind::Landing => {
                            let since = now().saturating_sub(DAY);
                            Loaded::Landing(
                                totals(report.entries.iter().filter(|entry| entry.at >= since))
                                    .total(),
                                totals(&report.entries).total(),
                                report.incomplete || totals(&report.entries).incomplete_events > 0,
                            )
                        }
                        ReadKind::Screen => {
                            let mut rows = lines(&report.entries, now(), |entry| {
                                let attribution = entry.attribution.as_ref()?;
                                let provider = crate::commands::parse_provider(&entry.provider)?;
                                let home = crate::accounts::get_account(&root, &entry.account)?
                                    .home_path?
                                    .canonicalize()
                                    .ok()?;
                                if crate::token_log::Attribution::bound_home(Some(&home))
                                    != attribution.home_id
                                {
                                    return None;
                                }
                                crate::session_titles::get(
                                    &root,
                                    provider,
                                    &entry.account,
                                    &home,
                                    entry.chat.as_deref()?,
                                )
                            });
                            if cursor.is_some() || report.incomplete {
                                rows.insert(
                                    0,
                                    PreviewLine::Title(format!(
                                        "Partial page totals · {} records · {} unavailable records",
                                        report.entries.len(),
                                        report.skipped
                                    )),
                                );
                            }
                            if report.next.is_some() {
                                rows.push(PreviewLine::Hint(
                                    "N next page · R first page · Esc close".into(),
                                ));
                            }
                            Loaded::Screen(rows, report.next)
                        }
                    });
            let _ = tx.send(result);
        });
    if spawned.is_err() {
        return false;
    }
    match kind {
        ReadKind::Screen => app.token_reads.screen = Some(generation),
        ReadKind::Landing => super::landing::begin_usage(app, generation),
    }
    app.token_reads.jobs.push(ReadJob {
        generation,
        conversation: app.deleg.d.conversation_id().into(),
        kind,
        cancel,
        rx,
    });
    true
}

pub(super) fn open(app: &mut App) {
    open_page(app, None);
}

fn open_page(app: &mut App, cursor: Option<crate::token_log::Cursor>) {
    close(&mut app.token_reads);
    if !start(app, cursor, ReadKind::Screen) {
        app.flash("Usage reads are finishing; try again shortly");
        return;
    }
    app.update_preview = Some(UpdatePreview {
        history: Vec::new(),
        history_expanded: false,
        lines: vec![PreviewLine::Title("Loading recorded usage…".into())],
        install_available: false,
        release_url: None,
    });
    app.show_picker(PickerKind::Tokens, "tokens".into(), Vec::new(), false);
    app.footer_left = "↑↓ scroll · Esc close".into();
}

pub(super) fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};
    if !app
        .picker
        .as_ref()
        .is_some_and(|picker| picker.kind == PickerKind::Tokens)
        || key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return false;
    }
    match key.code {
        KeyCode::Char('n' | 'N') => {
            if let Some(cursor) = app.token_reads.next.clone() {
                open_page(app, Some(cursor));
            } else {
                app.flash("No later usage page in this snapshot");
            }
            true
        }
        KeyCode::Char('r' | 'R') => {
            open(app);
            true
        }
        _ => false,
    }
}

pub(super) fn poll(app: &mut App) {
    let current_landing = super::landing::usage_generation(app);
    let current_screen = app.token_reads.screen;
    let conversation = app.deleg.d.conversation_id().to_owned();
    let screen_visible = app
        .picker
        .as_ref()
        .is_some_and(|picker| picker.kind == PickerKind::Tokens);
    let mut index = 0;
    while index < app.token_reads.jobs.len() {
        let job = &app.token_reads.jobs[index];
        let valid = match job.kind {
            ReadKind::Screen => {
                screen_visible
                    && current_screen == Some(job.generation)
                    && job.conversation == conversation
            }
            ReadKind::Landing => current_landing == Some(job.generation),
        };
        if !valid {
            job.cancel.store(true, Ordering::Relaxed);
        }
        let result = match job.rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => {
                index += 1;
                continue;
            }
            Err(_) => Err("Usage reader stopped".into()),
        };
        let job = app.token_reads.jobs.swap_remove(index);
        if !valid {
            continue;
        }
        match (job.kind, result) {
            (ReadKind::Landing, result) => {
                let value = match result {
                    Ok(Loaded::Landing(day, total, partial)) => Ok((day, total, partial)),
                    Err(error) => Err(error),
                    _ => Err("Usage reader returned a different view".into()),
                };
                super::landing::apply_usage(app, job.generation, value);
            }
            (ReadKind::Screen, result) => {
                let (rows, next) = match result {
                    Ok(Loaded::Screen(rows, next)) => (rows, next),
                    Err(error) => (
                        vec![
                            PreviewLine::Title("Usage unavailable".into()),
                            PreviewLine::Text(error),
                            PreviewLine::Hint("R retry · Esc close".into()),
                        ],
                        None,
                    ),
                    _ => (
                        vec![PreviewLine::Text(
                            "Usage reader returned a different view".into(),
                        )],
                        None,
                    ),
                };
                app.token_reads.next = next;
                if let Some(preview) = app.update_preview.as_mut() {
                    preview.lines = rows;
                }
                app.footer_left = if app.token_reads.next.is_some() {
                    "N next page · R first page · Esc close"
                } else {
                    "R refresh · Esc close"
                }
                .into();
            }
        }
    }
    if super::landing::needs_usage(app) {
        start(app, None, ReadKind::Landing);
    }
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
            attribution: None,
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

#[cfg(test)]
mod persistence_tests {
    use super::*;

    #[test]
    fn reopening_the_log_keeps_old_usage_in_all_time_only() {
        let root = tempfile::tempdir().unwrap();
        let make = |at, output| Entry {
            at,
            provider: "codex".into(),
            account: "fixture".into(),
            model: None,
            project: "fixture".into(),
            chat: None,
            new_input: 0,
            cached_input: 0,
            output,
            cost_usd: None,
            attribution: None,
        };
        crate::token_log::record(root.path(), &make(now() - 2 * DAY, 500)).unwrap();
        crate::token_log::record(root.path(), &make(now(), 100)).unwrap();
        assert_eq!(landing_totals(root.path()), (100, 600));
        crate::token_log::record(root.path(), &make(now(), 200)).unwrap();
        assert_eq!(landing_totals(root.path()), (300, 800));
    }
}
