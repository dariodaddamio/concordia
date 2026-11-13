//! The landing page: recent chats grouped by project folder and each project's open work as its
//! Harness ledger reports it. It replaces the empty transcript until
//! the person types, resumes a chat or presses Esc.

use super::{App, MsgRole, FOOTER_HINT};
use crate::sessions::{format_relative_time, list_recent_sessions, SessionSummary};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

/// How many recent chats the page lists, and how many projects it asks for open work.
const RECENT_CHATS: usize = 40;
const WORK_PROJECTS: usize = 6;
/// Below this width the work pane is left out and the chats use the whole row.
const TWO_PANES_MIN: u16 = 70;

#[derive(Debug, Clone)]
pub struct ChatGroup {
    pub folder: String,
    pub sessions: Vec<SessionSummary>,
    pub expanded: bool,
    pub quick_questions: bool,
    pub quick_sessions: Vec<SessionSummary>,
}

#[derive(Debug, Clone)]
pub(crate) struct LandingMeter {
    pub(crate) provider: crate::store::ProviderKind,
    pub(crate) account: String,
    pub(crate) snapshot: crate::usage::UsageSnapshot,
}

/// Sessions grouped by the folder they ran in: the current folder first and expanded, the rest
/// collapsed and ordered by their newest chat.
pub fn group_sessions(sessions: Vec<SessionSummary>, cwd: &Path) -> Vec<ChatGroup> {
    let current = display_path(&project_root(cwd));
    let probe = crate::cursor_usage::probe_dir();
    let mut groups: BTreeMap<PathBuf, Vec<SessionSummary>> = BTreeMap::new();
    for session in sessions {
        let folder = match session.cwd.as_deref().map(Path::new) {
            Some(path) if path == probe => continue,
            Some(path) => display_path(&project_root(path)),
            None => PathBuf::from("other"),
        };
        groups.entry(folder).or_default().push(session);
    }
    let name = |path: &Path| {
        path.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "other".into())
    };
    let mut names = BTreeMap::<String, usize>::new();
    for root in groups.keys() {
        *names.entry(name(root)).or_default() += 1;
    }
    let mut groups: Vec<_> = groups
        .into_iter()
        .map(|(root, mut sessions)| {
            sessions.sort_by_key(|s| std::cmp::Reverse(s.modified_ms));
            let expanded = current == root;
            let folder = name(&root);
            let folder = if names[&folder] > 1 {
                let parent = root
                    .parent()
                    .and_then(Path::file_name)
                    .map(|p| p.to_string_lossy());
                format!("{}/{}", parent.as_deref().unwrap_or("?"), folder)
            } else {
                folder
            };
            ChatGroup {
                folder,
                sessions,
                expanded,
                quick_questions: false,
                quick_sessions: Vec::new(),
            }
        })
        .collect();
    groups.sort_by(|a, b| {
        b.expanded.cmp(&a.expanded).then_with(|| {
            let newest = |g: &ChatGroup| {
                g.sessions
                    .first()
                    .or(g.quick_sessions.first())
                    .map(|s| s.modified_ms)
                    .unwrap_or(0)
            };
            newest(b).cmp(&newest(a))
        })
    });
    groups
}

fn display_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else {
        PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
    }
}

fn hidden_workspace(path: &Path, temp_root: Option<&Path>) -> bool {
    let norm = |p: &Path| {
        display_path(p)
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase()
    };
    let text = norm(path);
    if let Some(temp_root) = temp_root {
        let temp = norm(temp_root);
        if text == temp || text.starts_with(&(temp.clone() + "\\")) {
            return true;
        }
    }
    text.split('\\')
        .filter(|part| !part.is_empty())
        .any(|part| {
            part == "target"
                || part.starts_with("concordia-test")
                || part.starts_with("concordia-qual")
                || part == "live-qualification-20261104"
        })
}

fn landing_sessions(
    sessions: Vec<SessionSummary>,
    cwd: &Path,
    show_all: bool,
    temp_root: Option<&Path>,
) -> Vec<SessionSummary> {
    sessions
        .into_iter()
        .filter(|session| show_all || !hidden_session(session, cwd, temp_root))
        .collect()
}

fn hidden_session(session: &SessionSummary, cwd: &Path, temp_root: Option<&Path>) -> bool {
    session.cwd.as_deref().is_some_and(|session_cwd| {
        let path = Path::new(session_cwd);
        hidden_workspace(path, temp_root) || {
            let folder = display_path(&project_root(path));
            folder != display_path(&project_root(cwd)) && !folder.exists()
        }
    })
}

fn hidden_count(sessions: &[SessionSummary], cwd: &Path, temp_root: Option<&Path>) -> usize {
    sessions
        .iter()
        .filter(|s| hidden_session(s, cwd, temp_root))
        .count()
}

fn display_groups_at(sessions: Vec<SessionSummary>, cwd: &Path, now_ms: u128) -> Vec<ChatGroup> {
    let mut groups = group_sessions(sessions, cwd);
    let mut result = Vec::new();
    for mut group in groups.drain(..) {
        let (quick, recent): (Vec<_>, Vec<_>) = group.sessions.into_iter().partition(|session| {
            session.user_messages <= 2 && now_ms.saturating_sub(session.modified_ms) >= 86_400_000
        });
        group.sessions = recent;
        group.quick_sessions = quick;
        if !group.sessions.is_empty() || !group.quick_sessions.is_empty() {
            result.push(group);
        }
    }
    result
}

