//! Bounded image/file capture on an explicit paste. Never reads clipboard text.
use image::ImageEncoder;
use std::{io, mem, path::PathBuf, ptr::null_mut, time::Duration};
use windows_sys::Win32::{
    Foundation::HGLOBAL,
    Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
    },
    System::{
        DataExchange::{
            CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
            RegisterClipboardFormatW,
        },
        Memory::{GlobalLock, GlobalSize, GlobalUnlock},
    },
    UI::Shell::DragQueryFileW,
};

const BITMAP_FORMAT: u32 = 2;
const FILES_FORMAT: u32 = 15;
const MAX_EDGE: u32 = 4096;
const MAX_PIXELS: usize = 16_000_000;

pub(super) enum Capture {
    Image(Vec<u8>),
    Files(Vec<PathBuf>),
    Empty,
}

struct Clipboard;
impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}
struct Locked(HGLOBAL);
impl Drop for Locked {
    fn drop(&mut self) {
        unsafe {
            GlobalUnlock(self.0);
        }
    }
}
struct Dc(HDC);
impl Drop for Dc {
    fn drop(&mut self) {
        unsafe {
            DeleteDC(self.0);
        }
    }
}

fn open() -> Result<Clipboard, String> {
    for attempt in 0..8 {
        // SAFETY: a null owner opens the clipboard for reading only.
        if unsafe { OpenClipboard(null_mut()) } != 0 {
            return Ok(Clipboard);
        }
        if attempt < 7 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    Err("Clipboard is busy; paste again".into())
}

pub(super) fn read() -> Result<Capture, String> {
    let clipboard = open()?;
    // RegisterClipboardFormat only obtains the format id; it changes no clipboard contents.
    let png = unsafe { RegisterClipboardFormatW([80u16, 78, 71, 0].as_ptr()) };
    let mut png_error = None;
    if png != 0 && unsafe { IsClipboardFormatAvailable(png) } != 0 {
        match copy_png(png) {
            Ok(bytes) => return Ok(Capture::Image(bytes)),
            Err(error) => png_error = Some(error),
        }
    }
    // Windows synthesizes CF_BITMAP from CF_DIB/CF_DIBV5 when necessary. This is a
    // borrowed clipboard handle: neither this function nor the encoder deletes it.
    let bitmap = unsafe { GetClipboardData(BITMAP_FORMAT) };
    if !bitmap.is_null() {
        return encode_bitmap(bitmap, || drop(clipboard)).map(Capture::Image);
    }
    let files = unsafe { GetClipboardData(FILES_FORMAT) };
    if !files.is_null() {
        let count = unsafe { DragQueryFileW(files, u32::MAX, null_mut(), 0) };
        if !(1..=4).contains(&count) {
            return Err("Paste up to four image or document files".into());
        }
        let mut paths = Vec::new();
        let mut total = 0usize;
        for index in 0..count {
            let len = unsafe { DragQueryFileW(files, index, null_mut(), 0) };
            if len == 0 || len > 4096 {
                return Err("Clipboard file path exceeds 4096 characters".into());
            }
            let mut wide = vec![0u16; len as usize + 1];
            if unsafe { DragQueryFileW(files, index, wide.as_mut_ptr(), wide.len() as u32) } != len
            {
                return Err("Clipboard file list changed while reading".into());
            }
            let path = String::from_utf16(&wide[..len as usize])
                .map_err(|_| "Clipboard file path is not valid Unicode")?;
            total = total.saturating_add(path.len() + 3);
            if path.len() > 4096 || total > 16 * 1024 {
                return Err("Clipboard file paths exceed the attachment limit".into());
            }
            paths.push(PathBuf::from(path));
        }
        // The CF_HDROP belongs to the clipboard: do not call DragFinish on it.
        return Ok(Capture::Files(paths));
    }
    match png_error {
        Some(error) => Err(error),
        None => Ok(Capture::Empty),
    }
}

fn copy_png(format: u32) -> Result<Vec<u8>, String> {
    let handle = unsafe { GetClipboardData(format) };
    if handle.is_null() {
        return Err("Clipboard PNG is unavailable".into());
    }
    let size = unsafe { GlobalSize(handle) };
    if size == 0 || size > crate::attachments::MAX_BYTES {
        return Err("Clipboard PNG exceeds the 4 MiB attachment limit".into());
    }
    let data = unsafe { GlobalLock(handle) };
    if data.is_null() {
        return Err("Clipboard PNG cannot be read".into());
    }
    let _locked = Locked(handle);
    // SAFETY: GlobalSize bounds the live locked memory and the clipboard remains open.
    let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) };
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("Clipboard PNG has an invalid signature".into());
    }
    Ok(bytes.to_vec())
}

