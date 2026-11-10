//! Small files the prompt box keeps between runs: the unsent draft, the sent-prompt history and
//! the prompt queue. All of it lives under the state directory and never leaves the machine.
//! Write failures stay visible and dirty until a bounded retry or explicit retry succeeds.
//!
//! Recovery records have one OS-locked owner and atomic writes (temp file + rename).
//! Sent history is shared; older shared draft/queue files are read only for explicit import.

#[cfg(test)]
use serde::{Deserialize, Serialize};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
#[cfg(test)]
use std::io::Write;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::{Arc, Weak};

/// Sent prompts kept on disk (newest last).
pub const HISTORY_CAP: usize = 500;
pub(super) const HISTORY_TEXT_BYTES: usize = 16 * 1024 * 1024;

pub fn draft_path(root: &Path) -> PathBuf {
    root.join("draft.json")
}

pub fn history_path(root: &Path) -> PathBuf {
    root.join("history.jsonl")
}

pub fn queue_path(root: &Path) -> PathBuf {
    root.join("queue.json")
}

/// Write `bytes` to `path` through a temp file in the same directory, then rename over it.
#[cfg(test)]
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let safe = crate::redact::redact_bytes(bytes);
    crate::state_io::write_atomic(path, &safe)
}

/// Move an unreadable file aside as `<name>.bad` (replacing an older one) so it is kept for
/// inspection but no longer in the way.
#[cfg(test)]
fn quarantine(path: &Path) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let _ = fs::rename(path, path.with_file_name(format!("{name}.bad")));
}

#[cfg(test)]
fn remove(path: &Path) {
    let _ = fs::remove_file(path);
}

// ---- draft -----------------------------------------------------------------------------

#[cfg(test)]
#[derive(Serialize, Deserialize)]
struct DraftFile {
    text: String,
}

/// Save the unsent draft; an empty one deletes the file.
#[cfg(test)]
pub fn save_draft(root: &Path, text: &str) {
    let path = draft_path(root);
    if text.trim().is_empty() {
        remove(&path);
        return;
    }
    if let Ok(json) = serde_json::to_vec(&DraftFile { text: text.into() }) {
        let _ = write_atomic(&path, &json);
    }
}

#[cfg(test)]
pub fn load_draft(root: &Path) -> Option<String> {
    let path = draft_path(root);
    let raw = fs::read(&path).ok()?;
    match serde_json::from_slice::<DraftFile>(&raw) {
        Ok(d) if !d.text.trim().is_empty() => Some(d.text),
        Ok(_) => None,
        Err(_) => {
            quarantine(&path);
            None
        }
    }
}

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

// ---- history ---------------------------------------------------------------------------

/// Parse a history file: valid lines (one JSON string each) and whether any line was bad.
#[cfg(test)]
fn parse_history(raw: &str) -> (Vec<String>, bool) {
    let mut bad = false;
    let mut out = Vec::new();
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<String>(line) {
            Ok(s) if !s.trim().is_empty() => out.push(s),
            _ => bad = true,
        }
    }
    (out, bad)
}

#[cfg(test)]
fn history_text(lines: &[String]) -> String {
    let mut s = String::new();
    for l in lines {
        s.push_str(&serde_json::to_string(l).unwrap_or_default());
        s.push('\n');
    }
    s
}

#[derive(Default)]
pub(super) struct HistoryLoad {
    pub messages: Vec<String>,
    pub unavailable: usize,
}

