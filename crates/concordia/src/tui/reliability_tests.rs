//! Deterministic reliability checks requested for the overnight feature audit.
use super::*;

fn resize(h: &mut Harness, width: u16, height: u16) {
    h.term.backend_mut().resize(width, height);
    h.term.resize(Rect::new(0, 0, width, height)).unwrap();
    h.draw();
}

fn open_landing(h: &mut Harness) {
    let session = crate::sessions::SessionSummary {
        account: "fixture".into(),
        provider: ProviderKind::Codex,
        session_id: "synthetic-session".into(),
        cwd: Some(h.app.cwd.to_string_lossy().into_owned()),
        preview: Some("fixture chat".into()),
        modified_ms: 1,
    };
    landing::open_with(&mut h.app, vec![session], None);
    h.draw();
}

#[test]
fn resizing_through_tiny_windows_preserves_draft_and_never_panics() {
    let mut h = Harness::new(120, 40);
    let draft = "Unicode \u{65e5}\u{672c}\u{8a9e} \u{1f600}\nsecond line\nthird";
    h.app.editor.set_text(draft);
    for i in 0..30 {
        h.app.push_assistant(format!(
            "reply {i}: **bold** and `code`\n- item\n\tindented"
        ));
    }
    for width in [0, 1, 2, 3, 4, 8, 12, 20, 40, 69, 70, 80, 120, 180] {
        for height in [0, 1, 2, 3, 4, 5, 8, 12, 24, 40] {
            resize(&mut h, width, height);
            assert_eq!(h.app.editor.text(), draft, "{width}x{height}");
        }
    }
    resize(&mut h, 120, 40);
    assert!(h.screen().contains("second line"));
    h.app.editor.clear();
    h.app.messages.clear();
    open_landing(&mut h);
    for (width, height) in [(0, 0), (1, 1), (8, 4), (40, 8), (70, 10), (120, 40)] {
        resize(&mut h, width, height);
    }
}

#[test]
fn long_goal_lists_keep_selected_goal_confirmation_and_entry_visible() {
    let mut h = Harness::new(100, 20);
    let goals: Vec<_> = (0..40)
        .map(|i| landing::Goal {
            id: format!("goal-{i}"),
            text: format!("goal-{i:02}"),
            done: false,
            added_at: "0".into(),
            closed_at: None,
            ..Default::default()
        })
        .collect();
    landing::save_goals(&h.app.state_root, &goals).unwrap();
    open_landing(&mut h);
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    for _ in 0..39 {
        h.key_down();
    }
    assert!(h.screen().contains("> \u{25cb} goal-39"), "{}", h.screen());
    h.enter();
    assert!(h.screen().contains("Mark done?"), "{}", h.screen());
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    h.type_str("a");
    h.type_str("new-visible-goal");
    assert!(h.screen().contains("new-visible-goal"), "{}", h.screen());
    h.enter();
    assert!(h.screen().contains("new-visible-goal"));
}

#[test]
fn narrowing_the_landing_page_keeps_goal_input_available() {
    let mut h = Harness::new(100, 24);
    open_landing(&mut h);
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    h.type_str("a");
    h.type_str("keep this goal");
    resize(&mut h, 40, 20);
    assert!(h.screen().contains("keep this goal"), "{}", h.screen());
    h.enter();
    assert_eq!(
        landing::load_goals(&h.app.state_root)[0].text,
        "keep this goal"
    );
}

#[test]
fn independent_goal_saves_preserve_other_panes_additions_and_completed_status() {
    let h = Harness::new(80, 24);
    let goal = |id: &str| landing::Goal {
        id: id.into(),
        text: id.into(),
        done: false,
        added_at: "0".into(),
        closed_at: None,
        ..Default::default()
    };
    let base = goal("base");
    landing::save_goals(&h.app.state_root, std::slice::from_ref(&base)).unwrap();
    landing::save_goals(&h.app.state_root, &[base.clone(), goal("pane-one")]).unwrap();
    landing::save_goals(&h.app.state_root, &[base.clone(), goal("pane-two")]).unwrap();
    assert_eq!(landing::load_goals(&h.app.state_root).len(), 3);
    let mut done = base.clone();
    done.done = true;
    done.closed_at = Some("1".into());
    landing::save_goals(&h.app.state_root, &[done]).unwrap();
    landing::save_goals(&h.app.state_root, &[base, goal("pane-three")]).unwrap();
    let saved = landing::load_goals(&h.app.state_root);
    assert_eq!(saved.len(), 4);
    assert!(saved.iter().find(|g| g.id == "base").unwrap().done);
}

