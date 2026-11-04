//! Selectable conversational replies. These send messages, never native tool approvals.
//!
//! A reply is offered only when the assistant's message ends with a question followed by
//! top-level choices. The note row lets the user send a choice with a note, or a note alone.
use super::*;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, Debug)]
struct Question {
    prompt: String,
    options: Vec<String>,
}
#[derive(Default)]
pub(super) struct State {
    questions: Vec<Question>,
    current: usize,
    selected: usize,
    answers: Vec<String>,
    pub focused: bool,
    last_source: String,
    targets: Vec<(Rect, usize)>,
    pressed: Option<usize>,
    native: Option<crate::provider::turn::NativeRequest>,
    pending_native: std::collections::VecDeque<crate::provider::turn::NativeRequest>,
    native_selections: Vec<Vec<usize>>,
    native_selected: Vec<usize>,
    native_texts: Vec<Option<String>>,
    note: String,
}
impl State {
    pub fn waiting(&self) -> bool {
        !self.questions.is_empty()
    }
    pub fn refresh_native(&mut self) {
        if self
            .native
            .as_ref()
            .is_some_and(|request| !request.is_active())
        {
            self.advance_native();
        }
    }
    pub fn clear(&mut self) {
        for request in self.pending_native.drain(..) {
            request.invalidate();
        }
        if let Some(request) = self.native.take() {
            request.invalidate();
        }
        self.native_selections.clear();
        self.native_selected.clear();
        self.native_texts.clear();
        self.questions.clear();
        self.focused = false;
        self.targets.clear();
        self.last_source.clear();
        self.note.clear();
    }
    pub fn cancel_native(&mut self) {
        if self.native.is_some() {
            self.clear();
        }
    }
    fn advance_native(&mut self) {
        let mut pending = std::mem::take(&mut self.pending_native);
        self.clear();
        while let Some(request) = pending.pop_front() {
            if request.is_active() {
                self.offer_native(request, false);
                self.pending_native = pending;
                break;
            }
        }
    }
    pub fn offer(&mut self, text: &str) {
        self.offer_questions(text, false);
    }

    pub fn offer_tool(&mut self, text: &str) {
        self.offer_questions(text, true);
    }

    fn offer_questions(&mut self, text: &str, structured: bool) {
        if self.native.is_some() {
            return;
        }
        if text.len() > 128 * 1024 || self.last_source == text {
            return;
        }
        let questions = if structured {
            parse_tool(text)
        } else {
            parse(text)
        };
        if questions.is_empty() {
            return;
        }
        self.last_source = text.into();
        self.questions = questions;
        self.current = 0;
        self.selected = 0;
        self.answers.clear();
        self.note.clear();
        // Inferred conversational replies must never take keyboard ownership from the composer.
        // F8 or a click explicitly enters the options; native requests keep their own contract.
        self.focused = false;
    }
    /// Rows the cursor can reach: the choices plus, for conversational replies, the note row.
    fn rows(&self) -> usize {
        self.questions
            .get(self.current)
            .map_or(0, |q| q.options.len() + usize::from(self.note_allowed()))
    }
    fn note_allowed(&self) -> bool {
        self.native
            .as_ref()
            .is_none_or(|request| request.allows_text(self.current))
    }
    pub fn offer_native(&mut self, request: crate::provider::turn::NativeRequest, focus: bool) {
        self.refresh_native();
        if self
            .native
            .as_ref()
            .is_some_and(|current| current.is_active())
        {
            if self.pending_native.len() < 7 {
                self.pending_native.push_back(request);
            } else {
                request.cancel();
            }
            return;
        }
        self.clear();
        self.questions = native_questions(&request)
            .into_iter()
            .map(|(prompt, options)| Question { prompt, options })
            .collect();
        if self.questions.is_empty()
            || self
                .questions
                .iter()
                .enumerate()
                .any(|(index, q)| q.options.is_empty() && !request.allows_text(index))
        {
            request.cancel();
            self.clear();
            return;
        }
        self.current = 0;
        self.selected = 0;
        self.answers.clear();
        self.focused = focus;
        self.native = Some(request);
    }
}

impl State {
    fn append_note(&mut self, value: &str) -> bool {
        if self.native.is_some() {
            crate::provider::interaction::append_reply(&mut self.note, value)
        } else {
            self.note.push_str(value);
            true
        }
    }
}

