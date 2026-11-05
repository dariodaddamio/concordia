//! Shared exit affordances and input ownership for command screens.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Picker,
    Activity,
    WorkerInput,
    Raster,
}

pub(super) fn draw_close(f: &mut Frame, popup: Rect, app: &mut App, target: Target) {
    if let Some(rect) = draw_close_button(f, popup) {
        app.areas.modal_close = Some((rect, target));
    }
}

pub(super) fn draw_close_button(f: &mut Frame, popup: Rect) -> Option<Rect> {
    if popup.width < 8 || popup.height == 0 {
        return None;
    }
    let rect = Rect::new(popup.right() - 4, popup.y, 3, 1);
    f.render_widget(
        Paragraph::new(" X ").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        rect,
    );
    Some(rect)
}

pub(super) fn mouse(app: &mut App, event: &MouseEvent) -> bool {
    let hit = app
        .areas
        .modal_close
        .filter(|(rect, _)| contains(*rect, event.column, event.row))
        .map(|(_, target)| target);
    if event.kind == MouseEventKind::Down(MouseButton::Left) && hit.is_some() {
        app.modal_close_pressed = hit;
        return true;
    }
    if let Some(pressed) = app.modal_close_pressed {
        if event.kind == MouseEventKind::Up(MouseButton::Left) {
            app.modal_close_pressed = None;
            if hit == Some(pressed) {
                match pressed {
                    Target::WorkerInput => app.deleg.input = None,
                    Target::Raster => app.raster.close(),
                    Target::Picker => app.dismiss_picker(),
                    Target::Activity => {
                        app.activity.detail = None;
                        app.activity.detail_text = None;
                        app.activity.query.clear();
                    }
                }
            }
            return true;
        }
        if matches!(event.kind, MouseEventKind::Drag(_)) {
            return true;
        }
    }
    false
}

pub(super) fn draw_input(f: &mut Frame, area: Rect, app: &mut App) -> bool {
    if app
        .picker
        .as_ref()
        .is_some_and(|p| p.kind == PickerKind::Trust)
    {
        return false;
    }
    let hint = if let Some(picker) = &app.picker {
        if picker.kind == PickerKind::Update
            && app.job.as_ref().is_some_and(|j| j.kind == JobKind::Update)
        {
            "Esc or X cancels the update. Chat input is paused."
        } else if picker.kind == PickerKind::Usage {
            "Esc or X closes usage. r refreshes. Chat input is paused."
        } else if matches!(
            picker.kind,
            PickerKind::Update
                | PickerKind::Tokens
                | PickerKind::Related
                | PickerKind::CreditDetails
        ) {
            "Esc or X closes this screen. Chat input is paused."
        } else {
            "Esc or X closes this list. Typing filters the list, not your chat."
        }
    } else if app.activity.detail.is_some() {
        "Esc or X closes output. Typing searches output, not your chat."
    } else {
        return false;
    };
    app.areas.bottom_button = Rect::default();
    app.areas.input = Rect::default();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(" message · paused ")
        .padding(ratatui::widgets::Padding::new(2, 1, 0, 0))
        .border_style(Style::default().fg(Color::DarkGray));
    let width = block.inner(area).width as usize;
    let hint = if width < 46 {
        "Esc / X closes · chat paused"
    } else {
        hint
    };
    f.render_widget(
        Paragraph::new(hint)
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .style(Style::default().fg(Color::Cyan)),
        area,
    );
    true
}
