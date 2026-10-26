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
        serde_json::json!({"type":"session_meta","payload":{"id":"target-session"}}).to_string(),
    ];
    for i in 0..80 {
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
    assert!(app
        .window_title()
        .starts_with("Fix the project graph refresh"));
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
    app.apply_picker_selection();
    assert_eq!(app.activity.detail, Some(1));
    assert!(rx.try_recv().is_err());
    assert!(app.handle_slash("/new"));
    assert_eq!(app.native_agents.count(), 0);
}
