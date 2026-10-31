use super::*;
use ratatui::backend::TestBackend;

struct Harness {
    app: App,
    terminal: Terminal<TestBackend>,
    dir: tempfile::TempDir,
}

impl Harness {
    fn new(width: u16, height: u16) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut h = Self {
            app: App::new(dir.path().into(), dir.path().into()),
            terminal: Terminal::new(TestBackend::new(width, height)).unwrap(),
            dir,
        };
        h.draw();
        h
    }
    fn draw(&mut self) {
        self.terminal.draw(|f| draw_ui(f, &mut self.app)).unwrap();
    }
    fn open(&mut self, name: &str, body: &str) -> u32 {
        std::fs::write(self.dir.path().join(name), body).unwrap();
        assert!(self.app.handle_slash(&format!("/open {name}")));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.app.workspace.jobs.is_empty() {
            self.app.workspace.poll();
            assert!(Instant::now() < deadline, "reader failed to finish");
            std::thread::yield_now();
        }
        self.draw();
        self.app.workspace.focused.unwrap()
    }
    fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let (tx, mut rx) = mpsc::unbounded_channel();
        assert!(!super::super::handle_key(
            &mut self.app,
            KeyEvent::new(code, modifiers),
            &tx
        ));
        assert!(
            rx.try_recv().is_err(),
            "file pane dispatched a provider prompt"
        );
        self.draw();
    }
    fn mouse(&mut self, kind: MouseEventKind, x: u16, y: u16) {
        super::super::handle_mouse(
            &mut self.app,
            MouseEvent {
                kind,
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
        );
        self.draw();
    }
    fn screen(&self) -> String {
        self.terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }
}

#[test]
fn opens_rendered_panes_and_closes_exact_ids_without_changing_draft() {
    let mut h = Harness::new(110, 30);
    h.app.editor.insert_paste_chunk("unsent draft", false);
    let first = h.open("first.txt", "first file contents");
    assert!(h.screen().contains("first file contents"));
    assert!(h.app.areas.input.right() < h.app.workspace.panes[0].area.x);
    let second = h.open("second.txt", "second file contents");
    assert_ne!(first, second);
    assert!(h.app.workspace.close(first));
    h.draw();
    assert_eq!(h.app.workspace.panes.len(), 1);
    assert_eq!(h.app.workspace.panes[0].id, second);
    assert!(!h.app.workspace.close(999));
    assert_eq!(h.app.workspace.panes.len(), 1);
    let rect = h.app.workspace.panes[0].area;
    h.mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.right() - 2,
        rect.y,
    );
    h.mouse(
        MouseEventKind::Up(MouseButton::Left),
        rect.right() - 2,
        rect.y,
    );
    assert!(h.app.workspace.panes.is_empty());
    assert_eq!(h.app.editor.text(), "unsent draft");
    assert_eq!(h.app.areas.input.width, 108);
}

#[test]
fn focused_preview_consumes_typing_enter_and_paste_and_esc_restores_chat() {
    let mut h = Harness::new(100, 24);
    h.app.editor.insert_paste_chunk("preserve me", false);
    h.open("example.txt", "hello\nworld");
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    h.key(KeyCode::Char('x'), KeyModifiers::NONE);
    h.app.paste_text("accidental\nprovider prompt");
    assert_eq!(h.app.editor.text(), "preserve me");
    assert!(!h.app.busy);
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.app.workspace.focused.is_none());
    h.key(KeyCode::Char('!'), KeyModifiers::NONE);
    assert_eq!(h.app.editor.text(), "preserve me!");
    h.key(KeyCode::F(6), KeyModifiers::NONE);
    assert!(h.app.workspace.focused.is_some());
    h.key(KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert!(h.app.workspace.panes.is_empty());
}

#[test]
fn duplicate_open_focuses_existing_pane_and_limits_do_not_lose_existing_content() {
    let mut h = Harness::new(100, 24);
    let first = h.open("one.txt", "one");
    h.open("two.txt", "two");
    assert!(h.app.handle_slash("/open one.txt:1:1"));
    assert_eq!(h.app.workspace.focused, Some(first));
    assert_eq!(h.app.workspace.panes.len(), 2);
    h.open("three.txt", "three");
    std::fs::write(h.dir.path().join("four.txt"), "four").unwrap();
    assert!(h.app.handle_slash("/open four.txt"));
    assert_eq!(h.app.workspace.panes.len(), 3);
    assert!(h.app.flash.as_ref().unwrap().0.contains("limit"));
    assert!(h.app.handle_slash("/open missing.txt"));
    assert_eq!(h.app.workspace.panes.len(), 3);
}

