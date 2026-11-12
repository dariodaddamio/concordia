//! `@chat` references: `@` in the prompt completes to a recent thread, and on send each
//! `@<id-or-prefix>` is replaced by that thread's map (docs/architecture.md, "Delivery").
//!
//! The pure pieces (finding the token being typed, filtering, expansion) are plain functions so
//! they are tested without a terminal. The slash palette is untouched: this is a separate mode with
//! its own state, drawn in the same spot and answering the same navigation keys.

use super::*;
use crate::threads::{self, ThreadMeta};

/// At most this many references are expanded per prompt; the rest stay literal.
pub const MAX_REFS: usize = 3;
/// A thread list younger than this is reused instead of re-reading `<state>/threads`.
const LIST_TTL: Duration = Duration::from_secs(5);
const EMPTY_NOTICE: &str = "no recorded chats - enable [threads] in config.toml";

// ---------------------------------------------------------------------------------------------
// Pure logic
// ---------------------------------------------------------------------------------------------

/// Characters that may directly precede an `@` that starts a reference.
fn opens_word(prev: Option<char>) -> bool {
    prev.is_none_or(|c| c.is_whitespace() || matches!(c, '(' | '[' | '{' | '"' | '\''))
}

fn is_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Thread ids are UUIDs: hex digits and dashes. Anything else after `@` is not a reference.
fn looks_like_id(run: &str) -> bool {
    let n = run.chars().count();
    (4..=36).contains(&n) && run.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// The `@query` being typed at the end of the prompt: (byte offset of the `@`, text after it).
/// None when the last word is not a reference start (`a@b.com`, `\@x`, no `@`, ...).
pub fn active_query(text: &str) -> Option<(usize, String)> {
    let word_start = text
        .rfind(char::is_whitespace)
        .map(|i| i + text[i..].chars().next().map_or(1, char::len_utf8))
        .unwrap_or(0);
    let word = &text[word_start..];
    let at = word
        .char_indices()
        .find(|(_, c)| *c == '@')
        .map(|(i, _)| i)
        .filter(|i| *i == 0 || opens_word(word[..*i].chars().last()))?;
    let start = word_start + at;
    if text[..start].ends_with('\\') {
        return None;
    }
    let q = &word[at + 1..];
    if !q.chars().all(is_id_char) {
        return None;
    }
    Some((start, q.to_string()))
}

pub(super) fn reference_query(text: &str) -> Option<(usize, String)> {
    for prefix in ["/open "] {
        if let Some(query) = text.strip_prefix(prefix).filter(|q| !q.contains('\n')) {
            return Some((
                prefix.len(),
                query.trim_matches(['"', '`']).replace('\\', "/"),
            ));
        }
    }
    if let Some(query) = active_query(text) {
        return Some(query);
    }
    let start = text.rfind('@')?;
    if !opens_word(text[..start].chars().last()) || text[..start].ends_with('\\') {
        return None;
    }
    let query = &text[start + 1..];
    let quoted = query.starts_with(['`', '"']);
    if quoted && query.ends_with(char::is_whitespace) {
        return None;
    }
    let query = query.trim_matches(['`', '"']);
    if query.is_empty()
        || query.chars().any(|c| {
            (!quoted && c.is_whitespace())
                || c.is_control()
                || matches!(c, '@' | '`' | '"' | '<' | '>')
        })
    {
        return None;
    }
    Some((start, query.replace('\\', "/")))
}

/// Threads whose id starts with the query or whose title contains it, newest first.
pub fn filter<'a>(all: &'a [ThreadMeta], query: &str) -> Vec<&'a ThreadMeta> {
    let q = query.to_lowercase();
    all.iter()
        .filter(|m| {
            q.is_empty()
                || m.id.to_lowercase().starts_with(&q)
                || m.title.to_lowercase().contains(&q)
        })
        .collect()
}

/// Shortest id prefix of at least 8 characters that no other thread shares.
pub fn short_id(all: &[ThreadMeta], id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    for n in 8..=chars.len() {
        let p: String = chars[..n].iter().collect();
        if all.iter().filter(|m| m.id.starts_with(&p)).count() <= 1 {
            return p;
        }
    }
    id.to_string()
}

