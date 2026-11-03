//! Actual renderer and handlers; isolated fixtures and a fake dispatch channel.
use super::*;
use serde_json::{json, Value};

fn capture(s: &mut Shot, caption: &str, ms: u32, target: Option<Rect>) -> Value {
    s.draw();
    let b = s.term.backend().buffer();
    let cells: Vec<Value> = resolve_cells(b).into_iter().enumerate().flat_map(|(y,row)| {
        row.into_iter().map(move |c| json!({"x":c.col,"y":y,"w":c.width,"text":c.sym,"fg":c.style.fg,"bg":c.bg,"bold":c.style.bold,"italic":c.style.italic,"underline":c.style.underline,"dim":c.style.dim}))
    }).collect();
    let cursor = target
        .map(|r| json!({"x":r.x,"y":r.y,"shape":pointer::shape(&s.app,r.x,r.y),"click":false}));
    json!({"cols":b.area.width,"rows":b.area.height,"duration":ms,"caption":caption,"cursor":cursor,"cells":cells})
}
fn key(s: &mut Shot, code: KeyCode, modifiers: KeyModifiers) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    assert!(!handle_key(&mut s.app, KeyEvent::new(code, modifiers), &tx));
    assert!(rx.try_recv().is_err(), "unexpected model dispatch");
    s.draw();
}
fn click(s: &mut Shot, rect: Rect) {
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        handle_mouse(
            &mut s.app,
            MouseEvent {
                kind,
                column: rect.x,
                row: rect.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        s.draw();
    }
}
fn settle(s: &mut Shot) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        s.app.workspace.poll();
        s.draw();
        if s.app.workspace.file_workers_idle() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
fn clicked(mut frame: Value) -> Value {
    if let Some(c) = frame.get_mut("cursor").filter(|v| v.is_object()) {
        c["click"] = json!(true);
    }
    frame
}
fn text_target(s: &mut Shot, label: &str) -> Rect {
    s.draw();
    let b = s.term.backend().buffer();
    for y in 0..b.area.height {
        let row: String = (0..b.area.width).map(|x| b[(x, y)].symbol()).collect();
        if let Some(x) = row.find(label) {
            return Rect::new(x as u16, y, label.len() as u16, 1);
        }
    }
    panic!("demo target absent: {label}");
}
fn files() -> Value {
    let mut s = Shot::new(104, 19);
    s.app.restore_state();
    s.app.messages.clear();
    s.app.account_email = None;
    s.app.model = "model-a".into();
    s.app.usage_summary.clear();
    let draft = "Next: check the recovery tests.";
    let path = s._dir.path().join("notes.md");
    std::fs::write(&path,"# Release review\n\n| Check | Status |\n| --- | --- |\n| File recovery | Pending |\n| Native logs | Preserved |\n\nKeep the original sources.\n").unwrap();
    s.app.push_user("Review the release checklist.".into());
    s.app.push_assistant("The [release checklist](notes.md) is ready.\n\nUpdate the recovery status after checking the tests.".into());
    s.app.editor.set_text(draft);
    s.draw();
    let messages = s.app.messages.len();
    let (i, row) = s
        .app
        .view
        .rows
        .iter()
        .enumerate()
        .find(|(_, r)| !r.links.is_empty())
        .unwrap();
    let target = Rect::new(
        s.app.areas.transcript.x
            + row.gutter.chars().count() as u16
            + text::slice_width(&row.text[..row.links[0].0]) as u16,
        s.app.areas.transcript.y + (i - s.app.view.visible().start) as u16,
        17,
        1,
    );
    let mut frames = vec![capture(
        &mut s,
        "One click. The file opens beside your conversation.",
        1800,
        Some(target),
    )];
    click(&mut s, target);
    settle(&mut s);
    frames.push(clicked(capture(
        &mut s,
        "Markdown opens formatted. Your unsent message stays put.",
        2200,
        Some(target),
    )));
    let bar = Rect::new(
        s.app.workspace.panes[0].area.x + 4,
        s.app.workspace.panes[0].area.y + 1,
        6,
        1,
    );
    frames.push(capture(
        &mut s,
        "Click Raw to edit the source.",
        1200,
        Some(bar),
    ));
    click(&mut s, bar);
    frames.push(clicked(capture(
        &mut s,
        "Raw mode owns typing. Chat keeps its own draft.",
        1000,
        Some(bar),
    )));
    key(&mut s, KeyCode::Home, KeyModifiers::CONTROL);
    for _ in 0..4 {
        key(&mut s, KeyCode::Down, KeyModifiers::NONE);
    }
    for _ in 0..18 {
        key(&mut s, KeyCode::Right, KeyModifiers::NONE);
    }
    for _ in 0..7 {
        key(&mut s, KeyCode::Right, KeyModifiers::SHIFT);
    }
    frames.push(capture(
        &mut s,
        "Change one status: Pending → Passed.",
        1000,
        None,
    ));
    for c in "Passed".chars() {
        key(&mut s, KeyCode::Char(c), KeyModifiers::NONE);
    }
    assert!(s.app.workspace.has_unsaved());
    frames.push(capture(
        &mut s,
        "The star marks unsaved changes. Ctrl+S saves the file.",
        1500,
        None,
    ));
    key(&mut s, KeyCode::Char('s'), KeyModifiers::CONTROL);
    settle(&mut s);
    assert!(!s.app.workspace.has_unsaved());
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("| File recovery | Passed |"));
    frames.push(capture(
        &mut s,
        "Saved to disk. Click Formatted to review the result.",
        1400,
        Some(bar),
    ));
    click(&mut s, bar);
    frames.push(clicked(capture(
        &mut s,
        "Updated table. Same conversation. Same unsent draft.",
        2600,
        Some(bar),
    )));
    assert_eq!(s.app.editor.text(), draft);
    assert_eq!(s.app.messages.len(), messages);
    for frame in &mut frames[3..7] {
        frame["zoom"] = json!([1.6, 0.85, 0.35]);
    }
    json!({"name":"concordia-file-preview","title":"Review → edit → save. Stay in the conversation.","synthetic":true,"frames":frames})
}
fn continuity() -> Value {
    let mut s = Shot::new(104, 25);
    s.app.restore_state();
    s.app.messages.clear();
    s.app.model = "model-a".into();
    s.app.account_email = None;
    s.app.usage_summary.clear();
    for id in ["codex-3", "backup"] {
        let home = s._dir.path().join(id);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("auth.json"), "{}").unwrap(); // Synthetic presence marker, no credential.
        crate::store::upsert_stored_account(
            s._dir.path(),
            crate::store::StoredAccount {
                id: id.into(),
                provider: ProviderKind::Codex,
                home_path: Some(home.to_string_lossy().into_owned()),
                label: if id == "backup" {
                    "Backup account".into()
                } else {
                    "Primary account".into()
                },
                created_at: "0".into(),
            },
        )
        .unwrap();
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    s.app.run_line(
        "Finish the recovery checks. Preserve native logs.".into(),
        &tx,
    );
    assert!(rx
        .try_recv()
        .unwrap()
        .contains("Finish the recovery checks"));
    s.app.phase = Phase::Thinking;
    s.app.push_card(StreamCard {
        id: "check".into(),
        title: "shell".into(),
        detail: Some("command: cargo test recovery\nOutput: 18 passed; 0 failed".into()),
        status: CardStatus::Done,
    });
    s.app
        .push_assistant("The recovery checks pass. Next: review restart behavior.".into());
    let mut frames = vec![capture(
        &mut s,
        "The task has history, decisions and completed checks.",
        2200,
        None,
    )];
    s.app
        .queue
        .items
        .push("Then review source freshness.".into());
    s.app
        .apply_progress(TurnProgress::Error("You've hit your usage limit".into()));
    s.app.on_turn_done(
        TurnOutcome {
            exit_code: Some(1),
            ..Default::default()
        },
        &tx,
    );
    assert!(rx.try_recv().is_err());
    assert!(s
        .app
        .picker
        .as_ref()
        .is_some_and(|p| p.kind == PickerKind::Continuation));
    frames.push(capture(
        &mut s,
        "Simulated limit: Concordia pauses the request and the queue.",
        2400,
        None,
    ));
    let account_target = text_target(&mut s, "Backup account");
    frames.push(capture(
        &mut s,
        "Choose the backup account. Press Enter to continue.",
        1600,
        Some(account_target),
    ));
    assert!(!handle_key(
        &mut s.app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &tx
    ));
    let instruction = rx
        .try_recv()
        .expect("handoff dispatches once into the fake channel");
    assert!(instruction
        .contains("Original request:\nFinish the recovery checks. Preserve native logs."));
    assert_eq!(s.app.account, "backup");
    assert!(s.app.queue.paused);
    frames.push(capture(
        &mut s,
        "The original request moves to the backup. No retyping.",
        2100,
        None,
    ));
    s.app.on_turn_done(TurnOutcome {exit_code:Some(0),text:"Restart review complete. Unsaved edits are retained; changed files are not overwritten.".into(),..Default::default()},&tx);
    assert!(rx.try_recv().is_err());
    frames.push(capture(
        &mut s,
        "Same thread. Earlier checks remain; queued work stays paused.",
        2900,
        None,
    ));
    for frame in &mut frames[1..3] {
        frame["zoom"] = json!([1.45, 0.5, 0.65]);
    }
    json!({"name":"concordia-command-controls","title":"Hit a limit. Keep the task.","synthetic":true,"frames":frames})
}
#[test]
#[ignore = "deterministic GIF frames; synthetic events, no provider processes"]
fn write_gif_frames() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gif-frames");
    std::fs::create_dir_all(&dir).unwrap();
    for scene in [files(), continuity()] {
        std::fs::write(
            dir.join(format!("{}.json", scene["name"].as_str().unwrap())),
            serde_json::to_vec(&scene).unwrap(),
        )
        .unwrap();
    }
}
