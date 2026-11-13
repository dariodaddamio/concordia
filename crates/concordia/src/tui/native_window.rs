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
    collections::VecDeque,
    io, mem,
    net::{SocketAddr, TcpListener, TcpStream},
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use unicode_width::UnicodeWidthStr;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Dwm::DwmSetWindowAttribute,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED},
    UI::{
        HiDpi::*,
        Input::KeyboardAndMouse::*,
        Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW},
        WindowsAndMessaging::*,
    },
};

const UPDATE: u32 = WM_APP + 1;
const TITLE: u32 = WM_APP + 2;
const POINTER: u32 = WM_APP + 3;
const SHUTDOWN: u32 = WM_APP + 4;
const TEST_CONTROL: u32 = WM_APP + 5;
const TRAY_NOTIFY: u32 = WM_APP + 6;
const TRAY_ID: u32 = 1;
const MAX_COLS: u16 = 500;
const MAX_ROWS: u16 = 250;
const DWMWA_CAPTION_COLOR: u32 = 35;

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
    images: Vec<RasterImage>,
    /// Frames committed by `Backend::flush`; test timing ties paints to the frame they show.
    seq: u64,
}
impl Screen {
    fn resize(&mut self, cols: u16, rows: u16, pixels: Size) {
        let old_cols = self.size.width;
        let old_rows = self.size.height;
        let old_cells = std::mem::take(&mut self.cells);
        self.size = Size::new(cols.clamp(1, MAX_COLS), rows.clamp(1, MAX_ROWS));
        self.pixels = pixels;
        self.cells = vec![
            Cell::default();
            usize::from(self.size.width) * usize::from(self.size.height)
        ];
        for y in 0..usize::from(old_rows.min(self.size.height)) {
            let width = usize::from(old_cols.min(self.size.width));
            let old_start = y * usize::from(old_cols);
            let new_start = y * usize::from(self.size.width);
            self.cells[new_start..new_start + width]
                .clone_from_slice(&old_cells[old_start..old_start + width]);
        }
        self.images.clear();
    }
}
struct Shared {
    hwnd: AtomicIsize,
    pointer: AtomicUsize,
    dirty: AtomicBool,
    closing: AtomicBool,
    screen: Mutex<Screen>,
    requested_status: Mutex<StatusDot>,
    pending_attention: Mutex<Option<(String, String)>>,
    attention_target: Mutex<Option<String>>,
    test_control: Option<TestControl>,
}

#[derive(Default)]
struct TestControl {
    state: Mutex<TestState>,
    queue: Mutex<VecDeque<TestRequest>>,
}

#[derive(Default)]
struct TestState {
    focused_tab: String,
    overlays: Vec<String>,
    status_dot: String,
    /// Test-only frame timing: the label, start and committed frame of the measured input.
    measure: Option<(String, std::time::Instant, u64)>,
    /// Every paint since `measure` started: milliseconds, frame shown and non-blank cells.
    paints: Vec<(f64, u64, usize)>,
    /// Application draw durations in milliseconds, newest last.
    draws: Vec<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StatusDot {
    Idle,
    Working,
    NeedsAttention,
}

impl StatusDot {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::NeedsAttention => "needs_attention",
        }
    }
}

struct TestRequest {
    request: serde_json::Value,
    reply: mpsc::SyncSender<String>,
}

/// Signed cells belong to the captured mouse-up message, before terminal clamping.
pub(super) enum NativeEvent {
    Event(Event),
    MouseUp { event: MouseEvent, x: i32, y: i32 },
    Attention(String),
}

fn send_until_closed<T>(sender: &SyncSender<T>, mut value: T, closing: &AtomicBool) {
    while !closing.load(Ordering::Acquire) {
        match sender.try_send(value) {
            Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => return,
            Err(mpsc::TrySendError::Full(pending)) => value = pending,
        }
        thread::sleep(std::time::Duration::from_millis(2));
    }
}

pub(super) struct WindowBackend {
    shared: Arc<Shared>,
    thread: Option<thread::JoinHandle<()>>,
    stopped: Receiver<()>,
}

/// Every receiver belongs to one HWND and therefore one Concordia process/run UI.
pub(super) fn open() -> io::Result<(WindowBackend, Receiver<NativeEvent>)> {
    let (events, receiver) = mpsc::sync_channel(4096);
    Ok((open_with_sender(events)?, receiver))
}

fn open_with_sender(events: SyncSender<NativeEvent>) -> io::Result<WindowBackend> {
    let shared = Arc::new(Shared {
        hwnd: AtomicIsize::new(0),
        pointer: AtomicUsize::new(0),
        dirty: AtomicBool::new(true),
        closing: AtomicBool::new(false),
        screen: Mutex::new(Screen {
            size: Size::new(120, 42),
            pixels: Size::new(1080, 840),
            cells: vec![Cell::default(); 120 * 42],
            cursor: Position::default(),
            cursor_visible: false,
            title: "Concordia".into(),
            images: Vec::new(),
            seq: 0,
        }),
        requested_status: Mutex::new(StatusDot::Idle),
        pending_attention: Mutex::new(None),
        attention_target: Mutex::new(None),
        test_control: std::env::var_os("CONCORDIA_TEST_CONTROL").map(|_| TestControl::default()),
    });
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
        Ok(Ok(())) => Ok(WindowBackend {
            shared,
            thread: Some(thread),
            stopped,
        }),
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
    pub(super) fn set_test_state(&mut self, focused_tab: String, overlays: Vec<String>) {
        let Some(test) = self.shared.test_control.as_ref() else {
            return;
        };
        let mut state = test.state.lock().unwrap_or_else(|e| e.into_inner());
        state.focused_tab = focused_tab;
        state.overlays = overlays;
    }
    /// Test-only: record how long the application spent producing one frame.
    pub(super) fn note_draw(&mut self, elapsed: Duration) {
        let Some(test) = self.shared.test_control.as_ref() else {
            return;
        };
        let mut state = test.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.draws.len() >= 4096 {
            state.draws.clear();
        }
        state.draws.push(elapsed.as_secs_f64() * 1000.0);
    }
    pub(super) fn set_status_dot(&mut self, status: StatusDot) {
        *self.shared.requested_status.lock().unwrap_or_else(|e| e.into_inner()) = status;
        if let Some(test) = self.shared.test_control.as_ref() {
            test.state.lock().unwrap_or_else(|e| e.into_inner()).status_dot = status.as_str().into();
        }
        let _ = self.post(UPDATE);
    }
    pub(super) fn set_attention_notification(&mut self, item_id: String, text: String) {
        *self
            .shared
            .pending_attention
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some((item_id, text));
        let _ = self.post(UPDATE);
    }
    pub(super) fn set_images(&mut self, images: Vec<RasterImage>) -> io::Result<()> {
        if images.len() > 4
            || images.iter().any(|image| {
                image.pixels.width() == 0
                    || image.pixels.height() == 0
                    || image.pixels.width() > 4096
                    || image.pixels.height() > 4096
                    || u64::from(image.pixels.width()) * u64::from(image.pixels.height())
                        > 16_000_000
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Image exceeds native preview bounds",
            ));
        }
        let mut screen = self.shared.screen.lock().unwrap_or_else(|e| e.into_inner());
        let unchanged =
            screen.images.len() == images.len()
                && screen.images.iter().zip(&images).all(|(old, new)| {
                    old.area == new.area && Arc::ptr_eq(&old.pixels, &new.pixels)
                });
        if unchanged {
            return Ok(());
        }
        screen.images = images;
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
        self.shared.closing.store(true, Ordering::Release);
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
            self.shared.screen.lock().unwrap_or_else(|e| e.into_inner()).seq += 1;
            self.post(UPDATE)?;
        }
        Ok(())
    }
}

