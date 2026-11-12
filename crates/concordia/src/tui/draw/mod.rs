use super::reply_header;
use super::*;

pub(super) fn draw_ui(f: &mut Frame, app: &mut App) {
    app.permission_rows.clear();
    app.areas.permissions_popup = Rect::default();
    app.areas.run_cancel = Rect::default();
    app.areas.pinned_card = None;
    app.areas.modal_close = None;
    let (screen, rename_cursor) = draw_tab_strip(f, app);
    if app.local_terminal.visible() {
        local_terminal::draw(f, app, screen);
        theme::apply(f.buffer_mut(), app.no_color, app.raster.native);
        return;
    }
    if app
        .picker
        .as_ref()
        .is_some_and(|p| p.kind == PickerKind::Update)
    {
        // A page owns the screen; preserve the workspace without drawing through it.
        app.areas = Areas::default();
        let page = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(screen);
        f.render_widget(Clear, screen);
        draw_header(f, page[0], app);
        draw_picker(f, page[1], app);
        draw_input(f, page[2], app);
        draw_footer(f, page[3], app);
    } else {
        let (chat_screen, panel_screen) =
            if worker_panel::visible(screen.width, app.worker_panel_open) {
                let areas = Layout::horizontal([
                    Constraint::Min(1),
                    Constraint::Length(worker_panel::DEFAULT_WIDTH),
                ])
                .split(screen);
                (areas[0], areas[1])
            } else {
                (screen, Rect::default())
            };
        let chat_area = workspace::draw_in(f, app, chat_screen);
        // The prompt box grows with its content (up to MAX_INPUT_ROWS), so size it first.
        let text_w = (chat_area.width as usize)
            .saturating_sub(2 + PROMPT_W)
            .max(1);
        let input_rows = app.editor.layout(text_w).len().clamp(1, MAX_INPUT_ROWS);
        let mut chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(3),
                Constraint::Length(
                    input_rows as u16 + 2 + app.images.height(chat_area.width) as u16,
                ),
                Constraint::Length(1),
            ])
            .split(chat_area)
            .to_vec();
        // Queue and question panels reserve space first. The worker strip is carved last so it
        // stays directly above the composer instead of being separated from it by another panel.
        let queue_area = queue::carve(&mut chunks[1], app);
        let question_area = questions::carve(&mut chunks[1], app);
        let strip = workers::carve(&mut chunks[1], app);
        let run_status = if app.busy && app.picker.is_none() && app.activity.detail.is_none() {
            chunks[1].height = chunks[1].height.saturating_sub(1);
            Rect::new(chunks[1].x, chunks[1].bottom(), chunks[1].width, 1)
        } else {
            Rect::default()
        };

        draw_header(f, chunks[0], app);
        if landing::visible(app) {
            landing::draw(f, chunks[1], app);
        } else {
            draw_transcript(f, chunks[1], app);
        }
        draw_input(f, chunks[2], app);
        draw_run_status(f, run_status, app);
        questions::draw(f, question_area, app);
        queue::draw(f, queue_area, app);
        if app
            .picker
            .as_ref()
            .is_some_and(|p| p.kind != PickerKind::Trust)
        {
            draw_picker(f, chunks[1], app);
        }
        draw_palette(f, chunks[1], chunks[2], app);
        chatref::draw(f, chunks[1], chunks[2], app);
        draw_footer(f, chunks[3], app);
        workers::draw(f, strip, chunks[1], app);
        if panel_screen.width > 0 {
            worker_panel::draw(f, panel_screen, app);
        }
    }
    if app
        .picker
        .as_ref()
        .is_some_and(|p| p.kind == PickerKind::Trust)
    {
        draw_picker(f, screen, app);
    }
    if app.project_search.active() {
        app.areas = Areas::default();
        app.project_search.draw(f, screen);
    }
    // Cover every widget, including provider-controlled titles, menus and detail panes.
    if worker_input::active(app) {
        app.areas = Areas::default();
        worker_input::draw(f, app);
    }
    if app.raster.active() && !worker_input::active(app) {
        app.areas = Areas::default();
        raster::draw(f, app);
    }
    if app.key_sheet {
        draw_key_sheet(f, app);
    }
    // A raw ESC in a cell would bypass ratatui's diff buffer and corrupt the real screen.
    for cell in &mut f.buffer_mut().content {
        if cell.symbol().chars().any(char::is_control) {
            let clean: String = cell
                .symbol()
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            cell.set_symbol(&clean);
        }
    }
    if let Some(cursor) = rename_cursor.filter(|_| {
        !worker_input::active(app)
            && !app.raster.active()
            && !app.project_search.active()
            && app.picker.is_none()
            && !app.key_sheet
    }) {
        f.set_cursor_position(cursor);
    }
    theme::apply(f.buffer_mut(), app.no_color, app.raster.native);
}

/// Keep the title editor in the tab row, including a single-tab window during rename.
fn draw_tab_strip(f: &mut Frame, app: &mut App) -> (Rect, Option<Position>) {
    app.tab_hits.clear();
    app.tab_close_hits.clear();
    let area = f.area();
    if app.tab_strip.len() < 2 && app.rename_editor.is_none() || area.height < 4 {
        return (area, None);
    }
    let row = Rect::new(area.x, area.y, area.width, 1);
    f.render_widget(Clear, row);
    let mut x = row.x;
    let mut spans = Vec::new();
    let fallback;
    let labels = if app.tab_strip.is_empty() {
        fallback = vec![app.tab_label()];
        &fallback
    } else {
        &app.tab_strip
    };
    let mut rename = None;
    for (index, label) in labels.iter().enumerate() {
        let prefix = format!(" {} ", index + 1);
        let editor = app
            .rename_editor
            .as_ref()
            .filter(|_| index == app.tab_index);
        let edit_width = 24usize
            .min(usize::from(row.right().saturating_sub(x)).saturating_sub(prefix.len() + 5));
        let text = if editor.is_some() {
            format!("{prefix}{}  ×  ", " ".repeat(edit_width))
        } else {
            format!("{prefix}{label}  ×  ")
        };
        let text_width = Line::raw(text.as_str()).width();
        let width = text_width.min(usize::from(row.right().saturating_sub(x))) as u16;
        if width == 0 {
            break;
        }
        let style = if index == app.tab_index {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray).bg(Color::Indexed(236))
        };
        app.tab_hits.push(Rect::new(x, row.y, width, 1));
        app.tab_close_hits.push(Rect::new(
            x.saturating_add(width.saturating_sub(5)),
            row.y,
            if usize::from(width) == text_width {
                5
            } else {
                0
            },
            1,
        ));
        if let Some(editor) = editor.filter(|_| edit_width > 0) {
            let (line, col) = editor.cursor_pos(edit_width);
            let rows = editor.layout(edit_width);
            let slice = rows[line];
            let selection = editor.selection();
            let title: Vec<_> = (slice.start..slice.end)
                .map(|i| {
                    let selected = selection.is_some_and(|(a, b)| i >= a && i < b);
                    Span::styled(
                        editor.chars()[i].to_string(),
                        if selected {
                            Style::default().fg(Color::White).bg(Color::Blue)
                        } else {
                            Style::default().fg(Color::Black).bg(Color::White)
                        },
                    )
                })
                .collect();
            rename = Some((
                Rect::new(x + prefix.len() as u16, row.y, edit_width as u16, 1),
                title,
                col,
            ));
        }
        spans.push(Span::styled(text, style));
        spans.push(Span::raw(" "));
        x = x.saturating_add(width + 1);
    }
    f.render_widget(Paragraph::new(Line::from(spans)), row);
    let cursor = rename.map(|(rect, title, col)| {
        f.render_widget(
            Paragraph::new(Line::from(title)).style(Style::default().bg(Color::White)),
            rect,
        );
        Position::new(
            rect.x + col.min(rect.width.saturating_sub(1) as usize) as u16,
            rect.y,
        )
    });
    (
        Rect::new(area.x, area.y + 1, area.width, area.height - 1),
        cursor,
    )
}

