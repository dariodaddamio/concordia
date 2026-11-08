//! Bounded, local event feed for observing a running Concordia process.
use crate::redact::redact;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const MAX_EVENT_TEXT: usize = 4096;
pub const MAX_FEED_BYTES: u64 = 2 * 1024 * 1024;
const FEED: &str = "feed.jsonl";
const COALESCE_WINDOW: Duration = Duration::from_millis(100);
type DeltaKey = (PathBuf, String);
type DeltaValue = (std::time::Instant, String);
static DELTAS: OnceLock<Mutex<HashMap<DeltaKey, DeltaValue>>> = OnceLock::new();
static SCREENS: OnceLock<Mutex<HashMap<PathBuf, (std::time::Instant, u64)>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeedEvent {
    pub ts: u64,
    pub pane: String,
    pub conversation: Option<String>,
    pub account: Option<String>,
    pub provider: Option<String>,
    pub kind: String,
    pub text: String,
}

pub struct FeedWriter {
    root: PathBuf,
    pane: String,
    enabled: bool,
}

impl FeedWriter {
    pub fn new(root: &Path, pane: impl Into<String>, enabled: bool) -> Self {
        Self {
            root: root.into(),
            pane: pane.into(),
            enabled,
        }
    }
    pub fn emit(&self, kind: impl Into<String>, text: impl AsRef<str>) -> std::io::Result<()> {
        self.flush_text_delta()?;
        self.emit_raw(kind.into(), text.as_ref())
    }
    fn emit_raw(&self, kind: String, text: &str) -> std::io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        fs::create_dir_all(&self.root)?;
        let mut text = redact(text).into_owned();
        text.retain(|c| !c.is_control() || c == '\n' || c == '\t');
        if text.chars().count() > MAX_EVENT_TEXT {
            text = text.chars().take(MAX_EVENT_TEXT).collect();
        }
        let event = FeedEvent {
            ts: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            pane: self.pane.clone(),
            conversation: None,
            account: None,
            provider: None,
            kind,
            text,
        };
        let path = self.root.join(FEED);
        if fs::metadata(&path)
            .map(|m| m.len() > MAX_FEED_BYTES)
            .unwrap_or(false)
        {
            let _ = fs::rename(&path, self.root.join("feed.jsonl.1"));
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        serde_json::to_writer(&mut file, &event)?;
        file.write_all(b"\n")
    }
    pub fn emit_text_delta(&self, text: &str) -> std::io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let key = (self.root.clone(), self.pane.clone());
        let mut pending = DELTAS.get_or_init(Default::default).lock().unwrap();
        if let Some((started, value)) = pending.get_mut(&key) {
            if started.elapsed() < COALESCE_WINDOW {
                value.push_str(text);
                return Ok(());
            }
            let old = std::mem::take(value);
            *started = std::time::Instant::now();
            drop(pending);
            self.emit_raw("text_delta".into(), &old)?;
        } else {
            pending.insert(key.clone(), (std::time::Instant::now(), String::new()));
        }
        let mut pending = DELTAS.get().unwrap().lock().unwrap();
        pending.get_mut(&key).unwrap().1.push_str(text);
        Ok(())
    }
    pub fn flush_text_delta(&self) -> std::io::Result<()> {
        let key = (self.root.clone(), self.pane.clone());
        let Some((_, text)) = DELTAS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .remove(&key)
        else {
            return Ok(());
        };
        if text.is_empty() {
            Ok(())
        } else {
            self.emit_raw("text_delta".into(), &text)
        }
    }
    pub fn screen(&self, text: &str) -> std::io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let value = redact(text).into_owned();
        let hash = value.bytes().fold(0xcbf29ce484222325u64, |h, b| {
            h.wrapping_mul(0x100000001b3).wrapping_add(b as u64)
        });
        let mut screens = SCREENS.get_or_init(Default::default).lock().unwrap();
        if let Some((at, previous)) = screens.get(&self.root) {
            if *previous == hash || at.elapsed() < Duration::from_secs(1) {
                return Ok(());
            }
        }
        screens.insert(self.root.clone(), (std::time::Instant::now(), hash));
        drop(screens);
        let tmp = self.root.join("screen.txt.tmp");
        fs::write(&tmp, value)?;
        fs::rename(tmp, self.root.join("screen.txt"))
    }
}

