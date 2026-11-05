use super::{
    activity::{full_output, grouped_cards, Activity},
    detail_lines_parts, detail_scroll_target_parts, dispatch_key_burst, handle_key, App, ChatLine,
    Editor, MsgRole, Picker, PickerKind,
};
use crate::provider::stream_cards::{CardStatus, StreamCard};
use crate::provider::turn::{TurnOutcome, TurnProgress};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier};
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn expansion_and_detail_use_independent_message_identity() {
    #[cfg(windows)]
    super::native_window::status_fixture_proof();
    let mut a = Activity::default();
    a.toggle(2);
    assert!(a.expanded.contains(&2));
    assert!(!a.expanded.contains(&4));
    a.open(4);
    assert_eq!(a.detail, Some(4));
    let message = |id: &str, status: CardStatus, title: &str| ChatLine {
        role: MsgRole::Card,
        text: String::new(),
        card: Some(StreamCard {
            id: id.into(),
            title: title.into(),
            detail: Some("command: rg widgets\nexact output".into()),
            status,
        }),
        meta: None,
    };
    let messages = vec![
        message("read-one", CardStatus::Done, "shell_command"),
        message("read-two", CardStatus::Done, "read_file"),
        message("failed", CardStatus::Error, "shell_command"),
        message("live", CardStatus::Running, "shell_command"),
        message("approval", CardStatus::Done, "approval_request"),
        message("edit", CardStatus::Done, "apply_patch"),
    ];
    let groups = grouped_cards(&messages, &a, false);
    assert!(groups.text[0]
        .as_deref()
        .unwrap()
        .contains("Inspect project · 2 operations"));
    assert!(groups.text[0].as_deref().unwrap().starts_with("│ ✓ "));
    assert_eq!(groups.hidden, vec![false, true, false, false, false, false]);
    a.toggle_card(0, false);
    let groups = grouped_cards(&messages, &a, false);
    assert!(!groups.hidden.iter().any(|hidden| *hidden));
    assert!(groups.text[0].as_deref().unwrap().contains("exact output"));
    assert_eq!(messages[1].card.as_ref().unwrap().id, "read-two");
    a.toggle_card(0, true);
    assert!(grouped_cards(&messages, &a, true).hidden[1]);

    // Action/subject weight is one intentional span, independent of provider capitalization,
    // filename punctuation and UTF-8 byte widths. Status and disclosure stay separate.
    for (status, mark, color) in [
        (CardStatus::Running, '◉', Color::Cyan),
        (CardStatus::Done, '✓', Color::Green),
        (CardStatus::Error, '✗', Color::Red),
    ] {
        let card = StreamCard {
            id: "styled".into(),
            title: "apply_patch".into(),
            detail: None,
            status,
        };
        for label in ["Edit File", "Edit file", "Ran"] {
            let heading = format!("│ {mark} {label} src/café.rs - 2 output lines - Expand");
            let runs = super::activity::card_runs(&heading, Some(&card));
            let style_at = |index| {
                runs.iter()
                    .filter(|(_, start, end, _)| *start <= index && index < *end)
                    .fold(
                        ratatui::style::Style::default(),
                        |style, (_, _, _, next)| style.patch(*next),
                    )
            };
            let chars = heading.chars().collect::<Vec<_>>();
            let end = heading.split(" - ").next().unwrap().chars().count();
            for (index, character) in chars.iter().enumerate().take(end).skip(4) {
                assert!(
                    style_at(index).add_modifier.contains(Modifier::BOLD),
                    "unbold heading character {:?} in {heading}",
                    character
                );
            }
            assert_eq!(style_at(2).fg, Some(color));
            assert!(!style_at(2).add_modifier.contains(Modifier::BOLD));
            assert!(!style_at(chars.len() - 1)
                .add_modifier
                .contains(Modifier::BOLD));
        }
    }
}

#[test]
fn search_cycles_preserves_position_and_handles_unicode_case_expansion() {
    let mut a = Activity::default();
    a.query = "tool".into();
    let text = "tool began\nnone\ntool ended";
    a.search_parts(&[text]);
    assert_eq!(
        a.matches.iter().map(|m| m.line).collect::<Vec<_>>(),
        vec![0, 2]
    );
    a.next(false);
    assert_eq!(a.selected_match, 1);
    a.search_parts(&[text]); // draw-frame search is cached and must not reset navigation
    assert_eq!(a.selected_match, 1);
    a.next(false);
    assert_eq!(a.selected_match, 0);
    a.next(true);
    assert_eq!(a.selected_match, 1);
    a.invalidate_search(); // a streaming card update marks cached search results stale
    a.search_parts(&["tool began\ntool resumed\ntool ended"]);
    assert_eq!(a.matches.len(), 3);
    a.query = "i\u{307}".into();
    a.invalidate_search();
    a.search_parts(&["İ"]);
    assert_eq!(a.matches[0].col, 0);
    assert_eq!(a.matches[0].len, 1);
}

