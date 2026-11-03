use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Line;
use std::collections::HashSet;

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

    pub fn toggle(&mut self, message: usize) {
        if !self.expanded.remove(&message) {
            self.expanded.insert(message);
        }
    }
    pub fn open(&mut self, message: usize) {
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
