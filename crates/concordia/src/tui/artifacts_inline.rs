//! Recover source-backed patches into their original chat rows, off the UI thread.
use super::*;

type OwnerKey = (String, String, String);
#[derive(Clone)]
struct Target {
    index: usize,
    original: StreamCard,
    record: crate::artifacts::Record,
    not_before: i64,
}
struct Job {
    conversation: String,
    rows: Vec<(usize, String)>,
    rx: std::sync::mpsc::Receiver<Result<Vec<Option<StreamCard>>, String>>,
}
#[derive(Default)]
pub(super) struct State {
    targets: Vec<Target>,
    job: Option<Job>,
    reads: std::collections::HashMap<OwnerKey, u8>,
    finalized: std::collections::HashSet<OwnerKey>,
    next: Option<Instant>,
}

fn owner(record: &crate::artifacts::Record) -> OwnerKey {
    (
        record.owner.conversation_id.clone(),
        record.owner.run_id.clone(),
        record.owner.attempt_id.clone(),
    )
}

pub(super) fn record_live(app: &mut App, record: &crate::artifacts::Record) -> bool {
    if app.native_owner.as_ref() != Some(&record.owner) {
        return false;
    }
    let start = app
        .messages
        .iter()
        .rposition(|line| line.role == MsgRole::User)
        .unwrap_or(0);
    let Some((index, card)) =
        app.messages
            .iter()
            .enumerate()
            .skip(start)
            .rev()
            .find_map(|(index, message)| {
                let card = message.card.as_ref()?;
                (card.id == record.event_id
                    && card.title == "Edit"
                    && card.status == CardStatus::Done)
                    .then_some((index, card.clone()))
            })
    else {
        return false;
    };
    if record.owner.provider != ProviderKind::Codex
        || !crate::provider::edit_display::needs_codex_patch(std::slice::from_ref(&card))
    {
        return true;
    }
    let state = &mut app.artifacts.inline;
    if state.targets.iter().any(|target| {
        target.index == index
            && target.record.owner == record.owner
            && target.original.id == card.id
    }) {
        return true;
    }
    let not_before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|now| {
            now.checked_sub(
                app.turn_started
                    .map_or(Duration::ZERO, |start| start.elapsed()),
            )
        })
        .and_then(|start| i64::try_from(start.as_millis()).ok())
        .unwrap_or(i64::MAX);
    state.targets.push(Target {
        index,
        original: card,
        record: record.clone(),
        not_before,
    });
    // Metadata-only targets are bounded like the file catalog; no per-frame disk reads.
    if state.targets.len() > 256 {
        state.targets.remove(0);
    }
    if state.reads.len() > 256 {
        state.reads.clear();
        state.finalized.clear();
    }
    state.next = Some(Instant::now() + Duration::from_millis(150));
    true
}

fn unresolved(app: &App, target: &Target) -> bool {
    target.record.owner.conversation_id == app.deleg.d.conversation_id()
        && app
            .messages
            .get(target.index)
            .and_then(|line| line.card.as_ref())
            .is_some_and(|card| {
                card.id == target.original.id
                    && crate::provider::edit_display::needs_codex_patch(std::slice::from_ref(card))
            })
}

pub(super) fn projection_pending(app: &App) -> bool {
    app.artifacts.inline.job.is_some()
        || app.artifacts.inline.targets.iter().any(|target| {
            unresolved(app, target)
                && !app
                    .artifacts
                    .inline
                    .finalized
                    .contains(&owner(&target.record))
        })
}

pub(super) fn poll(app: &mut App) -> bool {
    let mut changed = false;
    let completed = app.artifacts.inline.job.as_ref().and_then(|job| {
        let result = match job.rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
            Err(_) => Err("native inline edit reader stopped".into()),
        };
        Some((job.conversation.clone(), job.rows.clone(), result))
    });
    if let Some((conversation, rows, result)) = completed {
        app.artifacts.inline.job = None;
        if conversation == app.deleg.d.conversation_id() {
            if let Ok(cards) = result {
                for ((index, expected), card) in rows.into_iter().zip(cards) {
                    let Some(card) = card else {
                        continue;
                    };
                    if let Some(message) = app.messages.get_mut(index).filter(|message| {
                        message
                            .card
                            .as_ref()
                            .is_some_and(|old| old.id == expected && old.title == "Edit")
                    }) {
                        changed = true;
                        message.text = format_card_line(&card);
                        message.card = Some(card);
                    }
                }
                app.activity.invalidate_search();
                app.related_files.invalidate();
            }
        }
    }
    if app.artifacts.inline.job.is_some()
        || app
            .artifacts
            .inline
            .next
            .is_some_and(|at| at > Instant::now())
    {
        return changed;
    }
    let selected = app
        .artifacts
        .inline
        .targets
        .iter()
        .find(|target| {
            if !unresolved(app, target) {
                return false;
            }
            let key = owner(&target.record);
            let active = app.busy && app.native_owner.as_ref() == Some(&target.record.owner);
            if active {
                // Each newly captured completed edit permits two debounced group reads. Later
                // edits in a long turn reopen the budget without a per-frame polling loop.
                let budget = app
                    .artifacts
                    .inline
                    .targets
                    .iter()
                    .filter(|peer| owner(&peer.record) == key)
                    .count()
                    .min(32) as u8
                    * 2;
                app.artifacts.inline.reads.get(&key).copied().unwrap_or(0) < budget
            } else {
                !app.artifacts.inline.finalized.contains(&key)
            }
        })
        .cloned();
    let Some(selected) = selected else {
        return changed;
    };
    let key = owner(&selected.record);
    let active = app.busy && app.native_owner.as_ref() == Some(&selected.record.owner);
    let targets = app
        .artifacts
        .inline
        .targets
        .iter()
        .filter(|target| {
            owner(&target.record) == key
                && target.record.native_session == selected.record.native_session
        })
        .cloned()
        .collect::<Vec<_>>();
    if active {
        *app.artifacts.inline.reads.entry(key).or_default() += 1;
    } else {
        app.artifacts.inline.finalized.insert(key);
    }
    app.artifacts.inline.next = Some(Instant::now() + Duration::from_millis(300));
    let rows = targets
        .iter()
        .map(|target| (target.index, target.original.id.clone()))
        .collect();
    let conversation = selected.record.owner.conversation_id.clone();
    let root = app.state_root.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let patches = if active {
                let home = selected
                    .record
                    .owner
                    .home
                    .as_deref()
                    .ok_or("inline edit has no native home")?;
                let binding = crate::native_memory::Binding::new(
                    ProviderKind::Codex,
                    &selected.record.owner.account,
                    home,
                    &selected.record.native_session,
                )?;
                crate::native_memory::live_codex_patches(&root, &binding, selected.not_before)?
            } else {
                crate::native_memory::historical_codex_patches(&root, &selected.record)?
            };
            let mut cards = targets
                .into_iter()
                .map(|target| target.original)
                .collect::<Vec<_>>();
            let changed = crate::provider::edit_display::enrich_codex(
                &mut cards,
                &patches,
                &selected.record.owner.execution_cwd,
            )
            .into_iter()
            .map(|card| card.id)
            .collect::<std::collections::HashSet<_>>();
            Ok(cards
                .into_iter()
                .map(|card| changed.contains(&card.id).then_some(card))
                .collect())
        })();
        let _ = tx.send(result);
    });
    app.artifacts.inline.job = Some(Job {
        conversation,
        rows,
        rx,
    });
    changed
}

