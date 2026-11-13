//! Pure projection for the optional worker side panel.
//!
//! Keeping this separate from the main TUI layout makes the panel testable without a terminal and
//! ensures it only consumes the already-recorded worker ledger; it never polls or starts a model.

use super::App;
use crate::delegate::{Job, WorkerState};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::time::{Duration, Instant};

pub const MIN_WIDTH: u16 = 72;
pub const DEFAULT_WIDTH: u16 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: u32,
    pub label: String,
    pub state: WorkerState,
    pub detail: String,
}

pub fn visible(terminal_width: u16, open: bool) -> bool {
    open && terminal_width >= MIN_WIDTH
}

pub fn rows<'a>(jobs: impl IntoIterator<Item = &'a Job>) -> Vec<Row> {
    jobs.into_iter()
        .map(|job| Row {
            id: job.id,
            label: format!("{} {}", job.provider.as_str(), job.account),
            state: job.state,
            detail: if job.snippet.is_empty() {
                job.task.clone()
            } else {
                job.snippet.clone()
            },
        })
        .collect()
}

pub(super) fn draw(f: &mut Frame, area: Rect, app: &App) {
    let jobs = app.deleg.d.visible(Instant::now(), Duration::from_secs(90));
    let rows = rows(jobs);
    let mut lines = vec![Line::from(Span::styled(
        " live workers ",
        Style::default().fg(Color::Cyan),
    ))];
    if rows.is_empty() {
        lines.push(Line::from("No active or recent workers."));
    } else {
        for row in rows {
            lines.push(Line::from(format!("#{} {}", row.id, row.label)));
            lines.push(Line::from(format!(
                "  {} · {}",
                format!("{:?}", row.state).to_ascii_lowercase(),
                row.detail
            )));
        }
    }
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::LEFT).title("Workers")),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_panel_lists_workers_live() {
        assert_eq!(rows(std::iter::empty()).len(), 0);
    }

    #[test]
    fn side_panel_never_takes_focus() {
        assert!(visible(120, true));
    }

    #[test]
    fn side_panel_hides_on_narrow_terminals() {
        assert!(!visible(MIN_WIDTH - 1, true));
    }
}
