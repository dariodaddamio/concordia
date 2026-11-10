//! New completed-history records contain native references; drafts and recovery stay separate.
use super::*;
use crate::native_memory::{references as native, Binding};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::sync::mpsc as channel;

const OWNER: &str = "concordia-native-history-v1";
const FILE_BYTES: u64 = 4 * 1024 * 1024;
const EVENT_COUNT: usize = 255; // Reserve one of the 256 slots for an explicit gap.
const EVENT_BYTES: usize = 4 * 1024 * 1024 - 4096;
type Work = Box<dyn FnOnce() + Send + 'static>;

struct Completion {
    saved: bool,
    durable: bool,
    verified: Option<native::Reference>,
    generation: u64,
}

pub(super) struct Started {
    pub run: String,
    pub scope: Result<Option<native::CaptureScope>, String>,
    pub input: String,
    pub original: Option<std::ops::Range<usize>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryEntry {
    owner: String,
    version: u32,
    id: String,
    source: Option<native::Reference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    binding: Option<Binding>,
    attachments: Vec<crate::attachments::Attachment>,
    unavailable: bool,
    #[serde(default)]
    created: u64,
}

struct Pending {
    thread: String,
    event: crate::threads::ThreadEvent,
    call: Option<String>,
}

impl Pending {
    fn bytes(&self) -> usize {
        let event = &self.event;
        128 + self.thread.len()
            + self.call.as_ref().map_or(0, String::len)
            + event.k.len()
            + event.text.as_ref().map_or(0, String::len)
            + event.provider.as_ref().map_or(0, String::len)
            + event.account.as_ref().map_or(0, String::len)
            + event.model.as_ref().map_or(0, String::len)
            + event.card.as_ref().map_or(0, |card| {
                card.title.len()
                    + card.detail.as_ref().map_or(0, String::len)
                    + card.status.as_ref().map_or(0, String::len)
            })
    }
}

struct Active {
    run: String,
    generation: u64,
    provider: ProviderKind,
    account: String,
    home: Option<PathBuf>,
    original: crate::attachments::Message,
    history: bool,
    created: u64,
}

#[derive(Default)]
struct Loaded {
    legacy: Vec<String>,
    messages: Vec<String>,
    unavailable: usize,
}

pub(super) struct State {
    tx: channel::Sender<Started>,
    rx: channel::Receiver<Started>,
    active: Option<Active>,
    started: Option<Started>,
    events: Vec<Pending>,
    omitted: Option<Pending>,
    omission_notice: bool,
    latest_run: Option<String>,
    generation: u64,
    verified: Option<native::Reference>,
    writes: Vec<(String, channel::Receiver<Completion>)>,
    work: channel::SyncSender<Work>,
    deferred_close: Option<String>,
    durable: HashMap<String, bool>,
    load: Option<channel::Receiver<Loaded>>,
    added: usize,
}

impl Default for State {
    fn default() -> Self {
        let (tx, rx) = channel::channel();
        let (work, jobs) = channel::sync_channel::<Work>(16);
        let _ = std::thread::Builder::new()
            .name("concordia-reference-journal".into())
            .spawn(move || {
                for job in jobs {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                }
            });
        Self {
            tx,
            rx,
            active: None,
            started: None,
            events: Vec::new(),
            omitted: None,
            omission_notice: false,
            latest_run: None,
            generation: 0,
            verified: None,
            writes: Vec::new(),
            work,
            deferred_close: None,
            durable: HashMap::new(),
            load: None,
            added: 0,
        }
    }
}

impl State {
    pub fn sender(&self) -> channel::Sender<Started> {
        self.tx.clone()
    }

    pub fn begin(
        &mut self,
        owner: &crate::provider::interaction::InteractionOwner,
        original: &str,
        history: bool,
    ) {
        self.started = None;
        self.latest_run = Some(owner.run_id.clone());
        self.generation = self.generation.wrapping_add(1);
        self.verified = None;
        self.deferred_close = None;
        self.active = Some(Active {
            run: owner.run_id.clone(),
            generation: self.generation,
            provider: owner.provider,
            account: owner.account.clone(),
            home: owner.home.clone(),
            original: crate::attachments::Message::decode(original),
            history: history && !original.trim_start().starts_with('/'),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
                .unwrap_or(0),
        });
    }

