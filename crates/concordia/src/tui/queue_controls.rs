//! Explicit controls for resuming a paused prompt queue.
//!
//! Resuming only sets the queue state. The existing event loop runs `queue_advance` after the
//! key/slash command, so queued prompts still use the normal submit path and only one starts.

use super::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Handle Ctrl+R. Returns true when the shortcut was consumed.
pub(super) fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if !ctrl || alt || !matches!(key.code, KeyCode::Char('r' | 'R')) {
        return false;
    }

    let message = resume_queue(app);
    app.flash(message);
    true
}

/// Resume from `/queue resume` and return a visible status message.
pub(super) fn resume(app: &mut App) -> Vec<String> {
    vec![resume_queue(app)]
}

fn resume_queue(app: &mut App) -> String {
    if app.queue.items.is_empty() {
        return "queue is empty - add a prompt with Tab before resuming".into();
    }
    if app.queue.edit.is_some() {
        return "save or cancel the queued edit before resuming (Ctrl+R)".into();
    }
    if app.cancel_requested {
        return "queue remains paused while cancellation finishes".into();
    }

    app.queue.paused = false;
    app.questions.clear();
    app.queue.advance = true;
    if app.busy {
        "queue resumed - it will continue after the current turn".into()
    } else {
        "queue resumed".into()
    }
}
