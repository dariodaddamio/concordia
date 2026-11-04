//! Discovery and source inspection reuse existing picker/file action surfaces.
use super::*;
use crate::file_links;
type ReadResult = Result<Vec<crate::artifacts::Record>, String>;
type ReadJob = (String, std::sync::mpsc::Receiver<ReadResult>);
type DetailResult = Result<Option<StreamCard>, String>;
type ActivityJob = (
    String,
    usize,
    String,
    std::sync::mpsc::Receiver<DetailResult>,
);
#[derive(Default)]
pub(super) struct State {
    records: Vec<crate::artifacts::Record>,
    selected: Option<crate::artifacts::Record>,
    job: Option<ReadJob>,
    activity_job: Option<ActivityJob>,
    activity_attempted: Option<(String, usize, String)>,
    detail_job: Option<(
        String,
        String,
        std::sync::mpsc::Receiver<Result<Option<StreamCard>, String>>,
    )>,
}
pub(super) fn open(app: &mut App) {
    if app.artifacts.job.is_some() {
        app.flash("File output metadata is still loading");
        return;
    }
    let conversation = app.deleg.d.conversation_id().to_string();
    let root = app.state_root.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let source = conversation.clone();
    std::thread::spawn(move || {
        let _ = tx.send(crate::artifacts::read(&root, &source));
    });
    app.artifacts.job = Some((conversation, rx));
    app.artifacts.records.clear();
    app.artifacts.selected = None;
    app.show_picker(
        PickerKind::Artifacts,
        "Reported file outputs".into(),
        vec![PickerRow {
            id: "none".into(),
            label: "Reading bounded output metadata…".into(),
            detail: "No file contents are read".into(),
        }],
        false,
    );
    app.footer_left = "Enter inspect source · type to filter · Esc close".into();
}
pub(super) fn poll(app: &mut App) {
    poll_activity(app);
    poll_detail(app);
    let Some((conversation, rx)) = &app.artifacts.job else {
        return;
    };
    let valid = conversation == app.deleg.d.conversation_id()
        && app
            .picker
            .as_ref()
            .is_some_and(|picker| picker.kind == PickerKind::Artifacts);
    let result = match rx.try_recv() {
        Ok(result) => result,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(_) => Err("File output metadata reader stopped".into()),
    };
    app.artifacts.job = None;
    if !valid {
        return;
    }
    let rows = match result {
        Ok(records) => {
            app.artifacts.records = records;
            let mut rows = app
                .artifacts
                .records
                .iter()
                .rev()
                .map(|record| PickerRow {
                    id: record.id.clone(),
                    label: format!("{} · {}", record.kind, record.reported_path),
                    detail: format!(
                        "{} / {} · native event {} · inspect source",
                        record.owner.provider.as_str(),
                        record.owner.account,
                        record.event_id
                    ),
                })
                .collect::<Vec<_>>();
            if rows.is_empty() {
                rows.push(PickerRow { id: "none".into(), label: "No structured file output records in this conversation".into(), detail: "Codex completed file-change events are supported; prose paths remain ordinary links".into() });
            }
            rows
        }
        Err(error) => vec![PickerRow {
            id: "none".into(),
            label: "File output catalog unavailable".into(),
            detail: error,
        }],
    };
    app.show_picker(
        PickerKind::Artifacts,
        "Reported file outputs".into(),
        rows,
        false,
    );
    app.footer_left = "Enter inspect source · type to filter · Esc close".into();
}
fn poll_activity(app: &mut App) {
    let target = app.activity.detail.and_then(|index| {
        let id = &app.messages.get(index)?.card.as_ref()?.id;
        id.strip_prefix("artifact:")
            .filter(|id| id.len() == 64)
            .map(|id| {
                (
                    app.deleg.d.conversation_id().to_owned(),
                    index,
                    id.to_owned(),
                )
            })
    });
    if target.is_none() {
        app.artifacts.activity_attempted = None;
    }
    if let Some((conversation, index, id, rx)) = &app.artifacts.activity_job {
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("native edit reader stopped".into()),
        };
        let valid = target.as_ref().is_some_and(|target| {
            &target.0 == conversation && target.1 == *index && &target.2 == id
        });
        let index = *index;
        app.artifacts.activity_job = None;
        if valid {
            let message = &mut app.messages[index];
            match result {
                Ok(Some(card)) if message.card.as_ref().is_some_and(|old| old.id == card.id) => {
                    message.text = format_card_line(&card);
                    message.card = Some(card);
                }
                Err(error) => {
                    if let Some(card) = &mut message.card {
                        card.detail = Some(format!(
                            "{}\n\nHistorical line diff unavailable: {error}",
                            card.detail.as_deref().unwrap_or_default()
                        ));
                    }
                }
                Ok(_) => {}
            }
            app.activity.invalidate_search();
        }
    }
    let Some((conversation, index, id)) = target else {
        return;
    };
    if app.artifacts.activity_attempted.as_ref() == Some(&(conversation.clone(), index, id.clone()))
    {
        return;
    }
    app.artifacts.activity_attempted = Some((conversation.clone(), index, id.clone()));
    let root = app.state_root.clone();
    let source = conversation.clone();
    let selected = id.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = crate::artifacts::read(&root, &source).and_then(|records| {
            records
                .into_iter()
                .find(|record| record.id == selected)
                .ok_or_else(|| "reported file source identity is unavailable".to_owned())?
                .inspect_edit(&root)
        });
        let _ = tx.send(result);
    });
    app.artifacts.activity_job = Some((conversation, index, id, rx));
}

