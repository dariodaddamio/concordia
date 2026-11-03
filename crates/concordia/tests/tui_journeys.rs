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
        s.submit("/attach diagram.png");
        s.until("attached image chip", |s| {
            s.screen().contains("Image 1: diagram.png")
        });
        let preview_draft = s.draft();
        s.until("automatic image preview without a click", |s| {
            let screen = s.screen();
            screen.contains("Preview · diagram.png")
                && screen.contains("12 × 8")
                && screen.contains('▀')
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
        s.paste("preview must keep draft empty");
        s.send("\x1b");
        s.until("raster close preserves composer", |s| {
            !s.screen().contains("cell preview") && s.draft() == preview_draft
        });
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
        let blobs = std::fs::read_dir(dir.path().join("attachments"))
            .unwrap()
            .count();
        let pasted = format!("\"{}\"", document.display());
        s.paste(&pasted);
        s.until("pasted paths stay text until explicit attach", |s| {
            s.draft() == pasted && s.screen().contains("pasted file(s) too")
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
        let at = row.rfind("[X]").unwrap();
        let x = row[..at].chars().count() + 1;
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
        s.screen().contains("synthetic-command") && s.journal().iter().all(|e| e["kind"] != "done")
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
#[ignore = "isolated landing/goals terminal journey; no provider turns"]
fn landing_goal_scroll_entry_and_completion_work_through_the_real_terminal() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let sessions = dir.path().join("profile/.codex/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let meta = serde_json::json!({"type":"session_meta","payload":{"id":"landing-fixture",
        "cwd":dir.path().join("workspace").to_string_lossy()}});
    let message = serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user",
        "content":[{"type":"input_text","text":"synthetic landing chat"}]}});
    std::fs::write(
        sessions.join("landing.jsonl"),
        format!("{meta}\n{message}\n"),
    )
    .unwrap();
    let goals: Vec<_> = (0..40)
        .map(|i| {
            serde_json::json!({"id":format!("g-{i}"),
        "text":format!("terminal-goal-{i:02}"),"done":false,"added_at":"0","closed_at":null})
        })
        .collect();
    std::fs::write(
        dir.path().join("goals.json"),
        serde_json::to_vec(&goals).unwrap(),
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.until("landing goals visible", |s| {
        s.screen().contains("terminal-goal-00")
    });
    s.send("\t");
    s.send("\x1b[A");
    s.until("Up wraps to the last goal", |s| {
        s.screen().contains("> \u{25cb} terminal-goal-39")
    });
    s.send("\x1b[B");
    s.until("Down wraps to the first goal", |s| {
        s.screen().contains("> \u{25cb} terminal-goal-00")
    });
    s.send(&"\x1b[B".repeat(39));
    s.until("last goal scrolled into view", |s| {
        s.screen().contains("> \u{25cb} terminal-goal-39")
    });
    s.send("a");
    s.paste("new terminal goal");
    s.until("goal entry visible", |s| {
        s.screen().contains("new terminal goal")
    });
    s.send("\x1b[D\x1a"); // Navigation/undo remain inside the goal entry.
    assert!(s.draft().is_empty());
    s.enter();
    let saved = || -> Vec<Value> {
        concordia::goals::read_goals(dir.path())
            .unwrap()
            .into_iter()
            .map(|goal| serde_json::to_value(goal).unwrap())
            .collect()
    };
    s.until("goal saved", |_| {
        saved().iter().any(|g| g["text"] == "new terminal goal")
    });
    s.enter();
    s.until("completion confirmation visible", |s| {
        s.screen().contains("Mark done?")
    });
    s.enter();
    s.until("goal completion saved", |_| {
        saved()
            .iter()
            .any(|g| g["text"] == "new terminal goal" && g["done"] == true)
    });
    assert!(s.prompts().is_empty());
    s.send("\t");
    s.paste("/usage");
    s.until("draft keeps chats and goals visible", |s| {
        s.draft() == "/usage" && s.screen().contains(" chats ") && s.screen().contains(" goals ")
    });
    s.enter();
    s.until("Enter sends the drafted usage command", |s| {
        !s.screen().contains(" chats ") && s.draft().is_empty()
    });
    s.send("\x1b");
    s.paste("preserve my goal-review draft");
    s.until("goal review draft persisted", |s| {
        s.draft() == "preserve my goal-review draft"
    });
    let propose = |action: &str, request: &str, goal: Option<&str>, text: Option<&str>| {
        let revision =
            concordia::goals::revision(&concordia::goals::read_goals(dir.path()).unwrap()).unwrap();
        let mut arguments =
            serde_json::json!({"action":action,"request_id":request,"revision":revision});
        if let Some(goal) = goal {
            arguments["goal"] = serde_json::json!(goal);
        }
        if let Some(text) = text {
            arguments["text"] = serde_json::json!(text);
        }
        let response = concordia::mcp::handle_line(dir.path(), &serde_json::json!({"jsonrpc":"2.0","id":90,"method":"tools/call","params":{"name":"goals","arguments":arguments}}).to_string()).unwrap();
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["result"]["isError"], false, "{response}");
    };
    let add_request = uuid::Uuid::new_v4().to_string();
    let goal_id = format!("g-{add_request}");
    let unchanged = std::fs::read(dir.path().join("goals.json")).unwrap();
    propose(
        "propose_add",
        &add_request,
        None,
        Some("Review the exact proposed MCP goal"),
    );
    s.until("full goal proposal awaits explicit focus", |s| {
        let screen = s.screen();
        screen.contains("MCP proposes ADD")
            && screen.contains("Review the exact proposed MCP goal")
            && screen.contains("F7 focuses goal review")
    });
    s.send("y");
    s.until("ordinary yes-shaped typing edits the draft", |s| {
        s.draft() == "preserve my goal-review drafty"
    });
    assert_eq!(
        std::fs::read(dir.path().join("goals.json")).unwrap(),
        unchanged
    );
    s.send("\x1b[18~");
    s.until("goal review explicitly focused", |s| {
        s.screen().contains("Goal review focused")
    });
    s.send("y");
    s.until("locally approved MCP goal added", |_| {
        concordia::goals::read_goals(dir.path())
            .unwrap()
            .iter()
            .any(|goal| {
                goal.id == goal_id
                    && goal.origin.as_ref().is_some_and(|origin| {
                        origin.request_id.as_deref() == Some(add_request.as_str())
                    })
            })
    });
    assert_eq!(s.draft(), "preserve my goal-review drafty");
    assert!(s.prompts().is_empty());
    let decline_request = uuid::Uuid::new_v4().to_string();
    propose("propose_close", &decline_request, Some(&goal_id), None);
    s.until("full close proposal visible", |s| {
        s.screen().contains("MCP proposes CLOSE") && s.screen().contains(&decline_request)
    });
    s.send("\x1b[18~");
    s.until("close proposal focus", |s| {
        s.screen().contains("Goal review focused")
    });
    s.send("n");
    s.until("close proposal declined without goal mutation", |_| {
        concordia::goals::changes::status(dir.path(), &decline_request)
            .is_ok_and(|status| status.contains("declined"))
    });
    assert!(
        !concordia::goals::read_goals(dir.path())
            .unwrap()
            .iter()
            .find(|goal| goal.id == goal_id)
            .unwrap()
            .done
    );
    let close_request = uuid::Uuid::new_v4().to_string();
    propose("propose_close", &close_request, Some(&goal_id), None);
    s.until("new close proposal visible", |s| {
        s.screen().contains("MCP proposes CLOSE") && s.screen().contains(&close_request)
    });
    s.send("\x1b[18~");
    s.until("second close proposal focus", |s| {
        s.screen().contains("Goal review focused")
    });
    s.send("y");
    s.until("approved goal close attributed to exact request", |_| {
        concordia::goals::read_goals(dir.path())
            .unwrap()
            .iter()
            .any(|goal| {
                goal.id == goal_id
                    && goal.done
                    && goal
                        .closed_origin
                        .as_ref()
                        .and_then(|origin| origin.request_id.as_deref())
                        == Some(close_request.as_str())
            })
    });
    assert_eq!(s.draft(), "preserve my goal-review drafty");
    let stale_request = uuid::Uuid::new_v4().to_string();
    propose(
        "propose_add",
        &stale_request,
        None,
        Some("Must not overwrite concurrent goal changes"),
    );
    s.until("proposal before concurrent goal change", |s| {
        s.screen().contains(&stale_request)
    });
    concordia::goals::save_goals(
        dir.path(),
        &[concordia::goals::Goal {
            id: "concurrent-terminal-goal".into(),
            text: "Concurrent user goal".into(),
            added_at: "0".into(),
            origin: Some(concordia::goals::Attribution::user()),
            ..Default::default()
        }],
    )
    .unwrap();
    s.send("\x1b[18~");
    s.until("stale proposal review focused", |s| {
        s.screen().contains("Goal review focused")
    });
    s.send("y");
    s.until("stale goal proposal rejected", |s| {
        s.screen().contains("goals changed since this proposal")
    });
    assert!(concordia::goals::read_goals(dir.path())
        .unwrap()
        .iter()
        .all(|goal| goal.id != format!("g-{stale_request}")));
    assert_eq!(s.draft(), "preserve my goal-review drafty");
    assert!(s.prompts().is_empty());
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
    let pid = s.child.process_id().expect("child pid");
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
        s.screen().contains("type / to see the commands")
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
    assert!(s.prompts().is_empty());
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
    s.send("\x17"); // Another common Windows host encoding of word deletion.
    s.until("Ctrl+W encoding deletes a word", |s| s.draft() == "alpha ");
    s.send("\x7f");
    s.until("Backspace deletes one character", |s| s.draft() == "alpha");
    s.send("\x01");
    s.send("\x7f");
    s.submit("/use");
    s.until("account picker", |s| s.screen().contains("77% left"));
    s.send("\x1b");
    // Simulate a host losing its visible contents while Concordia retains its diff buffer.
    s.bytes.lock().unwrap().extend_from_slice(b"\x1b[2J\x1b[H");
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
#[ignore = "isolated session-selected provider handoff journey; fake providers only"]
fn a_session_selected_provider_continues_without_changing_the_saved_default() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch(dir.path());
    let config_before = std::fs::read(dir.path().join("config.toml")).unwrap();
    s.submit("/continuation policy set claude");
    s.until("ordered local policy saved without a provider turn", |s| {
        s.screen().contains("Routing policy saved")
    });
    s.submit("/continuation routes");
    s.until("route eligibility visible", |s| {
        s.screen()
            .contains("Local routing snapshot MainContinuation")
    });
    assert!(s.prompts().is_empty());
    assert_eq!(
        std::fs::read(dir.path().join("config.toml")).unwrap(),
        config_before
    );
    s.submit("/delegate routes");
    s.until("worker route inspection makes no provider turn", |s| {
        s.screen().contains("Local routing snapshot Worker")
    });
    assert!(s.prompts().is_empty());
    s.submit("/continuation");
    s.until("session policy screen", |s| {
        s.screen().contains("next provider at a usage limit")
    });
    s.paste("claude");
    s.enter();
    s.until("session account chosen", |s| {
        s.screen().contains("try claude first")
    });
    assert!(s.prompts().is_empty());
    assert!(concordia::store::get_settings(dir.path())
        .continuation_automatic
        .is_none());
    s.submit("Fix graph [quota] [coded-limit]");
    s.until("selected provider completes continuation", |s| {
        s.journal()
            .iter()
            .any(|e| e["kind"] == "done" && e["provider"] == "claude")
    });
    let starts: Vec<_> = s
        .journal()
        .into_iter()
        .filter(|e| e["kind"] == "start")
        .collect();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0]["provider"], "codex");
    assert_eq!(starts[1]["provider"], "claude");
    assert!(!starts[1]["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|arg| arg == "quota-source-session" || arg == "--resume"));
    s.exit();
}

#[test]
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
fn usage_limit_automatically_continues_on_configured_provider_without_foreign_session() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    std::fs::write(dir.path().join("config.toml"), "[threads]\nenabled=true\n[ui]\ntips=false\n[continuation]\nmode='automatic'\naccounts=['claude']\n").unwrap();
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
    parent_shell: bool,
    child: Box<dyn Child + Send + Sync>,
    writer: Option<Box<dyn Write + Send>>,
    master: Option<Box<dyn MasterPty + Send>>,
    bytes: Arc<Mutex<Vec<u8>>>,
    root: PathBuf,
}

