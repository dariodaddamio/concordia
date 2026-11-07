//! One semantic palette for terminal and native cells; image pixels remain source pixels.
use crate::config::{contrast, luminance, Rgb, Theme, Ui};
use ratatui::{
    buffer::Buffer,
    style::{Color, Modifier},
};
use std::collections::HashMap;
#[cfg(not(test))]
use std::sync::OnceLock;

#[derive(Clone, Copy)]
struct Palette {
    enabled: bool,
    terminal: bool,
    background: Rgb,
    foreground: Rgb,
    accent: Rgb,
    brand_color: Rgb,
    motion: bool,
    custom_background: Option<Rgb>,
    custom_foreground: Option<Rgb>,
    custom_accent: Option<Rgb>,
    panel: Rgb,
    muted: Rgb,
    success: Rgb,
    warning: Rgb,
    error: Rgb,
    purple: Rgb,
    added: Rgb,
    removed: Rgb,
}

fn color(value: Rgb) -> Color {
    Color::Rgb(value[0], value[1], value[2])
}

fn mix(first: Rgb, second: Rgb, amount: f64) -> Rgb {
    std::array::from_fn(|index| {
        (f64::from(first[index]) * (1.0 - amount) + f64::from(second[index]) * amount).round() as u8
    })
}

fn readable(foreground: Rgb, background: Rgb, minimum: f64) -> Rgb {
    if contrast(foreground, background) >= minimum {
        return foreground;
    }
    let ink = if contrast([0, 0, 0], background) > contrast([255, 255, 255], background) {
        [0, 0, 0]
    } else {
        [255, 255, 255]
    };
    for step in 1..=20 {
        let candidate = mix(foreground, ink, f64::from(step) / 20.0);
        if contrast(candidate, background) >= minimum {
            return candidate;
        }
    }
    ink
}

impl Palette {
    fn from_ui(ui: &Ui) -> Self {
        let [mut background, mut foreground, mut accent] = ui.theme.colors();
        if let Some(theme) = &ui.library_theme {
            background = theme.background;
            foreground = theme.foreground;
            accent = theme.accent;
        }
        let background = ui.background.unwrap_or(background);
        let foreground = readable(ui.foreground.unwrap_or(foreground), background, 4.5);
        let accent = readable(ui.accent.unwrap_or(accent), background, 4.5);
        let light = luminance(background) > 0.35;
        let semantic = |dark: Rgb, light_color: Rgb| {
            readable(if light { light_color } else { dark }, background, 4.5)
        };
        let success = ui.library_theme.as_ref().map_or_else(
            || semantic([104, 197, 139], [23, 111, 57]),
            |t| readable(t.success, background, 4.5),
        );
        let error = semantic([239, 118, 129], [176, 40, 57]);
        Self {
            enabled: !matches!(ui.theme, Theme::Default | Theme::Terminal)
                || ui.background.is_some()
                || ui.foreground.is_some()
                || ui.accent.is_some(),
            terminal: ui.theme == Theme::Terminal,
            background,
            foreground,
            accent,
            brand_color: ui.brand_color.unwrap_or([255, 176, 0]),
            motion: ui.motion,
            custom_background: ui.background,
            custom_foreground: ui.foreground,
            custom_accent: ui.accent,
            panel: mix(background, foreground, if light { 0.065 } else { 0.11 }),
            muted: readable(mix(background, foreground, 0.62), background, 4.5),
            success,
            warning: ui.library_theme.as_ref().map_or_else(
                || semantic([230, 193, 111], [136, 89, 15]),
                |t| readable(t.warning, background, 4.5),
            ),
            error: ui
                .library_theme
                .as_ref()
                .map_or(error, |t| readable(t.error, background, 4.5)),
            purple: semantic([193, 155, 234], [119, 68, 164]),
            added: mix(background, success, if light { 0.12 } else { 0.16 }),
            removed: mix(background, error, if light { 0.11 } else { 0.16 }),
        }
    }

    fn background(&self, value: Color) -> Color {
        if self.terminal {
            return if value == Color::Reset {
                self.custom_background.map(color).unwrap_or(value)
            } else {
                value
            };
        }
        match value {
            Color::Reset | Color::Black | Color::Rgb(20, 23, 31) => color(self.background),
            Color::DarkGray | Color::Gray => color(self.panel),
            Color::Rgb(25, 48, 38) => color(self.added),
            Color::Rgb(54, 31, 36) => color(self.removed),
            Color::Rgb(r, g, b) => color(mix(self.background, [r, g, b], 0.18)),
            other => color(self.semantic(other)),
        }
    }