pub(super) fn choose(app: &mut App, id: &str) {
    let Some(record) = app
        .artifacts
        .records
        .iter()
        .find(|record| {
            record.id == id && record.owner.conversation_id == app.deleg.d.conversation_id()
        })
        .cloned()
    else {
        return;
    };
    show_detail(app, &record, &record.card());
    let root = app.state_root.clone();
    let selected = record.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(selected.inspect_edit(&root));
    });
    app.artifacts.detail_job = Some((app.deleg.d.conversation_id().into(), record.id.clone(), rx));
    app.artifacts.selected = Some(record);
    app.show_picker(
        PickerKind::ArtifactDetail,
        "File output source".into(),
        Vec::new(),
        false,
    );
    app.footer_left = "O current-file actions · R catalog · Esc close".into();
}

fn show_detail(app: &mut App, record: &crate::artifacts::Record, card: &StreamCard) {
    let detail = card.detail.as_deref().unwrap_or_default();
    app.update_preview = Some(crate::update::UpdatePreview { history: Vec::new(), history_expanded: false, lines: std::iter::once(crate::update::PreviewLine::Title("Reported file output".into())).chain(detail.lines().map(|line| crate::update::PreviewLine::Text(line.into()))).chain([crate::update::PreviewLine::Text(format!("Execution directory: {}", record.owner.execution_cwd.display())), crate::update::PreviewLine::Hint("O current-file actions · R catalog · Esc close. Opening never reruns the producing tool.".into())]).collect(), install_available: false, release_url: None });
}

fn poll_detail(app: &mut App) {
    let Some((conversation, id, rx)) = &app.artifacts.detail_job else {
        return;
    };
    let valid = conversation == app.deleg.d.conversation_id()
        && app
            .artifacts
            .selected
            .as_ref()
            .is_some_and(|record| &record.id == id)
        && app
            .picker
            .as_ref()
            .is_some_and(|picker| picker.kind == PickerKind::ArtifactDetail);
    let result = match rx.try_recv() {
        Ok(result) => result,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(_) => Err("native edit reader stopped".into()),
    };
    app.artifacts.detail_job = None;
    if !valid {
        return;
    }
    let Some(record) = app.artifacts.selected.clone() else {
        return;
    };
    match result {
        Ok(Some(card)) => {
            // Artifact ids include owner/run/attempt/event/revision. Bare exec event
            // ids repeat across turns and cannot safely identify old Edit rows.
            for message in &mut app.messages {
                if message.card.as_ref().is_some_and(|old| old.id == card.id) {
                    message.card = Some(card.clone());
                    message.text = format_card_line(&card);
                }
            }
            app.activity.invalidate_search();
            show_detail(app, &record, &card);
        }
        Err(error) => {
            let mut card = record.card();
            card.detail = Some(format!(
                "{}\n\nHistorical line diff unavailable: {error}",
                card.detail.as_deref().unwrap_or_default()
            ));
            show_detail(app, &record, &card);
        }
        Ok(None) => {}
    }
}
pub(super) fn key(app: &mut App, key: KeyEvent) -> bool {
    if !app.picker.as_ref().is_some_and(|picker| {
        matches!(
            picker.kind,
            PickerKind::Artifacts | PickerKind::ArtifactDetail
        )
    }) || key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return false;
    }
    if matches!(key.code, KeyCode::Char('r' | 'R')) {
        app.dismiss_picker();
        open(app);
        return true;
    }
    if app
        .picker
        .as_ref()
        .is_some_and(|picker| picker.kind == PickerKind::ArtifactDetail)
        && matches!(key.code, KeyCode::Char('o' | 'O'))
    {
        if let Some(record) = app
            .artifacts
            .selected
            .clone()
            .filter(|record| record.owner.conversation_id == app.deleg.d.conversation_id())
        {
            match record.open_current() {
                Ok(path) => {
                    app.dismiss_picker();
                    let target = path.to_string_lossy().into_owned();
                    app.show_file_actions(
                        file_links::Candidate {
                            display: target.clone(),
                            range: 0..target.len(),
                            target,
                            line: None,
                            column: None,
                            markdown: false,
                        },
                        record.owner.execution_cwd,
                    );
                    app.flash("Actions use the current disk version; source revision describes the native event");
                }
                Err(error) => app.flash(error),
            }
        }
        return true;
    }
    false
}