struct Window {
    shared: Arc<Shared>,
    events: SyncSender<NativeEvent>,
    fonts: [HFONT; 16],
    symbol_fonts: [HFONT; 16],
    emoji_fonts: [HFONT; 16],
    glyph_fonts: [std::collections::HashMap<String, u8>; 16],
    icons: [HICON; 2],
    overlay_icons: [HICON; 2],
    taskbar: *mut TaskbarList3,
    com_initialized: bool,
    status_dot: StatusDot,
    tray_added: bool,
    icon_color: crate::config::Rgb,
    icon_active: bool,
    icon_frame: u8,
    cell_width: i32,
    cell_height: i32,
    surrogate: Option<u16>,
    pressed: Option<MouseButton>,
    focused: bool,
    blink: bool,
    wheel: i32,
    last_motion: Option<MouseEvent>,
    surface: Option<Surface>,
    image_pixels: Vec<BitmapPixels>,
}

#[repr(C)]
struct TaskbarList3 {
    vtable: *const *const (),
}

const CLSID_TASKBAR_LIST: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x56fdf344, data2: 0xfd6d, data3: 0x11d0,
    data4: [0x95, 0x8a, 0x00, 0x60, 0x97, 0xc9, 0xa0, 0x90],
};
const IID_ITASKBAR_LIST3: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0xea1afb91, data2: 0x9e28, data3: 0x4b86,
    data4: [0x90, 0xe9, 0x9e, 0x9f, 0x8a, 0x5e, 0xea, 0x84],
};

struct BitmapPixels {
    source: Arc<image::RgbaImage>,
    background: [u8; 3],
    bgra: Vec<u8>,
}