fn display_groups(sessions: Vec<SessionSummary>, cwd: &Path) -> Vec<ChatGroup> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    display_groups_at(sessions, cwd, now_ms)
}

/// The project identity, or for a git worktree its main repository: a worktree's
/// `.git` is a file reading `gitdir: <repo>/.git/worktrees/<name>`.
fn project_root(folder: &Path) -> PathBuf {
    let pointer = folder.join(".git");
    let main = std::fs::symlink_metadata(&pointer)
        .ok()
        .filter(|metadata| metadata.is_file())
        .and_then(|_| std::fs::File::open(&pointer).ok())
        .and_then(|file| {
            use std::io::Read;
            let mut text = String::new();
            file.take(4097).read_to_string(&mut text).ok()?;
            if text.len() > 4096 {
                return None;
            }
            let gitdir = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
            let gitdir = if gitdir.is_absolute() { gitdir } else { folder.join(gitdir) };
            let dot_git = gitdir.parent()?.parent()?;
            (dot_git.file_name()? == ".git").then(|| dot_git.parent().map(Path::to_path_buf))?
        });
    let root = main.as_deref().unwrap_or(folder);
    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

/// One project's open work. `None` when its Harness predates `plan:open --json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectWork {
    pub folder: String,
    pub items: Option<Vec<WorkItem>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    pub text: String,
    /// Parked for later, as opposed to in progress.
    pub queued: bool,
}

/// `harness run plan:open --json` output: active plans first, then the open and queued rows.
pub fn parse_open_work(stdout: &str) -> Option<Vec<WorkItem>> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    let text = |v: &serde_json::Value, key: &str| {
        v.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let mut items = Vec::new();
    for plan in value.get("plans")?.as_array()? {
        if let Some(slice) = text(plan, "slice") {
            items.push(WorkItem {
                text: slice,
                queued: false,
            });
        }
    }
    for row in value.get("rows")?.as_array()? {
        if let Some(item) = text(row, "item") {
            let queued = text(row, "status").is_some_and(|s| s.starts_with("queued"));
            items.push(WorkItem { text: item, queued });
        }
    }
    Some(items)
}

