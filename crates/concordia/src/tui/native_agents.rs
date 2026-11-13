//! Read-only Agents view projection for provider-native and untracked provider runs.
use super::*;
use crate::provider::stream_cards::{CardStatus, StreamCard};
use std::collections::{HashMap, HashSet};

pub(super) fn panel_area(area: Rect) -> Rect {
    if area.width < 96 {
        return area;
    }
    let width = (area.width / 2).clamp(48, 86);
    Rect { x: area.x + area.width - width, width, ..area }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Native,
    Untracked,
}

#[derive(Clone)]
struct Entry {
    id: String,
    index: usize,
    parent: Option<String>,
    kind: Kind,
    started: Instant,
}

#[derive(Default)]
pub(super) struct State {
    cards: Vec<(String, usize)>,
    launch_tools: Vec<String>,
    entries: HashMap<String, Entry>,
    collapsed: HashSet<String>,
    other_runs_open: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Running,
    Done,
    Error,
}

impl Status {
    fn from(card: CardStatus) -> Self {
        match card {
            CardStatus::Running => Self::Running,
            CardStatus::Done => Self::Done,
            CardStatus::Error => Self::Error,
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::Running => "◐",
            Self::Done => "✓",
            Self::Error => "✗",
        }
    }
    fn word(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done => "finished",
            Self::Error => "failed/interrupted",
        }
    }
}

#[derive(Clone, Default)]
struct Facts {
    role: String,
    model: String,
    effort: String,
    used: Option<u64>,
    limit: Option<u64>,
    compaction: Option<f64>,
    turns: Option<u64>,
}

fn fact_value(detail: Option<&str>, key: &str) -> Option<String> {
    detail?.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim().eq_ignore_ascii_case(key).then(|| value.trim().to_owned())
    })
}

fn fact_number(detail: Option<&str>, key: &str) -> Option<u64> {
    fact_value(detail, key)?.replace(',', "").parse().ok()
}

fn facts(detail: Option<&str>) -> Facts {
    Facts {
        role: fact_value(detail, "Role").unwrap_or_else(|| "unknown".into()),
        model: fact_value(detail, "Model").unwrap_or_else(|| "unknown".into()),
        effort: fact_value(detail, "Effort").unwrap_or_else(|| "unknown".into()),
        used: fact_number(detail, "Tokens used").or_else(|| fact_number(detail, "Used")),
        limit: fact_number(detail, "Context window").or_else(|| fact_number(detail, "Limit")),
        compaction: fact_value(detail, "Compaction reference").and_then(|value| {
            value.trim_end_matches('%').parse::<f64>().ok()
        }),
        turns: fact_number(detail, "Turns").or_else(|| fact_number(detail, "Tasks")),
    }
}

fn context_bar(used: Option<u64>, limit: Option<u64>, width: usize) -> String {
    let filled = match (used, limit) {
        (Some(used), Some(limit)) if limit > 0 => {
            ((used as f64 / limit as f64) * width as f64).round() as usize
        }
        _ => 0,
    }
    .min(width);
    (0..width)
        .map(|index| if index < filled { '\u{25ae}' } else { '\u{25af}' })
        .collect()
}

fn formatted_number(value: Option<u64>) -> String {
    value.map_or_else(|| "unknown".into(), |value| value.to_string())
}

fn task_title(detail: Option<&str>, fallback: &str) -> String {
    let line = detail
        .unwrap_or("")
        .lines()
        .find_map(|line| {
            line.strip_prefix("Task: ")
                .or_else(|| line.strip_prefix("task: "))
        })
        .unwrap_or("")
        .trim();
    let title = serde_json::from_str::<String>(line).unwrap_or_else(|_| line.into());
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        fallback.chars().take(120).collect()
    } else {
        title.chars().take(120).collect()
    }
}

pub(super) const GROUP_ID: &str = "agents-group:other";
const ROW_WIDTH: usize = 40;
const NO_TASK: &str = "task not shown";

/// A provider CLI launch recognised in a shell tool card. Only the provider, the model flag and
/// the first line of the task are kept; prompt bodies and tool output never reach the list.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Run {
    provider: &'static str,
    model: Option<String>,
    task: String,
}