#[test]
#[ignore = "isolated ConPTY resize and exit rendering journey"]
fn repeated_half_full_resizes_repaint_at_the_origin_and_exit_cleanly() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let mut s = Session::launch_mode(dir.path(), &dir.path().join("workspace"), &[], true);
    s.submit("home navigation");
    s.until("existing conversation is visible", |s| {
        s.screen().contains("FAKE REPLY: home navigation")
    });
    s.submit("/synthetic resize-chat-marker");
    s.until("transcript ready", |s| {
        s.screen().contains("type / to see the commands")
    });
    s.paste("resize-draft-marker");
    s.until("draft persisted", |s| s.draft() == "resize-draft-marker");
    s.send("\x1b[<0;2;1M\x1b[<0;2;1m");
    s.until(
        "Home arrow opens the chat browser without losing the draft",
        |s| s.screen().contains(" chats ") && s.draft() == "resize-draft-marker",
    );
    s.send("\x1b");
    s.until("Home returns to the existing chat", |s| {
        !s.screen().contains(" chats ") && s.screen().contains("resize-chat-marker")
    });
    s.send("\x1b[<0;2;2M\x1b[<0;2;2m");
    s.until("credit row opens balance details", |s| {
        let screen = s.screen();
        screen.contains("Credit balance")
            && screen.contains("Balance: Not reported by provider")
            && screen.contains("Open full credit balance")
    });
    s.paste("details must retain the draft");
    assert_eq!(s.draft(), "resize-draft-marker");
    s.send("\x1b");
    s.until("credit details close without dispatching", |s| {
        s.screen().contains("resize-chat-marker") && s.draft() == "resize-draft-marker"
    });
    for (cols, rows) in [
        (60, 40),
        (120, 40),
        (60, 20),
        (160, 50),
        (40, 12),
        (120, 40),
        (180, 55),
        (60, 55),
        (120, 40),
    ] {
        let offset = s.bytes.lock().unwrap().len();
        s.master
            .as_ref()
            .unwrap()
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        s.until(&format!("aligned repaint at {cols}x{rows}"), |s| {
            let bytes = s.bytes.lock().unwrap();
            let screen = render_sized(&bytes[offset..], cols as usize, rows as usize);
            screen
                .lines()
                .next()
                .unwrap_or_default()
                .trim_start()
                .starts_with("← Home")
                && screen.contains("resize-draft-marker")
                && screen
                    .lines()
                    .nth(rows as usize - 2)
                    .unwrap_or_default()
                    .contains('\u{2570}')
        });
        assert_eq!(s.draft(), "resize-draft-marker");
    }
    // Intermediate events can be coalesced; repaint using the final host size.
    let offset = s.bytes.lock().unwrap().len();
    for (cols, rows) in [(55, 18), (180, 55), (75, 35), (100, 30)] {
        s.master
            .as_ref()
            .unwrap()
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(25));
    }
    s.until("rapid resize settles at the current size", |s| {
        let bytes = s.bytes.lock().unwrap();
        let screen = render_sized(&bytes[offset..], 100, 30);
        screen
            .lines()
            .next()
            .unwrap_or_default()
            .trim_start()
            .starts_with("← Home")
            && screen
                .lines()
                .nth(28)
                .unwrap_or_default()
                .contains('\u{2570}')
            && screen.contains("resize-draft-marker")
    });
    assert_eq!(s.prompts(), ["home navigation"]);
    s.send("\x01\x08");
    s.send("\x03");
    s.send("\x03");
    s.until("parent shell regained control", |s| {
        String::from_utf8_lossy(&s.bytes.lock().unwrap()).contains("CONCORDIA_SHELL_RETURNED")
    });
    let bytes = s.bytes.lock().unwrap();
    let output = String::from_utf8_lossy(&bytes);
    let leave = output.rfind("\x1b[?1049l").unwrap_or_else(|| {
        panic!(
            "alternate screen not restored; start={:?}; tail={:?}",
            output.chars().take(250).collect::<String>(),
            output.chars().rev().take(700).collect::<String>()
        )
    });
    let before = &output[..leave];
    assert!(
        render(before.as_bytes()).trim().is_empty(),
        "alternate contents must be erased before returning to shell"
    );
    let shell = render(output[leave..].as_bytes());
    assert!(shell.contains("CONCORDIA_SHELL_ORIGINAL"));
    assert!(shell.contains("CONCORDIA_SHELL_RETURNED"));
    assert!(!shell.contains("resize-draft-marker"));
    assert!(!shell.contains("concordia"));
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