/// The connected Harness program (`/harness connect harness`), if any.
fn harness_program(state_root: &Path) -> Option<String> {
    crate::harness::connections::load(state_root)
        .ok()?
        .into_iter()
        .find_map(|c| {
            let program = c.manifest_path.strip_prefix("command:")?.to_string();
            crate::harness::adapter::is_harness_program(&program).then_some(program)
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Chats,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Browse,
    AddGoal(String),
    ConfirmDone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatRow {
    Group(usize),
    Chat(usize, usize),
    Quick(usize),
    QuickChat(usize, usize),
}

pub struct Landing {
    welcome: bool,
    provider_health: Vec<(crate::store::ProviderKind, bool)>,
    from_chat: bool,
    all_sessions: Vec<SessionSummary>,
    show_all: bool,
    pub(crate) hidden_temp_root: Option<PathBuf>,
    groups: Vec<ChatGroup>,
    goals: Vec<String>,
    pane: Pane,
    mode: Mode,
    chat_sel: usize,
    goal_sel: usize,
    chat_top: usize,
    goal_top: usize,
    goal_follow: bool,
    goal_hits: Vec<Option<usize>>,
    pub(crate) work: Vec<ProjectWork>,
    work_rx: Option<mpsc::Receiver<ProjectWork>>,
    /// Where the rows were drawn, for the mouse.
    chats_area: Rect,
    work_area: Rect,
    goals_area: Rect,
    last_click: Option<(usize, Instant)>,
    /// Tokens through Concordia in the last 24 hours, read once when the page opens.
    tokens_24h: u64,
    tokens_all_time: u64,
    tokens_checked: Instant,
    tokens_generation: Option<u64>,
    tokens_ready: bool,
    tokens_partial: bool,
    tokens_error: Option<String>,
    pub(crate) meters: Vec<LandingMeter>,
}

impl Landing {
    fn rows(&self) -> Vec<ChatRow> {
        let mut rows = Vec::new();
        for (g, group) in self.groups.iter().enumerate() {
            rows.push(ChatRow::Group(g));
            if group.expanded {
                rows.extend((0..group.sessions.len()).map(|s| ChatRow::Chat(g, s)));
                if !group.quick_sessions.is_empty() {
                    rows.push(ChatRow::Quick(g));
                    if group.quick_questions {
                        rows.extend(
                            (0..group.quick_sessions.len()).map(|s| ChatRow::QuickChat(g, s)),
                        );
                    }
                }
            }
        }
        rows
    }

    /// Open work is still being read.
    #[cfg(test)]
    pub fn work_pending(&self) -> bool {
        self.work_rx.is_some()
    }

    fn hint(&self) -> &'static str {
        "Enter open chat · Esc chat"
    }
}

/// Show the landing page for this folder; with no recorded chats there is nothing to show.
pub(super) fn open(app: &mut App) {
    let sessions = list_recent_sessions(&app.state_root, RECENT_CHATS);
    let program = harness_program(&app.state_root);
    open_with(app, sessions, program);
}

pub(super) fn open_with(app: &mut App, sessions: Vec<SessionSummary>, program: Option<String>) {
    open_with_mode(app, sessions, program, false);
}

#[cfg(test)]
pub(crate) fn set_hidden_temp_root(app: &mut App, root: PathBuf) {
    if let Some(landing) = app.landing.as_mut() {
        landing.hidden_temp_root = Some(root);
        landing.groups = display_groups(
            landing_sessions(
                landing.all_sessions.clone(),
                &app.cwd,
                landing.show_all,
                landing.hidden_temp_root.as_deref(),
            ),
            &app.cwd,
        );
    }
}

pub(super) fn open_home(app: &mut App) {
    app.tab_request = Some(super::tabs::TabAction::Home);
}

pub(super) fn back_to_chat(app: &mut App) {
    app.tab_request = Some(super::tabs::TabAction::BackToChat);
}

/// An unbound conversation can host Home without replacing a saved or running chat.
pub(super) fn can_host_home(app: &App) -> bool {
    !app.busy
        && app.native_session_id.is_none()
        && !app
            .messages
            .iter()
            .any(|message| matches!(message.role, MsgRole::User | MsgRole::Assistant))
}

pub(super) fn open_home_tab(app: &mut App) {
    let sessions = list_recent_sessions(&app.state_root, RECENT_CHATS);
    let program = harness_program(&app.state_root);
    open_with_mode(app, sessions, program, true);
}

fn open_with_mode(
    app: &mut App,
    sessions: Vec<SessionSummary>,
    program: Option<String>,
    from_chat: bool,
) {
    app.refresh_store_warning();
    let welcome = sessions.is_empty()
        && app.store_warning.is_none()
        && crate::store::load_store(&app.state_root)
            .accounts
            .is_empty();
    if sessions.is_empty() && !from_chat && !welcome {
        app.landing = None;
        return;
    }
    let hidden_temp_root = if cfg!(test) {
        None
    } else {
        Some(std::env::temp_dir())
    };
    let groups = display_groups(
        landing_sessions(
            sessions.clone(),
            &app.cwd,
            false,
            hidden_temp_root.as_deref(),
        ),
        &app.cwd,
    );
    let work_rx = program.map(|program| {
        let mut projects: Vec<PathBuf> = Vec::new();
        for group in &groups {
            let Some(cwd) = group.sessions.iter().find_map(|s| s.cwd.as_deref()) else {
                continue;
            };
            let path = PathBuf::from(cwd);
            if path.join("docs/plans/active.md").is_file() && !projects.contains(&path) {
                projects.push(path);
            }
        }
        projects.truncate(WORK_PROJECTS);
        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("concordia-open-work".into())
            .spawn(move || {
                for project in projects {
                    let argv = [
                        program.clone(),
                        "run".into(),
                        "plan:open".into(),
                        "--json".into(),
                    ];
                    let outcome = crate::harness::run::run_command(
                        &argv,
                        &project,
                        crate::harness::run::DEFAULT_TIMEOUT,
                    );
                    let folder = project
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let items = parse_open_work(&outcome.stdout);
                    if tx.send(ProjectWork { folder, items }).is_err() {
                        return;
                    }
                }
            });
        rx
    });
    let landing = Landing {
        welcome,
        provider_health: crate::doctor::fast_provider_status(),
        from_chat,
        all_sessions: sessions,
        show_all: false,
        hidden_temp_root,
        groups,
        goals: Vec::new(),
        pane: Pane::Chats,
        mode: Mode::Browse,
        chat_sel: 0,
        goal_sel: 0,
        chat_top: 0,
        goal_top: 0,
        goal_follow: true,
        goal_hits: Vec::new(),
        work: Vec::new(),
        work_rx,
        chats_area: Rect::default(),
        work_area: Rect::default(),
        goals_area: Rect::default(),
        last_click: None,
        tokens_24h: 0,
        tokens_all_time: 0,
        tokens_checked: Instant::now(),
        tokens_generation: None,
        tokens_ready: false,
        tokens_partial: false,
        tokens_error: None,
        meters: Vec::new(),
    };
    app.footer_left = landing.hint().into();
    app.landing = Some(landing);
    refresh_meters(app);
}

fn refresh_meters(app: &mut App) {
    let accounts = crate::accounts::accounts_with_auth(&app.state_root);
    let meters = accounts
        .into_iter()
        .filter_map(|account| {
            let home = account.home_path?;
            Some(LandingMeter {
                provider: account.provider,
                account: account.account.clone(),
                snapshot: app
                    .usage_fetcher
                    .snapshot(account.provider, &account.account, &home),
            })
        })
        .collect();
    if let Some(landing) = app.landing.as_mut() {
        landing.meters = meters;
    }
}

pub(super) fn close(app: &mut App) {
    if app.landing.take().is_some() {
        app.footer_left = FOOTER_HINT.into();
    }
}

/// Drafting keeps the landing page visible.
pub(super) fn handle_paste(app: &mut App, text: &str) -> bool {
    if !visible(app) || text.is_empty() {
        return false;
    }
    let _ = text;
    false
}

/// On screen: opened, nothing said in this chat yet, and no picker over it.
pub(super) fn visible(app: &App) -> bool {
    app.landing.is_some()
        && app.picker.is_none()
        && (app
            .landing
            .as_ref()
            .is_some_and(|landing| landing.from_chat)
            || (!app.continuation.waiting()
                && !app.keep.turn_recovery_notice
                && !app.deleg.has_recovery_notice()
                && !app
                    .messages
                    .iter()
                    .any(|m| matches!(m.role, MsgRole::User | MsgRole::Assistant))))
}

pub(super) fn editing_goal(_app: &App) -> bool {
    false
}

/// The landing usage pane owns subscription facts whenever it is actually drawn.
pub(super) fn usage_box_visible(app: &App, width: u16) -> bool {
    visible(app)
        && app.landing.as_ref().is_some_and(|landing| {
            !landing.meters.is_empty() && width >= TWO_PANES_MIN
        })
}

/// Collect open work that arrived from the background reader.
pub(super) fn poll(app: &mut App) -> bool {
    if app.landing.is_none() {
        return false;
    }
    refresh_meters(app);
    let Some(landing) = app.landing.as_mut() else {
        return false;
    };
    let Some(rx) = landing.work_rx.as_ref() else {
        return false;
    };
    let mut changed = false;
    loop {
        match rx.try_recv() {
            Ok(work) => {
                landing.work.push(work);
                changed = true;
            }
            Err(mpsc::TryRecvError::Empty) => return changed,
            Err(mpsc::TryRecvError::Disconnected) => {
                landing.work_rx = None;
                return true;
            }
        }
    }
}

fn activate_chat(app: &mut App) {
    let Some(landing) = app.landing.as_mut() else {
        return;
    };
    match landing.rows().get(landing.chat_sel).copied() {
        Some(ChatRow::Group(g)) => landing.groups[g].expanded = !landing.groups[g].expanded,
        Some(ChatRow::Chat(g, s)) => {
            let session = landing.groups[g].sessions[s].clone();
            close(app);
            app.request_resume_session(&session.account, &session.session_id);
        }
        Some(ChatRow::Quick(g)) => {
            landing.groups[g].quick_questions = !landing.groups[g].quick_questions
        }
        Some(ChatRow::QuickChat(g, s)) => {
            let session = landing.groups[g].quick_sessions[s].clone();
            close(app);
            app.request_resume_session(&session.account, &session.session_id);
        }
        None => {}
    }
}

fn set_pane(app: &mut App, pane: Pane) {
    if let Some(landing) = app.landing.as_mut() {
        landing.pane = pane;
        app.footer_left = landing.hint().into();
    }
}

/// Keys while the page shows. `false` leaves the key to the rest of the interface (Ctrl
/// shortcuts, and a printable key after it has closed the page and joined the input).
pub(super) fn navigation_key(app: &mut App, key: KeyEvent) -> bool {
    if app.picker.is_some()
        || app.workspace.focused.is_some()
        || app.queue.focused
        || app.queue.edit.is_some()
        || app.activity.detail.is_some()
        || app.questions.focused
        || editing_goal(app)
    {
        return false;
    }
    let plain = key.modifiers.is_empty() && app.editor.text().is_empty();
    let navigation = key.modifiers == KeyModifiers::ALT;
    if !plain && !navigation {
        return false;
    }
    match key.code {
        KeyCode::Left => {
            if !visible(app) {
                open_home(app);
            }
            true
        }
        KeyCode::Right if visible(app) => {
            back_to_chat(app);
            true
        }
        _ => false,
    }
}

pub(super) fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    // Slash completion owns its keys even while the landing lists stay visible.
    if app
        .landing
        .as_ref()
        .is_some_and(|l| l.mode == Mode::Browse && l.pane == Pane::Chats)
        && app.editor.text().starts_with('/')
        && !app.palette_entries().is_empty()
    {
        return false;
    }

    let Some(landing) = app.landing.as_mut() else {
        return false;
    };
    let plain = !key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    // A nonempty draft owns editing and Enter; Up/Down still
    // move through the chats while the draft is one line.
    let list_key = matches!(key.code, KeyCode::Up | KeyCode::Down)
        && plain
        && !app.editor.text().contains('\n');
    if !app.editor.text().is_empty()
        && !list_key
        && !matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Esc)
        && landing.pane == Pane::Chats
    {
        return false;
    }
    if key.code == KeyCode::Enter
        && landing.welcome
        && crate::store::load_store(&app.state_root)
            .accounts
            .is_empty()
        && landing.pane == Pane::Chats
    {
        app.open_use_picker();
        return true;
    }
    let rows = landing.rows().len();
    match (key.code, landing.pane) {
        (KeyCode::Esc, _) => back_to_chat(app),
        (KeyCode::Up, Pane::Chats) => landing.chat_sel = previous(landing.chat_sel, rows),
        (KeyCode::Down, Pane::Chats) => landing.chat_sel = next(landing.chat_sel, rows),
        (KeyCode::Enter, Pane::Chats) => activate_chat(app),
        (KeyCode::Char(' '), Pane::Chats)
            if matches!(
                landing.rows().get(landing.chat_sel),
                Some(ChatRow::Group(_))
            ) =>
        {
            activate_chat(app)
        }
        (KeyCode::Char('h'), Pane::Chats)
            if plain
                && landing.all_sessions.len()
                    != landing_sessions(
                        landing.all_sessions.clone(),
                        &app.cwd,
                        false,
                        landing.hidden_temp_root.as_deref(),
                    )
                    .len() =>
        {
            landing.show_all = !landing.show_all;
            landing.groups = display_groups(
                landing_sessions(
                    landing.all_sessions.clone(),
                    &app.cwd,
                    landing.show_all,
                    landing.hidden_temp_root.as_deref(),
                ),
                &app.cwd,
            );
            landing.chat_sel = landing.chat_sel.min(landing.rows().len().saturating_sub(1));
            landing.chat_top = 0;
            app.footer_left = landing.hint().into();
        }
        (KeyCode::Char(_), _) if plain => {
            // Drafting keeps the landing page visible.
            set_pane(app, Pane::Chats);
            return false;
        }
        _ => return false,
    }
    true
}

