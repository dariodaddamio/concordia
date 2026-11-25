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
    let hero = Rect::new(area.x + 1, area.y + 1, 14, 7);
    raster::draw_brand(f, hero, app, pixels);
    let text = Rect::new(
        hero.right() + 2,
        hero.y,
        area.width.saturating_sub(18),
        hero.height,
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
    app.areas.home_documentation = push_documentation(&mut lines, text);
    lines.push(Line::from("F1  Shortcuts"));
    f.render_widget(Paragraph::new(lines), text);
    Rect::new(
        area.x,
        area.y + 8,
        area.width,
        area.height.saturating_sub(8),
    )
}

pub(super) fn push_documentation(lines: &mut Vec<Line<'_>>, area: Rect) -> Option<Rect> {
    const LABEL: &str = "Documentation";
    let row = lines.len();
    lines.push(Line::from(Span::styled(
        LABEL,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::UNDERLINED),
    )));
    (row < usize::from(area.height) && usize::from(area.width) >= LABEL.len())
        .then(|| Rect::new(area.x, area.y + row as u16, LABEL.len() as u16, 1))
}

pub(super) fn whats_new(count: usize, width: usize) -> Vec<String> {
    include_str!("../../../../CHANGELOG.md")
        .split("\n### ")
        .filter_map(|entry| {
            let mut lines = entry.lines();
            let first = lines.find_map(|line| line.strip_prefix("- "))?;
            let mut bullet = first.to_owned();
            for continuation in
                lines.take_while(|line| line.starts_with("  ") && !line.trim().is_empty())
            {
                bullet.push(' ');
                bullet.push_str(continuation.trim());
            }
            Some(super::draw::ellipsis(
                &format!("· {}", bullet.replace('`', "")),
                width,
            ))
        })
        .take(count)
        .collect()
}
