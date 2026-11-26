use super::*;

#[test]
fn remove_picker_uses_doctors_login_check() {
    let iso = Isolated::new();
    let codex = iso.base.path().join(".codex");
    let claude = iso.base.path().join(".claude");
    for (home, file, marker) in [
        (&codex, "auth.json", "{}"),
        (
            &claude,
            ".credentials.json",
            "synthetic login existence marker",
        ),
    ] {
        std::fs::create_dir_all(home).unwrap();
        std::fs::write(home.join(file), marker).unwrap();
    }
    let mut h = Harness::new(120, 36);
    h.type_str("/use remove");
    h.enter();
    let screen = h.screen();
    for provider in ["Codex", "Claude"] {
        assert!(
            screen
                .lines()
                .any(|line| line.contains(provider) && line.contains("logged in")),
            "{screen}"
        );
    }
    assert!(!screen.contains("logged out"), "{screen}");
}

#[test]
fn doctor_keeps_severity_color_and_fills_detail_space_inside_a_path() {
    use crate::doctor::{Check, Level};
    let mut h = Harness::new(120, 36);
    h.app.doctor = Some(DoctorUi {
        checks: [
            (
                Level::Ok,
                "codex",
                format!("codex at C:/fixture/{}", "路".repeat(80)),
            ),
            (Level::Warn, "grok", "grok not found".into()),
            (Level::Fail, "store", "unreadable".into()),
        ]
        .into_iter()
        .map(|(level, name, detail)| Check {
            level,
            name,
            detail,
        })
        .collect(),
        fixes: Vec::new(),
        fixed: HashMap::new(),
    });
    h.app.refresh_doctor_picker();
    h.draw();
    let buf = h.term.backend().buffer();
    let styles: Vec<_> = ["ok  codex", "warn  grok", "fail  store"]
        .into_iter()
        .map(|label| buf[find_cell(buf, label)].fg)
        .collect();
    assert_ne!(styles[0], styles[1]);
    assert_ne!(styles[1], styles[2]);
    assert_ne!(styles[0], styles[2]);
    let screen = h.screen();
    let row = screen
        .lines()
        .find(|line| line.contains("ok  codex"))
        .unwrap();
    assert!(
        row.contains("C:/fixture/路") && row.matches('路').count() >= 15 && row.contains("…"),
        "{row}"
    );
}

#[test]
fn usage_clears_the_transcript_and_its_scrollbar_at_every_size() {
    for (width, height) in [(80, 24), (120, 36), (160, 45)] {
        let mut h = Harness::new(width, height);
        for i in 0..height * 2 {
            h.app.push_system(format!("Transcript fixture {i}"));
        }
        h.draw();
        assert!(h.screen().contains("Transcript fixture"));
        assert!(!h.app.areas.transcript_rail.is_empty());
        h.app.show_picker(
            PickerKind::Usage,
            "usage".into(),
            vec![PickerRow {
                id: "usage:fixture".into(),
                label: "Fixture\nno usage data".into(),
                detail: String::new(),
            }],
            false,
        );
        h.draw();
        let card = h.app.picker_rows[0].0;
        let buf = h.term.backend().buffer();
        for y in card.bottom()..h.app.areas.transcript.bottom() {
            assert!(
                (0..width).all(|x| buf[(x, y)].symbol() == " "),
                "{}",
                h.screen()
            );
        }
    }
}

#[test]
fn settings_help_uses_the_available_height() {
    for (width, height) in [(80, 24), (120, 36), (160, 45)] {
        let mut h = Harness::new(width, height);
        settings::open(&mut h.app);
        h.draw();
        let screen = h.screen();
        if width == 80 {
            assert!(
                screen
                    .lines()
                    .any(|line| line.trim().trim_matches('│').trim() == "with"),
                "{screen}"
            );
            h.type_str("permissions");
            let screen = h.screen();
            assert!(screen.contains("with"));
            assert!(!screen.contains("started…"), "{screen}");
        } else {
            let buf = h.term.backend().buffer();
            let (x, _) = find_cell(buf, "│ Help");
            assert_eq!(buf[(x, height - 3)].symbol(), "│", "{screen}");
        }
    }
}