/// Cached pixels belong to the window thread. Unchanged rows need no GDI calls.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous_bitmap: HGDIOBJ,
    width: i32,
    height: i32,
    background: crate::config::Rgb,
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
        for icon in self.overlay_icons {
            if !icon.is_null() {
                unsafe { DestroyIcon(icon); }
            }
        }
        if self.tray_added {
            unsafe { notify_tray(self.shared.hwnd.load(Ordering::Acquire) as HWND, NIM_DELETE, None); }
        }
        if !self.taskbar.is_null() {
            unsafe {
                let release: unsafe extern "system" fn(*mut TaskbarList3) -> u32 = mem::transmute(*(*self.taskbar).vtable.add(2));
                release(self.taskbar);
            }
        }
        if self.com_initialized { unsafe { CoUninitialize(); } }
        for font in self.fonts {
            if !font.is_null() {
                unsafe {
                    DeleteObject(font);
                }
            }
        }
        for font in self.symbol_fonts.into_iter().chain(self.emoji_fonts) {
            if !font.is_null() {
                unsafe {
                    DeleteObject(font);
                }
            }
        }
        for icon in self.icons {
            if !icon.is_null() {
                unsafe {
                    DestroyIcon(icon);
                }
            }
        }
    }

}
impl Window {
    unsafe fn set_status(&mut self, hwnd: HWND, status: StatusDot) {
        let pending = self
            .shared
            .pending_attention
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if self.status_dot == status {
            if status == StatusDot::NeedsAttention && !self.focused {
                if let Some((id, text)) = pending.as_ref() {
                    *self.shared.attention_target.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(id.clone());
                    notify_tray(
                        hwnd,
                        if self.tray_added { NIM_MODIFY } else { NIM_ADD },
                        Some(text),
                    );
                    *self.shared.pending_attention.lock().unwrap_or_else(|e| e.into_inner()) =
                        None;
                    self.tray_added = true;
                }
            } else if self.focused && self.tray_added {
                notify_tray(hwnd, NIM_DELETE, None);
                self.tray_added = false;
            }
            return;
        }
        self.status_dot = status;
        let icon = match status {
            StatusDot::Idle => null_mut(),
            StatusDot::Working | StatusDot::NeedsAttention => status_icon(16, super::theme::status_color(status)),
        };
        if status != StatusDot::Idle && !icon.is_null() {
            let index = if status == StatusDot::Working { 0 } else { 1 };
            if !self.overlay_icons[index].is_null() { DestroyIcon(self.overlay_icons[index]); }
            self.overlay_icons[index] = icon;
            if !self.taskbar.is_null() && native_signal_allowed(self.shared.test_control.is_none()) {
                taskbar_overlay(self.taskbar, hwnd, icon);
            }
        } else if !self.taskbar.is_null()
            && native_signal_allowed(self.shared.test_control.is_none())
        {
            taskbar_overlay(self.taskbar, hwnd, null_mut());
        }
        if status == StatusDot::NeedsAttention && !self.focused {
            let Some((id, text)) = pending.as_ref() else {
                return;
            };
            *self.shared.attention_target.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.clone());
            notify_tray(hwnd, if self.tray_added { NIM_MODIFY } else { NIM_ADD }, Some(text));
            *self.shared.pending_attention.lock().unwrap_or_else(|e| e.into_inner()) = None;
            self.tray_added = true;
        } else if status != StatusDot::NeedsAttention && self.tray_added {
            notify_tray(hwnd, NIM_DELETE, None);
            self.tray_added = false;
        }
    }

    fn emit(&self, event: Event) {
        send_until_closed(
            &self.events,
            NativeEvent::Event(event),
            &self.shared.closing,
        );
    }
    fn emit_native(&self, event: NativeEvent) {
        send_until_closed(&self.events, event, &self.shared.closing);
    }
    unsafe fn fonts(&mut self, hwnd: HWND, dpi: u32) {
        // Cached glyphs must be regenerated after a DPI/font change.
        self.surface = None;
        for cache in &mut self.glyph_fonts {
            cache.clear();
        }
        for font in &mut self.fonts {
            if !font.is_null() {
                DeleteObject(*font);
            }
            *font = null_mut();
        }
        for font in self
            .symbol_fonts
            .iter_mut()
            .chain(self.emoji_fonts.iter_mut())
        {
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
        for (family, fonts) in [
            ("Segoe UI Symbol", &mut self.symbol_fonts),
            ("Segoe UI Emoji", &mut self.emoji_fonts),
        ] {
            let face = wide(family);
            for (style, font) in fonts.iter_mut().enumerate() {
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
                    DEFAULT_PITCH as u32,
                    face.as_ptr(),
                );
            }
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
        if matches!(kind, MouseEventKind::Up(_)) {
            send_until_closed(
                &self.events,
                NativeEvent::MouseUp {
                    event,
                    x: x.div_euclid(self.cell_width.max(1)),
                    y: y.div_euclid(self.cell_height.max(1)),
                },
                &self.shared.closing,
            );
            return;
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
        let surface_background = super::theme::native_default(false);
        if self.surface.as_ref().is_none_or(|surface| {
            surface.width != client.right
                || surface.height != client.bottom
                || surface.background != surface_background
        }) {
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
            SetDCBrushColor(memory, color(Color::Reset, false));
            FillRect(memory, &client, GetStockObject(DC_BRUSH) as HBRUSH);
            self.surface = Some(Surface {
                dc: memory,
                bitmap,
                previous_bitmap,
                width: client.right,
                height: client.bottom,
                background: surface_background,
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
                if paint_status_symbol(memory, cell.symbol(), &rect) {
                    // These UI status marks have a fixed outline, independent of font linking.
                } else if cell.symbol() == "▀" || cell.symbol() == "█" {
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
                    if cell.symbol().is_ascii() {
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
                    } else {
                        paint_glyph(
                            memory,
                            cell.symbol(),
                            &rect,
                            [
                                self.fonts[style],
                                self.symbol_fonts[style],
                                self.emoji_fonts[style],
                            ],
                            &mut self.glyph_fonts[style],
                        );
                    }
                }
            }
        }
        if let Some(test) = self.shared.test_control.as_ref() {
            let mut state = test.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((_, started, _)) = &state.measure {
                let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                let filled = screen.cells.iter().filter(|cell| cell.symbol() != " ").count();
                if state.paints.len() < 1024 {
                    state.paints.push((elapsed, screen.seq, filled));
                }
            }
        }
        surface.columns = screen.size.width;
        surface.rows = screen.size.height;
        surface.cells = screen.cells;
        SelectObject(memory, original_font);
        // Caret is an overlay, never part of the cached backing image. Expose and
        // blink events therefore only copy cached pixels and repaint this rectangle.
        // Images are overlays too: clip them out of the copy so the blank cells beneath
        // never reach the screen between the copy and the image paint (visible flicker).
        let saved = SaveDC(dc);
        for image in &screen.images {
            if let Some(rect) = self.image_rect(image, &client) {
                ExcludeClipRect(dc, rect.left, rect.top, rect.right, rect.bottom);
            }
        }
        BitBlt(dc, 0, 0, client.right, client.bottom, memory, 0, 0, SRCCOPY);
        if saved != 0 {
            RestoreDC(dc, saved);
        }
        let [red, green, blue] = super::theme::image_background();
        let image_background =
            u32::from(blue) | (u32::from(green) << 8) | (u32::from(red) << 16);
        SetDCBrushColor(dc, image_background);
        for image in &screen.images {
            let area = RECT {
                left: i32::from(image.area.x) * self.cell_width,
                top: i32::from(image.area.y) * self.cell_height,
                right: (i32::from(image.area.right()) * self.cell_width).min(client.right),
                bottom: (i32::from(image.area.bottom()) * self.cell_height).min(client.bottom),
            };
            if area.right > area.left && area.bottom > area.top {
                FillRect(dc, &area, GetStockObject(DC_BRUSH) as HBRUSH);
            }
        }
        self.paint_images(dc, &screen.images, &client);
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
            SetDCBrushColor(dc, color(Color::Reset, true));
            FillRect(dc, &rect, GetStockObject(DC_BRUSH) as HBRUSH);
        }
        if supplied_dc.is_none() {
            EndPaint(hwnd, &paint);
        }
    }

    /// Pixel rectangle an image covers: scaled down to fit its cells, never up, top-left aligned.
    fn image_rect(&self, image: &RasterImage, client: &RECT) -> Option<RECT> {
        Self::image_rect_for(
            image.area,
            image.pixels.width(),
            image.pixels.height(),
            client,
            self.cell_width,
            self.cell_height,
        )
    }

    fn image_rect_for(
        area: Rect,
        width: u32,
        height: u32,
        client: &RECT,
        cell_width: i32,
        cell_height: i32,
    ) -> Option<RECT> {
        let left = i32::from(area.x) * cell_width;
        let top = i32::from(area.y) * cell_height;
        let available_width = (i32::from(area.width) * cell_width).min(client.right - left);
        let available_height = (i32::from(area.height) * cell_height).min(client.bottom - top);
        if available_width <= 0 || available_height <= 0 {
            return None;
        }
        let scale = (f64::from(available_width) / f64::from(width))
            .min(f64::from(available_height) / f64::from(height))
            .min(1.0);
        Some(RECT {
            left,
            top,
            right: left + (f64::from(width) * scale).floor().max(1.0) as i32,
            bottom: top + (f64::from(height) * scale).floor().max(1.0) as i32,
        })
    }

    unsafe fn paint_images(&mut self, dc: HDC, images: &[RasterImage], client: &RECT) {
        let background = super::theme::native_default(false);
        self.image_pixels.retain(|cached| {
            cached.background == background
                && images
                    .iter()
                    .any(|image| Arc::ptr_eq(&cached.source, &image.pixels))
        });
        for image in images {
            let index = if let Some(index) = self.image_pixels.iter().position(|cached| {
                cached.background == background && Arc::ptr_eq(&cached.source, &image.pixels)
            }) {
                index
            } else {
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
                        composite(blue, background[2]),
                        composite(green, background[1]),
                        composite(red, background[0]),
                        255,
                    ];
                }
                self.image_pixels.push(BitmapPixels {
                    source: image.pixels.clone(),
                    background,
                    bgra,
                });
                self.image_pixels.len() - 1
            };
            let Some(target) = self.image_rect(image, client) else {
                continue;
            };
            let cached = &self.image_pixels[index];
            let width = image.pixels.width();
            let height = image.pixels.height();
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
                target.left,
                target.top,
                target.right - target.left,
                target.bottom - target.top,
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
}

fn test_modifiers(value: Option<&serde_json::Value>) -> KeyModifiers {
    let mut modifiers = KeyModifiers::NONE;
    for name in value
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
    {
        modifiers |= match name.to_ascii_uppercase().as_str() {
            "CTRL" | "CONTROL" => KeyModifiers::CONTROL,
            "ALT" => KeyModifiers::ALT,
            "SHIFT" => KeyModifiers::SHIFT,
            "SUPER" | "META" => KeyModifiers::SUPER,
            _ => KeyModifiers::NONE,
        };
    }
    modifiers
}

fn test_key(value: &str) -> Option<KeyCode> {
    Some(match value {
        "Enter" => KeyCode::Enter,
        "Esc" | "Escape" => KeyCode::Esc,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Left" => KeyCode::Left,
        "Right" => KeyCode::Right,
        "Up" => KeyCode::Up,
        "Down" => KeyCode::Down,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "Delete" => KeyCode::Delete,
        "Insert" => KeyCode::Insert,
        value if value.len() == 1 => KeyCode::Char(value.chars().next()?),
        value
            if value
                .strip_prefix('F')
                .and_then(|n| n.parse::<u8>().ok())
                .is_some() =>
        {
            KeyCode::F(value[1..].parse().ok()?)
        }
        _ => return None,
    })
}

fn test_response(shared: &Shared, request: &serde_json::Value) -> serde_json::Value {
    if request.get("op").and_then(|v| v.as_str()) == Some("timing") {
        let Some(test) = shared.test_control.as_ref() else {
            return serde_json::json!({"ok": false, "error": "test control disabled"});
        };
        let mut state = test.state.lock().unwrap_or_else(|e| e.into_inner());
        let (label, start_seq) = state
            .measure
            .as_ref()
            .map(|(label, _, seq)| (label.clone(), *seq))
            .unwrap_or_default();
        let paints = state
            .paints
            .iter()
            .map(|(ms, seq, filled)| serde_json::json!({"ms": ms, "seq": seq, "filled": filled}))
            .collect::<Vec<_>>();
        let draws = std::mem::take(&mut state.draws);
        return serde_json::json!({
            "ok": true,
            "label": label,
            "start_seq": start_seq,
            "paints": paints,
            "draws": draws,
        });
    }
    if request.get("op").and_then(|v| v.as_str()) != Some("snapshot") {
        return serde_json::json!({"ok": true});
    }
    let screen = shared.screen.lock().unwrap_or_else(|e| e.into_inner());
    let Some(test) = shared.test_control.as_ref() else {
        return serde_json::json!({"ok": false, "error": "test control disabled"});
    };
    let state = test.state.lock().unwrap_or_else(|e| e.into_inner());
    let rows = (0..screen.size.height)
        .map(|row| {
            (0..screen.size.width)
                .map(|column| {
                    screen.cells
                        [usize::from(row) * usize::from(screen.size.width) + usize::from(column)]
                    .symbol()
                    .to_owned()
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "ok": true,
        "title": screen.title,
        "focused_tab": state.focused_tab,
        "overlays": state.overlays,
        "status_dot": state.status_dot,
        "columns": screen.size.width,
        "rows": screen.size.height,
        "grid": rows,
    })
}

unsafe fn handle_test_control(window: &mut Window) {
    let Some(test) = window.shared.test_control.as_ref() else {
        return;
    };
    let requests = {
        let mut queue = test.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.drain(..).collect::<Vec<_>>()
    };
    for request in requests {
        if let Some(label) = request.request.get("measure").and_then(|v| v.as_str()) {
            let seq = window.shared.screen.lock().unwrap_or_else(|e| e.into_inner()).seq;
            let shared = window.shared.clone();
            let test = shared.test_control.as_ref().expect("test control is enabled");
            let mut state = test.state.lock().unwrap_or_else(|e| e.into_inner());
            state.measure = Some((label.to_owned(), std::time::Instant::now(), seq));
            state.paints.clear();
        }
        match request.request.get("op").and_then(|v| v.as_str()) {
            // Simulated Alt+Tab: the same focus messages Windows sends, without changing
            // the foreground window of the user's desktop.
            Some("deactivate") => {
                let hwnd = window.shared.hwnd.load(Ordering::Acquire) as HWND;
                SendMessageW(hwnd, WM_ACTIVATE, WA_INACTIVE as WPARAM, 0);
                SendMessageW(hwnd, WM_KILLFOCUS, 0, 0);
            }
            Some("activate") => {
                let hwnd = window.shared.hwnd.load(Ordering::Acquire) as HWND;
                SendMessageW(hwnd, WM_ACTIVATE, WA_ACTIVE as WPARAM, 0);
                SendMessageW(hwnd, WM_SETFOCUS, 0, 0);
                // The synthetic focus message does not enter the desktop compositor, so
                // request the same repaint that a real activation exposes.
                InvalidateRect(hwnd, null(), 0);
                RedrawWindow(
                    hwnd,
                    null(),
                    null_mut(),
                    RDW_INVALIDATE | RDW_UPDATENOW | RDW_NOERASE,
                );
                UpdateWindow(hwnd);
                if window.shared.test_control.is_some() {
                    window.paint(hwnd, None);
                }
            }
            Some("key") => {
                if let Some(code) = request
                    .request
                    .get("code")
                    .and_then(|v| v.as_str())
                    .and_then(test_key)
                {
                    window.emit(Event::Key(KeyEvent::new(
                        code,
                        test_modifiers(request.request.get("modifiers")),
                    )));
                }
            }
            Some("text") => {
                if let Some(text) = request.request.get("text").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        window.emit(Event::Paste(text.to_owned()));
                    }
                }
            }
            Some("mouse") => {
                let x = request
                    .request
                    .get("x")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32
                    * window.cell_width;
                let y = request
                    .request
                    .get("y")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32
                    * window.cell_height;
                let kind = match request.request.get("kind").and_then(|v| v.as_str()) {
                    Some("down") => MouseEventKind::Down(MouseButton::Left),
                    Some("up") => MouseEventKind::Up(MouseButton::Left),
                    Some("drag") => MouseEventKind::Drag(MouseButton::Left),
                    Some("scroll_up") => MouseEventKind::ScrollUp,
                    Some("scroll_down") => MouseEventKind::ScrollDown,
                    _ => MouseEventKind::Moved,
                };
                window.mouse(kind, x, y);
            }
            Some("resize") => {
                let width = request
                    .request
                    .get("columns")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(120) as i32
                    * window.cell_width;
                let height = request
                    .request
                    .get("rows")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(40) as i32
                    * window.cell_height;
                SetWindowPos(
                    window.shared.hwnd.load(Ordering::Acquire) as HWND,
                    null_mut(),
                    0,
                    0,
                    width,
                    height,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            _ => {}
        }
        let response = if request.request.get("op").and_then(|v| v.as_str()) == Some("pixels") {
            pixel_check(window, &request.request)
        } else {
            test_response(&window.shared, &request.request)
        };
        let _ = request.reply.send(response.to_string());
    }
}

#[link(name = "user32")]
extern "system" {
    fn PrintWindow(hwnd: HWND, dc: HDC, flags: u32) -> i32;
}

/// A top-down 32-bit DIB selected into its own memory DC; test-only pixel access.
struct PixelBuffer {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u32,
    width: i32,
    height: i32,
}
impl PixelBuffer {
    unsafe fn new(width: i32, height: i32) -> Option<Self> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..mem::zeroed()
            },
            ..mem::zeroed()
        };
        let dc = CreateCompatibleDC(null_mut());
        let mut bits = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if dc.is_null() || bitmap.is_null() || bits.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            if !dc.is_null() {
                DeleteDC(dc);
            }
            return None;
        }
        let previous = SelectObject(dc, bitmap);
        Some(Self { dc, bitmap, previous, bits: bits.cast(), width, height })
    }
    fn pixel(&self, x: i32, y: i32) -> u32 {
        // SAFETY: callers stay inside the DIB, which lives as long as `self`.
        unsafe { *self.bits.add((y * self.width + x) as usize) & 0x00ff_ffff }
    }
}
impl Drop for PixelBuffer {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

/// Test-only proof that the window shows no stale pixels: compare the composited window
/// with a from-scratch paint of the current frame, and require every image area to hold
/// only the image itself and the image background.
unsafe fn pixel_check(window: &mut Window, request: &serde_json::Value) -> serde_json::Value {
    let hwnd = window.shared.hwnd.load(Ordering::Acquire) as HWND;
    let mut client: RECT = mem::zeroed();
    GetClientRect(hwnd, &mut client);
    let (Some(actual), Some(fresh)) = (
        PixelBuffer::new(client.right.max(1), client.bottom.max(1)),
        PixelBuffer::new(client.right.max(1), client.bottom.max(1)),
    ) else {
        return serde_json::json!({"ok": false, "error": "no pixel buffer"});
    };
    // Finish only the paints Windows already has pending; never invalidate more here, or
    // a missing invalidation would be hidden by the check itself.
    UpdateWindow(hwnd);
    let captured = PrintWindow(hwnd, actual.dc, 1 | 2) != 0; // client only, full content
    let blink = window.blink;
    window.blink = false;
    let cached = window.surface.take();
    window.paint(hwnd, Some(fresh.dc));
    window.surface = cached;
    window.blink = blink;
    // The caret blinks independently of frames; leave its cell out of the comparison.
    let caret = {
        let screen = window.shared.screen.lock().unwrap_or_else(|e| e.into_inner());
        screen.cursor_visible.then(|| {
            let left = i32::from(screen.cursor.x) * window.cell_width;
            let top = i32::from(screen.cursor.y) * window.cell_height;
            (left, top, left + window.cell_width, top + window.cell_height)
        })
    };
    let mut differ = 0usize;
    let mut first_difference = None;
    for y in 0..actual.height {
        for x in 0..actual.width {
            if caret.is_some_and(|(l, t, r, b)| x >= l && x < r && y >= t && y < b) {
                continue;
            }
            if actual.pixel(x, y) != fresh.pixel(x, y) {
                differ += 1;
                first_difference.get_or_insert((x, y));
            }
        }
    }
    let screen = window.shared.screen.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let [red, green, blue] = super::theme::image_background();
    let background = u32::from(blue) | (u32::from(green) << 8) | (u32::from(red) << 16);
    let mut images = Vec::new();
    for image in &screen.images {
        let area = RECT {
            left: i32::from(image.area.x) * window.cell_width,
            top: i32::from(image.area.y) * window.cell_height,
            right: (i32::from(image.area.right()) * window.cell_width).min(client.right),
            bottom: (i32::from(image.area.bottom()) * window.cell_height).min(client.bottom),
        };
        let drawn = window.image_rect(image, &client);
        let mut stale = 0usize;
        for y in area.top..area.bottom {
            for x in area.left..area.right {
                let inside = drawn.is_some_and(|d| x >= d.left && x < d.right && y >= d.top && y < d.bottom);
                if !inside && fresh.pixel(x, y) != background {
                    stale += 1;
                }
            }
        }
        images.push(serde_json::json!({
            "area": [area.left, area.top, area.right, area.bottom],
            "drawn": drawn.map(|d| vec![d.left, d.top, d.right, d.bottom]),
            "stale": stale,
        }));
    }
    // Callers name the exact colour of a synthetic image to prove a closed image left no pixels.
    let probe = request
        .get("color")
        .and_then(|v| v.as_str())
        .and_then(|hex| u32::from_str_radix(hex, 16).ok());
    let mut probe_count = 0usize;
    if let Some(color) = probe {
        for y in 0..actual.height {
            for x in 0..actual.width {
                probe_count += usize::from(actual.pixel(x, y) == color);
            }
        }
    }
    serde_json::json!({
        "ok": captured,
        "width": actual.width,
        "height": actual.height,
        "differ": differ,
        "first_difference": first_difference,
        "images": images,
        "color_count": probe_count,
    })
}

/// Tiny application status marks must never depend on a fallback font or script substitution.
/// Keep their ordinary Unicode text in the shared UI and copied output; only native paint is geometric.
unsafe fn paint_status_symbol(dc: HDC, symbol: &str, rect: &RECT) -> bool {
    if !matches!(symbol, "◉" | "✓" | "✗") {
        return false;
    }
    let width = (rect.right - rect.left).max(1);
    let height = (rect.bottom - rect.top).max(1);
    let size = (width - 2).max(1).min((height - 2).max(1));
    let left = rect.left + (width - size) / 2;
    let top = rect.top + (height - size) / 2;
    let stroke = (height / 12).max(1);
    let saved = SaveDC(dc);
    if saved == 0 {
        return false;
    }
    let pen = CreatePen(PS_SOLID, stroke, GetTextColor(dc));
    if pen.is_null() {
        RestoreDC(dc, saved);
        return false;
    }
    IntersectClipRect(dc, rect.left, rect.top, rect.right, rect.bottom);
    SetDCBrushColor(dc, GetBkColor(dc));
    FillRect(dc, rect, GetStockObject(DC_BRUSH) as HBRUSH);
    let old_pen = SelectObject(dc, pen);
    let old_brush = SelectObject(dc, GetStockObject(HOLLOW_BRUSH));
    match symbol {
        "✓" => {
            MoveToEx(dc, left, top + size / 2, null_mut());
            LineTo(dc, left + size / 3, top + size - 1);
            LineTo(dc, left + size, top);
        }
        "✗" => {
            MoveToEx(dc, left, top, null_mut());
            LineTo(dc, left + size, top + size);
            MoveToEx(dc, left + size, top, null_mut());
            LineTo(dc, left, top + size);
        }
        _ => {
            Ellipse(dc, left, top, left + size + 1, top + size + 1);
            SetDCBrushColor(dc, GetTextColor(dc));
            SelectObject(dc, GetStockObject(DC_BRUSH));
            let inset = (size / 3).max(1);
            Ellipse(
                dc,
                left + inset,
                top + inset,
                left + size - inset + 1,
                top + size - inset + 1,
            );
        }
    }
    SelectObject(dc, old_brush);
    SelectObject(dc, old_pen);
    DeleteObject(pen);
    RestoreDC(dc, saved);
    true
}

#[cfg(test)]
pub(super) fn status_fixture_proof() {
    unsafe {
        let dc = CreateCompatibleDC(null_mut());
        assert!(!dc.is_null());
        let bitmap = CreateBitmap(32, 40, 1, 32, null());
        assert!(!bitmap.is_null());
        let previous = SelectObject(dc, bitmap);
        let whole = RECT {
            left: 0,
            top: 0,
            right: 32,
            bottom: 40,
        };
        let rect = RECT {
            left: 10,
            top: 8,
            right: 20,
            bottom: 30,
        };
        let background = rgb(20, 23, 31);
        let foreground = rgb(100, 210, 130);
        SetTextColor(dc, foreground);
        SetBkColor(dc, background);
        for symbol in ["◉", "✓", "✗"] {
            SetDCBrushColor(dc, background);
            FillRect(dc, &whole, GetStockObject(DC_BRUSH) as HBRUSH);
            assert!(paint_status_symbol(dc, symbol, &rect));
            let mut ink = 0;
            for y in 0..40 {
                for x in 0..32 {
                    let pixel = GetPixel(dc, x, y);
                    if x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom {
                        ink += usize::from(pixel == foreground);
                    } else {
                        assert_eq!(pixel, background, "status mark escaped its cell");
                    }
                }
            }
            assert!(ink > 0, "status mark did not draw: {symbol}");
        }
        assert!(!paint_status_symbol(dc, "é", &rect));
        SelectObject(dc, previous);
        DeleteObject(bitmap);
        DeleteDC(dc);
    }
}

/// Glyphs that neither the window font nor its Segoe UI Symbol fallback can draw.
/// The emoji font is excluded on purpose: icons must not depend on it.
#[cfg(test)]
pub(super) fn glyphs_without_font(glyphs: &[String]) -> Vec<String> {
    unsafe {
        let dc = CreateCompatibleDC(null_mut());
        assert!(!dc.is_null());
        let fonts = ["Consolas", "Segoe UI Symbol"].map(|family| {
            let face = wide(family);
            CreateFontW(
                -18,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                FIXED_PITCH as u32 | FF_MODERN as u32,
                face.as_ptr(),
            )
        });
        let previous = SelectObject(dc, fonts[0]);
        let missing = glyphs
            .iter()
            .filter(|glyph| {
                let text: Vec<u16> = glyph.encode_utf16().collect();
                let mut indices = vec![0; text.len()];
                !fonts.iter().any(|font| {
                    SelectObject(dc, *font);
                    GetGlyphIndicesW(
                        dc,
                        text.as_ptr(),
                        text.len() as i32,
                        indices.as_mut_ptr(),
                        GGI_MARK_NONEXISTING_GLYPHS,
                    ) != GDI_ERROR as u32
                        && indices.iter().all(|index| *index != 0xffff)
                })
            })
            .cloned()
            .collect();
        SelectObject(dc, previous);
        for font in fonts {
            DeleteObject(font);
        }
        DeleteDC(dc);
        missing
    }
}

/// GDI's fixed Consolas cell metrics are not the natural metrics of symbol/emoji
/// fonts. Choose an installed glyph face, then fit its complete ink into the
/// allocated terminal cells rather than clipping a wider glyph at the right edge.
unsafe fn paint_glyph(
    dc: HDC,
    symbol: &str,
    rect: &RECT,
    fonts: [HFONT; 3],
    cache: &mut std::collections::HashMap<String, u8>,
) {
    let text: Vec<u16> = symbol
        .chars()
        .filter(|c| !matches!(c, '\u{fe0e}' | '\u{fe0f}'))
        .flat_map(|c| {
            let mut units = [0; 2];
            let count = c.encode_utf16(&mut units).len();
            units.into_iter().take(count)
        })
        .collect();
    if text.is_empty() {
        return;
    }
    let family = if let Some(family) = cache.get(symbol) {
        *family
    } else {
        let mut chosen = 0;
        if symbol.chars().any(|c| c as u32 > 0xffff) {
            chosen = 2;
        } else {
            let mut indices = vec![0; text.len()];
            for (index, font) in fonts.iter().enumerate() {
                SelectObject(dc, *font);
                if GetGlyphIndicesW(
                    dc,
                    text.as_ptr(),
                    text.len() as i32,
                    indices.as_mut_ptr(),
                    GGI_MARK_NONEXISTING_GLYPHS,
                ) != GDI_ERROR as u32
                    && indices.iter().all(|glyph| *glyph != 0xffff)
                {
                    chosen = index as u8;
                    break;
                }
            }
        }
        if symbol.len() <= 64 {
            if cache.len() >= 512 {
                cache.clear();
            }
            cache.insert(symbol.into(), chosen);
        }
        chosen
    };
    SelectObject(dc, fonts[usize::from(family)]);
    let saved = SaveDC(dc);
    if saved == 0 {
        return;
    }
    let mut measured: windows_sys::Win32::Foundation::SIZE = mem::zeroed();
    if GetTextExtentPoint32W(dc, text.as_ptr(), text.len() as i32, &mut measured) == 0 {
        RestoreDC(dc, saved);
        return;
    }
    // Advance widths exclude negative side bearings (notably italic symbols).
    // Include their actual ink, then translate it back inside the destination.
    let mut left = 0_i32;
    let mut right = measured.cx;
    let mut top = 0_i32;
    let mut bottom = measured.cy;
    if let Some(first) = text
        .first()
        .copied()
        .filter(|unit| !(0xd800..=0xdfff).contains(unit))
    {
        let mut abc: ABC = mem::zeroed();
        if GetCharABCWidthsW(dc, u32::from(first), u32::from(first), &mut abc) != 0 {
            left = left.min(abc.abcA);
        }
    }
    if let Some(last) = text
        .last()
        .copied()
        .filter(|unit| !(0xd800..=0xdfff).contains(unit))
    {
        let mut abc: ABC = mem::zeroed();
        if GetCharABCWidthsW(dc, u32::from(last), u32::from(last), &mut abc) != 0 {
            right = right.max(measured.cx.saturating_sub(abc.abcC));
        }
    }
    if text.len() == 1 {
        let mut transform: MAT2 = mem::zeroed();
        transform.eM11.value = 1;
        transform.eM22.value = 1;
        let mut glyph: GLYPHMETRICS = mem::zeroed();
        let mut metrics: TEXTMETRICW = mem::zeroed();
        if GetTextMetricsW(dc, &mut metrics) != 0
            && GetGlyphOutlineW(
                dc,
                u32::from(text[0]),
                GGO_METRICS,
                &mut glyph,
                0,
                null_mut(),
                &transform,
            ) != GDI_ERROR as u32
        {
            left = left.min(glyph.gmptGlyphOrigin.x);
            right = right.max(
                glyph
                    .gmptGlyphOrigin
                    .x
                    .saturating_add(glyph.gmBlackBoxX as i32),
            );
            let ink_top = metrics.tmAscent.saturating_sub(glyph.gmptGlyphOrigin.y);
            top = top.min(ink_top);
            bottom = bottom.max(ink_top.saturating_add(glyph.gmBlackBoxY as i32));
        }
    }
    let ink_width = right.saturating_sub(left).max(1);
    let ink_height = bottom.saturating_sub(top).max(1);
    let width = (rect.right - rect.left).max(1);
    let height = (rect.bottom - rect.top).max(1);
    let scale = (width as f32 / ink_width as f32)
        .min(height as f32 / ink_height as f32)
        .min(1.0);
    SetDCBrushColor(dc, GetBkColor(dc));
    FillRect(dc, rect, GetStockObject(DC_BRUSH) as HBRUSH);
    IntersectClipRect(dc, rect.left, rect.top, rect.right, rect.bottom);
    SetBkMode(dc, TRANSPARENT as i32);
    SetGraphicsMode(dc, GM_ADVANCED);
    let transform = XFORM {
        eM11: scale,
        eM12: 0.0,
        eM21: 0.0,
        eM22: scale,
        eDx: rect.left as f32 + (width as f32 - ink_width as f32 * scale) / 2.0
            - left as f32 * scale,
        eDy: rect.top as f32 + (height as f32 - ink_height as f32 * scale) / 2.0
            - top as f32 * scale,
    };
    if SetWorldTransform(dc, &transform) != 0 {
        ExtTextOutW(
            dc,
            0,
            0,
            0,
            null(),
            text.as_ptr(),
            text.len() as u32,
            null(),
        );
    }
    RestoreDC(dc, saved);
}

/// Load the executable's master icon at the requested native size. The resource
/// includes 16/32/48/256-pixel frames shared by Explorer, taskbar and titlebar.
/// Without LR_SHARED the window owns this handle and releases it on shutdown.
unsafe fn app_icon(size: i32, frame: Option<u8>) -> HICON {
    if let Some(portrait) = super::brand::portrait() {
        let mut pixels = image::imageops::resize(
            &*portrait,
            size as u32,
            size as u32,
            image::imageops::FilterType::Lanczos3,
        );
        let color = super::theme::brand_color();
        if let Some(frame) = frame {
            // Native pixels, not a title glyph: a short orbit around the original mark.
            let inset = (size / 6).max(2) as u32;
            let inner = image::imageops::resize(
                &pixels,
                size as u32 - inset * 2,
                size as u32 - inset * 2,
                image::imageops::FilterType::Lanczos3,
            );
            pixels = image::RgbaImage::new(size as u32, size as u32);
            image::imageops::overlay(&mut pixels, &inner, inset.into(), inset.into());
            let center = (size - 1) as f32 / 2.0;
            let radius = center - 0.8;
            for segment in 0..12u8 {
                let angle = f32::from(segment) * std::f32::consts::TAU / 12.0;
                let x = (center + radius * angle.cos()).round() as u32;
                let y = (center + radius * angle.sin()).round() as u32;
                let behind = (frame + 12 - segment) % 12;
                let alpha = match behind {
                    0 => 255,
                    1 => 200,
                    2 => 130,
                    3 => 75,
                    _ => 25,
                };
                pixels.put_pixel(x, y, image::Rgba([255, 255, 255, alpha]));
            }
        }
        let mut data = vec![0u8; 40];
        let mut header = |offset: usize, value: u32| {
            data[offset..offset + 4].copy_from_slice(&value.to_le_bytes())
        };
        header(0, 40);
        header(4, size as u32);
        header(8, (size * 2) as u32);
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&32u16.to_le_bytes());
        for y in (0..size as u32).rev() {
            for x in 0..size as u32 {
                let p = pixels.get_pixel_mut(x, y);
                let shade = f32::from(p[0].max(p[1]).max(p[2])) / 255.0;
                let tint = color.map(|v| (f32::from(v) * shade).round() as u8);
                data.extend_from_slice(&[tint[2], tint[1], tint[0], p[3]]);
            }
        }
        data.resize(
            data.len() + ((size as usize).div_ceil(32) * 4) * size as usize,
            0,
        );
        let icon = CreateIconFromResourceEx(
            data.as_mut_ptr(),
            data.len() as u32,
            1,
            0x00030000,
            size,
            size,
            0,
        );
        if !icon.is_null() {
            return icon;
        }
    }
    LoadImageW(
        GetModuleHandleW(null()),
        // MAKEINTRESOURCEW(1): resource identifier, never a dereferenced string pointer.
        std::ptr::without_provenance(1),
        IMAGE_ICON,
        size,
        size,
        0,
    ) as HICON
}

