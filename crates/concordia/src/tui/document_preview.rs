//! Document pixels for the existing background raster worker. No provider or shell calls.
use super::*;

pub(super) fn svg(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    use resvg::{tiny_skia, usvg};
    static FONTS: std::sync::OnceLock<Arc<usvg::fontdb::Database>> = std::sync::OnceLock::new();
    let mut options = usvg::Options {
        fontdb: FONTS
            .get_or_init(|| {
                let mut fonts = usvg::fontdb::Database::new();
                fonts.load_system_fonts();
                Arc::new(fonts)
            })
            .clone(),
        ..Default::default()
    };
    // A preview may use embedded images, but must not read paths named by the SVG.
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let tree = usvg::Tree::from_data(bytes, &options).map_err(|e| format!("Invalid SVG: {e}"))?;
    let size = tree.size();
    let scale = (2048.0 / size.width()).min(2048.0 / size.height()).min(1.0);
    let width = (size.width() * scale).ceil().max(1.0) as u32;
    let height = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixels =
        tiny_skia::Pixmap::new(width, height).ok_or("SVG preview allocation failed")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixels.as_mut(),
    );
    let mut bytes = pixels.take();
    for pixel in bytes.chunks_exact_mut(4) {
        if pixel[3] != 0 {
            for channel in 0..3 {
                pixel[channel] = ((u32::from(pixel[channel]) * 255 + u32::from(pixel[3]) / 2)
                    / u32::from(pixel[3]))
                .min(255) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(width, height, bytes).ok_or_else(|| "Invalid SVG pixels".into())
}

#[cfg(windows)]
pub(super) fn pdf(bytes: &[u8], page_index: u32) -> Result<(image::RgbaImage, u32), String> {
    use windows::{
        Data::Pdf::{PdfDocument, PdfPageRenderOptions},
        Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream},
    };
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
    let initialized = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
    if initialized < 0 {
        return Err(format!(
            "PDF renderer initialization failed: 0x{initialized:08x}"
        ));
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    let _apartment = Apartment;
    let render = || -> windows::core::Result<(image::RgbaImage, u32)> {
        let input = InMemoryRandomAccessStream::new()?;
        let writer = DataWriter::CreateDataWriter(&input)?;
        writer.WriteBytes(bytes)?;
        writer.StoreAsync()?.join()?;
        writer.DetachStream()?;
        input.Seek(0)?;
        let document = PdfDocument::LoadFromStreamAsync(&input)?.join()?;
        let pages = document.PageCount()?;
        let page = document.GetPage(page_index.min(pages.saturating_sub(1)))?;
        let size = page.Size()?;
        let scale = (2048.0 / size.Width).min(2048.0 / size.Height).min(2.0);
        let options = PdfPageRenderOptions::new()?;
        options.SetDestinationWidth((size.Width * scale).ceil().max(1.0) as u32)?;
        options.SetDestinationHeight((size.Height * scale).ceil().max(1.0) as u32)?;
        let output = InMemoryRandomAccessStream::new()?;
        page.RenderWithOptionsToStreamAsync(&output, &options)?
            .join()?;
        let length = output.Size()?;
        if length > 32 * 1024 * 1024 {
            return Err(windows::core::Error::new(
                windows::core::HRESULT(0x8007000Eu32 as i32),
                "PDF page exceeds preview limit",
            ));
        }
        let reader = DataReader::CreateDataReader(&output.GetInputStreamAt(0)?)?;
        reader.LoadAsync(length as u32)?.join()?;
        let mut data = vec![0; length as usize];
        reader.ReadBytes(&mut data)?;
        let pixels = image::load_from_memory(&data)
            .map_err(|e| {
                windows::core::Error::new(
                    windows::core::HRESULT(0x8007000Du32 as i32),
                    e.to_string(),
                )
            })?
            .into_rgba8();
        Ok((pixels, pages))
    };
    render().map_err(|e| format!("PDF preview failed: {e}"))
}

#[cfg(not(windows))]
pub(super) fn pdf(_: &[u8], _: u32) -> Result<(image::RgbaImage, u32), String> {
    Err("PDF preview currently requires Windows; press E for the system viewer".into())
}
