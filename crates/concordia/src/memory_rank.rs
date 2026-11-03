//! Opt-in deterministic retrieval. Evidence never grants a relationship or bypasses source checks.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

const PAGES: usize = 8;
const HITS: usize = 12;
const RECORD_BYTES: u64 = 64 * 1024 * 1024;

pub struct Request<'a> {
    pub text: &'a str,
    pub source: Option<&'a str>,
    pub account: Option<&'a str>,
    pub workspace: Option<&'a Path>,
    pub goal: Option<&'a str>,
    pub file: Option<&'a str>,
    pub from: Option<&'a str>,
    pub at: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Reason {
    pub kind: &'static str,
    pub points: u32,
    pub detail: String,
    pub evidence: Value,
}

#[derive(Debug, Serialize)]
pub struct Hit {
    #[serde(flatten)]
    pub hit: retrieval::Hit,
    pub score: u32,
    pub reasons: Vec<Reason>,
    pub recorded_workspace: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub version: u8,
    pub hits: Vec<Hit>,
    pub pages: Vec<Summary>,
    pub incomplete: bool,
    pub unavailable: Vec<String>,
    pub limits: Vec<String>,
    pub record_bytes_checked: u64,
    pub scope: Value,
}

/// Millisecond-resolution RFC 3339 subset. Unsupported/leap-second dates remain unknown.
pub(super) fn timestamp_ms(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if !text.is_ascii()
        || !(20..=40).contains(&bytes.len())
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let number = |start: usize, end: usize| {
        let value = text.get(start..end)?;
        value
            .bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| value.parse::<u32>().ok())
            .flatten()
    };
    let (year, month, day, hour, minute, second) = (
        number(0, 4)?,
        number(5, 7)?,
        number(8, 10)?,
        number(11, 13)?,
        number(14, 16)?,
        number(17, 19)?,
    );
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = crate::prompt_time::days_from_civil(year as i32, month, day);
    if crate::prompt_time::civil_from_days(days) != (year as i32, month, day) {
        return None;
    }
    let mut zone = 19;
    let mut millis = 0;
    if bytes.get(zone) == Some(&b'.') {
        zone += 1;
        let start = zone;
        while bytes.get(zone).is_some_and(u8::is_ascii_digit) {
            zone += 1;
        }
        let count = zone - start;
        if !(1..=9).contains(&count) {
            return None;
        }
        millis = number(start, start + count.min(3))?;
        for _ in count..3 {
            millis *= 10;
        }
    }
    let offset = match &bytes[zone..] {
        [b'Z' | b'z'] => 0,
        [sign @ (b'+' | b'-'), _, _, b':', _, _] => {
            let (hours, minutes) = (number(zone + 1, zone + 3)?, number(zone + 4, zone + 6)?);
            if hours > 23 || minutes > 59 {
                return None;
            }
            let offset = i64::from(hours * 3600 + minutes * 60);
            if *sign == b'-' {
                -offset
            } else {
                offset
            }
        }
        _ => return None,
    };
    Some(
        (days * 86400 + i64::from(hour * 3600 + minute * 60 + second) - offset) * 1000
            + i64::from(millis),
    )
}

pub(super) fn same_workspace(left: &Path, right: &Path) -> bool {
    files::normalize_workspace(left.to_str().unwrap_or(""))
        .zip(files::normalize_workspace(right.to_str().unwrap_or("")))
        .is_some_and(|(left, right)| {
            crate::workspaces::same_key(
                &crate::workspaces::path_key(&left),
                &crate::workspaces::path_key(&right),
            )
        })
}

fn bounded_entries(path: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut paths = Vec::new();
    for entry in entries.take(513) {
        paths.push(entry.map_err(|error| error.to_string())?.path());
    }
    if paths.len() > 512 {
        return Err("relationship directory exceeds 512 entries; suggestions withheld rather than choosing an unstable subset".into());
    }
    paths.sort();
    Ok(paths)
}

