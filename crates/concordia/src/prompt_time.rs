//! Time on prompts: one leading line with the local date and time, then one line for each
//! relative time the prompt mentions ("yesterday" = Thu Oct 30 2026). No model is involved.
//!
//! The header goes to the provider only. Anything that reads a provider's session file back
//! for display removes it with [`strip_header`].

const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAYS: [&str; 7] = [
    "sunday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
/// Bounds the scan of an enormous paste and the lines a prompt can add.
const SCAN_CHARS: usize = 64 * 1024;
const MAX_RESOLVED: usize = 6;
/// "N days ago" further back than this is left to the model.
const MAX_DAYS_AGO: i64 = 366;

/// A wall-clock reading. `zone` is `local`, or `UTC` where the local zone is not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub zone: &'static str,
}

/// The clock the prompt is stamped with.
#[cfg(windows)]
pub fn now() -> LocalTime {
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
    LocalTime {
        year: t.year.into(),
        month: t.month.into(),
        day: t.day.into(),
        hour: t.hour.into(),
        minute: t.minute.into(),
        zone: "local",
    }
}

/// The clock the prompt is stamped with. The MVP is Windows only, so elsewhere it is UTC and
/// says so.
#[cfg(not(windows))]
pub fn now() -> LocalTime {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let of_day = secs.rem_euclid(86_400);
    LocalTime {
        year,
        month,
        day,
        hour: (of_day / 3600) as u32,
        minute: (of_day % 3600 / 60) as u32,
        zone: "UTC",
    }
}

/// The lines put before a prompt: the stamp, then any resolved relative times.
pub fn header(prompt: &str, now: &LocalTime) -> String {
    let mut lines = vec![stamp_line(now)];
    lines.extend(resolve_relative(prompt, now));
    lines.join("\n")
}

/// `[Fri Oct 31 2026, 2:05pm local]`
pub fn stamp_line(now: &LocalTime) -> String {
    let (hour, suffix) = match now.hour {
        0 => (12, "am"),
        1..=11 => (now.hour, "am"),
        12 => (12, "pm"),
        h => (h - 12, "pm"),
    };
    format!(
        "[{}, {hour}:{:02}{suffix} {}]",
        date(today(now)),
        now.minute,
        now.zone
    )
}

/// One line per relative time the prompt mentions, in the order they first appear.
pub fn resolve_relative(prompt: &str, now: &LocalTime) -> Vec<String> {
    let words = words(prompt);
    let today = today(now);
    let mut lines: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() && lines.len() < MAX_RESOLVED {
        let w = |k: usize| words.get(i + k).map(String::as_str);
        let found: Option<(usize, String, String)> = match (w(0), w(1), w(2)) {
            (Some("today"), _, _) => Some((1, "today".into(), date(today))),
            (Some("yesterday"), _, _) => Some((1, "yesterday".into(), date(today - 1))),
            (Some("this"), Some("morning"), _) => Some((
                2,
                "this morning".into(),
                format!("the morning of {}", date(today)),
            )),
            (Some("last"), Some("night"), _) => Some((
                2,
                "last night".into(),
                format!("the night of {}", date(today - 1)),
            )),
            (Some("last"), Some("week"), _) => {
                // Weeks run Monday to Sunday.
                let monday = today - (weekday(today) + 6) % 7 - 7;
                Some((
                    2,
                    "last week".into(),
                    format!("{} to {}", date(monday), date(monday + 6)),
                ))
            }
            (Some(n), Some("day" | "days"), Some("ago")) => count(n)
                .filter(|n| (1..=MAX_DAYS_AGO).contains(n))
                .map(|days| {
                    (
                        3,
                        format!("{n} {} ago", w(1).unwrap_or("days")),
                        date(today - days),
                    )
                }),
            (Some(lead @ ("on" | "last")), Some(name), _) => {
                WEEKDAYS.iter().position(|d| *d == name).map(|target| {
                    let target = target as i64;
                    let back = (weekday(today) - target + 6).rem_euclid(7) + 1;
                    let ahead = (target - weekday(today) + 6).rem_euclid(7) + 1;
                    let phrase = format!("{lead} {}", capitalized(name));
                    let resolved = if lead == "last" {
                        date(today - back)
                    } else {
                        // "On Monday" can point either way; give both and let the prompt decide.
                        format!(
                            "{} if past, {} if coming",
                            date(today - back),
                            date(today + ahead)
                        )
                    };
                    (2, phrase, resolved)
                })
            }
            _ => None,
        };
        match found {
            Some((len, phrase, resolved)) => {
                if !seen.contains(&phrase) {
                    lines.push(format!("\"{phrase}\" = {resolved}"));
                    seen.push(phrase);
                }
                i += len;
            }
            None => i += 1,
        }
    }
    lines
}

