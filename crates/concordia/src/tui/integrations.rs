//! A bounded, read-only integration catalog owned by the current conversation surface.
use super::{App, PickerKind};
use crate::update::{PreviewLine, UpdatePreview};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

/// The mutation types exposed by F10. Confirmation is intentionally owned by the shared picker;
/// this module only provides the stable review payload and bounded execution primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Connect,
    NewConnect,
    Verify,
    Disconnect,
}

#[derive(Debug, Clone)]
pub(super) struct Review {
    pub action: Action,
    pub target: crate::harness::connections::ActionTarget,
    pub effect: String,
    pub argv: Vec<String>,
    pub conversation: String,
    pub account: String,
    directory_id: String,
    home_id: Option<String>,
}

pub(super) fn review(action: Action, target: crate::harness::connections::ActionTarget) -> Review {
    let effect = match action {
        Action::Connect => "run the selected integration connect command and replace its receipt",
        Action::NewConnect => {
            "run the selected local manifest connect command and create its receipt"
        }
        Action::Verify => "run the selected integration verification probe",
        Action::Disconnect => "withdraw Concordia receipts; provider homes are unchanged",
    };
    Review {
        action,
        directory_id: crate::workspaces::directory_id(std::path::Path::new(&target.project))
            .unwrap_or_default(),
        home_id: target.home.as_deref().map(|home| {
            crate::workspaces::directory_id(std::path::Path::new(home)).unwrap_or_default()
        }),
        target,
        effect: effect.into(),
        argv: Vec::new(),
        conversation: String::new(),
        account: String::new(),
    }
}

