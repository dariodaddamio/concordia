use super::*;

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

#[test]
fn update_is_a_fresh_page_and_escape_restores_chat_and_draft() {
    for (width, height) in [(40, 12), (90, 28), (150, 45)] {
        let mut h = Harness::new(width, height);
        h.app.push_user("CHAT_SENTINEL".into());
        h.type_str("DRAFT_SENTINEL");
        update_screen(&mut h, update_preview(true, 1));
        let screen = h.screen();
        assert!(screen.contains("Installed"), "{screen}");
        assert!(!screen.contains("CHAT_SENTINEL"), "{screen}");
        assert!(!screen.contains("DRAFT_SENTINEL"), "{screen}");
        assert_eq!(h.app.areas.input, Rect::default());
        assert_eq!(h.app.areas.transcript, Rect::default());
        h.key(KeyCode::Esc, KeyModifiers::NONE);
        assert!(h.screen().contains("CHAT_SENTINEL"), "{}", h.screen());
        assert_eq!(h.app.editor.text(), "DRAFT_SENTINEL");
    }
}

#[test]
fn update_check_failure_is_visible_on_the_page() {
    let mut h = Harness::new(90, 28);
    update_screen(&mut h, update_preview(false, 0));
    h.app.update_preview = None;
    h.app.job = Some(spawn_job(JobKind::Check, |_| {
        JR::Check(Err(anyhow::anyhow!("synthetic feed unavailable")))
    }));
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.app.job.is_some() && Instant::now() < deadline {
        h.app.poll_job();
        std::thread::sleep(Duration::from_millis(5));
    }
    h.draw();
    let screen = h.screen();
    assert!(screen.contains("synthetic feed unavailable"), "{screen}");
    assert!(!screen.contains("checking for updates"), "{screen}");
    assert!(!h.app.update_preview.as_ref().unwrap().install_available);
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
    h.type_str("keep my draft");
    update_screen(&mut h, update_preview(false, 0));
    h.enter();
    assert!(h.app.picker.is_none() && h.app.update_preview.is_none());
    assert!(h.app.job.is_none() && !h.app.busy);
    assert_eq!(h.app.editor.text(), "keep my draft");

    // A busy chat blocks installation and keeps the explanation on screen.
    let mut h = Harness::new(80, 30);
    update_screen(&mut h, update_preview(true, 0));
    h.app.busy = true;
    h.enter();
    assert!(h.app.picker.is_some());
    assert!(h.app.job.is_none());
    assert!(h.screen().contains("Update blocked"), "{}", h.screen());
    assert!(h.transcript_text().contains("has not been queued"));
}

#[test]
fn the_update_screen_scrolls_to_its_last_line() {
    let mut h = Harness::new(80, 16);
    update_screen(&mut h, update_preview(true, 30));
    assert!(!h.screen().contains("change number 29"));
    for _ in 0..40 {
        h.key(KeyCode::PageDown, KeyModifiers::NONE);
        h.draw();
    }
    let screen = h.screen();
    assert!(screen.contains("Esc close"), "{screen}");
    assert!(screen.contains("change number 29"), "{screen}");
}

#[test]
fn update_action_stays_visible_and_emphasized_while_the_log_scrolls() {
    for no_color in [false, true] {
        let mut h = Harness::new(90, 24);
        h.app.no_color = no_color;
        update_screen(&mut h, update_preview(true, 40));
        for _ in 0..20 {
            h.key(KeyCode::PageDown, KeyModifiers::NONE);
        }
        let screen = h.screen();
        assert!(
            screen.contains("Enter  Install + restart this tab"),
            "{screen}"
        );
        assert!(screen.contains("Other tabs keep running"), "{screen}");
        assert!(screen.contains("change number 39"), "{screen}");
        let buffer = h.term.backend().buffer();
        let caption_y = screen
            .lines()
            .position(|line| line.contains("Enter  Install + restart this tab"))
            .unwrap();
        let caption_x = screen
            .lines()
            .nth(caption_y)
            .unwrap()
            .chars()
            .position(|c| c == 'E')
            .unwrap();
        assert!(buffer[(caption_x as u16, caption_y as u16)]
            .modifier
            .contains(Modifier::BOLD));
        if no_color {
            assert!(buffer.content.iter().all(|cell| cell.bg == Color::Reset));
        } else {
            assert!(buffer.content.iter().any(|cell| cell.bg == Color::Green
                && cell.fg == Color::Black
                && cell.modifier.contains(Modifier::BOLD)));
        }
    }
}

