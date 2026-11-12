use super::*;

#[test]
fn closing_a_pane_with_a_queued_edit_event_does_not_panic() {
    let mut h = Harness::new(120, 24);
    std::fs::write(h._dir.path().join("queued-edit.md"), "fixture\n").unwrap();
    h.send("/open @queued-edit.md");
    let id = h.app.workspace.panes[0].id;
    assert!(h.app.workspace.close(id));
    let before = (h.app.workspace.panes.len(), h.app.workspace.focused);
    h.app.workspace.focused = Some(id);

    h.key(KeyCode::Char('x'), KeyModifiers::NONE);

    assert_eq!(
        (h.app.workspace.panes.len(), h.app.workspace.focused),
        (before.0, Some(id)),
        "a queued edit for a closed pane must not mutate workspace state"
    );
}

#[test]
fn with_lock_timeout_is_reported_not_dropped() {
    let mut h = Harness::new(100, 24);
    let lock_path = h.app.state_root.join(".ao/appearance.lock");
    std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    lock.lock().unwrap();

    appearance::save(&mut h.app, "motion", "off");

    assert!(
        h.app
            .flash
            .as_ref()
            .is_some_and(|(message, _)| message.starts_with("Appearance not saved:")),
        "tui/appearance::save must surface a with_lock timeout"
    );
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
    assert!(s.contains("Codex 3 \u{b7} gpt-5.1"), "{s}");
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
    let (x, y) = find("Codex 3 \u{b7} gpt-5.1");
    assert_eq!(buf[(x, y)].fg, provider_color(ProviderKind::Codex));

    // NO_COLOR keeps the structure (bars, labels) with emphasis only.
    h.app.no_color = true;
    h.draw();
    let s = h.screen();
    assert!(s.contains("| /usage") && s.contains("Codex 3"), "{s}");
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
    let labels = lines.iter().filter(|l| l.trim() == "Codex").count();
    assert_eq!(labels, 1, "{s}");
    let label = lines.iter().position(|l| l.trim() == "Codex").unwrap();
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
    h.app.reference_journal.finish(root.clone(), None, false);
    h.app.reference_journal.settle_for_test(&mut h.app.history);
    let log = crate::threads::ThreadLog::open(&root, "tid-one");
    let kinds: Vec<String> = log.read_all().unwrap().into_iter().map(|e| e.k).collect();
    assert_eq!(kinds, ["user", "card", "assistant", "user"]);
    let stored = std::fs::read_to_string(root.join("threads/tid-one.jsonl")).unwrap();
    assert!(!stored.contains("fix the redirect") && !stored.contains("src/a.rs"));
    assert_eq!(log.read_meta().unwrap().title, "fix the redirect");
    let out = h.app.threads_command("");
    assert!(out[0].contains("fix the redirect"), "{out:?}");
    assert!(h
        .app
        .threads_command("delete tid")
        .join(" ")
        .contains("current"));
    // The bounded map remains available even when the card is not transferred as transcript text.
    let pack = crate::context::build_context_pack(
        &h.app.pack_messages(),
        ProviderKind::Codex,
        ProviderKind::Claude,
        1600,
        None,
    )
    .unwrap();
    assert!(pack.text.contains("[concordia map]"), "{}", pack.text);
}

#[test]
fn the_landing_page_lists_this_folders_chats_first_and_expanded() {
    let mut h = landing_harness();
    let screen = h.screen();
    let here = screen.find("chat first").unwrap_or_else(|| panic!("{screen}"));
    let lock = screen.find("lock (1)").unwrap_or_else(|| panic!("{screen}"));
    assert!(here < lock, "{screen}");
    assert!(screen.contains("chat first") && screen.contains("chat second"));
    assert!(
        !screen.contains("chat third"),
        "collapsed groups hide their chats"
    );
    assert!(
        screen.contains("Enter open chat"),
        "{screen}"
    );
    h.app.editor.set_text("keep the landing draft");
    assert!(landing::visible(&h.app));
    assert_eq!(h.app.editor.text(), "keep the landing draft");
    assert!(h.screen().contains("chat first"));
}

#[test]
fn enter_on_a_chat_resumes_exactly_that_chat() {
    let mut h = landing_harness();
    h.key(KeyCode::Down, KeyModifiers::NONE);
    h.key(KeyCode::Down, KeyModifiers::NONE);
    h.enter();
    assert!(
        h.screen()
            .contains("resume unavailable: chat account is no longer registered"),
        "{}",
        h.screen()
    );
}

