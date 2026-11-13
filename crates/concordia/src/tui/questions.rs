//! Selectable conversational replies. These send messages, never native tool approvals.
//!
//! Single questions require trailing choices; explicit bundles may include descriptions and
//! a trailing progress note. The note row accepts a choice with a note, or a note alone.
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
    /// First visible line of the scrolled list, and rows per page, both set by `draw`.
    scroll: usize,
    page: usize,
}
impl State {
    pub fn waiting(&self) -> bool {
        !self.questions.is_empty()
    }

    /// Whether the visible native panel is waiting for a permission or plan decision.
    ///
    /// Conversational questions use the same panel, so callers must distinguish these native
    /// approvals before publishing attention items.
    pub(super) fn native_approval_waiting(&self) -> bool {
        self.native.as_ref().is_some_and(|request| {
            request.is_active()
                && matches!(
                    request.kind(),
                    crate::provider::interaction::NativeInteraction::Permission { .. }
                        | crate::provider::interaction::NativeInteraction::CodexPermission { .. }
                        | crate::provider::interaction::NativeInteraction::PlanApproval { .. }
                )
        })
    }

    pub(super) fn native_session_id(&self) -> Option<String> {
        self.native
            .as_ref()
            .filter(|request| request.is_active())
            .map(|request| request.envelope().native_session_id.clone())
    }
    pub fn refresh_native(&mut self) -> bool {
        if self
            .native
            .as_ref()
            .is_some_and(|request| !request.is_active())
        {
            self.advance_native();
            return true;
        }
        false
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
        self.scroll = 0;
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
        self.scroll = 0;
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
    let value = if let Some(s) = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("• "))
    {
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
    if let Some(questions) = parse_bundle(text) {
        return questions;
    }
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

/// The relaxed form requires at least two complete, explicit question/choice groups.
/// It never interprets example code or a lone status list as a request for input.
fn parse_bundle(text: &str) -> Option<Vec<Question>> {
    if text.len() > 128 * 1024 {
        return Some(Vec::new());
    }
    let lines = text.lines().collect::<Vec<_>>();
    let mut headings = Vec::new();
    let mut fenced = false;
    for (index, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || line.starts_with([' ', '\t']) || option(line).is_some() {
            continue;
        }
        let prompt = plain(line.trim_start_matches('#').trim());
        let first = prompt
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if prompt.contains('?')
            && prompt.len() <= 4096
            && matches!(
                first.as_str(),
                "may"
                    | "can"
                    | "should"
                    | "would"
                    | "do"
                    | "does"
                    | "which"
                    | "what"
                    | "how"
                    | "where"
                    | "when"
                    | "are"
                    | "is"
                    | "will"
            )
        {
            headings.push((index, prompt));
        }
    }
    if headings.len() < 2 {
        return None;
    }
    if headings.len() > 3 {
        return Some(Vec::new());
    }
    if lines[..headings[0].0]
        .iter()
        .rev()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| {
            let lower = line.trim().to_ascii_lowercase();
            lower.starts_with("example")
                || lower.starts_with("for example")
                || lower.starts_with("sample")
        })
    {
        return Some(Vec::new());
    }
    let mut questions = Vec::new();
    for (position, (start, prompt)) in headings.iter().enumerate() {
        let end = headings
            .get(position + 1)
            .map_or(lines.len(), |(index, _)| *index);
        let mut question = Question {
            prompt: prompt.clone(),
            options: Vec::new(),
        };
        let mut after_choices = false;
        for line in &lines[*start + 1..end] {
            if line.trim().is_empty() {
                continue;
            }
            if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
                return Some(Vec::new());
            }
            if !line.starts_with([' ', '\t']) {
                if let Some(choice) = option(line) {
                    if after_choices || question.options.len() >= 9 {
                        return Some(Vec::new());
                    }
                    question.options.push(choice);
                    continue;
                }
            } else if !after_choices {
                if let Some(last) = question.options.last_mut() {
                    let continuation = option(line).unwrap_or_else(|| plain(line));
                    if last.len() + continuation.len() + 1 > 1200 {
                        return Some(Vec::new());
                    }
                    last.push(' ');
                    last.push_str(&continuation);
                    continue;
                }
            }
            if question.options.is_empty() {
                if question.prompt.len() + line.len() + 1 > 4096 {
                    return Some(Vec::new());
                }
                question.prompt.push(' ');
                question.prompt.push_str(&plain(line));
            } else {
                after_choices = true;
            }
        }
        if question.options.len() < 2 {
            return Some(Vec::new());
        }
        questions.push(question);
    }
    Some(questions)
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
        s.scroll = 0;
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
    // An inferred question has no native response channel. Its reply is a normal
    // FIFO follow-up, never an instruction to interrupt or replace the active task.
    app.queue.items.push(answer);
    app.queue_changed();
    if !app.busy && !app.queue.paused {
        app.queue.dispatch.set_automatic();
    }
    app.flash(if app.queue.paused {
        "Reply queued as a follow-up; the queue remains paused (Ctrl+R resumes)."
    } else if app.busy {
        "Reply queued as a follow-up; the active task continues."
    } else {
        "Reply queued as a normal follow-up."
    });
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
        KeyCode::Home => app.questions.selected = 0,
        KeyCode::End => app.questions.selected = count - 1,
        KeyCode::PageUp => {
            app.questions.selected = app
                .questions
                .selected
                .saturating_sub(app.questions.page.max(1))
        }
        KeyCode::PageDown => {
            app.questions.selected =
                (app.questions.selected + app.questions.page.max(1)).min(count - 1)
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
/// Greedy word wrap to `width` columns. Spaces are kept (a trailing space still moves the caret),
/// words wider than a line are split, and `\n` always starts a new line.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for logical in text.split('\n') {
        let mut tokens: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut in_space = false;
        for c in logical.chars() {
            if c == ' ' {
                in_space = true;
            } else if in_space {
                tokens.push(std::mem::take(&mut current));
                in_space = false;
            }
            current.push(c);
        }
        if !current.is_empty() {
            tokens.push(current);
        }
        let mut line = String::new();
        let mut used = 0usize;
        for token in tokens {
            let word = token.trim_end_matches(' ');
            let word_width = word.width();
            if used > 0 && used + word_width > width {
                out.push(std::mem::take(&mut line));
                used = 0;
            }
            if word_width > width {
                for c in token.chars() {
                    let cw = c.width().unwrap_or(0);
                    if used > 0 && used + cw > width {
                        out.push(std::mem::take(&mut line));
                        used = 0;
                    }
                    line.push(c);
                    used += cw;
                }
                continue;
            }
            line.push_str(word);
            used += word_width;
            let spaces = (token.len() - word.len()).min(width - used);
            line.push_str(&" ".repeat(spaces));
            used += spaces;
        }
        out.push(line);
    }
    out
}