#[test]
fn corrupt_goals_are_never_silently_overwritten_by_a_new_goal() {
    let h = Harness::new(80, 24);
    let path = h.app.state_root.join("goals.json");
    std::fs::write(&path, b"{damaged original goals").unwrap();
    let original = std::fs::read(&path).unwrap();
    let goal = landing::Goal {
        id: "new".into(),
        text: "new".into(),
        done: false,
        added_at: "0".into(),
        closed_at: None,
        ..Default::default()
    };
    assert!(landing::save_goals(&h.app.state_root, &[goal]).is_err());
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
fn clearing_the_primary_buffer_does_not_make_the_other_draft_unreachable() {
    let mut h = Harness::new(80, 24);
    h.type_str("primary");
    h.key_down();
    h.type_str("secondary");
    h.key_up();
    h.key(KeyCode::Char('a'), KeyModifiers::CONTROL);
    h.key(KeyCode::Backspace, KeyModifiers::NONE);
    h.key_down();
    assert_eq!(h.app.editor.text(), "secondary");
    h.key_up();
    assert_eq!(h.app.editor.text(), "");
}

#[test]
fn many_draft_switches_keep_both_buffers_after_every_recovery() {
    let mut h = Harness::new(80, 24);
    h.app.restore_state();
    for i in 0..100 {
        let primary = format!("primary {i} \u{1f600}");
        let secondary = format!("secondary {i} \u{65e5}");
        h.app.editor.set_text(&primary);
        h.key_down();
        h.app.editor.set_text(&secondary);
        h.app.persist_draft_now();
        let saved = h.app.keep.lease.as_ref().unwrap().data().unwrap();
        assert_eq!(saved.draft, secondary);
        assert_eq!(saved.alternate_draft.as_deref(), Some(primary.as_str()));
        drop(h.app.keep.lease.take());
        h.app = App::new(h._dir.path().to_path_buf(), h._dir.path().to_path_buf());
        h.app.restore_state();
        h.draw();
        assert_eq!(h.app.editor.text(), secondary);
        h.key_down();
        assert_eq!(h.app.editor.text(), primary);
        h.key_up();
        assert_eq!(h.app.editor.text(), secondary);
    }
}

#[test]
fn terminal_escape_filter_preserves_text_style_boundaries_and_is_idempotent() {
    for escape in [
        "\u{1b}[2J",
        "\u{1b}]52;c;payload\u{7}",
        "\u{1b}]0;title\u{1b}\\",
        "\u{1b}Ppayload\u{1b}\\",
        "\u{9b}31m",
        "\u{9d}payload\u{9c}",
        "\u{1b}(B",
    ] {
        let input = format!("before{escape}\t\u{65e5}\u{1f600}\r\nafter");
        let (clean, offsets) = text::terminal_text(&input);
        assert_eq!(clean, "before    \u{65e5}\u{1f600}\nafter");
        assert_eq!(offsets.len(), input.chars().count() + 1);
        assert!(offsets.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(offsets.last().copied(), Some(clean.chars().count()));
        assert_eq!(text::terminal_text(&clean).0, clean);
    }
}

#[test]
fn extreme_usage_updates_finish_and_render_without_counter_or_formatter_overflow() {
    let mut h = Harness::new(100, 24);
    let usage = crate::provider::stream_cards::TurnUsage {
        input: Some(u64::MAX),
        output: Some(u64::MAX),
        cached: Some(u64::MAX),
        cost_usd: None,
    };
    for _ in 0..2 {
        h.app.apply_progress(TurnProgress::Usage(usage.clone()));
    }
    assert_eq!(h.app.turn_usage.input, Some(u64::MAX));
    h.app.session_input = u64::MAX;
    h.app.session_output = u64::MAX;
    h.app.finalize_turn(TurnOutcome::default());
    assert_eq!(h.app.session_input, u64::MAX);
    assert_eq!(h.app.session_output, u64::MAX);
    h.draw();
    assert_eq!(token_line::compact(u64::MAX), "1.84e19");
}

#[test]
fn goal_mouse_hit_testing_follows_wrapped_and_scrolled_visual_rows() {
    let mut h = Harness::new(100, 20);
    let goals: Vec<_> = (0..40)
        .map(|i| landing::Goal {
            id: format!("id-{i}"),
            text: if i == 0 {
                "wrapped goal ".repeat(9)
            } else {
                format!("unique-goal-{i:02}")
            },
            done: false,
            added_at: "0".into(),
            closed_at: None,
            ..Default::default()
        })
        .collect();
    landing::save_goals(&h.app.state_root, &goals).unwrap();
    open_landing(&mut h);
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    for target in ["unique-goal-01", "unique-goal-37"] {
        if target.ends_with("37") {
            for _ in 0..36 {
                h.key_down();
            }
        }
        let screen = h.screen();
        let (x, y) = screen
            .lines()
            .enumerate()
            .find_map(|(y, line)| {
                line.find(target)
                    .map(|at| (line[..at].chars().count() as u16, y as u16))
            })
            .unwrap_or_else(|| panic!("missing {target}: {screen}"));
        h.click(x, y);
        h.enter();
        h.enter();
        let saved = landing::load_goals(&h.app.state_root);
        assert!(saved.iter().find(|g| g.text == target).unwrap().done);
        assert!(!saved[0].done);
    }
}

#[test]
fn deterministic_editor_sequences_match_an_independent_text_model() {
    use crate::tui::editor::Editor;
    for seed in [1u64, 17, 0x12345678, 0xdeadbeef] {
        let mut state = seed;
        let mut editor = Editor::default();
        let mut model = Vec::<char>::new();
        let mut cursor = 0;
        for step in 0..10_000 {
            editor.move_to(cursor, false); // Each modeled edit is its own undo boundary.
            let before = model.clone();
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let n = (state >> 32) as usize;
            match n % 6 {
                0 | 1 => {
                    let c = ['a', ' ', '\n', '\u{65e5}', '\u{1f600}', '\u{301}'][(n / 13) % 6];
                    editor.insert_char(c);
                    model.insert(cursor, c);
                    cursor += 1;
                }
                2 => {
                    editor.backspace();
                    if cursor > 0 {
                        cursor -= 1;
                        model.remove(cursor);
                    }
                }
                3 => {
                    editor.delete();
                    if cursor < model.len() {
                        model.remove(cursor);
                    }
                }
                4 => {
                    cursor = n % (model.len() + 1);
                    editor.move_to(cursor, false);
                }
                _ => {
                    let start = n % (model.len() + 1);
                    let end = (n / 7) % (model.len() + 1);
                    editor.select_range(start, end);
                    editor.insert_str("x");
                    let (a, b) = (start.min(end), start.max(end));
                    model.splice(a..b, ['x']);
                    cursor = a + 1;
                }
            }
            assert_eq!(editor.chars(), model, "seed {seed}, step {step}");
            assert_eq!(editor.cursor(), cursor, "seed {seed}, step {step}");
            if before != model {
                assert!(editor.undo());
                assert_eq!(editor.chars(), before, "undo seed {seed}, step {step}");
                assert!(editor.redo());
                assert_eq!(editor.chars(), model, "redo seed {seed}, step {step}");
                assert_eq!(editor.cursor(), cursor);
            }
            for width in [1, 2, 7, 40, 120] {
                let rows = editor.layout(width);
                let (row, _) = editor.cursor_pos(width);
                assert!(row < rows.len());
                assert!(rows[row].start <= cursor && cursor <= rows[row].end);
            }
        }
    }
}

#[test]
fn goal_additions_from_concurrent_panes_all_survive() {
    let dir = tempfile::tempdir().unwrap();
    std::thread::scope(|scope| {
        for writer in 0..8 {
            let root = dir.path();
            scope.spawn(move || {
                for i in 0..25 {
                    let goal = landing::Goal {
                        id: format!("{writer}-{i}"),
                        text: format!("goal {writer}/{i}"),
                        done: false,
                        added_at: "0".into(),
                        closed_at: None,
                        ..Default::default()
                    };
                    // The production lock deliberately times out after two seconds.
                    // Under a tight stress loop another writer can consume that
                    // budget; retry only a known lock timeout, never uncertain I/O.
                    let deadline = Instant::now() + Duration::from_secs(15);
                    loop {
                        match landing::save_goals(root, std::slice::from_ref(&goal)) {
                            Ok(()) => break,
                            Err(error)
                                if error.kind() == std::io::ErrorKind::WouldBlock
                                    && Instant::now() < deadline =>
                            {
                                std::thread::sleep(Duration::from_millis(10));
                            }
                            Err(error) => panic!("goal save failed: {error}"),
                        }
                    }
                }
            });
        }
    });
    assert_eq!(landing::load_goals(dir.path()).len(), 200);
}

#[test]
fn unrelated_projects_with_the_same_folder_name_keep_separate_chat_groups() {
    let session = |cwd: &str, id: &str| crate::sessions::SessionSummary {
        account: "fixture".into(),
        provider: ProviderKind::Codex,
        session_id: id.into(),
        cwd: Some(cwd.into()),
        preview: Some(id.into()),
        modified_ms: 1,
    };
    let groups = landing::group_sessions(
        vec![
            session("C:/one/project", "one"),
            session("C:/two/project", "two"),
        ],
        Path::new("C:/one/project"),
    );
    assert_eq!(groups.len(), 2);
    assert!(groups[0].expanded);
    assert_eq!(groups[0].sessions[0].session_id, "one");
    assert!(!groups[1].expanded);
    assert_ne!(groups[0].folder, groups[1].folder);
}

#[test]
fn goal_entry_navigation_cannot_change_the_prompt_underneath() {
    let mut h = Harness::new(100, 24);
    h.app.editor.set_text("underlying draft");
    open_landing(&mut h);
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    h.type_str("a");
    h.type_str("unsent goal");
    let cursor = h.app.editor.cursor();
    for (key, modifiers) in [
        (KeyCode::Left, KeyModifiers::NONE),
        (KeyCode::Up, KeyModifiers::NONE),
        (KeyCode::Down, KeyModifiers::NONE),
        (KeyCode::Char('z'), KeyModifiers::CONTROL),
        (KeyCode::Delete, KeyModifiers::NONE),
    ] {
        h.key(key, modifiers);
        assert_eq!(h.app.editor.text(), "underlying draft");
        assert_eq!(h.app.editor.cursor(), cursor);
    }
    h.enter();
    assert_eq!(
        landing::load_goals(&h.app.state_root)[0].text,
        "unsent goal"
    );
}

#[test]
fn goal_entry_keeps_clipboard_paste_and_explicit_quit_shortcuts_available() {
    let mut h = Harness::new(100, 24);
    open_landing(&mut h);
    h.key(KeyCode::Tab, KeyModifiers::NONE);
    h.type_str("a");
    clipboard::set_text("clipboard goal").unwrap();
    h.key(KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert!(h.screen().contains("clipboard goal"), "{}", h.screen());
    h.key(
        KeyCode::Char('\u{20ac}'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert_eq!(h.app.editor.text(), "");
    h.key(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(h.app.quit_armed.is_some());
    h.enter();
    assert_eq!(
        landing::load_goals(&h.app.state_root)[0].text,
        "clipboard goal\u{20ac}"
    );
}