#[test]
fn update_blocks_background_workers_and_never_queues_an_install() {
    let mut h = Harness::new(100, 30);
    h.app.deleg.d.push_test_job(
        "synthetic-worker",
        ProviderKind::Codex,
        crate::delegate::WorkerState::Tool,
        "working",
        1,
    );
    update_screen(&mut h, update_preview(true, 10));
    h.enter();
    assert!(h.app.job.is_none() && !h.app.should_quit);
    assert!(h.app.picker.is_some());
    assert!(h.screen().contains("Update blocked: work pending"));
    assert!(h
        .term
        .backend()
        .buffer()
        .content
        .iter()
        .any(|cell| cell.bg == Color::Yellow));
    assert_eq!(
        h.app.deleg.d.jobs[0].state,
        crate::delegate::WorkerState::Tool
    );
    h.app.deleg.d.jobs[0].state = crate::delegate::WorkerState::Done;
    h.draw();
    assert!(h.screen().contains("Enter  Install + restart this tab"));
    assert!(
        h.app.job.is_none(),
        "finishing work must not start the update automatically"
    );
}

#[test]
fn workers_cannot_start_during_an_update() {
    let mut h = Harness::new(100, 24);
    h.app.job = Some(fake_update_job());
    let (command, _) =
        crate::commands::parse_slash("/delegate synthetic-worker check files").unwrap();
    assert!(h.app.try_delegate_slash(&command));
    assert!(h
        .transcript_text()
        .contains("Workers are paused while Concordia updates"));
    assert!(!h.app.deleg.d.any_active());
    let (command, _) = crate::commands::parse_slash("/delegate use").unwrap();
    assert!(h.app.try_delegate_slash(&command));
    assert!(h
        .transcript_text()
        .contains("Worker results cannot be sent"));
    assert!(!h.app.deleg.has_pending_prompt());
    h.app.job.as_ref().unwrap().cancel();
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
            history: Vec::new(),
            history_expanded: false,
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
        visible.find("Checking the first file.").unwrap() < visible.find("Run command").unwrap()
    );
    h.key(KeyCode::Char('o'), KeyModifiers::CONTROL);
    h.draw();
    assert!(
        h.screen().contains("first-check"),
        "expanded command keeps its exact source"
    );
    h.key(KeyCode::Char('o'), KeyModifiers::CONTROL);
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
    assert!(h.screen().contains("Codex · starting the turn"));

    h.app
        .apply_progress(TurnProgress::Thinking("weighing the options".into()));
    h.draw();
    let s = h.screen();
    assert!(
        s.contains("Working through the next step") && s.contains("weighing the options"),
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
    assert!(h.screen().contains("Run tests"));
    h.key(KeyCode::Char('o'), KeyModifiers::CONTROL);
    h.draw();
    assert!(
        h.screen().contains("cargo test"),
        "expanded tool keeps its exact command"
    );
    h.key(KeyCode::Char('o'), KeyModifiers::CONTROL);
    assert_eq!(h.app.phase.color(), Color::Yellow);

    h.app.apply_progress(TurnProgress::Text("Done.".into()));
    h.draw();
    assert!(h.screen().contains("Putting the answer together"));
    assert_eq!(h.app.phase.color(), Color::Green);
}

