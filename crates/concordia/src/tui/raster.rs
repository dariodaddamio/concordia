//! Cell-based raster previews: no terminal image protocol, shell decoder or persistent worker.
use super::*;
use image::ImageDecoder;
use ratatui::widgets::Wrap;
use sha2::{Digest, Sha256};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

const EDGE: u32 = 4096;
const PIXELS: u64 = 16_000_000;
const THUMB: u32 = 256;
const CACHE: usize = 8;

struct Thumbnail {
    hash: String,
    width: u32,
    height: u32,
    pixels: image::RgbaImage,
}
struct View {
    path: PathBuf,
    conversation: String,
    result: Option<Result<Arc<Thumbnail>, String>>,
}
struct Job {
    rx: std::sync::mpsc::Receiver<Result<Thumbnail, String>>,
    cancelled: Arc<AtomicBool>,
}
#[derive(Default)]
pub(super) struct State {
    view: Option<View>,
    job: Option<Job>,
    cache: std::collections::VecDeque<Arc<Thumbnail>>,
}
impl State {
    pub fn active(&self) -> bool {
        self.view.is_some()
    }
    pub fn close(&mut self) {
        self.view = None;
        if let Some(job) = &self.job {
            job.cancelled.store(true, Ordering::Relaxed);
        }
    }
    pub fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        let result = match job.rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("Image decoder stopped; use the system viewer".into()),
        };
        let cancelled = job.cancelled.load(Ordering::Relaxed);
        self.job = None;
        if cancelled {
            return;
        }
        let result = result.map(|thumb| {
            if let Some(index) = self.cache.iter().position(|old| old.hash == thumb.hash) {
                self.cache.remove(index).unwrap()
            } else {
                Arc::new(thumb)
            }
        });
        if let Ok(thumb) = &result {
            self.cache.push_back(thumb.clone());
            while self.cache.len() > CACHE {
                self.cache.pop_front();
            }
        }
        if let Some(view) = &mut self.view {
            view.result = Some(result);
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.close();
    }
}

