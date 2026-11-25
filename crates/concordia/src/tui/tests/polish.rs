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
    assert!(h.app.picker_tones.contains_key("provider:codex"));
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
    assert!(
        popup.y == row.bottom() || popup.bottom() == row.y,
        "popup {popup:?} touches row {row:?}"
    );
    assert!(h.screen().contains("all providers"), "{}", h.screen());
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