fn draw_key_sheet(f: &mut Frame, app: &App) {
    let bindings: Vec<_> = crate::commands::command_registry()
        .iter()
        .filter_map(|c| c.key_binding.map(|key| (key, c.name, c.description)))
        .collect();
    let mut lines = vec![Line::from("Key sheet")];
    let mut last = None;
    for (key, command, description) in bindings {
        let category = crate::commands::command_registry()
            .iter()
            .find(|c| c.name == command)
            .map(|c| format!("{:?}", c.category))
            .unwrap_or_default();
        if last.as_deref() != Some(category.as_str()) {
            lines.push(Line::from(format!("  {category}")));
            last = Some(category);
        }
        lines.push(Line::from(format!("  {key:<14} {command} — {description}")));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("  ? / F1 / Esc  close"));
    let width = lines.iter().map(|l| l.width()).max().unwrap_or(20) as u16 + 4;
    let height = lines.len() as u16 + 2;
    let area = Rect::new(
        f.area().x + f.area().width.saturating_sub(width) / 2,
        f.area().y + f.area().height.saturating_sub(height) / 2,
        width.min(f.area().width),
        height.min(f.area().height),
    );
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" shortcuts ")),
        area,
    );
    let _ = app;
}

/// Slash-command suggestions, floating just above the prompt box.
fn draw_palette(f: &mut Frame, above: Rect, input: Rect, app: &mut App) {
    app.areas.palette = Rect::default();
    let entries = app.palette_entries();
    if entries.is_empty() {
        return;
    }
    app.palette.clamp(entries.len());
    let shown = entries.len().min(10);
    let height = shown as u16 + 2;
    // Command column (padded) + the longest description + highlight marker (2) + borders (2).
    let cmd_col = entries
        .iter()
        .map(|e| e.command.chars().count())
        .max()
        .unwrap_or(8)
        + 2;
    let help_col = entries
        .iter()
        .map(|e| e.help.chars().count() + if e.custom { 8 } else { 0 })
        .max()
        .unwrap_or(12);
    let max_width = f.area().width.saturating_sub(2);
    let width = (cmd_col + help_col + 4).min(usize::from(max_width)) as u16;
    let y = input.y.saturating_sub(height).max(above.y);
    let rect = Rect {
        x: input.x + 1,
        y,
        width: width.min(f.area().width.saturating_sub(input.x + 1)),
        height: height.min(input.y.saturating_sub(above.y).max(3)),
    };
    app.areas.palette = rect;
    f.render_widget(Clear, rect);
    let cmd_w = entries
        .iter()
        .map(|e| e.command.chars().count())
        .max()
        .unwrap_or(8)
        + 2;
    let items: Vec<ListItem> = entries
        .iter()
        .map(|e| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<w$}", e.command, w = cmd_w),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(e.help.clone(), Style::default().fg(Color::Indexed(245))),
                Span::styled(
                    if e.custom { "  custom" } else { "" },
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(if palette::has_argument_menu(&app.editor.text()) {
                    format!(
                        " {} options ",
                        app.editor.text().split_whitespace().next().unwrap_or("/")
                    )
                } else if entries.len() > shown {
                    format!(" commands \u{b7} {} ", entries.len())
                } else {
                    " commands ".to_string()
                }),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Indexed(238))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select(Some(app.palette.selected));
    f.render_stateful_widget(list, rect, &mut state);
}

const BAR_BG: Color = Color::Indexed(236);
const SPINNER: [&str; 4] = ["◇", "◈", "◆", "◈"];

/// Omit unused leading units from the live timer beside the run status.
fn run_elapsed(seconds: u64) -> String {
    crate::delegate::fmt_elapsed(Duration::from_secs(seconds))
}

pub(super) fn spinner(tick: u64) -> &'static str {
    SPINNER[(tick as usize) % SPINNER.len()]
}

fn draw_run_status(f: &mut Frame, area: Rect, app: &mut App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let key = Style::default().fg(Color::DarkGray);
    let right = format!(
        " \u{b7} {} \u{b7} ",
        run_elapsed(app.turn_started.map_or(0, |t| t.elapsed().as_secs()))
    );
    let label = if area.width < 60 {
        match app.phase {
            Phase::Waiting => "Starting".into(),
            Phase::Thinking => "Thinking".into(),
            Phase::Tool(_) => "Running a tool".into(),
            Phase::Writing => "Writing the reply".into(),
        }
    } else {
        app.run_status_label()
    };
    let room = (area.width as usize).saturating_sub(Line::from(right.clone()).width() + 18);
    let mut status = String::new();
    let mut width = 0;
    for c in label.chars() {
        let n = text::char_width(c);
        if width + n > room {
            break;
        }
        status.push(c);
        width += n;
    }
    let prefix = format!(
        " {} {status}",
        app.phase
            .frame_with_motion(app.tick_count, app.appearance.ui.motion)
    );
    let x =
        area.x + (Line::from(prefix.clone()).width() + Line::from(right.clone()).width()) as u16;
    if x + 7 <= area.right() {
        app.areas.run_cancel = Rect::new(x, area.y, 7, 1);
    }
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(app.phase.color())),
            Span::styled(right, Style::default().fg(Color::DarkGray)),
            Span::styled("[ Esc ]", key),
            Span::raw(" cancel"),
        ])),
        area,
    );
}