    pub fn buffer(&mut self, thread: &str, event: crate::threads::ThreadEvent, call: Option<&str>) {
        if let Some(id) = call {
            if let Some(existing) = self
                .events
                .iter_mut()
                .rev()
                .take_while(|pending| pending.event.k != "user")
                .find(|pending| pending.thread == thread && pending.call.as_deref() == Some(id))
            {
                existing.event = event;
                self.bound_events();
                return;
            }
        }
        self.events.push(Pending {
            thread: thread.to_owned(),
            event,
            call: call.map(str::to_owned),
        });
        self.bound_events();
    }

    fn bound_events(&mut self) {
        let mut bytes = self.events.iter().map(Pending::bytes).sum::<usize>();
        while self.events.len() > EVENT_COUNT || bytes > EVENT_BYTES {
            if self.events.len() == 1 && self.events[0].event.k == "user" {
                // The full submitted request is separately owned by Active.original.
                // Retain its journal position without duplicating an oversized body.
                let t = self.events[0].event.t;
                self.events[0].event = crate::threads::ThreadEvent::new("user");
                self.events[0].event.t = t;
                self.events[0].call = None;
                if self.events[0].bytes() <= EVENT_BYTES {
                    break;
                }
            }
            let index = usize::from(self.events.len() > 1 && self.events[0].event.k == "user");
            let removed = self.events.remove(index);
            bytes = bytes.saturating_sub(removed.bytes());
            if self.omitted.is_none() {
                self.omitted = Some(Pending {
                    thread: removed.thread,
                    event: crate::threads::ThreadEvent::new("note"),
                    call: None,
                });
                self.omission_notice = true;
            }
        }
    }

    /// Pruning requires a verified capture of this exact latest native owner.
    /// Restored-only or failed-capture views stay protected until a verified turn.
    pub fn projection_verified(&self, root: &Path, binding: &Binding) -> bool {
        self.verified.as_ref().is_some_and(|reference| {
            reference.binding() == binding && native::hydrate_text(root, reference).is_ok()
        })
    }

    fn poll_started(&mut self) {
        while let Ok(started) = self.rx.try_recv() {
            if self
                .active
                .as_ref()
                .is_some_and(|active| active.run == started.run)
            {
                self.started = Some(started);
            }
        }
    }

    pub fn load(&mut self, root: PathBuf) {
        self.added = 0;
        let (tx, rx) = channel::channel();
        self.load = Some(rx);
        std::thread::spawn(move || {
            let legacy = super::persist::load_history_status(&root);
            let mut loaded = load_references(&root);
            loaded.legacy = legacy.messages;
            loaded.unavailable += legacy.unavailable;
            let _ = tx.send(loaded);
        });
    }