#[cfg(test)]
pub(super) fn fixture_proof(app: &mut App, dir: &Path) {
    use serde_json::json;
    let owner = crate::provider::interaction::InteractionOwner {
        conversation_id: app.deleg.d.conversation_id().into(),
        run_id: uuid::Uuid::new_v4().to_string(),
        attempt_id: uuid::Uuid::new_v4().to_string(),
        provider: ProviderKind::Codex,
        account: "fixture-codex".into(),
        home: Some(dir.to_path_buf()),
        workspace: dir.to_path_buf(),
        execution_cwd: dir.to_path_buf(),
    };
    let mut item = json!({"id":"file-output","type":"file_change","status":"completed","changes":[{"path":"output.md","kind":"add","diff":"+first"},{"path":"../outside.md","kind":"add"},{"path":".env","kind":"add"}]});
    let records = crate::artifacts::from_item(&owner, "native-session", Some("native-turn"), &item);
    assert_eq!(records.len(), 3);
    assert!(records[1].local_path.is_none());
    assert!(records[2].local_path.is_none());
    assert!(records[0].open_current().is_err());
    std::fs::write(dir.join("output.md"), "first\n").unwrap();
    assert!(records[0].open_current().is_ok());
    assert!(crate::artifacts::save(&app.state_root, &records[0]).unwrap());
    assert!(!crate::artifacts::save(&app.state_root, &records[0]).unwrap());
    item["changes"][0]["diff"] = json!("+second");
    let revised = crate::artifacts::from_item(&owner, "native-session", Some("native-turn"), &item);
    assert_ne!(revised[0].id, records[0].id);
    assert!(crate::artifacts::save(&app.state_root, &revised[0]).unwrap());
    assert_eq!(
        crate::artifacts::read(&app.state_root, &owner.conversation_id)
            .unwrap()
            .len(),
        2
    );
    // Hydration follows the selected exact artifact identity, never a reused native event id.
    let message_index = app.messages.len();
    for record in [&records[0], &revised[0]] {
        let card = record.card();
        app.messages.push(ChatLine {
            role: MsgRole::Card,
            text: format_card_line(&card),
            card: Some(card),
            meta: None,
        });
    }
    app.activity.detail = Some(message_index);
    let conversation = app.deleg.d.conversation_id().to_owned();
    app.artifacts.activity_attempted =
        Some((conversation.clone(), message_index, records[0].id.clone()));
    let (tx, rx) = std::sync::mpsc::channel();
    let mut hydrated = records[0].card();
    hydrated.title = "Edit".into();
    hydrated.detail = Some("File: output.md\n@@ reported edit @@\n-before\n+after".into());
    app.artifacts.activity_job = Some((
        conversation.clone(),
        message_index,
        records[0].id.clone(),
        rx,
    ));
    tx.send(Ok(Some(hydrated))).unwrap();
    poll_activity(app);
    assert!(app.messages[message_index]
        .card
        .as_ref()
        .unwrap()
        .detail
        .as_deref()
        .unwrap()
        .contains("-before\n+after"));
    assert!(!app.messages[message_index + 1]
        .card
        .as_ref()
        .unwrap()
        .detail
        .as_deref()
        .unwrap()
        .contains("-before\n+after"));
    let (tx, rx) = std::sync::mpsc::channel();
    app.artifacts.activity_job = Some((conversation, message_index, revised[0].id.clone(), rx));
    let mut stale = revised[0].card();
    stale.detail = Some("stale result must not replace selected content".into());
    tx.send(Ok(Some(stale))).unwrap();
    poll_activity(app);
    assert!(!app.messages[message_index]
        .card
        .as_ref()
        .unwrap()
        .detail
        .as_deref()
        .unwrap()
        .contains("stale result"));
    app.activity.detail = None;
    poll_activity(app);
    app.messages.truncate(message_index);
    app.editor.set_text("file catalog keeps this draft");
    open(app);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.artifacts.job.is_some() {
        poll(app);
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(app
        .picker
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|row| row.label.contains("output.md")));
    choose(app, &revised[0].id);
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 40)).unwrap();
    terminal.draw(|f| super::draw_ui(f, app)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(screen.contains("Source revision:"));
    assert!(screen.contains("Current file bytes are unverified"));
    assert!(key(
        app,
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE)
    ));
    assert_eq!(app.picker.as_ref().unwrap().kind, PickerKind::FileActions);
    assert_eq!(app.editor.text(), "file catalog keeps this draft");
    app.dismiss_picker();
    std::fs::remove_file(dir.join("output.md")).unwrap();
    assert!(revised[0].open_current().is_err());
    item["status"] = json!("failed");
    assert!(
        crate::artifacts::from_item(&owner, "native-session", Some("native-turn"), &item)
            .is_empty()
    );
    let metadata = app
        .state_root
        .join("artifact-index")
        .join(format!("{}.json", owner.conversation_id));
    let future = b"{\"version\":99,\"records\":[]}";
    std::fs::write(&metadata, future).unwrap();
    assert!(crate::artifacts::save(&app.state_root, &records[0]).is_err());
    assert_eq!(std::fs::read(&metadata).unwrap(), future);
}
