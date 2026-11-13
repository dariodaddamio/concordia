//! Small files the prompt box keeps between runs: the unsent draft, the sent-prompt history and
//! the prompt queue. All of it lives under the state directory and never leaves the machine.
//! Write failures stay visible and dirty until a bounded retry or explicit retry succeeds.
//!
//! Recovery records have one OS-locked owner and atomic writes (temp file + rename).
//! Sent history stays in memory; drafts and queues use pane-owned recovery records.

use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::{Arc, Weak};

pub const HISTORY_CAP: usize = 500;

// The panic hook cannot see the app, so the latest draft is mirrored here on every change.
type DraftSlot = Mutex<
    Option<(
        Weak<super::recovery::RecoveryLease>,
        String,
        Vec<String>,
        Option<super::recovery::RecoveryEdit>,
        Option<String>,
    )>,
>;
static PANIC_DRAFT: DraftSlot = Mutex::new(None);

fn mirror_snapshot(
    lease: Arc<super::recovery::RecoveryLease>,
    text: String,
    queue: Vec<String>,
    pending: Option<super::recovery::RecoveryEdit>,
    alternate: Option<String>,
) {
    if let Ok(mut g) = PANIC_DRAFT.lock() {
        *g = Some((Arc::downgrade(&lease), text, queue, pending, alternate));
    }
}

/// Panic-hook path: write the mirrored draft. Never blocks (a panic may hold the lock).
pub fn flush_on_panic() {
    flush_from(&PANIC_DRAFT);
}

fn flush_from(slot: &DraftSlot) {
    if let Ok(g) = slot.try_lock() {
        if let Some((lease, text, queue, pending, alternate)) = g.as_ref() {
            if let Some(lease) = lease.upgrade() {
                let _ =
                    lease.save_with_alternate(text, queue, pending.as_ref(), alternate.as_deref());
            }
        }
    }
}

// ---- wiring into the app ---------------------------------------------------------------

/// Ticks (200 ms each) without an edit before the draft is written: about one second.
const DRAFT_DEBOUNCE_TICKS: u64 = 5;
const RETRY_DELAY_TICKS: u64 = 25;
const MAX_RETRIES: u8 = 5;

/// Bookkeeping for what has been written, so the tick only writes when something changed.
pub(super) struct Keep {
    /// `[ui] persist_history`.
    pub history: bool,
    pub turn_recovery_notice: bool,
    pub restored_conversation: bool,
    /// Revision bookkeeping for in-memory sent history.
    rev_saved: u64,
    rev_seen: u64,
    changed_tick: u64,
    saved_edit_signature: u64,
    seen_edit_signature: u64,
    retry_at: u64,
    retries: u8,
    force_dirty: bool,
    warning: Option<String>,
    pub lease: Option<Arc<super::recovery::RecoveryLease>>,
    pub recovery_root: Option<PathBuf>,
    pub recovery_choices: Vec<super::recovery::RecoveryChoice>,
}

impl Keep {
    pub fn new(root: &Path) -> Keep {
        Keep {
            history: crate::config::for_root(root).ui.persist_history,
            turn_recovery_notice: false,
            restored_conversation: false,
            rev_saved: 0,
            rev_seen: 0,
            changed_tick: 0,
            saved_edit_signature: 0,
            seen_edit_signature: 0,
            retry_at: 0,
            retries: 0,
            force_dirty: true,
            warning: None,
            lease: None,
            recovery_root: None,
            recovery_choices: Vec::new(),
        }
    }
}

fn pending_edit(app: &super::App) -> Option<super::recovery::RecoveryEdit> {
    app.queue
        .edit
        .as_ref()
        .map(|edit| super::recovery::RecoveryEdit {
            index: edit.index,
            text: edit.editor.text(),
            attachments: Some(app.images.draft.clone()),
        })
}

fn edit_signature(app: &super::App) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let Some(edit) = &app.queue.edit {
        true.hash(&mut h);
        edit.editor.revision().hash(&mut h);
        for attachment in &app.images.draft {
            attachment.hash().hash(&mut h);
        }
        edit.index.hash(&mut h);
    } else {
        false.hash(&mut h);
    }
    h.finish()
}

