use super::*;

fn cursor_shown(h: &mut Harness) -> Option<ratatui::layout::Position> {
    let backend = h.term.backend_mut();
    ratatui::backend::Backend::get_cursor_position(backend).ok()
}

fn sign_in_codex(base: &std::path::Path) {
    let home = base.join(".codex");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("auth.json"), "{}").unwrap();
}

#[test]
fn the_account_chip_opens_a_dropdown_under_the_header_not_the_bottom_panel() {
    let iso = Isolated::new();
    sign_in_codex(iso.base.path());
    let mut h = Harness::new(140, 30);
    let chip = h.app.areas.header_provider.expect("account chip is drawn");
    h.click(chip.x, chip.y);
    assert_eq!(
        h.app.picker.as_ref().map(|picker| picker.kind),
        Some(PickerKind::Provider)
    );
    let dropdown = h.app.areas.header_dropdown.expect("anchored dropdown");
    assert_eq!(dropdown.y, chip.y + 1, "drops down from the chip");
    assert!(dropdown.x <= chip.x + chip.width);
    h.click(chip.x, chip.y);
    assert!(h.app.picker.is_none(), "a second click closes it");
}

#[test]
fn an_empty_extra_slot_is_not_listed_once_the_provider_has_an_account() {
    let iso = Isolated::new();
    sign_in_codex(iso.base.path());
    let mut h = Harness::new(140, 30);
    h.app.open_provider_picker();
    let ids = h.picker_ids();
    assert!(ids.iter().any(|id| id == "provider:codex"), "{ids:?}");
    assert!(!ids.iter().any(|id| id == "provider:codex-2"), "{ids:?}");
    let claude_rows = ids
        .iter()
        .filter(|id| id.starts_with("provider:claude"))
        .count();
    assert_eq!(
        claude_rows, 1,
        "one connect row per unconnected provider: {ids:?}"
    );
}

#[test]
fn account_rows_are_coloured_by_state() {
    let iso = Isolated::new();
    sign_in_codex(iso.base.path());
    let mut h = Harness::new(140, 30);
    h.app.open_provider_picker();
    assert_eq!(
        h.app.picker_tones.get("provider:claude"),
        Some(&UsageTone::Mute),
        "not connected reads as inactive"
    );
    assert_ne!(
        h.app.picker_tones.get("provider:codex"),
        Some(&UsageTone::Mute)
    );
}

#[test]
fn a_paused_composer_still_shows_a_caret() {
    let mut h = Harness::new(100, 24);
    h.app.open_model_picker();
    h.draw();
    assert!(
        h.screen().contains("message \u{b7} paused"),
        "{}",
        h.screen()
    );
    let caret = cursor_shown(&mut h).expect("cursor position");
    let row = h
        .screen()
        .lines()
        .nth(usize::from(caret.y))
        .unwrap_or("")
        .to_string();
    assert!(
        row.contains("paused") || row.contains("filters") || row.contains("Typing"),
        "caret sits in the paused composer: {caret:?} {row:?}"
    );
}

#[test]
fn settings_tabs_have_no_caret_marker_and_values_line_up() {
    let mut h = Harness::new(140, 30);
    settings::open(&mut h.app);
    h.draw();
    let s = h.screen();
    assert!(!s.contains(">General"), "{s}");
    assert!(s.contains(" General "), "{s}");
    assert!(
        s.contains("Help"),
        "a help pane explains the selected setting: {s}"
    );
    let columns: Vec<usize> = s
        .lines()
        .filter(|line| line.contains("[ "))
        .map(|line| line[..line.find("[ ").unwrap()].chars().count())
        .collect();
    assert!(columns.len() >= 2, "{s}");
    assert!(
        columns.windows(2).all(|pair| pair[0] == pair[1]),
        "values share one column: {columns:?}\n{s}"
    );
}

#[test]
fn settings_permissions_opens_under_its_row_not_in_the_footer() {
    let mut h = Harness::new(140, 30);
    settings::open(&mut h.app);
    h.draw();
    let row = h
        .app
        .areas
        .trust_anchor
        .expect("permissions row is selected first");
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        h.app.picker.as_ref().map(|picker| picker.kind),
        Some(PickerKind::Trust)
    );
    let popup = h.app.areas.permissions_popup;
    assert_eq!(popup.x, row.x);
    assert!(
        popup.y == row.bottom() || popup.bottom() == row.y,
        "popup {popup:?} touches row {row:?}"
    );
    assert!(h.screen().contains("all providers"), "{}", h.screen());
    assert!(
        !h.screen().contains("Help"),
        "the Help pane hides while the permissions menu covers it: {}",
        h.screen()
    );
}

#[test]
fn the_documentation_link_shows_a_hand_pointer() {
    let mut h = Harness::new(140, 40);
    landing::open_with(&mut h.app, Vec::new(), None);
    h.draw();
    let link = h
        .app
        .areas
        .home_documentation
        .expect("Home shows the Documentation link");
    assert_eq!(
        super::super::pointer::shape(&h.app, link.x, link.y),
        "pointer"
    );
}

