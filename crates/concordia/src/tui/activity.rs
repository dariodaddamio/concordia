use super::{ChatLine, MsgRole};
use crate::provider::stream_cards::{
    format_card_line, tool_label, CardStatus, StreamCard, ToolKind,
};
use ratatui::style::{Color, Modifier, Style};

/// Kind icons share one accent so the status mark keeps its meaning.
pub(super) const KIND_ICON: Color = Color::Indexed(110);

/// Style the presentation, never the provider payload. All offsets are Unicode characters.
pub(super) fn card_runs(text: &str, card: Option<&StreamCard>) -> Vec<super::transcript::LineRun> {
    let heading = text.lines().next().unwrap_or("");
    let mut runs = Vec::new();
    let muted = Style::default().fg(Color::Indexed(244));
    let mut mark = |start: usize, end: usize, style: Style| {
        runs.push((
            0,
            heading[..start].chars().count(),
            heading[..end].chars().count(),
            style,
        ));
    };
    mark(0, heading.len(), Style::default().fg(Color::Indexed(245)));
    if let Some(card) = card {
        let status = match card.status {
            CardStatus::Running => Color::Cyan,
            CardStatus::Done => Color::Green,
            CardStatus::Error => Color::Red,
        };
        for symbol in ['◉', '✓', '✗'] {
            if let Some(start) = heading
                .strip_prefix("│ ")
                .filter(|body| body.starts_with(symbol))
                .map(|_| "│ ".len())
            {
                mark(
                    start,
                    start + symbol.len_utf8(),
                    Style::default().fg(status),
                );
                break;
            }
        }
        let label = tool_label(&card.title);
        let mut body = heading
            .strip_prefix("│ ")
            .unwrap_or(heading)
            .trim_start_matches(['◉', '✓', '✗'])
            .trim_start();
        let icon = ToolKind::of(card).icon();
        if let Some(rest) = body.strip_prefix(icon) {
            let start = heading.len() - body.len();
            mark(start, start + icon.len(), Style::default().fg(KIND_ICON));
            body = rest.trim_start();
        }
        let mut start = heading.len() - body.len();
        for part in body.split(" + ") {
            // The action and its subject form one heading. Styling only guessed action words
            // made adjacent words/files change weight with provider label capitalization.
            let end = part
                .find(" · ")
                .or_else(|| part.find(" - "))
                .unwrap_or(part.len());
            mark(
                start,
                start + end,
                Style::default()
                    .fg(Color::Indexed(252))
                    .add_modifier(Modifier::BOLD),
            );
            start += part.len() + 3;
        }
        // Only edit summaries own +/- counts; command flags and output are never coloured as edits.
        if matches!(label.as_str(), "Edit file" | "Write file") {
            for token in heading.split_whitespace() {
                let color = match token.as_bytes().first() {
                    Some(b'+') => Color::Green,
                    Some(b'-') => Color::Red,
                    _ => continue,
                };
                if token.len() > 1 && token[1..].bytes().all(|b| b.is_ascii_digit()) {
                    if let Some(start) = heading.rfind(token) {
                        mark(start, start + token.len(), Style::default().fg(color));
                    }
                }
            }
        }
    }
    for separator in [" - ", " · "] {
        if let Some(start) = heading.rfind(separator) {
            if matches!(&heading[start + separator.len()..], "Expand" | "Collapse") {
                mark(start, heading.len(), muted);
                break;
            }
        }
    }
    runs
}