impl super::App {
    fn alternate_draft(&self) -> Option<String> {
        self.parked_draft
            .as_ref()
            .filter(|s| !s.is_empty() && **s != self.draft_text())
            .cloned()
    }
    /// The draft as it should be saved: while browsing history, the text that was being typed.
    pub(super) fn draft_text(&self) -> String {
        if self.hist_pos.is_some() {
            self.hist_draft.clone()
        } else if self.fresh_draft
            && self.editor.text().is_empty()
            && self.composer_images().count() == 0
        {
            self.parked_draft.clone().unwrap_or_default()
        } else {
            self.composer_value()
        }
    }

    /// Restore the draft, history and queue left by the previous run (at launch, once).
    pub(super) fn restore_state(&mut self) {
        self.restore_state_for_record(None);
    }

    pub(super) fn restore_state_for_record(&mut self, record: Option<&str>) {
        if self.keep.history {
            self.reference_journal.load(self.state_root.clone());
        }
        let root = match super::recovery::recovery_root(&self.state_root, &self.cwd) {
            Ok(root) => Some(root),
            Err(error) => {
                self.keep.warning = Some(format!("Recovery save unavailable: {error}"));
                None
            }
        };
        if let Some(root) = root {
            self.keep.recovery_root = Some(root.clone());
            let restored = match record {
                Some(id) => super::recovery::claim(&root, id).and_then(|lease| {
                    let data = lease.data()?;
                    Ok((lease, Some(data), Vec::new()))
                }),
                None => super::recovery::startup_with_restore(&root),
            };
            match restored {
                Ok((lease, restored, choices)) => {
                    self.keep.lease = Some(Arc::new(lease));
                    self.keep.recovery_choices = choices;
                    if let Some(data) = restored {
                        self.restore_composer(&data.draft);
                        self.parked_draft = data.alternate_draft;
                        self.queue.items = data.queue;
                        self.restore_pending_edit(data.pending_edit);
                        self.queue.paused = true;
                        self.push_system("Your unsent draft and queued messages were restored.");
                    }
                }
                Err(error) => {
                    self.keep.warning = Some(format!("Recovery save unavailable: {error}"))
                }
            }
        }
        self.restore_turn_checkpoint();
        self.restore_workers();
        if let Some(lease) = self.keep.lease.clone() {
            if let Err(error) = self.workspace.set_file_recovery(lease, &self.cwd) {
                self.keep.warning = Some(format!("File recovery unavailable: {error}"));
            }
        }
        self.keep.rev_saved = self.editor.revision();
        self.keep.rev_seen = self.keep.rev_saved;
        self.persist_draft_now();
    }

    /// Called every tick: write the draft once it has been still for about a second.
    pub(super) fn persist_tick(&mut self) {
        let rev = self.editor.revision();
        let signature = edit_signature(self);
        let draft_changed = rev != self.keep.rev_seen;
        let edit_changed = signature != self.keep.seen_edit_signature;
        if draft_changed {
            self.keep.rev_seen = rev;
            self.keep.changed_tick = self.tick_count;
            self.keep.retries = 0;
        }
        if edit_changed {
            self.keep.seen_edit_signature = signature;
            self.keep.changed_tick = self.tick_count;
            self.keep.retries = 0;
        }
        if draft_changed || edit_changed {
            if let Some(lease) = self.keep.lease.clone() {
                mirror_snapshot(
                    lease,
                    self.draft_text(),
                    self.queue.items.clone(),
                    pending_edit(self),
                    self.alternate_draft(),
                );
            }
        }
        if self.keep.lease.is_none()
            && self.keep.retries < MAX_RETRIES
            && self.tick_count >= self.keep.retry_at
        {
            let root = self
                .keep
                .recovery_root
                .clone()
                .or_else(|| super::recovery::recovery_root(&self.state_root, &self.cwd).ok());
            if let Some(root) = root {
                self.keep.recovery_root = Some(root.clone());
                match super::recovery::create(&root) {
                    Ok(lease) => {
                        self.keep.recovery_choices =
                            super::recovery::available(&root).unwrap_or_default();
                        self.keep.lease = Some(Arc::new(lease));
                        self.keep.warning = None;
                        if let Some(lease) = self.keep.lease.clone() {
                            if let Err(error) = self.workspace.set_file_recovery(lease, &self.cwd) {
                                self.keep.warning =
                                    Some(format!("File recovery unavailable: {error}"));
                            }
                        }
                        self.keep.retries = 0;
                    }
                    Err(error) => self.retry_failed(error),
                }
            } else {
                self.retry_failed(io::Error::new(
                    io::ErrorKind::NotFound,
                    "recovery root unavailable",
                ));
            }
        }
        let dirty = self.keep.force_dirty
            || rev != self.keep.rev_saved
            || signature != self.keep.saved_edit_signature;
        if dirty
            && self.tick_count.wrapping_sub(self.keep.changed_tick) >= DRAFT_DEBOUNCE_TICKS
            && self.tick_count >= self.keep.retry_at
            && self.keep.retries < MAX_RETRIES
        {
            self.persist_draft_now();
        }
    }