/// A quiet colour cue per provider, so the active account is recognisable at a glance.
pub(super) fn provider_color(p: ProviderKind) -> Color {
    match p {
        ProviderKind::Codex => Color::Indexed(114),
        ProviderKind::Claude => Color::Indexed(215),
        ProviderKind::Cursor => Color::Indexed(111),
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &mut App) {
    app.sync_needs_you();
    app.areas.header_email = None;
    app.areas.header_home = None;
    app.areas.header_credits = None;
    app.areas.header_attention = None;
    let bar = Style::default().bg(BAR_BG).fg(Color::Indexed(252));
    let model = if app.model == "?" {
        "model ?".to_string()
    } else {
        app.model.clone()
    };
    let email = if app.header_email_revealed {
        app.account_email.as_deref().unwrap_or_default()
    } else if app.account_email.is_some() {
        "view email"
    } else {
        ""
    };
    let account = app.account_label();
    let right = String::new();
    let available = area.width as usize;
    let brand = if area.width >= 75 {
        format!(" concordia {}   ", short_stamp())
    } else {
        " concordia  ".into()
    };
    let model = format!("  {model}");
    let navigation = if landing::visible(app) {
        " → Chat "
    } else {
        " ← Home "
    };
    if app.picker.is_none() {
        app.areas.header_home = Some(Rect::new(area.x, area.y, area.width.min(8), 1));
    }
    let mut left = vec![
        Span::styled(navigation, bar.fg(Color::Cyan)),
        Span::styled(brand, bar.fg(Color::Indexed(75))),
    ];
    left.push(Span::styled(
        format!(" {account} "),
        bar.bg(provider_color(app.provider))
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
    ));
    let email_x = left.iter().map(Span::width).sum::<usize>();
    let email_text = format!("  {email}");
    let email_text = if email_x + Line::from(email_text.clone()).width() <= available {
        email_text
    } else {
        "  email".into()
    };
    let email_width = Line::from(email_text.clone()).width();
    if app.account_email.is_some() && email_x + email_width <= available {
        if app.account_email.is_some() {
            app.areas.header_email = Some(Rect::new(
                area.x + email_x as u16,
                area.y,
                email_width as u16,
                1,
            ));
        }
        left.push(Span::styled(email_text, bar));
    }
    if left.iter().map(Span::width).sum::<usize>() + model.len() <= available {
        left.push(Span::styled(model, bar.fg(Color::Gray)));
    }
    let agent_count = app.native_agents.count() + app.deleg.d.jobs.len();
    if agent_count > 0 {
        let agents = format!("  Ctrl+G agents {agent_count}");
        if left.iter().map(Span::width).sum::<usize>() + agents.len() <= available {
            left.push(Span::styled(agents, bar.fg(Color::Cyan)));
        }
    }
    let needs_you = app.needs_you.len();
    if needs_you > 0 {
        let badge = format!("  Attention {needs_you} (Ctrl+N)");
        if left.iter().map(Span::width).sum::<usize>() + badge.len() <= available {
            let x = left.iter().map(Span::width).sum::<usize>() as u16;
            app.areas.header_attention = Some(Rect::new(area.x + x, area.y, badge.len() as u16, 1));
            left.push(Span::styled(
                badge,
                bar.fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        }
    }
    let usage_color = match app.usage_tone {
        UsageTone::Ok => Color::Green,
        UsageTone::Warn => Color::Yellow,
        UsageTone::Bad => Color::Red,
        UsageTone::Mute => Color::DarkGray,
    };
    let used: usize = left.iter().map(Span::width).sum();
    let pad = (area.width as usize).saturating_sub(used + right.chars().count());
    left.push(Span::styled(" ".repeat(pad), bar));
    left.push(Span::styled(right, bar.fg(usage_color)));
    f.render_widget(
        Paragraph::new(Line::from(left)).style(bar),
        Rect {
            height: area.height.min(1),
            ..area
        },
    );
    if area.height > 1 {
        if app.picker.is_none() {
            let width = Line::from(format!(" {} ", app.usage_credits))
                .width()
                .min(area.width as usize) as u16;
            app.areas.header_credits = Some(Rect::new(area.x, area.y + 1, width, 1));
        }
        let home_owns_usage = landing::usage_box_visible(app, area.width);
        let credits_known = app.usage_credits != "Credits: not reported";
        let mut line = Vec::new();
        if !home_owns_usage || credits_known {
            line.push(Span::styled(
                format!(" {}", app.usage_credits),
                bar.fg(Color::Cyan),
            ));
        }
        if !home_owns_usage {
            let subscription = app
                .usage_summary
                .split(" · ")
                .filter(|part| !part.starts_with("credits "))
                .collect::<Vec<_>>()
                .join(" · ");
            let subscription = if subscription.is_empty() {
                "not reported".to_string()
            } else {
                subscription
            };
            if subscription != "not reported" || credits_known {
                let reset = app
                    .usage_reset
                    .as_deref()
                    .map(|reset| format!(", {reset}"))
                    .unwrap_or_default();
                line.push(Span::styled(
                    format!(
                        "{}│  Subscription: {subscription}{reset}",
                        if credits_known { "  " } else { " " }
                    ),
                    bar.fg(usage_color),
                ));
            }
        }
        f.render_widget(
            Paragraph::new(Line::from(line)).style(bar),
            Rect {
                y: area.y + 1,
                height: 1,
                ..area
            },
        );
    }
}

#[allow(clippy::if_same_then_else)]
fn draw_transcript(f: &mut Frame, area: Rect, app: &mut App) {
    // No box: the conversation is the page. A little side padding keeps text off the edges.
    let block = Block::default().padding(ratatui::widgets::Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    app.areas.transcript = inner;

    let spacer = || TBlock {
        identity: None,
        gutter: "",
        gutter_style: Style::default(),
        style: Style::default(),
        text: "\n",
        markdown: false,
        linkify: true,
        runs: &[],
        header_files: &[],
    };
    let live_thought = if app.expanded_activity {
        app.live_thinking.clone()
    } else {
        tail_lines(&condense_thinking(&app.live_thinking), 3)
    };
    let summaries: Vec<_> = app
        .messages
        .iter()
        .map(|m| {
            m.card
                .as_ref()
                .map(crate::provider::stream_cards::compact_card_summary)
        })
        .collect();
    let mut displayed: Vec<Option<String>> = app
        .messages
        .iter()
        .enumerate()
        .map(|(idx, m)| match m.role {
            MsgRole::Card if app.activity.is_expanded(idx, app.expanded_activity) => m
                .card
                .as_ref()
                .map(|c| {
                    format!(
                        "{}\n{}",
                        summaries[idx]
                            .as_ref()
                            .map_or(m.text.as_str(), |summary| summary.text.as_str()),
                        c.detail.as_deref().unwrap_or("")
                    )
                })
                .map(Some)
                .unwrap_or(None),
            // Short thinking is shown whole; a toggle that reveals nothing new is noise.
            MsgRole::Thinking if !thinking_expandable(&m.text) => {
                Some(format!("Thinking\n{}", m.text.trim_end()))
            }
            MsgRole::Thinking => Some(format!(
                "Thinking - {}\n{}",
                if app.activity.is_expanded(idx, app.expanded_activity) {
                    "Collapse"
                } else {
                    "Expand"
                },
                if app.activity.is_expanded(idx, app.expanded_activity) {
                    m.text.trim_end().to_string()
                } else {
                    condense_thinking(&m.text)
                }
            )),
            MsgRole::System if m.is_map() => {
                let (heading, rest) = m.text.split_once('\n').unwrap_or((&m.text, ""));
                Some(if app.activity.is_expanded(idx, app.expanded_activity) {
                    format!("{heading} - Collapse\n{rest}")
                } else {
                    format!("{heading} - Expand")
                })
            }
            _ => None,
        })
        .collect();
    let groups = activity::grouped_cards(&app.messages, &app.activity, app.expanded_activity);
    for (index, text) in groups.text.into_iter().enumerate() {
        if text.is_some() {
            displayed[index] = text;
        }
    }
    for (index, message) in app.messages.iter().enumerate() {
        if message.role == MsgRole::Card && !groups.hidden[index] {
            let body = displayed[index]
                .take()
                .or_else(|| {
                    summaries[index]
                        .as_ref()
                        .map(|summary| summary.text.clone())
                })
                .unwrap_or_else(|| message.text.clone());
            let (heading, tail) = body.split_once('\n').unwrap_or((&body, ""));
            let action = if app.activity.is_expanded(index, app.expanded_activity) {
                "Collapse"
            } else {
                "Expand"
            };
            displayed[index] = Some(format!(
                "{heading} - {action}{}{}",
                if tail.is_empty() { "" } else { "\n" },
                tail
            ));
        }
    }
    let tip_text = app.visible_tip().map(|t| format!("tip: {t}"));
    let cur_meta = app.reply_meta();
    // Preserve semantic colour through wrapping, selection and saved-history rendering.
    let msg_runs: Vec<Vec<transcript::LineRun>> = app
        .messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            if m.is_map() {
                activity::card_runs(displayed[i].as_deref().unwrap_or(&m.text), None)
            } else if m.role == MsgRole::System {
                usage_style::runs(&m.text)
            } else if m.role == MsgRole::Card {
                activity::card_runs(displayed[i].as_deref().unwrap_or(&m.text), m.card.as_ref())
            } else {
                Vec::new()
            }
        })
        .collect();
    let msg_files: Vec<&[transcript::HeaderFile]> = summaries
        .iter()
        .enumerate()
        .map(|(i, summary)| {
            summary
                .as_ref()
                .filter(|summary| {
                    displayed[i]
                        .as_deref()
                        .unwrap_or(&app.messages[i].text)
                        .starts_with(&summary.text)
                })
                .map_or(&[][..], |summary| summary.files.as_slice())
        })
        .collect();
    let mut blocks: Vec<TBlock> = Vec::with_capacity(app.messages.len() * 3 + 4);
    // Inside a slash-command output block (after its echo) plain lines get no icon.
    let mut in_block = false;
    let count = app.messages.len();
    for (i, m) in app.messages.iter().enumerate() {
        if groups.hidden[i] {
            continue;
        }
        let (mut gutter, gutter_style, style) = match m.role {
            MsgRole::User => (
                "You › ",
                Style::default()
                    .fg(Color::Indexed(117))
                    .bg(Color::Indexed(235))
                    .add_modifier(Modifier::BOLD),
                Style::default().bg(Color::Indexed(235)),
            ),
            MsgRole::Assistant => (
                "\u{25cf} ",
                Style::default().fg(Color::Indexed(179)),
                Style::default(),
            ),
            // Command echo: the title of a dim, barred output block.
            MsgRole::Command => (
                "| ",
                Style::default().fg(Color::Indexed(244)),
                Style::default()
                    .fg(Color::Indexed(252))
                    .add_modifier(Modifier::BOLD),
            ),
            // Command output and notices: neutral bar, small coloured level icon, dim text.
            MsgRole::System => {
                let lvl = level_of(&m.text);
                let g = match (lvl, in_block) {
                    (Level::Error, _) => "| x ",
                    (Level::Warn, _) => "| ! ",
                    (Level::Ok, _) => "| + ",
                    (Level::Info, false) => "| i ",
                    (Level::Info, true) => "|   ",
                };
                let col = match lvl {
                    Level::Error => Color::Red,
                    Level::Warn => Color::Yellow,
                    Level::Ok => Color::Green,
                    Level::Info if in_block => Color::Indexed(240),
                    Level::Info => Color::Indexed(75),
                };
                let text_col = if lvl == Level::Error {
                    Color::Red
                } else {
                    Color::Indexed(245)
                };
                (g, Style::default().fg(col), Style::default().fg(text_col))
            }
            MsgRole::Thinking => (
                "▸ ",
                Style::default().fg(Color::Magenta),
                Style::default()
                    .fg(Color::Indexed(244))
                    .add_modifier(Modifier::ITALIC),
            ),
            MsgRole::Meta => (
                "  ",
                Style::default().fg(Color::Indexed(244)),
                Style::default()
                    .fg(Color::Indexed(244))
                    .add_modifier(Modifier::DIM | Modifier::ITALIC),
            ),
            MsgRole::Card => (
                "  ",
                Style::default().fg(Color::Indexed(244)),
                Style::default().fg(Color::Indexed(250)),
            ),
        };
        if m.role == MsgRole::Thinking && !m.toggles() {
            gutter = "  ";
        } else if m.toggles() {
            gutter = if app.activity.is_expanded(i, app.expanded_activity) {
                "▾ "
            } else {
                "▸ "
            };
        }
        // Breathing room between turns, but tool cards stay grouped with what triggered them.
        if matches!(m.role, MsgRole::User | MsgRole::Command) && i > 0 {
            blocks.push(spacer());
        }
        match m.role {
            MsgRole::Command => in_block = true,
            MsgRole::System => {}
            _ => in_block = false,
        }
        // Label the start of each agent turn (its cards, thoughts and reply share one label).
        if let Some((p, label)) = &m.meta {
            if label == "__concordia_idle_notice__" {
                // Marker-only metadata: do not render a provider header.
            } else {
                let same = i > 0 && app.messages[i - 1].meta.as_ref().map(|x| &x.1) == Some(label);
                if !same {
                    blocks.push(reply_header(*p, label));
                }
            }
        }
        blocks.push(TBlock {
            identity: Some(i),
            gutter,
            gutter_style,
            style,
            text: displayed[i].as_deref().unwrap_or(&m.text),
            markdown: matches!(m.role, MsgRole::Assistant | MsgRole::Thinking)
                || m.role == MsgRole::User && m.text.contains("[Image: "),
            linkify: m.role != MsgRole::Meta,
            runs: &msg_runs[i],
            header_files: msg_files[i],
        });
        let next_is_card = app
            .messages
            .get(i + 1)
            .is_some_and(|n| matches!(n.role, MsgRole::Card | MsgRole::Meta));
        if matches!(m.role, MsgRole::Assistant | MsgRole::Meta) && i + 1 < count && !next_is_card {
            blocks.push(spacer());
        }
    }
    let last_labelled = app
        .messages
        .last()
        .and_then(|x| x.meta.as_ref())
        .map(|x| &x.1);
    if app.busy
        && (!live_thought.is_empty() || !app.live_text.trim().is_empty())
        && last_labelled != Some(&cur_meta.1)
    {
        blocks.push(reply_header(cur_meta.0, &cur_meta.1));
    }
    // Live reasoning while the turn runs: the last few lines, dimmed.
    if app.busy && !live_thought.is_empty() {
        blocks.push(TBlock {
            identity: None,
            gutter: "~ ",
            gutter_style: Style::default().fg(Color::Magenta),
            style: Style::default()
                .fg(Color::Indexed(244))
                .add_modifier(Modifier::ITALIC),
            text: &live_thought,
            markdown: true,
            linkify: true,
            runs: &[],
            header_files: &[],
        });
    }
    // Live assistant text while the turn is still running.
    if app.busy && !app.live_text.trim().is_empty() {
        blocks.push(TBlock {
            identity: None,
            gutter: "\u{25cf} ",
            gutter_style: Style::default().fg(Color::Indexed(179)),
            style: Style::default(),
            text: &app.live_text,
            markdown: true,
            linkify: true,
            runs: &[],
            header_files: &[],
        });
    }
    if let Some(tip) = &tip_text {
        blocks.push(TBlock {
            identity: None,
            gutter: "  ",
            gutter_style: Style::default(),
            style: Style::default()
                .fg(Color::Indexed(244))
                .add_modifier(Modifier::DIM | Modifier::ITALIC),
            text: tip,
            markdown: false,
            linkify: false,
            runs: &[],
            header_files: &[],
        });
    }
    if app.view.link_root.as_deref() != Some(app.cwd.as_path()) {
        app.view.link_root = Some(app.cwd.clone());
    }
    app.view
        .rebuild(&blocks, inner.width as usize, inner.height as usize);
    f.render_widget(Paragraph::new(app.view.lines()).block(block), area);
    if inner.height > 1 && inner.width > 1 && app.view.rows.len() > app.view.height() {
        let rail_x = inner.right().saturating_sub(1);
        let rail = Rect::new(rail_x, inner.y, 1, inner.height);
        for y in rail.y..rail.bottom() {
            f.buffer_mut()[(rail.x, y)]
                .set_symbol("│")
                .set_style(Style::default().fg(Color::Indexed(238)));
        }
        let total = app.view.rows.len().max(1);
        let thumb_height =
            (u32::from(inner.height) * u32::from(inner.height) / total as u32).max(1) as u16;
        let max_offset = inner.height.saturating_sub(thumb_height);
        let thumb_y = inner.y
            + max_offset.saturating_mul(app.view.scroll as u16)
                / app.view.rows.len().saturating_sub(app.view.height()).max(1) as u16;
        for y in thumb_y..thumb_y.saturating_add(thumb_height).min(inner.bottom()) {
            f.buffer_mut()[(rail.x, y)]
                .set_symbol("┃")
                .set_style(Style::default().fg(Color::Cyan));
        }
    }
    // Paint the complete user block, including blank and trailing cells. Line styling alone
    // leaves empty paragraph rows at the terminal's default background.
    for (row, index) in app.view.visible().enumerate() {
        if app.view.rows[index]
            .identity
            .and_then(|i| app.messages.get(i))
            .is_some_and(|m| m.role == MsgRole::User)
            && !app.no_color
        {
            f.buffer_mut().set_style(
                Rect::new(inner.x, inner.y + row as u16, inner.width, 1),
                Style::default().bg(Color::Indexed(235)),
            );
        }
    }

    // Keep the expanded card's control reachable after its original header scrolls away.
    let first = app.view.visible().start;
    if inner.height > 1 && inner.width > 0 {
        if let Some(id) = app
            .view
            .rows
            .get(first)
            .and_then(|r| r.identity)
            .filter(|&id| {
                app.activity.is_expanded(id, app.expanded_activity)
                    && first
                        .checked_sub(1)
                        .and_then(|i| app.view.rows.get(i))
                        .is_some_and(|r| r.identity == Some(id))
            })
        {
            if let Some(card) = app.messages.get(id).and_then(|m| m.card.as_ref()) {
                let rect = Rect::new(inner.x, inner.y, inner.width, 1);
                f.render_widget(Clear, rect);
                f.render_widget(
                    Paragraph::new(format!(
                        "\u{25be} Collapse \u{b7} {}",
                        crate::provider::stream_cards::format_card_line(card)
                            .trim_start_matches('│')
                            .trim()
                    ))
                    .style(
                        Style::default()
                            .fg(Color::Cyan)
                            .bg(Color::Indexed(235))
                            .add_modifier(Modifier::BOLD),
                    ),
                    rect,
                );
                app.areas.pinned_card = Some((rect, id));
            }
        }
    }

    if let Some(i) = app.activity.detail {
        let detail_text = app.activity.detail_text.take();
        let content = detail_text
            .as_ref()
            .map(|(title, body)| (title.as_str(), body.as_str()))
            .or_else(|| {
                app.messages
                    .get(i)
                    .and_then(|m| m.card.as_ref())
                    .map(|card| (card.title.as_str(), card.detail.as_deref().unwrap_or("")))
            });
        if let Some((title, body)) = content {
            app.activity.search_parts(&[title, body]);
            let popup = centered_rect(80, 75, area);
            f.render_widget(Clear, popup);
            let inner = Block::default().borders(Borders::ALL).inner(popup);
            let avail_w = inner.width.max(1) as usize;
            if app.activity.follow_match || app.activity.render_width != avail_w {
                if let Some(found) = app.activity.selected() {
                    app.activity.detail_scroll = detail_scroll_target_parts(
                        &[title, body],
                        found,
                        avail_w,
                        inner.height as usize,
                    );
                }
                app.activity.follow_match = false;
            }
            if app.activity.render_dirty
                || app.activity.render_width != avail_w
                || app.activity.render_selected != app.activity.selected_match
            {
                let lines = detail_lines_parts(
                    &[title, body],
                    &app.activity.matches,
                    app.activity.selected_match,
                    avail_w,
                );
                app.activity.render_lines = lines
                    .into_iter()
                    .map(|line| {
                        Line::from(
                            line.spans
                                .into_iter()
                                .map(|span| Span::styled(span.content.into_owned(), span.style))
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect();
                app.activity.render_width = avail_w;
                app.activity.render_selected = app.activity.selected_match;
                app.activity.render_dirty = false;
            }
            let query = app.activity.query_with_cursor();
            let block=Block::default().title(format!("Activity · Find: {query} · {} matches · Enter/Shift+Enter find · Alt+↑/↓ card · Ctrl+C copy · Esc close", app.activity.matches.len())).borders(Borders::ALL);
            let inner = block.inner(popup);
            f.render_widget(block, popup);
            let max_scroll = app
                .activity
                .render_lines
                .len()
                .saturating_sub(inner.height as usize);
            let scroll = (app.activity.detail_scroll as usize).min(max_scroll);
            app.activity.detail_scroll = scroll.min(u16::MAX as usize) as u16;
            app.activity.file_targets.clear();
            let mut row = 0;
            for source in [title, body].into_iter().flat_map(str::lines) {
                let count = text::wrap_ranges(&source.chars().collect::<Vec<_>>(), avail_w).len();
                if let Some(file) = source
                    .strip_prefix("File: ")
                    .and_then(crate::file_links::explicit_reference)
                {
                    for visual in row..row + count {
                        if let Some(line) = app.activity.render_lines.get_mut(visual) {
                            line.style = line
                                .style
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::UNDERLINED);
                        }
                        if visual >= scroll && visual < scroll + inner.height as usize {
                            app.activity.file_targets.push((
                                Rect::new(
                                    inner.x,
                                    inner.y + (visual - scroll) as u16,
                                    inner.width,
                                    1,
                                ),
                                file.clone(),
                            ));
                        }
                    }
                }
                row += count;
            }
            f.render_widget(
                DetailRows {
                    lines: &app.activity.render_lines,
                    scroll,
                },
                inner,
            );
            modal::draw_close(f, popup, app, modal::Target::Activity);
        }
        app.activity.detail_text = detail_text;
    }
}

pub(super) fn detail_scroll_target_parts(
    parts: &[&str],
    found: activity::Match,
    width: usize,
    height: usize,
) -> u16 {
    let width = width.max(1);
    let mut visual = 0usize;
    let mut target = None;
    for (line_no, line) in parts.iter().flat_map(|p| p.lines()).enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let ranges = text::wrap_ranges(&chars, width);
        if line_no == found.line {
            let row = ranges
                .iter()
                .position(|(a, b)| *a <= found.col && found.col < *b)
                .unwrap_or_else(|| ranges.len().saturating_sub(1));
            target = Some(visual + row);
        }
        visual += ranges.len();
    }
    let max_scroll = visual.saturating_sub(height);
    let wanted = target
        .unwrap_or(0)
        .saturating_sub(height / 2)
        .min(max_scroll);
    wanted.min(u16::MAX as usize) as u16
}

pub(super) fn detail_lines_parts<'a>(
    parts: &[&'a str],
    matches: &[activity::Match],
    selected: usize,
    width: usize,
) -> Vec<Line<'a>> {
    let mut output = Vec::new();
    let mut match_cursor = 0;
    let is_diff = parts
        .iter()
        .any(|part| part.lines().any(|line| line.starts_with("@@")));
    for (line_no, line) in parts.iter().flat_map(|p| p.lines()).enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let byte_offsets: Vec<usize> = line
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(line.len()))
            .collect();
        while match_cursor < matches.len() && matches[match_cursor].line < line_no {
            match_cursor += 1;
        }
        let first = match_cursor;
        while match_cursor < matches.len() && matches[match_cursor].line == line_no {
            match_cursor += 1;
        }
        let line_matches = &matches[first..match_cursor];
        let mut row_match_cursor = 0;
        for (row_start, row_end) in text::wrap_ranges(&chars, width.max(1)) {
            while row_match_cursor < line_matches.len()
                && line_matches[row_match_cursor].col + line_matches[row_match_cursor].len
                    <= row_start
            {
                row_match_cursor += 1;
            }
            let row_matches = &line_matches[row_match_cursor..];
            let row_matches = &row_matches[..row_matches.partition_point(|m| m.col < row_end)];
            let mut cuts = vec![row_start, row_end];
            for m in row_matches {
                let a = m.col.max(row_start);
                let b = (m.col + m.len).min(row_end);
                if a < b {
                    cuts.extend([a, b]);
                }
            }
            cuts.sort_unstable();
            cuts.dedup();
            let mut spans = Vec::new();
            let mut mi = 0;
            for pair in cuts.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if a == b {
                    continue;
                }
                while mi < row_matches.len() && row_matches[mi].col + row_matches[mi].len <= a {
                    mi += 1;
                }
                let active = row_matches
                    .get(mi)
                    .filter(|m| m.col <= a && b <= m.col + m.len);
                let style = active
                    .map(|_| {
                        if first + row_match_cursor + mi == selected {
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Cyan)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::Black).bg(Color::Yellow)
                        }
                    })
                    .unwrap_or_else(|| {
                        if is_diff {
                            transcript::diff_line_style(line)
                        } else {
                            Style::default()
                        }
                    });
                spans.push(Span::styled(&line[byte_offsets[a]..byte_offsets[b]], style));
            }
            output.push(Line::from(spans));
        }
    }
    output
}

