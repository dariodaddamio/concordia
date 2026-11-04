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
const CACHE_BYTES: usize = 64 * 1024 * 1024;

struct Thumbnail {
    hash: String,
    width: u32,
    height: u32,
    pixels: image::RgbaImage,
    original: Arc<image::RgbaImage>,
}
struct View {
    inline: bool,
    snapshot: Option<String>,
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
    pub native: bool,
    native_draws: std::cell::RefCell<Vec<(Rect, Arc<image::RgbaImage>)>>,
    view: Option<View>,
    job: Option<Job>,
    cache: std::collections::VecDeque<Arc<Thumbnail>>,
    preview_errors: std::collections::VecDeque<(String, String)>,
}
impl State {
    pub fn active(&self) -> bool {
        self.view.as_ref().is_some_and(|view| !view.inline)
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
        if let (Err(error), Some(hash)) = (
            &result,
            self.view.as_ref().and_then(|view| view.snapshot.as_ref()),
        ) {
            self.preview_errors.retain(|(old, _)| old != hash);
            self.preview_errors.push_back((hash.clone(), error.clone()));
            while self.preview_errors.len() > CACHE {
                self.preview_errors.pop_front();
            }
        }
        let result = result.map(|thumb| {
            if let Some(index) = self.cache.iter().position(|old| old.hash == thumb.hash) {
                let old = self.cache.remove(index).unwrap();
                if old.original.len() >= thumb.original.len() {
                    old
                } else {
                    Arc::new(thumb)
                }
            } else {
                Arc::new(thumb)
            }
        });
        if let Ok(thumb) = &result {
            self.preview_errors.retain(|(hash, _)| hash != &thumb.hash);
            self.cache.push_back(thumb.clone());
            while self.cache.len() > 1
                && (self.cache.len() > CACHE
                    || self
                        .cache
                        .iter()
                        .map(|image| image.original.len() + image.pixels.len())
                        .sum::<usize>()
                        > CACHE_BYTES)
            {
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
    if let Some(old) = cached
        .iter()
        .find(|old| old.hash == hash && old.original.dimensions() == (old.width, old.height))
    {
        return Ok(Thumbnail {
            hash,
            width: old.width,
            height: old.height,
            pixels: old.pixels.clone(),
            original: old.original.clone(),
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
    let original = Arc::new(image.into_rgba8());
    Ok(Thumbnail {
        hash,
        width,
        height,
        pixels,
        original,
    })
}
impl App {
    pub(super) fn start_raster(&mut self, path: PathBuf, expected: Option<String>) {
        self.start_raster_mode(path, expected, false);
    }
    pub(super) fn ensure_inline_raster(&mut self) {
        if self.raster.active() {
            return;
        }
        let first = self.images.draft.iter().find_map(|attachment| {
            if let crate::attachments::Attachment::Image(image) = attachment {
                (!self
                    .raster
                    .cache
                    .iter()
                    .any(|cached| cached.hash == image.sha256)
                    && !self
                        .raster
                        .preview_errors
                        .iter()
                        .any(|(hash, _)| hash == &image.sha256))
                .then(|| image.clone())
            } else {
                None
            }
        });
        let Some(image) = first else {
            if !self
                .images
                .draft
                .iter()
                .any(|item| matches!(item, crate::attachments::Attachment::Image(_)))
            {
                self.raster.close();
            }
            return;
        };
        if self.raster.view.as_ref().is_some_and(|view| {
            view.inline
                && view.snapshot.as_deref() == Some(&image.sha256)
                && view.conversation == self.deleg.d.conversation_id()
        }) {
            return;
        }
        self.raster.close();
        self.start_raster_mode(image.path, Some(image.sha256), true);
    }
    fn start_raster_mode(&mut self, path: PathBuf, expected: Option<String>, inline: bool) {
        self.raster.poll();
        if self.raster.job.is_some() {
            if !inline {
                self.flash("An image preview is still finishing; try shortly");
            }
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.raster.view = Some(View {
            inline,
            snapshot: expected.clone(),
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
            let result =
                decode(&path, expected.as_deref(), &cached, &cancelled).map(|mut image| {
                    // Four composer previews must fit the shared cache without eviction
                    // repeatedly restarting their decoders. The explicit viewer decodes
                    // the original again when a full-resolution entry is not cached.
                    if inline && (image.width > 512 || image.height > 512) {
                        image.original =
                            Arc::new(image::imageops::thumbnail(&*image.original, 512, 512));
                    }
                    image
                });
            let _ = tx.send(result);
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
    app.raster.native_draws.borrow_mut().clear();
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
    paint_view(f, content, thumb, app);
    let footer = format!(
        "{} × {} · SHA-256 {} · {}",
        thumb.width,
        thumb.height,
        &thumb.hash[..12],
        if app.raster.native && !app.no_color {
            "image preview"
        } else {
            "cell preview"
        }
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
pub(super) fn draw_inline(f: &mut Frame, area: Rect, app: &App) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let first = app.images.draft.iter().find_map(|attachment| {
        if let crate::attachments::Attachment::Image(image) = attachment {
            Some(image)
        } else {
            None
        }
    });
    let Some(image) = first else { return };
    f.render_widget(
        Paragraph::new(format!(" Preview · {} · click to enlarge", image.name))
            .style(Style::default().fg(Color::Cyan)),
        Rect { height: 1, ..area },
    );
    let content = Rect {
        y: area.y + 1,
        height: area.height.saturating_sub(1),
        ..area
    };
    draw_attachment(
        f,
        Rect {
            width: content.width.min(12),
            ..content
        },
        app,
        image,
    );
}

pub(super) fn draw_attachment(
    f: &mut Frame,
    area: Rect,
    app: &App,
    image: &crate::attachments::Image,
) {
    if area.is_empty() {
        return;
    }
    if let Some(thumb) = app
        .raster
        .cache
        .iter()
        .find(|cached| cached.hash == image.sha256)
    {
        paint_view(f, area, thumb, app);
    } else if let Some((_, error)) = app
        .raster
        .preview_errors
        .iter()
        .find(|(hash, _)| hash == &image.sha256)
    {
        f.render_widget(
            Paragraph::new(format!("Preview unavailable\n{error}"))
                .style(Style::default().fg(Color::DarkGray))
                .wrap(Wrap { trim: false }),
            area,
        );
    } else {
        f.render_widget(
            Paragraph::new("Preparing preview...").style(Style::default().fg(Color::DarkGray)),
            area,
        );
    }
}

fn paint_view(f: &mut Frame, content: Rect, thumb: &Thumbnail, app: &App) {
    if app.raster.native && !app.no_color {
        // A marker confined to blank image cells lets later modal widgets suppress
        // the overlay, including a Clear widget that covers only part of the image.
        for y in content.y..content.bottom() {
            for x in content.x..content.right() {
                f.buffer_mut()[(x, y)].reset();
                f.buffer_mut()[(x, y)]
                    .set_fg(Color::Rgb(20, 23, 31))
                    .set_bg(Color::Rgb(20, 23, 31));
            }
        }
        app.raster
            .native_draws
            .borrow_mut()
            .push((content, thumb.original.clone()));
        return;
    }
    paint(f, content, thumb, app.no_color);
}

pub(super) fn begin_frame(app: &App) {
    app.raster.native_draws.borrow_mut().clear();
}

#[cfg(windows)]
pub(super) fn native_images(f: &mut Frame, app: &App) -> Vec<super::native_window::RasterImage> {
    app.raster
        .native_draws
        .borrow()
        .iter()
        .filter_map(|(area, pixels)| {
            let area = *area;
            if area.is_empty() || area.intersection(f.area()) != area {
                return None;
            }
            for y in area.y..area.bottom() {
                for x in area.x..area.right() {
                    let cell = &f.buffer_mut()[(x, y)];
                    if cell.symbol() != " "
                        || cell.fg != Color::Rgb(20, 23, 31)
                        || cell.bg != Color::Rgb(20, 23, 31)
                    {
                        return None;
                    }
                }
            }
            Some(super::native_window::RasterImage {
                area,
                pixels: pixels.clone(),
            })
        })
        .collect()
}

fn paint(f: &mut Frame, content: Rect, thumb: &Thumbnail, monochrome: bool) {
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
            let cell =
                &mut f.buffer_mut()[(content.x + (content.width - w) / 2 + x, content.y + y)];
            if monochrome {
                let Color::Rgb(r, g, b) = sample(top) else {
                    continue;
                };
                let light =
                    (u32::from(r) * 2126 + u32::from(g) * 7152 + u32::from(b) * 722) / 10000;
                let symbols = [" ", ".", ":", "-", "=", "+", "*", "#", "%", "@"];
                cell.set_symbol(symbols[(light * 9 / 255) as usize]);
            } else {
                cell.set_symbol("▀")
                    .set_fg(sample(top))
                    .set_bg(sample(bottom));
            }
        }
    }
}

#[cfg(test)]
pub(super) fn fixture_proof(app: &mut App, dir: &Path) {
    // Earlier attachment scenarios can still have a cancelled inline decode in
    // flight. Settle it before opening this fixture's explicit viewer.
    app.raster.close();
    let settle = Instant::now() + Duration::from_secs(5);
    while app.raster.job.is_some() {
        app.raster.poll();
        assert!(Instant::now() < settle);
        std::thread::yield_now();
    }
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
    let sharp = dir.join("sharp-pattern.png");
    image::RgbaImage::from_fn(512, 256, |x, y| {
        if (x + y) % 2 == 0 {
            image::Rgba([255, 255, 255, 255])
        } else {
            image::Rgba([0, 0, 0, 255])
        }
    })
    .save(&sharp)
    .unwrap();
    let sharp = decode(&sharp, None, &[], &cancel).unwrap();
    assert_eq!(sharp.original.dimensions(), (512, 256));
    assert_eq!(sharp.original.get_pixel(1, 0).0, [0, 0, 0, 255]);
    assert_eq!(sharp.original.get_pixel(2, 0).0, [255, 255, 255, 255]);
    assert_eq!(sharp.pixels.dimensions(), (256, 128));
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
    app.images.draft = vec![crate::attachments::Attachment::Image(
        crate::attachments::import(&app.state_root, &path).unwrap(),
    )];
    app.ensure_inline_raster();
    while app.raster.job.is_some() {
        app.raster.poll();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(
        !app.raster.active(),
        "a composer thumbnail must not capture input"
    );
    terminal.draw(|f| super::draw_ui(f, app)).unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(screen.contains("Image 1: raster-preview.png"), "{screen}");
    assert!(!screen.contains("Verified attachment snapshot"));
    assert!(!screen.contains("The message stays editable"));
    assert!(
        screen.contains('▀'),
        "the default composer shows actual pixels"
    );
    app.paste_text(" plus typing");
    assert!(app.editor.text().contains("plus typing"));
    app.no_color = true;
    terminal
        .draw(|f| draw_inline(f, Rect::new(0, 0, 60, 4), app))
        .unwrap();
    assert!(
        (1..4).any(|y| (0..12).any(|x| {
            let symbol = terminal.backend().buffer()[(x, y)].symbol();
            symbol != " " && symbol != "▀"
        })),
        "monochrome terminals also show image pixels"
    );
    #[cfg(windows)]
    {
        app.no_color = false;
        app.raster.native = true;
        terminal
            .draw(|f| {
                begin_frame(app);
                draw_inline(f, Rect::new(0, 0, 60, 4), app);
                let overlay = native_images(f, app)
                    .pop()
                    .expect("native thumbnail uses original pixels");
                assert_eq!(overlay.pixels.dimensions(), (12, 8));
                f.render_widget(Clear, overlay.area);
                assert!(
                    native_images(f, app).is_empty(),
                    "a modal hides the image overlay"
                );
                begin_frame(app);
                assert!(
                    native_images(f, app).is_empty(),
                    "a new frame cannot reuse an old overlay"
                );
            })
            .unwrap();
        app.raster.native = false;
    }
    let broken = crate::attachments::import(&app.state_root, &bad).unwrap();
    let next_path = dir.join("next-preview.png");
    image::RgbaImage::from_pixel(32, 16, image::Rgba([30, 90, 200, 255]))
        .save(&next_path)
        .unwrap();
    let next = crate::attachments::import(&app.state_root, &next_path).unwrap();
    app.images.draft = vec![
        crate::attachments::Attachment::Image(broken.clone()),
        crate::attachments::Attachment::Image(next.clone()),
    ];
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.raster.poll();
        app.ensure_inline_raster();
        if app
            .raster
            .cache
            .iter()
            .any(|cached| cached.hash == next.sha256)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "a failed image must not block later previews"
        );
        std::thread::yield_now();
    }
    assert!(app
        .raster
        .preview_errors
        .iter()
        .any(|(hash, _)| hash == &broken.sha256));
    terminal
        .draw(|f| draw_attachment(f, Rect::new(0, 0, 60, 3), app, &broken))
        .unwrap();
    let error_text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(error_text.contains("Preview unavailable"));
    app.images.draft.clear();
    app.ensure_inline_raster();
    assert!(app.raster.view.is_none());
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