    /// Write the draft immediately (send, exit).
    pub(super) fn persist_draft_now(&mut self) {
        if let Some(lease) = self.keep.lease.clone() {
            mirror_snapshot(
                lease,
                self.draft_text(),
                self.queue.items.clone(),
                pending_edit(self),
                self.alternate_draft(),
            );
        }
        if let Some(lease) = &self.keep.lease {
            match lease.save_with_alternate(
                &self.draft_text(),
                &self.queue.items,
                pending_edit(self).as_ref(),
                self.alternate_draft().as_deref(),
            ) {
                Ok(()) => {
                    self.keep.rev_saved = self.editor.revision();
                    self.keep.saved_edit_signature = edit_signature(self);
                    self.keep.force_dirty = false;
                    self.keep.warning = None;
                    self.keep.retries = 0;
                    self.keep.retry_at = 0;
                }
                Err(error) => self.retry_failed(error),
            }
        } else if self.keep.retries < MAX_RETRIES {
            self.retry_failed(io::Error::new(
                io::ErrorKind::NotFound,
                "recovery lease unavailable",
            ));
        }
    }

    pub(super) fn recovery_warning(&self) -> Option<&str> {
        self.keep.warning.as_deref()
    }

    pub(super) fn retry_recovery_now(&mut self) {
        self.keep.retries = 0;
        self.keep.retry_at = self.tick_count;
        self.persist_tick();
        if self.keep.lease.is_some() {
            self.persist_draft_now();
        }
    }

    fn retry_failed(&mut self, error: io::Error) {
        self.keep.warning = Some(format!(
            "Recovery save failed; unsaved changes are kept in memory: {error}"
        ));
        self.keep.force_dirty = true;
        self.keep.retries = self.keep.retries.saturating_add(1);
        self.keep.retry_at = self.tick_count.saturating_add(RETRY_DELAY_TICKS);
    }

    fn restore_pending_edit(&mut self, edit: Option<super::recovery::RecoveryEdit>) {
        let Some(edit) = edit.filter(|e| e.index < self.queue.items.len()) else {
            return;
        };
        let mut editor = super::editor::Editor::default();
        editor.set_text(&edit.text);
        editor.reset_undo();
        self.queue.sel = edit.index;
        self.begin_queue_edit(editor);
        if let Some(attachments) = edit.attachments {
            self.images.draft = attachments;
        }
        self.queue.focused = true;
        self.queue.paused = true;
        self.queue.dispatch.clear_automatic();
    }

    /// Saved records available for automatic restoration into tabs.
    pub(super) fn recovery_choices(&self) -> &[super::recovery::RecoveryChoice] {
        &self.keep.recovery_choices
    }

