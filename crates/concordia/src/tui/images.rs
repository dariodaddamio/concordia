//! Image composer state. Snapshots travel with durable queue/draft envelopes.
use super::*;
use crate::attachments::{Image, Message};
enum Paste {
    Image(Image),
    Text(String),
    Empty,
}
#[derive(Default)]
pub(super) struct State {
    pub draft: Vec<Image>,
    job: Option<std::sync::mpsc::Receiver<Result<Paste, String>>>,
    revision: u64,
    targets: Vec<(Rect, usize, bool)>,
    pressed: Option<(usize, bool)>,
}
impl State {
    pub fn height(&self) -> usize {
        self.draft.len() + usize::from(self.job.is_some())
    }
}
impl App {
    pub(super) fn composer_value(&self) -> String {
        Message {
            text: self.editor.text(),
            images: self.images.draft.clone(),
        }
        .encode()
    }
    pub(super) fn restore_composer(&mut self, raw: &str) {
        let message = Message::decode(raw);
        self.editor.set_text(&message.text);
        self.images.draft = message.images;
    }
    pub(super) fn image_command(&mut self, arg: &str) {
        let arg = arg.trim().trim_matches(['"', '`']);
        if arg == "clear" {
            self.images.draft.clear();
            self.persist_draft_now();
            return;
        }
        let path = (!arg.is_empty()).then(|| {
            let p = Path::new(arg);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                self.cwd.join(p)
            }
        });
        self.start_image_import(path, false);
    }
    pub(super) fn start_image_import(&mut self, path: Option<PathBuf>, clipboard: bool) {
        if self.images.job.is_some() {
            self.flash("An image is being prepared");
            return;
        }
        if self.images.draft.len() >= crate::attachments::MAX_IMAGES {
            self.flash("Four images per message; remove one first");
            return;
        }
        let root = self.state_root.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.images.revision = self.editor.revision();
        self.images.job = Some(rx);
        std::thread::spawn(move || {
            let result = if let Some(path) = path {
                crate::attachments::import(&root, &path).map(Paste::Image)
            } else {
                capture(&root, clipboard)
            };
            let _ = tx.send(result);
        });
    }
    pub(super) fn poll_images(&mut self) {
        let result = self.images.job.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Some(Err("Image helper stopped".into()))
            }
            Err(_) => None,
        });
        if let Some(result) = result {
            self.images.job = None;
            match result {
                Ok(Paste::Image(image)) => {
                    if self
                        .images
                        .draft
                        .iter()
                        .any(|old| old.sha256 == image.sha256)
                    {
                        self.flash("This image is already attached");
                        return;
                    }
                    self.images.draft.push(image);
                    self.persist_draft_now();
                    self.flash("Image attached · click its preview or X to remove");
                }
                Ok(Paste::Text(text))
                    if self.editor.revision() == self.images.revision
                        && self.picker.is_none()
                        && self.workspace.focused.is_none()
                        && self.queue.edit.is_none() =>
                {
                    self.paste_text(&text)
                }
                Ok(Paste::Text(_)) => {
                    self.flash("Draft changed while reading the clipboard; paste again")
                }
                Ok(Paste::Empty) => {}
                Err(error) => self.flash(error),
            }
        }
    }
}
#[cfg(not(test))]
fn capture(root: &Path, clipboard: bool) -> Result<Paste, String> {
    let temp = root
        .join("attachments")
        .join(format!("capture-{}.png", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(temp.parent().unwrap()).map_err(|_| "Image storage unavailable")?;
    let mut cmd = std::process::Command::new("powershell.exe");
    crate::process_ui::background_std(&mut cmd);
    let script = if clipboard {
        "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; $image=[Windows.Forms.Clipboard]::GetImage(); if ($null -eq $image) {[Console]::Write('NO_IMAGE'); exit 0}; try {$image.Save($env:CONCORDIA_IMAGE_CAPTURE,[Drawing.Imaging.ImageFormat]::Png)} finally {$image.Dispose()}"
    } else {
        "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Windows.Forms; $dialog=New-Object Windows.Forms.OpenFileDialog; $dialog.Title='Attach image to Concordia'; $dialog.Filter='Images|*.png;*.jpg;*.jpeg;*.webp'; if ($dialog.ShowDialog() -ne 'OK') {[Console]::Write('CANCELLED'); exit 0}; [Console]::OutputEncoding=[Text.Encoding]::UTF8; [Console]::Write($dialog.FileName)"
    };
    cmd.args(["-NoProfile", "-NonInteractive", "-STA", "-Command", script])
        .env("CONCORDIA_IMAGE_CAPTURE", &temp);
    let output = clipboard::run_with_timeout(
        cmd,
        None,
        Duration::from_secs(if clipboard { 8 } else { 120 }),
    );
    let result = match output {
        Ok(bytes) if clipboard && bytes == b"NO_IMAGE" => clipboard::get_text().map(|text| {
            if text.is_empty() {
                Paste::Empty
            } else {
                Paste::Text(text)
            }
        }),
        Ok(bytes) if !clipboard && bytes == b"CANCELLED" => Ok(Paste::Empty),
        Ok(bytes) => {
            let path = if clipboard {
                temp.clone()
            } else {
                PathBuf::from(String::from_utf8_lossy(&bytes).trim())
            };
            crate::attachments::import(root, &path).map(Paste::Image)
        }
        Err(reason) => Err(format!("Could not prepare image: {reason}")),
    };
    let _ = std::fs::remove_file(temp);
    result
}
#[cfg(test)]
fn capture(_: &Path, _: bool) -> Result<Paste, String> {
    Ok(Paste::Empty)
}

pub(super) fn draw(f: &mut Frame, mut area: Rect, app: &mut App) -> Rect {
    app.images.targets.clear();
    let count = app
        .images
        .height()
        .min(area.height.saturating_sub(3) as usize);
    for index in 0..count {
        let row = Rect::new(area.x, area.y, area.width, 1);
        if let Some(image) = app.images.draft.get(index) {
            let label = format!(
                " [Image {}: {} · {} KB]",
                index + 1,
                image.name,
                image.bytes.div_ceil(1024)
            );
            f.render_widget(
                Paragraph::new(label).style(Style::default().fg(Color::Cyan)),
                row,
            );
            let close = Rect::new(row.right().saturating_sub(5), row.y, 4, 1);
            f.render_widget(
                Paragraph::new("[X]").style(Style::default().fg(Color::DarkGray)),
                close,
            );
            app.images.targets.push((
                Rect::new(row.x, row.y, row.width.saturating_sub(5), 1),
                index,
                false,
            ));
            app.images.targets.push((close, index, true));
        } else {
            f.render_widget(
                Paragraph::new(" Preparing image...").style(Style::default().fg(Color::DarkGray)),
                row,
            );
        }
        area.y += 1;
        area.height -= 1;
    }
    area
}
pub(super) fn mouse(app: &mut App, event: &MouseEvent) -> bool {
    let hit = app
        .images
        .targets
        .iter()
        .find(|(r, _, _)| contains(*r, event.column, event.row))
        .map(|(_, i, c)| (*i, *c));
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) if hit.is_some() => {
            app.images.pressed = hit;
            true
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if let Some(pressed) = app.images.pressed.take() {
                if Some(pressed) == hit {
                    if pressed.1 {
                        app.images.draft.remove(pressed.0);
                        app.persist_draft_now();
                    } else if let Some(image) = app.images.draft.get(pressed.0) {
                        let path = image.path.to_string_lossy().into_owned();
                        if let Some(candidate) = crate::file_links::explicit_reference(&path) {
                            app.open_file_candidate(candidate, true);
                        }
                    }
                }
                true
            } else {
                false
            }
        }
        _ => false,
    }
}
pub(super) fn clickable(app: &App, x: u16, y: u16) -> bool {
    app.images
        .targets
        .iter()
        .any(|(r, _, _)| contains(*r, x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_draft_queue_edit_and_recovery_keep_the_same_snapshot_without_exposing_envelopes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("state");
        let work = dir.path().join("workspace");
        std::fs::create_dir(&work).unwrap();
        let original = dir.path().join("diagram.png");
        std::fs::write(&original, b"\x89PNG\r\n\x1a\nsynthetic pixels").unwrap();
        let image = crate::attachments::import(&root, &original).unwrap();
        let mut app = App::new(root.clone(), work.clone());
        app.restore_state();
        app.editor.set_text("Inspect it");
        app.images.draft = vec![image.clone()];
        app.persist_draft_now();
        drop(app);
        let mut app = App::new(root, work);
        app.restore_state();
        assert_eq!(app.editor.text(), "Inspect it");
        assert_eq!(app.images.draft, vec![image.clone()]);
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| draw_ui(f, &mut app)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(screen.contains("Image 1: diagram.png"));
        assert!(!screen.contains("concordia-message-v1"));
        app.busy = true;
        assert!(queue::tab(&mut app));
        assert!(app.images.draft.is_empty());
        let queued = crate::attachments::Message::decode(&app.queue.items[0]);
        assert_eq!(queued.images, vec![image]);
        assert_eq!(queued.text, "Inspect it");
        app.busy = false;
        app.queue.focused = true;
        queue::handle_key(&mut app, &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.queue.edit.as_ref().unwrap().text(), "Inspect it");
        let (tx, rx) = std::sync::mpsc::channel();
        app.images.job = Some(rx);
        app.images.revision = app.editor.revision();
        tx.send(Ok(Paste::Text("clipboard text".into()))).unwrap();
        app.poll_images();
        // Queue edit owns paste routing; async completion never interprets an attachment envelope.
        assert!(!app.editor.text().contains("concordia-message-v1"));
    }
}