fn previous(selected: usize, count: usize) -> usize {
    if selected == 0 {
        count.saturating_sub(1)
    } else {
        selected - 1
    }
}

fn next(selected: usize, count: usize) -> usize {
    if count == 0 || selected + 1 >= count {
        0
    } else {
        selected + 1
    }
}

pub(super) fn clickable(app: &App, x: u16, y: u16) -> bool {
    if let Some(landing) = app.landing.as_ref() {
        let label = landing.pane == Pane::Chats
            && hidden_count(
                &landing.all_sessions,
                &app.cwd,
                landing.hidden_temp_root.as_deref(),
            ) > 0
            && app.footer_left.find(" · h").is_some_and(|byte_start| {
                let start = app.footer_left[..byte_start].chars().count() as u16;
                y == app.areas.footer.y && x >= start && x < app.footer_left.chars().count() as u16
            });
        if label {
            return true;
        }
    }
    app.landing.as_ref().is_some_and(|l| {
        (super::contains(l.chats_area, x, y)
            && l.chat_top + ((y - l.chats_area.y) as usize) < l.rows().len())
            || super::contains(l.work_area, x, y)
    })
}

/// Wheel and clicks while the page shows; `true` when the page used the event.
pub(super) fn handle_mouse(app: &mut App, m: MouseEvent) -> bool {
    let Some(landing) = app.landing.as_mut() else {
        return false;
    };
    let inside = |r: Rect| {
        m.column >= r.x && m.column < r.x + r.width && m.row >= r.y && m.row < r.y + r.height
    };
    let footer_label = landing.pane == Pane::Chats
        && hidden_count(
            &landing.all_sessions,
            &app.cwd,
            landing.hidden_temp_root.as_deref(),
        ) > 0
        && inside(app.areas.footer)
        && m.row == app.areas.footer.y
        && app.footer_left.find(" · h").is_some_and(|byte_start| {
            let start = app.footer_left[..byte_start].chars().count() as u16;
            m.column >= start && m.column < app.footer_left.chars().count() as u16
        });
    if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) && footer_label {
        landing.show_all = !landing.show_all;
        landing.groups = display_groups(
            landing_sessions(
                landing.all_sessions.clone(),
                &app.cwd,
                landing.show_all,
                landing.hidden_temp_root.as_deref(),
            ),
            &app.cwd,
        );
        landing.chat_sel = landing.chat_sel.min(landing.rows().len().saturating_sub(1));
        landing.chat_top = 0;
        return true;
    }
    let rows = landing.rows().len();
    match m.kind {
        MouseEventKind::ScrollUp if inside(landing.chats_area) => {
            landing.chat_sel = landing.chat_sel.saturating_sub(3);
        }
        MouseEventKind::ScrollDown if inside(landing.chats_area) => {
            landing.chat_sel = (landing.chat_sel + 3).min(rows.saturating_sub(1));
        }
        MouseEventKind::Down(MouseButton::Left) if inside(landing.chats_area) => {
            let row = landing.chat_top + (m.row - landing.chats_area.y) as usize;
            if row >= rows {
                return true;
            }
            let again = landing
                .last_click
                .is_some_and(|(r, at)| r == row && at.elapsed().as_millis() < 500);
            landing.chat_sel = row;
            landing.last_click = Some((row, Instant::now()));
            set_pane(app, Pane::Chats);
            if again {
                activate_chat(app);
            }
        }
        _ => return false,
    }
    true
}

