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
    /// Byte offset of the question line in the source message; the transcript shows the text
    /// before it while the picker shows the question itself.
    start: usize,
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
    visible: bool,
    native: Option<crate::provider::turn::NativeRequest>,
    native_choices: Vec<usize>,
    note: String,
}
impl State {
    pub fn presentation(&self, text: &str) -> Option<String> {
        if !self.visible
            || !self.waiting()
            || self.native.is_some()
            || text.trim() != self.last_source.trim()
        {
            return None;
        }
        let question = self.questions.first()?;
        let prefix = self
            .last_source
            .get(..question.start)
            .unwrap_or("")
            .trim_end();
        Some(if prefix.is_empty() {
            "Choose a reply below.".into()
        } else {
            prefix.into()
        })
    }
    pub fn waiting(&self) -> bool {
        !self.questions.is_empty()
    }
    pub fn clear(&mut self) {
        self.native.take();
        self.native_choices.clear();
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
    pub fn offer(&mut self, text: &str, focus: bool) {
        if self.native.is_some() {
            return;
        }
        if text.len() > 128 * 1024 || self.last_source == text {
            return;
        }
        let questions = parse(text);
        if questions.is_empty() {
            return;
        }
        self.last_source = text.into();
        self.questions = questions;
        self.current = 0;
        self.selected = 0;
        self.answers.clear();
        self.note.clear();
        self.focused = focus;
    }
    /// Rows the cursor can reach: the choices plus, for conversational replies, the note row.
    fn rows(&self) -> usize {
        self.questions
            .get(self.current)
            .map_or(0, |q| q.options.len() + usize::from(self.native.is_none()))
    }
    pub fn offer_native(&mut self, request: crate::provider::turn::NativeRequest, focus: bool) {
        self.clear();
        let params = &request.params;
        self.questions = if request.method == "session/request_permission" {
            vec![Question {
                prompt: params
                    .pointer("/toolCall/title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Allow this tool action?")
                    .to_owned(),
                options: params["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|o| o["name"].as_str().unwrap_or("Permission option").to_owned())
                    .collect(),
                start: 0,
            }]
        } else {
            params["questions"]
                .as_array()
                .into_iter()
                .flatten()
                .take(3)
                .map(|q| Question {
                    prompt: q["prompt"].as_str().unwrap_or("Choose a reply").to_owned(),
                    options: q["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .take(9)
                        .map(|o| o["label"].as_str().unwrap_or("Option").to_owned())
                        .collect(),
                    start: 0,
                })
                .collect()
        };
        if self.questions.is_empty() || self.questions.iter().any(|q| q.options.is_empty()) {
            request.respond(serde_json::json!({"outcome":{"outcome":"cancelled"}}));
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
    let end = line.trim_end().trim_end_matches(['*', '_', ':', ' ']);
    let lower = line.to_ascii_lowercase();
    end.ends_with('?')
        || [
            "choose one",
            "pick one",
            "select an option",
            "which option",
            "your preference",
        ]
        .iter()
        .any(|s| lower.contains(s))
}
fn close(open: &mut Option<Question>, found: &mut Option<Question>, after: &mut usize) {
    if let Some(q) = open.take().filter(|q| q.options.len() >= 2) {
        *found = Some(q);
        *after = 0;
    }
}
fn parse(text: &str) -> Vec<Question> {
    // Tool cards retain their input JSON, including multi-question requests.
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
                        let prompt = q.get("question")?.as_str()?.to_owned();
                        let options = q
                            .get("options")?
                            .as_array()?
                            .iter()
                            .take(9)
                            .filter_map(|o| {
                                o.as_str()
                                    .or_else(|| o.get("label").and_then(|s| s.as_str()))
                                    .map(str::to_owned)
                            })
                            .collect::<Vec<_>>();
                        (options.len() >= 2).then_some(Question {
                            prompt,
                            options,
                            start: 0,
                        })
                    })
                    .collect();
            }
        }
    }
    // Only the last question-and-choices block counts, and only when at most one short line (a
    // sign-off) follows it. Choices in the middle of an answer are content, not a prompt.
    let mut found: Option<Question> = None;
    let mut after = 0usize;
    let mut open: Option<Question> = None;
    let mut gap = 0usize;
    let mut fenced = false;
    let mut offset = 0usize;
    for raw in text.split_inclusive('\n') {
        let start = offset;
        offset += raw.len();
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
            if q.options.is_empty() && gap < 3 && !asks(line) {
                gap += 1; // A short explanation may sit between the question and its choices.
                continue;
            }
            close(&mut open, &mut found, &mut after);
        }
        if asks(line) {
            open = Some(Question {
                prompt: plain(line.trim().trim_matches(['*', '#', ' '])),
                options: Vec::new(),
                start,
            });
            gap = 0;
            continue;
        }
        after += 1;
    }
    close(&mut open, &mut found, &mut after);
    match found {
        Some(q) if after <= 1 => vec![q],
        _ => Vec::new(),
    }
}
/// `index` past the last choice is the note row: the note is sent on its own.
fn choose(app: &mut App, index: usize) {
    let s = &mut app.questions;
    let Some(q) = s.questions.get(s.current) else {
        return;
    };
    let note = s.note.trim().to_owned();
    if s.native.is_none() && index == q.options.len() {
        if note.is_empty() {
            return;
        }
        s.answers.push(format!("{}\n{note}", q.prompt));
    } else {
        let Some(answer) = q.options.get(index) else {
            return;
        };
        s.answers.push(if note.is_empty() {
            format!("{}\n{answer}", q.prompt)
        } else {
            format!("{}\n{answer}\nNote: {note}", q.prompt)
        });
        s.native_choices.push(index);
    }
    s.note.clear();
    if s.current + 1 < s.questions.len() {
        s.current += 1;
        s.selected = 0;
        return;
    }
    if let Some(native) = s.native.take() {
        let result = if native.method == "session/request_permission" {
            serde_json::json!({"outcome":{"outcome":"selected","optionId":native.params["options"][index]["optionId"]}})
        } else {
            let answers:Vec<_>=s.native_choices.iter().enumerate().map(|(q,i)|serde_json::json!({"questionId":native.params["questions"][q]["id"],"selectedOptionIds":[native.params["questions"][q]["options"][*i]["id"]]})).collect();
            serde_json::json!({"outcome":{"outcome":"answered","answers":answers}})
        };
        native.respond(result);
        s.clear();
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
    match key.code {
        KeyCode::Esc => {
            if let Some(native) = app.questions.native.take() {
                native.respond(serde_json::json!({"outcome":{"outcome":"cancelled"}}));
                app.questions.clear();
            } else if !app.questions.note.is_empty() {
                app.questions.note.clear();
            } else {
                app.questions.focused = false;
            }
        }
        KeyCode::Up => app.questions.selected = (app.questions.selected + count - 1) % count,
        KeyCode::Down | KeyCode::Tab => {
            app.questions.selected = (app.questions.selected + 1) % count
        }
        KeyCode::Enter => choose(app, app.questions.selected),
        // Digits pick a choice until a note is started; after that they are part of the note.
        KeyCode::Char(c @ '1'..='9')
            if app.questions.note.is_empty() && (c as usize - '1' as usize) < choices =>
        {
            choose(app, c as usize - '1' as usize)
        }
        KeyCode::Char(c) if !native => app.questions.note.push(c),
        KeyCode::Backspace if !native => {
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
/// A paste while the picker has focus goes into the note, on one line.
pub(super) fn paste(app: &mut App, text: &str) -> bool {
    if !app.questions.focused || app.questions.native.is_some() || !app.questions.waiting() {
        return false;
    }
    let text = text.replace("\r\n", " ").replace(['\r', '\n'], " ");
    app.questions.note.push_str(&text);
    true
}
pub(super) fn carve(transcript: &mut Rect, app: &mut App) -> Rect {
    let h = if app.questions.waiting() {
        (app.questions.rows() + 2).min(transcript.height.saturating_sub(3) as usize) as u16
    } else {
        0
    };
    app.questions.visible = h >= 3;
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
    let rows = q.options.len() + usize::from(s.native.is_none());
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
            Line::from(vec![
                marker,
                Span::styled(number, base.fg(Color::DarkGray)),
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
                (format!("{}▏", tail(&s.note, room.saturating_sub(1))), base)
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
        Paragraph::new(if s.native.is_some() {
            " F8 options · ↑↓ choose · Enter answer · Esc cancel request"
        } else if s.focused {
            " ↑↓ choose · type a note · Enter send · Esc clear note, then back to the message box"
        } else {
            " F8 options · ↑↓ choose · type a note · Enter send"
        })
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
    fn a_short_terminal_retains_the_original_question_when_choices_cannot_fit() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().into(), dir.path().into());
        let text = "Which approach?\n1. Small patch\n2. Full refactor";
        app.questions.offer(text, true);
        carve(&mut Rect::new(0, 0, 80, 4), &mut app);
        assert!(app.questions.presentation(text).is_none());
        carve(&mut Rect::new(0, 0, 80, 20), &mut app);
        assert_eq!(
            app.questions.presentation(text).as_deref(),
            Some("Choose a reply below.")
        );
    }
    #[test]
    fn choices_require_a_question_and_skip_code_and_ordinary_steps() {
        let choices=parse("Which approach?\nA) Small patch\n   Keeps the scope focused.\nB) Full refactor\n   Includes a larger review.");
        assert_eq!(choices[0].options.len(), 2);
        assert!(choices[0].options[0].contains("scope focused"));
        assert!(parse("Steps:\n1. Build\n2. Test").is_empty());
        assert!(parse("```\nWhich?\n1. one\n2. two\n```").is_empty());
        assert_eq!(parse("Which approach do you prefer?\n\n1. **Small patch** (Recommended)\n2. Full refactor")[0].options.len(),2);
        assert_eq!(parse(r#"input: {"questions":[{"question":"Which?","options":[{"label":"One"},{"label":"Two"}]}]}"#)[0].options[0],"One");
    }
}
