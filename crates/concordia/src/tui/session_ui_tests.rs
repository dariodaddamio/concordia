use super::*;
use ratatui::backend::TestBackend;

fn setup() -> (App, tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("native-home");
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    crate::store::upsert_stored_account(
        dir.path(),
        crate::store::StoredAccount {
            id: "synthetic".into(),
            provider: ProviderKind::Codex,
            home_path: Some(home.to_string_lossy().into_owned()),
            label: "Synthetic".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    let mut events = vec![
        serde_json::json!({"type":"session_meta","payload":{"id":"target-session","cwd":dir.path()}}).to_string(),
    ];
    for i in 0..80 {
        if i == 79 {
            events.push(serde_json::json!({"type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":format!("Provider reported inspection {}", "y".repeat(600))}],"encrypted_content":"unreadable-private-data"}}).to_string());
            events.push(serde_json::json!({"type":"response_item","payload":{"type":"function_call","call_id":"restored-native-tool","name":"shell_command","arguments":"{\"command\":\"git status\"}"}}).to_string());
            events.push(serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"restored-native-tool","output":"working tree clean"}}).to_string());
        }
        events.push(serde_json::json!({"type":"response_item","payload":{"type":"message","role":if i%2==0 {"user"} else {"assistant"},"content":[{"type":"output_text","text":format!("message-{i}: synthetic history")}]}}).to_string());
    }
    std::fs::write(home.join("sessions/target.jsonl"), events.join("\n") + "\n").unwrap();
    (App::new(dir.path().into(), dir.path().into()), dir, home)
}

fn finish(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.session_ui.loading() {
        app.poll_session_history();
        assert!(Instant::now() < deadline, "history failed to load");
        std::thread::yield_now();
    }
}

#[test]
fn explicit_resume_across_accounts_loads_scrollable_history_and_keeps_unsent_draft() {
    let (mut app, _dir, home) = setup();
    app.provider = ProviderKind::Claude;
    app.account = "previous-account".into();
    app.home_path = Some(PathBuf::from("previous-home"));
    app.editor.set_text("unsent draft");
    app.usage_block = Some(0);
    app.resume_session("synthetic", "target-session");
    assert_eq!(app.native_session_id.as_deref(), Some("target-session"));
    assert_eq!(app.provider, ProviderKind::Codex);
    assert_eq!(app.home_path.as_ref(), Some(&home));
    assert!(!app.need_context_pack);
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.submit_input(&tx);
    assert!(
        rx.try_recv().is_err(),
        "loading must not dispatch a provider"
    );
    finish(&mut app);
    assert_eq!(app.editor.text(), "unsent draft");
    assert!(app.usage_block.is_none());
    let (index, card) = app
        .messages
        .iter()
        .enumerate()
        .find_map(|(index, message)| message.card.as_ref().map(|card| (index, card)))
        .unwrap();
    assert!(app.messages[index].role == MsgRole::Card);
    assert_eq!(card.id, "restored-native-tool");
    assert_eq!(card.status, CardStatus::Done);
    assert!(card.detail.as_deref().unwrap().contains("git status"));
    assert!(card
        .detail
        .as_deref()
        .unwrap()
        .contains("working tree clean"));
    assert!(app
        .messages
        .iter()
        .any(|message| message.role == MsgRole::Thinking
            && message.text.starts_with("Provider reported inspection")));
    assert!(
        !app.messages.iter().any(|message| {
            message.role == MsgRole::Meta && message.text.contains("Review activity")
        }),
        "history restores provider activity without adding synthetic completion rows"
    );
    assert!(!app.messages.iter().any(|message| {
        message.text.contains("unreadable-private-data")
            || message
                .card
                .as_ref()
                .and_then(|card| card.detail.as_ref())
                .is_some_and(|detail| detail.contains("unreadable-private-data"))
    }));
    assert_eq!(
        app.messages
            .iter()
            .filter(|m| m.role == MsgRole::User)
            .count(),
        40
    );
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|f| draw_ui(f, &mut app)).unwrap();
    let screen = |terminal: &Terminal<TestBackend>| {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
    };
    assert!(screen(&terminal).contains("message-79"));
    assert!(screen(&terminal).contains("Thinking - Expand"));
    let thought_index = app
        .messages
        .iter()
        .position(|m| m.role == MsgRole::Thinking)
        .unwrap();
    let thought_row = app
        .view
        .rows
        .iter()
        .position(|row| row.identity == Some(thought_index) && row.source_offset == 0)
        .unwrap();
    let area = app.areas.transcript;
    let thought_y = area.y + (thought_row - app.view.visible().start) as u16;
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        handle_mouse(
            &mut app,
            MouseEvent {
                kind,
                column: area.x + 4,
                row: thought_y,
                modifiers: KeyModifiers::NONE,
            },
        );
    }
    terminal.draw(|f| draw_ui(f, &mut app)).unwrap();
    assert!(screen(&terminal).contains("Thinking - Collapse"));
    assert!(screen(&terminal).contains("Provider reported inspection"));
    assert!(
        app.activity.detail.is_none(),
        "restored thinking expands inside history"
    );
    assert_eq!(app.editor.text(), "unsent draft");
    assert!(rx.try_recv().is_err());
    let old = app.view.visible().start;
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE),
        &tx,
    );
    terminal.draw(|f| draw_ui(f, &mut app)).unwrap();
    assert!(app.view.visible().start < old);
    assert!(!screen(&terminal).contains("message-79"));
    assert!(rx.try_recv().is_err());
    assert!(app.window_title().contains("message-0"));
}