#[test]
fn integrations_uses_the_bottom_picker_width() {
    let _iso = Isolated::new();
    for (width, height) in [(80, 24), (120, 36), (160, 45)] {
        let mut h = Harness::new(width, height);
        h.type_str("/harness");
        h.enter();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !integrations::poll(&mut h.app) {
            assert!(
                Instant::now() < deadline,
                "integration inspection did not finish"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        h.draw();
        let screen = h.screen();
        let row = screen
            .lines()
            .find(|line| line.contains("No integrations connected"))
            .unwrap();
        assert_eq!(
            row.trim().chars().count(),
            usize::from(width - 4),
            "{screen}"
        );
    }
}

#[test]
fn home_places_usage_loading_inside_the_usage_pane() {
    for (width, height) in [(80, 24), (120, 36), (160, 45)] {
        let mut h = Harness::new(width, height);
        let cwd = h.app.cwd.clone();
        landing::open_with(
            &mut h.app,
            vec![landing_session("fixture", &cwd, "Parser review", 1)],
            None,
        );
        h.draw();
        let buf = h.term.backend().buffer();
        let (usage_x, _) = find_cell(buf, "┌ usage");
        let (loading_x, _) = find_cell(buf, "reading usage…");
        assert!(loading_x > usage_x);
        assert!(!h.screen().contains("checking limits..."));
    }
}

#[test]
fn shortcuts_footer_uses_arrow_keys() {
    let mut h = Harness::new(80, 24);
    h.key(KeyCode::F(1), KeyModifiers::NONE);
    let footer = footer_row(&h);
    assert!(footer.contains("↑↓ or PgUp/PgDn scroll"), "{footer}");
}

#[test]
fn header_dropdowns_start_on_the_current_choice() {
    let iso = Isolated::new();
    let home = iso.base.path().join(".codex");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("models_cache.json"),
        r#"{"models":[{"slug":"another-model","display_name":"Another model","visibility":"list"},{"slug":"gpt-5.6-luna","display_name":"GPT-5.6 Luna","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"}]}]}"#,
    ).unwrap();
    for width in [80, 120, 160] {
        let mut h = Harness::new(width, 30);
        h.app.model = "gpt-5.6-luna".into();
        h.app.effort = Some("medium".into());
        h.app.usage_summary = "5h 77% left · 7d 39% left".into();
        h.draw();
        let header = h.screen();
        assert!(header.lines().next().unwrap().contains("GPT-5.6 Luna"));
        assert!(!header.lines().next().unwrap().contains("gpt-5.6-luna"));
        for (chip, id) in [
            (h.app.areas.header_provider, "provider:codex"),
            (h.app.areas.header_model, "model:gpt-5.6-luna"),
            (h.app.areas.header_effort, "effort:medium"),
        ] {
            let chip = chip.unwrap();
            let background = h.term.backend().buffer()[(0, chip.bottom())].bg;
            assert!(h.screen().lines().nth(1).unwrap().contains("5h 77% left"));
            h.click(chip.x, chip.y);
            let picker = h.app.picker.as_ref().unwrap();
            assert_eq!(picker.filtered()[picker.state.selected().unwrap()].id, id);
            let dropdown = h.app.areas.header_dropdown.unwrap();
            assert_eq!(dropdown.y, chip.bottom());
            assert!(dropdown.x > 0);
            let buffer = h.term.backend().buffer();
            for y in dropdown.y..dropdown.bottom() {
                for x in 0..dropdown.x {
                    assert_ne!(buffer[(x, y)].bg, background, "{id} at ({x}, {y})");
                }
            }
            if picker.kind == PickerKind::Model {
                assert!(h.screen().contains("gpt-5.6-luna"));
            }
            h.key(KeyCode::Esc, KeyModifiers::NONE);
        }
    }
}

#[test]
fn account_picker_distinguishes_connection_from_usage() {
    let iso = Isolated::new();
    let cursor = iso.base.path().join(".cursor");
    std::fs::create_dir_all(&cursor).unwrap();
    std::fs::write(
        cursor.join("cli-config.json"),
        r#"{"authInfo":{"email":"fixture@example.invalid"}}"#,
    )
    .unwrap();
    std::fs::write(cursor.join("concordia-api-key"), "synthetic").unwrap();
    let mut h = Harness::new(120, 36);
    h.app.usage_fetcher = UsageFetcher::with_runner(
        h.app.state_root.clone(),
        std::sync::Arc::new(|_| Err("no usage fixture".into())),
    )
    .with_cursor_runner(std::sync::Arc::new(|_| Err("no usage fixture".into())));
    h.app.open_provider_picker();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !h
        .app
        .picker
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|row| row.id == "provider:cursor" && row.detail == "no usage data")
    {
        assert!(Instant::now() < deadline, "usage fixture is unavailable");
        std::thread::sleep(Duration::from_millis(10));
        h.app.usage_fetcher.poll();
        h.app.open_provider_picker();
    }
    h.draw();
    let screen = h.screen();
    assert!(screen
        .lines()
        .any(|line| line.contains("Cursor") && line.contains("no usage data")));
    assert!(screen
        .lines()
        .any(|line| line.contains("Claude") && line.contains("not connected")));
    let buf = h.term.backend().buffer();
    let connected = find_cell(buf, "no usage data");
    let disconnected = find_cell(buf, "not connected");
    assert_ne!(buf[connected].fg, buf[disconnected].fg);
    h.app.open_use_picker();
    h.draw();
    let screen = h.screen();
    assert!(!screen.contains("fixture@example.invalid"));
    for action in [
        "Add account",
        "Remove account",
        "Choose next provider",
        "Set up Grok Bot routine",
    ] {
        assert!(screen.contains(action), "{screen}");
    }
    assert!(!screen.contains("+ Add"));
    assert!(!screen.contains("- Remove"));
}

