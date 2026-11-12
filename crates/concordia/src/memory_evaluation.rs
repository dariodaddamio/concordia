//! Versioned synthetic evaluation driven through an existing suite; never compiled in production.
use super::*;
use std::cell::Cell;
use std::collections::BTreeSet;
use std::time::Instant;

thread_local! { static READ_BYTES: Cell<u64> = const { Cell::new(0) }; }

pub(super) fn count_read(bytes: usize) {
    READ_BYTES.with(|counter| counter.set(counter.get().saturating_add(bytes as u64)));
}

fn measured<T>(read: impl FnOnce() -> T) -> (T, u64, u128) {
    READ_BYTES.with(|counter| counter.set(0));
    let start = Instant::now();
    let value = read();
    (
        value,
        READ_BYTES.with(Cell::get),
        start.elapsed().as_micros(),
    )
}

fn precision_recall(actual: &[usize], relevant: &[usize]) -> Value {
    let relevant: BTreeSet<_> = relevant.iter().copied().collect();
    let found: BTreeSet<_> = actual.iter().copied().collect();
    let true_positive = found.intersection(&relevant).count();
    json!({"precision_at_12":(!actual.is_empty()).then(|| true_positive as f64 / actual.len() as f64),
        "recall_at_12":(!relevant.is_empty()).then(|| true_positive as f64 / relevant.len() as f64),
        "precision_at_1":actual.first().map(|first| usize::from(relevant.contains(first))),
        "relevant":relevant.len(),"returned":actual.len(),"true_positive":true_positive})
}

fn ordinals(value: &Value, key: &str) -> Vec<usize> {
    value[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|number| number.as_u64().unwrap() as usize)
        .collect()
}