    pub(super) fn claim_recovery(&mut self, id: &str) -> io::Result<()> {
        if self.deleg.d.any_active() {
            return Err(io::Error::other(
                "finish or cancel workers before changing recovery ownership",
            ));
        }
        if self.workspace.has_unsaved() {
            return Err(io::Error::other(
                "save or close current file edits before switching recovery",
            ));
        }
        self.persist_draft_now();
        if let Some(warning) = self.keep.warning.as_deref() {
            return Err(io::Error::other(warning.to_owned()));
        }
        let root =
            self.keep.recovery_root.as_ref().ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "recovery root unavailable")
            })?;
        let lease = super::recovery::claim(root, id)?;
        let data = lease.data()?;
        let lease = Arc::new(lease);
        self.workspace
            .set_file_recovery(lease.clone(), &self.cwd)
            .map_err(io::Error::other)?;
        self.clear_queue_editor();
        self.hist_pos = None;
        self.hist_draft.clear();
        self.parked_draft = None;
        self.fresh_draft = false;
        self.keep.lease.take();
        self.restore_composer(&data.draft);
        self.parked_draft = data.alternate_draft;
        self.queue.items = data.queue;
        self.restore_pending_edit(data.pending_edit);
        self.queue.paused = true;
        self.keep.lease = Some(lease);
        self.keep.recovery_choices.clear();
        self.restore_turn_checkpoint();
        self.restore_workers();
        self.persist_draft_now();
        self.push_system("restored selected unsent draft and queued prompts");
        Ok(())
    }


    fn clear_queue_editor(&mut self) {
        self.close_queue_edit();
        self.queue.focused = false;
        self.queue.dispatch.clear_automatic();
        self.queue.sel = 0;
        self.queue.edit_scroll = 0;
        self.queue.edit_rect = None;
    }

    /// Remember a sent prompt: in memory (deduplicated, capped) and, unless it is a slash
    /// command, in memory until the current session ends.
    pub(super) fn push_history(&mut self, line: &str) {
        if self.history.last().map(String::as_str) == Some(line) {
            return;
        }
        self.history.push(line.to_string());
        self.reference_journal.remember_added();
        if self.history.len() > super::HISTORY_MAX {
            self.history.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn panic_mirror_flushes_the_latest_draft() {
        let d = root();
        let lease = Arc::new(crate::tui::recovery::create(d.path()).unwrap());
        // Use a local slot: other App tests update the process-global mirror concurrently.
        let slot: DraftSlot = Mutex::new(Some((
            Arc::downgrade(&lease),
            "typed before the crash".into(),
            vec!["latest queue".into()],
            Some(super::super::recovery::RecoveryEdit {
                index: 0,
                text: "pending upper edit".into(),
                attachments: None,
            }),
            Some("other unsent draft".into()),
        )));
        flush_from(&slot);
        assert_eq!(lease.data().unwrap().draft, "typed before the crash");
        assert_eq!(lease.data().unwrap().queue, ["latest queue"]);
        assert_eq!(
            lease.data().unwrap().alternate_draft.as_deref(),
            Some("other unsent draft")
        );
        assert_eq!(
            lease.data().unwrap().pending_edit.unwrap().text,
            "pending upper edit"
        );
        *slot.lock().unwrap() = Some((
            Arc::downgrade(&lease),
            String::new(),
            Vec::new(),
            None,
            None,
        ));
        flush_from(&slot);
        assert_eq!(lease.data().unwrap().draft, "");
    }

    #[test]
    fn app_restores_pending_queue_editor_separately_from_draft() {
        let d = root();
        let recovery_root = super::super::recovery::recovery_root(d.path(), d.path()).unwrap();
        let lease = super::super::recovery::create(&recovery_root).unwrap();
        lease
            .save_with_alternate(
                "main draft",
                &["first queued".into(), "committed queue value".into()],
                Some(&super::super::recovery::RecoveryEdit {
                    index: 1,
                    text: "unfinished edit".into(),
                    attachments: None,
                }),
                Some("alternate prompt"),
            )
            .unwrap();
        drop(lease);

        let mut app = super::super::App::new(d.path().to_path_buf(), d.path().to_path_buf());
        app.restore_state();
        assert_eq!(app.editor.text(), "main draft");
        assert_eq!(app.parked_draft.as_deref(), Some("alternate prompt"));
        assert_eq!(app.queue.items[1], "committed queue value");
        assert_eq!(
            app.queue.edit.as_ref().unwrap().editor.text(),
            "unfinished edit"
        );
        assert_eq!(app.queue.sel, 1);
        assert!(app.queue.focused);
        assert!(app.queue.paused);
        assert!(!app.queue.dispatch.is_automatic());
    }

    #[test]
    fn failed_app_save_stays_dirty_and_retries_after_repair() {
        let d = root();
        let mut app = super::super::App::new(d.path().to_path_buf(), d.path().to_path_buf());
        app.restore_state();
        let lease = app.keep.lease.as_ref().unwrap().clone();
        let record = super::super::recovery::recovery_root(d.path(), d.path())
            .unwrap()
            .join(lease.id());
        let record = record.join("recovery.json");
        let backup = record.with_extension("saved");
        let original = fs::read(&record).unwrap();
        fs::rename(&record, &backup).unwrap();
        fs::create_dir(&record).unwrap();

        app.editor.set_text("dirty revision survives");
        app.tick_count += 1;
        app.persist_tick();
        app.tick_count += DRAFT_DEBOUNCE_TICKS;
        app.persist_draft_now();
        assert!(app.recovery_warning().is_some());
        assert_eq!(fs::read(&backup).unwrap(), original);
        let previous = fs::read(&backup).unwrap();
        fs::remove_dir(&record).unwrap();
        fs::rename(&backup, &record).unwrap();
        app.tick_count += RETRY_DELAY_TICKS;
        app.persist_tick();
        assert_eq!(lease.data().unwrap().draft, "dirty revision survives");
        assert!(app.recovery_warning().is_none());
        assert_ne!(fs::read(record).unwrap(), previous);
    }

    #[test]
    fn failed_initial_recovery_acquisition_retries_without_restoring_over_typing() {
        let d = root();
        let missing_workspace = d.path().join("workspace-not-created-yet");
        let mut app = super::super::App::new(d.path().to_path_buf(), missing_workspace);
        app.restore_state();
        assert!(app.recovery_warning().is_some());
        app.editor.set_text("typed while recovery was unavailable");
        app.cwd = d.path().to_path_buf();
        app.tick_count = RETRY_DELAY_TICKS;
        app.persist_tick();
        app.tick_count += DRAFT_DEBOUNCE_TICKS;
        app.persist_tick();
        let data = app.keep.lease.as_ref().unwrap().data().unwrap();
        assert_eq!(data.draft, "typed while recovery was unavailable");
    }

    #[test]
    fn exhausted_save_retries_stay_visible_and_explicit_retry_preserves_the_draft() {
        let d = root();
        let mut app = super::super::App::new(d.path().into(), d.path().into());
        app.restore_state();
        let lease = app.keep.lease.as_ref().unwrap().clone();
        let record = super::super::recovery::recovery_root(d.path(), d.path())
            .unwrap()
            .join(lease.id())
            .join("recovery.json");
        let backup = record.with_extension("saved");
        fs::rename(&record, &backup).unwrap();
        fs::create_dir(&record).unwrap();
        app.editor
            .set_text("keep this draft through the storage error");
        app.persist_tick();
        app.persist_draft_now();
        for _ in 0..MAX_RETRIES + 2 {
            app.tick_count += RETRY_DELAY_TICKS;
            app.persist_tick();
        }
        assert_eq!(app.keep.retries, MAX_RETRIES);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|f| super::super::draw_ui(f, &mut app))
            .unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(screen.contains("draft not saved locally"));
        assert!(!screen.contains("/recover"));
        fs::remove_dir(&record).unwrap();
        fs::rename(&backup, &record).unwrap();
        app.persist_draft_now();
        assert!(app.recovery_warning().is_none());
        assert_eq!(
            lease.data().unwrap().draft,
            "keep this draft through the storage error"
        );
        assert_eq!(
            app.editor.text(),
            "keep this draft through the storage error"
        );
    }

}
