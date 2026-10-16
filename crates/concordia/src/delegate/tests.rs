use super::*;
use crate::context::PackRole;
use crate::store::{upsert_stored_account, StoredAccount};

/// A codex-style home that reports a login (`chatgpt:<first 8 of account_id>`).
fn make_account(root: &Path, id: &str, provider: ProviderKind, who: &str) {
    let home = root.join(format!("home-{id}"));
    std::fs::create_dir_all(&home).unwrap();
    match provider {
        ProviderKind::Codex => std::fs::write(
            home.join("auth.json"),
            format!(r#"{{"tokens":{{"access_token":"x","account_id":"{who}"}}}}"#),
        )
        .unwrap(),
        _ => unreachable!("tests use codex homes"),
    }
    upsert_stored_account(
        root,
        StoredAccount {
            id: id.into(),
            provider,
            home_path: Some(home.to_string_lossy().into_owned()),
            label: id.into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
}

fn setup() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    make_account(d.path(), "codex-o", ProviderKind::Codex, "ORCHESTR1");
    make_account(d.path(), "codex-w1", ProviderKind::Codex, "WORKERAA1");
    make_account(d.path(), "codex-w2", ProviderKind::Codex, "WORKERBB2");
    make_account(d.path(), "codex-w3", ProviderKind::Codex, "WORKERCC3");
    make_account(d.path(), "codex-w4", ProviderKind::Codex, "WORKERDD4");
    d
}

#[test]
fn roles_validate_and_persist() {
    let d = setup();
    let r = d.path();
    // The orchestrator is never its own worker.
    assert!(add_worker(r, "codex-o", "codex-o").unwrap_err().contains("orchestrator"));
    // Unknown and logged-out accounts are refused.
    assert!(add_worker(r, "codex-o", "nope").unwrap_err().contains("no account"));
    let home = r.join("home-codex-w4");
    std::fs::remove_file(home.join("auth.json")).unwrap();
    assert!(add_worker(r, "codex-o", "codex-w4").unwrap_err().contains("not logged in"));
    make_account(r, "codex-w4", ProviderKind::Codex, "WORKERDD4");

    assert!(add_worker(r, "codex-o", "codex-w1").unwrap().contains("worker added"));
    assert!(add_worker(r, "codex-o", "codex-w1").unwrap_err().contains("already"));
    add_worker(r, "codex-o", "codex-w2").unwrap();
    add_worker(r, "codex-o", "codex-w3").unwrap();
    assert!(add_worker(r, "codex-o", "codex-w4").unwrap_err().contains("at most 3"));
    assert_eq!(get_settings(r).workers, ["codex-w1", "codex-w2", "codex-w3"]);

    assert!(remove_worker(r, "codex-w2").unwrap().contains("removed"));
    assert!(remove_worker(r, "codex-w2").unwrap_err().contains("not a worker"));
    assert_eq!(get_settings(r).workers, ["codex-w1", "codex-w3"]);
}

#[test]
fn the_same_login_is_refused_as_a_worker() {
    let d = setup();
    let r = d.path();
    // Same subscription as the orchestrator, signed in twice.
    make_account(r, "codex-dup", ProviderKind::Codex, "ORCHESTR1");
    let e = add_worker(r, "codex-o", "codex-dup").unwrap_err();
    assert!(e.contains("same login"), "{e}");
    // ...and the same login as an existing worker.
    add_worker(r, "codex-o", "codex-w1").unwrap();
    make_account(r, "codex-dup2", ProviderKind::Codex, "WORKERAA1");
    assert!(add_worker(r, "codex-o", "codex-dup2").unwrap_err().contains("same login"));
}

#[test]
fn a_worker_is_never_full_unless_the_user_is() {
    use ChildTrust::*;
    use TrustMode as T;
    assert_eq!(worker_trust(T::Workspace, Inherit), T::Workspace);
    assert_eq!(worker_trust(T::Ask, Inherit), T::Ask);
    assert_eq!(worker_trust(T::Full, Inherit), T::Full);
    assert_eq!(worker_trust(T::Workspace, Full), T::Workspace);
    assert_eq!(worker_trust(T::Ask, Full), T::Workspace);
    assert_eq!(worker_trust(T::Full, Ask), T::Ask);
}

#[test]
fn worker_context_is_bounded_and_never_carries_a_session() {
    let d = setup();
    let w = get_account(d.path(), "codex-w1").unwrap();
    let msgs: Vec<PackMessage> = (0..40)
        .map(|i| PackMessage {
            role: if i % 2 == 0 { PackRole::User } else { PackRole::Assistant },
            text: format!("message {i} {}", "x".repeat(900)),
        })
        .collect();
    let (prefix, note) = build_worker_context(ProviderKind::Claude, Some("C:\\a"), &w, &msgs);
    let prefix = prefix.unwrap();
    assert!(prefix.contains("NOT transferred"));
    assert!(prefix.contains("message 39"), "newest message is kept");
    assert!(!prefix.contains("message 30 "), "only the last few are offered");
    assert!(prefix.chars().count() < PACK_CHARS + 600, "{}", prefix.chars().count());
    assert!(note.contains("msgs"));
    let (p2, n2) = build_worker_context(ProviderKind::Claude, None, &w, &[]);
    assert!(p2.unwrap().contains("you are a worker") && n2.contains("no prior"));
}

fn bare_job(id: u32, account: &str, state: WorkerState) -> Job {
    Job {
        id,
        account: account.into(),
        provider: ProviderKind::Codex,
        task: "t".into(),
        state,
        origin: Origin::User,
        created: Instant::now(),
        started: Some(Instant::now()),
        finished: state.is_finished().then(Instant::now),
        snippet: String::new(),
        cards: Vec::new(),
        text: String::new(),
        thinking: String::new(),
        session_id: None,
        exit_code: None,
        error: None,
        trust: TrustMode::Workspace,
        cancel: None,
        cancel_requested: false,
        announced: false,
        spec: JobSpec {
            home: None,
            cwd: PathBuf::from("."),
            prompt: "t".into(),
            prefix: None,
            trust: TrustMode::Workspace,
        },
    }
}

fn card(id: &str, status: CardStatus) -> StreamCard {
    StreamCard {
        id: id.into(),
        title: "shell".into(),
        detail: Some("cargo test".into()),
        status,
    }
}

#[test]
fn progress_drives_the_state_machine() {
    let mut j = bare_job(1, "a", WorkerState::Starting);
    apply_progress(&mut j, TurnProgress::Session("sid".into()));
    assert_eq!((j.state, j.session_id.as_deref()), (WorkerState::Thinking, Some("sid")));
    apply_progress(&mut j, TurnProgress::Card(card("c1", CardStatus::Running)));
    assert_eq!(j.state, WorkerState::Tool);
    assert!(j.snippet.contains("cargo test"));
    apply_progress(&mut j, TurnProgress::Card(card("c1", CardStatus::Done)));
    assert_eq!(j.state, WorkerState::Thinking);
    apply_progress(&mut j, TurnProgress::Text("hello\nworld".into()));
    assert_eq!((j.state, j.snippet.as_str()), (WorkerState::Writing, "world"));
    let out = TurnOutcome {
        exit_code: Some(0),
        ..Default::default()
    };
    apply_outcome(&mut j, out, Instant::now());
    assert_eq!(j.state, WorkerState::Done);
    assert_eq!(j.final_text(), "hello\nworld");
    assert_eq!(j.tool_cards(), 1);
    // A finished job ignores late events.
    apply_progress(&mut j, TurnProgress::Text("late".into()));
    assert!(!j.text.contains("late"));
}

#[test]
fn failure_and_cancel_outcomes() {
    let mut j = bare_job(1, "a", WorkerState::Writing);
    let out = TurnOutcome {
        exit_code: Some(3),
        stderr_tail: "boom\nbad login".into(),
        ..Default::default()
    };
    apply_outcome(&mut j, out, Instant::now());
    assert_eq!(j.state, WorkerState::Failed);
    assert!(j.error.as_deref().unwrap().contains("code 3"));
    assert_eq!(j.snippet, "bad login");

    let mut j = bare_job(2, "a", WorkerState::Writing);
    let out = TurnOutcome {
        cancelled: true,
        text: "cancelled".into(),
        ..Default::default()
    };
    apply_outcome(&mut j, out, Instant::now());
    assert_eq!(j.state, WorkerState::Cancelled);
    assert!(j.text.is_empty(), "the runner's placeholder is not a result");
}

#[test]
fn one_turn_per_worker_with_a_bounded_queue_and_cancel() {
    let mut d = Delegates::default();
    let now = Instant::now();
    d.jobs.push(bare_job(1, "a", WorkerState::Thinking));
    d.next_id = 2;
    let sub = |t: &str| Submit {
        account: "a".into(),
        provider: ProviderKind::Codex,
        task: t.into(),
        origin: Origin::User,
        spec: bare_job(0, "a", WorkerState::Queued).spec,
    };
    let q1 = d.submit(sub("one"), now).unwrap();
    let _q2 = d.submit(sub("two"), now).unwrap();
    assert_eq!(d.get(q1).unwrap().state, WorkerState::Queued, "waits for the worker");
    let e = d.submit(sub("three"), now).unwrap_err();
    assert!(e.contains("already has"), "{e}");
    // A different worker is independent.
    assert!(d.running_for("b").is_none());
    // Stop drops the queue and flags the running job.
    assert_eq!(d.cancel_account("a", now), 3);
    assert_eq!(d.get(q1).unwrap().state, WorkerState::Cancelled);
    assert!(d.get(1).unwrap().cancel_requested);
}

#[test]
fn visible_rows_are_active_first_then_recent_and_old_results_are_pruned() {
    let mut d = Delegates::default();
    for i in 0..12 {
        d.jobs.push(bare_job(i, "a", WorkerState::Done));
    }
    d.jobs.push(bare_job(99, "b", WorkerState::Writing));
    d.prune();
    assert_eq!(
        d.jobs.iter().filter(|j| j.state.is_finished()).count(),
        KEEP_FINISHED
    );
    let now = Instant::now();
    let v = d.visible(now, Duration::from_secs(60));
    assert_eq!(v[0].id, 99);
    assert_eq!(v.len(), 1 + KEEP_FINISHED);
    assert_eq!(d.visible(now + Duration::from_secs(600), Duration::from_secs(60)).len(), 1);
}

#[test]
fn result_block_is_labelled_and_marks_the_output_untrusted() {
    let mut j = bare_job(7, "codex-w1", WorkerState::Done);
    j.text = "the answer is 42".into();
    j.cards.push(card("c", CardStatus::Done));
    let b = result_block(&j, "Codex");
    assert!(b.contains("delegate result #7") && b.contains("(codex)") && b.contains("done"));
    assert!(b.contains("1 tool card") && b.contains("the answer is 42"));
    assert!(b.contains("untrusted") && b.contains("/delegate use"));
    let p = use_prompt(&j);
    assert!(p.contains("UNTRUSTED") && p.contains("the answer is 42") && p.contains("[end of delegate result]"));
    j.text = "y".repeat(RESULT_SEND_CHARS * 2);
    assert!(use_prompt(&j).chars().count() < RESULT_SEND_CHARS + 600);
}

#[test]
fn protocol_blocks_parse_strictly_and_cap_at_three() {
    let reply = "Sure.\n```concordia-delegate\n{\"to\":\"Codex-W1\",\"task\":\"run the tests\"}\n```\n\
        ```concordia-delegate\nnot json\n```\n\
        ```rust\nfn main(){}\n```\n\
        ```concordia-delegate\n{\"to\":\"\",\"task\":\"x\"}\n```\n\
        ```concordia-delegate\n{\"to\":\"b\",\"task\":\"two\"}\n```\n\
        ```concordia-delegate\n{\"to\":\"c\",\"task\":\"three\"}\n```\n\
        ```concordia-delegate\n{\"to\":\"d\",\"task\":\"four\"}\n```\n";
    let b = extract_delegate_blocks(reply);
    assert_eq!(b.len(), 3);
    assert_eq!(b[0], DelegateBlock { to: "codex-w1".into(), task: "run the tests".into() });
    assert_eq!(b[2].to, "c");
    assert!(extract_delegate_blocks("no blocks here").is_empty());
    // An unterminated fence yields nothing.
    assert!(extract_delegate_blocks("```concordia-delegate\n{\"to\":\"a\",\"task\":\"b\"}").is_empty());
}

#[test]
fn workers_report_shows_status_and_last_result() {
    let d = setup();
    let r = d.path();
    add_worker(r, "codex-o", "codex-w1").unwrap();
    let mut del = Delegates::default();
    let lines = workers_report(r, "codex-o", &del, Instant::now());
    assert!(lines[0].contains("workers 1/3") && lines[0].contains("protocol off"));
    assert!(lines[1].contains("idle"));
    let mut j = bare_job(4, "codex-w1", WorkerState::Done);
    j.snippet = "all green".into();
    del.jobs.push(j);
    let lines = workers_report(r, "codex-o", &del, Instant::now());
    assert!(lines[1].contains("last: done #4") && lines[1].contains("all green"), "{lines:?}");
    // The orchestrator switching onto a worker is called out.
    let lines = workers_report(r, "codex-w1", &del, Instant::now());
    assert!(lines[1].contains("orchestrator now"));
}
