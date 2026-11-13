//! Account limits: what is left of each provider's rolling windows.
//!
//! Codex reports its limits in its own session files. Claude answers `claude -p "/usage"`
//! locally (no model call) but takes seconds, so it is fetched on a background thread and cached
//! (see [`UsageFetcher`]). Cursor's CLI has no usage command; its interactive `/usage` screen is
//! read in a pseudo-console (see `cursor_usage`) the same way, and where that fails the account
//! just says the limits are not shown.

use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// How long a fetched Claude reading is trusted before it is refreshed in the background.
pub const USAGE_TTL_SECS: u64 = 120;
pub const CREDIT_DASHBOARD_URL: &str = "https://chatgpt.com/settings/usage";
/// Supported native read-only RPC; exposed for isolated process integration checks.
pub async fn read_codex_limits(bin: &str, home: &Path) -> Result<String, String> {
    crate::usage_codex::fetch_with(bin, home).await
}
/// A hung `claude -p "/usage"` is killed (with its process tree) after this long.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    /// Short name: `5h`, `7d`, `7d Opus`.
    pub label: String,
    pub used_pct: f64,
    /// When the window resets, already human-readable (`3h`, `Oct 21, 9am`).
    pub resets: Option<String>,
}

impl UsageWindow {
    /// The reading predates the window's reset, so its percentage no longer applies.
    fn expired(&self) -> bool {
        self.expired_at(local_now())
    }

    pub(crate) fn expired_for_ui(&self) -> bool {
        local_now()
            .map(|now| self.expired_for_ui_at((now.month, now.day, now.hour, now.minute)))
            .unwrap_or_else(|| self.expired())
    }

    /// Deterministic clock seam for UI rendering tests: `(month, day, hour, minute)` local time.
    pub(crate) fn expired_for_ui_at(&self, now: (u32, u32, u32, u32)) -> bool {
        self.expired_at(Some(WallTime {
            month: now.0,
            day: now.1,
            hour: now.2,
            minute: now.3,
        }))
    }

    fn expired_at(&self, now: Option<WallTime>) -> bool {
        self.resets.as_deref().is_some_and(|r| {
            r.starts_with("already reset")
                || r.starts_with("reset at ")
                || now
                    .zip(parse_wall(r, now))
                    .is_some_and(|(now, then)| minutes_until(now, then) <= 0)
        })
    }
    pub fn left_pct(&self) -> f64 {
        (100.0 - self.used_pct).clamp(0.0, 100.0)
    }

    fn used_i(&self) -> i64 {
        self.used_pct.clamp(0.0, 100.0).round() as i64
    }

    fn left_i(&self) -> i64 {
        self.left_pct().round() as i64
    }

    /// `5h 59% left (41% used)`
    fn used_left(&self) -> String {
        if self.expired() {
            return format!(
                "{} · {} · current usage unavailable (last reading {}% used)",
                self.label,
                self.resets
                    .as_deref()
                    .map(reset_phrase)
                    .unwrap_or_else(|| "reset time unavailable".into()),
                self.used_i()
            );
        }
        format!(
            "{} {}% left ({}% used)",
            self.label,
            self.left_i(),
            self.used_i()
        )
    }

    fn picker_text(&self) -> String {
        let mut text = self.used_left();
        if !self.expired() {
            text.push_str(" · ");
            text.push_str(
                &self
                    .resets
                    .as_deref()
                    .map(reset_phrase)
                    .unwrap_or_else(|| "reset time unavailable".into()),
            );
        }
        text
    }
}

#[derive(Debug, Clone)]
pub struct UsageSnapshot {
    pub credits: Option<Credits>,
    /// Subscription tier reported by the provider, never inferred from account names.
    pub plan: Option<String>,
    /// Short, for the footer: `5h 59% left · 7d 41% left`.
    pub summary: String,
    /// Multi-line, one line per window with a text bar.
    pub detail: String,
    pub primary_pct: Option<f64>,
    pub weekly_pct: Option<f64>,
    pub windows: Vec<UsageWindow>,
    pub limit_hit: bool,
}

impl UsageSnapshot {
    /// A snapshot with no numbers, only an explanation.
    pub fn no_data(summary: impl Into<String>, detail: impl Into<String>) -> Self {
        UsageSnapshot {
            credits: None,
            plan: None,
            summary: summary.into(),
            detail: detail.into(),
            primary_pct: None,
            weekly_pct: None,
            windows: Vec::new(),
            limit_hit: false,
        }
    }

    /// Full `used / left` text for picker rows:
    /// `5h 41% used / 59% left · 7d 59% used / 41% left · resets 3h`.
    pub fn used_left_text(&self) -> String {
        if self.windows.is_empty() {
            return self.summary.clone();
        }
        let mut parts: Vec<String> = self.windows.iter().map(UsageWindow::picker_text).collect();
        if self.limit_hit {
            parts.insert(0, "LIMIT HIT".into());
        }
        parts.join(" · ")
    }

    pub fn credits_text(&self) -> String {
        match &self.credits {
            Some(credits) if credits.unlimited == Some(true) => "Credits: unlimited".into(),
            Some(credits) if credits.balance.is_some() => {
                format!("Credits: {} balance", credits.balance.unwrap())
            }
            Some(credits) if credits.has_credits == Some(false) => "Credits: none available".into(),
            Some(credits) if credits.has_credits == Some(true) => {
                "Credits: balance not reported".into()
            }
            _ => "Credits: not reported".into(),
        }
    }

    pub fn credit_balance_text(&self) -> String {
        match &self.credits {
            Some(credits) if credits.balance.is_some() => credits.balance.unwrap().to_string(),
            Some(credits) if credits.unlimited == Some(true) => "Unlimited".into(),
            _ => "Not reported by provider".into(),
        }
    }

    pub fn credit_availability_text(&self) -> &'static str {
        match self
            .credits
            .as_ref()
            .and_then(|credits| credits.has_credits)
        {
            Some(true) => "Available",
            Some(false) => "None available",
            None => "Not reported",
        }
    }

    fn max_used(&self) -> Option<f64> {
        self.windows
            .iter()
            .filter(|window| !window.expired())
            .map(|w| w.used_pct)
            .reduce(f64::max)
            .or_else(|| match (self.primary_pct, self.weekly_pct) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            })
    }
}

/// How a reset reads in a sentence: `resets in 3h 20m`, `resets Nov 6, 9am`, `reset since last
/// read`. Wall-clock resets are measured against the local time now.
pub fn reset_phrase(r: &str) -> String {
    reset_phrase_at(r, local_now())
}

fn reset_phrase_at(r: &str, now: Option<WallTime>) -> String {
    let r = r.trim();
    if r.starts_with("reset at ") {
        return r.into();
    }
    if r.starts_with("already reset") {
        return "reset time unavailable".to_string();
    }
    if let Some((now, then)) = now.zip(parse_wall(r, now)) {
        let minutes = minutes_until(now, then);
        return match minutes {
            ..=0 => format!("reset at {}", format_wall(then)),
            1..=59 => format!("resets in {minutes}m"),
            60..=1439 if minutes % 60 == 0 => format!("resets in {}h", minutes / 60),
            60..=1439 => format!("resets in {}h {}m", minutes / 60, minutes % 60),
            _ => format!("resets {}", format_wall(then)),
        };
    }
    if r == "soon" {
        return "resets soon".to_string();
    }
    let relative = r.len() > 1
        && r.ends_with(['m', 'h', 'd'])
        && r[..r.len() - 1].chars().all(|c| c.is_ascii_digit());
    if relative {
        format!("resets in {r}")
    } else {
        format!("resets {r}")
    }
}

/// A moment on the local wall clock, as providers print resets (`Oct 29, 4:19am`). No year: a
/// reset is never months away, so the nearest year is the right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WallTime {
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `Oct 29, 4:19am`, `Oct 29 4am`, `Oct 29`, or a bare `4:19am` (today, which needs `now`).
fn parse_wall(text: &str, now: Option<WallTime>) -> Option<WallTime> {
    let text = text.trim().trim_end_matches(" local");
    let (date, time) = match text.split_once(',') {
        Some((date, time)) => (Some(date.trim()), Some(time.trim())),
        None if text.chars().next()?.is_ascii_digit() => (None, Some(text)),
        None => match text.rsplit_once(' ') {
            Some((date, time)) if time.ends_with(['m', 'M']) => (Some(date), Some(time)),
            _ => (Some(text), None),
        },
    };
    let (month, day) = match date {
        Some(date) => {
            let (month, day) = date.split_once(' ')?;
            let month = MONTHS
                .iter()
                .position(|m| month.get(..3).is_some_and(|p| p.eq_ignore_ascii_case(m)))?;
            (month as u32 + 1, day.trim().parse().ok()?)
        }
        None => now.map(|n| (n.month, n.day))?,
    };
    let (hour, minute) = match time {
        Some(time) => parse_clock(time)?,
        None => (0, 0),
    };
    ((1..=31).contains(&day)).then_some(WallTime {
        month,
        day,
        hour,
        minute,
    })
}

/// `4:19am` or `9pm` as 24-hour `(hour, minute)`.
fn parse_clock(time: &str) -> Option<(u32, u32)> {
    let lower = time.trim().to_ascii_lowercase();
    let (clock, pm) = match (lower.strip_suffix("am"), lower.strip_suffix("pm")) {
        (Some(c), _) => (c, false),
        (_, Some(c)) => (c, true),
        _ => return None,
    };
    let (hour, minute) = match clock.trim().split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (clock.trim().parse::<u32>().ok()?, 0),
    };
    if !(1..=12).contains(&hour) || minute > 59 {
        return None;
    }
    Some((hour % 12 + if pm { 12 } else { 0 }, minute))
}

/// `Nov 6, 9am` or `Oct 29, 4:19pm`.
fn format_wall(w: WallTime) -> String {
    let (hour, suffix) = match w.hour {
        0 => (12, "am"),
        1..=11 => (w.hour, "am"),
        12 => (12, "pm"),
        _ => (w.hour - 12, "pm"),
    };
    let month = MONTHS[(w.month.clamp(1, 12) - 1) as usize];
    if w.minute == 0 {
        format!("{month} {}, {hour}{suffix}", w.day)
    } else {
        format!("{month} {}, {hour}:{:02}{suffix}", w.day, w.minute)
    }
}