struct DetailRows<'a> {
    lines: &'a [Line<'static>],
    scroll: usize,
}
impl Widget for DetailRows<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for (row, line) in self
            .lines
            .iter()
            .skip(self.scroll)
            .take(area.height as usize)
            .enumerate()
        {
            buf.set_line(area.x, area.y + row as u16, line, area.width);
        }
    }
}

fn centered_rect(width_pct: u16, height_pct: u16, area: Rect) -> Rect {
    let w = (u32::from(area.width) * u32::from(width_pct) / 100)
        .max(1)
        .min(u32::from(area.width)) as u16;
    let h = (u32::from(area.height) * u32::from(height_pct) / 100)
        .max(1)
        .min(u32::from(area.height)) as u16;
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

/// One `/update` line as styled rows `width` wide; bullets wrap under their text.
fn update_rows(line: &crate::update::PreviewLine, width: usize) -> Vec<Line<'static>> {
    use crate::update::PreviewLine as P;
    let dim = Style::default().add_modifier(Modifier::DIM);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let (lead, lead_style, text, style) = match line {
        P::Field(label, value) => (
            format!("{label:<10} "),
            dim,
            value.clone(),
            Style::default(),
        ),
        P::Title(t) => (String::new(), bold, t.clone(), bold.fg(Color::Cyan)),
        P::Kind(k) => {
            let color = if k.eq_ignore_ascii_case("Fixed") {
                Color::Indexed(107)
            } else if k.eq_ignore_ascii_case("Removed") || k.eq_ignore_ascii_case("Deprecated") {
                Color::Indexed(179)
            } else {
                Color::Indexed(75)
            };
            (" ".to_owned(), bold, k.clone(), bold.fg(color))
        }
        P::Bullet(b) => (" · ".to_string(), dim, b.clone(), Style::default()),
        P::Text(t) => (String::new(), dim, t.clone(), Style::default()),
        P::Hint(h) => (String::new(), dim, h.clone(), dim),
        P::Blank => return vec![Line::default()],
    };
    let indent = lead.chars().count();
    let chars: Vec<char> = text.chars().collect();
    let room = width.saturating_sub(indent).max(1);
    text::wrap_ranges_hang(&chars, room, room)
        .into_iter()
        .enumerate()
        .map(|(i, (s, e))| {
            let head = if i == 0 {
                Span::styled(lead.clone(), lead_style)
            } else {
                Span::raw(" ".repeat(indent))
            };
            let body: String = chars[s..e].iter().collect();
            Line::from(vec![head, Span::styled(body.trim_end().to_string(), style)])
        })
        .collect()
}