#[test]
fn panel_titles_and_picker_footers_use_shared_spacing_and_keys() {
    for (width, height) in [(80, 24), (120, 36), (160, 45)] {
        let mut h = Harness::new(width, height);
        for command in [
            "/use", "/use add", "/usage", "/resume", "/model", "/mode", "/config", "/open",
        ] {
            h.send(command);
            let screen = h.screen();
            for row in screen.lines() {
                for (left, right) in [('┌', '┐'), ('╭', '╮')] {
                    for panel in row.split_inclusive(right) {
                        if let Some((_, title)) = panel.split_once(left) {
                            if let Some((words, _)) = title.split_once('─') {
                                assert!(
                                    words.is_empty()
                                        || words.starts_with(' ') && words.ends_with(' '),
                                    "{command}\n{row}"
                                );
                            }
                        }
                    }
                }
            }
            let footer = footer_row(&h);
            assert!(footer.contains("Esc close"), "{command}\n{footer}");
            assert!(!footer.contains("Enter apply"), "{command}\n{footer}");
            h.key(KeyCode::Esc, KeyModifiers::NONE);
        }
    }
}

#[test]
fn usage_cards_put_titles_in_borders_and_reveal_selected_rows() {
    for (width, height) in [(80, 24), (120, 36), (160, 45)] {
        let mut h = Harness::new(width, height);
        let rows = (0..8)
            .map(|i| PickerRow {
                id: format!("usage:fixture-{i}"),
                label: format!("Account {i}\nno usage data"),
                detail: "line one\nline two\nline three".repeat(i % 2),
            })
            .collect();
        h.app
            .show_picker(PickerKind::Usage, "usage".into(), rows, false);
        h.draw();
        let columns = h.app.picker.as_ref().unwrap().usage_cols;
        let cards = &h.app.picker_rows;
        let mut previous = None;
        for (card, index) in cards.iter().step_by(columns) {
            let screen = h.screen();
            assert!(screen
                .lines()
                .nth(card.y as usize)
                .unwrap()
                .contains(&format!("┌ Account {index} ")));
            if let Some(bottom) = previous {
                assert_eq!(card.y, bottom + 1);
            }
            previous = Some(card.bottom());
        }
        for _ in 0..7 {
            h.key(KeyCode::Right, KeyModifiers::NONE);
        }
        assert!(h.screen().contains("┌ Account 7 "));
    }
}

