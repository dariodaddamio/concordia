use super::*;

#[test]
fn attention_badge_opens_and_selects_the_paused_queue() {
    let mut h = Harness::new(180, 30);
    landing::open_home_tab(&mut h.app);
    h.app.queue.items.push("pending prompt".into());
    h.app.queue.paused = true;
    h.draw();
    let badge = h
        .app
        .areas
        .header_attention
        .expect("clickable attention badge");
    h.click(badge.x + 2, badge.y);
    assert!(h
        .app
        .picker
        .as_ref()
        .is_some_and(|p| p.kind == PickerKind::NeedsYou));
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(h.app.picker.is_none());
    assert!(h.app.queue.focused);
    assert!(
        h.app.queue.paused,
        "inspection must not send a queued prompt"
    );
    assert_eq!(h.app.queue.items[0], "pending prompt");
}

#[test]
fn home_picker_owns_navigation_and_preserves_the_composer() {
    let mut h = Harness::new(120, 30);
    landing::open_home_tab(&mut h.app);
    h.app.editor.set_text("unsent home draft");
    h.app.queue.items.push("queued prompt".into());
    h.app.queue.paused = true;
    h.key(KeyCode::Char('n'), KeyModifiers::CONTROL);
    assert_eq!(h.app.picker.as_ref().unwrap().kind, PickerKind::NeedsYou);
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.app.picker.is_none());
    assert!(landing::visible(&h.app));
    assert_eq!(h.app.editor.text(), "unsent home draft");
    h.key(KeyCode::Char('n'), KeyModifiers::CONTROL);
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(h.app.picker.is_none());
    assert!(h.app.queue.focused);
    assert!(h.app.queue.paused);
    assert!(!h.app.busy);
}

#[test]
fn empty_paused_queue_has_no_attention_badge() {
    let mut h = Harness::new(120, 24);
    h.app.queue.paused = true;
    h.draw();
    assert!(h.app.areas.header_attention.is_none());
    h.key(KeyCode::Char('n'), KeyModifiers::CONTROL);
    assert!(h.app.picker.is_none());
}

fn pending_approval_is_a_needs_you_item() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
    let owner = crate::provider::interaction::InteractionOwner {
        conversation_id: app.deleg.d.conversation_id().to_owned(),
        workspace: dir.path().to_path_buf(),
        execution_cwd: dir.path().to_path_buf(),
        run_id: uuid::Uuid::new_v4().to_string(),
        attempt_id: uuid::Uuid::new_v4().to_string(),
        provider: ProviderKind::Codex,
        account: "fixture".into(),
        home: Some(dir.path().to_path_buf()),
    };
    app.native_owner = Some(owner.clone());
    let mut connection = crate::provider::interaction::InteractionConnection::new(
        Some(owner),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    connection.initialized_codex("0.166.0");
    let (request, mut rx) = connection
        .request(
            "fixture-session",
            serde_json::json!(1),
            "item/commandExecution/requestApproval".into(),
            serde_json::json!({
                "threadId": "fixture-session",
                "turnId": "fixture-turn",
                "itemId": "item-1",
                "reason": "Run the synthetic command?",
                "availableDecisions": ["accept", "decline"]
            }),
        )
        .unwrap()
        .unwrap();
    app.questions.offer_native(request, true);
    app.sync_needs_you();
    assert_eq!(app.needs_you.len(), 1);
    let item = app.needs_you.item(0).unwrap();
    assert_eq!(item.kind, crate::needs_you::ItemKind::Approval);
    assert_eq!(item.session_id, "fixture-session");

    crate::tui::questions::handle_key(
        &mut app,
        &KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    );
    assert!(rx.try_recv().is_ok());
    app.sync_needs_you();
    assert!(app.needs_you.is_empty());
}

#[test]
fn chat_map_row_expands_for_v2_maps() {
    let map = crate::threads::build_map(
        &[crate::threads::ThreadEvent::with_text(
            "user",
            "continue here",
        )],
        "thread-1",
        1600,
        true,
        None,
        None,
    );
    let row = ChatLine {
        role: MsgRole::System,
        text: map,
        card: None,
        meta: None,
    };

    assert!(row.is_map());
    assert!(row.toggles());
}