pub(super) fn native_questions(
    request: &crate::provider::turn::NativeRequest,
) -> Vec<(String, Vec<String>)> {
    use crate::provider::interaction::{DecisionScope, NativeInteraction};
    match request.kind() {
        NativeInteraction::Permission { title, options }
        | NativeInteraction::CodexPermission { title, options }
        | NativeInteraction::PlanApproval { title, options } => vec![(
            title.clone(),
            options
                .iter()
                .map(|option| {
                    format!(
                        "{} · {}",
                        option.label,
                        match option.scope {
                            DecisionScope::Once => "once",
                            DecisionScope::Session => "this session",
                            DecisionScope::Always => "always",
                            DecisionScope::Unknown => "scope unknown",
                        }
                    )
                })
                .collect(),
        )],
        NativeInteraction::Questions(questions) | NativeInteraction::CodexQuestions(questions) => {
            questions
                .iter()
                .map(|question| {
                    (
                        question.prompt.clone(),
                        question
                            .options
                            .iter()
                            .map(|option| option.label.clone())
                            .collect(),
                    )
                })
                .collect()
        }
    }
}
fn option(line: &str) -> Option<String> {
    let line = line.trim();
    let value = if let Some(s) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        s
    } else {
        let n = line.find(['.', ')'])?;
        if n == 0
            || n > 2
            || !(line[..n].bytes().all(|b| b.is_ascii_digit())
                || n == 1 && line.as_bytes()[0].is_ascii_alphabetic())
        {
            return None;
        }
        line[n + 1..].strip_prefix(' ')?
    };
    let value = plain(value);
    (!value.is_empty() && value.len() <= 1200).then_some(value)
}
/// Inline Markdown emphasis and code marks read as noise in a one-line choice.
fn plain(text: &str) -> String {
    text.replace("**", "").replace('`', "").trim().to_owned()
}
/// A line that asks the user something. List items are answers, not questions, even when they
/// quote one ("2. **Is it ready?** No.").
fn asks(line: &str) -> bool {
    if option(line).is_some() || line.starts_with([' ', '\t']) {
        return false;
    }
    let line = line.trim_start_matches('#').trim();
    let end = line.trim_end().trim_end_matches(['*', '_', ':', ' ']);
    let lower = end.trim_start_matches(['*', '_']).to_ascii_lowercase();
    end.ends_with('?')
        || [
            "choose one",
            "pick one",
            "select an option",
            "which option",
            "your preference",
        ]
        .iter()
        .any(|s| lower == *s)
}
fn close(open: &mut Option<Question>, found: &mut Option<Question>, after: &mut usize) {
    if let Some(q) = open.take().filter(|q| q.options.len() >= 2) {
        *found = Some(q);
        *after = 0;
    }
}
fn parse_tool(text: &str) -> Vec<Question> {
    // Only explicit question-tool cards interpret JSON. Code examples in assistant prose do not.
    if let Some(start) = text.find('{') {
        if let Some(Ok(value)) = serde_json::Deserializer::from_str(&text[start..])
            .into_iter::<serde_json::Value>()
            .next()
        {
            let value = value.get("arguments").unwrap_or(&value);
            if let Some(questions) = value.get("questions").and_then(|v| v.as_array()) {
                return questions
                    .iter()
                    .take(3)
                    .filter_map(|q| {
                        let prompt = q
                            .get("question")
                            .or_else(|| q.get("title"))?
                            .as_str()?
                            .trim();
                        if prompt.is_empty() || prompt.len() > 4096 {
                            return None;
                        }
                        // Host async prompts use `title` and optional string choices. These
                        // remain conversational replies, never native permission decisions.
                        let options = match q.get("options") {
                            None if q.get("title").is_some() => Vec::new(),
                            Some(options) => {
                                let options = options.as_array()?;
                                if options.is_empty() || options.len() > 9 {
                                    return None;
                                }
                                options
                                    .iter()
                                    .map(|option| {
                                        let label = option.as_str().or_else(|| {
                                            option.get("label").and_then(|label| label.as_str())
                                        })?;
                                        (!label.trim().is_empty() && label.len() <= 1200)
                                            .then(|| label.to_owned())
                                    })
                                    .collect::<Option<Vec<_>>>()?
                            }
                            _ => return None,
                        };
                        (!options.is_empty() || q.get("title").is_some()).then_some(Question {
                            prompt: prompt.to_owned(),
                            options,
                        })
                    })
                    .collect();
            }
        }
    }
    Vec::new()
}