pub(super) fn review_lines(review: &Review) -> Vec<PreviewLine> {
    let target = &review.target;
    vec![
        PreviewLine::Title("Review integration action".into()),
        PreviewLine::Kind(format!("{} · {}", target.name, target.provider)),
        PreviewLine::Field("Project", target.project.clone()),
        PreviewLine::Field(
            "Home",
            target.home.clone().unwrap_or_else(|| "default home".into()),
        ),
        PreviewLine::Field("Manifest revision", target.manifest_sha256.clone()),
        PreviewLine::Field("Manifest", target.manifest_path.clone()),
        PreviewLine::Text(review.effect.clone()),
        PreviewLine::Field(
            "Command",
            if review.argv.is_empty() {
                "none; Concordia receipts only".into()
            } else {
                serde_json::to_string(&review.argv).unwrap_or_default()
            },
        ),
        PreviewLine::Hint(
            "Confirm once; a changed revision requires refresh and reselection".into(),
        ),
    ]
}

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    job: Option<Job>,
    catalog: Option<crate::harness::catalog::Catalog>,
    review: Option<Review>,
    action: Option<ActionJob>,
    new_path: Option<String>,
    selected: usize,
}
struct Job {
    generation: u64,
    conversation: String,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<crate::harness::catalog::Catalog, String>>,
}
struct ActionJob {
    generation: u64,
    conversation: String,
    cancel: Arc<AtomicBool>,
    review: Review,
    rx: mpsc::Receiver<Result<String, String>>,
}
fn catalog_lines(catalog: &crate::harness::catalog::Catalog, selected: usize) -> Vec<PreviewLine> {
    let mut lines = vec![PreviewLine::Title("Integrations and agent tools".into())];
    if catalog.integrations.is_empty() {
        lines.push(PreviewLine::Text(
            "No Harness integrations connected. Press N for a new local manifest.".into(),
        ));
    } else {
        let index = selected.min(catalog.integrations.len() - 1);
        let integration = &catalog.integrations[index];
        lines.push(PreviewLine::Kind(format!(
            "Selected {}/{} · {}",
            index + 1,
            catalog.integrations.len(),
            integration.name
        )));
        lines.push(PreviewLine::Field("Source", integration.source.clone()));
        lines.push(PreviewLine::Field("Revision", integration.revision.clone()));
        lines.push(PreviewLine::Text(integration.status.clone()));
        lines.push(PreviewLine::Text(format!(
            "{} tools · {} bindings",
            integration.tools.len(),
            integration.bindings.len()
        )));
        for binding in &integration.bindings {
            lines.push(PreviewLine::Text(binding.clone()));
        }
        for tool in &integration.tools {
            lines.push(PreviewLine::Bullet(format!(
                "{} — {}",
                tool.name, tool.description
            )));
        }
    }
    lines.push(PreviewLine::Hint(
        "↑/↓ or Tab select · C reconnect · V verify · D disconnect · N new · R refresh · Esc close"
            .into(),
    ));
    lines
}
pub(super) fn open(app: &mut App) {
    if app.integrations.action.is_some() {
        app.flash("An integration action is finishing; refresh after it completes");
        return;
    }
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
            let result = crate::harness::catalog::inspect(&root, &job_cancel);
            let _ = tx.send(result);
        })
        .is_err()
    {
        app.flash("Integration inspector could not start");
        return;
    }
    app.integrations.generation = app.integrations.generation.wrapping_add(1);
    app.integrations.job = Some(Job {
        generation: app.integrations.generation,
        conversation: app.deleg.d.conversation_id().into(),
        cancel,
        rx,
    });
    app.integrations.catalog = None;
    app.integrations.review = None;
    app.integrations.new_path = None;
    app.integrations.selected = 0;
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
pub(super) fn handle_key(app: &mut App, mut key: KeyEvent) -> bool {
    if app.integrations.new_path.is_some()
        && !key.modifiers.contains(KeyModifiers::ALT)
        && (matches!(
            key.code,
            KeyCode::Backspace | KeyCode::Char('\u{8}' | '\u{7f}')
        ) || key.code == KeyCode::Char('h') && key.modifiers == KeyModifiers::CONTROL)
    {
        key = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
    }
    if !app
        .picker
        .as_ref()
        .is_some_and(|picker| picker.kind == PickerKind::Integrations)
        || key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
    {
        return false;
    }
    if matches!(key.code, KeyCode::Char('r' | 'R'))
        && app.integrations.review.is_none()
        && app.integrations.new_path.is_none()
    {
        open(app);
        return true;
    }
    if app.integrations.review.is_none()
        && app.integrations.new_path.is_none()
        && matches!(key.code, KeyCode::Up | KeyCode::Down | KeyCode::Tab)
    {
        if let Some(catalog) = app.integrations.catalog.as_ref() {
            if !catalog.integrations.is_empty() {
                let count = catalog.integrations.len();
                app.integrations.selected = match key.code {
                    KeyCode::Up => (app.integrations.selected + count - 1) % count,
                    _ => (app.integrations.selected + 1) % count,
                };
                if let Some(picker) = app.picker.as_mut() {
                    picker.state.select(Some(0));
                }
                app.update_preview = Some(UpdatePreview {
                    history: Vec::new(),
                    history_expanded: false,
                    lines: catalog_lines(catalog, app.integrations.selected),
                    install_available: false,
                    release_url: None,
                });
            }
        }
        return true;
    }
    if key.code == KeyCode::Esc {
        if app.integrations.new_path.take().is_some() {
            app.footer_left =
                "C reconnect · V verify · D disconnect · N new · R refresh · Esc close".into();
            if let Some(catalog) = app.integrations.catalog.as_ref() {
                app.update_preview = Some(UpdatePreview {
                    history: Vec::new(),
                    history_expanded: false,
                    lines: catalog_lines(catalog, app.integrations.selected),
                    install_available: false,
                    release_url: None,
                });
            }
            return true;
        }
        if app.integrations.review.take().is_some() {
            if let Some(catalog) = app.integrations.catalog.as_ref() {
                app.update_preview = Some(UpdatePreview {
                    history: Vec::new(),
                    history_expanded: false,
                    lines: catalog_lines(catalog, app.integrations.selected),
                    install_available: false,
                    release_url: None,
                });
            }
            app.footer_left =
                "C reconnect · V verify · D disconnect · N new · R refresh · Esc close".into();
            return true;
        }
        return false;
    }
    if let Some(path) = app.integrations.new_path.as_mut() {
        match key.code {
            KeyCode::Enter => {
                let selection = path.trim().to_string();
                if selection.is_empty() {
                    app.flash("Enter a local harness.json path");
                    return true;
                }
                match crate::harness::command::load_local_manifest_for_action(&selection, &app.cwd)
                {
                    Ok((manifest, sha, manifest_path)) => {
                        let target = crate::harness::connections::ActionTarget {
                            name: manifest.name.clone(),
                            manifest_path,
                            manifest_sha256: sha,
                            provider: app.provider.as_str().into(),
                            home: app
                                .home_path
                                .as_ref()
                                .map(|p| p.to_string_lossy().into_owned()),
                            project: app.cwd.to_string_lossy().into_owned(),
                        };
                        let mut reviewed = review(Action::NewConnect, target);
                        if let Some(commands) = manifest.providers.get(app.provider.as_str()) {
                            if let Ok(mut argv) = crate::harness::expand(
                                &commands.connect,
                                &crate::harness::Context {
                                    provider: app.provider,
                                    home: app.home_path.as_deref(),
                                    project: &app.cwd,
                                },
                            ) {
                                argv[0] = crate::harness::resolve_program(&argv[0], &manifest.dir)
                                    .to_string_lossy()
                                    .into_owned();
                                reviewed.argv = argv;
                            }
                        }
                        reviewed.conversation = app.deleg.d.conversation_id().to_string();
                        reviewed.account = app.account.clone();
                        app.integrations.review = Some(reviewed);
                        app.integrations.new_path = None;
                        if let Some(review) = app.integrations.review.as_ref() {
                            app.update_preview = Some(UpdatePreview {
                                history: Vec::new(),
                                history_expanded: false,
                                lines: review_lines(review),
                                install_available: false,
                                release_url: None,
                            });
                        }
                        app.footer_left = "Enter confirm · Esc cancel".into();
                    }
                    Err(error) => app.flash(error),
                }
                return true;
            }
            KeyCode::Backspace => {
                path.pop();
                app.update_preview = Some(UpdatePreview { history: Vec::new(), history_expanded: false, lines: vec![PreviewLine::Title("New local integration".into()), PreviewLine::Field("Path", path.clone()), PreviewLine::Hint("Type or paste a local folder/manifest path · Enter inspect · Esc cancel".into())], install_available: false, release_url: None });
                return true;
            }
            KeyCode::Char(c) if !c.is_control() && path.chars().count() < 4096 => {
                path.push(c);
                app.update_preview = Some(UpdatePreview { history: Vec::new(), history_expanded: false, lines: vec![PreviewLine::Title("New local integration".into()), PreviewLine::Field("Path", path.clone()), PreviewLine::Hint("Type or paste a local folder/manifest path · Enter inspect · Esc cancel".into())], install_available: false, release_url: None });
                return true;
            }
            KeyCode::Char(_) => {
                app.flash("Manifest path is limited to 4096 printable characters");
                return true;
            }
            _ => return true,
        }
    }
    if matches!(key.code, KeyCode::Char('n' | 'N')) && app.integrations.review.is_none() {
        app.integrations.new_path = Some(String::new());
        app.update_preview = Some(UpdatePreview {
            history: Vec::new(),
            history_expanded: false,
            lines: vec![
                PreviewLine::Title("New local integration".into()),
                PreviewLine::Field("Path", String::new()),
                PreviewLine::Hint(
                    "Type or paste a local folder/manifest path · Enter inspect · Esc cancel"
                        .into(),
                ),
            ],
            install_available: false,
            release_url: None,
        });
        app.footer_left = "Type path · Enter inspect · Esc cancel".into();
        return true;
    }
    if app.integrations.review.is_some() && key.code == KeyCode::Enter {
        start_action(app);
        return true;
    }
    let action = match key.code {
        KeyCode::Char('c' | 'C') => Action::Connect,
        KeyCode::Char('v' | 'V') => Action::Verify,
        KeyCode::Char('d' | 'D') => Action::Disconnect,
        _ => return false,
    };
    let Some(id) = app.integrations.catalog.as_ref().and_then(|catalog| {
        catalog
            .integrations
            .get(app.integrations.selected)
            .map(|row| row.id.clone())
    }) else {
        app.flash("Select an integration row first");
        return true;
    };
    let Some(target) = app.integrations.catalog.as_ref().and_then(|_| {
        crate::harness::catalog::action_target(
            &app.state_root,
            &id,
            app.provider.as_str(),
            &app.cwd,
            app.home_path.as_deref(),
        )
        .ok()
    }) else {
        app.flash("No binding for this provider, home and project");
        return true;
    };
    let mut selected_review = review(action, target);
    if app
        .integrations
        .catalog
        .as_ref()
        .and_then(|catalog| catalog.integrations.get(app.integrations.selected))
        .is_none_or(|entry| entry.revision != selected_review.target.manifest_sha256)
    {
        app.flash("Integration revision changed; refresh and reselect");
        return true;
    }
    if !selected_review.target.manifest_path.starts_with("command:") {
        if let Ok((manifest, _, _)) = crate::harness::command::load_local_manifest_for_action(
            &selected_review.target.manifest_path,
            &app.cwd,
        ) {
            let commands = if action == Action::Verify {
                manifest
                    .providers
                    .get(app.provider.as_str())
                    .and_then(|p| p.verify.as_ref())
            } else {
                manifest
                    .providers
                    .get(app.provider.as_str())
                    .map(|p| &p.connect)
            };
            if let Some(mut argv) = commands.and_then(|argv| {
                crate::harness::expand(
                    argv,
                    &crate::harness::Context {
                        provider: app.provider,
                        home: app.home_path.as_deref(),
                        project: &app.cwd,
                    },
                )
                .ok()
            }) {
                argv[0] = crate::harness::resolve_program(&argv[0], &manifest.dir)
                    .to_string_lossy()
                    .into_owned();
                selected_review.argv = argv;
            }
        }
    }
    selected_review.conversation = app.deleg.d.conversation_id().to_string();
    selected_review.account = app.account.clone();
    if selected_review.argv.is_empty() && action != Action::Disconnect {
        app.flash("Cannot review this mutation: exact provider command is unavailable");
        return true;
    }
    app.integrations.review = Some(selected_review);
    if let Some(review) = app.integrations.review.as_ref() {
        app.update_preview = Some(UpdatePreview {
            history: Vec::new(),
            history_expanded: false,
            lines: review_lines(review),
            install_available: false,
            release_url: None,
        });
    }
    app.footer_left = "Enter confirm · Esc cancel".into();
    true
}