pub fn read(root: &Path, since: usize, pane: Option<&str>) -> std::io::Result<Vec<FeedEvent>> {
    let mut out = Vec::new();
    for path in [root.join("feed.jsonl.1"), root.join(FEED)] {
        let Ok(file) = fs::File::open(path) else {
            continue;
        };
        for line in BufReader::new(file).lines() {
            let Ok(line) = line else { continue };
            if let Ok(event) = serde_json::from_str::<FeedEvent>(&line) {
                if pane.is_none_or(|p| p == event.pane) {
                    out.push(event);
                }
            }
        }
    }
    Ok(out.into_iter().skip(since).collect())
}

pub fn cli(root: &Path, args: &[String]) -> Result<String, (String, i32)> {
    let follow = args.iter().any(|a| a == "--follow");
    let json_out = args.iter().any(|a| a == "--json");
    let screen = args.iter().any(|a| a == "--screen");
    let pane = args
        .windows(2)
        .find(|w| w[0] == "--pane")
        .map(|w| w[1].as_str());
    let since = args
        .windows(2)
        .find(|w| w[0] == "--since")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(0);
    if screen {
        return fs::read_to_string(root.join("screen.txt")).map_err(|e| (e.to_string(), 1));
    }
    let mut out = String::new();
    let mut n = since;
    loop {
        let events = read(root, n, pane).map_err(|e| (e.to_string(), 1))?;
        n += events.len();
        for e in events {
            let line = if json_out {
                serde_json::to_string(&e).unwrap()
            } else {
                format!("{} [{}] {}: {}\n", e.ts, e.pane, e.kind, e.text)
            };
            out.push_str(&line);
            if json_out {
                out.push('\n');
            }
        }
        if !follow {
            break;
        }
        print!("{out}");
        out.clear();
        thread::sleep(Duration::from_millis(250));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_redacted_bounded_json_and_rotates() {
        let dir = tempdir().unwrap();
        let writer = FeedWriter::new(dir.path(), "pane-a", true);
        writer
            .emit("text_delta", "token=sk-abcdefghijklmnop")
            .unwrap();
        let event = read(dir.path(), 0, None).unwrap().pop().unwrap();
        assert_eq!(event.kind, "text_delta");
        assert!(event.text.contains("[REDACTED:"));
        assert!(!event.text.contains("sk-abcdefghijklmnop"));
        fs::write(
            dir.path().join(FEED),
            vec![b'x'; MAX_FEED_BYTES as usize + 1],
        )
        .unwrap();
        writer.emit("error", "rotated").unwrap();
        assert!(dir.path().join("feed.jsonl.1").exists());
    }

    #[test]
    fn cli_reads_json_and_filters_pane() {
        let dir = tempdir().unwrap();
        FeedWriter::new(dir.path(), "one", true)
            .emit("turn_start", "a")
            .unwrap();
        FeedWriter::new(dir.path(), "two", true)
            .emit("turn_end", "b")
            .unwrap();
        let output = cli(
            dir.path(),
            &["--json".into(), "--pane".into(), "one".into()],
        )
        .unwrap();
        assert!(output.contains("turn_start"));
        assert!(!output.contains("turn_end"));
    }

    #[test]
    fn text_deltas_coalesce_until_flushed() {
        let dir = tempdir().unwrap();
        let writer = FeedWriter::new(dir.path(), "one", true);
        writer.emit_text_delta("hel").unwrap();
        writer.emit_text_delta("lo").unwrap();
        assert!(read(dir.path(), 0, None).unwrap().is_empty());
        writer.flush_text_delta().unwrap();
        let events = read(dir.path(), 0, None).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].text, "hello");
    }
}
