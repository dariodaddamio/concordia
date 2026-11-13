use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn app() -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
    (app, dir)
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn ctrl_up_focuses_nonempty_queue_without_touching_multiline_draft_state() {
    let (mut app, _dir) = app();
    app.editor.set_text("first line\nsecond line");
    app.editor.move_to(8, false);
    app.editor.select_range(1, 4);
    let draft_cursor = app.editor.cursor();
    let draft_selection = app.editor.selection();
    app.history.push("earlier prompt".into());
    app.hist_pos = Some(0);
    app.queue.items = vec!["queued one".into(), "queued two".into()];
    app.queue.sel = 1;

    assert!(super::queue_focus::handle_key(
        &mut app,
        &key(KeyCode::Up, KeyModifiers::CONTROL)
    ));

    assert!(app.queue.focused);
    assert_eq!(app.queue.sel, 1);
    assert_eq!(app.editor.text(), "first line\nsecond line");
    assert_eq!(app.editor.cursor(), draft_cursor);
    assert_eq!(app.editor.selection(), draft_selection);
    assert_eq!(app.hist_pos, Some(0));
}

#[test]
fn empty_queue_shortcut_is_harmless_and_ctrl_down_returns_to_draft() {
    let (mut app, _dir) = app();
    assert!(!super::queue_focus::handle_key(
        &mut app,
        &key(KeyCode::Up, KeyModifiers::CONTROL)
    ));
    assert!(!app.queue.focused);

    app.queue.items.push("queued".into());
    assert!(super::queue_focus::handle_key(
        &mut app,
        &key(KeyCode::Up, KeyModifiers::CONTROL)
    ));
    assert!(super::queue_focus::handle_key(
        &mut app,
        &key(KeyCode::Down, KeyModifiers::CONTROL)
    ));
    assert!(!app.queue.focused);
    assert_eq!(app.queue.items, ["queued"]);
}

#[test]
fn ctrl_down_keeps_inline_edit_open_until_explicit_save_or_cancel() {
    let (mut app, _dir) = app();
    app.queue.items.push("queued".into());
    app.queue.focused = true;
    let mut edit = Editor::default();
    edit.set_text("unsaved change");
    app.begin_queue_edit(edit);

    assert!(super::queue_focus::handle_key(
        &mut app,
        &key(KeyCode::Down, KeyModifiers::CONTROL)
    ));

    assert!(app.queue.focused);
    assert_eq!(
        app.queue.edit.as_ref().unwrap().editor.text(),
        "unsaved change"
    );
}
