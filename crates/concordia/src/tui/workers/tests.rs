//! Worker strip layout, state colours, selection / viewer / cancel keys, the protocol confirm and
//! the commands, rendered with ratatui's `TestBackend`. Nothing here spawns a provider: jobs are
//! injected in their states, and the one test that starts a delegate runs without a tokio runtime
//! (the job then fails immediately instead of launching).

use super::super::*;
use crate::delegate::{Origin, WorkerState as S};
use crate::store::{
    modify_store, set_workers, upsert_stored_account, ChildTrust, StoredAccount, TrustMode,
};
use ratatui::backend::TestBackend;

struct H {
    app: App,
    term: Terminal<TestBackend>,
    dir: tempfile::TempDir,
}

fn account(root: &Path, id: &str, who: &str) {
    let home = root.join(format!("home-{id}"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("auth.json"),
        format!(r#"{{"tokens":{{"access_token":"x","account_id":"{who}"}}}}"#),
    )
    .unwrap();
    upsert_stored_account(
        root,
        StoredAccount {
            id: id.into(),
            provider: ProviderKind::Codex,
            home_path: Some(home.to_string_lossy().into_owned()),
            label: id.into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
}

impl H {
    fn new(w: u16, h: u16) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        // Worker color assertions cover the legacy semantic palette explicitly.
        app.appearance.ui.theme = crate::config::Theme::Default;
        crate::tui::theme::install(&app.appearance.ui);
        app.no_color = false;
        app.account = "codex-o".into();
        let term = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut hn = H { app, term, dir };
        hn.draw();
        hn
    }

    fn draw(&mut self) {
        let app = &mut self.app;
        self.term.draw(|f| draw_ui(f, app)).unwrap();
    }

    fn add(&mut self, account: &str, state: S, snippet: &str) -> u32 {
        let id = self
            .app
            .deleg
            .d
            .push_test_job(account, ProviderKind::Codex, state, snippet, 12);
        self.app.deleg.owners.insert(id, self.app.deleg.generation);
        id
    }

    fn key(&mut self, code: KeyCode, mods: KeyModifiers) {
        let (tx, _rx) = mpsc::unbounded_channel();
        handle_key(&mut self.app, KeyEvent::new(code, mods), &tx);
        self.draw();
    }

    fn click(&mut self, x: u16, y: u16) {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            handle_mouse(
                &mut self.app,
                MouseEvent {
                    kind,
                    column: x,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                },
            );
        }
        self.draw();
    }

    fn rows(&self) -> Vec<String> {
        let buf = self.term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    /// Screen y of the prompt box's top border.
    fn input_top(&self) -> u16 {
        self.app.areas.input.y - 1
    }

    fn type_str(&mut self, s: &str) {
        for c in s.chars() {
            self.key(KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    fn submit(&mut self, line: &str) -> mpsc::UnboundedReceiver<String> {
        self.type_str(line);
        let (tx, rx) = mpsc::unbounded_channel();
        handle_key(
            &mut self.app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &tx,
        );
        self.draw();
        rx
    }

    fn messages(&self) -> String {
        self.app
            .messages
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[test]
fn the_strip_is_hidden_without_workers_and_sits_above_the_prompt_otherwise() {
    for w in [60u16, 100, 140] {
        let mut h = H::new(w, 24);
        let before = h.app.areas.transcript.height;
        assert!(
            !h.rows().iter().any(|r| r.contains(" \u{b7} thinking")),
            "no strip without workers"
        );
        h.add("codex-w1", S::Thinking, "reading src/main.rs");
        h.add(
            "codex-w2",
            S::Tool,
            "shell cargo test --workspace --all-targets",
        );
        h.draw();
        let rows = h.rows();
        let top = h.input_top() as usize;
        assert!(
            rows[top - 1].contains("codex-w2") && rows[top - 1].contains("tool"),
            "w={w}: {:?}",
            rows[top - 1]
        );
        assert!(
            rows[top - 2].contains("thinking") && rows[top - 2].contains("12s"),
            "w={w}: {:?}",
            rows[top - 2]
        );
        assert_eq!(h.app.areas.transcript.height, before - 2);
        for r in &rows {
            assert!(r.chars().count() <= w as usize);
        }
        // The snippet is cut to the width, never wrapped onto another row.
        if w == 60 {
            assert!(rows[top - 1].contains('\u{2026}'), "{:?}", rows[top - 1]);
        } else {
            assert!(
                rows[top - 1].contains("--all-targets") || w == 100,
                "{:?}",
                rows[top - 1]
            );
        }
    }
}

#[test]
fn at_most_four_rows_then_plus_n_more() {
    for w in [60u16, 100, 140] {
        let mut h = H::new(w, 24);
        for i in 0..6 {
            h.add(&format!("codex-w{i}"), S::Thinking, "x");
        }
        h.draw();
        let top = h.input_top() as usize;
        let rows = h.rows();
        assert!(
            rows[top - 1].contains("+3 more"),
            "w={w}: {:?}",
            rows[top - 1]
        );
        assert!(rows[top - 4].contains("thinking") && !rows[top - 5].contains("12s"));
        // A tiny terminal never squeezes the transcript below its minimum.
        let mut small = H::new(w, 9);
        for i in 0..6 {
            small.add(&format!("codex-w{i}"), S::Thinking, "x");
        }
        small.draw();
        assert!(small.app.areas.transcript.height >= 1);
    }
}

#[test]
fn each_state_has_its_own_colour_and_spinner_animates() {
    let mut h = H::new(100, 24);
    let ids = [
        (S::Starting, Color::Cyan),
        (S::Thinking, Color::Magenta),
        (S::Tool, Color::Yellow),
        (S::Writing, Color::Green),
        (S::Done, Color::Green),
    ];
    for (i, (st, _)) in ids.iter().enumerate() {
        h.add(&format!("codex-w{i}"), *st, "");
    }
    h.draw();
    let top = h.input_top();
    // 5 rows -> 3 shown + "+2 more"; check the first three in order.
    let buf = h.term.backend().buffer().clone();
    let rows_y = (top - 4)..top;
    let colors: Vec<Color> = rows_y.clone().take(3).map(|y| buf[(2, y)].fg).collect();
    assert_eq!(colors, [Color::Cyan, Color::Magenta, Color::Yellow]);
    let glyph = |h: &H, y: u16| h.term.backend().buffer()[(2, y)].symbol().to_string();
    let g0 = glyph(&h, top - 4);
    h.app.tick_count += 1;
    h.draw();
    assert_ne!(glyph(&h, top - 4), g0, "the spinner moves with the tick");
    // Finished states are static marks.
    let mut h = H::new(100, 24);
    h.add("codex-w1", S::Done, "ok");
    h.add("codex-w2", S::Failed, "boom");
    h.add("codex-w3", S::Cancelled, "");
    h.add("codex-w4", S::Queued, "");
    h.draw();
    let top = h.input_top();
    let b = h.term.backend().buffer().clone();
    assert_eq!(
        (b[(2, top - 4)].symbol(), b[(2, top - 4)].fg),
        ("\u{25cc}", Color::DarkGray)
    );
    // Active first (queued), then finished newest first (cancelled, failed, done).
    assert_eq!(b[(2, top - 3)].symbol(), "-");
    assert_eq!(
        (b[(2, top - 2)].symbol(), b[(2, top - 2)].fg),
        ("\u{2717}", Color::Red)
    );
    assert_eq!(
        (b[(2, top - 1)].symbol(), b[(2, top - 1)].fg),
        ("\u{2713}", Color::Green)
    );
}

#[test]
fn no_color_keeps_emphasis_only() {
    let mut h = H::new(100, 24);
    h.app.no_color = true;
    let id = h.add("codex-w1", S::Tool, "cargo test");
    h.draw();
    h.key(KeyCode::Down, KeyModifiers::ALT);
    assert_eq!(h.app.deleg.selected, Some(id));
    let buf = h.term.backend().buffer().clone();
    for c in buf.content.iter() {
        assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset));
    }
    let y = h.input_top() - 1;
    assert_eq!(
        buf[(0, y)].symbol(),
        "\u{25b8}",
        "the focus marker survives without colour"
    );
    assert!(buf[(0, y)].modifier.contains(Modifier::BOLD));
}

#[test]
fn alt_arrows_and_clicks_select_enter_opens_the_viewer_esc_closes() {
    let mut h = H::new(100, 24);
    let a = h.add("codex-w1", S::Writing, "a");
    let b = h.add("codex-w2", S::Thinking, "b");
    h.key(KeyCode::Down, KeyModifiers::ALT);
    assert_eq!(h.app.deleg.selected, Some(a));
    h.key(KeyCode::Down, KeyModifiers::ALT);
    assert_eq!(h.app.deleg.selected, Some(b));
    // Past the end releases the focus.
    h.key(KeyCode::Down, KeyModifiers::ALT);
    assert_eq!(h.app.deleg.selected, None);
    h.key(KeyCode::Up, KeyModifiers::ALT);
    assert_eq!(h.app.deleg.selected, Some(b));
    // Click the first row.
    let y = h.input_top() - 2;
    h.click(5, y);
    assert_eq!(h.app.deleg.selected, Some(a));
    // Enter with an empty prompt opens the viewer; the transcript of the worker is shown.
    h.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(h.app.deleg.viewer.is_some());
    assert!(h
        .rows()
        .iter()
        .any(|r| r.contains("worker #1") && r.contains("writing")));
    assert!(h.rows().iter().any(|r| r.contains("untrusted data")));
    // While it is open, typing does not reach the prompt.
    h.type_str("zzz");
    assert!(h.app.editor.text().is_empty());
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.app.deleg.viewer.is_none());
    assert_eq!(h.app.deleg.selected, Some(a), "Esc closed the viewer only");
}

#[test]
fn enter_still_sends_when_the_prompt_has_text() {
    let mut h = H::new(100, 24);
    h.add("codex-w1", S::Writing, "a");
    h.key(KeyCode::Down, KeyModifiers::ALT);
    h.type_str("/workers");
    h.key(KeyCode::Enter, KeyModifiers::NONE); // runs it (the prompt was not empty)
    assert!(h.app.deleg.viewer.is_none());
    assert!(h.messages().contains("workers 0/3"), "{}", h.messages());
}

#[test]
fn esc_on_a_focused_running_worker_cancels_it_and_a_finished_one_just_unfocuses() {
    let mut h = H::new(100, 24);
    let run = h.add("codex-w1", S::Thinking, "busy");
    let done = h.add("codex-w2", S::Done, "ok");
    h.key(KeyCode::Down, KeyModifiers::ALT);
    assert_eq!(h.app.deleg.selected, Some(run));
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(h.app.deleg.d.get(run).unwrap().snippet, "cancelling");
    h.key(KeyCode::Down, KeyModifiers::ALT);
    assert_eq!(h.app.deleg.selected, Some(done));
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(h.app.deleg.selected, None);
    // With nothing focused Esc keeps its old meaning (here: nothing running, nothing happens).
    h.key(KeyCode::Esc, KeyModifiers::NONE);
}

#[test]
fn finished_workers_leave_the_strip_after_a_while_but_keep_their_result() {
    let mut h = H::new(100, 24);
    let id = h.add("codex-w1", S::Done, "ok");
    h.draw();
    let top = h.input_top() as usize;
    assert!(h.rows()[top - 1].contains("done"));
    // 10 minutes later.
    h.app.deleg.d.jobs[0].finished = Instant::now().checked_sub(Duration::from_secs(600));
    h.draw();
    assert!(!h.rows()[top - 1].contains("done"));
    assert!(h.app.deleg.d.get(id).is_some());
}

#[test]
fn workers_commands_validate_and_report() {
    let mut h = H::new(100, 24);
    let root = h.dir.path().to_path_buf();
    account(&root, "codex-o", "ORCHESTR1");
    account(&root, "codex-w1", "WORKERAA1");
    let _ = h.submit("/workers");
    assert!(h.messages().contains("none yet"));
    let _ = h.submit("/workers add codex-o");
    assert!(h.messages().contains("cannot be its own worker"));
    let _ = h.submit("/workers add codex-w1");
    assert!(h.messages().contains("worker added"));
    let _ = h.submit("/workers");
    assert!(h.messages().contains("Codex") || h.messages().contains("codex-w1"));
    assert!(h.messages().contains("idle"));
    // A running worker cannot be removed from under its task.
    h.add("codex-w1", S::Thinking, "busy");
    let _ = h.submit("/workers remove codex-w1");
    assert!(h.messages().contains("/delegate stop codex-w1 first"));
    let _ = h.submit("/delegate stop codex-w1");
    assert!(h.messages().contains("stopping 1 worker task"));
    let _ = h.submit("/delegate help");
    assert!(h.messages().contains("concordia-delegate") && h.messages().contains("untrusted"));
}

#[test]
fn delegate_refuses_non_workers_the_orchestrator_and_missing_tasks() {
    let mut h = H::new(100, 24);
    let root = h.dir.path().to_path_buf();
    account(&root, "codex-o", "ORCHESTR1");
    account(&root, "codex-w1", "WORKERAA1");
    let _ = h.submit("/delegate codex-w1 do a thing");
    assert!(h.messages().contains("no workers yet"));
    set_workers(&root, vec!["codex-w1".into(), "codex-o".into()]).unwrap();
    let _ = h.submit("/delegate codex-o do a thing");
    assert!(h.messages().contains("is the orchestrator now"));
    let _ = h.submit("/delegate nobody do a thing");
    assert!(h.messages().contains("not a worker"));
    let _ = h.submit("/delegate codex-w1");
    assert!(h.messages().contains("usage: /delegate"));
}

#[test]
fn delegating_creates_a_job_with_bounded_context_and_no_runtime_means_a_clean_failure() {
    // No tokio runtime in this test: the job is created and fails at once instead of spawning.
    let mut h = H::new(100, 24);
    let root = h.dir.path().to_path_buf();
    account(&root, "codex-o", "ORCHESTR1");
    account(&root, "codex-w1", "WORKERAA1");
    set_workers(&root, vec!["codex-w1".into()]).unwrap();
    h.app.push_user("earlier question".into());
    h.app.push_assistant("earlier answer".into());
    let _ = h.submit("/delegate auto Fix the thing, now");
    let m = h.messages();
    assert!(m.contains("delegated #1 to"), "{m}");
    assert!(m.contains("trust workspace") && m.contains("2 msgs"), "{m}");
    let j = h.app.deleg.d.get(1).unwrap();
    assert_eq!(j.task, "Fix the thing, now");
    h.app.poll_delegates();
    assert!(h.messages().contains("delegate result #1"));
    // The result block never claims anything was executed, and is labelled untrusted.
    assert!(h.messages().contains("untrusted worker output"));
}

#[test]
fn delegation_entry_point_clamps_workspace_child_to_plan() {
    let mut h = H::new(100, 24);
    let root = h.dir.path().to_path_buf();
    account(&root, "codex-o", "ORCHESTR1");
    account(&root, "codex-w1", "WORKERAA1");
    set_workers(&root, vec!["codex-w1".into()]).unwrap();
    h.app.trust = TrustMode::Plan;
    modify_store(&root, |store| {
        store.settings.child_trust = ChildTrust::Workspace;
        Ok(())
    })
    .unwrap();
    let _ = h.submit("/delegate codex-w1 inspect safely");
    let job = h.app.deleg.d.get(1).unwrap();
    assert_eq!(job.trust, TrustMode::Plan);
    assert!(h.messages().contains("trust plan") && h.messages().contains("your mode is plan"));
}

#[test]
fn use_sends_a_finished_result_only_on_request_and_only_when_idle() {
    let mut h = H::new(100, 24);
    let id = h.add("codex-w1", S::Done, "");
    h.app.deleg.d.jobs[0].text = "forty-two".into();
    // Nothing is sent by finishing alone.
    h.app.poll_delegates();
    assert!(h.app.deleg.outbox.is_none() && !h.app.busy);
    let mut rx = h.submit("/delegate use");
    let sent = rx.try_recv().expect("the user asked for it");
    assert!(
        sent.contains("UNTRUSTED") && sent.contains("forty-two"),
        "{sent}"
    );
    assert!(h.app.busy);
    assert!(h
        .app
        .messages
        .iter()
        .any(|m| m.role == MsgRole::User && m.text == sent));
    // While the orchestrator is busy it is refused.
    let mut rx2 = h.submit("/delegate use");
    assert!(rx2.try_recv().is_err());
    assert!(h.messages().contains("orchestrator is busy"));
    let _ = id;
}

#[test]
fn file_preview_cannot_answer_an_armed_delegation_confirmation() {
    let mut h = H::new(100, 24);
    let root = h.dir.path().to_path_buf();
    account(&root, "codex-o", "ORCHESTR1");
    account(&root, "codex-w1", "WORKERAA1");
    set_workers(&root, vec!["codex-w1".into()]).unwrap();
    let _ = h.submit("/delegate protocol on");
    h.app.scan_delegation(
        "```concordia-delegate\n{\"to\":\"codex-w1\",\"task\":\"synthetic task\"}\n```",
    );
    assert_eq!(h.app.deleg.confirms.len(), 1);
    h.app.deleg.confirms[0].since = Instant::now() - Duration::from_secs(5);
    std::fs::write(root.join("preview.txt"), "read-only content").unwrap();
    assert!(h.app.handle_slash("/open preview.txt"));
    h.draw();
    assert!(h.app.workspace.focused.is_some());
    assert!(h.app.editor.text().is_empty());
    for code in [KeyCode::Char('y'), KeyCode::Char('n')] {
        h.key(code, KeyModifiers::NONE);
        assert_eq!(h.app.deleg.confirms.len(), 1);
        assert!(
            h.app.deleg.d.jobs.is_empty(),
            "preview key started a worker"
        );
        assert!(h.app.workspace.focused.is_some());
        assert!(h.app.editor.text().is_empty());
    }
    h.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.app.workspace.focused.is_none());
    assert_eq!(
        h.app.deleg.confirms.len(),
        1,
        "Esc must return to chat without rejecting"
    );
    assert!(h.app.deleg.d.jobs.is_empty());
    h.key(KeyCode::Char('y'), KeyModifiers::NONE);
    assert!(h.app.deleg.confirms.is_empty());
    assert_eq!(h.app.deleg.d.jobs.len(), 1);
    assert_eq!(h.app.deleg.d.jobs[0].origin, Origin::Protocol);
    // No tokio runtime exists in this synthetic test, so acceptance cannot launch a provider.
}

#[test]
fn the_protocol_is_off_by_default_and_needs_an_explicit_y_when_on() {
    let mut h = H::new(100, 24);
    let root = h.dir.path().to_path_buf();
    account(&root, "codex-o", "ORCHESTR1");
    account(&root, "codex-w1", "WORKERAA1");
    set_workers(&root, vec!["codex-w1".into()]).unwrap();
    let reply =
        "ok\n```concordia-delegate\n{\"to\":\"codex-w1\",\"task\":\"run the tests\"}\n```\n";

    // Off: nothing happens.
    h.app.scan_delegation(reply);
    assert!(h.app.deleg.confirms.is_empty());

    let _ = h.submit("/delegate protocol on");
    h.app.scan_delegation(reply);
    assert_eq!(h.app.deleg.confirms.len(), 1);
    h.draw();
    assert!(h
        .rows()
        .iter()
        .any(|r| r.contains("Orchestrator wants to delegate to")
            && r.contains("run the tests")
            && r.contains("[y/n]")));
    // Type-ahead inside the arming window goes to the prompt and answers nothing.
    h.type_str("y");
    assert_eq!(h.app.deleg.confirms.len(), 1);
    assert_eq!(h.app.editor.text(), "y");
    h.key(KeyCode::Backspace, KeyModifiers::NONE);
    h.app.deleg.confirms[0].since = Instant::now() - Duration::from_secs(5);
    // Once armed, a non-empty prompt still cannot answer it.
    h.type_str("hello");
    assert_eq!(h.app.deleg.confirms.len(), 1);
    h.key(KeyCode::Char('n'), KeyModifiers::NONE);
    assert_eq!(
        h.app.deleg.confirms.len(),
        1,
        "n in a non-empty prompt is just text"
    );
    assert!(h.app.deleg.d.jobs.is_empty());
    for _ in 0..10 {
        h.key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    h.key(KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(h.app.deleg.confirms.is_empty() && h.app.deleg.d.jobs.is_empty());
    assert!(h.messages().contains("declined delegation"));

    // Unknown targets are ignored, never queued.
    h.app
        .scan_delegation("```concordia-delegate\n{\"to\":\"stranger\",\"task\":\"x\"}\n```");
    assert!(h.app.deleg.confirms.is_empty());
    assert!(h.messages().contains("ignored: not one of your workers"));

    // y runs it (here: no runtime, so the job fails cleanly and is labelled as a protocol job).
    h.app.scan_delegation(reply);
    h.app.deleg.confirms[0].since = Instant::now() - Duration::from_secs(5);
    h.key(KeyCode::Char('y'), KeyModifiers::NONE);
    assert_eq!(h.app.deleg.d.jobs.len(), 1);
    assert_eq!(h.app.deleg.d.jobs[0].origin, Origin::Protocol);
    // Max 3 pending.
    let many = reply.repeat(5);
    h.app.scan_delegation(&many);
    assert_eq!(h.app.deleg.confirms.len(), 3);
    let _ = h.submit("/delegate protocol off");
}

#[test]
fn mcp_proposals_reach_the_confirm_prompt_only_while_the_protocol_is_on() {
    let mut h = H::new(100, 24);
    let root = h.dir.path().to_path_buf();
    account(&root, "codex-o", "ORCHESTR1");
    account(&root, "codex-w1", "WORKERAA1");
    set_workers(&root, vec!["codex-w1".into()]).unwrap();
    let dir = crate::mcp::proposals_dir(&root);
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, to: &str| {
        std::fs::write(
            dir.join(name),
            format!(r#"{{"to":"{to}","task":"run the tests","from_thread":"3f9a"}}"#),
        )
        .unwrap();
    };
    write("a.json", "codex-w1");

    // Polled only on the 2 s beat, and left alone while the protocol is off.
    h.app.tick_count = 10;
    h.app.poll_proposals();
    assert!(h.app.deleg.confirms.is_empty() && dir.join("a.json").exists());

    let _ = h.submit("/delegate protocol on");
    h.app.tick_count = 3;
    h.app.poll_proposals();
    assert!(
        h.app.deleg.confirms.is_empty(),
        "off the beat: no read at all"
    );
    h.app.tick_count = 10;
    h.app.poll_proposals();
    assert_eq!(h.app.deleg.confirms.len(), 1);
    assert!(!dir.join("a.json").exists(), "a taken proposal is consumed");
    assert_eq!(h.app.deleg.confirms[0].task, "run the tests");
    h.draw();
    assert!(h
        .rows()
        .iter()
        .any(|r| r.contains("wants to delegate to") && r.contains("[y/n]")));
    assert!(
        h.app.deleg.d.jobs.is_empty(),
        "nothing runs without the user's y"
    );

    // A target that is not one of the user's workers is dropped, never queued.
    write("b.json", "stranger");
    h.app.tick_count = 20;
    h.app.poll_proposals();
    assert_eq!(h.app.deleg.confirms.len(), 1);
    assert!(h.messages().contains("ignored: not one of your workers"));
}

#[test]
fn worker_models_use_their_own_catalog_and_apply_to_new_jobs() {
    let mut h = H::new(120, 30);
    let root = h.dir.path().to_path_buf();
    account(&root, "codex-w1", "WORKERAA1");
    set_workers(&root, vec!["codex-w1".into()]).unwrap();
    std::fs::write(
        root.join("home-codex-w1/models_cache.json"),
        r#"{"models":[{"slug":"fixture-fast"}]}"#,
    )
    .unwrap();
    h.submit("/delegate model codex-w1 fixture-fast");
    assert_eq!(
        get_settings(&root)
            .worker_models
            .get("codex-w1")
            .map(String::as_str),
        Some("fixture-fast")
    );
    h.submit("/delegate model codex-w1 invented-model");
    assert!(h.messages().contains("not in codex-w1's local catalog"));
    h.submit("/delegate codex-w1 scoped fixture task");
    assert_eq!(h.app.deleg.d.jobs[0].model.as_deref(), Some("fixture-fast"));
    assert!(super::viewer_text(&h.app.deleg.d.jobs[0]).contains("model: fixture-fast"));
    h.submit("/delegate model codex-w1 default");
    assert!(!get_settings(&root).worker_models.contains_key("codex-w1"));
    assert_eq!(h.app.deleg.d.jobs[0].model.as_deref(), Some("fixture-fast"));
}

#[test]
fn automatic_returns_batch_once_preserve_drafts_and_exclude_other_conversations() {
    let mut h = H::new(120, 30);
    h.submit("/delegate returns auto");
    for i in 0..2 {
        h.add(&format!("codex-w{i}"), S::Done, "");
        h.app.deleg.d.jobs[i].text = format!("worker result {i}");
    }
    h.app.editor.set_text("my unsent draft");
    h.app.busy = true;
    h.app.poll_delegates();
    assert!(h.app.deleg.outbox.is_none());
    h.app.busy = false;
    h.app.poll_delegates();
    let (tx, mut rx) = mpsc::unbounded_channel();
    h.app.flush_delegate_prompt(&tx);
    let sent = rx.try_recv().unwrap();
    assert!(sent.contains("worker result 0") && sent.contains("worker result 1"));
    assert_eq!(sent.matches("UNTRUSTED").count(), 2);
    assert_eq!(h.app.editor.text(), "my unsent draft");
    assert!(h.app.deleg.d.jobs.iter().all(|j| j.collected));
    assert!(h
        .app
        .continuation
        .checkpoint(None, vec![], h.app.cwd.clone())
        .is_some());
    h.app.busy = false;
    h.app.poll_delegates();
    h.app.flush_delegate_prompt(&tx);
    assert!(rx.try_recv().is_err());
    let old = h.add("codex-old", S::Done, "");
    h.app.deleg.d.jobs.last_mut().unwrap().text = "foreign result".into();
    h.app.deleg.change_conversation();
    h.app.poll_delegates();
    h.app.flush_delegate_prompt(&tx);
    assert!(rx.try_recv().is_err());
    assert!(!h.app.deleg.d.get(old).unwrap().collected);
}
