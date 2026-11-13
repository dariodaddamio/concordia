//! The icon set for tabs and tool cards. Every glyph is one cell wide and is drawn by Consolas
//! or the Segoe UI Symbol fallback in the native window; the shortcuts screen and the docs list
//! the same set from here.

use crate::provider::stream_cards::ToolKind;
use crate::store::ProviderKind;
use ratatui::style::Color;

pub(super) fn provider(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Codex => "⬡",
        ProviderKind::Claude => "✻",
        ProviderKind::Cursor => "➤",
        ProviderKind::Grok => "⊘",
        ProviderKind::Opencode => "◇",
    }
}

pub(super) fn provider_name(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Codex => "Codex",
        ProviderKind::Claude => "Claude Code",
        ProviderKind::Cursor => "Cursor",
        ProviderKind::Grok => "Grok",
        ProviderKind::Opencode => "opencode",
    }
}

/// What a tab shows next to its provider glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabStatus {
    Idle,
    Running,
    Waiting,
    Done,
    Failed,
}

const SPINNER: [&str; 4] = ["◐", "◓", "◑", "◒"];

impl TabStatus {
    pub(super) const LEGEND: [TabStatus; 4] = [
        TabStatus::Running,
        TabStatus::Waiting,
        TabStatus::Done,
        TabStatus::Failed,
    ];

    /// `tick` advances the running animation; every other status is still.
    pub(super) fn icon(self, tick: u64) -> &'static str {
        match self {
            Self::Idle => " ",
            Self::Running => SPINNER[(tick % SPINNER.len() as u64) as usize],
            Self::Waiting => "●",
            Self::Done => "✓",
            Self::Failed => "✗",
        }
    }

    pub(super) fn color(self) -> Color {
        match self {
            Self::Idle => Color::Gray,
            Self::Running => Color::Cyan,
            Self::Waiting => Color::Yellow,
            Self::Done => Color::Green,
            Self::Failed => Color::Red,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running (animated)",
            Self::Waiting => "waiting for you",
            Self::Done => "finished while you were in another tab",
            Self::Failed => "failed while you were in another tab",
        }
    }
}

/// Rows of (glyph, meaning) for the shortcuts screen and docs.
pub(super) fn legend() -> Vec<(&'static str, Vec<(String, &'static str)>)> {
    vec![
        (
            "Tab providers",
            [
                ProviderKind::Codex,
                ProviderKind::Claude,
                ProviderKind::Cursor,
                ProviderKind::Grok,
                ProviderKind::Opencode,
            ]
            .into_iter()
            .map(|kind| (provider(kind).to_owned(), provider_name(kind)))
            .collect(),
        ),
        (
            "Tab status",
            TabStatus::LEGEND
                .into_iter()
                .map(|status| {
                    let glyph = if status == TabStatus::Running {
                        SPINNER.concat()
                    } else {
                        status.icon(0).to_owned()
                    };
                    (glyph, status.name())
                })
                .collect(),
        ),
        (
            "Tool cards",
            ToolKind::LEGEND
                .into_iter()
                .map(|kind| (kind.icon().to_owned(), kind.name()))
                .chain([
                    ("◉".to_owned(), "tool running"),
                    ("✓".to_owned(), "tool finished"),
                    ("✗".to_owned(), "tool failed"),
                ])
                .collect(),
        ),
    ]
}

/// Every glyph in the set, for width and font coverage checks.
#[cfg(test)]
pub(super) fn all_glyphs() -> Vec<String> {
    legend()
        .into_iter()
        .flat_map(|(_, rows)| rows.into_iter().map(|(glyph, _)| glyph))
        .flat_map(|glyph| glyph.chars().map(String::from).collect::<Vec<_>>())
        .chain([ToolKind::Other.icon().to_owned()])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Span;

    #[test]
    fn every_icon_is_one_cell_and_distinct_within_its_group() {
        for glyph in all_glyphs() {
            assert_eq!(Span::raw(glyph.as_str()).width(), 1, "{glyph:?} is not one cell");
        }
        for (group, rows) in legend() {
            let mut glyphs: Vec<_> = rows.iter().map(|(glyph, _)| glyph.clone()).collect();
            glyphs.sort();
            glyphs.dedup();
            assert_eq!(glyphs.len(), rows.len(), "{group} reuses a glyph");
        }
    }

    #[cfg(windows)]
    #[test]
    fn every_icon_is_drawn_by_the_native_window_fonts() {
        let missing = super::super::native_window::glyphs_without_font(&all_glyphs());
        assert!(missing.is_empty(), "no Consolas or Segoe UI Symbol glyph: {missing:?}");
    }
}