/// The prompt with the `@query` at `start` replaced by `@<id> `.
pub fn complete(text: &str, start: usize, short: &str) -> String {
    format!("{}@{short} ", &text[..start])
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Expansion {
    /// The original prompt, followed by separately labelled bounded source maps.
    pub sent: String,
    /// (short id, map) per expanded reference, for the dimmed transcript block.
    pub maps: Vec<(String, String)>,
    /// Why a reference was left as typed.
    pub notices: Vec<String>,
}

/// Keep the original request intact and append unique `@<id-or-unique-prefix>` source maps
/// (at most `map_chars` characters each, with `more:` pointers). Invalid references get notices.
pub fn expand(state_root: &Path, text: &str, map_chars: usize) -> Expansion {
    if !text.contains('@') {
        return Expansion {
            sent: text.to_string(),
            ..Default::default()
        };
    }
    let mut ex = Expansion::default();
    let mut listed: Option<Vec<ThreadMeta>> = None;
    let mut done: Vec<String> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // An escaped reference remains literal, including its original backslash.
        if c == '\\' && chars.get(i + 1) == Some(&'@') {
            i += 2;
            continue;
        }
        let prev = (i > 0).then(|| chars[i - 1]);
        if c != '@' || !opens_word(prev) {
            i += 1;
            continue;
        }
        let end = (i + 1..chars.len())
            .find(|&j| !is_id_char(chars[j]))
            .unwrap_or(chars.len());
        let run: String = chars[i + 1..end].iter().collect();
        // `@abcd.example` reads as a domain, not a reference.
        let domainish = (matches!(chars.get(end), Some('/' | '\\'))
            || (chars.get(end) == Some(&':')
                && chars.get(end + 1).is_some_and(char::is_ascii_digit)))
            || chars.get(end) == Some(&'.')
                && chars
                    .get(end + 1)
                    .is_some_and(|c| c.is_ascii_alphanumeric());
        if !looks_like_id(&run) || domainish {
            i += 1;
            continue;
        }
        let token: String = chars[i..end].iter().collect();
        let metas = listed.get_or_insert_with(|| threads::list(state_root));
        let hits: Vec<&ThreadMeta> = {
            let q = run.to_lowercase();
            let exact: Vec<&ThreadMeta> = metas.iter().filter(|m| m.id == q).collect();
            if exact.is_empty() {
                metas.iter().filter(|m| m.id.starts_with(&q)).collect()
            } else {
                exact
            }
        };
        let literal = |ex: &mut Expansion, why: String| {
            ex.notices.push(format!("{token}: {why} - left as typed"));
        };
        match hits.as_slice() {
            _ if metas.is_empty() => {
                literal(&mut ex, EMPTY_NOTICE.to_string());
            }
            [] => {
                literal(&mut ex, "no recorded chat has that id".to_string());
            }
            [m] => {
                let id = m.id.clone();
                // Repeated references keep their original spelling and share one source map.
                if !done.contains(&id) {
                    if done.len() >= MAX_REFS {
                        literal(&mut ex, format!("at most {MAX_REFS} chats per prompt"));
                        i = end;
                        continue;
                    }
                    match threads::ThreadLog::open(state_root, &id).read_map(map_chars) {
                        Ok(map) => {
                            ex.maps.push((short_id(metas, &id), map));
                            done.push(id);
                        }
                        Err(_) => {
                            literal(&mut ex, "that chat could not be read".to_string());
                        }
                    }
                }
            }
            many => {
                literal(
                    &mut ex,
                    format!("matches {} chats, use more characters", many.len()),
                );
            }
        }
        i = end;
    }
    // Native sources must retain the exact original composer value for reference-only recall.
    // Maps are context for this single request; they do not replace the user's words.
    ex.sent = text.to_string();
    if !ex.maps.is_empty() {
        ex.sent
            .push_str("\n\n[Referenced chats: source context, not new requests]\n");
        for (id, map) in &ex.maps {
            ex.sent.push_str(&format!("@{id}\n{map}\n"));
        }
    }
    let mut seen = std::collections::HashSet::new();
    ex.notices.retain(|notice| seen.insert(notice.clone()));
    ex
}

// ---------------------------------------------------------------------------------------------
// Prompt-box state and the overlay
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct ChatRef {
    list: Vec<ThreadMeta>,
    loaded: Option<Instant>,
    pub selected: usize,
    last_query: Option<String>,
    /// Prompt text at the moment Esc closed the list; it stays closed until the text changes.
    dismissed_for: Option<String>,
    empty_shown_once: bool,
    showing_empty: bool,
    targets: Vec<(Rect, usize)>,
    pressed: Option<usize>,
    last_keys: Vec<String>,
    selection_touched: bool,
}

#[derive(Clone, Copy)]
enum Reference {
    Chat(usize),
    File(usize),
}

/// What the overlay shows right now.
enum View {
    Hidden,
    Empty,
    Rows(Vec<Reference>),
}