/// Rows the goals pane needs before wrapping: goals, the add or confirm line, and open work.
fn goal_rows(landing: &Landing, width: usize) -> usize {
    let work: usize = landing
        .work
        .iter()
        .map(|p| match &p.items {
            Some(items) if items.is_empty() => 0,
            Some(items) => {
                1 + items
                    .iter()
                    .map(|item| wrapped_rows(&format!("  ○ {}", item.text), width))
                    .sum::<usize>()
            }
            None => 2,
        })
        .sum();
    let usage = if landing.meters.is_empty() {
        0
    } else {
        meter_rows(&landing.meters, width) + 2
    };
    if work == 0 {
        work.max(3).max(usage + 5)
    } else {
        work.max(3).max(usage + 5)
    }
}

fn clipped(text: String, width: usize) -> String {
    if text.chars().count() <= width {
        return text;
    }
    if width <= 1 {
        return "…".chars().take(width).collect();
    }
    let mut result: String = text.chars().take(width - 1).collect();
    result.push('…');
    result
}

fn meter_window(window: &crate::usage::UsageWindow, cells: usize, include_reset: bool) -> String {
    let left = window.left_pct().round() as usize;
    let filled = (left * cells / 100).min(cells);
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(cells - filled));
    let reset = window
        .resets
        .as_deref()
        .map(|raw| meter_reset_label(raw, window.expired_for_ui()))
        .unwrap_or_else(|| "reset unavailable".into());
    format!(
        "{} [{bar}] {}%{}",
        window.label,
        left,
        if !include_reset || reset == "reset unavailable" {
            String::new()
        } else {
            format!("  {reset}")
        }
    )
}

fn meter_reset_label(raw: &str, expired: bool) -> String {
    if raw.starts_with("reset at ") || raw.starts_with("already reset") {
        return crate::usage::reset_phrase(raw);
    }
    if expired {
        format!("reset at {raw}")
    } else {
        format!("resets {raw}")
    }
}