fn connect_result(entry: crate::harness::connections::Entry) -> Result<String, String> {
    if entry.connect_ok && entry.connect_note == "ok" {
        Ok(format!(
            "Connected {} and recorded its receipt",
            entry.provider
        ))
    } else {
        Err(format!(
            "Connect {}: {}. Changes may already exist; refresh and reconcile before retrying.",
            entry.provider, entry.connect_note
        ))
    }
}

fn start_action(app: &mut App) {
    let Some(review) = app.integrations.review.take() else {
        return;
    };
    let current_home = app
        .home_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());
    if review.conversation != app.deleg.d.conversation_id()
        || review.account != app.account
        || review.target.provider != app.provider.as_str()
        || review.target.home != current_home
        || review.target.project != app.cwd.to_string_lossy()
        || review.directory_id.is_empty()
        || review
            .home_id
            .as_ref()
            .is_some_and(|identity| identity.is_empty())
        || crate::workspaces::directory_id(&app.cwd).ok().as_deref()
            != Some(review.directory_id.as_str())
        || review.home_id
            != app
                .home_path
                .as_deref()
                .map(|home| crate::workspaces::directory_id(home).unwrap_or_default())
    {
        app.flash("Integration target changed; refresh and reselect");
        return;
    }
    if app.integrations.action.is_some() {
        app.flash("An integration action is already running");
        return;
    }
    let root = app.state_root.clone();
    let cwd = app.cwd.clone();
    let conversation = app.deleg.d.conversation_id().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let worker_review = review.clone();
    let (tx, rx) = mpsc::channel();
    if std::thread::Builder::new()
        .name("concordia-integration-action".into())
        .spawn(move || {
            if worker_cancel.load(Ordering::SeqCst) {
                return;
            }
            if crate::workspaces::directory_id(&cwd).ok().as_deref()
                != Some(worker_review.directory_id.as_str())
                || worker_review.target.home.as_deref().map(|home| {
                    crate::workspaces::directory_id(std::path::Path::new(home)).unwrap_or_default()
                }) != worker_review.home_id
            {
                let _ = tx.send(Err(
                    "Integration directory identity changed; refresh and reselect".into(),
                ));
                return;
            }
            let run_cwd = cwd.clone();
            let run = |argv: &[String]| {
                crate::harness::run::run_command_cancellable(
                    argv,
                    &run_cwd,
                    crate::harness::run::DEFAULT_TIMEOUT,
                    &worker_cancel,
                )
            };
            let result = match worker_review.action {
                Action::Disconnect => {
                    crate::harness::command::disconnect_reviewed(&root, &worker_review.target)
                }
                Action::NewConnect | Action::Connect | Action::Verify => {
                    let loaded = if worker_review.action == Action::NewConnect {
                        crate::harness::command::load_local_manifest_for_action(
                            &worker_review.target.manifest_path,
                            &cwd,
                        )
                        .map(|(manifest, sha, _)| (manifest, sha))
                    } else {
                        crate::harness::command::load_manifest_for_action(
                            &worker_review.target.manifest_path,
                            &cwd,
                            &run,
                        )
                    }
                    .and_then(|(manifest, sha)| {
                        if sha != worker_review.target.manifest_sha256 {
                            return Err("manifest revision changed; refresh and reselect".into());
                        }
                        if worker_review.action == Action::NewConnect {
                            crate::harness::command::connect_new_reviewed(
                                &root,
                                &worker_review.target,
                                &manifest,
                                &run,
                            )
                            .and_then(connect_result)
                        } else if worker_review.action == Action::Connect {
                            crate::harness::command::reconnect_reviewed(
                                &root,
                                &worker_review.target,
                                &manifest,
                                &run,
                            )
                            .and_then(connect_result)
                        } else {
                            crate::harness::command::verify_reviewed(
                                &root,
                                &worker_review.target,
                                &manifest,
                                &run,
                            )
                            .map(|verdict| {
                                format!("verify {}: {:?}", worker_review.target.provider, verdict)
                            })
                        }
                    });
                    loaded
                }
            };
            let _ = tx.send(result);
        })
        .is_err()
    {
        app.flash("Could not start integration action");
        return;
    }
    app.integrations.action = Some(ActionJob {
        generation: app.integrations.generation,
        conversation,
        cancel,
        review,
        rx,
    });
    app.footer_left = "Integration action running · Esc cancels publication".into();
}