impl App {
    fn chat_view(&mut self) -> View {
        let text = self.editor.text();
        let query = if self.palette.dismissed
            || self.picker.is_some()
            || self.hist_pos.is_some()
            || self.workspace.focused.is_some()
            || !self.palette_entries().is_empty()
            || self.chatref.dismissed_for.as_deref() == Some(text.as_str())
        {
            None
        } else {
            reference_query(&text)
        };
        let Some((_, q)) = query else {
            self.chatref.loaded = None;
            self.chatref.showing_empty = false;
            self.chatref.last_query = None;
            return View::Hidden;
        };
        if q.len() > 4096 {
            return View::Hidden;
        }
        let folded_query = q.to_lowercase();
        self.request_related_files();
        if self.chatref.loaded.is_none_or(|t| t.elapsed() > LIST_TTL) {
            self.chatref.list = threads::list(&self.state_root);
            if let Some(current) = self.thread_id().map(str::to_owned) {
                self.chatref.list.retain(|meta| meta.id != current);
            }
            self.chatref.loaded = Some(Instant::now());
        }
        if self.chatref.last_query.as_deref() != Some(q.as_str()) {
            self.chatref.selected = 0;
            self.chatref.last_keys.clear();
            self.chatref.selection_touched = false;
            self.chatref.last_query = Some(q.clone());
        }
        let mut rows: Vec<_> = self
            .related_files
            .entries
            .iter()
            .enumerate()
            .filter(|(_, file)| {
                file.label.to_lowercase().contains(&folded_query)
                    || file
                        .candidate
                        .target
                        .replace('\\', "/")
                        .to_lowercase()
                        .contains(&folded_query)
            })
            .map(|(i, _)| Reference::File(i))
            .collect();
        rows.sort_by_key(|r| {
            if let Reference::File(i) = r {
                if self.related_files.entries[*i]
                    .label
                    .trim_end_matches('/')
                    .eq_ignore_ascii_case(&q)
                {
                    0
                } else {
                    1
                }
            } else {
                1
            }
        });
        if !text.starts_with("/open ") {
            rows.extend(
                self.chatref
                    .list
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| filter(std::slice::from_ref(*m), &q).len() == 1)
                    .map(|(i, _)| Reference::Chat(i)),
            );
        }
        if rows.is_empty() {
            self.chatref.empty_shown_once = true;
            self.chatref.showing_empty = true;
            return View::Empty;
        }
        let keys: Vec<_> = rows
            .iter()
            .map(|entry| match entry {
                Reference::Chat(i) => format!("chat:{}", self.chatref.list[*i].id),
                Reference::File(i) => {
                    format!("file:{}", self.related_files.entries[*i].candidate.target)
                }
            })
            .collect();
        if let Some(key) = self
            .chatref
            .last_keys
            .get(self.chatref.selected)
            .filter(|_| self.chatref.selection_touched)
        {
            if let Some(index) = keys.iter().position(|candidate| candidate == key) {
                self.chatref.selected = index;
            }
        }
        self.chatref.last_keys = keys;
        self.chatref.selected = self.chatref.selected.min(rows.len() - 1);
        View::Rows(rows)
    }

    fn complete_reference(&mut self, entry: Reference) {
        match entry {
            Reference::Chat(index) => self.complete_chat_ref(index),
            Reference::File(index) => {
                let text = self.editor.text();
                let Some((start, _)) = reference_query(&text) else {
                    return;
                };
                if let Some(file) = self.related_files.entries.get(index) {
                    if text.starts_with("/open ") {
                        let name = if file.label.contains(char::is_whitespace) {
                            format!("\"{}\"", file.label)
                        } else {
                            file.label.clone()
                        };
                        self.editor.set_text(&format!("{}{name}", &text[..start]));
                        return;
                    }
                    let reference =
                        if file.label.contains(char::is_whitespace) || looks_like_id(&file.label) {
                            if file.directory {
                                format!("@`{}", file.label)
                            } else {
                                format!("@`{}`", file.label)
                            }
                        } else {
                            format!("@{}", file.label)
                        };
                    self.editor.set_text(&format!(
                        "{}{reference}{}",
                        &text[..start],
                        if file.directory { "" } else { " " }
                    ));
                }
            }
        }
    }

    /// Put the chosen thread's id in the prompt in place of the `@query`.
    fn complete_chat_ref(&mut self, list_idx: usize) {
        let text = self.editor.text();
        let Some((start, _)) = active_query(&text) else {
            return;
        };
        let Some(m) = self.chatref.list.get(list_idx) else {
            return;
        };
        let short = short_id(&self.chatref.list, &m.id);
        self.editor.set_text(&complete(&text, start, &short));
    }

    /// Show the maps that were sent with the prompt (dimmed, under the user's own line) and the
    /// reasons any reference was left alone. Returns the text to send.
    pub(super) fn show_chat_refs(&mut self, ex: Expansion) -> String {
        for (short, map) in &ex.maps {
            self.messages.push(ChatLine {
                role: MsgRole::Meta,
                text: format!("sent with your prompt (@{short}):\n{map}"),
                card: None,
                meta: None,
            });
        }
        for n in &ex.notices {
            self.push_system(n.clone());
        }
        ex.sent
    }
}

/// Navigation keys while the list is open. True when the key was used.
pub(super) fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    let preview = key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('p' | 'P'));
    if (key.modifiers.contains(KeyModifiers::CONTROL) && !preview)
        || key.modifiers.contains(KeyModifiers::ALT)
    {
        return false;
    }
    let View::Rows(rows) = app.chat_view() else {
        return false;
    };
    let n = rows.len();
    if preview {
        if let Reference::File(index) = rows[app.chatref.selected] {
            app.preview_related_file(index);
        }
        return true;
    }
    match key.code {
        KeyCode::Up | KeyCode::BackTab => {
            app.chatref.selection_touched = true;
            app.chatref.selected = (app.chatref.selected + n - 1) % n;
            true
        }
        KeyCode::Down => {
            app.chatref.selection_touched = true;
            app.chatref.selected = (app.chatref.selected + 1) % n;
            true
        }
        KeyCode::Enter if app.editor.text().starts_with("/open ") => {
            if let Reference::File(index) = rows[app.chatref.selected] {
                app.editor.clear();
                app.preview_related_file(index);
            }
            true
        }
        KeyCode::Tab | KeyCode::Enter => {
            app.complete_reference(rows[app.chatref.selected]);
            true
        }
        KeyCode::Esc => {
            app.chatref.dismissed_for = Some(app.editor.text());
            true
        }
        _ => false,
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!(
            "{}...",
            s.chars().take(max.saturating_sub(3)).collect::<String>()
        )
    }
}

