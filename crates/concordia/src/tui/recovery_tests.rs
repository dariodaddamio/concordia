use super::*;

#[test]
fn dismissed_recovery_stays_available_without_reappearing_at_startup() {
    let dir = tempfile::tempdir().unwrap();
    let saved = create(dir.path()).unwrap();
    saved
        .save("saved draft", &["queued prompt".into()])
        .unwrap();
    let id = saved.id().to_owned();
    drop(saved);
    dismiss(dir.path(), &available(dir.path()).unwrap()).unwrap();
    let (_, restored, choices) = startup_with_restore(dir.path()).unwrap();
    assert!(restored.is_none());
    assert!(choices.is_empty());
    assert!(available(dir.path())
        .unwrap()
        .iter()
        .any(|choice| choice.id == id));
    let saved = claim(dir.path(), &id).unwrap();
    assert_eq!(saved.data().unwrap().draft, "saved draft");
    saved.save("new version after dismissal", &[]).unwrap();
    // Give this revision a deterministic identity distinct from the dismissed one.
    let mut data = saved.data().unwrap();
    data.updated_unix_ms += 1;
    fs::write(
        saved.directory.join(RECORD_FILE),
        serde_json::to_vec(&data).unwrap(),
    )
    .unwrap();
    drop(saved);
    assert!(startup_with_restore(dir.path()).unwrap().1.is_some());
}
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
    assert_eq!(choices.len(), 3);
    assert!(choices
        .iter()
        .any(|choice| choice.draft.contains("Original kept unchanged")));
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
fn pending_edit_round_trips_and_old_record_json_defaults_it() {
    let d = tempfile::tempdir().unwrap();
    let lease = create(d.path()).unwrap();
    let edit = RecoveryEdit {
        index: 1,
        text: "unfinished upper edit".into(),
        attachments: None,
    };
    lease
        .save_with_alternate(
            "main draft",
            &["one".into(), "two".into()],
            Some(&edit),
            None,
        )
        .unwrap();
    let loaded = lease.data().unwrap();
    assert_eq!(loaded.draft, "main draft");
    assert_eq!(loaded.queue, vec!["one", "two"]);
    assert_eq!(loaded.pending_edit, Some(edit));

    let old_record = br#"{"owner_pid":1,"draft":"old","queue":[],"updated_unix_ms":1}"#;
    let decoded: RecoveryData = serde_json::from_slice(old_record).unwrap();
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