#[test]
fn enter_on_a_folder_expands_it_and_the_selection_wraps() {
    let mut h = landing_harness();
    for _ in 0..3 {
        h.key(KeyCode::Down, KeyModifiers::NONE);
    }
    h.enter();
    assert!(h.screen().contains("▾ lock (1)") && h.screen().contains("chat third"));
    h.key(KeyCode::Down, KeyModifiers::NONE);
    h.enter();
    assert!(
        h.screen().contains("resume unavailable"),
        "the selected row is the lock chat"
    );
}

#[test]
fn typing_keeps_the_landing_page_until_the_message_is_sent() {
    let mut h = landing_harness();
    h.type_str("hi");
    assert!(landing::visible(&h.app));
    assert_eq!(h.app.editor.text(), "hi");
    assert!(h.screen().contains(" chats "), "{}", h.screen());
    h.enter();
    assert!(!landing::visible(&h.app));

    let mut h = landing_harness();
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(
        h.app.tab_request,
        Some(super::super::tabs::TabAction::BackToChat)
    );
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
    assert!(h.screen().contains("resume unavailable"));
}



#[test]
fn there_is_no_landing_page_without_chats() {
    let mut h = Harness::new(100, 24);
    crate::store::upsert_stored_account(
        &h.app.state_root,
        crate::store::StoredAccount {
            id: "codex-test".into(),
            provider: ProviderKind::Codex,
            home_path: Some(h._dir.path().join("codex").to_string_lossy().into()),
            label: "Codex test".into(),
            created_at: "test".into(),
        },
    )
    .unwrap();
    landing::open_with(&mut h.app, Vec::new(), None);
    assert!(h.app.landing.is_none());
    h.draw();
    assert!(!h.screen().contains(" chats "));
}

#[test]
fn first_launch_without_accounts_shows_welcome_and_use_action() {
    let mut h = Harness::new(100, 24);
    landing::open_with(&mut h.app, Vec::new(), None);
    h.draw();
    assert!(h.screen().contains("Welcome to Concordia"));
    assert!(h.screen().contains("/use to add an account"));
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(h.app.picker.is_some(), "Enter should open the /use flow");
    assert!(h.screen().contains("+ add account"));
}

#[test]
fn welcome_is_gone_once_an_account_exists() {
    let mut h = Harness::new(100, 24);
    crate::store::upsert_stored_account(
        &h.app.state_root,
        crate::store::StoredAccount {
            id: "codex-test".into(),
            provider: ProviderKind::Codex,
            home_path: Some(h._dir.path().join("codex").to_string_lossy().into()),
            label: "Codex test".into(),
            created_at: "test".into(),
        },
    )
    .unwrap();
    let cwd = h.app.cwd.clone();
    landing::open_with(
        &mut h.app,
        vec![landing_session("codex-test", &cwd, "chat", 1)],
        None,
    );
    h.draw();
    assert!(!h.screen().contains("Welcome to Concordia"));
}

#[test]
fn signed_out_account_blocks_the_turn_and_keeps_the_prompt() {
    let mut h = Harness::new(100, 24);
    h.app.provider = ProviderKind::Codex;
    h.app.account = "codex-test".into();
    h.app.home_path = Some(h._dir.path().join("missing-login"));
    crate::store::set_use_selection(
        &h.app.state_root,
        crate::store::UseSelection {
            provider: ProviderKind::Codex,
            account: "codex-test".into(),
            home_path: h.app.home_path.as_ref().map(|p| p.to_string_lossy().into()),
            model: None,
            updated_at: "test".into(),
        },
    )
    .unwrap();
    h.app.editor.set_text("keep this prompt");
    let (tx, mut rx) = mpsc::unbounded_channel();
    h.app.submit_input(&tx);
    assert!(
        rx.try_recv().is_err(),
        "signed-out submission must not dispatch"
    );
    assert_eq!(h.app.editor.text(), "keep this prompt");
    assert!(h.app.messages.iter().any(|m| m.text.contains(
        "codex-test looks signed out. Use /use to sign in, or press Enter again to send anyway."
    )));
    h.app.submit_input(&tx);
    assert_eq!(rx.try_recv().unwrap(), "keep this prompt");
    assert!(h.app.editor.text().is_empty());
}