fn value_to_command(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Array(items) => {
            let parts = items.iter().map(|v| v.as_str()).collect::<Option<Vec<_>>>()?;
            if parts.len() >= 3
                && matches!(
                    parts[parts.len() - 2],
                    "-c" | "-lc" | "-Command" | "-command" | "/c"
                )
            {
                return Some(parts[parts.len() - 1].to_owned());
            }
            Some(
                parts
                    .iter()
                    .map(|part| {
                        if part.is_empty() || part.contains(char::is_whitespace) || part.contains('"') {
                            format!("\"{}\"", part.replace('\\', "\\\\").replace('"', "\\\""))
                        } else {
                            (*part).to_owned()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        }
        serde_json::Value::Object(map) => ["command", "cmd"]
            .iter()
            .find_map(|key| map.get(*key).and_then(value_to_command))
            .or_else(|| {
                ["input", "args", "arguments"]
                    .iter()
                    .find_map(|key| map.get(*key).and_then(value_to_command))
            }),
        _ => None,
    }
}

/// The shell command of a tool card: JSON tool input (string or array), or `command: ...` text.
fn command_text(detail: &str) -> Option<String> {
    let body = detail.trim_start();
    let body = ["arguments:", "Arguments:", "input:", "Input:"]
        .iter()
        .find_map(|prefix| body.strip_prefix(prefix))
        .map_or(body, str::trim_start);
    if body.starts_with(['{', '[', '"']) {
        if let Some(Ok(value)) = serde_json::Deserializer::from_str(body)
            .into_iter::<serde_json::Value>()
            .next()
        {
            return value_to_command(&value);
        }
    }
    let mut offset = 0;
    let mut text = body;
    for line in detail.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(rest) = ["command:", "cmd:", "command =", "cmd ="]
            .iter()
            .find_map(|prefix| trimmed.strip_prefix(prefix))
        {
            let start = offset + (line.len() - trimmed.len()) + (trimmed.len() - rest.len());
            text = detail[start..].trim_start();
            break;
        }
        offset += line.len();
    }
    let text = text.split("\nOutput:").next().unwrap_or(text).trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Shell-style tokens. Statement separators, pipes and redirections become their own tokens.
fn shell_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut have = false;
    let mut chars = text.chars().peekable();
    fn flush(out: &mut Vec<String>, cur: &mut String, have: &mut bool) {
        if *have {
            out.push(std::mem::take(cur));
            *have = false;
        }
    }
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                have = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    cur.push(q);
                }
            }
            '"' => {
                have = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' => match chars.peek().copied() {
                            Some(n @ ('"' | '\\' | '$' | '`')) => {
                                cur.push(n);
                                chars.next();
                            }
                            Some('\n') => {
                                chars.next();
                            }
                            _ => cur.push('\\'),
                        },
                        _ => cur.push(q),
                    }
                }
            }
            '\\' => {
                have = true;
                match chars.peek().copied() {
                    Some(n @ ('"' | '\'' | ' ' | '\\')) => {
                        cur.push(n);
                        chars.next();
                    }
                    _ => cur.push('\\'),
                }
            }
            '\n' | ';' | '|' | '&' => {
                flush(&mut out, &mut cur, &mut have);
                out.push(";".into());
            }
            c if c.is_whitespace() => flush(&mut out, &mut cur, &mut have),
            c => {
                have = true;
                cur.push(c);
            }
        }
    }
    flush(&mut out, &mut cur, &mut have);
    out
}

fn executable(token: &str) -> String {
    let name = token.rsplit(['/', '\\']).next().unwrap_or(token).to_ascii_lowercase();
    for suffix in [".exe", ".cmd", ".bat", ".ps1"] {
        if let Some(stem) = name.strip_suffix(suffix) {
            return stem.to_owned();
        }
    }
    name
}

fn value_flags(provider: &str) -> &'static [&'static str] {
    match provider {
        "Codex" => &[
            "-m", "--model", "-c", "--config", "-p", "--profile", "-C", "--cd", "-s", "--sandbox",
            "-i", "--image", "-o", "--output-last-message", "--color", "--output-schema",
            "--add-dir", "--local-provider",
        ],
        "Claude" => &[
            "--model", "--permission-mode", "--allowedTools", "--allowed-tools",
            "--disallowedTools", "--disallowed-tools", "--output-format", "--input-format",
            "--max-turns", "--append-system-prompt", "--system-prompt", "--resume", "-r",
            "--session-id", "--mcp-config", "--settings", "--add-dir", "--fallback-model",
            "--effort", "--agent", "--agents", "--tools",
        ],
        _ => &[
            "-m", "--model", "--output-format", "--resume", "--workspace", "--cwd", "--api-key",
            "-H", "--header", "--effort",
        ],
    }
}

