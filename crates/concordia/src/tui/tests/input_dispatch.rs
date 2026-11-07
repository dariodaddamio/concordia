use super::*;

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
        h.app.view.selected_text().is_some(),
        "selection stays highlighted after copy"
    );
    h.key(KeyCode::Esc, KeyModifiers::NONE);
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
    assert_eq!(h.app.flash.as_ref().unwrap().0, "cut 7 chars");
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
        PickerKind::Use,
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

#[test]
fn up_and_down_walk_sent_prompts_and_restore_the_draft() {
    let mut h = Harness::new(80, 24);
    h.send("/version");
    h.send("/doctor");
    h.type_str("dra");
    h.key_up();
    assert_eq!(h.app.editor.text(), "/doctor");
    h.key_up();
    assert_eq!(h.app.editor.text(), "/version");
    h.key_up(); // oldest entry: stays put
    assert_eq!(h.app.editor.text(), "/version");
    h.key_down();
    assert_eq!(h.app.editor.text(), "/doctor");
    h.key_down(); // past the newest entry: the unsent draft comes back
    assert_eq!(h.app.editor.text(), "dra");
    h.key_down();
    assert_eq!(h.app.editor.text(), "");
    h.key_up();
    assert_eq!(h.app.editor.text(), "dra");
}

#[test]
fn down_opens_a_fresh_prompt_and_up_preserves_both_unsent_buffers() {
    let mut h = Harness::new(80, 24);
    h.app.restore_state();
    h.type_str("unfinished thought");
    h.key_down();
    assert_eq!(h.app.editor.text(), "");
    assert_eq!(h.app.draft_text(), "unfinished thought");
    h.key_down(); // Repeated Down must not overwrite the saved draft.
    h.type_str("another thought");
    h.key_up();
    assert_eq!(h.app.editor.text(), "unfinished thought");
    h.key_down();
    assert_eq!(h.app.editor.text(), "another thought");
    h.app.persist_draft_now();
    let saved = h.app.keep.lease.as_ref().unwrap().data().unwrap();
    assert_eq!(saved.draft, "another thought");
    assert_eq!(saved.alternate_draft.as_deref(), Some("unfinished thought"));
    h.key_up();
    h.key(KeyCode::Down, KeyModifiers::SHIFT);
    assert_eq!(h.app.editor.text(), "unfinished thought");
    h.app.editor.set_text("first\nsecond");
    h.key(KeyCode::Home, KeyModifiers::CONTROL);
    h.key_down(); // Move within multiline text before switching buffers.
    assert_eq!(h.app.editor.text(), "first\nsecond");
    assert!(!h.app.fresh_draft);
}

#[test]
fn centered_bottom_button_follows_new_output_without_changing_the_draft() {
    for width in [40, 80, 120] {
        let mut h = Harness::new(width, 24);
        for i in 0..60 {
            h.app.push_assistant(format!("reply {i}"));
        }
        h.type_str("keep this draft");
        h.app.view.scroll_up(20);
        h.draw();
        assert!(h.app.view.scroll > 0);
        let button = h.app.areas.bottom_button;
        assert!(button.width > 0);
        assert_eq!(button.y, h.app.areas.input.y - 1);
        h.click(button.x + button.width / 2, button.y);
        assert_eq!(h.app.view.scroll, 0);
        assert_eq!(h.app.editor.text(), "keep this draft");
        h.app.push_assistant("newest reply".into());
        h.draw();
        assert!(h.screen().contains("newest reply"));
    }
}

