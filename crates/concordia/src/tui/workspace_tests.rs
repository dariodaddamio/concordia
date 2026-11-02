use super::*;
use ratatui::backend::TestBackend;

struct Harness {
    app: App,
    terminal: Terminal<TestBackend>,
    dir: tempfile::TempDir,
}

impl Harness {
    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.app.workspace.poll();
            self.draw();
            if self.app.workspace.jobs.is_empty()
                && !self.app.workspace.panes.iter().any(|p| p.saving)
            {
                break;
            }
            assert!(Instant::now() < deadline, "file workers did not finish");
            std::thread::yield_now();
        }
    }
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
fn recovery_reconciles_a_save_that_reached_disk_before_its_acknowledgement() {
    use std::sync::Arc;
    let mut h = Harness::new(120, 30);
    let lease = Arc::new(recovery::create(&h.dir.path().join("recovery-test")).unwrap());
    h.app
        .workspace
        .set_file_recovery(lease.clone(), h.dir.path())
        .unwrap();
    h.open("save.c", "int saved;\n");
    h.key(KeyCode::Char('!'), KeyModifiers::NONE);
    h.app.workspace.flush_file_recovery().unwrap();
    let edited = h.app.workspace.panes[0]
        .document
        .as_ref()
        .unwrap()
        .text()
        .to_owned();
    std::fs::write(h.dir.path().join("save.c"), &edited).unwrap();
    h.app.workspace = Workspace::default();
    h.app
        .workspace
        .set_file_recovery(lease, h.dir.path())
        .unwrap();
    h.draw();
    h.app.workspace.poll();
    h.settle();
    assert!(!h.app.workspace.has_unsaved());
    assert_eq!(
        h.app.workspace.panes[0].document.as_ref().unwrap().text(),
        edited
    );
}

#[test]
fn corrupt_file_recovery_is_retained_and_repaired_storage_preserves_open_panes() {
    use std::sync::Arc;
    let mut h = Harness::new(120, 30);
    let id = h.open("keep.c", "int keep = 1;\n");
    let root = h.dir.path().join("recovery-test");
    let lease = Arc::new(recovery::create(&root).unwrap());
    lease.save_files(b"{broken").unwrap();
    assert!(h
        .app
        .workspace
        .set_file_recovery(lease.clone(), h.dir.path())
        .is_err());
    h.app.workspace.flush_file_recovery().unwrap();
    assert_eq!(lease.files().unwrap(), b"{broken");
    assert_eq!(h.app.workspace.panes[0].id, id);
    lease.save_files(b"[]").unwrap();
    h.app
        .workspace
        .set_file_recovery(lease.clone(), h.dir.path())
        .unwrap();
    assert_eq!(h.app.workspace.panes[0].id, id);
    h.key(KeyCode::Char('!'), KeyModifiers::NONE);
    h.app.workspace.flush_file_recovery().unwrap();
    assert!(String::from_utf8(lease.files().unwrap())
        .unwrap()
        .contains("!int keep"));
}

