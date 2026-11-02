//! Selectable conversational replies. These send messages, never native tool approvals.
use super::*;

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
    visible: bool,
    native: Option<crate::provider::turn::NativeRequest>,
    native_choices: Vec<usize>,
}
impl State {
    pub fn presentation(&self, text: &str) -> Option<String> {
        if !self.visible || !self.waiting() || text.trim() != self.last_source.trim() {
            return None;
        }
        let question = self.questions.first()?;
        let prefix = text
            .split_once(&question.prompt)
            .map(|(before, _)| before.trim_end())
            .unwrap_or("");
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
        self.focused = focus;
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
    let value = value.replace("**", "").trim().to_owned();
    (!value.is_empty() && value.len() <= 1200).then_some(value)
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
                        (options.len() >= 2).then_some(Question { prompt, options })
                    })
                    .collect();
            }
        }
    }
    let mut prompt = String::new();
    let mut options: Vec<String> = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if !options.is_empty()
            && line.starts_with([' ', '\t'])
            && option(line).is_none()
            && !line.trim().is_empty()
        {
            if let Some(last) = options.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
            continue;
        }
        if !prompt.is_empty() {
            if let Some(o) = option(line) {
                options.push(o);
                if options.len() == 9 {
                    break;
                }
                continue;
            }
        }
        let lower = line.to_ascii_lowercase();
        if line.contains('?')
            || [
                "choose one",
                "pick one",
                "select an option",
                "which option",
                "your preference",
            ]
            .iter()
            .any(|s| lower.contains(s))
        {
            if !options.is_empty() {
                break;
            }
            prompt = line.trim().trim_matches('*').to_owned();
            continue;
        }
        if !prompt.is_empty() {
            if let Some(o) = option(line) {
                options.push(o);
                if options.len() == 9 {
                    break;
                }
            } else if !options.is_empty() && !line.trim().is_empty() {
                break;
            }
        }
    }
    if options.len() >= 2 {
        vec![Question { prompt, options }]
    } else {
        Vec::new()
    }
}
fn choose(app: &mut App, index: usize) {
    let s = &mut app.questions;
    let Some(q) = s.questions.get(s.current) else {
        return;
    };
    let Some(answer) = q.options.get(index) else {
        return;
    };
    s.answers.push(format!("{}\n{}", q.prompt, answer));
    s.native_choices.push(index);
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
    let count = app
        .questions
        .questions
        .get(app.questions.current)
        .map_or(0, |q| q.options.len());
    if count == 0 {
        return false;
    }
    match key.code {
        KeyCode::Esc => {
            if let Some(native) = app.questions.native.take() {
                native.respond(serde_json::json!({"outcome":{"outcome":"cancelled"}}));
                app.questions.clear();
            } else {
                app.questions.focused = false;
            }
        }
        KeyCode::Up => app.questions.selected = (app.questions.selected + count - 1) % count,
        KeyCode::Down | KeyCode::Tab => {
            app.questions.selected = (app.questions.selected + 1) % count
        }
        KeyCode::Enter => choose(app, app.questions.selected),
        KeyCode::Char(c @ '1'..='9') if key.modifiers.is_empty() => {
            choose(app, c as usize - '1' as usize)
        }
        KeyCode::Char(_) | KeyCode::Backspace => {
            app.questions.focused = false;
            return false;
        }
        _ => return false,
    }
    true
}
pub(super) fn carve(transcript: &mut Rect, app: &mut App) -> Rect {
    let q = app.questions.questions.get(app.questions.current);
    let h = if app.questions.waiting() {
        q.map_or(0, |q| q.options.len() + 2)
            .min(transcript.height.saturating_sub(3) as usize) as u16
    } else {
        0
    };
    app.questions.visible = h >= 3;
    transcript.height -= h;
    Rect::new(transcript.x, transcript.bottom(), transcript.width, h)
}
pub(super) fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    app.questions.targets.clear();
    let s = &mut app.questions;
    let Some(q) = s.questions.get(s.current).filter(|_| area.height > 1) else {
        return;
    };
    let header = Line::from(vec![Span::styled(
        format!(" {} ", q.prompt),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]);
    f.render_widget(
        Paragraph::new(header),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let count = (area.height as usize).saturating_sub(2);
    let first = s.selected.saturating_sub(count.saturating_sub(1));
    for (row, (index, choice)) in q
        .options
        .iter()
        .enumerate()
        .skip(first)
        .take(count)
        .enumerate()
    {
        let rect = Rect::new(
            area.x + 1,
            area.y + 1 + row as u16,
            area.width.saturating_sub(2),
            1,
        );
        let style = if s.focused && s.selected == index {
            Style::default()
                .fg(Color::White)
                .bg(Color::Indexed(238))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };
        f.render_widget(
            Paragraph::new(format!("[ {} ] {choice}", index + 1)).style(style),
            rect,
        );
        s.targets.push((rect, index));
    }
    f.render_widget(
        Paragraph::new(if s.native.is_some() {
            " F8 options · ↑↓ choose · Enter answer · Esc cancel request"
        } else {
            " F8 options · ↑↓ choose · Enter reply · Esc type instead"
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
                    choose(app, index);
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
