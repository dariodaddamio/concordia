use super::*;

#[test]
fn concurrent_panes_own_distinct_records_and_workspace_roots() {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a");
    let b = d.path().join("b");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    let root_a = recovery_root(d.path(), &a).unwrap();
    let root_a2 = recovery_root(d.path(), &a).unwrap();
    let root_b = recovery_root(d.path(), &b).unwrap();
    assert_eq!(root_a, root_a2);
    assert_ne!(root_a, root_b);
    let one = create(&root_a).unwrap();
    let two = create(&root_a).unwrap();
    one.save("first", &["q1".into()]).unwrap();
    two.save("second", &["q2".into()]).unwrap();
    assert_ne!(one.id(), two.id());
    assert!(claim(&root_a, one.id()).is_err());
}

#[test]
fn abandoned_record_is_claimed_and_restored() {
    let d = tempfile::tempdir().unwrap();
    let first = create(d.path()).unwrap();
    first.save("draft", &["queued".into()]).unwrap();
    let id = first.id().to_owned();
    drop(first);
    let (reclaimed, restored, choices) = startup_with_restore(d.path()).unwrap();
    assert_eq!(reclaimed.id(), id);
    assert!(choices.is_empty());
    let data = restored.unwrap();
    assert_eq!(data.draft, "draft");
    assert_eq!(data.queue, vec!["queued"]);
}

#[test]
fn multiple_records_require_explicit_choice_and_corruption_is_preserved() {
    let d = tempfile::tempdir().unwrap();
    let a = create(d.path()).unwrap();
    a.save("a", &[]).unwrap();
    drop(a);
    let b = create(d.path()).unwrap();
    b.save("b", &[]).unwrap();
    drop(b);
    let choices = available(d.path()).unwrap();
    assert_eq!(choices.len(), 2);
    let (fresh, restored, startup_choices) = startup_with_restore(d.path()).unwrap();
    assert!(restored.is_none());
    assert_eq!(startup_choices.len(), 2);
    drop(fresh);
    assert!(claim(d.path(), "missing").is_err());
    let corrupt = create(d.path()).unwrap();
    fs::write(corrupt.directory.join(RECORD_FILE), b"broken").unwrap();
    drop(corrupt);
    let choices = available(d.path()).unwrap();
    assert_eq!(choices.len(), 2);
    assert!(!d.path().join("broken").exists());
    // The corrupt record remains available for manual inspection; it is never overwritten.
    let dirs: Vec<_> = fs::read_dir(d.path())
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(dirs
        .iter()
        .any(|e| fs::read(e.path().join(RECORD_FILE)).ok().as_deref() == Some(b"broken")));
}

#[test]
fn legacy_import_copies_without_deleting_shared_files() {
    let d = tempfile::tempdir().unwrap();
    crate::tui::persist::save_draft(d.path(), "legacy draft");
    crate::tui::persist::save_queue(d.path(), &["legacy queue".into()]);
    let lease = create(&d.path().join("records")).unwrap();
    assert!(legacy_available(d.path()));
    import_legacy(d.path(), &lease).unwrap();
    assert_eq!(lease.data().unwrap().draft, "legacy draft");
    assert_eq!(
        crate::tui::persist::load_draft(d.path()).as_deref(),
        Some("legacy draft")
    );
    assert_eq!(
        crate::tui::persist::load_queue(d.path()),
        vec!["legacy queue"]
    );
}

#[test]
fn pending_edit_round_trips_and_old_record_json_defaults_it() {
    let d = tempfile::tempdir().unwrap();
    let lease = create(d.path()).unwrap();
    let edit = RecoveryEdit {
        index: 1,
        text: "unfinished upper edit".into(),
    };
    lease
        .save_with_edit("main draft", &["one".into(), "two".into()], Some(&edit))
        .unwrap();
    let loaded = lease.data().unwrap();
    assert_eq!(loaded.draft, "main draft");
    assert_eq!(loaded.queue, vec!["one", "two"]);
    assert_eq!(loaded.pending_edit, Some(edit));

    let legacy = br#"{"owner_pid":1,"draft":"old","queue":[],"updated_unix_ms":1}"#;
    let decoded: RecoveryData = serde_json::from_slice(legacy).unwrap();
    assert_eq!(decoded.pending_edit, None);
}

#[test]
fn failed_atomic_save_keeps_previous_record_and_can_be_repaired() {
    let d = tempfile::tempdir().unwrap();
    let lease = create(d.path()).unwrap();
    lease.save("previous", &["kept".into()]).unwrap();
    let record = lease.directory.join(RECORD_FILE);
    fs::remove_file(&record).unwrap();
    fs::create_dir(&record).unwrap();
    let result = lease.save("dirty revision", &["kept".into()]);
    assert!(result.is_err());
    fs::remove_dir(&record).unwrap();
    lease.save("dirty revision", &["kept".into()]).unwrap();
    assert_eq!(lease.data().unwrap().draft, "dirty revision");
}

#[test]
fn legacy_import_refuses_to_overwrite_an_existing_pane_record() {
    let d = tempfile::tempdir().unwrap();
    crate::tui::persist::save_draft(d.path(), "legacy draft");
    let lease = create(&d.path().join("records")).unwrap();
    lease
        .save("current pane", &["current queued".into()])
        .unwrap();
    assert_eq!(
        import_legacy(d.path(), &lease).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    let current = lease.data().unwrap();
    assert_eq!(current.draft, "current pane");
    assert_eq!(current.queue, vec!["current queued"]);
}