fn initialize_git_fixture(workspace: &Path, empty_hooks: &Path) {
    use std::os::windows::process::CommandExt;
    std::fs::create_dir_all(empty_hooks).unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["commit", "--allow-empty", "-m", "synthetic base"],
    ] {
        let output = std::process::Command::new(which::which("git").unwrap())
            .current_dir(workspace)
            .creation_flags(0x08000000)
            .args([
                "-c",
                &format!("core.hooksPath={}", empty_hooks.display()),
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgSign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fixture Git initialization failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Session {
    fn launch(root: &Path) -> Self {
        Self::launch_at(root, &root.join("workspace"))
    }

    fn launch_at(root: &Path, workspace: &Path) -> Self {
        Self::launch_args(root, workspace, &[])
    }

    fn launch_args(root: &Path, workspace: &Path, args: &[&str]) -> Self {
        Self::launch_mode(root, workspace, args, false)
    }

    fn launch_mode(root: &Path, workspace: &Path, args: &[&str], parent_shell: bool) -> Self {
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
        let mut cmd = if parent_shell {
            let script = root.join("parent.cmd");
            std::fs::write(
                &script,
                format!(
                    "@echo off\r\necho CONCORDIA_SHELL_ORIGINAL\r\n\"{}\"\r\necho CONCORDIA_SHELL_RETURNED\r\npause >nul\r\n",
                    Path::new(&binary).display()
                ),
            )
            .unwrap();
            let mut cmd = CommandBuilder::new("cmd.exe");
            cmd.args(["/d", "/c"]);
            cmd.arg(script);
            cmd
        } else {
            CommandBuilder::new(binary)
        };
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
            parent_shell,
            child,
            writer: Some(writer),
            master: Some(pair.master),
            bytes,
            root: root.into(),
        };
        session.send("\x1b[1;1R");
        session.until("startup", |s| {
            s.screen().contains(if args.first() == Some(&"file-edit") {
                "Ctrl+Q close"
            } else if args.first() == Some(&"preview") {
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
            .map(|e| {
                concordia::prompt_time::strip_header(e["prompt"].as_str().unwrap()).to_string()
            })
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
                                if self.parent_shell
                                    || record["owner_pid"].as_u64() == Some(pid as u64)
                                {
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

    fn acknowledge_stopped_scope(&mut self) {
        let draft = self.draft();
        let prompts = self.prompts().len();
        let registry = concordia::workspaces::load(&self.root).unwrap();
        let run = registry
            .reservations
            .iter()
            .find(|entry| entry.phase == concordia::workspaces::Phase::ReconcileRequired)
            .unwrap()
            .run
            .clone();
        self.send("\x1b[1;5B");
        self.send("\x01\x08");
        self.until("draft cleared for reconciliation command", |s| {
            s.draft().is_empty()
        });
        self.submit(&format!("/scope stopped {run}"));
        self.until("stopped process acknowledged", |s| {
            s.screen().contains("Stopped-process acknowledgment saved")
        });
        assert_eq!(
            self.prompts().len(),
            prompts,
            "acknowledgment never replays a turn"
        );
        self.paste(&draft);
        self.until("independent draft restored", |s| s.draft() == draft);
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
    render_sized(bytes, COLS, ROWS)
}

fn render_sized(bytes: &[u8], cols: usize, rows: usize) -> String {
    let mut cells = vec![vec![' '; cols]; rows];
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
                            y = amount.saturating_sub(1).min(rows - 1);
                            x = n
                                .get(1)
                                .copied()
                                .unwrap_or(1)
                                .max(1)
                                .saturating_sub(1)
                                .min(cols - 1);
                        }
                        'A' => y = y.saturating_sub(amount),
                        'B' => y = (y + amount).min(rows - 1),
                        'C' => x = (x + amount).min(cols - 1),
                        'D' => x = x.saturating_sub(amount),
                        'G' => x = amount.saturating_sub(1).min(cols - 1),
                        'd' => y = amount.saturating_sub(1).min(rows - 1),
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
                        'X' => cells[y][x..(x + amount).min(cols)].fill(' '),
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
                if y + 1 == rows {
                    cells.remove(0);
                    cells.push(vec![' '; cols]);
                } else {
                    y += 1;
                }
            }
            '\x08' => x = x.saturating_sub(1),
            c if !c.is_control() => {
                if x >= cols {
                    x = 0;
                    y = (y + 1).min(rows - 1);
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
        s.draft().is_empty() && s.screen().contains("cancelled")
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
fn two_panes_keep_live_drafts_and_recovery_files_separate() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for (first_provider, second_provider) in [
        (ProviderKind::Codex, ProviderKind::Codex),
        (ProviderKind::Claude, ProviderKind::Claude),
        (ProviderKind::Cursor, ProviderKind::Cursor),
        (ProviderKind::Codex, ProviderKind::Claude),
    ] {
        let dir = tempfile::tempdir().unwrap();
        setup(dir.path(), first_provider);
        let mut first = Session::launch(dir.path());
        first.paste("draft from first pane");
        first.until("first draft saved", |s| {
            s.draft() == "draft from first pane"
        });
        if second_provider != first_provider {
            setup(dir.path(), second_provider);
        }
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
        first.send("\x01\x08");
        first.submit("[wait] concurrent first pane");
        first.until("first provider is running", |s| s.prompts().len() == 1);
        second.send("\x01\x08");
        second.submit("[wait] concurrent second pane");
        second.until("both panes dispatched to the same provider", |s| {
            s.prompts().len() == 2
        });
        assert!(!second.screen().contains("Workspace admission:"));
        first.release();
        first.until("first concurrent reply", |s| {
            s.screen()
                .contains("FAKE REPLY: [wait] concurrent first pane")
        });
        second.until("second concurrent reply", |s| {
            s.screen()
                .contains("FAKE REPLY: [wait] concurrent second pane")
        });
        eprintln!("PASS: active panes have separate draft recovery");
    }
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
    s.send("\x1b[200~queued\tcode\r\nfirst\x1b[201~\t\x1b[200~draft\r\n/exit\r\nsecond\x1b[201~");
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
    s.acknowledge_stopped_scope();
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
    s.acknowledge_stopped_scope();
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
        format!(
            "[ui]\ntips=false\n[editor]\nprogram='{FAKE}'\nargs=['+{{line}}','--','{{file}}']\n"
        ),
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.submit("/edit hello world.rs:7");
    s.until("file launcher called", |s| {
        s.root.join("file-launch.json").is_file()
    });
    let args: Vec<String> =
        serde_json::from_slice(&std::fs::read(s.root.join("file-launch.json")).unwrap()).unwrap();
    assert!(args
        .iter()
        .any(|arg| arg.ends_with("concordia-fake-provider.exe")));
    assert!(args.contains(&"+7".into()));
    assert!(
        args.iter().any(|arg| arg.ends_with("hello world.rs")),
        "{args:?}"
    );
    assert!(s.prompts().is_empty());
    s.submit("/open hello world.rs");
    s.until("file action source pane", |s| {
        s.screen().contains("Raw · edit")
    });
    s.send("\x1b[14~");
    s.until("shared file actions", |s| {
        s.screen().contains("File actions")
            && s.screen().contains("Split off")
            && s.screen().contains("Open in editor")
    });
    s.send("\x1b");
    s.send("\x1b");
    s.paste("draft after file launch");
    s.until("chat remains usable", |s| {
        s.draft() == "draft after file launch"
    });
    s.send("\x1b[17~");
    s.send("\x1b[14~");
    s.until("file choices with main draft", |s| {
        s.screen().contains("File actions")
    });
    s.send("\x1b[B");
    s.enter();
    s.until("independent editor launch arguments", |s| {
        std::fs::read_to_string(s.root.join("file-launch.json"))
            .is_ok_and(|body| body.contains("file-edit"))
    });
    assert_eq!(s.draft(), "draft after file launch");
    assert!(s.prompts().is_empty());
    s.send("\x1b");
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
#[ignore = "isolated raw editing/save/conflict terminal journey; no model turns"]
fn formatted_defaults_raw_edits_save_and_dirty_close_work_in_real_terminal() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let path = dir.path().join("workspace/edit.md");
    std::fs::write(&path, "# EDIT_TITLE\r\n\r\n**ORIGINAL**\r\n").unwrap();
    let mut s = Session::launch(dir.path());
    s.submit("/open edit.md");
    s.until("formatted default", |s| {
        s.screen().contains("EDIT_TITLE") && !s.screen().contains("**ORIGINAL**")
    });
    s.paste("must not edit formatted");
    s.send("\x1bv");
    s.until("raw editable", |s| {
        s.screen().contains("**ORIGINAL**") && s.screen().contains("Ctrl+S save")
    });
    s.send("\x01");
    s.paste("# SAVED_TITLE\r\n\r\nNew content\r\n");
    s.until("dirty buffer", |s| s.screen().contains("* edit.md"));
    s.send("\x13");
    s.until("saved bytes", |_| {
        std::fs::read_to_string(&path).is_ok_and(|text| text.contains("SAVED_TITLE"))
    });
    s.until("save acknowledged", |s| {
        !s.screen().contains("* edit.md") && !s.screen().contains("Saving")
    });
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("must not edit"));
    s.paste("LOCAL_EDIT");
    s.until("new unsaved edit", |s| s.screen().contains("LOCAL_EDIT"));
    std::fs::write(&path, "EXTERNAL_CHANGE\r\n").unwrap();
    s.send("\x13");
    s.until("save conflict", |s| s.screen().contains("save refused"));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "EXTERNAL_CHANGE\r\n"
    );
    s.until("file edits reach pane recovery", |s| {
        fn contains_edit(path: &Path) -> bool {
            std::fs::read_dir(path)
                .into_iter()
                .flatten()
                .flatten()
                .any(|e| {
                    let path = e.path();
                    if path.is_dir() {
                        contains_edit(&path)
                    } else {
                        path.file_name().is_some_and(|n| n == "files.json")
                            && std::fs::read_to_string(path)
                                .is_ok_and(|body| body.contains("LOCAL_EDIT"))
                    }
                })
        }
        contains_edit(&s.root.join("pane-recovery"))
    });
    s.child.kill().unwrap(); // Only this isolated, provider-free fixture process.
    s.child.wait().unwrap();
    drop(s);
    let mut s = Session::launch(dir.path());
    s.until(
        "interrupted file edits recover without overwriting disk",
        |s| s.screen().contains("LOCAL_EDIT") && s.screen().contains("recovered edits retained"),
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "EXTERNAL_CHANGE\r\n"
    );
    s.send("\x1b");
    s.submit("/close 1");
    s.until("dirty close", |s| s.screen().contains("Unsaved"));
    s.send("d");
    s.until("closed", |s| !s.screen().contains("LOCAL_EDIT"));
    assert!(s.prompts().is_empty());
    s.exit();
    let work = dir.path().join("workspace");
    let mut standalone = Session::launch_args(dir.path(), &work, &["file-edit", "edit.md"]);
    standalone.until("independent formatted editor", |s| {
        s.screen().contains("EXTERNAL_CHANGE")
    });
    standalone.send("\x1bv");
    standalone.until("independent raw editor", |s| {
        s.screen().contains("Ctrl+S save")
    });
    standalone.send("\x01");
    standalone.paste("STANDALONE_SAVED\r\n");
    standalone.send("\x13");
    standalone.until("standalone native save", |_| {
        std::fs::read_to_string(&path).is_ok_and(|text| text == "STANDALONE_SAVED\r\n")
    });
    standalone.until("standalone acknowledged", |s| {
        !s.screen().contains("* edit.md") && !s.screen().contains("Saving")
    });
    standalone.send("\x01");
    standalone.paste("STANDALONE_UNSAVED\r\n");
    let roots = std::fs::read_dir(dir.path().join("file-editors/pane-recovery"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(roots.len(), 1);
    let records = std::fs::read_dir(&roots[0])
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    let id = records[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    standalone.until("independent owned recovery", |_| {
        std::fs::read_to_string(records[0].join("files.json"))
            .is_ok_and(|text| text.contains("STANDALONE_UNSAVED"))
    });
    standalone.child.kill().unwrap();
    standalone.child.wait().unwrap();
    drop(standalone);
    let mut restored = Session::launch_args(
        dir.path(),
        &work,
        &["file-edit", "edit.md", "--recover", &id],
    );
    restored.until("independent recovered edits", |s| {
        s.screen().contains("STANDALONE_UNSAVED") && s.screen().contains("* edit.md")
    });
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "STANDALONE_SAVED\r\n"
    );
    restored.send("\x11");
    restored.until("independent dirty quit", |s| s.screen().contains("Unsaved"));
    restored.send("d");
    let deadline = Instant::now() + Duration::from_secs(8);
    while restored.child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "Standalone editor did not exit");
        std::thread::sleep(Duration::from_millis(40));
    }
    assert!(restored.prompts().is_empty());
}

#[test]
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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

    s.submit("/usage");
    s.until("usage picker is open during the gated turn", |s| {
        s.screen().contains("┌usage")
    });
    let prompts_before_picker = s.prompts();

    // y is a valid armed approval key, but the picker owns it as a filter character.  Esc then
    // closes only the picker; the armed confirmation remains pending for chat focus.
    s.send("y");
    std::thread::sleep(Duration::from_millis(150));
    assert!(s.screen().contains("filter: y"));
    s.send("\x1b");
    s.until("usage picker is dismissed", |s| {
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
#[ignore = "slow PTY journey; run with: cargo test -p concordia --test tui_journeys -- --ignored"]
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
        s.screen().contains("Collapse")
    });
    let y = s
        .screen()
        .lines()
        .position(|line| line.contains("Collapse"))
        .unwrap()
        + 1;
    s.send(&format!("\x1b[<0;8;{y}M\x1b[<0;8;{y}m"));
    s.until("pinned header collapsed its card", |s| {
        !s.screen().contains("Collapse") && !s.screen().contains("synthetic output line")
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
        r#"{"models":[{"slug":"fixture-model"}]}"#,
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.until("header email is private by default", |s| {
        let screen = s.screen();
        let header = screen.lines().next().unwrap_or_default();
        header.contains("click to show") && !header.contains("member@example.invalid")
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
        "# RELATED_PREVIEW\n**FORMATTED_TEXT**\n\n| Area | Proof |\n| --- | --- |\n| Memory | TABLE_PREVIEW |\n",
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
    std::fs::write(dir.path().join("config.toml"), "[threads]\nenabled=false\n[ui]\ntips=false\n[continuation]\nmode='automatic'\naccounts=['claude']\n").unwrap();
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
        "[threads]\nenabled=false\n[ui]\ntips=false\n[continuation]\nmode='manual'\n",
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
        s.screen().contains("unfinished request recovered")
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
    std::fs::write(dir.path().join("config.toml"), "[threads]\nenabled=false\n[ui]\ntips=false\n[continuation]\nmode='automatic'\naccounts=['claude']\n").unwrap();
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
#[ignore = "isolated proactive context policy journey; fake providers only"]
fn proactive_context_policy_rolls_before_dispatch_and_keeps_original_pointers() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    std::fs::write(
        dir.path().join("profile/.codex/models_cache.json"),
        r#"{"models":[{"slug":"synthetic-model"}]}"#,
    )
    .unwrap();
    let folder = dir.path().join("profile/.codex/sessions");
    std::fs::create_dir_all(&folder).unwrap();
    let lines = [
        serde_json::json!({"type":"session_meta","payload":{"id":"proactive-session","cwd":dir.path().join("workspace")}}),
        serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"ORIGINAL_CONTEXT_SOURCE"}]}}),
        serde_json::json!({"type":"turn_context","payload":{"model":"synthetic-model","turn_id":"proactive-turn-1"}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"total_tokens":800,"input_tokens":600,"cached_input_tokens":100,"output_tokens":200},"total_token_usage":{"total_tokens":9999999},"model_context_window":1000}}}),
    ];
    std::fs::write(
        folder.join("proactive.jsonl"),
        lines.iter().map(|v| format!("{v}\n")).collect::<String>(),
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.submit("/resume");
    s.enter();
    s.until("native history loaded", |s| {
        s.screen().contains("ORIGINAL_CONTEXT_SOURCE")
    });
    s.submit("/model synthetic-model");
    s.until("explicit model selected", |_s| {
        concordia::store::get_use_selection(dir.path())
            .is_some_and(|selected| selected.model.as_deref() == Some("synthetic-model"))
    });
    s.submit("/usage");
    s.send("t");
    s.until("Concordia usage view", |s| {
        s.screen().contains("No turns recorded yet.")
    });
    s.send("e");
    s.until("separate native usage source coverage", |s| {
        let screen = s.screen();
        screen.contains("Native source observations")
            && screen.contains("proactive-session")
            && screen.contains("1 snapshots this page")
    });
    assert!(s.prompts().is_empty());
    s.send("c");
    s.until("native snapshots do not change Concordia totals", |s| {
        s.screen().contains("No turns recorded yet.")
    });
    s.send("\x1b");
    s.submit("/context rollover 80");
    s.until("policy enabled", |s| s.screen().contains("at 80%"));
    s.submit("A new task after the threshold");
    s.until("new task completed", |s| {
        s.journal().iter().any(|e| e["kind"] == "done")
    });
    let journal = s.journal();
    let starts: Vec<_> = journal.iter().filter(|e| e["kind"] == "start").collect();
    assert_eq!(starts.len(), 1);
    let args = starts[0]["args"].as_array().unwrap();
    assert!(!args.iter().any(|a| a == "resume"));
    assert!(starts[0]["prompt"]
        .as_str()
        .unwrap()
        .contains("[native source pointers]"));
    assert!(starts[0]["prompt"]
        .as_str()
        .unwrap()
        .contains("proactive-session"));
    s.submit("/context rollover off");
    s.until("policy disabled", |s| {
        s.screen().contains("proactive rollover off")
    });
    s.exit();
}

#[test]
#[ignore = "isolated memory retrieval terminal journey; no provider turns"]
fn memory_search_reads_original_sources_without_consuming_a_numbered_draft() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Codex);
    let folder = dir.path().join("profile/.codex/sessions");
    std::fs::create_dir_all(&folder).unwrap();
    let source = format!(
        "{}\n{}\n{}\n",
        serde_json::json!({"type":"session_meta","payload":{"id":"memory-fixture","cwd":dir.path().join("workspace")}}),
        serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"MEMORY_NEEDLE exact original"}]}}),
        serde_json::json!({"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","input":"*** Begin Patch\n*** Update File: src/preview space.rs\n@@\n-before\n+synthetic replacement\n*** End Patch"}})
    );
    std::fs::write(folder.join("memory.jsonl"), source).unwrap();
    let manifest = dir.path().join("workspace/harness.json");
    let manifest_text = serde_json::json!({"name":"ui-catalog","providers":{"codex":{"connect":["must-never-run"]}},"tools":[{"name":"ui-search","description":"Discover fixture tools without running them","argv":["must-never-run"]}]}).to_string();
    std::fs::write(&manifest, &manifest_text).unwrap();
    let manifest_path = manifest.display().to_string();
    let manifest_sha = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(manifest_text.as_bytes()))
    };
    concordia::harness::connections::record(
        dir.path(),
        &concordia::harness::connections::Identity {
            name: "ui-catalog",
            version: None,
            manifest_path: &manifest_path,
            manifest_sha256: &manifest_sha,
            skill: None,
        },
        concordia::harness::connections::Entry {
            provider: "codex".into(),
            home: None,
            project: dir.path().join("workspace").display().to_string(),
            connect_ok: true,
            connect_note: String::new(),
            verify: "not-run".into(),
            verify_note: String::new(),
            at: 0,
        },
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.paste("UNSENT_INTEGRATION_DRAFT");
    s.until(
        "integration draft is persisted before opening the catalog",
        |s| s.draft() == "UNSENT_INTEGRATION_DRAFT",
    );
    s.send("\x1b[21~");
    s.until("integration catalog shows verified namespaced tools", |s| {
        let screen = s.screen();
        screen.contains("Integrations and agent tools")
            && screen.contains("ui-search")
            && screen.contains("revision verified")
    });
    assert_eq!(s.draft(), "UNSENT_INTEGRATION_DRAFT");
    assert!(s.prompts().is_empty());
    s.send("\x1b");
    s.until(
        "integration catalog closes without consuming the draft",
        |s| {
            s.draft() == "UNSENT_INTEGRATION_DRAFT"
                && !s.screen().contains("Integrations and agent tools")
        },
    );
    s.send("\x01\x08");
    s.until("clear synthetic draft before source lookup", |s| {
        s.draft().is_empty()
    });
    s.submit("/memory recall-account codex MEMORY_NEEDLE");
    s.until("verified lexical result", |s| {
        s.screen().contains("MEMORY_NEEDLE exact original")
            && s.screen().contains("Lexical suggestion")
    });
    let source_id = std::fs::read_dir(dir.path().join(".ao/native-memory"))
        .unwrap()
        .flatten()
        .find_map(|entry| {
            let path = entry.path();
            (path
                .extension()
                .is_some_and(|extension| extension == "json"))
            .then(|| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(str::to_owned)
            })
            .flatten()
            .filter(|id| id.len() == 64)
        })
        .unwrap();
    s.submit(&format!("/memory file-facts {source_id}"));
    s.until("historical file fact shown", |s| {
        s.screen().contains("Historical file facts") && s.screen().contains("preview space.rs")
    });
    assert!(!dir.path().join("workspace/src/preview space.rs").exists());
    s.submit("/memory ranked-file \"src/preview space.rs\" MEMORY_NEEDLE");
    s.until("ranked file evidence shown", |s| {
        s.screen().contains("Ranked memory") && s.screen().contains("Historical file +1000")
    });
    concordia::goals::save_goals(
        dir.path(),
        &[concordia::goals::Goal {
            id: "fixture-ranking-goal".into(),
            text: "Synthetic retrieval goal".into(),
            done: false,
            added_at: "0".into(),
            closed_at: None,
            ..Default::default()
        }],
    )
    .unwrap();
    s.submit(&format!(
        "/memory goal-link fixture-ranking-goal {source_id}"
    ));
    s.until("goal association confirmed", |s| {
        s.screen().contains("attached") && s.screen().contains("true")
    });
    s.submit("/memory ranked-goal fixture-ranking-goal MEMORY_NEEDLE");
    s.until("confirmed goal ranks with visible reason", |s| {
        s.screen().contains("Confirmed goal +10000")
    });
    s.submit(&format!("/memory group-create {source_id}"));
    s.until("explicit group created", |s| {
        s.screen().contains("created") && s.screen().contains("true")
    });
    s.submit("/memory groups");
    s.until("confirmed related-work view", |s| {
        s.screen().contains("Confirmed related-chat groups")
            && s.screen().contains("/memory group-members")
    });
    s.send("\x1b");
    s.paste("2 finish all requested features");
    s.until("number remains ordinary draft text", |s| {
        s.draft() == "2 finish all requested features"
    });
    assert!(s.prompts().is_empty());
    s.exit();
}