#[derive(Clone, Serialize)]
enum Candidate {
    Pointer(String),
    Session(Binding),
}

#[derive(Clone, Serialize)]
struct HandoffReason {
    candidate: Candidate,
    reference: String,
    label: &'static str,
}

fn neighbors(
    root: &Path,
    anchor: &str,
    index: &SourceIndex,
    workspace: Option<&Path>,
) -> Result<Vec<HandoffReason>, String> {
    let mut rows = Vec::new();
    for path in bounded_entries(&directory(root).join("lineage"))? {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("invalid lineage filename")?;
        if !name.starts_with(anchor) && !name.ends_with(&format!("{anchor}.json")) {
            continue;
        }
        let meta = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if !meta.is_file() || crate::workspaces::linked(&meta) {
            return Err("lineage must use regular owned metadata".into());
        }
        let mut bytes = Vec::new();
        File::open(&path)
            .and_then(|file| file.take(4097).read_to_end(&mut bytes))
            .map_err(|error| error.to_string())?;
        if bytes.len() > 4096 {
            return Err("lineage metadata exceeds 4 KiB".into());
        }
        let edge: retrieval::Link =
            serde_json::from_slice(&bytes).map_err(|_| "invalid lineage metadata")?;
        if !valid_id(&edge.parent)
            || !valid_id(&edge.child)
            || edge.parent == edge.child
            || name != format!("{}-{}.json", edge.parent, edge.child)
            || !matches!(
                edge.reason.as_str(),
                "user-confirmed" | "context-handoff" | "worker"
            )
        {
            return Err("unsupported lineage identity or provenance".into());
        }
        let other = if edge.parent == anchor {
            edge.child
        } else if edge.child == anchor {
            edge.parent
        } else {
            return Err("lineage source mismatch".into());
        };
        rows.push(HandoffReason {
            candidate: Candidate::Pointer(other),
            reference: format!("lineage/{name}"),
            label: "explicit source relationship",
        });
    }
    for path in bounded_entries(&directory(root).join("handoffs"))? {
        let meta = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if !meta.is_file() || crate::workspaces::linked(&meta) {
            return Err("handoffs must use regular owned metadata".into());
        }
        let event = retrieval::read_handoff(&path)?;
        let bytes = serde_json::to_vec(&event).map_err(|error| error.to_string())?;
        if path.file_stem().and_then(|name| name.to_str()) != Some(digest(&bytes).as_str()) {
            return Err("handoff metadata identity mismatch".into());
        }
        if workspace.is_some_and(|workspace| !same_workspace(workspace, &event.workspace)) {
            continue;
        }
        let other = if event.parent == index.binding {
            event.child
        } else if event.child == index.binding {
            event.parent
        } else {
            continue;
        };
        rows.push(HandoffReason {
            candidate: Candidate::Session(other),
            reference: format!("handoffs/{}.json", digest(&bytes)),
            label: "observed Concordia handoff; not native ancestry",
        });
    }
    Ok(rows)
}

fn candidate_matches(candidate: &Candidate, id: &str, index: &SourceIndex) -> bool {
    match candidate {
        Candidate::Pointer(pointer) => pointer == id,
        Candidate::Session(binding) => *binding == index.binding,
    }
}

struct Evidence<'a> {
    goals: &'a [relations::Relation],
    handoffs: &'a [HandoffReason],
}

