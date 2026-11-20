use super::*;
use ratatui::backend::{Backend, TestBackend};

fn app() -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (
        App::new(dir.path().to_path_buf(), dir.path().to_path_buf()),
        dir,
    )
}

fn open_edit(app: &mut App, text: &str) {
    app.queue.items = vec![text.to_string()];
    app.queue.focused = true;
    app.queue.sel = 0;
    let mut ed = Editor::default();
    ed.set_text(text);
    ed.reset_undo();
    app.begin_queue_edit(ed);
}

fn draw(app: &mut App, width: u16, height: u16) -> (Terminal<TestBackend>, Rect) {
    let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
    term.draw(|f| {
        let mut transcript = f.area();
        let panel = queue::carve(&mut transcript, app);
        queue::draw(f, panel, app);
    })
    .unwrap();
    let mut transcript = Rect::new(0, 0, width, height);
    (term, queue::carve(&mut transcript, app))
}

fn click(x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn editor_layout_and_caret_remain_inside_clipped_panel_across_terminal_sizes() {
    for width in [1, 2, 8, 40, 90] {
        for height in [4, 8, 13] {
            let (mut a, _dir) = app();
            open_edit(
                &mut a,
                "日本語 wide 界面\nsecond line\nthird line\nfourth line\nfifth line",
            );
            a.queue
                .edit
                .as_mut()
                .unwrap()
                .editor
                .move_to(usize::MAX, false);
            let (mut term, panel) = draw(&mut a, width, height);
            if let Some((hit, _)) = a.queue.edit_rect {
                assert!(
                    panel.intersects(hit),
                    "panel={panel:?} hit={hit:?} {width}x{height}"
                );
                assert!(hit.width > 0 && hit.height > 0);
                let cursor = term.backend_mut().get_cursor_position().unwrap();
                assert!(
                    contains(hit, cursor.x, cursor.y),
                    "cursor={cursor:?}, hit={hit:?} {width}x{height}"
                );
            } else if panel.height > 0 {
                let cursor = term.backend_mut().get_cursor_position().unwrap();
                assert!(
                    contains(panel, cursor.x, cursor.y),
                    "cursor={cursor:?}, panel={panel:?} {width}x{height}"
                );
            }
            assert!(a.queue.edit.is_some());
        }
    }
}

#[test]
fn outside_and_other_row_clicks_keep_the_edit_and_only_editor_click_moves_caret() {
    let (mut a, _dir) = app();
    a.queue.items = (1..=5).map(|i| format!("queued {i}")).collect();
    a.queue.sel = 0;
    a.queue.focused = true;
    let mut ed = Editor::default();
    ed.set_text("first editable line\nsecond editable line");
    ed.move_to(0, false);
    a.begin_queue_edit(ed);
    let (_term, panel) = draw(&mut a, 100, 20);
    let original = a.queue.edit.as_ref().unwrap().editor.text();
    let sibling = a.queue.rects.first().expect("a sibling queue row").0;
    assert!(queue::handle_mouse(&mut a, &click(sibling.x, sibling.y)));
    assert_eq!(a.queue.sel, 0);
    assert_eq!(a.queue.edit.as_ref().unwrap().editor.text(), original);
    assert!(queue::handle_mouse(&mut a, &click(0, 0)));
    assert!(a.queue.focused);
    assert_eq!(a.queue.edit.as_ref().unwrap().editor.text(), original);

    let (hit, scroll) = a.queue.edit_rect.unwrap();
    assert!(panel.intersects(hit));
    let old_cursor = a.queue.edit.as_ref().unwrap().editor.cursor();
    queue::handle_mouse(&mut a, &click(hit.x + 10, hit.y));
    assert_ne!(a.queue.edit.as_ref().unwrap().editor.cursor(), old_cursor);
    assert!(a.queue.edit_scroll == scroll);
}

#[test]
fn fifth_selected_item_stays_visible_and_clearing_requires_resolving_edit() {
    let (mut a, _dir) = app();
    a.queue.items = (1..=8).map(|i| format!("item {i}")).collect();
    a.queue.sel = 4;
    a.queue.focused = true;
    let mut ed = Editor::default();
    ed.set_text("the fifth queued item remains selected");
    a.begin_queue_edit(ed);
    let (term, _) = draw(&mut a, 80, 20);
    let buffer = term.backend().buffer();
    let screen = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer.cell((x, y)).unwrap().symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert!(
        screen.iter().any(|line| line.contains("editing 5")),
        "{screen:?}"
    );
    assert!(
        screen
            .iter()
            .any(|line| line.contains("the fifth queued item")),
        "{screen:?}"
    );
    let response = a.queue_command("clear");
    assert!(response[0].contains("save with Enter or cancel"));
    assert_eq!(a.queue.items.len(), 8);
    assert!(a.queue.edit.is_some());
}

#[test]
fn multiline_unicode_navigation_selection_save_and_cancel_preserve_the_main_draft() {
    let (mut a, _dir) = app();
    a.editor.set_text("main draft");
    a.editor.move_to(3, false);
    let before = (a.editor.text(), a.editor.cursor());
    open_edit(&mut a, "日本語 hello world");
    key(&mut a, KeyCode::Enter, KeyModifiers::CONTROL);
    key(&mut a, KeyCode::Char('j'), KeyModifiers::CONTROL);
    assert_eq!(
        a.queue.edit.as_ref().unwrap().editor.text(),
        "日本語 hello world\n\n"
    );
    key(&mut a, KeyCode::Up, KeyModifiers::SHIFT);
    assert!(a.queue.edit.as_ref().unwrap().editor.selection().is_some());
    key(&mut a, KeyCode::Home, KeyModifiers::NONE);
    key(&mut a, KeyCode::End, KeyModifiers::NONE);
    key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
    assert!(a.queue.edit.is_none());
    assert!(a.queue.items[0].contains("日本語"));
    assert_eq!((a.editor.text(), a.editor.cursor()), before);

    open_edit(&mut a, "cancel remains unchanged");
    a.queue.items[0] = "saved value".into();
    key(&mut a, KeyCode::Char('x'), KeyModifiers::NONE);
    key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(a.queue.items[0], "saved value");
    assert_eq!((a.editor.text(), a.editor.cursor()), before);
}

fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    assert!(queue::handle_key(app, &KeyEvent::new(code, modifiers)));
}