/// Bounded, read-only hydration. Call on a worker: migrated rows read native sources.
pub(super) fn load_history_status(root: &Path) -> HistoryLoad {
    const TAIL: u64 = 16 * 1024 * 1024;
    const LINE: usize = 2 * 1024 * 1024;
    let mut result = HistoryLoad::default();
    let path = history_path(root);
    if !path.exists() {
        return result;
    }
    let Some(directory) = crate::update::resolved_directory(root) else {
        return result;
    };
    if !crate::update::regular_owned_file(&path, &directory) {
        result.unavailable = 1;
        return result;
    }
    let read = (|| -> io::Result<(Vec<u8>, bool)> {
        let mut file = fs::File::open(&path)?;
        let length = file.metadata()?.len();
        let start = length.saturating_sub(TAIL);
        let skip_partial = if start > 0 {
            file.seek(SeekFrom::Start(start - 1))?;
            let mut previous = [0];
            file.read_exact(&mut previous)?;
            previous[0] != b'\n'
        } else {
            false
        };
        file.seek(SeekFrom::Start(start))?;
        let mut raw = Vec::new();
        file.take(TAIL).read_to_end(&mut raw)?;
        if skip_partial {
            let skip = raw
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(raw.len(), |i| i + 1);
            raw.drain(..skip);
        }
        Ok((raw, start > 0))
    })();
    let Ok((raw, truncated)) = read else {
        result.unavailable = 1;
        return result;
    };
    result.unavailable += usize::from(truncated);
    let mut text_bytes = 0usize;
    for bytes in raw.split(|byte| *byte == b'\n').rev() {
        if bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let message = (bytes.len() <= LINE)
            .then(|| std::str::from_utf8(bytes).ok())
            .flatten()
            .and_then(|line| crate::legacy_migration::hydrate_history_line(root, line))
            .filter(|message| !message.trim().is_empty());
        match message {
            Some(message) => {
                if text_bytes.saturating_add(message.len()) > HISTORY_TEXT_BYTES {
                    result.unavailable += 1;
                    break;
                }
                text_bytes += message.len();
                result.messages.push(message);
            }
            None => result.unavailable += 1,
        }
        if result.messages.len() >= HISTORY_CAP {
            break;
        }
    }
    result.messages.reverse();
    result
}

/// The last [`HISTORY_CAP`] valid sent prompts, oldest first. Existing bytes are never rewritten.
#[cfg(test)]
pub fn load_history(root: &Path) -> (Vec<String>, usize) {
    let lines = load_history_status(root).messages;
    let n = lines.len();
    (lines, n)
}

/// Append one sent prompt. `on_disk` is the running line count; when it passes the cap the file
/// is compacted (re-read, so other panes' lines are kept) to the newest [`HISTORY_CAP`].
#[cfg(test)]
pub fn append_history(root: &Path, line: &str, on_disk: &mut usize) {
    let path = history_path(root);
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let Ok(mut entry) = serde_json::to_string(line) else {
        return;
    };
    entry.push('\n');
    let safe_entry = crate::redact::redact(&entry);
    let appended = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(safe_entry.as_bytes()));
    if appended.is_err() {
        return;
    }
    *on_disk += 1;
    if *on_disk > HISTORY_CAP {
        let raw = fs::read(&path).unwrap_or_default();
        let (mut lines, _) = parse_history(&String::from_utf8_lossy(&raw));
        if lines.len() > HISTORY_CAP {
            lines.drain(..lines.len() - HISTORY_CAP);
        }
        if write_atomic(&path, history_text(&lines).as_bytes()).is_ok() {
            *on_disk = lines.len();
        }
    }
}

// ---- queue -----------------------------------------------------------------------------

#[cfg(test)]
#[derive(Serialize, Deserialize)]
struct QueueFile {
    items: Vec<String>,
}

/// Save the queued prompts; an empty queue deletes the file.
#[cfg(test)]
pub fn save_queue(root: &Path, items: &[String]) {
    let path = queue_path(root);
    if items.is_empty() {
        remove(&path);
        return;
    }
    if let Ok(json) = serde_json::to_vec(&QueueFile {
        items: items.to_vec(),
    }) {
        let _ = write_atomic(&path, &json);
    }
}

