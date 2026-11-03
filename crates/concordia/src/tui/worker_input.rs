//! Explicit owner-bound worker review. Opening/closing this view never switches the main run.
use super::*;

pub(super) struct Review {
    job: u32,
    request: Arc<crate::provider::turn::NativeRequest>,
    answers: Vec<usize>,
    selected: usize,
    armed: bool,
    follow_selection: bool,
    scroll: usize,
    max_scroll: usize,
    targets: Vec<(Rect, usize)>,
    pressed: Option<usize>,
}

pub(super) fn active(app: &App) -> bool {
    app.deleg.input.is_some()
}

pub(super) fn open(app: &mut App, id: u32) {
    let Some(request) = app
        .deleg
        .d
        .get(id)
        .and_then(|job| job.native_inputs().into_iter().next())
    else {
        app.flash("No live native request for that worker attempt");
        return;
    };
    app.deleg.input = Some(Review {
        job: id,
        request,
        answers: Vec::new(),
        selected: 0,
        armed: false,
        follow_selection: false,
        scroll: 0,
        max_scroll: 0,
        targets: Vec::new(),
        pressed: None,
    });
}

fn choose(app: &mut App, option: usize) {
    let Some(mut review) = app.deleg.input.take() else {
        return;
    };
    let questions = questions::native_questions(&review.request);
    if questions
        .get(review.answers.len())
        .is_none_or(|(_, options)| option >= options.len())
    {
        app.deleg.input = Some(review);
        return;
    }
    // Validate live ownership at every step, including before moving between questions.
    if !app.deleg.d.get(review.job).is_some_and(|job| {
        job.native_inputs()
            .iter()
            .any(|request| request.envelope() == review.request.envelope())
    }) {
        app.flash("Native worker request expired; inspect the current attempt");
        return;
    }
    review.answers.push(option);
    review.pressed = None;
    review.targets.clear();
    if review.answers.len() < questions.len() {
        review.selected = 0;
        review.scroll = 0;
        review.armed = false;
        app.deleg.input = Some(review);
        return;
    }
    let result = review
        .request
        .single_choices(&review.answers)
        .and_then(|answer| {
            app.deleg
                .d
                .answer_native_input(review.job, review.request.envelope(), answer)
        });
    app.flash(match result {
        Ok(()) => "Native decision sent to the selected worker attempt".into(),
        Err(error) => error,
    });
}

pub(super) fn key(app: &mut App, key: &KeyEvent) -> bool {
    if app.deleg.input.is_none()
        && key.code == KeyCode::F(8)
        && app.picker.is_none()
        && !app.project_search.active()
    {
        if let Some(id) = app.deleg.selected.filter(|id| {
            app.deleg
                .d
                .get(*id)
                .is_some_and(|job| !job.native_inputs().is_empty())
        }) {
            open(app, id);
            return true;
        }
    }
    let Some(review) = app.deleg.input.as_mut() else {
        return false;
    };
    let count = questions::native_questions(&review.request)
        .get(review.answers.len())
        .map_or(0, |(_, options)| options.len());
    match key.code {
        KeyCode::Esc => app.deleg.input = None,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.deleg.input = None
        }
        KeyCode::F(8) => {
            review.armed = !review.armed;
            review.follow_selection = review.armed;
        }
        KeyCode::PageUp => {
            review.armed = false;
            review.scroll = review.scroll.saturating_sub(10);
        }
        KeyCode::PageDown => {
            review.armed = false;
            review.scroll = (review.scroll + 10).min(review.max_scroll);
        }
        KeyCode::Up if review.armed && count > 0 => {
            review.selected = (review.selected + count - 1) % count;
            review.follow_selection = true;
        }
        KeyCode::Down | KeyCode::Tab if review.armed && count > 0 => {
            review.selected = (review.selected + 1) % count;
            review.follow_selection = true;
        }
        KeyCode::Up => review.scroll = review.scroll.saturating_sub(1),
        KeyCode::Down => review.scroll = (review.scroll + 1).min(review.max_scroll),
        KeyCode::Enter if review.armed => {
            let selected = review.selected;
            choose(app, selected);
        }
        KeyCode::Char(option @ '1'..='9') if review.armed && key.modifiers.is_empty() => {
            choose(app, (option as u8 - b'1') as usize)
        }
        _ => {}
    }
    true
}