#[test]
fn mouse_and_keyboard_scroll_and_resize_without_editing_or_dispatch() {
    let mut h = Harness::new(110, 30);
    let body = (0..100).map(|i| format!("row {i}\n")).collect::<String>();
    h.open("rows.txt", &body);
    h.open("other.txt", "another file");
    let first = h.app.workspace.panes[0].area;
    h.mouse(
        MouseEventKind::Down(MouseButton::Left),
        first.x + 2,
        first.y + 2,
    );
    h.mouse(MouseEventKind::ScrollDown, first.x + 2, first.y + 2);
    assert_eq!(h.app.workspace.panes[0].scroll, 3);
    h.key(KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(h.app.workspace.panes[0].scroll, 4);
    let divider = h.app.workspace.divider;
    h.mouse(MouseEventKind::Down(MouseButton::Left), divider.x, 2);
    h.mouse(MouseEventKind::Drag(MouseButton::Left), divider.x - 10, 2);
    h.mouse(MouseEventKind::Up(MouseButton::Left), divider.x - 10, 2);
    assert!(h.app.workspace.panes[0].area.width > first.width);
    let first = h.app.workspace.panes[0].area;
    h.mouse(
        MouseEventKind::Down(MouseButton::Left),
        first.x + 2,
        first.bottom() - 1,
    );
    h.mouse(
        MouseEventKind::Drag(MouseButton::Left),
        first.x + 2,
        first.bottom() - 5,
    );
    h.mouse(
        MouseEventKind::Up(MouseButton::Left),
        first.x + 2,
        first.bottom() - 5,
    );
    assert!(h.app.workspace.panes[0].area.height < first.height);
}

#[test]
fn shrinking_hides_hit_targets_preserves_panes_and_restores_on_enlarge() {
    let mut h = Harness::new(100, 24);
    h.open("keep.txt", "keep this");
    h.terminal.backend_mut().resize(40, 8);
    h.terminal.autoresize().unwrap();
    h.draw();
    assert_eq!(h.app.workspace.panes[0].area, Rect::default());
    assert!(h.app.workspace.focused.is_none());
    assert!(h.screen().contains("file panes hidden"));
    h.terminal.backend_mut().resize(100, 24);
    h.terminal.autoresize().unwrap();
    h.draw();
    assert!(h.screen().contains("keep this"));
    assert!(h.app.workspace.panes[0].area.width > 0);
}

#[test]
fn bounded_reads_reject_binary_invalid_utf8_oversize_and_control_sequences() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.txt");
    std::fs::write(&path, [0xff, 0xfe]).unwrap();
    assert!(read_lines(&path).unwrap_err().contains("UTF-8"));
    std::fs::write(&path, "abc\0def").unwrap();
    assert!(read_lines(&path).unwrap_err().contains("binary"));
    std::fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
    assert!(read_lines(&path).unwrap_err().contains("2 MiB"));
    std::fs::write(&path, "\n".repeat(MAX_LINES + 1)).unwrap();
    assert!(read_lines(&path).unwrap_err().contains("50000"));
    std::fs::write(&path, "safe\u{1b}[31m\u{7}text").unwrap();
    assert!(read_lines(&path)
        .unwrap()
        .iter()
        .all(|s| !s.contains('\u{1b}') && !s.contains('\u{7}')));
}

#[test]
fn closing_loading_pane_cannot_attach_late_results_to_a_new_pane() {
    let mut h = Harness::new(100, 24);
    std::fs::write(h.dir.path().join("old.txt"), "old contents").unwrap();
    h.app.handle_slash("/open old.txt");
    let old = h.app.workspace.focused.unwrap();
    h.app.workspace.close(old);
    let new = h.open("new.txt", "new contents");
    assert_ne!(old, new);
    assert_eq!(h.app.workspace.panes.len(), 1);
    assert_eq!(h.app.workspace.panes[0].lines, vec!["new contents"]);
}

#[test]
fn file_focus_does_not_steal_modal_or_queued_edit_input() {
    let mut h = Harness::new(100, 24);
    h.app.editor.insert_paste_chunk("original draft", false);
    h.open("file.txt", "read only");
    h.app.queue.items.push("queued prompt".into());
    h.key(KeyCode::Up, KeyModifiers::CONTROL);
    assert!(h.app.queue.focused);
    assert!(h.app.workspace.focused.is_none());
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(h.app.queue.edit.is_some());
    let rect = h.app.workspace.panes[0].area;
    h.mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 2,
        rect.y + 2,
    );
    assert!(h.app.queue.edit.is_some());
    assert!(h.app.workspace.focused.is_none());
    h.app.paste_text(" queued edit");
    assert!(h
        .app
        .queue
        .edit
        .as_ref()
        .unwrap()
        .text()
        .contains("queued edit"));
    assert_eq!(h.app.editor.text(), "original draft");
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    h.key(KeyCode::F(6), KeyModifiers::NONE);
    assert!(h.app.workspace.focused.is_some());
    assert!(!h.app.queue.focused);
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(h.app.queue.edit.is_none());
    assert_eq!(h.app.editor.text(), "original draft");
}