fn update_action_rows(blocked: bool, width: usize) -> Vec<Line<'static>> {
    use crate::update::PreviewLine as P;
    let (caption, explanation, exit, color) = if blocked {
        (
            " Update blocked: work pending ",
            "Finish chat/workers and save or close edited files, then press Enter again.",
            "Esc returns to your task. No update is queued.",
            Color::Yellow,
        )
    } else {
        (
            if width < 40 {
                " Enter  Install + restart "
            } else {
                " Enter  Install + restart this tab "
            },
            "Installs, saves your draft and queue, then restarts this tab.",
            "Other tabs keep running. Esc closes without updating.",
            Color::Green,
        )
    };
    let action_style = Style::default()
        .fg(Color::Black)
        .bg(color)
        .add_modifier(Modifier::BOLD);
    let mut rows = update_rows(&P::Title(caption.into()), width);
    for row in &mut rows {
        row.style = action_style;
        for span in &mut row.spans {
            span.style = action_style;
        }
    }
    rows.extend(update_rows(&P::Text(explanation.into()), width));
    rows.extend(update_rows(&P::Text(exit.into()), width));
    rows.push(Line::default());
    rows
}

fn draw_picker(f: &mut Frame, area: Rect, app: &mut App) {
    let agents_panel = app
        .picker
        .as_ref()
        .is_some_and(|p| p.kind == PickerKind::Agents);
    let area = if agents_panel {
        native_agents::panel_area(area)
    } else {
        area
    };
    let update_blocked = app.update_blocked_by_work();
    let update_lines = app.update_preview.as_ref().map(|p| p.visible_lines());
    let Some(picker) = app.picker.as_mut() else {
        return;
    };
    if matches!(
        picker.kind,
        PickerKind::Update
            | PickerKind::Tokens
            | PickerKind::Related
            | PickerKind::Integrations
            | PickerKind::ArtifactDetail
    ) {
        let is_page = picker.kind == PickerKind::Update;
        let width = if is_page {
            area.width
        } else {
            area.width.saturating_sub(4).min(100)
        };
        let inner = width.saturating_sub(4) as usize;
        let loading = [crate::update::PreviewLine::Text(
            "checking for updates…".into(),
        )];
        let lines = update_lines.as_deref().unwrap_or(&loading);
        let rows: Vec<Line> = lines
            .iter()
            .flat_map(|line| update_rows(line, inner.max(10)))
            .collect();
        let mut actions = if picker.kind == PickerKind::Update
            && app
                .update_preview
                .as_ref()
                .is_some_and(|p| p.install_available)
        {
            update_action_rows(update_blocked, inner.max(10))
        } else {
            Vec::new()
        };
        // As tall as its content, up to 24 rows or the space there is.
        let height = if is_page {
            area.height
        } else {
            ((rows.len() + actions.len()) as u16 + 2)
                .min(area.height.min(24))
                .max(3)
        };
        let popup = Rect {
            x: if is_page { area.x } else { area.x + 2 },
            y: area.y,
            width,
            height,
        };
        f.render_widget(Clear, popup);
        // Keep one changelog row available even in a very short terminal.
        actions.truncate(popup.height.saturating_sub(3) as usize);
        let view = popup
            .height
            .saturating_sub(2)
            .saturating_sub(actions.len() as u16) as usize;
        // Scrolling reads these, so it always matches what was last drawn.
        app.update_rows = rows.len();
        app.update_view = view;
        let offset = picker
            .state
            .selected()
            .unwrap_or(0)
            .min(rows.len().saturating_sub(view));
        let visible: Vec<Line> = actions
            .into_iter()
            .chain(rows.into_iter().skip(offset).take(view))
            .collect();
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", picker.title))
            .padding(ratatui::widgets::Padding::horizontal(1));
        f.render_widget(Paragraph::new(visible).block(block), popup);
        modal::draw_close(f, popup, app, modal::Target::Picker);
        return;
    }
    let width = if picker.kind == PickerKind::Trust {
        area.width.saturating_sub(2).min(54)
    } else {
        area.width.saturating_sub(4).max(20)
    };
    let row_width = width.saturating_sub(6).max(1) as usize;
    let permission_lines = (picker.kind == PickerKind::Trust).then(|| {
        let available = app
            .areas
            .permissions
            .y
            .saturating_sub(area.y)
            .saturating_sub(2);
        let rows = picker.filtered();
        let expanded_height = rows
            .iter()
            .map(|row| {
                1 + text::wrap_ranges(&row.detail.chars().collect::<Vec<_>>(), row_width).len()
            })
            .sum::<usize>();
        let compact = row_width < 24 || expanded_height > usize::from(available);
        rows.into_iter()
            .enumerate()
            .map(|(index, row)| {
                let (label, current) = row
                    .label
                    .split_once(" \u{b7} current")
                    .map_or((row.label.as_str(), false), |(label, _)| (label, true));
                let selected = picker.state.selected() == Some(index);
                let label_style = if selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let mut head = vec![Span::styled(label.to_owned(), label_style)];
                if current {
                    head.push(Span::styled(
                        " \u{b7} current",
                        Style::default().fg(Color::Green),
                    ));
                }
                let mut lines = vec![Line::from(head)];
                if !compact {
                    let detail_style = Style::default().fg(if row.id == "trust:full" {
                        Color::Yellow
                    } else {
                        Color::Gray
                    });
                    let chars = row.detail.chars().collect::<Vec<_>>();
                    for (start, end) in text::wrap_ranges(&chars, row_width)
                        .into_iter()
                        .take(usize::from(available.saturating_sub(1)))
                    {
                        lines.push(Line::styled(
                            chars[start..end].iter().collect::<String>(),
                            detail_style,
                        ));
                    }
                }
                lines
            })
            .collect::<Vec<_>>()
    });
    let height = (permission_lines
        .as_ref()
        .map_or_else(
            || {
                picker
                    .filtered()
                    .iter()
                    .map(|r| {
                        if r.label.contains('\n') {
                            r.label
                                .lines()
                                .map(|line| {
                                    text::wrap_ranges(&line.chars().collect::<Vec<_>>(), row_width)
                                        .len()
                                })
                                .sum::<usize>()
                                + usize::from(!r.detail.is_empty())
                        } else {
                            1
                        }
                    })
                    .sum::<usize>()
            },
            |rows| rows.iter().map(Vec::len).sum::<usize>(),
        )
        .min(24) as u16)
        .saturating_add(2)
        .max(5);
    let mut popup = Rect {
        x: area.x + 2,
        y: if agents_panel {
            area.y
        } else {
            area.y + area.height.saturating_sub(height)
        },
        width,
        height: if agents_panel {
            area.height
        } else {
            height.min(area.height)
        },
    };
    if picker.kind == PickerKind::Trust {
        // The footer owns this control; keep its existing picker next to the clicked mode.
        let right = app.areas.permissions.right().min(area.right());
        popup.width = popup.width.min(right.saturating_sub(area.x));
        popup.height = popup
            .height
            .min(app.areas.permissions.y.saturating_sub(area.y));
        popup.x = right.saturating_sub(popup.width);
        popup.y = app.areas.permissions.y.saturating_sub(popup.height);
        app.areas.permissions_popup = popup;
    }
    f.render_widget(Clear, popup);
    if let Some(rows) = permission_lines {
        let heights = rows
            .iter()
            .map(|lines| lines.len() as u16)
            .collect::<Vec<_>>();
        let list = List::new(rows.into_iter().map(ListItem::new).collect::<Vec<_>>())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .padding(ratatui::widgets::Padding::horizontal(1))
                    .title(picker.title.clone()),
            )
            .highlight_style(Style::default().bg(Color::Indexed(235)))
            .highlight_symbol("> ");
        f.render_stateful_widget(list, popup, &mut picker.state);
        let mut top = popup.y.saturating_add(1);
        let mut remaining = popup.height.saturating_sub(2);
        for (index, height) in heights.into_iter().enumerate().skip(picker.state.offset()) {
            if height > remaining {
                break;
            }
            app.permission_rows.push((
                Rect::new(
                    popup.x.saturating_add(1),
                    top,
                    popup.width.saturating_sub(2),
                    height,
                ),
                index,
            ));
            top = top.saturating_add(height);
            remaining = remaining.saturating_sub(height);
        }
        app.related_files.targets.clear();
        modal::draw_close(f, popup, app, modal::Target::Picker);
        return;
    }
    let heading = if picker.kind == PickerKind::Files {
        let filter_width = if picker.filter.is_empty() {
            0
        } else {
            picker.filter.chars().map(text::char_width).sum::<usize>() + 11
        };
        related_files::heading(
            app.related_files.base.as_deref().unwrap_or(&app.cwd),
            (width as usize).saturating_sub(7 + filter_width),
        )
    } else {
        picker.title.clone()
    };
    let title = if picker.filter.is_empty() {
        heading
    } else {
        format!("{} · filter: {}", heading, picker.filter)
    };
    let tones = &app.picker_tones;
    let items: Vec<ListItem> = picker
        .filtered()
        .into_iter()
        .map(|r| {
            let mut label_style = match tones.get(&r.id) {
                Some(UsageTone::Ok) => Style::default().fg(Color::Green),
                Some(UsageTone::Warn) => Style::default().fg(Color::Yellow),
                Some(UsageTone::Bad) => Style::default().fg(Color::Red),
                Some(UsageTone::Mute) => Style::default().fg(Color::DarkGray),
                None => Style::default(),
            };
            if r.id.starts_with("model-group:") {
                label_style = Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM);
            }
            if r.label.contains('\n') {
                let mut lines = Vec::new();
                for line in r.label.lines() {
                    let chars: Vec<_> = line.chars().collect();
                    for (start, end) in text::wrap_ranges(&chars, row_width) {
                        let part: String = chars[start..end].iter().collect();
                        lines.push(Line::from(
                            usage_style::picker_spans(&part)
                                .unwrap_or_else(|| vec![Span::styled(part, label_style)]),
                        ));
                    }
                }
                if !r.detail.is_empty() {
                    lines.push(Line::styled(
                        r.detail.clone(),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                return ListItem::new(lines);
            }
            let (head, current) = match r.label.split_once(" \u{b7} current") {
                Some((h, _)) => (h.to_string(), true),
                None => (r.label.clone(), false),
            };
            let mut spans = match usage_style::picker_spans(&head) {
                Some(s) => s,
                None => vec![Span::styled(head, label_style)],
            };
            if current {
                spans.push(Span::styled(
                    " \u{b7} current",
                    Style::default().fg(Color::Green),
                ));
            }
            if !r.detail.is_empty() {
                spans.extend([Span::styled(
                    format!("  {}", r.detail),
                    Style::default().fg(Color::DarkGray),
                )]);
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .padding(ratatui::widgets::Padding::horizontal(1))
                .title(title),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    f.render_stateful_widget(list, popup, &mut picker.state);
    app.related_files.targets.clear();
    if picker.kind == PickerKind::Files {
        let offset = picker.state.offset();
        for (row, index) in (offset..picker.filtered().len())
            .take(popup.height.saturating_sub(2) as usize)
            .enumerate()
        {
            app.related_files.targets.push((
                Rect {
                    x: popup.x + 2,
                    y: popup.y + 1 + row as u16,
                    width: popup.width.saturating_sub(4),
                    height: 1,
                },
                index,
            ));
        }
    }
    modal::draw_close(f, popup, app, modal::Target::Picker);
}

fn draw_input(f: &mut Frame, area: Rect, app: &mut App) {
    if modal::draw_input(f, area, app) {
        return;
    }
    let area = images::draw(f, area, app);
    const BOTTOM_LABEL: &str = " [ Bottom ] ";
    app.areas.bottom_button = if app.view.scroll > 0 && area.width >= BOTTOM_LABEL.len() as u16 + 2
    {
        Rect::new(
            area.x + (area.width - BOTTOM_LABEL.len() as u16) / 2,
            area.y,
            BOTTOM_LABEL.len() as u16,
            1,
        )
    } else {
        Rect::default()
    };
    // Sending waits for the restart, so the box itself says so, not only the footer.
    let updating = app
        .job
        .as_ref()
        .filter(|j| j.kind == JobKind::Update)
        .map(|j| j.started.elapsed().as_secs());
    let border = if updating.is_some() {
        Color::Cyan
    } else if app.busy {
        app.phase.color()
    } else {
        Color::Indexed(240)
    };
    let title = {
        Line::from(Span::styled(
            if updating.is_some() {
                " message (paused) "
            } else if app.workspace.focused.is_some() {
                " message · Esc returns from file "
            } else if app.busy
                && app.busy_send_confirmation.as_ref().is_some_and(|armed| {
                    armed.turn_started == app.turn_started
                        && armed.conversation == app.deleg.d.conversation_id()
                        && armed.armed.elapsed() <= Duration::from_secs(5)
                })
            {
                " Enter again to interrupt + send · Tab queues "
            } else {
                " message "
            },
            Style::default().fg(Color::DarkGray),
        ))
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(border));
    let block = block.title(title);
    let inner = block.inner(area);
    let text_w = (inner.width as usize).saturating_sub(PROMPT_W).max(1);
    app.areas.input = inner;
    app.areas.input_text_w = text_w;

    let rows = app.editor.layout(text_w);
    let (cur_row, cur_col) = app.editor.cursor_pos(text_w);
    let h = (inner.height as usize).max(1);
    let anchor = (app.editor.revision(), cur_row, cur_col, text_w);
    if app.input_scroll_anchor.is_some_and(|old| old != anchor) {
        app.input_scroll_anchor = None;
    }
    // Wheel scrolling is independent of the caret until an edit/navigation/resize.
    if app.input_scroll_anchor.is_none() && cur_row < app.input_scroll {
        app.input_scroll = cur_row;
    } else if app.input_scroll_anchor.is_none() && cur_row >= app.input_scroll + h {
        app.input_scroll = cur_row + 1 - h;
    }
    app.input_scroll = app.input_scroll.min(rows.len().saturating_sub(h));

    let chars = app.editor.chars();
    let sel = app.editor.selection();
    let hl = Style::default().add_modifier(Modifier::REVERSED);
    let prompt = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let empty = chars.is_empty() && !app.busy;
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .skip(app.input_scroll)
        .take(h)
        .map(|(i, r)| {
            let seg = |a: usize, b: usize| -> String { chars[a..b].iter().collect() };
            let mut spans = vec![if i == 0 {
                Span::styled("> ", prompt)
            } else {
                Span::raw("  ")
            }];
            if empty && i == 0 {
                spans.push(Span::styled(
                    if updating.is_some() {
                        "Messages are paused until Concordia restarts on the new build"
                    } else if app.workspace.focused.is_some() {
                        "File pane has focus. Esc returns to chat."
                    } else {
                        "Message Concordia...  (/ for commands)"
                    },
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ));
                return Line::from(spans);
            }
            match sel.map(|(a, b)| (a.max(r.start), b.min(r.end))) {
                Some((a, b)) if a < b => {
                    spans.push(Span::raw(seg(r.start, a)));
                    spans.push(Span::styled(seg(a, b), hl));
                    spans.push(Span::raw(seg(b, r.end)));
                }
                _ => spans.push(Span::raw(seg(r.start, r.end))),
            }
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(block), area);
    if app.areas.bottom_button.width > 0 {
        f.render_widget(
            Paragraph::new(BOTTOM_LABEL).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            app.areas.bottom_button,
        );
    }

    // Real terminal caret (blinks natively, works with IME); hidden while a picker is open.
    if app.picker.is_none()
        && app.activity.detail.is_none()
        && !app.queue.focused
        && app.workspace.focused.is_none()
        && cur_row >= app.input_scroll
        && cur_row < app.input_scroll + h
    {
        let x = inner.x + (PROMPT_W + cur_col).min(inner.width.saturating_sub(1) as usize) as u16;
        let y = inner.y + (cur_row - app.input_scroll) as u16;
        f.set_cursor_position(Position::new(x, y));
    }
}

fn draw_footer(f: &mut Frame, area: Rect, app: &mut App) {
    app.areas.footer = area;
    let w = area.width as usize;
    let mode = app.trust;
    let label = if mode == TrustMode::Full {
        "full \u{26a0}"
    } else {
        mode_label(mode)
    };
    let color = mode_color(mode);
    // The mode column is kept only while it leaves room for the first hint.
    const MIN_LEFT: usize = 12;
    let right = [
        (100, format!("permissions: {label} ▾ ")),
        (0, format!("{label} ▾ ")),
    ]
    .into_iter()
    .find(|(min_width, r)| w >= *min_width && r.chars().count() + MIN_LEFT <= w)
    .map(|(_, r)| r)
    .unwrap_or_default();
    let rw = right.chars().count() as u16;
    let cols = [
        Rect {
            width: area.width - rw,
            ..area
        },
        Rect {
            x: area.x + area.width - rw,
            width: rw,
            ..area
        },
    ];
    app.areas.permissions = cols[1];
    let flash = app
        .flash
        .as_ref()
        .filter(|(_, at)| at.elapsed() < FLASH_FOR)
        .map(|(m, _)| m.as_str());
    let job_line;
    let idle_footer_hint = format!("{FOOTER_HINT} \u{b7} Ctrl+K options");
    let scrolled = format!("\u{2191} {} rows up \u{b7} PgDn to follow", app.view.scroll);
    let recovery_hint = format!(
        "{} saved drafts/requests · /recover",
        app.keep.recovery_choices.len()
    );
    let (left, left_style) = if app.recovery_warning().is_some() {
        (
            "draft not saved locally · /recover retry",
            Style::default().fg(Color::Red),
        )
    } else if let Some(warning) = app.store_warning.as_deref() {
        (warning, Style::default().fg(Color::Red))
    } else if let Some(m) = flash {
        (m, Style::default().fg(Color::Indexed(179)))
    } else if app
        .picker
        .as_ref()
        .is_some_and(|p| p.kind == PickerKind::Update)
        && app.job.is_some()
    {
        (
            "\u{2191}\u{2193} scroll \u{b7} Esc cancels",
            Style::default().fg(Color::DarkGray),
        )
    } else if let Some(job) = &app.job {
        let what = match job.kind {
            JobKind::Update => "updating",
            JobKind::Check => "checking",
        };
        let line = job.status();
        let shown: String = line.chars().take(60).collect();
        job_line = format!(
            "{} {what} \u{b7} {}{}{} \u{b7} Esc cancels",
            spinner(app.tick_count),
            crate::delegate::fmt_elapsed(job.started.elapsed()),
            if shown.is_empty() { "" } else { " \u{b7} " },
            shown
        );
        (job_line.as_str(), Style::default().fg(Color::Cyan))
    } else if app
        .picker
        .as_ref()
        .is_some_and(|p| p.kind == PickerKind::Update)
        && app
            .update_preview
            .as_ref()
            .is_some_and(|p| p.install_available)
    {
        if app.update_blocked_by_work() {
            (
                "Update blocked: task running · Esc returns to task",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            (
                "Enter installs and restarts this tab · ↑↓ scroll · Esc close",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
        }
    } else if app
        .picker
        .as_ref()
        .is_some_and(|p| p.kind == PickerKind::Update)
        && app.job.is_none()
        && app.update_preview.is_some()
    {
        ("Enter / Esc close", Style::default().fg(Color::DarkGray))
    } else if !app.palette_entries().is_empty() {
        (
            "\u{2191}\u{2193} select \u{b7} Tab complete \u{b7} Enter run \u{b7} Esc close",
            Style::default().fg(Color::DarkGray),
        )
    } else if app.has_selection() {
        (
            "right-click or Ctrl+C to copy \u{b7} click to clear",
            Style::default().fg(Color::Indexed(179)),
        )
    } else if app.view.scroll > 0 {
        (scrolled.as_str(), Style::default().fg(Color::Indexed(179)))
    } else if app.busy {
        (
            if app.idle_notice.is_some() {
                "Tab queue \u{b7} Ctrl+O expand \u{b7} Ctrl+C cancel \u{b7} Ctrl+K options"
            } else {
                "Tab queue \u{b7} Ctrl+O expand \u{b7} Ctrl+C cancel"
            },
            Style::default().fg(Color::DarkGray),
        )
    } else if !app.keep.recovery_choices.is_empty() && app.picker.is_none() {
        (recovery_hint.as_str(), Style::default().fg(Color::DarkGray))
    } else {
        (
            if app.idle_notice.is_some() && app.picker.is_none() {
                idle_footer_hint.as_str()
            } else {
                app.footer_left.as_str()
            },
            Style::default().fg(Color::DarkGray),
        )
    };
    let left = if app.workspace.focused.is_some() && app.picker.is_none() {
        "File pane · Esc chat · F6 change focus · Ctrl+S save"
    } else {
        left
    };
    let focus_hint = queue_focus::focus_hint(app).map(|hint| format!("{left} · {hint}"));
    let left = focus_hint.as_deref().unwrap_or(left);
    let stats_reserve = app
        .stats
        .as_ref()
        .and_then(|h| stats_ui::fit_stats(&h.latest(), (cols[0].width as usize).saturating_sub(20)))
        .map(|s| s.chars().count() + 3)
        .unwrap_or(0);
    let avail = (cols[0].width as usize).saturating_sub(2 + stats_reserve);
    let shown = if left == FOOTER_HINT || left == idle_footer_hint {
        fit_hints(avail)
    } else {
        fit_segments(left, avail)
    };
    f.render_widget(
        Paragraph::new(format!(" {shown}")).style(left_style),
        cols[0],
    );
    stats_ui::draw_footer_stats(f, cols[0], 1 + shown.chars().count(), app);
    // Approvals and sandboxing are bypassed in full mode: its red glyph must stay visible.
    if rw > 0 {
        f.render_widget(
            Paragraph::new(right)
                .alignment(Alignment::Right)
                .style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
            cols[1],
        );
    }
}