fn decode(
    path: &Path,
    expected: Option<&str>,
    cached: &[Arc<Thumbnail>],
    cancelled: &AtomicBool,
) -> Result<Thumbnail, String> {
    let before = std::fs::metadata(path).map_err(|_| "Image file is unavailable")?;
    if !before.is_file() {
        return Err("Preview requires a regular image file".into());
    }
    let bytes = crate::attachments::read(path)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if expected.is_some_and(|expected| expected != hash) {
        return Err("Attachment snapshot changed; preview refused".into());
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err("Preview cancelled".into());
    }
    if let Some(old) = cached.iter().find(|old| old.hash == hash) {
        return Ok(Thumbnail {
            hash,
            width: old.width,
            height: old.height,
            pixels: old.pixels.clone(),
        });
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "Unknown image format")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(EDGE);
    limits.max_image_height = Some(EDGE);
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|error| format!("Image cannot be decoded: {error}"))?;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || u64::from(width) * u64::from(height) > PIXELS
        || decoder.total_bytes() > 64 * 1024 * 1024
    {
        return Err(
            "Image exceeds preview limits (4096 per edge / 16 megapixels / 64 MiB pixels)".into(),
        );
    }
    let image = image::DynamicImage::from_decoder(decoder)
        .map_err(|error| format!("Image cannot be decoded: {error}"))?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("Preview cancelled".into());
    }
    let pixels = image.thumbnail(THUMB, THUMB).to_rgba8();
    Ok(Thumbnail {
        hash,
        width,
        height,
        pixels,
    })
}
impl App {
    pub(super) fn start_raster(&mut self, path: PathBuf, expected: Option<String>) {
        self.raster.poll();
        if self.raster.job.is_some() {
            self.flash("An image preview is still finishing; try shortly");
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.raster.view = Some(View {
            path: path.clone(),
            conversation: self.deleg.d.conversation_id().into(),
            result: None,
        });
        self.raster.job = Some(Job {
            rx,
            cancelled: cancelled.clone(),
        });
        let cached = self.raster.cache.iter().cloned().collect::<Vec<_>>();
        std::thread::spawn(move || {
            let _ = tx.send(decode(&path, expected.as_deref(), &cached, &cancelled));
        });
    }
}
pub(super) fn key(app: &mut App, key: &KeyEvent) -> bool {
    if !app.raster.active() {
        return false;
    }
    if worker_input::active(app) {
        return false;
    }
    if app
        .raster
        .view
        .as_ref()
        .is_some_and(|view| view.conversation != app.deleg.d.conversation_id())
    {
        app.raster.close();
        return true;
    }
    match key.code {
        KeyCode::Esc => app.raster.close(),
        KeyCode::Char('e' | 'E') => {
            let path = app
                .raster
                .view
                .as_ref()
                .unwrap()
                .path
                .to_string_lossy()
                .into_owned();
            app.raster.close();
            app.open_host_target(path.strip_prefix(r"\\?\").unwrap_or(&path).into(), "image");
        }
        _ => {}
    }
    true
}
pub(super) fn draw(f: &mut Frame, app: &mut App) {
    let Some(view) = &app.raster.view else { return };
    if view.conversation != app.deleg.d.conversation_id() {
        app.raster.close();
        return;
    }
    let area = f.area();
    f.render_widget(Clear, area);
    let block = Block::bordered().title(" Image preview · Esc back · E system viewer ");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let title = view.path.to_string_lossy();
    f.render_widget(
        Paragraph::new(title.as_ref()),
        Rect {
            height: inner.height.min(1),
            ..inner
        },
    );
    let content = Rect {
        y: inner.y.saturating_add(2),
        height: inner.height.saturating_sub(4),
        ..inner
    };
    let Some(result) = &view.result else {
        f.render_widget(Paragraph::new("Preparing bounded image preview…"), content);
        return;
    };
    let thumb = match result {
        Ok(thumb) => thumb,
        Err(error) => {
            f.render_widget(
                Paragraph::new(error.as_str()).wrap(Wrap { trim: false }),
                content,
            );
            return;
        }
    };
    if app.no_color {
        f.render_widget(
            Paragraph::new(
                "Color preview disabled by no-color mode. E opens the system viewer; Esc returns.",
            ),
            content,
        );
        return;
    }
    let scale = (f64::from(content.width) / f64::from(thumb.pixels.width()))
        .min(f64::from(content.height) * 2.0 / f64::from(thumb.pixels.height()));
    let w = (f64::from(thumb.pixels.width()) * scale).floor() as u16;
    let h = (f64::from(thumb.pixels.height()) * scale / 2.0).floor() as u16;
    let w = w.min(content.width);
    let h = h.min(content.height);
    for y in 0..h {
        for x in 0..w {
            let sample = |row: u32| {
                let px = thumb.pixels.get_pixel(
                    u32::from(x) * thumb.pixels.width() / u32::from(w.max(1)),
                    row.min(thumb.pixels.height() - 1),
                );
                let blend = |value: u8| {
                    ((u16::from(value) * u16::from(px[3]) + 28 * (255 - u16::from(px[3]))) / 255)
                        as u8
                };
                Color::Rgb(blend(px[0]), blend(px[1]), blend(px[2]))
            };
            let top = u32::from(y) * 2 * thumb.pixels.height() / u32::from(h.max(1) * 2);
            let bottom = (u32::from(y) * 2 + 1) * thumb.pixels.height() / u32::from(h.max(1) * 2);
            f.buffer_mut()[(content.x + (content.width - w) / 2 + x, content.y + y)]
                .set_symbol("▀")
                .set_fg(sample(top))
                .set_bg(sample(bottom));
        }
    }
    let footer = format!(
        "{} × {} · SHA-256 {} · cell preview",
        thumb.width,
        thumb.height,
        &thumb.hash[..12]
    );
    f.render_widget(
        Paragraph::new(footer),
        Rect {
            y: inner.y + inner.height.saturating_sub(1),
            height: inner.height.min(1),
            ..inner
        },
    );
}

#[cfg(test)]
pub(super) fn fixture_proof(app: &mut App, dir: &Path) {
    let path = dir.join("raster-preview.png");
    image::RgbaImage::from_pixel(12, 8, image::Rgba([200, 40, 90, 255]))
        .save(&path)
        .unwrap();
    let cancel = AtomicBool::new(false);
    let thumb = decode(&path, None, &[], &cancel).unwrap();
    assert_eq!((thumb.width, thumb.height), (12, 8));
    assert!(decode(&path, Some("wrong"), &[], &cancel).is_err());
    let hash = thumb.hash.clone();
    assert_eq!(
        decode(&path, Some(&hash), &[Arc::new(thumb)], &cancel)
            .unwrap()
            .hash,
        hash
    );
    let huge = dir.join("too-wide.png");
    image::RgbaImage::new(EDGE + 1, 1).save(&huge).unwrap();
    assert!(decode(&huge, None, &[], &cancel).is_err());
    let bad = dir.join("bad.png");
    std::fs::write(&bad, b"\x89PNG\r\n\x1a\nnot pixels").unwrap();
    assert!(decode(&bad, None, &[], &cancel).is_err());
    app.editor.set_text("preview preserves this draft");
    app.start_raster(path.clone(), Some(hash));
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.raster.job.is_some() {
        app.raster.poll();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    let no_color = app.no_color;
    app.no_color = false;
    terminal.draw(|f| draw(f, app)).unwrap();
    assert!(terminal
        .backend()
        .buffer()
        .content
        .iter()
        .any(|cell| cell.symbol() == "▀"));
    app.paste_text("should not alter draft");
    assert_eq!(app.editor.text(), "preview preserves this draft");
    assert!(key(app, &KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert!(!app.raster.active());
    app.start_raster(path, None);
    app.raster.close();
    while app.raster.job.is_some() {
        app.raster.poll();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!app.raster.active());
    app.no_color = no_color;
}
