//! Windows ConPTY input through its VT byte stream. Crossterm's Windows backend uses
//! `ReadConsoleInputW`, which does not receive VT input sequences when VT input mode is enabled.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::ffi::c_void;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const STD_INPUT_HANDLE: i32 = -10;
const ENABLE_LINE_INPUT: u32 = 0x0002;
const ENABLE_ECHO_INPUT: u32 = 0x0004;
const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
const ENABLE_VIRTUAL_TERMINAL_INPUT: u32 = 0x0200;
const UTF8_CODE_PAGE: u32 = 65001;
const READ_SIZE: usize = 4096;
const RAW_QUEUE_CAPACITY: usize = 16;
const MAX_PASTE_BYTES: usize = 16 * 1024 * 1024;
const MAX_ESCAPE_BYTES: usize = 4096;
const RESIZE_POLL_INTERVAL: Duration = Duration::from_millis(200);

type Handle = *mut c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetStdHandle(which: i32) -> Handle;
    fn GetConsoleMode(handle: Handle, mode: *mut u32) -> i32;
    fn SetConsoleMode(handle: Handle, mode: u32) -> i32;
    fn GetConsoleCP() -> u32;
    fn SetConsoleCP(code_page: u32) -> i32;
    fn ReadConsoleW(
        handle: Handle,
        buffer: *mut c_void,
        characters_to_read: u32,
        characters_read: *mut u32,
        control: *mut c_void,
    ) -> i32;
    fn CancelSynchronousIo(thread: Handle) -> i32;
}

enum RawMessage {
    Bytes(Vec<u8>, Instant),
    Error(io::Error),
}

/// Owns the VT input mode and a blocking raw-byte reader. It must be dropped before Crossterm
/// raw mode is disabled, so its original console mode is restored as part of input shutdown.
pub(super) struct WindowsVt {
    input: Handle,
    original_mode: u32,
    original_code_page: u32,
    receiver: Option<Receiver<RawMessage>>,
    shutdown: Arc<AtomicBool>,
    raw_reader: Option<JoinHandle<()>>,
}

pub(super) enum StartError {
    Unsupported,
    Failed(io::Error),
}

impl WindowsVt {
    /// Enables VT input before creating any reader. Failure here is safe for the caller to treat
    /// as unsupported and fall back to Crossterm, because no input has yet been consumed.
    pub(super) fn start() -> Result<Self, StartError> {
        let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        if input.is_null() || input == invalid_handle() {
            return Err(StartError::Unsupported);
        }

        let mut original_mode = 0;
        if unsafe { GetConsoleMode(input, &mut original_mode) } == 0 {
            return Err(StartError::Unsupported);
        }

        let original_code_page = unsafe { GetConsoleCP() };
        if original_code_page == 0 {
            return Err(StartError::Unsupported);
        }
        if unsafe { SetConsoleCP(UTF8_CODE_PAGE) } == 0 {
            return Err(StartError::Unsupported);
        }

        let vt_mode = (original_mode
            & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT))
            | ENABLE_VIRTUAL_TERMINAL_INPUT;
        if unsafe { SetConsoleMode(input, vt_mode) } == 0 {
            unsafe {
                SetConsoleCP(original_code_page);
            }
            return Err(StartError::Unsupported);
        }

        let (sender, receiver) = mpsc::sync_channel(RAW_QUEUE_CAPACITY);
        let shutdown = Arc::new(AtomicBool::new(false));
        let reader_shutdown = shutdown.clone();
        let input_value = input as usize;
        let reader = match thread::Builder::new()
            .name("concordia-vt-input-bytes".into())
            .spawn(move || read_bytes(input_value as Handle, sender, reader_shutdown))
        {
            Ok(reader) => reader,
            Err(error) => {
                unsafe {
                    SetConsoleMode(input, original_mode);
                    SetConsoleCP(original_code_page);
                }
                return Err(StartError::Failed(error));
            }
        };