pub(super) fn poll_action(app: &mut App) {
    let Some(job) = app.integrations.action.as_ref() else {
        return;
    };
    let valid = job.conversation == app.deleg.d.conversation_id()
        && job.generation == app.integrations.generation
        && !job.cancel.load(Ordering::Relaxed)
        && job.review.account == app.account
        && job.review.target.provider == app.provider.as_str()
        && job.review.target.project == app.cwd.to_string_lossy()
        && job.review.target.home.as_deref()
            == app.home_path.as_deref().and_then(|path| path.to_str())
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
        Err(_) => Err("integration action stopped".into()),
    };
    app.integrations.action = None;
    if valid {
        app.update_preview = Some(UpdatePreview {
            history: Vec::new(),
            history_expanded: false,
            lines: result
                .map(|text| {
                    vec![
                        PreviewLine::Title("Integration action finished".into()),
                        PreviewLine::Text(text),
                        PreviewLine::Hint("R refresh · Esc close".into()),
                    ]
                })
                .unwrap_or_else(|error| {
                    vec![
                        PreviewLine::Title("Integration action requires reconciliation".into()),
                        PreviewLine::Text(error),
                        PreviewLine::Hint("Refresh and reselect before retrying".into()),
                    ]
                }),
            install_available: false,
            release_url: None,
        });
        app.footer_left = "R refresh · Esc close".into();
    }
}