fn parse(text: &str) -> Vec<Question> {
    // Choices must immediately follow their question and finish the reply. Intervening or
    // trailing prose makes this ordinary content, not an inferred request for user input.
    let mut found: Option<Question> = None;
    let mut after = 0usize;
    let mut open: Option<Question> = None;
    let mut fenced = false;
    for raw in text.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\n', '\r']);
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            close(&mut open, &mut found, &mut after);
            after += 1;
            continue;
        }
        if fenced || line.trim().is_empty() {
            continue;
        }
        if let Some(q) = open.as_mut() {
            if !line.starts_with([' ', '\t']) {
                if let Some(choice) = option(line) {
                    q.options.push(choice);
                    if q.options.len() == 9 {
                        close(&mut open, &mut found, &mut after);
                    }
                    continue;
                }
            } else if let Some(last) = q.options.last_mut() {
                // Wrapped or nested text belongs to the choice above it.
                last.push(' ');
                last.push_str(&option(line).unwrap_or_else(|| plain(line)));
                continue;
            }
            close(&mut open, &mut found, &mut after);
        }
        if asks(line) {
            found = None;
            open = Some(Question {
                prompt: plain(line.trim().trim_matches(['*', '#', ' '])),
                options: Vec::new(),
            });
            continue;
        }
        after += 1;
    }
    close(&mut open, &mut found, &mut after);
    match found {
        Some(q) if after == 0 => vec![q],
        _ => Vec::new(),
    }
}
/// `index` past the last choice is the note row: the note is sent on its own.
fn choose(app: &mut App, index: usize) {
    if app.questions.native.as_ref().is_some_and(|request| {
        !app.native_owner
            .as_ref()
            .is_some_and(|owner| request.is_owned_by(owner))
    }) {
        app.questions.cancel_native();
        app.flash("native request is no longer active");
        return;
    }
    let s = &mut app.questions;
    let Some(q) = s.questions.get(s.current) else {
        return;
    };
    let note = s.note.clone();
    let multiple = s
        .native
        .as_ref()
        .is_some_and(|request| request.allows_multiple(s.current));
    let selected = if index >= q.options.len() {
        Vec::new()
    } else if multiple {
        if s.native_selected.is_empty() {
            vec![index]
        } else {
            s.native_selected.clone()
        }
    } else {
        vec![index]
    };
    if s.note_allowed() && index == q.options.len() {
        if note.trim().is_empty() {
            return;
        }
        // A free reply stands on its own; never prepend an inferred question or selected option.
        s.answers.push(note);
        if s.native.is_some() {
            s.native_selections.push(selected);
            s.native_texts.push(Some(s.note.clone()));
        }
    } else {
        let Some(answer) = q.options.get(index) else {
            return;
        };
        s.answers.push(if note.is_empty() {
            format!("{}\n{answer}", q.prompt)
        } else {
            format!("{}\n{answer}\nNote: {note}", q.prompt)
        });
        s.native_selections.push(selected);
        s.native_texts.push(
            (s.native.is_some() && s.note_allowed() && !s.note.trim().is_empty())
                .then(|| s.note.clone()),
        );
    }
    s.note.clear();
    if s.current + 1 < s.questions.len() {
        s.current += 1;
        s.selected = 0;
        s.native_selected.clear();
        return;
    }
    if let Some(native) = s.native.take() {
        let outcome = if let (Some(owner), Ok(result)) = (
            app.native_owner.as_ref(),
            native.choices_with_selections(&s.native_selections, &s.native_texts),
        ) {
            let label = result
                .pointer("/outcome/outcome")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("answered")
                .to_owned();
            native.respond_owned(owner, result).map(|()| label)
        } else {
            native.invalidate();
            Err("Native request is no longer active".into())
        };
        s.advance_native();
        match outcome {
            Ok(label) if native.method == "cursor/create_plan" => {
                let id = format!(
                    "native-plan:{}:{}",
                    native.envelope().connection_id,
                    native.envelope().request_id
                );
                if let Some(mut card) = app
                    .messages
                    .iter()
                    .find_map(|message| message.card.as_ref().filter(|card| card.id == id))
                    .cloned()
                {
                    card.title = format!("Plan {label}");
                    card.status = CardStatus::Done;
                    app.push_card(card);
                }
            }
            Err(error) => app.flash(error),
            _ => {}
        }
        return;
    }
    let answer = s.answers.join("\n\n");
    s.clear();
    app.queue.items.insert(0, answer);
    app.queue_changed();
    let notice = app.force_queued(0);
    app.flash(notice);
}
pub(super) fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    if key.code == KeyCode::F(8) && app.questions.waiting() {
        app.questions.focused = !app.questions.focused;
        app.workspace.focused = None;
        app.queue.focused = false;
        return true;
    }
    if !app.questions.focused
        || app.workspace.focused.is_some()
        || app.queue.focused
        || app.queue.edit.is_some()
        || key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return false;
    }
    let count = app.questions.rows();
    if count == 0 {
        return false;
    }
    let choices = app
        .questions
        .questions
        .get(app.questions.current)
        .map_or(0, |q| q.options.len());
    let native = app.questions.native.is_some();
    let note_allowed = app.questions.note_allowed();
    match key.code {
        KeyCode::Esc => {
            if let Some(native) = app.questions.native.take() {
                native.cancel();
                app.questions.advance_native();
            } else {
                app.questions.focused = false;
            }
        }
        KeyCode::Up => app.questions.selected = (app.questions.selected + count - 1) % count,
        KeyCode::Down | KeyCode::Tab => {
            app.questions.selected = (app.questions.selected + 1) % count
        }
        KeyCode::Enter if note_allowed && key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.questions.append_note("\n");
            app.questions.selected = choices;
        }
        KeyCode::Char(' ')
            if native
                && app
                    .questions
                    .native
                    .as_ref()
                    .is_some_and(|request| request.allows_multiple(app.questions.current))
                && app.questions.selected < choices =>
        {
            if let Some(position) = app
                .questions
                .native_selected
                .iter()
                .position(|choice| *choice == app.questions.selected)
            {
                app.questions.native_selected.remove(position);
            } else {
                app.questions.native_selected.push(app.questions.selected);
            }
        }
        KeyCode::Enter => choose(app, app.questions.selected),
        // Native permission choices retain digit shortcuts. Conversational digits are text.
        KeyCode::Char(c @ '1'..='9')
            if native
                && app.questions.selected < choices
                && (c as usize - '1' as usize) < choices =>
        {
            choose(app, c as usize - '1' as usize)
        }
        KeyCode::Char(c) if note_allowed => {
            if !app.questions.append_note(&c.to_string()) {
                app.flash("Reply exceeds the 4096-byte input limit");
            }
            app.questions.selected = choices;
        }
        KeyCode::Backspace if note_allowed => {
            app.questions.note.pop();
        }
        KeyCode::Char(_) | KeyCode::Backspace => {
            app.questions.focused = false;
            return false;
        }
        _ => return false,
    }
    true
}
/// A deliberate paste into the focused reply panel preserves logical line breaks.
pub(super) fn paste(app: &mut App, text: &str) -> bool {
    if !app.questions.focused || !app.questions.note_allowed() || !app.questions.waiting() {
        return false;
    }
    if !app.questions.append_note(&text::normalize_newlines(text)) {
        app.flash("Reply paste exceeds 4096 bytes or contains controls; reply retained");
        return true;
    }
    app.questions.selected = app.questions.questions[app.questions.current].options.len();
    true
}
pub(super) fn carve(transcript: &mut Rect, app: &mut App) -> Rect {
    let h = if app.questions.waiting() {
        (app.questions.rows() + 2).min(transcript.height.saturating_sub(3) as usize) as u16
    } else {
        0
    };
    transcript.height -= h;
    Rect::new(transcript.x, transcript.bottom(), transcript.width, h)
}
/// Cut `text` to `width` columns, ending with an ellipsis when it does not fit.
fn fit(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}
/// The end of `text` that fits in `width` columns, so the typing position stays visible.
fn tail(text: &str, width: usize) -> String {
    let mut used = 0;
    let mut out: Vec<char> = Vec::new();
    for c in text.chars().rev() {
        let w = c.width().unwrap_or(0);
        if used + w > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out.into_iter().rev().collect()
}
pub(super) fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    app.questions.targets.clear();
    let s = &mut app.questions;
    let Some(q) = s.questions.get(s.current).filter(|_| area.height > 1) else {
        return;
    };
    let width = area.width.saturating_sub(2) as usize;
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {}", fit(&q.prompt, width)),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let rows = q.options.len() + usize::from(s.note_allowed());
    let count = (area.height as usize).saturating_sub(2);
    let first = s.selected.saturating_sub(count.saturating_sub(1));
    for (row, index) in (0..rows).skip(first).take(count).enumerate() {
        let rect = Rect::new(
            area.x + 1,
            area.y + 1 + row as u16,
            area.width.saturating_sub(2),
            1,
        );
        let current = s.focused && s.selected == index;
        let base = if current {
            Style::default().fg(Color::White).bg(Color::Indexed(238))
        } else {
            Style::default().fg(Color::Gray)
        };
        let marker = Span::styled(
            if current { "› " } else { "  " },
            base.fg(Color::Cyan).add_modifier(Modifier::BOLD),
        );
        let line = if let Some(choice) = q.options.get(index) {
            let number = format!("{}. ", index + 1);
            let room = width.saturating_sub(2 + number.len());
            let marked = s.native_selected.contains(&index);
            Line::from(vec![
                marker,
                Span::styled(number, base.fg(Color::DarkGray)),
                Span::styled(
                    if s.native.is_some()
                        && s.native
                            .as_ref()
                            .is_some_and(|request| request.allows_multiple(s.current))
                    {
                        if marked {
                            "[x] "
                        } else {
                            "[ ] "
                        }
                    } else {
                        ""
                    },
                    base.fg(Color::Cyan),
                ),
                Span::styled(fit(choice, room), base),
            ])
        } else {
            let room = width.saturating_sub(4);
            let (text, style) = if s.note.is_empty() {
                (
                    fit("Add a note: type here, then Enter on a choice (or here to send only the note)", room),
                    base.fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                )
            } else {
                (
                    format!(
                        "{}▏",
                        tail(&s.note.replace('\n', " ↵ "), room.saturating_sub(1))
                    ),
                    base,
                )
            };
            Line::from(vec![
                marker,
                Span::styled("✎ ", base.fg(Color::Cyan)),
                Span::styled(text, style),
            ])
        };
        f.render_widget(Paragraph::new(line).style(base), rect);
        s.targets.push((rect, index));
    }
    f.render_widget(
        Paragraph::new(
            if s.native.is_some()
                && s.native
                    .as_ref()
                    .is_some_and(|request| request.allows_multiple(s.current))
            {
                " F8 options · ↑↓ choose · Space toggle · Enter confirm · Esc cancel request"
            } else if s.native.is_some() {
                " F8 options · ↑↓ choose · Enter answer · Esc cancel request"
            } else if s.focused {
                " ↑↓ choose · type a reply · Shift+Enter newline · Enter send · Esc message box"
            } else {
                " Type in the message box, or F8 / click to reply with an option or note"
            },
        )
        .style(Style::default().fg(Color::DarkGray)),
        Rect::new(area.x, area.bottom() - 1, area.width, 1),
    );
}
pub(super) fn mouse(app: &mut App, m: &MouseEvent) -> bool {
    let hit = app
        .questions
        .targets
        .iter()
        .find(|(r, _)| contains(*r, m.column, m.row))
        .map(|(_, i)| *i);
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) if hit.is_some() => {
            app.questions.pressed = hit;
            true
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if let Some(index) = app.questions.pressed.take() {
                if hit == Some(index) {
                    let note_row = app
                        .questions
                        .questions
                        .get(app.questions.current)
                        .is_some_and(|q| index == q.options.len());
                    if note_row && app.questions.note.trim().is_empty() {
                        // Clicking the empty note row starts a note rather than sending nothing.
                        app.questions.focused = true;
                        app.questions.selected = index;
                    } else if app
                        .questions
                        .native
                        .as_ref()
                        .is_some_and(|request| request.allows_multiple(app.questions.current))
                        && !note_row
                    {
                        if let Some(position) = app
                            .questions
                            .native_selected
                            .iter()
                            .position(|choice| *choice == index)
                        {
                            app.questions.native_selected.remove(position);
                        } else {
                            app.questions.native_selected.push(index);
                        }
                        app.questions.focused = true;
                        app.questions.selected = index;
                    } else {
                        choose(app, index);
                    }
                }
                true
            } else {
                false
            }
        }
        _ => false,
    }
}
pub(super) fn clickable(app: &App, x: u16, y: u16) -> bool {
    app.questions
        .targets
        .iter()
        .any(|(r, _)| contains(*r, x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_updates_and_example_json_do_not_become_questions() {
        let status = "I also found an input edge case: typing a number at the start of a note could select an option. I’m fixing that and checking both note-only and choice-with-note replies before building.\n\nInstalled the new build. Reopen the app.\n\n- `/use` lets you choose an account.\n- Includes note and session fixes.\n- Notes can start with numbers.";
        assert!(parse(status).is_empty());
        assert!(parse("Which approach?\nHere is what shipped.\n- Notes\n- Providers").is_empty());
        assert!(parse("Example payload:\n```json\n{\"questions\":[{\"question\":\"Which?\",\"options\":[\"One\",\"Two\"]}]}\n```").is_empty());
        assert_eq!(
            parse("Choose one:\n- Small patch\n- Full refactor").len(),
            1
        );
        assert!(
            parse("Which approach?\n- Small patch\n- Full refactor\nWhat should we do next?")
                .is_empty()
        );
        assert!(parse(
            "Which approach?\n- Small patch\n- Full refactor\nThese are the changes that shipped."
        )
        .is_empty());
    }

    #[test]
    fn native_permission_digit_shortcuts_still_answer_the_exact_request() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().into(), dir.path().into());
        let owner = crate::provider::interaction::InteractionOwner {
            conversation_id: app.deleg.d.conversation_id().to_owned(),
            workspace: dir.path().into(),
            execution_cwd: dir.path().into(),
            run_id: uuid::Uuid::new_v4().to_string(),
            attempt_id: uuid::Uuid::new_v4().to_string(),
            provider: ProviderKind::Cursor,
            account: "fixture".into(),
            home: Some(dir.path().into()),
        };
        app.native_owner = Some(owner.clone());
        let mut connection = crate::provider::interaction::InteractionConnection::new(
            Some(owner.clone()),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let (request, mut rx) = connection.request(
            "fixture-session", serde_json::json!(1), "session/request_permission".into(),
            serde_json::json!({"sessionId":"fixture-session","toolCall":{"title":"Read synthetic file?"},"options":[{"name":"Allow once","kind":"allow_once","optionId":"allow-once"},{"name":"Reject","kind":"reject_once","optionId":"reject"}]}),
        ).unwrap();
        app.questions.offer_native(request, true);
        assert!(handle_key(
            &mut app,
            &KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE)
        ));
        assert_eq!(
            rx.try_recv().unwrap(),
            serde_json::json!({"outcome":{"outcome":"selected","optionId":"allow-once"}})
        );
        assert!(!app.questions.waiting());
        assert!(app.queue.items.is_empty());

        // Claude AskUserQuestion uses the same foreground broker, including a real multi-select
        // toggle sequence. The draft must remain untouched while native input is answered.
        let claude_owner = crate::provider::interaction::InteractionOwner {
            conversation_id: app.deleg.d.conversation_id().to_owned(),
            workspace: dir.path().into(),
            execution_cwd: dir.path().into(),
            run_id: uuid::Uuid::new_v4().to_string(),
            attempt_id: uuid::Uuid::new_v4().to_string(),
            provider: ProviderKind::Claude,
            account: "claude-fixture".into(),
            home: Some(dir.path().into()),
        };
        let mut claude_connection = crate::provider::interaction::InteractionConnection::new(
            Some(claude_owner.clone()),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        claude_connection
            .initialized_claude(crate::capacity::claude_capacity::CLI_VERSION)
            .unwrap();
        let (question, mut question_rx) = claude_connection
            .request(
                "fixture-session",
                serde_json::json!(2),
                "claude/ask_question".into(),
                serde_json::json!({
                    "sessionId":"fixture-session",
                    "toolCall":{"title":"Answer questions","toolCallId":"ask-1","rawInput":{}},
                    "questions":[
                        {"id":"route","prompt":"Choose a route","options":[{"id":"Direct","label":"Direct"},{"id":"Review","label":"Review"}],"allowMultiple":false},
                        {"id":"colors","prompt":"Choose colors","options":[{"id":"Red","label":"Red"},{"id":"Blue","label":"Blue"}],"allowMultiple":true}
                    ]
                }),
            )
            .unwrap();
        app.native_owner = Some(claude_owner);
        app.editor.set_text("preserve foreground draft");
        app.questions.offer_native(question, true);
        handle_key(
            &mut app,
            &KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        );
        handle_key(
            &mut app,
            &KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        handle_key(&mut app, &KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(
            &mut app,
            &KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        handle_key(&mut app, &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            question_rx.try_recv().unwrap(),
            serde_json::json!({"outcome":{"outcome":"answered","answers":[
                {"questionId":"route","selectedOptionIds":["Direct"]},
                {"questionId":"colors","selectedOptionIds":["Red","Blue"]}
            ]}})
        );
        assert_eq!(app.editor.text(), "preserve foreground draft");
        app.native_owner = Some(owner.clone());

        connection.initialized_cursor(&serde_json::json!({"protocolVersion":1}));
        let (plan, mut plan_rx) = connection.request("fixture-session", serde_json::json!(10), "cursor/create_plan".into(), serde_json::json!({"toolCallId":"plan-tool","plan":"Inspect the exact parser.\nPreserve unrelated files.","todos":[]})).unwrap();
        app.busy = true;
        app.apply_progress(TurnProgress::Interaction(Box::new(plan)));
        assert!(app.activity.detail.is_some());
        assert!(!app.questions.focused);
        assert!(app
            .messages
            .last()
            .unwrap()
            .card
            .as_ref()
            .unwrap()
            .detail
            .as_deref()
            .unwrap()
            .contains("\nPreserve unrelated files."));
        app.activity.detail = None;
        handle_key(&mut app, &KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
        handle_key(
            &mut app,
            &KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
        );
        assert_eq!(
            plan_rx.try_recv().unwrap(),
            serde_json::json!({"outcome":{"outcome":"rejected"}})
        );
        assert_eq!(
            app.messages.last().unwrap().card.as_ref().unwrap().title,
            "Plan rejected"
        );

        // A queued request from the prior attempt must never acquire the new attempt's UI.
        let (stale, mut stale_rx) = connection.request(
            "fixture-session", serde_json::json!(2), "session/request_permission".into(),
            serde_json::json!({"sessionId":"fixture-session","options":[{"name":"Allow once","kind":"allow_once","optionId":"allow-once"}]}),
        ).unwrap();
        app.busy = true;
        app.native_owner.as_mut().unwrap().attempt_id = uuid::Uuid::new_v4().to_string();
        app.apply_progress(TurnProgress::Interaction(Box::new(stale)));
        assert!(!app.questions.waiting());
        assert!(matches!(
            stale_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        ));
        let main_owner = app.native_owner.clone();
        app.editor.set_text("keep this worker-review draft");
        let worker = app.deleg.d.push_test_job(
            "fixture-worker",
            ProviderKind::Cursor,
            crate::delegate::WorkerState::Thinking,
            "synthetic worker",
            0,
        );
        let worker_owner = app.deleg.d.test_native_owner(worker, dir.path());
        let mut worker_connection = crate::provider::interaction::InteractionConnection::new(
            Some(worker_owner),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let (worker_request, mut worker_rx) = worker_connection.request("fixture-session", serde_json::json!(1), "session/request_permission".into(),
            serde_json::json!({"sessionId":"fixture-session","toolCall":{"title":"Read worker file?"},"options":[{"name":"Allow once","kind":"allow_once","optionId":"worker-allow"},{"name":"Reject","kind":"reject_once","optionId":"worker-reject"}]})).unwrap();
        app.deleg.d.test_native_input(worker, worker_request);
        assert_eq!(
            app.deleg.d.get(worker).unwrap().state,
            crate::delegate::WorkerState::WaitingInput
        );
        worker_input::open(&mut app, worker);
        worker_input::key(
            &mut app,
            &KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        );
        assert!(matches!(
            worker_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        app.paste_text("must not enter the draft");
        worker_input::key(&mut app, &KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.deleg.d.get(worker).unwrap().native_inputs().len(), 1);
        worker_input::open(&mut app, worker);
        worker_input::key(&mut app, &KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
        worker_input::key(
            &mut app,
            &KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        );
        assert_eq!(
            worker_rx.try_recv().unwrap(),
            serde_json::json!({"outcome":{"outcome":"selected","optionId":"worker-allow"}})
        );
        assert_eq!(app.native_owner, main_owner);
        assert_eq!(app.editor.text(), "keep this worker-review draft");
        let (cancelled, mut cancelled_rx) = worker_connection.request("fixture-session", serde_json::json!(2), "session/request_permission".into(),
            serde_json::json!({"sessionId":"fixture-session","options":[{"name":"Allow once","kind":"allow_once","optionId":"worker-allow"}]})).unwrap();
        app.deleg.d.test_native_input(worker, cancelled);
        worker_input::open(&mut app, worker);
        app.deleg.d.cancel(worker, Instant::now());
        worker_input::key(&mut app, &KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
        worker_input::key(
            &mut app,
            &KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        );
        assert!(matches!(
            cancelled_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        ));
        assert_eq!(app.native_owner, main_owner);
        native_text_proof(&mut app, dir.path());
    }

    fn native_text_proof(app: &mut App, dir: &Path) {
        use serde_json::json;
        let owner = crate::provider::interaction::InteractionOwner {
            provider: ProviderKind::Codex,
            ..app.native_owner.clone().unwrap()
        };
        app.native_owner = Some(owner.clone());
        app.editor.set_text("preserve native reply draft");
        let mut connection = crate::provider::interaction::InteractionConnection::new(
            Some(owner.clone()),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        connection.initialized_codex("0.166.0");
        let params = json!({"threadId":"text-thread","turnId":"text-turn","itemId":"text-item","questions":[{"id":"text","header":"Text","question":"Describe the route","isSecret":false,"isOther":false,"options":null},{"id":"mixed","header":"Mixed","question":"Choose or comment","isSecret":false,"isOther":true,"options":[{"label":"One","description":"First"},{"label":"Two","description":"Second"}]},{"id":"fixed","header":"Fixed","question":"Confirm choice","isSecret":false,"isOther":false,"options":[{"label":"Fixed","description":"Exact"}]}]});
        let (request, mut rx) = connection
            .request(
                "text-thread",
                json!(21),
                "item/tool/requestUserInput".into(),
                params.clone(),
            )
            .unwrap();
        assert!(request
            .choices_with_text(&[0, 0, 0], &[Some("x".repeat(4097)), None, None])
            .is_err());
        app.questions.offer_native(request, false);
        assert!(!paste(app, "not focused"));
        handle_key(app, &KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
        assert!(paste(app, "first answer"));
        handle_key(app, &KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        handle_key(app, &KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        handle_key(app, &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(paste(app, "comment"));
        handle_key(app, &KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        handle_key(app, &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!paste(app, "fixed choice has no text field"));
        handle_key(app, &KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
        assert_eq!(
            rx.try_recv().unwrap(),
            json!({"answers":{"text":{"answers":["user_note: first answer\n2"]},"mixed":{"answers":["Two","user_note: comment"]},"fixed":{"answers":["Fixed"]}}})
        );
        assert_eq!(app.editor.text(), "preserve native reply draft");
        assert!(app.queue.items.is_empty());
        let mut secret = params.clone();
        secret["questions"][0]["isSecret"] = json!(true);
        assert!(connection
            .request(
                "text-thread",
                json!(22),
                "item/tool/requestUserInput".into(),
                secret
            )
            .is_err());
        let mut malformed = params.clone();
        malformed["questions"][0]["isSecret"] = json!("true");
        assert!(connection
            .request(
                "text-thread",
                json!(23),
                "item/tool/requestUserInput".into(),
                malformed
            )
            .is_err());
        let worker = app.deleg.d.push_test_job(
            "text-worker",
            ProviderKind::Codex,
            crate::delegate::WorkerState::Thinking,
            "synthetic native text worker",
            0,
        );
        let worker_owner = app.deleg.d.test_native_owner(worker, dir);
        let mut worker_connection = crate::provider::interaction::InteractionConnection::new(
            Some(worker_owner),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        worker_connection.initialized_codex("0.166.0");
        let mut worker_params = params;
        worker_params["threadId"] = json!("fixture-session");
        worker_params["questions"] = json!([{"id":"text","header":"Worker","question":"Describe result","isSecret":false,"options":null}]);
        let (request, mut worker_rx) = worker_connection
            .request(
                "fixture-session",
                json!(31),
                "item/tool/requestUserInput".into(),
                worker_params,
            )
            .unwrap();
        app.deleg.d.test_native_input(worker, request);
        worker_input::open(app, worker);
        assert!(worker_input::active(app));
        app.paste_text("unarmed paste");
        assert_eq!(app.editor.text(), "preserve native reply draft");
        worker_input::key(app, &KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
        app.paste_text(&"x".repeat(4097));
        assert!(worker_rx.try_recv().is_err());
        app.paste_text("worker\nanswer");
        worker_input::key(app, &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            worker_rx.try_recv().unwrap(),
            json!({"answers":{"text":{"answers":["user_note: worker\nanswer"]}}})
        );
        assert_eq!(app.native_owner, Some(owner));
        assert_eq!(app.editor.text(), "preserve native reply draft");

        let claude_worker = app.deleg.d.push_test_job(
            "claude-question-worker",
            ProviderKind::Claude,
            crate::delegate::WorkerState::Thinking,
            "synthetic Claude question worker",
            0,
        );
        let claude_worker_owner = app.deleg.d.test_native_owner(claude_worker, dir);
        let mut claude_worker_connection = crate::provider::interaction::InteractionConnection::new(
            Some(claude_worker_owner),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        claude_worker_connection
            .initialized_claude(crate::capacity::claude_capacity::CLI_VERSION)
            .unwrap();
        let (claude_request, mut claude_rx) = claude_worker_connection
            .request(
                "fixture-session",
                json!(32),
                "claude/ask_question".into(),
                json!({
                    "sessionId":"fixture-session",
                    "toolCall":{"title":"Worker questions","toolCallId":"ask-worker","rawInput":{}},
                    "questions":[{"id":"colors","prompt":"Choose colors","options":[{"id":"Red","label":"Red"},{"id":"Blue","label":"Blue"}],"allowMultiple":true}]
                }),
            )
            .unwrap();
        app.deleg.d.test_native_input(claude_worker, claude_request);
        worker_input::open(app, claude_worker);
        worker_input::key(app, &KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
        worker_input::key(app, &KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        worker_input::key(app, &KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        worker_input::key(app, &KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        worker_input::key(app, &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            claude_rx.try_recv().unwrap(),
            json!({"outcome":{"outcome":"answered","answers":[{"questionId":"colors","selectedOptionIds":["Red","Blue"]}]}})
        );
        assert_eq!(app.editor.text(), "preserve native reply draft");
    }

    #[test]
    fn offering_choices_never_takes_focus_and_small_layouts_do_not_underflow() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().into(), dir.path().into());
        let text = "Which approach?\n1. Small patch\n2. Full refactor";
        app.questions.offer(text);
        assert!(!app.questions.focused);
        for height in 0..20 {
            let mut transcript = Rect::new(0, 0, 80, height);
            let area = carve(&mut transcript, &mut app);
            assert_eq!(area.height + transcript.height, height);
        }
        let mut connection = crate::provider::interaction::InteractionConnection::new(
            None,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        connection.initialized_codex("0.166.0");
        let params = serde_json::json!({"threadId":"fixture-thread","turnId":"fixture-turn","itemId":"fixture-item","availableDecisions":["accept","decline","cancel"]});
        let (first, mut first_rx) = connection
            .request(
                "fixture-thread",
                serde_json::json!(1),
                "item/commandExecution/requestApproval".into(),
                params.clone(),
            )
            .unwrap();
        let (second, mut second_rx) = connection
            .request(
                "fixture-thread",
                serde_json::json!(2),
                "item/commandExecution/requestApproval".into(),
                params,
            )
            .unwrap();
        app.questions.offer_native(first.clone(), false);
        app.questions.offer_native(second.clone(), false);
        assert_eq!(
            app.questions.native.as_ref().unwrap().envelope().request_id,
            serde_json::json!(1)
        );
        assert_eq!(app.questions.pending_native.len(), 1);
        assert!(first_rx.try_recv().is_err() && second_rx.try_recv().is_err());
        first.resolve_native();
        app.questions.refresh_native();
        assert_eq!(
            app.questions.native.as_ref().unwrap().envelope().request_id,
            serde_json::json!(2)
        );
        assert!(!app.questions.focused && second.is_active());
        app.questions.clear();
        assert!(!second.is_active());
    }
    #[test]
    fn choices_require_a_question_and_skip_code_and_ordinary_steps() {
        let choices=parse("Which approach?\nA) Small patch\n   Keeps the scope focused.\nB) Full refactor\n   Includes a larger review.");
        assert_eq!(choices[0].options.len(), 2);
        assert!(choices[0].options[0].contains("scope focused"));
        assert!(parse("Steps:\n1. Build\n2. Test").is_empty());
        assert!(parse("```\nWhich?\n1. one\n2. two\n```").is_empty());
        assert_eq!(parse("Which approach do you prefer?\n\n1. **Small patch** (Recommended)\n2. Full refactor")[0].options.len(),2);
        assert_eq!(parse_tool(r#"input: {"questions":[{"question":"Which?","options":[{"label":"One"},{"label":"Two"}]}]}"#)[0].options[0],"One");
        let payload = serde_json::json!({"questions":[{"title":"How are you pasting the image?","options":["Ctrl+V","Ctrl+Shift+V","Drag and drop"]}]}).to_string();
        assert_eq!(
            parse_tool(&payload)[0].prompt,
            "How are you pasting the image?"
        );
        assert_eq!(parse_tool(&payload)[0].options.len(), 3);
        assert_eq!(
            parse_tool(r#"{"questions":[{"title":"Describe the result"}]}"#)[0]
                .options
                .len(),
            0
        );
        assert!(
            parse_tool(r#"{"questions":[{"title":"Choose","options":["One",17]}]}"#).is_empty()
        );
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().into(), dir.path().into());
        app.editor.set_text("keep the current draft");
        app.push_card(StreamCard {
            id: "async-question-fixture".into(),
            title: "functions.request_user_input_async".into(),
            detail: Some(format!("arguments: {payload}")),
            status: CardStatus::Done,
        });
        assert!(app.questions.waiting());
        assert!(app.questions.native.is_none());
        assert!(!app.questions.focused);
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| {
                let mut transcript = frame.area();
                let area = carve(&mut transcript, &mut app);
                draw(frame, area, &mut app);
            })
            .unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(screen.contains("How are you pasting the image?"));
        assert!(screen.contains("Ctrl+Shift+V"));
        assert!(screen.contains("Drag and drop"));
        choose(&mut app, 1);
        assert_eq!(
            app.queue.items[0],
            "How are you pasting the image?\nCtrl+Shift+V"
        );
        assert_eq!(app.editor.text(), "keep the current draft");
        assert!(app.questions.native.is_none());
    }
}