    fn semantic(&self, value: Color) -> Rgb {
        match value {
            Color::Reset | Color::White => self.foreground,
            Color::Gray | Color::DarkGray => self.muted,
            Color::Cyan | Color::LightCyan | Color::Blue | Color::LightBlue => self.accent,
            Color::Green | Color::LightGreen => self.success,
            Color::Red | Color::LightRed => self.error,
            Color::Yellow | Color::LightYellow => self.warning,
            Color::Magenta | Color::LightMagenta => self.purple,
            Color::Black => [12, 12, 12],
            Color::Rgb(r, g, b) => [r, g, b],
            Color::Indexed(index) if index >= 232 => {
                let level = 8 + (index - 232) * 10;
                [level, level, level]
            }
            Color::Indexed(index) if index >= 16 => {
                let level = |value| if value == 0 { 0 } else { 55 + value * 40 };
                let value = index - 16;
                [level(value / 36), level(value / 6 % 6), level(value % 6)]
            }
            Color::Indexed(index) => match index {
                0 => [12, 12, 12],
                1 | 9 => self.error,
                2 | 10 => self.success,
                3 | 11 => self.warning,
                4 | 6 | 12 | 14 => self.accent,
                5 | 13 => self.purple,
                _ => self.foreground,
            },
        }
    }

    fn apply(&self, buffer: &mut Buffer, no_color: bool, native: bool) {
        if no_color {
            for cell in &mut buffer.content {
                cell.fg = Color::Reset;
                cell.bg = Color::Reset;
            }
            return;
        }
        if !self.enabled {
            return;
        }
        // Syntax colors repeat across thousands of cells; adjust each color/background once.
        let mut foregrounds = HashMap::new();
        for cell in &mut buffer.content {
            if cell.symbol() == " " && cell.fg == Color::Rgb(20, 23, 31) && cell.bg == cell.fg {
                // Keep native image-overlay blanks identifiable after semantic recoloring.
                cell.fg = color(self.background);
                cell.bg = color(self.background);
                continue;
            }
            // Half-block cell previews represent image data, not UI semantic colors.
            if cell.symbol() == "▀"
                && matches!(cell.fg, Color::Rgb(..))
                && matches!(cell.bg, Color::Rgb(..))
            {
                continue;
            }
            cell.bg = self.background(cell.bg);
            if self.terminal {
                cell.fg = match cell.fg {
                    Color::Reset => self
                        .custom_foreground
                        .map(|value| color(if native { self.foreground } else { value }))
                        .unwrap_or(Color::Reset),
                    Color::Cyan | Color::LightCyan => self
                        .custom_accent
                        .map(|value| color(if native { self.accent } else { value }))
                        .unwrap_or(cell.fg),
                    other => other,
                };
                continue;
            }
            let dim = cell.modifier.contains(Modifier::DIM);
            let key = (cell.fg, cell.bg, dim);
            cell.fg = *foregrounds.entry(key).or_insert_with(|| {
                let Color::Rgb(r, g, b) = cell.bg else {
                    unreachable!()
                };
                let background = [r, g, b];
                let foreground = self.semantic(cell.fg);
                let foreground = if dim {
                    mix(background, foreground, 0.68)
                } else {
                    foreground
                };
                color(readable(foreground, background, 4.5))
            });
            // Muted RGB already carries DIM; applying native GDI dimming again loses contrast.
            if dim {
                cell.modifier.remove(Modifier::DIM);
            }
        }
    }
}

#[cfg(not(test))]
fn palette_slot() -> &'static std::sync::RwLock<Palette> {
    static PALETTE: OnceLock<std::sync::RwLock<Palette>> = OnceLock::new();
    PALETTE.get_or_init(|| std::sync::RwLock::new(Palette::from_ui(&crate::config::get().ui)))
}