/// One heading per account (plan, credits, limit state), then one row per usage window.
fn meter_lines(meter: &LandingMeter, width: usize) -> Vec<String> {
    let snapshot = &meter.snapshot;
    let mut heading = format!("{} {}", meter.provider.as_str(), meter.account);
    if let Some(plan) = snapshot.plan.as_deref().filter(|p| !p.trim().is_empty()) {
        heading.push_str(&format!(" · {plan}"));
    }
    let credits = snapshot.credits_text();
    if credits != "Credits: not reported" {
        heading.push_str(&format!(" · {credits}"));
    }
    if snapshot.limit_hit {
        heading.push_str(" · limit reached");
    }
    let mut lines = vec![clipped(heading, width)];
    let cells = if width < 40 { 6 } else { 10 };
    for window in &snapshot.windows {
        let full = format!("  {}", meter_window(window, cells, true));
        lines.push(if full.chars().count() <= width {
            full
        } else {
            clipped(format!("  {}", meter_window(window, cells, false)), width)
        });
    }
    if snapshot.windows.is_empty() {
        lines.push(clipped(format!("  {}", snapshot.summary), width));
    }
    lines
}

fn meter_rows(meters: &[LandingMeter], width: usize) -> usize {
    meters.iter().map(|m| meter_lines(m, width).len()).sum()
}

fn goal_label(_goal: &str) -> String {
    let proposed = false;
    format!(
        "{} {}{}",
        if proposed { "◇" } else { "○" },
        "",
        if proposed {
            " [MCP proposal · approved locally]"
        } else {
            ""
        }
    )
}

/// Rows a line takes once wrapped into the goals pane.
fn wrapped_rows(text: &str, width: usize) -> usize {
    wrap_goal_line(Line::from(text.to_string()), width)
        .len()
        .max(1)
}

pub(super) fn usage_generation(app: &App) -> Option<u64> {
    app.landing
        .as_ref()
        .and_then(|landing| landing.tokens_generation)
}

pub(super) fn needs_usage(app: &App) -> bool {
    app.landing.as_ref().is_some_and(|landing| {
        landing.tokens_generation.is_none()
            && (!landing.tokens_ready || landing.tokens_checked.elapsed().as_secs() >= 60)
    })
}

pub(super) fn begin_usage(app: &mut App, generation: u64) {
    if let Some(landing) = app.landing.as_mut() {
        landing.tokens_generation = Some(generation);
        landing.tokens_checked = Instant::now();
    }
}

pub(super) fn apply_usage(
    app: &mut App,
    generation: u64,
    result: Result<(u64, u64, bool), String>,
) {
    let Some(landing) = app
        .landing
        .as_mut()
        .filter(|landing| landing.tokens_generation == Some(generation))
    else {
        return;
    };
    landing.tokens_generation = None;
    landing.tokens_ready = true;
    landing.tokens_checked = Instant::now();
    match result {
        Ok((day, total, partial)) => {
            landing.tokens_24h = day;
            landing.tokens_all_time = total;
            landing.tokens_partial = partial;
            landing.tokens_error = None;
        }
        Err(error) => landing.tokens_error = Some(error),
    }
}

