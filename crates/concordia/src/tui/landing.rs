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
use serde::{Deserialize, Serialize};
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
}

/// Sessions grouped by the folder they ran in: the current folder first and expanded, the rest
/// collapsed and ordered by their newest chat.
pub fn group_sessions(sessions: Vec<SessionSummary>, cwd: &Path) -> Vec<ChatGroup> {
    let current = project_root(cwd);
    let probe = crate::cursor_usage::probe_dir();
    let mut groups: BTreeMap<PathBuf, Vec<SessionSummary>> = BTreeMap::new();
    for session in sessions {
        let folder = match session.cwd.as_deref().map(Path::new) {
            Some(path) if path == probe => continue,
            Some(path) => project_root(path),
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
                format!("{folder} [{}]", root.display())
            } else {
                folder
            };
            ChatGroup {
                folder,
                sessions,
                expanded,
            }
        })
        .collect();
    groups.sort_by(|a, b| {
        b.expanded
            .cmp(&a.expanded)
            .then_with(|| b.sessions[0].modified_ms.cmp(&a.sessions[0].modified_ms))
    });
    groups
}

/// The project identity, or for a git worktree its main repository: a worktree's
/// `.git` is a file reading `gitdir: <repo>/.git/worktrees/<name>`.
fn project_root(folder: &Path) -> PathBuf {
    let main = std::fs::read_to_string(folder.join(".git"))
        .ok()
        .and_then(|text| {
            let gitdir = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
            let gitdir = if gitdir.is_absolute() {
                gitdir
            } else {
                folder.join(gitdir)
            };
            let dot_git = gitdir.parent()?.parent()?;
            (dot_git.file_name()? == ".git").then(|| dot_git.parent().map(Path::to_path_buf))?
        });
    let root = main.as_deref().unwrap_or(folder);
    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Goal {
    pub id: String,
    pub text: String,
    pub done: bool,
    pub added_at: String,
    pub closed_at: Option<String>,
}

fn goals_path(root: &Path) -> PathBuf {
    root.join("goals.json")
}

pub fn load_goals(root: &Path) -> Vec<Goal> {
    read_goals(root).unwrap_or_default()
}

const MAX_GOALS_BYTES: usize = 2 * 1024 * 1024;

fn read_goals(root: &Path) -> std::io::Result<Vec<Goal>> {
    use std::io::Read;
    let file = match std::fs::File::open(goals_path(root)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_GOALS_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_GOALS_BYTES {
        return Err(std::io::Error::other(
            "goals file exceeds the 2 MiB read limit; it was kept unchanged",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        std::io::Error::other("existing goals file is unreadable; it was kept unchanged")
    })
}

/// Temp file plus rename, so a crash never leaves half a file.
pub fn save_goals(root: &Path, goals: &[Goal]) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    crate::state_io::with_lock(&root.join("goals.lock"), || {
        let mut saved = read_goals(root)?;
        for goal in goals {
            if let Some(existing) = saved.iter_mut().find(|g| g.id == goal.id) {
                // Goal text is immutable and completion is monotonic. A stale pane
                // cannot undo completion recorded by another pane.
                if goal.done && !existing.done {
                    *existing = goal.clone();
                }
            } else {
                saved.push(goal.clone());
            }
        }
        let json = serde_json::to_vec_pretty(&saved).map_err(std::io::Error::other)?;
        if json.len() > MAX_GOALS_BYTES {
            return Err(std::io::Error::other(
                "goals exceed the 2 MiB save limit; original file kept",
            ));
        }
        super::persist::write_atomic(&goals_path(root), &json)
    })
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
}

pub struct Landing {
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
    work: Vec<ProjectWork>,
    work_rx: Option<mpsc::Receiver<ProjectWork>>,
    /// Where the rows were drawn, for the mouse.
    chats_area: Rect,
    goals_area: Rect,
    last_click: Option<(usize, Instant)>,
    /// Tokens through Concordia in the last 24 hours, read once when the page opens.
    tokens_24h: u64,
    tokens_all_time: u64,
    tokens_checked: Instant,
}

impl Landing {
    fn rows(&self) -> Vec<ChatRow> {
        let mut rows = Vec::new();
        for (g, group) in self.groups.iter().enumerate() {
            rows.push(ChatRow::Group(g));
            if group.expanded {
                rows.extend((0..group.sessions.len()).map(|s| ChatRow::Chat(g, s)));
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
            (Mode::Browse, Pane::Chats) => "Enter open chat · Tab goals · Esc chat",
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
    if sessions.is_empty() {
        app.landing = None;
        return;
    }
    let groups = group_sessions(sessions, &app.cwd);
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
    let (tokens_24h, tokens_all_time) = super::tokens::landing_totals(&app.state_root);
    let landing = Landing {
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
        tokens_24h,
        tokens_all_time,
        tokens_checked: Instant::now(),
    };
    app.footer_left = landing.hint().into();
    app.landing = Some(landing);
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
pub(super) fn visible(app: &App) -> bool {
    app.landing.is_some()
        && !app.continuation.waiting()
        && !app.keep.turn_recovery_notice
        && app.picker.is_none()
        && !app
            .messages
            .iter()
            .any(|m| matches!(m.role, MsgRole::User | MsgRole::Assistant))
}

/// Collect open work that arrived from the background reader.
pub(super) fn poll(app: &mut App) {
    let Some(landing) = app.landing.as_mut() else {
        return;
    };
    if landing.tokens_checked.elapsed().as_secs() >= 60 {
        (landing.tokens_24h, landing.tokens_all_time) =
            super::tokens::landing_totals(&app.state_root);
        landing.tokens_checked = Instant::now();
    }
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
        goals
    } else {
        goals + usize::from(goals > 0) + 1 + work
    }
}

pub(super) fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    if let Some(landing) = app.landing.as_ref() {
        app.footer_left = if landing.mode == Mode::Browse
            && landing.pane == Pane::Chats
            && !app.editor.text().is_empty()
        {
            FOOTER_HINT
        } else {
            landing.hint()
        }
        .into();
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
        if landing.tokens_24h > 0 || landing.tokens_all_time > 0 {
            block = block.title_bottom(
                Line::styled(
                    format!(
                        " {} all time / {} tokens in 24 hours ",
                        super::token_line::compact(landing.tokens_all_time),
                        super::token_line::compact(landing.tokens_24h)
                    ),
                    dim,
                )
                .right_aligned(),
            );
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
                            format_relative_time(group.sessions[0].modified_ms, now),
                            true,
                        )
                    }
                    ChatRow::Chat(g, s) => (
                        format!("  {}", titles[g][s]),
                        format_relative_time(landing.groups[g].sessions[s].modified_ms, now),
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
            Span::styled(format!("○ {}", landing.goals[i].text), style),
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
        let goals = vec![Goal {
            id: "g-1".into(),
            text: "ship".into(),
            done: false,
            added_at: "now".into(),
            closed_at: None,
        }];
        save_goals(dir.path(), &goals).unwrap();
        assert_eq!(load_goals(dir.path()), goals);
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
