//! The landing page: recent chats grouped by project folder, the person's goals, and each
//! project's open work as its Harness ledger reports it. It replaces the empty transcript until
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
/// Below this width the goals pane is left out and the chats use the whole row.
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
#[cfg_attr(test, allow(dead_code))]
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
    show_all: bool,
    temp_root: Option<&Path>,
) -> Vec<SessionSummary> {
    sessions
        .into_iter()
        .filter(|session| {
            show_all
                || session
                    .cwd
                    .as_deref()
                    .is_none_or(|cwd| !hidden_workspace(Path::new(cwd), temp_root))
        })
        .collect()
}

fn hidden_count(sessions: &[SessionSummary], temp_root: Option<&Path>) -> usize {
    sessions
        .iter()
        .filter(|s| {
            s.cwd
                .as_deref()
                .is_some_and(|cwd| hidden_workspace(Path::new(cwd), temp_root))
        })
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
    crate::native_memory::relations::groups::project_root(folder)
}

use crate::goals::read_goals;
pub use crate::goals::{load_goals, save_goals, Goal};

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
    Goals,
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
    from_chat: bool,
    all_sessions: Vec<SessionSummary>,
    show_all: bool,
    pub(crate) hidden_temp_root: Option<PathBuf>,
    groups: Vec<ChatGroup>,
    goals: Vec<Goal>,
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

    fn open_goals(&self) -> Vec<usize> {
        (0..self.goals.len())
            .filter(|&i| !self.goals[i].done)
            .collect()
    }

    /// Open work is still being read.
    #[cfg(test)]
    pub fn work_pending(&self) -> bool {
        self.work_rx.is_some()
    }

    fn hint(&self) -> &'static str {
        match (&self.mode, self.pane) {
            (Mode::AddGoal(_), _) => "Enter save · Esc cancel",
            (Mode::ConfirmDone, _) => "Enter mark done · Esc keep",
            (Mode::Browse, Pane::Chats) => {
                "Enter open chat · F6 related work · Tab goals · Esc chat"
            }
            (Mode::Browse, Pane::Goals) => "a add goal · Enter mark done · Tab chats · Esc chat",
        }
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
                landing.show_all,
                landing.hidden_temp_root.as_deref(),
            ),
            &app.cwd,
        );
    }
}