pub(crate) fn local_time_label() -> String {
    local_now()
        .or_else(system_now)
        .map(format_wall)
        .unwrap_or_else(|| format!("Unix {}", unix_now()))
}

/// Signed minutes from `now` to `then`, taking whichever year puts them closest.
fn minutes_until(now: WallTime, then: WallTime) -> i64 {
    const DAYS_BEFORE: [i64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    const YEAR: i64 = 365 * 24 * 60;
    let of_year = |w: WallTime| {
        (DAYS_BEFORE[(w.month.clamp(1, 12) - 1) as usize] + i64::from(w.day) - 1) * 24 * 60
            + i64::from(w.hour) * 60
            + i64::from(w.minute)
    };
    let delta = of_year(then) - of_year(now);
    if delta > YEAR / 2 {
        delta - YEAR
    } else if delta < -YEAR / 2 {
        delta + YEAR
    } else {
        delta
    }
}

#[cfg(test)]
thread_local! {
    /// The clock tests run against; unset means no wall-clock ageing, as before.
    static TEST_NOW: std::cell::Cell<Option<WallTime>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn local_now() -> Option<WallTime> {
    TEST_NOW.with(std::cell::Cell::get)
}

#[cfg(not(test))]
fn local_now() -> Option<WallTime> {
    system_now()
}

#[cfg(windows)]
fn system_now() -> Option<WallTime> {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        weekday: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(time: *mut SystemTime);
    }
    let mut t = SystemTime::default();
    // Fills the documented Win32 SYSTEMTIME layout; it cannot fail.
    unsafe { GetLocalTime(&mut t) };
    Some(WallTime {
        month: t.month.into(),
        day: t.day.into(),
        hour: t.hour.into(),
        minute: t.minute.into(),
    })
}

#[cfg(not(windows))]
fn system_now() -> Option<WallTime> {
    None
}

fn window_lines(windows: &[UsageWindow]) -> Vec<String> {
    let width = windows.iter().map(|w| w.label.len()).max().unwrap_or(0);
    windows
        .iter()
        .map(|w| {
            if w.expired() {
                return w.used_left();
            }
            let mut line = format!(
                "{:<width$}  {:>3}% left · {:>3}% used",
                w.label,
                w.left_i(),
                w.used_i(),
            );
            if w.left_i() == 0 {
                line.push_str(" · limit reached");
            }
            if let Some(r) = &w.resets {
                line.push_str(&format!(" · {}", reset_phrase(r)));
            }
            line
        })
        .collect()
}

/// Build the footer summary and the multi-line detail from the windows.
fn snapshot_from_windows(
    windows: Vec<UsageWindow>,
    limit_kind: Option<String>,
    extras: Vec<String>,
    source: &str,
) -> UsageSnapshot {
    let windows: Vec<_> = windows
        .into_iter()
        .filter(|window| window.used_pct.is_finite() && (0.0..=100.0).contains(&window.used_pct))
        .collect();
    let mut parts = Vec::new();
    if let Some(kind) = &limit_kind {
        parts.push(format!("LIMIT · {kind}"));
    }
    for w in &windows {
        parts.push(if w.expired() {
            format!("{} reset · usage unknown", w.label)
        } else {
            format!("{} {}% left", w.label, w.left_i())
        });
    }
    parts.extend(extras.iter().cloned());
    if parts.is_empty() {
        return UsageSnapshot::no_data(
            "no data yet - chat once",
            "Codex reports its limits after a turn; chat once to see them",
        );
    }
    let mut lines = window_lines(&windows);
    if let Some(kind) = &limit_kind {
        lines.insert(0, format!("limit reached: {kind}"));
    }
    lines.extend(extras);
    if !source.is_empty() {
        lines.push(source.to_string());
    }
    let primary_pct = windows.first().filter(|w| !w.expired()).map(|w| w.used_pct);
    let weekly_pct = windows
        .iter()
        .find(|w| w.label.starts_with("7d"))
        .or_else(|| windows.get(1))
        .filter(|w| !w.expired())
        .map(|w| w.used_pct);
    UsageSnapshot {
        credits: None,
        plan: None,
        summary: parts.join(" · "),
        detail: lines.join("\n"),
        primary_pct,
        weekly_pct,
        windows,
        limit_hit: limit_kind.is_some(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexRateLimits {
    primary: Option<RateWindow>,
    secondary: Option<RateWindow>,
    #[serde(alias = "planType")]
    plan_type: Option<String>,
    #[serde(alias = "limitId")]
    limit_id: Option<String>,
    #[serde(alias = "rateLimitReachedType")]
    rate_limit_reached_type: Option<String>,
    credits: Option<Credits>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RateWindow {
    #[serde(alias = "usedPercent")]
    used_percent: Option<f64>,
    #[serde(alias = "windowDurationMins")]
    window_minutes: Option<u32>,
    #[serde(alias = "resetsAt")]
    resets_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credits {
    #[serde(alias = "hasCredits")]
    has_credits: Option<bool>,
    unlimited: Option<bool>,
    #[serde(default, deserialize_with = "optional_number")]
    balance: Option<f64>,
}

fn optional_number<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<f64>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|value| value.as_f64().or_else(|| value.as_str()?.parse().ok()))
        .filter(|number| number.is_finite()))
}

pub fn read_codex_usage(codex_home: &Path) -> UsageSnapshot {
    let cached = read_tmp_rate_limits(codex_home);
    let session = read_session_rate_limits(codex_home);
    let latest = match (cached, session) {
        (Some(cached), Some(session)) => Some(if cached.1 > session.1 {
            cached
        } else {
            session
        }),
        (cached, session) => cached.or(session),
    };
    if let Some((snapshot, _)) = latest {
        return snapshot;
    }
    UsageSnapshot::no_data(
        "no data yet - chat once",
        "Codex only reports its limits after a turn, so there is nothing to read yet - chat once with Codex",
    )
}

pub fn cursor_usage() -> UsageSnapshot {
    UsageSnapshot::no_data(
        "not shown by Cursor CLI",
        "Cursor's CLI has no usage command, so Concordia cannot show it. See cursor.com/dashboard/usage",
    )
}

fn cursor_waiting() -> UsageSnapshot {
    UsageSnapshot::no_data(
        "checking limits...",
        "asking Cursor for this account's limits (takes a few seconds)",
    )
}

fn claude_waiting() -> UsageSnapshot {
    UsageSnapshot::no_data(
        "checking limits...",
        "asking Claude for this account's limits (takes a few seconds)",
    )
}

pub struct UsageRow {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub tone: UsageTone,
}

/// Current account first, then other ready accounts.
pub fn usage_rows_for_accounts(
    fetcher: &mut UsageFetcher,
    accounts: &[(String, crate::store::ProviderKind, std::path::PathBuf)],
    current_account: &str,
) -> Vec<UsageRow> {
    let mut rows = Vec::new();
    let mut rest = Vec::new();
    for (id, provider, home) in accounts {
        let snap = fetcher.snapshot(*provider, id, home);
        let identity = crate::identity::inspect_provider(*provider, home);
        let row = profiled_picker_row(id, *provider, &snap, &identity);
        if id == current_account {
            rows.push(row);
        } else {
            rest.push(row);
        }
    }
    rows.extend(rest);
    rows
}

/// One picker row (`/use`, `/usage all`): name, icon, full used / left text.
pub fn picker_row(
    id: &str,
    provider: crate::store::ProviderKind,
    snap: &UsageSnapshot,
) -> UsageRow {
    let mut label = crate::accounts::display_account_name(provider, id);
    if provider == crate::store::ProviderKind::Cursor && !snap.windows.is_empty() {
        label.push_str(" · monthly limits");
    }
    UsageRow {
        id: id.to_string(),
        label: if snap.windows.is_empty() {
            format!("{label}  {}", snap.used_left_text())
        } else {
            let windows = snap
                .windows
                .iter()
                .map(UsageWindow::picker_text)
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{label}{}\n{windows}",
                if snap.limit_hit { " · LIMIT HIT" } else { "" }
            )
        },
        detail: if snap.windows.is_empty() {
            snap.detail.clone()
        } else {
            String::new()
        },
        tone: usage_tone(snap),
    }
}

/// Keep public account metadata above each account's vertical limits.
pub fn profiled_picker_row(
    id: &str,
    provider: crate::store::ProviderKind,
    snap: &UsageSnapshot,
    identity: &crate::identity::ProviderIdentity,
) -> UsageRow {
    let mut row = picker_row(id, provider, snap);
    if !snap.windows.is_empty() {
        row.detail = snap.detail.lines().last().unwrap_or_default().to_owned();
    }
    let email = identity
        .login
        .as_deref()
        .filter(|s| s.contains('@'))
        .unwrap_or("not reported");
    let plan = identity
        .plan
        .as_deref()
        .or(snap.plan.as_deref())
        .filter(|p| !p.trim().is_empty() && *p != "unknown")
        .unwrap_or("not reported");
    let plan = match plan.to_ascii_lowercase().as_str() {
        "free" => "Free",
        "plus" => "Plus",
        "pro" => "Pro",
        "max" => "Max",
        "team" => "Team",
        "business" => "Business",
        "enterprise" => "Enterprise",
        "edu" => "Education",
        "individual" => "Individual",
        _ => plan,
    };
    let head = crate::accounts::display_account_name(provider, id);
    let limits = row
        .label
        .split_once('\n')
        .map(|(_, limits)| limits.to_string())
        .unwrap_or_else(|| snap.used_left_text());
    let mut label = format!("{head} \u{b7} Email: {email} \u{b7} Plan: {plan}");
    if let Some(team) = &identity.team {
        label.push_str(&format!("\nWorkspace: {team}"));
    }
    if !limits.is_empty() {
        label.push('\n');
        label.push_str(&limits);
    }
    row.label = label;
    row
}

pub fn usage_tone(snap: &UsageSnapshot) -> UsageTone {
    let pct = snap.max_used();
    if snap.limit_hit || pct.is_some_and(|p| p >= 95.0) {
        UsageTone::Bad
    } else if pct.is_some_and(|p| p >= 80.0) {
        UsageTone::Warn
    } else if pct.is_some() {
        UsageTone::Ok
    } else {
        UsageTone::Mute
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageTone {
    Ok,
    Warn,
    Bad,
    Mute,
}

// ---- Claude: `claude -p "/usage"`, fetched off the UI thread and cached --------------------

/// Parse the output of `claude -p "/usage"`. Tolerant: only `Current ...: N% used` lines count,
/// everything else is ignored, and unrecognised text gives `None`.
pub fn parse_claude_usage(text: &str) -> Option<UsageSnapshot> {
    let mut windows = Vec::new();
    for raw in text.lines() {
        let clean = strip_ansi(raw);
        let line = clean.trim();
        let Some(rest) = line.strip_prefix("Current ") else {
            continue;
        };
        let Some((name, body)) = rest.split_once(':') else {
            continue;
        };
        let Some(pct_at) = body.find('%') else {
            continue;
        };
        let digits: String = body[..pct_at]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let Ok(n) = digits.parse::<f64>() else {
            continue;
        };
        let after = &body[pct_at + 1..];
        let used_pct = if after.trim_start().starts_with("left") {
            100.0 - n
        } else {
            n
        };
        let resets = after
            .find("resets ")
            .map(|i| strip_tz(after[i + "resets ".len()..].trim()))
            .filter(|r| !r.is_empty());
        windows.push(UsageWindow {
            label: claude_label(name),
            used_pct,
            resets,
        });
    }
    if windows.is_empty() {
        return None;
    }
    Some(snapshot_from_windows(
        windows,
        None,
        Vec::new(),
        "from claude /usage",
    ))
}

/// Parse the screen text of Cursor's interactive `/usage`. Tolerant: ANSI is stripped, the plan
/// and reset date come from `Usage • <Plan> ... Resets <date>`, and only the rows
/// Included / Auto / API / On-Demand with `N% used` become windows. `Disabled` on-demand and the
/// plan go into the note. Unrecognised text gives `None`.
pub fn parse_cursor_usage(text: &str) -> Option<UsageSnapshot> {
    let (windows, note) = parse_cursor_windows(text)?;
    let mut snapshot = snapshot_from_windows(
        windows,
        None,
        Vec::new(),
        &cursor_source(&note, "from agent /usage"),
    );
    snapshot.plan = cursor_plan(&note);
    Some(snapshot)
}

fn cursor_plan(note: &str) -> Option<String> {
    note.strip_prefix("Cursor ")
        .map(|p| p.split(',').next().unwrap_or(p).trim().to_string())
        .filter(|p| !p.is_empty())
}

fn cursor_source(note: &str, tail: &str) -> String {
    if note.is_empty() {
        tail.to_string()
    } else {
        format!("{note} · {tail}")
    }
}

fn parse_cursor_windows(text: &str) -> Option<(Vec<UsageWindow>, String)> {
    let mut plan: Option<String> = None;
    let mut reset: Option<String> = None;
    let mut on_demand_off = false;
    let mut windows: Vec<UsageWindow> = Vec::new();
    for raw in strip_ansi(text).lines() {
        let line = raw.trim();
        if let Some((head, rest)) = line.split_once('\u{2022}') {
            if head.trim().eq_ignore_ascii_case("usage") && plan.is_none() {
                let rest = rest.trim();
                match rest.find("Resets") {
                    Some(i) => {
                        plan = Some(rest[..i].trim().to_string());
                        reset = Some(rest[i + "Resets".len()..].trim().to_string())
                            .filter(|r| !r.is_empty());
                    }
                    None => plan = Some(rest.to_string()),
                }
                continue;
            }
        }
        let Some((label, rest)) = cursor_row(line) else {
            continue;
        };
        let lower = rest.to_ascii_lowercase();
        if let Some(i) = lower.find("% used") {
            let digits: String = rest[..i]
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let Ok(used_pct) = digits.parse::<f64>() else {
                continue;
            };
            // A redrawn screen repeats rows: the last reading wins.
            windows.retain(|w| w.label != label);
            windows.push(UsageWindow {
                label: label.to_string(),
                used_pct,
                resets: None,
            });
        } else if label == "On-Demand" && lower.contains("disabled") {
            on_demand_off = true;
        }
    }
    if windows.is_empty() {
        return None;
    }
    for w in &mut windows {
        w.resets = reset.clone();
    }
    let mut note = Vec::new();
    if let Some(p) = plan.filter(|p| !p.is_empty()) {
        note.push(format!("Cursor {p}"));
    }
    if on_demand_off {
        note.push("on-demand off".to_string());
    }
    Some((windows, note.join(", ")))
}

/// `(canonical label, text after it)` for a usage-table row.
fn cursor_row(line: &str) -> Option<(&'static str, &str)> {
    for (name, label) in [
        ("included", "Included"),
        ("auto", "Auto"),
        ("api", "API"),
        ("on-demand", "On-Demand"),
        ("on demand", "On-Demand"),
    ] {
        if line.len() >= name.len() && line.is_char_boundary(name.len()) {
            let (head, rest) = line.split_at(name.len());
            if head.eq_ignore_ascii_case(name) && rest.starts_with(char::is_whitespace) {
                return Some((label, rest));
            }
        }
    }
    None
}

fn claude_label(name: &str) -> String {
    let name = name.trim();
    let lower = name.to_ascii_lowercase();
    if lower == "session" {
        return "5h".into();
    }
    if lower.starts_with("week") {
        let qual = name
            .split_once('(')
            .map(|(_, q)| q.trim_end_matches(')').trim().trim_end_matches(" only"))
            .unwrap_or("");
        return if qual.is_empty() || qual.eq_ignore_ascii_case("all models") {
            "7d".into()
        } else {
            format!("7d {qual}")
        };
    }
    name.to_string()
}

/// `Oct 15, 9:10pm (America/Phoenix)` -> `Oct 15, 9:10pm`.
fn strip_tz(s: &str) -> String {
    match s.rfind('(') {
        Some(i) if s.ends_with(')') => s[..i].trim().to_string(),
        _ => s.to_string(),
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                for n in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(n) = chars.next() {
                    if n == '\u{7}' {
                        break;
                    }
                    if n == '\u{1b}' {
                        chars.next();
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

/// Run `<bin> -p /usage --no-session-persistence` with `CLAUDE_CONFIG_DIR=<home>`: no window, stdin closed, and the whole
/// process tree killed after `timeout`. The `.cmd` shim case is handled by `Command` itself; the
/// arguments are fixed and shell-safe.
pub fn run_claude_usage(bin: &str, home: &Path, timeout: Duration) -> Result<String, String> {
    use std::process::Stdio;
    let mut cmd = crate::process_ui::command(bin);
    // A usage read must not leave a native session behind (it showed as "untitled session").
    cmd.args(["-p", "/usage", "--no-session-persistence"])
        .env("CLAUDE_CONFIG_DIR", home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let out = crate::process_lifetime::capture(cmd, timeout, 1024 * 1024)?;
    if !out.status.success() {
        return Err(format!("Claude usage reader exited with {}", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct UsageIdentity {
    account: String,
    provider: crate::store::ProviderKind,
    home: String,
}

impl UsageIdentity {
    fn new(provider: crate::store::ProviderKind, account: &str, home: &Path) -> Self {
        let home = if home.is_absolute() {
            home.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(home)
        };
        let home = home.canonicalize().unwrap_or(home);
        let raw = home.to_string_lossy();
        let mut normalized = crate::file_links::normalized_local_path(&raw).replace('\\', "/");
        if cfg!(windows) {
            normalized.make_ascii_lowercase();
        }
        UsageIdentity {
            account: account.to_string(),
            provider,
            home: normalized,
        }
    }

    fn cache_path(&self, state_root: &Path) -> PathBuf {
        let safe: String = self
            .account
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let mut hasher = DefaultHasher::new();
        self.home.hash(&mut hasher);
        let home_id = format!("{:016x}", hasher.finish());
        state_root.join("usage-cache").join(format!(
            "{safe}-{}-{}.json",
            self.provider.as_str(),
            home_id
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedUsage {
    fetched_at: u64,
    windows: Vec<UsageWindow>,
    /// Retain credits, exact reset timestamps and limit state across asynchronous refreshes.
    #[serde(default)]
    codex_limits: Option<CodexRateLimits>,
    /// Plan and similar, shown after the windows (Cursor).
    #[serde(default)]
    note: String,
    identity: UsageIdentity,
}

#[derive(Debug, Clone)]
pub struct CachedObservation {
    pub observed_at: u64,
    pub fresh: bool,
    pub exhausted: bool,
    pub remaining_pct: Option<f64>,
}

pub struct CachedUsageView {
    pub observed_at: u64,
    pub fresh: bool,
    pub windows: Vec<String>,
}

/// Exact account/provider/home cache only: no directory scan, provider launch or refresh.
pub fn cached_usage_view(
    root: &Path,
    provider: crate::store::ProviderKind,
    account: &str,
    home: &Path,
) -> Option<CachedUsageView> {
    let cached = load_cache(root, &UsageIdentity::new(provider, account, home))?;
    Some(CachedUsageView {
        observed_at: cached.fetched_at,
        fresh: unix_now().saturating_sub(cached.fetched_at) < USAGE_TTL_SECS,
        windows: window_lines(&cached.windows),
    })
}

/// Local, identity-bound cache inspection only. Never constructs a fetcher or starts a provider.
pub fn peek_cached_usage(
    root: &Path,
    provider: crate::store::ProviderKind,
    account: &str,
    home: &Path,
) -> Option<CachedObservation> {
    let identity = UsageIdentity::new(provider, account, home);
    let cached = load_cache(root, &identity)?;
    let now = unix_now();
    Some(CachedObservation {
        observed_at: cached.fetched_at,
        fresh: now.saturating_sub(cached.fetched_at) < USAGE_TTL_SECS,
        exhausted: cached
            .windows
            .iter()
            .any(|window| !window.expired() && window.used_pct >= 100.0),
        remaining_pct: cached
            .windows
            .iter()
            .find(|window| !window.expired())
            .map(UsageWindow::left_pct),
    })
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load_cache(state_root: &Path, identity: &UsageIdentity) -> Option<CachedUsage> {
    let path = identity.cache_path(state_root);
    let directory = path.parent()?.canonicalize().ok()?;
    if crate::workspaces::directory_id(path.parent()?).is_err()
        || !crate::fs_safe::regular_owned_file(&path, &directory)
    {
        return None;
    }
    let raw = crate::fs_safe::read_text_bounded(&path, 64 * 1024)?;
    let cached: CachedUsage = serde_json::from_str(&raw).ok()?;
    (cached.identity == *identity
        && cached.fetched_at <= unix_now()
        && cached.windows.len() <= 16
        && cached.note.len() <= 4096
        && cached.windows.iter().all(|window| {
            window.used_pct.is_finite()
                && (0.0..=100.0).contains(&window.used_pct)
                && window.label.len() <= 128
                && window
                    .resets
                    .as_ref()
                    .is_none_or(|reset| reset.len() <= 512)
        }))
    .then_some(cached)
}

fn save_cache(state_root: &Path, c: &CachedUsage) {
    let path = c.identity.cache_path(state_root);
    if let Some(dir) = path.parent() {
        if fs::create_dir_all(dir).is_err() || crate::workspaces::directory_id(dir).is_err() {
            return;
        }
        if path.exists()
            && !dir
                .canonicalize()
                .is_ok_and(|dir| crate::fs_safe::regular_owned_file(&path, &dir))
        {
            return;
        }
    }
    if let Ok(json) = serde_json::to_string(c) {
        if json.len() <= 64 * 1024 {
            let _ = crate::state_io::write_atomic(&path, json.as_bytes());
        }
    }
}

/// Produces the raw `/usage` text for a Claude config dir. Swapped for a fake in tests.
pub type UsageRunner = Arc<dyn Fn(&Path) -> Result<String, String> + Send + Sync>;

fn default_runner() -> UsageRunner {
    Arc::new(|home| {
        let bin = crate::provider::claude::find_claude_bin()?;
        run_claude_usage(&bin, home, FETCH_TIMEOUT)
    })
}

fn default_cursor_runner() -> UsageRunner {
    Arc::new(crate::cursor_usage::fetch_cursor_usage_text)
}

struct Entry {
    identity: UsageIdentity,
    cached: Option<CachedUsage>,
    pending: bool,
    error: Option<String>,
    last_attempt: u64,
    /// A turn used some of the limits since this was read: re-check soon.
    dirty: bool,
}

struct Fetched {
    identity: UsageIdentity,
    result: Result<CachedUsage, String>,
}

/// Per-account Claude limits: cached on disk, refreshed on a worker thread, never blocking.
pub struct UsageFetcher {
    state_root: PathBuf,
    runner: UsageRunner,
    cursor_runner: UsageRunner,
    codex_runner: UsageRunner,
    tx: Sender<Fetched>,
    rx: Receiver<Fetched>,
    entries: HashMap<UsageIdentity, Entry>,
}

impl UsageFetcher {
    pub fn new(state_root: PathBuf) -> Self {
        Self::with_runner(state_root, default_runner())
    }

    pub fn with_runner(state_root: PathBuf, runner: UsageRunner) -> Self {
        let (tx, rx) = channel();
        UsageFetcher {
            state_root,
            runner,
            cursor_runner: default_cursor_runner(),
            codex_runner: Arc::new(crate::usage_codex::fetch),
            tx,
            rx,
            entries: HashMap::new(),
        }
    }

    /// Replace how Cursor's `/usage` text is obtained (tests).
    pub fn with_cursor_runner(mut self, runner: UsageRunner) -> Self {
        self.cursor_runner = runner;
        self
    }
    pub fn with_codex_runner(mut self, runner: UsageRunner) -> Self {
        self.codex_runner = runner;
        self
    }

    /// Cached limits for an account, with a background refresh when missing or stale.
    pub fn snapshot(
        &mut self,
        provider: crate::store::ProviderKind,
        account: &str,
        home: &Path,
    ) -> UsageSnapshot {
        self.fetched_snapshot(provider, account, home, false)
    }

    /// Like [`snapshot`](Self::snapshot), but refreshes even a fresh cache (`/usage`).
    pub fn snapshot_refresh(
        &mut self,
        provider: crate::store::ProviderKind,
        account: &str,
        home: &Path,
    ) -> UsageSnapshot {
        self.fetched_snapshot(provider, account, home, true)
    }

    /// All providers share identity, freshness and refresh scheduling here.
    fn fetched_snapshot(
        &mut self,
        provider: crate::store::ProviderKind,
        account: &str,
        home: &Path,
        force: bool,
    ) -> UsageSnapshot {
        let cursor = provider == crate::store::ProviderKind::Cursor;
        let codex = provider == crate::store::ProviderKind::Codex;
        let now = unix_now();
        let root = self.state_root.clone();
        let identity = UsageIdentity::new(provider, account, home);
        let e = self
            .entries
            .entry(identity.clone())
            .or_insert_with(|| Entry {
                identity: identity.clone(),
                cached: load_cache(&root, &identity),
                pending: false,
                error: None,
                last_attempt: 0,
                dirty: false,
            });
        let usable_cache = e
            .cached
            .as_ref()
            .filter(|c| c.identity == identity && c.fetched_at <= now)
            .cloned();
        let stale = usable_cache
            .as_ref()
            .is_none_or(|c| now.saturating_sub(c.fetched_at) >= USAGE_TTL_SECS);
        let stale = stale || e.dirty;
        let retry_ok = force || e.dirty || now.saturating_sub(e.last_attempt) >= USAGE_TTL_SECS;
        let start = !e.pending && (force || (stale && retry_ok));
        if start {
            e.pending = true;
            e.dirty = false;
            e.last_attempt = now;
        }
        let view = match (usable_cache, e.pending, &e.error) {
            (Some(c), _, _) => Ok((c.clone(), now.saturating_sub(c.fetched_at))),
            (None, true, _) | (None, false, None) => Err(None),
            (None, false, Some(err)) => Err(Some(err.clone())),
        };
        let cache_state = if e.pending {
            Some("cached; refreshing")
        } else if e.error.is_some() {
            Some("cached; refresh failed")
        } else {
            None
        };
        let refresh_error = e.error.clone();
        if start {
            self.spawn(provider, identity.clone(), home.to_path_buf());
        }
        match view {
            Ok((c, age)) => {
                let age_note = if age < 60 {
                    "checked just now".to_string()
                } else {
                    format!("checked {} ago", format_age_minutes(age / 60))
                };
                let mut snap = snapshot_from_windows(
                    c.windows,
                    None,
                    Vec::new(),
                    &if codex {
                        format!("Codex live limits · {age_note}")
                    } else {
                        cursor_source(&c.note, &age_note)
                    },
                );
                if let Some(limits) = c.codex_limits.as_ref().filter(|_| codex) {
                    snap = format_limits(limits, &format!("Codex live limits · {age_note}"));
                }
                if cursor {
                    snap.plan = cursor_plan(&c.note);
                }
                if codex {
                    snap.plan = (!c.note.is_empty()).then_some(c.note);
                }
                // A reading that is a few minutes old says so right in the footer.
                if age >= USAGE_TTL_SECS && !snap.windows.is_empty() {
                    snap.summary
                        .push_str(&format!(" \u{b7} {} old", format_age_minutes(age / 60)));
                }
                if let Some(state) = cache_state {
                    snap.summary.push_str(&format!(" · {state}"));
                    snap.detail.push_str(&format!(
                        "\n{state}; the displayed values are the last successful reading"
                    ));
                }
                if let Some(error) = refresh_error {
                    snap.detail.push_str(&format!(
                        "\nRefresh unavailable: {error}. /usage tries again."
                    ));
                }
                snap
            }
            Err(reason) if codex => {
                let mut snap = read_codex_usage(home);
                if snap.windows.is_empty() {
                    return if let Some(reason) = reason {
                        UsageSnapshot::no_data("live limits unavailable", format!("Live refresh unavailable: {reason}. No recorded quota windows; /usage tries again."))
                    } else {
                        UsageSnapshot::no_data("reading live limits", "Requesting current quota windows; no recorded limits are available yet.")
                    };
                }
                let note = if let Some(ref reason) = reason {
                    format!("Live refresh unavailable: {reason}. Showing native recorded data.")
                } else {
                    "Reading live limits… Showing native recorded data until the response arrives."
                        .into()
                };
                snap.detail.push_str(&format!("\n{note}"));
                snap.summary.push_str(if reason.is_none() {
                    " · refreshing"
                } else {
                    " · recorded"
                });
                snap
            }
            Err(None) if cursor => cursor_waiting(),
            Err(None) => claude_waiting(),
            // Cursor has no fallback command: any failure is the plain "not shown" message.
            Err(Some(_)) if cursor => cursor_usage(),
            Err(Some(err)) => UsageSnapshot::no_data(
                "limits unavailable",
                format!("could not read Claude limits ({err}); /usage tries again"),
            ),
        }
    }

    /// A turn just used some of this account's limits: the next read re-checks in the background
    /// (the old numbers stay on screen, with their age, until the new ones land).
    pub fn invalidate(&mut self, account: &str) {
        for e in self
            .entries
            .values_mut()
            .filter(|e| e.identity.account == account)
        {
            e.dirty = true;
        }
    }

    fn spawn(&self, provider: crate::store::ProviderKind, identity: UsageIdentity, home: PathBuf) {
        let cursor = provider == crate::store::ProviderKind::Cursor;
        let codex = provider == crate::store::ProviderKind::Codex;
        let runner = Arc::clone(if cursor {
            &self.cursor_runner
        } else if codex {
            &self.codex_runner
        } else {
            &self.runner
        });
        let tx = self.tx.clone();
        let root = self.state_root.clone();
        std::thread::spawn(move || {
            let result = runner(&home).and_then(|text| {
                let parsed_limits = codex.then(|| parse_rate_limits_blob(&text)).flatten();
                let codex_limits = parsed_limits.clone();
                let (windows, note) = if codex {
                    parsed_limits.map(|limits| {
                        let snap = format_limits(&limits, "Codex live limits");
                        (snap.windows, snap.plan.unwrap_or_default())
                    })
                } else if cursor {
                    parse_cursor_windows(&text)
                } else {
                    parse_claude_usage(&text).map(|s| (s.windows, String::new()))
                }
                .ok_or_else(|| "unrecognised /usage output".to_string())?;
                Ok(CachedUsage {
                    fetched_at: unix_now(),
                    windows,
                    codex_limits,
                    note,
                    identity: identity.clone(),
                })
            });
            if cursor {
                if let Err(e) = &result {
                    // Event only: never the screen text.
                    crate::logging::info(&format!("cursor usage unavailable: {e}"));
                }
            }
            if let Ok(c) = &result {
                save_cache(&root, c);
            }
            let _ = tx.send(Fetched { identity, result });
        });
    }

    /// Collect finished background fetches. True when any account's limits changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(f) = self.rx.try_recv() {
            if let Some(e) = self.entries.get_mut(&f.identity) {
                e.pending = false;
                match f.result {
                    Ok(c) => {
                        e.cached = Some(c);
                        e.error = None;
                    }
                    Err(err) => e.error = Some(err),
                }
                changed = true;
            }
        }
        changed
    }
}

fn read_tmp_rate_limits(codex_home: &Path) -> Option<(UsageSnapshot, SystemTime)> {
    let tmp = codex_home.join("tmp");
    if crate::fs_safe::resolved_directory(&tmp).is_none() {
        return None;
    }
    let mut files: Vec<_> = fs::read_dir(&tmp)
        .ok()?
        .take(4096)
        .flatten()
        .filter(|e| {
            fs::symlink_metadata(e.path())
                .is_ok_and(|metadata| metadata.is_file() && !crate::workspaces::linked(&metadata))
        })
        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    files.sort_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()));
    files.reverse();
    for entry in files.into_iter().take(8) {
        let Some(raw) = crate::fs_safe::read_text_bounded(&entry.path(), 512 * 1024) else {
            continue;
        };
        if let Some(limits) = parse_rate_limits_blob(&raw) {
            if has_percents(&limits) {
                let modified = entry.metadata().ok()?.modified().ok()?;
                return Some((
                    format_limits(&limits, "from CODEX_HOME/tmp cache"),
                    modified,
                ));
            }
        }
    }
    None
}

fn read_session_rate_limits(codex_home: &Path) -> Option<(UsageSnapshot, SystemTime)> {
    let sessions = codex_home.join("sessions");
    if crate::workspaces::directory_id(&sessions).is_err() {
        return None;
    }
    let mut files: Vec<(PathBuf, SystemTime)> = Vec::new();
    let mut stack = vec![(sessions, 0)];
    let mut remaining = 4096;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.take(remaining) {
            remaining = remaining.saturating_sub(1);
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if crate::workspaces::linked(&metadata) {
                continue;
            }
            if metadata.is_dir() && depth < 8 {
                stack.push((path, depth + 1));
            } else if metadata.is_file() && path.extension().map(|x| x == "jsonl").unwrap_or(false)
            {
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                files.push((path, mtime));
            }
        }
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.1));
    let mut best: Option<(CodexRateLimits, SystemTime)> = None;
    for (path, mtime) in files.into_iter().take(12) {
        let Some(hit) = scan_file_for_rate_limits(&path) else {
            continue;
        };
        let prefer = has_percents(&hit);
        if prefer {
            best = Some((hit, mtime));
            break;
        } else if best.is_none() {
            best = Some((hit, mtime));
        }
    }
    let (limits, mtime) = best?;
    let age = SystemTime::now()
        .duration_since(mtime)
        .ok()
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let detail = if age > 3600 {
        format!("as of your last chat, {} ago", format_age_minutes(age / 60))
    } else {
        "as of your last chat".into()
    };
    Some((format_limits(&limits, &detail), mtime))
}

fn scan_file_for_rate_limits(path: &Path) -> Option<CodexRateLimits> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    // Rate-limit events arrive at turn ends — read a trailing window for large rollouts.
    const TAIL: u64 = 512 * 1024;
    if len > TAIL {
        file.seek(SeekFrom::Start(len - TAIL)).ok()?;
    }
    let mut bytes = Vec::new();
    file.take(TAIL).read_to_end(&mut bytes).ok()?;
    // A trailing read may start inside a multibyte character on the discarded first line.
    let buf = String::from_utf8_lossy(&bytes);
    // Drop partial first line when we seeked mid-file.
    let text = if len > TAIL {
        buf.split_once('\n').map(|(_, rest)| rest).unwrap_or(&buf)
    } else {
        buf.as_ref()
    };
    let mut last_any: Option<CodexRateLimits> = None;
    let mut last_pct: Option<CodexRateLimits> = None;
    for line in text.lines() {
        if !line.contains("rate_limits") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(block) = extract_rate_limits_node(&v) else {
            continue;
        };
        let Ok(limits) = serde_json::from_value::<CodexRateLimits>(block) else {
            continue;
        };
        if has_percents(&limits) {
            last_pct = Some(limits);
        } else {
            last_any = Some(limits);
        }
    }
    last_pct.or(last_any)
}

fn parse_rate_limits_blob(raw: &str) -> Option<CodexRateLimits> {
    if !raw.contains("used_percent") && !raw.contains("rate_limits") {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let node = extract_rate_limits_node(&v)?;
    serde_json::from_value(node).ok()
}

fn extract_rate_limits_node(v: &serde_json::Value) -> Option<serde_json::Value> {
    if v.get("primary").is_some() || v.get("secondary").is_some() {
        return Some(v.clone());
    }
    if let Some(rl) = v.get("rate_limits") {
        return Some(rl.clone());
    }
    if let Some(payload) = v.get("payload") {
        if let Some(rl) = payload.get("rate_limits") {
            return Some(rl.clone());
        }
        if payload.get("primary").is_some() || payload.get("secondary").is_some() {
            return Some(payload.clone());
        }
    }
    None
}

fn has_percents(u: &CodexRateLimits) -> bool {
    u.primary.as_ref().and_then(|p| p.used_percent).is_some()
        || u.secondary.as_ref().and_then(|s| s.used_percent).is_some()
}

fn format_limits(u: &CodexRateLimits, source: &str) -> UsageSnapshot {
    let mut windows = Vec::new();
    for (w, fallback) in [(&u.primary, "5h"), (&u.secondary, "7d")] {
        if let Some(w) = w {
            if let Some(pct) = w.used_percent {
                // The numbers are from the last chat. If the window has reset since, the old
                // percentage no longer applies.
                windows.push(UsageWindow {
                    label: w
                        .window_minutes
                        .map(format_window)
                        .unwrap_or_else(|| fallback.into()),
                    used_pct: pct,
                    resets: w.resets_at.map(|r| {
                        if r > 0 && r <= unix_now() as i64 {
                            format!(
                                "reset at {}",
                                format_reset_unix(r).unwrap_or_else(|| format!("Unix {r}"))
                            )
                        } else {
                            format_reset_unix(r).unwrap_or_else(|| "time unavailable".into())
                        }
                    }),
                });
            }
        }
    }
    let limit_kind = u
        .rate_limit_reached_type
        .as_ref()
        .map(|k| k.trim_end_matches("_reached").replace('_', " "))
        .filter(|_| !windows.iter().any(UsageWindow::expired));
    let mut extras = Vec::new();
    if let Some(c) = &u.credits {
        if c.unlimited == Some(true) {
            extras.push("credits unlimited".to_string());
        } else if let Some(bal) = c.balance {
            extras.push(format!("credits balance {bal}"));
        } else if c.has_credits == Some(false) {
            extras.push("credits unavailable".to_string());
        } else if c.has_credits == Some(true) && windows.is_empty() {
            extras.push("credits available".to_string());
        }
    }
    if windows.is_empty() && extras.is_empty() && limit_kind.is_none() {
        if let Some(plan) = u.plan_type.as_ref().filter(|p| *p != "unknown") {
            extras.push(format!("plan {plan}"));
        } else if let Some(id) = &u.limit_id {
            extras.push(id.clone());
        }
    }
    let mut snapshot = snapshot_from_windows(windows, limit_kind, extras, source);
    snapshot.credits = u.credits.clone();
    snapshot.plan = u.plan_type.clone().filter(|p| p != "unknown");
    snapshot
}

fn format_window(minutes: u32) -> String {
    if minutes < 60 {
        format!("{minutes}m")
    } else if minutes < 60 * 24 {
        format!("{}h", minutes / 60)
    } else {
        format!("{}d", minutes / (60 * 24))
    }
}

fn format_age_minutes(mins: u64) -> String {
    if mins < 60 {
        format!("{mins}m")
    } else if mins < 60 * 24 {
        format!("{}h", mins / 60)
    } else {
        format!("{}d", mins / (60 * 24))
    }
}

fn format_reset_unix(sec: i64) -> Option<String> {
    if sec <= 0 {
        return None;
    }
    if let Some(local) = local_reset_time(sec) {
        return Some(local);
    }
    let ms = if sec > 1_000_000_000_000 {
        sec
    } else {
        sec * 1000
    };
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    let delta = ms - now;
    if delta <= 0 {
        return Some("soon".into());
    }
    let mins = (delta as f64 / 60_000.0).round() as i64;
    if mins < 60 {
        Some(format!("{mins}m"))
    } else if mins < 60 * 24 {
        Some(format!("{}h", mins / 60))
    } else {
        Some(format!("{}d", mins / (60 * 24)))
    }
}

#[cfg(windows)]
fn local_reset_time(seconds: i64) -> Option<String> {
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct LocalTime {
        year: u16,
        month: u16,
        weekday: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn FileTimeToSystemTime(file: *const FileTime, time: *mut LocalTime) -> i32;
        fn SystemTimeToTzSpecificLocalTime(
            zone: *const std::ffi::c_void,
            utc: *const LocalTime,
            local: *mut LocalTime,
        ) -> i32;
    }
    let seconds = if seconds > 1_000_000_000_000 {
        seconds / 1000
    } else {
        seconds
    };
    let ticks = u64::try_from(seconds)
        .ok()?
        .checked_add(11_644_473_600)?
        .checked_mul(10_000_000)?;
    let file = FileTime {
        low: ticks as u32,
        high: (ticks >> 32) as u32,
    };
    let mut utc = LocalTime::default();
    let mut local = LocalTime::default();
    // Both structs use the documented Win32 SYSTEMTIME/FILETIME layouts. A null zone selects OS local time.
    if unsafe { FileTimeToSystemTime(&file, &mut utc) } == 0
        || unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) } == 0
    {
        return None;
    }
    // The same wall-clock form Claude prints, so every provider's reset reads and ages alike.
    Some(format_wall(WallTime {
        month: local.month.into(),
        day: local.day.into(),
        hour: local.hour.into(),
        minute: local.minute.into(),
    }))
}

#[cfg(not(windows))]
fn local_reset_time(_seconds: i64) -> Option<String> {
    None
}

/// Warn only about observed low capacity; reset/unknown values are never inferred available.
pub fn low_limit_warning(
    snapshot: &UsageSnapshot,
    provider: crate::store::ProviderKind,
) -> Option<String> {
    let windows: Vec<_> = snapshot
        .windows
        .iter()
        .filter(|w| {
            (w.label == "5h" || w.label.starts_with("7d"))
                && w.used_pct.is_finite()
                && w.used_pct >= 80.0
                && !w.expired()
        })
        .map(|w| {
            format!(
                "{} {:.0}% left · {}",
                w.label,
                w.left_pct(),
                w.resets
                    .as_deref()
                    .map(reset_phrase)
                    .unwrap_or_else(|| "reset unknown".into())
            )
        })
        .collect();
    if windows.is_empty() {
        return None;
    }
    let freshness = if provider == crate::store::ProviderKind::Codex {
        "updates after a Codex response"
    } else if snapshot.summary.contains("cached") || snapshot.summary.contains(" old") {
        "saved reading; check /usage for refresh status"
    } else {
        "checked by the provider"
    };
    Some(format!("{} · {freshness}", windows.join(" | ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ProviderKind;

    fn wall(month: u32, day: u32, hour: u32, minute: u32) -> WallTime {
        WallTime {
            month,
            day,
            hour,
            minute,
        }
    }

    #[test]
    fn resets_read_the_same_for_every_provider_and_age_with_the_clock() {
        let now = Some(wall(10, 30, 13, 0));
        for (reset, phrase) in [
            ("Oct 30, 4:20pm", "resets in 3h 20m"),
            ("Oct 30, 4pm", "resets in 3h"),
            ("Oct 30, 1:45pm", "resets in 45m"),
            ("1:45pm", "resets in 45m"),
            ("Nov 6, 9am", "resets Nov 6, 9am"),
            ("Oct 29, 4:19am", "reset at Oct 29, 4:19am"),
            ("4:19am", "reset at Oct 30, 4:19am"),
            ("already reset (chat to refresh)", "reset time unavailable"),
        ] {
            assert_eq!(reset_phrase_at(reset, now), phrase, "{reset}");
        }
        // Near the new year the nearest year decides.
        assert_eq!(
            reset_phrase_at("Jan 1, 1am", Some(wall(12, 31, 23, 0))),
            "resets in 2h"
        );
        // Codex's timestamps are written in the same form as Claude's text.
        assert_eq!(format_wall(wall(10, 30, 17, 17)), "Oct 30, 5:17pm");
        assert_eq!(format_wall(wall(11, 6, 0, 0)), "Nov 6, 12am");
    }

    #[test]
    fn a_window_past_its_reset_shows_no_percentage() {
        TEST_NOW.with(|n| n.set(Some(wall(10, 30, 13, 0))));
        let snapshot = snapshot_from_windows(
            vec![
                UsageWindow {
                    label: "5h".into(),
                    used_pct: 64.0,
                    resets: Some("Oct 29, 4:19am".into()),
                },
                UsageWindow {
                    label: "7d".into(),
                    used_pct: 21.0,
                    resets: Some("Nov 6, 9am".into()),
                },
            ],
            None,
            Vec::new(),
            "test",
        );
        let text = snapshot.used_left_text();
        TEST_NOW.with(|n| n.set(None));
        assert_eq!(
            text,
            "5h · reset at Oct 29, 4:19am · current usage unavailable (last reading 64% used) · 7d 79% left (21% used) · resets Nov 6, 9am"
        );
    }

    const SAMPLE: &str = "You are currently using your subscription to power your Claude Code usage\n\nCurrent session: 7% used · resets Oct 15, 9:10pm (America/Phoenix)\nCurrent week (all models): 13% used · resets Oct 21, 9am (America/Phoenix)\n\nWhat's contributing to your limits usage?\nsomething else: 5% used\n";

    #[test]
    fn codex_credit_strings_do_not_discard_usage_or_limit_events() {
        for balance in [
            serde_json::json!("0"),
            serde_json::json!(0),
            serde_json::json!(null),
            serde_json::json!("unknown"),
        ] {
            let raw = serde_json::json!({"rate_limits": {
                "primary": {"used_percent": 98.0, "window_minutes": 300},
                "credits": {"balance": balance},
                "rate_limit_reached_type": "workspace_member_usage_limit_reached"
            }})
            .to_string();
            let limits =
                parse_rate_limits_blob(&raw).expect("balance must not reject the entire reading");
            let snap = format_limits(&limits, "synthetic");
            assert_eq!(snap.primary_pct, Some(98.0));
            assert!(snap.limit_hit);
        }
    }

    #[test]
    fn codex_uses_the_newest_session_with_percentages_not_the_second_newest() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        std::fs::create_dir(&sessions).unwrap();
        for (name, used, age) in [("old.jsonl", 20, 200), ("new.jsonl", 99, 10)] {
            let path = sessions.join(name);
            std::fs::write(
                &path,
                serde_json::json!({"payload": {"rate_limits": {
                    "primary": {"used_percent": used}, "credits": {"balance": "0"}
                }}})
                .to_string(),
            )
            .unwrap();
            File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_times(
                    std::fs::FileTimes::new()
                        .set_modified(SystemTime::now() - Duration::from_secs(age)),
                )
                .unwrap();
        }
        assert_eq!(read_codex_usage(dir.path()).primary_pct, Some(99.0));
        let cache = dir.path().join("tmp");
        std::fs::create_dir(&cache).unwrap();
        let cache_file = cache.join("usage.json");
        std::fs::write(&cache_file, r#"{"primary":{"used_percent":12}}"#).unwrap();
        File::options()
            .write(true)
            .open(&cache_file)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(SystemTime::now() - Duration::from_secs(300)),
            )
            .unwrap();
        assert_eq!(
            read_codex_usage(dir.path()).primary_pct,
            Some(99.0),
            "old temporary cache cannot mask a newer session"
        );
        File::options()
            .write(true)
            .open(&cache_file)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(SystemTime::now()))
            .unwrap();
        assert_eq!(
            read_codex_usage(dir.path()).primary_pct,
            Some(12.0),
            "newer cache is still usable"
        );
    }

    #[test]
    fn codex_tail_starting_inside_unicode_still_reads_complete_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let event =
            serde_json::json!({"payload": {"rate_limits": {"primary": {"used_percent": 87}}}})
                .to_string();
        let suffix = format!("\n{event}\n");
        let padding = 512 * 1024 - suffix.len() - 2;
        let raw = format!("prefix雪{}{}", "x".repeat(padding), suffix);
        std::fs::write(&path, raw).unwrap();
        let limits =
            scan_file_for_rate_limits(&path).expect("partial first UTF-8 character is discarded");
        assert_eq!(limits.primary.unwrap().used_percent, Some(87.0));
    }

    #[test]
    fn parses_the_claude_usage_sample() {
        let s = parse_claude_usage(SAMPLE).expect("parsed");
        assert_eq!(s.windows.len(), 2);
        assert_eq!(s.windows[0].label, "5h");
        assert_eq!(s.windows[0].used_pct, 7.0);
        assert_eq!(s.windows[0].resets.as_deref(), Some("Oct 15, 9:10pm"));
        assert_eq!(s.windows[1].label, "7d");
        assert_eq!(s.windows[1].resets.as_deref(), Some("Oct 21, 9am"));
        assert_eq!(s.summary, "5h 93% left · 7d 87% left");
        assert!(s
            .used_left_text()
            .starts_with("5h 93% left (7% used) · resets Oct 15"));
        assert!(s
            .used_left_text()
            .contains("7d 87% left (13% used) · resets Oct 21"));
        assert_eq!(usage_tone(&s), UsageTone::Ok);
    }

    #[test]
    fn parses_extra_model_weeks_and_tolerates_noise() {
        let t = "\u{1b}[1mCurrent session: 96% used\u{1b}[0m\nCurrent week (Opus): 81% used · resets Oct 22, 1am (UTC)\nCurrent week (all models): 40% used\nCurrent thing without percent\n";
        let s = parse_claude_usage(t).unwrap();
        let labels: Vec<_> = s.windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, ["5h", "7d Opus", "7d"]);
        assert_eq!(s.windows[0].resets, None);
        assert_eq!(usage_tone(&s), UsageTone::Bad);
    }

    #[test]
    fn unrecognised_text_is_none() {
        assert!(parse_claude_usage("").is_none());
        assert!(parse_claude_usage("Error: not logged in").is_none());
    }

    #[test]
    fn cursor_and_codex_no_data_wording() {
        let c = cursor_usage();
        assert_eq!(c.summary, "not shown by Cursor CLI");
        assert!(c.detail.contains("cursor.com/dashboard/usage"));
        assert_eq!(usage_tone(&c), UsageTone::Mute);
        let dir = tempfile::tempdir().unwrap();
        let x = read_codex_usage(dir.path());
        assert!(x.summary.starts_with("no data yet"));
        assert_eq!(usage_tone(&x), UsageTone::Mute);
    }

    #[test]
    fn tone_thresholds_follow_used_percent() {
        let mk = |p| {
            snapshot_from_windows(
                vec![UsageWindow {
                    label: "5h".into(),
                    used_pct: p,
                    resets: None,
                }],
                None,
                vec![],
                "",
            )
        };
        assert_eq!(usage_tone(&mk(79.0)), UsageTone::Ok);
        assert_eq!(usage_tone(&mk(80.0)), UsageTone::Warn);
        assert_eq!(usage_tone(&mk(95.0)), UsageTone::Bad);
    }

    fn fake_runner(text: &'static str) -> UsageRunner {
        Arc::new(move |_| Ok(text.to_string()))
    }

    fn wait_for(f: &mut UsageFetcher) {
        for _ in 0..300 {
            if f.poll() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("fetch never finished");
    }

    #[test]
    fn every_forced_codex_read_fetches_new_limits_and_labels_pending_cached_values() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let root = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let runner: UsageRunner = Arc::new(move |_| {
            let n = count.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(serde_json::json!({"rate_limits":{"planType":"pro","credits":{"hasCredits":true,"balance":"12.5"},"primary":{"usedPercent":n*10,"windowDurationMins":300,"resetsAt":1900000000i64}}}).to_string())
        });
        let mut fetcher = UsageFetcher::with_runner(root.path().into(), fake_runner(SAMPLE))
            .with_codex_runner(runner);
        let initial = fetcher.snapshot_refresh(ProviderKind::Codex, "synthetic", root.path());
        assert_eq!(initial.summary, "reading live limits");
        assert!(!initial.detail.contains("chat once"));
        wait_for(&mut fetcher);
        let first = fetcher.snapshot(ProviderKind::Codex, "synthetic", root.path());
        assert_eq!(first.primary_pct, Some(10.0));
        assert_eq!(first.plan.as_deref(), Some("pro"));
        assert!(
            first.summary.contains("credits balance 12.5"),
            "{}",
            first.summary
        );
        assert!(first.detail.contains("credits balance 12.5"));
        assert_eq!(first.credits_text(), "Credits: 12.5 balance");
        let mut restarted = UsageFetcher::with_runner(root.path().into(), fake_runner(SAMPLE));
        assert!(restarted
            .snapshot(ProviderKind::Codex, "synthetic", root.path())
            .summary
            .contains("credits balance 12.5"));
        let pending = fetcher.snapshot_refresh(ProviderKind::Codex, "synthetic", root.path());
        assert_eq!(pending.credits_text(), "Credits: 12.5 balance");
        assert!(pending.detail.contains("refreshing"));
        wait_for(&mut fetcher);
        assert_eq!(
            fetcher
                .snapshot(ProviderKind::Codex, "synthetic", root.path())
                .primary_pct,
            Some(20.0)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn claude_fetch_is_async_cached_and_reused() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let mut f = UsageFetcher::with_runner(root.path().to_path_buf(), fake_runner(SAMPLE));
        let first = f.snapshot(ProviderKind::Claude, "claude-2", &home);
        assert_eq!(first.summary, "checking limits...");
        wait_for(&mut f);
        let second = f.snapshot(ProviderKind::Claude, "claude-2", &home);
        assert_eq!(second.summary, "5h 93% left · 7d 87% left");
        assert!(UsageIdentity::new(ProviderKind::Claude, "claude-2", &home)
            .cache_path(root.path())
            .is_file());
        // A new fetcher (restart) shows the cached value immediately and does not refetch.
        let failing: UsageRunner = Arc::new(|_| Err("must not run".into()));
        let mut g = UsageFetcher::with_runner(root.path().to_path_buf(), failing);
        let cached = g.snapshot(ProviderKind::Claude, "claude-2", &home);
        assert_eq!(cached.summary, "5h 93% left · 7d 87% left");
        std::thread::sleep(Duration::from_millis(100));
        assert!(!g.poll());
    }

    #[test]
    fn claude_fetch_failure_is_reported_not_retried_in_a_loop() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let runner: UsageRunner = Arc::new(|_| Err("boom".into()));
        let mut f = UsageFetcher::with_runner(root.path().to_path_buf(), runner);
        f.snapshot(ProviderKind::Claude, "claude", &home);
        wait_for(&mut f);
        let s = f.snapshot(ProviderKind::Claude, "claude", &home);
        assert_eq!(s.summary, "limits unavailable");
        assert!(s.detail.contains("boom"));
        assert_eq!(usage_tone(&s), UsageTone::Mute);
    }

    #[test]
    fn rows_put_current_account_first_with_used_and_left() {
        let root = tempfile::tempdir().unwrap();
        let mut f = UsageFetcher::new(root.path().to_path_buf())
            .with_cursor_runner(Arc::new(|_| Err("no agent in tests".into())));
        let accounts = vec![
            (
                "cursor".to_string(),
                ProviderKind::Cursor,
                root.path().into(),
            ),
            ("codex".to_string(), ProviderKind::Codex, root.path().into()),
        ];
        let rows = usage_rows_for_accounts(&mut f, &accounts, "codex");
        assert_eq!(rows[0].id, "codex");
        // Still checking: never a stale or invented number.
        assert!(rows[1].label.contains("checking limits"));
        assert_eq!(rows[1].tone, UsageTone::Mute);
    }

    const CURSOR_SAMPLE: &str = "\u{1b}[1mUsage \u{2022} Pro                                                                             Resets Nov 6\u{1b}[0m\n Monthly plan and on-demand usage\n\n Category        Current             Usage\n Included        27% used            \u{2588}\u{2588}\u{2588}\u{2591}\u{2591}\n   Auto          21% used            \u{2588}\u{2588}\u{2591}\u{2591}\u{2591}\n   API           79% used            \u{2588}\u{2588}\u{2588}\u{2588}\u{2591}\n On-Demand       Disabled            \u{2014}\u{2014}\u{2014}\u{2014}\n\n On-demand usage is off\n\n View in dashboard: cursor.com/dashboard?tab=usage\n";

    #[test]
    fn parses_the_cursor_usage_sample() {
        let s = parse_cursor_usage(CURSOR_SAMPLE).expect("parsed");
        assert_eq!(s.plan.as_deref(), Some("Pro"));
        let got: Vec<_> = s
            .windows
            .iter()
            .map(|w| (w.label.as_str(), w.used_pct))
            .collect();
        assert_eq!(got, [("Included", 27.0), ("Auto", 21.0), ("API", 79.0)]);
        assert_eq!(s.windows[0].resets.as_deref(), Some("Nov 6"));
        assert!(s
            .windows
            .iter()
            .all(|w| w.resets.as_deref() == Some("Nov 6")));
        assert!(picker_row("cursor", crate::store::ProviderKind::Cursor, &s)
            .label
            .contains("monthly limits"));
        assert_eq!(
            s.summary,
            "Included 73% left · Auto 79% left · API 21% left"
        );
        assert!(s.used_left_text().contains("Included 73% left (27% used)"));
        assert!(s
            .used_left_text()
            .contains("Included 73% left (27% used) · resets Nov 6"));
        assert!(
            s.detail.contains("Cursor Pro, on-demand off"),
            "{}",
            s.detail
        );
        assert_eq!(usage_tone(&s), UsageTone::Ok);
    }

    #[test]
    fn cursor_parse_handles_on_demand_percent_redraws_and_garbage() {
        let t = "Usage \u{2022} Ultra\n Included  90% used\n Included  96% used\n On-Demand  12% used\n";
        let s = parse_cursor_usage(t).unwrap();
        assert_eq!(s.windows.len(), 2);
        assert_eq!(s.windows[0].used_pct, 96.0, "last redraw wins");
        assert_eq!(s.windows[1].label, "On-Demand");
        assert_eq!(usage_tone(&s), UsageTone::Bad);
        for junk in [
            "",
            "hello",
            "Usage \u{2022} Pro\n Included  lots",
            "\u{1b}[31m\u{1b}]0;x\u{7}",
        ] {
            assert!(parse_cursor_usage(junk).is_none(), "{junk:?}");
        }
        // A row word inside prose is not a row.
        assert!(parse_cursor_usage("Automatic 50% used").is_none());
    }

    #[test]
    fn cursor_fetch_is_async_cached_and_failure_degrades() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let ok: UsageRunner = Arc::new(|_| Ok(CURSOR_SAMPLE.to_string()));
        let mut f = UsageFetcher::with_runner(root.path().to_path_buf(), fake_runner(SAMPLE))
            .with_cursor_runner(ok);
        assert_eq!(
            f.snapshot(ProviderKind::Cursor, "cursor", &home).summary,
            "checking limits..."
        );
        wait_for(&mut f);
        let s = f.snapshot(ProviderKind::Cursor, "cursor", &home);
        assert!(s.summary.starts_with("Included 73% left"));
        assert_eq!(s.plan.as_deref(), Some("Pro"));
        assert!(UsageIdentity::new(ProviderKind::Cursor, "cursor", &home)
            .cache_path(root.path())
            .is_file());
        // After a restart the cache answers; a failing runner is not even called.
        let bad: UsageRunner = Arc::new(|_| Err("must not run".into()));
        let mut g = UsageFetcher::new(root.path().to_path_buf()).with_cursor_runner(bad);
        assert!(g
            .snapshot(ProviderKind::Cursor, "cursor", &home)
            .summary
            .starts_with("Included 73% left"));
        assert_eq!(
            g.snapshot(ProviderKind::Cursor, "cursor", &home)
                .plan
                .as_deref(),
            Some("Pro")
        );
        // No cache + failure (timeout, login, layout): the old plain message.
        let bad: UsageRunner = Arc::new(|_| Err("timed out after 15s".into()));
        let mut h = UsageFetcher::new(root.path().to_path_buf()).with_cursor_runner(bad);
        h.snapshot(ProviderKind::Cursor, "cursor-new", &home);
        wait_for(&mut h);
        let s = h.snapshot(ProviderKind::Cursor, "cursor-new", &home);
        assert_eq!(s.summary, "not shown by Cursor CLI");
        // Unparseable text is a failure too.
        let junk: UsageRunner = Arc::new(|_| Ok("garbage".into()));
        let mut j = UsageFetcher::new(root.path().to_path_buf()).with_cursor_runner(junk);
        j.snapshot(ProviderKind::Cursor, "cursor-junk", &home);
        wait_for(&mut j);
        assert_eq!(
            j.snapshot(ProviderKind::Cursor, "cursor-junk", &home)
                .summary,
            "not shown by Cursor CLI"
        );
    }

    /// Runs a fake `claude` script (never the real CLI) through the real launcher.
    #[test]
    fn real_runner_runs_a_fake_script_with_the_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let (script, body) = (
            dir.path().join("fakeclaude.cmd"),
            "@echo off\r\nif \"%CLAUDE_CONFIG_DIR%\"==\"\" exit /b 3\r\necho Current session: 7%% used - resets Oct 15, 9:10pm (America/Phoenix)\r\necho Current week (all models): 13%% used\r\n",
        );
        #[cfg(unix)]
        let (script, body) = (
            dir.path().join("fakeclaude.sh"),
            "#!/bin/sh\n[ -n \"$CLAUDE_CONFIG_DIR\" ] || exit 3\necho 'Current session: 7% used - resets Oct 15, 9:10pm (America/Phoenix)'\necho 'Current week (all models): 13% used'\n",
        );
        std::fs::write(&script, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let out = run_claude_usage(
            script.to_str().unwrap(),
            dir.path(),
            Duration::from_secs(20),
        )
        .unwrap();
        let s = parse_claude_usage(&out).expect(&out);
        assert_eq!(s.windows.len(), 2);
    }

    #[test]
    fn real_runner_reports_a_missing_binary() {
        let dir = tempfile::tempdir().unwrap();
        let e = run_claude_usage(
            "concordia-no-such-binary",
            dir.path(),
            Duration::from_secs(2),
        );
        assert!(e.is_err());
    }

    #[test]
    fn no_bar_glyphs_appear_anywhere_in_the_text() {
        let snap = parse_claude_usage(SAMPLE).unwrap();
        let row = picker_row("claude-1", crate::store::ProviderKind::Claude, &snap);
        for text in [&snap.summary, &snap.detail, &row.label] {
            assert!(
                !text.contains("[#") && !text.contains("[-") && !text.contains("[."),
                "{text}"
            );
        }
        assert!(
            snap.detail.contains("used") && snap.detail.contains("left"),
            "{}",
            snap.detail
        );
    }

    #[test]
    fn a_codex_window_that_has_reset_since_the_last_chat_is_not_still_used() {
        let past = unix_now() as i64 - 3600;
        let future = unix_now() as i64 + 7200;
        let limits = CodexRateLimits {
            primary: Some(RateWindow {
                used_percent: Some(88.0),
                window_minutes: Some(300),
                resets_at: Some(past),
            }),
            secondary: Some(RateWindow {
                used_percent: Some(40.0),
                window_minutes: Some(10080),
                resets_at: Some(future),
            }),
            plan_type: None,
            limit_id: None,
            rate_limit_reached_type: None,
            credits: None,
        };
        let snap = format_limits(&limits, "as of your last chat");
        assert_eq!(
            snap.windows[0].used_pct, 88.0,
            "historical reading is preserved internally without inventing new capacity"
        );
        assert_eq!(snap.primary_pct, None);
        assert!(snap.summary.contains("usage unknown"));
        assert!(!snap.summary.contains("5h 100% left"));
        assert!(snap.windows[0]
            .resets
            .as_deref()
            .unwrap()
            .contains("reset at"));
        assert_eq!(
            snap.windows[1].used_pct, 40.0,
            "a window still running keeps its reading"
        );
        assert_eq!(usage_tone(&snap), UsageTone::Ok);
    }

    #[test]
    fn a_finished_turn_marks_the_reading_dirty_so_it_is_checked_again() {
        let root = tempfile::tempdir().unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c2 = Arc::clone(&calls);
        let runner: UsageRunner = Arc::new(move |_| {
            c2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(SAMPLE.to_string())
        });
        let mut f = UsageFetcher::with_runner(root.path().to_path_buf(), runner);
        let home = root.path().join("home");
        let p = crate::store::ProviderKind::Claude;
        f.snapshot(p, "claude-1", &home);
        wait_for(&mut f);
        f.snapshot(p, "claude-1", &home);
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "fresh reading is reused"
        );
        f.invalidate("claude-1");
        let shown = f.snapshot(p, "claude-1", &home);
        assert!(
            !shown.windows.is_empty(),
            "the old numbers stay up while re-checking"
        );
        wait_for(&mut f);
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "a turn triggers one re-check"
        );
    }

    #[test]
    fn cache_identity_separates_homes_and_late_results_cannot_replace_selection() {
        let root = tempfile::tempdir().unwrap();
        let home_a = root.path().join("home-a");
        let home_b = root.path().join("home-b");
        let identity_a = UsageIdentity::new(ProviderKind::Claude, "same-account", &home_a);
        let identity_b = UsageIdentity::new(ProviderKind::Claude, "same-account", &home_b);
        assert_ne!(
            identity_a, identity_b,
            "different normalized homes are distinct cache identities"
        );
        assert_ne!(identity_a.home, identity_b.home);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = std::sync::Mutex::new(release_rx);
        let runner: UsageRunner = Arc::new(move |home| {
            if home.ends_with("home-a") {
                started_tx.send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
                Ok(SAMPLE.to_string())
            } else {
                Ok(SAMPLE.replace("7% used", "42% used"))
            }
        });
        let mut f = UsageFetcher::with_runner(root.path().to_path_buf(), runner);
        f.snapshot(ProviderKind::Claude, "same-account", &home_a);
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        f.snapshot(ProviderKind::Claude, "same-account", &home_b);
        wait_for(&mut f); // Home B finishes while A remains deliberately blocked.
        let selected = f.snapshot(ProviderKind::Claude, "same-account", &home_b);
        assert!(
            selected.summary.contains("5h 58% left"),
            "{}",
            selected.summary
        );

        release_tx.send(()).unwrap();
        wait_for(&mut f);
        let still_selected = f.snapshot(ProviderKind::Claude, "same-account", &home_b);
        assert!(
            still_selected.summary.contains("5h 58% left"),
            "{}",
            still_selected.summary
        );
        assert!(
            UsageIdentity::new(ProviderKind::Claude, "same-account", &home_a)
                .cache_path(root.path())
                .is_file()
        );
        let cache_b = UsageIdentity::new(ProviderKind::Claude, "same-account", &home_b)
            .cache_path(root.path());
        assert!(cache_b.is_file());

        // A restart reuses only the matching provider/account/home cache.
        let must_not_run: UsageRunner =
            Arc::new(|_| Err("fresh bound cache should be used".into()));
        let mut restarted = UsageFetcher::with_runner(root.path().to_path_buf(), must_not_run)
            .with_cursor_runner(Arc::new(|_| Ok(CURSOR_SAMPLE.to_string())));
        let warm = restarted.snapshot(ProviderKind::Claude, "same-account", &home_b);
        assert!(warm.summary.contains("5h 58% left"), "{}", warm.summary);
        let provider_switch = restarted.snapshot(ProviderKind::Cursor, "same-account", &home_b);
        assert_eq!(provider_switch.summary, "checking limits...");
    }

    #[test]
    fn failed_forced_refresh_keeps_values_labeled_as_last_successful_cache() {
        let root = tempfile::tempdir().unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls2 = Arc::clone(&calls);
        let runner: UsageRunner = Arc::new(move |_| {
            if calls2.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                Ok(SAMPLE.to_string())
            } else {
                Err("synthetic refresh failure".into())
            }
        });
        let home = root.path().join("home");
        let mut f = UsageFetcher::with_runner(root.path().to_path_buf(), runner);
        f.snapshot(ProviderKind::Claude, "claude-test", &home);
        wait_for(&mut f);
        let refresh = f.snapshot_refresh(ProviderKind::Claude, "claude-test", &home);
        assert!(
            refresh.summary.contains("cached; refreshing"),
            "{}",
            refresh.summary
        );
        wait_for(&mut f);
        let after = f.snapshot(ProviderKind::Claude, "claude-test", &home);
        assert!(after.summary.contains("5h 93% left"), "{}", after.summary);
        assert!(
            after.summary.contains("cached; refresh failed"),
            "{}",
            after.summary
        );
        assert!(
            after.detail.contains("last successful reading"),
            "{}",
            after.detail
        );
    }
}

#[cfg(test)]
mod profile_row_tests {
    use super::*;
    #[test]
    fn account_email_plan_and_workspace_are_separate_from_limits() {
        let identity = crate::identity::ProviderIdentity {
            provider: crate::store::ProviderKind::Claude,
            home_path: "fixture".into(),
            login: Some("member@example.invalid".into()),
            team: Some("Fixture Team".into()),
            plan: Some("pro".into()),
            model: None,
            logged_in: true,
        };
        let mut snapshot = UsageSnapshot::no_data("checking limits", "");
        snapshot.windows.push(UsageWindow {
            label: "5h".into(),
            used_pct: 20.0,
            resets: None,
        });
        let row = profiled_picker_row("claude", identity.provider, &snapshot, &identity);
        assert_eq!(
            row.label.lines().next().unwrap(),
            "Claude \u{b7} Email: member@example.invalid \u{b7} Plan: Pro"
        );
        assert!(row.label.contains(
            "Email: member@example.invalid \u{b7} Plan: Pro\nWorkspace: Fixture Team\n5h"
        ));
        let missing = crate::identity::ProviderIdentity {
            login: None,
            plan: None,
            team: None,
            ..identity
        };
        let row = profiled_picker_row("claude", missing.provider, &snapshot, &missing);
        assert!(row
            .label
            .contains("Email: not reported \u{b7} Plan: not reported"));
    }
}
