//! End-to-end input tests: real key/mouse events through the handlers, rendered with ratatui's
//! `TestBackend`, asserting on text, selection, caret and clipboard (in-memory under `cfg(test)`).

use super::*;
#[path = "tests/app.rs"]
mod app;
#[path = "tests/draw.rs"]
mod draw;
#[path = "tests/input_dispatch.rs"]
mod input_dispatch;
#[path = "tests/run.rs"]
mod run;
use crate::usage::picker_row;
use ratatui::backend::TestBackend;

struct Harness {
    app: App,
    term: Terminal<TestBackend>,
    _dir: tempfile::TempDir,
}

struct DoctorFake {
    applied: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl crate::doctor::FixApplier for DoctorFake {
    fn previous_value(
        &self,
        _fix: &crate::doctor::Fix,
        _root: &std::path::Path,
    ) -> Result<String, String> {
        Ok("old value".into())
    }

    fn apply(&self, _fix: &crate::doctor::Fix, _root: &std::path::Path) -> Result<(), String> {
        self.applied
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

fn doctor_fixture(fixable: bool) -> (Vec<crate::doctor::Check>, Vec<crate::doctor::Fix>) {
    let check = crate::doctor::Check {
        level: if fixable {
            crate::doctor::Level::Warn
        } else {
            crate::doctor::Level::Ok
        },
        name: "fixture",
        detail: "synthetic doctor finding".into(),
    };
    let fixes = fixable
        .then(|| crate::doctor::Fix {
            id: "fixture-fix",
            finding: check.detail.clone(),
            before: "old PATH folder".into(),
            after: "PATH folder to append: C:\\fixture\\bin".into(),
            impact: "synthetic impact",
        })
        .into_iter()
        .collect();
    (vec![check], fixes)
}

fn open_doctor_fixture(
    h: &mut Harness,
    fixable: bool,
) -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
    let (checks, fixes) = doctor_fixture(fixable);
    let applied = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    h.app.doctor_applier = Box::new(DoctorFake {
        applied: applied.clone(),
    });
    h.app.doctor = Some(DoctorUi {
        checks,
        fixes,
        fixed: HashMap::new(),
    });
    h.app.refresh_doctor_picker();
    applied
}

impl Harness {
    fn new(w: u16, h: u16) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        // These rendering fixtures assert the historical terminal palette; pin it so
        // changing the product default does not change what they exercise.
        app.appearance.ui.theme = crate::config::Theme::Default;
        super::theme::install(&app.appearance.ui);
        app.no_color = false; // Fixtures choose color explicitly, independent of the caller's env.
        let term = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut hn = Harness {
            app,
            term,
            _dir: dir,
        };
        hn.draw();
        hn
    }

    fn draw(&mut self) {
        let app = &mut self.app;
        self.term.draw(|f| draw_ui(f, app)).unwrap();
    }

    fn key(&mut self, code: KeyCode, mods: KeyModifiers) -> bool {
        let (tx, _rx) = mpsc::unbounded_channel();
        let quit = handle_key(&mut self.app, KeyEvent::new(code, mods), &tx);
        self.draw();
        quit
    }

    fn type_str(&mut self, s: &str) {
        for c in s.chars() {
            self.key(KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    fn mouse(&mut self, kind: MouseEventKind, x: u16, y: u16) {
        handle_mouse(
            &mut self.app,
            MouseEvent {
                kind,
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
        );
        self.draw();
    }

    fn down(&mut self, x: u16, y: u16) {
        self.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    }

    fn drag(&mut self, x: u16, y: u16) {
        self.mouse(MouseEventKind::Drag(MouseButton::Left), x, y);
    }

    fn up(&mut self, x: u16, y: u16) {
        self.mouse(MouseEventKind::Up(MouseButton::Left), x, y);
    }

    fn click(&mut self, x: u16, y: u16) {
        self.down(x, y);
        self.up(x, y);
    }

    /// Index of the transcript row whose text equals `text`.
    fn row_of(&self, text: &str) -> usize {
        self.app
            .view
            .rows
            .iter()
            .position(|r| r.text.iter().collect::<String>() == text)
            .unwrap_or_else(|| panic!("no transcript row {text:?}"))
    }

    /// Screen cell of char `idx` in transcript row `row` (gutter included).
    fn t_cell(&self, row: usize, idx: usize) -> (u16, u16) {
        let t = self.app.areas.transcript;
        let g = self.app.view.rows[row].gutter.chars().count();
        let first = self.app.view.visible().start;
        (t.x + (g + idx) as u16, t.y + (row - first) as u16)
    }

    /// Screen cell of char column `col` on prompt row `row`.
    fn i_cell(&self, row: u16, col: usize) -> (u16, u16) {
        let r = self.app.areas.input;
        (r.x + (PROMPT_W + col) as u16, r.y + row)
    }
}

// ---- account connect ---------------------------------------------------------------------

use crate::connect::{add_account_with, Effects, LoginPlan};
use std::cell::RefCell;
use std::rc::Rc;

/// Points `profile_home()` at an empty temp dir so tests never see the developer's real homes.
struct Isolated {
    _guard: std::sync::MutexGuard<'static, ()>,
    base: tempfile::TempDir,
}

impl Isolated {
    fn new() -> Self {
        let guard = crate::accounts::TEST_ENV
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let base = tempfile::tempdir().unwrap();
        std::env::set_var("CONCORDIA_CODEX_HOME_BASE", base.path());
        for k in [
            "AO_CODEX_HOME",
            "AO_CODEX_HOME_A",
            "AO_CODEX_HOME_2",
            "AO_CODEX_HOME_B",
            "AO_CLAUDE_CONFIG_DIR",
            "AO_CURSOR_HOME",
        ] {
            std::env::remove_var(k);
        }
        Isolated {
            _guard: guard,
            base,
        }
    }
}

impl Drop for Isolated {
    fn drop(&mut self) {
        std::env::remove_var("CONCORDIA_CODEX_HOME_BASE");
    }
}

impl Harness {
    /// Replace the connect step with one that records launches instead of opening windows.
    fn fake_connect(&mut self) -> Rc<RefCell<Vec<LoginPlan>>> {
        let launched = Rc::new(RefCell::new(Vec::new()));
        let rec = Rc::clone(&launched);
        self.app.connect = Box::new(move |root, provider| {
            let rec = Rc::clone(&rec);
            let launch = move |p: &LoginPlan| {
                rec.borrow_mut().push(p.clone());
                Ok(())
            };
            add_account_with(
                root,
                provider,
                &Effects {
                    has_bin: &|_| true,
                    launch: &launch,
                },
            )
        });
        launched
    }

    fn enter(&mut self) {
        self.key(KeyCode::Enter, KeyModifiers::NONE);
    }

    fn transcript_text(&self) -> String {
        self.app
            .messages
            .iter()
            .map(|m| m.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.app.on_tick();
        }
    }
}

// ---- history, exit keys ------------------------------------------------------------------

impl Harness {
    fn send(&mut self, line: &str) {
        self.type_str(line);
        self.enter();
    }
    fn key_up(&mut self) {
        self.key(KeyCode::Up, KeyModifiers::NONE);
    }
    fn key_down(&mut self) {
        self.key(KeyCode::Down, KeyModifiers::NONE);
    }
}

// ---- provider failure reporting ----------------------------------------------------------

use crate::provider::turn::TurnOutcome;

// ---- performance -------------------------------------------------------------------------

// ---- slash-command palette -----------------------------------------------------------------

impl Harness {
    /// The rendered screen as plain text, one line per terminal row.
    fn screen(&self) -> String {
        let buf = self.term.backend().buffer();
        let area = buf.area;
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ---- duplicates and removal ----------------------------------------------------------------

use crate::test_support::codex_login;

impl Harness {
    fn picker_ids(&self) -> Vec<String> {
        self.app
            .picker
            .as_ref()
            .map(|p| p.rows.iter().map(|r| r.id.clone()).collect())
            .unwrap_or_default()
    }

    /// `/use add codex`, then finish the (faked) login as `email` and let orc notice.
    fn add_codex_signed_in_as(&mut self, email: &str) -> PathBuf {
        self.type_str("/use add codex");
        self.enter();
        let home = self
            .app
            .pending_login
            .as_ref()
            .expect("login pending")
            .home
            .clone();
        codex_login(&home, email);
        self.tick(10);
        home
    }
}

/// Prints what the screen looks like. Run: cargo test -p concordia --lib show_screen -- --ignored --nocapture
#[test]
#[ignore]
fn show_screen() {
    let _iso = Isolated::new();
    let mut h = Harness::new(100, 34);
    h.app.provider = ProviderKind::Codex;
    h.app.account = "codex-3".into();
    h.app.model = "gpt-5.1-codex".into();
    h.app.usage_summary = "5h 41% · 7d 59% · resets 3h".into();
    h.app
        .push_user("Add a /doctor check for duplicate accounts and show me the diff".into());
    for (id, title, detail, status) in [
        ("1", "shell", Some("rg -n duplicate src"), CardStatus::Done),
        ("2", "edit", Some("src/doctor.rs"), CardStatus::Done),
        (
            "3",
            "shell",
            Some("cargo test -p concordia"),
            CardStatus::Running,
        ),
    ] {
        h.app.push_card(crate::provider::stream_cards::StreamCard {
            id: id.into(),
            title: title.into(),
            detail: detail.map(str::to_string),
            status,
        });
    }
    h.app.push_assistant(
        "## What I changed\n\nI added a **duplicate check** to `doctor`. It groups accounts by provider and email, so two slots signed in to the same login are reported once.\n\n- `accounts::duplicate_groups` finds the groups\n- `doctor` prints a warning per group, naming both accounts and the fix\n- the `/use` picker marks each side with *same login as ...*\n\n```rust\nfor group in duplicate_groups(root) {\n    warn(&group);\n}\n```\n\n1. Run `/doctor` to see it\n2. Use `/use remove` to drop one".into(),
    );
    h.app.push_user("looks good, thanks".into());
    h.app.push_system("provider exited with code 1");
    h.draw();
    println!("{}\n", h.screen());

    let mut h2 = Harness::new(100, 20);
    h2.type_str("/u");
    println!("{}", h2.screen());
}

// ---- background /update ---------------------------------------------------------------------------

use crate::update::{spawn_job, JobResult as JR, UpdateOutcome as UO};

impl Harness {
    fn wait_for(&mut self, mut cond: impl FnMut(&Harness) -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            self.tick(1);
            if cond(self) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }
}

/// The `/update` screen with `preview` loaded, as a finished check leaves it.
fn update_screen(h: &mut Harness, preview: crate::update::UpdatePreview) {
    h.app
        .show_picker(PickerKind::Update, "update".into(), Vec::new(), false);
    h.app.update_preview = Some(preview);
    h.draw();
}

fn update_preview(install: bool, bullets: usize) -> crate::update::UpdatePreview {
    use crate::update::PreviewLine as P;
    let mut lines = vec![
        P::Field("Installed", "0.0.1 · build 2026-10-30a".into()),
        P::Field("Available", "0.0.1 · build 2026-10-30b".into()),
        P::Blank,
        P::Title("What's new".into()),
        P::Kind("Changed".into()),
        P::Bullet(
            "The command list has 12 commands, each opening one screen, and removed commands name their replacement.".into(),
        ),
    ];
    lines.extend((0..bullets).map(|i| P::Bullet(format!("change number {i}"))));
    lines.push(P::Hint("Esc close".into()));
    crate::update::UpdatePreview {
        history: Vec::new(),
        history_expanded: false,
        lines,
        install_available: install,
        release_url: None,
    }
}

// ---- waiting states, thinking, repeated errors ----------------------------------------------------

fn footer_row(h: &Harness) -> String {
    let s = h.screen();
    s.lines().last().unwrap().to_string()
}

// ---- token line and tips ------------------------------------------------------------------

use crate::provider::stream_cards::TurnUsage;

fn finished_reply(h: &mut Harness, usage: Option<TurnUsage>) {
    h.app.push_user("question".into());
    h.app.busy = true;
    h.app.turn_started = Some(Instant::now());
    h.app
        .apply_progress(TurnProgress::Text("The answer.".into()));
    if let Some(u) = usage {
        h.app.apply_progress(TurnProgress::Usage(u.clone()));
        h.app.apply_progress(TurnProgress::Usage(u));
    }
    h.app.busy = false;
    h.app.finalize_turn(TurnOutcome::default());
    h.draw();
}

fn find_cell(buf: &ratatui::buffer::Buffer, needle: &str) -> (u16, u16) {
    (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .find(|&(x, y)| {
            let n = needle.chars().count() as u16;
            x + n <= buf.area.width
                && (0..n).map(|d| buf[(x + d, y)].symbol()).collect::<String>() == needle
        })
        .unwrap_or_else(|| panic!("{needle:?} not on screen"))
}

fn sample_stats() -> crate::sysstats::Stats {
    crate::sysstats::Stats {
        self_mem: 41 << 20,
        self_cpu: 1.0,
        agents_mem: 380 << 20,
        agents_cpu: 6.0,
        agents_count: 2,
        ready: true,
    }
}

fn landing_session(
    account: &str,
    cwd: &std::path::Path,
    id: &str,
    ms: u128,
) -> crate::sessions::SessionSummary {
    crate::sessions::SessionSummary {
        account: account.into(),
        provider: ProviderKind::Codex,
        session_id: id.into(),
        cwd: Some(cwd.to_string_lossy().into_owned()),
        preview: Some(format!("chat {id}")),
        modified_ms: 1_800_000_000_000 + ms,
        user_messages: 1,
    }
}

/// A landing page over two chats in this folder and one in a sibling `lock` folder.
fn landing_harness() -> Harness {
    let mut h = Harness::new(100, 24);
    let here = h.app.cwd.clone();
    let lock = here.parent().unwrap().join("lock");
    std::fs::create_dir_all(&lock).unwrap();
    let sessions = vec![
        landing_session("acct-x", &here, "first", 3),
        landing_session("acct-x", &here, "second", 2),
        landing_session("acct-x", &lock, "third", 1),
    ];
    landing::open_with(&mut h.app, sessions, None);
    h.draw();
    h
}

/// A project folder with a ledger, and a fake harness that prints `output`.
fn landing_with_work(output: &str) -> Harness {
    let mut h = Harness::new(110, 24);
    let here = h.app.cwd.clone();
    std::fs::create_dir_all(here.join("docs/plans")).unwrap();
    std::fs::write(here.join("docs/plans/active.md"), "# Active plan\n").unwrap();
    let script = h.app.state_root.join("fake-harness.cmd");
    std::fs::write(&script, format!("@echo off\r\necho {output}\r\n")).unwrap();
    let program = script.to_string_lossy().into_owned();
    landing::open_with(
        &mut h.app,
        vec![landing_session("acct-x", &here, "first", 1)],
        Some(program),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while h
        .app
        .landing
        .as_ref()
        .is_some_and(landing::Landing::work_pending)
        && std::time::Instant::now() < deadline
    {
        landing::poll(&mut h.app);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.draw();
    h
}

#[cfg(windows)]
fn log_turn(h: &Harness, provider: &str, parts: (u64, u64, u64)) {
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    crate::token_log::record(
        &h.app.state_root,
        &crate::token_log::Entry {
            at,
            provider: provider.into(),
            account: "acct-x".into(),
            model: Some("auto".into()),
            project: h.app.cwd.to_string_lossy().into_owned(),
            chat: Some("chat-1".into()),
            new_input: parts.0,
            cached_input: parts.1,
            output: parts.2,
            cost_usd: None,
            attribution: None,
        },
    )
    .unwrap();
}

fn fake_update_job() -> crate::update::Job {
    spawn_job(JobKind::Update, |ctl| {
        ctl.say("Compiling concordia");
        while !ctl.cancelled() {
            std::thread::sleep(Duration::from_millis(10));
        }
        JR::Update(Err(anyhow::anyhow!("cancelled")))
    })
}
