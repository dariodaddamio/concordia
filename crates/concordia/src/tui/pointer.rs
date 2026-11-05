//! OSC 22 mouse feedback. Hosts that do not support it ignore the request.
use super::*;
use std::io::Write;

#[derive(Default)]
pub(super) struct Pointer {
    shape: Option<&'static str>,
    position: Option<(u16, u16)>,
}

impl Pointer {
    pub fn refresh(&mut self, app: &App) -> io::Result<()> {
        if let Some((x, y)) = self.position {
            self.update(app, x, y)?;
        }
        Ok(())
    }

    pub fn update(&mut self, app: &App, x: u16, y: u16) -> io::Result<()> {
        self.position = Some((x, y));
        let shape = shape(app, x, y);
        if self.shape != Some(shape) {
            let mut out = stdout();
            write!(out, "\x1b]22;{shape}\x1b\\")?;
            out.flush()?;
            self.shape = Some(shape);
        }
        Ok(())
    }
}

pub(super) fn reset(out: &mut impl Write) -> io::Result<()> {
    out.write_all(b"\x1b]22;\x1b\\")?;
    out.flush()
}

pub(super) fn shape(app: &App, x: u16, y: u16) -> &'static str {
    if app.project_search.active() {
        return "default";
    }
    if contains(app.areas.run_cancel, x, y) {
        return "pointer";
    }
    if contains(app.areas.permissions, x, y)
        && app
            .picker
            .as_ref()
            .is_none_or(|picker| picker.kind == PickerKind::Trust)
        && app.activity.detail.is_none()
        && !workers::overlay_open(app)
    {
        return "pointer";
    }
    if app
        .areas
        .modal_close
        .is_some_and(|(rect, _)| contains(rect, x, y))
    {
        return "pointer";
    }
    if app
        .picker
        .as_ref()
        .is_some_and(|picker| picker.kind == PickerKind::Trust)
        && app
            .permission_rows
            .iter()
            .any(|(rect, _)| contains(*rect, x, y))
    {
        return "pointer";
    }
    if app
        .picker
        .as_ref()
        .is_some_and(|p| p.kind == PickerKind::Files)
        && app
            .related_files
            .targets
            .iter()
            .any(|(rect, _)| contains(*rect, x, y))
    {
        return "pointer";
    }
    if app.activity.detail.is_some()
        && app
            .activity
            .file_targets
            .iter()
            .any(|(rect, _)| contains(*rect, x, y))
    {
        return "pointer";
    }
    if app.picker.is_some() || app.activity.detail.is_some() || workers::overlay_open(app) {
        return "default";
    }
    if chatref::clickable(app, x, y) {
        return "pointer";
    }
    if images::clickable(app, x, y) || questions::clickable(app, x, y) {
        return "pointer";
    }
    if let Some(shape) = app.workspace.pointer_shape(x, y) {
        return shape;
    }
    if landing::visible(app) && landing::clickable(app, x, y) {
        return "pointer";
    }
    if app.workspace.panes.iter().any(|pane| {
        contains(pane.area, x, y) && y == pane.area.y && x == pane.area.right().saturating_sub(2)
    }) {
        return "pointer";
    }
    if app
        .areas
        .pinned_card
        .is_some_and(|(r, _)| contains(r, x, y))
    {
        return "pointer";
    }
    if contains(app.areas.bottom_button, x, y)
        || contains(app.areas.palette, x, y)
        || workers::clickable(app, x, y)
        || (app.queue.edit.is_none() && app.queue.rects.iter().any(|(r, _)| contains(*r, x, y)))
    {
        return "pointer";
    }
    let t = app.areas.transcript;
    if !landing::visible(app) && contains(t, x, y) {
        if super::activity::card_header_at(app, x, y).is_some() {
            return "pointer";
        }
        if x.saturating_sub(t.x) < 2
            && app
                .view
                .identity_at((y - t.y) as usize)
                .and_then(|i| app.messages.get(i))
                .is_some_and(|m| matches!(m.role, MsgRole::Card | MsgRole::Thinking))
        {
            return "pointer";
        }
        if let Some(pos) = app.view.pos_at((y - t.y) as isize, (x - t.x) as usize) {
            if app.view.rows.get(pos.row).is_some_and(|row| {
                row.links
                    .iter()
                    .any(|(a, b, _)| *a <= pos.col && pos.col < *b)
                    || row
                        .web_links
                        .iter()
                        .any(|(a, b, _)| *a <= pos.col && pos.col < *b)
            }) {
                return "pointer";
            }
        }
        return "text";
    }
    if contains(app.areas.input, x, y) {
        "text"
    } else {
        "default"
    }
}