fn is_terminator(token: &str) -> bool {
    token == ";" || token.starts_with(['<', '>']) || token.ends_with('>')
}

/// Parse the arguments after the provider (and `exec` for Codex).
fn parse_args(provider: &'static str, args: &[String]) -> Run {
    let value_flags = value_flags(provider);
    let mut model = None;
    let mut task = None;
    let mut i = 0;
    while i < args.len() {
        let token = args[i].as_str();
        if is_terminator(token) || token == "-" {
            break;
        }
        if let Some(value) = token.strip_prefix("--model=") {
            model = Some(value.to_owned());
        } else if value_flags.contains(&token) {
            if matches!(token, "-m" | "--model") {
                model = args.get(i + 1).filter(|v| !is_terminator(v)).cloned();
            }
            i += 1;
        } else if !token.starts_with('-') {
            task = Some(token.to_owned());
            break;
        }
        i += 1;
    }
    let task = task.map(|text| first_line(&text)).filter(|line| !line.is_empty());
    Run {
        provider,
        model: model.map(|m| first_line(&m)).filter(|m| !m.is_empty()),
        task: task.unwrap_or_else(|| NO_TASK.into()),
    }
}

fn find_run(tokens: &[String], depth: usize) -> Option<Run> {
    for (at, token) in tokens.iter().enumerate() {
        let name = executable(token);
        let rest: Vec<String> = tokens[at + 1..]
            .iter()
            .take_while(|t| t.as_str() != ";")
            .cloned()
            .collect();
        let run = match name.as_str() {
            "codex" => rest
                .iter()
                .take(8)
                .position(|t| t == "exec")
                .map(|p| parse_args("Codex", &rest[p + 1..])),
            "claude" => rest
                .iter()
                .any(|t| t == "-p" || t == "--print")
                .then(|| parse_args("Claude", &rest)),
            "cursor-agent" => Some(parse_args("Cursor", &rest)),
            "grok" if !rest.is_empty() => Some(parse_args("Grok", &rest)),
            "opencode" if !rest.is_empty() => Some(parse_args("opencode", &rest)),
            _ => None,
        };
        if run.is_some() {
            return run;
        }
    }
    if depth < 2 {
        for pair in tokens.windows(2) {
            let shell_flag = pair[0].to_ascii_lowercase();
            if matches!(shell_flag.as_str(), "-c" | "-lc" | "-command" | "/c")
                && pair[1].contains(char::is_whitespace)
            {
                if let Some(run) = find_run(&shell_tokens(&pair[1]), depth + 1) {
                    return Some(run);
                }
            }
        }
    }
    None
}

