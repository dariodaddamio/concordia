//! Image composer state. Snapshots travel with durable queue/draft envelopes.
use super::*;
use crate::attachments::{Attachment, Document, Image, Message, PdfDocument};
enum Paste {
    Image(Image),
    Document(Document),
    Pdf(PdfDocument),
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
        let documents = self.draft.iter().any(|attachment| {
            matches!(
                attachment,
                Attachment::TextDocument(_) | Attachment::PdfDocument(_)
            )
        });
        match (images, documents) {
            (true, false) => "Describe the attached image.",
            (false, true) => "Review the attached documents.",
            _ => "Review the attached images and documents.",
        }
    }
    pub fn height(&self, width: u16) -> usize {
        let columns = self.columns(width);
        self.count().div_ceil(columns) * 6
            + usize::from(self.job.is_some())
            + usize::from(self.pasted.is_some())
    }
    fn columns(&self, width: u16) -> usize {
        let maximum = (usize::from(width) / 28).clamp(1, 4);
        let count = self.count().max(1);
        count.div_ceil(count.div_ceil(maximum))
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
        // A standalone pasted image path is an image input, not prose mentioning a file.
        // Import it immediately so the same preview appears as for a clipboard bitmap.
        if self.images.pasted.as_ref().is_some_and(|pasted| {
            pasted.paths.iter().all(|path| {
                path.extension().is_some_and(|extension| {
                    matches!(
                        extension.to_string_lossy().to_ascii_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "webp"
                    )
                })
            })
        }) {
            self.accept_pasted_files();
        }
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
            let _ = tx.send(Ok(import_paths(&root, pasted.paths)));
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
        if let Attachment::PdfDocument(document) = &attachment {
            let bytes = self
                .images
                .draft
                .iter()
                .filter_map(|attachment| match attachment {
                    Attachment::PdfDocument(document) => Some(document.bytes),
                    _ => None,
                })
                .sum::<usize>();
            if bytes.saturating_add(document.bytes) > crate::attachments::MAX_PDF_TOTAL_BYTES {
                return Err(
                    "PDF attachments exceed 16 MiB per message; original draft retained".into(),
                );
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
        } else if let Attachment::PdfDocument(pdf) = &self.images.draft[self.images.draft.len() - 1]
        {
            self.flash(format!(
                "PDF snapshot attached: {} · {} bytes; click its chip for the system viewer",
                pdf.name, pdf.bytes
            ));
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
        // A restored draft owns its attachments; a previous import must never append to it.
        self.images.job = None;
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
            if self.images.conversation != self.deleg.d.conversation_id()
                || self.images.cwd != self.cwd
            {
                self.flash("Conversation changed while importing; choose the attachment again");
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
                Ok(Paste::Pdf(document)) => {
                    if let Err(error) =
                        self.attach_imported(Attachment::PdfDocument(document), false)
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
fn import_paths(root: &Path, paths: Vec<PathBuf>) -> Paste {
    let mut attachments = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        match import_attachment(root, &path) {
            Ok(Paste::Image(image)) => attachments.push(Attachment::Image(image)),
            Ok(Paste::Document(document)) => attachments.push(Attachment::TextDocument(document)),
            Ok(Paste::Pdf(document)) => attachments.push(Attachment::PdfDocument(document)),
            Ok(_) => {}
            Err(error) => errors.push(format!(
                "{}: {error}",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }
    Paste::Batch(attachments, errors)
}
fn clipboard_files(root: &Path, bytes: &[u8]) -> Result<Paste, String> {
    if bytes.len() > 32 * 1024 {
        return Err("Pasted file paths exceed the attachment limit".into());
    }
    let paths: Vec<String> =
        serde_json::from_slice(bytes).map_err(|_| "Invalid clipboard file list")?;
    let quoted = paths
        .iter()
        .map(|path| format!("\"{path}\""))
        .collect::<Vec<_>>()
        .join("\n");
    let paths = crate::attachments::pasted_paths(&quoted)
        .ok_or("Paste up to four local images, documents or PDFs")?;
    Ok(import_paths(root, paths))
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
    } else if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        PdfDocument::import(root, path).map(Paste::Pdf)
    } else {
        Err("Attach PNG/JPEG/WebP, UTF-8 .txt/.md/.json, or a PDF".into())
    }
}
#[cfg(not(test))]
fn capture(root: &Path, clipboard: bool) -> Result<Paste, String> {
    #[cfg(windows)]
    if clipboard {
        return capture_native_clipboard(root);
    }
    let temp = root
        .join("attachments")
        .join(format!("capture-{}.png", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(temp.parent().unwrap()).map_err(|_| "Image storage unavailable")?;
    let mut cmd = crate::process_ui::command("powershell.exe");
    crate::process_ui::background_std(&mut cmd);
    let script = if clipboard {
        "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; $image=[Windows.Forms.Clipboard]::GetImage(); if ($null -eq $image) {$data=[Windows.Forms.Clipboard]::GetDataObject(); if ($null -ne $data -and $data.GetDataPresent('PNG',$false)) {$png=$data.GetData('PNG',$false); if ($png -isnot [IO.Stream] -or -not $png.CanSeek -or $png.Length -gt 4194304) {throw 'Clipboard PNG exceeds 4 MiB or has an unsupported stream'}; try {$png.Position=0; $file=[IO.File]::Create($env:CONCORDIA_IMAGE_CAPTURE); try {$png.CopyTo($file)} finally {$file.Dispose()}} finally {$png.Dispose()}; exit 0}; $files=[Windows.Forms.Clipboard]::GetFileDropList(); if ($files.Count -gt 0) {if ($files.Count -gt 4) {throw 'At most four attachments'}; [Console]::OutputEncoding=[Text.Encoding]::UTF8; [Console]::Write('FILES:'); [Console]::Write((ConvertTo-Json -InputObject @($files) -Compress)); exit 0}; [Console]::Write('NO_IMAGE'); exit 0}; try {$image.Save($env:CONCORDIA_IMAGE_CAPTURE,[Drawing.Imaging.ImageFormat]::Png)} finally {$image.Dispose()}"
    } else {
        "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Windows.Forms; $dialog=New-Object Windows.Forms.OpenFileDialog; $dialog.Title='Attach image or document to Concordia'; $dialog.Filter='Images, documents and PDF|*.png;*.jpg;*.jpeg;*.webp;*.txt;*.md;*.json;*.pdf|Images|*.png;*.jpg;*.jpeg;*.webp|UTF-8 documents|*.txt;*.md;*.json|PDF documents|*.pdf'; if ($dialog.ShowDialog() -ne 'OK') {[Console]::Write('CANCELLED'); exit 0}; [Console]::OutputEncoding=[Text.Encoding]::UTF8; [Console]::Write($dialog.FileName)"
    };
    cmd.args(["-NoProfile", "-NonInteractive", "-STA", "-Command", script])
        .env("CONCORDIA_IMAGE_CAPTURE", &temp);
    let output = clipboard::run_with_timeout(
        cmd,
        None,
        Duration::from_secs(if clipboard { 8 } else { 120 }),
    );
    let result = match output {
        Ok(bytes) if clipboard && bytes.starts_with(b"FILES:") => {
            clipboard_files(root, &bytes[6..])
        }
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
#[cfg(all(windows, not(test)))]
fn capture_native_clipboard(root: &Path) -> Result<Paste, String> {
    match clipboard_capture::read()? {
        clipboard_capture::Capture::Image(bytes) => {
            let directory = root.join("attachments");
            std::fs::create_dir_all(&directory).map_err(|_| "Image storage unavailable")?;
            let path = directory.join(format!("capture-{}.png", uuid::Uuid::new_v4()));
            let result = std::fs::write(&path, bytes)
                .map_err(|_| "Clipboard image could not be saved".to_string())
                .and_then(|_| crate::attachments::import(root, &path))
                .map(|mut image| {
                    image.name = "Clipboard image.png".into();
                    Paste::Image(image)
                });
            let _ = std::fs::remove_file(path);
            result
        }
        clipboard_capture::Capture::Files(paths) => {
            let quoted = paths
                .iter()
                .map(|path| format!("\"{}\"", path.display()))
                .collect::<Vec<_>>()
                .join("\n");
            let paths = crate::attachments::pasted_paths(&quoted)
                .ok_or("Paste up to four local image or document files")?;
            Ok(import_paths(root, paths))
        }
        clipboard_capture::Capture::Empty => clipboard::get_text().map(|text| {
            if text.is_empty() {
                Paste::Empty
            } else {
                Paste::Text(text)
            }
        }),
    }
}
#[cfg(test)]
fn capture(_: &Path, _: bool) -> Result<Paste, String> {
    Ok(Paste::Empty)
}

pub(super) fn draw(f: &mut Frame, mut area: Rect, app: &mut App) -> Rect {
    app.images.targets.clear();
    let columns = app.images.columns(area.width);
    let rows = app.images.count().div_ceil(columns);
    let card_height = 6u16;
    let available = area.height.saturating_sub(3);
    let mut card_rows = rows.min(usize::from(available / card_height));
    while card_rows > 0
        && card_rows * usize::from(card_height)
            + app.images.count().saturating_sub(card_rows * columns)
            > usize::from(available)
    {
        card_rows -= 1;
    }
    for index in 0..app.images.count().min(card_rows * columns) {
        let column = index % columns;
        let row = index / columns;
        let start = area.width as usize * column / columns;
        let end = area.width as usize * (column + 1) / columns;
        let card = Rect::new(
            area.x + start as u16,
            area.y + row as u16 * card_height,
            (end - start) as u16,
            card_height,
        );
        let attachment = &app.images.draft[index];
        let (kind, name, bytes) = match attachment {
            Attachment::Image(image) => ("Image", image.name.as_str(), image.bytes),
            Attachment::TextDocument(document) => {
                ("Document", document.name.as_str(), document.bytes)
            }
            Attachment::PdfDocument(document) => ("PDF", document.name.as_str(), document.bytes),
        };
        let label = format!("{kind} {}: {name}", index + 1);
        let label: String = label
            .chars()
            .take(usize::from(card.width.saturating_sub(7)))
            .collect();
        let block = Block::bordered()
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Line::styled(
                format!(" {label} "),
                Style::default().fg(Color::Cyan),
            ));
        let inner = block.inner(card);
        f.render_widget(block, card);
        let close = Rect::new(
            card.x + card.width.saturating_sub(4),
            card.y,
            3.min(card.width),
            1,
        );
        f.render_widget(
            Paragraph::new(" X ").style(Style::default().fg(Color::Cyan)),
            close,
        );
        app.images.targets.push((card, index, false));
        app.images.targets.push((close, index, true));
        let content = Rect {
            height: inner.height.saturating_sub(1),
            ..inner
        };
        match attachment {
            Attachment::Image(image) => raster::draw_attachment(f, content, app, image),
            Attachment::TextDocument(document) => {
                let text = if document.range.is_some() {
                    "Text selection"
                } else {
                    "Text document"
                };
                f.render_widget(
                    Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
                    content,
                );
            }
            Attachment::PdfDocument(_) => {
                f.render_widget(
                    Paragraph::new("PDF document\nClick to open")
                        .style(Style::default().fg(Color::DarkGray)),
                    content,
                );
            }
        }
        f.render_widget(
            Paragraph::new(format!("{} KB", bytes.div_ceil(1024)))
                .style(Style::default().fg(Color::DarkGray)),
            Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        );
    }
    let used = card_rows as u16 * card_height;
    area.y += used;
    area.height = area.height.saturating_sub(used);
    for index in (card_rows * columns).min(app.images.count())..app.images.count() {
        if area.height <= 3 {
            break;
        }
        let (kind, name) = match &app.images.draft[index] {
            Attachment::Image(image) => ("Image", image.name.as_str()),
            Attachment::TextDocument(document) => ("Document", document.name.as_str()),
            Attachment::PdfDocument(document) => ("PDF", document.name.as_str()),
        };
        let row = Rect { height: 1, ..area };
        let close = Rect::new(
            row.x + row.width.saturating_sub(4),
            row.y,
            3.min(row.width),
            1,
        );
        f.render_widget(
            Paragraph::new(format!("{kind} {}: {name}", index + 1))
                .style(Style::default().fg(Color::Cyan)),
            Rect {
                width: row.width.saturating_sub(4),
                ..row
            },
        );
        f.render_widget(
            Paragraph::new(" X ").style(Style::default().fg(Color::Cyan)),
            close,
        );
        app.images.targets.push((row, index, false));
        app.images.targets.push((close, index, true));
        area.y += 1;
        area.height -= 1;
    }
    for offered in [false, true] {
        if area.height <= 3
            || if offered {
                app.images.pasted.is_none()
            } else {
                !app.images.importing()
            }
        {
            continue;
        }
        let row = Rect { height: 1, ..area };
        let label = if offered {
            format!(
                " Attach {} pasted file(s) · F9",
                app.images.pasted.as_ref().map_or(0, |p| p.paths.len())
            )
        } else {
            " Preparing attachment...".into()
        };
        f.render_widget(
            Paragraph::new(label).style(Style::default().fg(Color::DarkGray)),
            row,
        );
        if offered {
            let close = Rect::new(row.right().saturating_sub(4), row.y, 3.min(row.width), 1);
            f.render_widget(
                Paragraph::new(" X ").style(Style::default().fg(Color::Cyan)),
                close,
            );
            app.images.targets.push((row, usize::MAX, false));
            app.images.targets.push((close, usize::MAX, true));
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
        .rev()
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
                app.images.targets.clear(); // Indices become valid again after the next draw.
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
                Attachment::PdfDocument(document) => {
                    if let Err(error) = document.data() {
                        app.flash(error);
                        return true;
                    }
                    let path = document.path.to_string_lossy();
                    app.open_host_target(
                        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned(),
                        "PDF",
                    );
                    return true;
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
        let image_card = app
            .images
            .targets
            .iter()
            .find(|(_, i, close)| *i == 0 && !close)
            .unwrap()
            .0;
        let document_card = app
            .images
            .targets
            .iter()
            .find(|(_, i, close)| *i == 1 && !close)
            .unwrap()
            .0;
        assert_eq!(
            image_card.y, document_card.y,
            "mixed attachments share a horizontal row"
        );
        let close = app
            .images
            .targets
            .iter()
            .find(|(_, i, close)| *i == 1 && *close)
            .unwrap()
            .0;
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            assert!(mouse(
                &mut app,
                &MouseEvent {
                    kind,
                    column: close.x,
                    row: close.y,
                    modifiers: KeyModifiers::NONE
                }
            ));
        }
        assert_eq!(
            app.images.count(),
            1,
            "the top close target removes only its own attachment"
        );
        assert_eq!(app.editor.text(), "Inspect it");
        app.images
            .draft
            .push(Attachment::TextDocument(document.clone()));
        let saved = app.images.draft.clone();
        app.images.draft.extend([
            Attachment::Image(image.clone()),
            Attachment::TextDocument(document.clone()),
        ]);
        let mut narrow = Terminal::new(ratatui::backend::TestBackend::new(24, 12)).unwrap();
        narrow
            .draw(|f| {
                let remaining = draw(f, f.area(), &mut app);
                assert!(remaining.height >= 3);
            })
            .unwrap();
        assert_eq!(
            app.images
                .targets
                .iter()
                .filter(|(_, _, close)| *close)
                .count(),
            4,
            "compact overflow rows retain every attachment's remove control"
        );
        app.images.draft = saved;
        app.busy = true;
        assert!(queue::tab(&mut app));
        assert!(app.images.draft.is_empty());
        let queued = crate::attachments::Message::decode(&app.queue.items[0]);
        assert_eq!(queued.images(), vec![image.clone()]);
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
        // A pasted image path imports immediately, and typing during the asynchronous
        // import must not discard the attachment or overwrite the evolving message.
        image::RgbaImage::from_pixel(12, 8, image::Rgba([200, 40, 90, 255]))
            .save(&original)
            .unwrap();
        app.editor.clear();
        let pasted = format!("\"{}\"", original.display());
        app.paste_text_chunk(&pasted, false);
        assert!(app.images.importing());
        app.editor.insert_char('!');
        while app.images.importing() {
            app.poll_images();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(app.images.count(), 1);
        assert_eq!(app.editor.text(), format!("{pasted}!"));
        assert!(app.images.pasted.is_none());
        let files = serde_json::to_vec(&vec![original.to_string_lossy().to_string()]).unwrap();
        let Paste::Batch(copied, errors) = clipboard_files(&app.state_root, &files).unwrap() else {
            panic!("copied Explorer images must use the attachment path");
        };
        assert_eq!(copied.len(), 1);
        assert!(errors.is_empty());
        assert!(clipboard_files(&app.state_root, b"[]").is_err());
        assert!(
            clipboard_files(&app.state_root, b"[\"https://example.invalid/image.png\"]").is_err()
        );
        let (tx, rx) = std::sync::mpsc::channel();
        app.images.job = Some(rx);
        app.restore_composer("replacement draft");
        assert!(!app.images.importing());
        assert!(tx.send(Ok(Paste::Image(image.clone()))).is_err());
        assert_eq!(app.images.count(), 0);
        let (turn_tx, _) = tokio::sync::mpsc::unbounded_channel();
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE),
            &turn_tx,
        );
        assert!(
            app.images.importing(),
            "F9 opens attachment input without a pasted path"
        );
        app.restore_composer("replacement draft");
        raster::fixture_proof(&mut app, dir.path());
        artifacts::fixture_proof(&mut app, dir.path());
    }
}
