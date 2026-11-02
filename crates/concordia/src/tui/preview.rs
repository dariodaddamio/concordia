//! Bounded, read-only Markdown preview in its own terminal. No browser or service required.
use super::*;
use ratatui::backend::CrosstermBackend;
use std::io::Read;

pub(super) fn run(path: &Path) -> Result<()> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= 2 * 1024 * 1024,
        "preview is limited to 2 MiB; open this file in your editor"
    );
    let text = String::from_utf8(bytes)
        .map_err(|_| anyhow::anyhow!("preview requires a UTF-8 text file"))?;
    let text: String = text
        .chars()
        .filter(|c| !c.is_control() || matches!(*c, '\n' | '\t'))
        .collect();
    enable_raw_mode()?;
    let _restore = Restore;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let mut view = TranscriptView::default();
    let mut first_draw = true;
    loop {
        terminal.draw(|f| {
            let parts = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(f.area());
            f.render_widget(
                Paragraph::new(format!(
                    " {} · read-only",
                    path.file_name().unwrap_or_default().to_string_lossy()
                )),
                parts[0],
            );
            view.rebuild(
                &[TBlock {
                    linkify: true,
                    identity: None,
                    gutter: "",
                    gutter_style: Style::default(),
                    style: Style::default(),
                    text: &text,
                    markdown: true,
                    runs: &[],
                }],
                parts[1].width.max(1) as usize,
                parts[1].height as usize,
            );
            if first_draw {
                view.scroll_up(view.rows.len());
                first_draw = false;
            }
            f.render_widget(Paragraph::new(view.lines()), parts[1]);
            f.render_widget(
                Paragraph::new(" ↑/↓ scroll · PgUp/PgDn page · Home start · End end · q/Esc close")
                    .style(Style::default().fg(Color::DarkGray)),
                parts[2],
            );
        })?;
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Release {
                continue;
            }
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Up => view.scroll_up(1),
                KeyCode::Down => view.scroll_down(1),
                KeyCode::PageUp => view.scroll_up(view.page()),
                KeyCode::PageDown => view.scroll_down(view.page()),
                KeyCode::Home => view.scroll_up(view.rows.len()),
                KeyCode::End => view.follow_tail(),
                _ => {}
            }
        }
    }
    Ok(())
}

struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
    }
}
