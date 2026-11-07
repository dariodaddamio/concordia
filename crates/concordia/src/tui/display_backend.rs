//! One UI and event loop, backed by a console or Concordia's own native input surface.
use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use std::io::{self, Stdout, Write};

pub(super) enum DisplayBackend {
    Terminal(ratatui::backend::CrosstermBackend<Stdout>),
    #[cfg(windows)]
    Window(super::native_window::WindowBackend),
}

impl DisplayBackend {
    #[cfg(windows)]
    pub fn image(&mut self, image: Vec<super::native_window::RasterImage>) -> io::Result<()> {
        match self {
            Self::Terminal(_) => Ok(()),
            Self::Window(window) => window.set_images(image),
        }
    }
    pub fn title(&mut self, title: &str) -> io::Result<()> {
        match self {
            Self::Terminal(_) => super::run::write_osc_title(title),
            #[cfg(windows)]
            Self::Window(window) => window.set_title(title),
        }
    }
    pub fn pointer(&mut self, shape: &str) -> io::Result<()> {
        match self {
            Self::Terminal(_) => Ok(()),
            #[cfg(windows)]
            Self::Window(window) => window.set_pointer(shape),
        }
    }
}

impl Write for DisplayBackend {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Terminal(backend) => backend.write(bytes),
            #[cfg(windows)]
            Self::Window(_) => Ok(bytes.len()),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Backend::flush(self)
    }
}

macro_rules! delegate {
    ($this:expr, $method:ident $(, $argument:expr)*) => {
        match $this {
            DisplayBackend::Terminal(backend) => Backend::$method(backend $(, $argument)*),
            #[cfg(windows)]
            DisplayBackend::Window(backend) => Backend::$method(backend $(, $argument)*),
        }
    };
}

impl Backend for DisplayBackend {
    type Error = io::Error;
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        delegate!(self, draw, content)
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        delegate!(self, hide_cursor)
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        delegate!(self, show_cursor)
    }
    fn get_cursor_position(&mut self) -> io::Result<Position> {
        delegate!(self, get_cursor_position)
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        delegate!(self, set_cursor_position, position)
    }
    fn clear(&mut self) -> io::Result<()> {
        delegate!(self, clear)
    }
    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        delegate!(self, clear_region, clear_type)
    }
    fn size(&self) -> io::Result<Size> {
        delegate!(self, size)
    }
    fn window_size(&mut self) -> io::Result<WindowSize> {
        delegate!(self, window_size)
    }
    fn flush(&mut self) -> io::Result<()> {
        delegate!(self, flush)
    }
}