    pub fn poll(&mut self, history: &mut Vec<String>) -> Option<&'static str> {
        self.poll_started();
        let mut notice = None;
        let omitted = std::mem::take(&mut self.omission_notice);
        if let Some(mut result) = self.load.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(loaded) => Some(loaded),
            Err(channel::TryRecvError::Disconnected) => Some(Loaded {
                unavailable: 1,
                ..Loaded::default()
            }),
            Err(channel::TryRecvError::Empty) => None,
        }) {
            self.load = None;
            let mut merged = result.legacy;
            for message in result.messages {
                if merged.last() != Some(&message) {
                    merged.push(message);
                }
            }
            // New messages typed while the background read ran stay newest.
            for message in history
                .iter()
                .skip(history.len().saturating_sub(self.added))
            {
                if merged.last() != Some(message) {
                    merged.push(message.clone());
                }
            }
            let mut bytes = merged.iter().map(String::len).sum::<usize>();
            result.unavailable += usize::from(bytes > super::persist::HISTORY_TEXT_BYTES);
            let mut remove = 0;
            while remove < merged.len()
                && (merged.len() - remove > super::HISTORY_MAX
                    || bytes > super::persist::HISTORY_TEXT_BYTES)
            {
                bytes -= merged[remove].len();
                remove += 1;
            }
            merged.drain(..remove);
            *history = merged;
            if result.unavailable > 0 {
                notice = Some("Some sent-history entries could not be loaded; existing history and drafts were kept.");
            }
        }
        self.writes.retain(|(run, rx)| match rx.try_recv() {
            Ok(result) => {
                self.durable.insert(run.clone(), result.saved && result.durable);
                if self.latest_run.as_deref() == Some(run.as_str()) && result.generation == self.generation {
                    self.verified = result.verified.filter(|_| result.saved && result.durable);
                }
                if !result.saved || !result.durable { notice = Some("Some history references are unavailable; native originals and recovery were kept."); }
                false
            }
            Err(channel::TryRecvError::Disconnected) => {
                self.durable.insert(run.clone(), false);
                notice = Some("History reference saving stopped; native originals and recovery were kept.");
                false
            }
            Err(channel::TryRecvError::Empty) => true,
        });
        self.durable.retain(|run, _| {
            self.deferred_close.as_deref() == Some(run.as_str())
                || self.writes.iter().any(|(pending, _)| pending == run)
        });
        if omitted {
            Some("Thread recording is partial: older pending events exceeded the local projection bound. An explicit gap is recorded; original native history is unchanged.")
        } else {
            notice
        }
    }

    pub fn backlogged(&self) -> bool {
        self.writes.len() >= 16
    }
    pub fn pending(&self) -> bool {
        !self.writes.is_empty()
    }
    pub fn defer_close(&mut self, run: &str) {
        self.deferred_close = Some(run.to_owned());
    }
    pub fn prevent_close(&mut self) {
        self.deferred_close = None;
    }
    pub fn completed_close(&mut self) -> Option<String> {
        let run = self.deferred_close.as_ref()?;
        if self.writes.iter().any(|(id, _)| id == run) || self.durable.get(run) != Some(&true) {
            return None;
        }
        let run = self.deferred_close.take()?;
        self.durable.remove(&run);
        Some(run)
    }

    pub fn remember_added(&mut self) {
        if self.load.is_some() {
            self.added = self.added.saturating_add(1);
        }
    }

    #[cfg(test)]
    pub fn settle_for_test(&mut self, history: &mut Vec<String>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.writes.is_empty() || self.load.is_some() {
            self.poll(history);
            assert!(
                Instant::now() < deadline,
                "reference journal did not settle"
            );
            std::thread::yield_now();
        }
    }

    pub fn finish(&mut self, root: PathBuf, session: Option<String>, settled: bool) {
        self.poll_started();
        let mut events = std::mem::take(&mut self.events);
        if let Some(gap) = self.omitted.take() {
            events.insert(
                usize::from(
                    events
                        .first()
                        .is_some_and(|pending| pending.event.k == "user"),
                ),
                gap,
            );
        }
        let active = self.active.take();
        let started = self.started.take();
        if events.is_empty() && active.as_ref().is_none_or(|active| !active.history) {
            return;
        }
        let (tx, rx) = channel::channel();
        self.writes.push((
            active
                .as_ref()
                .map_or_else(String::new, |active| active.run.clone()),
            rx,
        ));
        let work: Work = Box::new(move || {
            let scope = (|| {
                if !settled {
                    return Err("unbound".to_string());
                }
                let active = active.as_ref().ok_or("unbound")?;
                let started = started.as_ref().ok_or("unbound")?;
                let session = session.as_deref().ok_or("unbound")?;
                let home = active.home.as_deref().ok_or("unbound")?;
                let binding = Binding::new(active.provider, &active.account, home, session)?;
                let scope = match &started.scope {
                    Ok(Some(scope)) if scope.binding() == &binding => Ok(scope.clone()),
                    // None is emitted only for an explicitly fresh request before dispatch.
                    Ok(None) => native::fresh_scope(&root, &binding),
                    _ => Err("unbound".into()),
                }?;
                native::seal_scope(&root, &scope)
            })();
            let original_ref = match (scope.as_ref(), started.as_ref()) {
                (Ok(scope), Some(started)) => started
                    .original
                    .as_ref()
                    .ok_or("unsupported".to_owned())
                    .and_then(|range| {
                        native::capture_projected_text(&root, scope, "user", &started.input, range)
                    }),
                _ => Err("unbound".into()),
            };
            let mut saved = true;
            for pending in events {
                let log = crate::threads::ThreadLog::open(&root, &pending.thread);
                let mut event = pending.event;
                if event.k == "note" {
                    saved &= log
                        .append_unavailable(&event, scope.as_ref().ok(), "projection_omitted")
                        .is_ok();
                    continue;
                }
                let reference = match (scope.as_ref(), event.k.as_str()) {
                    (Ok(_), "user") => {
                        if let Some(active) = &active {
                            event.text = Some(active.original.text.clone());
                        }
                        original_ref.clone()
                    }
                    (Ok(scope), "assistant") => native::capture_text(
                        &root,
                        scope,
                        "assistant",
                        event.text.as_deref().unwrap_or(""),
                    ),
                    (Ok(scope), "card") => pending
                        .call
                        .as_deref()
                        .ok_or("unsupported".to_owned())
                        .and_then(|id| native::capture_tool(&root, scope, id)),
                    _ => Err("unbound".into()),
                };
                let result = reference.and_then(|reference| {
                    log.append_referenced(&event, reference)
                        .map_err(|e| e.to_string())
                });
                if result.is_err() {
                    saved &= log
                        .append_unavailable(&event, scope.as_ref().ok(), "capture_failed")
                        .is_ok();
                }
            }
            let durable = original_ref.is_ok();
            let verified = original_ref.as_ref().ok().cloned();
            let generation = active.as_ref().map_or(0, |active| active.generation);
            if let Some(active) = active.filter(|active| active.history) {
                let binding =
                    active
                        .home
                        .as_deref()
                        .zip(session.as_deref())
                        .and_then(|(home, session)| {
                            Binding::new(active.provider, &active.account, home, session).ok()
                        });
                let entry = HistoryEntry {
                    owner: OWNER.into(),
                    version: 1,
                    id: active.run,
                    unavailable: original_ref.is_err(),
                    source: original_ref.ok(),
                    attachments: active.original.attachments,
                    created: active.created,
                    binding,
                };
                saved &= append_reference(&root, entry).is_ok();
            }
            let _ = tx.send(Completion {
                saved,
                durable,
                verified,
                generation,
            });
        });
        let _ = self.work.try_send(work);
    }
}