/// Accept a terminal paste while the new-manifest field is active. The shared event loop may
/// route clipboard payloads here without placing them in the composer draft.
pub(super) fn paste(app: &mut App, text: &str) -> bool {
    if !app
        .picker
        .as_ref()
        .is_some_and(|picker| picker.kind == PickerKind::Integrations)
    {
        return false;
    }
    let Some(path) = app.integrations.new_path.as_mut() else {
        return false;
    };
    let pasted = text.trim_matches(['\r', '\n']);
    if pasted.chars().any(char::is_control) || path.chars().count() + pasted.chars().count() > 4096
    {
        app.flash("Manifest path accepts printable text up to 4096 characters");
        return true;
    }
    path.push_str(pasted);
    app.update_preview = Some(UpdatePreview {
        history: Vec::new(),
        history_expanded: false,
        lines: vec![
            PreviewLine::Title("New local integration".into()),
            PreviewLine::Field("Path", path.clone()),
            PreviewLine::Hint("Enter inspect · Esc cancel".into()),
        ],
        install_available: false,
        release_url: None,
    });
    true
}

pub(super) fn poll(app: &mut App) {
    poll_action(app);
    let Some(job) = app.integrations.job.as_ref() else {
        return;
    };
    let valid = job.conversation == app.deleg.d.conversation_id()
        && job.generation == app.integrations.generation
        && !job.cancel.load(Ordering::Relaxed)
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
        let lines = result
            .map(|catalog| {
                app.integrations.catalog = Some(catalog);
                catalog_lines(
                    app.integrations.catalog.as_ref().unwrap(),
                    app.integrations.selected,
                )
            })
            .unwrap_or_else(|error| {
                vec![
                    PreviewLine::Title("Integration catalog unavailable".into()),
                    PreviewLine::Text(error),
                    PreviewLine::Hint("R retry · Esc close".into()),
                ]
            });
        if let Some(preview) = app.update_preview.as_mut() {
            preview.lines = lines;
        }
        if let Some(catalog) = app.integrations.catalog.as_ref() {
            let rows = catalog
                .integrations
                .iter()
                .map(|integration| super::PickerRow {
                    id: integration.id.clone(),
                    label: integration.name.clone(),
                    detail: integration.status.clone(),
                })
                .collect();
            app.show_picker(PickerKind::Integrations, "integrations".into(), rows, false);
            app.footer_left =
                "C reconnect · V verify · D disconnect · N new · R refresh · Esc close".into();
        }
    }
}