fn rank_page(
    root: &Path,
    id: &str,
    request: &Request<'_>,
    evidence: &Evidence<'_>,
    anchor_only: bool,
    report: &mut Report,
    found: &mut BTreeMap<String, Hit>,
) -> Result<SourceIndex, String> {
    let (goals, handoffs) = (evidence.goals, evidence.handoffs);
    let index = load(root, id)?;
    if !anchor_only
        && request
            .account
            .is_some_and(|account| account != index.binding.account)
    {
        return Err("source belongs to another requested account".into());
    }
    if index.binding.provider == ProviderKind::Cursor
        && request.source != Some(id)
        && request.from != Some(id)
    {
        return Err("Cursor requires explicit --source or --from; broad ranking never starts native history clients".into());
    }
    let mut source = open_index_source(root, &index)?;
    let header = read_at(&mut source, 0, index.anchor_len as usize)?;
    let mut files = files::Page::new(id, &index, &header);
    if request.workspace.is_some_and(|workspace| {
        files
            .workspace
            .as_ref()
            .is_none_or(|reported| !same_workspace(workspace, reported))
    }) && request.source != Some(id)
        && !goals.iter().any(|relation| relation.source == id)
        && !anchor_only
    {
        return Err("source header does not establish the requested workspace".into());
    }
    let query = request.text.to_lowercase();
    let anchor_time = request.at.and_then(timestamp_ms);
    let goal = goals.iter().find(|relation| relation.source == id);
    let handoff = handoffs
        .iter()
        .find(|handoff| candidate_matches(&handoff.candidate, id, &index));
    let mut staged = Vec::new();
    for (position, span) in index.messages.iter().enumerate() {
        if report.record_bytes_checked + span.len as u64 > RECORD_BYTES {
            return Err("64 MiB verified-record budget reached".into());
        }
        let bytes = read_at(&mut source, span.start, span.len)?;
        report.record_bytes_checked += bytes.len() as u64;
        if digest(&bytes) != span.sha256 {
            return Err("native message changed; ranked hits withheld".into());
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid native record")?;
        let first_fact = files.facts.len();
        if request.file.is_some() && !anchor_only {
            files.observe(&index, position, &value);
        }
        if anchor_only {
            continue;
        }
        let message = index.history.as_ref().map_or(0, |page| page.message_base) + position + 1;
        let lexical = retrieval::contains_text(&value, &query);
        let matching_file = request.file.and_then(|file| {
            let workspace = files.workspace.as_ref()?;
            if request
                .workspace
                .is_some_and(|requested| !same_workspace(requested, workspace))
            {
                return None;
            }
            let requested = files::relative_path(workspace, file)?;
            files.facts[first_fact..].iter().find(|fact| {
                crate::workspaces::same_key(
                    &crate::workspaces::path_key(&fact.path),
                    &crate::workspaces::path_key(&requested),
                )
            })
        });
        if lexical.is_none() && goal.is_none() && handoff.is_none() && matching_file.is_none() {
            continue;
        }
        let mut reasons = Vec::new();
        let evidence = json!({"source":id,"message":message,"sha256":span.sha256});
        if let Some(goal) = goal {
            reasons.push(Reason {
                kind: "confirmed_goal_source",
                points: 10_000,
                detail: "User-confirmed source association to the requested goal".into(),
                evidence: json!({"relation":goal.id,"goal":goal.goal,"source":id}),
            });
        }
        if let Some(handoff) = handoff {
            reasons.push(Reason {
                kind: "explicit_relationship",
                points: 3_000,
                detail: handoff.label.into(),
                evidence: json!({"from":request.from,"metadata":handoff.reference,"source":id}),
            });
        }
        if let Some(fact) = matching_file {
            reasons.push(Reason { kind: "exact_historical_file", points: 1_000, detail: "Exact historical path in a verified tool request; current disk state unverified".into(), evidence: json!({"source":id,"message":fact.message,"sha256":fact.sha256,"path":fact.path}) });
        }
        if lexical.is_some() {
            reasons.push(Reason {
                kind: "lexical",
                points: 100,
                detail: "Literal case-insensitive hit in this native record".into(),
                evidence: evidence.clone(),
            });
        }
        if let (Some(anchor), Some(timestamp)) =
            (anchor_time, value.get("timestamp").and_then(Value::as_str))
        {
            if let Some(native) = timestamp_ms(timestamp) {
                let distance = native.abs_diff(anchor);
                reasons.push(Reason { kind: "temporal_distance", points: 49u32.saturating_sub((distance / 3_600_000).min(49) as u32), detail: "Native timestamp distance from the requested instant, at millisecond resolution".into(), evidence: json!({"source":id,"message":message,"sha256":span.sha256,"native_timestamp":timestamp,"reference_timestamp":request.at,"distance_ms":distance}) });
            }
        }
        let score = reasons.iter().map(|reason| reason.points).sum();
        let preview = lexical
            .or_else(|| retrieval::contains_text(&value, ""))
            .unwrap_or_default();
        let preview = crate::chat_cli::terminal_safe(&preview)
            .chars()
            .take(240)
            .collect();
        let key = digest(
            &serde_json::to_vec(&(
                &index.binding,
                &index.file_identity,
                span.start,
                &span.sha256,
            ))
            .map_err(|error| error.to_string())?,
        );
        staged.push((
            key,
            Hit {
                hit: retrieval::Hit {
                    source: summary(id, &index),
                    message,
                    role: span.role.clone(),
                    byte_start: span.start,
                    sha256: span.sha256.clone(),
                    preview,
                    relationship: "ranked suggestion; evidence does not create a relationship",
                },
                score,
                reasons,
                recorded_workspace: files.workspace.clone(),
            },
        ));
    }
    let _ = open_index_source(root, &index)?;
    report.incomplete |= index.truncated
        || index
            .history
            .as_ref()
            .is_some_and(|page| page.next.is_some() || page.skipped_records > 0);
    if request.file.is_some() && !anchor_only {
        report.incomplete |=
            files.incomplete || files.overflow || files.unsupported_tool_records > 0;
        report.limits.extend(files.limits);
        if files.unsupported_tool_records > 0 {
            report.unavailable.push(format!(
                "{id}: {} tool records lack a supported path schema",
                files.unsupported_tool_records
            ));
        }
        report.unavailable.extend(
            files
                .unavailable
                .into_iter()
                .map(|reason| format!("{id}: {reason}")),
        );
    }
    if !report.pages.iter().any(|page| page.id == id) {
        report.pages.push(summary(id, &index));
    }
    for (key, hit) in staged {
        let replace = found.get(&key).is_none_or(|old| {
            hit.score > old.score
                || hit.score == old.score
                    && (&hit.hit.source.id, hit.hit.message) < (&old.hit.source.id, old.hit.message)
        });
        if replace {
            found.insert(key, hit);
        }
    }
    Ok(index)
}

pub fn recall(root: &Path, request: Request<'_>) -> Result<Report, String> {
    if request.source.is_some_and(|source| !valid_id(source))
        || request.from.is_some_and(|source| !valid_id(source))
    {
        return Err("ranked source context requires exact source IDs".into());
    }
    if request.text.trim().is_empty() || request.text.len() > 512 {
        return Err("ranked query must contain 1..512 UTF-8 bytes".into());
    }
    if request.at.is_some_and(|at| timestamp_ms(at).is_none()) {
        return Err("reference time requires a supported RFC 3339 timestamp with UTC offset; leap seconds remain unsupported".into());
    }
    let workspace = request
        .workspace
        .map(|workspace| {
            files::normalize_workspace(workspace.to_str().unwrap_or(""))
                .ok_or("workspace context must be a supported absolute path")
        })
        .transpose()?;
    let request = Request {
        workspace: workspace.as_deref(),
        ..request
    };
    if let Some(file) = request.file {
        if request.workspace.is_none() && request.source.is_none() {
            return Err("file ranking needs an explicit workspace or source; same-named files across projects are distinct".into());
        }
        if file.len() > 4096
            || file.chars().any(char::is_control)
            || request
                .workspace
                .is_some_and(|workspace| files::relative_path(workspace, file).is_none())
        {
            return Err(
                "file context is outside the workspace or has an unsupported/sensitive path".into(),
            );
        }
    }
    if let Some(source) = request.source {
        let index = load(root, source)?;
        if request
            .account
            .is_some_and(|account| account != index.binding.account)
        {
            return Err("ranked source belongs to another account".into());
        }
    }
    let goals = request
        .goal
        .map(|goal| relations::ranking_sources(root, request.workspace, goal))
        .transpose()?
        .unwrap_or_default();
    let goal_snapshot = digest(&serde_json::to_vec(&goals).map_err(|error| error.to_string())?);
    let mut report = Report {
        version: 1,
        hits: Vec::new(),
        pages: Vec::new(),
        incomplete: false,
        unavailable: Vec::new(),
        limits: Vec::new(),
        record_bytes_checked: 0,
        scope: json!({"candidate_page_reads_limit":PAGES,"index_metadata_probes_limit":64,"record_bytes_checked_limit":RECORD_BYTES,"hit_limit":HITS,"output_bytes_limit":65536,"discovery_records_limit":1000,
            "workspace":request.workspace,"goal":request.goal,"file_context":request.file,"from":request.from,"reference_time":request.at,
            "byte_counter":"indexed record bytes checked, including rejected hashes; native discovery, index construction and bounded source verification are separate reads",
            "ordering":"score descending, source ID ascending, message ordinal ascending; no inferred dates or generated summaries"}),
    };
    let mut found = BTreeMap::new();
    let mut handoffs = Vec::new();
    let mut context_snapshot = None;
    let mut page_reads = 0;
    if let Some(anchor) = request.from {
        page_reads += 1;
        match rank_page(
            root,
            anchor,
            &request,
            &Evidence {
                goals: &[],
                handoffs: &[],
            },
            true,
            &mut report,
            &mut found,
        ) {
            Ok(index) => match neighbors(root, anchor, &index, request.workspace) {
                Ok(rows) => {
                    context_snapshot = Some((
                        index,
                        digest(&serde_json::to_vec(&rows).map_err(|error| error.to_string())?),
                    ));
                    handoffs = rows;
                }
                Err(error) => {
                    report.incomplete = true;
                    report
                        .unavailable
                        .push(format!("context relationships unavailable: {error}"));
                }
            },
            Err(error) => {
                report.incomplete = true;
                report
                    .unavailable
                    .push(format!("context source {anchor} unavailable: {error}"));
            }
        }
    }
    let mut queue = Vec::new();
    if let Some(source) = request.source {
        queue.push(Candidate::Pointer(source.into()));
    } else {
        queue.extend(
            goals
                .iter()
                .map(|relation| Candidate::Pointer(relation.source.clone())),
        );
        queue.extend(handoffs.iter().map(|handoff| handoff.candidate.clone()));
        let scan = sessions::list_native_records_bounded(root, 1000, request.account, None);
        report.incomplete |= scan.truncated;
        for (session, _) in scan.records {
            if request.workspace.is_some_and(|workspace| {
                session
                    .cwd
                    .as_ref()
                    .is_none_or(|cwd| !same_workspace(workspace, Path::new(cwd)))
            }) {
                continue;
            }
            if session.provider == ProviderKind::Cursor {
                report.incomplete = true;
                report.unavailable.push(
                    "Cursor discovery requires explicit --source; no native history client started"
                        .into(),
                );
                continue;
            }
            let binding = accounts::get_account(root, &session.account)
                .and_then(|account| account.home_path)
                .ok_or_else(|| "native account home is unavailable".to_string())
                .and_then(|home| {
                    Binding::new(
                        session.provider,
                        &session.account,
                        &home,
                        &session.session_id,
                    )
                });
            match binding {
                Ok(binding) => queue.push(Candidate::Session(binding)),
                Err(error) => {
                    report.incomplete = true;
                    report.unavailable.push(error);
                }
            }
        }
    }
    let mut queued = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut probes = 0;
    for candidate in queue {
        let key = match &candidate {
            Candidate::Pointer(id) => format!("pointer:{id}"),
            Candidate::Session(binding) => format!(
                "binding:{}",
                digest(&serde_json::to_vec(binding).map_err(|error| error.to_string())?)
            ),
        };
        if !queued.insert(key) {
            continue;
        }
        if page_reads >= PAGES || probes >= 64 {
            report.incomplete = true;
            report
                .limits
                .push("8 candidate page reads or 64 index metadata probes".into());
            break;
        }
        probes += 1;
        let resolved = match &candidate {
            Candidate::Pointer(id) => load(root, id).map(|index| (id.clone(), index.binding)),
            Candidate::Session(binding) => {
                if request
                    .account
                    .is_some_and(|account| account != binding.account)
                {
                    continue;
                }
                if binding.provider == ProviderKind::Cursor {
                    report.incomplete = true;
                    report.unavailable.push(
                        "Cursor relationship requires explicit --source; native client not started"
                            .into(),
                    );
                    continue;
                }
                page_reads += 1;
                index_bound(root, binding).map(|page| (page.id, binding.clone()))
            }
        };
        let (id, binding) = match resolved {
            Ok(resolved) => resolved,
            Err(error) => {
                report.incomplete = true;
                report.unavailable.push(error);
                continue;
            }
        };
        if request
            .account
            .is_some_and(|account| account != binding.account)
            || !seen.insert(id.clone())
        {
            continue;
        }
        if matches!(candidate, Candidate::Pointer(_)) {
            page_reads += 1;
        }
        if let Err(error) = rank_page(
            root,
            &id,
            &request,
            &Evidence {
                goals: &goals,
                handoffs: &handoffs,
            },
            false,
            &mut report,
            &mut found,
        ) {
            report.incomplete = true;
            report.unavailable.push(format!("{id}: {error}"));
        }
    }
    if let Some(goal) = request.goal {
        let current = relations::ranking_sources(root, request.workspace, goal)?;
        if digest(&serde_json::to_vec(&current).map_err(|error| error.to_string())?)
            != goal_snapshot
        {
            return Err("goal/source relationships changed during ranking; repeat the read for a new snapshot".into());
        }
    }
    if let Some((index, snapshot)) = context_snapshot {
        let current = neighbors(
            root,
            request.from.unwrap_or_default(),
            &index,
            request.workspace,
        )?;
        if digest(&serde_json::to_vec(&current).map_err(|error| error.to_string())?) != snapshot {
            return Err(
                "context relationships changed during ranking; repeat the read for a new snapshot"
                    .into(),
            );
        }
        report.scope["context_snapshot_digest"] = json!(snapshot);
    }
    report.scope["goal_snapshot_digest"] = json!(goal_snapshot);
    report.scope["candidate_page_reads"] = json!(page_reads);
    report.scope["index_metadata_probes"] = json!(probes);
    report.hits = found.into_values().collect();
    report.hits.sort_by(|left, right| {
        right.score.cmp(&left.score).then_with(|| {
            (&left.hit.source.id, left.hit.message).cmp(&(&right.hit.source.id, right.hit.message))
        })
    });
    if report.hits.len() > HITS {
        report.hits.truncate(HITS);
        report.incomplete = true;
        report.limits.push("12 ranked hits".into());
    }
    report.limits.sort();
    report.limits.dedup();
    report.unavailable.sort();
    report.unavailable.dedup();
    while serde_json::to_vec_pretty(&report)
        .map_err(|error| error.to_string())?
        .len()
        > 65536
    {
        if report.hits.pop().is_none() {
            return Err("ranking metadata exceeds its 64 KiB result budget".into());
        }
        report.incomplete = true;
        if !report
            .limits
            .iter()
            .any(|reason| reason == "64 KiB result metadata")
        {
            report.limits.push("64 KiB result metadata".into());
        }
    }
    Ok(report)
}