#[test]
#[ignore = "isolated worker crash recovery terminal journey; fake providers only"]
fn worker_crash_recovery_preserves_request_and_pauses_without_reexecution() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Claude);
    let mut s = Session::launch(dir.path());
    s.submit("/workers add codex");
    s.until("worker enrolled", |s| s.screen().contains("worker added"));
    s.submit("/delegate codex RECOVERY_TASK [wait]");
    s.until("worker request started", |s| {
        s.journal().iter().any(|e| e["kind"] == "start")
    });
    let ledgers = || {
        std::fs::read_dir(dir.path().join("pane-recovery"))
            .unwrap()
            .flatten()
            .flat_map(|w| std::fs::read_dir(w.path()).unwrap().flatten())
            .filter_map(|p| std::fs::read_to_string(p.path().join("worker-runs/workers.json")).ok())
            .collect::<Vec<_>>()
    };
    s.until("native worker session checkpointed", |_| {
        ledgers()
            .iter()
            .any(|l| l.contains("99999999-2222-3333-4444-555555555555") && l.contains("Tool"))
    });
    drop(s); // Kill only the owned synthetic TUI/process tree, simulating an interrupted host.
    let mut s = Session::launch(dir.path());
    s.until("worker metadata restored", |s| {
        s.screen().contains("Recovered 1 worker run records")
    });
    assert_eq!(
        s.journal().iter().filter(|e| e["kind"] == "start").count(),
        1
    );
    s.submit("/delegate recover 1");
    s.until("uncertain run inspection requires reconciliation", |s| {
        s.screen().contains("No verified result pointer") && s.screen().contains("RECOVERY_TASK")
    });
    assert_eq!(
        s.journal().iter().filter(|e| e["kind"] == "start").count(),
        1
    );
    assert!(ledgers().iter().any(|l| l.contains("Paused")));
    let registry = || concordia::workspaces::load(dir.path()).unwrap();
    assert_eq!(
        registry().reservations[0].phase,
        concordia::workspaces::Phase::ReconcileRequired
    );
    s.submit("/delegate scope stopped 1");
    s.until("stopped worker acknowledged without replay", |_| {
        registry().reservations[0].phase == concordia::workspaces::Phase::Released
    });
    assert_eq!(
        s.journal().iter().filter(|e| e["kind"] == "start").count(),
        1
    );
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
#[ignore = "isolated cross-provider worker/model/automatic return journey; fake providers only"]
fn cross_provider_worker_model_returns_once_to_main_agent_and_keeps_draft() {
    let _guard = JOURNEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path(), ProviderKind::Claude);
    initialize_git_fixture(
        &dir.path().join("workspace"),
        &dir.path().join("empty-hooks"),
    );
    std::fs::write(
        dir.path().join("profile/.codex/models_cache.json"),
        r#"{"models":[{"slug":"fixture-fast"}]}"#,
    )
    .unwrap();
    let mut s = Session::launch(dir.path());
    s.submit("/workers add codex");
    s.until("worker enrollment", |s| s.screen().contains("worker added"));
    s.submit("/delegate model codex fixture-fast");
    s.until("model configured", |s| {
        s.screen().contains("model: fixture-fast")
    });
    s.submit("/delegate returns auto");
    s.until("automatic returns enabled", |s| {
        s.screen().contains("Worker returns: auto")
    });
    s.submit("/scope set write:main.txt");
    s.until("main scope configured", |s| {
        s.screen().contains("Future main scope")
    });
    s.submit("/delegate scoped codex write:worker.txt -- Implement scoped parser [wait]");
    s.until("native worker running", |s| {
        s.journal()
            .iter()
            .any(|e| e["kind"] == "start" && e["provider"] == "codex")
    });
    s.paste("keep this unsent draft");
    std::fs::write(dir.path().join("gate"), "release fixture").unwrap();
    s.until("main agent receives result", |s| {
        s.journal()
            .iter()
            .any(|e| e["kind"] == "done" && e["provider"] == "claude")
    });
    s.until("draft preserved on return", |s| {
        s.draft() == "keep this unsent draft" && s.screen().contains("Worked for")
    });
    let journal = s.journal();
    let starts: Vec<_> = journal.iter().filter(|e| e["kind"] == "start").collect();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0]["provider"], "codex");
    assert!(starts[0]["prompt"]
        .as_str()
        .unwrap()
        .contains("Write:worker.txt"));
    assert!(starts[1]["prompt"]
        .as_str()
        .unwrap()
        .contains("Write:main.txt"));
    let args = starts[0]["args"].as_array().unwrap();
    assert!(args
        .windows(2)
        .any(|w| w[0] == "-m" && w[1] == "fixture-fast"));
    assert!(!args.iter().any(|v| v == "resume" || v == "--last"));
    assert!(starts[1]["prompt"].as_str().unwrap().contains("UNTRUSTED"));
    assert!(!starts[1]["prompt"]
        .as_str()
        .unwrap()
        .contains("keep this unsent draft"));
    s.send("\x01\x08");
    s.until("draft cleared for explicit worktree command", |s| {
        s.draft().is_empty()
    });
    s.submit("/worktree create HEAD");
    s.until("managed worktree creation verified", |_| {
        concordia::managed_workspaces::load(dir.path()).is_ok_and(|registry| {
            registry.records.len() == 1
                && registry.records[0].phase == concordia::managed_workspaces::Phase::Ready
        })
    });
    let managed = concordia::managed_workspaces::load(dir.path())
        .unwrap()
        .records
        .remove(0);
    let native_sources = dir.path().join("profile/.codex/sessions");
    std::fs::create_dir_all(&native_sources).unwrap();
    std::fs::write(native_sources.join("99999999-2222-3333-4444-555555555555.jsonl"), format!("{}\n{}\n",
        serde_json::json!({"type":"session_meta","payload":{"id":"99999999-2222-3333-4444-555555555555"}}),
        serde_json::json!({"type":"response_item","payload":{"role":"assistant","content":[{"type":"output_text","text":"Verified managed prerequisite source"}]}}))).unwrap();
    std::fs::remove_file(dir.path().join("gate")).unwrap();
    s.submit(&format!(
        "/delegate worktree {} codex Inspect managed workspace [wait]",
        managed.id
    ));
    s.until("managed native worker started", |s| {
        s.journal()
            .iter()
            .filter(|entry| entry["kind"] == "start")
            .count()
            == 3
    });
    let journal = s.journal();
    let started = journal
        .iter()
        .filter(|entry| entry["kind"] == "start")
        .nth(2)
        .unwrap();
    assert_eq!(
        Path::new(started["cwd"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        managed.execution
    );
    s.paste("keep this unsent draft");
    s.release();
    s.until("managed result returned to owner", |s| {
        s.journal()
            .iter()
            .filter(|entry| entry["kind"] == "done")
            .count()
            == 4
    });
    s.until("managed return preserves draft", |s| {
        s.draft() == "keep this unsent draft" && s.screen().contains("Worked for")
    });
    let journal = s.journal();
    let returned = journal
        .iter()
        .filter(|entry| entry["kind"] == "start")
        .nth(3)
        .unwrap();
    assert!(returned["prompt"]
        .as_str()
        .unwrap()
        .contains("delivery does not integrate"));
    assert_eq!(
        Path::new(returned["cwd"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        dir.path().join("workspace").canonicalize().unwrap()
    );
    s.send("\x01\x08");
    s.until("draft cleared for native input configuration", |s| {
        s.draft().is_empty()
    });
    s.submit("/delegate after 2 codex Wait for reviewed managed results");
    s.until(
        "cross-worktree dependency waits for explicit delivery",
        |s| s.screen().contains("integration_required"),
    );
    assert_eq!(
        s.journal()
            .iter()
            .filter(|entry| entry["kind"] == "start")
            .count(),
        4
    );
    s.submit("/delegate delivery 3");
    s.until("exact managed dependency review displayed", |s| {
        s.screen().contains("Accept results only") && s.screen().contains("result-only")
    });
    s.submit(&format!(
        "/delegate delivery 3 result-only {}",
        "0".repeat(64)
    ));
    s.until("stale or wrong delivery receipt rejected", |s| {
        s.screen().contains("obtain a fresh receipt")
    });
    assert_eq!(
        s.journal()
            .iter()
            .filter(|entry| entry["kind"] == "start")
            .count(),
        4
    );
    s.submit("/delegate stop codex");
    s.until("waiting dependency cancelled without execution", |s| {
        s.screen().contains("stopped") || s.screen().contains("cancelled")
    });
    use concordia::managed_workspaces::lifecycle::{self, Action};
    let flag = std::sync::atomic::AtomicBool::new(false);
    for (action, label) in [
        (Action::Integrate, "integrate"),
        (Action::Retire, "retire"),
        (Action::DeleteBranch, "delete-branch"),
    ] {
        s.submit(&format!("/worktree review {} {label}", managed.id));
        s.until("explicit lifecycle review visible", |s| {
            s.screen().contains("Explicit action:")
        });
        let reviewed = lifecycle::preview(
            dir.path(),
            &dir.path().join("workspace"),
            &managed.id,
            action,
            &flag,
        )
        .unwrap();
        assert!(reviewed.blockers.is_empty(), "{reviewed:?}");
        s.submit(&format!(
            "/worktree {label} {} {}",
            managed.id, reviewed.receipt
        ));
        s.until("reviewed lifecycle action completed", |s| {
            s.screen().contains(&format!("Managed {label} finished"))
        });
    }
    assert!(!managed.execution.exists());
    assert_eq!(
        s.journal()
            .iter()
            .filter(|entry| entry["kind"] == "start")
            .count(),
        4,
        "lifecycle operations must not dispatch providers"
    );
    s.submit("/delegate returns manual");
    s.until("manual native result return", |s| {
        s.screen().contains("Worker returns: manual")
    });
    s.submit("/workers add cursor");
    s.until("Cursor worker enrolled", |s| {
        s.screen().contains("worker added") && s.screen().contains("cursor")
    });
    std::fs::write(dir.path().join("workspace/acp-fixture.json"),
          serde_json::json!({"image":false,"permission":true,"permission_title":"Read worker text","reply":"Native worker text received"}).to_string()).unwrap();
    s.submit("/native cursor on");
    s.until("native text preference saved", |s| {
        s.screen().contains("native text input on")
    });
    s.submit("/delegate cursor Review native text");
    s.until("worker waits for exact native input", |s| {
        s.screen().contains("waiting for native input")
    });
    s.paste("keep this native draft");
    s.send("\x1b[1;3B\x1b[19~"); // Select the active worker, then review without replacing the draft.
    s.until("owner-bound native worker review", |s| {
        s.screen().contains("native worker input") && s.screen().contains("Read worker text")
    });
    s.send("1");
    s.send("\x1b");
    s.until("closing review preserves draft and pending request", |s| {
        s.draft() == "keep this native draft" && s.screen().contains("waiting for native input")
    });
    s.send("\x1b[19~");
    s.until("worker review reopened", |s| {
        s.screen().contains("native worker input")
    });
    s.send("\x1b[19~1");
    s.until("worker receives its native decision", |s| {
        s.screen().contains("Native worker text received")
    });
    assert_eq!(s.draft(), "keep this native draft");
    assert_eq!(
        s.journal()
            .iter()
            .filter(|entry| entry["kind"] == "start")
            .count(),
        4,
        "native worker review must not launch a main turn or fall back to print mode"
    );
    let calls: Vec<Value> = std::fs::read_to_string(dir.path().join("workspace/acp-calls.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call["method"] == "session/prompt")
            .count(),
        1
    );
    assert!(calls
        .iter()
        .any(|call| call["id"] == 100 && call["result"]["outcome"]["optionId"] == "yes"));
    s.send("\x01\x08");
    s.until("draft cleared for plan request", |s| s.draft().is_empty());
    std::fs::write(
        dir.path().join("workspace/acp-fixture.json"),
        serde_json::json!({"image":false,"plan":true,"reply":"Worker plan rejected"}).to_string(),
    )
    .unwrap();
    s.submit("/delegate cursor Propose a scoped plan");
    s.until("worker plan pending", |s| {
        s.screen().contains("waiting for native input")
    });
    s.paste("preserve plan draft");
    s.until("plan draft persisted before opening review", |s| {
        s.draft() == "preserve plan draft"
    });
    s.send("\x1b[1;3A\x1b[19~"); // Move from the previous completed worker to the new active worker.
    s.until("full plan visible", |s| {
        s.screen().contains("Edit only the requested function")
    });
    s.send("\x1b[19~2");
    s.until("native plan rejection returned", |s| {
        s.screen().contains("Worker plan rejected")
    });
    assert_eq!(s.draft(), "preserve plan draft");
    let calls = std::fs::read_to_string(dir.path().join("workspace/acp-calls.jsonl")).unwrap();
    assert!(calls
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .any(|call| call["id"] == 100 && call["result"]["outcome"]["outcome"] == "rejected"));
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