/// Copy borrowed GDI pixels, then release any clipboard guard before compression.
/// The callback also lets an isolated probe use its own bitmap without opening a clipboard.
pub(super) fn encode_bitmap(bitmap: HBITMAP, release: impl FnOnce()) -> Result<Vec<u8>, String> {
    let pixels = bitmap_pixels(bitmap);
    release();
    let (width, height, rgba) = pixels?;
    let mut output = BoundedPng {
        bytes: Vec::new(),
        exceeded: false,
    };
    let encoded = image::codecs::png::PngEncoder::new(&mut output).write_image(
        &rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    );
    if output.exceeded {
        return Err("Clipboard image exceeds the 4 MiB PNG attachment limit".into());
    }
    encoded.map_err(|_| "Clipboard image could not be encoded as PNG")?;
    Ok(output.bytes)
}

fn bitmap_pixels(bitmap: HBITMAP) -> Result<(u32, u32, Vec<u8>), String> {
    let mut object: BITMAP = unsafe { mem::zeroed() };
    if unsafe {
        GetObjectW(
            bitmap,
            mem::size_of::<BITMAP>() as i32,
            (&mut object as *mut BITMAP).cast(),
        )
    } != mem::size_of::<BITMAP>() as i32
    {
        return Err("Clipboard bitmap metadata is unavailable".into());
    }
    let width = u32::try_from(object.bmWidth).map_err(|_| "Invalid clipboard bitmap width")?;
    let height = object
        .bmHeight
        .checked_abs()
        .and_then(|height| u32::try_from(height).ok())
        .ok_or("Invalid clipboard bitmap height")?;
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .ok_or("Clipboard bitmap dimensions overflow")?;
    if width == 0 || height == 0 || width > MAX_EDGE || height > MAX_EDGE || pixels > MAX_PIXELS {
        return Err(
            "Clipboard image exceeds preview limits (4096 per edge / 16 megapixels)".into(),
        );
    }
    let bytes = pixels
        .checked_mul(4)
        .filter(|bytes| *bytes <= 64 * 1024 * 1024)
        .ok_or("Clipboard pixels exceed the 64 MiB limit")?;
    let mut rgba = vec![0u8; bytes];
    let mut info: BITMAPINFO = unsafe { mem::zeroed() };
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        biHeight: -(height as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        biSizeImage: bytes as u32,
        ..unsafe { mem::zeroed() }
    };
    let dc = unsafe { CreateCompatibleDC(null_mut()) };
    if dc.is_null() {
        return Err("Clipboard bitmap conversion is unavailable".into());
    }
    let _dc = Dc(dc);
    // Do not select or delete this borrowed HBITMAP. GetDIBits copies its pixels into our buffer.
    if unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height,
            rgba.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        )
    } != height as i32
    {
        return Err("Clipboard bitmap pixels could not be read".into());
    }
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    Ok((width, height, rgba))
}

struct BoundedPng {
    bytes: Vec<u8>,
    exceeded: bool,
}
impl io::Write for BoundedPng {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(input.len()) > crate::attachments::MAX_BYTES {
            self.exceeded = true;
            return Err(io::Error::other("PNG size limit exceeded"));
        }
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
