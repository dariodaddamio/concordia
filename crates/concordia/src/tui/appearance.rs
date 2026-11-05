//! User-facing appearance controls. Never rewrites the hand-edited config.toml.
use super::*;
use crate::config::{Rgb, Theme, Ui};
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    version: u32,
    theme: Option<String>,
    brand_color: Option<Rgb>,
    motion: Option<bool>,
    token_line: Option<bool>,
    tips: Option<bool>,
    stats: Option<bool>,
}

pub(super) struct State {
    pub ui: Ui,
    pub notice: Option<String>,
}

fn read(root: &Path) -> io::Result<Saved> {
    let path = root.join(".ao/appearance.json");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Saved::default()),
        Err(error) => return Err(error),
    };
    if !metadata.is_file() || metadata.len() > 16 * 1024 {
        return Err(io::Error::other(
            "appearance settings must be a regular file below 16 KiB",
        ));
    }
    let saved: Saved = serde_json::from_slice(&std::fs::read(path)?)?;
    if saved.version != 1
        || saved
            .theme
            .as_deref()
            .is_some_and(|value| parse_theme(value).is_none())
    {
        return Err(io::Error::other(
            "unsupported appearance settings; preserved unchanged",
        ));
    }
    Ok(saved)
}

pub(super) fn parse_theme(value: &str) -> Option<Theme> {
    match value.to_ascii_lowercase().as_str() {
        "default" => Some(Theme::Default),
        "dark" => Some(Theme::Dark),
        "light" => Some(Theme::Light),
        "terminal" => Some(Theme::Terminal),
        "opifex" | "opifex studio" => Some(Theme::Opifex),
        _ => None,
    }
}

fn theme_name(theme: Theme) -> &'static str {
    match theme {
        Theme::Default => "Default",
        Theme::Dark => "Dark",
        Theme::Light => "Light",
        Theme::Terminal => "Terminal",
        Theme::Opifex => "Opifex Studio",
    }
}

pub(super) fn parse_color(value: &str) -> Option<Rgb> {
    let hex = value.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let number = u32::from_str_radix(hex, 16).ok()?;
    Some([(number >> 16) as u8, (number >> 8) as u8, number as u8])
}

fn color_text(color: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}

impl State {
    pub fn load(root: &Path) -> Self {
        let mut ui = crate::config::for_root(root).ui;
        let mut notice = None;
        match read(root) {
            Ok(saved) => {
                if let Some(theme) = saved.theme.as_deref().and_then(parse_theme) {
                    ui.theme = theme;
                    // An explicitly chosen preset replaces config palette overrides.
                    ui.background = None;
                    ui.foreground = None;
                    ui.accent = None;
                }
                if let Some(color) = saved.brand_color {
                    ui.brand_color = Some(color);
                }
                if let Some(tokens) = saved.token_line {
                    ui.token_line = tokens;
                }
                if let Some(tips) = saved.tips {
                    ui.tips = tips;
                }
                if let Some(stats) = saved.stats {
                    ui.stats = stats;
                }
                if let Some(motion) = saved.motion {
                    ui.motion = motion;
                }
            }
            Err(error) => notice = Some(format!("Appearance settings: {error}")),
        }
        theme::install(&ui);
        Self { ui, notice }
    }

    pub fn color(&self) -> Rgb {
        self.ui.brand_color.unwrap_or([255, 176, 0])
    }
}

fn row(id: &str, label: impl Into<String>, detail: impl Into<String>) -> PickerRow {
    PickerRow {
        id: id.into(),
        label: label.into(),
        detail: detail.into(),
    }
}

pub(super) fn open(app: &mut App) {
    let ui = &app.appearance.ui;
    let rows = vec![
        row(
            "theme",
            format!("Theme · {}", theme_name(ui.theme)),
            "Choose a palette; applies immediately",
        ),
        row(
            "color",
            format!("Concordia color · {}", color_text(app.appearance.color())),
            "Running-window icon; Home retains the original gold portrait",
        ),
        row(
            "defaults",
            "Use config defaults",
            "Clear appearance overrides; preserve config.toml",
        ),
    ];
    app.show_picker(PickerKind::Appearance, "appearance".into(), rows, false);
    if let Some(notice) = app.appearance.notice.take() {
        app.flash(notice);
    }
}