/// The list, floating just above the prompt box like the slash palette.
pub(super) fn draw(f: &mut Frame, above: Rect, input: Rect, app: &mut App) {
    app.chatref.targets.clear();
    let view = app.chat_view();
    let references = if let View::Rows(rows) = &view {
        rows.clone()
    } else {
        Vec::new()
    };
    let (items, title, selected): (Vec<ListItem>, String, Option<usize>) = match view {
        View::Hidden => return,
        View::Empty => (
            vec![ListItem::new(Span::styled(
                if app.thread_id().is_some() {
                    "No matching files/chats. /open browses workspace files and folders."
                } else {
                    "No matching files. /open browses folders, recent files and workspace files."
                },
                Style::default().fg(Color::Indexed(245)),
            ))],
            " files / chats ".to_string(),
            None,
        ),
        View::Rows(rows) => {
            let width = (f.area().width.saturating_sub(4) as usize).clamp(30, 90);
            let items = rows
                .iter()
                .map(|entry| {
                    let i = match *entry {
                        Reference::File(index) => {
                            let file = &app.related_files.entries[index];
                            return ListItem::new(Line::from(vec![
                                Span::styled("file  ", Style::default().fg(Color::Cyan)),
                                Span::raw(clip(&file.label, width.saturating_sub(24))),
                                Span::styled(
                                    "  Ctrl+P preview",
                                    Style::default().fg(Color::DarkGray),
                                ),
                            ]));
                        }
                        Reference::Chat(index) => index,
                    };
                    let m = &app.chatref.list[i];
                    let id = short_id(&app.chatref.list, &m.id);
                    let tail = format!(
                        "  {}{}",
                        threads::age_label(m.started),
                        m.account
                            .as_deref()
                            .map(|a| format!(" {a}"))
                            .unwrap_or_default()
                    );
                    let title = if m.title.is_empty() {
                        "(untitled)"
                    } else {
                        &m.title
                    };
                    let room = width.saturating_sub(id.len() + tail.len() + 6).max(8);
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("@{id}  "),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(clip(title, room)),
                        Span::styled(tail, Style::default().fg(Color::DarkGray)),
                    ]))
                })
                .collect::<Vec<_>>();
            let shown = items.len();
            (
                items,
                format!(" files/chats \u{b7} {shown} \u{b7} Tab/Enter completes "),
                Some(app.chatref.selected),
            )
        }
    };
    let shown = items.len().min(10);
    let height = shown as u16 + 2;
    let width = (f.area().width.saturating_sub(input.x + 2) as usize).clamp(20, 94) as u16;
    let y = input.y.saturating_sub(height).max(above.y);
    let rect = Rect {
        x: input.x + 1,
        y,
        width: width.min(f.area().width.saturating_sub(input.x + 1)),
        height: height.min(input.y.saturating_sub(above.y).max(3)),
    };
    f.render_widget(Clear, rect);
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(title),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Indexed(238))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select(selected);
    f.render_stateful_widget(list, rect, &mut state);
    for (row, index) in (state.offset()..references.len())
        .take(rect.height.saturating_sub(2) as usize)
        .enumerate()
    {
        app.chatref.targets.push((
            Rect {
                x: rect.x + 1,
                y: rect.y + 1 + row as u16,
                width: rect.width.saturating_sub(2),
                height: 1,
            },
            index,
        ));
    }
}

pub(super) fn clickable(app: &App, x: u16, y: u16) -> bool {
    app.chatref
        .targets
        .iter()
        .any(|(rect, _)| contains(*rect, x, y))
}

