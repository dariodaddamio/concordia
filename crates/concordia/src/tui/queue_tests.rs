//! Prompt queue, undo keys, and draft/history persistence through the real key handlers,
//! rendered with `TestBackend`. Turns are simulated: a prompt that would start a provider turn
//! lands on `rx`, and `finish` plays the fake provider's reply into `on_turn_done`.

use super::*;
use crate::provider::turn::TurnOutcome;
use ratatui::backend::TestBackend;

struct H {
    app: App,
    term: Terminal<TestBackend>,
    tx: mpsc::UnboundedSender<String>,
    rx: mpsc::UnboundedReceiver<String>,
    dir: tempfile::TempDir,
}

const NONE: KeyModifiers = KeyModifiers::NONE;

impl H {
    fn new(w: u16, h: u16) -> H {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        app.restore_state();
        let (tx, rx) = mpsc::unbounded_channel();
        let mut hn = H {
            app,
            term: Terminal::new(TestBackend::new(w, h)).unwrap(),
            tx,
            rx,
            dir,
        };
        hn.draw();
        hn
    }

    /// A new process over the same state directory.
    fn relaunch(&mut self) {
        let (w, h) = (
            self.term.size().unwrap().width,
            self.term.size().unwrap().height,
        );
        self.app = App::new(self.dir.path().to_path_buf(), self.dir.path().to_path_buf());
        self.app.restore_state();
        self.term = Terminal::new(TestBackend::new(w, h)).unwrap();
        self.draw();
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn draw(&mut self) {
        let app = &mut self.app;
        self.term.draw(|f| draw_ui(f, app)).unwrap();
    }

    fn key(&mut self, code: KeyCode, mods: KeyModifiers) -> bool {
        let quit = handle_key(&mut self.app, KeyEvent::new(code, mods), &self.tx);
        self.draw();
        quit
    }

    fn press(&mut self, code: KeyCode) {
        self.key(code, NONE);
    }

    fn type_str(&mut self, s: &str) {
        for c in s.chars() {
            self.key(KeyCode::Char(c), NONE);
        }
    }

    /// Type and press Enter.
    fn send(&mut self, s: &str) {
        self.type_str(s);
        self.press(KeyCode::Enter);
    }

    /// Type and press Tab.
    fn queue(&mut self, s: &str) {
        self.type_str(s);
        self.press(KeyCode::Tab);
    }

    /// Prompts handed to the provider so far.
    fn sent(&mut self) -> Vec<String> {
        let mut v = Vec::new();
        while let Ok(p) = self.rx.try_recv() {
            v.push(p);
        }
        v
    }

    fn finish(&mut self, outcome: TurnOutcome) {
        self.app.on_turn_done(outcome, &self.tx);
        self.draw();
    }

    fn finish_ok(&mut self) {
        self.finish(TurnOutcome {
            exit_code: Some(0),
            text: "ok".into(),
            ..Default::default()
        });
    }

    fn screen(&self) -> Vec<String> {
        let b = self.term.backend().buffer();
        (0..b.area.height)
            .map(|y| {
                (0..b.area.width)
                    .map(|x| b.cell((x, y)).map_or(" ", |c| c.symbol()))
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    fn row_with(&self, needle: &str) -> Option<usize> {
        self.screen().iter().position(|l| l.contains(needle))
    }

    fn items(&self) -> Vec<&str> {
        self.app.queue.items.iter().map(String::as_str).collect()
    }

    fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.app.on_tick();
        }
    }
}

fn saved_recovery(root: &Path) -> Option<recovery::RecoveryData> {
    let dir = recovery::recovery_root(root, root).ok()?;
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let bytes = std::fs::read(e.path().join("recovery.json")).ok()?;
            serde_json::from_slice::<recovery::RecoveryData>(&bytes).ok()
        })
        .max_by_key(|data| data.updated_unix_ms)
}

fn saved_draft(root: &Path) -> Option<String> {
    saved_recovery(root)
        .map(|data| data.draft)
        .filter(|s| !s.is_empty())
}

fn queue_json(root: &Path) -> Option<String> {
    let data = saved_recovery(root)?;
    (!data.queue.is_empty()).then(|| serde_json::to_string(&data.queue).unwrap())
}

// ---- Tab ---------------------------------------------------------------------------------