fn path(root: &Path) -> PathBuf {
    root.join("history.refs.jsonl")
}

fn read_entries(root: &Path) -> Result<Vec<HistoryEntry>, String> {
    let file = path(root);
    if !file.exists() {
        return Ok(Vec::new());
    }
    let owned = crate::update::resolved_directory(root).ok_or("history directory unavailable")?;
    if !crate::update::regular_owned_file(&file, &owned) {
        return Err("history reference file is not a regular owned file".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(file)
        .map_err(|e| e.to_string())?
        .take(FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > FILE_BYTES {
        return Err("history reference file exceeds its bound".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "invalid history reference encoding")?;
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let entry: HistoryEntry =
                serde_json::from_str(line).map_err(|_| "invalid history reference record")?;
            if entry.owner != OWNER
                || entry.version != 1
                || uuid::Uuid::parse_str(&entry.id).is_err()
                || entry.attachments.len() > crate::attachments::MAX_IMAGES
                || entry.source.is_some() == entry.unavailable
                || entry.source.as_ref().is_some_and(|source| {
                    entry
                        .binding
                        .as_ref()
                        .is_some_and(|binding| source.binding() != binding)
                })
            {
                return Err("unsupported history reference record".into());
            }
            Ok(entry)
        })
        .collect()
}

fn append_reference(root: &Path, entry: HistoryEntry) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let root = crate::update::resolved_directory(root).ok_or("history directory unavailable")?;
    let lock_path = root.join("history.refs.lock");
    if lock_path.exists() && !crate::update::regular_owned_file(&lock_path, &root) {
        return Err("invalid history lock".into());
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(1);
    while lock.try_lock().is_err() {
        if Instant::now() >= deadline {
            return Err("history reference writer is busy".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut entries = read_entries(&root)?;
    entries.retain(|old| old.id != entry.id);
    entries.push(entry);
    entries.sort_by(|left, right| {
        left.created
            .cmp(&right.created)
            .then_with(|| left.id.cmp(&right.id))
    });
    if entries.len() > super::persist::HISTORY_CAP {
        entries.drain(..entries.len() - super::persist::HISTORY_CAP);
    }
    let mut bytes = Vec::new();
    for entry in entries {
        serde_json::to_writer(&mut bytes, &entry).map_err(|e| e.to_string())?;
        bytes.write_all(b"\n").map_err(|e| e.to_string())?;
    }
    if bytes.len() as u64 > FILE_BYTES {
        return Err("history references exceed storage bound".into());
    }
    crate::state_io::write_atomic(&path(&root), &bytes).map_err(|e| e.to_string())
}

fn load_references(root: &Path) -> Loaded {
    let entries = match read_entries(root) {
        Ok(entries) => entries,
        Err(_) => {
            return Loaded {
                unavailable: 1,
                ..Loaded::default()
            }
        }
    };
    let mut result = Loaded::default();
    let mut bytes = 0usize;
    for entry in entries.into_iter().rev().take(super::persist::HISTORY_CAP) {
        let text = entry
            .source
            .as_ref()
            .filter(|reference| reference.role() == "user")
            .ok_or("unavailable")
            .and_then(|reference| native::hydrate_text(root, reference).map_err(|_| "unavailable"));
        match text {
            Ok(text) => {
                let message = crate::attachments::Message {
                    text,
                    attachments: entry.attachments,
                    invalid: None,
                };
                if message.validate().is_ok() {
                    let encoded = message.encode();
                    if bytes.saturating_add(encoded.len()) > super::persist::HISTORY_TEXT_BYTES {
                        result.unavailable += 1;
                        break;
                    }
                    bytes += encoded.len();
                    result.messages.push(encoded);
                } else {
                    result.unavailable += 1;
                }
            }
            Err(_) => result.unavailable += 1,
        }
    }
    result.messages.reverse();
    result
}

/// Derive a composer projection from known dispatch construction, never from a guessed native match.
pub(super) fn original_range(
    full: &str,
    raw: &str,
    original: &str,
) -> Option<std::ops::Range<usize>> {
    if !full.ends_with(raw) || !raw.starts_with(original) {
        return None;
    }
    let start = full.len().checked_sub(raw.len())?;
    Some(start..start + original.len())
}

impl super::App {
    pub(super) fn poll_reference_journal(&mut self) -> bool {
        let mut changed = false;
        if let Some(notice) = self.reference_journal.poll(&mut self.history) {
            self.flash(notice);
            changed = true;
        }
        if !self.busy {
            if let Some(run) = self.reference_journal.completed_close() {
                if let Some(lease) = &self.keep.lease {
                    if lease
                        .turn()
                        .ok()
                        .flatten()
                        .is_some_and(|checkpoint| checkpoint.run_id == run)
                        && lease
                            .save_turn(None::<&super::turn_checkpoint::Checkpoint>)
                            .is_err()
                    {
                        changed = true;
                        self.flash("History references were saved; request recovery cleanup remains pending.");
                    }
                }
            }
        }
        changed
    }
}

#[cfg(test)]
pub(super) fn fixture_proof() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let home = root.join("native-home");
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    crate::store::upsert_stored_account(
        root,
        crate::store::StoredAccount {
            id: "history-fixture".into(),
            provider: ProviderKind::Codex,
            home_path: Some(home.to_string_lossy().into_owned()),
            label: "Synthetic".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    let session = "00000000-0000-4000-8000-000000000083";
    let source = home.join(format!("sessions/{session}.jsonl"));
    let full = "TIME\nCONTEXT\nhello 雪";
    let records = [
        serde_json::json!({"type":"session_meta","payload":{"id":session,"cwd":root}}),
        serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":full}]}}),
    ];
    std::fs::write(
        &source,
        records
            .iter()
            .map(|value| value.to_string() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    let binding = Binding::new(ProviderKind::Codex, "history-fixture", &home, session).unwrap();
    let scope = native::fresh_scope(root, &binding).unwrap();
    let range = original_range(full, "hello 雪", "hello 雪").unwrap();
    let reference = native::capture_projected_text(root, &scope, "user", full, &range).unwrap();
    let legacy = root.join("history.jsonl");
    std::fs::write(&legacy, "\"OLD LEGACY BODY\"\n").unwrap();
    let old = std::fs::read(&legacy).unwrap();
    append_reference(
        root,
        HistoryEntry {
            owner: OWNER.into(),
            version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            source: Some(reference),
            binding: Some(binding.clone()),
            attachments: Vec::new(),
            unavailable: false,
            created: 1,
        },
    )
    .unwrap();
    let stored = std::fs::read_to_string(path(root)).unwrap();
    assert!(!stored.contains("hello") && !stored.contains('雪') && !stored.contains("CONTEXT"));
    assert_eq!(load_references(root).messages, ["hello 雪"]);
    assert_eq!(std::fs::read(&legacy).unwrap(), old);
    assert!(original_range(full, "different", "hello").is_none());
    let mut app = super::App::new(root.to_path_buf(), root.to_path_buf());
    let lease =
        std::sync::Arc::new(super::recovery::create(&root.join("reference-recovery")).unwrap());
    let run = uuid::Uuid::new_v4().to_string();
    let checkpoint = super::turn_checkpoint::Checkpoint {
        version: 1,
        run_id: run.clone(),
        workspace: root.to_path_buf(),
        context_rollovers: 0,
        prompt: "hello 雪".into(),
        title: None,
        sources: vec![binding.clone()],
        tried: Vec::new(),
        reason: "reference transaction pending".into(),
        routing: None,
        scope: None,
        conversation: None,
    };
    lease.save_turn(Some(&checkpoint)).unwrap();
    app.keep.lease = Some(lease.clone());
    let owner = crate::provider::interaction::InteractionOwner {
        conversation_id: uuid::Uuid::new_v4().to_string(),
        workspace: root.to_path_buf(),
        execution_cwd: root.to_path_buf(),
        run_id: run.clone(),
        attempt_id: uuid::Uuid::new_v4().to_string(),
        provider: ProviderKind::Codex,
        account: "history-fixture".into(),
        home: Some(home.clone()),
    };
    app.reference_journal.begin(&owner, "hello 雪", true);
    app.reference_journal.buffer(
        "completed-reference",
        crate::threads::ThreadEvent::with_text("user", "hello 雪"),
        None,
    );
    app.reference_journal
        .sender()
        .send(Started {
            run: run.clone(),
            scope: Ok(Some(native::seal_scope(root, &scope).unwrap())),
            input: full.into(),
            original: Some(range),
        })
        .unwrap();
    app.reference_journal
        .finish(root.to_path_buf(), Some(session.into()), true);
    app.reference_journal.defer_close(&run);
    assert!(
        lease.turn().unwrap().is_some(),
        "request remains until reference commit"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while lease.turn().unwrap().is_some() {
        app.poll_reference_journal();
        assert!(
            Instant::now() < deadline,
            "reference commit did not clear matching recovery"
        );
        std::thread::yield_now();
    }
    assert!(
        !std::fs::read_to_string(root.join("threads/completed-reference.jsonl"))
            .unwrap()
            .contains("hello")
    );
    let stored = std::fs::read_to_string(path(root)).unwrap();
    std::fs::remove_file(&source).unwrap();
    let unavailable = load_references(root);
    assert_eq!(unavailable.unavailable, 2);
    assert!(unavailable.messages.is_empty());
    assert_eq!(std::fs::read_to_string(path(root)).unwrap(), stored);
    assert_eq!(std::fs::read(&legacy).unwrap(), old);
}
