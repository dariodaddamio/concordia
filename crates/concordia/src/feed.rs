//! Bounded, local event feed for observing a running Concordia process.
use crate::redact::redact;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const MAX_EVENT_TEXT: usize = 4096;
pub const MAX_FEED_BYTES: u64 = 2 * 1024 * 1024;
const FEED: &str = "feed.jsonl";

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
        if !self.enabled {
            return Ok(());
        }
        fs::create_dir_all(&self.root)?;
        let mut text = redact(text.as_ref()).into_owned();
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
            kind: kind.into(),
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
    pub fn screen(&self, text: &str) -> std::io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let tmp = self.root.join("screen.txt.tmp");
        fs::write(&tmp, redact(text).as_ref())?;
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
}
