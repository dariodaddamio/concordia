use serde::Deserialize;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct UsageSnapshot {
    pub summary: String,
    pub detail: String,
    pub primary_pct: Option<f64>,
    pub weekly_pct: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct CodexRateLimits {
    primary: Option<RateWindow>,
    secondary: Option<RateWindow>,
    plan_type: Option<String>,
    limit_id: Option<String>,
    rate_limit_reached_type: Option<String>,
    credits: Option<Credits>,
}

#[derive(Debug, Deserialize)]
struct RateWindow {
    used_percent: Option<f64>,
    window_minutes: Option<u32>,
    resets_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct Credits {
    has_credits: Option<bool>,
    unlimited: Option<bool>,
    balance: Option<f64>,
}

pub fn read_codex_usage(codex_home: &Path) -> UsageSnapshot {
    if let Some(snapshot) = read_tmp_rate_limits(codex_home) {
        return snapshot;
    }
    if let Some(snapshot) = read_session_rate_limits(codex_home) {
        return snapshot;
    }
    UsageSnapshot {
        summary: "limits unknown".into(),
        detail: "chat once with Codex to refresh limits".into(),
        primary_pct: None,
        weekly_pct: None,
    }
}

pub fn read_provider_usage(provider: crate::store::ProviderKind, home: &Path) -> UsageSnapshot {
    match provider {
        crate::store::ProviderKind::Codex => read_codex_usage(home),
        crate::store::ProviderKind::Claude => UsageSnapshot {
            summary: "limits unknown".into(),
            detail: "Claude subscription remaining is not exposed via a read-only CLI".into(),
            primary_pct: None,
            weekly_pct: None,
        },
        crate::store::ProviderKind::Cursor => UsageSnapshot {
            summary: "limits unknown".into(),
            detail: "Cursor CLI has no official usage % command".into(),
            primary_pct: None,
            weekly_pct: None,
        },
    }
}

/// Current account first, then other ready accounts.
pub fn usage_rows_for_accounts(
    accounts: &[(String, crate::store::ProviderKind, std::path::PathBuf)],
    current_account: &str,
) -> Vec<(String, String, String)> {
    let mut rows = Vec::new();
    let mut rest = Vec::new();
    for (id, provider, home) in accounts {
        let snap = read_provider_usage(*provider, home);
        let label = crate::accounts::display_account_name(*provider, id);
        let row = (
            id.clone(),
            format!("{label}  {}", snap.summary),
            snap.detail,
        );
        if id == current_account {
            rows.push(row);
        } else {
            rest.push(row);
        }
    }
    rows.extend(rest);
    rows
}

pub fn usage_tone(snap: &UsageSnapshot) -> UsageTone {
    let pct = snap.primary_pct.or(snap.weekly_pct).unwrap_or(0.0);
    if snap.summary.contains("LIMIT") || pct >= 95.0 {
        UsageTone::Bad
    } else if pct >= 80.0 {
        UsageTone::Warn
    } else if snap.primary_pct.is_some() || snap.weekly_pct.is_some() {
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

fn read_tmp_rate_limits(codex_home: &Path) -> Option<UsageSnapshot> {
    let tmp = codex_home.join("tmp");
    if !tmp.is_dir() {
        return None;
    }
    let mut files: Vec<_> = fs::read_dir(&tmp)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_file())
        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    files.sort_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()));
    files.reverse();
    for entry in files.into_iter().take(8) {
        let Ok(raw) = fs::read_to_string(entry.path()) else {
            continue;
        };
        if let Some(limits) = parse_rate_limits_blob(&raw) {
            if has_percents(&limits) {
                return Some(format_limits(&limits, "from CODEX_HOME/tmp cache"));
            }
        }
    }
    None
}

fn read_session_rate_limits(codex_home: &Path) -> Option<UsageSnapshot> {
    let sessions = codex_home.join("sessions");
    if !sessions.is_dir() {
        return None;
    }
    let mut files: Vec<(PathBuf, SystemTime)> = Vec::new();
    let mut stack = vec![sessions];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().map(|x| x == "jsonl").unwrap_or(false) {
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
        if best.is_none() {
            best = Some((hit, mtime));
        } else if prefer {
            best = Some((hit, mtime));
            break;
        }
    }
    let (limits, mtime) = best?;
    let age = SystemTime::now()
        .duration_since(mtime)
        .ok()
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let detail = if age > 3600 {
        format!("updated {} ago", format_age_minutes(age / 60))
    } else {
        "from last session".into()
    };
    Some(format_limits(&limits, &detail))
}

fn scan_file_for_rate_limits(path: &Path) -> Option<CodexRateLimits> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    // Rate-limit events arrive at turn ends — read a trailing window for large rollouts.
    const TAIL: u64 = 512 * 1024;
    if len > TAIL {
        file.seek(SeekFrom::Start(len - TAIL)).ok()?;
    }
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    // Drop partial first line when we seeked mid-file.
    let text = if len > TAIL {
        buf.split_once('\n').map(|(_, rest)| rest).unwrap_or(&buf)
    } else {
        buf.as_str()
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

fn format_limits(u: &CodexRateLimits, detail: &str) -> UsageSnapshot {
    let mut parts = Vec::new();
    if let Some(kind) = &u.rate_limit_reached_type {
        let kind = kind.trim_end_matches("_reached").replace('_', " ");
        parts.push(format!("LIMIT · {kind}"));
    }
    let primary_pct = u.primary.as_ref().and_then(|p| p.used_percent);
    let weekly_pct = u.secondary.as_ref().and_then(|s| s.used_percent);
    if let Some(p) = &u.primary {
        if let Some(pct) = p.used_percent {
            let w = p
                .window_minutes
                .map(format_window)
                .unwrap_or_else(|| "5h".into());
            parts.push(format!("{w} {}%", pct.round() as i64));
        }
    }
    if let Some(s) = &u.secondary {
        if let Some(pct) = s.used_percent {
            let w = s
                .window_minutes
                .map(format_window)
                .unwrap_or_else(|| "7d".into());
            parts.push(format!("{w} {}%", pct.round() as i64));
        }
    }
    if let Some(reset) = u
        .primary
        .as_ref()
        .and_then(|p| p.resets_at)
        .or_else(|| u.secondary.as_ref().and_then(|s| s.resets_at))
        .and_then(format_reset_unix)
    {
        if primary_pct.is_some() || u.rate_limit_reached_type.is_some() {
            parts.push(reset);
        }
    }
    if let Some(c) = &u.credits {
        if c.unlimited == Some(true) {
            parts.push("credits ∞".into());
        } else if let Some(bal) = c.balance {
            parts.push(format!("credits {bal}"));
        } else if c.has_credits == Some(true) && parts.is_empty() {
            parts.push("credits available".into());
        }
    }
    if parts.is_empty() {
        if let Some(plan) = &u.plan_type {
            if plan != "unknown" {
                parts.push(format!("plan {plan}"));
            }
        }
    }
    if parts.is_empty() {
        if let Some(id) = &u.limit_id {
            parts.push(id.clone());
        }
    }
    UsageSnapshot {
        summary: if parts.is_empty() {
            "limits unknown".into()
        } else {
            parts.join(" · ")
        },
        detail: detail.into(),
        primary_pct,
        weekly_pct,
    }
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
        return Some("resets soon".into());
    }
    let mins = (delta as f64 / 60_000.0).round() as i64;
    if mins < 60 {
        Some(format!("resets {mins}m"))
    } else if mins < 60 * 24 {
        Some(format!("resets {}h", mins / 60))
    } else {
        Some(format!("resets {}d", mins / (60 * 24)))
    }
}