#[test]
fn header_and_status_never_repeat_usage() {
    let mut h = Harness::new(120, 30);
    h.app.landing = None;
    let mut snap = crate::usage::UsageSnapshot::no_data("", "");
    snap.windows.push(crate::usage::UsageWindow {
        label: "5h".into(),
        used_pct: 60.0,
        resets: Some("3h".into()),
        bucket: None,
    });
    snap.windows.push(crate::usage::UsageWindow {
        label: "7d".into(),
        used_pct: 86.0,
        resets: Some("Nov 11, 8:59am".into()),
        bucket: None,
    });
    h.app.set_usage_footer(&snap);
    h.app.usage_summary = "5h 40% left \u{b7} 7d 14% left".into();
    h.draw();
    let screen = h.screen();
    assert_eq!(screen.matches("7d 14% left").count(), 1, "{screen}");
    assert!(screen.contains("Subscription: 5h 40% left · 7d 14% left, resets"));
    assert!(!screen.contains("checked by the provider"));
}

#[test]
fn home_header_omits_subscription_when_usage_box_visible() {
    let mut h = Harness::new(120, 30);
    crate::tui::landing::open_home_tab(&mut h.app);
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    let landing = h.app.landing.as_mut().unwrap();
    let mut snapshot = crate::usage::UsageSnapshot::no_data("", "");
    snapshot.windows.push(crate::usage::UsageWindow {
        label: "7d".into(),
        used_pct: 86.0,
        resets: Some("Nov 11, 8:59am".into()),
        bucket: None,
    });
    landing.meters.push(crate::tui::landing::LandingMeter {
        provider: ProviderKind::Codex,
        account: "codex".into(),
        snapshot,
    });
    h.app.usage_summary = "7d 14% left".into();
    assert!(crate::tui::landing::usage_box_visible(&h.app, 120));
    h.draw();
    let screen = h.screen();
    assert!(!screen.contains("Subscription:"), "{screen}");
}

#[test]
fn chat_header_keeps_credits_text() {
    let mut h = Harness::new(120, 30);
    h.app.landing = None;
    h.draw();
    assert!(!h.screen().contains("Credits: not reported"), "{}", h.screen());
}

#[test]
fn use_picker_shows_every_limit_reset_at_narrow_widths_and_clears_on_close() {
    for width in [40, 64, 80, 120] {
        let mut h = Harness::new(width, 30);
        let mut snap = crate::usage::UsageSnapshot::no_data("", "");
        for (label, used_pct, reset) in [("5h", 23.0, "3h"), ("7d", 61.0, "2d")] {
            snap.windows.push(crate::usage::UsageWindow {
                label: label.into(),
                used_pct,
        resets: Some(reset.into()),
        bucket: None,
            });
        }
        let row = picker_row("codex", ProviderKind::Codex, &snap);
        h.app.show_picker(
            PickerKind::Use,
            "accounts".into(),
            vec![PickerRow {
                id: row.id,
                label: row.label,
                detail: "".into(),
            }],
            false,
        );
        h.draw();
        let screen = h.screen();
        let joined = screen
            .lines()
            .map(|line| line.trim().trim_matches('│').trim())
            .collect::<Vec<_>>()
            .join(" ");
        for expected in ["5h 77% left", "7d 39% left", "resets in 3h", "resets in 2d"] {
            assert!(
                joined.contains(expected),
                "width {width}: missing {expected}: {screen}"
            );
        }
        h.key(KeyCode::Esc, KeyModifiers::NONE);
        assert!(!h.screen().contains("in 2d"));
    }
}

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
        bucket: None,
    });
    h.app.provider = ProviderKind::Codex;
    h.app.set_usage_footer(&snap);
    h.draw();
    assert!(h
        .app
        .usage_warning
        .as_deref()
        .is_some_and(|warning| warning.contains("5h 4% left")));
    assert!(!h.screen().contains("checked by the provider"));
    h.app.provider = ProviderKind::Claude;
    h.app.set_usage_footer(&snap);
    assert!(h
        .app
        .usage_warning
        .as_deref()
        .unwrap()
        .contains("saved reading"));
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
    h.app.begin_queue_edit(edit);
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
    // A held navigation key must behave like repeated distinct presses, including modifiers.
    for code in [KeyCode::Left, KeyCode::Right, KeyCode::Up, KeyCode::Down] {
        for modifiers in [
            KeyModifiers::NONE,
            KeyModifiers::CONTROL,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ] {
            let mut held = Harness::new(80, 24);
            let mut pressed = Harness::new(80, 24);
            for app in [&mut held.app, &mut pressed.app] {
                app.editor
                    .set_text("one two three four\nfive six seven eight\nnine ten eleven twelve");
                if matches!(code, KeyCode::Right | KeyCode::Down) {
                    handle_key(
                        app,
                        KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL),
                        &tx,
                    );
                }
            }
            let before = held.app.editor.cursor_pos(74);
            for _ in 0..3 {
                dispatch_key_event(
                    &mut held.app,
                    KeyEvent::new_with_kind(code, modifiers, KeyEventKind::Repeat),
                    &mut burst,
                    &tx,
                    Instant::now(),
                );
                handle_key(&mut pressed.app, KeyEvent::new(code, modifiers), &tx);
            }
            assert_eq!(
                held.app.editor.cursor_pos(74),
                pressed.app.editor.cursor_pos(74),
                "{code:?} {modifiers:?}"
            );
            assert_eq!(held.app.editor.text(), pressed.app.editor.text());
            assert!(
                held.app.editor.cursor_pos(74) != before
                    || held.app.editor.text()
                        != "one two three four\nfive six seven eight\nnine ten eleven twelve",
                "held key changes cursor or history state: {code:?} {modifiers:?}"
            );
            assert_eq!(held.app.editor.selection(), pressed.app.editor.selection());
        }
    }
    let mut held = Harness::new(80, 24);
    held.app.editor.set_text("unsent draft");
    for code in [
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::Char('o'),
        KeyCode::Char('c'),
    ] {
        let modifiers = if matches!(code, KeyCode::Char(_)) {
            KeyModifiers::CONTROL
        } else {
            KeyModifiers::NONE
        };
        assert!(!handle_key(
            &mut held.app,
            KeyEvent::new_with_kind(code, modifiers, KeyEventKind::Repeat),
            &tx
        ));
    }
    assert_eq!(held.app.editor.text(), "unsent draft");
    assert!(!held.app.expanded_activity);
    assert!(held.app.quit_armed.is_none());
}

