//! App-owned Windows input and cell painting. No terminal bindings, hooks or global listeners.
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{
    backend::{Backend, ClearType, WindowSize},
    buffer::Cell,
    layout::{Position, Rect, Size},
    style::{Color, Modifier},
};
use std::{
    io, mem,
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread,
};
use unicode_width::UnicodeWidthStr;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::*,
        Input::KeyboardAndMouse::*,
        Shell::{DragAcceptFiles, DragFinish, DragQueryFileW},
        WindowsAndMessaging::*,
    },
};

const UPDATE: u32 = WM_APP + 1;
const TITLE: u32 = WM_APP + 2;
const POINTER: u32 = WM_APP + 3;
const SHUTDOWN: u32 = WM_APP + 4;
const MAX_COLS: u16 = 500;
const MAX_ROWS: u16 = 250;

/// Decoded attachment pixels and their cell-aligned viewport. Provider input retains
/// its separate immutable file snapshot; this is only a display overlay.
#[derive(Clone)]
pub(super) struct RasterImage {
    pub area: Rect,
    pub pixels: Arc<image::RgbaImage>,
}

#[derive(Clone)]
struct Screen {
    size: Size,
    pixels: Size,
    cells: Vec<Cell>,
    cursor: Position,
    cursor_visible: bool,
    title: String,
    image: Option<RasterImage>,
}
impl Screen {
    fn resize(&mut self, cols: u16, rows: u16, pixels: Size) {
        self.size = Size::new(cols.clamp(1, MAX_COLS), rows.clamp(1, MAX_ROWS));
        self.pixels = pixels;
        self.cells =
            vec![Cell::default(); usize::from(self.size.width) * usize::from(self.size.height)];
        self.image = None;
    }
}
struct Shared {
    hwnd: AtomicIsize,
    pointer: AtomicUsize,
    dirty: AtomicBool,
    screen: Mutex<Screen>,
}

pub(super) struct WindowBackend {
    shared: Arc<Shared>,
    thread: Option<thread::JoinHandle<()>>,
    stopped: Receiver<()>,
}

/// Every receiver belongs to one HWND and therefore one Concordia process/run UI.
pub(super) fn open() -> io::Result<(WindowBackend, Receiver<Event>)> {
    let shared = Arc::new(Shared {
        hwnd: AtomicIsize::new(0),
        pointer: AtomicUsize::new(0),
        dirty: AtomicBool::new(true),
        screen: Mutex::new(Screen {
            size: Size::new(120, 42),
            pixels: Size::new(1080, 840),
            cells: vec![Cell::default(); 120 * 42],
            cursor: Position::default(),
            cursor_visible: false,
            title: "Concordia".into(),
            image: None,
        }),
    });
    let (events, receiver) = mpsc::sync_channel(4096);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (stopped_tx, stopped) = mpsc::sync_channel(1);
    let window_shared = shared.clone();
    let thread = thread::Builder::new()
        .name("concordia-native-window".into())
        .spawn(move || {
            let result = unsafe { window_thread(window_shared, events, &ready_tx) };
            if let Err(error) = result {
                let _ = ready_tx.send(Err(error));
            }
            let _ = stopped_tx.send(());
        })?;
    match ready_rx.recv() {
        Ok(Ok(())) => Ok((
            WindowBackend {
                shared,
                thread: Some(thread),
                stopped,
            },
            receiver,
        )),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(_) => {
            let _ = thread.join();
            Err(io::Error::other(
                "Native window stopped before initialization",
            ))
        }
    }
}

