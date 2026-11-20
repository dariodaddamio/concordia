//! A compact, local-only Concordia portrait on roomy Home screens.
use super::*;
use std::sync::{Arc, OnceLock};

pub(super) fn portrait() -> Option<Arc<image::RgbaImage>> {
    static PORTRAIT: OnceLock<Option<Arc<image::RgbaImage>>> = OnceLock::new();
    PORTRAIT
        .get_or_init(|| {
            image::load_from_memory_with_format(
                include_bytes!("../../assets/concordia.png"),
                image::ImageFormat::Png,
            )
            .ok()
            .map(|image| Arc::new(image.into_rgba8()))
        })
        .clone()
}

pub(super) fn draw(f: &mut Frame, area: Rect, app: &mut App) -> Rect {
    if area.width < 76 || area.height < 24 {
        return area;
    }
    let Some(pixels) = portrait() else {
        return area;
    };
    let hero = Rect::new(area.x + 1, area.y, 14, 6);
    raster::draw_brand(f, hero, app, pixels);
    let text = Rect::new(
        hero.right() + 2,
        area.y + 1,
        area.width.saturating_sub(18),
        7,
    );
    let mut lines = vec![Line::from(Span::styled(
        "Concordia",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::from(Span::styled(
        "What's new",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.extend(
        whats_new(3, text.width as usize)
            .into_iter()
            .map(|line| Line::styled(line, Style::default().fg(Color::Gray))),
    );
    lines.push(Line::from(Span::styled(
        "Documentation",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::UNDERLINED),
    )));
    lines.push(Line::from("F1  Shortcuts"));
    f.render_widget(Paragraph::new(lines), text);
    app.areas.home_documentation = Some(Rect::new(
        text.x,
        text.y + 5,
        "Documentation".len() as u16,
        1,
    ));
    Rect::new(
        area.x,
        area.y + 8,
        area.width,
        area.height.saturating_sub(8),
    )
}

/// The newest changes, one short line each: the first bullet of each changelog entry, not the
/// entry heading (headings are only a kind and a build stamp).
pub(super) fn whats_new(count: usize, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_entry = false;
    for line in include_str!("../../../../CHANGELOG.md").lines() {
        if line.starts_with("### ") {
            in_entry = true;
            continue;
        }
        if in_entry {
            if let Some(text) = line.strip_prefix("- ") {
                let text = text.replace('`', "");
                let first = text
                    .split(". ")
                    .next()
                    .unwrap_or(&text)
                    .trim_end_matches('.');
                let max = width.saturating_sub(2).max(8);
                let short: String = if first.chars().count() > max {
                    first
                        .chars()
                        .take(max.saturating_sub(1))
                        .collect::<String>()
                        + "…"
                } else {
                    first.to_string()
                };
                out.push(format!("· {short}"));
                in_entry = false;
                if out.len() == count {
                    break;
                }
            }
        }
    }
    out
}
