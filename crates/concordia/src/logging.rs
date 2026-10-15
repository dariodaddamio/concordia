//! A small append-only event log in the state folder (`concordia.log`), so a crash or a strange
//! failure leaves a trace. It records *events* (startup, turn outcomes, updates, account changes,
//! panics) and never prompt text, replies, or credentials.
//!
//! It rotates at 1 MB (`concordia.log.1`). `CONCORDIA_LOG=off` disables it.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 1024 * 1024;

static PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn log_path(state_root: &Path) -> PathBuf {
    state_root.join("concordia.log")
}

fn disabled() -> bool {
    std::env::var("CONCORDIA_LOG").is_ok_and(|v| v.eq_ignore_ascii_case("off") || v == "0")
}

/// Start logging to `<state_root>/concordia.log`. Safe to call more than once; the first wins.
pub fn init(state_root: &Path) {
    if !disabled() {
        let _ = PATH.set(log_path(state_root));
    }
}

pub fn info(msg: &str) {
    write_event("INFO", msg);
}

pub fn error(msg: &str) {
    write_event("ERROR", msg);
}

fn write_event(level: &str, msg: &str) {
    if let Some(path) = PATH.get() {
        let _ = append(path, level, msg, now_secs());
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// UTC `YYYY-MM-DDTHH:MM:SSZ` for a unix timestamp (civil-from-days, no dependencies).
pub fn iso_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Append one line, rotating first if the file is already at its size cap. A line is written with
/// a single `write_all` in append mode, so concurrent panes do not interleave inside a line.
fn append(path: &Path, level: &str, msg: &str, secs: u64) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    if fs::metadata(path).map(|m| m.len()).unwrap_or(0) >= MAX_BYTES {
        let rotated = path.with_extension("log.1");
        let _ = fs::remove_file(&rotated);
        let _ = fs::rename(path, rotated);
    }
    // Keep it to one line per event.
    let flat = msg.replace(['\r', '\n'], " | ");
    let line = format!(
        "{} {level:<5} pid={} {flat}\n",
        iso_utc(secs),
        std::process::id()
    );
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_correct_utc() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(iso_utc(951_782_400), "2000-02-29T00:00:00Z"); // a leap day
        assert_eq!(iso_utc(4_102_444_799), "2099-12-31T23:59:59Z");
    }

    #[test]
    fn an_event_is_one_flat_line() {
        let d = tempfile::tempdir().unwrap();
        let p = log_path(d.path());
        append(&p, "INFO", "turn codex exit=0\nsecond line", 1_700_000_000).unwrap();
        append(&p, "ERROR", "boom", 1_700_000_001).unwrap();
        let text = fs::read_to_string(&p).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("2023-11-14T22:13:20Z INFO "));
        assert!(lines[0].contains("turn codex exit=0 | second line"));
        assert!(lines[1].contains("ERROR") && lines[1].ends_with("boom"));
    }

    #[test]
    fn the_log_rotates_at_the_size_cap_and_keeps_one_old_file() {
        let d = tempfile::tempdir().unwrap();
        let p = log_path(d.path());
        fs::write(&p, vec![b'x'; MAX_BYTES as usize]).unwrap();
        append(&p, "INFO", "after rotation", 0).unwrap();
        assert!(fs::read_to_string(&p).unwrap().contains("after rotation"));
        assert!(
            fs::metadata(&p).unwrap().len() < 1000,
            "fresh file after rotating"
        );
        assert_eq!(
            fs::metadata(p.with_extension("log.1")).unwrap().len(),
            MAX_BYTES
        );
        // A second rotation replaces the old file rather than accumulating.
        fs::write(&p, vec![b'y'; MAX_BYTES as usize]).unwrap();
        append(&p, "INFO", "again", 0).unwrap();
        assert_eq!(fs::read(p.with_extension("log.1")).unwrap()[0], b'y');
    }

    #[test]
    fn concurrent_writers_never_split_a_line() {
        let d = tempfile::tempdir().unwrap();
        let p = log_path(d.path());
        let threads: Vec<_> = (0..6)
            .map(|t| {
                let p = p.clone();
                std::thread::spawn(move || {
                    for i in 0..50 {
                        append(
                            &p,
                            "INFO",
                            &format!("writer {t} event {i} {}", "z".repeat(200)),
                            0,
                        )
                        .unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let text = fs::read_to_string(&p).unwrap();
        assert_eq!(text.lines().count(), 300);
        assert!(text
            .lines()
            .all(|l| l.contains("writer ") && l.ends_with(&"z".repeat(200))));
    }
}