#[test]
fn provider_escape_sequences_cannot_corrupt_terminal_cells() {
    let mut h = Harness::new(80, 24);
    h.app
        .push_assistant("before\u{1b}[2J\u{1b}[Hafter\u{1b}]52;c;c2VjcmV0\u{7}\tend".into());
    h.app
        .push_system("plain\toutput\u{1b}[31m red\u{1b}[0m\r\nnext");
    h.app.model = "model\u{1b}[2J".into();
    h.draw();
    let screen = h.screen();
    assert!(screen.contains("beforeafter    end"), "{screen}");
    assert!(screen.contains("plain    output red"), "{screen}");
    assert!(!screen.contains("52;c;"));
    for cell in &h.term.backend().buffer().content {
        assert!(!cell.symbol().chars().any(char::is_control));
    }
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
fn idle_notice_updates_in_place_and_clears_on_output() {
    let _iso = Isolated::new();
    let mut h = Harness::new(160, 24);
    h.type_str("draft");
    h.app.busy = true;
    h.app.busy = true;
    h.app.apply_progress(TurnProgress::Idle {
        provider: ProviderKind::Claude,
        minutes: 10,
        silent_for: Duration::from_secs(600),
    });
    h.app.apply_progress(TurnProgress::Idle {
        provider: ProviderKind::Claude,
        minutes: 20,
        silent_for: Duration::from_secs(1200),
    });
    let t = h.transcript_text();
    assert_eq!(t.matches("No output from Claude Code").count(), 1, "{t}");
    assert!(t.contains("20 minutes"), "{t}");
    h.app.apply_progress(TurnProgress::Text("back".into()));
    assert!(!h.transcript_text().contains("No output from"));
}

#[test]
fn idle_footer_hint_only_while_notice_is_shown() {
    let mut h = Harness::new(160, 24);
    h.type_str("draft");
    assert!(!h.screen().contains("Ctrl+K options"));
    h.app.apply_progress(TurnProgress::Idle {
        provider: ProviderKind::Codex,
        minutes: 10,
        silent_for: Duration::from_secs(600),
    });
    h.app.busy = true;
    h.draw();
    assert!(h.screen().contains("Ctrl+K options"), "{}", h.screen());
    h.app.apply_progress(TurnProgress::Text("output".into()));
    h.draw();
    assert!(!h.screen().contains("Ctrl+K options"));
}

#[test]
fn idle_notice_never_steals_focus() {
    let mut h = Harness::new(100, 24);
    h.type_str("draft");
    h.app.apply_progress(TurnProgress::Idle {
        provider: ProviderKind::Claude,
        minutes: 10,
        silent_for: Duration::from_secs(600),
    });
    h.draw();
    assert!(h.app.picker.is_none());
    assert_eq!(h.app.editor.text(), "draft");
    h.type_str(" more");
    assert_eq!(h.app.editor.text(), "draft more");
}

#[test]
fn idle_notice_key_and_click_open_options_and_cancel_cancels() {
    let mut h = Harness::new(100, 24);
    h.app.apply_progress(TurnProgress::Idle {
        provider: ProviderKind::Claude,
        minutes: 10,
        silent_for: Duration::from_secs(600),
    });
    h.key(KeyCode::Char('k'), KeyModifiers::CONTROL);
    let picker = h.app.picker.as_ref().expect("idle picker open");
    assert!(picker.rows.iter().any(|r| r.label == "keep waiting"));
    assert!(picker.rows.iter().any(|r| r.label == "cancel"));
    assert!(picker.rows.iter().any(|r| r.label == "hand off"));
    h.key(KeyCode::Down, KeyModifiers::NONE);
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(!h.app.busy);
    assert!(h.transcript_text().contains("cancelled"));

    h.key(KeyCode::Esc, KeyModifiers::NONE);
    h.app.cancel_requested = false;
    h.app.apply_progress(TurnProgress::Idle {
        provider: ProviderKind::Claude,
        minutes: 11,
        silent_for: Duration::from_secs(660),
    });
    h.draw();
    let row = h
        .app
        .view
        .rows
        .iter()
        .position(|r| {
            r.text
                .iter()
                .collect::<String>()
                .contains("No output from Claude Code")
        })
        .expect("idle notice rendered row");
    let (x, y) = h.t_cell(row, 2);
    h.click(x, y);
    assert!(h.app.picker.is_some());
}

#[test]
fn idle_notice_removes_only_itself() {
    let mut h = Harness::new(100, 24);
    h.app.push_system("system before");
    h.app.push_user("user line".into());
    h.app.apply_progress(TurnProgress::Idle {
        provider: ProviderKind::Claude,
        minutes: 10,
        silent_for: Duration::from_secs(600),
    });
    h.app.push_system("system after");
    h.app.apply_progress(TurnProgress::Text("reply".into()));
    let t = h.transcript_text();
    assert!(!t.contains("No output from"), "{t}");
    let positions =
        ["system before", "user line", "system after"].map(|line| t.find(line).expect(line));
    assert!(
        positions[0] < positions[1] && positions[1] < positions[2],
        "{t}"
    );
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
        rows <= crate::tui::transcript::MAX_RENDERED_ROWS,
        "transcript row cache must stay bounded, got {rows}"
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
        s.contains(&format!("commands · {}", palette::matching("/").len())),
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
    let _iso = Isolated::new();
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
    assert_eq!(h.app.palette.selected, palette::matching("/").len() - 1);
}

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
    let _iso = Isolated::new();
    let mut h = Harness::new(90, 28);
    h.type_str("/use");
    let pal = h.app.areas.palette;
    assert!(pal.height >= 3, "palette should list /use");
    // Once /use is complete, the next row is its add subcommand.
    h.click(pal.x + 4, pal.y + 2);
    assert_eq!(h.app.editor.text(), "/use add");
}