/// The text without a leading header written by [`header`]. Text without one is returned whole.
pub fn strip_header(text: &str) -> &str {
    let mut rest = text.trim_start();
    let Some((first, after)) = split_line(rest) else {
        return text;
    };
    if !is_stamp_line(first) {
        return text;
    }
    rest = after;
    while let Some((line, after)) = split_line(rest) {
        if !is_resolution_line(line) {
            break;
        }
        rest = after;
    }
    rest
}

fn split_line(text: &str) -> Option<(&str, &str)> {
    if text.is_empty() {
        return None;
    }
    let (line, rest) = text.split_once('\n').unwrap_or((text, ""));
    Some((line.trim_end_matches('\r'), rest))
}

fn is_stamp_line(line: &str) -> bool {
    line.len() <= 48
        && line.starts_with('[')
        && (line.ends_with("m local]") || line.ends_with("m UTC]"))
        && DAYS.iter().any(|d| line[1..].starts_with(d))
        && line.contains(", ")
}

fn is_resolution_line(line: &str) -> bool {
    line.starts_with('"') && line.contains("\" = ")
}

/// Days since 1970-01-01 for the reading's calendar date.
fn today(now: &LocalTime) -> i64 {
    days_from_civil(now.year, now.month, now.day)
}

/// `Thu Oct 30 2026`
fn date(days: i64) -> String {
    let (year, month, day) = civil_from_days(days);
    format!(
        "{} {} {day} {year}",
        DAYS[weekday(days) as usize],
        MONTHS[(month.clamp(1, 12) - 1) as usize]
    )
}

/// 0 is Sunday. 1970-01-01 was a Thursday.
fn weekday(days: i64) -> i64 {
    (days + 4).rem_euclid(7)
}

/// Days since 1970-01-01 in the proleptic Gregorian calendar (Howard Hinnant's algorithm).
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let month = i64::from(month.clamp(1, 12));
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    (year, month, day)
}

/// Lowercase ASCII words and numbers, bounded for huge pastes.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in text.chars().take(SCAN_CHARS) {
        if c.is_ascii_alphanumeric() {
            word.push(c.to_ascii_lowercase());
        } else if !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