#[test]
fn query_editing_tracks_unicode_characters_not_utf8_bytes() {
    let mut a = Activity::default();
    a.insert_query("é😀x");
    assert_eq!(a.query_cursor, 3);
    a.query_backspace();
    assert_eq!(a.query, "é😀");
    a.query_cursor = 1;
    a.query_delete();
    assert_eq!(a.query, "é");
}

#[test]
fn long_query_title_is_clipped_around_the_query_cursor() {
    let mut a = Activity::default();
    a.query = "x".repeat(400);
    a.query_cursor = 200;
    let shown = a.query_with_cursor();
    assert!(shown.chars().count() <= 50);
    assert!(shown.contains('|'));
    assert!(shown.starts_with('…') && shown.ends_with('…'));
}

#[test]
fn selected_search_match_is_visibly_highlighted() {
    let mut a = Activity::default();
    a.query = "needle".into();
    a.search_parts(&["a needle b\na needle"]);
    let lines = detail_lines_parts(&["a needle b\na needle"], &a.matches, 1, 40);
    let selected: Vec<_> = lines[1]
        .spans
        .iter()
        .filter(|s| s.style.bg == Some(Color::Cyan))
        .collect();
    assert_eq!(
        selected
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>(),
        "needle"
    );
    assert!(selected[0].style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn search_in_a_long_wrapped_line_scrolls_the_match_into_view() {
    let body = "word ".repeat(60);
    let mut a = Activity::default();
    a.query = "word".into();
    a.search_parts(&[&body]);
    a.selected_match = 42;
    let found = a.selected().unwrap();
    let lines = detail_lines_parts(&[&body], &a.matches, a.selected_match, 20);
    let selected_row = lines
        .iter()
        .position(|line| line.spans.iter().any(|s| s.style.bg == Some(Color::Cyan)))
        .unwrap();
    let scroll = detail_scroll_target_parts(&[&body], found, 20, 8) as usize;
    assert!(scroll > 0);
    assert!(
        selected_row >= scroll && selected_row < scroll + 8,
        "selected match row {selected_row} should be visible at {scroll}..{}",
        scroll + 8
    );
}

#[test]
fn modal_consumes_prompt_undo_and_queue_shortcuts_and_closes_on_escape() {
    let mut a = Activity::default();
    let ctrl = KeyModifiers::CONTROL;
    assert_eq!(
        a.handle_key(KeyEvent::new(KeyCode::Char('z'), ctrl)),
        super::activity::ModalAction::Consumed
    );
    assert_eq!(
        a.handle_key(KeyEvent::new(KeyCode::Char('r'), ctrl)),
        super::activity::ModalAction::Consumed
    );
    assert_eq!(
        a.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        super::activity::ModalAction::Close
    );
}

fn card(id: &str, title: &str, detail: &str) -> ChatLine {
    let card = StreamCard {
        id: id.into(),
        title: title.into(),
        detail: Some(detail.into()),
        status: CardStatus::Done,
    };
    ChatLine {
        role: MsgRole::Card,
        text: title.into(),
        card: Some(card),
        meta: None,
    }
}

#[test]
fn activity_modal_handles_older_and_newer_cards_through_main_key_handler() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
    app.editor.set_text("preserve draft");
    app.queue.items.push("queued prompt".into());
    let mut pending = Editor::default();
    pending.set_text("pending queue edit");
    app.queue.edit = Some(pending);
    app.messages.push(card("one", "older", "one full result"));
    app.messages.push(card("two", "newer", "two full result"));
    app.activity.open(2);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    handle_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &tx);
    assert_eq!(app.activity.detail, Some(1));
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Down, KeyModifiers::ALT),
        &tx,
    );
    assert_eq!(app.activity.detail, Some(2));
    assert_eq!(app.editor.text(), "preserve draft");
    assert_eq!(
        app.queue.edit.as_ref().unwrap().text(),
        "pending queue edit"
    );
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        &tx,
    );
    assert_eq!(app.editor.text(), "preserve draft");
    assert_eq!(
        full_output("newer", Some("two full result")),
        "newer\ntwo full result"
    );
}

#[test]
fn paste_events_and_overflow_chunks_are_routed_to_activity_query_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
    app.editor.set_text("main draft");
    app.queue.items.push("queued prompt".into());
    app.queue.sel = 0;
    let mut edit = Editor::default();
    edit.set_text("pending queued edit");
    app.queue.edit = Some(edit);
    app.messages.push(card("one", "tool", "output"));
    app.activity.open(1);
    // Same centralized path used by Event::Paste and Windows' buffered overflow chunks.
    app.paste_text("first\r\n");
    dispatch_key_burst(
        &mut app,
        crate::tui::input::KeyBurst {
            keys: vec![],
            text: true,
            text_content: Some("large".repeat(300)),
            continues_paste: true,
        },
        &tokio::sync::mpsc::unbounded_channel().0,
    );
    dispatch_key_burst(
        &mut app,
        crate::tui::input::KeyBurst {
            keys: vec![],
            text: true,
            text_content: Some("\nlast".into()),
            continues_paste: true,
        },
        &tokio::sync::mpsc::unbounded_channel().0,
    );
    assert!(app.activity.query.starts_with("first\n"));
    assert!(!app.activity.query.contains('\r'));
    assert!(app.activity.query.ends_with("\nlast"));
    assert_eq!(app.editor.text(), "main draft");
    assert_eq!(
        app.queue.edit.as_ref().unwrap().text(),
        "pending queued edit"
    );
    assert_eq!(app.queue.items[0], "queued prompt");
}