#[test]
fn the_palette_stays_out_of_the_way_while_browsing_history() {
    let mut h = Harness::new(90, 28);
    h.send("/version");
    h.send("/doctor");
    h.key_up();
    assert_eq!(h.app.editor.text(), "/doctor");
    assert_eq!(
        h.app.areas.palette.height, 0,
        "no palette over a recalled prompt"
    );
    h.key_up();
    assert_eq!(h.app.editor.text(), "/version");
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

#[test]
fn markdown_table_links_preview_exact_file_and_web_labels_open_the_url() {
    let mut h = Harness::new(100, 30);
    std::fs::write(h._dir.path().join("plan.md"), "# File preview").unwrap();
    h.app.messages = vec![ChatLine { role: MsgRole::Assistant, text:"| Area | Source |\n| --- | --- |\n| Files | [friendly](plan.md) |\n\n[plan.md](https://example.test/docs)".into(), card: None,meta:None }];
    h.draw();
    let visible = h.screen();
    assert!(!visible.contains("[friendly]("), "{visible}");
    assert!(!visible.contains("| --- |"), "{visible}");
    let (row_index, row) = h
        .app
        .view
        .rows
        .iter()
        .enumerate()
        .find(|(_, r)| !r.web_links.is_empty())
        .unwrap();
    assert!(
        row.links.is_empty(),
        "a web label shaped like a file must retain its URL target"
    );
    let col = row.web_links[0].0;
    let x = h.app.areas.transcript.x
        + row.gutter.chars().count() as u16
        + text::slice_width(&row.text[..col]) as u16;
    let y = h.app.areas.transcript.y + (row_index - h.app.view.visible().start) as u16;
    h.down(x, y);
    h.up(x, y);
    assert!(h
        .app
        .flash
        .as_ref()
        .unwrap()
        .0
        .contains("https://example.test/docs"));
    let (index, row) = h
        .app
        .view
        .rows
        .iter()
        .enumerate()
        .find(|(_, r)| !r.links.is_empty())
        .unwrap();
    let col = row.links[0].0;
    let x = h.app.areas.transcript.x
        + row.gutter.chars().count() as u16
        + text::slice_width(&row.text[..col]) as u16;
    let y = h.app.areas.transcript.y + (index - h.app.view.visible().start) as u16;
    h.down(x, y);
    h.up(x, y);
    assert!(h
        .app
        .last_file_open
        .as_ref()
        .unwrap()
        .path
        .ends_with("plan.md"));
    assert!(!h.app.busy);
}

#[test]
fn explicit_gif_link_opens_the_image_viewer_instead_of_a_text_pane() {
    let mut h = Harness::new(100, 28);
    std::fs::write(h._dir.path().join("demo.gif"), b"GIF89a").unwrap();
    h.app.editor.set_text("keep this draft");
    h.app.open_file_candidate(
        crate::file_links::explicit_reference("demo.gif").unwrap(),
        true,
    );
    assert!(h.app.workspace.panes.is_empty());
    assert!(h.app.flash.as_ref().unwrap().0.starts_with("opened image "));
    assert_eq!(h.app.editor.text(), "keep this draft");
}

#[test]
fn command_screens_explain_input_ownership_and_close_with_x_without_losing_draft() {
    for kind in [
        PickerKind::Usage,
        PickerKind::Use,
        PickerKind::Model,
        PickerKind::Resume,
        PickerKind::Files,
        PickerKind::Tokens,
        PickerKind::Related,
        PickerKind::Update,
    ] {
        let mut h = Harness::new(100, 28);
        h.app.editor.set_text("keep this draft");
        h.app.show_picker(
            kind,
            "screen".into(),
            vec![PickerRow {
                id: "fixture".into(),
                label: "Fixture entry".into(),
                detail: "".into(),
            }],
            true,
        );
        if matches!(
            kind,
            PickerKind::Tokens | PickerKind::Update | PickerKind::Related
        ) {
            h.app.update_preview = Some(update_preview(false, 0));
        }
        h.draw();
        let screen = h.screen();
        assert!(screen.contains("Esc or X"), "{kind:?}: {screen}");
        assert!(
            !screen.contains("keep this draft"),
            "a modal must show input ownership: {screen}"
        );
        let (rect, _) = h.app.areas.modal_close.unwrap();
        assert_eq!(pointer::shape(&h.app, rect.x, rect.y), "pointer");
        h.down(rect.x, rect.y);
        h.drag(rect.x.saturating_sub(5), rect.y);
        h.up(rect.x.saturating_sub(5), rect.y);
        assert!(h.app.picker.is_some(), "dragging off X must not close");
        h.down(rect.x, rect.y);
        h.up(rect.x, rect.y);
        assert!(h.app.picker.is_none(), "{kind:?}");
        assert_eq!(h.app.editor.text(), "keep this draft");
        assert!(!h.app.busy);
    }
    // A read that finishes after dismissal or a conversation change cannot repaint the UI.
    let mut h = Harness::new(100, 28);
    h.app.editor.set_text("preserve related-work draft");
    let before = h.app.messages.len();
    h.app.memory_command("groups");
    assert_eq!(h.app.picker.as_ref().unwrap().kind, PickerKind::Related);
    h.app.dismiss_picker();
    let deadline = Instant::now() + Duration::from_secs(3);
    while h.app.session_ui.memory_reading() && Instant::now() < deadline {
        h.app.poll_memory();
        std::thread::yield_now();
    }
    assert!(!h.app.session_ui.memory_reading());
    assert!(h.app.picker.is_none());
    assert_eq!(h.app.messages.len(), before);
    assert_eq!(h.app.editor.text(), "preserve related-work draft");
    h.app.memory_command("groups");
    h.app.session_ui.invalidate();
    h.app.dismiss_picker();
    let deadline = Instant::now() + Duration::from_secs(3);
    while h.app.session_ui.memory_reading() && Instant::now() < deadline {
        h.app.poll_memory();
        std::thread::yield_now();
    }
    assert!(!h.app.session_ui.memory_reading());
    assert_eq!(h.app.messages.len(), before);
}

#[test]
fn modal_typing_is_a_filter_and_output_x_matches_escape() {
    let mut h = Harness::new(100, 28);
    h.app.editor.set_text("keep this draft");
    h.app.show_picker(
        PickerKind::Model,
        "models".into(),
        vec![PickerRow {
            id: "fixture".into(),
            label: "Fixture model".into(),
            detail: "".into(),
        }],
        true,
    );
    h.type_str("Fixture");
    assert_eq!(h.app.picker.as_ref().unwrap().filter, "Fixture");
    assert_eq!(h.app.editor.text(), "keep this draft");
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    h.app.push_card(StreamCard {
        id: "cmd".into(),
        title: "shell".into(),
        detail: Some("command: cargo test\nOutput: passed".into()),
        status: CardStatus::Done,
    });
    let id = h.app.messages.len() - 1;
    h.app.activity.open(id);
    h.draw();
    assert!(h.screen().contains("Typing searches output"));
    let (rect, _) = h.app.areas.modal_close.unwrap();
    h.down(rect.x, rect.y);
    h.up(rect.x, rect.y);
    assert!(h.app.activity.detail.is_none());
    assert_eq!(h.app.editor.text(), "keep this draft");
}

#[test]
fn update_page_close_button_cancels_its_job_and_preserves_draft() {
    let mut h = Harness::new(100, 28);
    h.app.editor.set_text("keep this draft");
    update_screen(&mut h, update_preview(true, 0));
    h.app.job = Some(fake_update_job());
    h.app.update_progress();
    h.draw();
    assert!(h.screen().contains("Esc or X cancels the update"));
    let (rect, _) = h.app.areas.modal_close.unwrap();
    h.down(rect.x, rect.y);
    h.up(rect.x, rect.y);
    assert!(h.app.picker.is_none());
    assert!(h.app.job.as_ref().unwrap().is_cancelled());
    assert_eq!(h.app.editor.text(), "keep this draft");
}

#[test]
fn right_click_keeps_the_selection_for_repeated_copy() {
    let mut h = Harness::new(60, 20);
    h.app.push_assistant("hello brave world".into());
    h.draw();
    let row = h.row_of("hello brave world");
    let (x, y) = h.t_cell(row, 6);
    let (end, _) = h.t_cell(row, 11);
    h.down(x, y);
    h.drag(end, y);
    h.up(end, y);
    for _ in 0..2 {
        h.mouse(MouseEventKind::Down(MouseButton::Right), end, y);
        assert_eq!(clipboard::get_text().unwrap(), "brave");
        assert_eq!(h.app.view.selected_text().as_deref(), Some("brave"));
        assert_eq!(h.app.editor.text(), "");
    }
}

#[test]
fn mouse_pointer_matches_clickable_targets_and_text() {
    let h = landing_harness();
    let y = h
        .screen()
        .lines()
        .position(|line| line.contains("chat first"))
        .unwrap();
    assert_eq!(pointer::shape(&h.app, 4, y as u16), "pointer");
    let r = h.app.areas.input;
    assert_eq!(pointer::shape(&h.app, r.x + 3, r.y), "text");
    assert_eq!(pointer::shape(&h.app, 0, 0), "default");
}

#[test]
fn current_run_timer_omits_unused_leading_units() {
    let mut h = Harness::new(120, 24);
    h.app.busy = true;
    for (seconds, shown) in [(5, "5s"), (500, "8m 20s"), (3723, "1h 02m 03s")] {
        h.app.turn_started = Some(Instant::now() - Duration::from_secs(seconds));
        h.draw();
        assert!(h.screen().contains(shown), "{}", h.screen());
    }
}

#[test]
fn expanded_tool_header_stays_reachable_and_collapses_after_scrolling() {
    for all in [false, true] {
        let mut h = Harness::new(100, 20);
        h.app.push_user("inspect fixture".into());
        h.app.push_card(StreamCard {
            id: "long-result".into(),
            title: "shell".into(),
            detail: Some(format!(
                "command: cargo test -p concordia\nOutput: {}",
                (0..100).map(|i| format!("line {i}\n")).collect::<String>()
            )),
            status: CardStatus::Done,
        });
        let id = h.app.messages.len() - 1;
        h.app.expanded_activity = all;
        if !all {
            h.app.activity.toggle(id);
        }
        h.draw();
        h.app.view.scroll_up(3);
        h.draw();
        let (rect, pinned_id) = h.app.areas.pinned_card.expect("expanded header is pinned");
        assert_eq!(pinned_id, id);
        assert!(h.screen().contains("Collapse"));
        let pinned = h.screen().lines().nth(rect.y as usize).unwrap().to_owned();
        assert!(pinned.contains("Ran tests"), "{pinned}");
        assert!(pinned.contains("output lines"), "{pinned}");
        assert_eq!(pointer::shape(&h.app, rect.x + 5, rect.y), "pointer");
        h.click(rect.x + 5, rect.y);
        assert!(!h.app.activity.is_expanded(id, all));
        assert!(h.app.areas.pinned_card.is_none());
        assert_eq!(h.app.editor.text(), "");
        h.app.view.follow_tail();
        h.draw();
        let row = h
            .app
            .view
            .rows
            .iter()
            .position(|row| row.text.iter().collect::<String>().contains("Expand"))
            .unwrap();
        let (x, y) = h.t_cell(row, 8);
        assert_eq!(pointer::shape(&h.app, x, y), "pointer");
        h.down(x, y);
        assert!(!h.app.activity.is_expanded(id, all));
        h.up(x, y);
        assert!(
            h.app.activity.is_expanded(id, all),
            "completed header text expands its work"
        );
    }
}

#[test]
fn terminal_titles_use_chat_or_project_names_and_distinct_status_icons() {
    let mut h = Harness::new(100, 20);
    h.app.cwd = PathBuf::from("C:/work/fixture-project");
    let title = h.app.window_title();
    assert!(title == "Concordia | fixture-project", "{title}");
    assert!(
        !title.contains("Codex") && !title.contains("gpt-"),
        "{title}"
    );
    h.app.session_ui.title = Some("Fix parser".into());
    h.app.busy = true;
    h.app.phase = Phase::Thinking;
    assert!(h.app.window_title().contains("Thinking"));
    assert!(h.app.window_title().contains("Fix parser"));
    assert!(h
        .app
        .window_title()
        .starts_with("Concordia | Thinking | Fix parser"));
    h.app.phase = Phase::Writing;
    assert!(h.app.window_title().contains("Writing"));
    assert_eq!(h.app.phase.frame_with_motion(1, false), "▁");
    assert_ne!(h.app.phase.frame_with_motion(1, true), "▁");
    h.app.phase = Phase::Tool("Read".into());
    assert!(h.app.window_title().contains("Read"));
    assert_eq!(h.app.phase.frame(0), "▤");
    h.app.phase = Phase::Tool("apply_patch".into());
    assert_eq!(h.app.phase.frame(0), "✎");
    h.app.cancel_requested = true;
    assert!(h.app.window_title().contains("Stopping"));
    // Appearance uses the normal keyboard/mouse picker and preserves a working draft.
    h.app.editor.set_text("preserved draft");
    h.app.handle_slash("/config");
    assert_eq!(h.app.picker.as_ref().unwrap().kind, PickerKind::Settings);
    assert!(!h.app.window_title().contains('—'));
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.app.picker.as_ref().unwrap().kind, PickerKind::Appearance);
    assert_eq!(h.app.editor.text(), "preserved draft");
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.app.picker.as_ref().unwrap().kind, PickerKind::Theme);
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.app.appearance.ui.theme, crate::config::Theme::Opifex);
    assert_eq!(h.app.editor.text(), "preserved draft");
    h.app.handle_slash("/config color #78BFFF");
    assert_eq!(h.app.appearance.color(), [120, 191, 255]);
    h.app.handle_slash("/config motion off");
    assert!(!h.app.appearance.ui.motion);
    let reloaded = appearance::State::load(&h.app.state_root);
    assert_eq!(reloaded.ui.theme, crate::config::Theme::Opifex);
    assert_eq!(reloaded.color(), [120, 191, 255]);
    assert!(!reloaded.ui.motion);
    assert!(!h.app.state_root.join("config.toml").exists());
    // Invalid custom input leaves saved settings untouched and is corrected in-place.
    appearance::choose(&mut h.app, PickerKind::BrandColor, "custom");
    h.app.picker.as_mut().unwrap().filter = "#nope".into();
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.app.picker.as_ref().unwrap().kind, PickerKind::BrandCustom);
    assert_eq!(h.app.appearance.color(), [120, 191, 255]);
    h.app.picker.as_mut().unwrap().filter = "#C9A5FF".into();
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.app.appearance.color(), [201, 165, 255]);
    let portrait = brand::portrait().expect("bundled original portrait");
    let original = image::load_from_memory_with_format(
        include_bytes!("../../../assets/concordia.png"),
        image::ImageFormat::Png,
    )
    .unwrap()
    .into_rgba8();
    assert_eq!(portrait.as_ref(), &original);
    // Display preferences and file actions survive rebuilding the application state.
    appearance::save(&mut h.app, "tokens", "off");
    appearance::save(&mut h.app, "tips", "off");
    let reloaded = appearance::State::load(&h.app.state_root);
    assert!(!reloaded.ui.token_line && !reloaded.ui.tips);
    appearance::save(&mut h.app, "stats", "off");
    appearance::save(&mut h.app, "defaults", "");
    let reloaded = appearance::State::load(&h.app.state_root);
    assert_eq!(
        reloaded.ui.token_line,
        crate::config::Config::default().ui.token_line
    );
    assert_eq!(reloaded.ui.tips, crate::config::Config::default().ui.tips);
    assert_eq!(reloaded.ui.stats, crate::config::Config::default().ui.stats);
    settings::choose(&mut h.app, PickerKind::FilePreferences, "preview");
    assert_eq!(
        crate::store::get_settings(&h.app.state_root).file_action,
        Some(crate::file_open::FileAction::Preview)
    );
    // Same-named definitions keep their project/account scope; inspection executes nothing.
    let project = h.app.state_root.join("fixture-project");
    let home = h.app.state_root.join("fixture-provider");
    for root in [project.join(".claude"), home.clone()] {
        std::fs::create_dir_all(root.join("skills/review")).unwrap();
        std::fs::write(
            root.join("skills/review/SKILL.md"),
            "---\nname: review\ndescription: Fixture review\n---\nDo not execute anything.",
        )
        .unwrap();
        std::fs::write(root.join("settings.json"), r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"fixture-never-execute"}]}]}}"#).unwrap();
    }
    h.app.cwd = project;
    h.app.home_path = Some(home);
    h.app.provider = ProviderKind::Claude;
    h.app.handle_slash("/skills");
    let rows = &h.app.picker.as_ref().unwrap().rows;
    assert!(rows.iter().any(|r| r.label == "Project · review"));
    assert!(rows
        .iter()
        .any(|r| r.label == "User · selected account · review"));
    h.app.handle_slash("/hooks");
    let rows = &h.app.picker.as_ref().unwrap().rows;
    assert_eq!(
        rows.iter()
            .filter(|r| r.label.contains("PreToolUse"))
            .count(),
        2
    );
    assert!(rows
        .iter()
        .all(|r| !r.detail.contains("fixture-never-execute")));
    assert_eq!(h.app.editor.text(), "preserved draft");
    h.app.session_ui.title =
        Some("A deliberately long chat title with an em dash — and more words than fit".into());
    assert!(!h.app.window_title().contains('—'));
    assert!(h.app.window_title().chars().count() < 85);
    // An incompatible saved version is reported and never overwritten by a new selection.
    let path = h.app.state_root.join(".ao/appearance.json");
    let original = b"{\"version\":99,\"theme\":\"future\"}";
    std::fs::write(&path, original).unwrap();
    h.app.handle_slash("/config theme dark");
    assert_eq!(std::fs::read(&path).unwrap(), original);
    theme::install(&crate::config::Config::default().ui);
}

