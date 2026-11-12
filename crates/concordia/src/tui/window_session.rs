//! Window-owned tab layout and bounded display snapshots. Native sources remain authoritative.
use super::super::session_ui::HISTORY_OMITTED as OMITTED_NOTICE;
use super::*;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_TABS: usize = 512;
const MAX_STARTUP_RECOVERIES: usize = 8;

#[derive(Serialize, Deserialize)]
struct Window {
    version: u32,
    workspace: PathBuf,
    active: usize,
    tabs: Vec<SavedTab>,
    #[serde(default)]
    retained: Vec<SavedTab>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedTab {
    id: String,
    home_origin: Option<String>,
    recovery: String,
    provider: ProviderKind,
    account: String,
    home: Option<PathBuf>,
    model: String,
    session: Option<String>,
    sources: Vec<crate::native_memory::Binding>,
    title: Option<String>,
    conversation: String,
    home_open: bool,
    messages: Vec<SavedLine>,
    omitted: bool,
    #[serde(default)]
    scroll: usize,
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedLine {
    role: String,
    text: String,
    card: Option<SavedCard>,
    meta: Option<(ProviderKind, String)>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedCard {
    id: String,
    title: String,
    detail: Option<String>,
    status: u8,
}

pub(super) struct Owner {
    path: PathBuf,
    workspace: PathBuf,
    _lock: File,
    last_save: Instant,
    previous: Vec<u8>,
    retained: Vec<SavedTab>,
}

fn same_home(left: &Path, right: &Path) -> bool {
    matches!((std::fs::canonicalize(left),std::fs::canonicalize(right)),(Ok(left),Ok(right)) if left==right)
}

fn lock(path: &Path, root: &Path) -> io::Result<File> {
    if path.exists() && !crate::fs_safe::regular_owned_file(path, root) {
        return Err(io::Error::other(
            "tab-layout lock is not a regular owned file",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let file = options.open(path)?;
    file.try_lock().map_err(io::Error::from)?;
    Ok(file)
}

fn owner(root: &Path, workspace: &Path) -> io::Result<(Owner, Option<Window>)> {
    let directory = super::super::recovery::recovery_root(root, workspace)?.join("windows");
    std::fs::create_dir_all(&directory)?;
    let canonical = crate::fs_safe::resolved_directory(&directory)
        .ok_or_else(|| io::Error::other("tab-layout directory is not regular"))?;
    let workspace = std::fs::canonicalize(workspace)?;
    let mut candidates = Vec::new();
    for (index, entry) in std::fs::read_dir(&directory)?.enumerate() {
        if index >= 4096 {
            return Err(io::Error::other(
                "tab-layout directory exceeds its scan limit; existing layouts preserved",
            ));
        }
        let entry = entry?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
            && path
                .file_stem()
                .and_then(|name| name.to_str())
                .is_some_and(|name| uuid::Uuid::parse_str(name).is_ok())
            && crate::fs_safe::regular_owned_file(&path, &canonical)
        {
            candidates.push((entry.metadata()?.modified()?, path));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in candidates {
        let held = match lock(&path.with_extension("lock"), &canonical) {
            Ok(held) => held,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || matches!(error.raw_os_error(), Some(32 | 33)) =>
            {
                continue
            }
            Err(error) => return Err(error),
        };
        let result = crate::fs_safe::read_text_bounded(&path, MAX_BYTES)
            .and_then(|text| serde_json::from_str::<Window>(&text).ok());
        if let Some(saved) = result.filter(|saved| {
            matches!(saved.version, 1 | 2)
                && saved.workspace == workspace
                && !saved.tabs.is_empty()
                && saved.tabs.len().saturating_add(saved.retained.len()) <= MAX_TABS
                && saved.active < saved.tabs.len()
                && saved.tabs.iter().chain(saved.retained.iter()).all(|tab| {
                    uuid::Uuid::parse_str(&tab.id).is_ok_and(|id| id.to_string() == tab.id)
                        && uuid::Uuid::parse_str(&tab.conversation)
                            .is_ok_and(|id| id.to_string() == tab.conversation)
                        && tab.sources.len() <= 8
                })
                && saved
                    .tabs
                    .iter()
                    .map(|tab| &tab.id)
                    .collect::<HashSet<_>>()
                    .len()
                    == saved.tabs.len()
                && saved
                    .tabs
                    .iter()
                    .map(|tab| &tab.recovery)
                    .collect::<HashSet<_>>()
                    .len()
                    == saved.tabs.len()
        }) {
            return Ok((
                Owner {
                    path,
                    workspace,
                    _lock: held,
                    last_save: Instant::now(),
                    previous: Vec::new(),
                    retained: Vec::new(),
                },
                Some(saved),
            ));
        }
        // Do not silently substitute an older Home window for the newest saved tabs.
        return Err(io::Error::other(
            "saved tab layout is unreadable or unsupported; original preserved",
        ));
    }
    let path = directory.join(format!("{}.json", uuid::Uuid::new_v4()));
    let held = lock(&path.with_extension("lock"), &canonical)?;
    Ok((
        Owner {
            path,
            workspace,
            _lock: held,
            last_save: Instant::now(),
            previous: Vec::new(),
            retained: Vec::new(),
        },
        None,
    ))
}

pub(super) fn open(root: &Path, cwd: &Path, native: bool) -> (Option<Owner>, tabs::TabSet<RunTab>) {
    match owner(root, cwd) {
        Ok((mut owner, Some(mut saved))) => {
            owner.retained = std::mem::take(&mut saved.retained);
            let mut selected_id = saved.tabs[saved.active].id.clone();
            let recorded: std::collections::HashSet<_> = saved
                .tabs
                .iter()
                .chain(owner.retained.iter())
                .map(|tab| tab.recovery.clone())
                .collect();
            let mut duplicate_records = Vec::new();
            let mut restored = Vec::new();
            for saved in saved.tabs {
                let original = saved.clone();
                let mut app = App::new(root.into(), cwd.into());
                app.refresh_binding();
                let target = get_account(root, &saved.account).filter(|account| {
                    account.provider == saved.provider
                        && account
                            .home_path
                            .as_ref()
                            .zip(saved.home.as_ref())
                            .is_some_and(|(home, saved)| same_home(Path::new(home), saved))
                });
                let mut valid = target.is_some();
                app.provider = saved.provider;
                app.account = if valid {
                    saved.account.clone()
                } else {
                    String::new()
                };
                app.home_path = target.and_then(|account| account.home_path.map(PathBuf::from));
                app.refresh_account_email();
                app.model = saved.model;
                app.refresh_usage();
                app.messages
                    .retain(|message| message.text != super::super::START_HINT);
                app.restore_state_for_record(Some(&saved.recovery));
                let notices = std::mem::take(&mut app.messages);
                let checkpoint = app.keep.turn_recovery_notice && app.continuation.waiting();
                if checkpoint && !app.native_sources.is_empty() {
                    valid = bind_recovered_source(&mut app);
                }
                let same_conversation = !checkpoint
                    || (app.keep.restored_conversation
                        && app.deleg.d.conversation_id() == saved.conversation);
                app.messages = if same_conversation {
                    saved.messages.into_iter().map(SavedLine::restore).collect()
                } else {
                    Vec::new()
                };
                app.native_agents.rebuild(&app.messages);
                if saved.omitted && same_conversation {
                    app.push_system(OMITTED_NOTICE);
                }
                app.messages.extend(notices);
                if !same_conversation {
                    app.push_system(
                        "Recovered a newer unfinished request; its conversation remains paused.",
                    );
                }
                app.view.scroll = saved.scroll;
                // The request checkpoint can be newer than the periodic window snapshot.
                if !app.keep.turn_recovery_notice {
                    app.native_sources = saved.sources;
                }
                if !app.keep.turn_recovery_notice && valid {
                    app.native_session_id = saved.session;
                } else {
                    app.native_session_id = None;
                }
                if !checkpoint {
                    app.session_ui.title = saved.title;
                }
                app.session_chip = app.session_ui.title.clone();
                if !app.keep.restored_conversation {
                    if let Err(error) = app
                        .deleg
                        .d
                        .restore_conversation_identity(&saved.conversation)
                    {
                        app.push_system(error);
                    }
                }
                app.prior_provider = app.provider;
                app.prior_home = app.continuity_home();
                app.workspace.set_native_windows(native);
                if saved.home_open && !checkpoint {
                    landing::open_home_tab(&mut app);
                }
                if !valid && !saved.account.is_empty() {
                    app.push_system("This tab's account is unavailable. Its conversation view and draft were kept; select an account before sending.");
                }
                if valid
                    && (checkpoint
                        || saved.omitted && app.native_session_id.is_some()
                        || !app.messages.iter().any(|message| {
                            matches!(
                                message.role,
                                MsgRole::User
                                    | MsgRole::Assistant
                                    | MsgRole::Card
                                    | MsgRole::Thinking
                            )
                        }))
                {
                    let session = app.native_session_id.clone().or_else(|| {
                        app.native_sources
                            .last()
                            .filter(|source| {
                                source.provider == app.provider
                                    && source.account == app.account
                                    && app
                                        .home_path
                                        .as_ref()
                                        .is_some_and(|home| same_home(home, &source.home))
                            })
                            .map(|source| source.session_id.clone())
                    });
                    if let Some(session) = session {
                        app.reload_restored_history(&session);
                    }
                }
                let mut tab = RunTab::new(app);
                tab.id = uuid::Uuid::parse_str(&saved.id).expect("validated tab identity");
                tab.home_origin = saved
                    .home_origin
                    .and_then(|id| uuid::Uuid::parse_str(&id).ok());
                if let Some(existing) = restored
                    .iter()
                    .find(|existing: &&RunTab| same_target(&existing.app, &tab.app))
                {
                    if original.id == selected_id {
                        selected_id = existing.id.to_string();
                    }
                    duplicate_records.push(saved.recovery);
                    owner.retained.push(original);
                    // Original layout, checkpoint, draft, queue and display snapshot remain
                    // untouched. Dropping this temporary App releases its recovery lease.
                } else if let Err(error) = tab.acquire_chat_ownership() {
                    duplicate_records.push(format!("{} ({error})", saved.recovery));
                    owner.retained.push(original);
                } else {
                    restored.push(tab);
                }
            }
            if restored.is_empty() {
                restored.push(fresh_home(root, cwd, native,
                    "Saved chats are already open in another window or their ownership is unavailable. Their snapshots and recovery records were kept for the next available tab."));
            }
            let active = restored
                .iter()
                .position(|tab| tab.id.to_string() == selected_id)
                .unwrap_or(0);
            if !duplicate_records.is_empty() {
                restored[active].app.push_system(format!("Duplicate saved conversations were retained: {}. Their complete distinct snapshots remain in the canonical window record.", duplicate_records.join(", ")));
            }
            append_orphan_requests(
                root,
                cwd,
                native,
                &recorded,
                &mut restored,
                active,
                MAX_TABS.saturating_sub(owner.retained.len()),
            );
            (
                Some(owner),
                tabs::TabSet {
                    tabs: restored,
                    active,
                    closed: Vec::new(),
                },
            )
        }
        result => {
            let (mut owner, warning) = match result {
                Ok((owner, None)) => (Some(owner), None),
                Err(error) => (None, Some(error.to_string())),
                _ => unreachable!(),
            };
            let mut app = App::new(root.into(), cwd.into());
            app.refresh_binding();
            app.restore_state();
            // Pre-layout builds saved unfinished source bindings. Restore that exact history,
            // keeping the request paused, instead of showing an empty Home recovery screen.
            let session = app.native_sources.last().cloned();
            if let Some(source) = session {
                let registered=get_account(root,&source.account).is_some_and(|account|
                    account.provider==source.provider && account.home_path.as_ref().map(PathBuf::from).is_some_and(|home| home==source.home || matches!((std::fs::canonicalize(&home),std::fs::canonicalize(&source.home)),(Ok(a),Ok(b)) if a==b)));
                if registered {
                    app.provider = source.provider;
                    app.account = source.account;
                    app.home_path = get_account(root, &app.account)
                        .and_then(|account| account.home_path.map(PathBuf::from));
                    app.refresh_account_email();
                    app.refresh_usage();
                    app.reload_restored_history(&source.session_id);
                } else {
                    app.push_system("The recovered conversation's account is unavailable; sources and draft were kept.");
                }
            } else if !app.continuation.waiting() {
                landing::open(&mut app);
            }
            if let Some(warning) = warning {
                landing::close(&mut app);
                app.push_system(format!(
                    "Open-tab restoration unavailable: {warning}. Saved work remains retained."
                ));
            }
            app.prior_provider = app.provider;
            app.prior_home = app.continuity_home();
            app.workspace.set_native_windows(native);
            let mut tab = RunTab::new(app);
            if let Err(error) = tab.acquire_chat_ownership() {
                if let Some(owner) = owner.as_mut() {
                    match SavedTab::capture(&tab, 1024 * 1024) {
                        Ok(snapshot) => owner.retained.push(snapshot),
                        Err(error) => crate::logging::info(&format!(
                            "Blocked restored chat snapshot unavailable: {error}; recovery kept"
                        )),
                    }
                }
                drop(tab);
                tab = fresh_home(root, cwd, native, &format!("Recovered chat is already open or ownership is unavailable: {error}. No request was replayed; saved work remains retained."));
            }
            (owner, tabs::TabSet::new(tab))
        }
    }
}

/// A fresh recovery record must not auto-claim a blocked conversation again.
fn fresh_home(root: &Path, cwd: &Path, native: bool, notice: &str) -> RunTab {
    let mut app = super::new_tab_app(root, cwd, native);
    landing::open_home_tab(&mut app);
    app.push_system(notice);
    let mut tab = RunTab::new(app);
    if let Err(error) = tab.acquire_chat_ownership() {
        tab.app.account.clear();
        tab.app.home_path = None;
        tab.app.push_system(format!(
            "New chat ownership unavailable; sending remains blocked: {error}"
        ));
    }
    tab
}

/// Compare owned conversation/native targets, not the focused tab or latest file.
fn same_target(left: &App, right: &App) -> bool {
    if left.deleg.d.conversation_id() == right.deleg.d.conversation_id() {
        return true;
    }
    matches!((super::super::chat_ownership::Key::for_app(left), super::super::chat_ownership::Key::for_app(right)),
        (Some(a), Some(b)) if a == b)
}

fn bind_recovered_source(app: &mut App) -> bool {
    let Some(source) = app.native_sources.last().cloned() else {
        return false;
    };
    let target = get_account(&app.state_root, &source.account).filter(|account| {
        account.provider == source.provider
            && account
                .home_path
                .as_ref()
                .is_some_and(|home| same_home(home, &source.home))
    });
    let Some(target) = target else {
        app.account.clear();
        app.home_path = None;
        app.push_system("The recovered request's account is unavailable; its original prompt and sources remain paused.");
        return false;
    };
    app.provider = source.provider;
    app.account = source.account;
    app.home_path = target.home_path;
    app.refresh_account_email();
    app.refresh_usage();
    true
}

fn append_orphan_requests(
    root: &Path,
    cwd: &Path,
    native: bool,
    recorded: &std::collections::HashSet<String>,
    tabs: &mut Vec<RunTab>,
    active: usize,
    tab_limit: usize,
) {
    let Ok(recovery_root) = super::super::recovery::recovery_root(root, cwd) else {
        return;
    };
    let choices = match super::super::recovery::available(&recovery_root) {
        Ok(choices) => choices,
        Err(error) => {
            tabs[active].app.push_system(format!("Unfinished request discovery failed: {error}; existing recovery records were kept."));
            return;
        }
    };
    let mut appended = 0;
    let mut inspected = 0;
    let mut remaining = Vec::new();
    for choice in choices {
        if recorded.contains(&choice.id) {
            remaining.push(choice);
            continue;
        }
        if inspected >= MAX_TABS {
            remaining.push(choice);
            continue;
        }
        inspected += 1;
        if appended >= MAX_STARTUP_RECOVERIES || tabs.len() >= tab_limit {
            remaining.push(choice);
            continue;
        }
        let mut app = App::new(root.into(), cwd.into());
        app.refresh_binding();
        app.restore_state_for_record(Some(&choice.id));
        if app
            .keep
            .lease
            .as_ref()
            .is_none_or(|lease| lease.record_id() != choice.id)
        {
            remaining.push(choice);
            continue;
        }
        let bound = bind_recovered_source(&mut app);
        if tabs.iter().any(|tab| same_target(&tab.app, &app)) {
            remaining.push(choice);
            continue;
        }
        app.queue.paused = true;
        app.queue.dispatch = super::super::queue::Dispatch::Idle;
        app.prior_provider = app.provider;
        app.prior_home = app.continuity_home();
        app.workspace.set_native_windows(native);
        if bound {
            if let Some(session) = app
                .native_sources
                .last()
                .map(|source| source.session_id.clone())
            {
                app.reload_restored_history(&session);
            }
        }
        app.push_system("Restored saved draft and queued work in a separate tab; it remains paused until you resume it.");
        let mut tab = RunTab::new(app);
        if let Err(error) = tab.acquire_chat_ownership() {
            tabs[active].app.push_system(format!(
                "Saved work {} remains retained because its tab could not be opened: {error}",
                choice.id
            ));
            remaining.push(choice);
            continue;
        }
        tabs.push(tab);
        appended += 1;
    }
    if appended > 0 {
        tabs[active].app.push_system(format!(
            "Restored {appended} saved work tab(s), paused; the selected tab is unchanged."
        ));
    }
    if !remaining.is_empty() {
        tabs[active].app.keep.recovery_choices = remaining;
    }
}

impl Owner {
    pub(super) fn save(&mut self, tabs: &tabs::TabSet<RunTab>, force: bool) -> io::Result<()> {
        if !force && self.last_save.elapsed() < Duration::from_secs(1) {
            return Ok(());
        }
        self.last_save = Instant::now();
        if tabs.tabs.len().saturating_add(self.retained.len()) > MAX_TABS {
            return Err(io::Error::other(
                "too many open tabs to checkpoint; previous layout kept",
            ));
        }
        let budget = (8 * 1024 * 1024 / tabs.tabs.len().max(1)).min(1024 * 1024);
        let saved = Window {
            version: 2,
            retained: self.retained.clone(),
            workspace: self.workspace.clone(),
            active: tabs.active,
            tabs: tabs
                .tabs
                .iter()
                .map(|tab| SavedTab::capture(tab, budget))
                .collect::<io::Result<_>>()?,
        };
        let bytes = super::super::recovery::bounded_json(&saved, MAX_BYTES)?;
        let bytes = crate::redact::redact_bytes(&bytes);
        if force || bytes != self.previous {
            crate::state_io::write_atomic(&self.path, &bytes)?;
            self.previous = bytes;
        }
        Ok(())
    }
}

impl SavedTab {
    fn capture(tab: &RunTab, budget: usize) -> io::Result<Self> {
        let app = &tab.app;
        let recovery = app
            .keep
            .lease
            .as_ref()
            .ok_or_else(|| {
                io::Error::other("a tab has no recovery ownership; previous layout kept")
            })?
            .record_id();
        let mut messages = Vec::new();
        let mut used = 0usize;
        let mut omitted = app
            .messages
            .iter()
            .any(|message| message.role == MsgRole::System && message.text == OMITTED_NOTICE);
        for message in app.messages.iter().rev() {
            if message.role == MsgRole::System {
                continue;
            }
            let size = message.text.len()
                + message.card.as_ref().map_or(0, |card| {
                    card.id.len() + card.title.len() + card.detail.as_ref().map_or(0, String::len)
                });
            if messages.len() >= 1000 || used.saturating_add(size) > budget {
                omitted = true;
                break;
            }
            used += size;
            messages.push(SavedLine::capture(message));
        }
        messages.reverse();
        // Live text has not yet become a message when the window is closed mid-turn.
        if !app.live_text.is_empty() && used.saturating_add(app.live_text.len()) <= budget {
            messages.push(SavedLine {
                role: "assistant".into(),
                text: app.live_text.clone(),
                card: None,
                meta: Some(app.reply_meta()),
            });
        } else if !app.live_text.is_empty() {
            omitted = true;
        }
        Ok(Self {
            id: tab.id.to_string(),
            home_origin: tab.home_origin.map(|id| id.to_string()),
            recovery,
            provider: app.provider,
            account: app.account.clone(),
            home: app
                .home_path
                .as_ref()
                .map(|home| std::fs::canonicalize(home).unwrap_or_else(|_| home.clone())),
            model: app.model.clone(),
            session: app.native_session_id.clone(),
            sources: app.native_sources.clone(),
            title: app.session_ui.title.clone(),
            conversation: app.deleg.d.conversation_id().into(),
            home_open: app.landing.is_some(),
            messages,
            omitted,
            scroll: app.view.scroll,
        })
    }
}

impl SavedLine {
    fn capture(line: &ChatLine) -> Self {
        let role = match line.role {
            MsgRole::User => "user",
            MsgRole::Assistant => "assistant",
            MsgRole::System => "system",
            MsgRole::Command => "command",
            MsgRole::Card => "card",
            MsgRole::Thinking => "thinking",
            MsgRole::Meta => "meta",
        };
        Self {
            role: role.into(),
            text: line.text.clone(),
            meta: line.meta.clone(),
            card: line.card.as_ref().map(|card| SavedCard {
                id: card.id.clone(),
                title: card.title.clone(),
                detail: card.detail.clone(),
                status: match card.status {
                    CardStatus::Running => 0,
                    CardStatus::Done => 1,
                    CardStatus::Error => 2,
                },
            }),
        }
    }
    fn restore(self) -> ChatLine {
        ChatLine {
            role: match self.role.as_str() {
                "user" => MsgRole::User,
                "assistant" => MsgRole::Assistant,
                "system" => MsgRole::System,
                "command" => MsgRole::Command,
                "card" => MsgRole::Card,
                "thinking" => MsgRole::Thinking,
                _ => MsgRole::Meta,
            },
            text: self.text,
            meta: self.meta,
            card: self.card.map(|card| StreamCard {
                id: card.id,
                title: card.title,
                detail: card.detail,
                status: match card.status {
                    1 => CardStatus::Done,
                    2 => CardStatus::Error,
                    _ => CardStatus::Error,
                },
            }),
        }
    }
}
