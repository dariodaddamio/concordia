//! Footer process stats (`mem 41 MB - cpu 1%  + agents 380 MB - 6% (2)`) and the `/stats` toggle.
//!
//! The sampler thread lives in `sysstats`; this only reads its last reading on the UI thread.

use super::App;
use crate::sysstats::{footer_parts, Stats, StatsHandle};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

const GAP: usize = 2;

impl App {
    /// `/stats`: switch the footer figures (and the sampler thread) on or off for this session.
    pub(super) fn toggle_stats(&mut self) {
        if self.stats.take().is_some() {
            self.flash("stats off");
        } else {
            self.stats = Some(StatsHandle::start());
            self.flash("stats on - first reading in a moment");
        }
    }
}

/// The text to show given the room left of the hints: both segments, own only, or nothing.
pub(super) fn fit_stats(stats: &Stats, room: usize) -> Option<String> {
    if !stats.ready {
        return None;
    }
    let (own, agents) = footer_parts(stats);
    if let Some(a) = agents {
        let both = format!("{own}  {a}");
        if both.chars().count() <= room {
            return Some(both);
        }
    }
    (own.chars().count() <= room).then_some(own)
}

/// Draw the stats right-aligned in the footer's left column, only in the space the hints did
/// not use (`used` cells including the leading space), so hints are never dropped for them.
pub(super) fn draw_footer_stats(f: &mut Frame, col: Rect, used: usize, app: &App) {
    let Some(handle) = &app.stats else {
        return;
    };
    let room = (col.width as usize).saturating_sub(used + GAP + 1);
    let Some(text) = fit_stats(&handle.latest(), room) else {
        return;
    };
    f.render_widget(
        Paragraph::new(format!("{text} "))
            .alignment(Alignment::Right)
            .style(
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            ),
        col,
    );
}