impl WindowBackend {
    pub(super) fn set_image(&mut self, image: Option<RasterImage>) -> io::Result<()> {
        if image.as_ref().is_some_and(|image| {
            image.pixels.width() == 0
                || image.pixels.height() == 0
                || image.pixels.width() > 4096
                || image.pixels.height() > 4096
                || u64::from(image.pixels.width()) * u64::from(image.pixels.height()) > 16_000_000
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Image exceeds native preview bounds",
            ));
        }
        let mut screen = self.shared.screen.lock().unwrap_or_else(|e| e.into_inner());
        let unchanged = match (&screen.image, &image) {
            (None, None) => true,
            (Some(old), Some(new)) => old.area == new.area && Arc::ptr_eq(&old.pixels, &new.pixels),
            _ => false,
        };
        if unchanged {
            return Ok(());
        }
        screen.image = image;
        drop(screen);
        self.shared.dirty.store(true, Ordering::Release);
        Backend::flush(self)
    }
    fn post(&self, message: u32) -> io::Result<()> {
        let hwnd = self.shared.hwnd.load(Ordering::Acquire) as HWND;
        if hwnd.is_null() {
            return Err(io::Error::other("Native window has closed"));
        }
        // SAFETY: the handle is published by our own live message thread. PostMessage
        // copies numeric arguments and never borrows application data across threads.
        if unsafe { PostMessageW(hwnd, message, 0, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    pub(super) fn set_title(&mut self, title: &str) -> io::Result<()> {
        let title: String = title
            .chars()
            .filter(|c| !c.is_control())
            .take(512)
            .collect();
        let mut screen = self.shared.screen.lock().unwrap_or_else(|e| e.into_inner());
        if screen.title == title {
            return Ok(());
        }
        screen.title = title;
        drop(screen);
        self.post(TITLE)
    }
    pub(super) fn set_pointer(&mut self, pointer: &str) -> io::Result<()> {
        let shape = match pointer {
            "pointer" => 1,
            "text" => 2,
            "ew-resize" | "col-resize" => 3,
            "ns-resize" | "row-resize" => 4,
            _ => 0,
        };
        if self.shared.pointer.swap(shape, Ordering::Relaxed) != shape {
            self.post(POINTER)?;
        }
        Ok(())
    }
}
impl Drop for WindowBackend {
    fn drop(&mut self) {
        let _ = self.post(SHUTDOWN);
        // A stuck display driver must not block provider cancellation or application exit.
        if self
            .stopped
            .recv_timeout(std::time::Duration::from_secs(3))
            .is_ok()
        {
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
impl io::Write for WindowBackend {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Backend::flush(self)
    }
}
impl Backend for WindowBackend {
    type Error = io::Error;
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut screen = self.shared.screen.lock().unwrap_or_else(|e| e.into_inner());
        let width = screen.size.width;
        let mut changed = false;
        for (x, y, cell) in content {
            if x < width && y < screen.size.height {
                screen.cells[usize::from(y) * usize::from(width) + usize::from(x)] = cell.clone();
                changed = true;
            }
        }
        if changed {
            self.shared.dirty.store(true, Ordering::Release);
        }
        Ok(())
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        let mut screen = self.shared.screen.lock().unwrap_or_else(|e| e.into_inner());
        if screen.cursor_visible {
            screen.cursor_visible = false;
            self.shared.dirty.store(true, Ordering::Release);
        }
        Ok(())
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        let mut screen = self.shared.screen.lock().unwrap_or_else(|e| e.into_inner());
        if !screen.cursor_visible {
            screen.cursor_visible = true;
            self.shared.dirty.store(true, Ordering::Release);
        }
        Ok(())
    }
    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self
            .shared
            .screen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cursor)
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        let position = position.into();
        let mut screen = self.shared.screen.lock().unwrap_or_else(|e| e.into_inner());
        if screen.cursor != position {
            screen.cursor = position;
            self.shared.dirty.store(true, Ordering::Release);
        }
        Ok(())
    }
    fn clear(&mut self) -> io::Result<()> {
        self.shared
            .screen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cells
            .fill(Cell::default());
        self.shared.dirty.store(true, Ordering::Release);
        Backend::flush(self)
    }
    fn clear_region(&mut self, region: ClearType) -> io::Result<()> {
        let mut screen = self.shared.screen.lock().unwrap_or_else(|e| e.into_inner());
        let width = usize::from(screen.size.width);
        let cursor = usize::from(screen.cursor.y) * width + usize::from(screen.cursor.x);
        let len = screen.cells.len();
        let range = match region {
            ClearType::All => 0..len,
            ClearType::AfterCursor => cursor.saturating_add(1).min(len)..len,
            ClearType::BeforeCursor => 0..cursor.min(len),
            ClearType::CurrentLine => {
                (usize::from(screen.cursor.y) * width).min(len)
                    ..((usize::from(screen.cursor.y) + 1) * width).min(len)
            }
            ClearType::UntilNewLine => {
                cursor.min(len)..((usize::from(screen.cursor.y) + 1) * width).min(len)
            }
        };
        screen.cells[range].fill(Cell::default());
        drop(screen);
        self.shared.dirty.store(true, Ordering::Release);
        Backend::flush(self)
    }
    fn size(&self) -> io::Result<Size> {
        Ok(self
            .shared
            .screen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .size)
    }
    fn window_size(&mut self) -> io::Result<WindowSize> {
        let screen = self.shared.screen.lock().unwrap_or_else(|e| e.into_inner());
        Ok(WindowSize {
            columns_rows: screen.size,
            pixels: screen.pixels,
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.shared.dirty.swap(false, Ordering::AcqRel) {
            self.post(UPDATE)?;
        }
        Ok(())
    }
}

struct Window {
    shared: Arc<Shared>,
    events: SyncSender<Event>,
    fonts: [HFONT; 16],
    cell_width: i32,
    cell_height: i32,
    surrogate: Option<u16>,
    pressed: Option<MouseButton>,
    focused: bool,
    blink: bool,
    wheel: i32,
    last_motion: Option<MouseEvent>,
    surface: Option<Surface>,
    image_pixels: Option<BitmapPixels>,
}

struct BitmapPixels {
    source: Arc<image::RgbaImage>,
    bgra: Vec<u8>,
}

/// Cached pixels belong to the window thread. Unchanged rows need no GDI calls.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous_bitmap: HGDIOBJ,
    width: i32,
    height: i32,
    cells: Vec<Cell>,
    columns: u16,
    rows: u16,
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous_bitmap);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        for font in self.fonts {
            if !font.is_null() {
                unsafe {
                    DeleteObject(font);
                }
            }
        }
    }
}
impl Window {
    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }
    unsafe fn fonts(&mut self, hwnd: HWND, dpi: u32) {
        // Cached glyphs must be regenerated after a DPI/font change.
        self.surface = None;
        for font in &mut self.fonts {
            if !font.is_null() {
                DeleteObject(*font);
            }
            *font = null_mut();
        }
        let face = wide("Consolas");
        for (style, font) in self.fonts.iter_mut().enumerate() {
            *font = CreateFontW(
                -((18 * dpi.max(96) / 96) as i32),
                0,
                0,
                0,
                if style & 1 != 0 { 700 } else { 400 },
                u32::from(style & 2 != 0),
                u32::from(style & 4 != 0),
                u32::from(style & 8 != 0),
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                FIXED_PITCH as u32 | FF_MODERN as u32,
                face.as_ptr(),
            );
        }
        let dc = GetDC(hwnd);
        if !dc.is_null() {
            let previous = SelectObject(dc, self.fonts[0]);
            let mut metrics: TEXTMETRICW = mem::zeroed();
            if GetTextMetricsW(dc, &mut metrics) != 0 {
                self.cell_width = metrics.tmAveCharWidth.max(1);
                self.cell_height = (metrics.tmHeight + metrics.tmExternalLeading).max(1);
            }
            SelectObject(dc, previous);
            ReleaseDC(hwnd, dc);
        }
    }
    unsafe fn resized(&mut self, hwnd: HWND) {
        let mut client: RECT = mem::zeroed();
        if GetClientRect(hwnd, &mut client) == 0 || client.right <= 0 || client.bottom <= 0 {
            return;
        }
        let cols = (client.right / self.cell_width).clamp(1, i32::from(MAX_COLS)) as u16;
        let rows = (client.bottom / self.cell_height).clamp(1, i32::from(MAX_ROWS)) as u16;
        self.shared
            .screen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .resize(
                cols,
                rows,
                Size::new(
                    client.right.min(i32::from(u16::MAX)) as u16,
                    client.bottom.min(i32::from(u16::MAX)) as u16,
                ),
            );
        self.emit(Event::Resize(cols, rows));
        self.shared.dirty.store(true, Ordering::Release);
        InvalidateRect(hwnd, null(), 0);
    }
    fn coordinates(&self, x: i32, y: i32) -> (u16, u16) {
        let screen = self.shared.screen.lock().unwrap_or_else(|e| e.into_inner());
        (
            (x / self.cell_width).clamp(0, i32::from(screen.size.width.saturating_sub(1))) as u16,
            (y / self.cell_height).clamp(0, i32::from(screen.size.height.saturating_sub(1))) as u16,
        )
    }
    unsafe fn mouse(&mut self, kind: MouseEventKind, x: i32, y: i32) {
        let (column, row) = self.coordinates(x, y);
        let event = MouseEvent {
            kind,
            column,
            row,
            modifiers: modifiers(),
        };
        if matches!(kind, MouseEventKind::Moved | MouseEventKind::Drag(_)) {
            if self.last_motion == Some(event) {
                return;
            }
            self.last_motion = Some(event);
        } else {
            self.last_motion = None;
        }
        self.emit(Event::Mouse(event));
    }
    unsafe fn paint(&mut self, hwnd: HWND, supplied_dc: Option<HDC>) {
        let mut paint: PAINTSTRUCT = mem::zeroed();
        let dc = supplied_dc.unwrap_or_else(|| BeginPaint(hwnd, &mut paint));
        if dc.is_null() {
            return;
        }
        let mut client: RECT = mem::zeroed();
        GetClientRect(hwnd, &mut client);
        if self
            .surface
            .as_ref()
            .is_none_or(|surface| surface.width != client.right || surface.height != client.bottom)
        {
            self.surface = None;
            let memory = CreateCompatibleDC(dc);
            let bitmap = CreateCompatibleBitmap(dc, client.right.max(1), client.bottom.max(1));
            if memory.is_null() || bitmap.is_null() {
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                if !memory.is_null() {
                    DeleteDC(memory);
                }
                if supplied_dc.is_none() {
                    EndPaint(hwnd, &paint);
                }
                return;
            }
            let previous_bitmap = SelectObject(memory, bitmap);
            SetDCBrushColor(memory, rgb(20, 23, 31));
            FillRect(memory, &client, GetStockObject(DC_BRUSH) as HBRUSH);
            self.surface = Some(Surface {
                dc: memory,
                bitmap,
                previous_bitmap,
                width: client.right,
                height: client.bottom,
                cells: Vec::new(),
                columns: 0,
                rows: 0,
            });
        }
        // GDI rendering can be much slower than updating the cell buffer. Never keep
        // the application thread waiting on the screen mutex while painting.
        let screen = self
            .shared
            .screen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let surface = self.surface.as_mut().expect("surface initialized above");
        let memory = surface.dc;
        let original_font = SelectObject(memory, self.fonts[0]);
        let same_grid = surface.columns == screen.size.width && surface.rows == screen.size.height;
        for y in 0..screen.size.height {
            let start = usize::from(y) * usize::from(screen.size.width);
            let end = start + usize::from(screen.size.width);
            if same_grid && surface.cells.get(start..end) == Some(&screen.cells[start..end]) {
                continue;
            }
            // Redraw the entire changed row so removing a wide character also clears
            // both of its old cells and styled background runs remain continuous.
            let mut covered = 0;
            for x in 0..screen.size.width {
                if x < covered {
                    continue;
                }
                let cell =
                    &screen.cells[usize::from(y) * usize::from(screen.size.width) + usize::from(x)];
                let width = UnicodeWidthStr::width(cell.symbol()).clamp(1, 2) as i32;
                covered = x.saturating_add(width as u16);
                let rect = RECT {
                    left: i32::from(x) * self.cell_width,
                    top: i32::from(y) * self.cell_height,
                    right: (i32::from(x) + width) * self.cell_width,
                    bottom: (i32::from(y) + 1) * self.cell_height,
                };
                let mut fg = color(cell.fg, true);
                let mut bg = color(cell.bg, false);
                if cell.modifier.contains(Modifier::REVERSED) {
                    mem::swap(&mut fg, &mut bg);
                }
                if cell.modifier.contains(Modifier::DIM) {
                    fg = dim(fg, bg);
                }
                if cell.modifier.contains(Modifier::HIDDEN) {
                    fg = bg;
                }
                SetTextColor(memory, fg);
                SetBkColor(memory, bg);
                let style = usize::from(cell.modifier.contains(Modifier::BOLD))
                    | (usize::from(cell.modifier.contains(Modifier::ITALIC)) << 1)
                    | (usize::from(cell.modifier.contains(Modifier::UNDERLINED)) << 2)
                    | (usize::from(cell.modifier.contains(Modifier::CROSSED_OUT)) << 3);
                SelectObject(memory, self.fonts[style]);
                if cell.symbol() == "▀" || cell.symbol() == "█" {
                    SetDCBrushColor(memory, bg);
                    FillRect(memory, &rect, GetStockObject(DC_BRUSH) as HBRUSH);
                    let top = RECT {
                        bottom: if cell.symbol() == "▀" {
                            rect.top + (self.cell_height + 1) / 2
                        } else {
                            rect.bottom
                        },
                        ..rect
                    };
                    SetDCBrushColor(memory, fg);
                    FillRect(memory, &top, GetStockObject(DC_BRUSH) as HBRUSH);
                } else {
                    let text: Vec<u16> = cell.symbol().encode_utf16().collect();
                    ExtTextOutW(
                        memory,
                        rect.left,
                        rect.top,
                        ETO_OPAQUE | ETO_CLIPPED,
                        &rect,
                        text.as_ptr(),
                        text.len() as u32,
                        null(),
                    );
                }
            }
        }
        surface.columns = screen.size.width;
        surface.rows = screen.size.height;
        surface.cells = screen.cells;
        SelectObject(memory, original_font);
        // Caret is an overlay, never part of the cached backing image. Expose and
        // blink events therefore only copy cached pixels and repaint this rectangle.
        BitBlt(dc, 0, 0, client.right, client.bottom, memory, 0, 0, SRCCOPY);
        self.paint_image(dc, screen.image.as_ref(), &client);
        if self.focused
            && self.blink
            && screen.cursor_visible
            && screen.cursor.x < screen.size.width
            && screen.cursor.y < screen.size.height
        {
            let left = i32::from(screen.cursor.x) * self.cell_width;
            let top = i32::from(screen.cursor.y) * self.cell_height;
            let rect = RECT {
                left,
                top: top + 2,
                right: left + 2,
                bottom: top + self.cell_height - 2,
            };
            SetDCBrushColor(dc, rgb(230, 232, 240));
            FillRect(dc, &rect, GetStockObject(DC_BRUSH) as HBRUSH);
        }
        if supplied_dc.is_none() {
            EndPaint(hwnd, &paint);
        }
    }

    unsafe fn paint_image(&mut self, dc: HDC, image: Option<&RasterImage>, client: &RECT) {
        let Some(image) = image else {
            self.image_pixels = None;
            return;
        };
        if self
            .image_pixels
            .as_ref()
            .is_none_or(|cached| !Arc::ptr_eq(&cached.source, &image.pixels))
        {
            let mut bgra = image.pixels.as_raw().clone();
            for pixel in bgra.as_chunks_mut::<4>().0 {
                let [red, green, blue, alpha] = *pixel;
                let composite = |channel: u8, background: u8| {
                    ((u32::from(channel) * u32::from(alpha)
                        + u32::from(background) * (255 - u32::from(alpha))
                        + 127)
                        / 255) as u8
                };
                *pixel = [
                    composite(blue, 31),
                    composite(green, 23),
                    composite(red, 20),
                    255,
                ];
            }
            self.image_pixels = Some(BitmapPixels {
                source: image.pixels.clone(),
                bgra,
            });
        }
        let cached = self.image_pixels.as_ref().expect("image pixels prepared");
        let left = i32::from(image.area.x) * self.cell_width;
        let top = i32::from(image.area.y) * self.cell_height;
        let available_width =
            (i32::from(image.area.width) * self.cell_width).min(client.right - left);
        let available_height =
            (i32::from(image.area.height) * self.cell_height).min(client.bottom - top);
        if available_width <= 0 || available_height <= 0 {
            return;
        }
        let width = image.pixels.width();
        let height = image.pixels.height();
        let scale = (f64::from(available_width) / f64::from(width))
            .min(f64::from(available_height) / f64::from(height))
            .min(1.0);
        let rendered_width = (f64::from(width) * scale).floor().max(1.0) as i32;
        let rendered_height = (f64::from(height) * scale).floor().max(1.0) as i32;
        let bitmap = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: cached.bgra.len() as u32,
                ..mem::zeroed()
            },
            ..mem::zeroed()
        };
        let previous_mode = SetStretchBltMode(dc, HALFTONE);
        let mut previous_origin: POINT = mem::zeroed();
        SetBrushOrgEx(dc, 0, 0, &mut previous_origin);
        StretchDIBits(
            dc,
            left,
            top,
            rendered_width,
            rendered_height,
            0,
            0,
            width as i32,
            height as i32,
            cached.bgra.as_ptr().cast(),
            &bitmap,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        SetBrushOrgEx(dc, previous_origin.x, previous_origin.y, null_mut());
        if previous_mode != 0 {
            SetStretchBltMode(dc, previous_mode);
        }
    }
}