unsafe fn refresh_icon(hwnd: HWND, window: &mut Window) {
    let next = [
        app_icon(32, window.icon_active.then_some(window.icon_frame)),
        app_icon(16, window.icon_active.then_some(window.icon_frame)),
    ];
    for (index, icon) in next.into_iter().enumerate() {
        if !icon.is_null() {
            SendMessageW(
                hwnd,
                WM_SETICON,
                if index == 0 { ICON_BIG } else { ICON_SMALL } as usize,
                icon as LPARAM,
            );
            if !window.icons[index].is_null() {
                DestroyIcon(window.icons[index]);
            }
            window.icons[index] = icon;
        }
    }
    window.icon_color = super::theme::brand_color();
}

unsafe fn taskbar_hr_init(taskbar: *mut TaskbarList3) {
    let method: unsafe extern "system" fn(*mut TaskbarList3) -> i32 = mem::transmute(*(*taskbar).vtable.add(3));
    let _ = method(taskbar);
}

unsafe fn taskbar_overlay(taskbar: *mut TaskbarList3, hwnd: HWND, icon: HICON) {
    let method: unsafe extern "system" fn(*mut TaskbarList3, HWND, HICON, *const u16) -> i32 = mem::transmute(*(*taskbar).vtable.add(17));
    let _ = method(taskbar, hwnd, icon, null());
}