#[test]
fn rename_footer_follows_the_editor_on_save_escape_and_click() {
    let mut h = Harness::new(120, 30);
    for action in [KeyCode::Enter, KeyCode::Esc, KeyCode::Null] {
        h.key(KeyCode::F(2), KeyModifiers::NONE);
        h.type_str("Fixture title");
        assert!(footer_row(&h).contains("Enter save title · Esc cancel"));
        if action == KeyCode::Null {
            let input = h.app.areas.input;
            h.click(input.x, input.y);
        } else {
            h.key(action, KeyModifiers::NONE);
        }
        assert!(h.app.rename_editor.is_none());
        assert!(!footer_row(&h).contains("save title"));
    }
}

#[test]
fn empty_chat_and_running_tools_show_only_their_own_hints() {
    let mut h = Harness::new(120, 30);
    assert!(h.screen().contains(START_HINT));
    h.app.push_user("Run the tests".into());
    h.app.busy = true;
    h.app.apply_progress(TurnProgress::Card(StreamCard {
        id: "shell-fixture".into(),
        title: "Run command".into(),
        detail: Some("command: cargo test".into()),
        status: CardStatus::Running,
    }));
    h.draw();
    let screen = h.screen();
    assert!(!screen.contains(START_HINT));
    assert_eq!(screen.matches("Run tests").count(), 1, "{screen}");
    assert!(
        screen
            .lines()
            .any(|line| line.contains("Codex · running a tool · 0s · [ Esc ] cancel")),
        "{screen}"
    );
    assert!(footer_row(&h).contains("Esc cancel"));
    assert!(!screen.contains("Ctrl+C cancel"));
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(!h.app.busy);
}

#[test]
fn home_keeps_chat_rows_clear_of_the_intro() {
    for (width, height) in [(80, 24), (120, 36), (160, 45)] {
        let mut h = Harness::new(width, height);
        let cwd = h.app.cwd.clone();
        landing::open_with(
            &mut h.app,
            vec![landing_session("intro-fixture", &cwd, "Parser review", 1)],
            None,
        );
        h.draw();
        let screen = h.screen();
        let chats = screen
            .lines()
            .position(|row| row.contains("┌ chats "))
            .unwrap();
        for label in ["What's new", "Documentation", "F1  Shortcuts"] {
            assert!(
                screen.lines().take(chats).any(|row| row.contains(label))
                    || !screen.contains(label)
            );
        }
        assert!(screen.contains("Parser review"));
    }
}