unsafe fn window_thread(
    shared: Arc<Shared>,
    events: SyncSender<Event>,
    ready: &mpsc::SyncSender<io::Result<()>>,
) -> io::Result<()> {
    // Only this thread creates/destroys windows and owns all GDI objects.
    SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    let instance = GetModuleHandleW(null());
    // Stable class plus our exact process id gives diagnostics an explicit owned HWND.
    let class = wide("Concordia.Native.Cells");
    let window_class = WNDCLASSW {
        lpszClassName: class.as_ptr(),
        style: CS_DBLCLKS,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        ..mem::zeroed()
    };
    if RegisterClassW(&window_class) == 0 && io::Error::last_os_error().raw_os_error() != Some(1410)
    {
        return Err(io::Error::last_os_error());
    }
    let mut window = Box::new(Window {
        shared: shared.clone(),
        events,
        fonts: [null_mut(); 16],
        cell_width: 9,
        cell_height: 20,
        surrogate: None,
        pressed: None,
        focused: false,
        blink: true,
        wheel: 0,
        last_motion: None,
        surface: None,
        image_pixels: None,
    });
    let dpi = GetDpiForSystem().clamp(96, 768) as i32;
    let initial_width = (1120 * dpi / 96).min((GetSystemMetrics(SM_CXSCREEN) * 9 / 10).max(400));
    let initial_height = (860 * dpi / 96).min((GetSystemMetrics(SM_CYSCREEN) * 9 / 10).max(280));
    let hwnd = CreateWindowExW(
        WS_EX_ACCEPTFILES,
        class.as_ptr(),
        wide("Concordia").as_ptr(),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        initial_width,
        initial_height,
        null_mut(),
        null_mut(),
        instance,
        (&mut *window as *mut Window).cast(),
    );
    if hwnd.is_null() {
        return Err(io::Error::last_os_error());
    }
    shared.hwnd.store(hwnd as isize, Ordering::Release);
    window.fonts(hwnd, GetDpiForWindow(hwnd));
    window.resized(hwnd);
    DragAcceptFiles(hwnd, 1);
    SetTimer(hwnd, 1, 500, None);
    ShowWindow(hwnd, SW_SHOW);
    UpdateWindow(hwnd);
    let _ = ready.send(Ok(()));
    let mut message: MSG = mem::zeroed();
    loop {
        let result = GetMessageW(&mut message, null_mut(), 0, 0);
        if result <= 0 {
            break;
        }
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    shared.hwnd.store(0, Ordering::Release);
    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: GWLP_USERDATA is set only from our boxed Window during CreateWindowEx;
    // the box stays alive on this message thread until after WM_DESTROY/GetMessage exit.
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Window;
    if pointer.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    // Native APIs can synchronously re-enter the window procedure. Handle stateless
    // messages and focus/capture transitions before borrowing our Window instance.
    match message {
        SHUTDOWN => {
            DestroyWindow(hwnd);
            return 0;
        }
        WM_DESTROY => {
            KillTimer(hwnd, 1);
            PostQuitMessage(0);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_SYSCHAR => return 0,
        WM_GETMINMAXINFO => {
            let info = &mut *(lparam as *mut MINMAXINFO);
            info.ptMinTrackSize = POINT { x: 400, y: 280 };
            return 0;
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            SetFocus(hwnd);
            SetCapture(hwnd);
        }
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
            ReleaseCapture();
        }
        _ => {}
    }
    if !matches!(
        message,
        WM_CLOSE
            | WM_PAINT
            | WM_PRINTCLIENT
            | UPDATE
            | TITLE
            | POINTER
            | WM_SETCURSOR
            | WM_TIMER
            | WM_SIZE
            | WM_DPICHANGED
            | WM_SETFOCUS
            | WM_KILLFOCUS
            | WM_KEYDOWN
            | WM_SYSKEYDOWN
            | WM_CHAR
            | WM_PASTE
            | WM_LBUTTONDOWN
            | WM_LBUTTONDBLCLK
            | WM_RBUTTONDOWN
            | WM_MBUTTONDOWN
            | WM_LBUTTONUP
            | WM_RBUTTONUP
            | WM_MBUTTONUP
            | WM_MOUSEMOVE
            | WM_MOUSEWHEEL
            | WM_MOUSEHWHEEL
            | WM_DROPFILES
    ) {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    let window = &mut *pointer;
    match message {
        WM_CLOSE => {
            window.emit(Event::Key(KeyEvent::new(
                KeyCode::F(12),
                KeyModifiers::CONTROL,
            )));
            0
        }
        WM_PAINT => {
            window.paint(hwnd, None);
            0
        }
        WM_PRINTCLIENT => {
            window.paint(hwnd, Some(wparam as HDC));
            0
        }
        UPDATE => {
            window.blink = true;
            InvalidateRect(hwnd, null(), 0);
            0
        }
        TITLE => {
            let title = wide(
                &window
                    .shared
                    .screen
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .title,
            );
            SetWindowTextW(hwnd, title.as_ptr());
            0
        }
        POINTER => {
            let mut position: POINT = mem::zeroed();
            if GetCursorPos(&mut position) != 0 && WindowFromPoint(position) == hwnd {
                set_pointer(window);
            }
            0
        }
        WM_SETCURSOR if lparam as u32 & 0xffff == HTCLIENT => {
            set_pointer(window);
            1
        }
        WM_TIMER => {
            if window.focused
                && window
                    .shared
                    .screen
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .cursor_visible
            {
                window.blink = !window.blink;
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_SIZE => {
            window.resized(hwnd);
            0
        }
        WM_DPICHANGED => {
            let next = &*(lparam as *const RECT);
            window.fonts(hwnd, (wparam & 0xffff) as u32);
            SetWindowPos(
                hwnd,
                null_mut(),
                next.left,
                next.top,
                next.right - next.left,
                next.bottom - next.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            0
        }
        WM_SETFOCUS => {
            window.focused = true;
            window.blink = true;
            window.emit(Event::FocusGained);
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_KILLFOCUS => {
            window.focused = false;
            let pressed = window.pressed.take();
            if let Some(button) = pressed {
                let mut point: POINT = mem::zeroed();
                GetCursorPos(&mut point);
                ScreenToClient(hwnd, &mut point);
                window.mouse(MouseEventKind::Up(button), point.x, point.y);
            }
            window.surrogate = None;
            window.emit(Event::FocusLost);
            InvalidateRect(hwnd, null(), 0);
            if pressed.is_some() {
                ReleaseCapture();
            }
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let mods = modifiers();
            let vk = wparam as u16;
            let repeating = (lparam as usize & (1 << 30)) != 0;
            if vk == VK_F4 && mods == KeyModifiers::ALT {
                if !repeating {
                    window.emit(Event::Key(KeyEvent::new(
                        KeyCode::F(12),
                        KeyModifiers::CONTROL,
                    )));
                }
                return 0;
            }
            if let Some(key) = key(vk, mods) {
                window.emit(Event::Key(KeyEvent::new_with_kind(
                    key,
                    mods,
                    if repeating {
                        KeyEventKind::Repeat
                    } else {
                        KeyEventKind::Press
                    },
                )));
                return 0;
            }
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_CHAR => {
            let unit = wparam as u16;
            if (0xd800..=0xdbff).contains(&unit) {
                window.surrogate = Some(unit);
                return 0;
            }
            let character = if (0xdc00..=0xdfff).contains(&unit) {
                window.surrogate.take().and_then(|high| {
                    char::from_u32(
                        0x10000 + ((u32::from(high) - 0xd800) << 10) + u32::from(unit) - 0xdc00,
                    )
                })
            } else {
                window.surrogate = None;
                char::from_u32(u32::from(unit))
            };
            if let Some(character) = character.filter(|c| !c.is_control()) {
                window.emit(Event::Key(KeyEvent::new_with_kind(
                    KeyCode::Char(character),
                    modifiers(),
                    if lparam as usize & (1 << 30) != 0 {
                        KeyEventKind::Repeat
                    } else {
                        KeyEventKind::Press
                    },
                )));
            }
            0
        }
        WM_PASTE => {
            window.emit(Event::Key(KeyEvent::new(
                KeyCode::Char('v'),
                KeyModifiers::CONTROL,
            )));
            0
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            let button = if message == WM_RBUTTONDOWN {
                MouseButton::Right
            } else if message == WM_MBUTTONDOWN {
                MouseButton::Middle
            } else {
                MouseButton::Left
            };
            window.pressed = Some(button);
            window.mouse(
                MouseEventKind::Down(button),
                low_signed(lparam),
                high_signed(lparam),
            );
            0
        }
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
            let button = if message == WM_RBUTTONUP {
                MouseButton::Right
            } else if message == WM_MBUTTONUP {
                MouseButton::Middle
            } else {
                MouseButton::Left
            };
            window.pressed = None;
            window.mouse(
                MouseEventKind::Up(button),
                low_signed(lparam),
                high_signed(lparam),
            );
            0
        }
        WM_MOUSEMOVE => {
            window.mouse(
                window
                    .pressed
                    .map_or(MouseEventKind::Moved, MouseEventKind::Drag),
                low_signed(lparam),
                high_signed(lparam),
            );
            0
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let mut point = POINT {
                x: low_signed(lparam),
                y: high_signed(lparam),
            };
            ScreenToClient(hwnd, &mut point);
            window.wheel += (wparam >> 16) as u16 as i16 as i32;
            let steps = (window.wheel / 120).clamp(-24, 24);
            window.wheel %= 120;
            let kind = if message == WM_MOUSEHWHEEL {
                if steps > 0 {
                    MouseEventKind::ScrollRight
                } else {
                    MouseEventKind::ScrollLeft
                }
            } else if steps > 0 {
                MouseEventKind::ScrollUp
            } else {
                MouseEventKind::ScrollDown
            };
            for _ in 0..steps.abs() {
                window.mouse(kind, point.x, point.y);
            }
            0
        }
        WM_DROPFILES => {
            let valid = dropped_files(window, wparam as _);
            if !valid {
                MessageBoxW(
                    hwnd,
                    wide("Drop up to four local images or documents (16 KiB of paths maximum).")
                        .as_ptr(),
                    wide("Concordia attachment").as_ptr(),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn dropped_files(window: &Window, drop: windows_sys::Win32::UI::Shell::HDROP) -> bool {
    let count = DragQueryFileW(drop, u32::MAX, null_mut(), 0);
    let mut paths = Vec::new();
    let mut bytes = 0;
    if (1..=4).contains(&count) {
        for index in 0..count {
            let len = DragQueryFileW(drop, index, null_mut(), 0);
            if len == 0 || len > 4096 {
                break;
            }
            let mut path = vec![0u16; len as usize + 1];
            if DragQueryFileW(drop, index, path.as_mut_ptr(), path.len() as u32) != len {
                break;
            }
            let path = String::from_utf16_lossy(&path[..len as usize]);
            bytes += path.len() + 3;
            if bytes > 16 * 1024 || path.contains(['"', '\r', '\n', '\0']) {
                break;
            }
            paths.push(format!("\"{path}\""));
        }
    }
    DragFinish(drop);
    if paths.len() == count as usize && !paths.is_empty() {
        window.emit(Event::Paste(paths.join("\n")));
        true
    } else {
        false
    }
}

unsafe fn modifiers() -> KeyModifiers {
    let mut modifiers = KeyModifiers::NONE;
    if GetKeyState(i32::from(VK_CONTROL)) < 0 {
        modifiers |= KeyModifiers::CONTROL;
    }
    if GetKeyState(i32::from(VK_SHIFT)) < 0 {
        modifiers |= KeyModifiers::SHIFT;
    }
    if GetKeyState(i32::from(VK_MENU)) < 0 {
        modifiers |= KeyModifiers::ALT;
    }
    modifiers
}
fn key(vk: u16, modifiers: KeyModifiers) -> Option<KeyCode> {
    Some(match vk {
        VK_BACK => KeyCode::Backspace,
        VK_TAB => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                KeyCode::BackTab
            } else {
                KeyCode::Tab
            }
        }
        VK_RETURN => KeyCode::Enter,
        VK_ESCAPE => KeyCode::Esc,
        VK_DELETE => KeyCode::Delete,
        VK_INSERT => KeyCode::Insert,
        VK_LEFT => KeyCode::Left,
        VK_RIGHT => KeyCode::Right,
        VK_UP => KeyCode::Up,
        VK_DOWN => KeyCode::Down,
        VK_HOME => KeyCode::Home,
        VK_END => KeyCode::End,
        VK_PRIOR => KeyCode::PageUp,
        VK_NEXT => KeyCode::PageDown,
        VK_F1..=VK_F24 => KeyCode::F((vk - VK_F1 + 1) as u8),
        0x41..=0x5a
            if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            KeyCode::Char((b'a' + (vk - 0x41) as u8) as char)
        }
        VK_SPACE if modifiers == KeyModifiers::CONTROL => KeyCode::Char(' '),
        _ => return None,
    })
}
unsafe fn set_pointer(window: &Window) {
    let resource = match window.shared.pointer.load(Ordering::Relaxed) {
        1 => IDC_HAND,
        2 => IDC_IBEAM,
        3 => IDC_SIZEWE,
        4 => IDC_SIZENS,
        _ => IDC_ARROW,
    };
    SetCursor(LoadCursorW(null_mut(), resource));
}
fn low_signed(value: LPARAM) -> i32 {
    value as u16 as i16 as i32
}
fn high_signed(value: LPARAM) -> i32 {
    (value as usize >> 16) as u16 as i16 as i32
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn rgb(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
}
fn dim(foreground: u32, background: u32) -> u32 {
    let blend =
        |shift: u32| (((foreground >> shift) & 255u32) + ((background >> shift) & 255u32)) / 2;
    blend(0) | (blend(8) << 8) | (blend(16) << 16)
}
fn color(color: Color, foreground: bool) -> u32 {
    let index = match color {
        Color::Reset => {
            return if foreground {
                rgb(224, 226, 235)
            } else {
                rgb(20, 23, 31)
            }
        }
        Color::Rgb(r, g, b) => return rgb(r, g, b),
        Color::Indexed(index) => index,
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
    };
    const ANSI: [(u8, u8, u8); 16] = [
        (12, 12, 12),
        (197, 15, 31),
        (19, 161, 14),
        (193, 156, 0),
        (0, 55, 218),
        (136, 23, 152),
        (58, 150, 221),
        (204, 204, 204),
        (118, 118, 118),
        (231, 72, 86),
        (22, 198, 12),
        (249, 241, 165),
        (59, 120, 255),
        (180, 0, 158),
        (97, 214, 214),
        (242, 242, 242),
    ];
    if index < 16 {
        let (r, g, b) = ANSI[index as usize];
        return rgb(r, g, b);
    }
    if index >= 232 {
        let level = 8 + (index - 232) * 10;
        return rgb(level, level, level);
    }
    let level = |value| if value == 0 { 0 } else { 55 + value * 40 };
    let value = index - 16;
    rgb(level(value / 36), level(value / 6 % 6), level(value % 6))
}