pub(super) fn run() {
    let corpus_bytes = include_bytes!("../tests/fixtures/memory-evaluation-v1.json");
    let corpus: Value = serde_json::from_slice(corpus_bytes).unwrap();
    assert_eq!(corpus["version"], 1);
    for provider in [ProviderKind::Codex, ProviderKind::Claude] {
        for case in corpus["cases"].as_array().unwrap() {
            let dir = tempfile::tempdir().unwrap();
            let (path, session) = super::tests::fixture(provider, dir.path(), "fixture");
            let header = if provider == ProviderKind::Codex {
                json!({"type":"session_meta","payload":{"id":session,"cwd":dir.path()}})
            } else {
                json!({"type":"system","sessionId":session,"cwd":dir.path()})
            };
            fs::write(&path, format!("{header}\n")).unwrap();
            for (position, text) in case["texts"].as_array().unwrap().iter().enumerate() {
                let mut record: Value =
                    serde_json::from_str(&super::tests::message(provider, text.as_str().unwrap()))
                        .unwrap();
                record["timestamp"] = json!(if position == 0 {
                    "2026-11-01T06:59:59Z"
                } else {
                    "2026-11-03T06:59:59Z"
                });
                super::tests::append(&path, format!("{record}\n").as_bytes());
            }
            let context = case["context"].as_str().unwrap_or_default();
            if context == "file" {
                let file = case["path"].as_str().unwrap();
                let tool = if provider == ProviderKind::Codex {
                    json!({"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","input":format!("*** Begin Patch\n*** Update File: {file}\n@@\n-old\n+new\n*** End Patch")}})
                } else {
                    json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Read","id":"fixture-file","input":{"file_path":file}}]}})
                };
                super::tests::append(&path, format!("{tool}\n").as_bytes());
            }
            if case["malformed"] == true {
                super::tests::append(&path, b"{malformed\n");
            }
            if case["compaction"] == true {
                super::tests::append(&path, b"{\"type\":\"compacted\",\"payload\":{}}\n");
            }
            if case["oversized"] == true {
                super::tests::append(
                    &path,
                    super::tests::message(provider, &"x".repeat(LINE_BYTES + 100)).as_bytes(),
                );
            }
            for _ in 0..case["padding"].as_u64().unwrap_or(0) {
                super::tests::append(
                    &path,
                    super::tests::message(provider, "unrelated padding").as_bytes(),
                );
            }
            let page = index_session(dir.path(), provider, "fixture", &session).unwrap();
            if context == "goal" {
                crate::goals::save_goals(
                    dir.path(),
                    &[crate::goals::Goal {
                        id: "evaluation-goal".into(),
                        text: "Synthetic evaluation".into(),
                        done: false,
                        added_at: "0".into(),
                        closed_at: None,
                        ..Default::default()
                    }],
                )
                .unwrap();
                relations::attach(dir.path(), dir.path(), "evaluation-goal", &page.id).unwrap();
            }
            let query = case["query"].as_str().unwrap();
            let (lexical, lexical_bytes, lexical_us) = measured(|| {
                retrieval::recall(dir.path(), query, Some(&page.id), Some("fixture"), None).unwrap()
            });
            let (ranked, ranked_bytes, ranked_us) = measured(|| {
                rank::recall(
                    dir.path(),
                    rank::Request {
                        text: query,
                        source: Some(&page.id),
                        account: Some("fixture"),
                        workspace: Some(dir.path()),
                        goal: (context == "goal").then_some("evaluation-goal"),
                        file: case["path"].as_str(),
                        from: None,
                        at: (context == "time").then_some("2026-11-03T06:59:59Z"),
                    },
                )
                .unwrap()
            });
            let lexical_ordinals: Vec<_> = lexical.hits.iter().map(|hit| hit.message).collect();
            let ranked_ordinals: Vec<_> = ranked.hits.iter().map(|hit| hit.hit.message).collect();
            assert_eq!(
                lexical_ordinals,
                ordinals(case, "lexical"),
                "lexical {} {provider:?}",
                case["id"]
            );
            assert_eq!(
                ranked_ordinals,
                ordinals(case, "ranked"),
                "ranked {} {provider:?}",
                case["id"]
            );
            let incomplete = case["incomplete"].as_bool().unwrap_or(false);
            assert_eq!(
                lexical.incomplete, incomplete,
                "lexical coverage {}",
                case["id"]
            );
            assert_eq!(
                ranked.incomplete, incomplete,
                "ranked coverage {}",
                case["id"]
            );
            let relevant = ordinals(case, "expected");
            if !incomplete {
                assert!(relevant
                    .iter()
                    .all(|message| ranked_ordinals.contains(message)));
            }
            let mut resolved = 0;
            for hit in lexical
                .hits
                .iter()
                .chain(ranked.hits.iter().map(|hit| &hit.hit))
            {
                assert_eq!(hit.source.id, page.id);
                assert_eq!(hit.source.account, "fixture");
                let mut raw = String::new();
                let mut offset = 0;
                loop {
                    let excerpt = read(
                        dir.path(),
                        &hit.source.id,
                        hit.message,
                        offset,
                        EXCERPT_BYTES,
                    )
                    .unwrap();
                    assert_eq!(excerpt.sha256, hit.sha256);
                    raw.push_str(&excerpt.raw);
                    if let Some(next) = excerpt.next_offset {
                        offset = next;
                    } else {
                        break;
                    }
                }
                assert_eq!(digest(raw.as_bytes()), hit.sha256);
                resolved += 1;
            }
            let lexical_metadata = serde_json::to_vec(&lexical).unwrap().len();
            let ranked_metadata = serde_json::to_vec(&ranked).unwrap().len();
            assert!(serde_json::to_vec_pretty(&ranked).unwrap().len() <= 65536);
            println!(
                "MEMORY_EVALUATION {}",
                json!({"version":1,"corpus_sha256":digest(corpus_bytes),"provider":provider,"case":case["id"],
                "lexical":precision_recall(&lexical_ordinals,&relevant),"ranked":precision_recall(&ranked_ordinals,&relevant),
                "lexical_read_at_bytes":lexical_bytes,"ranked_read_at_bytes":ranked_bytes,"lexical_elapsed_us":lexical_us,"ranked_elapsed_us":ranked_us,
                "indexed_record_bytes_checked":ranked.record_bytes_checked,"retained_serialized_report_bytes":lexical_metadata+ranked_metadata,
                "resolved_excerpts":resolved,"returned_excerpts":lexical.hits.len()+ranked.hits.len(),"incomplete":incomplete,
                "notes":"read_at successful native bytes include repeated verification; excludes metadata/discovery/index construction. Serialized retained reports exclude allocator/container overhead; not process peak memory."})
            );
            // Negative integrity arms share each case's exact source and account binding.
            assert!(rank::recall(
                dir.path(),
                rank::Request {
                    text: query,
                    source: Some(&page.id),
                    account: Some("other"),
                    workspace: None,
                    goal: None,
                    file: None,
                    from: None,
                    at: None
                }
            )
            .is_err());
            fs::write(&path, "{}").unwrap();
            assert!(
                retrieval::recall(dir.path(), query, Some(&page.id), Some("fixture"), None)
                    .is_err()
            );
            let stale = rank::recall(
                dir.path(),
                rank::Request {
                    text: query,
                    source: Some(&page.id),
                    account: Some("fixture"),
                    workspace: None,
                    goal: None,
                    file: None,
                    from: None,
                    at: None,
                },
            )
            .unwrap();
            assert!(stale.hits.is_empty() && stale.incomplete);
            fs::remove_file(&path).unwrap();
            assert!(verify_source(dir.path(), &page.id).is_err());
        }
    }
}
