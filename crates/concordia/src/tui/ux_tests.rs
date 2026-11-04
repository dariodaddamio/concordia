use super::*;
use ratatui::backend::TestBackend;

#[test]
fn mermaid_is_rendered_inside_chat_and_completion_metrics_have_no_link_targets() {
    let mut ui = Ui::new();
    ui.app
        .push_user("First paragraph\n\nSecond paragraph".into());
    ui.app
        .push_assistant("```mermaid\nflowchart LR\n A[Prompt] --> B[Provider]\n```".into());
    ui.app.messages.push(ChatLine {
        role: MsgRole::Meta,
        text: "Worked for 8.2s · stopped Nov 1, 10:12pm · in 12.3k - out 1.4k".into(),
        card: None,
        meta: None,
    });
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        let screen = ui.draw();
        if screen.contains("Prompt")
            && screen.contains("Provider")
            && !screen.contains("flowchart LR")
            && !screen.contains("Rendering Mermaid")
        {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    let metadata = ui.app.messages.len() - 1;
    assert!(ui
        .app
        .view
        .rows
        .iter()
        .filter(|r| r.identity == Some(metadata))
        .all(|r| r.links.is_empty()
            && r.web_links.is_empty()
            && r.runs
                .iter()
                .all(|(_, _, s)| !s.add_modifier.contains(Modifier::UNDERLINED))));
    for (row, index) in ui.app.view.visible().enumerate() {
        if ui.app.view.rows[index]
            .identity
            .and_then(|i| ui.app.messages.get(i))
            .is_some_and(|m| m.role == MsgRole::User)
        {
            let area = ui.app.areas.transcript;
            if row + 1 < area.height as usize {
                assert_eq!(
                    ui.terminal.backend().buffer()[(area.x + area.width - 2, area.y + row as u16)]
                        .bg,
                    Color::Indexed(235)
                );
            }
        }
    }
}

struct Ui {
    app: App,
    terminal: Terminal<TestBackend>,
    _dir: tempfile::TempDir,
    tx: mpsc::UnboundedSender<String>,
    rx: mpsc::UnboundedReceiver<String>,
}
impl Ui {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        for path in ["workspace", "state", "home"] {
            std::fs::create_dir(dir.path().join(path)).unwrap();
        }
        let mut app = App::new(dir.path().join("state"), dir.path().join("workspace"));
        app.home_path = Some(dir.path().join("home"));
        app.no_color = false;
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            app,
            terminal: Terminal::new(TestBackend::new(120, 32)).unwrap(),
            _dir: dir,
            tx,
            rx,
        }
    }
    fn draw(&mut self) -> String {
        self.terminal.draw(|f| draw_ui(f, &mut self.app)).unwrap();
        self.terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }
    fn key(&mut self, code: KeyCode, mods: KeyModifiers) {
        assert!(!handle_key(
            &mut self.app,
            KeyEvent::new(code, mods),
            &self.tx
        ));
    }
    fn click(&mut self, rect: Rect) {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            handle_mouse(
                &mut self.app,
                MouseEvent {
                    kind,
                    column: rect.x,
                    row: rect.y,
                    modifiers: KeyModifiers::NONE,
                },
            );
        }
    }
    fn target(&mut self, text: &str) -> Rect {
        self.draw();
        let b = self.terminal.backend().buffer();
        for y in 0..b.area.height {
            let row: String = (0..b.area.width).map(|x| b[(x, y)].symbol()).collect();
            if let Some(x) = row.find(text) {
                return Rect::new(x as u16, y, text.len() as u16, 1);
            }
        }
        panic!("missing target {text}");
    }
    fn files(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.app.request_related_files();
            self.app.related_files.poll();
            self.draw();
            if !self.app.related_files.loading() && !self.app.related_files.entries.is_empty() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
#[test]
fn conversational_choices_never_take_a_typed_instruction_from_the_composer() {
    for explicit_focus in [false, true] {
        let mut ui = Ui::new();
        ui.app.run_line("Review the change".into(), &ui.tx);
        ui.rx.try_recv().unwrap();
        ui.app.on_turn_done(
            TurnOutcome {
                exit_code: Some(0),
                text: "Which approach?\n1. Small patch\n2. Full refactor".into(),
                ..Default::default()
            },
            &ui.tx,
        );
        assert!(!ui.app.questions.focused);
        if explicit_focus {
            ui.key(KeyCode::F(8), KeyModifiers::NONE);
            // Return from the choices to ordinary typing without choosing or clearing work.
            ui.key(KeyCode::Esc, KeyModifiers::NONE);
        }
        let instruction =
            "2 tasks: commit and push the features for review; test everything while I sleep.";
        for c in instruction.chars() {
            ui.key(KeyCode::Char(c), KeyModifiers::NONE);
        }
        assert_eq!(ui.app.editor.text(), instruction);
        assert!(ui.rx.try_recv().is_err());
        ui.key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(ui.rx.try_recv().unwrap(), instruction);
        assert!(ui.rx.try_recv().is_err());
        assert!(!ui.app.questions.waiting());
    }
}

#[test]
fn explicitly_focused_reply_text_never_sends_the_highlighted_option() {
    let mut ui = Ui::new();
    let text = "Which approach?\n1. Small patch\n2. Full refactor";
    ui.app.push_assistant(text.into());
    ui.app.questions.offer(text);
    ui.app.editor.set_text("separate composer draft");
    ui.draw();
    ui.key(KeyCode::F(8), KeyModifiers::NONE);
    ui.key(KeyCode::Char('2'), KeyModifiers::NONE);
    ui.key(KeyCode::Char('x'), KeyModifiers::NONE);
    ui.key(KeyCode::Enter, KeyModifiers::SHIFT);
    assert!(questions::paste(
        &mut ui.app,
        "  commit\r\n\r\n  push\rtest  "
    ));
    assert!(ui.rx.try_recv().is_err());
    ui.key(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(ui.app.editor.text(), "separate composer draft");
    ui.key(KeyCode::F(8), KeyModifiers::NONE);
    ui.key(KeyCode::Enter, KeyModifiers::NONE);
    ui.app.queue_advance(&ui.tx);
    assert_eq!(ui.rx.try_recv().unwrap(), "2x\n  commit\n\n  push\ntest  ");
    assert!(ui.rx.try_recv().is_err());
    assert_eq!(ui.app.editor.text(), "separate composer draft");
}

#[test]
fn a_choice_panel_keeps_the_full_reply_and_long_options_in_the_transcript() {
    let mut ui = Ui::new();
    let option = "Read and test the complete change before committing it. ".repeat(5);
    let reply = format!("Which approach?\n1. {option}\n2. Defer");
    let message_index = ui.app.messages.len();
    ui.app.push_assistant(reply.clone());
    ui.app.questions.offer(&reply);
    ui.draw();
    let transcript: String = ui
        .app
        .view
        .rows
        .iter()
        .filter(|r| r.identity == Some(message_index))
        .flat_map(|r| r.text.iter())
        .collect();
    assert!(transcript.contains("Which approach?"));
    assert_eq!(transcript.matches("Read and test").count(), 5);
    assert!(!transcript.contains("Choose a reply below"));
}

#[test]
fn question_notes_can_start_with_digits_and_send_alone_or_with_a_choice() {
    for note_only in [true, false] {
        let mut ui = Ui::new();
        ui.app.editor.set_text("unsent draft");
        let text = "Which approach?\n1. **Small patch**\n2. Full refactor";
        ui.app.push_assistant(text.into());
        ui.app.questions.offer(text);
        let screen = ui.draw();
        assert!(screen.contains("1. Small patch"));
        assert!(!screen.contains("**Small patch**"));
        let note_row = ui.target("Add a note:");
        ui.click(note_row);
        ui.key(KeyCode::Char('2'), KeyModifiers::NONE);
        assert!(ui.rx.try_recv().is_err());
        assert!(questions::paste(&mut ui.app, " iterations\nplease"));
        assert!(ui.draw().contains("2 iterations ↵ please"));
        if !note_only {
            ui.key(KeyCode::Up, KeyModifiers::NONE);
        }
        ui.key(KeyCode::Enter, KeyModifiers::NONE);
        ui.app.queue_advance(&ui.tx);
        let sent = ui.rx.try_recv().unwrap();
        assert!(sent.contains("2 iterations\nplease"));
        if note_only {
            assert_eq!(sent, "2 iterations\nplease");
        }
        assert_eq!(sent.contains("Full refactor"), !note_only);
        assert!(!sent.contains("Small patch"));
        assert_eq!(ui.app.editor.text(), "unsent draft");
        assert!(ui.rx.try_recv().is_err());
    }
}

#[test]
fn questions_are_selectable_preserve_drafts_and_pause_automatic_queued_work() {
    let mut ui = Ui::new();
    ui.app.native_session_id = Some("exact-native-session".into());
    ui.app.run_line("Review the change".into(), &ui.tx);
    ui.rx.try_recv().unwrap();
    ui.app.editor.set_text("unsent draft");
    ui.app.queue.items.push("later task".into());
    ui.app.on_turn_done(
        TurnOutcome {
            exit_code: Some(0),
            text: "Which approach do you prefer?\n1. Small patch\n2. Full refactor".into(),
            ..Default::default()
        },
        &ui.tx,
    );
    assert!(ui.rx.try_recv().is_err());
    assert!(ui.app.queue.paused);
    assert!(ui.draw().contains("2. Full refactor"));
    ui.key(KeyCode::F(8), KeyModifiers::NONE);
    ui.key(KeyCode::Down, KeyModifiers::NONE);
    ui.key(KeyCode::Enter, KeyModifiers::NONE);
    ui.app.queue_advance(&ui.tx);
    let sent = ui.rx.try_recv().unwrap();
    assert!(sent.contains("Full refactor"));
    assert_eq!(ui.app.editor.text(), "unsent draft");
    assert_eq!(
        ui.app.native_session_id.as_deref(),
        Some("exact-native-session")
    );
    assert_eq!(ui.app.queue.items, ["later task"]);
    assert!(ui.app.queue.paused);
    ui.app.on_turn_done(
        TurnOutcome {
            exit_code: Some(0),
            text: "Done".into(),
            ..Default::default()
        },
        &ui.tx,
    );
    assert!(ui.rx.try_recv().is_err());
}
#[test]
fn mouse_answers_and_force_send_wait_for_cancellation_and_dispatch_once() {
    let mut ui = Ui::new();
    ui.app.run_line("active task".into(), &ui.tx);
    ui.rx.try_recv().unwrap();
    ui.app.editor.set_text("draft stays here");
    ui.app.queue.items = vec!["first".into(), "second".into()];
    ui.draw();
    let rect = ui.app.queue.send_targets[1].0;
    ui.click(rect);
    assert!(ui.app.cancel_requested);
    ui.app.queue_advance(&ui.tx);
    assert!(ui.rx.try_recv().is_err());
    ui.app.on_turn_done(
        TurnOutcome {
            cancelled: true,
            exit_code: Some(1),
            ..Default::default()
        },
        &ui.tx,
    );
    assert_eq!(ui.rx.try_recv().unwrap(), "second");
    assert!(ui.rx.try_recv().is_err());
    assert_eq!(ui.app.queue.items, ["first"]);
    assert_eq!(ui.app.editor.text(), "draft stays here");
    ui.app.on_turn_done(
        TurnOutcome {
            exit_code: Some(0),
            text: "Which now?\n- Keep it\n- Change it".into(),
            ..Default::default()
        },
        &ui.tx,
    );
    ui.draw();
    // Mouse targets come from real question layout; no selection means no dispatch.
    assert!(ui.app.questions.waiting());
    let rect = ui.target("2. Change it");
    ui.click(rect);
    ui.app.queue_advance(&ui.tx);
    assert!(ui.rx.try_recv().unwrap().contains("Change it"));
    assert!(ui.rx.try_recv().is_err());
    assert_eq!(ui.app.editor.text(), "draft stays here");
}
#[test]
fn workspace_completion_handles_paths_without_chat_mentions_and_recents_survive_restart() {
    let mut ui = Ui::new();
    let cwd = ui.app.cwd.clone();
    std::fs::write(cwd.join("Cargo.toml"), "[workspace]").unwrap();
    std::fs::create_dir(cwd.join("src")).unwrap();
    std::fs::create_dir(cwd.join("target")).unwrap();
    std::fs::write(cwd.join("src/code.c"), "int main(void) { return 0; }").unwrap();
    std::fs::write(cwd.join("target/hidden.c"), "generated").unwrap();
    std::fs::write(cwd.join(".env"), "synthetic private marker").unwrap();
    std::fs::write(cwd.join("notes with spaces.md"), "# Notes").unwrap();
    ui.app.editor.set_text("/open src/co");
    ui.files();
    assert!(ui.draw().contains("src/code.c"));
    assert!(!ui
        .app
        .related_files
        .entries
        .iter()
        .any(|e| e.label.contains("hidden.c") || e.label == ".env"));
    ui.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(ui.app.workspace.panes.len(), 1);
    assert!(ui.app.editor.text().is_empty());
    assert!(ui.rx.try_recv().is_err());
    ui.app.workspace.focused = None;
    ui.app.editor.set_text("review @notes");
    ui.files();
    ui.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(ui.app.editor.text(), "review @`notes with spaces.md` ");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        ui.app.related_files.poll();
        if related_files::State::new(&ui.app.state_root, &cwd)
            .recent
            .iter()
            .any(|path| path.ends_with("src/code.c"))
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let reopened = App::new(ui.app.state_root.clone(), cwd.clone());
    assert!(reopened
        .related_files
        .recent
        .iter()
        .any(|p| p.ends_with("src/code.c")));
    let absolute = cwd
        .join("notes with spaces.md")
        .to_string_lossy()
        .replace('\\', "/");
    ui.app.editor.set_text(&format!("/open \"{absolute}\""));
    ui.files();
    ui.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(ui.app.workspace.panes.len(), 2);
    assert!(ui.rx.try_recv().is_err());
}
#[test]
fn run_status_sits_below_transcript_and_user_rows_have_background_and_completion_time() {
    let mut ui = Ui::new();
    ui.app.push_user("USER_MESSAGE".into());
    ui.app.busy = true;
    ui.app.turn_started = Some(Instant::now() - Duration::from_secs(12));
    ui.draw();
    assert_eq!(ui.app.areas.bottom_button.width, 0);
    let rect = ui.app.areas.run_cancel;
    assert!(rect.width > 0);
    assert!(rect.y < ui.app.areas.input.y);
    assert!(!ui.terminal.backend().buffer()[(rect.x + 2, rect.y)]
        .modifier
        .contains(Modifier::BOLD));
    assert!(ui
        .terminal
        .backend()
        .buffer()
        .content
        .iter()
        .any(|c| c.bg == Color::Indexed(235)));
    ui.app.busy = false;
    ui.app.finalize_turn(TurnOutcome {
        exit_code: Some(0),
        text: "Answer".into(),
        ..Default::default()
    });
    let screen = ui.draw();
    assert!(screen.contains("Worked for 12s") && screen.contains("stopped"));
    for _ in 0..50 {
        ui.app.push_assistant("Scrollback".into());
    }
    ui.draw();
    ui.app.view.scroll_up(10);
    ui.draw();
    assert!(ui.app.areas.bottom_button.width > 0);
}

#[test]
fn project_search_owns_keys_and_pastes_without_submitting_or_changing_draft() {
    let mut ui = Ui::new();
    ui.app.editor.set_text("unfinished chat draft");
    ui.app.busy = true;
    ui.app.open_project_search();
    assert!(ui.app.project_search.active());
    ui.app.paste_text("needle");
    clipboard::set_text("clipboard").unwrap();
    handle_key(
        &mut ui.app,
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL),
        &ui.tx,
    );
    assert!(ui.draw().contains("needleclipboard"));
    handle_key(
        &mut ui.app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &ui.tx,
    );
    let _ = ui.draw();
    assert_eq!(ui.app.editor.text(), "unfinished chat draft");
    assert!(ui.rx.try_recv().is_err());
    handle_key(
        &mut ui.app,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &ui.tx,
    );
    assert!(!ui.app.project_search.active());
    assert_eq!(ui.app.editor.text(), "unfinished chat draft");
    assert!(ui.app.busy);
}
