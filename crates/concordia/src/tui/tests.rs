//! End-to-end input tests: real key/mouse events through the handlers, rendered with ratatui's
//! `TestBackend`, asserting on text, selection, caret and clipboard (in-memory under `cfg(test)`).

use super::*;
use ratatui::backend::TestBackend;

struct Harness {
    app: App,
    term: Terminal<TestBackend>,
    _dir: tempfile::TempDir,
}

impl Harness {
    fn new(w: u16, h: u16) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
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
    assert_eq!(picker.rows.last().unwrap().id, "use-add");
    assert!(picker
        .rows
        .last()
        .unwrap()
        .detail
        .contains("no logged-in accounts yet"));
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
        "/use add",
        "connect a new account",
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
    assert!(s.contains("/exit") && s.contains("check your setup"), "{s}");
}

#[test]
fn the_palette_narrows_as_you_type_and_disappears_for_normal_text() {
    let mut h = Harness::new(90, 28);
    h.type_str("/up");
    let s = h.screen();
    assert!(
        s.contains("/update check") && s.contains("look for a newer version"),
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
    assert_eq!(h.app.palette.selected, 1);
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(h.app.editor.text(), "/update check");
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
    h.type_str("/ver");
    h.enter();
    assert_eq!(
        h.app.editor.text(),
        "/version",
        "first Enter only completes"
    );
    assert_eq!(h.app.messages.len(), before, "nothing ran yet");
    h.enter();
    assert!(
        h.transcript_text().contains("concordia 0.0.1"),
        "{}",
        h.transcript_text()
    );
    assert_eq!(h.app.editor.text(), "");
}

#[test]
fn esc_closes_the_palette_until_the_text_changes() {
    let mut h = Harness::new(90, 28);
    h.type_str("/");
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(!h.screen().contains("check your setup"));
    assert_eq!(h.app.editor.text(), "/", "Esc must not clear the prompt");
    assert!(!h.app.should_quit);
    h.type_str("d");
    assert!(h.screen().contains("check your setup"), "typing reopens it");
}

#[test]
fn clicking_a_suggestion_completes_it() {
    let mut h = Harness::new(90, 28);
    h.type_str("/use");
    let pal = h.app.areas.palette;
    assert!(pal.height >= 4, "palette should list /use and /use add");
    // First list row is one below the border; the second entry is /use add.
    h.click(pal.x + 4, pal.y + 2);
    assert_eq!(h.app.editor.text(), "/use add");
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
        s.contains('\u{256d}') && s.contains("auth workspace"),
        "{s}"
    );
    h.app.trust = crate::store::TrustMode::Full;
    h.draw();
    assert!(h.screen().contains("auth full \u{26a0}"));
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
    h.app.job = Some(spawn_job(JobKind::Update, |ctl| {
        ctl.say("Compiling concordia v0.0.1");
        while !ctl.cancelled() {
            std::thread::sleep(Duration::from_millis(10));
        }
        JR::Update(Err(anyhow::anyhow!("cancelled")))
    }));
    std::thread::sleep(Duration::from_millis(100));
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

#[test]
fn a_second_update_cannot_start_while_one_is_running() {
    let mut h = Harness::new(100, 24);
    h.app.job = Some(spawn_job(JobKind::Check, |ctl| {
        while !ctl.cancelled() {
            std::thread::sleep(Duration::from_millis(10));
        }
        JR::Check(Ok("done".into()))
    }));
    h.type_str("/update");
    h.enter();
    assert!(h.transcript_text().contains("an update is already running"));
    h.app.job.as_ref().unwrap().cancel();
}

// ---- waiting states, thinking, repeated errors ----------------------------------------------------

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
    assert_eq!(thought.text, "first idea\nsecond idea");
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
