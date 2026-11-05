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

pub(super) fn draw(f: &mut Frame, area: Rect, app: &App) -> Rect {
    if area.width < 76 || area.height < 24 {
        return area;
    }
    let motion = app.appearance.ui.motion;
    #[cfg(windows)]
    let motion = motion && super::native_window::motion_allowed();
    let frame = if motion {
        (app.appearance.clock.elapsed().as_millis() / 80 % 180) as u64
    } else {
        0
    };
    let pixels = {
        let mut cached = app.appearance.frame.borrow_mut();
        if cached.as_ref().is_none_or(|(key, _)| *key != frame) {
            *cached = Some((
                frame,
                Arc::new(brand_model::render(
                    frame,
                    app.appearance.color(),
                    app.appearance.ui.theme == crate::config::Theme::Opifex && motion,
                )),
            ));
        }
        cached.as_ref().unwrap().1.clone()
    };
    let hero = Rect::new(area.x + 1, area.y, 14, 6);
    raster::draw_brand(f, hero, app, pixels);
    let text = Rect::new(
        hero.right() + 2,
        area.y + 1,
        area.width.saturating_sub(18),
        4,
    );
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Concordia",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Continue a chat or start a new one.",
                Style::default().fg(Color::DarkGray),
            )),
        ]),
        text,
    );
    Rect::new(area.x, area.y + 7, area.width, area.height - 7)
}