        Ok(Self {
            input,
            original_mode,
            original_code_page,
            receiver: Some(receiver),
            shutdown,
            raw_reader: Some(reader),
        })
    }

    /// Parse raw bytes on the bounded event-reader thread, preserving all bytes in each event.
    pub(super) fn run(
        &mut self,
        sender: &SyncSender<super::Message>,
        shutdown: &Arc<AtomicBool>,
    ) -> io::Result<()> {
        let receiver = self.receiver.as_ref().expect("raw reader is alive");
        let mut parser = VtParser::default();
        let mut quiet = false;
        let mut last_resize = Instant::now();
        let mut previous_size = crossterm::terminal::size().ok();

        while !shutdown.load(Ordering::Acquire) {
            match receiver.recv_timeout(super::super::input::BURST_QUIET) {
                Ok(RawMessage::Bytes(bytes, at)) => {
                    quiet = false;
                    for byte in bytes {
                        if let Some(event) = parser.push(byte)? {
                            if sender.send(super::Message::Event(event, at)).is_err() {
                                return Ok(());
                            }
                        }
                    }
                }
                Ok(RawMessage::Error(error)) => return Err(error),
                Err(RecvTimeoutError::Disconnected) => {
                    if shutdown.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Windows terminal input reader stopped",
                    ));
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(event) = parser.quiet()? {
                        if sender
                            .send(super::Message::Event(event, Instant::now()))
                            .is_err()
                        {
                            return Ok(());
                        }
                    }
                    if !quiet {
                        quiet = true;
                        if sender.send(super::Message::Quiet(Instant::now())).is_err() {
                            return Ok(());
                        }
                    }
                    if last_resize.elapsed() >= RESIZE_POLL_INTERVAL {
                        last_resize = Instant::now();
                        if let Ok(size) = crossterm::terminal::size() {
                            if previous_size != Some(size) {
                                previous_size = Some(size);
                                if sender
                                    .send(super::Message::Event(
                                        Event::Resize(size.0, size.1),
                                        Instant::now(),
                                    ))
                                    .is_err()
                                {
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl Drop for WindowsVt {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        // A full raw-byte channel can block the reader in send; drop it before cancelling the
        // synchronous console read and joining, so either blocking point can always be released.
        drop(self.receiver.take());
        if let Some(reader) = self.raw_reader.take() {
            let thread_handle = reader.as_raw_handle() as Handle;
            while !reader.is_finished() {
                unsafe {
                    CancelSynchronousIo(thread_handle);
                }
                thread::sleep(Duration::from_millis(5));
            }
            let _ = reader.join();
        }
        unsafe {
            SetConsoleMode(self.input, self.original_mode);
            SetConsoleCP(self.original_code_page);
        }
    }
}

fn read_bytes(input: Handle, sender: SyncSender<RawMessage>, shutdown: Arc<AtomicBool>) {
    let mut buffer = [0u16; READ_SIZE];
    let mut pending_high_surrogate = None;
    while !shutdown.load(Ordering::Acquire) {
        let mut characters_read = 0;
        let result = unsafe {
            ReadConsoleW(
                input,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut characters_read,
                std::ptr::null_mut(),
            )
        };
        if result == 0 {
            let error = io::Error::last_os_error();
            if shutdown.load(Ordering::Acquire) {
                break;
            }
            let _ = sender.send(RawMessage::Error(error));
            break;
        }
        if characters_read == 0 {
            if !shutdown.load(Ordering::Acquire) {
                let _ = sender.send(RawMessage::Error(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Windows terminal input closed",
                )));
            }
            break;
        }
        let bytes = match encode_utf16_chunk(
            &buffer[..characters_read as usize],
            &mut pending_high_surrogate,
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = sender.send(RawMessage::Error(error));
                break;
            }
        };
        if bytes.is_empty() {
            continue;
        }
        let message = RawMessage::Bytes(bytes, Instant::now());
        if sender.send(message).is_err() {
            break;
        }
    }
}

fn encode_utf16_chunk(
    characters: &[u16],
    pending_high_surrogate: &mut Option<u16>,
) -> io::Result<Vec<u8>> {
    use std::char::decode_utf16;

    let mut units =
        Vec::with_capacity(characters.len() + usize::from(pending_high_surrogate.is_some()));
    if let Some(high) = pending_high_surrogate.take() {
        units.push(high);
    }
    units.extend_from_slice(characters);

    let mut output = String::new();
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        if (0xD800..=0xDBFF).contains(&unit) {
            if index + 1 == units.len() {
                *pending_high_surrogate = Some(unit);
                break;
            }
            let scalar = decode_utf16([unit, units[index + 1]])
                .next()
                .expect("two UTF-16 units produce one decoder item")
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-16 surrogate pair")
                })?;
            output.push(scalar);
            index += 2;
            continue;
        }
        if (0xDC00..=0xDFFF).contains(&unit) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unpaired UTF-16 low surrogate in terminal input",
            ));
        }
        output.push(char::from_u32(unit as u32).expect("non-surrogate UTF-16 unit is valid"));
        index += 1;
    }
    Ok(output.into_bytes())
}

fn invalid_handle() -> Handle {
    -1isize as Handle
}

#[derive(Default)]
struct VtParser {
    event: Vec<u8>,
}

