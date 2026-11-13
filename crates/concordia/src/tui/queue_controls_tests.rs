//! Focused tests for explicit queue resume. The root module wires this test module alongside
//! `queue_controls` during integration.

use super::editor::Editor;
use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn app() -> (
    App,
    tempfile::TempDir,
    tokio::sync::mpsc::UnboundedSender<String>,
    tokio::sync::mpsc::UnboundedReceiver<String>,
) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (app, dir, tx, rx)
}

#[test]
fn ctrl_r_resumes_one_queued_prompt_and_preserves_the_draft() {
    let (mut app, _dir, tx, mut rx) = app();
    app.editor.set_text("unfinished draft");
    app.editor.move_left(false);
    app.editor.move_left(true);
    let cursor = app.editor.cursor();
    let selection = app.editor.selection();
    app.history.push("earlier prompt".into());
    app.hist_pos = Some(0);
    app.queue.items = vec!["queued prompt".into(), "next prompt".into()];
    app.queue.paused = true;

    let key = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert!(queue_controls::handle_key(&mut app, &key));
    assert!(!app.queue.paused);
    assert_eq!(app.editor.text(), "unfinished draft");
    assert_eq!(app.editor.cursor(), cursor);
    assert_eq!(app.editor.selection(), selection);
    assert_eq!(app.history, vec!["earlier prompt"]);
    assert_eq!(app.hist_pos, Some(0));
    assert!(
        rx.try_recv().is_err(),
        "the event loop owns queue advancement"
    );

    app.queue_advance(&tx);
    assert_eq!(rx.try_recv().unwrap(), "queued prompt");
    assert!(
        rx.try_recv().is_err(),
        "one queued item starts per advancement"
    );
    assert_eq!(app.queue.items, vec!["next prompt"]);
}

#[test]
fn slash_resume_rejects_empty_queue_without_changing_state() {
    let (mut app, _dir, _tx, _rx) = app();
    app.editor.set_text("keep this draft");
    app.queue.paused = true;

    let messages = queue_controls::resume(&mut app);

    assert_eq!(
        messages,
        vec!["queue is empty - add a prompt with Tab before resuming"]
    );
    assert!(app.queue.paused);
    assert!(!app.queue.dispatch.is_automatic());
    assert_eq!(app.editor.text(), "keep this draft");
}

#[test]
fn resume_defers_during_active_turn_and_rejects_cancellation_or_inline_edit() {
    let (mut app, _dir, _tx, _rx) = app();
    app.queue.items.push("queued".into());
    app.queue.paused = true;
    app.busy = true;

    assert_eq!(
        queue_controls::resume(&mut app),
        vec!["queue resumed - it will continue after the current turn"]
    );
    assert!(!app.queue.paused);
    assert!(app.queue.dispatch.is_automatic());

    app.queue.paused = true;
    app.queue.dispatch.clear_automatic();
    app.cancel_requested = true;
    assert_eq!(
        queue_controls::resume(&mut app),
        vec!["queue remains paused while cancellation finishes"]
    );
    assert!(app.queue.paused);
    assert!(!app.queue.dispatch.is_automatic());

    app.cancel_requested = false;
    app.begin_queue_edit(Editor::default());
    assert_eq!(
        queue_controls::resume(&mut app),
        vec!["save or cancel the queued edit before resuming (Ctrl+R)"]
    );
    assert!(app.queue.paused);
    assert!(!app.queue.dispatch.is_automatic());
    assert!(app.queue.edit.is_some());
}