#[test]
fn resume_of_session_without_task_record_works() {
    let (mut app, _dir, home) = setup();
    crate::tui::landing::open_home_tab(&mut app);
    select_account(&app.state_root, "synthetic").unwrap();
    set_model(&app.state_root, "chosen-small-model").unwrap();
    app.open_resume_picker();
    let picker = app.picker.as_ref().expect("resume picker");
    assert_eq!(picker.rows.len(), 1);
    assert_eq!(picker.rows[0].id, "resume:synthetic:target-session");
    assert!(crate::store::list_tasks(&app.state_root).is_empty());

    let (tx, mut rx) = mpsc::unbounded_channel();
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &tx,
    );
    assert!(
        rx.try_recv().is_err(),
        "resume reads history without a model turn"
    );

    assert!(
        app.native_session_id.is_none(),
        "selection must wait for window-level ownership routing"
    );
    super::run::inspect_requested_resume(app, |app| {
        assert_eq!(app.native_session_id.as_deref(), Some("target-session"));
        assert_eq!(app.account, "synthetic");
        assert_eq!(app.home_path.as_ref(), Some(&home));
        assert!(app.picker.is_none());
        assert!(app.task_id.is_none());
        finish(app);
        assert!(!crate::tui::landing::visible(app));
        assert_eq!(app.model, "chosen-small-model");
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal.draw(|f| draw_ui(f, app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(
            text.contains("message-79"),
            "resumed history must replace Home"
        );
    });
}

#[test]
fn selecting_an_account_defers_context_until_submission() {
    let (mut app, _dir, _) = setup();
    app.provider = ProviderKind::Claude;
    app.account = "previous-account".into();
    app.native_session_id = Some("previous-session".into());
    app.push_user("previous task".into());
    app.push_system("previous local notice");
    app.editor.set_text("unsent followup");
    app.show_picker(
        PickerKind::Use,
        "account".into(),
        vec![PickerRow {
            id: "use:synthetic".into(),
            label: "synthetic".into(),
            detail: String::new(),
        }],
        false,
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.apply_picker_selection(Some(&tx));
    assert_eq!(app.provider, ProviderKind::Codex);
    assert_eq!(app.editor.text(), "unsent followup");
    assert!(app.native_session_id.is_none());
    assert!(app.need_context_pack);
    assert!(!app.messages.iter().any(ChatLine::is_map));
    assert!(
        rx.try_recv().is_err(),
        "an account switch must not dispatch a model turn"
    );
}

#[test]
fn home_model_selection_is_saved_without_sending_a_turn_or_map() {
    let (mut app, _dir, _) = setup();
    select_account(&app.state_root, "synthetic").unwrap();
    app.refresh_binding();
    crate::tui::landing::open_home_tab(&mut app);
    app.show_picker(
        PickerKind::Model,
        "model".into(),
        vec![PickerRow {
            id: "model:chosen-small-model".into(),
            label: "chosen-small-model".into(),
            detail: String::new(),
        }],
        true,
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &tx,
    );
    assert!(app.picker.is_none());
    assert_eq!(app.model, "chosen-small-model");
    assert_eq!(
        get_use_selection(&app.state_root).unwrap().model.as_deref(),
        Some("chosen-small-model")
    );
    assert!(crate::tui::landing::visible(&app));
    assert!(!app.messages.iter().any(ChatLine::is_map));
    assert!(rx.try_recv().is_err());
}

#[test]
fn a_new_conversation_rejects_late_history_and_resets_its_title() {
    let (mut app, _dir, _) = setup();
    app.resume_session("synthetic", "target-session");
    app.handle_slash("/new");
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.session_ui.reading() {
        app.poll_session_history();
        std::thread::yield_now();
        assert!(Instant::now() < deadline);
    }
    assert!(app.native_session_id.is_none());
    assert!(app.session_ui.title.is_none());
    assert!(app
        .messages
        .iter()
        .all(|m| !m.text.contains("synthetic history")));
}

#[test]
fn titles_are_generated_without_thread_logging_and_survive_native_binding() {
    let (mut app, dir, home) = setup();
    app.provider = ProviderKind::Codex;
    app.account = "synthetic".into();
    app.home_path = Some(home.clone());
    app.thread = None;
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.editor.set_text("Fix the project graph refresh");
    app.submit_input(&tx);
    rx.try_recv().unwrap();
    app.apply_progress(TurnProgress::Session("new-session".into()));
    assert_eq!(
        crate::session_titles::get(
            dir.path(),
            ProviderKind::Codex,
            "synthetic",
            &home,
            "new-session"
        )
        .as_deref(),
        Some("Fix the project graph refresh")
    );
    assert!(app.window_title().contains("Fix the project graph refresh"));
}

#[test]
fn native_agents_are_visible_and_open_full_report_without_dispatching_a_turn() {
    let (mut app, _dir, _) = setup();
    app.push_card(StreamCard {
        id: "native-agent:child-1".into(),
        title: "agent child-1".into(),
        detail: Some("task: inspect parser\nreported result: completed review".into()),
        status: CardStatus::Done,
    });
    let (tx, mut rx) = mpsc::unbounded_channel();
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
        &tx,
    );
    assert!(app
        .picker
        .as_ref()
        .is_some_and(|p| p.kind == PickerKind::Agents));
    app.push_card(StreamCard {
        id: "native-agent:child-1".into(),
        title: "agent child-1".into(),
        detail: Some("reported result: changed while picker open".into()),
        status: CardStatus::Error,
    });
    assert!(app.picker.as_ref().unwrap().rows[0]
        .label
        .contains("failed/interrupted"));
    app.apply_picker_selection(None);
    assert_eq!(app.activity.detail, Some(1));
    assert!(rx.try_recv().is_err());
    let launch = StreamCard {
        id: "toolu-background".into(),
        title: "Agent".into(),
        detail: Some("Background launch accepted".into()),
        status: CardStatus::Done,
    };
    app.push_card(launch.clone());
    let child_id = format!(
        "native-agent:claude:{}",
        serde_json::json!(["fixture-connection", "child-background", "toolu-background"])
    );
    app.push_card(StreamCard {
        id: child_id.clone(),
        title: "agent child-background".into(),
        detail: Some("Task: inspect the parser\nNative work is running".into()),
        status: CardStatus::Running,
    });
    app.push_card(launch);
    assert_eq!(
        app.native_agents.count(),
        2,
        "a launch receipt is not a completed child"
    );
    app.open_agents_picker();
    assert!(app
        .picker
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|row| { row.label.contains("child-background") && row.label.contains("running") }));
    app.push_card(StreamCard {
        id: child_id,
        title: "agent child-background".into(),
        detail: Some("Task: inspect the parser\nNative result: parser verified".into()),
        status: CardStatus::Done,
    });
    assert!(app
        .picker
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|row| { row.label.contains("child-background") && row.label.contains("finished") }));
    assert!(app.handle_slash("/new"));
    assert_eq!(app.native_agents.count(), 0);
}