#[test]
fn paste_into_picker_edits_its_filter_without_touching_draft_or_queue_edit() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
    app.editor.set_text("main draft");
    let mut edit = Editor::default();
    edit.set_text("pending edit");
    app.queue.edit = Some(edit);
    app.picker = Some(Picker {
        kind: PickerKind::Usage,
        title: "picker".into(),
        rows: vec![],
        filter: String::new(),
        state: Default::default(),
    });
    app.paste_text_chunk("filter\ntext", true);
    assert_eq!(app.picker.as_ref().unwrap().filter, "filter\ntext");
    assert_eq!(app.editor.text(), "main draft");
    assert_eq!(app.queue.edit.as_ref().unwrap().text(), "pending edit");
}

#[test]
fn selected_match_in_tall_output_is_present_in_the_drawn_detail_viewport() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
    let body = (0..80)
        .map(|i| {
            if i == 79 {
                format!("{} needle at the end", "界".repeat(80))
            } else {
                format!("line {i} ordinary output")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    app.messages.push(card("long", "tool output", &body));
    app.activity.open(1);
    app.activity.query = "needle".into();
    app.activity.query_cursor = 6;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| super::draw_ui(f, &mut app)).unwrap();
    let render_cache = app.activity.render_lines.as_ptr();
    terminal.draw(|f| super::draw_ui(f, &mut app)).unwrap();
    assert_eq!(
        app.activity.render_lines.as_ptr(),
        render_cache,
        "idle redraw should reuse wrapped detail rows"
    );
    let cells = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(
        cells.contains("needle at the end"),
        "the selected search result should be rendered in the visible popup"
    );
}

fn cards_after_finalizing_running_tool(
    mut outcome: TurnOutcome,
    with_completed: bool,
    streamed: bool,
) -> Vec<StreamCard> {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
    let running = StreamCard {
        id: "running-call".into(),
        title: "shell".into(),
        detail: Some("command: cargo test".into()),
        status: CardStatus::Running,
    };
    if streamed {
        app.apply_progress(TurnProgress::Card(running));
    } else {
        outcome.cards.push(running);
    }
    if with_completed {
        app.apply_progress(TurnProgress::Card(StreamCard {
            id: "completed-call".into(),
            title: "edit".into(),
            detail: Some("path: src/lib.rs\npatch: +safe".into()),
            status: CardStatus::Done,
        }));
    }
    app.finalize_turn(outcome);
    app.messages.iter().filter_map(|m| m.card.clone()).collect()
}

#[test]
fn incomplete_tool_cards_never_look_successful_at_turn_end() {
    let cards = cards_after_finalizing_running_tool(
        TurnOutcome {
            cancelled: true,
            exit_code: Some(1),
            stderr_tail: "terminated".into(),
            ..Default::default()
        },
        true,
        true,
    );
    let interrupted = cards.iter().find(|c| c.id == "running-call").unwrap();
    assert_eq!(interrupted.status, CardStatus::Error);
    let detail = interrupted.detail.as_deref().unwrap();
    assert!(detail.contains("command: cargo test"));
    assert!(detail.contains("cancelled before the provider reported tool completion"));
    let completed = cards.iter().find(|c| c.id == "completed-call").unwrap();
    assert_eq!(completed.status, CardStatus::Done);
    assert_eq!(
        completed.detail.as_deref(),
        Some("path: src/lib.rs\npatch: +safe")
    );

    let cards = cards_after_finalizing_running_tool(
        TurnOutcome {
            exit_code: Some(7),
            ..Default::default()
        },
        false,
        true,
    );
    let failed = &cards[0];
    assert_eq!(failed.status, CardStatus::Error);
    assert!(failed.detail.as_deref().unwrap().contains("exit code 7"));

    let cards = cards_after_finalizing_running_tool(
        TurnOutcome {
            exit_code: Some(0),
            ..Default::default()
        },
        false,
        true,
    );
    let incomplete = &cards[0];
    assert_eq!(incomplete.status, CardStatus::Error);
    assert!(incomplete
        .detail
        .as_deref()
        .unwrap()
        .contains("Incomplete: provider stream ended"));

    let cards = cards_after_finalizing_running_tool(
        TurnOutcome {
            exit_code: Some(0),
            ..Default::default()
        },
        false,
        false,
    );
    assert_eq!(cards[0].status, CardStatus::Error);
    assert!(cards[0]
        .detail
        .as_deref()
        .unwrap()
        .contains("Incomplete: provider stream ended"));
}
