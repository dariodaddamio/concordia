//! End-to-end input tests: real key/mouse events through the handlers, rendered with ratatui's
//! `TestBackend`, asserting on text, selection, caret and clipboard (in-memory under `cfg(test)`).

use super::*;
use ratatui::backend::TestBackend;

#[test]
fn file_link_click_opens_exact_target_and_drag_keeps_selection() {
    let mut h = Harness::new(90, 24);
    std::fs::write(h._dir.path().join("hello world.md"), "# Preview").unwrap();
    h.app.messages = vec![ChatLine {
        role: MsgRole::Assistant,
        text: "See [friendly label](<hello world.md:7:2>)".into(),
        card: None,
        meta: None,
    }];
    h.draw();
    let row_index = h
        .app
        .view
        .rows
        .iter()
        .position(|row| !row.links.is_empty())
        .unwrap();
    let row = &h.app.view.rows[row_index];
    let (start, _, _) = row.links.first().unwrap();
    let x = h.app.areas.transcript.x
        + row.gutter.chars().count() as u16
        + row.text[..*start]
            .iter()
            .map(|c| text::char_width(*c) as u16)
            .sum::<u16>();
    let y = h.app.areas.transcript.y + (row_index - h.app.view.visible().start) as u16;
    h.down(x, y);
    assert!(h.app.last_file_open.is_none());
    h.up(x, y);
    let file = h.app.last_file_open.take().unwrap();
    assert_eq!(file.line, Some(7));
    assert_eq!(file.column, Some(2));
    assert!(file.path.ends_with("hello world.md"));
    h.app.clicks = ClickTracker::default();
    h.down(x, y);
    h.drag(x + 3, y);
    h.up(x + 3, y);
    assert!(h.app.last_file_open.is_none());
    assert!(h.app.view.selected_text().is_some());
    h.app.clicks = ClickTracker::default();
    h.down(x, y);
    h.drag(x + 3, y);
    h.drag(x, y);
    h.up(x, y);
    assert!(
        h.app.last_file_open.is_none(),
        "a drag returning to its origin is still a drag"
    );
}

#[test]
fn native_chat_cli_keeps_accounts_separate_and_binds_reads_explicitly() {
    let iso = Isolated::new();
    let state = tempfile::tempdir().unwrap();
    for (folder, answer) in [
        (".codex", "education answer"),
        (".codex-2", "personal answer"),
    ] {
        let home = iso.base.path().join(folder);
        std::fs::create_dir_all(home.join("sessions")).unwrap();
        let events=serde_json::json!({"type":"session_meta","payload":{"id":"shared-session","cwd":"synthetic-workspace"}}).to_string()+"\n"+
            &serde_json::json!({"type":"response_item","payload":{"role":"assistant","content":[{"type":"output_text","text":answer}]}}).to_string()+"\n";
        std::fs::write(home.join("sessions/chat.jsonl"), events).unwrap();
    }
    let args = |values: &[&str]| values.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let listing = crate::chat_cli::run(state.path(), &args(&["list", "--json"])).unwrap();
    let listed: serde_json::Value = serde_json::from_str(&listing).unwrap();
    let accounts: Vec<_> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["account"].as_str().unwrap())
        .collect();
    assert!(
        accounts.contains(&"codex") && accounts.contains(&"codex-2"),
        "{listing}"
    );
    let read = crate::chat_cli::run(
        state.path(),
        &args(&["read", "shared-session", "--account", "codex-2"]),
    )
    .unwrap();
    assert!(read.contains("personal answer"));
    assert!(!read.contains("education answer"));
    assert!(crate::chat_cli::run(state.path(), &args(&["read", "shared-session"])).is_err());
}

#[test]
fn limit_warning_updates_with_account_snapshot_and_unknown_clears_it() {
    let mut h = Harness::new(120, 30);
    let mut snap = crate::usage::UsageSnapshot::no_data("cached; refreshing", "last report");
    snap.windows.push(crate::usage::UsageWindow {
        label: "5h".into(),
        used_pct: 96.0,
        resets: Some("2026-10-16 06:03 local".into()),
    });
    h.app.provider = ProviderKind::Codex;
    h.app.set_usage_footer(&snap);
    h.draw();
    assert!(h.screen().contains("5h 4% left"));
    assert!(h.screen().contains("06:03 local"));
    assert!(h.screen().contains("last reported"));
    h.app.provider = ProviderKind::Claude;
    h.app.set_usage_footer(&snap);
    assert!(h.app.usage_warning.as_deref().unwrap().contains("cached"));
    h.app
        .set_usage_footer(&crate::usage::UsageSnapshot::no_data(
            "unknown",
            "no report",
        ));
    h.draw();
    assert!(h.app.usage_warning.is_none());
    assert!(!h.screen().contains("5h 4% left"));
}

#[test]
fn distinct_escape_presses_drained_together_close_each_focused_layer() {
    let mut h = Harness::new(80, 24);
    h.app.queue.items = vec!["queued original".into()];
    h.app.queue.focused = true;
    let mut edit = Editor::default();
    edit.set_text("unsaved upper edit");
    h.app.queue.edit = Some(edit);
    h.app.activity.open(0);
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut burst = input::BurstBuffer::default();
    for _ in 0..2 {
        dispatch_key_event(
            &mut h.app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut burst,
            &tx,
            Instant::now(),
        );
    }
    assert!(h.app.activity.detail.is_none());
    assert!(h.app.queue.edit.is_none());
    assert_eq!(h.app.queue.items, ["queued original"]);
    assert!(!should_accept_key(&KeyEvent::new_with_kind(
        KeyCode::Esc,
        KeyModifiers::NONE,
        KeyEventKind::Release
    )));
}

struct Harness {
    app: App,
    term: Terminal<TestBackend>,
    _dir: tempfile::TempDir,
}