#[test]
fn tab_queues_only_while_busy_or_when_the_queue_is_not_empty() {
    let mut h = H::new(80, 24);
    // idle, empty queue: Tab still completes slash commands and never queues
    h.type_str("/vers");
    h.press(KeyCode::Tab);
    assert_eq!(h.app.editor.text(), "/version");
    assert!(h.app.queue.items.is_empty());
    h.app.editor.clear();
    // idle, plain text, empty queue: nothing to queue behind
    h.queue("hello");
    assert!(h.app.queue.items.is_empty());
    assert_eq!(h.app.editor.text(), "hello");
    h.app.editor.clear();

    h.send("first");
    assert_eq!(h.sent(), vec!["first"]);
    assert!(h.app.busy);
    h.queue("second");
    assert_eq!(h.items(), vec!["second"]);
    assert_eq!(h.app.editor.text(), "", "the prompt box is free again");
    // a slash prompt is never queued: Tab completes it
    h.type_str("/vers");
    h.press(KeyCode::Tab);
    assert_eq!(h.items(), vec!["second"]);
    assert_eq!(h.app.editor.text(), "/version");
    h.app.editor.clear();
    // with a non-empty queue, Tab queues even when idle
    h.app.busy = false;
    h.queue("third");
    assert_eq!(h.items(), vec!["second", "third"]);
}

// ---- layout ------------------------------------------------------------------------------

#[test]
fn panel_sits_directly_above_the_prompt_at_every_width() {
    for w in [60u16, 100, 140] {
        let mut h = H::new(w, 24);
        h.send("go");
        h.queue("alpha");
        h.queue("bravo");
        h.queue(&"x".repeat(100));
        let scr = h.screen();
        let r1 = h.row_with("queued 1: alpha").expect("row 1");
        assert_eq!(scr[r1 + 1].trim(), "queued 2: bravo", "w={w}");
        assert!(
            scr[r1 + 2].contains(&format!("queued 3: {}...", "x".repeat(60))) || w < 80,
            "w={w}: {:?}",
            scr[r1 + 2]
        );
        // the prompt box top border follows the panel immediately
        assert!(scr[r1 + 3].contains('\u{256d}'), "w={w}: {:?}", scr[r1 + 3]);
        assert!(scr.iter().all(|l| l.chars().count() <= w as usize));
    }
}

#[test]
fn more_than_five_shows_five_rows_then_a_count() {
    let mut h = H::new(100, 30);
    h.send("go");
    for i in 1..=8 {
        h.queue(&format!("item {i}"));
    }
    let scr = h.screen();
    let first = h.row_with("queued 1: item 1").unwrap();
    for i in 0..5 {
        assert!(scr[first + i].contains(&format!("queued {}: item {}", i + 1, i + 1)));
    }
    assert_eq!(scr[first + 5].trim(), "+3 more");
    assert!(h.row_with("queued 6").is_none());
    assert!(scr[first + 6].contains('\u{256d}'));
}

#[test]
fn a_tiny_terminal_never_panics_and_keeps_the_prompt() {
    let mut h = H::new(40, 8);
    h.send("go");
    for i in 0..6 {
        h.queue(&format!("item {i}"));
    }
    assert!(h.row_with("waiting").is_some());
}

// ---- panel keys --------------------------------------------------------------------------

fn three_queued() -> H {
    let mut h = H::new(100, 24);
    h.send("go");
    h.queue("one");
    h.queue("two");
    h.queue("three");
    h
}

#[test]
fn up_with_an_empty_draft_focuses_the_panel_and_arrows_select() {
    let mut h = three_queued();
    h.press(KeyCode::Up);
    assert!(h.app.queue.focused);
    assert_eq!(h.app.queue.sel, 2);
    h.press(KeyCode::Up);
    h.press(KeyCode::Up);
    h.press(KeyCode::Up); // clamps at the first row
    assert_eq!(h.app.queue.sel, 0);
    h.press(KeyCode::Down);
    assert_eq!(h.app.queue.sel, 1);
    h.press(KeyCode::Esc);
    assert!(!h.app.queue.focused);
    assert!(
        h.app.busy,
        "Esc in the panel returns to the prompt, it does not cancel the turn"
    );
    assert_eq!(h.items(), vec!["one", "two", "three"]);
}

#[test]
fn up_walks_history_when_the_queue_is_empty_or_the_draft_is_not() {
    let mut h = H::new(100, 24);
    h.send("earlier");
    h.finish_ok();
    h.press(KeyCode::Up);
    assert_eq!(h.app.editor.text(), "earlier");
    assert!(!h.app.queue.focused);
    // a non-empty draft never focuses the panel
    let mut h = three_queued();
    h.type_str("typing");
    h.press(KeyCode::Up);
    assert!(!h.app.queue.focused);
}