#[cfg(not(test))]
fn current() -> Palette {
    *palette_slot()
        .read()
        .unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
thread_local! {
    static TEST_PALETTE: std::cell::RefCell<Palette> = std::cell::RefCell::new(Palette::from_ui(&crate::config::Config::default().ui));
}

#[cfg(test)]
fn current() -> Palette {
    TEST_PALETTE.with(|slot| *slot.borrow())
}

/// Tests set `no_color` explicitly so a developer shell's `NO_COLOR` cannot change expectations.
pub(crate) fn env_no_color() -> bool {
    !cfg!(test) && std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

pub(super) fn install(ui: &Ui) {
    let mut palette = if env_no_color() {
        Palette::from_ui(&crate::config::Config::default().ui)
    } else {
        Palette::from_ui(ui)
    };
    palette.motion = ui.motion;
    #[cfg(not(test))]
    {
        *palette_slot()
            .write()
            .unwrap_or_else(|error| error.into_inner()) = palette;
    }
    #[cfg(test)]
    TEST_PALETTE.with(|slot| *slot.borrow_mut() = palette);
}

#[cfg(windows)]
pub(super) fn brand_color() -> Rgb {
    current().brand_color
}

#[cfg(windows)]
pub(super) fn motion_enabled() -> bool {
    current().motion
}

pub(super) fn apply(buffer: &mut Buffer, no_color: bool, native: bool) {
    current().apply(buffer, no_color, native);
}

#[cfg(windows)]
pub(super) fn image_marker() -> Color {
    // Native overlays are collected before the frame-wide theme pass. Keep this
    // marker stable so an installed theme cannot make a valid overlay invisible.
    Color::Rgb(20, 23, 31)
}

pub(super) fn image_background() -> Rgb {
    let palette = current();
    if palette.enabled && (!palette.terminal || palette.custom_background.is_some()) {
        palette.background
    } else {
        [28, 28, 28]
    }
}

/// Native Reset colors also fill the client margin outside the whole-cell grid.
#[cfg(windows)]
pub(super) fn native_default(foreground: bool) -> Rgb {
    let palette = current();
    if foreground {
        palette.foreground
    } else {
        palette.background
    }
}

#[cfg(test)]
pub(super) fn fixture_proof() {
    use ratatui::{layout::Rect, style::Style};
    let mut buffer = Buffer::empty(Rect::new(0, 0, 7, 1));
    buffer[(0, 0)]
        .set_symbol("a")
        .set_style(Style::default().add_modifier(Modifier::BOLD));
    buffer[(1, 0)]
        .set_symbol("+")
        .set_fg(Color::Rgb(181, 189, 104))
        .set_bg(Color::Rgb(25, 48, 38));
    buffer[(2, 0)]
        .set_symbol("-")
        .set_fg(Color::Indexed(252))
        .set_bg(Color::Rgb(54, 31, 36));
    buffer[(3, 0)].set_symbol("d").set_style(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
    );
    buffer[(4, 0)]
        .set_symbol("▀")
        .set_fg(Color::Rgb(220, 15, 33))
        .set_bg(Color::Rgb(42, 73, 205));
    buffer[(5, 0)].set_symbol("c").set_fg(Color::Cyan);
    buffer[(6, 0)]
        .set_fg(Color::Rgb(20, 23, 31))
        .set_bg(Color::Rgb(20, 23, 31));
    let original = buffer.clone();
    let (config, warnings) = crate::config::Config::parse("[ui]\ntheme='default'\n");
    assert!(warnings.is_empty());
    Palette::from_ui(&config.ui).apply(&mut buffer, false, false);
    assert_eq!(
        buffer, original,
        "default theme preserves existing styles and host defaults"
    );
    for name in ["dark", "light", "opifex"] {
        let (config, warnings) = crate::config::Config::parse(&format!("[ui]\ntheme='{name}'\n"));
        assert!(warnings.is_empty());
        let palette = Palette::from_ui(&config.ui);
        let mut themed = original.clone();
        palette.apply(&mut themed, false, false);
        assert!(themed[(0, 0)].modifier.contains(Modifier::BOLD));
        assert_eq!(
            themed[(4, 0)],
            original[(4, 0)],
            "image pixels must be unchanged"
        );
        assert_eq!(themed[(0, 0)].bg, color(palette.background));
        assert_eq!(themed[(6, 0)].fg, color(palette.background));
        assert_eq!(themed[(6, 0)].bg, color(palette.background));
        assert_ne!(themed[(1, 0)].bg, themed[(2, 0)].bg);
        assert_ne!(
            themed[(1, 0)].fg,
            themed[(2, 0)].fg,
            "syntax colors survive"
        );
        assert_ne!(
            themed[(3, 0)].fg,
            themed[(0, 0)].fg,
            "muted text stays distinct"
        );
        for index in [0, 1, 2, 3, 5] {
            let Color::Rgb(r, g, b) = themed[(index, 0)].fg else {
                panic!("foreground")
            };
            let Color::Rgb(br, bg, bb) = themed[(index, 0)].bg else {
                panic!("background")
            };
            assert!(contrast([r, g, b], [br, bg, bb]) >= 4.5);
        }
        palette.apply(&mut themed, true, false);
        assert!(themed
            .content
            .iter()
            .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset));
        assert!(themed[(0, 0)].modifier.contains(Modifier::BOLD));
    }
    let (config, warnings) =
        crate::config::Config::parse("[ui]\ntheme='terminal'\naccent='#72b9fa'\n");
    assert!(warnings.is_empty());
    let mut terminal = original.clone();
    Palette::from_ui(&config.ui).apply(&mut terminal, false, false);
    assert_eq!(
        terminal[(0, 0)].bg,
        Color::Reset,
        "terminal theme retains host background"
    );
    assert_eq!(terminal[(5, 0)].fg, Color::Rgb(114, 185, 250));
    let (config, warnings) = crate::config::Config::parse(
        "[ui]\nbackground='#fbf9f1'\nforeground='#232729'\naccent='#1b5a9e'\n",
    );
    assert!(warnings.is_empty());
    let mut custom = original;
    Palette::from_ui(&config.ui).apply(&mut custom, false, false);
    assert_eq!(custom[(0, 0)].bg, Color::Rgb(251, 249, 241));
    assert_eq!(custom[(0, 0)].fg, Color::Rgb(35, 39, 41));
    assert_eq!(custom[(5, 0)].fg, Color::Rgb(27, 90, 158));
    let (config, warnings) =
        crate::config::Config::parse("[ui]\ntheme='terminal'\nforeground='#111111'\n");
    assert!(warnings.is_empty());
    let palette = Palette::from_ui(&config.ui);
    let mut native = Buffer::empty(Rect::new(0, 0, 1, 1));
    palette.apply(&mut native, false, true);
    assert_eq!(
        native[(0, 0)].fg,
        color(palette.foreground),
        "native has a known background even with terminal preset"
    );
}