#[test]
fn slash_completion_keeps_tab_while_the_landing_lists_are_visible() {
    let mut h = landing_harness();
    h.type_str("/mod");
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(h.app.editor.text(), "/model");
    assert!(landing::visible(&h.app));
    h.app.editor.clear();
    h.send("/config");
    assert!(
        h.app.landing.is_some(),
        "appearance keeps the Home destination"
    );
    assert_eq!(h.app.picker.as_ref().unwrap().kind, PickerKind::Settings);
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(landing::visible(&h.app));
    assert_eq!(h.app.window_title(), "Concordia | Home");
}

#[test]
fn model_argument_completion_uses_the_account_catalog_and_can_apply_it() {
    let mut h = Harness::new(100, 24);
    let home = h.app.state_root.join("fixture-provider");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("models_cache.json"),
        r#"{"models":[{"slug":"fixture-model"},{"slug":"another-model"}]}"#,
    )
    .unwrap();
    set_use_selection(
        &h.app.state_root,
        UseSelection {
            provider: ProviderKind::Codex,
            account: "codex".into(),
            home_path: Some(home.to_string_lossy().into_owned()),
            model: None,
            updated_at: "fixture".into(),
        },
    )
    .unwrap();
    h.app.home_path = Some(home);
    h.app.provider = ProviderKind::Codex;
    h.type_str("/model fix");
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(h.app.editor.text(), "/model fixture-model");
    h.enter();
    assert_eq!(h.app.model, "fixture-model");
    assert!(h.app.picker.is_none());
}