#[test]
fn doctor_enter_on_fixable_row_asks_confirmation() {
    let mut h = Harness::new(100, 28);
    open_doctor_fixture(&mut h, true);
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        h.app.picker.as_ref().map(|p| p.kind),
        Some(PickerKind::DoctorConfirm)
    );
    assert!(h.screen().contains("old PATH folder") && h.screen().contains("PATH folder to append"));
    assert!(h.screen().contains("y apply / n cancel"));
}

#[test]
fn doctor_y_applies_then_rechecks() {
    let mut h = Harness::new(100, 28);
    let applied = open_doctor_fixture(&mut h, true);
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    h.key(KeyCode::Char('y'), KeyModifiers::NONE);
    assert_eq!(applied.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(h
        .app
        .flash
        .as_ref()
        .is_some_and(|(text, _)| text.contains("fixed (backup:")));
}

#[test]
fn doctor_n_changes_nothing() {
    let mut h = Harness::new(100, 28);
    let applied = open_doctor_fixture(&mut h, true);
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    h.key(KeyCode::Char('n'), KeyModifiers::NONE);
    assert_eq!(applied.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(h
        .app
        .picker
        .as_ref()
        .is_none_or(|p| p.kind != PickerKind::DoctorConfirm));
}

#[test]
fn doctor_non_fixable_row_has_no_apply() {
    let mut h = Harness::new(100, 28);
    let applied = open_doctor_fixture(&mut h, false);
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(applied.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(
        h.app.picker.as_ref().map(|p| p.kind),
        Some(PickerKind::Doctor)
    );
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
    std::fs::create_dir_all(h._dir.path().join("src")).unwrap();
    std::fs::write(h._dir.path().join("src/lib.rs"), "new\nextra\n").unwrap();
    h.app.push_card(StreamCard {
        id: "edit".into(),
        title: "edit".into(),
        detail: Some(
            "File: src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1,2 @@\n-old\n+new\n+extra".into(),
        ),
        status: CardStatus::Done,
    });
    h.draw();
    assert!(h.screen().contains("+2 -1"));
    assert!(
        !h.screen().contains("src/lib.rs"),
        "collapsed headers show the basename"
    );
    let heading = h
        .app
        .view
        .rows
        .iter()
        .find(|r| r.text.iter().collect::<String>().contains("+2 -1"))
        .unwrap();
    let heading_text: String = heading.text.iter().collect();
    assert!(heading
        .links
        .iter()
        .any(|(_, _, target)| target.target == "src/lib.rs" && target.display == "lib.rs"));
    for (token, color) in [
        ("+2", Color::Green),
        ("-1", Color::Red),
        ("Edit File", Color::Indexed(252)),
        ("Expand", Color::Indexed(244)),
    ] {
        let start = heading_text[..heading_text.find(token).unwrap()]
            .chars()
            .count();
        assert!(
            heading
                .runs
                .iter()
                .any(|(a, b, style)| *a <= start && start < *b && style.fg == Some(color)),
            "missing {token} style: {:?}",
            heading.runs
        );
    }

    h.key(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    h.draw();
    assert!(
        h.screen().contains("Edit File"),
        "expanded headings keep the readable action"
    );
    assert!(!h.screen().contains("edit · Collapse"));
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
    assert_eq!(added.style.bg, Some(Color::Rgb(25, 48, 38)));
    assert_eq!(added.gutter.trim(), "1");
    let removed = h
        .app
        .view
        .rows
        .iter()
        .find(|row| row.text.iter().collect::<String>() == "-old")
        .unwrap();
    assert_eq!(removed.style.bg, Some(Color::Rgb(54, 31, 36)));
    assert_eq!(removed.gutter.trim(), "1");
    let header = h
        .app
        .view
        .rows
        .iter()
        .find(|row| row.text.iter().collect::<String>() == "@@ -1 +1,2 @@")
        .unwrap();
    assert_eq!(header.style.fg, Some(Color::Indexed(244)));
    assert!(header.style.add_modifier.contains(Modifier::DIM));
    h.app.activity.open(h.app.messages.len() - 1);
    h.draw();
    let (target, _) = h.app.activity.file_targets.first().unwrap().clone();
    assert_eq!(pointer::shape(&h.app, target.x, target.y), "pointer");
    h.click(target.x, target.y);
    assert!(h.app.activity.detail.is_none());
    assert!(h
        .app
        .last_file_open
        .as_ref()
        .unwrap()
        .path
        .ends_with("src/lib.rs"));
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
        s.contains("You › first") && s.contains("\u{25cf} reply") && s.contains("You › second"),
        "{s}"
    );
    // A blank row separates turns.
    let lines: Vec<&str> = s.lines().collect();
    let reply = lines.iter().position(|l| l.contains("reply")).unwrap();
    assert!(lines[reply + 1].trim().is_empty(), "{s}");
    // The input box is rounded, and trust is stated in the footer (full is flagged).
    assert!(
        s.contains('\u{256d}') && s.contains("permissions: edit"),
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
        s.contains("Codex · starting the turn") && s.contains("[ Esc ]"),
        "{s}"
    );
    assert!(
        h.app.phase.frames().iter().any(|frame| s.contains(frame)),
        "{s}"
    );
}

#[test]
fn no_color_draws_with_the_terminals_own_colours_but_keeps_emphasis() {
    super::theme::fixture_proof();
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

#[test]
fn footer_never_cuts_a_hint_mid_word_and_never_overlaps_the_mode_column() {
    let all: Vec<&str> = HINTS.iter().flat_map(|h| [h.0, h.1]).collect();
    for w in [40u16, 60, 80, 100, 140] {
        let h = Harness::new(w, 12);
        let row = footer_row(&h);
        let right = if row.ends_with("permissions: edit ▾") {
            "permissions: edit ▾"
        } else if row.ends_with("edit ▾") {
            "edit ▾"
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
    let narrow = footer_row(&Harness::new(60, 12));
    assert!(narrow.contains("Ctrl+C copy/stop") || narrow.contains("Ctrl+C copy or cancel"));
    assert!(!footer_row(&Harness::new(40, 12)).contains("drag"));
}

#[test]
fn shift_tab_never_reaches_full() {
    use ratatui::style::Color;
    let mut h = Harness::new(100, 12);
    assert_eq!(h.app.trust, TrustMode::Edit);
    let mut seen = Vec::new();
    for _ in 0..3 {
        h.key(KeyCode::BackTab, KeyModifiers::SHIFT);
        seen.push(h.app.trust);
        assert_eq!(get_settings(&h.app.state_root).trust, h.app.trust);
    }
    assert_eq!(
        seen,
        [TrustMode::Plan, TrustMode::Ask, TrustMode::Edit]
    );
    for (mode, want) in [
        (TrustMode::Plan, Color::Cyan),
        (TrustMode::Ask, Color::Yellow),
        (TrustMode::Edit, Color::Green),
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
    let mode = h.app.areas.permissions;
    assert_eq!(pointer::shape(&h.app, mode.x + 1, mode.y), "pointer");
    h.type_str("keep this draft");
    h.click(mode.x + 1, mode.y);
    assert!(h
        .app
        .picker
        .as_ref()
        .is_some_and(|p| p.kind == PickerKind::Trust));
    assert_eq!(h.app.editor.text(), "keep this draft");
    let close = h.app.areas.modal_close.expect("picker close control").0;
    assert!(close.x > 80, "permissions popup anchors at the right edge");
    assert!(close.y < mode.y);
    let popup = h.app.areas.permissions_popup;
    assert!(popup.width <= 54 && popup.x >= 40);
    assert_eq!(popup.bottom(), mode.y);
    assert!(h.app.areas.input.width > 0);
    assert!(!h.screen().contains("message · paused"));
    let input = h.app.areas.input;
    h.click(input.x + 2, input.y);
    assert!(
        h.app.picker.is_none(),
        "outside click returns directly to the composer"
    );
    assert_eq!(h.app.editor.text(), "keep this draft");
    h.click(mode.x + 1, mode.y);
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.app.picker.is_none());
    assert_eq!(
        h.app.trust,
        TrustMode::Full,
        "opening and dismissing grants nothing"
    );
    h.click(mode.x + 1, mode.y);
    let (row, _) = h.app.permission_rows[0];
    assert_eq!(pointer::shape(&h.app, row.x + 2, row.y), "pointer");
    h.app.busy = true;
    h.click(row.x + 2, row.y);
    assert_eq!(h.app.trust, TrustMode::Plan);
    assert_eq!(get_settings(&h.app.state_root).trust, TrustMode::Plan);
    assert!(h.app.picker.is_none());
    assert_eq!(h.app.editor.text(), "keep this draft");
    assert!(h.screen().contains("next turn"));
    let mut roomy = Harness::new(100, 26);
    roomy.type_str("visible draft");
    let anchor = roomy.app.areas.permissions;
    roomy.click(anchor.x + 1, anchor.y);
    assert!(roomy.screen().contains("Read and plan; no file edits."));
    let (choice, index) = roomy.app.permission_rows[0];
    assert_eq!(index, 0);
    assert!(choice.height >= 2, "descriptions have their own row");
    roomy.click(choice.x + 4, choice.y + 1);
    assert_eq!(
        roomy.app.trust,
        TrustMode::Plan,
        "description clicks select the same choice"
    );
    assert_eq!(roomy.app.editor.text(), "visible draft");
}

#[test]
fn full_mode_badge_is_red() {
    use ratatui::style::Color;
    let mut h = Harness::new(100, 12);
    h.app.trust = TrustMode::Full;
    h.draw();
    let buf = h.term.backend().buffer();
    assert_eq!(
        buf[(buf.area.width - 3, buf.area.height - 1)].fg,
        Color::Red
    );
}

#[test]
fn shift_tab_moves_the_palette_up_and_tab_still_completes() {
    let _iso = Isolated::new();
    let mut h = Harness::new(100, 24);
    h.type_str("/use");
    let before = h.app.trust;
    h.key(KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(h.app.palette.selected, 1);
    h.key(KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!(h.app.palette.selected, 0);
    assert_eq!(
        h.app.trust, before,
        "the mode must not change while the palette is open"
    );
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(h.app.editor.text(), "/use ");
}

#[test]
fn mode_command_shows_and_sets_the_mode() {
    let mut h = Harness::new(100, 24);
    h.type_str("/mode");
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    let s = h.screen();
    assert!(s.contains("mode: edit"), "{s}");
    h.type_str("/mode plan");
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.app.trust, TrustMode::Plan);
    assert!(h.screen().contains("mode \u{2192} plan"));
    h.type_str("/mode bogus");
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.app.trust, TrustMode::Plan);
    assert!(h.screen().contains("unknown mode bogus"));
    h.type_str("/mode edit");
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.app.trust, TrustMode::Edit);
}

#[test]
fn mode_full_requires_confirmation_once_per_session() {
    let mut h = Harness::new(100, 24);
    assert_eq!(h.app.trust, TrustMode::Edit);

    h.send("/mode full");
    assert_eq!(h.app.trust, TrustMode::Edit);
    assert_eq!(
        h.app.picker.as_ref().unwrap().kind,
        PickerKind::FullModeConfirm
    );
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(h.app.trust, TrustMode::Edit);

    h.send("/mode full");
    assert_eq!(
        h.app.picker.as_ref().unwrap().kind,
        PickerKind::FullModeConfirm
    );
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.app.trust, TrustMode::Full);
    assert!(h.app.full_mode_confirmed);

    h.send("/mode plan");
    assert_eq!(h.app.trust, TrustMode::Plan);
    h.send("/mode full");
    assert_eq!(h.app.trust, TrustMode::Full);
    assert!(h.app.picker.is_none());
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
    assert!(
        s.contains("in 12.3k - out 1.4k - cached 9.8k")
            && s.contains("Worked for")
            && s.contains("stopped"),
        "{s}"
    );
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
        last.text.starts_with("Worked for")
            && last.text.contains("stopped")
            && !last.text.contains("in "),
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
    assert!(
        t.starts_with("Worked for") && t.ends_with("out 7") && t.contains("stopped"),
        "{t}"
    );
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
    h.key(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert!(rows(&h).contains("hidden-output"));
    assert!(rows(&h).contains("early-reasoning"));
    assert_eq!(h.app.editor.text(), "keep my draft");
    h.app.expanded_activity = true;
    h.key(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
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

#[test]
fn landing_fixture_render_has_no_extended_prefix_or_hidden_workspaces() {
    let mut h = Harness::new(120, 30);
    let hidden_root = h._dir.path().join("hidden-temp");
    let folder = h.app.cwd.clone();
    let old = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .saturating_sub(86_400_001);
    let mut sessions = vec![
        landing_session(
            "acct-x",
            std::path::Path::new(r"\\?\C:\fixture\target\chat"),
            "target",
            1,
        ),
        landing_session(
            "acct-x",
            std::path::Path::new(r"\\?\UNC\server\share\chat"),
            "unc",
            2,
        ),
        landing_session("acct-x", &hidden_root.join("secret"), "hidden", 3),
    ];
    let mut q1 = landing_session("acct-x", &folder, "old-a", 4);
    q1.modified_ms = old;
    let mut q2 = landing_session("acct-x", &folder, "old-b", 5);
    q2.modified_ms = old;
    q2.preview = Some("short question".into());
    sessions.extend([q1, q2]);
    landing::open_with(&mut h.app, sessions, None);
    landing::set_hidden_temp_root(&mut h.app, hidden_root);
    h.draw();
    let screen = h.screen();
    assert!(!screen.contains(r"\\?\"));
    assert!(!screen.contains("secret"));
    assert!(screen.contains("Quick questions"));
}

#[test]
fn ctrl_w_is_left_to_the_terminal() {
    let mut h = Harness::new(80, 20);
    h.type_str("hello there world");
    h.key(KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert_eq!(h.app.editor.text(), "hello there world");
}

#[test]
fn two_tabs_draw_a_clickable_strip_on_the_top_row_above_the_header() {
    let mut h = Harness::new(100, 30);
    let single = h.screen();
    assert!(h.app.tab_hits.is_empty());
    let chip = |title: &str, provider, status| TabChip {
        title: title.into(),
        provider,
        status,
    };
    h.app.tab_strip = vec![
        chip("Home", ProviderKind::Codex, icons::TabStatus::Idle),
        chip("Fix the images …", ProviderKind::Claude, icons::TabStatus::Failed),
        chip("Docs", ProviderKind::Cursor, icons::TabStatus::Waiting),
    ];
    h.app.tab_index = 1;
    h.draw();
    let screen = h.screen();
    let top = screen.lines().next().unwrap();
    assert!(
        top.contains("1 ⬡   Home") && top.contains("2 ✻ ✗ Fix the images") && top.contains("3 ➤ ● Docs"),
        "{top}"
    );
    assert_eq!(h.app.tab_hits.len(), 3);
    assert!(h.app.tab_hits.iter().all(|r| r.y == 0 && r.width > 0));
    // The header moves down one row instead of being drawn over.
    assert_eq!(screen.lines().nth(1), single.lines().next());

    // While a tab is dragged, a bar marks the gap where it will land.
    h.app.tab_drag = Some((0, 2));
    h.draw();
    let marker = h.app.tab_hits[2].x - 1;
    let top: Vec<char> = h.screen().lines().next().unwrap().chars().collect();
    assert_eq!(top[usize::from(marker)], '┃', "{top:?}");
    h.app.tab_strip[1].status = icons::TabStatus::Running;
    h.app.tick_count = 1;
    h.app.tab_drag = None;
    h.draw();
    assert!(h.screen().lines().next().unwrap().contains("2 ✻ ◓ Fix"));
}
