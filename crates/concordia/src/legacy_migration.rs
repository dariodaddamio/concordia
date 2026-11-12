//! Explicit, revision-bound legacy conversion. Original bytes remain in immutable backups.
use crate::{
    attachments::{Attachment, Message},
    native_memory::{
        self,
        references::{self, Reference},
        Binding,
    },
    threads::ThreadEvent,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const OWNER: &str = "concordia-legacy-migration-v1";
const HISTORY_OWNER: &str = "concordia-migrated-history-v1";
const FILE_BYTES: u64 = 16 * 1024 * 1024;
const PLAN_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ROWS: usize = 64;
type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryRecord {
    owner: String,
    version: u32,
    source: Reference,
    attachments: Vec<Attachment>,
    original_sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Replacement {
    Thread { source: Reference },
    History { record: HistoryRecord },
}
impl Replacement {
    fn reference(&self) -> &Reference {
        match self {
            Self::Thread { source } => source,
            Self::History { record } => &record.source,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    line: usize,
    original_sha256: String,
    replacement: Replacement,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    owner: String,
    version: u32,
    target: String,
    source_index: String,
    binding: Binding,
    before_sha256: String,
    after_sha256: String,
    original_bytes: usize,
    total_lines: usize,
    unchanged_lines: usize,
    rows: Vec<Row>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    owner: String,
    version: u32,
    plan: String,
    status: String,
    before_sha256: String,
    after_sha256: String,
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn id_valid(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn owned_dir(path: &Path) -> Result<PathBuf> {
    crate::update::resolved_directory(path)
        .ok_or_else(|| "migration directory is not regular".into())
}

fn child_dir(parent: &Path, name: &str) -> Result<PathBuf> {
    let parent = owned_dir(parent)?;
    let path = parent.join(name);
    if !path.exists() {
        fs::create_dir(&path).map_err(error)?;
    }
    let resolved = owned_dir(&path)?;
    if resolved.parent() != Some(parent.as_path()) {
        return Err("migration directory escaped its owner".into());
    }
    Ok(resolved)
}

fn directory(root: &Path) -> Result<PathBuf> {
    child_dir(&child_dir(root, ".ao")?, "legacy-migrations")
}

fn bounded(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    let parent = owned_dir(path.parent().ok_or("migration file has no owner")?)?;
    if !crate::update::regular_owned_file(path, &parent) {
        return Err("migration file is not a regular owned file".into());
    }
    let mut data = Vec::new();
    fs::File::open(path)
        .map_err(error)?
        .take(maximum + 1)
        .read_to_end(&mut data)
        .map_err(error)?;
    if data.len() as u64 > maximum {
        return Err("migration file exceeds its byte budget".into());
    }
    Ok(data)
}

fn exclusive(path: &Path, data: &[u8]) -> Result<()> {
    if path.exists() {
        if bounded(path, FILE_BYTES.max(PLAN_BYTES))? == data {
            return Ok(());
        }
        return Err("existing migration file differs; original preserved".into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(error)?;
    file.write_all(data)
        .and_then(|_| file.sync_all())
        .map_err(error)
}

fn atomic(path: &Path, data: &[u8]) -> Result<()> {
    if path.exists() {
        let _ = bounded(path, FILE_BYTES.max(PLAN_BYTES))?;
    }
    let temporary = path.with_file_name(format!("migration-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(error)?;
    file.write_all(data)
        .and_then(|_| file.sync_all())
        .map_err(error)?;
    drop(file);
    let result = fs::rename(&temporary, path).map_err(error);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn target_guard(path: &Path) -> Result<fs::File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Permit readers and our atomic replacement, but exclude legacy append writers.
        options.share_mode(0x1 | 0x4);
    }
    options.open(path).map_err(error)
}

fn target_path(root: &Path, target: &str) -> Result<PathBuf> {
    if target == "history" {
        return Ok(owned_dir(root)?.join("history.jsonl"));
    }
    if target.is_empty()
        || target.len() > 128
        || !target
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err("invalid legacy thread identity".into());
    }
    Ok(owned_dir(&root.join("threads"))?.join(format!("{target}.jsonl")))
}

fn lock_path(root: &Path, target: &str) -> Result<PathBuf> {
    let path = if target == "history" {
        owned_dir(root)?.join("history.legacy-migration.lock")
    } else {
        owned_dir(&root.join("threads"))?.join(format!("{target}.lock"))
    };
    if path.exists() {
        let _ = bounded(&path, FILE_BYTES)?;
    }
    Ok(path)
}

fn hydrate_history(root: &Path, record: &HistoryRecord) -> Result<String> {
    if record.owner != HISTORY_OWNER
        || record.version != 1
        || record.source.role() != "user"
        || !id_valid(&record.original_sha256)
    {
        return Err("unsupported migrated history record".into());
    }
    let message = Message {
        text: references::hydrate_text(root, &record.source)?,
        attachments: record.attachments.clone(),
        invalid: None,
    };
    validate_message(&message)?;
    let encoded = message.encode();
    if sha(encoded.as_bytes()) != record.original_sha256 {
        return Err("migrated history projection changed".into());
    }
    Ok(encoded)
}

fn validate_message(message: &Message) -> Result<()> {
    message.validate()?;
    if message.attachments.iter().any(|attachment| {
        let hash = attachment.hash();
        hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err("invalid migrated attachment metadata".into());
    }
    Ok(())
}

/// Legacy strings remain readable; unknown or stale reference objects never become prompts.
pub fn hydrate_history_line(root: &Path, line: &str) -> Option<String> {
    if let Ok(text) = serde_json::from_str::<String>(line) {
        return Some(text);
    }
    let record = serde_json::from_str::<HistoryRecord>(line).ok()?;
    hydrate_history(root, &record).ok()
}

fn lines(bytes: &[u8]) -> Result<Vec<&str>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "legacy source encoding is invalid; original retained")?;
    if !text.is_empty() && !text.ends_with('\n') {
        return Err("legacy source has an unfinished record; original retained".into());
    }
    let rows = text.split_inclusive('\n').collect::<Vec<_>>();
    if rows.len() > 20_000 {
        return Err("legacy source exceeds the line budget".into());
    }
    Ok(rows)
}

fn replacement_line(root: &Path, original: &str, replacement: &Replacement) -> Result<String> {
    match replacement {
        Replacement::Thread { source } => {
            let event: ThreadEvent = serde_json::from_str(original).map_err(error)?;
            crate::threads::referenced_legacy_line(root, &event, source.clone())
        }
        Replacement::History { record } => {
            let original: String = serde_json::from_str(original).map_err(error)?;
            if hydrate_history(root, record)? != original {
                return Err("history conversion differs from original".into());
            }
            serde_json::to_string(record).map_err(error)
        }
    }
}

fn render(root: &Path, plan: &Plan, original: &[u8]) -> Result<Vec<u8>> {
    if original.len() != plan.original_bytes || sha(original) != plan.before_sha256 {
        return Err("legacy source changed since preview".into());
    }
    let original_lines = lines(original)?;
    if original_lines.len() != plan.total_lines {
        return Err("legacy source line count changed".into());
    }
    let mut output = Vec::new();
    let mut next = plan.rows.iter().peekable();
    for (index, line) in original_lines.into_iter().enumerate() {
        if next.peek().is_some_and(|row| row.line == index + 1) {
            let row = next.next().unwrap();
            if sha(line.as_bytes()) != row.original_sha256
                || row.replacement.reference().binding() != &plan.binding
            {
                return Err("migration row/source binding changed".into());
            }
            if (plan.target == "history") != matches!(&row.replacement, Replacement::History { .. })
            {
                return Err("migration target kind changed".into());
            }
            let converted = replacement_line(root, line, &row.replacement)?;
            output.extend_from_slice(converted.as_bytes());
            output.push(b'\n');
        } else {
            output.extend_from_slice(line.as_bytes());
        }
    }
    if next.next().is_some() || output.len() as u64 > FILE_BYTES * 2 {
        return Err("migration output exceeds its budget".into());
    }
    Ok(output)
}

fn preview(root: &Path, requested: &str, source: &str) -> Result<String> {
    preview_from(root, requested, source, 1)
}

fn preview_from(root: &Path, requested: &str, source: &str, from_line: usize) -> Result<String> {
    if !id_valid(source) {
        return Err("invalid explicit native source index".into());
    }
    let summary = native_memory::verify_source(root, source)?;
    let account =
        crate::accounts::get_account(root, &summary.account).ok_or("native account unavailable")?;
    let home = account.home_path.ok_or("native account home unavailable")?;
    let binding = Binding::new(
        summary.provider,
        &summary.account,
        &home,
        &summary.session_id,
    )?;
    let scope = references::historical_scope(root, source)?;
    if scope.binding() != &binding {
        return Err("selected native source binding changed".into());
    }
    let target = if requested == "history" {
        "history".into()
    } else {
        crate::threads::resolve_prefix(root, requested).map_err(error)?
    };
    let path = target_path(root, &target)?;
    let original = bounded(&path, FILE_BYTES)?;
    let original_lines = lines(&original)?;
    if from_line == 0 || from_line > original_lines.len() + 1 {
        return Err("migration from-line is outside the exact legacy source".into());
    }
    let mut used = std::collections::HashSet::new();
    let mut duplicates = std::collections::HashSet::new();
    for line in &original_lines {
        let key = if target == "history" {
            serde_json::from_str::<String>(line)
                .ok()
                .map(|raw| format!("user:{}", sha(Message::text(&raw).as_bytes())))
        } else {
            serde_json::from_str::<ThreadEvent>(line)
                .ok()
                .and_then(|event| {
                    event
                        .text
                        .map(|text| format!("{}:{}", event.k, sha(text.as_bytes())))
                })
        };
        if let Some(key) = key {
            if !used.insert(key.clone()) {
                duplicates.insert(key);
            }
        }
    }
    let mut rows = Vec::new();
    let mut attempts = 0usize;
    let mut source_ambiguities = 0usize;
    let mut source_unavailable = 0usize;
    let mut capture =
        |role: &str, text: &str| match references::capture_text(root, &scope, role, text) {
            Ok(reference) => Some(reference),
            Err(error) => {
                if error.contains("ambiguous") {
                    source_ambiguities += 1;
                } else {
                    source_unavailable += 1;
                }
                None
            }
        };
    let mut scanned_through = from_line - 1;
    for (ordinal, line) in original_lines.iter().enumerate().skip(from_line - 1) {
        if attempts >= MAX_ROWS {
            break;
        }
        scanned_through = ordinal + 1;
        let candidate = if target == "history" {
            serde_json::from_str::<String>(line).ok().and_then(|raw| {
                let message = Message::decode(&raw);
                if validate_message(&message).is_err()
                    || message.encode() != raw
                    || message.text.is_empty()
                {
                    return None;
                }
                if duplicates.contains(&format!("user:{}", sha(message.text.as_bytes()))) {
                    return None;
                }
                attempts += 1;
                let reference = capture("user", &message.text)?;
                Some(Replacement::History {
                    record: HistoryRecord {
                        owner: HISTORY_OWNER.into(),
                        version: 1,
                        source: reference,
                        attachments: message.attachments,
                        original_sha256: sha(raw.as_bytes()),
                    },
                })
            })
        } else {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .filter(|value| value.get("version").is_none())
                .and_then(|value| serde_json::from_value::<ThreadEvent>(value).ok())
                .and_then(|event| {
                    if !matches!(event.k.as_str(), "user" | "assistant")
                        || event.card.is_some()
                        || event
                            .provider
                            .as_deref()
                            .is_some_and(|p| p != binding.provider.as_str())
                        || event
                            .account
                            .as_deref()
                            .is_some_and(|a| a != binding.account)
                    {
                        return None;
                    }
                    let text = event.text.as_deref().filter(|text| !text.is_empty())?;
                    if duplicates.contains(&format!("{}:{}", event.k, sha(text.as_bytes()))) {
                        return None;
                    }
                    attempts += 1;
                    capture(&event.k, text).map(|source| Replacement::Thread { source })
                })
        };
        if let Some(replacement) = candidate {
            rows.push(Row {
                line: ordinal + 1,
                original_sha256: sha(line.as_bytes()),
                replacement,
            });
        }
    }
    let mut plan = Plan {
        owner: OWNER.into(),
        version: 1,
        target,
        source_index: source.into(),
        binding,
        before_sha256: sha(&original),
        after_sha256: String::new(),
        original_bytes: original.len(),
        total_lines: original_lines.len(),
        unchanged_lines: original_lines.len() - rows.len(),
        rows,
    };
    plan.after_sha256 = sha(&render(root, &plan, &original)?);
    native_memory::verify_source(root, source)?;
    if bounded(&path, FILE_BYTES)? != original {
        return Err("legacy source changed during preview".into());
    }
    let encoded = serde_json::to_vec(&plan).map_err(error)?;
    if encoded.len() as u64 > PLAN_BYTES {
        return Err("migration plan exceeds its metadata budget".into());
    }
    let id = sha(&encoded);
    let dir = child_dir(&directory(root)?, &id)?;
    exclusive(&dir.join("plan.json"), &encoded)?;
    Ok(serde_json::json!({"plan":id,"target":plan.target,"source_index":source,"binding":plan.binding,
        "convertible_rows":plan.rows.len(),"converted_lines":plan.rows.iter().map(|row|row.line).collect::<Vec<_>>(),
        "preserved_rows":plan.unchanged_lines,"original_sha256":plan.before_sha256,"replacement_sha256":plan.after_sha256,
        "original_unchanged":true,"match_attempt_limit":MAX_ROWS,"match_attempts":attempts,
        "ambiguous_native_matches":source_ambiguities,"unavailable_native_matches":source_unavailable,
        "ambiguous_legacy_values":duplicates.len(),
        "scanned_through_line":scanned_through,"next_line":(scanned_through<plan.total_lines).then_some(scanned_through+1),
        "continuation":"Use --from-line NEXT_LINE for the next bounded batch, or preview again after applying converted rows",
        "backup_policy":"Exact original bytes retained; no archive deletion", "next":format!("concordia chat thread migrate apply {id}")}).to_string())
}

fn load_plan(root: &Path, id: &str) -> Result<(Plan, PathBuf)> {
    if !id_valid(id) {
        return Err("invalid migration plan identity".into());
    }
    let dir = owned_dir(&directory(root)?.join(id))?;
    let data = bounded(&dir.join("plan.json"), PLAN_BYTES)?;
    if sha(&data) != id {
        return Err("migration plan revision changed".into());
    }
    let plan: Plan = serde_json::from_slice(&data).map_err(error)?;
    if plan.owner != OWNER
        || plan.version != 1
        || plan.rows.len() > MAX_ROWS
        || !id_valid(&plan.before_sha256)
        || !id_valid(&plan.after_sha256)
        || !id_valid(&plan.source_index)
        || plan
            .rows
            .windows(2)
            .any(|pair| pair[0].line >= pair[1].line)
        || plan.rows.iter().any(|row| {
            row.line == 0
                || row.line > plan.total_lines
                || !id_valid(&row.original_sha256)
                || row.replacement.reference().binding() != &plan.binding
        })
    {
        return Err("invalid migration plan contract".into());
    }
    let _ = target_path(root, &plan.target)?;
    Ok((plan, dir))
}

fn inspect(root: &Path, id: &str, plan: &Plan, dir: &Path) -> Result<String> {
    let current = sha(&bounded(&target_path(root, &plan.target)?, FILE_BYTES * 2)?);
    let backup = dir.join("original.jsonl");
    let backup_ok = backup.exists() && sha(&bounded(&backup, FILE_BYTES)?) == plan.before_sha256;
    let receipt_path = dir.join("receipt.json");
    let receipt = if receipt_path.exists() {
        let receipt: Receipt =
            serde_json::from_slice(&bounded(&receipt_path, PLAN_BYTES)?).map_err(error)?;
        if receipt.owner != OWNER
            || receipt.version != 1
            || receipt.plan != id
            || receipt.before_sha256 != plan.before_sha256
            || receipt.after_sha256 != plan.after_sha256
            || !matches!(receipt.status.as_str(), "prepared" | "applied")
        {
            return Err("migration receipt differs from exact plan".into());
        }
        Some(receipt)
    } else {
        None
    };
    let status = if plan.rows.is_empty() && current == plan.before_sha256 {
        "no_changes"
    } else if current == plan.after_sha256 && backup_ok {
        "applied"
    } else if current == plan.before_sha256
        && receipt.as_ref().is_some_and(|r| r.status == "applied")
    {
        "conflict"
    } else if current == plan.before_sha256 && receipt.is_some() && backup_ok {
        "prepared"
    } else if current == plan.before_sha256 && receipt.is_none() {
        "preview"
    } else {
        "conflict"
    };
    Ok(serde_json::json!({"plan":id,"target":plan.target,"status":status,"backup_verified":backup_ok,
        "converted_rows":plan.rows.len(),"preserved_rows":plan.unchanged_lines,"current_sha256":current}).to_string())
}

fn receipt(dir: &Path, id: &str, plan: &Plan, status: &str) -> Result<()> {
    let record = Receipt {
        owner: OWNER.into(),
        version: 1,
        plan: id.into(),
        status: status.into(),
        before_sha256: plan.before_sha256.clone(),
        after_sha256: plan.after_sha256.clone(),
    };
    atomic(
        &dir.join("receipt.json"),
        &serde_json::to_vec(&record).map_err(error)?,
    )
}

fn apply(root: &Path, id: &str) -> Result<String> {
    let (plan, dir) = load_plan(root, id)?;
    let lock = lock_path(root, &plan.target)?;
    crate::state_io::with_lock(&lock, || {
        let operation = || -> Result<String> {
            let status: serde_json::Value =
                serde_json::from_str(&inspect(root, id, &plan, &dir)?).map_err(error)?;
            match status["status"].as_str() {
                Some("applied") => {
                    receipt(&dir, id, &plan, "applied")?;
                    return Ok(status.to_string());
                }
                Some("no_changes") => return Ok(status.to_string()),
                Some("preview" | "prepared") => {}
                _ => return Err("migration conflicts with current bytes; nothing replaced".into()),
            }
            native_memory::verify_source(root, &plan.source_index)?;
            let path = target_path(root, &plan.target)?;
            let _target_guard = target_guard(&path)?;
            let original = bounded(&path, FILE_BYTES)?;
            let converted = render(root, &plan, &original)?;
            if sha(&converted) != plan.after_sha256 {
                return Err("migration output differs from preview".into());
            }
            exclusive(&dir.join("original.jsonl"), &original)?;
            receipt(&dir, id, &plan, "prepared")?;
            native_memory::verify_source(root, &plan.source_index)?;
            // Rehydrate every native reference immediately before replacing the legacy file.
            if render(root, &plan, &original)? != converted
                || bounded(&path, FILE_BYTES)? != original
            {
                return Err("source changed during migration; originals retained".into());
            }
            atomic(&path, &converted)?;
            receipt(&dir, id, &plan, "applied")?;
            inspect(root, id, &plan, &dir)
        };
        operation().map_err(std::io::Error::other)
    })
    .map_err(error)
}

pub fn run_cli(root: &Path, args: &[String]) -> std::result::Result<String, (String, i32)> {
    let usage="usage: concordia chat thread migrate preview <thread-id|history> --source INDEX [--from-line N] | apply PLAN | status PLAN";
    let result = match args {
        [action, target, flag, source] if action == "preview" && flag == "--source" => {
            preview(root, target, source)
        }
        [action, target, flag, source, next, line]
            if action == "preview" && flag == "--source" && next == "--from-line" =>
        {
            line.parse::<usize>()
                .map_err(|_| "--from-line needs a positive ordinal".to_owned())
                .and_then(|line| preview_from(root, target, source, line))
        }
        [action, id] if action == "apply" => apply(root, id),
        [action, id] if action == "status" => {
            load_plan(root, id).and_then(|(plan, dir)| inspect(root, id, &plan, &dir))
        }
        _ => return Err((usage.into(), 2)),
    };
    result
        .map(|value| format!("{value}\n"))
        .map_err(|error| (error, 1))
}

#[cfg(test)]
pub(crate) fn fixture_proof() {
    use crate::store::{ProviderKind, StoredAccount};
    use serde_json::json;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let home = root.join("fixture-home");
    fs::create_dir_all(home.join("sessions")).unwrap();
    crate::store::upsert_stored_account(
        root,
        StoredAccount {
            id: "migration-fixture".into(),
            provider: ProviderKind::Codex,
            home_path: Some(home.to_string_lossy().into_owned()),
            label: "Synthetic migration fixture".into(),
            created_at: "0".into(),
        },
    )
    .unwrap();
    let session = "00000000-0000-4000-8000-00000000005b";
    let native = home.join("sessions").join(format!("{session}.jsonl"));
    let mut records = vec![json!({"type":"session_meta","payload":{"id":session}})];
    for (role, text) in [
        ("user", "migration request café"),
        ("assistant", "migration reply"),
        ("user", "repeat"),
        ("user", "repeat"),
        ("user", "history envelope"),
    ] {
        records.push(json!({"type":"response_item","payload":{"type":"message","role":role,"content":[{"type":if role=="user" {"input_text"}else{"output_text"},"text":text}]}}));
    }
    let native_bytes = records
        .iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>()
        .into_bytes();
    fs::write(&native, &native_bytes).unwrap();
    let binding = Binding::new(ProviderKind::Codex, "migration-fixture", &home, session).unwrap();
    let source = native_memory::index_bound(root, &binding).unwrap().id;
    let thread = crate::threads::ThreadLog::open(root, "migration-fixture");
    thread
        .write_meta(&crate::threads::ThreadMeta {
            id: thread.id().into(),
            started: 0,
            title: "fixture".into(),
            cwd: String::new(),
            account: None,
        })
        .unwrap();
    let events = [
        ThreadEvent::with_text("user", "migration request café"),
        ThreadEvent::with_text("assistant", "migration reply"),
        ThreadEvent::with_text("user", "repeat"),
        ThreadEvent::card("Run command", Some("legacy tool body"), "done"),
    ];
    let mut legacy = events
        .iter()
        .map(|event| format!("{}\r\n", serde_json::to_string(event).unwrap()))
        .collect::<String>();
    legacy.push_str("unsupported malformed row\r\n");
    let target = root.join("threads/migration-fixture.jsonl");
    fs::write(&target, legacy.as_bytes()).unwrap();
    let preview_value: serde_json::Value =
        serde_json::from_str(&preview(root, thread.id(), &source).unwrap()).unwrap();
    let id = preview_value["plan"].as_str().unwrap();
    assert_eq!(preview_value["convertible_rows"], 2);
    assert_eq!(fs::read(&target).unwrap(), legacy.as_bytes());
    let (plan, dir) = load_plan(root, id).unwrap();
    let plan_bytes = fs::read(dir.join("plan.json")).unwrap();
    let plan_text = String::from_utf8(plan_bytes).unwrap();
    assert!(
        !plan_text.contains("migration request")
            && !plan_text.contains("migration reply")
            && !plan_text.contains("legacy tool body")
    );
    fs::write(&target, format!("{legacy}{{}}\n")).unwrap();
    assert!(
        apply(root, id).is_err(),
        "a concurrent append invalidates the reviewed legacy revision"
    );
    fs::write(&target, legacy.as_bytes()).unwrap();
    fs::write(
        &native,
        String::from_utf8(native_bytes.clone())
            .unwrap()
            .replace("migration reply", "MIGRATION REPLY"),
    )
    .unwrap();
    assert!(
        apply(root, id).is_err(),
        "rewritten native source cannot replace legacy originals"
    );
    assert_eq!(fs::read(&target).unwrap(), legacy.as_bytes());
    fs::write(&native, &native_bytes).unwrap();
    OpenOptions::new().append(true).open(&native).unwrap().write_all(
        format!("{}\n",json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"migration request café"}]}})).as_bytes()
    ).unwrap();
    let applied: serde_json::Value = serde_json::from_str(&apply(root, id).unwrap()).unwrap();
    assert_eq!(applied["status"], "applied");
    let converted = fs::read(&target).unwrap();
    assert_eq!(
        fs::read(dir.join("original.jsonl")).unwrap(),
        legacy.as_bytes()
    );
    assert!(String::from_utf8_lossy(&converted).contains("legacy tool body"));
    assert!(String::from_utf8_lossy(&converted).contains("unsupported malformed row\r\n"));
    assert!(!String::from_utf8_lossy(&converted).contains("migration request café"));
    let hydrated = thread.read_all().unwrap();
    assert_eq!(hydrated[0].text.as_deref(), Some("migration request café"));
    assert_eq!(hydrated[1].text.as_deref(), Some("migration reply"));
    assert_eq!(hydrated[2].text.as_deref(), Some("repeat"));
    apply(root, id).unwrap();
    assert_eq!(
        fs::read(&target).unwrap(),
        converted,
        "repeat apply only reconciles already committed bytes"
    );
    receipt(&dir, id, &plan, "prepared").unwrap();
    assert!(inspect(root, id, &plan, &dir)
        .unwrap()
        .contains("\"status\":\"applied\""));
    apply(root, id).unwrap();
    // Simulate interruption after backup/prepare and before the known replacement.
    fs::write(&target, legacy.as_bytes()).unwrap();
    receipt(&dir, id, &plan, "prepared").unwrap();
    assert!(inspect(root, id, &plan, &dir)
        .unwrap()
        .contains("\"status\":\"prepared\""));
    apply(root, id).unwrap();
    assert_eq!(fs::read(&target).unwrap(), converted);
    fs::write(&target, b"foreign replacement\n").unwrap();
    assert!(apply(root, id).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"foreign replacement\n");
    assert_eq!(
        fs::read(dir.join("original.jsonl")).unwrap(),
        legacy.as_bytes()
    );

    let history = "  history envelope  ";
    // Whitespace cannot be silently trimmed merely to produce a match.
    fs::write(
        root.join("history.jsonl"),
        format!("{}\n", serde_json::to_string(history).unwrap()),
    )
    .unwrap();
    let unmatched: serde_json::Value =
        serde_json::from_str(&preview(root, "history", &source).unwrap()).unwrap();
    assert_eq!(unmatched["convertible_rows"], 0);
    let history = "migration request café";
    let raw = format!(
        "{}\r\n{{\"unknown\":true}}\r\n",
        serde_json::to_string(history).unwrap()
    );
    fs::write(root.join("history.jsonl"), raw.as_bytes()).unwrap();
    let preview_value: serde_json::Value =
        serde_json::from_str(&preview(root, "history", &source).unwrap()).unwrap();
    let id = preview_value["plan"].as_str().unwrap();
    apply(root, id).unwrap();
    let converted = fs::read_to_string(root.join("history.jsonl")).unwrap();
    assert!(!converted.contains(history));
    assert!(converted.contains("{\"unknown\":true}\r\n"));
    let first = converted.lines().next().unwrap();
    assert_eq!(hydrate_history_line(root, first).as_deref(), Some(history));
    assert_eq!(
        hydrate_history_line(root, &serde_json::to_string(history).unwrap()).as_deref(),
        Some(history)
    );
    assert!(hydrate_history_line(root, "{\"unknown\":true}").is_none());
    let original_path = load_plan(root, id).unwrap().1.join("original.jsonl");
    assert_eq!(fs::read(original_path).unwrap(), raw.as_bytes());

    // Canonical attachment envelopes retain descriptor order and exact original bytes.
    let message = Message {
        text: "history envelope".into(),
        attachments: vec![Attachment::Image(crate::attachments::Image {
            path: root.join("snapshot.png"),
            name: "snapshot.png".into(),
            mime: "image/png".into(),
            bytes: 12,
            sha256: "a".repeat(64),
        })],
        invalid: None,
    };
    let envelope = message.encode();
    fs::write(
        root.join("history.jsonl"),
        format!("{}\n", serde_json::to_string(&envelope).unwrap()),
    )
    .unwrap();
    let selected: serde_json::Value =
        serde_json::from_str(&preview(root, "history", &source).unwrap()).unwrap();
    assert_eq!(selected["convertible_rows"], 1);
    let envelope_id = selected["plan"].as_str().unwrap();
    apply(root, envelope_id).unwrap();
    let converted = fs::read_to_string(root.join("history.jsonl")).unwrap();
    assert_eq!(
        hydrate_history_line(root, converted.lines().next().unwrap()).as_deref(),
        Some(envelope.as_str())
    );

    // More than one batch makes progress; failed lookups also have a bounded continuation.
    let session2 = "00000000-0000-4000-8000-00000000006b";
    let mut native2 = format!(
        "{}\n",
        json!({"type":"session_meta","payload":{"id":session2}})
    );
    let mut legacy2 = String::new();
    for index in 0..70 {
        let text = format!("bounded request {index}.");
        native2.push_str(&format!("{}\n",json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":text}]}})));
        legacy2.push_str(&format!(
            "{}\n",
            serde_json::to_string(&ThreadEvent::with_text("user", &text)).unwrap()
        ));
    }
    let native2_path = home.join("sessions").join(format!("{session2}.jsonl"));
    fs::write(&native2_path, native2).unwrap();
    let binding2 = Binding::new(ProviderKind::Codex, "migration-fixture", &home, session2).unwrap();
    let source2 = native_memory::index_bound(root, &binding2).unwrap().id;
    let target2 = root.join("threads/bounded-fixture.jsonl");
    fs::write(&target2, legacy2).unwrap();
    let batch1: serde_json::Value =
        serde_json::from_str(&preview(root, "bounded-fixture", &source2).unwrap()).unwrap();
    assert_eq!(batch1["convertible_rows"], 64);
    assert_eq!(batch1["next_line"], 65);
    apply(root, batch1["plan"].as_str().unwrap()).unwrap();
    let batch2: serde_json::Value =
        serde_json::from_str(&preview(root, "bounded-fixture", &source2).unwrap()).unwrap();
    assert_eq!(batch2["convertible_rows"], 6);
    apply(root, batch2["plan"].as_str().unwrap()).unwrap();
    assert_eq!(
        crate::threads::ThreadLog::open(root, "bounded-fixture")
            .read_all()
            .unwrap()
            .len(),
        70
    );
    let mut misses = (0..64)
        .map(|index| {
            format!(
                "{}\n",
                serde_json::to_string(&ThreadEvent::with_text(
                    "user",
                    &format!("unmatched {index}")
                ))
                .unwrap()
            )
        })
        .collect::<String>();
    misses.push_str(&format!(
        "{}\n",
        serde_json::to_string(&ThreadEvent::with_text("user", "bounded request 0.")).unwrap()
    ));
    fs::write(root.join("threads/unmatched-fixture.jsonl"), misses).unwrap();
    let missed: serde_json::Value =
        serde_json::from_str(&preview(root, "unmatched-fixture", &source2).unwrap()).unwrap();
    assert_eq!(missed["convertible_rows"], 0);
    assert_eq!(missed["match_attempts"], 64);
    assert_eq!(missed["next_line"], 65);
    let continued: serde_json::Value =
        serde_json::from_str(&preview_from(root, "unmatched-fixture", &source2, 65).unwrap())
            .unwrap();
    assert_eq!(continued["convertible_rows"], 1);

    fs::remove_file(&native).unwrap();
    assert!(
        hydrate_history_line(root, first).is_none(),
        "missing source is skipped, never a placeholder prompt"
    );
    assert!(
        apply(root, envelope_id)
            .unwrap()
            .contains("\"status\":\"applied\""),
        "completed migration reconciliation does not rerun unavailable sources"
    );
}