pub(super) fn open_home(app: &mut App) {
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
    if sessions.is_empty() && !from_chat {
        app.landing = None;
        return;
    }
    let hidden_temp_root = if cfg!(test) {
        None
    } else {
        Some(std::env::temp_dir())
    };
    let groups = display_groups(
        landing_sessions(sessions.clone(), false, hidden_temp_root.as_deref()),
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
    let goals = match read_goals(&app.state_root) {
        Ok(goals) => goals,
        Err(_) => {
            app.flash("Goals could not be loaded; the original file was kept unchanged");
            Vec::new()
        }
    };
    let landing = Landing {
        from_chat,
        all_sessions: sessions,
        show_all: false,
        hidden_temp_root,
        groups,
        goals,
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
                snapshot: app.usage_fetcher.cached_snapshot(
                    account.provider,
                    &account.account,
                    &home,
                ),
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

/// Drafting keeps the landing page visible. Goal entry keeps its own input.
pub(super) fn handle_paste(app: &mut App, text: &str) -> bool {
    if !visible(app) || text.is_empty() {
        return false;
    }
    match &mut app.landing.as_mut().expect("visible landing page").mode {
        Mode::AddGoal(goal) => {
            goal.push_str(text);
            true
        }
        Mode::ConfirmDone => true,
        Mode::Browse => {
            set_pane(app, Pane::Chats);
            false
        }
    }
}

/// On screen: opened, nothing said in this chat yet, and no picker over it.
pub(super) fn editing_goal(app: &App) -> bool {
    app.landing
        .as_ref()
        .is_some_and(|landing| !matches!(&landing.mode, Mode::Browse))
}

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

/// Collect open work that arrived from the background reader.
pub(super) fn poll(app: &mut App) {
    if app.landing.is_none() {
        return;
    }
    refresh_meters(app);
    let Some(landing) = app.landing.as_mut() else {
        return;
    };
    let Some(rx) = landing.work_rx.as_ref() else {
        return;
    };
    loop {
        match rx.try_recv() {
            Ok(work) => landing.work.push(work),
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                landing.work_rx = None;
                return;
            }
        }
    }
}

fn save(app: &mut App) {
    let Some(landing) = app.landing.as_ref() else {
        return;
    };
    let selected_id = landing
        .open_goals()
        .get(landing.goal_sel)
        .map(|&i| landing.goals[i].id.clone());
    if let Err(e) = save_goals(&app.state_root, &landing.goals) {
        app.flash(format!("goals could not be saved: {e}"));
    } else {
        let goals = load_goals(&app.state_root);
        let landing = app.landing.as_mut().expect("landing page is open");
        landing.goals = goals;
        let open = landing.open_goals();
        landing.goal_sel = selected_id
            .and_then(|id| open.iter().position(|&i| landing.goals[i].id == id))
            .unwrap_or_else(|| landing.goal_sel.min(open.len().saturating_sub(1)));
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
            app.resume_session(&session.account, &session.session_id);
        }
        Some(ChatRow::Quick(g)) => {
            landing.groups[g].quick_questions = !landing.groups[g].quick_questions
        }
        Some(ChatRow::QuickChat(g, s)) => {
            let session = landing.groups[g].quick_sessions[s].clone();
            close(app);
            app.resume_session(&session.account, &session.session_id);
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
            close(app);
            true
        }
        _ => false,
    }
}

pub(super) fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if key.code == KeyCode::F(6) {
        app.memory_command("groups");
        return true;
    }
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
    if landing.pane == Pane::Goals {
        landing.goal_follow = true;
    }
    match landing.mode.clone() {
        Mode::AddGoal(mut text) => {
            match key.code {
                KeyCode::Esc => landing.mode = Mode::Browse,
                KeyCode::Enter => {
                    landing.mode = Mode::Browse;
                    if !text.trim().is_empty() {
                        let now = super::now_stamp();
                        landing.goals.push(Goal {
                            id: format!("g-{}", uuid::Uuid::new_v4()),
                            text: text.trim().to_string(),
                            done: false,
                            added_at: now,
                            closed_at: None,
                            origin: Some(crate::goals::Attribution::user()),
                            closed_origin: None,
                            ..Default::default()
                        });
                        landing.goal_sel = landing.open_goals().len().saturating_sub(1);
                        save(app);
                    }
                }
                KeyCode::Backspace => {
                    text.pop();
                    landing.mode = Mode::AddGoal(text);
                }
                KeyCode::Char(c) if plain || key.modifiers.contains(KeyModifiers::ALT) => {
                    text.push(c);
                    landing.mode = Mode::AddGoal(text);
                }
                KeyCode::Char(c)
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT)
                        && matches!(c.to_ascii_lowercase(), 'v' | 'c') =>
                {
                    return false
                }
                // Goal entry owns input until saved or cancelled. Letting navigation
                // or undo fall through would silently edit the prompt underneath.
                _ => return true,
            }
            let hint = app
                .landing
                .as_ref()
                .map(Landing::hint)
                .unwrap_or(FOOTER_HINT);
            app.footer_left = hint.into();
            return true;
        }
        Mode::ConfirmDone => {
            match key.code {
                KeyCode::Enter => {
                    let open = landing.open_goals();
                    if let Some(&i) = open.get(landing.goal_sel) {
                        landing.goals[i].done = true;
                        landing.goals[i].closed_at = Some(super::now_stamp());
                        landing.goals[i].closed_origin = Some(crate::goals::Attribution::user());
                    }
                    landing.mode = Mode::Browse;
                    landing.goal_sel = landing
                        .goal_sel
                        .min(landing.open_goals().len().saturating_sub(1));
                    save(app);
                }
                KeyCode::Esc => landing.mode = Mode::Browse,
                _ => return true,
            }
            let hint = app
                .landing
                .as_ref()
                .map(Landing::hint)
                .unwrap_or(FOOTER_HINT);
            app.footer_left = hint.into();
            return true;
        }
        Mode::Browse => {}
    }
    // A nonempty draft owns editing and Enter; Tab still reaches the goals.
    if !app.editor.text().is_empty()
        && !matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Esc)
        && landing.pane == Pane::Chats
    {
        return false;
    }
    let rows = landing.rows().len();
    let goals = landing.open_goals().len();
    match (key.code, landing.pane) {
        (KeyCode::Esc, _) => close(app),
        (KeyCode::Tab | KeyCode::BackTab, Pane::Chats) => set_pane(app, Pane::Goals),
        (KeyCode::Tab | KeyCode::BackTab, Pane::Goals) => set_pane(app, Pane::Chats),
        (KeyCode::Up, Pane::Chats) => landing.chat_sel = previous(landing.chat_sel, rows),
        (KeyCode::Down, Pane::Chats) => landing.chat_sel = next(landing.chat_sel, rows),
        (KeyCode::Up, Pane::Goals) => landing.goal_sel = previous(landing.goal_sel, goals),
        (KeyCode::Down, Pane::Goals) => landing.goal_sel = next(landing.goal_sel, goals),
        (KeyCode::Enter, Pane::Chats) => activate_chat(app),
        (KeyCode::Char(' '), Pane::Chats)
            if matches!(
                landing.rows().get(landing.chat_sel),
                Some(ChatRow::Group(_))
            ) =>
        {
            activate_chat(app)
        }
        (KeyCode::Enter, Pane::Goals) if goals > 0 => {
            landing.mode = Mode::ConfirmDone;
            app.footer_left = landing.hint().into();
        }
        (KeyCode::Char('a'), Pane::Goals) if plain => {
            landing.mode = Mode::AddGoal(String::new());
            app.footer_left = landing.hint().into();
        }
        (KeyCode::Char('h'), Pane::Chats)
            if plain
                && landing.all_sessions.len()
                    != landing_sessions(
                        landing.all_sessions.clone(),
                        false,
                        landing.hidden_temp_root.as_deref(),
                    )
                    .len() =>
        {
            landing.show_all = !landing.show_all;
            landing.groups = display_groups(
                landing_sessions(
                    landing.all_sessions.clone(),
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
            // Drafting keeps chats and goals visible.
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
            && hidden_count(&landing.all_sessions, landing.hidden_temp_root.as_deref()) > 0
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
            || (super::contains(l.goals_area, x, y)
                && l.goal_hits
                    .get((y - l.goals_area.y) as usize)
                    .is_some_and(Option::is_some))
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
        && hidden_count(&landing.all_sessions, landing.hidden_temp_root.as_deref()) > 0
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
        MouseEventKind::ScrollUp if inside(landing.goals_area) => {
            landing.goal_top = landing.goal_top.saturating_sub(3);
            landing.goal_follow = false;
        }
        MouseEventKind::ScrollDown if inside(landing.goals_area) => {
            landing.goal_top = landing.goal_top.saturating_add(3);
            landing.goal_follow = false;
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
        MouseEventKind::Down(MouseButton::Left) if inside(landing.goals_area) => {
            let row = (m.row - landing.goals_area.y) as usize;
            if let Some(Some(goal)) = landing.goal_hits.get(row) {
                landing.goal_sel = *goal;
                landing.goal_follow = true;
            }
            set_pane(app, Pane::Goals);
        }
        _ => return false,
    }
    true
}

/// Rows the goals pane needs before wrapping: goals, the add or confirm line, and open work.
fn goal_rows(landing: &Landing) -> usize {
    let goals = landing.open_goals().len() + usize::from(landing.mode != Mode::Browse);
    let meters = if landing.meters.is_empty() {
        0
    } else {
        1 + landing.meters.iter().map(|_| 1).sum::<usize>()
    };
    let work: usize = landing
        .work
        .iter()
        .map(|p| match &p.items {
            Some(items) if items.is_empty() => 0,
            Some(items) => 1 + items.len(),
            None => 2,
        })
        .sum();
    if work == 0 {
        goals + meters
    } else {
        goals + meters + usize::from(goals + meters > 0) + 1 + work
    }
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
            && hidden_count(&landing.all_sessions, landing.hidden_temp_root.as_deref()) > 0
        {
            let hidden = hidden_count(&landing.all_sessions, landing.hidden_temp_root.as_deref());
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
    let content = landing
        .rows()
        .len()
        .max(goal_rows(landing))
        .min(u16::MAX as usize - 2) as u16;
    let area = Rect {
        height: (content + 2).clamp(3, area.height.max(3)).min(area.height),
        ..area
    };
    let narrow_goals = area.width < TWO_PANES_MIN && landing.pane == Pane::Goals;
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
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" goals ")
        .border_style(border(landing.pane == Pane::Goals));
    let inner = block.inner(right);
    f.render_widget(block, right);
    landing.goals_area = inner;
    let mut lines: Vec<Line> = Vec::new();
    let mut hits = Vec::new();
    let mut focus = None;
    for (row, &i) in landing.open_goals().iter().enumerate() {
        let focused = row == landing.goal_sel && landing.pane == Pane::Goals;
        let style = if focused { selected } else { Style::default() };
        if focused {
            focus = Some(lines.len());
        }
        lines.push(Line::from(vec![
            Span::styled(if focused { "> " } else { "  " }, style),
            Span::styled(
                format!(
                    "{} {}{}",
                    if landing.goals[i]
                        .origin
                        .as_ref()
                        .is_some_and(|origin| origin.source
                            == crate::goals::ChangeSource::McpProposalApprovedLocally)
                    {
                        "◇"
                    } else {
                        "○"
                    },
                    landing.goals[i].text,
                    if landing.goals[i]
                        .origin
                        .as_ref()
                        .is_some_and(|origin| origin.source
                            == crate::goals::ChangeSource::McpProposalApprovedLocally)
                    {
                        " [MCP proposal · approved locally]"
                    } else {
                        ""
                    }
                ),
                style,
            ),
        ]));
        hits.push(Some(row));
        if focused && landing.mode == Mode::ConfirmDone {
            lines.push(Line::styled("    Mark done? Enter yes · Esc no", dim));
            hits.push(Some(row));
            focus = Some(lines.len() - 1);
        }
    }
    if let Mode::AddGoal(text) = &landing.mode {
        focus = Some(lines.len());
        lines.push(Line::from(vec![
            Span::styled("> ○ ", selected),
            Span::raw(text.clone()),
            Span::styled("▏", selected),
        ]));
        hits.push(None);
    }
    if !landing.meters.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::default());
            hits.push(None);
        }
        lines.push(Line::styled(
            "usage",
            Style::default().add_modifier(Modifier::BOLD),
        ));
        hits.push(None);
        for meter in &landing.meters {
            if meter.snapshot.windows.is_empty() {
                lines.push(Line::styled(
                    format!(
                        "{} · {} · {}",
                        meter.provider.as_str(),
                        meter.account,
                        meter.snapshot.summary
                    ),
                    dim,
                ));
            } else {
                let windows = meter
                    .snapshot
                    .windows
                    .iter()
                    .map(|window| {
                        let left = window.left_pct().round() as usize;
                        let filled = (left * 10 / 100).min(10);
                        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled));
                        format!(
                            "{} [{bar}] {}% · {}",
                            window.label,
                            left,
                            window.resets.as_deref().unwrap_or("reset unavailable")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · ");
                lines.push(Line::styled(
                    format!(
                        "{} · {} · {windows}",
                        meter.provider.as_str(),
                        meter.account
                    ),
                    dim,
                ));
            }
            hits.push(None);
        }
    }
    if lines.is_empty() && landing.work.is_empty() {
        // An empty pane must still say what it is for and how to fill it. Open work, when
        // present, already fills the pane and keeps its rows.
        for (text, style) in [
            (
                "No goals yet.",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            ("", dim),
            (
                "Goals are the outcomes you are working toward across chats.",
                dim,
            ),
            (
                "Agents can read them and propose new ones for you to approve.",
                dim,
            ),
            ("", dim),
            ("Press Tab, then a, to add one.", Style::default()),
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
    landing.goal_hits = visual_hits
        .into_iter()
        .skip(landing.goal_top)
        .take(height)
        .collect();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ProviderKind;
    use tempfile::tempdir;

    fn session(folder: &str, id: &str, modified_ms: u128) -> SessionSummary {
        SessionSummary {
            account: "a".into(),
            provider: ProviderKind::Codex,
            session_id: id.into(),
            cwd: Some(format!("C:/work/{folder}")),
            preview: Some(id.into()),
            modified_ms,
            user_messages: 1,
        }
    }

    #[test]
    fn groups_current_first_and_newest_first() {
        let groups = group_sessions(
            vec![
                session("older", "o1", 1),
                session("here", "new", 3),
                session("here", "old", 2),
                session("newer", "n1", 9),
            ],
            Path::new("C:/work/here"),
        );
        assert_eq!(
            groups.iter().map(|g| g.folder.as_str()).collect::<Vec<_>>(),
            ["here", "newer", "older"]
        );
        assert!(groups[0].expanded && !groups[1].expanded);
        assert_eq!(groups[0].sessions[0].session_id, "new");
    }

    #[test]
    fn hidden_workspace_rules() {
        let root = Path::new(r"C:\tmp\fixture");
        for p in [
            r"C:\tmp\fixture\x",
            r"C:\code\target\x",
            r"C:\concordia-test-1",
            r"C:\concordia-qual-1",
            r"C:\live-qualification-20261104",
        ] {
            assert!(hidden_workspace(Path::new(p), Some(root)));
        }
        for p in [
            r"C:\code\latest",
            r"C:\code\contest",
            r"C:\notes\temp-notes",
            r"C:\code\pytest-demo",
            r"\\?\C:\code\latest",
            "C:/code/latest",
        ] {
            assert!(!hidden_workspace(Path::new(p), None));
        }
    }

    #[test]
    fn display_path_strips_extended_and_unc_prefixes() {
        assert_eq!(display_path(Path::new(r"\\?\C:\x")), PathBuf::from(r"C:\x"));
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share\x")),
            PathBuf::from(r"\\server\share\x")
        );
        assert_eq!(display_path(Path::new(r"C:\x")), PathBuf::from(r"C:\x"));
    }

    #[test]
    fn duplicate_folder_names_get_their_parent() {
        let groups = group_sessions(
            vec![
                SessionSummary {
                    cwd: Some(r"C:\dev\concordia".into()),
                    ..session("concordia", "a", 1)
                },
                SessionSummary {
                    cwd: Some(r"D:\work\concordia".into()),
                    ..session("concordia", "b", 2)
                },
                session("unique", "c", 3),
            ],
            Path::new(r"C:\none"),
        );
        let names: Vec<_> = groups.iter().map(|g| g.folder.as_str()).collect();
        assert!(names.contains(&"dev/concordia"));
        assert!(names.contains(&"work/concordia"));
        assert!(names.contains(&"unique"));
    }

    #[test]
    fn quick_questions_collect_short_old_chats_per_folder() {
        let now = 10 * 86_400_000u128;
        let mut old = session("one", "old", now - 2 * 86_400_000);
        old.user_messages = 2;
        let mut long = session("one", "long", now - 2 * 86_400_000);
        long.user_messages = 3;
        let recent = session("one", "recent", now - 3_600_000);
        let groups = display_groups_at(vec![old, long, recent], Path::new(r"C:\none"), now);
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0]
                .sessions
                .iter()
                .map(|s| s.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["recent", "long"]
        );
        assert_eq!(groups[0].quick_sessions[0].session_id, "old");
        assert!(!groups[0].quick_questions);
    }

    #[test]
    fn worktree_chats_join_their_repository_and_probe_chats_are_skipped() {
        let dir = tempdir().unwrap();
        let repo = dir.path().join("repo");
        let tree = dir.path().join("repo-feature");
        std::fs::create_dir_all(repo.join(".git/worktrees/repo-feature")).unwrap();
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(
            tree.join(".git"),
            format!(
                "gitdir: {}\n",
                repo.join(".git/worktrees/repo-feature").display()
            ),
        )
        .unwrap();
        let at = |cwd: &Path, id: &str, ms| SessionSummary {
            cwd: Some(cwd.to_string_lossy().into_owned()),
            ..session("x", id, ms)
        };
        let groups = group_sessions(
            vec![
                at(&repo, "main", 2),
                at(&tree, "branch", 3),
                at(&crate::cursor_usage::probe_dir(), "probe", 9),
            ],
            &repo,
        );
        assert_eq!(groups.len(), 1, "{groups:?}");
        assert_eq!(groups[0].folder, "repo");
        assert_eq!(groups[0].sessions.len(), 2);
    }

    #[test]
    fn goals_round_trip() {
        let dir = tempdir().unwrap();
        let mut goals = vec![Goal {
            id: "g-1".into(),
            text: "ship".into(),
            done: false,
            added_at: "now".into(),
            closed_at: None,
            ..Default::default()
        }];
        goals[0].extra.insert(
            "external_note".into(),
            serde_json::json!("keep existing metadata"),
        );
        std::fs::write(
            dir.path().join("goals.json"),
            serde_json::to_vec(&goals).unwrap(),
        )
        .unwrap();
        let legacy = std::fs::read(dir.path().join("goals.json")).unwrap();
        assert_eq!(load_goals(dir.path()), goals);
        assert_eq!(
            std::fs::read(dir.path().join("goals.json")).unwrap(),
            legacy,
            "reading does not migrate data"
        );
        save_goals(dir.path(), &goals).unwrap();
        assert!(
            serde_json::from_slice::<Vec<Goal>>(
                &std::fs::read(dir.path().join("goals.json")).unwrap()
            )
            .is_err(),
            "legacy writers must fail closed on attributed goal stores"
        );
        assert_eq!(load_goals(dir.path()), goals);
        let mut close = goals[0].clone();
        close.done = true;
        close.text = "must not replace the immutable text".into();
        close.added_at = "must not replace original creation".into();
        close.closed_at = Some("closed".into());
        save_goals(dir.path(), &[close]).unwrap();
        save_goals(dir.path(), &goals).unwrap();
        let saved = load_goals(dir.path());
        assert!(saved[0].done);
        assert_eq!(saved[0].text, "ship");
        assert_eq!(saved[0].added_at, "now");
        assert_eq!(saved[0].extra["external_note"], "keep existing metadata");

        use crate::goals::{changes, ChangeSource};
        let request = uuid::Uuid::new_v4().to_string();
        let revision = crate::goals::revision(&saved).unwrap();
        let before = std::fs::read(dir.path().join("goals.json")).unwrap();
        let add =
            changes::propose_add(dir.path(), &request, &revision, "Agent-proposed exact goal")
                .unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("goals.json")).unwrap(),
            before
        );
        changes::apply(dir.path(), &add).unwrap();
        let added = load_goals(dir.path());
        let goal = added.iter().find(|goal| goal.id == add.goal_id()).unwrap();
        assert_eq!(
            goal.origin.as_ref().unwrap().source,
            ChangeSource::McpProposalApprovedLocally
        );
        assert_eq!(
            goal.origin.as_ref().unwrap().request_id.as_deref(),
            Some(request.as_str())
        );
        let applied_bytes = std::fs::read(dir.path().join("goals.json")).unwrap();
        changes::apply(dir.path(), &add).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("goals.json")).unwrap(),
            applied_bytes
        );
        assert!(
            changes::propose_add(dir.path(), &request, &revision, "different reused request")
                .is_err()
        );
        assert_eq!(
            changes::propose_add(dir.path(), &request, &revision, "Agent-proposed exact goal")
                .unwrap(),
            add
        );

        let stale = changes::propose_add(
            dir.path(),
            &uuid::Uuid::new_v4().to_string(),
            &crate::goals::revision(&added).unwrap(),
            "Stale proposal",
        )
        .unwrap();
        save_goals(
            dir.path(),
            &[Goal {
                id: "concurrent-user-goal".into(),
                text: "User goal".into(),
                added_at: "now".into(),
                origin: Some(crate::goals::Attribution::user()),
                ..Default::default()
            }],
        )
        .unwrap();
        let concurrent = std::fs::read(dir.path().join("goals.json")).unwrap();
        assert!(changes::apply(dir.path(), &stale)
            .unwrap_err()
            .contains("goals changed"));
        assert_eq!(
            std::fs::read(dir.path().join("goals.json")).unwrap(),
            concurrent
        );

        let close = changes::propose_close(
            dir.path(),
            &uuid::Uuid::new_v4().to_string(),
            &crate::goals::revision(&load_goals(dir.path())).unwrap(),
            &add.goal_id(),
        )
        .unwrap();
        changes::apply(dir.path(), &close).unwrap();
        let closed = load_goals(dir.path());
        let closed_goal = closed.iter().find(|goal| goal.id == add.goal_id()).unwrap();
        assert!(closed_goal.done);
        assert_eq!(
            closed_goal
                .closed_origin
                .as_ref()
                .unwrap()
                .request_id
                .as_deref(),
            Some(close.request_id.as_str())
        );
        let duplicate = changes::propose_close(
            dir.path(),
            &uuid::Uuid::new_v4().to_string(),
            &crate::goals::revision(&closed).unwrap(),
            &add.goal_id(),
        )
        .unwrap();
        assert!(changes::apply(dir.path(), &duplicate)
            .unwrap()
            .contains("already complete"));
        assert_eq!(load_goals(dir.path()), closed);

        let current_revision = crate::goals::revision(&closed).unwrap();
        let declined = changes::propose_add(
            dir.path(),
            &uuid::Uuid::new_v4().to_string(),
            &current_revision,
            "Declined goal",
        )
        .unwrap();
        changes::decline(dir.path(), &declined).unwrap();
        assert!(changes::apply(dir.path(), &declined)
            .unwrap_err()
            .contains("declined"));
        assert_eq!(load_goals(dir.path()), closed);
        let uncertain = changes::propose_add(
            dir.path(),
            &uuid::Uuid::new_v4().to_string(),
            &current_revision,
            "Uncertain goal",
        )
        .unwrap();
        std::fs::write(dir.path().join(".ao/goal-changes").join(format!("{}.json", uncertain.request_id)), serde_json::to_vec(&serde_json::json!({"version":1,"proposal":uncertain,"proposal_sha256":uncertain.hash().unwrap(),"status":"prepared"})).unwrap()).unwrap();
        assert!(changes::apply(dir.path(), &uncertain)
            .unwrap_err()
            .contains("unconfirmed"));
        assert!(changes::status(dir.path(), &uncertain.request_id)
            .unwrap()
            .contains("\"current_goal_matches_request\":false"));
        assert_eq!(load_goals(dir.path()), closed);

        let receipt_path = dir
            .path()
            .join(".ao/goal-changes")
            .join(format!("{request}.json"));
        let mut receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&receipt_path).unwrap()).unwrap();
        receipt["status"] = serde_json::json!("prepared");
        std::fs::write(receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert!(changes::apply(dir.path(), &add)
            .unwrap_err()
            .contains("unconfirmed"));
        assert!(changes::status(dir.path(), &request)
            .unwrap()
            .contains("\"current_goal_matches_request\":true"));
        assert_eq!(load_goals(dir.path()), closed);

        let pending = changes::propose_add(
            dir.path(),
            &uuid::Uuid::new_v4().to_string(),
            &current_revision,
            "Future schema refusal",
        )
        .unwrap();
        let future = serde_json::to_vec(&serde_json::json!({"version":99,"goals":closed})).unwrap();
        std::fs::write(dir.path().join("goals.json"), &future).unwrap();
        assert!(changes::apply(dir.path(), &pending).is_err());
        assert_eq!(
            std::fs::read(dir.path().join("goals.json")).unwrap(),
            future
        );
    }

    #[test]
    fn open_work_reads_plans_then_rows_and_rejects_other_output() {
        let items = parse_open_work(
            r#"{"plans":[{"plan":"p","slice":"Ship the plan","owner":"o","status":"open","next":"n"}],
                "rows":[{"section":"backlog","id":"B2","item":"Parked","status":"queued"},
                        {"section":"backlog","id":"B3","item":"Doing","status":"open"}]}"#,
        )
        .unwrap();
        assert_eq!(
            items,
            [
                WorkItem {
                    text: "Ship the plan".into(),
                    queued: false
                },
                WorkItem {
                    text: "Parked".into(),
                    queued: true
                },
                WorkItem {
                    text: "Doing".into(),
                    queued: false
                },
            ]
        );
        assert_eq!(parse_open_work("PASS plan:open: 0 open row(s)"), None);
        assert_eq!(parse_open_work(r#"{"rows":[]}"#), None);
    }
}