pub(super) fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    let area = super::brand::draw(f, area, app);
    if let Some(landing) = app.landing.as_ref() {
        app.footer_left = if landing.mode == Mode::Browse
            && landing.pane == Pane::Chats
            && !app.editor.text().is_empty()
        {
            FOOTER_HINT
        } else {
            landing.hint()
        }
        .to_string();
        if landing.pane == Pane::Chats
            && hidden_count(
                &landing.all_sessions,
                &app.cwd,
                landing.hidden_temp_root.as_deref(),
            ) > 0
        {
            let hidden = hidden_count(
                &landing.all_sessions,
                &app.cwd,
                landing.hidden_temp_root.as_deref(),
            );
            if landing.show_all {
                app.footer_left.push_str(" · h hide");
            } else {
                app.footer_left
                    .push_str(&format!(" · h show {hidden} hidden"));
            }
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let titles: Vec<Vec<String>> = app
        .landing
        .as_ref()
        .map(|l| {
            l.groups
                .iter()
                .map(|g| g.sessions.iter().map(|s| chat_title(app, s)).collect())
                .collect()
        })
        .unwrap_or_default();
    let Some(landing) = app.landing.as_mut() else {
        return;
    };
    // As tall as the longer pane's content, within the space there is.
    let welcome_visible = landing.welcome
        && crate::store::load_store(&app.state_root)
            .accounts
            .is_empty();
    let content = landing
        .rows()
        .len()
        .max(if welcome_visible { 7 } else { 0 })
        .min(u16::MAX as usize - 2) as u16;
    let area = Rect {
        height: (content + 2).clamp(3, area.height.max(3)).min(area.height),
        ..area
    };
    let narrow_goals = false;
    let panes = if area.width >= TWO_PANES_MIN {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area)
            .to_vec()
    } else {
        vec![area]
    };
    let border = |focused: bool| {
        if focused {
            Style::default()
        } else {
            Style::default().fg(Color::DarkGray)
        }
    };
    let selected = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().add_modifier(Modifier::DIM);

    // A narrow window shows the focused pane, so Tab never sends input into a hidden pane.
    if !narrow_goals {
        // Chats: groups and their chats, the selection kept in view.
        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(" chats ")
            .border_style(border(landing.pane == Pane::Chats));
        let usage = if !landing.tokens_ready {
            Some(" reading usage… ".into())
        } else if landing.tokens_error.is_some() {
            Some(" recorded usage unavailable ".into())
        } else if landing.tokens_24h > 0 || landing.tokens_all_time > 0 || landing.tokens_partial {
            Some(format!(
                " {} {} / {} tokens in 24 hours{} ",
                super::token_line::compact(landing.tokens_all_time),
                if landing.tokens_partial {
                    "recorded"
                } else {
                    "all time"
                },
                super::token_line::compact(landing.tokens_24h),
                if landing.tokens_partial {
                    " · partial"
                } else {
                    ""
                }
            ))
        } else {
            None
        };
        if let Some(usage) = usage {
            block = block.title_bottom(Line::styled(usage, dim).right_aligned());
        }
        let inner = block.inner(panes[0]);
        f.render_widget(block, panes[0]);
        landing.chats_area = inner;
        if welcome_visible {
            let mut lines = vec![
                Line::styled("Welcome to Concordia", selected),
                Line::from(
                    "Concordia keeps your provider chats, accounts and continuity together.",
                ),
                Line::from("Local CLI health check (no network or provider turn):"),
            ];
            for (provider, found) in &landing.provider_health {
                lines.push(Line::from(format!(
                    "  {} CLI: {}",
                    provider.as_str(),
                    if *found { "found" } else { "not found" }
                )));
            }
            lines.push(Line::styled(
                "Next: /use to add an account (Enter)",
                selected,
            ));
            lines.push(Line::default());
            lines.push(Line::styled("What's new", selected));
            for heading in include_str!("../../../../CHANGELOG.md")
                .lines()
                .filter(|line| line.starts_with("### "))
                .take(3)
            {
                lines.push(Line::from(heading.trim_start_matches("### ")));
            }
            lines.push(Line::default());
            lines.push(Line::from("Documentation  https://concordia.dev/docs"));
            lines.push(Line::from("F1  Shortcuts"));
            f.render_widget(Paragraph::new(lines), inner);
            landing.goals_area = Rect::default();
            return;
        }
        let rows = landing.rows();
        let height = inner.height as usize;
        if landing.chat_sel < landing.chat_top {
            landing.chat_top = landing.chat_sel;
        } else if height > 0 && landing.chat_sel >= landing.chat_top + height {
            landing.chat_top = landing.chat_sel + 1 - height;
        }
        let width = inner.width as usize;
        let lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .skip(landing.chat_top)
            .take(height)
            .map(|(i, row)| {
                let focused = i == landing.chat_sel && landing.pane == Pane::Chats;
                let lead = if focused { "> " } else { "  " };
                let (left, when, header) = match *row {
                    ChatRow::Group(g) => {
                        let group = &landing.groups[g];
                        let mark = if group.expanded { "▾" } else { "▸" };
                        (
                            format!("{mark} {} ({})", group.folder, group.sessions.len()),
                            format_relative_time(
                                group
                                    .sessions
                                    .first()
                                    .or(group.quick_sessions.first())
                                    .unwrap()
                                    .modified_ms,
                                now,
                            ),
                            true,
                        )
                    }
                    ChatRow::Chat(g, s) => (
                        format!("  {}", titles[g][s]),
                        format_relative_time(landing.groups[g].sessions[s].modified_ms, now),
                        false,
                    ),
                    ChatRow::Quick(g) => {
                        let group = &landing.groups[g];
                        let mark = if group.quick_questions { "▾" } else { "▸" };
                        (
                            format!("  {mark} Quick questions ({})", group.quick_sessions.len()),
                            format_relative_time(group.quick_sessions[0].modified_ms, now),
                            false,
                        )
                    }
                    ChatRow::QuickChat(g, s) => (
                        format!(
                            "    {}",
                            crate::sessions::clean_preview(
                                landing.groups[g].quick_sessions[s].preview.as_deref(),
                                80
                            )
                        ),
                        format_relative_time(landing.groups[g].quick_sessions[s].modified_ms, now),
                        false,
                    ),
                };
                let room = width.saturating_sub(lead.len() + when.chars().count() + 2);
                let left: String = if left.chars().count() > room {
                    let mut cut: String = left.chars().take(room.saturating_sub(1)).collect();
                    cut.push('…');
                    cut
                } else {
                    left
                };
                let gap =
                    width.saturating_sub(lead.len() + left.chars().count() + when.chars().count());
                let base = if focused {
                    selected
                } else if header {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                Line::from(vec![
                    Span::styled(lead, base),
                    Span::styled(left, base),
                    Span::raw(" ".repeat(gap)),
                    Span::styled(when, if focused { selected } else { dim }),
                ])
            })
            .collect();
        f.render_widget(Paragraph::new(lines), inner);
    } else {
        landing.chats_area = Rect::default();
    }

    let right = if narrow_goals {
        Some(area)
    } else {
        panes.get(1).copied()
    };
    let Some(right) = right else {
        landing.goals_area = Rect::default();
        landing.goal_hits.clear();
        return;
    };
    let (usage_area, right) = if landing.meters.is_empty() {
        (None, right)
    } else {
        let max_usage_rows = right.height.saturating_sub(7) as usize;
        let usage_rows = meter_rows(&landing.meters, right.width.saturating_sub(2) as usize)
            .min(max_usage_rows.max(1));
        let usage_height = usage_rows as u16 + 2;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(usage_height), Constraint::Min(5)])
            .split(right);
        (Some(chunks[0]), chunks[1])
    };
    if let Some(usage_area) = usage_area {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" usage ")
            .border_style(border(false));
        let inner = block.inner(usage_area);
        f.render_widget(block, usage_area);
        let width = inner.width as usize;
        let capacity = inner.height as usize;
        let all: Vec<Vec<String>> = landing
            .meters
            .iter()
            .map(|meter| meter_lines(meter, width))
            .collect();
        let total: usize = all.iter().map(Vec::len).sum();
        let budget = if total > capacity {
            capacity.saturating_sub(1)
        } else {
            capacity
        };
        let mut lines: Vec<Line> = Vec::new();
        let mut shown = 0;
        for meter in &all {
            if lines.len() + meter.len() > budget {
                break;
            }
            for (row, text) in meter.iter().enumerate() {
                lines.push(if row == 0 {
                    Line::styled(text.clone(), Style::default().add_modifier(Modifier::BOLD))
                } else {
                    Line::styled(text.clone(), dim)
                });
            }
            shown += 1;
        }
        if shown < all.len() {
            lines.push(Line::styled(
                format!("+{} more accounts · /usage", all.len() - shown),
                dim,
            ));
        }
        f.render_widget(Paragraph::new(lines), inner);
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" open work ")
        .border_style(border(false));
    let inner = block.inner(right);
    f.render_widget(block, right);
    let mut lines: Vec<Line> = Vec::new();
    let mut hits: Vec<Option<usize>> = Vec::new();
    let mut focus = None;
    if lines.is_empty() && landing.work.is_empty() {
        // An empty pane must still say what it is for and how to fill it. Open work, when
        // present, already fills the pane and keeps its rows.
        for (text, style) in [
            (
                "No open work.",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            ("", dim),
            (
                "Open work from the connected Harness appears here.",
                dim,
            ),
            (
                "Use the chat and repository files for task objectives.",
                dim,
            ),
            ("", dim),
            ("Open a chat to continue.", Style::default()),
        ] {
            lines.push(Line::styled(text, style));
            hits.push(None);
        }
    }
    if !landing.work.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(Line::styled(
            "open to-dos",
            Style::default().add_modifier(Modifier::BOLD),
        ));
        for project in &landing.work {
            match &project.items {
                Some(items) if items.is_empty() => continue,
                Some(items) => {
                    lines.push(Line::styled(
                        format!("{} · {}", project.folder, items.len()),
                        dim,
                    ));
                    for item in items {
                        lines.push(Line::styled(
                            format!("  ○ {}", item.text),
                            if item.queued { dim } else { Style::default() },
                        ));
                    }
                }
                None => {
                    lines.push(Line::styled(project.folder.clone(), dim));
                    lines.push(Line::styled(
                        "  Harness here is too old to list open work",
                        dim,
                    ));
                }
            }
        }
    }
    let mut visual = Vec::new();
    let mut visual_hits = Vec::new();
    let mut focused_rows = 0..0;
    for (i, line) in lines.into_iter().enumerate() {
        let first = visual.len();
        let wrapped = wrap_goal_line(line, inner.width as usize);
        visual_hits.extend(std::iter::repeat_n(
            hits.get(i).copied().flatten(),
            wrapped.len(),
        ));
        visual.extend(wrapped);
        if focus == Some(i) {
            focused_rows = first..visual.len();
        }
    }
    let height = inner.height as usize;
    if landing.goal_follow && !focused_rows.is_empty() {
        let target = if matches!(landing.mode, Mode::AddGoal(_) | Mode::ConfirmDone) {
            focused_rows.end.saturating_sub(1)
        } else {
            focused_rows.start
        };
        if target < landing.goal_top {
            landing.goal_top = target;
        } else if target >= landing.goal_top + height {
            landing.goal_top = target.saturating_add(1).saturating_sub(height);
        }
    }
    landing.goal_top = landing.goal_top.min(visual.len().saturating_sub(height));
    landing.goal_hits.clear();
    f.render_widget(
        Paragraph::new(
            visual
                .into_iter()
                .skip(landing.goal_top)
                .take(height)
                .collect::<Vec<_>>(),
        ),
        inner,
    );
}