const NOTE_HINT: &str =
    "Add a note: type here, then Enter on a choice (or here to send only the note)";
const NOTE_INDENT: usize = 4;

struct DocRow {
    index: usize,
    indent: usize,
    lines: Vec<String>,
    note: bool,
}
struct Doc {
    prompt: Vec<String>,
    rows: Vec<DocRow>,
}
impl Doc {
    fn height(&self) -> usize {
        self.prompt.len() + self.rows.iter().map(|row| row.lines.len()).sum::<usize>()
    }
}

impl State {
    /// The prompt and every row wrapped to `inner` columns. `None` when nothing is asked.
    fn doc(&self, inner: usize) -> Option<Doc> {
        let q = self.questions.get(self.current)?;
        let title = if self.questions.len() > 1 {
            format!("{} of {} · {}", self.current + 1, self.questions.len(), q.prompt)
        } else {
            q.prompt.clone()
        };
        let multiple = self
            .native
            .as_ref()
            .is_some_and(|request| request.allows_multiple(self.current));
        let mut rows = Vec::new();
        for (index, choice) in q.options.iter().enumerate() {
            let indent = 2 + format!("{}. ", index + 1).len() + if multiple { 4 } else { 0 };
            rows.push(DocRow {
                index,
                indent,
                lines: wrap(choice, inner.saturating_sub(indent)),
                note: false,
            });
        }
        if self.note_allowed() {
            let text = if self.note.is_empty() {
                NOTE_HINT.to_owned()
            } else {
                format!("{}▏", self.note.replace('\n', " ↵ "))
            };
            rows.push(DocRow {
                index: q.options.len(),
                indent: NOTE_INDENT,
                lines: wrap(&text, inner.saturating_sub(NOTE_INDENT)),
                note: true,
            });
        }
        Some(Doc {
            prompt: wrap(&title, inner.saturating_sub(1)),
            rows,
        })
    }
}

pub(super) fn carve(transcript: &mut Rect, app: &mut App) -> Rect {
    let h = if app.questions.waiting() {
        let inner = transcript.width.saturating_sub(2) as usize;
        let wanted = app.questions.doc(inner).map_or(0, |doc| doc.height() + 1);
        let cap = (transcript.height as usize * 60 / 100)
            .min(transcript.height.saturating_sub(3) as usize);
        let h = wanted.min(cap);
        // One row cannot hold a prompt and a hint; leave the transcript alone instead.
        if h < 2 {
            0
        } else {
            h as u16
        }
    } else {
        0
    };
    transcript.height -= h;
    Rect::new(transcript.x, transcript.bottom(), transcript.width, h)
}