#[test]
fn thinking_is_kept_dimmed_after_the_turn_and_never_sent_as_context() {
    let _iso = Isolated::new();
    let mut h = Harness::new(100, 36);
    h.app.push_user("why".into());
    h.app.busy = true;
    // Long enough that the collapsed preview hides text, so the accordion has work to do.
    let long = "x".repeat(600);
    h.app.apply_progress(TurnProgress::Thinking(format!(
        "first   idea\n\n\nsecond idea {long}"
    )));
    h.app.apply_progress(TurnProgress::Text("Because.".into()));
    h.app.apply_progress(TurnProgress::Card(StreamCard {
        id: "streamed-edit".into(),
        title: "Edit".into(),
        detail: Some("File: example.rs".into()),
        status: CardStatus::Running,
    }));
    h.app.finalize_turn(TurnOutcome {
        exit_code: Some(0),
        cards: vec![
            StreamCard {
                id: "streamed-edit".into(),
                title: "Edit".into(),
                detail: Some("File: example.rs\n@@\n-before\n+after".into()),
                status: CardStatus::Done,
            },
            StreamCard {
                id: "late-edit".into(),
                title: "Edit".into(),
                detail: Some("File: late.rs\n@@\n-old\n+new".into()),
                status: CardStatus::Done,
            },
        ],
        ..TurnOutcome::default()
    });
    let thought = h
        .app
        .messages
        .iter()
        .find(|m| m.role == MsgRole::Thinking)
        .expect("a thinking line");
    assert_eq!(
        thought.text,
        format!("first   idea\n\n\nsecond idea {long}\n")
    );
    assert!(h
        .app
        .pack_messages()
        .iter()
        .all(|m| !m.text.contains("idea")));
    assert!(h.app.live_thinking.is_empty());
    let cards: Vec<_> = h
        .app
        .messages
        .iter()
        .filter_map(|m| m.card.as_ref())
        .collect();
    assert_eq!(
        cards.len(),
        2,
        "final cards reconcile instead of disappearing or duplicating"
    );
    assert!(cards.iter().all(|card| card.status == CardStatus::Done));
    h.draw();
    assert!(!h.screen().contains("Review activity"));
    assert!(h.screen().contains("Worked for"));
    let thought_index = h
        .app
        .messages
        .iter()
        .position(|m| m.role == MsgRole::Thinking)
        .unwrap();
    let row = h
        .app
        .view
        .rows
        .iter()
        .position(|row| {
            row.text
                .iter()
                .collect::<String>()
                .contains("Thinking - Expand")
        })
        .unwrap();
    let y = h.app.areas.transcript.y + (row - h.app.view.visible().start) as u16;
    let x = h.app.areas.transcript.x + 4;
    assert_eq!(pointer::shape(&h.app, x, y), "pointer");
    h.click(x, y);
    h.draw();
    assert!(h.app.activity.is_expanded(thought_index, false));
    assert!(
        h.app.activity.detail.is_none(),
        "thinking expands in chat, never a modal"
    );
    assert!(h.screen().contains("Thinking - Collapse"));
    assert!(h.screen().contains("first") && h.screen().contains("second idea"));
    assert_eq!(
        h.app.messages[thought_index].text,
        format!("first   idea\n\n\nsecond idea {long}\n")
    );
    h.click(x, y);
    h.draw();
    assert!(!h.app.activity.is_expanded(thought_index, false));
    assert!(h.screen().contains("Thinking - Expand"));
    h.key(KeyCode::Char('o'), KeyModifiers::CONTROL);
    h.draw();
    assert!(h.screen().contains("Thinking - Collapse"));
    assert!(h.app.activity.detail.is_none());
    h.app.messages.clear();
    h.app.messages.push(ChatLine {
        role: MsgRole::Thinking,
        text: "## Checking **formatting**\nSee [E2E status](docs/e2e-status.md).".into(),
        card: None,
        meta: None,
    });
    h.app.expanded_activity = true;
    h.draw();
    assert!(h.screen().contains("Checking formatting"), "{}", h.screen());
    assert!(!h.screen().contains("**formatting**"));
    assert!(!h.screen().contains("](docs/"));
    assert!(h.app.view.rows.iter().any(|row| row
        .links
        .iter()
        .any(|(_, _, c)| c.target == "docs/e2e-status.md")));
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
    assert!(!screen.contains("updating Concordia"), "{screen}");
    assert_eq!(screen.matches("updating").count(), 1, "{screen}");
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

#[test]
fn the_update_screen_turns_into_progress_and_esc_cancels_the_update() {
    let mut h = Harness::new(100, 24);
    update_screen(&mut h, update_preview(true, 0));
    h.app.job = Some(fake_update_job());
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.app.job.as_ref().unwrap().status().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    h.app.update_progress();
    h.draw();
    let screen = h.screen();
    assert!(screen.contains("Updating Concordia"), "{screen}");
    assert_eq!(screen.matches("Updating Concordia").count(), 1, "{screen}");
    assert!(
        !screen.contains("updating ·"),
        "the page owns progress; footer has controls only: {screen}"
    );
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

#[test]
fn landing_draft_paste_keeps_lists_and_enter_runs_the_command() {
    let mut h = landing_harness();
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    h.app.paste_text("/usage");
    h.draw();
    assert!(landing::visible(&h.app));
    assert!(h.screen().contains(" chats "));
    assert_eq!(h.app.editor.text(), "/usage");
    h.enter();
    assert!(h.app.picker.is_some());
}

#[test]
fn landing_chats_wrap_up_and_down() {
    let mut h = landing_harness();
    h.key(KeyCode::Up, KeyModifiers::NONE);
    h.enter();
    assert!(
        h.screen().contains("chat third"),
        "Up wraps to the last folder"
    );
    h.key(KeyCode::Up, KeyModifiers::NONE);
    h.key(KeyCode::Down, KeyModifiers::NONE);
    h.key(KeyCode::Down, KeyModifiers::NONE);
    h.enter();
    assert!(
        !h.screen().contains("chat first"),
        "Down wraps to the first folder, which Enter collapses"
    );
}

#[test]
fn landing_goals_wrap_in_both_directions() {
    let mut h = landing_harness();
    let goals: Vec<_> = ["first goal", "last goal"]
        .iter()
        .enumerate()
        .map(|(i, text)| landing::Goal {
            id: format!("goal-{i}"),
            text: text.to_string(),
            done: false,
            added_at: "fixture".into(),
            closed_at: None,
            ..Default::default()
        })
        .collect();
    landing::save_goals(&h.app.state_root, &goals).unwrap();
    let here = h.app.cwd.clone();
    landing::open_with(
        &mut h.app,
        vec![landing_session("acct-x", &here, "first", 1)],
        None,
    );
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    h.key(KeyCode::Up, KeyModifiers::NONE);
    h.key(KeyCode::Down, KeyModifiers::NONE);
    h.enter();
    h.enter();
    let saved = landing::load_goals(&h.app.state_root);
    assert!(saved[0].done);
    assert!(!saved[1].done);
}