/// `m:ss`, or `h:mm:ss` from an hour.
pub(super) fn clock(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// A running card's elapsed time and, when the provider set a timeout, the time left:
/// ` · 0:42 · 9:18 left of 10:00`. Finished cards have no timer.
pub(super) fn card_timer(
    card: &StreamCard,
    started: Option<std::time::Instant>,
    now: std::time::Instant,
) -> Option<String> {
    if card.status != CardStatus::Running {
        return None;
    }
    let elapsed = now.saturating_duration_since(started?);
    let mut timer = format!(" · {}", clock(elapsed));
    if let Some(limit) = crate::provider::stream_cards::reported_timeout(card) {
        timer.push_str(&format!(
            " · {} left of {}",
            clock(limit.saturating_sub(elapsed)),
            clock(limit)
        ));
    }
    Some(timer)
}

/// The title row is an accordion target; links retain their file/web actions.
pub(super) fn card_header_at(app: &super::App, x: u16, y: u16) -> Option<usize> {
    let area = app.areas.transcript;
    if !super::contains(area, x, y) || app.picker.is_some() || app.activity.detail.is_some() {
        return None;
    }
    let pos = app
        .view
        .pos_at((y - area.y) as isize, (x - area.x) as usize)?;
    let row = app.view.rows.get(pos.row)?;
    let index = row.identity?;
    let mut first = pos.row;
    while first > 0 && app.view.rows[first].joins_prev {
        first -= 1;
    }
    if !app.messages.get(index)?.toggles()
        || app.view.rows[first].identity != Some(index)
        || app.view.rows[first].source_offset != 0
    {
        return None;
    }
    if row
        .links
        .iter()
        .any(|(start, end, _)| *start <= pos.col && pos.col < *end)
        || row
            .web_links
            .iter()
            .any(|(start, end, _)| *start <= pos.col && pos.col < *end)
    {
        return None;
    }
    Some(index)
}
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Line;
use std::collections::HashSet;

/// Presentation only: message indexes, native IDs and complete tool output stay untouched.
pub(super) struct GroupedCards {
    pub text: Vec<Option<String>>,
    pub hidden: Vec<bool>,
}

pub(super) fn grouped_cards(messages: &[ChatLine], activity: &Activity, all: bool) -> GroupedCards {
    let mut grouped = GroupedCards {
        text: vec![None; messages.len()],
        hidden: vec![false; messages.len()],
    };
    let mut start = 0;
    while start < messages.len() {
        let Some(category) = messages[start].card.as_ref().and_then(group_category) else {
            start += 1;
            continue;
        };
        if !matches!(messages[start].role, MsgRole::Card) {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < messages.len()
            && matches!(messages[end].role, MsgRole::Card)
            && messages[end].meta == messages[start].meta
            && messages[end].card.as_ref().and_then(group_category) == Some(category)
        {
            end += 1;
        }
        if end - start > 1 {
            // The first card's icon, so the heading styles the same way as a single card.
            let icon = messages[start]
                .card
                .as_ref()
                .map_or("›", |card| ToolKind::of(card).icon());
            let summary = format!(
                "│ ✓ {icon} {category} · {} operations · completed",
                end - start
            );
            if activity.is_expanded(start, all) {
                let card = messages[start].card.as_ref().unwrap();
                grouped.text[start] = Some(format!(
                    "{summary}\n{}",
                    full_output(&format_card_line(card), card.detail.as_deref())
                ));
            } else {
                grouped.text[start] = Some(summary);
                grouped.hidden[start + 1..end].fill(true);
            }
        }
        start = end;
    }
    grouped
}

fn group_category(card: &StreamCard) -> Option<&'static str> {
    // Errors, live operations, approvals, edits and agent cards remain individually visible.
    if card.status != CardStatus::Done || card.id.starts_with("native-agent:") {
        return None;
    }
    let title = tool_label(&card.title);
    match title.as_str() {
        "Read file" | "Search files" => Some("Inspect project"),
        "Run command" => {
            let detail = card.detail.as_deref().unwrap_or("");
            let lower = detail.to_ascii_lowercase();
            if lower.contains("approval") || lower.contains("require_escalated") {
                return None;
            }
            let mut subject = detail.lines().next().unwrap_or("").trim().to_owned();
            let json = detail
                .strip_prefix("arguments: ")
                .or_else(|| detail.strip_prefix("input: "))
                .unwrap_or(detail);
            if let Some(Ok(value)) = serde_json::Deserializer::from_str(json)
                .into_iter::<serde_json::Value>()
                .next()
            {
                if let Some(command) = value
                    .get("cmd")
                    .or_else(|| value.get("command"))
                    .and_then(serde_json::Value::as_str)
                {
                    subject = command.to_owned();
                }
            }
            let command = subject
                .trim()
                .trim_start_matches("command: ")
                .to_ascii_lowercase();
            if [
                "cargo test",
                "cargo check",
                "cargo clippy",
                "cargo build",
                "cargo fmt",
                "npm test",
                "npm run test",
                "npm run build",
                "npm run lint",
                "pnpm test",
                "pnpm exec",
                "pnpm build",
                "pnpm lint",
                "pytest",
                "python -m pytest",
                "tsc ",
            ]
            .iter()
            .any(|prefix| command.starts_with(prefix))
            {
                Some("Check project")
            } else if [
                "rg ",
                "grep ",
                "cat ",
                "ls",
                "pwd",
                "get-content ",
                "get-childitem",
                "git status",
                "git diff",
                "git log",
                "git show",
            ]
            .iter()
            .any(|prefix| command == prefix.trim() || command.starts_with(prefix))
            {
                Some("Inspect project")
            } else {
                Some("Run commands")
            }
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModalAction {
    Close,
    Copy,
    Paste,
    PreviousCard,
    NextCard,
    Consumed,
}

pub(super) fn full_output(title: &str, detail: Option<&str>) -> String {
    format!("{title}\n{}", detail.unwrap_or(""))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Match {
    pub line: usize,
    pub col: usize,
    pub len: usize,
}

#[derive(Default, Clone)]
pub(super) struct Activity {
    pub expanded: HashSet<usize>,
    pub collapsed: HashSet<usize>,
    pub detail: Option<usize>,
    pub detail_text: Option<(String, String)>,
    pub query: String,
    pub query_cursor: usize,
    pub matches: Vec<Match>,
    pub selected_match: usize,
    pub detail_scroll: u16,
    pub follow_match: bool,
    pub render_lines: Vec<Line<'static>>,
    pub render_width: usize,
    pub render_selected: usize,
    pub render_dirty: bool,
    pub file_targets: Vec<(ratatui::layout::Rect, crate::file_links::Candidate)>,
    pub pressed_file: Option<crate::file_links::Candidate>,
    searched_query: Option<String>,
    search_dirty: bool,
}

impl Activity {
    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => ModalAction::Close,
            KeyCode::Enter => {
                self.next(key.modifiers.contains(KeyModifiers::SHIFT));
                ModalAction::Consumed
            }
            KeyCode::Backspace => {
                self.query_backspace();
                ModalAction::Consumed
            }
            KeyCode::Delete => {
                self.query_delete();
                ModalAction::Consumed
            }
            KeyCode::Char('u' | 'U') if ctrl => {
                self.query.clear();
                self.query_cursor = 0;
                self.invalidate_search();
                ModalAction::Consumed
            }
            KeyCode::Left => {
                self.query_cursor = self.query_cursor.saturating_sub(1);
                ModalAction::Consumed
            }
            KeyCode::Right => {
                self.query_cursor = (self.query_cursor + 1).min(self.query.chars().count());
                ModalAction::Consumed
            }
            KeyCode::Home => {
                self.query_cursor = 0;
                ModalAction::Consumed
            }
            KeyCode::End => {
                self.query_cursor = self.query.chars().count();
                ModalAction::Consumed
            }
            KeyCode::Up if alt => ModalAction::PreviousCard,
            KeyCode::Down if alt => ModalAction::NextCard,
            KeyCode::Up => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
                self.follow_match = false;
                ModalAction::Consumed
            }
            KeyCode::Down => {
                self.detail_scroll = self.detail_scroll.saturating_add(1);
                self.follow_match = false;
                ModalAction::Consumed
            }
            KeyCode::PageUp => {
                self.detail_scroll = self.detail_scroll.saturating_sub(10);
                self.follow_match = false;
                ModalAction::Consumed
            }
            KeyCode::PageDown => {
                self.detail_scroll = self.detail_scroll.saturating_add(10);
                self.follow_match = false;
                ModalAction::Consumed
            }
            KeyCode::Char(c) if ctrl && c.eq_ignore_ascii_case(&'c') => ModalAction::Copy,
            KeyCode::Char(c) if ctrl && c.eq_ignore_ascii_case(&'v') => ModalAction::Paste,
            KeyCode::Char(c) if !ctrl && !alt => {
                self.insert_query(&c.to_string());
                ModalAction::Consumed
            }
            _ => ModalAction::Consumed,
        }
    }
    pub fn insert_query(&mut self, text: &str) {
        let clean: Vec<char> = text
            .chars()
            .filter(|c| *c != '\r')
            .flat_map(|c| if c == '\t' { vec![' '; 4] } else { vec![c] })
            .collect();
        let mut chars: Vec<char> = self.query.chars().collect();
        let at = self.query_cursor.min(chars.len());
        chars.splice(at..at, clean.iter().copied());
        self.query_cursor = at + clean.len();
        self.query = chars.into_iter().collect();
        self.invalidate_search();
    }
    pub fn query_backspace(&mut self) {
        let mut c: Vec<char> = self.query.chars().collect();
        if self.query_cursor > 0 && self.query_cursor <= c.len() {
            self.query_cursor -= 1;
            c.remove(self.query_cursor);
            self.query = c.into_iter().collect();
            self.invalidate_search();
        }
    }
    pub fn query_delete(&mut self) {
        let mut c: Vec<char> = self.query.chars().collect();
        if self.query_cursor < c.len() {
            c.remove(self.query_cursor);
            self.query = c.into_iter().collect();
            self.invalidate_search();
        }
    }
    pub fn is_expanded(&self, message: usize, all: bool) -> bool {
        !self.collapsed.contains(&message) && (all || self.expanded.contains(&message))
    }

    pub fn toggle_card(&mut self, message: usize, all: bool) {
        if all {
            if !self.collapsed.remove(&message) {
                self.collapsed.insert(message);
            }
        } else {
            self.collapsed.remove(&message);
            self.toggle(message);
        }
    }

    pub fn set_expanded(&mut self, message: usize, expanded: bool, all: bool) {
        if expanded {
            self.collapsed.remove(&message);
            self.expanded.insert(message);
        } else {
            self.expanded.remove(&message);
            if all {
                self.collapsed.insert(message);
            }
        }
    }

    /// Ctrl+O: expand the last collapsed item of the last message; when none is collapsed,
    /// collapse its last item. Ctrl+Shift+O (`every`): expand all of them, or collapse all
    /// when they are already open. Returns the number of items changed.
    pub fn expand_last_message(
        &mut self,
        messages: &[ChatLine],
        all: bool,
        every: bool,
    ) -> usize {
        let hidden = grouped_cards(messages, self, all).hidden;
        let start = messages
            .iter()
            .rposition(|m| m.role == MsgRole::User)
            .map_or(0, |user| user + 1);
        let items: Vec<usize> = (start..messages.len())
            .filter(|&i| messages[i].toggles() && !hidden[i])
            .collect();
        let collapsed: Vec<usize> = items
            .iter()
            .copied()
            .filter(|&i| !self.is_expanded(i, all))
            .collect();
        let (targets, expand) = match (every, collapsed.last()) {
            (true, Some(_)) => (collapsed.clone(), true),
            (true, None) => (items.clone(), false),
            (false, Some(&last)) => (vec![last], true),
            (false, None) => (items.last().copied().into_iter().collect(), false),
        };
        for &message in &targets {
            self.set_expanded(message, expand, all);
        }
        targets.len()
    }

    pub fn toggle(&mut self, message: usize) {
        if !self.expanded.remove(&message) {
            self.expanded.insert(message);
        }
    }
    pub fn open(&mut self, message: usize) {
        self.detail_text = None;
        self.file_targets.clear();
        self.pressed_file = None;
        self.detail = Some(message);
        self.query.clear();
        self.query_cursor = 0;
        self.matches.clear();
        self.selected_match = 0;
        self.detail_scroll = 0;
        self.follow_match = true;
        self.searched_query = None;
        self.search_dirty = true;
        self.render_dirty = true;
    }
    pub fn search_parts(&mut self, parts: &[&str]) {
        if !self.search_dirty && self.searched_query.as_ref() == Some(&self.query) {
            return;
        }
        let old = self.matches.get(self.selected_match).copied();
        self.matches.clear();
        if !self.query.is_empty() {
            let needle: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
            let mut prefix = vec![0; needle.len()];
            for i in 1..needle.len() {
                let mut j = prefix[i - 1];
                while j > 0 && needle[i] != needle[j] {
                    j = prefix[j - 1];
                }
                if needle[i] == needle[j] {
                    j += 1;
                }
                prefix[i] = j;
            }
            let mut line_no = 0;
            for part in parts {
                for line in part.lines() {
                    // Case folding can expand a source character. Keep a parallel map back to
                    // source character columns instead of using folded byte offsets on the source.
                    let mut folded = Vec::new();
                    let mut source_col = Vec::new();
                    for (col, ch) in line.chars().enumerate() {
                        for lower in ch.to_lowercase() {
                            folded.push(lower);
                            source_col.push(col);
                        }
                    }
                    if needle.is_empty() {
                        continue;
                    }
                    let mut matched = 0;
                    for (i, ch) in folded.iter().enumerate() {
                        while matched > 0 && *ch != needle[matched] {
                            matched = prefix[matched - 1];
                        }
                        if *ch == needle[matched] {
                            matched += 1;
                        }
                        if matched == needle.len() {
                            let at = i + 1 - needle.len();
                            self.matches.push(Match {
                                line: line_no,
                                col: source_col[at],
                                len: source_col[at..at + needle.len()]
                                    .last()
                                    .unwrap_or(&source_col[at])
                                    .saturating_sub(source_col[at])
                                    + 1,
                            });
                            matched = prefix[matched - 1];
                        }
                    }
                    line_no += 1;
                }
            }
        }
        self.selected_match = old
            .and_then(|m| self.matches.iter().position(|n| *n == m))
            .unwrap_or(0);
        self.searched_query = Some(self.query.clone());
        self.search_dirty = false;
        self.render_dirty = true;
    }
    pub fn next(&mut self, backwards: bool) {
        if !self.matches.is_empty() {
            self.selected_match = if backwards {
                self.selected_match
                    .checked_sub(1)
                    .unwrap_or(self.matches.len() - 1)
            } else {
                (self.selected_match + 1) % self.matches.len()
            };
        }
        self.follow_match = true;
        self.render_dirty = true;
    }
    pub fn query_with_cursor(&self) -> String {
        const WINDOW: usize = 46;
        let chars: Vec<char> = self.query.chars().collect();
        let cursor = self.query_cursor.min(chars.len());
        let start = cursor.saturating_sub(WINDOW / 2);
        let end = (start + WINDOW).min(chars.len());
        let mut out = String::new();
        if start > 0 {
            out.push('…');
        }
        for (i, ch) in chars.iter().enumerate().take(end).skip(start) {
            if i == self.query_cursor {
                out.push('|');
            }
            out.push(*ch);
        }
        if cursor >= end {
            out.push('|');
        }
        if end < chars.len() {
            out.push('…');
        }
        out
    }
    pub fn selected(&self) -> Option<Match> {
        self.matches.get(self.selected_match).copied()
    }
    pub fn invalidate_search(&mut self) {
        self.search_dirty = true;
        self.render_dirty = true;
        self.follow_match = true;
    }
}
