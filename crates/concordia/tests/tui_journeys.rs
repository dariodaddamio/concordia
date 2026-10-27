//! Drives the real TUI in a Windows pseudo-console with synthetic provider processes.
//! Every provider path, account home, workspace and state folder is isolated.
#![cfg(windows)]

use concordia::store::{
    save_store, set_delegation_protocol, set_workers, OrchestratorStore, ProviderKind, UseSelection,
};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_concordia");
const FAKE: &str = env!("CARGO_BIN_EXE_concordia-fake-provider");
const COLS: usize = 120;
const ROWS: usize = 40;
static JOURNEY_LOCK: Mutex<()> = Mutex::new(());

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn usage_limit_automatically_continues_on_configured_provider_without_foreign_session() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    std::fs::write(dir.path().join("config.toml"), "[threads]\nenabled=true\n[ui]\ntips=false\n[continuation]\nmode='automatic'\naccounts=['claude']\n").unwrap();
    let mut s = Session::launch(dir.path());
    s.submit("Fix graph [quota]");
    s.until("handoff reply completed", |s| {
        s.journal()
            .iter()
            .any(|e| e["kind"] == "done" && e["provider"] == "claude")
    });
    let events = s.journal();
    let starts: Vec<_> = events.iter().filter(|e| e["kind"] == "start").collect();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0]["provider"], "codex");
    assert_eq!(starts[1]["provider"], "claude");
    let prompt = starts[1]["prompt"].as_str().unwrap();
    assert!(prompt.contains("[concordia map]"));
    assert!(prompt.contains("Original request:\nFix graph [quota]"));
    assert!(!starts[1]["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|arg| arg == "quota-source-session" || arg == "--resume"));
    assert!(starts[1]["home"].as_str().unwrap().ends_with(".claude"));
    s.until("idle after continuation", |s| s.screen().contains("in 123"));
    s.exit();
}

struct Session {
    child: Box<dyn Child + Send + Sync>,
    writer: Option<Box<dyn Write + Send>>,
    master: Option<Box<dyn MasterPty + Send>>,
    bytes: Arc<Mutex<Vec<u8>>>,
    root: PathBuf,
}

fn setup(root: &Path, provider: ProviderKind) {
    let profile = root.join("profile");
    for (name, marker, body) in [
        (".codex", "auth.json", "{}"),
        (
            ".claude",
            ".credentials.json",
            "synthetic-login-existence-marker",
        ),
        (
            ".cursor",
            "cli-config.json",
            "{\"authInfo\":{\"email\":\"fake@example.invalid\"}}",
        ),
    ] {
        std::fs::create_dir_all(profile.join(name)).unwrap();
        std::fs::write(profile.join(name).join(marker), body).unwrap();
    }
    std::fs::create_dir_all(root.join("workspace")).unwrap();
    std::fs::write(
        root.join("config.toml"),
        "[threads]\nenabled = true\n[ui]\ntips = false\n",
    )
    .unwrap();
    let store = OrchestratorStore {
        selection: Some(UseSelection {
            provider,
            account: provider.as_str().into(),
            home_path: Some(
                profile
                    .join(format!(".{}", provider.as_str()))
                    .to_string_lossy()
                    .into_owned(),
            ),
            model: Some("synthetic-model".into()),
            updated_at: "0".into(),
        }),
        ..Default::default()
    };
    save_store(root, &store).unwrap();
}

impl Session {
    fn launch(root: &Path) -> Self {
        Self::launch_at(root, &root.join("workspace"))
    }

    fn launch_at(root: &Path, workspace: &Path) -> Self {
        Self::launch_args(root, workspace, &[])
    }

    fn launch_args(root: &Path, workspace: &Path, args: &[&str]) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: ROWS as u16,
                cols: COLS as u16,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let binary = std::env::var_os("CONCORDIA_JOURNEY_EXE")
            .unwrap_or_else(|| std::ffi::OsString::from(BIN));
        let mut cmd = CommandBuilder::new(binary);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.cwd(workspace);
        for key in [
            "NO_COLOR",
            "CONCORDIA_CONFIG",
            "AO_CODEX_HOME",
            "AO_CODEX_HOME_A",
            "AO_CODEX_HOME_2",
            "AO_CODEX_HOME_B",
            "AO_CLAUDE_CONFIG_DIR",
            "AO_CURSOR_HOME",
        ] {
            cmd.env_remove(key);
        }
        for key in [
            "CONCORDIA_CODEX_BIN",
            "CONCORDIA_CLAUDE_BIN",
            "CONCORDIA_AGENT_BIN",
        ] {
            cmd.env(key, FAKE);
        }
        cmd.env("CONCORDIA_STATE_DIR", root);
        cmd.env("CONCORDIA_WEZTERM_BIN", FAKE);
        cmd.env("WEZTERM_PANE", "7");
        cmd.env("FAKE_FILE_LAUNCH_JOURNAL", root.join("file-launch.json"));
        cmd.env("CONCORDIA_CODEX_HOME_BASE", root.join("profile"));
        cmd.env("FAKE_PROVIDER_MODE", "interaction");
        cmd.env("FAKE_PROVIDER_JOURNAL", root.join("received.jsonl"));
        cmd.env("FAKE_PROVIDER_GATE", root.join("gate"));
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let writer = pair.master.take_writer().unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let sink = bytes.clone();
        std::thread::spawn(move || {
            let mut buf = [0; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        let mut session = Self {
            child,
            writer: Some(writer),
            master: Some(pair.master),
            bytes,
            root: root.into(),
        };
        session.send("\x1b[1;1R");
        session.until("startup", |s| {
            s.screen().contains(if args.first() == Some(&"preview") {
                "read-only"
            } else {
                "concordia"
            })
        });
        session
    }

    fn send(&mut self, text: &str) {
        self.writer
            .as_mut()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
        self.writer.as_mut().unwrap().flush().unwrap();
        // Establish a quiet boundary between a paste/typing burst and an explicit key.
        std::thread::sleep(Duration::from_millis(90));
    }

    fn paste(&mut self, text: &str) {
        self.send(&format!("\x1b[200~{text}\x1b[201~"));
    }
    fn enter(&mut self) {
        self.send("\r");
    }
    fn submit(&mut self, text: &str) {
        self.paste(text);
        self.until("typed prompt ready", |s| s.draft() == text);
        self.enter();
    }

    fn queue_current_draft(&mut self, text: &str) {
        self.until("draft ready to queue", |s| s.draft() == text);
        self.send("\t");
        self.until("queued item persisted after Tab", |s| {
            s.queue().iter().any(|item| item == text) && s.draft().is_empty()
        });
    }

    fn focus_queue(&mut self) {
        self.send("\x1b[1;5A");
        self.until("queue focus acknowledged", |s| {
            s.screen().contains("Ctrl+Down: draft; Ctrl+Up: queue")
        });
    }

    fn enter_queue_editor(&mut self) {
        self.enter();
        self.until("upper editor active", |s| {
            let screen = s.screen();
            (1..=s.queue().len()).any(|n| screen.contains(&format!("edit {n}:")))
        });
    }

    fn journal(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.join("received.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    fn prompts(&self) -> Vec<String> {
        self.journal()
            .iter()
            .filter(|e| e["kind"] == "start")
            .map(|e| e["prompt"].as_str().unwrap().to_string())
            .collect()
    }

    fn recovery_data(&self) -> Value {
        let pid = self.child.process_id().unwrap_or(0);
        let mut records = Vec::new();
        if let Ok(workspaces) = std::fs::read_dir(self.root.join("pane-recovery")) {
            for workspace in workspaces.flatten() {
                if let Ok(panes) = std::fs::read_dir(workspace.path()) {
                    for pane in panes.flatten() {
                        if let Ok(bytes) = std::fs::read(pane.path().join("recovery.json")) {
                            if let Ok(record) = serde_json::from_slice::<Value>(&bytes) {
                                if record["owner_pid"].as_u64() == Some(pid as u64) {
                                    records.push(record);
                                }
                            }
                        }
                    }
                }
            }
        }
        records
            .into_iter()
            .max_by_key(|r| r["updated_unix_ms"].as_u64().unwrap_or(0))
            .unwrap_or(Value::Null)
    }

    fn draft(&self) -> String {
        self.recovery_data()["draft"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    fn queue(&self) -> Vec<String> {
        self.recovery_data()["queue"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    fn screen(&self) -> String {
        render(&self.bytes.lock().unwrap())
    }

    fn until(&self, label: &str, check: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(12) {
            if check(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!(
            "timed out: {label}\nscreen:\n{}\nprompts: {:?}\nqueue: {:?}\ndraft: {:?}",
            format_args!(
                "{}\nraw: {:?}",
                self.screen(),
                String::from_utf8_lossy(&self.bytes.lock().unwrap())
                    .chars()
                    .take(1800)
                    .collect::<String>()
            ),
            self.prompts(),
            self.queue(),
            self.draft()
        );
    }

    fn release(&self) {
        std::fs::write(self.root.join("gate"), "continue").unwrap();
    }

    fn exit(&mut self) {
        self.send("\x01"); // select draft, then delete it, so /exit is an actual command
        self.send("\x08");
        self.submit("/exit");
        let start = Instant::now();
        loop {
            if self.child.try_wait().unwrap().is_some() {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(8),
                "TUI would not exit"
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            if let Some(pid) = self.child.process_id() {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        self.writer.take();
        if let Some(master) = self.master.take() {
            std::thread::spawn(move || drop(master));
        }
    }
}

// Minimal VT screen for observing the real terminal's final cells, rather than searching old
// output that may already have been erased. Colors and modes do not affect these assertions.
fn render(bytes: &[u8]) -> String {
    let mut cells = vec![vec![' '; COLS]; ROWS];
    let (mut x, mut y) = (0usize, 0usize);
    let text = String::from_utf8_lossy(bytes);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => {
                    let mut params = String::new();
                    let mut op = ' ';
                    for c in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&c) {
                            op = c;
                            break;
                        }
                        params.push(c);
                    }
                    let n: Vec<usize> = params.split(';').map(|s| s.parse().unwrap_or(0)).collect();
                    let first = n.first().copied().unwrap_or(0);
                    let amount = first.max(1);
                    match op {
                        'H' | 'f' => {
                            y = amount.saturating_sub(1).min(ROWS - 1);
                            x = n
                                .get(1)
                                .copied()
                                .unwrap_or(1)
                                .max(1)
                                .saturating_sub(1)
                                .min(COLS - 1);
                        }
                        'A' => y = y.saturating_sub(amount),
                        'B' => y = (y + amount).min(ROWS - 1),
                        'C' => x = (x + amount).min(COLS - 1),
                        'D' => x = x.saturating_sub(amount),
                        'G' => x = amount.saturating_sub(1).min(COLS - 1),
                        'd' => y = amount.saturating_sub(1).min(ROWS - 1),
                        'J' if first == 2 || first == 3 => {
                            cells.iter_mut().for_each(|r| r.fill(' '))
                        }
                        'J' if first == 0 => {
                            cells[y][x..].fill(' ');
                            cells[y + 1..].iter_mut().for_each(|r| r.fill(' '));
                        }
                        'K' => match first {
                            1 => cells[y][..=x].fill(' '),
                            2 => cells[y].fill(' '),
                            _ => cells[y][x..].fill(' '),
                        },
                        'X' => cells[y][x..(x + amount).min(COLS)].fill(' '),
                        _ => {}
                    }
                }
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' {
                            chars.next();
                            break;
                        }
                    }
                }
                Some('(' | ')') => {
                    chars.next();
                }
                _ => {}
            },
            '\r' => x = 0,
            '\n' => {
                if y + 1 == ROWS {
                    cells.remove(0);
                    cells.push(vec![' '; COLS]);
                } else {
                    y += 1;
                }
            }
            '\x08' => x = x.saturating_sub(1),
            c if !c.is_control() => {
                if x >= COLS {
                    x = 0;
                    y = (y + 1).min(ROWS - 1);
                }
                cells[y][x] = c;
                x += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            }
            _ => {}
        }
    }
    cells
        .iter()
        .map(|r| r.iter().collect::<String>().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn multiline_paste_and_expand_work_through_the_real_terminal_for_each_provider() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for provider in [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Cursor,
    ] {
        let dir = tempfile::tempdir().unwrap();
        setup(dir.path(), provider);
        let mut s = Session::launch(dir.path());
        let prompt = "first line\n\nsecond line with quotes ' and \" and Unicode café 界 🦀";
        s.paste(prompt);
        s.until("multiline draft saved", |s| s.draft() == prompt);
        assert!(s.prompts().is_empty(), "paste submitted before Enter");
        s.enter();
        s.until("one complete prompt", |s| s.prompts() == [prompt]);
        s.until("reply visible", |s| s.screen().contains("FAKE REPLY:"));
        s.send("\x0f"); // Ctrl+O
        s.until("full command visible", |s| {
            s.screen().contains("command-tail")
        });
        s.until("full tool output visible", |s| {
            s.screen().contains("tool-output-end")
        });
        if provider == ProviderKind::Codex {
            eprintln!("EXPANDED ACTIVITY SCREEN:\n{}", s.screen());
        }
        s.exit();
        eprintln!("PASS {provider:?}: multiline paste, explicit send, tool activity expansion");
    }
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn queue_can_be_edited_reordered_and_automatically_sent_with_a_draft_in_progress() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("[wait] first");
    s.until("first started", |s| s.prompts().len() == 1);
    for prompt in ["second", "third"] {
        s.paste(prompt);
        s.queue_current_draft(prompt);
    }
    s.until("two queued", |s| s.queue() == ["second", "third"]);
    s.send("\x1b[A"); // Up focuses last queue row
    s.enter_queue_editor();
    s.paste(" revised");
    s.enter();
    s.until("queue edit saved", |s| {
        s.queue() == ["second", "third revised"]
    });
    s.send("\x1b[1;3A"); // Alt+Up
    s.until("queue reordered", |s| {
        s.queue() == ["third revised", "second"]
    });
    s.send("\x1b");
    s.paste("unfinished draft");
    s.until("draft saved", |s| s.draft() == "unfinished draft");
    s.release();
    s.until("all three sent in order", |s| {
        s.prompts() == ["[wait] first", "third revised", "second"]
    });
    s.until("last reply", |s| s.screen().contains("FAKE REPLY: second"));
    assert_eq!(s.draft(), "unfinished draft");
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn cancellation_pauses_queued_work_and_restart_restores_both_queue_and_draft() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for provider in [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Cursor,
    ] {
        let dir = tempfile::tempdir().unwrap();
        setup(dir.path(), provider);
        let mut s = Session::launch(dir.path());
        s.submit("[wait] cancel this");
        s.until("turn started", |s| s.prompts().len() == 1);
        s.until("running command visible before provider completion", |s| {
            s.screen().contains("synthetic-command")
        });
        assert!(
            !dir.path().join("gate").exists(),
            "the command must render while provider completion is gated"
        );
        assert!(
            s.journal().iter().all(|event| event["kind"] != "done"),
            "provider must still be running when its command is visible"
        );
        s.send("\x0f"); // Ctrl+O: inspect the running command, not only its compact row.
        s.until("full running command visible before completion", |s| {
            s.screen().contains("command-tail")
        });
        s.paste("queued followup");
        s.queue_current_draft("queued followup");
        s.paste("preserve this draft");
        s.send("\x1b");
        s.until("cancel and pause", |s| s.screen().contains("queue paused"));
        s.until("draft preserved", |s| s.draft() == "preserve this draft");
        assert_eq!(s.prompts().len(), 1);
        drop(s); // abrupt exit, not /exit
        let mut s = Session::launch(dir.path());
        s.until("restored draft", |s| {
            s.screen().contains("preserve this draft")
        });
        s.until("restored paused queue", |s| {
            s.screen().contains("queue paused")
        });
        assert_eq!(s.queue(), ["queued followup"]);
        assert_eq!(s.prompts().len(), 1);
        assert!(
            s.journal().iter().all(|event| event["kind"] != "done"),
            "cancellation and restart must not complete or replay the gated turn"
        );
        s.exit();
    }
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn completing_a_turn_while_a_queued_item_is_being_edited_does_not_send_stale_text() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("[wait] first");
    s.until("turn started", |s| s.prompts().len() == 1);
    s.paste("queued original");
    s.queue_current_draft("queued original");
    s.send("\x1b[A");
    s.enter_queue_editor();
    s.paste(" edited");
    s.release();
    s.until("first completed", |s| {
        s.screen().contains("FAKE REPLY: [wait] first")
    });
    assert_eq!(
        s.prompts().len(),
        1,
        "queued text must not be sent while the user is editing it"
    );
    s.enter();
    s.until("edited prompt sent", |s| {
        s.prompts() == ["[wait] first", "queued original edited"]
    });
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn unbracketed_crlf_paste_keeps_blank_lines_and_trailing_newline_until_send() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.send("alpha\r\n\r\nbeta\r\n");
    s.until("CRLF paste normalized in draft", |s| {
        s.draft() == "alpha\n\nbeta\n"
    });
    assert!(s.prompts().is_empty());
    s.enter();
    s.until("CRLF paste submitted once", |s| {
        s.prompts() == ["alpha\n\nbeta"]
    });
    s.until("reply", |s| s.screen().contains("FAKE REPLY:"));
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn undo_redo_and_resize_preserve_a_long_draft() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    let draft = format!(
        "long draft\n{}\nend of draft",
        "a quoted argument 'x' \"y\"\n".repeat(120)
    );
    s.paste(&draft);
    s.until("long paste persisted intact", |s| s.draft() == draft);
    assert!(s.prompts().is_empty());
    s.send("\x1a"); // Ctrl+Z
    s.until("undo entire paste", |s| s.draft().is_empty());
    s.send("\x19"); // Ctrl+Y
    s.until("redo entire paste", |s| s.draft() == draft);
    s.master
        .as_ref()
        .unwrap()
        .resize(PtySize {
            rows: 18,
            cols: 60,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    s.paste(" after resize");
    s.until("resized draft intact", |s| {
        s.draft() == format!("{draft} after resize")
    });
    assert!(s.prompts().is_empty());
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn cancelling_after_slash_commands_does_not_restore_the_submitted_prompt() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("[wait] original prompt");
    s.until("turn started", |s| s.prompts().len() == 1);
    s.submit("/stats");
    s.send("\x1b");
    s.until("cancellation completed with empty draft", |s| {
        s.draft().is_empty() && s.screen().contains("cancelled")
    });
    assert_eq!(s.prompts(), ["[wait] original prompt"]);
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn provider_failure_pauses_queue_and_preserves_a_newer_draft() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Claude);
    let mut s = Session::launch(dir.path());
    s.submit("[wait] [fail] deliberate failure");
    s.until("turn started", |s| s.prompts().len() == 1);
    s.paste("next prompt");
    s.queue_current_draft("next prompt");
    s.paste("newer draft");
    s.release();
    s.until("failure reported and queue paused", |s| {
        s.screen().contains("queue paused") && s.screen().contains("provider exited with code 3")
    });
    s.until("newer draft preserved", |s| s.draft() == "newer draft");
    assert_eq!(s.prompts().len(), 1);
    assert_eq!(s.queue(), ["next prompt"]);
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn two_panes_keep_live_drafts_and_recovery_files_separate() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut first = Session::launch(dir.path());
    first.paste("draft from first pane");
    first.until("first draft saved", |s| {
        s.draft() == "draft from first pane"
    });
    let mut second = Session::launch(dir.path());
    assert!(!second.screen().contains("draft from first pane"));
    assert_eq!(second.draft(), "");
    second.send("\x01");
    second.send("\x08");
    second.paste("draft from second pane");
    second.until("second draft saved", |s| {
        s.draft() == "draft from second pane"
    });
    assert!(first.screen().contains("draft from first pane"));
    assert!(!first.screen().contains("draft from second pane"));
    assert_eq!(first.draft(), "draft from first pane");
    assert_eq!(second.draft(), "draft from second pane");
    assert!(first.prompts().is_empty());
    assert!(second.prompts().is_empty());
    eprintln!("PASS: active panes have separate draft recovery");
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn ctrl_up_edits_multiline_queue_without_replacing_a_nonempty_draft() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("[wait] first");
    s.until("turn started", |s| s.prompts().len() == 1);
    s.paste("queued original");
    s.send("\t");
    s.paste("main draft\nstays here");
    s.until("main draft persisted", |s| {
        s.draft() == "main draft\nstays here"
    });
    s.focus_queue(); // Ctrl+Up, with a nonempty draft
    s.enter_queue_editor();
    s.paste("\nsecond line\nthird line");
    s.until("multiline edit visible", |s| {
        s.screen().contains("third line")
    });
    assert_eq!(s.draft(), "main draft\nstays here");
    s.enter();
    s.until("multiline queue edit saved", |s| {
        s.queue() == ["queued original\nsecond line\nthird line"]
    });
    s.send("\x1b[1;5B"); // Ctrl+Down returns to the draft
    s.paste(" plus");
    s.until("draft resumes unchanged", |s| {
        s.draft() == "main draft\nstays here plus"
    });
    s.release();
    s.until("edited queued prompt sent once", |s| {
        s.prompts() == ["[wait] first", "queued original\nsecond line\nthird line"]
    });
    assert_eq!(s.draft(), "main draft\nstays here plus");
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn framed_pastes_and_tab_stay_separate_in_a_single_terminal_write() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("[wait] first");
    s.until("turn started", |s| s.prompts().len() == 1);
    // No timing boundary exists between these messages: framing must carry the intent.
    s.send("\x1b[200~queued\tcode\r\nfirst\x1b[201~\t\x1b[200~draft\r\n/exit\r\nsecond\x1b[201~");
    s.until("framed paste and action identities preserved", |s| {
        s.queue() == ["queued    code\nfirst"] && s.draft() == "draft\n/exit\nsecond"
    });
    assert_eq!(s.prompts(), ["[wait] first"]);
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn explicit_queue_resume_preserves_the_recovered_draft() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("[wait] first");
    s.until("turn started", |s| s.prompts().len() == 1);
    s.paste("queued followup");
    s.queue_current_draft("queued followup");
    s.paste("separate draft");
    s.until("draft saved", |s| s.draft() == "separate draft");
    drop(s);
    let mut s = Session::launch(dir.path());
    s.until("queue restored paused", |s| {
        s.screen().contains("Ctrl+R resumes")
    });
    assert_eq!(s.prompts().len(), 1);
    s.send("\x12"); // Ctrl+R
    s.until("one resumed prompt", |s| s.prompts().len() == 2);
    s.until("followup complete", |s| {
        s.screen().contains("FAKE REPLY: queued followup")
    });
    assert_eq!(s.prompts(), ["[wait] first", "queued followup"]);
    assert_eq!(s.draft(), "separate draft");
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn abandoned_drafts_offer_a_choice_and_other_workspaces_are_isolated() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut first = Session::launch(dir.path());
    first.paste("first abandoned draft");
    first.until("first saved", |s| s.draft() == "first abandoned draft");
    let mut second = Session::launch(dir.path());
    second.paste("second abandoned draft");
    second.until("second saved", |s| s.draft() == "second abandoned draft");
    drop(first);
    drop(second);
    let other = dir.path().join("other-workspace");
    std::fs::create_dir(&other).unwrap();
    let mut separate = Session::launch_at(dir.path(), &other);
    separate.until("fresh workspace", |s| s.recovery_data().is_object());
    assert!(separate.draft().is_empty());
    assert!(!separate.screen().contains("abandoned draft"));
    separate.exit();
    let mut s = Session::launch(dir.path());
    s.until("recovery chooser", |s| {
        s.screen().contains("first abandoned draft")
            && s.screen().contains("second abandoned draft")
    });
    assert!(
        s.draft().is_empty(),
        "must not guess between abandoned panes"
    );
    s.enter();
    s.until("chosen draft restored", |s| {
        s.draft().ends_with("abandoned draft")
    });
    assert!(s.prompts().is_empty());
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn activity_detail_search_and_navigation_leave_the_draft_untouched() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("show activity");
    s.until("reply ready", |s| s.screen().contains("FAKE REPLY:"));
    s.paste("draft must survive");
    s.until("draft saved", |s| s.draft() == "draft must survive");
    s.send("\x05"); // Ctrl+E
    s.until("full activity detail", |s| {
        s.screen().contains("tool-output-end")
    });
    s.paste("tool-output");
    s.until("query visible", |s| s.screen().contains("tool-output"));
    s.enter();
    s.send("\x1b[6~"); // PageDown is owned by detail view
    s.send("\x1a"); // Undo must not reach underlying draft
    s.send("\x12"); // Resume must not reach queue
    s.send("\x1b");
    s.paste(" plus");
    s.until("draft retained", |s| s.draft() == "draft must survive plus");
    assert_eq!(s.prompts(), ["show activity"]);
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn crash_restores_the_unsaved_upper_editor_separately_and_holds_dispatch() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("[wait] first");
    s.until("turn started", |s| s.prompts().len() == 1);
    s.paste("saved queued item");
    s.queue_current_draft("saved queued item");
    s.paste("independent draft");
    s.until("independent draft persisted", |s| {
        s.draft() == "independent draft"
    });
    s.send("\x1b[1;5A");
    s.until("queue focus acknowledged", |s| {
        s.screen().contains("Ctrl+Down: draft; Ctrl+Up: queue")
    });
    s.enter_queue_editor();
    s.paste("\nunsaved upper edit");
    s.until("pending upper edit persisted", |s| {
        s.recovery_data()["pending_edit"]["text"] == "saved queued item\nunsaved upper edit"
    });
    assert_eq!(s.queue(), ["saved queued item"]);
    drop(s);
    let mut s = Session::launch(dir.path());
    s.until("upper edit restored", |s| {
        s.screen().contains("unsaved upper edit")
    });
    assert_eq!(s.draft(), "independent draft");
    assert_eq!(s.queue(), ["saved queued item"]);
    s.send("\x12");
    assert_eq!(
        s.prompts().len(),
        1,
        "resume cannot send an unresolved edit"
    );
    s.enter(); // explicitly commit recovered upper edit
    s.until("edit committed", |s| {
        s.queue() == ["saved queued item\nunsaved upper edit"]
    });
    s.send("\x12");
    s.until("recovered edited item sent", |s| {
        s.prompts() == ["[wait] first", "saved queued item\nunsaved upper edit"]
    });
    assert_eq!(s.draft(), "independent draft");
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn modal_crlf_and_overflow_paste_cannot_modify_a_draft_or_upper_edit() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("[wait] show activity");
    s.until("turn started", |s| s.prompts().len() == 1);
    s.paste("queued item");
    s.queue_current_draft("queued item");
    s.paste("underlying draft");
    s.until("underlying draft persisted", |s| {
        s.draft() == "underlying draft"
    });
    s.focus_queue();
    s.enter_queue_editor();
    s.paste(" plus upper edit");
    s.until("upper edit persisted", |s| {
        s.recovery_data()["pending_edit"]["text"] == "queued item plus upper edit"
    });
    s.send("\x05");
    s.until("detail opened", |s| s.screen().contains("Activity"));
    s.send("query\r\nsecond query\r\n");
    s.paste(&"overflow query ".repeat(160));
    s.send("\x15"); // Ctrl+U clears only the query
    s.send("\x1b");
    s.until("upper edit still visible", |s| {
        s.screen().contains("plus upper edit")
    });
    assert_eq!(s.draft(), "underlying draft");
    assert_eq!(s.queue(), ["queued item"]);
    assert_eq!(
        s.recovery_data()["pending_edit"]["text"],
        "queued item plus upper edit"
    );
    assert_eq!(s.prompts().len(), 1);
    s.send("\x1b"); // cancel upper edit explicitly
    s.until("upper edit explicitly cancelled", |s| {
        s.recovery_data()["pending_edit"].is_null()
    });
    s.send("\x1b[1;5B"); // return focus to the draft
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn explicit_file_open_passes_exact_path_to_a_fake_terminal_and_preserves_draft() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    std::fs::write(
        dir.path().join("workspace/hello world.rs"),
        "fn main() {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "[ui]\ntips=false\n[editor]\nprogram='synthetic-editor'\nargs=['+{line}','--','{file}']\n",
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.submit("/edit hello world.rs:7");
    s.until("file launcher called", |s| {
        s.root.join("file-launch.json").is_file()
    });
    let args: Vec<String> =
        serde_json::from_slice(&std::fs::read(s.root.join("file-launch.json")).unwrap()).unwrap();
    assert!(args.contains(&"synthetic-editor".into()));
    assert!(args.contains(&"+7".into()));
    assert!(
        args.iter().any(|arg| arg.ends_with("hello world.rs")),
        "{args:?}"
    );
    assert!(s.prompts().is_empty());
    s.paste("draft after file launch");
    s.until("chat remains usable", |s| {
        s.draft() == "draft after file launch"
    });
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn internal_file_panes_split_focus_close_and_never_dispatch_provider_work() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    std::fs::write(dir.path().join("workspace/first.txt"), "FIRST_PANE_CONTENT").unwrap();
    std::fs::write(
        dir.path().join("workspace/second.txt"),
        "SECOND_PANE_CONTENT",
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.submit("/open first.txt");
    s.until("internal first pane", |s| {
        s.screen().contains("FIRST_PANE_CONTENT")
    });
    s.paste("do not send this from a preview");
    s.send("\r");
    s.send("\x1b"); // return to chat
    s.submit("/open second.txt");
    s.until("two internal panes", |s| {
        let screen = s.screen();
        screen.contains("FIRST_PANE_CONTENT") && screen.contains("SECOND_PANE_CONTENT")
    });
    s.send("\x1b");
    s.submit("/close 1");
    s.until("first pane closed", |s| {
        let screen = s.screen();
        !screen.contains("FIRST_PANE_CONTENT") && screen.contains("SECOND_PANE_CONTENT")
    });
    assert!(!s.root.join("file-launch.json").exists());
    assert!(s.prompts().is_empty());
    s.paste("draft after internal preview");
    s.until("chat remains usable", |s| {
        s.draft() == "draft after internal preview"
    });
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn internal_preview_remains_responsive_during_gated_stream() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    std::fs::write(
        dir.path().join("workspace/stream-preview.txt"),
        "STREAM_PREVIEW_CONTENT",
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.submit("[wait] keep streaming");
    s.until("running command visible before preview", |s| {
        s.screen().contains("synthetic-command")
    });
    assert!(!dir.path().join("gate").exists());
    assert!(s.journal().iter().all(|event| event["kind"] != "done"));

    s.paste("/open stream-preview.txt");
    s.until("open command ready", |s| {
        s.draft() == "/open stream-preview.txt"
    });
    s.enter();
    s.until("preview opens during stream", |s| {
        s.screen().contains("STREAM_PREVIEW_CONTENT")
    });
    s.paste("preview input must not become a prompt");
    s.send("\r");
    s.send("\x1b");
    assert!(s.draft().is_empty());
    assert_eq!(s.prompts(), ["[wait] keep streaming"]);
    assert!(s.journal().iter().all(|event| event["kind"] != "done"));

    s.paste("draft while preview is open");
    s.until("chat draft remains responsive during stream", |s| {
        s.draft() == "draft while preview is open"
    });
    assert_eq!(s.prompts(), ["[wait] keep streaming"]);
    assert!(s.journal().iter().all(|event| event["kind"] != "done"));
    s.release();
    s.until("gated stream completes", |s| {
        s.screen().contains("FAKE REPLY: [wait] keep streaming")
    });
    assert_eq!(s.draft(), "draft while preview is open");
    assert!(s.screen().contains("STREAM_PREVIEW_CONTENT"));
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn picker_cancel_does_not_answer_armed_approval_during_gated_stream() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    set_workers(dir.path(), vec!["claude".into()]).unwrap();
    set_delegation_protocol(dir.path(), true).unwrap();

    let mut s = Session::launch(dir.path());
    s.submit("[wait] keep approval and picker separate");
    s.until("provider stream is gated", |s| {
        s.screen().contains("synthetic-command") && !s.journal().iter().any(|e| e["kind"] == "done")
    });

    // The MCP proposal file is the same state boundary used by the real MCP tool.  Creating it
    // here keeps the journey provider-free while exercising the TUI's polling and confirmation
    // path during the active provider turn.
    std::fs::create_dir_all(dir.path().join("proposals")).unwrap();
    std::fs::write(
        dir.path().join("proposals/approval.json"),
        r#"{"to":"claude","task":"review the gated synthetic change"}"#,
    )
    .unwrap();
    s.until("delegation approval is visible", |s| {
        s.screen().contains("delegation requested")
    });
    // workers::ARM_AFTER is 1500 ms; cross that boundary before opening the picker.
    std::thread::sleep(Duration::from_millis(1800));

    s.submit("/trust");
    s.until("permissions picker is open during the gated turn", |s| {
        s.screen().contains("permissions")
    });
    let prompts_before_picker = s.prompts();

    // y is a valid armed approval key, but the picker owns it as a filter character.  Esc then
    // closes only the picker; the armed confirmation remains pending for chat focus.
    s.send("y");
    std::thread::sleep(Duration::from_millis(150));
    assert!(s.screen().contains("permissions"));
    s.send("\x1b");
    s.until("permissions picker is dismissed", |s| {
        !s.screen().contains("filter: y")
    });
    s.paste("draft survives picker cancellation");
    s.until("chat draft returns after picker cancellation", |s| {
        s.draft() == "draft survives picker cancellation"
    });
    assert_eq!(s.prompts(), prompts_before_picker);
    assert!(s.screen().contains("delegation requested"));

    s.release();
    s.until("gated turn completes", |s| {
        s.screen()
            .contains("FAKE REPLY: [wait] keep approval and picker separate")
    });
    assert_eq!(s.draft(), "draft survives picker cancellation");
    assert_eq!(
        s.prompts(),
        vec!["[wait] keep approval and picker separate"]
    );
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn native_subagents_can_be_inspected_live_and_after_completion() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("[wait] [agents] inspect children");
    s.until("agent badge visible while provider waits", |s| {
        s.screen().contains("/agents 1")
    });
    s.send("\x07");
    s.until("agent browser", |s| s.screen().contains("Enter inspect"));
    s.enter();
    s.until("child task details", |s| {
        s.screen().contains("Inspect the graph parser")
    });
    s.send("\x1b");
    s.release();
    s.until("main reply", |s| s.screen().contains("FAKE REPLY:"));
    s.send("\x07");
    s.enter();
    s.until("reported child result retained", |s| {
        s.screen().contains("GRAPH_REVIEW_FINISHED")
    });
    assert_eq!(s.prompts().len(), 1);
    s.send("\x1b");
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn resuming_a_native_session_restores_recent_messages_without_dispatching_a_provider() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let sessions = dir.path().join("profile/.codex/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let mut lines = vec![
        serde_json::json!({"type":"session_meta","payload":{"id":"restore-session"}}).to_string(),
    ];
    for i in 0..60 {
        lines.push(serde_json::json!({"type":"response_item","payload":{"type":"message","role":if i%2==0 {"user"} else {"assistant"},"content":[{"type":"output_text","text":format!("RESTORED_HISTORY_{i}")}]}}).to_string());
    }
    std::fs::write(sessions.join("restore.jsonl"), lines.join("\n") + "\n").unwrap();
    let mut s = Session::launch(dir.path());
    s.submit("/resume");
    s.enter();
    s.until("last restored reply", |s| {
        s.screen().contains("RESTORED_HISTORY_59")
    });
    s.send("\x1b[5~");
    s.until("earlier messages after scroll", |s| {
        !s.screen().contains("RESTORED_HISTORY_59") && s.screen().contains("RESTORED_HISTORY_")
    });
    assert!(s.prompts().is_empty());
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; CI and release run it with --include-ignored"]
fn markdown_preview_opens_at_start_scrolls_and_exits_without_provider_work() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let path = dir.path().join("workspace/preview document.md");
    std::fs::write(
        &path,
        format!(
            "# PREVIEW START\n\n{}\nPREVIEW END",
            (0..90).map(|i| format!("- row {i}\n")).collect::<String>()
        ),
    )
    .unwrap();
    let path_text = path.to_string_lossy();
    let mut s = Session::launch_args(
        dir.path(),
        &dir.path().join("workspace"),
        &["preview", &path_text],
    );
    s.until("preview starts at first line", |s| {
        s.screen().contains("PREVIEW START")
    });
    assert!(!s.screen().contains("PREVIEW END"));
    s.send("\x1b[F");
    s.until("preview end", |s| s.screen().contains("PREVIEW END"));
    s.send("q");
    let start = Instant::now();
    while s.child.try_wait().unwrap().is_none() {
        assert!(start.elapsed() < Duration::from_secs(6));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(s.prompts().is_empty());
}