pub(super) fn mouse(app: &mut App, event: &MouseEvent) {
    let Some(review) = app.deleg.input.as_mut() else {
        return;
    };
    let hit = review
        .targets
        .iter()
        .find(|(rect, _)| contains(*rect, event.column, event.row))
        .map(|(_, option)| *option);
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => review.pressed = hit,
        MouseEventKind::Up(MouseButton::Left) => {
            let pressed = review.pressed.take();
            if let Some(option) = hit.filter(|_| pressed == hit) {
                choose(app, option);
            }
        }
        MouseEventKind::ScrollUp => {
            review.armed = false;
            review.scroll = review.scroll.saturating_sub(3);
        }
        MouseEventKind::ScrollDown => {
            review.armed = false;
            review.scroll = (review.scroll + 3).min(review.max_scroll);
        }
        _ => {}
    }
}

pub(super) fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let Some(review) = app.deleg.input.as_mut() else {
        return;
    };
    let envelope = review.request.envelope();
    let owner = review.request.owner();
    let mut rows: Vec<(String, Option<usize>)> = Vec::new();
    let mut text = |text: String, option| {
        rows.extend(
            workers::wrap_plain(&text, area.width.saturating_sub(4).max(1) as usize)
                .into_iter()
                .map(|line| (line, option)),
        );
    };
    if let Some(turn) = envelope.native_turn_id.as_deref() {
        text(format!("Native turn: {turn}"), None);
        text(
            serde_json::to_string_pretty(&review.request.params).unwrap_or_default(),
            None,
        );
        if let Some(detail) = app
            .deleg
            .d
            .get(review.job)
            .and_then(|job| {
                job.cards
                    .iter()
                    .find(|card| Some(card.id.as_str()) == review.request.params["itemId"].as_str())
            })
            .and_then(|card| card.detail.as_ref())
        {
            text(detail.clone(), None);
        }
    }
    text(
        format!(
            "Worker #{} · {} / {}",
            review.job,
            owner.map_or("unknown", |owner| owner.provider.as_str()),
            owner.map_or("unknown", |owner| owner.account.as_str())
        ),
        None,
    );
    text(
        format!(
            "Run {} · attempt {}",
            owner.map_or("unknown", |owner| owner.run_id.as_str()),
            owner.map_or("unknown", |owner| owner.attempt_id.as_str())
        ),
        None,
    );
    text(
        format!(
            "Native session: {} · connection {} · request {}",
            envelope.native_session_id, envelope.connection_id, envelope.request_id
        ),
        None,
    );
    text("F8 enters/leaves options · arrows select/scroll · Enter confirms · Esc / X returns to chat and leaves the request pending".into(), None);
    if let Some(plan) = review
        .request
        .params
        .get("plan")
        .and_then(serde_json::Value::as_str)
    {
        text(plan.to_owned(), None);
    }
    if !review.request.is_active() {
        text("This native request is no longer active. Close this view and inspect the current attempt.".into(), None);
    }
    let questions = questions::native_questions(&review.request);
    if let Some(tool) = review.request.params.get("toolCall") {
        for key in ["rawInput", "content", "locations"] {
            if let Some(detail) = tool.get(key).filter(|detail| !detail.is_null()) {
                text(format!("Tool {key}: {detail}"), None);
            }
        }
    }
    if let Some((prompt, options)) = questions.get(review.answers.len()) {
        text(
            format!(
                "Question {} of {}: {prompt}",
                review.answers.len() + 1,
                questions.len()
            ),
            None,
        );
        for (index, option) in options.iter().enumerate() {
            text(
                format!(
                    "{} {}. {option}",
                    if review.armed && review.selected == index {
                        ">"
                    } else {
                        " "
                    },
                    index + 1
                ),
                Some(index),
            );
        }
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" native worker input ");
    let inner = block.inner(area);
    review.max_scroll = rows.len().saturating_sub(inner.height as usize);
    if review.follow_selection {
        if let Some(selected) = rows
            .iter()
            .position(|(_, option)| *option == Some(review.selected))
        {
            if selected < review.scroll {
                review.scroll = selected;
            }
            if selected >= review.scroll + inner.height as usize {
                review.scroll = selected.saturating_sub(inner.height.saturating_sub(1) as usize);
            }
        }
        review.follow_selection = false;
    }
    review.scroll = review.scroll.min(review.max_scroll);
    review.targets.clear();
    let visible = rows
        .into_iter()
        .skip(review.scroll)
        .take(inner.height as usize)
        .enumerate()
        .map(|(row, (text, option))| {
            if let Some(option) = option {
                review.targets.push((
                    Rect::new(inner.x, inner.y + row as u16, inner.width, 1),
                    option,
                ));
            }
            Line::from(text)
        })
        .collect::<Vec<_>>();
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(visible).block(block), area);
    modal::draw_close(f, area, app, modal::Target::WorkerInput);
}
