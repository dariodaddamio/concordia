//! Keyboard shortcuts for moving between the prompt draft and the queued prompts.

use super::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Handle queue focus shortcuts before the queue's own focused-key handler.
///
/// Ctrl+Up never edits or clears the draft, so its text, cursor, selection and history state
/// remain owned by the prompt editor. Ctrl+Down returns to the draft unless an inline queue
/// edit is open; in that case the user must save or cancel it explicitly.
pub(super) fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    if !key.modifiers.contains(KeyModifiers::CONTROL) {
        return false;
    }

    match key.code {
        KeyCode::Up if !app.queue.items.is_empty() => {
            app.queue.sel = app.queue.sel.min(app.queue.items.len() - 1);
            app.queue.focused = true;
            app.flash("queue focused: Up/Down select; Enter edit; Ctrl+Down return to draft");
            true
        }
        KeyCode::Down if app.queue.focused => {
            if app.queue.edit.is_some() {
                app.flash("save or cancel the queued edit before returning to the draft");
            } else {
                app.queue.focused = false;
                app.flash("draft focused; Ctrl+Up returns to queue");
            }
            true
        }
        _ => false,
    }
}

/// Persistent footer hint while the queue has focus. Root and queue rendering can append this
/// to their existing navigation hint without duplicating the shortcut behavior.
pub(super) fn focus_hint(app: &App) -> Option<&'static str> {
    if !app.queue.focused {
        return None;
    }
    Some(if app.queue.edit.is_some() {
        "Ctrl+Down: save/cancel edit first"
    } else {
        "Ctrl+Down: draft; Ctrl+Up: queue"
    })
}