#[test]
fn alt_arrows_reorder_and_delete_removes() {
    let mut h = three_queued();
    h.press(KeyCode::Up); // selects "three"
    h.key(KeyCode::Up, KeyModifiers::ALT);
    assert_eq!(h.items(), vec!["one", "three", "two"]);
    assert_eq!(h.app.queue.sel, 1);
    h.key(KeyCode::Up, KeyModifiers::ALT);
    h.key(KeyCode::Up, KeyModifiers::ALT); // already first: stays
    assert_eq!(h.items(), vec!["three", "one", "two"]);
    assert_eq!(h.app.queue.sel, 0);
    h.key(KeyCode::Down, KeyModifiers::ALT);
    assert_eq!(h.items(), vec!["one", "three", "two"]);
    h.press(KeyCode::Delete);
    assert_eq!(h.items(), vec!["one", "two"]);
    assert!(h.app.queue.focused);
    // the file follows every change
    let json = queue_json(h.root()).unwrap();
    assert!(json.contains("one") && json.contains("two") && !json.contains("three"));
    h.press(KeyCode::Delete);
    h.press(KeyCode::Delete);
    assert!(h.app.queue.items.is_empty());
    assert!(!h.app.queue.focused);
    assert!(
        queue_json(h.root()).is_none(),
        "an empty queue deletes the file"
    );
}

#[test]
fn enter_edits_in_place_enter_saves_esc_cancels() {
    let mut h = three_queued();
    h.press(KeyCode::Up);
    h.press(KeyCode::Up); // "two"
    h.press(KeyCode::Enter);
    assert!(h.app.queue.edit.is_some());
    assert!(h.row_with("edit 2: two").is_some());
    h.type_str("!!");
    h.press(KeyCode::Esc); // cancel
    assert_eq!(h.items(), vec!["one", "two", "three"]);
    assert!(h.app.queue.focused && h.app.queue.edit.is_none());
    h.press(KeyCode::Enter);
    h.key(KeyCode::Backspace, KeyModifiers::CONTROL);
    h.type_str("deux");
    h.press(KeyCode::Enter); // save
    assert_eq!(h.items(), vec!["one", "deux", "three"]);
    assert!(queue_json(h.root()).unwrap().contains("deux"));
    // emptying an item and saving removes it
    h.press(KeyCode::Enter);
    h.key(KeyCode::Char('a'), KeyModifiers::CONTROL);
    h.press(KeyCode::Backspace);
    h.press(KeyCode::Enter);
    assert_eq!(h.items(), vec!["one", "three"]);
}

#[test]
fn editing_a_queued_prompt_never_touches_the_draft() {
    let mut h = three_queued();
    h.type_str("half a thought");
    // a click on a row focuses the panel without disturbing the draft
    let row = h.row_with("queued 2: two").unwrap() as u16;
    h.mouse_down(3, row);
    assert!(h.app.queue.focused);
    assert_eq!(h.app.queue.sel, 1);
    h.press(KeyCode::Enter);
    h.type_str(" and more");
    h.press(KeyCode::Enter);
    assert_eq!(h.items(), vec!["one", "two and more", "three"]);
    assert_eq!(h.app.editor.text(), "half a thought");
    // typing leaves the panel and goes to the draft
    h.type_str("!");
    assert!(!h.app.queue.focused);
    assert_eq!(h.app.editor.text(), "half a thought!");
}

#[test]
fn a_long_item_scrolls_inside_the_inline_editor() {
    let mut h = H::new(60, 24);
    h.send("go");
    h.queue(&"abcdefghij".repeat(10));
    h.press(KeyCode::Up);
    h.press(KeyCode::Enter);
    let pos = h.term.get_cursor_position().unwrap();
    assert!(pos.x < 60);
    assert!(h.row_with("edit 1:").is_some());
}

impl H {
    fn mouse_down(&mut self, x: u16, y: u16) {
        handle_mouse(
            &mut self.app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: NONE,
            },
        );
        self.draw();
    }
}

#[test]
fn clicking_a_row_selects_it_and_clicking_elsewhere_leaves() {
    let mut h = three_queued();
    let r3 = h.row_with("queued 3: three").unwrap() as u16;
    h.mouse_down(5, r3);
    assert!(h.app.queue.focused);
    assert_eq!(h.app.queue.sel, 2);
    let r1 = h.row_with("queued 1: one").unwrap() as u16;
    h.mouse_down(5, r1);
    assert_eq!(h.app.queue.sel, 0);
    h.mouse_down(5, 2); // transcript area
    assert!(!h.app.queue.focused);
}