/// First non-empty line of `text`, with escape sequences and control characters removed and
/// whitespace collapsed.
fn first_line(text: &str) -> String {
    let mut clean = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    for n in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(n) = chars.next() {
                        if n == '\u{7}' || (n == '\u{1b}' && chars.next() == Some('\\')) {
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if c == '\n' || c == '\r' || c == '\t' || !c.is_control() {
            clean.push(c);
        }
    }
    clean
        .split(['\n', '\r'])
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

fn cut(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_owned()
    } else {
        let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Recognise only provider CLI launchers in structured tool-call arguments. Ordinary shell cards
/// remain ordinary activity; no command is executed or inferred from prose.
fn provider_run(detail: &str) -> Option<Run> {
    let lower = detail.to_ascii_lowercase();
    if !["codex", "claude", "cursor-agent", "grok", "opencode"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return None;
    }
    find_run(&shell_tokens(&command_text(detail)?), 0)
}

fn untracked_run(card: &StreamCard) -> Option<Run> {
    let detail = card.detail.as_deref().unwrap_or("");
    if is_native_agent(&card.id, &card.title, detail) {
        return None;
    }
    provider_run(detail)
}

fn untracked_label(depth: usize, status: Status, run: &Run) -> String {
    let head = format!(
        "{}{} {}{}  ",
        "  ".repeat(depth),
        status.icon(),
        run.provider,
        run.model.as_deref().map(|m| format!(" {m}")).unwrap_or_default()
    );
    let budget = ROW_WIDTH.saturating_sub(head.chars().count()).max(12);
    format!("{head}{}", cut(&run.task, budget))
}

fn untracked_detail(run: &Run, status: Status, elapsed: &str, number: usize) -> Vec<String> {
    vec![
        format!("#{number} {} run", run.provider),
        format!("Provider: {}", run.provider),
        format!("Model: {}", run.model.as_deref().unwrap_or("unknown")),
        format!("Status: {} {}", status.icon(), status.word()),
        format!("Elapsed: {elapsed}"),
        format!("Task: {}", cut(&run.task, 120)),
        String::new(),
        "Concordia did not start this run, so it cannot receive messages or questions.".into(),
    ]
}

fn is_native_agent(id: &str, title: &str, detail: &str) -> bool {
    let lower = title.to_ascii_lowercase();
    id.starts_with("native-agent:")
        || lower.starts_with("agent ")
        || (matches!(title, "Agent" | "Task")
            && (detail.contains("Task:") || detail.contains("Child:")))
}

impl State {
    pub fn rebuild(&mut self, messages: &[ChatLine]) {
        self.cards.clear();
        self.entries.clear();
        self.launch_tools.clear();
        for (index, message) in messages.iter().enumerate() {
            if let Some(card) = &message.card {
                self.record(card, index);
            }
        }
    }

    pub fn record(&mut self, card: &StreamCard, index: usize) {
        let id = &card.id;
        if let Some(identity) = id.strip_prefix("native-agent:claude:") {
            if let Ok((_, _, Some(tool))) =
                serde_json::from_str::<(String, String, Option<String>)>(identity)
            {
                if !self.launch_tools.contains(&tool) {
                    if self.launch_tools.len() >= 64 {
                        self.launch_tools.remove(0);
                    }
                    self.launch_tools.push(tool.clone());
                }
                self.cards.retain(|(old, _)| old != &tool);
            }
        }
        if self.launch_tools.contains(id) {
            return;
        }
        let native = is_native_agent(id, &card.title, card.detail.as_deref().unwrap_or(""));
        let untracked = untracked_run(card).is_some();
        if !native && !untracked {
            return;
        }
        if let Some((_, old)) = self.cards.iter_mut().find(|(old, _)| old == id) {
            *old = index;
        } else {
            if self.cards.len() >= 128 {
                self.cards.remove(0);
            }
            self.cards.push((id.into(), index));
        }
        let sender = card.detail.as_deref().and_then(|detail| {
            detail
                .lines()
                .find_map(|line| line.strip_prefix("Sender: ").map(str::to_owned))
        });
        let parent = if untracked { None } else { sender };
        let entry = self.entries.entry(id.clone()).or_insert_with(|| Entry {
            id: id.clone(),
            index,
            parent,
            kind: if untracked {
                Kind::Untracked
            } else {
                Kind::Native
            },
            started: Instant::now(),
        });
        entry.index = index;
    }
    pub fn count(&self) -> usize {
        self.entries.len()
    }
    fn toggle_group(&mut self) {
        self.other_runs_open = !self.other_runs_open;
    }
    fn toggle(&mut self, id: &str) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.into());
        }
    }

    fn rows(
        &self,
        messages: &[ChatLine],
        workers: &[&crate::delegate::Job],
        state_root: &std::path::Path,
    ) -> Vec<PickerRow> {
        let mut children: HashMap<Option<&str>, Vec<(&Entry, &StreamCard)>> = HashMap::new();
        let mut all_entries = Vec::new();
        for (index, line) in messages.iter().enumerate() {
            let Some(card) = line.card.as_ref() else {
                continue;
            };
            let id = card.id.as_str();
            let native = is_native_agent(id, &card.title, card.detail.as_deref().unwrap_or(""));
            let untracked = untracked_run(card).is_some();
            if !native && !untracked {
                continue;
            }
            let mut entry = self.entries.get(id).cloned().unwrap_or(Entry {
                id: id.into(),
                index,
                parent: None,
                kind: if untracked {
                    Kind::Untracked
                } else {
                    Kind::Native
                },
                started: Instant::now(),
            });
            entry.index = index;
            all_entries.push(entry);
        }
        let mut runs: Vec<(&Entry, &StreamCard, Run)> = Vec::new();
        for entry in &all_entries {
            if let Some(card) = messages
                .get(entry.index)
                .and_then(|line| line.card.as_ref())
            {
                if entry.kind == Kind::Untracked {
                    if let Some(run) = untracked_run(card) {
                        runs.push((entry, card, run));
                    }
                    continue;
                }
                // Provider sender/thread IDs are not necessarily Agents-row IDs. Keep a
                // reported parent only when it is represented by another visible entry;
                // otherwise the agent is a root in this conversation's tree.
                let parent = entry
                    .parent
                    .as_deref()
                    .filter(|parent| all_entries.iter().any(|candidate| candidate.id == *parent));
                children.entry(parent).or_default().push((entry, card));
            }
        }
        for rows in children.values_mut() {
            rows.sort_by_key(|(entry, _)| entry.index);
        }
        let mut out = Vec::new();
        fn add(
            out: &mut Vec<PickerRow>,
            children: &HashMap<Option<&str>, Vec<(&Entry, &StreamCard)>>,
            parent: Option<&str>,
            depth: usize,
            collapsed: &HashSet<String>,
        ) {
            for (entry, card) in children.get(&parent).into_iter().flatten() {
                let status = Status::from(card.status);
                let child_count = children.get(&Some(entry.id.as_str())).map_or(0, Vec::len);
                let disclosure = if child_count == 0 {
                    " "
                } else if collapsed.contains(&entry.id) {
                    "▸"
                } else {
                    "▾"
                };
                let title = {
                    let task = task_title(card.detail.as_deref(), &card.title);
                    if card.title.starts_with("agent ")
                        && !task.contains(card.title.trim_start_matches("agent "))
                    {
                        format!("{task} · {}", card.title.trim_start_matches("agent "))
                    } else {
                        task
                    }
                };
                let elapsed = if status == Status::Running {
                    format_elapsed(entry.started.elapsed())
                } else {
                    String::new()
                };
                let metadata = facts(card.detail.as_deref());
                let label = format!(
                    "{}{} {} {}  {}  {}  {}  {}  {}",
                    "  ".repeat(depth),
                    disclosure,
                    status.icon(),
                    metadata.role,
                    metadata.model,
                    metadata.effort,
                    context_bar(metadata.used, metadata.limit, 7),
                    metadata.turns.map_or_else(|| "unknown".into(), |value| value.to_string()),
                    if elapsed.is_empty() { "".into() } else { elapsed },
                );
                let detail = format!(
                    "{} · {} · {}",
                    title,
                    status.word(),
                    card.detail
                        .as_deref()
                        .unwrap_or("recent activity")
                        .lines()
                        .filter(|line| !line.starts_with("Task: "))
                        .take(2)
                        .collect::<Vec<_>>()
                        .join(" · ")
                );
                out.push(PickerRow {
                    id: format!("native-agent:{}", entry.index),
                    label,
                    detail,
                });
                if !collapsed.contains(&entry.id) {
                    add(out, children, Some(entry.id.as_str()), depth + 1, collapsed);
                }
            }
        }
        add(&mut out, &children, None, 0, &self.collapsed);
        out.extend(workers.iter().map(|job| PickerRow {
            id: format!("worker-agent:{}", job.id),
            label: format!(
                "  {} {} · {}",
                if job.state.is_running() { "●" } else { "✓" },
                task_title(Some(&job.task), &job.task),
                format_elapsed(job.elapsed(Instant::now()))
            ),
            detail: format!(
                "{} · provider {} · model {}",
                job.state.label(),
                job.provider.as_str(),
                job.model.as_deref().unwrap_or("default")
            ),
        }));
        for record in crate::mcp::take_pending_records(state_root, 128) {
            let (icon, word) = match record.status.as_str() {
                "finished" => ("✓", "finished"),
                "needs_attention" => ("?", "needs attention"),
                "unreachable" => ("×", "unreachable"),
                "waiting" | "starting" => ("●", "waiting for Grok Bot"),
                _ => ("·", record.status.as_str()),
            };
            out.push(PickerRow {
                id: format!("grok-agent:{}", record.agent_id),
                label: format!(
                    "  {icon} {word} · {}",
                    task_title(Some(&record.task), "Grok Bot")
                ),
                detail: format!(
                    "Grok Bot · {} · {}",
                    record.account,
                    record.activity.last().cloned().unwrap_or_default()
                ),
            });
        }
        if !runs.is_empty() {
            out.push(PickerRow {
                id: GROUP_ID.into(),
                label: format!(
                    "{} Other runs on this machine ({})",
                    if self.other_runs_open { "▾" } else { "▸" },
                    runs.len()
                ),
                detail: "Provider CLI runs seen in this conversation that Concordia did not start"
                    .into(),
            });
            if self.other_runs_open {
                for (entry, card, run) in &runs {
                    out.push(PickerRow {
                        id: format!("native-agent:{}", entry.index),
                        label: untracked_label(1, Status::from(card.status), run),
                        detail: run.task.clone(),
                    });
                }
            }
        }
        out
    }
}

