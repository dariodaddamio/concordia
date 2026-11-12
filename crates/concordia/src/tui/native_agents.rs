//! Read-only Agents view projection for provider-native and untracked provider runs.
use super::*;
use crate::provider::stream_cards::{CardStatus, StreamCard};
use std::collections::{HashMap, HashSet};

pub(super) fn panel_area(area: Rect) -> Rect {
    if area.width < 96 { return area; }
    let width = (area.width / 2).clamp(48, 86);
    Rect { x: area.x + area.width - width, width, ..area }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind { Native, Untracked }

#[derive(Clone)]
struct Entry { id: String, index: usize, parent: Option<String>, kind: Kind, started: Instant }

#[derive(Default)]
pub(super) struct State {
    cards: Vec<(String, usize)>,
    launch_tools: Vec<String>,
    entries: HashMap<String, Entry>,
    collapsed: HashSet<String>,
    last_native_parent: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status { Running, Done, Error }

impl Status {
    fn from(card: CardStatus) -> Self { match card { CardStatus::Running => Self::Running, CardStatus::Done => Self::Done, CardStatus::Error => Self::Error } }
    fn icon(self) -> &'static str { match self { Self::Running => "●", Self::Done => "✓", Self::Error => "×" } }
    fn word(self) -> &'static str { match self { Self::Running => "running", Self::Done => "finished", Self::Error => "failed/interrupted" } }
}

fn task_title(detail: Option<&str>, fallback: &str) -> String {
    let line = detail.unwrap_or("").lines().find_map(|line| line.strip_prefix("Task: ").or_else(|| line.strip_prefix("task: "))).unwrap_or("").trim();
    let title = serde_json::from_str::<String>(line).unwrap_or_else(|_| line.into());
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() { fallback.chars().take(120).collect() } else { title.chars().take(120).collect() }
}

/// Recognise only provider CLI launchers in structured tool-call arguments. Ordinary shell cards
/// remain ordinary activity; no command is executed or inferred from prose.
fn provider_command(detail: &str) -> Option<String> {
    let lower = detail.to_ascii_lowercase();
    if !["codex exec", "claude -p", "cursor-agent", "grok "].iter().any(|needle| lower.contains(needle)) { return None; }
    let command = detail.lines().find_map(|line| {
        let line = line.trim();
        ["command: ", "cmd: ", "command = ", "cmd = "].iter().find_map(|prefix| line.strip_prefix(prefix))
    }).unwrap_or_else(|| {
        ["codex exec", "claude -p", "cursor-agent", "grok "].iter().find_map(|needle| lower.find(needle).map(|i| &detail[i..])).unwrap_or(detail)
    });
    Some(command.trim_matches(['"', '\'']).chars().take(120).collect())
}

fn is_native_agent(id: &str, title: &str, detail: &str) -> bool {
    let lower = title.to_ascii_lowercase();
    id.starts_with("native-agent:") || lower.starts_with("agent ") || (matches!(title, "Agent" | "Task") && (detail.contains("Task:") || detail.contains("Child:")))
}

impl State {
    pub fn record(&mut self, card: &StreamCard, index: usize) {
        let id = &card.id;
        if let Some(identity) = id.strip_prefix("native-agent:claude:") {
            if let Ok((_, _, Some(tool))) = serde_json::from_str::<(String, String, Option<String>)>(identity) {
                if !self.launch_tools.contains(&tool) { if self.launch_tools.len() >= 64 { self.launch_tools.remove(0); } self.launch_tools.push(tool.clone()); }
                self.cards.retain(|(old, _)| old != &tool);
            }
        }
        if self.launch_tools.contains(id) { return; }
        let native = is_native_agent(id, &card.title, card.detail.as_deref().unwrap_or(""));
        let untracked = !native && provider_command(card.detail.as_deref().unwrap_or("")).is_some();
        if !native && !untracked { return; }
        if let Some((_, old)) = self.cards.iter_mut().find(|(old, _)| old == id) { *old = index; } else { if self.cards.len() >= 128 { self.cards.remove(0); } self.cards.push((id.into(), index)); }
        let sender = card.detail.as_deref().and_then(|detail| detail.lines().find_map(|line| line.strip_prefix("Sender: ").map(str::to_owned)));
        let parent = if untracked { self.last_native_parent.clone() } else { sender };
        if native { self.last_native_parent = Some(id.clone()); }
        let entry = self.entries.entry(id.clone()).or_insert_with(|| Entry { id: id.clone(), index, parent, kind: if untracked { Kind::Untracked } else { Kind::Native }, started: Instant::now() });
        entry.index = index;
        if entry.parent.is_none() && untracked { entry.parent = self.last_native_parent.clone(); }
    }
    pub fn count(&self) -> usize { self.entries.len() }
    fn toggle(&mut self, id: &str) { if !self.collapsed.remove(id) { self.collapsed.insert(id.into()); } }

    fn rows(&self, messages: &[ChatLine], workers: &[&crate::delegate::Job]) -> Vec<PickerRow> {
        let mut children: HashMap<Option<&str>, Vec<(&Entry, &StreamCard)>> = HashMap::new();
        let mut all_entries = Vec::new();
        let mut parent = None;
        for (index, line) in messages.iter().enumerate() {
            let Some(card) = line.card.as_ref() else { continue; };
            let id = card.id.as_str();
            let native = is_native_agent(id, &card.title, card.detail.as_deref().unwrap_or(""));
            let untracked = !native && provider_command(card.detail.as_deref().unwrap_or("")).is_some();
            if !native && !untracked { continue; }
            let mut entry = self.entries.get(id).cloned().unwrap_or(Entry {
                id: id.into(), index, parent: if untracked { parent.clone() } else { None },
                kind: if untracked { Kind::Untracked } else { Kind::Native }, started: Instant::now(),
            });
            entry.index = index;
            if native { parent = Some(id.into()); }
            all_entries.push(entry);
        }
        for entry in &all_entries {
            if let Some(card) = messages.get(entry.index).and_then(|line| line.card.as_ref()) {
                children.entry(entry.parent.as_deref()).or_default().push((entry, card));
            }
        }
        for rows in children.values_mut() { rows.sort_by_key(|(entry, _)| entry.index); }
        let mut out = Vec::new();
        fn add(out: &mut Vec<PickerRow>, children: &HashMap<Option<&str>, Vec<(&Entry, &StreamCard)>>, parent: Option<&str>, depth: usize, collapsed: &HashSet<String>) {
            for (entry, card) in children.get(&parent).into_iter().flatten() {
                let status = Status::from(card.status);
                let child_count = children.get(&Some(entry.id.as_str())).map_or(0, Vec::len);
                let disclosure = if child_count == 0 { " " } else if collapsed.contains(&entry.id) { "▸" } else { "▾" };
                let title = if entry.kind == Kind::Untracked {
                    provider_command(card.detail.as_deref().unwrap_or("")).unwrap_or_else(|| "Provider run".into())
                } else {
                    let task = task_title(card.detail.as_deref(), &card.title);
                    if card.title.starts_with("agent ") && !task.contains(card.title.trim_start_matches("agent ")) { format!("{task} · {}", card.title.trim_start_matches("agent ")) } else { task }
                };
                let elapsed = if status == Status::Running { format_elapsed(entry.started.elapsed()) } else { String::new() };
                let label = format!("{}{} {} {} · {}{}", "  ".repeat(depth), disclosure, status.icon(), status.word(), title, if elapsed.is_empty() { String::new() } else { format!(" · {elapsed}") });
                let detail = if entry.kind == Kind::Untracked { "Untracked provider run · cannot receive messages or questions".into() } else { format!("{} · {}", status.word(), card.detail.as_deref().unwrap_or("recent activity").lines().filter(|line| !line.starts_with("Task: ")).take(2).collect::<Vec<_>>().join(" · ")) };
                out.push(PickerRow { id: format!("native-agent:{}", entry.index), label, detail });
                if !collapsed.contains(&entry.id) { add(out, children, Some(entry.id.as_str()), depth + 1, collapsed); }
            }
        }
        add(&mut out, &children, None, 0, &self.collapsed);
        out.extend(workers.iter().map(|job| PickerRow { id: format!("worker-agent:{}", job.id), label: format!("  {} {} · {}", if job.state.is_running() { "●" } else { "✓" }, task_title(Some(&job.task), &job.task), format_elapsed(job.elapsed(Instant::now()))), detail: format!("{} · provider {} · model {}", job.state.label(), job.provider.as_str(), job.model.as_deref().unwrap_or("default")) }));
        out
    }
}

fn format_elapsed(duration: Duration) -> String { let secs = duration.as_secs(); if secs < 60 { format!("{secs}s") } else { format!("{}m{:02}s", secs / 60, secs % 60) } }

impl App {
    pub(super) fn open_agents_picker(&mut self) {
        let rows = self.native_agents.rows(&self.messages, &self.deleg.d.visible(Instant::now(), Duration::MAX));
        if rows.is_empty() { self.push_system("No agents or untracked provider runs have been reported in this conversation."); return; }
        self.show_picker(PickerKind::Agents, "Agents · ←/→ collapse · Enter inspect · Esc close".into(), rows, true);
    }
    pub(super) fn toggle_agents_row(&mut self, id: &str) {
        if let Some(index) = id.strip_prefix("native-agent:").and_then(|id| id.parse::<usize>().ok()) {
            let entry_id = self.native_agents.cards.iter().find(|(_, stored)| *stored == index).map(|(id, _)| id.clone());
            if let Some(entry_id) = entry_id { self.native_agents.toggle(&entry_id); self.refresh_agents_picker(); }
        }
    }
    pub(super) fn refresh_agents_picker(&mut self) {
        if !self.picker.as_ref().is_some_and(|picker| picker.kind == PickerKind::Agents) { return; }
        let filter = self.picker.as_ref().map(|picker| picker.filter.clone()).unwrap_or_default();
        let selected = self.picker.as_ref().and_then(|picker| picker.state.selected().and_then(|i| picker.filtered().get(i).map(|row| row.id.clone())));
        self.open_agents_picker();
        if let Some(picker) = self.picker.as_mut() { picker.filter = filter; let index = selected.and_then(|id| picker.filtered().iter().position(|row| row.id == id)).unwrap_or(0); picker.state.select(Some(index)); picker.select_clamp(); }
    }
}