#[cfg(test)]
pub(super) fn fixture_proof(app: &mut App) {
    use serde_json::json;
    let root = app.state_root.clone();
    let home = root.join("inline-fixture-home");
    let session = uuid::Uuid::new_v4().to_string();
    let folder = home.join("sessions");
    std::fs::create_dir_all(&folder).unwrap();
    crate::store::upsert_stored_account(
        &root,
        crate::store::StoredAccount {
            id: "inline-fixture".into(),
            provider: ProviderKind::Codex,
            home_path: Some(home.to_string_lossy().into_owned()),
            label: "Synthetic inline fixture".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    let native_owner = crate::provider::interaction::InteractionOwner {
        conversation_id: app.deleg.d.conversation_id().into(),
        workspace: app.cwd.clone(),
        execution_cwd: app.cwd.clone(),
        run_id: uuid::Uuid::new_v4().to_string(),
        attempt_id: uuid::Uuid::new_v4().to_string(),
        provider: ProviderKind::Codex,
        account: "inline-fixture".into(),
        home: Some(home),
    };
    let patch = "*** Begin Patch\n*** Update File: fixture.rs\n@@\n-before\n+after\n*** End Patch";
    let script = format!(
        "// @exec: {{\"max_output_tokens\":1000}}\ntext(await tools.apply_patch({}))",
        serde_json::to_string(patch).unwrap()
    );
    let clock = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let timestamp = crate::logging::iso_utc(clock.as_secs());
    let native = [
        json!({"type":"session_meta","payload":{"id":session}}),
        json!({"type":"event_msg","timestamp":timestamp,"payload":{"type":"task_started","turn_id":"inline-turn"}}),
        json!({"type":"turn_context","timestamp":timestamp,"payload":{"turn_id":"inline-turn"}}),
        json!({"type":"response_item","payload":{"type":"custom_tool_call","call_id":"inline-patch","name":"exec","input":script}}),
        json!({"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"inline-patch","output":[{"type":"input_text","text":"Script completed\nOutput:\n"},{"type":"input_text","text":"{}"}]}}),
    ];
    std::fs::write(
        folder.join(format!("{session}.jsonl")),
        native
            .iter()
            .map(|row| format!("{row}\n"))
            .collect::<String>(),
    )
    .unwrap();
    app.native_owner = Some(native_owner.clone());
    app.native_session_id = Some(session.clone());
    app.busy = true;
    app.turn_started = Some(Instant::now() - Duration::from_secs(2));
    app.artifacts.inline = State::default();
    app.messages.push(ChatLine {
        role: MsgRole::User,
        text: "Synthetic inline diff request".into(),
        card: None,
        meta: None,
    });
    let item = json!({"id":"inline-edit","type":"file_change","status":"completed","changes":[{"path":"fixture.rs","kind":"update"}]});
    let record = crate::artifacts::from_item(&native_owner, &session, None, &item).remove(0);
    let card = StreamCard {
        id: "inline-edit".into(),
        title: "Edit".into(),
        status: CardStatus::Done,
        detail: Some("File: fixture.rs\nProvider reported a change; line diff unavailable.".into()),
    };
    let index = app.messages.len();
    app.messages.push(ChatLine {
        role: MsgRole::Card,
        text: format_card_line(&card),
        card: Some(card),
        meta: None,
    });
    assert!(record_live(app, &record));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.messages[index]
        .card
        .as_ref()
        .unwrap()
        .detail
        .as_deref()
        .unwrap()
        .contains("-before\n+after")
    {
        poll(app);
        assert!(
            Instant::now() < deadline,
            "original Edit row did not hydrate inline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.messages[index].card.as_ref().unwrap().id, "inline-edit");
    assert!(app.messages[index].text.contains("fixture.rs  +1 -1"));
    assert!(app.artifacts.inline.reads.values().all(|reads| *reads <= 2));
    let mut foreign = record;
    foreign.owner.account = "foreign".into();
    assert!(!record_live(app, &foreign));
    app.busy = false;
    app.native_owner = None;
}