pub(super) fn draw(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let Some(picker) = app.picker.as_ref().filter(|picker| picker.kind == PickerKind::Agents)
    else {
        return;
    };
    let rows: Vec<PickerRow> = picker.filtered().into_iter().cloned().collect();
    let selected = picker.state.selected().unwrap_or(0).min(rows.len().saturating_sub(1));
    let selected_row = rows.get(selected).cloned();
    let columns = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);
    f.render_widget(Clear, area);

    let mut detail_lines = Vec::new();
    let mut add_detail = |id: &str, row_number: usize| {
        if let Some(index) = id.strip_prefix("native-agent:").and_then(|value| value.parse::<usize>().ok()) {
            if let Some(card) = app.messages.get(index).and_then(|line| line.card.as_ref()) {
                let status = Status::from(card.status);
                let elapsed = app.native_agents.entries.get(&card.id)
                    .map(|entry| format_elapsed(entry.started.elapsed()))
                    .unwrap_or_else(|| "unknown".into());
                if let Some(run) = untracked_run(card) {
                    for (i, text) in untracked_detail(&run, status, &elapsed, row_number).into_iter().enumerate() {
                        detail_lines.push(if i == 0 {
                            Line::styled(text, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                        } else {
                            Line::from(text)
                        });
                    }
                    return;
                }
                let metadata = facts(card.detail.as_deref());
                let title = task_title(card.detail.as_deref(), &card.title);
                let used = formatted_number(metadata.used);
                let limit = formatted_number(metadata.limit);
                let percentage = metadata.compaction.map_or_else(|| "unknown".into(), |value| format!("{value:.1}%"));
                detail_lines.extend([
                    Line::from(vec![Span::styled(format!("#{} {}", row_number, metadata.role), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)), Span::raw(" "), Span::styled(format!("{} {}", status.icon(), status.word()), Style::default().fg(Color::Green))]),
                    Line::styled(format!("{} · {}", metadata.model, metadata.effort), Style::default().fg(Color::Gray)),
                    Line::from(format!("{used} / {limit} tokens")),
                    Line::from(context_bar(metadata.used, metadata.limit, 28)),
                    Line::styled(format!("{percentage} of compaction reference"), Style::default().fg(Color::Gray)),
                    Line::styled(format!("{} tokens remaining · last snapshot", metadata.limit.zip(metadata.used).map(|(limit, used)| limit.saturating_sub(used)).map_or_else(|| "unknown".into(), |value| value.to_string())), Style::default().fg(Color::Gray)),
                    Line::from(""),
                    Line::styled("Tasks", Style::default().add_modifier(Modifier::BOLD)),
                    Line::from(format!("{} {}    {}", status.icon(), title, elapsed)),
                    Line::styled("#1 · From Orchestrator", Style::default().fg(Color::Gray)),
                    Line::styled(status.word().to_owned(), Style::default().fg(Color::Gray)),
                ]);
            }
        }
    };
    if let Some(row) = selected_row.as_ref() {
        add_detail(&row.id, selected);
    }
    if detail_lines.is_empty() {
        detail_lines.push(Line::from("No agent selected"));
    }
    f.render_widget(
        Paragraph::new(detail_lines)
            .block(Block::default().borders(Borders::ALL).title(" Agent details "))
            .wrap(ratatui::widgets::Wrap { trim: true }),
        columns[0],
    );

    let items = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let selected = index == selected;
            let style = if selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{index:>2} "), style),
                Span::styled(row.label.clone(), style),
            ]))
        })
        .collect::<Vec<_>>();
    let mut state = picker.state;
    f.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Agents · ←/→ collapse · Enter inspect · Esc close "),
            )
            .highlight_style(Style::default().bg(Color::Indexed(235)).fg(Color::Cyan))
            .highlight_symbol("\u{279c} "),
        columns[1],
        &mut state,
    );
}