pub(super) fn handle_mouse(app: &mut App, mouse: MouseEvent) -> bool {
    if app.chatref.targets.is_empty() {
        return false;
    }
    let hit = app
        .chatref
        .targets
        .iter()
        .find(|(rect, _)| contains(*rect, mouse.column, mouse.row))
        .map(|(_, index)| *index);
    if let Some(index) = hit {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                app.chatref.selection_touched = true;
                app.chatref.pressed = Some(index);
                app.chatref.selected = index;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if app.chatref.pressed.take() == Some(index) {
                    if let View::Rows(rows) = app.chat_view() {
                        if let Some(entry) = rows.get(index).copied() {
                            match entry {
                                Reference::File(i) => app.preview_related_file(i),
                                _ => app.complete_reference(entry),
                            }
                        }
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => app.chatref.pressed = None,
            _ => {}
        }
        return true;
    }
    if matches!(mouse.kind, MouseEventKind::Up(_) | MouseEventKind::Drag(_)) {
        app.chatref.pressed = None;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threads::{ThreadEvent, ThreadLog};

    fn meta(id: &str, title: &str, started: u64) -> ThreadMeta {
        ThreadMeta {
            id: id.into(),
            started,
            title: title.into(),
            cwd: String::new(),
            account: Some("claude-2".into()),
        }
    }

    fn seed(root: &Path, id: &str, title: &str, reply: &str) {
        let log = ThreadLog::open(root, id);
        log.write_meta(&meta(id, title, threads::now_secs()))
            .unwrap();
        // Seed explicit legacy history; production writes now require native source references.
        let events = [
            ThreadEvent::with_text("user", title),
            ThreadEvent::with_text("assistant", reply),
        ];
        let legacy = events
            .iter()
            .map(|ev| serde_json::to_string(ev).unwrap() + "\n")
            .collect::<String>();
        std::fs::write(
            threads::threads_dir(root).join(format!("{id}.jsonl")),
            legacy,
        )
        .unwrap();
    }

    const A: &str = "3f9a1b2c-0000-4000-8000-000000000001";
    const B: &str = "3f9a1b2c-0000-4000-8000-000000000002";
    const C: &str = "c0ffee00-0000-4000-8000-000000000003";

    #[test]
    fn the_token_being_typed_is_found_only_at_a_word_start() {
        assert_eq!(active_query("@"), Some((0, String::new())));
        assert_eq!(active_query("see @3f9"), Some((4, "3f9".into())));
        assert_eq!(active_query("(@3f9"), Some((1, "3f9".into())));
        assert_eq!(active_query("a@b.com"), None);
        assert_eq!(active_query("mail me at a@b"), None);
        assert_eq!(active_query("\\@3f9"), None);
        assert_eq!(
            active_query("see @3f9 and"),
            None,
            "only the word at the caret"
        );
        assert_eq!(active_query("see @3f9.x"), None);
        assert_eq!(active_query("no at sign"), None);
        assert_eq!(active_query("line one\n@abc"), Some((9, "abc".into())));
    }

    #[test]
    fn filtering_matches_id_prefix_or_title_and_ids_stay_unique() {
        let all = vec![
            meta(A, "fix the login redirect", 3),
            meta(B, "other", 2),
            meta(C, "Login page", 1),
        ];
        assert_eq!(filter(&all, "").len(), 3);
        assert_eq!(filter(&all, "c0ff").len(), 1);
        assert_eq!(filter(&all, "login").len(), 2);
        assert_eq!(filter(&all, "LOGIN").len(), 2);
        assert!(filter(&all, "zzz").is_empty());
        assert_eq!(short_id(&all, C), "c0ffee00");
        let s = short_id(&all, A);
        assert!(s.len() > 8 && all.iter().filter(|m| m.id.starts_with(&s)).count() == 1);
        assert_eq!(complete("see @3f", 4, "c0ffee00"), "see @c0ffee00 ");
    }

    #[test]
    fn a_reference_becomes_the_threads_map_within_budget() {
        let d = tempfile::tempdir().unwrap();
        seed(
            d.path(),
            C,
            "fix the login redirect",
            &"long reply ".repeat(200),
        );
        let ex = expand(d.path(), "continue @c0ffee00 please", 600);
        assert!(ex.notices.is_empty(), "{:?}", ex.notices);
        assert_eq!(ex.maps.len(), 1);
        let map = &ex.maps[0].1;
        assert!(map.starts_with("[concordia map] thread c0ffee00-"));
        assert!(map.chars().count() <= 600);
        assert!(map.contains(&format!("more: concordia chat thread read {C}")));
        let request = "continue @c0ffee00 please";
        assert!(ex.sent.starts_with(&format!(
            "{request}\n\n[Referenced chats: source context, not new requests]\n"
        )));
        assert_eq!(ex.sent.matches(request).count(), 1);
        assert!(ex.sent.ends_with(&format!("@c0ffee00\n{map}\n")));
        // A unique short prefix works the same way.
        assert_eq!(expand(d.path(), "@c0ff", 1600).maps.len(), 1);
    }

    #[test]
    fn unknown_and_ambiguous_ids_stay_literal_with_a_reason() {
        let d = tempfile::tempdir().unwrap();
        seed(d.path(), A, "one", "r");
        seed(d.path(), B, "two", "r");
        let ex = expand(d.path(), "see @3f9a and @deadbeef", 1600);
        assert_eq!(ex.sent, "see @3f9a and @deadbeef");
        assert!(ex.maps.is_empty());
        assert_eq!(ex.notices.len(), 2);
        assert!(
            ex.notices[0].contains("matches 2 chats"),
            "{:?}",
            ex.notices
        );
        assert!(
            ex.notices[1].contains("no recorded chat"),
            "{:?}",
            ex.notices
        );
        // An exact id wins over being a prefix of nothing else.
        assert_eq!(expand(d.path(), &format!("@{A}"), 1600).maps.len(), 1);
    }

    #[test]
    fn escapes_emails_and_plain_words_are_never_references() {
        let d = tempfile::tempdir().unwrap();
        seed(d.path(), C, "t", "r");
        for text in [
            "mail a@b.com now",
            "ask @claude about @param",
            "see @c0ffee00.example.com",
        ] {
            let ex = expand(d.path(), text, 1600);
            assert_eq!(ex.sent, text);
            assert!(
                ex.maps.is_empty() && ex.notices.is_empty(),
                "{text}: {:?}",
                ex.notices
            );
        }
        let ex = expand(d.path(), "literal \\@c0ffee00 here", 1600);
        assert_eq!(ex.sent, "literal \\@c0ffee00 here");
        assert!(ex.maps.is_empty() && ex.notices.is_empty());
        assert_eq!(expand(d.path(), "no refs", 1600).sent, "no refs");
    }

    #[test]
    fn at_most_three_references_and_repeats_expand_once() {
        let d = tempfile::tempdir().unwrap();
        let ids: Vec<String> = (1..=5)
            .map(|n| format!("a{n}000000-0000-4000-8000-000000000000"))
            .collect();
        for (n, id) in ids.iter().enumerate() {
            seed(d.path(), id, &format!("chat {n}"), "r");
        }
        let text = "@a1000000 @a2000000 @a3000000 @a4000000 @a1000000";
        let ex = expand(d.path(), text, 1600);
        assert_eq!(ex.maps.len(), 3);
        assert_eq!(ex.notices.len(), 1);
        assert!(ex.notices[0].contains("at most 3"));
        assert!(ex.sent.contains("@a4000000"));
        assert!(
            ex.sent
                .starts_with(&format!("{text}\n\n[Referenced chats:")),
            "{}",
            ex.sent
        );
        assert_eq!(ex.sent.matches(text).count(), 1);
        assert_eq!(ex.sent.matches("[concordia map] thread").count(), 3);
    }

    #[test]
    fn with_no_threads_a_reference_says_how_to_enable_recording() {
        let d = tempfile::tempdir().unwrap();
        let ex = expand(d.path(), "hi @c0ffee00", 1600);
        assert_eq!(ex.sent, "hi @c0ffee00");
        assert!(ex.notices[0].contains("enable [threads] in config.toml"));
    }

    // ---- end to end through the real key handler, submit path and renderer ----

    use ratatui::backend::TestBackend;

    struct Rig {
        app: App,
        term: Terminal<TestBackend>,
        rx: mpsc::UnboundedReceiver<String>,
        tx: mpsc::UnboundedSender<String>,
        dir: tempfile::TempDir,
    }

    impl Rig {
        fn new() -> Rig {
            let dir = tempfile::tempdir().unwrap();
            let app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
            let (tx, rx) = mpsc::unbounded_channel();
            Rig {
                app,
                term: Terminal::new(TestBackend::new(100, 30)).unwrap(),
                rx,
                tx,
                dir,
            }
        }

        fn draw(&mut self) -> String {
            let app = &mut self.app;
            self.term.draw(|f| draw_ui(f, app)).unwrap();
            let buf = self.term.backend().buffer().clone();
            (0..buf.area.height)
                .map(|y| {
                    (0..buf.area.width)
                        .map(|x| buf[(x, y)].symbol().to_string())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        }

        fn key(&mut self, code: KeyCode) {
            crate::tui::handle_key(
                &mut self.app,
                KeyEvent::new(code, KeyModifiers::NONE),
                &self.tx,
            );
            self.draw();
        }

        fn type_str(&mut self, s: &str) {
            for c in s.chars() {
                self.key(KeyCode::Char(c));
            }
        }
    }

    #[test]
    fn typing_at_lists_threads_and_completing_then_sending_expands_the_map() {
        let mut r = Rig::new();
        seed(
            r.dir.path(),
            C,
            "fix the login redirect",
            "The redirect keeps the query string now.",
        );
        seed(r.dir.path(), A, "refactor the parser", "done");
        r.type_str("continue ");
        assert!(
            !r.draw().contains("Tab/Enter completes"),
            "no list before an @"
        );
        r.type_str("@");
        let screen = r.draw();
        assert!(screen.contains("Tab/Enter completes"), "{screen}");
        assert!(
            screen.contains("fix the login redirect") && screen.contains("refactor the parser")
        );
        assert!(screen.contains("claude-2"));
        // Narrow by id prefix, then complete with Tab.
        r.type_str("c0f");
        let screen = r.draw();
        assert!(
            screen.contains("fix the login redirect") && !screen.contains("refactor the parser")
        );
        r.key(KeyCode::Tab);
        assert_eq!(r.app.editor.text(), "continue @c0ffee00 ");
        assert!(
            !r.draw().contains("Tab/Enter completes"),
            "list closes after completing"
        );
        r.type_str("from there");
        r.key(KeyCode::Enter);
        let sent = r.rx.try_recv().expect("the prompt was sent");
        assert_eq!(sent, "continue @c0ffee00 from there");
        assert!(!r.app.messages.iter().any(|m| m.role == MsgRole::Meta));
        // Native-source hydration happens in preparation, then progress adds the dim context.
        let prepared = expand(r.dir.path(), &sent, 1600);
        assert!(
            prepared.sent.starts_with("continue @c0ffee00 from there\n\n[Referenced chats: source context, not new requests]\n@c0ffee00\n[concordia map] thread c0ffee00-"),
            "{}", prepared.sent
        );
        assert!(prepared.sent.contains("keeps the query string"));
        assert_eq!(
            prepared
                .sent
                .matches("continue @c0ffee00 from there")
                .count(),
            1
        );
        r.app.apply_progress(TurnProgress::ChatReferences {
            maps: prepared.maps,
            notices: prepared.notices,
        });
        // The transcript keeps the user's own line and shows the map dimmed under it.
        let user = r
            .app
            .messages
            .iter()
            .position(|m| m.role == MsgRole::User)
            .unwrap();
        assert_eq!(r.app.messages[user].text, "continue @c0ffee00 from there");
        let meta = &r.app.messages[user + 1];
        assert!(
            meta.role == MsgRole::Meta
                && meta.text.starts_with("sent with your prompt (@c0ffee00)")
        );
        assert!(meta.text.contains("[concordia map] thread c0ffee00-"));
        assert_eq!(
            r.app
                .messages
                .iter()
                .filter(|m| m.role == MsgRole::User)
                .count(),
            1
        );
        assert!(
            r.rx.try_recv().is_err(),
            "source context never creates a second request"
        );
    }

    #[test]
    fn arrows_select_enter_completes_and_esc_closes_the_list() {
        let mut r = Rig::new();
        seed(r.dir.path(), C, "first chat", "r");
        seed(r.dir.path(), A, "second chat", "r");
        r.type_str("@");
        r.key(KeyCode::Down);
        assert_eq!(r.app.chatref.selected, 1);
        r.key(KeyCode::Up);
        assert_eq!(r.app.chatref.selected, 0);
        r.key(KeyCode::Down);
        r.key(KeyCode::Enter);
        let t = r.app.editor.text();
        assert!(t.starts_with('@') && t.ends_with(' ') && t.len() > 8, "{t}");
        assert!(r.rx.try_recv().is_err(), "Enter completed, it did not send");
        r.app.editor.clear();
        r.type_str("@");
        r.key(KeyCode::Esc);
        assert!(!r.draw().contains("Tab/Enter completes"));
        r.type_str("c");
        assert!(
            r.draw().contains("Tab/Enter completes"),
            "typing reopens it"
        );
    }

    #[test]
    fn with_no_recorded_chats_the_list_says_how_to_enable_them_once() {
        let mut r = Rig::new();
        r.type_str("@");
        assert!(r.draw().contains("/open"));
        r.type_str("a");
        assert!(r.draw().contains("/open"), "stays while the token is open");
        r.app.editor.clear();
        r.key(KeyCode::Char('x'));
        r.app.editor.clear();
        r.type_str("@");
        assert!(
            r.draw().contains("/open"),
            "file guidance is available whenever the list opens"
        );
    }

    #[test]
    fn email_addresses_and_the_slash_palette_do_not_open_the_list() {
        let mut r = Rig::new();
        seed(r.dir.path(), C, "a chat", "r");
        r.type_str("write to a@");
        assert!(!r.draw().contains("Tab/Enter completes"));
        r.app.editor.clear();
        r.type_str("/use");
        let screen = r.draw();
        assert!(screen.contains("commands") && !screen.contains("Tab/Enter completes"));
    }

    #[test]
    fn an_unknown_reference_is_sent_as_typed_with_a_dim_notice() {
        let mut r = Rig::new();
        seed(r.dir.path(), C, "a chat", "r");
        r.app.editor.set_text("see @deadbeef");
        r.key(KeyCode::Enter);
        assert_eq!(r.rx.try_recv().unwrap(), "see @deadbeef");
        let prepared = expand(r.dir.path(), "see @deadbeef", 1600);
        r.app.apply_progress(TurnProgress::ChatReferences {
            maps: prepared.maps,
            notices: prepared.notices,
        });
        assert!(r
            .app
            .messages
            .iter()
            .any(|m| m.role == MsgRole::System && m.text.contains("@deadbeef: no recorded chat")));
        assert!(r.app.messages.iter().all(|m| m.role != MsgRole::Meta));
    }
    #[test]
    fn file_paths_with_hex_prefixes_are_never_expanded_as_chats_and_notices_are_unique() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path(), C, "chat", "reply");
        let input = "@c0ffee00/src/code.rs @c0ffee00\\src\\code.rs @c0ffee00:12 @c0ffee00.rs";
        let expanded = expand(dir.path(), input, 1600);
        assert_eq!(expanded.sent, input);
        assert!(expanded.maps.is_empty() && expanded.notices.is_empty());
        assert_eq!(
            expand(dir.path(), "@deadbeef @deadbeef", 1600)
                .notices
                .len(),
            1
        );
    }

    fn wait_for_files(r: &mut Rig) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            r.app.request_related_files();
            r.app.on_tick();
            if !r.app.related_files.loading() {
                // A source changed while an earlier scan was in flight: request its successor.
                r.app.request_related_files();
                if !r.app.related_files.loading() {
                    break;
                }
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        r.draw();
    }

    #[test]
    fn current_chat_files_complete_and_preview_without_dispatch_or_draft_loss() {
        let mut r = Rig::new();
        std::fs::create_dir(r.dir.path().join("src")).unwrap();
        std::fs::write(r.dir.path().join("src/code.rs"), "fn main() {}").unwrap();
        std::fs::write(
            r.dir.path().join("notes with spaces.md"),
            "# Notes\n**Preview text**",
        )
        .unwrap();
        r.app
            .push_user("Inspect src/code.rs:1 and [notes](<notes with spaces.md>)".into());
        wait_for_files(&mut r);
        assert_eq!(
            r.app
                .related_files
                .entries
                .iter()
                .filter(|e| e.origin == "chat")
                .count(),
            2
        );
        r.app.editor.set_text("review @src/");
        let screen = r.draw();
        assert!(
            screen.contains("src/code.rs") && screen.contains("Ctrl+P preview"),
            "{screen}"
        );
        crate::tui::handle_key(
            &mut r.app,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
            &r.tx,
        );
        assert_eq!(r.app.editor.text(), "review @src/");
        assert_eq!(r.app.workspace.panes.len(), 1);
        assert!(r.rx.try_recv().is_err());
        r.key(KeyCode::Esc);
        r.key(KeyCode::Tab);
        assert_eq!(r.app.editor.text(), "review @src/code.rs ");
        r.app.editor.set_text("review @notes");
        r.key(KeyCode::Tab);
        assert_eq!(r.app.editor.text(), "review @`notes with spaces.md` ");
        assert!(r.rx.try_recv().is_err());
    }

    #[test]
    fn file_picker_deduplicates_and_new_conversation_clears_its_index() {
        let mut r = Rig::new();
        std::fs::write(r.dir.path().join("file.md"), "# File").unwrap();
        r.app.push_user("file.md file.md:2 [same](file.md)".into());
        r.app.handle_slash("/open");
        wait_for_files(&mut r);
        let picker = r.app.picker.as_ref().unwrap();
        assert_eq!(picker.kind, PickerKind::Files);
        assert_eq!(
            picker.rows.iter().filter(|r| r.label == "file.md").count(),
            1
        );
        r.app.apply_picker_selection(None);
        assert_eq!(r.app.workspace.panes.len(), 1);
        assert!(r.rx.try_recv().is_err());
        r.app.workspace.focused = None;
        r.app.handle_slash("/new");
        assert!(r.app.related_files.entries.is_empty());
        r.app.handle_slash("/open");
        wait_for_files(&mut r);
        assert!(r
            .app
            .picker
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.label == "file.md"));
    }
    #[test]
    fn deleted_suggested_file_is_unavailable_without_opening_a_same_named_file_elsewhere() {
        let mut r = Rig::new();
        std::fs::write(r.dir.path().join("target.md"), "original").unwrap();
        std::fs::create_dir(r.dir.path().join("other")).unwrap();
        std::fs::write(r.dir.path().join("other/target.md"), "unrelated").unwrap();
        r.app.push_card(StreamCard {
            id: "read".into(),
            title: "Read".into(),
            detail: Some("target.md".into()),
            status: CardStatus::Done,
        });
        wait_for_files(&mut r);
        assert_eq!(
            r.app
                .related_files
                .entries
                .iter()
                .filter(|e| e.origin == "chat")
                .count(),
            1
        );
        std::fs::remove_file(r.dir.path().join("target.md")).unwrap();
        r.app.preview_related_file(0);
        assert!(r.app.workspace.panes.is_empty());
        assert!(r.app.flash.as_ref().unwrap().0.contains("unavailable"));
        assert!(r.rx.try_recv().is_err());
    }

    #[test]
    fn adding_file_suggestions_keeps_the_selected_chat_identity() {
        let mut r = Rig::new();
        seed(r.dir.path(), C, "fixture chat", "reply");
        r.app.editor.set_text("@");
        r.draw();
        assert!(
            matches!(r.app.chat_view(), View::Rows(ref rows) if matches!(rows[r.app.chatref.selected], Reference::Chat(_)))
        );
        r.key(KeyCode::Down); // Explicit selection must survive a later discovery result.
        std::fs::write(r.dir.path().join("new.md"), "new file").unwrap();
        r.app.push_user("new.md".into());
        wait_for_files(&mut r);
        assert_eq!(
            r.app
                .related_files
                .entries
                .iter()
                .filter(|e| e.origin == "chat")
                .count(),
            1
        );
        assert!(
            matches!(r.app.chat_view(), View::Rows(ref rows) if matches!(rows[r.app.chatref.selected], Reference::Chat(_)))
        );
        r.key(KeyCode::Tab);
        assert_eq!(r.app.editor.text(), "@c0ffee00 ");
        assert!(r.rx.try_recv().is_err());
    }
    #[test]
    fn current_conversation_is_not_offered_as_its_own_related_chat() {
        let mut r = Rig::new();
        seed(r.dir.path(), C, "current chat", "reply");
        seed(r.dir.path(), A, "other chat", "reply");
        r.app.thread = Some(ThreadLog::open(r.dir.path(), C));
        r.app.editor.set_text("@");
        let screen = r.draw();
        assert!(screen.contains("other chat"));
        assert!(!screen.contains("current chat"));
        assert!(r.app.chatref.list.iter().all(|meta| meta.id != C));
    }
}