fn count(word: &str) -> Option<i64> {
    let n = match word {
        "a" | "an" | "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        digits if digits.len() <= 4 => return digits.parse().ok(),
        _ => return None,
    };
    Some(n)
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> LocalTime {
        LocalTime {
            year,
            month,
            day,
            hour,
            minute,
            zone: "local",
        }
    }

    #[test]
    fn the_stamp_names_weekday_date_and_twelve_hour_time() {
        assert_eq!(
            stamp_line(&at(2026, 10, 31, 14, 5)),
            "[Sat Oct 31 2026, 2:05pm local]"
        );
        assert_eq!(
            stamp_line(&at(2026, 10, 30, 14, 5)),
            "[Fri Oct 30 2026, 2:05pm local]"
        );
        assert_eq!(
            stamp_line(&at(2026, 1, 1, 0, 0)),
            "[Thu Jan 1 2026, 12:00am local]"
        );
        assert_eq!(
            stamp_line(&at(2026, 6, 15, 12, 30)),
            "[Mon Jun 15 2026, 12:30pm local]"
        );
        assert_eq!(
            stamp_line(&at(2026, 6, 15, 23, 59)),
            "[Mon Jun 15 2026, 11:59pm local]"
        );
    }

    #[test]
    fn calendar_arithmetic_round_trips_across_leap_years_and_eras() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(
            civil_from_days(days_from_civil(2024, 2, 29) + 1),
            (2024, 3, 1)
        );
        assert_eq!(
            civil_from_days(days_from_civil(2100, 2, 28) + 1),
            (2100, 3, 1)
        );
        assert_eq!(
            civil_from_days(days_from_civil(2000, 2, 28) + 1),
            (2000, 2, 29)
        );
        for days in (-800_000..800_000).step_by(997) {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    #[test]
    fn a_prompt_without_relative_times_gets_only_the_stamp() {
        let now = at(2026, 10, 30, 14, 5);
        assert_eq!(
            header("Fix the resume picker", &now),
            "[Fri Oct 30 2026, 2:05pm local]"
        );
        assert!(resolve_relative("the yesterdays and todays of the code", &now).is_empty());
    }

    #[test]
    fn each_relative_time_resolves_once_in_order() {
        let now = at(2026, 10, 30, 14, 5);
        let lines = resolve_relative(
            "What did we do yesterday? Today I want to finish what started this morning, \
             and fix what broke last night. Yesterday again.",
            &now,
        );
        assert_eq!(
            lines,
            vec![
                "\"yesterday\" = Thu Oct 29 2026",
                "\"today\" = Fri Oct 30 2026",
                "\"this morning\" = the morning of Fri Oct 30 2026",
                "\"last night\" = the night of Thu Oct 29 2026",
            ]
        );
    }

    #[test]
    fn yesterday_crosses_month_and_year_boundaries() {
        assert_eq!(
            resolve_relative("yesterday", &at(2027, 1, 1, 9, 0)),
            vec!["\"yesterday\" = Thu Dec 31 2026"]
        );
        assert_eq!(
            resolve_relative("Yesterday", &at(2028, 3, 1, 9, 0)),
            vec!["\"yesterday\" = Tue Feb 29 2028"]
        );
    }

    #[test]
    fn last_week_is_the_previous_monday_to_sunday() {
        // Friday Oct 30 2026: this week starts Mon Oct 26.
        assert_eq!(
            resolve_relative("as we agreed last week", &at(2026, 10, 30, 9, 0)),
            vec!["\"last week\" = Mon Oct 19 2026 to Sun Oct 25 2026"]
        );
        // On a Monday, last week is the seven days just ended.
        assert_eq!(
            resolve_relative("last week", &at(2026, 10, 26, 9, 0)),
            vec!["\"last week\" = Mon Oct 19 2026 to Sun Oct 25 2026"]
        );
        // On a Sunday, the current week began six days earlier.
        assert_eq!(
            resolve_relative("last week", &at(2026, 11, 1, 9, 0)),
            vec!["\"last week\" = Mon Oct 19 2026 to Sun Oct 25 2026"]
        );
    }

    #[test]
    fn days_ago_accepts_digits_and_small_number_words() {
        let now = at(2026, 10, 30, 9, 0);
        assert_eq!(
            resolve_relative("3 days ago, two days ago and a day ago", &now),
            vec![
                "\"3 days ago\" = Tue Oct 27 2026",
                "\"two days ago\" = Wed Oct 28 2026",
                "\"a day ago\" = Thu Oct 29 2026",
            ]
        );
        assert_eq!(
            resolve_relative("30 days ago", &now),
            vec!["\"30 days ago\" = Wed Sep 30 2026"]
        );
        assert!(
            resolve_relative("0 days ago and 5000 days ago and many days ago", &now).is_empty()
        );
    }

    #[test]
    fn a_named_weekday_resolves_past_and_coming() {
        // Friday Oct 30 2026.
        let now = at(2026, 10, 30, 9, 0);
        assert_eq!(
            resolve_relative("we merged it on Monday", &now),
            vec!["\"on Monday\" = Mon Oct 26 2026 if past, Mon Nov 2 2026 if coming"]
        );
        assert_eq!(
            resolve_relative("since last Thursday", &now),
            vec!["\"last Thursday\" = Thu Oct 29 2026"]
        );
        // The same weekday as today means a week away, not today.
        assert_eq!(
            resolve_relative("on friday", &now),
            vec!["\"on Friday\" = Fri Oct 23 2026 if past, Fri Nov 6 2026 if coming"]
        );
        assert!(resolve_relative("on mondays we deploy", &now).is_empty());
    }

    #[test]
    fn a_long_list_of_times_is_capped() {
        let prompt = (1..=20)
            .map(|n| format!("{n} days ago"))
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(
            resolve_relative(&prompt, &at(2026, 10, 30, 9, 0)).len(),
            MAX_RESOLVED
        );
    }

    #[test]
    fn strip_header_returns_the_prompt_the_user_typed() {
        let now = at(2026, 10, 30, 14, 5);
        let prompt = "What changed yesterday?\nsecond line";
        let sent = format!("{}\n{prompt}", header(prompt, &now));
        assert_eq!(
            sent,
            "[Fri Oct 30 2026, 2:05pm local]\n\"yesterday\" = Thu Oct 29 2026\nWhat changed yesterday?\nsecond line"
        );
        assert_eq!(strip_header(&sent), prompt);
        assert_eq!(
            strip_header(&sent.replace('\n', "\r\n")),
            prompt.replace('\n', "\r\n")
        );
        let utc = format!("[Fri Oct 30 2026, 2:05pm UTC]\n{prompt}");
        assert_eq!(strip_header(&utc), prompt);
    }

    #[test]
    fn strip_header_leaves_other_text_alone() {
        for text in [
            "plain prompt",
            "[Fri Oct 30 2026] not a stamp\nrest",
            "[link](http://x) at 2:05pm local]",
            "\"yesterday\" = Thu Oct 29 2026\nno stamp first",
            "",
        ] {
            assert_eq!(strip_header(text), text);
        }
        // Only lines straight after the stamp are header lines.
        let sent = "[Fri Oct 30 2026, 2:05pm local]\nask\n\"quoted\" = later";
        assert_eq!(strip_header(sent), "ask\n\"quoted\" = later");
    }
}