unsafe fn status_icon(size: i32, [red, green, blue]: crate::config::Rgb) -> HICON {
    let mut pixels = image::RgbaImage::new(size as u32, size as u32);
    let center = (size as f32 - 1.0) / 2.0;
    let radius = center - 1.0;
    for y in 0..size as u32 {
        for x in 0..size as u32 {
            let distance = ((x as f32 - center).powi(2) + (y as f32 - center).powi(2)).sqrt();
            if distance <= radius { pixels.put_pixel(x, y, image::Rgba([blue, green, red, 255])); }
        }
    }
    let mut data = vec![0u8; 40];
    let mut header = |offset: usize, value: u32| data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    header(0, 40); header(4, size as u32); header(8, (size * 2) as u32);
    data[12..14].copy_from_slice(&1u16.to_le_bytes());
    data[14..16].copy_from_slice(&32u16.to_le_bytes());
    for y in (0..size as u32).rev() { for x in 0..size as u32 { data.extend_from_slice(&pixels.get_pixel(x, y).0); } }
    data.resize(data.len() + ((size as usize).div_ceil(32) * 4) * size as usize, 0);
    CreateIconFromResourceEx(data.as_mut_ptr(), data.len() as u32, 1, 0x00030000, size, size, 0)
}

unsafe fn notify_tray(hwnd: HWND, operation: u32, message: Option<&str>) {
    // Windows driven by the test control channel are hidden; they must never notify the user.
    if !native_signal_allowed(std::env::var_os("CONCORDIA_TEST_CONTROL").is_some()) {
        return;
    }
    let mut data: NOTIFYICONDATAW = mem::zeroed();
    data.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_ID;
    data.uFlags = NIF_MESSAGE | NIF_ICON | if message.is_some() { NIF_INFO } else { 0 };
    data.uCallbackMessage = TRAY_NOTIFY;
    data.hIcon = status_icon(16, super::theme::status_color(StatusDot::NeedsAttention));
    if let Some(message) = message {
        let mut text = message.encode_utf16().take(255).collect::<Vec<_>>();
        text.resize(256, 0);
        data.szInfo.copy_from_slice(&text);
        let mut title = "Concordia".encode_utf16().collect::<Vec<_>>();
        title.resize(64, 0);
        data.szInfoTitle.copy_from_slice(&title);
        data.dwInfoFlags = 1;
    }
    let _ = Shell_NotifyIconW(operation, &data);
    if !data.hIcon.is_null() { DestroyIcon(data.hIcon); }
}