/// Pick the first visible line so the selected row `[start, end)` is fully inside the window,
/// leaving a line for each "more" marker. Returns `(offset, more_above, more_below)`.
fn window(total: usize, start: usize, end: usize, view: usize, prev: usize) -> (usize, bool, bool) {
    if total <= view || view == 0 {
        return (0, false, false);
    }
    if view < 3 {
        return (start.min(total - view), false, false);
    }
    let check = |off: usize| {
        let above = off > 0;
        let below = off + view < total;
        let top = off + usize::from(above);
        let bottom = off + view - usize::from(below);
        (start >= top && end <= bottom, above, below)
    };
    let best = (0..=total - view)
        .filter(|off| check(*off).0)
        .min_by_key(|off| off.abs_diff(prev));
    match best {
        Some(off) => {
            let (_, above, below) = check(off);
            (off, above, below)
        }
        // A row taller than the window shows its first line.
        None => {
            let off = start.saturating_sub(1).min(total - view);
            let (_, above, below) = check(off);
            (off, above, below)
        }
    }
}

enum Item {
    Prompt(usize),
    Row(usize, usize),
}

pub(super) fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    app.questions.targets.clear();
    if area.height <= 1 {
        return;
    }
    let inner = area.width.saturating_sub(2) as usize;
    let Some(doc) = app.questions.doc(inner) else {
        return;
    };
    let s = &mut app.questions;
    let body = area.height as usize - 1;
    let pinned = if doc.prompt.len() * 2 <= body {
        doc.prompt.len()
    } else {
        0
    };
    let view = body - pinned;
    let mut items = Vec::new();
    if pinned == 0 {
        items.extend((0..doc.prompt.len()).map(Item::Prompt));
    }
    let mut spans = Vec::new();
    for (ri, row) in doc.rows.iter().enumerate() {
        let first = items.len();
        items.extend((0..row.lines.len()).map(|li| Item::Row(ri, li)));
        spans.push((first, items.len()));
    }
    let selected = s.selected.min(doc.rows.len().saturating_sub(1));
    let (start, end) = spans.get(selected).copied().unwrap_or((0, 0));
    let (off, above, below) = window(items.len(), start, end, view, s.scroll);
    s.scroll = off;
    let top = off + usize::from(above);
    let bottom = (off + view - usize::from(below)).min(items.len());
    s.page = spans
        .iter()
        .filter(|(a, b)| *a >= top && *b <= bottom)
        .count()
        .max(1);

    let title_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let title_line = |text: &str| Line::from(Span::styled(format!(" {text}"), title_style));
    let dim = Style::default().fg(Color::DarkGray);
    for (i, text) in doc.prompt.iter().take(pinned).enumerate() {
        f.render_widget(
            Paragraph::new(title_line(text)),
            Rect::new(area.x, area.y + i as u16, area.width, 1),
        );
    }
    let y0 = area.y + pinned as u16;
    if above {
        f.render_widget(
            Paragraph::new(" ↑ more above").style(dim),
            Rect::new(area.x, y0, area.width, 1),
        );
    }
    if below {
        f.render_widget(
            Paragraph::new(" ↓ more below").style(dim),
            Rect::new(area.x, y0 + view as u16 - 1, area.width, 1),
        );
    }
    let multiple = s
        .native
        .as_ref()
        .is_some_and(|request| request.allows_multiple(s.current));
    let mut visible: Vec<Option<(u16, u16)>> = vec![None; doc.rows.len()];
    for (position, item) in items.iter().enumerate().take(bottom).skip(top) {
        let y = y0 + (position - off) as u16;
        match item {
            Item::Prompt(i) => f.render_widget(
                Paragraph::new(title_line(&doc.prompt[*i])),
                Rect::new(area.x, y, area.width, 1),
            ),
            Item::Row(ri, li) => {
                let row = &doc.rows[*ri];
                let current = s.focused && s.selected == row.index;
                let base = if current {
                    Style::default().fg(Color::White).bg(Color::Indexed(238))
                } else {
                    Style::default().fg(Color::Gray)
                };
                let text = &row.lines[*li];
                let line = if *li > 0 {
                    Line::from(vec![
                        Span::styled(" ".repeat(row.indent), base),
                        Span::styled(text.clone(), base),
                    ])
                } else {
                    let marker = Span::styled(
                        if current { "› " } else { "  " },
                        base.fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    );
                    if row.note {
                        let style = if s.note.is_empty() {
                            base.fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
                        } else {
                            base
                        };
                        Line::from(vec![
                            marker,
                            Span::styled("✎ ", base.fg(Color::Cyan)),
                            Span::styled(text.clone(), style),
                        ])
                    } else {
                        let check = if !multiple {
                            ""
                        } else if s.native_selected.contains(&row.index) {
                            "[x] "
                        } else {
                            "[ ] "
                        };
                        Line::from(vec![
                            marker,
                            Span::styled(format!("{}. ", row.index + 1), base.fg(Color::DarkGray)),
                            Span::styled(check, base.fg(Color::Cyan)),
                            Span::styled(text.clone(), base),
                        ])
                    }
                };
                // Continuation lines of a row keep the row's background but not its marker.
                let rect = Rect::new(area.x + 1, y, area.width.saturating_sub(2), 1);
                f.render_widget(Paragraph::new(line).style(base), rect);
                let slot = &mut visible[*ri];
                *slot = Some(slot.map_or((y, y), |(first, _)| (first, y)));
            }
        }
    }
    for (row, span) in doc.rows.iter().zip(visible) {
        if let Some((first, last)) = span {
            s.targets.push((
                Rect::new(
                    area.x + 1,
                    first,
                    area.width.saturating_sub(2),
                    last - first + 1,
                ),
                row.index,
            ));
        }
    }
    f.render_widget(
        Paragraph::new(if s.native.is_some() && multiple {
            if s.focused {
                " ↑↓ select · Space toggle · Enter confirm · Esc cancel request · F8 focus"
            } else {
                " F8 focus · or click · Space toggle · Enter confirm"
            }
        } else if s.native.is_some() {
            if s.focused {
                " ↑↓ select · Enter choose · Esc cancel request · F8 focus"
            } else {
                " F8 focus, or click an option to answer"
            }
        } else if s.focused {
            " ↑↓ select · type a reply · Shift+Enter newline · Enter send · Esc close · F8 focus"
        } else {
            " Type in the message box, or F8 / click to reply with an option or note"
        })
        .style(dim),
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
                        app.questions.focused = true;
                        app.questions.selected = index;
                        app.workspace.focused = None;
                        app.queue.focused = false;
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
        ).unwrap().unwrap();
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
            .unwrap().unwrap();
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
        let (plan, mut plan_rx) = connection.request("fixture-session", serde_json::json!(10), "cursor/create_plan".into(), serde_json::json!({"toolCallId":"plan-tool","plan":"Inspect the exact parser.\nPreserve unrelated files.","todos":[]})).unwrap().unwrap();
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
        ).unwrap().unwrap();
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
            serde_json::json!({"sessionId":"fixture-session","toolCall":{"title":"Read worker file?"},"options":[{"name":"Allow once","kind":"allow_once","optionId":"worker-allow"},{"name":"Reject","kind":"reject_once","optionId":"worker-reject"}]})).unwrap().unwrap();
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
            serde_json::json!({"sessionId":"fixture-session","options":[{"name":"Allow once","kind":"allow_once","optionId":"worker-allow"}]})).unwrap().unwrap();
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
            .unwrap()
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
            .unwrap()
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
            .unwrap().unwrap();
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
            .unwrap()
            .unwrap();
        let (second, mut second_rx) = connection
            .request(
                "fixture-thread",
                serde_json::json!(2),
                "item/commandExecution/requestApproval".into(),
                params,
            )
            .unwrap()
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
        assert!(!app.queue.dispatch.is_explicit());
        assert!(app.queue.dispatch.is_automatic());

        for paused in [false, true] {
            let mut active = App::new(
                dir.path().join(if paused { "paused" } else { "active" }),
                dir.path().into(),
            );
            active.busy = true;
            active.submitted_prompt = Some("original sustained task".into());
            active.last_sent = Some("original sustained task".into());
            active.native_session_id = Some("original-session".into());
            active.turn_started = Some(Instant::now());
            active.continuation.begin(
                "fixture-account",
                "fixture-model",
                "original sustained task",
            );
            let conversation = active.deleg.d.conversation_id().to_owned();
            let run = active.continuation.run_id().to_owned();
            let started = active.turn_started;
            let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
            active.preparing_cancel = Some(cancelled.clone());
            active.editor.set_text("independent unsent draft");
            active.queue.items.push("earlier queued request".into());
            active.queue.paused = paused;
            active.push_card(StreamCard {
                id: "busy-async-question-fixture".into(),
                title: "functions.request_user_input_async".into(),
                detail: Some(format!("arguments: {payload}")),
                status: CardStatus::Running,
            });
            choose(&mut active, 1);
            assert!(active.busy && !active.cancel_requested);
            assert!(!cancelled.load(std::sync::atomic::Ordering::SeqCst));
            assert_eq!(
                active.submitted_prompt.as_deref(),
                Some("original sustained task")
            );
            assert_eq!(active.last_sent.as_deref(), Some("original sustained task"));
            assert_eq!(
                active.native_session_id.as_deref(),
                Some("original-session")
            );
            assert_eq!(active.turn_started, started);
            assert_eq!(active.deleg.d.conversation_id(), conversation);
            assert_eq!(active.continuation.run_id(), run);
            assert_eq!(
                active
                    .continuation
                    .checkpoint(None, Vec::new(), dir.path().into())
                    .unwrap()
                    .prompt,
                "original sustained task"
            );
            assert_eq!(active.editor.text(), "independent unsent draft");
            assert_eq!(
                active.queue.items,
                [
                    "earlier queued request",
                    "How are you pasting the image?\nCtrl+Shift+V"
                ]
            );
            assert_eq!(active.queue.paused, paused);
            assert!(!active.queue.dispatch.is_explicit() && !active.queue.dispatch.is_automatic());
            let (tx, mut rx) = mpsc::unbounded_channel();
            active.queue_advance(&tx);
            assert!(
                rx.try_recv().is_err(),
                "clarification must not launch a competing turn"
            );
            assert_eq!(active.queue.items.len(), 2);
            active.turn_finished(false, false);
            assert!(
                active.queue.dispatch.is_automatic(),
                "clean completion admits normal FIFO advancement"
            );
            assert_eq!(active.queue.paused, paused);
        }

        let text = "May I use up to 18 real CLI test requests total across the currently selected, signed-in Codex/Claude/Cursor accounts? These would use disposable fixtures to check approvals, images/PDFs, exact-session continuation and a three-arm memory comparison. Seeds and compaction requests count toward the cap; no automatic retries or account switching. Mention any provider/account exclusions in your answer.\n- Allow up to 18 requests total\n- Allow up to 6 requests total\n- No additional live requests\n\nWhat provider usage may those tests consume? I will not buy credits, add paid services or change your billing settings.\n- Subscription allowances only\n- Existing purchased credits are also allowed\nThe missing decisions are the live-test allowance and whether existing purchased credits may be used. I’ve asked both together; the previous one-request allowance is consumed.";
        let parsed = parse(text);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].options.len(), 3);
        assert_eq!(parsed[1].options.len(), 2);
        assert!(parse(&format!("Example questions:\n{text}")).is_empty());
        assert!(parse(&format!("```text\n{text}\n```")).is_empty());
        assert!(parse("May I use the fixture? This is an explanation.\n- Yes\n- No\nA status paragraph follows.").is_empty());
        assert_eq!(parse(&text.replace("\n- ", "\n• ")).len(), 2);

        let mut bundle = App::new(dir.path().into(), dir.path().into());
        bundle.editor.set_text("preserve the unsent draft");
        bundle.questions.offer(text);
        assert!(bundle.questions.waiting() && !bundle.questions.focused);
        assert!(bundle.questions.native.is_none());
        terminal
            .draw(|frame| {
                let mut transcript = frame.area();
                let area = carve(&mut transcript, &mut bundle);
                draw(frame, area, &mut bundle);
            })
            .unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(screen.contains("1 of 2"));
        assert!(screen.contains("Allow up to 18 requests total"));
        let row = bundle.questions.targets[0].0;
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            assert!(mouse(
                &mut bundle,
                &MouseEvent {
                    kind,
                    column: row.x + 2,
                    row: row.y,
                    modifiers: KeyModifiers::NONE
                }
            ));
        }
        assert_eq!(bundle.questions.current, 1);
        assert!(bundle.questions.focused);
        assert!(
            bundle.queue.items.is_empty(),
            "one answer must not send a partial bundle"
        );
        terminal
            .draw(|frame| {
                let mut transcript = frame.area();
                let area = carve(&mut transcript, &mut bundle);
                draw(frame, area, &mut bundle);
            })
            .unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(screen.contains("2 of 2"));
        assert!(screen.contains("Subscription allowances only"));
        assert!(handle_key(
            &mut bundle,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ));
        assert!(!bundle.questions.waiting());
        assert_eq!(bundle.queue.items.len(), 1);
        assert!(bundle.queue.items[0]
            .contains("\nAllow up to 18 requests total\n\nWhat provider usage"));
        assert!(bundle.queue.items[0].ends_with("\nSubscription allowances only"));
        assert!(!bundle.queue.items[0].contains("missing decisions"));
        assert_eq!(bundle.editor.text(), "preserve the unsent draft");
        assert!(bundle.questions.native.is_none());

        let structured = serde_json::json!({"questions":[
            {"title":"First async question? Explanation.","options":["One","Two"]},
            {"title":"Second async question? Explanation.","options":["Three","Four"]}
        ]})
        .to_string();
        let mut state = State::default();
        state.offer_tool(&structured);
        assert_eq!(state.questions.len(), 2);
        assert!(state.native.is_none() && !state.focused);
    }

    #[test]
    fn question_popup_shows_choices_for_each_supported_provider() {
        for provider in [
            ProviderKind::Codex,
            ProviderKind::Claude,
            ProviderKind::Cursor,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut app = App::new(dir.path().join("state"), dir.path().join("workspace"));
            let owner = crate::provider::interaction::InteractionOwner {
                conversation_id: uuid::Uuid::new_v4().to_string(),
                workspace: dir.path().to_path_buf(),
                execution_cwd: dir.path().to_path_buf(),
                run_id: uuid::Uuid::new_v4().to_string(),
                attempt_id: uuid::Uuid::new_v4().to_string(),
                provider,
                account: "synthetic-account".into(),
                home: Some(dir.path().to_path_buf()),
            };
            let mut connection = crate::provider::interaction::InteractionConnection::new(
                Some(owner.clone()),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
            );
            let session = "synthetic-session";
            let (method, params) = match provider {
                ProviderKind::Codex => {
                    connection.initialized_codex("0.166.0");
                    (
                        "item/tool/requestUserInput",
                        serde_json::json!({
                            "threadId":"synthetic-session","turnId":"synthetic-turn","itemId":"synthetic-item",
                            "questions":[{"id":"route","question":"Choose the route","isSecret":false,"isOther":false,
                                "options":[{"label":"Direct","description":"Use direct"},{"label":"Review","description":"Review first"}]}]
                        }),
                    )
                }
                ProviderKind::Claude => {
                    connection
                        .initialized_claude(crate::capacity::claude_capacity::CLI_VERSION)
                        .unwrap();
                    (
                        "claude/ask_question",
                        serde_json::json!({
                            "sessionId":session,"toolCall":{"toolCallId":"ask-1","title":"Answer"},
                            "questions":[{"id":"route","prompt":"Choose the route","options":[{"id":"Direct","label":"Direct"},{"id":"Review","label":"Review"}]}]
                        }),
                    )
                }
                ProviderKind::Cursor | ProviderKind::Grok | ProviderKind::Opencode => {
                    connection.initialized_cursor(&serde_json::json!({"protocolVersion":1}));
                    (
                        "cursor/ask_question",
                        serde_json::json!({
                            "sessionId":session,"toolCallId":"ask-1",
                            "questions":[{"id":"route","prompt":"Choose the route","options":[{"id":"Direct","label":"Direct"},{"id":"Review","label":"Review"}]}]
                        }),
                    )
                }
            };
            let (request, _rx) = connection
                .request(session, serde_json::json!(1), method.into(), params)
                .unwrap()
                .unwrap();
            app.native_owner = Some(owner);
            app.questions.offer_native(request, true);
            let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
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
            assert!(
                screen.contains("Direct"),
                "{provider:?} popup omitted first choice"
            );
            assert!(
                screen.contains("Review"),
                "{provider:?} popup omitted second choice"
            );
        }
    }

    #[test]
    fn popup_never_steals_focus_from_a_draft() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().join("state"), dir.path().join("workspace"));
        app.editor.set_text("unfinished draft");
        app.questions.offer("Which route?\n1. Direct\n2. Review");
        assert!(app.questions.waiting());
        assert!(!app.questions.focused);
        assert_eq!(app.editor.text(), "unfinished draft");
    }

    #[test]
    fn answer_is_sent_natively() {
        for (provider, expected) in [
            (
                ProviderKind::Codex,
                serde_json::json!({"answers":{"route":{"answers":["Direct"]}}}),
            ),
            (
                ProviderKind::Claude,
                serde_json::json!({"outcome":{"outcome":"answered","answers":[{"questionId":"route","selectedOptionIds":["Direct"]}]}}),
            ),
            (
                ProviderKind::Cursor,
                serde_json::json!({"outcome":{"outcome":"answered","answers":[{"questionId":"route","selectedOptionIds":["Direct"]}]}}),
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut app = App::new(dir.path().join("state"), dir.path().join("workspace"));
            let owner = crate::provider::interaction::InteractionOwner {
                conversation_id: uuid::Uuid::new_v4().to_string(),
                workspace: dir.path().into(),
                execution_cwd: dir.path().into(),
                run_id: uuid::Uuid::new_v4().to_string(),
                attempt_id: uuid::Uuid::new_v4().to_string(),
                provider,
                account: "synthetic-account".into(),
                home: Some(dir.path().into()),
            };
            let mut connection = crate::provider::interaction::InteractionConnection::new(
                Some(owner.clone()),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
            );
            let session = "synthetic-session";
            let (method, params) = match provider {
                ProviderKind::Codex => {
                    connection.initialized_codex("0.166.0");
                    (
                        "item/tool/requestUserInput",
                        serde_json::json!({"threadId":session,"turnId":"turn","itemId":"item","questions":[{"id":"route","question":"Route?","options":[{"label":"Direct"},{"label":"Review"}]}]}),
                    )
                }
                ProviderKind::Claude => {
                    connection
                        .initialized_claude(crate::capacity::claude_capacity::CLI_VERSION)
                        .unwrap();
                    (
                        "claude/ask_question",
                        serde_json::json!({"sessionId":session,"toolCall":{"toolCallId":"ask"},"questions":[{"id":"route","prompt":"Route?","options":[{"id":"Direct","label":"Direct"},{"id":"Review","label":"Review"}]}]}),
                    )
                }
                ProviderKind::Cursor | ProviderKind::Grok | ProviderKind::Opencode => {
                    connection.initialized_cursor(&serde_json::json!({"protocolVersion":1}));
                    (
                        "cursor/ask_question",
                        serde_json::json!({"sessionId":session,"toolCallId":"ask","questions":[{"id":"route","prompt":"Route?","options":[{"id":"Direct","label":"Direct"},{"id":"Review","label":"Review"}]}]}),
                    )
                }
            };
            let (request, mut rx) = connection
                .request(session, serde_json::json!(1), method.into(), params)
                .unwrap()
                .unwrap();
            app.busy = true;
            app.submitted_prompt = Some("original native task".into());
            app.editor.set_text("separate native draft");
            app.queue.items.push("queued after native task".into());
            app.queue.paused = true;
            app.native_owner = Some(owner.clone());
            app.questions.offer_native(request, true);
            assert!(handle_key(
                &mut app,
                &KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE)
            ));
            assert_eq!(
                rx.try_recv().unwrap(),
                expected,
                "{provider:?} answer changed transport shape"
            );
            assert!(app.busy && !app.cancel_requested);
            assert_eq!(app.native_owner.as_ref(), Some(&owner));
            assert_eq!(
                app.submitted_prompt.as_deref(),
                Some("original native task")
            );
            assert_eq!(app.editor.text(), "separate native draft");
            assert_eq!(app.queue.items, ["queued after native task"]);
            assert!(app.queue.paused && !app.queue.dispatch.is_explicit());
        }
    }

    #[test]
    fn setting_off_renders_inline() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().join("state"), dir.path().join("workspace"));
        app.editor.set_text("keep this draft");
        app.questions.offer("Which route?\n- Direct\n- Review");
        assert!(app.questions.waiting());
        assert!(!app.questions.focused);
        assert!(app.questions.native.is_none());
        assert_eq!(app.editor.text(), "keep this draft");
    }

    fn popup_app(dir: &tempfile::TempDir) -> App {
        App::new(dir.path().join("state"), dir.path().join("workspace"))
    }
    fn popup_rows(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let mut transcript = frame.area();
                let area = carve(&mut transcript, app);
                draw(frame, area, app);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }
    fn tokens(prefix: &str, count: usize) -> String {
        (0..count)
            .map(|n| format!("{prefix}{n:03}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn press(app: &mut App, code: KeyCode) {
        assert!(handle_key(app, &KeyEvent::new(code, KeyModifiers::NONE)));
    }
    fn column_of(row: &str, needle: &str) -> usize {
        row.find(needle).unwrap_or_else(|| panic!("{needle} missing in {row:?}"))
    }

    #[test]
    fn a_prompt_and_options_longer_than_three_widths_render_in_full_and_click_per_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = popup_app(&dir);
        let reply = format!(
            "Which {} route?\n1. {}\n2. {}",
            tokens("p", 70),
            tokens("a", 70),
            tokens("b", 70)
        );
        app.questions.offer(&reply);
        assert!(app.questions.waiting());
        let rows = popup_rows(&mut app, 100, 60);
        let screen = rows.join("\n");
        for prefix in ["p", "a", "b"] {
            for n in 0..70 {
                let token = format!("{prefix}{n:03}");
                assert_eq!(screen.matches(&token).count(), 1, "{token} was cut or doubled");
            }
        }
        assert!(!screen.contains('…'));
        // Continuation lines sit under the option text, not under the number.
        let first = rows.iter().position(|r| r.contains("a000")).unwrap();
        let text_column = column_of(&rows[first], "a000");
        assert_eq!(text_column, 6);
        let next = rows[first + 1].find(|c: char| c != ' ').unwrap();
        assert_eq!(next, text_column);
        // The click target spans every wrapped row of the option, so the last line chooses it.
        let (rect, index) = app.questions.targets[1];
        assert_eq!(index, 1);
        assert!(rect.height >= 4);
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            assert!(mouse(
                &mut app,
                &MouseEvent {
                    kind,
                    column: rect.x + 2,
                    row: rect.bottom() - 1,
                    modifiers: KeyModifiers::NONE
                }
            ));
        }
        assert!(app.queue.items[0].contains("b069") && !app.queue.items[0].contains("a000"));
    }

    #[test]
    fn a_tall_popup_is_capped_scrolls_and_every_navigation_key_works() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = popup_app(&dir);
        let options = (1..=9)
            .map(|i| format!("{i}. {}", tokens(&format!("o{i}t"), 20)))
            .collect::<Vec<_>>()
            .join("\n");
        app.questions.offer(&format!("Which route?\n{options}"));
        app.questions.focused = true;
        let (width, height) = (60, 20);
        let shown = |rows: &[String], i: usize, last: usize| {
            let screen = rows.join("\n");
            screen.contains(&format!("o{i}t000")) && screen.contains(&format!("o{i}t{last:03}"))
        };
        let rows = popup_rows(&mut app, width, height);
        let popup_height = rows.iter().rposition(|r| r.contains("F8")).unwrap()
            - rows.iter().position(|r| r.contains("Which route?")).unwrap()
            + 1;
        assert!(popup_height <= 12, "popup is capped at 60% of the area");
        assert!(shown(&rows, 1, 19));
        let screen = rows.join("\n");
        assert!(screen.contains("more below") && !screen.contains("more above"));

        press(&mut app, KeyCode::Down);
        assert_eq!(app.questions.selected, 1);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.questions.selected, 0);
        press(&mut app, KeyCode::End);
        assert_eq!(app.questions.selected, 9, "End reaches the note row");
        let rows = popup_rows(&mut app, width, height);
        let screen = rows.join("\n");
        assert!(screen.contains("Add a note"));
        assert!(screen.contains("more above") && !screen.contains("more below"));
        press(&mut app, KeyCode::Up);
        let rows = popup_rows(&mut app, width, height);
        assert!(shown(&rows, 9, 19), "the selected option is fully visible");
        press(&mut app, KeyCode::Home);
        assert_eq!(app.questions.selected, 0);
        let rows = popup_rows(&mut app, width, height);
        assert!(shown(&rows, 1, 19));
        let page = app.questions.page;
        assert!(page >= 1);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.questions.selected, page);
        for _ in 0..6 {
            popup_rows(&mut app, width, height);
            press(&mut app, KeyCode::PageDown);
        }
        assert_eq!(app.questions.selected, 9, "PgDn stops at the last row");
        press(&mut app, KeyCode::PageUp);
        assert!(app.questions.selected < 9);
        press(&mut app, KeyCode::Home);
        for step in 1..=8 {
            press(&mut app, KeyCode::Down);
            let rows = popup_rows(&mut app, width, height);
            assert!(shown(&rows, step + 1, 19), "option {} not fully visible", step + 1);
        }
        press(&mut app, KeyCode::Esc);
        assert!(!app.questions.focused);
        press(&mut app, KeyCode::F(8));
        assert!(app.questions.focused);
        press(&mut app, KeyCode::Enter);
        assert!(app.queue.items[0].contains("o9t000"));
    }

    #[test]
    fn a_long_note_wraps_in_full_with_the_caret_at_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = popup_app(&dir);
        app.questions.offer("Which route?\n1. Direct\n2. Review");
        app.questions.focused = true;
        app.questions.note = tokens("n", 40);
        app.questions.selected = 2;
        let rows = popup_rows(&mut app, 60, 40);
        let screen = rows.join("\n");
        for n in 0..40 {
            assert_eq!(screen.matches(&format!("n{n:03}")).count(), 1);
        }
        let last = rows.iter().position(|r| r.contains("n039")).unwrap();
        assert!(rows[last].contains("n039▏"));
        assert!(!screen.contains('…'));
    }
}
