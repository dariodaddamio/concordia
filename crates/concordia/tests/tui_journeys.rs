//! Drives the real TUI in a Windows pseudo-console with synthetic provider processes.
//! Every provider path, account home, workspace and state folder is isolated.
#![cfg(windows)]

use concordia::store::{
    save_store, OrchestratorStore, ProviderKind,
    StoredAccount, UseSelection,
};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_concordia");
const FAKE: &str = env!("CARGO_BIN_EXE_concordia-fake-provider");
static JOURNEY_LOCK: Mutex<()> = Mutex::new(());

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
    std::fs::write(
        profile.join(".cursor/concordia-api-key"),
        "synthetic-cursor-api-key",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("workspace")).unwrap();
    std::fs::write(
        root.join("config.toml"),
        "[threads]\nlogging = 'on'\n[ui]\ntips = false\n",
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
        accounts: [
            ("codex", ProviderKind::Codex),
            ("claude", ProviderKind::Claude),
            ("cursor", ProviderKind::Cursor),
        ]
        .into_iter()
        .map(|(id, provider)| StoredAccount {
            id: id.into(),
            provider,
            home_path: Some(
                profile
                    .join(format!(".{}", provider.as_str()))
                    .to_string_lossy()
                    .into_owned(),
            ),
            label: provider.as_str().into(),
            created_at: "0".into(),
        })
        .collect(),
        ..Default::default()
    };
    save_store(root, &store).unwrap();
    // Journeys script Cursor's print-mode fake; ACP journeys attach images or opt in explicitly.
    concordia::native_input::command(root, "cursor", "off").unwrap();
}

impl Session {
    fn launch(root: &Path) -> Self {
        Self::launch_at(root, &root.join("workspace"))
    }

    fn launch_at(root: &Path, workspace: &Path) -> Self {
        Self::launch_args(root, workspace, &[])
    }

    fn launch_args(root: &Path, workspace: &Path, args: &[&str]) -> Self {
        Self::launch_mode(root, workspace, args)
    }