#[test]
fn home_omits_doctor_failures_and_reports_a_stale_chat_on_open() {
    let _iso = Isolated::new();
    let mut h = Harness::new(140, 40);
    let home = h._dir.path().join("fixture-home");
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    crate::store::upsert_stored_account(
        &h.app.state_root,
        crate::store::StoredAccount {
            id: "fixture".into(),
            provider: ProviderKind::Codex,
            home_path: Some(home.to_string_lossy().into()),
            label: "Fixture".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    let cwd = h.app.cwd.to_string_lossy().into_owned();
    for (id, title) in [
        ("healthy", "Healthy session"),
        ("broken", "Broken handoff session"),
    ] {
        let native = [
            serde_json::json!({"type": "session_meta", "payload": {"id": id, "cwd": cwd}}),
            serde_json::json!({"type": "response_item", "payload": {
                "type": "message", "role": "user", "content": [{"text": title}]
            }}),
        ];
        std::fs::write(
            home.join(format!("sessions/rollout-{id}.jsonl")),
            format!("{}\n{}\n", native[0], native[1]),
        )
        .unwrap();
    }
    crate::store::record_task_session(
        &h.app.state_root,
        "broken-task",
        "Broken handoff session",
        &cwd,
        ProviderKind::Codex,
        "fixture",
        "broken",
        Some(crate::store::TaskMapPointer {
            version: 1,
            thread_id: "missing-map".into(),
        }),
    )
    .unwrap();
    let failures = crate::sessions::unopenable_tasks(&h.app.state_root);
    assert!(failures
        .iter()
        .any(|chat| chat.native_session_id == "broken"
            && chat.failure_detail().contains("missing-map")));
    assert!(crate::sessions::list_recent_sessions(&h.app.state_root, 40)
        .iter()
        .any(|chat| chat.session_id == "broken"));
    landing::open(&mut h.app);
    h.draw();
    let screen = h.screen();
    assert!(screen.contains("Healthy session"), "{screen}");
    assert!(!screen.contains("Broken handoff session"), "{screen}");
    crate::store::hide_account(&h.app.state_root, "fixture").unwrap();
    h.key(KeyCode::Down, KeyModifiers::NONE);
    h.enter();
    assert!(h.screen().contains("resume unavailable"), "{}", h.screen());
    assert!(h.app.session_ui.resume_request.is_none());
}

fn show_home_usage(h: &mut Harness, provider: ProviderKind, snapshot: crate::usage::UsageSnapshot) {
    let here = h.app.cwd.clone();
    landing::open_with(
        &mut h.app,
        vec![landing_session("fixture", &here, "chat", 1)],
        None,
    );
    h.app.landing.as_mut().unwrap().meters = vec![landing::LandingMeter {
        provider,
        account: "fixture".into(),
        snapshot,
    }];
    h.draw();
}

#[test]
fn home_and_account_usage_show_the_same_resets() {
    crate::usage::with_test_clock(11, 25, 8, 0, || {
        let snapshot = crate::usage::parse_claude_usage(concat!(
            "Current session: 46% used · resets Nov 25, 9:35am (local)\n",
            "Current week (all models): 20% used · resets Nov 26, 8am (local)\n",
        ))
        .unwrap();
        for width in [82, 160] {
            let mut h = Harness::new(width, 40);
            show_home_usage(&mut h, ProviderKind::Claude, snapshot.clone());
            let home = h.screen();
            landing::close(&mut h.app);
            let row = picker_row("fixture", ProviderKind::Claude, &snapshot);
            h.app.show_picker(
                PickerKind::Use,
                "accounts".into(),
                vec![PickerRow {
                    id: row.id,
                    label: row.label,
                    detail: row.detail,
                }],
                false,
            );
            h.draw();
            let accounts = h.screen();
            for phrase in ["resets in 1h 35m", "resets Nov 26, 8am"] {
                assert!(home.contains(phrase), "{home}");
                assert!(accounts.contains(phrase), "{accounts}");
                assert!(snapshot.detail.contains(phrase), "{}", snapshot.detail);
            }
        }
    });
}

#[test]
fn whats_new_shows_whole_bullets_or_marks_clipping() {
    let mut wide = Harness::new(220, 40);
    landing::open_with(&mut wide.app, Vec::new(), None);
    wide.draw();
    let wide_screen = wide.screen();
    let bullets: Vec<_> = wide_screen
        .lines()
        .filter_map(|line| line.split_once("  · ").map(|(_, bullet)| bullet.trim_end()))
        .collect();
    assert_eq!(bullets.len(), 3, "{wide_screen}");
    assert!(
        bullets.iter().all(|line| line.ends_with(['.', '…'])),
        "{wide_screen}"
    );
    let mut narrow = Harness::new(80, 40);
    landing::open_with(&mut narrow.app, Vec::new(), None);
    narrow.draw();
    let narrow_screen = narrow.screen();
    assert!(
        narrow_screen
            .lines()
            .any(|line| line.contains("  · ") && line.ends_with('…')),
        "{narrow_screen}"
    );
}

#[test]
fn hidden_accounts_are_omitted_from_switching_and_removal() {
    let iso = Isolated::new();
    sign_in_codex(iso.base.path());
    let mut h = Harness::new(140, 30);
    let account = h.app.areas.header_provider.unwrap();
    h.click(account.x, account.y);
    assert!(h.picker_ids().contains(&"provider:codex".into()));
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    h.type_str("/use remove codex");
    h.enter();
    assert!(
        h.picker_ids().iter().any(|id| id == "rm:hide:codex"),
        "{:?}",
        h.picker_ids()
    );
    h.enter();
    h.click(account.x, account.y);
    assert!(!h.picker_ids().contains(&"provider:codex".into()));
    assert!(!crate::connect::removable(&h.app.state_root)
        .iter()
        .any(|account| account.id == "codex"));
}

#[test]
fn documentation_hit_area_matches_the_rendered_label() {
    for (width, height, welcome) in [
        (140, 40, true),
        (95, 22, false),
        (68, 40, true),
        (68, 10, true),
    ] {
        let mut h = Harness::new(width, height);
        let cwd = h.app.cwd.clone();
        let chats = (!welcome).then(|| landing_session("fixture", &cwd, "hello", 1));
        landing::open_with(&mut h.app, chats.into_iter().collect(), None);
        h.draw();
        let buffer = h.term.backend().buffer();
        let hits: Vec<_> = (0..height)
            .filter_map(|y| {
                let row: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
                row.find("Documentation")
                    .map(|byte| Rect::new(row[..byte].chars().count() as u16, y, 13, 1))
            })
            .collect();
        assert_eq!(
            hits,
            h.app
                .areas
                .home_documentation
                .into_iter()
                .collect::<Vec<_>>(),
            "{}",
            h.screen()
        );
        if let Some(link) = hits.first() {
            assert_eq!(pointer::shape(&h.app, link.right() - 1, link.y), "pointer");
            h.click(link.x, link.y);
            assert!(h.screen().contains("opened link"), "{}", h.screen());
        }
    }
}

#[test]
fn header_chips_keep_the_hand_cursor_with_open_pickers() {
    let mut h = Harness::new(140, 40);
    h.app.model = "gpt-5.6-luna".into();
    h.app.effort = Some("low".into());
    h.draw();
    let account = h.app.areas.header_provider.unwrap();
    h.click(account.x, account.y);
    assert!(h.app.areas.header_dropdown.is_some());
    for bottom_picker in [false, true] {
        if bottom_picker {
            h.app.open_model_picker();
            h.draw();
        }
        let chips = [
            h.app.areas.header_provider,
            h.app.areas.header_model,
            h.app.areas.header_effort,
        ];
        for chip in chips {
            let chip = chip.expect("chip is rendered");
            assert_eq!(pointer::shape(&h.app, chip.x, chip.y), "pointer");
        }
    }
}

#[test]
fn home_token_totals_explain_the_time_period_and_incomplete_history() {
    let mut h = Harness::new(140, 40);
    log_turn(&h, "cursor", (1_000, 2_000, 300));
    let mut old = crate::token_log::load(&h.app.state_root).pop().unwrap();
    old.at -= 48 * 60 * 60;
    old.new_input = 5_000;
    old.cached_input = 0;
    old.output = 0;
    let attribution = old.attribution.as_mut().unwrap();
    attribution.run = uuid::Uuid::new_v4().to_string();
    attribution.complete = false;
    crate::token_log::record(&h.app.state_root, &old).unwrap();
    let cwd = h.app.cwd.clone();
    landing::open_with(
        &mut h.app,
        vec![landing_session("fixture", &cwd, "hello", 1)],
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        h.app.on_tick();
        h.draw();
        if h.screen().contains("8.3k in local history (incomplete)") {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let screen = h.screen();
    assert!(
        screen.contains("3.3k tokens logged in the last 24h · 8.3k in local history (incomplete)"),
        "{screen}"
    );
}

#[test]
fn cursor_usage_based_api_has_no_quota_bar_or_exhausted_percent() {
    let mut h = Harness::new(140, 40);
    let sample = "Usage • Pro\nIncluded 30% used\nAPI 100% used\nOn-Demand Enabled";
    let metered = crate::usage::parse_cursor_usage(sample).unwrap();
    let capped = crate::usage::parse_cursor_usage(&sample.replace("Enabled", "Disabled")).unwrap();
    assert_eq!(crate::usage::usage_tone(&metered), UsageTone::Ok);
    assert_eq!(crate::usage::usage_tone(&capped), UsageTone::Bad);
    show_home_usage(&mut h, ProviderKind::Cursor, metered.clone());
    let screen = h.screen();
    assert!(screen.contains("API usage-based"), "{screen}");
    assert!(!screen.contains("API ["), "{screen}");
    assert!(screen.contains("Included ["), "{screen}");
    let row = picker_row("fixture", ProviderKind::Cursor, &metered);
    assert!(row.label.contains("API usage-based"));
    assert!(!row.label.contains("API 0% left"));
    let details = h.app.usage_block_text(&metered);
    assert!(details.contains("API usage-based"));
    assert!(!details.contains("limit reached"));
}
