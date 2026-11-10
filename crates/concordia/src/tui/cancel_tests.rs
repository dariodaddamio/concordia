use super::*;

fn app() -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
    (app, dir)
}

#[test]
fn cancelled_turn_preserves_a_new_draft() {
    let (mut app, _dir) = app();
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.editor.set_text("submitted prompt");
    app.submit_input(&tx);
    assert_eq!(rx.try_recv().unwrap(), "submitted prompt");

    app.editor.set_text("new unsent draft");
    app.request_cancel();
    app.finalize_turn(TurnOutcome {
        cancelled: true,
        ..Default::default()
    });

    assert_eq!(app.editor.text(), "new unsent draft");
    assert!(rx.try_recv().is_err(), "restoring draft must not submit it");
}

#[test]
fn cancelled_turn_leaves_empty_editor_empty() {
    let (mut app, _dir) = app();
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.editor.set_text("retry this prompt");
    app.submit_input(&tx);
    assert_eq!(rx.try_recv().unwrap(), "retry this prompt");

    app.finalize_turn(TurnOutcome {
        cancelled: true,
        ..Default::default()
    });

    assert_eq!(app.editor.text(), "");
    assert!(
        rx.try_recv().is_err(),
        "restored prompt must not be resubmitted"
    );
}

#[test]
fn cancel_leaves_editor_empty_and_late_result_keeps_new_draft() {
    for (code, modifiers) in [
        (KeyCode::Esc, KeyModifiers::NONE),
        (KeyCode::Char('c'), KeyModifiers::CONTROL),
    ] {
        let (mut app, _dir) = app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.editor.set_text("  retry with spacing  ");
        app.submit_input(&tx);
        assert_eq!(rx.try_recv().unwrap(), "  retry with spacing  ");

        app.busy = true;
        handle_key(&mut app, KeyEvent::new(code, modifiers), &tx);
        assert_eq!(app.editor.text(), "");

        app.editor.set_text("replacement draft");
        app.finalize_turn(TurnOutcome {
            text: "late completion".into(),
            ..Default::default()
        });
        assert_eq!(app.editor.text(), "replacement draft");
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn cancel_before_dispatch_discards_pending_turn_without_restoring_prompt() {
    let (mut app, _dir) = app();
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.editor.set_text("cancel before startup");
    app.submit_input(&tx);
    assert_eq!(rx.try_recv().unwrap(), "cancel before startup");
    assert!(app.busy);

    app.request_cancel();
    assert!(
        app.busy,
        "pending turn remains active until dispatch consumes cancel"
    );
    app.request_cancel();
    assert!(
        app.cancel_requested,
        "a second cancel must not revoke the first"
    );
    assert_eq!(app.editor.text(), "");
    assert!(app.discard_cancelled_pending_turn());

    assert!(!app.busy);
    assert_eq!(app.editor.text(), "");
    assert!(!app.discard_cancelled_pending_turn());
}

#[test]
fn submitting_while_busy_does_not_replace_active_prompt_or_queue_another_turn() {
    let (mut app, _dir) = app();
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.editor.set_text("active prompt");
    app.submit_input(&tx);
    assert_eq!(rx.try_recv().unwrap(), "active prompt");

    let queued = vec!["older queued one".to_owned(), "older queued two".to_owned()];
    app.queue.items = queued.clone();
    app.editor.set_text("second prompt");
    app.submit_input(&tx);

    assert_eq!(app.editor.text(), "second prompt");
    assert!(app.busy);
    assert_eq!(app.submitted_prompt.as_deref(), Some("active prompt"));
    assert!(!app.cancel_requested && !app.queue.dispatch.is_explicit());
    assert_eq!(app.queue.items, queued);
    assert!(app.busy_send_confirmation.is_some());
    assert!(app.flash.as_ref().unwrap().0.contains("Enter again"));
    assert!(rx.try_recv().is_err());

    let mut repeat = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    repeat.kind = KeyEventKind::Repeat;
    assert!(!handle_key(&mut app, repeat, &tx));
    assert!(!app.cancel_requested && !app.queue.dispatch.is_explicit());
    assert_eq!(app.editor.text(), "second prompt");

    // Text identity, turn identity and conversation identity each require a fresh first press.
    app.editor.set_text("edited second prompt");
    app.submit_input(&tx);
    assert!(!app.cancel_requested);
    assert_eq!(
        app.busy_send_confirmation.as_ref().unwrap().prompt,
        "edited second prompt"
    );
    app.turn_started = app
        .turn_started
        .map(|started| started + Duration::from_millis(1));
    app.submit_input(&tx);
    assert!(!app.cancel_requested);
    app.deleg.change_conversation();
    app.submit_input(&tx);
    assert!(!app.cancel_requested);
    app.busy_send_confirmation.as_mut().unwrap().armed = Instant::now() - Duration::from_secs(6);
    app.submit_input(&tx);
    assert!(
        !app.cancel_requested,
        "an expired first press cannot interrupt a turn"
    );

    assert!(!handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        &tx
    ));
    assert!(app.busy_send_confirmation.is_none());
    app.submit_input(&tx);
    assert!(!app.cancel_requested);
    app.paste_text(" extra");
    assert!(app.busy_send_confirmation.is_none());
    let replacement = app.editor.text();
    app.submit_input(&tx);
    assert!(!app.cancel_requested);
    handle_mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: u16::MAX,
            row: u16::MAX,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert!(app.busy_send_confirmation.is_none());
    app.submit_input(&tx);
    assert!(!app.cancel_requested && !app.queue.dispatch.is_explicit());
    assert_eq!(app.queue.items, queued);

    // The confirmed press requests cancellation; it cannot dispatch a second native process.
    app.submit_input(&tx);
    assert!(app.busy && app.cancel_requested && app.queue.dispatch.is_explicit());
    assert!(app.editor.text().is_empty());
    assert_eq!(app.queue.items[0], replacement);
    assert_eq!(&app.queue.items[1..], queued.as_slice());
    assert!(rx.try_recv().is_err());
    app.queue_advance(&tx);
    assert!(
        rx.try_recv().is_err(),
        "forced send waits for the current turn to settle"
    );
    assert!(app.busy);
    app.on_turn_done(
        TurnOutcome {
            cancelled: true,
            ..Default::default()
        },
        &tx,
    );
    assert_eq!(rx.try_recv().unwrap(), replacement);
    assert!(rx.try_recv().is_err());
    assert_eq!(app.queue.items, queued);
    assert!(app.queue.paused);
}

#[test]
fn late_non_cancelled_result_does_not_replace_editor_draft() {
    let (mut app, _dir) = app();
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.editor.set_text("submitted prompt");
    app.submit_input(&tx);
    assert_eq!(rx.try_recv().unwrap(), "submitted prompt");

    app.editor.set_text("draft typed before completion");
    app.finalize_turn(TurnOutcome {
        text: "finished response".into(),
        ..Default::default()
    });

    assert_eq!(app.editor.text(), "draft typed before completion");
    assert!(rx.try_recv().is_err());
}

#[test]
fn a_slash_command_during_a_turn_does_not_replace_its_recovery_prompt() {
    let (mut app, _dir) = app();
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.editor.set_text("  original prompt  ");
    app.submit_input(&tx);
    rx.try_recv().unwrap();
    app.editor.set_text("/queue");
    app.submit_input(&tx);
    app.request_cancel();
    assert_eq!(app.editor.text(), "");
    assert!(rx.try_recv().is_err());
}

#[test]
fn a_running_turn_cannot_be_moved_into_a_new_thread_or_provider() {
    let (mut app, _dir) = app();
    let (tx, mut rx) = mpsc::unbounded_channel();
    app.editor.set_text("original conversation");
    app.submit_input(&tx);
    rx.try_recv().unwrap();
    app.native_session_id = Some("synthetic-session".into());
    for command in ["/new", "/use", "/resume", "/model"] {
        assert!(app.handle_slash(command));
        assert!(app.picker.is_none());
        assert_eq!(app.native_session_id.as_deref(), Some("synthetic-session"));
        assert!(app
            .messages
            .iter()
            .any(|m| m.role == MsgRole::User && m.text == "original conversation"));
    }
    assert!(rx.try_recv().is_err());
}