fn format_elapsed(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

impl App {
    pub(super) fn open_agents_picker(&mut self) {
        let rows = self.native_agents.rows(
            &self.messages,
            &self.deleg.d.visible(Instant::now(), Duration::MAX),
            &self.state_root,
        );
        if rows.is_empty() {
            self.push_system(
                "No agents or untracked provider runs have been reported in this conversation.",
            );
            return;
        }
        self.show_picker(
            PickerKind::Agents,
            "Agents · ←/→ collapse · Enter inspect · Esc close".into(),
            rows,
            true,
        );
    }
    pub(super) fn toggle_agents_row(&mut self, id: &str) {
        if id == GROUP_ID {
            self.native_agents.toggle_group();
            self.refresh_agents_picker();
            return;
        }
        if let Some(index) = id
            .strip_prefix("native-agent:")
            .and_then(|id| id.parse::<usize>().ok())
        {
            let entry_id = self
                .native_agents
                .cards
                .iter()
                .find(|(_, stored)| *stored == index)
                .map(|(id, _)| id.clone());
            if let Some(entry_id) = entry_id {
                self.native_agents.toggle(&entry_id);
                self.refresh_agents_picker();
            }
        }
    }
    pub(super) fn refresh_agents_picker(&mut self) {
        if !self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.kind == PickerKind::Agents)
        {
            return;
        }
        let filter = self
            .picker
            .as_ref()
            .map(|picker| picker.filter.clone())
            .unwrap_or_default();
        let selected = self.picker.as_ref().and_then(|picker| {
            picker
                .state
                .selected()
                .and_then(|i| picker.filtered().get(i).map(|row| row.id.clone()))
        });
        self.open_agents_picker();
        if let Some(picker) = self.picker.as_mut() {
            picker.filter = filter;
            let index = selected
                .and_then(|id| picker.filtered().iter().position(|row| row.id == id))
                .unwrap_or(0);
            picker.state.select(Some(index));
            picker.select_clamp();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_facts_keep_reported_values_and_unknowns() {
        let known = facts(Some(
            "Role: Impl\nModel: Claude Sonnet 5.5\nEffort: high\nTokens used: 633883\nContext window: 1000000\nCompaction reference: 65.5%\nTasks: 2",
        ));
        assert_eq!(known.role, "Impl");
        assert_eq!(known.model, "Claude Sonnet 5.5");
        assert_eq!(known.used, Some(633_883));
        assert_eq!(known.limit, Some(1_000_000));
        assert_eq!(known.compaction, Some(65.5));
        assert_eq!(known.turns, Some(2));

        let unknown = facts(None);
        assert_eq!(unknown.role, "unknown");
        assert_eq!(unknown.model, "unknown");
        assert_eq!(context_bar(unknown.used, unknown.limit, 7), "▯▯▯▯▯▯▯");
    }

    fn chat(id: &str, title: &str, detail: &str) -> ChatLine {
        ChatLine {
            role: MsgRole::Card,
            text: title.into(),
            card: Some(StreamCard {
                id: id.into(),
                title: title.into(),
                detail: Some(detail.into()),
                status: CardStatus::Running,
            }),
            meta: None,
        }
    }

    fn pretty(command: &str, output: &str) -> String {
        format!(
            "{}\nOutput: {output}",
            serde_json::to_string_pretty(&serde_json::json!({"command": command, "workdir": "/w"}))
                .unwrap()
        )
    }

    #[test]
    fn untracked_row_is_one_clean_line_for_a_multiline_prompt_with_escapes_and_diff_output() {
        let command = "codex exec -m gpt-5.6-luna \"Fix the parser\nsecond line with \\\"quotes\\\"\nthird\" --json";
        let detail = pretty(command, "diff --git a/x b/x\n-old line\n+new line\nSECRET_BODY");
        assert!(detail.contains("quotes") && detail.contains("\\n"));
        let messages = vec![chat("c1", "shell", &detail)];
        let mut state = State::default();
        state.rebuild(&messages);
        state.toggle_group();
        let dir = tempfile::tempdir().unwrap();
        let rows = state.rows(&messages, &[], dir.path());
        assert_eq!(rows.len(), 2);
        let label = &rows[1].label;
        assert_eq!(label.lines().count(), 1);
        assert!(label.contains("Codex gpt-5.6-luna"), "{label}");
        assert!(label.contains("Fix the parser"), "{label}");
        for leak in ["second line", "quotes", "\\", "diff", "old line", "SECRET_BODY", "role", "unknown"] {
            assert!(!label.contains(leak) && !rows[1].detail.contains(leak), "{leak}: {label}");
        }
    }

    #[test]
    fn model_flags_and_prompt_sources_are_parsed() {
        let run = |detail: &str| provider_run(detail).unwrap();
        assert_eq!(run("command: codex exec -m gpt-x \"do it\"").model.as_deref(), Some("gpt-x"));
        assert_eq!(run("command: codex exec --model gpt-y \"do it\"").model.as_deref(), Some("gpt-y"));
        assert_eq!(run("command: codex exec --model=gpt-z \"do it\"").model.as_deref(), Some("gpt-z"));
        let plain = run("command: codex exec --json -C /w \"do it\"");
        assert_eq!((plain.model, plain.task.as_str()), (None, "do it"));
        let claude = run(&pretty("claude -p --model haiku 'say hi'", ""));
        assert_eq!((claude.provider, claude.model.as_deref(), claude.task.as_str()), ("Claude", Some("haiku"), "say hi"));
        assert_eq!(run("cursor-agent -p --model auto \"go\"").provider, "Cursor");
        assert_eq!(
            run("opencode run --model openai/gpt-5 \"review it\"").provider,
            "opencode"
        );
        assert_eq!(run("{\"cmd\":[\"bash\",\"-lc\",\"cd /w && codex exec 'nested task'\"]}").task, "nested task");
        let stdin = run("command: codex exec - <<'EOF'\nBODY TEXT\nEOF");
        assert_eq!(stdin.task, NO_TASK);
        assert_eq!(run("arguments: {\"command\":\"codex exec\"}").task, NO_TASK);
        assert!(provider_run("command: echo \"codex exec x\"").is_none());
        assert!(provider_run("command: claude --version").is_none());
        let styled = run("command: codex exec \"\u{1b}[31m  red\u{1b}[0m \u{7}  task\"");
        assert_eq!(styled.task, "red task");
        assert_eq!(cut("abcdef", 4), "abc…");
    }

    #[test]
    fn other_runs_group_is_collapsed_by_default_and_expands_on_toggle() {
        let messages = vec![
            chat("c1", "shell", "command: codex exec \"one\""),
            chat("c2", "shell", "command: claude -p \"two\""),
            chat("n1", "agent child", "Task: native work"),
        ];
        let mut state = State::default();
        state.rebuild(&messages);
        let dir = tempfile::tempdir().unwrap();
        let rows = state.rows(&messages, &[], dir.path());
        assert_eq!(rows.len(), 2);
        assert!(rows[0].detail.contains("native work"));
        assert_eq!(rows[1].id, GROUP_ID);
        assert_eq!(rows[1].label, "▸ Other runs on this machine (2)");
        state.toggle_group();
        let rows = state.rows(&messages, &[], dir.path());
        assert_eq!(rows.len(), 4);
        assert!(rows[1].label.starts_with("▾"));
        assert!(rows[2].label.contains("Codex") && rows[3].label.contains("Claude"));
    }

    #[test]
    fn untracked_detail_states_the_limit_once() {
        let run = provider_run("command: codex exec \"fix it\"").unwrap();
        let text = untracked_detail(&run, Status::Running, "5s", 3).join("\n");
        assert_eq!(text.matches("cannot receive messages or questions").count(), 1);
        for field in ["Provider: Codex", "Model: unknown", "running", "Elapsed: 5s", "Task: fix it"] {
            assert!(text.contains(field), "{field}");
        }
        assert!(!text.contains("tokens"));
        assert!(!untracked_label(1, Status::Running, &run).contains("cannot receive"));
    }
}