impl Harness {
    fn new(w: u16, h: u16) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        app.no_color = false; // Fixtures choose color explicitly, independent of the caller's env.
        let term = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut hn = Harness {
            app,
            term,
            _dir: dir,
        };
        hn.draw();
        hn
    }

    fn draw(&mut self) {
        let app = &mut self.app;
        self.term.draw(|f| draw_ui(f, app)).unwrap();
    }

    fn key(&mut self, code: KeyCode, mods: KeyModifiers) -> bool {
        let (tx, _rx) = mpsc::unbounded_channel();
        let quit = handle_key(&mut self.app, KeyEvent::new(code, mods), &tx);
        self.draw();
        quit
    }

    fn type_str(&mut self, s: &str) {
        for c in s.chars() {
            self.key(KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    fn mouse(&mut self, kind: MouseEventKind, x: u16, y: u16) {
        handle_mouse(
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

    fn down(&mut self, x: u16, y: u16) {
        self.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    }

    fn drag(&mut self, x: u16, y: u16) {
        self.mouse(MouseEventKind::Drag(MouseButton::Left), x, y);
    }

    fn up(&mut self, x: u16, y: u16) {
        self.mouse(MouseEventKind::Up(MouseButton::Left), x, y);
    }

    fn click(&mut self, x: u16, y: u16) {
        self.down(x, y);
        self.up(x, y);
    }

    /// Index of the transcript row whose text equals `text`.
    fn row_of(&self, text: &str) -> usize {
        self.app
            .view
            .rows
            .iter()
            .position(|r| r.text.iter().collect::<String>() == text)
            .unwrap_or_else(|| panic!("no transcript row {text:?}"))
    }

    /// Screen cell of char `idx` in transcript row `row` (gutter included).
    fn t_cell(&self, row: usize, idx: usize) -> (u16, u16) {
        let t = self.app.areas.transcript;
        let g = self.app.view.rows[row].gutter.chars().count();
        let first = self.app.view.visible().start;
        (t.x + (g + idx) as u16, t.y + (row - first) as u16)
    }

    /// Screen cell of char column `col` on prompt row `row`.
    fn i_cell(&self, row: u16, col: usize) -> (u16, u16) {
        let r = self.app.areas.input;
        (r.x + (PROMPT_W + col) as u16, r.y + row)
    }
}

#[test]
fn click_places_caret_in_prompt_and_typing_inserts_there() {
    let mut h = Harness::new(60, 20);
    h.type_str("hello world");
    let (x, y) = h.i_cell(0, 5);
    h.click(x, y);
    assert_eq!(h.term.get_cursor_position().unwrap(), Position::new(x, y));
    h.type_str(",");
    assert_eq!(h.app.editor.text(), "hello, world");
}

#[test]
fn drag_in_prompt_selects_and_typing_replaces() {
    let mut h = Harness::new(60, 20);
    h.type_str("hello world");
    let (x0, y) = h.i_cell(0, 6);
    let (x1, _) = h.i_cell(0, 11);
    h.down(x0, y);
    h.drag(x1, y);
    h.up(x1, y);
    assert_eq!(h.app.editor.selected_text().as_deref(), Some("world"));
    h.type_str("there");
    assert_eq!(h.app.editor.text(), "hello there");
}

#[test]
fn double_and_triple_click_in_prompt() {
    let mut h = Harness::new(60, 20);
    h.type_str("alpha beta gamma");
    let (x, y) = h.i_cell(0, 7);
    h.click(x, y);
    h.click(x, y);
    assert_eq!(h.app.editor.selected_text().as_deref(), Some("beta"));
    h.click(x, y);
    assert_eq!(
        h.app.editor.selected_text().as_deref(),
        Some("alpha beta gamma")
    );
}

#[test]
fn drag_selects_transcript_text_and_ctrl_c_copies_it() {
    let mut h = Harness::new(60, 20);
    h.app.push_user("hello brave world".into());
    h.app.push_assistant("second reply".into());
    h.draw();
    let row = h.row_of("hello brave world");
    let (x0, y0) = h.t_cell(row, 6);
    let (x1, _) = h.t_cell(row, 11);
    h.down(x0, y0);
    h.drag(x1, y0);
    h.up(x1, y0);
    assert_eq!(h.app.view.selected_text().as_deref(), Some("brave"));

    // Ctrl+C with a selection copies instead of quitting/cancelling.
    assert!(!h.key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(clipboard::get_text().unwrap(), "brave");
    assert!(
        h.app.view.selected_text().is_none(),
        "selection clears after copy"
    );
    // With nothing selected and idle, Ctrl+C asks for a second press before quitting.
    assert!(!h.key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(h.key(KeyCode::Char('c'), KeyModifiers::CONTROL));
}

#[test]
fn drag_across_messages_keeps_line_breaks() {
    let mut h = Harness::new(60, 20);
    h.app.push_user("first line".into());
    h.app.push_assistant("second line".into());
    // No reply label between the rows: this test is about line breaks.
    h.app.messages.last_mut().unwrap().meta = None;
    h.draw();
    let a = h.row_of("first line");
    let (x0, y0) = h.t_cell(a, 6);
    let (x1, y1) = h.t_cell(a + 1, 6);
    h.down(x0, y0);
    h.drag(x1, y1);
    h.up(x1, y1);
    assert_eq!(h.app.view.selected_text().as_deref(), Some("line\nsecond"));
}

#[test]
fn double_click_selects_word_triple_selects_line_in_transcript() {
    let mut h = Harness::new(60, 20);
    h.app.push_assistant("alpha beta gamma".into());
    h.draw();
    let r = h.row_of("alpha beta gamma");
    let (x, y) = h.t_cell(r, 7);
    h.click(x, y);
    h.click(x, y);
    assert_eq!(h.app.view.selected_text().as_deref(), Some("beta"));
    h.click(x, y);
    assert_eq!(
        h.app.view.selected_text().as_deref(),
        Some("alpha beta gamma")
    );
}

#[test]
fn click_without_drag_does_not_leave_a_selection() {
    let mut h = Harness::new(60, 20);
    h.app.push_assistant("alpha beta".into());
    h.draw();
    let (x, y) = h.t_cell(h.row_of("alpha beta"), 2);
    h.click(x, y);
    assert!(h.app.view.selected_text().is_none());
}

#[test]
fn selection_highlight_is_rendered() {
    let mut h = Harness::new(60, 20);
    h.app.push_assistant("alpha beta".into());
    h.draw();
    let r = h.row_of("alpha beta");
    let (x0, y) = h.t_cell(r, 0);
    let (x1, _) = h.t_cell(r, 5);
    h.down(x0, y);
    h.drag(x1, y);
    h.up(x1, y);
    let buf = h.term.backend().buffer().clone();
    assert!(buf[(x0, y)].modifier.contains(Modifier::REVERSED));
    assert!(!buf[(x1 + 1, y)].modifier.contains(Modifier::REVERSED));
}

#[test]
fn ctrl_shift_c_and_slash_copy_fall_back_to_last_reply() {
    let mut h = Harness::new(60, 20);
    h.app.push_assistant("the answer".into());
    h.draw();
    h.key(
        KeyCode::Char('C'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert_eq!(clipboard::get_text().unwrap(), "the answer");

    h.app.push_assistant("newer answer".into());
    h.type_str("/copy");
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(clipboard::get_text().unwrap(), "newer answer");
}

#[test]
fn ctrl_c_cancels_busy_turn_when_nothing_selected() {
    let mut h = Harness::new(60, 20);
    h.app.busy = true;
    assert!(!h.key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(!h.app.busy, "cancel path ran");
}

#[test]
fn cut_copy_paste_in_prompt() {
    let mut h = Harness::new(60, 20);
    h.type_str("one two");
    h.key(KeyCode::Char('a'), KeyModifiers::CONTROL);
    h.key(KeyCode::Char('x'), KeyModifiers::CONTROL);
    assert_eq!(h.app.editor.text(), "");
    assert_eq!(clipboard::get_text().unwrap(), "one two");
    h.key(KeyCode::Char('v'), KeyModifiers::CONTROL);
    h.key(KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert_eq!(h.app.editor.text(), "one twoone two");
}

#[test]
fn keyboard_navigation_and_selection() {
    let mut h = Harness::new(60, 20);
    h.type_str("alpha beta");
    h.key(KeyCode::Home, KeyModifiers::NONE);
    h.key(KeyCode::Right, KeyModifiers::SHIFT);
    h.key(KeyCode::Right, KeyModifiers::SHIFT);
    assert_eq!(h.app.editor.selected_text().as_deref(), Some("al"));
    h.key(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    assert_eq!(h.app.editor.selected_text().as_deref(), Some("alpha"));
    h.key(KeyCode::Esc, KeyModifiers::NONE); // clears selection, does not quit
    assert!(h.app.editor.selected_text().is_none());
    assert!(!h.app.should_quit);
    h.key(KeyCode::End, KeyModifiers::NONE);
    h.key(KeyCode::Backspace, KeyModifiers::CONTROL);
    assert_eq!(h.app.editor.text(), "alpha ");
}

#[test]
fn bracketed_paste_keeps_newlines_without_submitting() {
    let mut h = Harness::new(60, 20);
    let before = h.app.messages.len();
    h.app.paste_text("line one\nline two\nline three");
    h.draw();
    assert_eq!(h.app.messages.len(), before, "paste must not submit");
    assert_eq!(h.app.editor.text(), "line one\nline two\nline three");
    // Prompt box grew to fit three text rows (+ 2 border rows).
    assert_eq!(h.app.areas.input.height, 3);
}

#[test]
fn prompt_box_scrolls_and_caret_stays_visible_when_text_is_tall() {
    let mut h = Harness::new(40, 24);
    let text = (0..20)
        .map(|i| format!("row{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    h.app.paste_text(&text);
    h.draw();
    assert_eq!(h.app.areas.input.height as usize, MAX_INPUT_ROWS);
    let cur = h.term.get_cursor_position().unwrap();
    let r = h.app.areas.input;
    assert!(cur.y >= r.y && cur.y < r.y + r.height);
    // The top visible row maps to a scrolled-down line, not line 0.
    let (x, y) = h.i_cell(0, 0);
    h.click(x, y);
    assert!(h.app.editor.cursor() > 0);
}

#[test]
fn wheel_and_page_keys_scroll_transcript() {
    let mut h = Harness::new(60, 14);
    for i in 0..40 {
        h.app.push_assistant(format!("line {i}"));
    }
    h.draw();
    assert_eq!(h.app.view.scroll, 0);
    h.mouse(MouseEventKind::ScrollUp, 10, 5);
    assert_eq!(h.app.view.scroll, 3);
    h.key(KeyCode::PageUp, KeyModifiers::NONE);
    assert!(h.app.view.scroll > 3);
    for _ in 0..20 {
        h.key(KeyCode::PageDown, KeyModifiers::NONE);
    }
    assert_eq!(h.app.view.scroll, 0);
    // Sending a message snaps back to the newest output.
    h.mouse(MouseEventKind::ScrollUp, 10, 5);
    h.type_str("hi");
    let (tx, _rx) = mpsc::unbounded_channel();
    h.app.submit_input(&tx);
    assert_eq!(h.app.view.scroll, 0);
}

#[test]
fn wheel_over_the_prompt_scrolls_the_draft_not_the_transcript() {
    let mut h = Harness::new(60, 16);
    for i in 0..40 {
        h.app.push_assistant(format!("line {i}"));
    }
    let draft: Vec<String> = (0..12).map(|i| format!("draft {i}")).collect();
    h.app.editor.set_text(&draft.join("\n"));
    h.draw();
    let end = h.app.editor.cursor();
    let input = h.app.areas.input;
    h.mouse(MouseEventKind::ScrollUp, input.x + 3, input.y + 1);
    assert_eq!(h.app.view.scroll, 0, "the transcript stays put");
    assert!(h.app.editor.cursor() < end, "the caret moved up the draft");
    h.mouse(MouseEventKind::ScrollDown, input.x + 3, input.y + 1);
    assert_eq!(h.app.editor.cursor(), end);
}

#[test]
fn a_selection_shows_how_to_copy_it() {
    let mut h = Harness::new(80, 14);
    h.type_str("copy me");
    h.app.editor.select_all();
    h.draw();
    assert!(footer_row(&h).contains("right-click or Ctrl+C to copy"));
    h.app.editor.clear_selection();
    h.draw();
    assert!(!footer_row(&h).contains("right-click"));
}

#[test]
fn dragging_below_transcript_scrolls_toward_newer_output() {
    let mut h = Harness::new(60, 14);
    for i in 0..40 {
        h.app.push_assistant(format!("line {i}"));
    }
    h.draw();
    h.app.view.scroll_up(10);
    h.draw();
    let t = h.app.areas.transcript;
    h.down(t.x + 6, t.y);
    let before = h.app.view.scroll;
    h.drag(t.x + 6, t.y + t.height + 2);
    assert!(
        h.app.view.scroll < before,
        "autoscroll while dragging past the bottom"
    );
    h.up(t.x + 6, t.y + t.height + 2);
    assert!(h.app.view.selected_text().is_some());
}

#[test]
fn mouse_is_ignored_while_a_picker_is_open() {
    let mut h = Harness::new(60, 20);
    h.app.push_assistant("alpha beta".into());
    h.app.show_picker(
        PickerKind::Trust,
        "t".into(),
        vec![PickerRow {
            id: "x".into(),
            label: "x".into(),
            detail: String::new(),
        }],
        false,
    );
    h.draw();
    let t = h.app.areas.transcript;
    h.down(t.x + 4, t.y);
    assert!(h.app.view.sel.is_none());
}

#[test]
fn multi_click_tracker_cycles_and_resets() {
    let mut c = ClickTracker::default();
    let t0 = Instant::now();
    assert_eq!(c.register(1, 1, t0), 1);
    assert_eq!(c.register(1, 1, t0 + Duration::from_millis(100)), 2);
    assert_eq!(c.register(1, 1, t0 + Duration::from_millis(200)), 3);
    assert_eq!(c.register(1, 1, t0 + Duration::from_millis(300)), 1);
    assert_eq!(c.register(2, 1, t0 + Duration::from_millis(350)), 1);
    assert_eq!(c.register(2, 1, t0 + Duration::from_millis(1500)), 1);
}

// ---- account connect ---------------------------------------------------------------------

use crate::connect::{add_account_with, Effects, LoginPlan};
use std::cell::RefCell;
use std::rc::Rc;

/// Points `profile_home()` at an empty temp dir so tests never see the developer's real homes.
struct Isolated {
    _guard: std::sync::MutexGuard<'static, ()>,
    base: tempfile::TempDir,
}

impl Isolated {
    fn new() -> Self {
        let guard = crate::accounts::TEST_ENV
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let base = tempfile::tempdir().unwrap();
        std::env::set_var("CONCORDIA_CODEX_HOME_BASE", base.path());
        for k in [
            "AO_CODEX_HOME",
            "AO_CODEX_HOME_A",
            "AO_CODEX_HOME_2",
            "AO_CODEX_HOME_B",
            "AO_CLAUDE_CONFIG_DIR",
            "AO_CURSOR_HOME",
        ] {
            std::env::remove_var(k);
        }
        Isolated {
            _guard: guard,
            base,
        }
    }
}

impl Drop for Isolated {
    fn drop(&mut self) {
        std::env::remove_var("CONCORDIA_CODEX_HOME_BASE");
    }
}

impl Harness {
    /// Replace the connect step with one that records launches instead of opening windows.
    fn fake_connect(&mut self) -> Rc<RefCell<Vec<LoginPlan>>> {
        let launched = Rc::new(RefCell::new(Vec::new()));
        let rec = Rc::clone(&launched);
        self.app.connect = Box::new(move |root, provider| {
            let rec = Rc::clone(&rec);
            let launch = move |p: &LoginPlan| {
                rec.borrow_mut().push(p.clone());
                Ok(())
            };
            add_account_with(
                root,
                provider,
                &Effects {
                    has_bin: &|_| true,
                    launch: &launch,
                },
            )
        });
        launched
    }

    fn enter(&mut self) {
        self.key(KeyCode::Enter, KeyModifiers::NONE);
    }

    fn transcript_text(&self) -> String {
        self.app
            .messages
            .iter()
            .map(|m| m.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.app.on_tick();
        }
    }
}

#[test]
fn use_picker_always_offers_add_account() {
    let _iso = Isolated::new();
    let mut h = Harness::new(80, 24);
    h.type_str("/use");
    h.enter();
    let picker = h.app.picker.as_ref().expect("picker open");
    assert!(picker.kind == PickerKind::Use);
    assert!(picker.rows.iter().any(|row| row.id == "use-add"));
    assert!(picker.rows.iter().any(|row| row.id == "use-continuation"));
}

#[test]
fn add_account_flow_from_the_picker_to_a_connected_announcement() {
    let _iso = Isolated::new();
    let mut h = Harness::new(80, 24);
    let launched = h.fake_connect();

    h.type_str("/use");
    h.enter();
    h.enter(); // "+ add account"
    let picker = h.app.picker.as_ref().expect("provider picker");
    assert!(picker.kind == PickerKind::AddProvider);
    let ids: Vec<_> = picker.rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["add:codex", "add:claude", "add:cursor"]);

    h.enter(); // Codex
    assert!(h.app.picker.is_none());
    assert_eq!(launched.borrow().len(), 1);
    assert_eq!(launched.borrow()[0].script, "codex login");
    assert!(h.transcript_text().contains("login window opened"));
    let home = h
        .app
        .pending_login
        .as_ref()
        .expect("watching login")
        .home
        .clone();
    assert!(home.starts_with(h.app.state_root.join("homes")));

    // Still signing in: polling stays quiet.
    h.tick(20);
    assert!(h.app.pending_login.is_some());
    assert!(!h.transcript_text().contains("connected"));

    // The login finishes (provider CLI writes auth.json); concordia announces it, without auto-switching.
    std::fs::write(home.join("auth.json"), "{}").unwrap();
    let before = h.app.account.clone();
    h.tick(10);
    assert!(h.app.pending_login.is_none());
    assert!(h.transcript_text().contains("Codex 3 connected"));
    assert_eq!(h.app.account, before, "must not switch accounts on its own");
}

#[test]
fn slash_use_add_with_provider_skips_the_picker() {
    let _iso = Isolated::new();
    let mut h = Harness::new(80, 24);
    let launched = h.fake_connect();
    h.type_str("/use add claude");
    h.enter();
    assert!(h.app.picker.is_none());
    assert_eq!(launched.borrow()[0].script, "claude login");
    assert_eq!(launched.borrow()[0].env[0].0, "CLAUDE_CONFIG_DIR");
}

#[test]
fn a_second_add_is_refused_while_a_login_is_pending() {
    let _iso = Isolated::new();
    let mut h = Harness::new(80, 24);
    let launched = h.fake_connect();
    h.type_str("/use add codex");
    h.enter();
    h.type_str("/use add claude");
    h.enter();
    assert_eq!(launched.borrow().len(), 1);
    assert!(h.transcript_text().contains("already in progress"));
}

#[test]
fn an_unfinished_login_is_given_up_on_with_a_retry_hint() {
    let _iso = Isolated::new();
    let mut h = Harness::new(80, 24);
    h.fake_connect();
    h.type_str("/use add codex");
    h.enter();
    h.app.pending_login.as_mut().unwrap().ticks = LOGIN_GIVE_UP_TICKS - 1;
    h.tick(1);
    assert!(h.app.pending_login.is_none());
    assert!(h.transcript_text().contains("/use add codex"));
}

#[test]
fn missing_cli_is_reported_not_swallowed() {
    let _iso = Isolated::new();
    let mut h = Harness::new(80, 24);
    h.app.connect = Box::new(|root, provider| {
        add_account_with(
            root,
            provider,
            &Effects {
                has_bin: &|_| false,
                launch: &|_| Ok(()),
            },
        )
    });
    h.type_str("/use add codex");
    h.enter();
    assert!(h.app.pending_login.is_none());
    assert!(h.transcript_text().contains("add account failed"));
    assert!(h.transcript_text().contains("codex not found on PATH"));
}

#[test]
fn unknown_provider_is_a_clear_message() {
    let _iso = Isolated::new();
    let mut h = Harness::new(80, 24);
    h.fake_connect();
    h.type_str("/use add gemini");
    h.enter();
    assert!(h.transcript_text().contains("unknown provider 'gemini'"));
}

#[test]
fn the_isolated_profile_is_empty_so_tests_never_touch_real_homes() {
    let iso = Isolated::new();
    assert_eq!(crate::accounts::profile_home(), iso.base.path());
}

// ---- history, exit keys ------------------------------------------------------------------

impl Harness {
    fn send(&mut self, line: &str) {
        self.type_str(line);
        self.enter();
    }
    fn key_up(&mut self) {
        self.key(KeyCode::Up, KeyModifiers::NONE);
    }
    fn key_down(&mut self) {
        self.key(KeyCode::Down, KeyModifiers::NONE);
    }
}

#[test]
fn up_and_down_walk_sent_prompts_and_restore_the_draft() {
    let mut h = Harness::new(80, 24);
    h.send("/version");
    h.send("/config");
    h.type_str("dra");
    h.key_up();
    assert_eq!(h.app.editor.text(), "/config");
    h.key_up();
    assert_eq!(h.app.editor.text(), "/version");
    h.key_up(); // oldest entry: stays put
    assert_eq!(h.app.editor.text(), "/version");
    h.key_down();
    assert_eq!(h.app.editor.text(), "/config");
    h.key_down(); // past the newest entry: the unsent draft comes back
    assert_eq!(h.app.editor.text(), "dra");
    h.key_down();
    assert_eq!(h.app.editor.text(), "dra");
}

#[test]
fn history_skips_consecutive_duplicates_and_is_bounded() {
    let mut h = Harness::new(80, 24);
    h.send("/version");
    h.send("/version");
    assert_eq!(h.app.history.len(), 1);
    for i in 0..(HISTORY_MAX + 20) {
        h.app.remember_prompt(&format!("p{i}"));
    }
    assert_eq!(h.app.history.len(), HISTORY_MAX);
    assert_eq!(
        h.app.history.last().unwrap(),
        &format!("p{}", HISTORY_MAX + 19)
    );
}

#[test]
fn editing_a_recalled_prompt_stops_browsing() {
    let mut h = Harness::new(80, 24);
    h.send("/version");
    h.key_up();
    assert_eq!(h.app.hist_pos, Some(0));
    h.type_str("x");
    assert_eq!(h.app.hist_pos, None);
    assert_eq!(h.app.editor.text(), "/versionx");
}

#[test]
fn up_moves_the_caret_inside_a_multi_line_prompt_instead_of_recalling() {
    let mut h = Harness::new(80, 24);
    h.send("/version");
    h.app.paste_text(
        "first
second",
    );
    h.draw();
    h.key_up(); // caret is on row 1 -> moves to row 0, no recall
    assert_eq!(
        h.app.editor.text(),
        "first
second"
    );
    assert_eq!(h.app.hist_pos, None);
    h.key_up(); // now on the first row -> recall
    assert_eq!(h.app.editor.text(), "/version");
}

#[test]
fn up_with_a_selection_collapses_it_instead_of_recalling() {
    let mut h = Harness::new(80, 24);
    h.send("/version");
    h.type_str("abc");
    h.key(KeyCode::Char('a'), KeyModifiers::CONTROL);
    h.key_up();
    assert_eq!(h.app.editor.text(), "abc");
}

#[test]
fn idle_ctrl_c_needs_a_second_press_inside_the_window() {
    let mut h = Harness::new(80, 24);
    assert!(!h.key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(h.app.flash.as_ref().unwrap().0.contains("again to exit"));
    assert!(h.key(KeyCode::Char('c'), KeyModifiers::CONTROL));

    // A stale first press does not count.
    let mut h = Harness::new(80, 24);
    h.app.quit_armed = Instant::now().checked_sub(QUIT_WINDOW + Duration::from_secs(1));
    assert!(!h.key(KeyCode::Char('c'), KeyModifiers::CONTROL));
}

#[test]
fn esc_never_quits_but_cancels_a_running_turn() {
    let mut h = Harness::new(80, 24);
    h.type_str("draft");
    assert!(!h.key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!h.app.should_quit);
    assert_eq!(h.app.editor.text(), "draft", "a draft must survive Esc");

    h.app.busy = true;
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(!h.app.busy);
}

// ---- provider failure reporting ----------------------------------------------------------

use crate::provider::turn::TurnOutcome;

#[test]
fn a_failed_provider_explains_itself_instead_of_showing_an_empty_reply() {
    let _iso = Isolated::new();
    let mut h = Harness::new(80, 24);
    h.app.finalize_turn(TurnOutcome {
        exit_code: Some(3),
        stderr_tail: "warn: something\nError: login expired\nrun `codex login`".into(),
        ..Default::default()
    });
    let t = h.transcript_text();
    assert!(t.contains("provider exited with code 3"), "{t}");
    assert!(t.contains("login expired"), "{t}");
    assert!(t.contains("codex login"), "{t}");
    assert!(!t.contains("no text in provider stream"), "{t}");
}

#[test]
fn a_provider_that_could_not_start_says_so() {
    let _iso = Isolated::new();
    let mut h = Harness::new(80, 24);
    h.app.finalize_turn(TurnOutcome {
        stderr_tail: "spawn codex: program not found".into(),
        ..Default::default()
    });
    let t = h.transcript_text();
    assert!(t.contains("provider could not run"), "{t}");
    assert!(t.contains("program not found"), "{t}");
}

#[test]
fn only_the_last_few_stderr_lines_are_shown() {
    let _iso = Isolated::new();
    let mut h = Harness::new(80, 24);
    let tail = (0..12)
        .map(|i| format!("noise {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    h.app.finalize_turn(TurnOutcome {
        exit_code: Some(1),
        stderr_tail: tail,
        ..Default::default()
    });
    let t = h.transcript_text();
    assert!(t.contains("noise 11") && t.contains("noise 6"));
    assert!(!t.contains("noise 5"), "{t}");
}

#[test]
fn success_with_no_output_and_cancel_keep_their_own_messages() {
    let _iso = Isolated::new();
    let mut h = Harness::new(80, 24);
    h.app.finalize_turn(TurnOutcome {
        exit_code: Some(0),
        ..Default::default()
    });
    assert!(h.transcript_text().contains("no text in provider stream"));
    h.app.finalize_turn(TurnOutcome {
        cancelled: true,
        exit_code: Some(1),
        stderr_tail: "killed".into(),
        ..Default::default()
    });
    let t = h.transcript_text();
    assert!(t.contains("cancelled"));
    assert!(
        !t.contains("provider exited"),
        "a cancel is not a failure: {t}"
    );
}

#[test]
fn a_failed_turn_with_partial_text_keeps_the_text_and_adds_the_failure() {
    let _iso = Isolated::new();
    let mut h = Harness::new(80, 24);
    h.app.finalize_turn(TurnOutcome {
        exit_code: Some(2),
        text: "partial answer".into(),
        stderr_tail: "connection reset".into(),
        ..Default::default()
    });
    let t = h.transcript_text();
    assert!(t.contains("partial answer"));
    assert!(t.contains("provider exited with code 2"));
    assert!(t.contains("connection reset"));
}

// ---- performance -------------------------------------------------------------------------

/// A long session must stay responsive: the transcript is re-wrapped on every draw, and mouse
/// drags redraw per event.
#[test]
fn a_very_long_session_still_draws_quickly() {
    let mut h = Harness::new(120, 40);
    let para = "The quick brown fox jumps over the lazy dog. ".repeat(8);
    for i in 0..4000 {
        h.app.push_user(format!("question {i}"));
        h.app
            .push_assistant(format!("{para}\nsecond line {i}\n{para}"));
    }
    h.draw();
    let rows = h.app.view.rows.len();
    assert!(
        rows > 30_000,
        "test should build a big transcript, got {rows}"
    );

    let start = Instant::now();
    for _ in 0..20 {
        h.draw();
    }
    let per_frame = start.elapsed() / 20;
    eprintln!("{rows} rows: {per_frame:?} per frame");
    // Goal: ~16 ms for smooth dragging (measured ~17 ms in a debug build, 500 ms before the
    // row cache). The bound is loose so a loaded CI machine does not flake.
    assert!(
        per_frame < Duration::from_millis(100),
        "drawing {rows} rows took {per_frame:?} per frame"
    );
}

// ---- slash-command palette -----------------------------------------------------------------

impl Harness {
    /// The rendered screen as plain text, one line per terminal row.
    fn screen(&self) -> String {
        let buf = self.term.backend().buffer();
        let area = buf.area;
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[test]
fn typing_a_slash_previews_every_command_with_its_description() {
    let mut h = Harness::new(90, 28);
    h.type_str("/");
    let s = h.screen();
    for needle in [
        "commands",
        "/use",
        "switch, add or remove accounts",
        "/model",
        "choose the model",
    ] {
        assert!(s.contains(needle), "palette missing {needle:?}:\n{s}");
    }
    assert!(
        s.contains(&format!(
            "commands · {}",
            crate::commands::SLASH_COMMANDS.len()
        )),
        "a scrolling list shows its size:\n{s}"
    );
    assert!(h.app.areas.palette.height > 0);
    // The rest are one wrap away: Up jumps to the last command and scrolls it into view.
    h.key_up();
    let s = h.screen();
    assert!(
        s.contains("/update") && s.contains("update Concordia"),
        "{s}"
    );
}

#[test]
fn the_palette_narrows_as_you_type_and_disappears_for_normal_text() {
    let mut h = Harness::new(90, 28);
    h.type_str("/up");
    let s = h.screen();
    assert!(
        s.contains("/update") && s.contains("update Concordia"),
        "{s}"
    );
    assert!(
        !s.contains("check your setup"),
        "unrelated commands must be filtered out:\n{s}"
    );

    let mut h = Harness::new(90, 28);
    h.type_str("hello there");
    assert!(!h.screen().contains("check your setup"));
    assert_eq!(h.app.areas.palette.height, 0);

    let mut h = Harness::new(90, 28);
    h.type_str("/use");
    h.key(KeyCode::Enter, KeyModifiers::ALT); // newline: now multi-line chat text
    h.type_str("more");
    assert!(!h.screen().contains("switch account or provider"));
}

#[test]
fn arrows_move_the_selection_and_tab_completes_it() {
    let mut h = Harness::new(90, 28);
    h.type_str("/up");
    assert_eq!(h.app.palette.selected, 0);
    h.key_down();
    assert_eq!(h.app.palette.selected, 0);
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(h.app.editor.text(), "/update");
    // Moving the selection never touches the prompt text or the history.
    let mut h = Harness::new(90, 28);
    h.type_str("/");
    h.key_up(); // wraps to the last command
    assert_eq!(h.app.editor.text(), "/");
    assert_eq!(h.app.palette.selected, SLASH_COMMANDS_LEN - 1);
}

const SLASH_COMMANDS_LEN: usize = crate::commands::SLASH_COMMANDS.len();

#[test]
fn enter_completes_a_partial_command_and_runs_it_once_complete() {
    let mut h = Harness::new(90, 28);
    let before = h.app.messages.len();
    h.type_str("/mo");
    h.enter();
    assert_eq!(h.app.editor.text(), "/model", "first Enter only completes");
    assert_eq!(h.app.messages.len(), before, "nothing ran yet");
    h.enter();
    assert_eq!(
        h.app.picker.as_ref().map(|p| p.kind),
        Some(PickerKind::Model)
    );
    assert_eq!(h.app.editor.text(), "");
}

#[test]
fn esc_closes_the_palette_until_the_text_changes() {
    let mut h = Harness::new(90, 28);
    h.type_str("/up");
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(!h.screen().contains("update Concordia"));
    assert_eq!(h.app.editor.text(), "/up", "Esc must not clear the prompt");
    assert!(!h.app.should_quit);
    h.type_str("d");
    assert!(h.screen().contains("update Concordia"), "typing reopens it");
}

#[test]
fn clicking_a_suggestion_completes_it() {
    let mut h = Harness::new(90, 28);
    h.type_str("/use");
    let pal = h.app.areas.palette;
    assert!(pal.height >= 3, "palette should list /use");
    // The consolidated palette has one /use row.
    h.click(pal.x + 4, pal.y + 2);
    assert_eq!(h.app.editor.text(), "/use");
}

#[test]
fn the_palette_stays_out_of_the_way_while_browsing_history() {
    let mut h = Harness::new(90, 28);
    h.send("/version");
    h.send("/config");
    h.key_up();
    assert_eq!(h.app.editor.text(), "/config");
    assert_eq!(
        h.app.areas.palette.height, 0,
        "no palette over a recalled prompt"
    );
    h.key_up();
    assert_eq!(h.app.editor.text(), "/version");
}

// ---- duplicates and removal ----------------------------------------------------------------

use crate::test_support::codex_login;

impl Harness {
    fn picker_ids(&self) -> Vec<String> {
        self.app
            .picker
            .as_ref()
            .map(|p| p.rows.iter().map(|r| r.id.clone()).collect())
            .unwrap_or_default()
    }

    /// `/use add codex`, then finish the (faked) login as `email` and let orc notice.
    fn add_codex_signed_in_as(&mut self, email: &str) -> PathBuf {
        self.type_str("/use add codex");
        self.enter();
        let home = self
            .app
            .pending_login
            .as_ref()
            .expect("login pending")
            .home
            .clone();
        codex_login(&home, email);
        self.tick(10);
        home
    }
}

#[test]
fn signing_in_to_the_same_login_twice_removes_the_duplicate_and_says_why() {
    let iso = Isolated::new();
    codex_login(&iso.base.path().join(".codex"), "me@example.com");
    let mut h = Harness::new(100, 30);
    h.fake_connect();

    let home = h.add_codex_signed_in_as("ME@Example.com"); // same login, different case
    assert!(h.app.pending_login.is_none());
    let t = h.transcript_text();
    assert!(t.contains("already connected as Codex"), "{t}");
    assert!(t.contains("duplicate slot was removed"), "{t}");
    assert!(t.contains("sign in with a different one"), "{t}");
    assert!(
        !home.exists(),
        "the duplicate's login folder must be deleted"
    );
    assert!(crate::store::list_stored_accounts(&h.app.state_root).is_empty());
    assert!(!t.contains("connected as me@example.com - /use"), "{t}");
}

#[test]
fn a_different_login_is_kept_and_announced_with_its_address() {
    let iso = Isolated::new();
    codex_login(&iso.base.path().join(".codex"), "me@example.com");
    let mut h = Harness::new(100, 30);
    h.fake_connect();
    let home = h.add_codex_signed_in_as("other@example.com");
    let t = h.transcript_text();
    assert!(t.contains("connected as other@example.com"), "{t}");
    assert!(home.join("auth.json").is_file());
    assert_eq!(
        crate::store::list_stored_accounts(&h.app.state_root).len(),
        1
    );
}

#[test]
fn the_account_picker_names_each_account_and_marks_duplicates() {
    let iso = Isolated::new();
    codex_login(&iso.base.path().join(".codex"), "same@example.com");
    codex_login(&iso.base.path().join(".codex-2"), "same@example.com");
    codex_login(&iso.base.path().join(".codex-3"), "different@example.com");
    let mut h = Harness::new(100, 30);
    h.type_str("/use");
    h.enter();
    let rows = &h.app.picker.as_ref().unwrap().rows;
    let by_id = |id: &str| {
        rows.iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("no row {id}"))
    };
    assert!(
        by_id("use:codex").label.starts_with("Codex "),
        "{}",
        by_id("use:codex").label
    );
    assert!(
        by_id("use:codex-2").label.starts_with("Codex 2"),
        "{}",
        by_id("use:codex-2").label
    );
    assert!(by_id("use:codex-3").label.starts_with("Codex 3"));
    assert!(by_id("use:codex").detail.contains("same login as Codex 2"));
    assert!(by_id("use:codex-2").detail.contains("same login as Codex"));
    assert!(!by_id("use:codex-3").detail.contains("same login"));
    assert!(h.picker_ids().contains(&"use-remove".to_string()));
}

#[test]
fn removing_an_account_through_the_picker_deletes_what_concordia_created() {
    let _iso = Isolated::new();
    let mut h = Harness::new(100, 30);
    h.fake_connect();
    let home = h.add_codex_signed_in_as("solo@example.com");
    assert!(home.is_dir());

    h.type_str("/use remove");
    h.enter();
    assert_eq!(h.picker_ids(), ["rmpick:codex-3"]);
    h.enter(); // choose the account
    let ids = h.picker_ids();
    assert_eq!(ids, ["rm:delete:codex-3", "rm:unreg:codex-3", "rm:cancel"]);
    h.enter(); // "Remove and delete its login data"
    assert!(!home.exists());
    assert!(h.transcript_text().contains("deleted its login folder"));
    assert!(crate::store::list_stored_accounts(&h.app.state_root).is_empty());
}

#[test]
fn cancelling_the_confirmation_changes_nothing() {
    let _iso = Isolated::new();
    let mut h = Harness::new(100, 30);
    h.fake_connect();
    let home = h.add_codex_signed_in_as("solo@example.com");
    h.type_str("/use remove codex-3"); // straight to the confirmation
    h.enter();
    assert!(h.picker_ids().contains(&"rm:cancel".to_string()));
    h.key_down();
    h.key_down();
    h.enter(); // Cancel
    assert!(home.join("auth.json").is_file());
    assert_eq!(
        crate::store::list_stored_accounts(&h.app.state_root).len(),
        1
    );
}

#[test]
fn your_own_login_can_only_be_hidden_and_removing_the_active_account_switches_away() {
    let iso = Isolated::new();
    codex_login(&iso.base.path().join(".codex"), "a@example.com");
    codex_login(&iso.base.path().join(".codex-2"), "b@example.com");
    let mut h = Harness::new(100, 30);
    super::select_account(&h.app.state_root, "codex").unwrap();
    h.app.refresh_binding();
    assert_eq!(h.app.account, "codex");

    h.type_str("/use remove codex");
    h.enter();
    assert_eq!(
        h.picker_ids(),
        ["rm:hide:codex", "rm:cancel"],
        "no delete option for your own home"
    );
    h.enter(); // Hide
    let t = h.transcript_text();
    assert!(t.contains("hidden from Concordia"), "{t}");
    assert!(t.contains("now using Codex 2"), "{t}");
    assert_eq!(h.app.account, "codex-2");
    assert!(
        iso.base.path().join(".codex").join("auth.json").is_file(),
        "files untouched"
    );
}

#[test]
fn removing_with_no_accounts_or_a_wrong_name_is_a_clear_message() {
    let _iso = Isolated::new();
    let mut h = Harness::new(100, 30);
    h.type_str("/use remove");
    h.enter();
    assert!(h.transcript_text().contains("no accounts to remove"));
}

#[test]
fn ctrl_c_in_a_picker_closes_it_and_never_quits_or_loses_the_draft() {
    let _iso = Isolated::new();
    let mut h = Harness::new(100, 30);
    h.type_str("/model");
    h.enter(); // already a complete command: runs and opens the picker
    assert!(h.app.picker.is_some());
    assert!(
        !h.key(KeyCode::Char('c'), KeyModifiers::CONTROL),
        "must not quit"
    );
    assert!(h.app.picker.is_none());
    assert!(!h.app.should_quit);
    // And with a draft being typed afterwards, nothing was lost or armed for exit.
    h.type_str("draft");
    assert_eq!(h.app.editor.text(), "draft");
    assert!(h.app.quit_armed.is_none());
}

/// Prints what the screen looks like. Run: cargo test -p concordia --lib show_screen -- --ignored --nocapture
#[test]
#[ignore]
fn show_screen() {
    let _iso = Isolated::new();
    let mut h = Harness::new(100, 34);
    h.app.provider = ProviderKind::Codex;
    h.app.account = "codex-3".into();
    h.app.model = "gpt-5.1-codex".into();
    h.app.usage_summary = "5h 41% · 7d 59% · resets 3h".into();
    h.app
        .push_user("Add a /doctor check for duplicate accounts and show me the diff".into());
    for (id, title, detail, status) in [
        ("1", "shell", Some("rg -n duplicate src"), CardStatus::Done),
        ("2", "edit", Some("src/doctor.rs"), CardStatus::Done),
        (
            "3",
            "shell",
            Some("cargo test -p concordia"),
            CardStatus::Running,
        ),
    ] {
        h.app.push_card(crate::provider::stream_cards::StreamCard {
            id: id.into(),
            title: title.into(),
            detail: detail.map(str::to_string),
            status,
        });
    }
    h.app.push_assistant(
        "## What I changed\n\nI added a **duplicate check** to `doctor`. It groups accounts by provider and email, so two slots signed in to the same login are reported once.\n\n- `accounts::duplicate_groups` finds the groups\n- `doctor` prints a warning per group, naming both accounts and the fix\n- the `/use` picker marks each side with *same login as ...*\n\n```rust\nfor group in duplicate_groups(root) {\n    warn(&group);\n}\n```\n\n1. Run `/doctor` to see it\n2. Use `/use remove` to drop one".into(),
    );
    h.app.push_user("looks good, thanks".into());
    h.app.push_system("provider exited with code 1");
    h.draw();
    println!("{}\n", h.screen());

    let mut h2 = Harness::new(100, 20);
    h2.type_str("/u");
    println!("{}", h2.screen());
}

#[test]
fn assistant_markdown_renders_without_its_markers_and_user_text_is_left_alone() {
    let mut h = Harness::new(100, 24);
    h.app
        .push_assistant("## Plan\nUse **bold** and `code` here\n- one\n- two".into());
    h.app.push_user("keep **these** `as typed`".into());
    h.draw();
    let s = h.screen();
    assert!(s.contains("Plan") && !s.contains("## Plan"), "{s}");
    assert!(s.contains("Use bold and code here"), "{s}");
    assert!(
        s.contains("\u{2022} one") && s.contains("\u{2022} two"),
        "{s}"
    );
    assert!(
        s.contains("keep **these** `as typed`"),
        "user messages are not markdown:\n{s}"
    );
}

#[test]
fn assistant_paragraphs_have_one_blank_row_and_code_blanks_are_preserved() {
    let mut h = Harness::new(100, 30);
    h.app
        .push_assistant("para one\n\n\npara two\n\n```\ncode one\n\n\ncode two\n```".into());
    h.draw();
    let rows: Vec<String> = h
        .app
        .view
        .rows
        .iter()
        .map(|row| row.text.iter().collect())
        .collect();
    let first = rows.iter().position(|row| row == "para one").unwrap();
    assert_eq!(rows[first + 1], "");
    assert_eq!(rows[first + 2], "para two");
    let code = rows.iter().position(|row| row == "code one").unwrap();
    assert_eq!(rows[code + 1], "");
    assert_eq!(rows[code + 2], "");
    assert_eq!(rows[code + 3], "code two");
}

#[test]
fn unified_diff_cards_show_counts_and_review_colours() {
    use ratatui::style::{Color, Modifier};

    let mut h = Harness::new(100, 24);
    h.app.push_card(StreamCard {
        id: "edit".into(),
        title: "edit".into(),
        detail: Some(
            "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1,2 @@\n-old\n+new\n+extra".into(),
        ),
        status: CardStatus::Done,
    });
    h.draw();
    assert!(h.screen().contains("+2 -1"));

    h.key(KeyCode::Char('o'), KeyModifiers::CONTROL);
    h.draw();
    let row_texts: Vec<String> = h
        .app
        .view
        .rows
        .iter()
        .map(|row| row.text.iter().collect())
        .collect();
    let added = h
        .app
        .view
        .rows
        .iter()
        .find(|row| row.text.iter().collect::<String>().ends_with("+new"))
        .unwrap_or_else(|| panic!("rows: {row_texts:?}"));
    assert_eq!(added.style.fg, Some(Color::Green));
    let removed = h
        .app
        .view
        .rows
        .iter()
        .find(|row| row.text.iter().collect::<String>() == "-old")
        .unwrap();
    assert_eq!(removed.style.fg, Some(Color::Red));
    let header = h
        .app
        .view
        .rows
        .iter()
        .find(|row| row.text.iter().collect::<String>() == "@@ -1 +1,2 @@")
        .unwrap();
    assert_eq!(header.style.fg, Some(Color::DarkGray));
    assert!(header.style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn the_look_shows_role_markers_spacing_and_the_trust_level() {
    let mut h = Harness::new(100, 24);
    h.app.push_user("first".into());
    h.app.push_assistant("reply".into());
    h.app.push_user("second".into());
    h.draw();
    let s = h.screen();
    assert!(
        s.contains("> first") && s.contains("\u{25cf} reply") && s.contains("> second"),
        "{s}"
    );
    // A blank row separates turns.
    let lines: Vec<&str> = s.lines().collect();
    let reply = lines.iter().position(|l| l.contains("reply")).unwrap();
    assert!(lines[reply + 1].trim().is_empty(), "{s}");
    // The input box is rounded, and trust is stated in the footer (full is flagged).
    assert!(
        s.contains('\u{256d}') && s.contains("permissions: workspace"),
        "{s}"
    );
    h.app.trust = crate::store::TrustMode::Full;
    h.draw();
    assert!(h.screen().contains("permissions: full \u{26a0}"));
}

#[test]
fn a_running_turn_shows_a_spinner_and_how_to_cancel() {
    let mut h = Harness::new(100, 24);
    h.app.busy = true;
    h.draw();
    let s = h.screen();
    assert!(
        s.contains("waiting for the model") && s.contains("Esc to cancel"),
        "{s}"
    );
    assert!(SPINNER.iter().any(|f| s.contains(f)), "{s}");
}

#[test]
fn no_color_draws_with_the_terminals_own_colours_but_keeps_emphasis() {
    use ratatui::style::Color;
    let mut h = Harness::new(100, 24);
    h.app.no_color = true;
    h.app.push_user("hello".into());
    h.app.push_assistant("## Title\nsome `code` here".into());
    h.type_str("/up");
    let buf = h.term.backend().buffer().clone();
    let mut bold = 0;
    let mut reversed_or_marker = false;
    for cell in buf.content.iter() {
        assert_eq!(cell.fg, Color::Reset, "{:?}", cell.symbol());
        assert_eq!(cell.bg, Color::Reset, "{:?}", cell.symbol());
        if cell.modifier.contains(ratatui::style::Modifier::BOLD) {
            bold += 1;
        }
        if cell.symbol() == ">" {
            reversed_or_marker = true;
        }
    }
    assert!(bold > 0, "emphasis must survive without colour");
    assert!(
        reversed_or_marker,
        "the palette still marks the selected row"
    );

    // And the normal path really does use colour (so the test proves something).
    let mut c = Harness::new(100, 24);
    c.app.push_user("hello".into());
    c.draw();
    assert!(c
        .term
        .backend()
        .buffer()
        .content
        .iter()
        .any(|x| x.fg != Color::Reset));
}

// ---- background /update ---------------------------------------------------------------------------

use crate::update::{spawn_job, JobResult as JR, UpdateOutcome as UO};

impl Harness {
    fn wait_for(&mut self, mut cond: impl FnMut(&Harness) -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            self.tick(1);
            if cond(self) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }
}

#[test]
fn a_running_update_does_not_freeze_the_ui_and_shows_progress() {
    let mut h = Harness::new(100, 24);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    h.app.job = Some(spawn_job(JobKind::Update, move |ctl| {
        ctl.say("Compiling concordia v0.0.1");
        ready_tx.send(()).unwrap();
        while !ctl.cancelled() {
            std::thread::sleep(Duration::from_millis(10));
        }
        JR::Update(Err(anyhow::anyhow!("cancelled")))
    }));
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("fake update worker ready");
    h.draw();
    let s = h.screen();
    assert!(
        s.contains("updating") && s.contains("Compiling concordia") && s.contains("Esc cancels"),
        "{s}"
    );
    // Typing, scrolling and slash commands keep working while it runs.
    h.type_str("draft text");
    assert_eq!(h.app.editor.text(), "draft text");
    h.app.editor.clear();
    h.type_str("/version");
    h.enter();
    assert!(h.transcript_text().contains("concordia 0.0.1"));
    // Esc cancels it, and the user is told.
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.wait_for(|h| h.app.job.is_none()));
    assert!(
        h.transcript_text().contains("update cancelled"),
        "{}",
        h.transcript_text()
    );
    assert!(!h.app.should_quit);
}

#[test]
fn a_chat_message_is_held_back_while_an_update_is_running_but_slash_commands_are_not() {
    let mut h = Harness::new(100, 24);
    h.app.job = Some(spawn_job(JobKind::Update, |ctl| {
        while !ctl.cancelled() {
            std::thread::sleep(Duration::from_millis(10));
        }
        JR::Update(Err(anyhow::anyhow!("cancelled")))
    }));
    h.type_str("fix the bug");
    h.enter();
    assert!(!h.app.busy, "no turn may start: the re-exec would kill it");
    assert!(h.transcript_text().contains("an update is in progress"));
    assert_eq!(h.app.editor.text(), "fix the bug", "the draft is kept");
    h.app.job.as_ref().unwrap().cancel();
}

#[test]
fn a_finished_update_restarts_into_the_new_build_and_a_failed_one_explains() {
    let mut h = Harness::new(100, 24);
    let installed = std::path::PathBuf::from(r"C:\\fake\\concordia.exe");
    let p = installed.clone();
    h.app.job = Some(spawn_job(JobKind::Update, move |_| {
        JR::Update(Ok(UO::Installed(crate::update::UpdateResult {
            install_path: p,
            built_from: std::path::PathBuf::from("x"),
            log_tail: String::new(),
            summary: "updated 0.0.1 -> 0.0.2".into(),
        })))
    }));
    assert!(h.wait_for(|h| h.app.reexec_path.is_some()));
    assert_eq!(h.app.reexec_path.as_deref(), Some(installed.as_path()));
    assert!(h.transcript_text().contains("updated 0.0.1 -> 0.0.2"));
    assert!(h.app.should_quit);

    let mut h = Harness::new(100, 24);
    h.app.job = Some(spawn_job(JobKind::Update, |_| {
        JR::Update(Err(anyhow::anyhow!("cargo build failed (101)")))
    }));
    assert!(h.wait_for(|h| h.app.job.is_none()));
    assert!(h
        .transcript_text()
        .contains("update failed - cargo build failed (101)"));
    assert!(!h.app.should_quit && h.app.reexec_path.is_none());
}

/// The `/update` screen with `preview` loaded, as a finished check leaves it.
fn update_screen(h: &mut Harness, preview: crate::update::UpdatePreview) {
    h.app
        .show_picker(PickerKind::Update, "update".into(), Vec::new(), false);
    h.app.update_preview = Some(preview);
    h.draw();
}

fn update_preview(install: bool, bullets: usize) -> crate::update::UpdatePreview {
    use crate::update::PreviewLine as P;
    let mut lines = vec![
        P::Field("Installed", "0.0.1 · build 2026-10-30a".into()),
        P::Field("Available", "0.0.1 · build 2026-10-30b".into()),
        P::Blank,
        P::Title("What's new".into()),
        P::Kind("Changed".into()),
        P::Bullet(
            "The command list has 12 commands, each opening one screen, and removed commands name their replacement.".into(),
        ),
    ];
    lines.extend((0..bullets).map(|i| P::Bullet(format!("change number {i}"))));
    lines.push(P::Hint("Esc close".into()));
    crate::update::UpdatePreview {
        lines,
        install_available: install,
        release_url: None,
    }
}

#[test]
fn the_update_screen_wraps_bullets_under_their_text() {
    let mut h = Harness::new(70, 30);
    update_screen(&mut h, update_preview(true, 0));
    let screen = h.screen();
    assert!(
        screen.contains("Installed  0.0.1 · build 2026-10-30a"),
        "{screen}"
    );
    assert!(
        screen.contains(" · The command list has 12 commands"),
        "{screen}"
    );
    // The wrapped half of the bullet starts under the bullet's text, not under the dot.
    let wrapped = screen
        .lines()
        .find(|l| l.contains("removed commands") || l.contains("replacement."))
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(wrapped.contains("│    "), "{wrapped}");
}

#[test]
fn enter_installs_only_when_the_screen_offers_an_update() {
    let mut h = Harness::new(80, 30);
    update_screen(&mut h, update_preview(false, 0));
    h.enter();
    assert!(
        h.app.picker.is_some(),
        "nothing to install keeps the screen open"
    );
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.app.picker.is_none() && h.app.update_preview.is_none());

    // A busy chat defers the update with a notice, which proves Enter asked for it.
    let mut h = Harness::new(80, 30);
    update_screen(&mut h, update_preview(true, 0));
    h.app.busy = true;
    h.enter();
    assert!(h.app.picker.is_none());
    assert!(h.screen().contains("update deferred"), "{}", h.screen());
}

#[test]
fn the_update_screen_scrolls_to_its_last_line() {
    let mut h = Harness::new(80, 16);
    update_screen(&mut h, update_preview(true, 30));
    assert!(!h.screen().contains("Esc close"));
    for _ in 0..5 {
        h.key(KeyCode::PageDown, KeyModifiers::NONE);
        h.draw();
    }
    let screen = h.screen();
    assert!(screen.contains("Esc close"), "{screen}");
    assert!(screen.contains("change number 29"), "{screen}");
}

#[test]
fn the_footer_names_enter_only_when_an_update_can_be_installed() {
    for (install, footer) in [
        (true, "Enter installs · ↑↓ scroll · Esc close"),
        (false, "↑↓ scroll · Esc close"),
    ] {
        let mut h = Harness::new(100, 24);
        h.app
            .show_picker(PickerKind::Update, "update".into(), Vec::new(), false);
        h.app.job = Some(spawn_job(JobKind::Check, move |_| {
            JR::Check(Ok(update_preview(install, 40)))
        }));
        let deadline = Instant::now() + Duration::from_secs(5);
        while h.app.update_preview.is_none() && Instant::now() < deadline {
            h.app.poll_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(h.app.footer_left, footer);
    }
}

#[test]
fn a_second_update_cannot_start_while_one_is_running() {
    let mut h = Harness::new(100, 24);
    h.app.job = Some(spawn_job(JobKind::Check, |ctl| {
        while !ctl.cancelled() {
            std::thread::sleep(Duration::from_millis(10));
        }
        JR::Check(Ok(crate::update::UpdatePreview {
            lines: vec![crate::update::PreviewLine::Text("done".into())],
            install_available: false,
            release_url: None,
        }))
    }));
    h.type_str("/update");
    h.enter();
    assert!(h.transcript_text().contains("an update is already running"));
    h.app.job.as_ref().unwrap().cancel();
}

// ---- waiting states, thinking, repeated errors ----------------------------------------------------

#[test]
fn streamed_commentary_and_commands_keep_arrival_order_without_duplicate_final_reply() {
    let mut h = Harness::new(110, 35);
    h.app.push_user("check both".into());
    h.app.busy = true;
    h.app
        .apply_progress(TurnProgress::Text("Checking the first file.".into()));
    let card = |id: &str, command: &str, status| {
        TurnProgress::Card(StreamCard {
            id: id.into(),
            title: "shell".into(),
            detail: Some(command.into()),
            status,
        })
    };
    h.app
        .apply_progress(card("first", "command: first-check", CardStatus::Running));
    h.draw();
    let visible = h.screen();
    assert!(
        visible.find("Checking the first file.").unwrap() < visible.find("first-check").unwrap()
    );
    h.app.apply_progress(card(
        "first",
        "command: first-check\nOutput: passed",
        CardStatus::Done,
    ));
    h.app
        .apply_progress(TurnProgress::Text("Checking the second file.".into()));
    h.app
        .apply_progress(card("second", "command: second-check", CardStatus::Running));
    assert!(
        h.app.busy,
        "both command cards must exist before the turn finishes"
    );
    h.app.apply_progress(TurnProgress::Text("Finished.".into()));
    h.app.on_turn_done(
        TurnOutcome {
            exit_code: Some(0),
            text: "Checking the first file.\nChecking the second file.\nFinished.".into(),
            ..Default::default()
        },
        &mpsc::unbounded_channel().0,
    );
    let sequence: Vec<_> = h
        .app
        .messages
        .iter()
        .filter(|m| matches!(m.role, MsgRole::Assistant | MsgRole::Card))
        .map(|m| {
            m.card
                .as_ref()
                .map(|c| c.id.as_str())
                .unwrap_or(m.text.as_str())
        })
        .collect();
    assert_eq!(
        sequence,
        [
            "Checking the first file.",
            "first",
            "Checking the second file.",
            "second",
            "Finished."
        ]
    );
}

#[test]
fn cancelling_preserves_partial_reply_and_draft_and_repeated_stop_is_idempotent() {
    let mut h = Harness::new(100, 24);
    let (tx, mut rx) = mpsc::unbounded_channel();
    h.app.editor.set_text("original request");
    h.app.submit_input(&tx);
    rx.try_recv().unwrap();
    h.app.editor.set_text("next unsent thought");
    h.app.queue.items.push("queued work".into());
    h.app
        .apply_progress(TurnProgress::Text("Partial answer worth keeping.".into()));
    h.app.request_cancel();
    h.app.request_cancel();
    assert_eq!(
        h.app
            .messages
            .iter()
            .filter(|m| m.text == "cancelling...")
            .count(),
        1
    );
    h.app.on_turn_done(
        TurnOutcome {
            cancelled: true,
            ..Default::default()
        },
        &tx,
    );
    assert_eq!(h.app.editor.text(), "next unsent thought");
    assert!(h.app.queue.paused);
    assert!(rx.try_recv().is_err());
    assert_eq!(
        h.app
            .messages
            .iter()
            .filter(|m| m.role == MsgRole::Assistant && m.text.contains("Partial answer"))
            .count(),
        1
    );
}

#[test]
fn reply_committed_before_last_tool_is_not_duplicated_and_keeps_token_line() {
    let mut h = Harness::new(100, 24);
    h.app
        .apply_progress(TurnProgress::Text("Commentary before the command.".into()));
    h.app.apply_progress(TurnProgress::Card(StreamCard {
        id: "last".into(),
        title: "shell".into(),
        detail: Some("command: check".into()),
        status: CardStatus::Done,
    }));
    h.app.apply_progress(TurnProgress::Usage(
        crate::provider::stream_cards::TurnUsage {
            input: Some(123),
            output: Some(45),
            ..Default::default()
        },
    ));
    h.app.finalize_turn(TurnOutcome {
        exit_code: Some(0),
        text: "Commentary before the command.".into(),
        ..Default::default()
    });
    assert_eq!(
        h.app
            .messages
            .iter()
            .filter(|m| m.role == MsgRole::Assistant)
            .count(),
        1
    );
    if token_line::ui_token_line_enabled() {
        assert!(h
            .app
            .messages
            .iter()
            .any(|m| m.role == MsgRole::Meta && m.text.contains("123")));
    }
}

#[test]
fn tool_result_keeps_the_command_and_full_output_in_the_visible_card() {
    let mut h = Harness::new(100, 24);
    h.app.push_user("run the check".into());
    for (title, detail, status) in [
        ("shell", "cargo test --locked", CardStatus::Running),
        (
            "result",
            "all checks passed\nfull output retained",
            CardStatus::Done,
        ),
    ] {
        h.app.apply_progress(TurnProgress::Card(StreamCard {
            id: "call-1".into(),
            title: title.into(),
            detail: Some(detail.into()),
            status,
        }));
    }
    let cards: Vec<_> = h
        .app
        .messages
        .iter()
        .filter_map(|m| m.card.as_ref())
        .collect();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].title, "shell");
    assert_eq!(
        cards[0].detail.as_deref(),
        Some("cargo test --locked\nOutput: all checks passed\nfull output retained")
    );
    assert_eq!(cards[0].status, CardStatus::Done);
}

#[test]
fn the_wait_names_what_the_model_is_doing_and_the_colour_follows_it() {
    use ratatui::style::Color;
    let mut h = Harness::new(100, 24);
    h.app.busy = true;
    h.app.turn_started = Some(Instant::now());
    h.draw();
    assert!(h.screen().contains("waiting for the model"));

    h.app
        .apply_progress(TurnProgress::Thinking("weighing the options".into()));
    h.draw();
    let s = h.screen();
    assert!(
        s.contains("thinking") && s.contains("weighing the options"),
        "{s}"
    );
    assert_eq!(h.app.phase.color(), Color::Magenta);

    h.app.apply_progress(TurnProgress::Card(
        crate::provider::stream_cards::StreamCard {
            id: "1".into(),
            title: "Bash".into(),
            detail: Some("cargo test".into()),
            status: CardStatus::Running,
        },
    ));
    h.draw();
    assert!(h.screen().contains("running Bash"));
    assert_eq!(h.app.phase.color(), Color::Yellow);

    h.app.apply_progress(TurnProgress::Text("Done.".into()));
    h.draw();
    assert!(h.screen().contains("writing"));
    assert_eq!(h.app.phase.color(), Color::Green);
}

#[test]
fn thinking_is_kept_dimmed_after_the_turn_and_never_sent_as_context() {
    let _iso = Isolated::new();
    let mut h = Harness::new(100, 24);
    h.app.push_user("why".into());
    h.app.busy = true;
    h.app.apply_progress(TurnProgress::Thinking(
        "first   idea\n\n\nsecond idea".into(),
    ));
    h.app.apply_progress(TurnProgress::Text("Because.".into()));
    h.app.finalize_turn(TurnOutcome::default());
    let thought = h
        .app
        .messages
        .iter()
        .find(|m| m.role == MsgRole::Thinking)
        .expect("a thinking line");
    assert_eq!(thought.text, "first   idea\n\n\nsecond idea\n");
    assert!(h
        .app
        .pack_messages()
        .iter()
        .all(|m| !m.text.contains("idea")));
    assert!(h.app.live_thinking.is_empty());
}

#[test]
fn one_failure_reported_three_ways_is_shown_once_and_cli_noise_is_dropped() {
    let _iso = Isolated::new();
    let mut h = Harness::new(100, 24);
    h.app.push_user("hi".into());
    h.app.busy = true;
    let cap = "You hit your spend cap set by the owner of your workspace.";
    // The `error` event, then `turn.failed`, both stream in as the same message.
    h.app.apply_progress(TurnProgress::Error(cap.into()));
    h.app.apply_progress(TurnProgress::Error(cap.into()));
    h.app.finalize_turn(TurnOutcome {
        exit_code: Some(1),
        stderr_tail: format!("Reading additional input from stdin...\n{cap}\nsomething new"),
        ..TurnOutcome::default()
    });
    let text = h.transcript_text();
    assert_eq!(text.matches("spend cap").count(), 1, "{text}");
    assert!(!text.contains("Reading additional input"), "{text}");
    assert!(
        text.contains("something new") && text.contains("exited with code 1"),
        "{text}"
    );
}

fn footer_row(h: &Harness) -> String {
    let s = h.screen();
    s.lines().last().unwrap().to_string()
}

#[test]
fn footer_never_cuts_a_hint_mid_word_and_never_overlaps_the_mode_column() {
    let all: Vec<&str> = HINTS.iter().flat_map(|h| [h.0, h.1]).collect();
    for w in [40u16, 60, 80, 100, 140] {
        let h = Harness::new(w, 12);
        let row = footer_row(&h);
        let right = if row.ends_with("permissions: workspace") {
            "permissions: workspace"
        } else if row.ends_with("workspace") {
            "workspace"
        } else {
            ""
        };
        assert!(!right.is_empty(), "w={w}: mode column missing: {row:?}");
        let left = row[..row.len() - right.len()].to_string();
        assert!(
            left.ends_with(' '),
            "w={w}: no gap before the mode column: {row:?}"
        );
        assert!(row.chars().count() <= w as usize, "w={w}: {row:?}");
        for seg in left.trim().split(" \u{b7} ") {
            assert!(
                all.contains(&seg),
                "w={w}: cut or unknown hint {seg:?} in {row:?}"
            );
        }
        assert!(left.contains("Enter send"), "w={w}: {row:?}");
    }
    // Wide terminals keep everything; narrow ones shorten, then drop the lowest priorities.
    assert!(footer_row(&Harness::new(140, 12)).contains("Ctrl+C copy or cancel"));
    assert!(footer_row(&Harness::new(50, 12)).contains("Ctrl+C copy/stop"));
    assert!(!footer_row(&Harness::new(40, 12)).contains("drag"));
}

#[test]
fn shift_tab_cycles_the_mode_and_is_persisted_and_coloured() {
    use ratatui::style::Color;
    let mut h = Harness::new(100, 12);
    assert_eq!(h.app.trust, TrustMode::Workspace);
    let mut seen = Vec::new();
    for _ in 0..4 {
        h.key(KeyCode::BackTab, KeyModifiers::SHIFT);
        seen.push(h.app.trust);
        assert_eq!(get_settings(&h.app.state_root).trust, h.app.trust);
    }
    assert_eq!(
        seen,
        [
            TrustMode::Full,
            TrustMode::Plan,
            TrustMode::Ask,
            TrustMode::Workspace
        ]
    );
    for (mode, want) in [
        (TrustMode::Plan, Color::Cyan),
        (TrustMode::Ask, Color::Yellow),
        (TrustMode::Workspace, Color::Green),
        (TrustMode::Full, Color::Red),
    ] {
        h.app.trust = mode;
        h.draw();
        let buf = h.term.backend().buffer();
        let y = buf.area.height - 1;
        let x = buf.area.width - 3;
        assert_eq!(buf[(x, y)].fg, want, "{mode:?}");
    }
    assert!(h.screen().contains("full \u{26a0}"));
}

#[test]
fn shift_tab_moves_the_palette_up_and_tab_still_completes() {
    let mut h = Harness::new(100, 24);
    h.type_str("/use");
    let before = h.app.trust;
    h.key(KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(h.app.palette.selected, 0);
    h.key(KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!(h.app.palette.selected, 0);
    assert_eq!(
        h.app.trust, before,
        "the mode must not change while the palette is open"
    );
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(h.app.editor.text(), "/use");
}

#[test]
fn mode_command_shows_and_sets_the_mode() {
    let mut h = Harness::new(100, 24);
    h.type_str("/mode");
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    let s = h.screen();
    assert!(s.contains("mode: workspace"), "{s}");
    h.type_str("/mode plan");
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.app.trust, TrustMode::Plan);
    assert!(h.screen().contains("mode \u{2192} plan"));
    h.type_str("/mode bogus");
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.app.trust, TrustMode::Plan);
    assert!(h.screen().contains("unknown mode bogus"));
    h.type_str("/mode auto");
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.app.trust, TrustMode::Workspace);
}

#[test]
fn command_output_is_a_barred_block_and_agent_replies_carry_a_label() {
    use ratatui::style::Color;
    let mut h = Harness::new(100, 24);
    h.app.account = "codex-3".into();
    h.app.model = "gpt-5.1".into();
    h.app.push_user("hello".into());
    h.app.push_assistant("hi there".into());
    h.app.push_command("/usage");
    h.app.push_system("codex 3  5h 12% used");
    h.app.push_system("model save failed - disk full");
    h.app.push_system("now using Codex 3");
    h.draw();
    let s = h.screen();
    assert!(s.contains("Codex \u{b7} codex-3 \u{b7} gpt-5.1"), "{s}");
    assert!(s.contains("\u{25cf} hi there"), "{s}");
    assert!(s.contains("| /usage"), "{s}");
    assert!(s.contains("|   codex 3  5h 12% used"), "{s}");
    assert!(s.contains("| x model save failed"), "{s}");
    assert!(s.contains("| + now using Codex 3"), "{s}");

    // Visually distinct: a command block's text is dim, the reply's is default; errors are red.
    let buf = h.term.backend().buffer().clone();
    let find = |needle: &str| -> (u16, u16) {
        for y in 0..buf.area.height {
            let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
            if let Some(i) = row.find(needle) {
                return (row[..i].chars().count() as u16, y);
            }
        }
        panic!("{needle} not on screen");
    };
    let (x, y) = find("hi there");
    assert_eq!(buf[(x, y)].fg, Color::Reset);
    let (x, y) = find("codex 3  5h");
    assert_eq!(buf[(x, y)].fg, Color::Indexed(245));
    let (x, y) = find("model save failed");
    assert_eq!(buf[(x, y)].fg, Color::Red);
    let (x, y) = find("| /usage");
    assert_eq!(buf[(x, y)].fg, Color::Indexed(244));
    let (x, y) = find("Codex \u{b7} codex-3");
    assert_eq!(buf[(x, y)].fg, provider_color(ProviderKind::Codex));

    // NO_COLOR keeps the structure (bars, labels) with emphasis only.
    h.app.no_color = true;
    h.draw();
    let s = h.screen();
    assert!(
        s.contains("| /usage") && s.contains("Codex \u{b7} codex-3"),
        "{s}"
    );
}

#[test]
fn tool_cards_stay_grouped_under_one_reply_label() {
    let mut h = Harness::new(100, 24);
    h.app.push_user("do it".into());
    for (id, title) in [("1", "Bash"), ("2", "Edit")] {
        h.app.push_card(crate::provider::stream_cards::StreamCard {
            id: id.into(),
            title: title.into(),
            detail: None,
            status: CardStatus::Done,
        });
    }
    h.app.push_assistant("done".into());
    h.draw();
    let s = h.screen();
    let lines: Vec<&str> = s.lines().collect();
    let labels = lines.iter().filter(|l| l.contains("Codex \u{b7}")).count();
    assert_eq!(labels, 1, "{s}");
    let label = lines
        .iter()
        .position(|l| l.contains("Codex \u{b7}"))
        .unwrap();
    let reply = lines
        .iter()
        .position(|l| l.contains("\u{25cf} done"))
        .unwrap();
    assert!(
        lines[label + 1..reply].iter().all(|l| !l.trim().is_empty()),
        "no blank rows between label, cards and reply:\n{s}"
    );
}

#[test]
fn usage_is_one_block_that_updates_in_place_when_the_check_lands() {
    use crate::usage::UsageFetcher;
    let iso = Isolated::new();
    let _ = &iso;
    let mut h = Harness::new(100, 30);
    h.app.provider = ProviderKind::Claude;
    h.app.account = "claude-1".into();
    h.app.home_path = Some(std::env::temp_dir());
    let sample = "Current session: 46% used \u{b7} resets Oct 15, 9:10pm (America/Phoenix)\nCurrent week (all models): 20% used \u{b7} resets Oct 21, 9am (America/Phoenix)\n";
    let slow: crate::usage::UsageRunner = std::sync::Arc::new(move |_| {
        std::thread::sleep(Duration::from_millis(200));
        Ok(sample.to_string())
    });
    h.app.usage_fetcher = UsageFetcher::with_runner(h.app.state_root.clone(), slow);

    h.type_str("/usage");
    h.enter();
    assert_eq!(
        h.app.picker.as_ref().map(|p| p.kind),
        Some(PickerKind::Usage)
    );
}

#[test]
fn codex_usage_block_refreshes_after_a_turn_without_a_background_fetch() {
    let mut h = Harness::new(100, 30);
    let home = h._dir.path().join("synthetic-codex");
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    h.app.provider = ProviderKind::Codex;
    h.app.account = "synthetic-codex".into();
    h.app.home_path = Some(home.clone());
    let path = home.join("sessions/usage.jsonl");
    let reading = |used: u32| {
        serde_json::json!({"payload": {"rate_limits": {
            "primary": {"used_percent": used}, "credits": {"balance": "0"}
        }}})
        .to_string()
    };
    std::fs::write(&path, reading(10)).unwrap();
    h.app.show_usage(false);
    let index = h.app.usage_block.unwrap();
    assert!(h.app.messages[index].text.contains("90% left"));
    std::fs::write(&path, reading(97)).unwrap();
    h.app.finalize_turn(TurnOutcome {
        exit_code: Some(0),
        ..Default::default()
    });
    assert_eq!(h.app.usage_block, Some(index));
    assert!(h.app.messages[index].text.contains("3% left"));
    assert!(h.app.usage_summary.contains("3% left"));
}

#[test]
fn recording_appends_events_and_writes_meta_once_and_new_starts_a_fresh_thread() {
    let mut h = Harness::new(100, 24);
    assert!(h.app.thread.is_none(), "off by default");
    let root = h.app.state_root.clone();
    h.app.thread = Some(crate::threads::ThreadLog::open(&root, "tid-one"));
    h.app.push_user("fix the redirect".into());
    h.app.push_card(StreamCard {
        id: "1".into(),
        title: "Edit".into(),
        detail: Some("src/a.rs".into()),
        status: CardStatus::Done,
    });
    h.app.push_assistant("done".into());
    h.app.push_user("second".into());
    let log = crate::threads::ThreadLog::open(&root, "tid-one");
    let kinds: Vec<String> = log.read_all().unwrap().into_iter().map(|e| e.k).collect();
    assert_eq!(kinds, ["user", "card", "assistant", "user"]);
    assert_eq!(log.read_meta().unwrap().title, "fix the redirect");
    let out = h.app.threads_command("");
    assert!(out[0].contains("fix the redirect"), "{out:?}");
    assert!(h
        .app
        .threads_command("delete tid")
        .join(" ")
        .contains("current"));
    // The map built from memory sees the card.
    let pack = crate::context::build_context_pack(
        &h.app.pack_messages(),
        ProviderKind::Codex,
        ProviderKind::Claude,
        1600,
        None,
    )
    .unwrap();
    assert!(pack.text.contains("files: src/a.rs"), "{}", pack.text);
}

// ---- token line and tips ------------------------------------------------------------------

use crate::provider::stream_cards::TurnUsage;

fn finished_reply(h: &mut Harness, usage: Option<TurnUsage>) {
    h.app.push_user("question".into());
    h.app.busy = true;
    h.app.turn_started = Some(Instant::now());
    h.app
        .apply_progress(TurnProgress::Text("The answer.".into()));
    if let Some(u) = usage {
        h.app.apply_progress(TurnProgress::Usage(u));
    }
    h.app.busy = false;
    h.app.finalize_turn(TurnOutcome::default());
    h.draw();
}

#[test]
fn the_token_line_shows_reported_fields_and_the_turn_time() {
    let mut h = Harness::new(100, 24);
    finished_reply(
        &mut h,
        Some(TurnUsage {
            input: Some(12_345),
            output: Some(1400),
            cached: Some(9800),
            cost_usd: None,
        }),
    );
    let s = h.screen();
    assert!(s.contains("in 12.3k - out 1.4k - cached 9.8k - 0."), "{s}");
    assert_eq!((h.app.session_input, h.app.session_output), (12_345, 1400));
    assert!(h
        .app
        .messages
        .last()
        .is_some_and(|m| m.role == MsgRole::Meta));
}

#[test]
fn the_token_line_with_nothing_reported_is_just_the_time() {
    let mut h = Harness::new(100, 24);
    finished_reply(&mut h, None);
    let last = h.app.messages.last().unwrap();
    assert!(last.role == MsgRole::Meta);
    assert!(
        last.text.ends_with('s') && !last.text.contains("in "),
        "{}",
        last.text
    );
    let mut h = Harness::new(100, 24);
    finished_reply(
        &mut h,
        Some(TurnUsage {
            output: Some(7),
            ..Default::default()
        }),
    );
    let t = &h.app.messages.last().unwrap().text;
    assert!(t.starts_with("out 7 - "), "{t}");
}

#[test]
fn the_token_line_is_not_context_and_not_copied() {
    let mut h = Harness::new(100, 24);
    finished_reply(
        &mut h,
        Some(TurnUsage {
            input: Some(10),
            ..Default::default()
        }),
    );
    let pack = h.app.pack_messages();
    assert_eq!(pack.len(), 2);
    assert!(pack.iter().all(|m| !m.text.contains("in 10")));
    assert_eq!(h.app.last_assistant_text().as_deref(), Some("The answer."));
}

#[test]
fn the_token_line_is_dim_and_italic_and_survives_no_color() {
    use ratatui::style::Modifier;
    let mut h = Harness::new(100, 24);
    h.app.no_color = true;
    finished_reply(
        &mut h,
        Some(TurnUsage {
            input: Some(10),
            ..Default::default()
        }),
    );
    let buf = h.term.backend().buffer().clone();
    let (x, y) = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width - 1).map(move |x| (x, y)))
        .find(|&(x, y)| {
            x + 4 < buf.area.width
                && (0..4).map(|d| buf[(x + d, y)].symbol()).collect::<String>() == "in 1"
        })
        .expect("token line drawn");
    let m = buf[(x, y)].modifier;
    assert!(m.contains(Modifier::DIM) && m.contains(Modifier::ITALIC));
}

#[test]
fn usage_is_never_reply_text() {
    let mut h = Harness::new(100, 24);
    h.app.busy = true;
    h.app.apply_progress(TurnProgress::Usage(TurnUsage {
        input: Some(5),
        ..Default::default()
    }));
    assert!(h.app.live_text.is_empty());
}

#[test]
fn a_tip_shows_on_the_fresh_empty_screen_and_follows_the_seed() {
    let mut h = Harness::new(120, 24);
    h.app.tip_seed = 42;
    h.draw();
    let tip = super::tips::tip_for(42);
    assert!(
        h.screen().contains(&format!("tip: {tip}")),
        "{}",
        h.screen()
    );
    h.app.tip_seed = 43;
    h.draw();
    let other = super::tips::tip_for(43);
    assert!(h.screen().contains(&format!("tip: {other}")));
}

#[test]
fn tips_hide_when_busy_or_typing_and_wait_20s_after_a_chat() {
    let mut h = Harness::new(120, 24);
    assert!(h.screen().contains("tip: "));
    h.type_str("h");
    assert!(!h.screen().contains("tip: "));
    h.key(KeyCode::Backspace, KeyModifiers::NONE);
    assert!(h.screen().contains("tip: "));
    h.app.busy = true;
    h.draw();
    assert!(!h.screen().contains("tip: "));
    h.app.busy = false;

    // After a conversation has started the tip waits for 20 s of idle time.
    h.app.push_user("hi".into());
    h.draw();
    assert!(!h.screen().contains("tip: "));
    for _ in 0..super::tips::IDLE_TICKS - 1 {
        h.app.on_tick();
    }
    h.draw();
    assert!(!h.screen().contains("tip: "));
    h.app.on_tick();
    h.draw();
    assert!(h.screen().contains("tip: "), "{}", h.screen());
    h.type_str("x");
    h.app.on_tick();
    assert_eq!(h.app.idle_ticks, 0);
}

#[test]
fn a_user_command_shows_as_typed_and_submits_the_rendered_prompt() {
    let mut h = Harness::new(80, 20);
    let cmds = crate::user_commands::commands_dir(&h.app.state_root);
    std::fs::create_dir_all(&cmds).unwrap();
    std::fs::write(
        cmds.join("review.md"),
        "---
description: review the diff
---
Review for bugs. Focus: $ARGS",
    )
    .unwrap();
    std::fs::write(cmds.join("doctor.md"), "shadowed").unwrap();
    h.app.user_cmds = crate::user_commands::scan(&h.app.state_root).commands;

    h.type_str("/rev");
    h.draw();
    let shown: String = h
        .term
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(shown.contains("/review") && shown.contains("custom"));

    h.app.editor.clear();
    h.type_str("/review the parser");
    let (tx, mut rx) = mpsc::unbounded_channel();
    h.app.submit_input(&tx);
    assert_eq!(rx.try_recv().unwrap(), "Review for bugs. Focus: the parser");
    assert!(h.app.busy);
    let last_user = h
        .app
        .messages
        .iter()
        .rev()
        .find(|m| m.role == MsgRole::User)
        .unwrap();
    assert_eq!(last_user.text, "/review the parser");

    // A built-in name is never shadowed: /doctor stays the built-in and sends no turn.
    h.type_str("/doctor");
    h.app.busy = false;
    h.app.submit_input(&tx);
    assert!(rx.try_recv().is_err());
}

fn find_cell(buf: &ratatui::buffer::Buffer, needle: &str) -> (u16, u16) {
    (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .find(|&(x, y)| {
            let n = needle.chars().count() as u16;
            x + n <= buf.area.width
                && (0..n).map(|d| buf[(x + d, y)].symbol()).collect::<String>() == needle
        })
        .unwrap_or_else(|| panic!("{needle:?} not on screen"))
}

#[test]
fn usage_block_colours_the_left_percentage_by_tone_and_survives_no_color() {
    use ratatui::style::{Color, Modifier};
    let text = "Codex 2 - limits\n5h   78% left · 22% used · resets in 3h\n7d   12% left · 88% used · resets in 2d\n7d Opus    2% left · 98% used · resets in 2d\nchecked just now";
    let mut h = Harness::new(100, 24);
    h.app.push_system(text.to_string());
    h.draw();
    let buf = h.term.backend().buffer().clone();
    let (x, y) = find_cell(&buf, "78% left");
    assert_eq!(buf[(x, y)].fg, Color::Green);
    assert!(buf[(x, y)].modifier.contains(Modifier::BOLD));
    let (x, y) = find_cell(&buf, "12% left");
    assert_eq!(buf[(x, y)].fg, Color::Yellow);
    let (x, y) = find_cell(&buf, " 2% left");
    assert_eq!(buf[(x + 1, y)].fg, Color::Red);
    let (x, y) = find_cell(&buf, "22% used");
    assert!(buf[(x, y)].modifier.contains(Modifier::DIM));
    assert_ne!(buf[(x, y)].fg, Color::Green);
    let (x, y) = find_cell(&buf, "checked just now");
    assert!(buf[(x, y)].modifier.contains(Modifier::ITALIC));
    let (x, y) = find_cell(&buf, "Codex 2 - limits");
    assert!(buf[(x, y)].modifier.contains(Modifier::BOLD));

    h.app.no_color = true;
    h.draw();
    let buf = h.term.backend().buffer().clone();
    let (x, y) = find_cell(&buf, "78% left");
    assert_eq!(buf[(x, y)].fg, Color::Reset);
    assert!(buf[(x, y)].modifier.contains(Modifier::BOLD));
    let (x, y) = find_cell(&buf, "22% used");
    assert!(buf[(x, y)].modifier.contains(Modifier::DIM));
}

#[test]
fn a_wrapped_usage_line_keeps_its_runs() {
    use ratatui::style::Color;
    let text = "Claude - limits\n5h   78% left · 22% used · resets Oct 21, 9am and then some more words follow";
    let mut h = Harness::new(40, 24);
    h.app.push_system(text.to_string());
    h.draw();
    let buf = h.term.backend().buffer().clone();
    let (x, y) = find_cell(&buf, "78% left");
    assert_eq!(buf[(x, y)].fg, Color::Green);
}

fn sample_stats() -> crate::sysstats::Stats {
    crate::sysstats::Stats {
        self_mem: 41 << 20,
        self_cpu: 1.0,
        agents_mem: 380 << 20,
        agents_cpu: 6.0,
        agents_count: 2,
        ready: true,
    }
}

#[test]
fn expand_activity_restores_full_tool_and_reasoning_text_without_touching_draft() {
    let mut h = Harness::new(100, 24);
    let detail = format!("command {} hidden-output", "x".repeat(100));
    h.app.push_card(StreamCard {
        id: "expand-tool".into(),
        title: "shell".into(),
        detail: Some(detail),
        status: CardStatus::Done,
    });
    let thought = format!("early-reasoning {} late-reasoning", "y".repeat(700));
    h.app
        .apply_progress(TurnProgress::Thinking(thought.clone()));
    h.app.finalize_turn(TurnOutcome {
        exit_code: Some(0),
        ..Default::default()
    });
    h.app.editor.set_text("keep my draft");
    h.draw();
    let rows = |h: &Harness| {
        h.app
            .view
            .rows
            .iter()
            .flat_map(|r| r.text.iter())
            .collect::<String>()
    };
    assert!(!rows(&h).contains("hidden-output"));
    assert!(!rows(&h).contains("early-reasoning"));
    h.key(KeyCode::Char('o'), KeyModifiers::CONTROL);
    assert!(rows(&h).contains("hidden-output"));
    assert!(rows(&h).contains("early-reasoning"));
    assert_eq!(h.app.editor.text(), "keep my draft");
    h.key(KeyCode::Char('o'), KeyModifiers::CONTROL);
    assert!(!rows(&h).contains("hidden-output"));
}

#[test]
fn codex_writer_conflict_restores_prompt_and_pauses_queue_without_replay() {
    let mut h = Harness::new(100, 24);
    let (tx, mut rx) = mpsc::unbounded_channel();
    h.app.editor.set_text("retry after closing other pane");
    h.app.submit_input(&tx);
    rx.try_recv().unwrap();
    h.app.queue.items.push("next prompt".into());
    h.app.on_turn_done(
        TurnOutcome {
            exit_code: Some(1),
            stderr_tail: "thread already has an active writer".into(),
            ..Default::default()
        },
        &tx,
    );
    assert!(h.app.queue.paused);
    assert_eq!(h.app.editor.text(), "retry after closing other pane");
    assert!(rx.try_recv().is_err());
    assert!(h
        .app
        .messages
        .iter()
        .any(|m| m.text.contains("another client or pane")));
}

#[test]
fn reused_tool_ids_do_not_overwrite_cards_from_previous_turns() {
    let mut h = Harness::new(100, 24);
    let card = |detail: &str| StreamCard {
        id: "tool-0".into(),
        title: "shell".into(),
        detail: Some(detail.into()),
        status: CardStatus::Done,
    };
    h.app.push_user("first turn".into());
    h.app.push_card(card("first command"));
    h.app.push_user("second turn".into());
    h.app.push_card(card("second command"));
    h.app.push_card(card("second command updated"));
    let details: Vec<_> = h
        .app
        .messages
        .iter()
        .filter_map(|m| m.card.as_ref())
        .filter_map(|c| c.detail.as_deref())
        .collect();
    assert_eq!(details, ["first command", "second command updated"]);
}

#[test]
fn footer_stats_reserve_room_while_preserving_send_hint() {
    // Off by default: nothing extra.
    let mut h = Harness::new(160, 12);
    h.app.stats = None;
    h.draw();
    assert!(!footer_row(&h).contains("mem "));

    h.app.stats = Some(crate::sysstats::StatsHandle::fixed(sample_stats()));
    h.draw();
    let row = footer_row(&h);
    assert!(
        row.contains("mem 41 MB - cpu 1%  + agents 380 MB - 6% (2)"),
        "{row:?}"
    );
    assert!(row.contains("Enter send"), "{row:?}");

    // Stats reserve space, reducing optional hints before hiding send or truncating.
    for w in [100u16, 90, 80, 60, 40] {
        let mut h = Harness::new(w, 12);
        h.app.stats = Some(crate::sysstats::StatsHandle::fixed(sample_stats()));
        h.draw();
        let row = footer_row(&h);
        assert!(row.chars().count() <= w as usize, "w={w}: {row:?}");
        assert!(row.contains("Enter send"), "w={w}: {row:?}");
        if w >= 60 {
            assert!(row.contains("mem 41 MB"), "w={w}: {row:?}");
        }
    }
}

#[test]
fn stats_command_toggles_the_sampler_for_the_session() {
    let mut h = Harness::new(120, 12);
    h.app.stats = None;
    h.app.toggle_stats();
    assert!(h.app.stats.is_some());
    h.app.toggle_stats();
    assert!(h.app.stats.is_none());
}

fn landing_session(
    account: &str,
    cwd: &std::path::Path,
    id: &str,
    ms: u128,
) -> crate::sessions::SessionSummary {
    crate::sessions::SessionSummary {
        account: account.into(),
        provider: ProviderKind::Codex,
        session_id: id.into(),
        cwd: Some(cwd.to_string_lossy().into_owned()),
        preview: Some(format!("chat {id}")),
        modified_ms: ms,
    }
}

/// A landing page over two chats in this folder and one in a sibling `lock` folder.
fn landing_harness() -> Harness {
    let mut h = Harness::new(100, 24);
    let here = h.app.cwd.clone();
    let lock = here.parent().unwrap().join("lock");
    let sessions = vec![
        landing_session("acct-x", &here, "first", 3),
        landing_session("acct-x", &here, "second", 2),
        landing_session("acct-x", &lock, "third", 1),
    ];
    landing::open_with(&mut h.app, sessions, None);
    h.draw();
    h
}

#[test]
fn the_landing_page_lists_this_folders_chats_first_and_expanded() {
    let h = landing_harness();
    let screen = h.screen();
    let folder = h
        .app
        .cwd
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let here = screen
        .find(&format!("▾ {folder} (2)"))
        .unwrap_or_else(|| panic!("{screen}"));
    let lock = screen
        .find("▸ lock (1)")
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(here < lock, "{screen}");
    assert!(screen.contains("chat first") && screen.contains("chat second"));
    assert!(
        !screen.contains("chat third"),
        "collapsed groups hide their chats"
    );
    assert!(screen.contains(" goals "));
    assert!(
        screen.contains("Enter open chat · Tab goals · Esc chat"),
        "{screen}"
    );
}

#[test]
fn enter_on_a_chat_resumes_exactly_that_chat() {
    let mut h = landing_harness();
    h.key(KeyCode::Down, KeyModifiers::NONE);
    h.key(KeyCode::Down, KeyModifiers::NONE);
    h.enter();
    assert!(h.app.landing.is_none());
    // The fake account is not registered, so the resume path reports it by name.
    assert!(
        h.screen()
            .contains("resume failed: account is no longer registered"),
        "{}",
        h.screen()
    );
}

#[test]
fn enter_on_a_folder_expands_it_and_the_selection_stops_at_the_last_row() {
    let mut h = landing_harness();
    for _ in 0..3 {
        h.key(KeyCode::Down, KeyModifiers::NONE);
    }
    h.enter();
    assert!(h.screen().contains("▾ lock (1)") && h.screen().contains("chat third"));
    for _ in 0..10 {
        h.key(KeyCode::Down, KeyModifiers::NONE);
    }
    h.enter();
    assert!(
        h.screen().contains("resume failed"),
        "the last row is the lock chat"
    );
}

#[test]
fn typing_hides_the_landing_page_until_the_draft_is_cleared() {
    let mut h = landing_harness();
    h.type_str("hi");
    assert!(!landing::visible(&h.app));
    assert_eq!(h.app.editor.text(), "hi");
    assert!(h.screen().contains("Enter send"), "{}", h.screen());
    assert!(!h.screen().contains(" chats "));
    h.key(KeyCode::Backspace, KeyModifiers::NONE);
    h.key(KeyCode::Backspace, KeyModifiers::NONE);
    assert!(h.screen().contains(" chats "), "{}", h.screen());
    assert!(h.screen().contains("Enter open chat"), "{}", h.screen());

    let mut h = landing_harness();
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.app.landing.is_none());
    assert!(!h.screen().contains(" chats "));
}

#[test]
fn a_double_click_opens_a_chat_and_a_single_click_selects_it() {
    let mut h = landing_harness();
    let screen = h.screen();
    let y = screen
        .lines()
        .position(|l| l.contains("chat second"))
        .unwrap() as u16;
    h.click(10, y);
    assert!(h.app.landing.is_some());
    assert!(h.screen().lines().nth(y as usize).unwrap().contains("> "));
    h.click(10, y);
    assert!(h.app.landing.is_none(), "second click opens it");
    assert!(h.screen().contains("resume failed"));
}

#[test]
fn goals_are_added_kept_and_marked_done() {
    let mut h = landing_harness();
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    assert!(h.screen().contains("a add goal"));
    h.type_str("aship it");
    h.enter();
    assert!(h.screen().contains("○ ship it"), "{}", h.screen());
    let saved = landing::load_goals(&h.app.state_root);
    assert_eq!(saved.len(), 1);
    assert!(!saved[0].done);

    h.enter();
    assert!(h.screen().contains("Mark done? Enter yes · Esc no"));
    h.enter();
    assert!(!h.screen().contains("ship it"));
    assert!(landing::load_goals(&h.app.state_root)[0].done);
}

/// A project folder with a ledger, and a fake harness that prints `output`.
fn landing_with_work(output: &str) -> Harness {
    let mut h = Harness::new(110, 24);
    let here = h.app.cwd.clone();
    std::fs::create_dir_all(here.join("docs/plans")).unwrap();
    std::fs::write(here.join("docs/plans/active.md"), "# Active plan\n").unwrap();
    let script = h.app.state_root.join("fake-harness.cmd");
    std::fs::write(&script, format!("@echo off\r\necho {output}\r\n")).unwrap();
    let program = script.to_string_lossy().into_owned();
    landing::open_with(
        &mut h.app,
        vec![landing_session("acct-x", &here, "first", 1)],
        Some(program),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while h
        .app
        .landing
        .as_ref()
        .is_some_and(landing::Landing::work_pending)
        && std::time::Instant::now() < deadline
    {
        landing::poll(&mut h.app);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.draw();
    h
}

#[cfg(windows)]
#[test]
fn open_work_comes_from_the_connected_harness() {
    let h = landing_with_work(
        r#"{"plans":[{"plan":"p","slice":"Ship the landing page","owner":"o","status":"open","next":"n"}],"rows":[{"section":"backlog","id":"B2","item":"Parked polish","status":"queued"}]}"#,
    );
    let screen = h.screen();
    let folder = h
        .app
        .cwd
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(screen.contains("open to-dos"), "{screen}");
    assert!(screen.contains(&format!("{folder} · 2")), "{screen}");
    assert!(screen.contains("○ Ship the landing page") && screen.contains("○ Parked polish"));
}

#[cfg(windows)]
#[test]
fn an_old_harness_says_so_and_no_harness_shows_no_to_dos() {
    let h = landing_with_work("PASS plan:open: 0 open row(s)");
    assert!(
        h.screen()
            .contains("Harness here is too old to list open work"),
        "{}",
        h.screen()
    );
    let h = landing_harness();
    assert!(!h.screen().contains("open to-dos"));
}

#[test]
fn there_is_no_landing_page_without_chats() {
    let mut h = Harness::new(100, 24);
    landing::open_with(&mut h.app, Vec::new(), None);
    assert!(h.app.landing.is_none());
    h.draw();
    assert!(!h.screen().contains(" chats "));
}

#[test]
fn ctrl_w_is_left_to_the_terminal() {
    let mut h = Harness::new(80, 20);
    h.type_str("hello there world");
    h.key(KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert_eq!(h.app.editor.text(), "hello there world");
}

fn log_turn(h: &Harness, provider: &str, parts: (u64, u64, u64)) {
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    crate::token_log::record(
        &h.app.state_root,
        &crate::token_log::Entry {
            at,
            provider: provider.into(),
            account: "acct-x".into(),
            model: Some("auto".into()),
            project: h.app.cwd.to_string_lossy().into_owned(),
            chat: Some("chat-1".into()),
            new_input: parts.0,
            cached_input: parts.1,
            output: parts.2,
            cost_usd: None,
        },
    )
    .unwrap();
}

#[test]
fn the_tokens_screen_shows_the_log_and_closes_with_esc() {
    let mut h = Harness::new(100, 40);
    log_turn(&h, "cursor", (12_000, 36_000, 200));
    log_turn(&h, "codex", (500, 1_500, 100));
    tokens::open(&mut h.app);
    h.draw();
    let screen = h.screen();
    assert!(screen.contains(" tokens "), "{screen}");
    assert!(
        screen.contains("24 hours   50.3k tokens · 2 turns"),
        "{screen}"
    );
    assert!(screen.contains("Re-read from the cache"), "{screen}");
    assert!(screen.contains("cursor") && screen.contains("codex"));
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.app.picker.is_none());
}

#[test]
fn the_landing_page_shows_the_last_24_hours_of_tokens() {
    let mut h = Harness::new(100, 24);
    log_turn(&h, "cursor", (1_000, 2_000, 300));
    let here = h.app.cwd.clone();
    landing::open_with(
        &mut h.app,
        vec![landing_session("acct-x", &here, "first", 1)],
        None,
    );
    h.draw();
    assert!(
        h.screen().contains("3.3k tokens in 24 hours"),
        "{}",
        h.screen()
    );
}

#[test]
fn while_updating_the_message_box_says_sending_waits_for_the_restart() {
    let mut h = Harness::new(110, 24);
    h.app.job = Some(spawn_job(JobKind::Update, |ctl| {
        while !ctl.cancelled() {
            std::thread::sleep(Duration::from_millis(10));
        }
        JR::Update(Err(anyhow::anyhow!("cancelled")))
    }));
    h.draw();
    let screen = h.screen();
    assert!(screen.contains("updating Concordia"), "{screen}");
    assert!(
        screen.contains("Messages are paused until Concordia restarts on the new build"),
        "{screen}"
    );
    h.type_str("hello");
    h.enter();
    assert!(h.transcript_text().contains("an update is in progress"));
    assert_eq!(h.app.editor.text(), "hello", "the draft is kept");
    h.app.job.as_ref().unwrap().cancel();
}

fn fake_update_job() -> crate::update::Job {
    spawn_job(JobKind::Update, |ctl| {
        ctl.say("Compiling concordia");
        while !ctl.cancelled() {
            std::thread::sleep(Duration::from_millis(10));
        }
        JR::Update(Err(anyhow::anyhow!("cancelled")))
    })
}

#[test]
fn the_update_screen_turns_into_progress_and_esc_cancels_the_update() {
    let mut h = Harness::new(100, 24);
    update_screen(&mut h, update_preview(true, 0));
    h.app.job = Some(fake_update_job());
    std::thread::sleep(Duration::from_millis(50));
    h.app.update_progress();
    h.draw();
    let screen = h.screen();
    assert!(screen.contains("Updating Concordia"), "{screen}");
    assert!(screen.contains("Compiling concordia"), "{screen}");
    assert!(screen.contains("Esc cancels the update"), "{screen}");
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.app.picker.is_none());
    assert!(h
        .app
        .job
        .as_ref()
        .is_some_and(crate::update::Job::is_cancelled));
}

#[test]
fn closing_another_picker_leaves_a_running_update_alone() {
    let mut h = Harness::new(100, 24);
    h.app.job = Some(fake_update_job());
    h.app
        .show_picker(PickerKind::Usage, "usage".into(), Vec::new(), false);
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.app.picker.is_none());
    assert!(!h.app.job.as_ref().unwrap().is_cancelled());
    h.app.job.as_ref().unwrap().cancel();
}