fn native_signal_allowed(test_control: bool) -> bool {
    !test_control
}

#[cfg(test)]
mod signal_tests {
    use super::native_signal_allowed;

    #[test]
    fn test_control_blocks_all_shell_signals() {
        assert!(!native_signal_allowed(true));
        assert!(native_signal_allowed(false));
    }
}

unsafe fn window_thread(
    shared: Arc<Shared>,
    events: SyncSender<NativeEvent>,
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
        symbol_fonts: [null_mut(); 16],
        emoji_fonts: [null_mut(); 16],
        glyph_fonts: std::array::from_fn(|_| std::collections::HashMap::new()),
        icons: [app_icon(32, None), app_icon(16, None)],
        overlay_icons: [null_mut(), null_mut()],
        taskbar: null_mut(),
        com_initialized: false,
        status_dot: StatusDot::Idle,
        tray_added: false,
        icon_color: super::theme::brand_color(),
        icon_active: false,
        icon_frame: 0,
        cell_width: 9,
        cell_height: 20,
        surrogate: None,
        pressed: None,
        focused: false,
        blink: true,
        wheel: 0,
        last_motion: None,
        surface: None,
        image_pixels: Vec::new(),
    });
    let dpi = GetDpiForSystem().clamp(96, 768) as i32;
    let initial_width = (1120 * dpi / 96).min((GetSystemMetrics(SM_CXSCREEN) * 9 / 10).max(400));
    let initial_height = (860 * dpi / 96).min((GetSystemMetrics(SM_CYSCREEN) * 9 / 10).max(280));
    let test_window = shared.test_control.is_some();
    let hwnd = CreateWindowExW(
        WS_EX_ACCEPTFILES | if test_window { WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE } else { 0 },
        class.as_ptr(),
        wide("Concordia").as_ptr(),
        WS_OVERLAPPEDWINDOW,
        if test_window { -32000 } else { CW_USEDEFAULT },
        if test_window { -32000 } else { CW_USEDEFAULT },
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
    let com_initialized = CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED as u32) >= 0;
    window.com_initialized = com_initialized;
    if com_initialized {
        let mut taskbar = null_mut();
        if CoCreateInstance(
            &CLSID_TASKBAR_LIST,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_ITASKBAR_LIST3,
            (&mut taskbar as *mut *mut TaskbarList3).cast(),
        ) >= 0 {
            window.taskbar = taskbar;
            taskbar_hr_init(taskbar);
        }
    }
    shared.hwnd.store(hwnd as isize, Ordering::Release);
    start_test_control(&shared, hwnd);
    if !window.icons[0].is_null() {
        SendMessageW(
            hwnd,
            WM_SETICON,
            ICON_BIG as usize,
            window.icons[0] as LPARAM,
        );
    }
    if !window.icons[1].is_null() {
        SendMessageW(
            hwnd,
            WM_SETICON,
            ICON_SMALL as usize,
            window.icons[1] as LPARAM,
        );
    }
    window.fonts(hwnd, GetDpiForWindow(hwnd));
    window.resized(hwnd);
    DragAcceptFiles(hwnd, 1);
    SetTimer(hwnd, 1, 500, None);
    set_caption_color(hwnd);
    // Keep journey windows rendered at their off-screen position.  SW_HIDE prevents normal
    // paint delivery on some Windows versions, while NOACTIVATE and TOOLWINDOW keep this
    // test-only window out of focus and the taskbar.
    ShowWindow(hwnd, if test_window { SW_SHOWNOACTIVATE } else { SW_SHOW });
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