#[test]
fn reply_identity_does_not_repeat_the_default_provider_account_name() {
    let mut h = Harness::new(100, 24);
    h.app.provider = ProviderKind::Codex;
    h.app.account = "codex".into();
    h.app.model = "fixture-model".into();
    let (_, label) = h.app.reply_meta();
    assert_eq!(label, "Codex \u{b7} fixture-model");
    h.app.session_chip = Some("first request".into());
    h.draw();
    assert!(!h.screen().lines().next().unwrap().contains("first request"));
}

#[test]
fn account_argument_completion_switches_only_to_a_connected_fixture() {
    let _iso = Isolated::new();
    let mut h = Harness::new(100, 24);
    let home = h.app.state_root.join("fixture-home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("auth.json"), "{}").unwrap();
    crate::store::upsert_stored_account(
        &h.app.state_root,
        crate::store::StoredAccount {
            id: "codex-fixture".into(),
            provider: ProviderKind::Codex,
            home_path: Some(home.to_string_lossy().into_owned()),
            label: "Fixture".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    h.type_str("/use codex-f");
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(h.app.editor.text(), "/use codex-fixture");
    h.enter();
    assert_eq!(h.app.account, "codex-fixture");
    assert_eq!(h.app.home_path, Some(home));
    assert!(h.app.picker.is_none());
}

#[test]
fn typed_model_ids_are_accepted_but_unsafe_and_unknown_accounts_cannot_change_binding() {
    let _iso = Isolated::new();
    let mut h = Harness::new(100, 24);
    let home = h.app.state_root.join("fixture-catalog");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("models_cache.json"),
        r#"{"models":[{"slug":"fixture-model"}]}"#,
    )
    .unwrap();
    h.app.home_path = Some(home);
    h.app.model = "fixture-model".into();
    let account = h.app.account.clone();
    assert!(h.app.handle_slash("/model extra"));
    assert_eq!(h.app.model, "extra");
    assert!(h.app.picker.is_none());
    for line in [
        "/model ../outside",
        "/model \"quoted\"",
        "/model && echo unsafe",
        "/use not-connected",
    ] {
        assert!(h.app.handle_slash(line));
        assert_eq!(h.app.model, "extra");
        assert_eq!(h.app.account, account);
        assert!(h.app.picker.is_none());
    }
}

#[test]
fn model_picker_groups_by_family_with_default_first() {
    let rows = super::model_picker_rows(
        vec![
            crate::models::ModelEntry {
                id: "b".into(),
                display_name: "B".into(),
                family: "Family B".into(),
                tier: crate::models::ModelTier::Standard,
                source: crate::models::ModelSource::Catalog,
                deprecated: false,
            },
            crate::models::ModelEntry {
                id: "a".into(),
                display_name: "A".into(),
                family: "Family A".into(),
                tier: crate::models::ModelTier::Standard,
                source: crate::models::ModelSource::Catalog,
                deprecated: false,
            },
        ],
        None,
    );
    assert!(rows[0].label.starts_with("provider default"));
    assert_eq!(rows[1].label, "Family A");
    assert_eq!(rows[3].label, "Family B");
}

#[test]
fn model_picker_shows_source_tags_and_marks_current() {
    let rows = super::model_picker_rows(
        vec![crate::models::ModelEntry {
            id: "sol".into(),
            display_name: "Sol".into(),
            family: "GPT-5.6".into(),
            tier: crate::models::ModelTier::Frontier,
            source: crate::models::ModelSource::Cached,
            deprecated: false,
        }],
        Some("sol"),
    );
    assert!(rows[2].label.contains("current"));
    assert!(rows[2].detail.contains("sol") && rows[2].detail.contains("cached"));
}

#[test]
fn model_picker_filter_matches_names_and_ids() {
    let rows = super::model_picker_rows(
        vec![crate::models::ModelEntry {
            id: "gpt-5.6-sol".into(),
            display_name: "GPT Sol".into(),
            family: "GPT-5.6".into(),
            tier: crate::models::ModelTier::Frontier,
            source: crate::models::ModelSource::Catalog,
            deprecated: false,
        }],
        None,
    );
    let picker = super::Picker {
        kind: super::PickerKind::Model,
        title: "model".into(),
        rows,
        filter: "5.6-sol".into(),
        state: ratatui::widgets::ListState::default(),
    };
    assert_eq!(
        picker
            .filtered()
            .iter()
            .filter(|r| r.id.starts_with("model:gpt"))
            .count(),
        1
    );
    let mut by_name = picker;
    by_name.filter = "GPT Sol".into();
    assert_eq!(
        by_name
            .filtered()
            .iter()
            .filter(|r| r.id.starts_with("model:gpt"))
            .count(),
        1
    );
}

#[test]
fn model_picker_r_refreshes_without_blocking() {
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (models_tx, models_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        let _ = models_tx.send(Vec::<crate::models::ModelEntry>::new());
    });

    started_rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("fake model refresh did not start");
    assert!(models_rx.try_recv().is_err());

    release_tx.send(()).unwrap();
    assert!(models_rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .is_ok());
    worker.join().unwrap();
}

#[test]
fn completed_commands_reveal_their_options_before_a_space_or_enter() {
    let _iso = Isolated::new();
    let mut h = Harness::new(110, 28);
    h.type_str("/us");
    assert!(!h.screen().contains("connect a provider account"));
    h.type_str("e");
    assert!(h.screen().contains("/use options"));
    assert!(h
        .app
        .palette_entries()
        .iter()
        .any(|e| e.command == "/use add"));
    assert!(!h
        .app
        .palette_entries()
        .iter()
        .any(|e| e.command == "/use add codex"));
    h.key(KeyCode::Down, KeyModifiers::NONE);
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(h.app.editor.text(), "/use add");
    assert!(h
        .app
        .palette_entries()
        .iter()
        .any(|e| e.command == "/use add codex"));
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(h.app.editor.text(), "/use add ");
    assert!(h.app.picker.is_none(), "completion never starts a login");
}

#[test]
fn waiting_status_and_the_adjacent_timer_describe_the_current_run() {
    let mut h = Harness::new(120, 24);
    h.app.busy = true;
    h.app.turn_started = Some(Instant::now() - Duration::from_secs(500));
    h.draw();
    let screen = h.screen();
    let line = screen
        .lines()
        .find(|line| line.contains("Codex · starting the turn"))
        .unwrap();
    assert!(line.contains("8m 20s"), "{line}");
    assert!(line.find("8m 20s").unwrap() > line.find("starting the turn").unwrap());
    h.app.apply_progress(TurnProgress::Card(StreamCard {
        id: "completed".into(),
        title: "Read fixture.rs".into(),
        detail: None,
        status: CardStatus::Done,
    }));
    h.draw();
    assert!(h.screen().contains("Reviewing the latest tool results"));
}

#[test]
fn account_nickname_persists_without_changing_the_native_binding() {
    let _iso = Isolated::new();
    let mut h = Harness::new(120, 24);
    let before = get_account(&h.app.state_root, "codex").unwrap();
    let selection = get_use_selection(&h.app.state_root);
    h.send("/use rename codex Work account");
    assert_eq!(h.app.account_label(), "Work account (Codex)");
    let after = get_account(&h.app.state_root, "codex").unwrap();
    assert_eq!(before.home_path, after.home_path);
    assert_eq!(before.custom, after.custom);
    assert_eq!(after.label, "Work account (Codex)");
    assert_eq!(
        serde_json::to_value(selection).unwrap(),
        serde_json::to_value(get_use_selection(&h.app.state_root)).unwrap()
    );
    h.app.account_names.clear();
    h.app.refresh_binding();
    assert!(h
        .app
        .named_account(ProviderKind::Codex, "codex")
        .starts_with("Work account"));
    assert!(crate::accounts::set_nickname(&h.app.state_root, "codex", "bad\u{1b}name").is_err());
    assert!(crate::accounts::set_nickname(&h.app.state_root, "missing-account", "Name").is_err());
    h.send("/use rename codex");
    assert_eq!(h.app.account_label(), "Codex");
}

#[test]
fn active_email_and_nickname_are_visible_in_header_without_a_picker() {
    for width in [80, 100, 160] {
        let mut h = Harness::new(width, 24);
        h.app.account_names.insert("codex".into(), "Work".into());
        h.app.account_email = Some("member@example.invalid".into());
        h.app.usage_summary = "5h 80% left".into();
        h.app.usage_credits = "Credits: 12.5 balance".into();
        h.draw();
        let screen = h.screen();
        let header = screen.lines().next().unwrap();
        assert!(header.contains("Work (Codex)"), "{header}");
        assert!(!header.contains("member@example.invalid"), "{header}");
        assert!(header.contains("view email"), "{header}");
        let usage = screen.lines().nth(1).unwrap();
        assert!(usage.contains("Credits: 12.5 balance"), "{usage}");
        assert!(usage.contains("Subscription: 5h 80% left"), "{usage}");
        let email = h.app.areas.header_email.unwrap();
        h.down(email.x, email.y);
        h.up(email.x, email.y);
        h.draw();
        assert!(h
            .screen()
            .lines()
            .next()
            .unwrap()
            .contains("member@example.invalid"));
        let email = h.app.areas.header_email.unwrap();
        h.down(email.x, email.y);
        h.up(email.x, email.y);
        h.draw();
        assert!(!h
            .screen()
            .lines()
            .next()
            .unwrap()
            .contains("member@example.invalid"));
    }
}

#[test]
fn at_file_reference_click_opens_the_internal_split() {
    let mut h = Harness::new(120, 24);
    std::fs::write(h._dir.path().join("reference.md"), "# File").unwrap();
    h.app.messages = vec![ChatLine {
        role: MsgRole::User,
        text: "Read @reference.md:1".into(),
        card: None,
        meta: None,
    }];
    h.draw();
    let (index, row) = h
        .app
        .view
        .rows
        .iter()
        .enumerate()
        .find(|(_, row)| !row.links.is_empty())
        .unwrap();
    let start = row
        .links
        .iter()
        .find(|(_, _, c)| c.target == "reference.md")
        .unwrap()
        .0;
    let x = h.app.areas.transcript.x
        + row.gutter.chars().count() as u16
        + row.text[..start]
            .iter()
            .map(|c| text::char_width(*c) as u16)
            .sum::<u16>();
    let y = h.app.areas.transcript.y + (index - h.app.view.visible().start) as u16;
    h.down(x, y);
    h.up(x, y);
    assert_eq!(h.app.workspace.panes.len(), 1);
    assert!(h
        .app
        .last_file_open
        .as_ref()
        .unwrap()
        .path
        .ends_with("reference.md"));
    h.send("/open @reference.md:1");
    assert_eq!(
        h.app.workspace.panes.len(),
        1,
        "reopening focuses the existing file"
    );
}

#[test]
fn each_usage_and_use_invocation_fetches_again_and_updates_open_rows() {
    let _iso = Isolated::new();
    let mut h = Harness::new(120, 35);
    let home = h._dir.path().join("claude-fixture");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("credentials.json"),
        r#"{"fixture":"presence marker only"}"#,
    )
    .unwrap();
    crate::store::upsert_stored_account(
        &h.app.state_root,
        crate::store::StoredAccount {
            id: "claude-fixture".into(),
            provider: ProviderKind::Claude,
            home_path: Some(home.to_string_lossy().into()),
            label: "Fixture".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = count.clone();
    h.app.usage_fetcher = UsageFetcher::with_runner(
        h.app.state_root.clone(),
        std::sync::Arc::new(move |_| {
            let n = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            Ok(format!("Current session: {}% used\n", n * 10))
        }),
    );
    for (iteration, command) in ["/usage", "/usage", "/use", "/use"].iter().enumerate() {
        h.app.picker = None;
        assert!(h.app.handle_slash(command));
        h.app.picker.as_mut().unwrap().state.select(Some(1));
        let deadline = Instant::now() + Duration::from_secs(5);
        let expected = format!("{}% left", 100 - (iteration + 1) * 10);
        loop {
            h.app.on_tick();
            let picker = h.app.picker.as_ref().unwrap();
            if picker.rows.iter().any(|r| {
                r.id.ends_with("claude-fixture")
                    && r.label.contains(&expected)
                    && !r.detail.contains("refreshing")
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "missing {expected}");
            std::thread::yield_now();
        }
        assert_eq!(
            count.load(std::sync::atomic::Ordering::SeqCst),
            iteration + 1
        );
        assert_eq!(h.app.picker.as_ref().unwrap().state.selected(), Some(1));
    }
}

#[test]
fn replayed_tool_completion_is_logged_once_but_reused_ids_in_new_turns_are_preserved() {
    let mut h = Harness::new(100, 24);
    h.app.thread = Some(crate::threads::ThreadLog::open(
        &h.app.state_root,
        "dedup-fixture",
    ));
    let card = StreamCard {
        id: "same-id".into(),
        title: "Read".into(),
        detail: Some("src/code.rs".into()),
        status: CardStatus::Done,
    };
    h.app.push_user("inspect file".into());
    h.app.push_card(card.clone());
    h.app.push_card(card.clone());
    h.app
        .reference_journal
        .finish(h.app.state_root.clone(), None, false);
    h.app.reference_journal.settle_for_test(&mut h.app.history);
    let log = crate::threads::ThreadLog::open(&h.app.state_root, "dedup-fixture");
    assert_eq!(
        log.read_all()
            .unwrap()
            .iter()
            .filter(|e| e.k == "card")
            .count(),
        1
    );
    assert_eq!(
        h.app
            .messages
            .iter()
            .filter(|m| m.role == MsgRole::Card)
            .count(),
        1
    );
    h.app.push_user("inspect again".into());
    h.app.push_card(card);
    h.app
        .reference_journal
        .finish(h.app.state_root.clone(), None, false);
    h.app.reference_journal.settle_for_test(&mut h.app.history);
    assert_eq!(
        log.read_all()
            .unwrap()
            .iter()
            .filter(|e| e.k == "card")
            .count(),
        2
    );
    assert_eq!(
        h.app
            .messages
            .iter()
            .filter(|m| m.role == MsgRole::Card)
            .count(),
        2
    );
}

#[test]
fn failed_cut_keeps_the_original_selection_and_text() {
    let mut h = Harness::new(60, 20);
    h.type_str("retain this text");
    h.key(KeyCode::Char('a'), KeyModifiers::CONTROL);
    clipboard::FAIL_WRITES.set(true);
    h.key(KeyCode::Char('x'), KeyModifiers::CONTROL);
    clipboard::FAIL_WRITES.set(false);
    assert_eq!(h.app.editor.text(), "retain this text");
    assert_eq!(
        h.app.editor.selected_text().as_deref(),
        Some("retain this text")
    );
    assert!(h.app.flash.as_ref().unwrap().0.contains("busy"));
}

#[test]
fn retained_patch_errors_are_inspectable_without_claiming_the_exit_cause() {
    let mut h = Harness::new(100, 30);
    let old_error = "2026-01-01 ERROR tools::router: apply_patch verification failed: Failed to find expected lines";
    h.app.finalize_turn(TurnOutcome {
        exit_code: Some(1),
        stderr_tail: format!("{old_error}\n  use old::import;"),
        ..Default::default()
    });
    let diagnostics = h
        .app
        .messages
        .iter()
        .filter_map(|m| m.card.as_ref())
        .find(|c| c.title.starts_with("Provider diagnostics"))
        .unwrap();
    assert!(diagnostics.detail.as_deref().unwrap().contains(old_error));
    assert!(diagnostics
        .detail
        .as_deref()
        .unwrap()
        .contains("do not establish why"));
    assert!(!h
        .app
        .messages
        .iter()
        .any(|m| m.role == MsgRole::System && m.text.contains(old_error)));
}
