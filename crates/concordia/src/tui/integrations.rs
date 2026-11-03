//! A bounded, read-only integration catalog owned by the current conversation surface.
use super::{App, PickerKind};
use crate::update::{PreviewLine, UpdatePreview};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

#[derive(Default)]
pub(super) struct State {
    job: Option<Job>,
}
struct Job {
    conversation: String,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<Vec<PreviewLine>, String>>,
}
pub(super) fn open(app: &mut App) {
    if let Some(job) = &app.integrations.job {
        job.cancel.store(true, Ordering::Relaxed);
        app.flash("Integration inspection is finishing; reopen shortly");
        return;
    }
    let root = app.state_root.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let job_cancel = cancel.clone();
    let (tx, rx) = mpsc::channel();
    if std::thread::Builder::new()
        .name("concordia-integrations".into())
        .spawn(move || {
            let result = crate::harness::catalog::inspect(&root, &job_cancel)
                .map(|catalog| crate::harness::catalog::rows(&catalog));
            let _ = tx.send(result);
        })
        .is_err()
    {
        app.flash("Integration inspector could not start");
        return;
    }
    app.integrations.job = Some(Job {
        conversation: app.deleg.d.conversation_id().into(),
        cancel,
        rx,
    });
    app.update_preview = Some(UpdatePreview {
        history: Vec::new(),
        history_expanded: false,
        lines: vec![PreviewLine::Title(
            "Inspecting integration metadata…".into(),
        )],
        install_available: false,
        release_url: None,
    });
    app.show_picker(
        PickerKind::Integrations,
        "integrations".into(),
        Vec::new(),
        false,
    );
    app.footer_left = "R refresh · Esc close".into();
}
pub(super) fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if app
        .picker
        .as_ref()
        .is_some_and(|picker| picker.kind == PickerKind::Integrations)
        && !key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('r' | 'R'))
    {
        open(app);
        true
    } else {
        false
    }
}
pub(super) fn poll(app: &mut App) {
    let Some(job) = app.integrations.job.as_ref() else {
        return;
    };
    let valid = job.conversation == app.deleg.d.conversation_id()
        && app
            .picker
            .as_ref()
            .is_some_and(|picker| picker.kind == PickerKind::Integrations);
    if !valid {
        job.cancel.store(true, Ordering::Relaxed);
    }
    let result = match job.rx.try_recv() {
        Ok(result) => result,
        Err(mpsc::TryRecvError::Empty) => return,
        Err(_) => Err("Integration inspection stopped".into()),
    };
    app.integrations.job = None;
    if valid {
        if let Some(preview) = app.update_preview.as_mut() {
            preview.lines = result.unwrap_or_else(|error| {
                vec![
                    PreviewLine::Title("Integration catalog unavailable".into()),
                    PreviewLine::Text(error),
                    PreviewLine::Hint("R retry · Esc close".into()),
                ]
            });
        }
    }
}