/// Test-only control is an authenticated, local loopback channel. It is deliberately
/// attached to the native message thread: injected events and real Win32 input share
/// the same `Window::emit`/`Window::mouse` path and therefore the same input precedence.
fn start_test_control(shared: &Arc<Shared>, hwnd: HWND) {
    let Some(spec) = std::env::var_os("CONCORDIA_TEST_CONTROL") else {
        return;
    };
    let Ok(spec) = spec.into_string() else { return };
    let Some((address, token)) = spec.split_once(';') else {
        return;
    };
    let Ok(address) = address.parse::<SocketAddr>() else {
        return;
    };
    if !address.ip().is_loopback() || token.is_empty() {
        return;
    }
    let Ok(listener) = TcpListener::bind(address) else {
        return;
    };
    let hwnd = hwnd as isize;
    let token = token.to_owned();
    let shared = shared.clone();
    thread::Builder::new()
        .name("concordia-test-control".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let shared = shared.clone();
                let token = token.to_owned();
                thread::spawn(move || test_control_client(stream, shared, hwnd, &token));
            }
        })
        .ok();
}

fn test_control_client(stream: TcpStream, shared: Arc<Shared>, hwnd: isize, token: &str) {
    use std::io::{BufRead, BufReader, Write};
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.is_empty() {
            break;
        }
        let Ok(request) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if request.get("token").and_then(|v| v.as_str()) != Some(token) {
            continue;
        }
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let Some(test) = shared.test_control.as_ref() else {
            break;
        };
        test.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(TestRequest {
                request,
                reply: reply_tx,
            });
        unsafe {
            PostMessageW(hwnd as HWND, TEST_CONTROL, 0, 0);
        }
        let Ok(reply) = reply_rx.recv_timeout(Duration::from_secs(5)) else {
            break;
        };
        if reader.get_mut().write_all(reply.as_bytes()).is_err() {
            break;
        }
        if reader.get_mut().write_all(b"\n").is_err() {
            break;
        }
        let _ = reader.get_mut().flush();
    }
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
        TEST_CONTROL => {
            handle_test_control(&mut *pointer);
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
            | TRAY_NOTIFY
            | TEST_CONTROL
            | WM_SETCURSOR
            | WM_TIMER
            | WM_SIZE
            | WM_DPICHANGED
            | WM_ACTIVATE
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
        WM_ACTIVATE => {
            if (wparam as u32 & 0xffff) != WA_INACTIVE {
                // Activation repaints the cached surface without a new application
                // frame. Give the paint probe a new visible-frame sequence so it can
                // distinguish that repaint from the frame shown before deactivation.
                window
                    .shared
                    .screen
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .seq += 1;
                InvalidateRect(hwnd, null(), 0);
                RedrawWindow(
                    hwnd,
                    null(),
                    null_mut(),
                    RDW_INVALIDATE | RDW_UPDATENOW | RDW_NOERASE,
                );
            }
            0
        }
        UPDATE => {
            let status = *window.shared.requested_status.lock().unwrap_or_else(|e| e.into_inner());
            window.set_status(hwnd, status);
            set_caption_color(hwnd);
            if super::theme::brand_color() != window.icon_color {
                refresh_icon(hwnd, window);
            }
            window.blink = true;
            InvalidateRect(hwnd, null(), 0);
            0
        }
        TRAY_NOTIFY if lparam as u32 == WM_LBUTTONUP || lparam as u32 == WM_LBUTTONDBLCLK => {
            if window.status_dot == StatusDot::NeedsAttention {
                SetForegroundWindow(hwnd);
                if let Some(item_id) = window
                    .shared
                    .attention_target
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                {
                    window.emit_native(NativeEvent::Attention(item_id));
                }
                notify_tray(hwnd, NIM_DELETE, None);
                window.tray_added = false;
            }
            0
        }
        TITLE => {
            let title = window
                .shared
                .screen
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .title
                .clone();
            let parts = title.split(" | ").collect::<Vec<_>>();
            let active = parts.len() >= 3
                && matches!(
                    parts[1],
                    "Thinking"
                        | "Writing"
                        | "Working"
                        | "Editing"
                        | "Reading"
                        | "Searching"
                        | "Searching the web"
                        | "Running command"
                        | "Workers running"
                        | "Updating"
                );
            if active != window.icon_active {
                window.icon_active = active;
                window.icon_frame = 0;
                refresh_icon(hwnd, window);
            }
            SetWindowTextW(hwnd, wide(&title).as_ptr());
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
            if window.icon_active && super::theme::motion_enabled() {
                let mut enabled = 1i32;
                let animated = SystemParametersInfoW(
                    SPI_GETCLIENTAREAANIMATION,
                    0,
                    (&mut enabled as *mut i32).cast(),
                    0,
                ) != 0
                    && enabled != 0;
                if animated {
                    window.icon_frame = (window.icon_frame + 1) % 12;
                    refresh_icon(hwnd, window);
                }
            }
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
            let status = *window.shared.requested_status.lock().unwrap_or_else(|e| e.into_inner());
            window.set_status(hwnd, status);
            window.emit(Event::FocusGained);
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_KILLFOCUS => {
            window.focused = false;
            let status = *window.shared.requested_status.lock().unwrap_or_else(|e| e.into_inner());
            window.set_status(hwnd, status);
            let pressed = window.pressed.take();
            if let Some(button) = pressed {
                let mut point: POINT = mem::zeroed();
                GetCursorPos(&mut point);
                ScreenToClient(hwnd, &mut point);
                // Focus loss cancels selection capture; it is not a captured release message
                // and must not detach a file based on a later global cursor position.
                let (column, row) = window.coordinates(point.x, point.y);
                window.emit(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(button),
                    column,
                    row,
                    modifiers: modifiers(),
                }));
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

unsafe fn set_caption_color(hwnd: HWND) {
    let [red, green, blue] = super::theme::native_default(false);
    // DWMWA_CAPTION_COLOR takes a COLORREF (BGR packed in a DWORD), not a ratatui RGB value.
    let color = u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16);
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_CAPTION_COLOR,
        (&color as *const u32).cast(),
        std::mem::size_of::<u32>() as u32,
    );
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
        0xc0 if modifiers.contains(KeyModifiers::CONTROL)
            && !modifiers.contains(KeyModifiers::ALT) =>
        {
            KeyCode::Char('`')
        }
        // Ctrl+1..9 selects a tab; AltGr (Ctrl+Alt) still types its character.
        0x31..=0x39
            if modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
        {
            KeyCode::Char((b'1' + (vk - 0x31) as u8) as char)
        }
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
            let [red, green, blue] = super::theme::native_default(foreground);
            return rgb(red, green, blue);
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

#[cfg(test)]
mod image_rect_tests {
    use super::*;

    #[test]
    fn preview_preserves_source_aspect_ratio_when_fitted_to_cells() {
        let client = RECT {
            left: 0,
            top: 0,
            right: 400,
            bottom: 400,
        };
        let rect = Window::image_rect_for(Rect::new(0, 0, 20, 20), 400, 200, &client, 9, 20)
            .unwrap();
        assert_eq!(rect.right - rect.left, 180);
        assert_eq!(rect.bottom - rect.top, 90);
    }
}