/// Pre-wrap with the same rows used by scrolling and mouse hit-testing.
fn wrap_goal_line(line: Line<'_>, width: usize) -> Vec<Line<'static>> {
    let mut chars = Vec::new();
    let mut runs = Vec::new();
    for span in line.spans {
        let start = chars.len();
        chars.extend(span.content.chars());
        runs.push((start, chars.len(), line.style.patch(span.style)));
    }
    let mut ranges = Vec::new();
    let mut logical_start = 0;
    for end in 0..=chars.len() {
        if end == chars.len() || chars[end] == '\n' {
            ranges.extend(
                super::text::wrap_ranges(&chars[logical_start..end], width)
                    .into_iter()
                    .map(|(a, b)| (logical_start + a, logical_start + b)),
            );
            logical_start = end + 1;
        }
    }
    ranges
        .into_iter()
        .map(|(start, end)| {
            Line::from(
                runs.iter()
                    .filter_map(|&(a, b, style)| {
                        let (a, b) = (a.max(start), b.min(end));
                        (a < b).then(|| Span::styled(chars[a..b].iter().collect::<String>(), style))
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// The provider's or Concordia's title for a chat, else its first words.
fn chat_title(app: &App, session: &SessionSummary) -> String {
    crate::accounts::get_account(&app.state_root, &session.account)
        .and_then(|a| a.home_path)
        .and_then(|home| {
            crate::session_titles::get(
                &app.state_root,
                session.provider,
                &session.account,
                &home,
                &session.session_id,
            )
        })
        .unwrap_or_else(|| crate::sessions::clean_preview(session.preview.as_deref(), 80))
}