pub(super) fn save(app: &mut App, key: &str, value: &str) {
    let root = &app.state_root;
    let result = (|| -> io::Result<()> {
        std::fs::create_dir_all(root.join(".ao"))?;
        crate::state_io::with_lock(&root.join(".ao/appearance.lock"), || {
            let mut saved = read(root)?;
            match key {
                "theme" if parse_theme(value).is_some() => saved.theme = Some(value.into()),
                "color" => {
                    saved.brand_color = Some(
                        parse_color(value)
                            .ok_or_else(|| io::Error::other("use a color such as #FFB000"))?,
                    )
                }
                "motion" if matches!(value, "on" | "off") => saved.motion = Some(value == "on"),
                "tokens" | "tips" | "stats" if matches!(value, "on" | "off") => {
                    let preference = Some(value == "on");
                    match key {
                        "tokens" => saved.token_line = preference,
                        "tips" => saved.tips = preference,
                        _ => saved.stats = preference,
                    }
                }
                "defaults" => {
                    saved.theme = None;
                    saved.brand_color = None;
                    saved.motion = None;
                }
                _ => {
                    return Err(io::Error::other(
                        "choose theme, color #RRGGBB, or motion on/off",
                    ))
                }
            }
            saved.version = 1;
            crate::state_io::write_atomic(
                &root.join(".ao/appearance.json"),
                &serde_json::to_vec_pretty(&saved)?,
            )
        })
    })();
    match result {
        Ok(()) => {
            app.appearance = State::load(root);
            if app.appearance.ui.stats != app.stats.is_some() {
                app.toggle_stats();
            }
            raster::begin_frame(app);
            app.flash("Appearance saved");
        }
        Err(error) => app.flash(format!("Appearance not saved: {error}")),
    }
    open(app);
}

pub(super) fn choose(app: &mut App, kind: PickerKind, id: &str) {
    match (kind, id) {
        (PickerKind::Appearance, "theme") => {
            let rows = [
                ("opifex", "Opifex Studio", "Near-black · amber · bone"),
                ("default", "Default", "Concordia's original palette"),
                ("dark", "Dark", "Cool black · blue accents"),
                ("light", "Light", "Warm white · dark ink"),
                ("terminal", "Terminal", "Follow your terminal's colors"),
            ]
            .into_iter()
            .map(|(id, title, detail)| row(id, title, detail))
            .collect();
            app.show_picker(PickerKind::Theme, "theme".into(), rows, false);
        }
        (PickerKind::Appearance, "color") => {
            let rows = [
                ("#FFB000", "Amber"),
                ("#F3CE79", "Gold"),
                ("#78BFFF", "Ice"),
                ("#A8E6BB", "Jade"),
                ("#C9A5FF", "Violet"),
                ("#FF8F9B", "Rose"),
            ]
            .into_iter()
            .map(|(color, title)| row(color, title, color))
            .chain(std::iter::once(row(
                "custom",
                "Custom color",
                "Enter any #RRGGBB value",
            )))
            .collect();
            app.show_picker(
                PickerKind::BrandColor,
                "Concordia color".into(),
                rows,
                false,
            );
        }
        (PickerKind::Appearance, "motion") => save(
            app,
            "motion",
            if app.appearance.ui.motion {
                "off"
            } else {
                "on"
            },
        ),
        (PickerKind::Appearance, "defaults") => save(app, "defaults", ""),
        (PickerKind::Theme, value) => save(app, "theme", value),
        (PickerKind::BrandColor, "custom") => {
            app.show_picker(
                PickerKind::BrandCustom,
                "color · type #RRGGBB".into(),
                vec![row(
                    "custom",
                    "Enter a hex color",
                    "Example: #FFB000 · Enter saves · Esc cancels",
                )],
                true,
            );
            app.footer_left = "Type #RRGGBB · Enter saves · Esc cancels".into();
        }
        (PickerKind::BrandColor, value) => save(app, "color", value),
        _ => {}
    }
}

pub(super) fn accept_custom(app: &mut App) {
    let value = app
        .picker
        .as_ref()
        .map(|picker| picker.filter.clone())
        .unwrap_or_default();
    if parse_color(&value).is_none() {
        app.flash("Use six hex digits, for example #FFB000");
        return;
    }
    app.dismiss_picker();
    save(app, "color", &value);
}

pub(super) fn command(app: &mut App, args: &str) {
    match args.split_once(' ') {
        Some(("theme", value)) => save(app, "theme", value.trim()),
        Some(("color", value)) => save(app, "color", value.trim()),
        Some(("motion", value)) => save(app, "motion", value.trim()),
        _ if args.is_empty() => open(app),
        _ => {
            app.flash("/config · /config theme opifex · /config color #FFB000 · /config motion off")
        }
    }
}