impl VtParser {
    fn push(&mut self, byte: u8) -> io::Result<Option<Event>> {
        // An unframed LF is ConPTY's Ctrl+Enter paste newline. Inside bracketed paste the
        // incomplete event buffer is nonempty, so the byte remains part of Event::Paste data.
        if byte == b'\n' && self.event.is_empty() {
            return Ok(Some(Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::CONTROL,
            ))));
        }
        self.event.push(byte);
        let limit = if self.event.starts_with(b"\x1B[200~") {
            MAX_PASTE_BYTES
        } else {
            MAX_ESCAPE_BYTES
        };
        if self.event.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "terminal input sequence exceeded its size limit",
            ));
        }

        let (consumed, event) = match crossterm::event::parse_windows_vt_event(&self.event, true) {
            Ok(parsed) => parsed,
            Err(_) if self.event.first() == Some(&b'\x1b') => {
                // Match Crossterm's stream parser: unsupported control sequences are not
                // text and must not close the chat or discard its existing draft.
                self.event.clear();
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if !consumed {
            return Ok(None);
        }
        self.event.clear();
        Ok(event.map(normalize_event))
    }

    fn quiet(&mut self) -> io::Result<Option<Event>> {
        if self.event.as_slice() != b"\x1B" {
            return Ok(None);
        }
        let (consumed, event) = crossterm::event::parse_windows_vt_event(&self.event, false)?;
        if !consumed {
            return Ok(None);
        }
        self.event.clear();
        Ok(event.map(normalize_event))
    }
}

fn normalize_event(event: Event) -> Event {
    match event {
        // Windows Terminal sends DEL (0x7f) for Backspace and BS (0x08, Ctrl+H) for Ctrl+Backspace,
        // so BS is Ctrl+Backspace: delete the previous word.
        Event::Key(key)
            if key.code == KeyCode::Char('h') && key.modifiers == KeyModifiers::CONTROL =>
        {
            Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL))
        }
        // ConPTY encodes LF in its VT byte stream. Preserve the same text meaning as the
        // Crossterm INPUT_RECORD backend, where pasted LF arrives as Ctrl+Enter.
        Event::Key(key)
            if (key.code == KeyCode::Char('\n') && key.modifiers.is_empty())
                || (key.code == KeyCode::Char('j') && key.modifiers == KeyModifiers::CONTROL) =>
        {
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL))
        }
        event => event,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(bytes: &[u8]) -> Vec<Event> {
        let mut parser = VtParser::default();
        bytes
            .iter()
            .filter_map(|byte| parser.push(*byte).unwrap())
            .collect()
    }

    #[test]
    fn bracketed_paste_is_one_exact_event_across_arbitrary_chunks() {
        let mut parser = VtParser::default();
        let mut events = Vec::new();
        for chunk in [b"\x1B[200~alpha\r".as_slice(), b"\nbeta\n", b"\r\x1B[201~"] {
            for byte in chunk {
                if let Some(event) = parser.push(*byte).unwrap() {
                    events.push(event);
                }
            }
        }
        assert_eq!(events, [Event::Paste("alpha\r\nbeta\n\r".into())]);
    }

    #[test]
    fn multiple_plain_keys_in_one_read_are_not_discarded() {
        assert_eq!(
            parse(b"abc"),
            [
                Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
                Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE)),
                Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)),
            ]
        );
    }

    #[test]
    fn vt_crlf_matches_the_legacy_conpty_enter_and_ctrl_enter_events() {
        assert_eq!(
            parse(b"\r\n"),
            [
                Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
                Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
            ]
        );
    }

    #[test]
    fn bs_is_ctrl_backspace_and_del_is_backspace() {
        assert_eq!(
            parse(b"\x08"),
            [Event::Key(KeyEvent::new(
                KeyCode::Backspace,
                KeyModifiers::CONTROL
            ))]
        );
        assert_eq!(
            parse(b"\x7f"),
            [Event::Key(KeyEvent::new(
                KeyCode::Backspace,
                KeyModifiers::NONE
            ))]
        );
    }

    #[test]
    fn focus_reports_arrive_as_focus_events() {
        // The terminal sends these after `EnableFocusChange`; a gained focus repaints the screen.
        assert_eq!(parse(b"\x1b[I"), [Event::FocusGained]);
        assert_eq!(parse(b"\x1b[O"), [Event::FocusLost]);
    }

    #[test]
    fn lone_escape_is_emitted_at_quiet_boundary() {
        let mut parser = VtParser::default();
        assert_eq!(parser.push(b'\x1B').unwrap(), None);
        assert_eq!(
            parser.quiet().unwrap(),
            Some(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))
        );
    }

    #[test]
    fn unknown_control_sequence_does_not_close_input_or_swallow_following_text() {
        assert_eq!(parse(b"\x1b[999xok"), parse(b"ok"));
        assert_eq!(parse(b"\x1b[0;0Rok"), parse(b"ok"));
    }

    #[test]
    fn utf16_surrogate_pair_can_cross_console_read_chunks() {
        let mut pending = None;
        assert_eq!(encode_utf16_chunk(&[0xD83D], &mut pending).unwrap(), b"");
        assert_eq!(
            encode_utf16_chunk(&[0xDE00], &mut pending).unwrap(),
            "😀".as_bytes()
        );
        assert_eq!(pending, None);
    }

    #[test]
    fn utf16_unpaired_low_surrogate_is_reported() {
        assert_eq!(
            encode_utf16_chunk(&[0xDE00], &mut None).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
