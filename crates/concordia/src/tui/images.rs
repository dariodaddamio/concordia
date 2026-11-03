//! Image composer state. Snapshots travel with durable queue/draft envelopes.
use super::*;
use crate::attachments::{Attachment, Document, Image, Message};
enum Paste {
    Image(Image),
    Document(Document),
    Batch(Vec<Attachment>, Vec<String>),
    Text(String),
    Empty,
}
#[derive(Default)]
pub(super) struct State {
    pub draft: Vec<Attachment>,
    job: Option<std::sync::mpsc::Receiver<Result<Paste, String>>>,
    revision: u64,
    conversation: String,
    cwd: PathBuf,
    targets: Vec<(Rect, usize, bool)>,
    pressed: Option<(usize, bool)>,
    pasted: Option<Pasted>,
}
struct Pasted {
    paths: Vec<PathBuf>,
    revision: u64,
    conversation: String,
    cwd: PathBuf,
}
impl State {
    pub fn count(&self) -> usize {
        self.draft.len()
    }
    pub fn importing(&self) -> bool {
        self.job.is_some()
    }
    pub fn default_prompt(&self) -> &'static str {
        let images = self
            .draft
            .iter()
            .any(|attachment| matches!(attachment, Attachment::Image(_)));
        let documents = self
            .draft
            .iter()
            .any(|attachment| matches!(attachment, Attachment::TextDocument(_)));
        match (images, documents) {
            (true, false) => "Describe the attached image.",
            (false, true) => "Review the attached documents.",
            _ => "Review the attached images and documents.",
        }
    }
    pub fn height(&self) -> usize {
        self.chip_height()
            + if self
                .draft
                .iter()
                .any(|attachment| matches!(attachment, Attachment::Image(_)))
            {
                8
            } else {
                0
            }
    }
    fn chip_height(&self) -> usize {
        self.count() + usize::from(self.job.is_some()) + usize::from(self.pasted.is_some())
    }
}
impl App {
    pub(super) fn offer_pasted_files(&mut self, text: &str) {
        if self.picker.is_some()
            || self.workspace.focused.is_some()
            || self.queue.edit.is_some()
            || self.activity.detail.is_some()
            || self.project_search.active()
            || worker_input::active(self)
        {
            return;
        }
        self.images.pasted = crate::attachments::pasted_paths(text).map(|paths| Pasted {
            paths,
            revision: self.editor.revision(),
            conversation: self.deleg.d.conversation_id().into(),
            cwd: self.cwd.clone(),
        });
    }
    pub(super) fn accept_pasted_files(&mut self) -> bool {
        if self.picker.is_some()
            || self.workspace.focused.is_some()
            || self.queue.edit.is_some()
            || self.questions.focused
            || self.images.importing()
        {
            return false;
        }
        let Some(pasted) = self.images.pasted.take() else {
            return false;
        };
        if pasted.revision != self.editor.revision()
            || pasted.conversation != self.deleg.d.conversation_id()
            || pasted.cwd != self.cwd
        {
            self.flash("Pasted paths belong to an earlier draft; paste them again");
            return true;
        }
        if self.images.count() + pasted.paths.len() > crate::attachments::MAX_IMAGES {
            self.flash("At most four attachments; pasted text is unchanged");
            return true;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let root = self.state_root.clone();
        self.images.revision = self.editor.revision();
        self.images.conversation = self.deleg.d.conversation_id().into();
        self.images.cwd = self.cwd.clone();
        self.images.job = Some(rx);
        std::thread::spawn(move || {
            let mut attachments = Vec::new();
            let mut errors = Vec::new();
            for path in pasted.paths {
                match import_attachment(&root, &path) {
                    Ok(Paste::Image(image)) => attachments.push(Attachment::Image(image)),
                    Ok(Paste::Document(document)) => {
                        attachments.push(Attachment::TextDocument(document))
                    }
                    Ok(_) => {}
                    Err(error) => errors.push(format!(
                        "{}: {error}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )),
                }
            }
            let _ = tx.send(Ok(Paste::Batch(attachments, errors)));
        });
        true
    }
    fn attach_imported(&mut self, attachment: Attachment, preview: bool) -> Result<(), String> {
        self.images.pressed = None;
        if self.images.count() >= crate::attachments::MAX_IMAGES {
            return Err("At most four attachments; original draft retained".into());
        }
        if self
            .images
            .draft
            .iter()
            .any(|old| old.hash() == attachment.hash())
        {
            return Err("This attachment is already present".into());
        }
        if let Attachment::TextDocument(document) = &attachment {
            let bytes = self
                .images
                .draft
                .iter()
                .filter_map(|attachment| {
                    if let Attachment::TextDocument(document) = attachment {
                        Some(document.bytes)
                    } else {
                        None
                    }
                })
                .sum::<usize>();
            if bytes.saturating_add(document.bytes) > crate::attachments::MAX_DOCUMENT_BYTES {
                return Err("Documents exceed 64 KiB per message; original draft retained".into());
            }
        }
        let document = if let Attachment::TextDocument(document) = &attachment {
            Some(document.clone())
        } else {
            None
        };
        self.images.draft.push(attachment);
        self.persist_draft_now();
        if let Some(document) = document {
            if preview
                && self.perform_file_action(
                    crate::file_links::ResolvedFile {
                        path: document.path,
                        line: Some(1),
                        column: None,
                    },
                    crate::file_open::FileAction::Preview,
                    self.cwd.clone(),
                    None,
                )
            {
                if let Some(id) = self.workspace.focused {
                    self.workspace.preview(id, true);
                }
                self.flash("Document snapshot attached; preview shows the included text. Esc returns to the draft");
            }
        } else {
            self.flash("Image attached · click its preview or X to remove");
        }
        Ok(())
    }
    pub(super) fn composer_value(&self) -> String {
        Message {
            text: self.editor.text(),
            attachments: self.images.draft.clone(),
            invalid: None,
        }
        .encode()
    }
    pub(super) fn restore_composer(&mut self, raw: &str) {
        self.images.pressed = None;
        self.images.targets.clear();
        self.images.pasted = None;
        let message = Message::decode(raw);
        self.editor.set_text(&message.text);
        self.images.draft = message.attachments;
    }
    pub(super) fn image_command(&mut self, arg: &str) {
        let arg = arg.trim().trim_matches(['"', '`']);
        if arg == "clear" {
            self.images.job = None;
            self.images.pasted = None;
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
        if self.images.count() >= crate::attachments::MAX_IMAGES {
            self.flash("Four attachments per message; remove one first");
            return;
        }
        let root = self.state_root.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.images.revision = self.editor.revision();
        self.images.conversation = self.deleg.d.conversation_id().to_owned();
        self.images.cwd = self.cwd.clone();
        self.images.job = Some(rx);
        std::thread::spawn(move || {
            let result = if let Some(path) = path {
                import_attachment(&root, &path)
            } else {
                capture(&root, clipboard)
            };
            let _ = tx.send(result);
        });
    }
    pub(super) fn poll_images(&mut self) {
        if self.images.pasted.as_ref().is_some_and(|pasted| {
            pasted.revision != self.editor.revision()
                || pasted.conversation != self.deleg.d.conversation_id()
                || pasted.cwd != self.cwd
        }) {
            self.images.pasted = None;
        }
        let result = self.images.job.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Some(Err("Image helper stopped".into()))
            }
            Err(_) => None,
        });
        if let Some(result) = result {
            self.images.job = None;
            if self.images.revision != self.editor.revision()
                || self.images.conversation != self.deleg.d.conversation_id()
                || self.images.cwd != self.cwd
            {
                self.flash(
                    "Draft or conversation changed while importing; choose the attachment again",
                );
                return;
            }
            match result {
                Ok(Paste::Image(image)) => {
                    if let Err(error) = self.attach_imported(Attachment::Image(image), false) {
                        self.flash(error);
                    }
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
                Ok(Paste::Document(document)) => {
                    if let Err(error) =
                        self.attach_imported(Attachment::TextDocument(document), true)
                    {
                        self.flash(error);
                    }
                }
                Ok(Paste::Batch(attachments, mut errors)) => {
                    let mut count = 0;
                    for attachment in attachments {
                        match self.attach_imported(attachment, false) {
                            Ok(()) => count += 1,
                            Err(error) => errors.push(error),
                        }
                    }
                    self.flash(format!("Attached {count} file(s); original pasted text is unchanged. Click document chips to review snapshots. {}",errors.join("; ")));
                }
                Ok(Paste::Empty) => {}
                Err(error) => self.flash(error),
            }
        }
        self.ensure_inline_raster();
    }
}
fn import_attachment(root: &Path, path: &Path) -> Result<Paste, String> {
    if path.extension().is_some_and(|extension| {
        matches!(
            extension.to_string_lossy().to_ascii_lowercase().as_str(),
            "txt" | "md" | "markdown" | "json"
        )
    }) {
        Document::import(root, path).map(Paste::Document)
    } else if path.extension().is_some_and(|extension| {
        matches!(
            extension.to_string_lossy().to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp"
        )
    }) {
        crate::attachments::import(root, path).map(Paste::Image)
    } else {
        Err(
            "Attach PNG/JPEG/WebP or UTF-8 .txt/.md/.json; native binary documents are unavailable"
                .into(),
        )
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
        "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Windows.Forms; $dialog=New-Object Windows.Forms.OpenFileDialog; $dialog.Title='Attach image or document to Concordia'; $dialog.Filter='Images and UTF-8 documents|*.png;*.jpg;*.jpeg;*.webp;*.txt;*.md;*.json|Images|*.png;*.jpg;*.jpeg;*.webp|UTF-8 documents|*.txt;*.md;*.json'; if ($dialog.ShowDialog() -ne 'OK') {[Console]::Write('CANCELLED'); exit 0}; [Console]::OutputEncoding=[Text.Encoding]::UTF8; [Console]::Write($dialog.FileName)"
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
            import_attachment(root, &path)
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
    if let Some(index) = app
        .images
        .draft
        .iter()
        .position(|attachment| matches!(attachment, Attachment::Image(_)))
    {
        let height = 8.min(
            area.height
                .saturating_sub(app.images.chip_height() as u16 + 3),
        );
        let preview = Rect { height, ..area };
        raster::draw_inline(f, preview, app);
        if height > 0 {
            app.images.targets.push((preview, index, false));
        }
        area.y += height;
        area.height -= height;
    }
    let count = app
        .images
        .chip_height()
        .min(area.height.saturating_sub(3) as usize);
    for index in 0..count {
        let row = Rect::new(area.x, area.y, area.width, 1);
        let entry = app.images.draft.get(index);
        let offered = index == app.images.count() + usize::from(app.images.importing())
            && app.images.pasted.is_some();
        if entry.is_some() || offered {
            let label = if let Some(attachment) = entry {
                match attachment {
                    Attachment::Image(image) => format!(
                        " [Image {}: {} · {} KB]",
                        index + 1,
                        image.name,
                        image.bytes.div_ceil(1024)
                    ),
                    Attachment::TextDocument(document) => format!(
                        " [Document {}: {} · {} bytes{}]",
                        index + 1,
                        document.name,
                        document.bytes,
                        document
                            .range
                            .as_ref()
                            .map(|range| format!(" · selection {}..{}", range.start, range.end))
                            .unwrap_or_default()
                    ),
                }
            } else {
                format!(
                    " [Attach {} pasted file(s) too · F9] text stays unchanged",
                    app.images
                        .pasted
                        .as_ref()
                        .map_or(0, |pasted| pasted.paths.len())
                )
            };
            f.render_widget(
                Paragraph::new(label).style(Style::default().fg(Color::Cyan)),
                row,
            );
            let close_width = 4.min(row.width);
            let close = Rect::new(
                row.right().saturating_sub(close_width),
                row.y,
                close_width,
                1,
            );
            f.render_widget(
                Paragraph::new("[X]").style(Style::default().fg(Color::DarkGray)),
                close,
            );
            let target = if offered { usize::MAX } else { index };
            app.images.targets.push((
                Rect::new(row.x, row.y, row.width.saturating_sub(close_width), 1),
                target,
                false,
            ));
            app.images.targets.push((close, target, true));
        } else {
            f.render_widget(
                Paragraph::new(" Preparing attachment...")
                    .style(Style::default().fg(Color::DarkGray)),
                row,
            );
        }
        area.y += 1;
        area.height -= 1;
    }
    area
}
pub(super) fn mouse(app: &mut App, event: &MouseEvent) -> bool {
    if app.picker.is_some()
        || app.project_search.active()
        || app.activity.detail.is_some()
        || worker_input::active(app)
    {
        return false;
    }
    let hit = app
        .images
        .targets
        .iter()
        .find(|(rect, _, _)| contains(*rect, event.column, event.row))
        .map(|(_, index, close)| (*index, *close));
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) if hit.is_some() => {
            app.images.pressed = hit;
            true
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let Some(pressed) = app.images.pressed.take() else {
                return false;
            };
            if Some(pressed) != hit {
                return true;
            }
            if pressed.0 == usize::MAX {
                if pressed.1 {
                    app.images.pasted = None;
                } else {
                    app.accept_pasted_files();
                }
                return true;
            }
            let Some(attachment) = app.images.draft.get(pressed.0).cloned() else {
                return true;
            };
            if pressed.1 {
                app.images.draft.remove(pressed.0);
                app.persist_draft_now();
                return true;
            }
            let (path, document) = match attachment {
                Attachment::Image(image) => {
                    app.start_raster(image.path, Some(image.sha256));
                    return true;
                }
                Attachment::TextDocument(document) => {
                    if let Err(error) = document.text() {
                        app.flash(error);
                        return true;
                    }
                    (document.path, true)
                }
            };
            app.perform_file_action(
                crate::file_links::ResolvedFile {
                    path,
                    line: Some(1),
                    column: None,
                },
                crate::file_open::FileAction::Preview,
                app.cwd.clone(),
                None,
            );
            if document {
                if let Some(id) = app.workspace.focused {
                    app.workspace.preview(id, true);
                }
            }
            true
        }
        _ => false,
    }
}
pub(super) fn clickable(app: &App, x: u16, y: u16) -> bool {
    app.images
        .targets
        .iter()
        .any(|(rect, _, _)| contains(*rect, x, y))
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
        let doc_source = dir.path().join("notes.md");
        std::fs::write(&doc_source, "Document snapshot bytes\r\n").unwrap();
        let document = Document::import(&root, &doc_source).unwrap();
        let mut app = App::new(root.clone(), work.clone());
        app.restore_state();
        app.editor.set_text("Inspect it");
        app.images.draft = vec![
            Attachment::Image(image.clone()),
            Attachment::TextDocument(document.clone()),
        ];
        app.persist_draft_now();
        drop(app);
        let mut app = App::new(root, work);
        app.restore_state();
        assert_eq!(app.editor.text(), "Inspect it");
        assert_eq!(
            app.images.draft,
            vec![
                Attachment::Image(image.clone()),
                Attachment::TextDocument(document.clone())
            ]
        );
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
        assert!(screen.contains("Document 2: notes.md"));
        assert!(!screen.contains("concordia-message-v1"));
        app.busy = true;
        assert!(queue::tab(&mut app));
        assert!(app.images.draft.is_empty());
        let queued = crate::attachments::Message::decode(&app.queue.items[0]);
        assert_eq!(queued.images(), vec![image]);
        assert_eq!(
            queued.documents().cloned().collect::<Vec<_>>(),
            vec![document]
        );
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
        app.queue.edit = None;
        app.queue.items.clear();
        app.editor.set_text("");
        let pasted = format!("\"{}\"", doc_source.display());
        app.paste_text(&pasted);
        assert_eq!(app.editor.text(), pasted);
        assert!(app.images.pasted.is_some());
        assert_eq!(app.images.count(), 0);
        assert!(app.accept_pasted_files());
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.images.importing() {
            app.poll_images();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(app.images.count(), 1);
        assert_eq!(app.editor.text(), pasted);
        app.image_command("clear");
        assert_eq!(app.images.count(), 0);
        assert_eq!(app.editor.text(), pasted);
        app.offer_pasted_files(&pasted);
        app.editor.insert_char('x');
        assert!(app.accept_pasted_files());
        assert!(!app.images.importing());
        assert_eq!(app.images.count(), 0);
        raster::fixture_proof(&mut app, dir.path());
        artifacts::fixture_proof(&mut app, dir.path());
    }
}