// ---- sending, pausing --------------------------------------------------------------------

#[test]
fn queued_prompts_are_sent_in_order_as_each_turn_finishes() {
    let mut h = H::new(100, 24);
    h.send("one");
    assert_eq!(h.sent(), vec!["one"]);
    h.queue("two");
    h.queue("three");
    h.type_str("my draft");
    h.finish_ok();
    assert_eq!(h.sent(), vec!["two"], "exactly one at a time");
    assert!(h.app.busy);
    assert_eq!(h.items(), vec!["three"]);
    assert_eq!(h.app.editor.text(), "my draft", "the draft is left alone");
    assert!(h.row_with("queued 1: three").is_some());
    assert!(h.row_with("queued 2").is_none());
    h.finish_ok();
    assert_eq!(h.sent(), vec!["three"]);
    assert!(h.app.queue.items.is_empty());
    assert!(queue_json(h.root()).is_none());
    h.finish_ok();
    assert!(h.sent().is_empty());
    let t: Vec<&str> = h
        .app
        .messages
        .iter()
        .filter(|m| m.role == MsgRole::User)
        .map(|m| m.text.as_str())
        .collect();
    assert_eq!(t, vec!["one", "two", "three"]);
}

#[test]
fn queued_prompts_enter_history_when_they_are_sent_not_before() {
    let mut h = H::new(100, 24);
    h.send("one");
    h.queue("two");
    assert_eq!(h.app.history, vec!["one"]);
    h.finish_ok();
    assert_eq!(h.app.history, vec!["one", "two"]);
}

#[test]
fn auto_advance_waits_for_an_inline_edit_to_be_saved() {
    let mut h = H::new(100, 24);
    h.send("first");
    h.sent();
    h.queue("second");
    h.queue("third");
    h.press(KeyCode::Up);
    h.press(KeyCode::Up);
    h.press(KeyCode::Enter);
    h.type_str(" revised");
    h.finish_ok();
    assert!(h.sent().is_empty());
    assert_eq!(h.items(), ["second", "third"]);
    assert_eq!(h.app.queue.edit.as_ref().unwrap().text(), "second revised");
    h.press(KeyCode::Enter);
    h.app.queue_advance(&h.tx);
    assert_eq!(h.sent(), ["second revised"]);
    assert_eq!(h.items(), ["third"]);
}

#[test]
fn cancelling_pauses_the_queue_and_ctrl_r_resumes_it() {
    let mut h = H::new(100, 24);
    h.send("one");
    h.sent();
    h.queue("two");
    h.queue("three");
    h.press(KeyCode::Esc);
    assert!(h.app.queue.paused);
    h.finish(TurnOutcome {
        cancelled: true,
        ..Default::default()
    });
    assert!(h.sent().is_empty(), "nothing fires after a cancel");
    assert!(h.row_with("queue paused - Ctrl+R resumes").is_some());
    assert_eq!(
        h.app.editor.text(),
        "",
        "cancellation leaves the prompt box empty"
    );
    h.app.editor.clear();
    h.key(KeyCode::Char('r'), KeyModifiers::CONTROL);
    h.app.queue_advance(&h.tx);
    assert_eq!(h.sent(), vec!["two"]);
    assert!(!h.app.queue.paused);
    assert!(h.row_with("queue paused").is_none());
    h.finish_ok();
    assert_eq!(h.sent(), vec!["three"]);
}