#[cfg(test)]
pub fn load_queue(root: &Path) -> Vec<String> {
    let path = queue_path(root);
    let Ok(raw) = fs::read(&path) else {
        return Vec::new();
    };
    match serde_json::from_slice::<QueueFile>(&raw) {
        Ok(q) => q
            .items
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect(),
        Err(_) => {
            quarantine(&path);
            Vec::new()
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
    /// Lines currently in history.jsonl (to know when to compact).
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
                    }
                }
                Err(error) => {
                    self.keep.warning = Some(format!("Recovery save unavailable: {error}"))
                }
            }
        }
        if super::recovery::legacy_available(&self.state_root) {
            self.push_system(
                "legacy shared draft or queue found; use /recover to import it safely",
            );
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

    /// Choices for the parent `/recover` picker. Selected records are claimed by explicit id.
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

    pub(super) fn import_legacy_recovery(&mut self) -> io::Result<()> {
        if self.deleg.d.any_active() || self.workspace.has_unsaved() {
            return Err(io::Error::other(
                "finish or cancel workers and save or close edited files before changing recovery ownership",
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
        let lease = super::recovery::create(root)?;
        super::recovery::import_legacy(&self.state_root, &lease)?;
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
        self.keep.lease = Some(lease);
        self.restore_composer(&data.draft);
        self.parked_draft = data.alternate_draft;
        self.queue.items = data.queue;
        self.restore_pending_edit(data.pending_edit);
        self.queue.paused = true;
        self.push_system(
            "imported legacy draft and queued prompts; original shared files were kept",
        );
        self.persist_draft_now();
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
    /// command, in history.jsonl.
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

    fn root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn draft_round_trips_and_empty_deletes_the_file() {
        let d = root();
        assert_eq!(load_draft(d.path()), None);
        save_draft(d.path(), "half a thought\nsecond line \u{2713}");
        assert_eq!(
            load_draft(d.path()).as_deref(),
            Some("half a thought\nsecond line \u{2713}")
        );
        save_draft(d.path(), "");
        assert!(!draft_path(d.path()).exists());
        save_draft(d.path(), "   \n");
        assert!(!draft_path(d.path()).exists());
        // no temp files left behind
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0);
    }

    #[test]
    fn corrupt_draft_is_moved_aside_once_and_ignored() {
        let d = root();
        fs::write(draft_path(d.path()), b"{ not json").unwrap();
        assert_eq!(load_draft(d.path()), None);
        assert!(!draft_path(d.path()).exists());
        assert!(d.path().join("draft.json.bad").exists());
        // a fresh draft works and the next load does not touch the .bad file
        save_draft(d.path(), "fresh");
        assert_eq!(load_draft(d.path()).as_deref(), Some("fresh"));
        assert_eq!(
            fs::read(d.path().join("draft.json.bad")).unwrap(),
            b"{ not json"
        );
    }

    #[test]
    fn history_appends_loads_and_keeps_multiline_prompts() {
        super::super::reference_journal::fixture_proof();
        let d = root();
        let mut n = 0;
        append_history(d.path(), "one", &mut n);
        append_history(d.path(), "two\nlines \"quoted\"", &mut n);
        assert_eq!(n, 2);
        let (h, lines) = load_history(d.path());
        assert_eq!(
            h,
            vec!["one".to_string(), "two\nlines \"quoted\"".to_string()]
        );
        assert_eq!(lines, 2);
        let mut journal = super::super::reference_journal::State::default();
        journal.load(d.path().to_path_buf());
        let mut recent = vec!["sent while recall was loading".to_owned()];
        journal.remember_added();
        journal.settle_for_test(&mut recent);
        assert_eq!(
            recent,
            [
                "one",
                "two\nlines \"quoted\"",
                "sent while recall was loading"
            ]
        );
        assert_eq!(
            load_history(d.path()).0,
            h,
            "background recall never rewrites existing history"
        );
    }

    #[test]
    fn history_is_compacted_past_the_cap_newest_last() {
        let d = root();
        let mut n = 0;
        for i in 0..HISTORY_CAP + 7 {
            append_history(d.path(), &format!("p{i}"), &mut n);
        }
        assert!(n <= HISTORY_CAP);
        let (h, _) = load_history(d.path());
        assert_eq!(h.len(), HISTORY_CAP);
        assert_eq!(h.first().unwrap(), "p7");
        assert_eq!(h.last().unwrap(), &format!("p{}", HISTORY_CAP + 6));
    }

    #[test]
    fn a_big_file_loads_only_the_newest_entries() {
        let d = root();
        let lines: Vec<String> = (0..HISTORY_CAP + 50).map(|i| format!("q{i}")).collect();
        fs::write(history_path(d.path()), history_text(&lines)).unwrap();
        let (h, _) = load_history(d.path());
        assert_eq!(h.len(), HISTORY_CAP);
        assert_eq!(h.first().unwrap(), "q50");
    }

    #[test]
    fn corrupt_history_lines_are_skipped_without_rewriting_legacy_history() {
        let d = root();
        fs::write(
            history_path(d.path()),
            "\"good\"\nnot json\n\"also good\"\n{\"x\":1}\n",
        )
        .unwrap();
        let original = fs::read(history_path(d.path())).unwrap();
        let (h, n) = load_history(d.path());
        assert_eq!(h, vec!["good".to_string(), "also good".to_string()]);
        assert_eq!(n, 2);
        assert_eq!(fs::read(history_path(d.path())).unwrap(), original);
        assert!(!d.path().join("history.jsonl.bad").exists());
        let (h2, _) = load_history(d.path());
        assert_eq!(h2, h);
    }

    #[test]
    fn fully_corrupt_history_is_ignored() {
        let d = root();
        fs::write(history_path(d.path()), b"\xff\xfe garbage\n").unwrap();
        let (h, n) = load_history(d.path());
        assert!(h.is_empty());
        assert_eq!(n, 0);
        assert_eq!(
            fs::read(history_path(d.path())).unwrap(),
            b"\xff\xfe garbage\n"
        );
    }

    #[test]
    fn queue_round_trips_and_empty_deletes_the_file() {
        let d = root();
        assert!(load_queue(d.path()).is_empty());
        let items = vec!["first".to_string(), "second\nline".to_string()];
        save_queue(d.path(), &items);
        assert_eq!(load_queue(d.path()), items);
        save_queue(d.path(), &[]);
        assert!(!queue_path(d.path()).exists());
    }

    #[test]
    fn corrupt_queue_is_ignored() {
        let d = root();
        fs::write(queue_path(d.path()), b"[1,2").unwrap();
        assert!(load_queue(d.path()).is_empty());
        assert!(d.path().join("queue.json.bad").exists());
    }

    #[test]
    fn concurrent_panes_last_writer_wins_and_no_partial_file() {
        let d = root();
        let root = d.path().to_path_buf();
        let texts: Vec<String> = (0..4)
            .map(|t| format!("pane {t} draft {}", "x".repeat(2000 * (t + 1))))
            .collect();
        let mut hs = Vec::new();
        for t in texts.clone() {
            let r = root.clone();
            hs.push(std::thread::spawn(move || {
                for _ in 0..40 {
                    save_draft(&r, &t);
                }
            }));
        }
        let r = root.clone();
        let reader = std::thread::spawn(move || {
            for _ in 0..200 {
                // never a parse failure: a partial file would be moved to .bad
                let _ = load_draft(&r);
            }
        });
        for h in hs {
            h.join().unwrap();
        }
        reader.join().unwrap();
        let got = load_draft(&root).expect("a draft survives");
        assert!(texts.contains(&got), "draft is exactly one pane's text");
        assert!(!root.join("draft.json.bad").exists());
        let leftovers = fs::read_dir(&root)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn concurrent_history_appends_keep_every_line_whole() {
        let d = root();
        let root = d.path().to_path_buf();
        let hs: Vec<_> = (0..4)
            .map(|t| {
                let r = root.clone();
                std::thread::spawn(move || {
                    let mut n = 0;
                    for i in 0..30 {
                        append_history(
                            &r,
                            &format!("pane {t} prompt {i} {}", "y".repeat(300)),
                            &mut n,
                        );
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        let (h, _) = load_history(&root);
        assert_eq!(h.len(), 120);
        assert!(!root.join("history.jsonl.bad").exists());
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
        assert!(screen.contains("/recover retry"));
        fs::remove_dir(&record).unwrap();
        fs::rename(&backup, &record).unwrap();
        assert!(app.handle_slash("/recover retry"));
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

    #[test]
    fn explicit_legacy_import_uses_fresh_record_and_preserves_current_record() {
        let d = root();
        let mut app = super::super::App::new(d.path().to_path_buf(), d.path().to_path_buf());
        app.restore_state();
        app.editor.set_text("current pane draft");
        app.persist_draft_now();
        let old_lease = app.keep.lease.as_ref().unwrap().clone();
        crate::tui::persist::save_draft(d.path(), "legacy draft");
        crate::tui::persist::save_queue(d.path(), &["legacy queued".into()]);

        app.import_legacy_recovery().unwrap();
        assert_eq!(old_lease.data().unwrap().draft, "current pane draft");
        assert_eq!(
            app.keep.lease.as_ref().unwrap().data().unwrap().draft,
            "legacy draft"
        );
        assert_eq!(app.queue.items, vec!["legacy queued"]);
    }

    #[test]
    fn golden_seeded_secrets_never_reach_disk_or_mcp() {
        let d = root();
        let secret = "sk-ant-abcdefghijklmnop";
        let token = "ghp_abcdefghijklmnop";
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nprivate\n-----END RSA PRIVATE KEY-----";
        let database = "postgres://user:passphrase@db.test/app";
        let body = format!("{secret} {token} {pem} {database}");

        save_draft(d.path(), &body);
        save_queue(d.path(), std::slice::from_ref(&body));
        let mut count = 0;
        append_history(d.path(), &body, &mut count);
        let log = crate::threads::ThreadLog::open(d.path(), "golden1111");
        log.append_unavailable(
            &crate::threads::ThreadEvent::with_text("assistant", "safe"),
            None,
            &body,
        )
        .unwrap();
        log.write_meta(&crate::threads::ThreadMeta {
            id: "golden1111".into(),
            started: 0,
            title: body.clone(),
            cwd: String::new(),
            account: None,
        })
        .unwrap();

        let mut cards = Vec::new();
        crate::provider::upsert_card(
            &mut cards,
            crate::provider::StreamCard {
                id: "golden".into(),
                title: body.clone(),
                detail: Some(body.clone()),
                status: crate::provider::CardStatus::Done,
            },
        );
        let recovery = d.path().join("editor-recovery.json");
        write_atomic(&recovery, body.as_bytes()).unwrap();

        for path in [
            draft_path(d.path()),
            queue_path(d.path()),
            history_path(d.path()),
            crate::threads::threads_dir(d.path()).join("golden1111.jsonl"),
            crate::threads::threads_dir(d.path()).join("golden1111.meta.json"),
            recovery,
        ] {
            let text = fs::read_to_string(&path).unwrap();
            assert!(!text.contains(secret));
            assert!(!text.contains(token));
            assert!(!text.contains("PRIVATE KEY"));
            assert!(!text.contains("passphrase"));
            if !path.ends_with("golden1111.jsonl") {
                assert!(
                    text.contains("[REDACTED:"),
                    "no token in {}: {text}",
                    path.display()
                );
            }
        }
        assert!(!cards[0].title.contains(secret));
        assert!(!cards[0].detail.as_deref().unwrap().contains(token));

        let legacy = crate::threads::ThreadEvent::with_text("assistant", &body);
        fs::write(
            crate::threads::threads_dir(d.path()).join("legacy1111.jsonl"),
            format!("{}\n", serde_json::to_string(&legacy).unwrap()),
        )
        .unwrap();
        let reply = crate::mcp::handle_line(
            d.path(),
            &serde_json::json!({
                "jsonrpc":"2.0", "id":1, "method":"tools/call",
                "params":{"name":"thread_read","arguments":{"id":"legacy"}}
            })
            .to_string(),
        )
        .unwrap();
        assert!(!reply.contains(secret));
        assert!(reply.contains("[REDACTED:"));
    }
}