#[test]
fn every_home_footer_hint_performs_its_action() {
    for (width, height) in [(80, 24), (120, 36), (160, 45)] {
        for state in [
            "welcome",
            "expanded",
            "collapsed",
            "hidden",
            "showing hidden",
        ] {
            let setup = || {
                let mut h = Harness::new(width, height);
                let cwd = h.app.cwd.clone();
                let mut sessions = Vec::new();
                if matches!(state, "expanded" | "collapsed") {
                    sessions.push(landing_session("fixture", &cwd, "visible", 1));
                }
                if state != "welcome" {
                    let hidden = cwd.join("concordia-test-hidden");
                    std::fs::create_dir_all(&hidden).unwrap();
                    sessions.push(landing_session("fixture", &hidden, "hidden", 1));
                }
                landing::open_with(&mut h.app, sessions, None);
                h.draw();
                if state == "collapsed" {
                    h.enter();
                } else if state == "showing hidden" {
                    h.key(KeyCode::Char('h'), KeyModifiers::NONE);
                }
                h
            };
            let h = setup();
            let footer = footer_row(&h);
            let end = h.app.areas.permissions.x as usize;
            let hints = footer.chars().take(end).collect::<String>();
            let hints = hints.trim().split(" · ").collect::<Vec<_>>();
            assert!(!hints[0].is_empty(), "{state}\n{footer}");
            for hint in hints {
                let mut h = setup();
                match hint {
                    "Enter add account" => {
                        h.enter();
                        assert_eq!(h.app.picker.as_ref().unwrap().kind, PickerKind::AddProvider);
                    }
                    "Enter collapse folder" | "Enter expand folder" => {
                        let chat = if state == "showing hidden" {
                            "chat hidden"
                        } else {
                            "chat visible"
                        };
                        let before = h.screen().contains(chat);
                        h.enter();
                        let screenshot = h.screen();
                        assert_ne!(screenshot.contains(chat), before, "{hint}\n{screenshot}");
                    }
                    "Esc chat" => {
                        h.key(KeyCode::Esc, KeyModifiers::NONE);
                        assert!(matches!(
                            h.app.tab_request,
                            Some(tabs::TabAction::BackToChat)
                        ));
                    }
                    "h show 1 hidden" | "h hide" => {
                        h.key(KeyCode::Char('h'), KeyModifiers::NONE);
                        assert!(!footer_row(&h).contains(hint));
                    }
                    _ => panic!("No action checked for {hint:?} in {state}"),
                }
            }
        }
    }
}

#[test]
fn rendered_chrome_uses_middle_dots() {
    let mut h = Harness::new(120, 36);
    h.app.push_user("Run the parser checks".into());
    h.app.live_thinking = "Checking parser coverage. ".repeat(30);
    h.app.commit_live_thinking();
    h.app.push_card(StreamCard {
        id: "command-fixture".into(),
        title: "Ran command".into(),
        detail: Some("command: check parser\nline one\nline two".into()),
        status: CardStatus::Done,
    });
    h.app.finalize_turn(TurnOutcome {
        failed: true,
        exit_code: Some(3),
        ..TurnOutcome::default()
    });
    let provider = h.app.reply_meta().1;
    for (expanded, label) in [(false, "Expand"), (true, "Collapse")] {
        h.app.expanded_activity = expanded;
        h.draw();
        let screen = h.screen();
        assert!(screen.contains(&format!("Thinking · {label}")), "{screen}");
        assert!(
            screen
                .lines()
                .any(|row| row.contains("Turn failed.") && row.contains(&format!(" · {label}"))),
            "{screen}"
        );
        assert_eq!(
            screen.lines().filter(|row| row.trim() == provider).count(),
            1,
            "{screen}"
        );
        assert!(!screen.contains(" - "), "{screen}");
    }
}

#[test]
fn doctor_footer_offers_review_on_a_fixable_row() {
    let mut h = Harness::new(80, 24);
    open_doctor_fixture(&mut h, true);
    h.draw();
    assert!(footer_row(&h).contains("Enter review fix"));
    h.enter();
    assert_eq!(
        h.app.picker.as_ref().unwrap().kind,
        PickerKind::DoctorConfirm
    );
    open_doctor_fixture(&mut h, false);
    h.draw();
    assert!(!footer_row(&h).contains("Enter"));
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.app.picker.is_none());
}

#[test]
fn footer_permissions_overlays_the_composer_with_the_draft_visible() {
    for (width, height) in [(80, 24), (120, 36), (160, 45)] {
        let mut h = Harness::new(width, height);
        for i in 0..height {
            h.app.push_system(format!(
                "transcript row {i} {}",
                "parser".repeat(width as usize / 6)
            ));
        }
        for slash in [false, true] {
            h.app.editor.set_text(if slash {
                "/mode"
            } else {
                "keep this draft visible"
            });
            h.draw();
            let chip = h.app.areas.permissions;
            if slash {
                h.enter();
            } else {
                h.click(chip.x + 1, chip.y);
            }
            let popup = h.app.areas.permissions_popup;
            let input = h.app.areas.input;
            assert_eq!(popup.bottom(), chip.y);
            assert_eq!(popup.right(), chip.right());
            assert!(popup.x > input.x && !popup.intersection(input).is_empty());
            let screen = h.screen();
            for row in popup.intersection(h.app.areas.transcript).rows() {
                assert!(
                    screen
                        .lines()
                        .nth(row.y as usize)
                        .unwrap()
                        .chars()
                        .take(popup.x as usize)
                        .all(|c| c == ' '),
                    "{screen}"
                );
            }
            if !slash {
                assert!(screen.contains("keep this draft visible"), "{screen}");
            }
            h.key(KeyCode::Esc, KeyModifiers::NONE);
        }
    }
}