#[test]
fn ctrl_c_during_a_turn_also_pauses() {
    let mut h = H::new(100, 24);
    h.send("one");
    h.queue("two");
    h.key(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(h.app.queue.paused);
}

#[test]
fn a_failed_turn_pauses_and_returns_its_prompt() {
    let mut h = H::new(100, 24);
    h.send("one");
    h.sent();
    h.queue("two");
    h.finish(TurnOutcome {
        exit_code: Some(3),
        stderr_tail: "boom".into(),
        ..Default::default()
    });
    assert!(h.sent().is_empty());
    assert!(h.app.queue.paused);
    assert_eq!(h.app.editor.text(), "one");
    assert!(h.row_with("queue paused - Ctrl+R resumes").is_some());
    assert!(h
        .app
        .messages
        .iter()
        .any(|m| m.text.contains("back in the box")));
}

#[test]
fn a_failed_prompt_is_not_put_back_over_a_new_draft() {
    let mut h = H::new(100, 24);
    h.send("one");
    h.type_str("already typing");
    h.finish(TurnOutcome {
        exit_code: Some(1),
        stderr_tail: "boom".into(),
        ..Default::default()
    });
    assert_eq!(h.app.editor.text(), "already typing");
}

#[test]
fn sending_a_prompt_while_paused_keeps_the_queue_paused() {
    let mut h = H::new(100, 24);
    h.send("one");
    h.queue("two");
    h.press(KeyCode::Esc);
    h.finish(TurnOutcome {
        cancelled: true,
        ..Default::default()
    });
    h.app.editor.set_text("one"); // Retrying is explicit; cancellation leaves the draft empty.
    h.sent();
    h.press(KeyCode::Enter); // Enter sends the explicitly entered retry.
    assert_eq!(h.sent(), vec!["one"]);
    h.finish_ok();
    assert!(h.sent().is_empty());
    assert!(h.app.queue.paused);
}

#[test]
fn queue_survives_an_account_switch_as_plain_text() {
    let mut h = H::new(100, 24);
    h.send("one");
    h.sent();
    h.queue("two");
    h.app.provider = ProviderKind::Claude;
    h.app.account = "claude-2".into();
    h.finish_ok();
    assert_eq!(h.sent(), vec!["two"]);
}

#[test]
fn queue_command_lists_and_clears() {
    let mut h = H::new(100, 24);
    h.send("one");
    h.queue("two");
    h.queue("three\nlines");
    h.send("/queue");
    let t: Vec<String> = h.app.messages.iter().map(|m| m.text.clone()).collect();
    assert!(t.iter().any(|l| l == "2 queued"), "{t:?}");
    assert!(t.iter().any(|l| l == "queued 2: three lines"), "{t:?}");
    h.send("/queue clear");
    assert!(h.app.queue.items.is_empty());
    assert!(h
        .app
        .messages
        .iter()
        .any(|m| m.text.contains("cleared 2 queued prompts")));
    assert!(queue_json(h.root()).is_none());
    assert!(
        h.sent() == vec!["one".to_string()],
        "slash commands never became turns"
    );
}

#[test]
fn queue_commands_are_in_the_palette() {
    let cmds: Vec<&str> = crate::commands::SLASH_COMMANDS
        .iter()
        .map(|c| c.0)
        .collect();
    assert!(cmds.contains(&"/queue") && cmds.contains(&"/queue clear"));
    let mut h = H::new(100, 24);
    h.type_str("/que");
    assert!(h.row_with("/queue clear").is_some());
}

#[test]
fn the_busy_footer_offers_tab_queue() {
    let mut h = H::new(100, 24);
    assert!(h.row_with("Tab queue").is_none());
    h.send("go");
    assert!(h.row_with("Tab queue").is_some());
    h.finish_ok();
    assert!(h.row_with("Tab queue").is_none());
}

// ---- persistence -------------------------------------------------------------------------

#[test]
fn queue_round_trips_through_a_restart_paused_with_a_notice() {
    let mut h = H::new(100, 24);
    h.send("one");
    h.sent();
    h.queue("two");
    h.queue("three\nlines");
    h.relaunch();
    assert_eq!(h.items(), vec!["two", "three\nlines"]);
    assert!(h.app.queue.paused);
    assert!(h.app.messages.iter().any(|m| m
        .text
        .contains("restored your unsent draft and queued prompts")));
    assert!(h.row_with("queue paused - Ctrl+R resumes").is_some());
    h.key(KeyCode::Char('r'), KeyModifiers::CONTROL);
    h.app.queue_advance(&h.tx);
    assert_eq!(h.sent(), vec!["two"]);
}

#[test]
fn a_corrupt_queue_file_is_ignored() {
    let mut h = H::new(100, 24);
    std::fs::write(h.root().join("queue.json"), b"{{{").unwrap();
    h.relaunch();
    h.app.import_legacy_recovery().unwrap();
    assert!(h.app.queue.items.is_empty());
    assert!(h.root().join("queue.json.bad").exists());
}

// ---- undo / redo keys --------------------------------------------------------------------

#[test]
fn ctrl_z_undoes_and_every_redo_chord_redoes() {
    for redo in [
        (
            KeyCode::Char('Z'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ),
        (
            KeyCode::Char('z'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ),
        (KeyCode::Char('Z'), KeyModifiers::CONTROL),
        (KeyCode::Char('y'), KeyModifiers::CONTROL),
    ] {
        let mut h = H::new(80, 24);
        h.type_str("hello world");
        h.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(h.app.editor.text(), "", "one burst, one step");
        h.key(redo.0, redo.1);
        assert_eq!(h.app.editor.text(), "hello world", "{redo:?}");
    }
}

#[test]
fn ctrl_z_right_after_a_send_brings_the_sent_text_back() {
    let mut h = H::new(80, 24);
    h.send("oops wrong chat");
    assert_eq!(h.app.editor.text(), "");
    h.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(h.app.editor.text(), "oops wrong chat");
}

#[test]
fn undo_does_not_leave_a_slash_palette_stuck() {
    let mut h = H::new(80, 24);
    h.type_str("/ver");
    h.press(KeyCode::Esc); // dismiss
    h.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(h.app.editor.text(), "");
    h.type_str("/ver");
    assert!(h.row_with("/version").is_some());
}

// ---- draft and history persistence -------------------------------------------------------

#[test]
fn the_draft_is_written_about_a_second_after_the_last_edit() {
    let mut h = H::new(80, 24);
    h.type_str("unsent thought");
    h.tick(1); // sees the change
    h.tick(3);
    assert!(saved_draft(h.root()).is_none(), "debounced");
    h.type_str("!"); // another edit restarts the wait
    h.tick(1);
    h.tick(4);
    assert!(saved_draft(h.root()).is_none());
    h.tick(1);
    assert_eq!(saved_draft(h.root()).as_deref(), Some("unsent thought!"));
    // emptying the box deletes the file
    h.app.editor.clear();
    h.tick(1);
    h.tick(5);
    assert!(saved_draft(h.root()).is_none());
}

#[test]
fn sending_removes_the_saved_draft_at_once() {
    let mut h = H::new(80, 24);
    h.type_str("will send");
    h.tick(7);
    assert!(saved_draft(h.root()).is_some());
    h.press(KeyCode::Enter);
    assert!(saved_draft(h.root()).is_none());
}

#[test]
fn the_draft_is_restored_with_a_notice_and_ctrl_z_clears_it() {
    let mut h = H::new(80, 24);
    h.app.editor.set_text("left over\nsecond line");
    h.app.persist_draft_now();
    h.relaunch();
    assert_eq!(h.app.editor.text(), "left over\nsecond line");
    assert!(h
        .app
        .messages
        .iter()
        .any(|m| m.text.contains("restored your unsent draft")));
    h.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(h.app.editor.text(), "");
    // restoring did not make it look edited: no rewrite until the user changes something
    h.relaunch();
    assert_eq!(h.app.editor.text(), "left over\nsecond line");
    h.tick(10);
    assert!(saved_draft(h.root()).is_some());
}

#[test]
fn a_corrupt_draft_file_does_not_crash_launch() {
    let mut h = H::new(80, 24);
    std::fs::write(h.root().join("draft.json"), b"\x00\x01 not json").unwrap();
    h.relaunch();
    h.app.import_legacy_recovery().unwrap();
    assert_eq!(h.app.editor.text(), "");
    assert!(h.root().join("draft.json.bad").exists());
}

#[test]
fn sent_prompts_persist_for_up_after_a_restart_but_slash_commands_do_not() {
    let mut h = H::new(80, 24);
    h.send("/queue");
    h.send("first prompt");
    h.finish_ok();
    h.send("second\nprompt");
    h.finish_ok();
    let file = std::fs::read_to_string(h.root().join("history.jsonl")).unwrap();
    assert_eq!(file.lines().count(), 2, "{file}");
    assert!(!file.contains("/queue"));
    h.relaunch();
    h.press(KeyCode::Up);
    assert_eq!(h.app.editor.text(), "second\nprompt");
    h.press(KeyCode::Up);
    h.press(KeyCode::Up);
    assert_eq!(h.app.editor.text(), "first prompt");
}

#[test]
fn history_persistence_can_be_turned_off() {
    let _g = crate::config::env_guard();
    let mut h = H::new(80, 24);
    h.app.keep.history = false;
    h.send("private");
    assert!(!h.root().join("history.jsonl").exists());
    assert_eq!(
        h.app.history,
        vec!["private"],
        "in-memory history still works"
    );
    // and a file left from an earlier run is not loaded
    persist::append_history(h.root(), "old", &mut 0);
    std::fs::write(
        h.root().join("config.toml"),
        "[ui]\npersist_history = false\n",
    )
    .unwrap();
    h.relaunch();
    assert!(h.app.history.is_empty());
}
