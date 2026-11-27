use super::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
    if app.questions.native_approval_waiting() {
        return "queue remains paused while a native approval is waiting".into();
    }

    app.queue.paused = false;
    app.queue.dispatch.set_automatic();
    if app.busy {
        "queue resumed - it will continue after the current turn".into()
    } else {
        "queue resumed".into()
    }
}