    fn launch_mode(root: &Path, workspace: &Path, args: &[&str]) -> Self {
        let binary = std::env::var_os("CONCORDIA_JOURNEY_EXE")
            .unwrap_or_else(|| std::ffi::OsString::from(BIN));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let token = format!("journey-{}", uuid::Uuid::new_v4());
        let mut cmd = Command::new(binary);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.current_dir(workspace);
        if workspace.join("appserver-fixture.json").is_file() {
            cmd.env("FAKE_PROVIDER_NATIVE_CODEX", "1");
        }
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
        cmd.env("FAKE_FILE_LAUNCH_JOURNAL", root.join("file-launch.json"));
        cmd.env("CONCORDIA_CODEX_HOME_BASE", root.join("profile"));
        cmd.env("FAKE_PROVIDER_MODE", "interaction");
        cmd.env("FAKE_PROVIDER_JOURNAL", root.join("received.jsonl"));
        cmd.env("FAKE_PROVIDER_GATE", root.join("gate"));
        cmd.env("CONCORDIA_TEST_CONTROL", format!("{};{}", address, token));
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = cmd.spawn().unwrap();
        let started = Instant::now();
        let control = loop {
            match TcpStream::connect(address) {
                Ok(stream) => break stream,
                Err(error) if started.elapsed() < Duration::from_secs(5) => {
                    let _ = error;
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => panic!("native test control did not start: {error}"),
            }
        };
        let session = Self {
            child,
            control,
            token,
            root: root.into(),
        };
        session.until("startup", |s| s.screen().contains("concordia"));
        session
    }

    fn send(&mut self, text: &str) {
        if text.chars().count() > 1
            && !text.contains('\x1b')
            && (text.contains('\r') || text.contains('\n'))
        {
            self.request(serde_json::json!({"op":"text","text":text}));
            std::thread::sleep(Duration::from_millis(90));
            return;
        }
        let mut chars = text.chars().peekable();
        while let Some(character) = chars.next() {
            if character == '\x1b' {
                if chars.next_if_eq(&'[').is_some() {
                    let mut sequence = String::new();
                    for next in chars.by_ref() {
                        sequence.push(next);
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                    self.send_escape(&sequence);
                } else if let Some(next) = chars.next() {
                    self.request(serde_json::json!({
                        "op":"key",
                        "code":next.to_string(),
                        "modifiers":["ALT"]
                    }));
                } else {
                    self.request(serde_json::json!({"op":"key","code":"Esc"}));
                }
            } else if character == '\r' || character == '\n' {
                self.request(serde_json::json!({"op":"key","code":"Enter"}));
            } else if character == '\t' {
                self.request(serde_json::json!({"op":"key","code":"Tab"}));
            } else if character == '\u{8}' {
                self.request(serde_json::json!({
                    "op":"key","code":"Backspace","modifiers":["CTRL"]
                }));
            } else if character == '\u{7f}' {
                self.request(serde_json::json!({"op":"key","code":"Backspace"}));
            } else if character.is_control() {
                let code = ((character as u32) + b'a' as u32 - 1) as u8 as char;
                self.request(
                    serde_json::json!({"op":"key","code":code.to_string(),"modifiers":["CTRL"]}),
                );
            } else {
                self.request(serde_json::json!({
                    "op":"key",
                    "code":character.to_string()
                }));
            }
        }
        std::thread::sleep(Duration::from_millis(90));
    }

    fn send_escape(&mut self, sequence: &str) {
        if let Some(mouse) = sequence.strip_prefix('<') {
            let kind = mouse.chars().last().unwrap_or_default();
            let fields: Vec<_> = mouse[..mouse.len().saturating_sub(kind.len_utf8())]
                .split(';')
                .collect();
            if fields.len() == 3 {
                if let (Ok(button), Ok(x), Ok(y)) = (
                    fields[0].parse::<u16>(),
                    fields[1].parse::<u16>(),
                    fields[2].parse::<u16>(),
                ) {
                    let kind = match (button, kind) {
                        (0, 'M') => "down",
                        (0, 'm') => "up",
                        (64, 'M') => "scroll_up",
                        (65, 'M') => "scroll_down",
                        _ => return,
                    };
                    self.request(serde_json::json!({
                        "op":"mouse",
                        "kind":kind,
                        "x":x.saturating_sub(1),
                        "y":y.saturating_sub(1)
                    }));
                }
            }
            return;
        }
        let (code, modifiers) = match sequence {
            "A" => ("Up", Vec::new()),
            "B" => ("Down", Vec::new()),
            "C" => ("Right", Vec::new()),
            "D" => ("Left", Vec::new()),
            "H" => ("Home", Vec::new()),
            "F" => ("End", Vec::new()),
            "1;2A" => ("Up", vec!["SHIFT"]),
            "1;2B" => ("Down", vec!["SHIFT"]),
            "1;2C" => ("Right", vec!["SHIFT"]),
            "1;2D" => ("Left", vec!["SHIFT"]),
            "1;3A" => ("Up", vec!["ALT"]),
            "1;3B" => ("Down", vec!["ALT"]),
            "1;3C" => ("Right", vec!["ALT"]),
            "1;3D" => ("Left", vec!["ALT"]),
            "1;5A" => ("Up", vec!["CTRL"]),
            "1;5B" => ("Down", vec!["CTRL"]),
            "1;5C" => ("Right", vec!["CTRL"]),
            "1;5D" => ("Left", vec!["CTRL"]),
            "5~" => ("PageUp", Vec::new()),
            "6~" => ("PageDown", Vec::new()),
            "3~" => ("Delete", Vec::new()),
            "14~" => ("F4", Vec::new()),
            "17~" => ("F6", Vec::new()),
            "18~" => ("F7", Vec::new()),
            "19~" => ("F8", Vec::new()),
            "20~" => ("F9", Vec::new()),
            "21~" => ("F10", Vec::new()),
            _ => return,
        };
        self.request(serde_json::json!({"op":"key","code":code,"modifiers":modifiers}));
    }

    fn request(&self, mut value: Value) -> Value {
        value["token"] = Value::String(self.token.clone());
        let line = format!("{}\n", value);
        let mut stream = self.control.try_clone().unwrap();
        stream.write_all(line.as_bytes()).unwrap();
        stream.flush().unwrap();
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
        serde_json::from_str(response.trim()).unwrap()
    }

    fn paste(&mut self, text: &str) {
        self.request(serde_json::json!({"op":"text","text":text}));
        std::thread::sleep(Duration::from_millis(90));
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
            .map(|e| {
                concordia::prompt_time::strip_header(e["prompt"].as_str().unwrap()).to_string()
            })
            .collect()
    }

    fn recovery_data(&self) -> Value {
        let pid = self.child.id();
        let mut records = Vec::new();
        if let Ok(workspaces) = std::fs::read_dir(self.root.join("pane-recovery")) {
            for workspace in workspaces.flatten() {
                if let Ok(panes) = std::fs::read_dir(workspace.path()) {
                    for pane in panes.flatten() {
                        if let Ok(bytes) = std::fs::read(pane.path().join("recovery.json")) {
                            if let Ok(record) = serde_json::from_slice::<Value>(&bytes) {
                                if record["owner_pid"].as_u64() == Some(u64::from(pid)) {
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
        let response = self.clone_request(serde_json::json!({"op":"snapshot"}));
        response["grid"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| format!("{}\n", row.as_str().unwrap()))
            .collect()
    }

    fn status_dot(&self) -> String {
        self.clone_request(serde_json::json!({"op":"snapshot"}))["status_dot"]
            .as_str()
            .unwrap_or_default()
            .into()
    }

    fn clone_request(&self, mut value: Value) -> Value {
        let mut stream = self.control.try_clone().unwrap();
        value["token"] = Value::String(self.token.clone());
        writeln!(stream, "{value}").unwrap();
        let mut response = String::new();
        BufReader::new(stream).read_line(&mut response).unwrap();
        serde_json::from_str(response.trim()).unwrap()
    }

    fn until(&self, label: &str, check: impl Fn(&Self) -> bool) {
        self.until_with_timeout(label, Duration::from_secs(12), check);
    }

    fn until_with_timeout(&self, label: &str, timeout: Duration, check: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if check(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!(
            "timed out: {label}\nscreen:\n{}\nprompts: {:?}\nqueue: {:?}\ndraft: {:?}",
            format_args!("{}", self.screen()),
            self.prompts(),
            self.queue(),
            self.draft()
        );
    }

    fn release(&self) {
        std::fs::write(self.root.join("gate"), "continue").unwrap();
    }

    fn exit(&mut self) {
        self.send("\x01"); // clear the draft before native cancel/quit keys
        self.send("\x08");
        let start = Instant::now();
        loop {
            if self.child.try_wait().unwrap().is_some() {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(8),
                "TUI would not exit; draft={:?}, screen={}",
                self.draft(),
                self.screen()
            );
            // A busy turn consumes Ctrl+C as cancellation before idle quit can be armed.
            self.send("\x03");
            std::thread::sleep(Duration::from_millis(150));
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let pid = self.child.id();
            if pid != 0 {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn needs_attention_question_journey_uses_the_codex_app_server_fixture() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    concordia::native_input::command(dir.path(), "codex", "on").unwrap();
    std::fs::write(
        dir.path().join("workspace/appserver-fixture.json"),
        serde_json::json!({
            "question": true,
            "question_then_approval": true,
            "finish_wait_for": "../gate"
        })
        .to_string(),
    )
    .unwrap();

    let mut s = Session::launch(dir.path());
    s.request(serde_json::json!({"op":"resize","columns":180,"rows":30}));
    s.submit("Inspect the synthetic app-server request");
    s.until("native question needs attention", |s| {
        s.status_dot() == "needs_attention"
    });

    s.send("\x0e");
    s.until("needs attention panel lists the question details", |s| {
        let screen = s.screen();
        screen.contains("needs you")
            && screen.contains("workspace")
            && screen.contains("Inspect the synthetic app-server request")
            && screen.contains("codex")
            && screen.contains("question")
            && screen.contains("waiting")
    });
    s.enter();
    s.request(serde_json::json!({"op":"key","code":"1"}));
    s.request(serde_json::json!({"op":"key","code":"Enter"}));
    std::thread::sleep(Duration::from_millis(120));
    s.request(serde_json::json!({"op":"key","code":"Esc"}));
    std::thread::sleep(Duration::from_millis(120));
    s.request(serde_json::json!({"op":"key","code":"F8"}));
    std::thread::sleep(Duration::from_millis(120));
    s.request(serde_json::json!({"op":"key","code":"1"}));
    s.request(serde_json::json!({"op":"key","code":"Enter"}));
    s.until("question answer is working", |s| s.status_dot() == "working");
    s.release();
    s.until("question answer resolves in the chat", |s| {
        s.screen().contains("Native approval handled")
    });
    s.until("app-server turn is idle", |s| s.status_dot() == "idle");
    s.exit();
}


#[test]
#[ignore = "isolated slash-command and shared skill lifecycle; no provider turns"]
fn shared_setup_commands_are_discoverable_and_work_through_the_terminal_ui() {
    let _guard = JOURNEY_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let source = dir.path().join("workspace/shared-skills");
    std::fs::create_dir_all(source.join("ui-review-fixture")).unwrap();
    std::fs::write(source.join("ui-review-fixture/SKILL.md"), "---\nname: ui-review-fixture\ndescription: Synthetic command journey\n---\nInspect only.\n").unwrap();
    let mut session = Session::launch(dir.path());
    session.send("/sk");
    session.until("skills root completion", |session| {
        session.screen().contains("/skills")
    });
    session.send("\x1b");
    session.until("completion dismissed", |session| {
        !session
            .screen()
            .contains("inspect skills and connect shared setup")
    });
    session.send("\x01\x7f");
    session.until("completion draft cleared", |session| {
        session.draft().is_empty()
    });
    session.submit(&format!("/skills install \"{}\"", source.display()));
    session.until_with_timeout(
        "shared source installed and recorded",
        Duration::from_secs(30),
        |_| {
            concordia::harness::connections::shared_skill_sources(dir.path())
                .is_ok_and(|sources| sources.len() == 1)
        },
    );
    for provider in [".codex", ".claude", ".cursor"] {
        assert!(
            dir.path()
                .join("profile")
                .join(provider)
                .join("skills/ui-review-fixture/SKILL.md")
                .is_file(),
            "{provider}"
        );
    }
    session.submit("/skills list");
    session.until("installed linked skill visible in catalog", |session| {
        session.screen().contains("ui-review-fixture")
    });
    session.send("\x1b");
    session.submit("/hooks list");
    session.until("native hook inventory and trust boundary", |session| {
        session.screen().contains("Native hook trust")
    });
    session.send("\x1b");
    session.submit("/skills");
    session.until("shared source status", |session| {
        session.screen().contains("Skills ·")
    });
    session.send("\x1b");
    session.submit("/hooks list extra");
    session.until("invalid command arguments rejected", |session| {
        session.screen().contains("unsupported arguments")
    });
    session.submit(&format!("/skills remove \"{}\"", source.display()));
    session.until_with_timeout(
        "source unshared and links removed",
        Duration::from_secs(30),
        |_| {
            concordia::harness::connections::shared_skill_sources(dir.path())
                .is_ok_and(|sources| sources.is_empty())
                && !dir
                    .path()
                    .join("profile/.codex/skills/ui-review-fixture")
                    .exists()
        },
    );
    session.until("removal result published", |session| {
        session.screen().contains("stopped sharing skill source")
    });
    for provider in [".codex", ".claude", ".cursor"] {
        assert!(
            !dir.path()
                .join("profile")
                .join(provider)
                .join("skills/ui-review-fixture")
                .exists(),
            "{provider}"
        );
    }
    assert!(source.join("ui-review-fixture/SKILL.md").is_file());
    assert!(
        session.journal().is_empty(),
        "setup must not start a model turn"
    );
    let artifact =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/09g-command-journey.txt");
    std::fs::write(artifact, session.screen()).unwrap();
    session.exit();
}

#[test]
#[ignore = "isolated Mermaid, native images and Cursor native permission journey; fake providers only"]
fn mermaid_and_all_provider_image_inputs_work_in_the_installed_ui() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for provider in [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Cursor,
    ] {
        let dir = tempfile::tempdir().unwrap();
        setup(dir.path(), provider);
        let work = dir.path().join("workspace");
        let image = work.join("diagram.png");
        image::RgbaImage::from_pixel(12, 8, image::Rgba([200, 40, 90, 255]))
            .save(&image)
            .unwrap();
        std::fs::write(
            work.join("acp-fixture.json"),
            "{\"image\":true,\"permission\":true}",
        )
        .unwrap();
        let mut s = Session::launch(dir.path());
        if provider == ProviderKind::Codex {
            s.submit("[mermaid]");
            s.until("Mermaid boxes rendered in chat", |s| {
                let screen = s.screen();
                screen.contains("Prompt")
                    && screen.contains("Provider")
                    && screen.contains('\u{2500}')
                    && !screen.contains("flowchart LR")
            });
        }
        let dropped = format!("\"{}\"", image.display());
        s.paste(&dropped);
        s.until("attached image chip", |s| {
            s.screen().contains("Image 1: diagram.png")
        });
        let preview_draft = s.draft();
        s.until("automatic image preview without a click", |s| {
            let screen = s.screen();
            screen.contains("Image 1: diagram.png") && screen.contains('▀')
        });
        assert!(s.screen().contains("Credits: not reported"));
        let screen = s.screen();
        let (y, row) = screen
            .lines()
            .enumerate()
            .find(|(_, row)| row.contains("Image 1: diagram.png"))
            .unwrap();
        let x = row.find("Image 1: diagram.png").unwrap();
        s.send(&format!(
            "\x1b[<0;{};{}M\x1b[<0;{};{}m",
            x + 1,
            y + 1,
            x + 1,
            y + 1
        ));
        s.until("inline raster snapshot", |s| {
            s.screen().contains("cell preview") && s.screen().contains("12 × 8")
        });
        s.paste("preview must preserve draft");
        s.send("\x1b");
        s.until("raster close preserves composer", |s| {
            !s.screen().contains("cell preview") && s.draft() == preview_draft
        });
        s.send("\x01\x7f");
        s.paste("Inspect image pixels");
        s.enter();
        if provider == ProviderKind::Cursor {
            s.until("native permission options", |s| {
                s.screen().contains("Allow once")
            });
            let screen = s.screen();
            let (y, line) = screen
                .lines()
                .enumerate()
                .find(|(_, l)| l.contains("1. Allow once"))
                .unwrap();
            let x = line.find("1. Allow once").unwrap();
            s.send(&format!(
                "\x1b[<0;{};{}M\x1b[<0;{};{}m",
                x + 1,
                y + 1,
                x + 1,
                y + 1
            ));
            s.until("native image turn completed", |s| {
                s.screen().contains("Native image received") && s.screen().contains("Worked for")
            });
            let calls = std::fs::read_to_string(work.join("acp-calls.jsonl")).unwrap();
            assert_eq!(
                calls
                    .lines()
                    .filter(|l| l.contains("\"method\":\"session/prompt\""))
                    .count(),
                1
            );
        } else {
            s.until("image reply completed", |s| {
                s.screen().contains("FAKE REPLY: Inspect image pixels")
                    && s.screen().contains("Worked for")
            });
            assert!(s
                .journal()
                .iter()
                .any(|e| e["kind"] == "start" && e["image_count"] == 1));
        }
        assert!(!s.screen().contains("concordia-message-v1"));
        let document = work.join("notes.md");
        std::fs::write(
            &document,
            "DOCUMENT_SNAPSHOT_FIRST\r\nExact document bytes\r\n",
        )
        .unwrap();
        let before = s.prompts().len();
        s.submit("/attach notes.md");
        s.until("document attached with exact preview", |s| {
            s.screen().contains("Document 1: notes.md")
                && s.screen().contains("DOCUMENT_SNAPSHOT_FIRST")
        });
        std::fs::write(&document, "CHANGED_AFTER_IMPORT\r\n").unwrap();
        s.send("\x1b");
        s.paste("Review document bytes");
        s.enter();
        s.until("document snapshot dispatched", |s| {
            s.prompts().len() == before + 1 && s.screen().contains("Worked for")
        });
        let prompt = s.prompts().last().unwrap().clone();
        assert!(
            prompt.contains("DOCUMENT_SNAPSHOT_FIRST") && !prompt.contains("CHANGED_AFTER_IMPORT")
        );
        assert!(!s.screen().contains("concordia-message-v2"));
        let pdf = work.join("fixture.pdf");
        std::fs::write(&pdf, b"%PDF-1.7\nsynthetic TUI PDF\n%%EOF\n").unwrap();
        let pdf_before = s.prompts().len();
        s.submit("/attach fixture.pdf");
        s.until("PDF descriptor chip visible", |s| {
            s.screen().contains("PDF 1: fixture.pdf") && s.screen().contains("PDF document")
        });
        if provider != ProviderKind::Claude {
            s.paste("Review this PDF");
            s.enter();
            s.until("unsupported PDF send retains draft", |s| {
                s.screen().contains("Native PDF attachments require Claude")
            });
            assert_eq!(s.prompts().len(), pdf_before);
            assert_eq!(
                concordia::attachments::Message::decode(&s.draft())
                    .attachments
                    .len(),
                1
            );
        }
        if provider != ProviderKind::Claude {
            s.send("\x01\x08");
        }
        s.paste("/attach clear");
        s.until("PDF clear command ready", |s| {
            concordia::attachments::Message::decode(&s.draft()).text == "/attach clear"
        });
        s.enter();
        s.until("PDF draft cleared", |s| s.draft().is_empty());
        let blobs = std::fs::read_dir(dir.path().join("attachments"))
            .unwrap()
            .count();
        let pasted = format!("\"{}\"", document.display());
        s.paste(&pasted);
        s.until("pasted paths stay text until explicit attach", |s| {
            s.draft() == pasted && s.screen().contains("pasted file(s)")
        });
        assert_eq!(
            std::fs::read_dir(dir.path().join("attachments"))
                .unwrap()
                .count(),
            blobs
        );
        s.send("\x1b[20~");
        s.until("explicit pasted-file attachment", |s| {
            s.screen().contains("Document 1: notes.md")
                && concordia::attachments::Message::decode(&s.draft()).text == pasted
        });
        assert_eq!(s.prompts().len(), before + 1);
        let screen = s.screen();
        let (y, row) = screen
            .lines()
            .enumerate()
            .find(|(_, row)| row.contains("Document 1: notes.md"))
            .unwrap();
        let at = row.rfind('X').unwrap();
        let x = row[..at].chars().count();
        s.send(&format!(
            "\x1b[<0;{};{}M\x1b[<0;{};{}m",
            x + 1,
            y + 1,
            x + 1,
            y + 1
        ));
        s.until("remove attachment preserves pasted text", |s| {
            !s.screen().contains("Document 1: notes.md") && s.draft() == pasted
        });
        s.exit();
    }
}

#[test]
#[ignore = "isolated conversational focus and exact free-reply journey; fake providers only"]
fn question_choices_do_not_swallow_an_unattended_work_instruction() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for provider in [ProviderKind::Codex, ProviderKind::Claude] {
        let dir = tempfile::tempdir().unwrap();
        setup(dir.path(), provider);
        let mut s = Session::launch(dir.path());
        s.submit("[choices]");
        s.until("completed reply with optional choices", |s| {
            let screen = s.screen();
            screen.contains("2. Full refactor") && screen.contains("Worked for")
        });
        // Slow individual keys must edit the composer, including an option-shaped first digit.
        s.send("2");
        s.until("digit is a draft, not an answer", |s| s.draft() == "2");
        assert_eq!(s.prompts().len(), 1);
        s.paste(" tasks:\n  commit and push for review\n  test while I sleep");
        let instruction = "2 tasks:\n  commit and push for review\n  test while I sleep";
        s.until("complete work instruction preserved", |s| {
            s.draft() == instruction
        });
        s.enter();
        s.until("exact instruction dispatched", |s| s.prompts().len() == 2);
        assert_eq!(s.prompts()[1], instruction);
        s.until("free reply completes", |s| {
            s.screen().contains("Worked for")
                && s.journal().iter().filter(|e| e["kind"] == "done").count() == 2
        });
        s.exit();
    }
}

#[test]
#[ignore = "isolated selectable question reply journey; fake provider only"]
fn question_choices_select_a_reply_without_losing_the_unsent_draft() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("[choices]");
    s.until("selectable provider options", |s| {
        s.screen().contains("2. Full refactor")
    });
    s.paste("keep my unsent draft");
    s.until("draft is safe", |s| s.draft() == "keep my unsent draft");
    s.send("\x1b[19~\x1b[B\r");
    s.until("answer dispatched once", |s| s.prompts().len() == 2);
    assert!(s.prompts()[1].contains("Full refactor"));
    s.until("answer reply completes", |s| {
        s.screen().contains("FAKE REPLY:") && s.screen().contains("Worked for")
    });
    assert_eq!(s.draft(), "keep my unsent draft");
    assert!(!s.screen().contains("[ Bottom ]"));
    s.exit();
}

#[test]
#[ignore = "isolated workspace completion and explicit queue send journey; fake provider only"]
fn workspace_files_complete_without_mentions_and_send_now_interrupts_then_sends_once() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    std::fs::create_dir(dir.path().join("workspace/src")).unwrap();
    std::fs::write(dir.path().join("workspace/Cargo.toml"), "[workspace]").unwrap();
    std::fs::write(
        dir.path().join("workspace/src/main.c"),
        "int WORKSPACE_CODE_MARKER = 1;\n",
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.paste("/open src/mai");
    s.until("workspace suggestion without a chat mention", |s| {
        s.screen().contains("src/main.c")
    });
    s.enter();
    s.until("file opened", |s| {
        s.screen().contains("WORKSPACE_CODE_MARKER")
    });
    assert!(s.prompts().is_empty());
    s.send("\x1b");
    s.submit("/close 1");
    s.until("file closed", |s| {
        !s.screen().contains("WORKSPACE_CODE_MARKER")
    });
    s.submit("[wait] active turn");
    s.until("provider is gated", |s| {
        s.screen()
            .lines()
            .any(|row| row.contains("Run command") && row.contains("Expand"))
            && s.journal().iter().all(|e| e["kind"] != "done")
    });
    for text in ["urgent queued message", "later queued message"] {
        s.paste(text);
        s.until("queue draft ready", |s| s.draft() == text);
        s.send("\t");
        s.until("message queued", |s| s.queue().iter().any(|q| q == text));
    }
    s.paste("draft stays here");
    s.until("new draft preserved", |s| s.draft() == "draft stays here");
    let screen = s.screen();
    let (y, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("queued 1:") && line.contains("[ Send now ]"))
        .unwrap();
    let x = line.find("[ Send now ]").unwrap();
    s.send(&format!(
        "\x1b[<0;{};{}M\x1b[<0;{};{}m",
        x + 1,
        y + 1,
        x + 1,
        y + 1
    ));
    s.until("selected queue entry sent after stop", |s| {
        s.screen().contains("FAKE REPLY: urgent queued message")
    });
    assert_eq!(s.prompts(), ["[wait] active turn", "urgent queued message"]);
    assert_eq!(s.queue(), ["later queued message"]);
    assert_eq!(s.draft(), "draft stays here");
    s.exit();
}



#[test]
#[ignore = "30-second isolated idle/input soak; no provider turns"]
fn idle_focus_and_caret_soak_preserves_draft_and_has_bounded_memory_growth() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.paste("persistent idle draft");
    s.until("draft persisted", |s| s.draft() == "persistent idle draft");
    let pid = s.child.id();
    let mut sampler = concordia::sysstats::Sampler::new();
    let first = sampler.sample(&[pid]);
    let mut peak_mem = first.agents_mem;
    let mut peak_cpu = 0.0f64;
    for i in 0..60 {
        std::thread::sleep(Duration::from_millis(500));
        s.send("\x1b[D\x1b[C");
        if i % 10 == 0 {
            s.send("\x1b[O\x1b[I");
        }
        let stats = sampler.sample(&[pid]);
        peak_mem = peak_mem.max(stats.agents_mem);
        peak_cpu = peak_cpu.max(stats.agents_cpu);
        assert!(s.prompts().is_empty());
    }
    eprintln!(
        "IDLE_SOAK start_bytes={} peak_bytes={} peak_cpu_machine_pct={peak_cpu:.3}",
        first.agents_mem, peak_mem
    );
    assert!(peak_mem.saturating_sub(first.agents_mem) < 64 * 1024 * 1024);
    assert!(
        peak_cpu < 20.0,
        "idle process tree unexpectedly busy: {peak_cpu}"
    );
    s.paste(" after idle");
    s.until("still responsive", |s| {
        s.draft() == "persistent idle draft after idle"
    });
    drop(s);
    let mut s = Session::launch(dir.path());
    s.until("recovery after idle", |s| {
        s.draft() == "persistent idle draft after idle"
    });
    assert!(s.prompts().is_empty());
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; uses isolated state and no real providers"]
fn fresh_drafts_and_bottom_button_work_in_the_windows_terminal() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit(&format!("/synthetic {}", "scroll fixture ".repeat(400)));
    s.until("long transcript visible", |s| {
        s.screen().contains("fixture scroll fixture")
    });
    s.paste("unfinished primary");
    s.until("primary draft saved", |s| s.draft() == "unfinished primary");
    s.send("\x1b[B");
    s.until("fresh prompt", |s| {
        !s.screen().contains("unfinished primary")
    });
    s.paste("secondary draft");
    s.until("both drafts saved", |s| {
        s.draft() == "secondary draft"
            && s.recovery_data()["alternate_draft"] == "unfinished primary"
    });
    s.send("\x1b[A");
    s.until("primary restored", |s| s.draft() == "unfinished primary");
    s.send("\x1b[5~");
    s.until("transcript scrolled", |s| s.screen().contains("rows up"));
    let screen = s.screen();
    let (x, y) = screen
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            line.find("[ Bottom ]")
                .map(|offset| (line[..offset].chars().count() + 4, y))
        })
        .expect("centered bottom button");
    s.send(&format!(
        "\x1b[<0;{};{}M\x1b[<0;{};{}m",
        x + 1,
        y + 1,
        x + 1,
        y + 1
    ));
    s.until("following latest output", |s| {
        !s.screen().contains("rows up")
    });
    assert_eq!(s.draft(), "unfinished primary");
    assert_eq!(s.prompts().len(), 1);
    drop(s);
    let mut s = Session::launch(dir.path());
    s.until("active draft restored after restart", |s| {
        s.draft() == "unfinished primary"
    });
    s.send("\x1b[B");
    s.until("alternate draft restored after restart", |s| {
        s.draft() == "secondary draft"
    });
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
fn terminal_word_deletion_usage_and_focus_repaint() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let usage = dir.path().join("profile/.codex/tmp");
    std::fs::create_dir_all(&usage).unwrap();
    // Use a native rate-limit fixture, never a live provider turn.
    std::fs::write(
        usage.join("rate_limits.json"),
        serde_json::json!({
            "primary": {"used_percent": 23, "window_minutes": 300},
            "secondary": {"used_percent": 61, "window_minutes": 10080}
        })
        .to_string(),
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.paste("alpha beta");
    s.until("draft ready", |s| s.draft() == "alpha beta");
    s.send("\x08"); // Windows Terminal Ctrl+Backspace sends BS; Backspace sends DEL.
    s.until("Ctrl+Backspace deletes a word", |s| s.draft() == "alpha ");
    s.paste("gamma");
    s.send("\x08"); // The native control channel's supported word-deletion key.
    s.until("Ctrl+W encoding deletes a word", |s| s.draft() == "alpha ");
    s.send("\x7f");
    s.until("Backspace deletes one character", |s| s.draft() == "alpha");
    s.send("\x01");
    s.send("\x7f");
    s.submit("/use");
    s.until("account picker", |s| s.screen().contains("77% left"));
    s.send("\x1b");
    // Focus transitions repaint from the retained native cell buffer.
    s.send("\x1b[O\x1b[I");
    s.until("focus repaints the full screen", |s| {
        s.screen().contains("concordia")
    });
    assert!(
        s.prompts().is_empty(),
        "UI commands must not spend provider turns"
    );
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
fn usage_limit_automatically_continues_on_configured_provider_without_foreign_session() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    std::fs::write(dir.path().join("config.toml"), "[threads]\nlogging='on'\n[ui]\ntips=false\n[continuation]\nmode='automatic'\naccounts=['claude']\n").unwrap();
    let mut s = Session::launch(dir.path());
    s.submit("Fix graph [quota] [coded-limit]");
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
    assert!(prompt.contains("Original request:\nFix graph [quota] [coded-limit]"));
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
    child: Child,
    control: TcpStream,
    token: String,
    root: PathBuf,
}

#[test]
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
            s.screen()
                .lines()
                .any(|row| row.contains("Run command") && row.contains("Expand"))
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
        s.until("cancel and pause", |s| {
            s.screen().contains("queue paused") && s.screen().contains("Cancelled.")
        });
        let cancelled = s.screen();
        assert!(
            cancelled.contains("| !") && cancelled.contains("Cancelled."),
            "native cancellation snapshot:\n{cancelled}"
        );
        assert!(!cancelled.contains("cancelled - queued work paused"));
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
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
        s.prompts() == ["alpha\n\nbeta\n"]
    });
    s.until("reply", |s| s.screen().contains("FAKE REPLY:"));
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
    s.request(serde_json::json!({"op":"resize","columns":60,"rows":18}));
    s.paste(" after resize");
    s.until("resized draft intact", |s| {
        s.draft() == format!("{draft} after resize")
    });
    assert!(s.prompts().is_empty());
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
        s.draft().is_empty() && s.screen().contains("Cancelled")
    });
    assert_eq!(s.prompts(), ["[wait] original prompt"]);
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
fn framed_pastes_and_tab_stay_separate_in_a_single_terminal_write() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("[wait] first");
    s.until("turn started", |s| s.prompts().len() == 1);
    // No timing boundary exists between these messages: framing must carry the intent.
    s.request(serde_json::json!({"op":"text","text":"queued\tcode\r\nfirst"}));
    s.request(serde_json::json!({"op":"key","code":"Tab"}));
    s.request(serde_json::json!({"op":"text","text":"draft\r\n/exit\r\nsecond"}));
    s.until("framed paste and action identities preserved", |s| {
        s.queue() == ["queued    code\nfirst"] && s.draft() == "draft\n/exit\nsecond"
    });
    assert_eq!(s.prompts(), ["[wait] first"]);
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
    s.until("dirty file close requires a decision", |s| {
        s.screen().contains("Unsaved")
    });
    s.send("d");
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
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
        s.screen()
            .lines()
            .any(|row| row.contains("Run command") && row.contains("Expand"))
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
    s.until(
        "preview editing leaves the persisted chat draft empty",
        |s| s.draft().is_empty(),
    );
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
    s.send("\x1b[17~"); // F6 focuses the file, then explicitly discard the fixture edit.
    s.send("\x1b");
    s.send("\x01\x08");
    s.submit("/close 1");
    s.until("unsaved streaming edit retained", |s| {
        s.screen().contains("Unsaved")
    });
    s.send("d");
    s.until("streaming file closes", |s| {
        !s.screen().contains("STREAM_PREVIEW_CONTENT")
    });
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
fn native_subagents_can_be_inspected_live_and_after_completion() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("[wait] [agents] inspect children");
    s.until("agent badge visible while provider waits", |s| {
        s.screen().contains("Ctrl+G agents 1")
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
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
#[ignore = "isolated sticky tool-header journey with fake providers"]
fn pinned_expanded_tool_header_collapses_through_real_mouse_input() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("inspect [long-tool]");
    s.until("reply completed", |s| s.screen().contains("FAKE REPLY:"));
    s.send("\x0f");
    s.until("sticky expanded header", |s| {
        s.screen()
            .lines()
            .any(|line| line.contains("Ran command") && line.contains("Collapse"))
    });
    let y = s
        .screen()
        .lines()
        .position(|line| line.contains("Ran command") && line.contains("Collapse"))
        .unwrap()
        + 1;
    s.send(&format!("\x1b[<0;8;{y}M\x1b[<0;8;{y}m"));
    s.until("pinned header collapsed its card", |s| {
        s.screen()
            .lines()
            .any(|line| line.contains("Ran command") && line.contains("Expand"))
            && !s.screen().contains("synthetic output line")
    });
    assert_eq!(s.prompts(), ["inspect [long-tool]"]);
    assert!(s.draft().is_empty());
    s.exit();
}

#[test]
#[ignore = "isolated account-profile and slash-argument journey; no model turns"]
fn usage_profiles_and_account_model_completion_work_in_the_real_terminal() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let home = dir.path().join("profile/.codex");
    std::fs::write(home.join("auth.json"), serde_json::json!({"tokens":{"id_token":"e30.eyJlbWFpbCI6ICJtZW1iZXJAZXhhbXBsZS5pbnZhbGlkIiwgImh0dHBzOi8vYXBpLm9wZW5haS5jb20vYXV0aCI6IHsiY2hhdGdwdF9wbGFuX3R5cGUiOiAicGx1cyJ9fQ.fixture"}}).to_string()).unwrap();
    std::fs::write(
        home.join("models_cache.json"),
        r#"{"models":[{"slug":"fixture-model","display_name":"fixture-model","visibility":"list"}]}"#,
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.until("header email is private by default", |s| {
        let screen = s.screen();
        let header = screen.lines().next().unwrap_or_default();
        header.contains("view email") && !header.contains("member@example.invalid")
    });
    let x = s.screen().lines().next().unwrap().find("email").unwrap() + 1;
    s.send(&format!("\x1b[<0;{x};1M\x1b[<0;{x};1m"));
    s.until("click reveals header email", |s| {
        s.screen()
            .lines()
            .next()
            .unwrap_or_default()
            .contains("member@example.invalid")
    });
    s.send(&format!("\x1b[<0;{x};1M\x1b[<0;{x};1m"));
    s.until("second click hides header email", |s| {
        !s.screen()
            .lines()
            .next()
            .unwrap_or_default()
            .contains("member@example.invalid")
    });
    s.paste("/use");
    s.until("complete command reveals actions", |s| {
        s.screen().contains("/use options") && s.screen().contains("/use add")
    });
    s.send("\x1b[B\t");
    s.until("subcommand completes without running login", |s| {
        s.draft() == "/use add"
    });
    s.send("\x01\x7f");
    s.until("command draft cleared", |s| s.draft().is_empty());
    s.paste("/model fix");
    s.send("\t");
    s.until("model argument completed", |s| {
        s.draft() == "/model fixture-model"
    });
    s.enter();
    s.until("model choice applied", |s| {
        s.screen().contains("fixture-model") && s.draft().is_empty()
    });
    s.submit("/usage");
    s.until("account metadata shown", |s| {
        s.screen().lines().any(|line| {
            line.contains("Codex")
                && line.contains("Email: member@example.invalid")
                && line.contains("Plan: Plus")
        })
    });
    s.send("\x1b");
    s.paste("/use cu");
    s.send("\t");
    s.until("account argument completed", |s| s.draft() == "/use cursor");
    s.enter();
    s.until("account switched", |s| {
        s.screen().contains("using Cursor") && s.draft().is_empty()
    });
    assert!(s.prompts().is_empty());
    s.exit();
}

#[test]
#[ignore = "isolated modal close/cancel input journey; fake providers only"]
fn usage_screen_shows_paused_input_and_mouse_x_returns_to_chat_without_sending() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    s.submit("/usage");
    s.until("paused message box and exit cue", |s| {
        s.screen().contains("Esc or X closes usage")
    });
    s.paste("fixture filter");
    s.until("paste stays in modal filter", |s| {
        s.screen().contains("fixture filter")
    });
    assert!(s.draft().is_empty());
    assert!(s.prompts().is_empty());
    let screen = s.screen();
    let (y, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("usage") && line.contains('X'))
        .unwrap();
    let x = line.chars().position(|c| c == 'X').unwrap();
    s.send(&format!(
        "\x1b[<0;{};{}M\x1b[<0;{};{}m",
        x + 1,
        y + 1,
        x + 1,
        y + 1
    ));
    s.until("X returned to editable chat", |s| {
        !s.screen().contains("Esc or X closes usage") && s.screen().contains("Message Concordia")
    });
    assert!(s.prompts().is_empty());
    s.exit();
}

#[test]
#[ignore = "isolated current-chat file completion and preview journey; fake provider only"]
fn current_chat_file_suggestions_preview_and_toggle_without_sending_the_draft() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    std::fs::write(
        dir.path().join("workspace/current-note.md"),
        "# RELATED_PREVIEW\n**FORMATTED_TEXT**\n\n| Area | Proof |\n| --- | --- |\n| Memory | TABLE_PREVIEW |\n| Queue | TABLE_SECOND_ROW |\n",
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.submit("Inspect current-note.md");
    s.until("fake reply mentions current file", |s| {
        s.screen().contains("FAKE REPLY: Inspect current-note.md")
    });
    s.paste("review @current-");
    s.until("current-chat file suggestion", |s| {
        s.screen().contains("Ctrl+P preview") && s.draft() == "review @current-"
    });
    s.send("\x10");
    s.until("formatted file preview", |s| {
        s.screen().contains("RELATED_PREVIEW")
            && s.screen().contains("FORMATTED_TEXT")
            && !s.screen().contains("**FORMATTED_TEXT**")
            && s.screen().contains("TABLE_PREVIEW")
            && s.screen().contains("TABLE_SECOND_ROW")
            && s.screen().contains("├")
            && !s.screen().contains("| --- |")
    });
    assert_eq!(s.draft(), "review @current-");
    s.send("\x1bv");
    s.until("raw view", |s| s.screen().contains("**FORMATTED_TEXT**"));
    s.send("\x1b");
    s.send("\t");
    s.until("file reference inserted without sending", |s| {
        s.draft() == "review @current-note.md "
    });
    assert_eq!(s.prompts(), ["Inspect current-note.md"]);
    s.send("\x01\x08");
    s.until("draft cleared", |s| s.draft().is_empty());
    s.submit("/open");
    s.until("related file chooser", |s| {
        s.screen().contains("Open ·")
            && s.screen().contains("workspace")
            && s.screen().contains("current-note.md")
    });
    s.enter();
    s.until("existing file pane reused", |s| {
        s.screen().contains("RELATED_PREVIEW") && s.draft().is_empty()
    });
    assert_eq!(s.prompts(), ["Inspect current-note.md"]);
    s.send("\x1b");
    s.exit();
}

fn seed_native_quota_source(root: &Path) -> Vec<u8> {
    concordia::store::upsert_stored_account(
        root,
        concordia::store::StoredAccount {
            id: "codex".into(),
            provider: ProviderKind::Codex,
            home_path: Some(root.join("profile/.codex").to_string_lossy().into_owned()),
            label: "Codex".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    let folder = root.join("profile/.codex/sessions");
    std::fs::create_dir_all(&folder).unwrap();
    let body = format!(
        "{}\n{}\n",
        serde_json::json!({"type":"session_meta","payload":{"id":"quota-source-session","cwd":root.join("workspace").to_string_lossy()}}),
        serde_json::json!({"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"Finish important task [quota]"}]}})
    );
    std::fs::write(folder.join("quota-source-session.jsonl"), &body).unwrap();
    body.into_bytes()
}

#[test]
#[ignore = "isolated native-pointer handoff journey; fake providers only"]
fn quota_handoff_supplies_native_source_pointers_with_concordia_recording_off() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    std::fs::write(dir.path().join("config.toml"), "[threads]\nlogging='off'\n[ui]\ntips=false\n[continuation]\nmode='automatic'\naccounts=['claude']\n").unwrap();
    let original = seed_native_quota_source(dir.path());
    let mut s = Session::launch(dir.path());
    s.submit("Finish important task [quota]");
    s.until("native pointer handoff reaches fake Claude", |s| {
        s.journal()
            .iter()
            .any(|event| event["kind"] == "start" && event["provider"] == "claude")
    });
    let journal = s.journal();
    let sent = journal
        .iter()
        .find(|event| event["kind"] == "start" && event["provider"] == "claude")
        .unwrap()["prompt"]
        .as_str()
        .unwrap();
    assert!(sent.contains("[native source pointers]"), "{sent}");
    assert!(
        sent.contains("chat excerpt")
            && sent.contains("Original request:\nFinish important task [quota]")
    );
    let native = dir
        .path()
        .join("profile/.codex/sessions/quota-source-session.jsonl");
    assert_eq!(std::fs::read(native).unwrap(), original);
    assert!(
        !dir.path().join("threads").exists(),
        "native pointer handoff does not require copied Concordia transcripts"
    );
    let index_file = std::fs::read_dir(dir.path().join(".ao/native-memory"))
        .unwrap()
        .flatten()
        .find(|e| e.path().extension().is_some_and(|e| e == "json"))
        .unwrap();
    let id = index_file
        .path()
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let excerpt = concordia::native_memory::read(dir.path(), &id, 1, 0, 4096).unwrap();
    assert!(excerpt.raw.contains("Finish important task [quota]"));
    s.until("continuation finishes", |s| {
        s.journal()
            .iter()
            .any(|event| event["kind"] == "done" && event["provider"] == "claude")
    });
    s.exit();
}

#[test]
#[ignore = "isolated unfinished-request restart journey; no automatic replay"]
fn usage_limited_request_survives_restart_separately_from_the_newer_draft() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    std::fs::write(
        dir.path().join("config.toml"),
        "[threads]\nlogging='off'\n[ui]\ntips=false\n[continuation]\nmode='manual'\n",
    )
    .unwrap();
    seed_native_quota_source(dir.path());
    let mut s = Session::launch(dir.path());
    s.submit("Finish important task [quota]");
    s.until("quota request paused", |s| {
        s.screen().contains("usage limit reached")
    });
    s.send("\x1b");
    s.send("\x01\x08");
    s.until("restored retry draft cleared", |s| s.draft().is_empty());
    s.paste("newer draft after failure");
    s.until("new draft saved", |s| {
        s.draft() == "newer draft after failure"
    });
    drop(s);
    let mut s = Session::launch(dir.path());
    s.until("original request restored without sending", |s| {
        s.screen().contains("unfinished request was recovered")
            && s.draft() == "newer draft after failure"
    });
    assert_eq!(s.prompts().len(), 1);
    let records: Vec<_> = std::fs::read_dir(dir.path().join("pane-recovery"))
        .unwrap()
        .flatten()
        .flat_map(|workspace| std::fs::read_dir(workspace.path()).unwrap().flatten())
        .filter_map(|record| std::fs::read_to_string(record.path().join("turn.json")).ok())
        .collect();
    assert!(records
        .iter()
        .any(|record| record.contains("Finish important task [quota]")
            && record.contains("quota-source-session")));
    s.exit();
}

#[test]
#[ignore = "isolated explicit-capacity rollover journey; fake provider only"]
fn reported_context_capacity_starts_one_new_native_session_with_original_source_pointers() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    std::fs::write(dir.path().join("config.toml"), "[threads]\nlogging='off'\n[ui]\ntips=false\n[continuation]\nmode='automatic'\naccounts=['claude']\n").unwrap();
    seed_native_quota_source(dir.path());
    let folder = dir.path().join("profile/.codex/sessions");
    let body = format!(
        "{}\n{}\n",
        serde_json::json!({"type":"session_meta","payload":{"id":"capacity-source-session"}}),
        serde_json::json!({"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"Finish task [context]"}]}})
    );
    std::fs::write(folder.join("capacity-source-session.jsonl"), body).unwrap();
    let mut s = Session::launch(dir.path());
    s.submit("Finish task [context]");
    s.until("capacity recovery completed", |s| {
        s.journal().iter().any(|event| event["kind"] == "done")
    });
    let journal = s.journal();
    let starts: Vec<_> = journal
        .iter()
        .filter(|event| event["kind"] == "start")
        .collect();
    assert_eq!(starts.len(), 2);
    assert!(starts.iter().all(|event| event["provider"] == "codex"));
    assert!(starts[1]["prompt"]
        .as_str()
        .unwrap()
        .contains("[native source pointers]"));
    assert!(starts[1]["prompt"]
        .as_str()
        .unwrap()
        .contains("capacity-source-session"));
    assert!(starts[1]["args"]
        .as_array()
        .unwrap()
        .iter()
        .all(|arg| arg.as_str() != Some("resume")));
    s.exit();
}

#[test]
#[ignore = "isolated exact Harness pin delivery; fake provider only"]
fn connected_harness_pins_are_sent_once_per_new_native_session_and_not_on_resume() {
    use sha2::{Digest, Sha256};
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let home = dir.path().join("profile/.codex");
    let project = dir.path().join("workspace");
    let path = dir.path().join("harness.json");
    let manifest = serde_json::json!({"name":"fixture-pins","providers":{"codex":{"connect":["fake-provider"]}},"pinned_decisions":["Preserve these exact  spaces.","Keep original logs."]}).to_string();
    std::fs::write(&path, &manifest).unwrap();
    let digest = format!("{:x}", Sha256::digest(manifest.as_bytes()));
    concordia::harness::connections::record_with_pins(
        dir.path(),
        &concordia::harness::connections::Identity {
            name: "fixture-pins",
            version: None,
            manifest_path: path.to_str().unwrap(),
            manifest_sha256: &digest,
            skill: None,
        },
        concordia::harness::connections::Entry {
            provider: "codex".into(),
            home: Some(home.to_string_lossy().into_owned()),
            project: project.to_string_lossy().into_owned(),
            connect_ok: true,
            connect_note: "synthetic explicit connection".into(),
            verify: "not-run".into(),
            verify_note: String::new(),
            at: 0,
        },
        true,
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.submit("Inspect first request");
    s.until("first pinned session complete", |s| {
        s.journal()
            .iter()
            .filter(|event| event["kind"] == "done")
            .count()
            == 1
    });
    let first = s
        .journal()
        .into_iter()
        .find(|event| event["kind"] == "start")
        .unwrap()["prompt"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(first.matches("Preserve these exact  spaces.").count(), 1);
    assert!(first.contains("Preserve these exact  spaces.\nKeep original logs."));
    s.submit("Inspect second request");
    s.until("native resume complete", |s| {
        s.journal()
            .iter()
            .filter(|event| event["kind"] == "done")
            .count()
            == 2
    });
    let starts: Vec<_> = s
        .journal()
        .into_iter()
        .filter(|event| event["kind"] == "start")
        .collect();
    assert!(!starts[1]["prompt"]
        .as_str()
        .unwrap()
        .contains("[pinned decisions from connected harness]"));
    s.exit();
}

#[test]
#[ignore = "isolated paste formatting and exact native payload journey; fake provider only"]
fn pasted_spacing_blank_lines_and_line_endings_reach_the_provider_unchanged() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    for (index, newline) in ["\n", "\r\n", "\r"].into_iter().enumerate() {
        let raw = format!("    first{index}  {newline}{newline}      second{index}{newline}");
        let expected = format!("    first{index}  \n\n      second{index}\n");
        s.paste(&raw);
        s.until("multiline draft retains distinct lines", |s| {
            let draft = s.draft();
            draft.contains(&format!("first{index}")) && draft.contains(&format!("second{index}"))
        });
        s.enter();
        s.until("formatted payload received", |s| {
            s.journal().iter().filter(|e| e["kind"] == "start").count() == index + 1
        });
        let journal = s.journal();
        let start = journal.iter().rfind(|e| e["kind"] == "start").unwrap();
        assert!(
            start["prompt"].as_str().unwrap().ends_with(&expected),
            "native payload lost whitespace: {:?}",
            start["prompt"]
        );
        s.until("paste turn completed", |s| {
            s.journal().iter().filter(|e| e["kind"] == "done").count() == index + 1
        });
    }
    s.exit();
}