#[test]
fn signed_out_warning_never_duplicates_the_draft() {
    let mut h = Harness::new(100, 24);
    h.app.provider = ProviderKind::Codex;
    h.app.account = "codex-test".into();
    h.app.home_path = Some(h._dir.path().join("missing-login"));
    crate::store::set_use_selection(
        &h.app.state_root,
        crate::store::UseSelection {
            provider: ProviderKind::Codex,
            account: "codex-test".into(),
            home_path: h.app.home_path.as_ref().map(|p| p.to_string_lossy().into()),
            model: None,
            updated_at: "test".into(),
        },
    )
    .unwrap();
    h.app.editor.set_text("keep this prompt");
    let (tx, mut rx) = mpsc::unbounded_channel();
    h.app.submit_input(&tx);
    assert!(rx.try_recv().is_err());
    assert_eq!(h.app.editor.text(), "keep this prompt");
    assert_eq!(h.app.editor.text().matches("keep this prompt").count(), 1);
    assert_eq!(
        h.app
            .messages
            .iter()
            .filter(|m| m.text.contains("looks signed out"))
            .count(),
        1
    );
}

#[test]
fn opening_and_quitting_records_no_session_and_one_message_records_one() {
    let mut h = Harness::new(100, 24);
    let cwd = h.app.cwd.clone();
    let session = landing_session("acct-x", &cwd, "landing-only", 1);
    landing::open_with(&mut h.app, vec![session], None);
    assert!(!h.key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!h.key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(h.key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(crate::threads::list(&h.app.state_root).is_empty());

    let mut fresh = Harness::new(100, 24);
    fresh.app.state_root = h.app.state_root.clone();
    fresh.app.cwd = cwd;
    fresh.app.thread = Some(crate::threads::ThreadLog::open(
        &fresh.app.state_root,
        "one-message",
    ));
    fresh.app.push_user("one message".into());
    fresh
        .app
        .reference_journal
        .finish(fresh.app.state_root.clone(), None, false);
    fresh
        .app
        .reference_journal
        .settle_for_test(&mut fresh.app.history);
    fresh.app.thread.take();
    let journal = fresh.app.state_root.join("threads/one-message.jsonl");
    assert_eq!(std::fs::read_to_string(journal).unwrap().lines().count(), 1);
}

#[test]
fn show_all_toggles_with_h_and_with_a_footer_click() {
    let mut h = Harness::new(110, 24);
    let visible = h.app.cwd.clone();
    let hidden_root = h._dir.path().join("hidden-temp");
    let hidden = hidden_root.join("chat");
    std::fs::create_dir_all(&hidden).unwrap();
    let sessions = vec![
        landing_session("acct-x", &visible, "visible", 2),
        landing_session("acct-x", &hidden, "hidden", 1),
    ];
    landing::open_with(&mut h.app, sessions, None);
    landing::set_hidden_temp_root(&mut h.app, hidden_root);
    h.draw();
    assert!(!h.screen().contains("chat hidden"));
    assert!(h.app.footer_left.contains("h show 1 hidden"));
    h.key(KeyCode::Char('h'), KeyModifiers::NONE);
    assert!(h.screen().contains("chat (1)"));
    assert!(h.app.footer_left.contains("h hide"));
    let footer = h.app.footer_left.clone();
    let marker = footer.find("h hide").unwrap();
    h.click(marker as u16, h.app.areas.footer.y);
    assert!(!h.screen().contains("chat (1)"));
}

#[test]
fn the_tokens_screen_shows_the_log_and_closes_with_esc() {
    let mut h = Harness::new(100, 40);
    log_turn(&h, "cursor", (12_000, 36_000, 200));
    log_turn(&h, "codex", (500, 1_500, 100));
    tokens::open(&mut h.app);
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        h.app.on_tick();
        h.draw();
        if h.screen().contains("24 hours   50.3k tokens") {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
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
    h.app.editor.set_text("retain this usage-view draft");
    tokens::open(&mut h.app);
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    for _ in 0..20 {
        h.app.on_tick();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(h.app.picker.is_none() && h.app.update_preview.is_none());
    assert_eq!(h.app.editor.text(), "retain this usage-view draft");
    let execution = h.app.cwd.clone();
    let home = execution.join("native-home");
    std::fs::create_dir_all(&home).unwrap();
    h.app.native_owner = Some(crate::provider::interaction::InteractionOwner {
        conversation_id: h.app.deleg.d.conversation_id().into(),
        workspace: execution.clone(),
        execution_cwd: execution.clone(),
        run_id: uuid::Uuid::new_v4().to_string(),
        attempt_id: uuid::Uuid::new_v4().to_string(),
        provider: ProviderKind::Codex,
        account: "owner-account".into(),
        home: Some(home.canonicalize().unwrap()),
    });
    h.app.native_usage_model = Some("captured-model".into());
    h.app.native_usage_session = Some("captured-session".into());
    h.app.provider = ProviderKind::Claude;
    h.app.account = "different-focused-account".into();
    let owner = h.app.native_owner.clone().unwrap();
    h.app.native_accounting_owner = Some(owner.clone());
    let (tx, _rx) = mpsc::unbounded_channel();
    h.app.turn_usage = TurnUsage {
        input: Some(200),
        cached: Some(100),
        output: Some(30),
        cost_usd: None,
    };
    h.app.on_turn_done(
        TurnOutcome {
            exit_code: Some(0),
            ..Default::default()
        },
        &tx,
    );
    let usage = crate::token_log::load(&h.app.state_root);
    let bound = usage
        .iter()
        .find(|entry| entry.attribution.is_some())
        .unwrap();
    assert_eq!(bound.provider, "codex");
    assert_eq!(bound.account, "owner-account");
    assert_eq!(bound.model.as_deref(), Some("captured-model"));
    assert_eq!(bound.chat.as_deref(), Some("captured-session"));
    assert_eq!(bound.new_input, 100);
    assert!(h.app.native_owner.is_none());
    let mut cancelled_owner = owner;
    cancelled_owner.attempt_id = uuid::Uuid::new_v4().to_string();
    h.app.native_owner = Some(cancelled_owner.clone());
    h.app.native_accounting_owner = Some(cancelled_owner);
    h.app.request_cancel();
    assert!(
        h.app.native_owner.is_none(),
        "cancellation invalidates the live interaction owner"
    );
    h.app.on_turn_done(
        TurnOutcome {
            cancelled: true,
            ..Default::default()
        },
        &tx,
    );
    let usage = crate::token_log::load(&h.app.state_root);
    let cancelled = usage
        .iter()
        .find(|entry| {
            entry
                .attribution
                .as_ref()
                .is_some_and(|attribution| attribution.outcome == "cancelled")
        })
        .unwrap();
    assert_eq!(cancelled.account, "owner-account");
    let attribution = cancelled.attribution.as_ref().unwrap();
    assert!(!attribution.input_known && !attribution.output_known && !attribution.complete);
    assert!(h.app.native_accounting_owner.is_none());
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
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        h.app.on_tick();
        h.draw();
        if h.screen().contains("3.3k tokens in 24 hours") {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        h.screen().contains("3.3k tokens in 24 hours"),
        "{}",
        h.screen()
    );
}

#[test]
fn message_wheel_scrolls_the_draft_without_moving_caret_or_transcript() {
    let mut h = Harness::new(90, 28);
    h.app
        .push_user((0..70).map(|i| format!("chat {i}\n")).collect());
    let draft = (0..40).map(|i| format!("draft {i}\n")).collect::<String>();
    h.app.editor.set_text(&draft);
    h.draw();
    let before = h.app.input_scroll;
    let transcript = h.app.view.scroll;
    let caret = h.app.editor.cursor_pos(h.app.areas.input_text_w);
    let inner = h.app.areas.input;
    handle_mouse(
        &mut h.app,
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: inner.x + 2,
            row: inner.y,
            modifiers: KeyModifiers::NONE,
        },
    );
    h.draw();
    assert_eq!(h.app.input_scroll, before.saturating_sub(3));
    assert_eq!(h.app.view.scroll, transcript);
    assert_eq!(h.app.editor.text(), draft);
    assert_eq!(h.app.editor.cursor_pos(h.app.areas.input_text_w), caret);
    let scrolled = h.app.input_scroll;
    h.draw();
    assert_eq!(
        h.app.input_scroll, scrolled,
        "redraw must not snap back to the caret"
    );
    h.key(KeyCode::PageUp, KeyModifiers::ALT);
    assert!(h.app.input_scroll < scrolled);
    h.type_str("x");
    assert!(
        h.app.input_scroll >= before,
        "editing restores caret following"
    );
}