#[test]
fn formatted_is_default_and_raw_save_is_lossless_with_undo_and_conflict_protection() {
    let mut h = Harness::new(130, 32);
    h.app.editor.set_text("unsent chat");
    let original = "\u{feff}# Notes\r\n\r\nOriginal\r\n";
    h.open("notes.md", original);
    assert!(!h.app.workspace.panes[0].raw());
    h.key(KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(
        h.app.workspace.panes[0].document.as_ref().unwrap().text(),
        original
    );
    h.key(KeyCode::Char('v'), KeyModifiers::ALT);
    h.key(KeyCode::Char('a'), KeyModifiers::CONTROL);
    assert!(super::paste(&mut h.app, "# Edited\r\n\tcontent\r\n"));
    assert!(h.app.workspace.has_unsaved());
    h.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    h.settle();
    let edited = "\u{feff}# Edited\r\n\tcontent\r\n";
    assert_eq!(
        std::fs::read(h.dir.path().join("notes.md")).unwrap(),
        edited.as_bytes()
    );
    assert!(!h.app.workspace.has_unsaved());
    h.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(
        h.app.workspace.panes[0].document.as_ref().unwrap().text(),
        original
    );
    h.key(KeyCode::Char('y'), KeyModifiers::CONTROL);
    assert!(!h.app.workspace.has_unsaved());
    h.key(KeyCode::Char('!'), KeyModifiers::NONE);
    let buffer = h.app.workspace.panes[0]
        .document
        .as_ref()
        .unwrap()
        .text()
        .to_owned();
    std::fs::write(h.dir.path().join("notes.md"), "external change").unwrap();
    h.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    h.settle();
    assert!(h.app.workspace.panes[0]
        .error
        .as_deref()
        .unwrap()
        .contains("changed"));
    assert_eq!(
        h.app.workspace.panes[0].document.as_ref().unwrap().text(),
        buffer
    );
    assert_eq!(
        std::fs::read_to_string(h.dir.path().join("notes.md")).unwrap(),
        "external change"
    );
    assert_eq!(h.app.editor.text(), "unsent chat");
}

#[test]
fn source_code_is_colored_editable_and_dirty_close_reload_and_quit_require_decisions() {
    let mut h = Harness::new(130, 32);
    let id = h.open("demo.c", "// Source\nint main(void) { return 42; }\n");
    let p = &h.app.workspace.panes[0];
    assert!(p.raw());
    let colors: std::collections::HashSet<_> =
        p.colors.iter().flatten().map(|(_, _, s)| s.fg).collect();
    assert!(
        colors.len() >= 3,
        "C syntax must have distinct token colors"
    );
    for c in "vrq/".chars() {
        h.key(KeyCode::Char(c), KeyModifiers::NONE);
    }
    assert!(h.app.workspace.panes[0]
        .document
        .as_ref()
        .unwrap()
        .text()
        .starts_with("vrq/"));
    assert!(h.app.workspace.close(id));
    assert!(h.app.workspace.panes[0].prompt.is_some());
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.app.workspace.panes[0].prompt.is_none());
    h.key(KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert!(h.app.workspace.panes[0].prompt == Some(edit::Action::Reload));
    h.key(KeyCode::Char('d'), KeyModifiers::NONE);
    h.settle();
    assert!(!h.app.workspace.has_unsaved());
    h.key(KeyCode::Char('!'), KeyModifiers::NONE);
    assert!(!h.app.workspace.request_file_exit());
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(!h.app.workspace.file_quit_requested());
    h.app.workspace.close(id);
    h.key(KeyCode::Char('s'), KeyModifiers::NONE);
    h.settle();
    assert!(h.app.workspace.panes.is_empty());
    assert!(std::fs::read_to_string(h.dir.path().join("demo.c"))
        .unwrap()
        .contains('!'));
}

#[test]
fn file_recovery_retains_layout_cursor_and_edits_when_disk_changes() {
    use std::sync::Arc;
    let mut h = Harness::new(130, 32);
    let root = h.dir.path().join("isolated-recovery");
    let lease = Arc::new(recovery::create(&root).unwrap());
    lease.save("chat draft", &[]).unwrap();
    h.app
        .workspace
        .set_file_recovery(lease.clone(), h.dir.path())
        .unwrap();
    h.open("recover.c", "int saved = 1;\n");
    h.key(KeyCode::End, KeyModifiers::NONE);
    h.key(KeyCode::Char('!'), KeyModifiers::NONE);
    let position = h.app.workspace.panes[0]
        .document
        .as_ref()
        .unwrap()
        .position();
    h.app.workspace.flush_file_recovery().unwrap();
    let checkpoint = lease.files().unwrap();
    // Switching instances must not overwrite the pending record before first draw.
    let mut restored = Workspace::default();
    restored
        .set_file_recovery(lease.clone(), h.dir.path())
        .unwrap();
    restored.flush_file_recovery().unwrap();
    assert_eq!(lease.files().unwrap(), checkpoint);
    assert!(restored.has_unsaved());
    std::fs::write(h.dir.path().join("recover.c"), "int external = 2;\n").unwrap();
    h.app.workspace = restored;
    h.draw();
    h.app.workspace.poll();
    h.settle();
    let p = &h.app.workspace.panes[0];
    assert_eq!(p.document.as_ref().unwrap().text(), "int saved = 1;!\n");
    assert_eq!(p.document.as_ref().unwrap().position(), position);
    assert!(p.document.as_ref().unwrap().dirty());
    assert!(p.changed);
    h.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    h.settle();
    assert_eq!(
        std::fs::read_to_string(h.dir.path().join("recover.c")).unwrap(),
        "int external = 2;\n"
    );
    assert_eq!(lease.data().unwrap().draft, "chat draft");
}

#[test]
fn formatted_document_links_resolve_relative_to_the_document() {
    let mut h = Harness::new(120, 32);
    std::fs::create_dir_all(h.dir.path().join("docs")).unwrap();
    std::fs::write(h.dir.path().join("docs/target.md"), "# Exact linked file").unwrap();
    std::fs::write(h.dir.path().join("target.md"), "WRONG cwd file").unwrap();
    h.app.editor.set_text("keep my draft");
    let id = h.open(
        "docs/source.md",
        "| Item | Source |\n| --- | --- |\n| Plan | [friendly](target.md) |",
    );
    h.app.workspace.preview(id, true);
    h.draw();
    let pane = h.app.workspace.panes.iter().find(|p| p.id == id).unwrap();
    let (row_index, row) = pane
        .view
        .rows
        .iter()
        .enumerate()
        .find(|(_, r)| !r.links.is_empty())
        .unwrap();
    let col = row.links[0].0;
    let x = pane.area.x + 1 + text::slice_width(&row.text[..col]) as u16;
    let y = pane.area.y + 2 + (row_index - pane.view.visible().start) as u16;
    assert_eq!(h.app.workspace.pointer_shape(x, y), Some("pointer"));
    h.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    h.mouse(MouseEventKind::Up(MouseButton::Left), x, y);
    let expected = h.dir.path().join("docs/target.md").canonicalize().unwrap();
    assert_eq!(h.app.last_file_open.as_ref().unwrap().path, expected);
    assert_eq!(h.app.editor.text(), "keep my draft");
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
fn focused_raw_editor_owns_typing_enter_paste_and_dirty_close_preserves_chat() {
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
    // Raw letters are file content; closing unsaved work requires a decision.
    h.key(KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert!(!h.app.workspace.panes.is_empty());
    h.key(KeyCode::Char('q'), KeyModifiers::NONE);
    assert!(h.app.workspace.panes[0]
        .document
        .as_ref()
        .unwrap()
        .text()
        .contains('q'));
    let id = h.app.workspace.panes[0].id;
    assert!(h.app.workspace.close(id));
    h.draw();
    assert!(h.screen().contains("Unsaved"));
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(!h.app.workspace.panes.is_empty());
    assert!(h.app.workspace.close(id));
    h.key(KeyCode::Char('d'), KeyModifiers::NONE);
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
    assert_eq!(
        h.app.workspace.panes[0]
            .document
            .as_ref()
            .unwrap()
            .position()
            .0,
        1
    );
    assert_eq!(h.app.workspace.panes[0].scroll, 1);
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

#[test]
fn formatted_markdown_and_json_toggle_without_changing_draft() {
    let mut h = Harness::new(110, 30);
    h.app.editor.set_text("keep this draft");
    h.open("note.md", "# Heading\n**bold text**\n");
    assert!(h.screen().contains("bold text"));
    assert!(!h.screen().contains("**bold text**"));
    assert!(!h.screen().contains("# Heading"));
    let pane = &h.app.workspace.panes[0];
    assert!(pane
        .view
        .rows
        .iter()
        .any(|r| r.style.add_modifier.contains(Modifier::BOLD)));
    let button = pane.mode_button();
    assert_eq!(
        super::super::pointer::shape(&h.app, button.x, button.y),
        "pointer"
    );
    h.mouse(MouseEventKind::Down(MouseButton::Left), button.x, button.y);
    assert!(h.screen().contains("**bold text**"));
    h.open("data.json", r#"{"a":1,"b":2}"#);
    assert!(h.app.handle_slash("/preview data.json"));
    h.draw();
    assert!(h.app.workspace.panes[1].formatted_mode);
    assert!(h.screen().contains("\"a\": 1,"));
    assert_eq!(h.app.editor.text(), "keep this draft");
    h.open("code.rs", "fn main() {}\n");
    assert!(!h.app.workspace.panes[2].formatted_mode);
}

#[test]
fn pointer_shapes_match_resize_drag_and_file_close_targets() {
    let mut h = Harness::new(110, 30);
    h.open("first.txt", "first");
    h.open("second.txt", "second");
    let divider = h.app.workspace.divider;
    let first = h.app.workspace.panes[0].area;
    assert_eq!(
        super::super::pointer::shape(&h.app, divider.x, 3),
        "ew-resize"
    );
    assert_eq!(
        super::super::pointer::shape(&h.app, first.x + 2, first.bottom() - 1),
        "ns-resize"
    );
    assert_eq!(
        super::super::pointer::shape(&h.app, first.right() - 2, first.y),
        "pointer"
    );
    h.mouse(MouseEventKind::Down(MouseButton::Left), divider.x, 3);
    assert_eq!(
        super::super::pointer::shape(&h.app, divider.x + 5, 3),
        "ew-resize"
    );
    h.mouse(MouseEventKind::Up(MouseButton::Left), divider.x + 5, 3);
    assert_eq!(
        super::super::pointer::shape(&h.app, first.x + 2, first.y + 3),
        "text"
    );
}

#[test]
fn file_search_reload_and_changed_indicator_preserve_chat_draft_and_pane_identity() {
    let mut h = Harness::new(120, 30);
    h.app.editor.set_text("unsent draft stays here");
    let body = (0..80)
        .map(|i| {
            if matches!(i, 10 | 60) {
                "target match\n".to_string()
            } else {
                format!("row {i}\n")
            }
        })
        .collect::<String>();
    let id = h.open("search.txt", &body);
    h.key(KeyCode::Char('f'), KeyModifiers::CONTROL);
    for c in "target".chars() {
        h.key(KeyCode::Char(c), KeyModifiers::NONE);
    }
    assert_eq!(h.app.workspace.panes[0].matches, [10, 60]);
    assert_eq!(h.app.workspace.panes[0].scroll, 10);
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    h.key(KeyCode::F(3), KeyModifiers::NONE);
    assert_eq!(h.app.workspace.panes[0].scroll, 60);
    h.key(KeyCode::F(3), KeyModifiers::SHIFT);
    assert_eq!(h.app.workspace.panes[0].scroll, 10);
    std::fs::write(h.dir.path().join("search.txt"), "NEW_FILE_CONTENTS\n").unwrap();
    h.app.workspace.watched_at = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while !h.app.workspace.panes[0].changed {
        h.app.workspace.poll();
        h.app.workspace.watched_at = None;
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    h.draw();
    assert!(h.screen().contains("changed on disk"));
    h.key(KeyCode::Char('r'), KeyModifiers::CONTROL);
    while !h.app.workspace.jobs.is_empty() {
        h.app.workspace.poll();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    h.draw();
    assert_eq!(h.app.workspace.panes[0].id, id);
    assert!(h.screen().contains("NEW_FILE_CONTENTS"));
    assert!(!h.app.workspace.panes[0].changed);
    assert_eq!(h.app.editor.text(), "unsent draft stays here");
}