#[test]
fn activity_search_renders_singular_and_plural_match_counts() {
    let mut h = Harness::new(100, 28);
    h.app.push_card(StreamCard {
        id: "cmd".into(),
        title: "shell".into(),
        detail: Some("parser one\nparser two".into()),
        status: CardStatus::Done,
    });
    h.app.activity.open(h.app.messages.len() - 1);
    h.draw();
    h.type_str("one");
    let screen = h.screen();
    assert!(screen.contains(" · 1 match "), "{screen}");
    for _ in 0..3 {
        h.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    h.type_str("parser");
    let screen = h.screen();
    assert!(screen.contains(" · 2 matches "), "{screen}");
}

#[test]
fn settings_selection_and_popup_follow_the_value_cells() {
    for width in [80, 120, 160] {
        let mut h = Harness::new(width, 30);
        settings::open(&mut h.app);
        h.draw();
        let anchor = h.app.areas.trust_anchor.unwrap();
        let before = h.term.backend().buffer().clone();
        let buf = &before;
        assert_eq!(buf[(anchor.x, anchor.y)].symbol(), "[");
        assert_eq!(buf[(anchor.right() - 1, anchor.y)].symbol(), "]");
        assert_ne!(
            buf[(anchor.right() - 1, anchor.y)].bg,
            buf[(anchor.right(), anchor.y)].bg
        );
        h.enter();
        let popup = h.app.areas.permissions_popup;
        assert_eq!(popup.x, anchor.x);
        assert!(popup.y == anchor.bottom() || popup.bottom() == anchor.y);
        let rows: Vec<u16> = h
            .app
            .settings_view
            .as_ref()
            .unwrap()
            .row_areas
            .iter()
            .map(|(rect, _)| rect.y)
            .filter(|y| (popup.y..popup.bottom()).contains(y))
            .collect();
        let buf = h.term.backend().buffer();
        for y in rows {
            for x in 0..popup.x {
                assert_eq!(buf[(x, y)], before[(x, y)], "{width} cols at ({x}, {y})");
            }
        }
    }
}

#[test]
fn agent_roles_and_completed_status_are_readable() {
    for width in [80, 120, 160] {
        let mut h = Harness::new(width, 30);
        for (i, role) in ["Orchestrator", "Re-Reviewer"].into_iter().enumerate() {
            h.app.push_card(StreamCard {
                id: format!("native-agent:fixture-{i}"),
                title: format!("agent fixture-{i}"),
                detail: Some(format!(
                    "Role: {role}\nModel: Fixture\nTask: Review the parser"
                )),
                status: CardStatus::Done,
            });
        }
        h.key(KeyCode::Char('g'), KeyModifiers::CONTROL);
        let screen = h.screen();
        let rows = screen
            .lines()
            .filter_map(|row| {
                row.split_once('│')
                    .and_then(|(_, detail)| detail.split_once("││"))
            })
            .map(|(_, list)| list)
            .collect::<Vec<_>>();
        assert!(
            rows.iter().any(|row| row.contains("Orchestrator")),
            "{screen}"
        );
        assert!(
            rows.iter().any(|row| row.contains("Re-Reviewer")),
            "{screen}"
        );
        assert!(screen.contains("✓ finished"), "{screen}");
        assert!(!screen.contains("✓ Inactive"));
    }
}
