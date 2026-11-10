//! One explicitly opened local shell per chat tab. Shell bytes never enter provider channels.
use super::*;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};

const QUEUE: usize = 64;
const CHUNK: usize = 8192;
const MAX_PASTE: usize = 64 * 1024;
const SCROLLBACK: usize = 2000;
const MAX_PENDING_INPUT: usize = 256 * 1024;

#[derive(Default, PartialEq, Eq)]
enum View {
    #[default]
    Chat,
    Shell,
}

enum Output {
    Bytes(Vec<u8>),
    Error(String),
}

enum Phase {
    Running,
    Exited(String),
}

struct Replies {
    input: Option<SyncSender<Vec<u8>>>,
    failed: bool,
}

impl vt100::Callbacks for Replies {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        first: Option<u8>,
        second: Option<u8>,
        params: &[&[u16]],
        command: char,
    ) {
        let parameter = params
            .first()
            .and_then(|values| values.first())
            .copied()
            .unwrap_or(0);
        let response = match (first, second, command, parameter) {
            (None, None, 'n', 6) => {
                let (row, column) = screen.cursor_position();
                format!("\x1b[{};{}R", row + 1, column + 1).into_bytes()
            }
            (None, None, 'n', 5) => b"\x1b[0n".to_vec(),
            (None, None, 'c', 0) => b"\x1b[?1;2c".to_vec(),
            _ => return,
        };
        self.failed |= self
            .input
            .as_ref()
            .is_none_or(|input| input.try_send(response).is_err());
    }
}

struct Session {
    parser: vt100::Parser<()>,
    reply_size: std::sync::Arc<std::sync::atomic::AtomicU32>,
    closing: std::sync::Arc<std::sync::atomic::AtomicBool>,
    master: Option<Box<dyn MasterPty + Send>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    tree: Option<crate::process_lifetime::ProcessTree>,
    input: Option<SyncSender<Vec<u8>>>,
    output: Option<Receiver<Output>>,
    phase: Phase,
    pending: std::collections::VecDeque<Vec<u8>>,
    pending_bytes: usize,
    refused_input: bool,
    name: String,
    size: (u16, u16),
}

impl Drop for Session {
    fn drop(&mut self) {
        crate::logging::info("local_terminal shutdown begin");
        self.closing
            .store(true, std::sync::atomic::Ordering::Release);
        self.input.take();
        self.output.take();
        crate::logging::info("local_terminal shutdown child_kill begin");
        let _ = self.child.kill();
        crate::logging::info("local_terminal shutdown child_kill end; tree_close begin");
        self.tree.take();
        crate::logging::info("local_terminal shutdown tree_close end; master_close begin");
        self.master.take();
        crate::logging::info("local_terminal shutdown master_close end");
    }
}

#[derive(Default)]
pub(super) struct State {
    session: Option<Session>,
    view: View,
    notice: Option<String>,
}

impl State {
    pub(super) fn visible(&self) -> bool {
        self.view == View::Shell
    }

    pub(super) fn running(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| matches!(session.phase, Phase::Running))
    }

    pub(super) fn stop(&mut self) {
        self.session = None;
        self.view = View::Chat;
        self.notice = None;
    }

    fn show(&mut self, cwd: &Path) {
        if self.session.is_none() {
            match Session::start(cwd) {
                Ok(session) => self.session = Some(session),
                Err(error) => {
                    self.notice = Some(format!("Could not start local terminal: {error}"))
                }
            }
        }
        self.view = View::Shell;
    }

    pub(super) fn poll(&mut self) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        let mut changed = false;
        session.flush_input();
        for _ in 0..QUEUE {
            match session
                .output
                .as_ref()
                .and_then(|receiver| receiver.try_recv().ok())
            {
                Some(Output::Bytes(bytes)) => {
                    session.parser.process(&bytes);
                    changed = true;
                }
                Some(Output::Error(error)) => {
                    self.notice = Some(error);
                    changed = true;
                }
                None => break,
            }
        }
        if matches!(session.phase, Phase::Running) {
            match session.child.try_wait() {
                Ok(Some(status)) => {
                    session.phase = Phase::Exited(status.to_string());
                    session.input.take();
                    session.pending.clear();
                    session.pending_bytes = 0;
                    // Reap/close remaining descendants even if the shell itself already exited.
                    session.tree.take();
                    changed = true;
                }
                Err(error) => {
                    self.notice = Some(format!("Terminal status unavailable: {error}"));
                }
                Ok(None) => {}
            }
        }
        changed && self.visible()
    }

    fn write(&mut self, bytes: Vec<u8>) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if !matches!(session.phase, Phase::Running) {
            return;
        }
        if bytes == b"\x03" {
            session.pending.clear();
            session.pending_bytes = 0;
            session.refused_input = false;
        } else if session.refused_input {
            return;
        }
        if session.pending_bytes.saturating_add(bytes.len()) > MAX_PENDING_INPUT {
            session.refused_input = true;
            self.notice = Some(
                "Terminal input limit reached. Ctrl+C cancels pending input; Enter is blocked."
                    .into(),
            );
            return;
        }
        session.parser.screen_mut().set_scrollback(0);
        for chunk in bytes.chunks(CHUNK) {
            session.pending.push_back(chunk.to_vec());
        }
        session.pending_bytes += bytes.len();
        session.flush_input();
        self.notice = None;
    }

    pub(super) fn paste(&mut self, text: &str) {
        if text.len() > MAX_PASTE {
            self.notice = Some("Terminal paste exceeds 64 KiB; nothing was sent".into());
            return;
        }
        // A clipboard escape cannot close bracketed paste and inject terminal control sequences.
        let clean = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .chars()
            .filter(|character| !character.is_control() || matches!(character, '\t' | '\n'))
            .collect::<String>();
        let bracketed = self
            .session
            .as_ref()
            .is_some_and(|session| session.parser.screen().bracketed_paste());
        if bracketed {
            self.write(format!("\x1b[200~{clean}\x1b[201~").into_bytes());
        } else {
            self.write(clean.replace('\n', "\r").into_bytes());
        }
    }

    fn scroll(&mut self, delta: isize) {
        if let Some(session) = self.session.as_mut() {
            let current = session.parser.screen().scrollback();
            session
                .parser
                .screen_mut()
                .set_scrollback(current.saturating_add_signed(delta));
        }
    }
}

impl Session {
    fn flush_input(&mut self) {
        let Some(sender) = self.input.as_ref() else {
            return;
        };
        while let Some(bytes) = self.pending.pop_front() {
            let length = bytes.len();
            match sender.try_send(bytes) {
                Ok(()) => self.pending_bytes -= length,
                Err(TrySendError::Full(bytes)) => {
                    self.pending.push_front(bytes);
                    break;
                }
                Err(TrySendError::Disconnected(bytes)) => {
                    self.pending.push_front(bytes);
                    break;
                }
            }
        }
    }
    fn start(cwd: &Path) -> anyhow::Result<Self> {
        let size = (24, 80);
        let reply_size = std::sync::Arc::new(std::sync::atomic::AtomicU32::new((24 << 16) | 80));
        let reader_size = reply_size.clone();
        let closing = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer_closing = closing.clone();
        let (out_tx, output) = mpsc::sync_channel(QUEUE);
        let errors = out_tx.clone();
        let (input, in_rx) = mpsc::sync_channel::<Vec<u8>>(QUEUE);
        let (reply_tx, reply_rx) = mpsc::sync_channel::<Vec<u8>>(QUEUE);
        // Start workers before creating a ConPTY: a thread-creation failure must
        // never leave a newly created console waiting for its cursor reply.
        let (reader_init, reader_ready) = mpsc::sync_channel::<Box<dyn Read + Send>>(1);
        let (writer_init, writer_ready) = mpsc::sync_channel::<Box<dyn Write + Send>>(1);
        std::thread::Builder::new()
            .name("concordia-terminal-output".into())
            .spawn(move || {
                let Ok(mut reader) = reader_ready.recv() else {
                    return;
                };
                let mut parser = vt100::Parser::new_with_callbacks(
                    NonZeroU16::new(24).unwrap(),
                    NonZeroU16::new(80).unwrap(),
                    0,
                    Replies {
                        input: Some(reply_tx),
                        failed: false,
                    },
                );
                let mut size = (24, 80);
                let mut connected = true;
                let mut buffer = [0; CHUNK];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(length) => {
                            let packed = reader_size.load(std::sync::atomic::Ordering::Relaxed);
                            let next = ((packed >> 16) as u16, packed as u16);
                            if next != size {
                                parser.set_size(
                                    NonZeroU16::new(next.0).unwrap(),
                                    NonZeroU16::new(next.1).unwrap(),
                                );
                                size = next;
                            }
                            // Sole owner of terminal replies, including during shutdown.
                            parser.process(&buffer[..length]);
                            if parser.callbacks().failed && connected {
                                connected = out_tx
                                    .send(Output::Error(
                                        "Terminal status reply queue overflowed".into(),
                                    ))
                                    .is_ok();
                                parser.callbacks_mut().failed = false;
                            }
                            if connected {
                                connected = out_tx
                                    .send(Output::Bytes(buffer[..length].to_vec()))
                                    .is_ok();
                            }
                            // A closed UI receiver means discard, never stop draining:
                            // ClosePseudoConsole can emit a final frame synchronously.
                        }
                        Err(error) => {
                            if connected {
                                let _ =
                                    out_tx.send(Output::Error(format!("Terminal output: {error}")));
                            }
                            break;
                        }
                    }
                }
            })?;
        std::thread::Builder::new()
            .name("concordia-terminal-input".into())
            .spawn(move || {
                let Ok(mut writer) = writer_ready.recv() else {
                    return;
                };
                let mut input_open = true;
                let mut cursor_ready = !cfg!(windows);
                let mut shutdown_started: Option<Instant> = None;
                loop {
                    // EOF on the output pipe can depend on closing the input pipe.
                    // Do not retain it until reader EOF. The initial ConPTY cursor
                    // reply must have been written before shutdown closes input.
                    if writer_closing.load(std::sync::atomic::Ordering::Acquire) {
                        let since = shutdown_started.get_or_insert_with(Instant::now);
                        if cursor_ready || since.elapsed() >= Duration::from_secs(1) {
                            if !cursor_ready {
                                crate::logging::info(
                                    "local_terminal cursor shutdown deadline; closing input",
                                );
                            }
                            break;
                        }
                    }
                    // Status replies take priority over queued interactive input.
                    let bytes = match reply_rx.try_recv() {
                        Ok(bytes) => bytes,
                        Err(mpsc::TryRecvError::Empty) if input_open => {
                            match in_rx.recv_timeout(Duration::from_millis(10)) {
                                Ok(bytes) => bytes,
                                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                                Err(mpsc::RecvTimeoutError::Disconnected) => {
                                    input_open = false;
                                    continue;
                                }
                            }
                        }
                        Err(mpsc::TryRecvError::Empty) => {
                            match reply_rx.recv_timeout(Duration::from_millis(10)) {
                                Ok(bytes) => bytes,
                                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            }
                        }
                        Err(mpsc::TryRecvError::Disconnected) if input_open => {
                            match in_rx.recv_timeout(Duration::from_millis(10)) {
                                Ok(bytes) => bytes,
                                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            }
                        }
                        Err(mpsc::TryRecvError::Disconnected) => break,
                    };
                    if let Err(error) = writer.write_all(&bytes).and_then(|_| writer.flush()) {
                        let _ = errors.send(Output::Error(format!("Terminal input: {error}")));
                        break;
                    }
                    if bytes.starts_with(b"\x1b[") && bytes.ends_with(b"R") {
                        cursor_ready = true;
                        crate::logging::info("local_terminal inherited_cursor reply written");
                    }
                }
                drop(writer);
                crate::logging::info("local_terminal input worker closed");
            })?;
        #[cfg(windows)]
        let program = which::which("pwsh.exe").or_else(|_| which::which("powershell.exe"))?;
        #[cfg(not(windows))]
        let program = std::env::var_os("SHELL")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/bin/sh"));
        let name = program
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut command = CommandBuilder::new(&program);
        #[cfg(windows)]
        command.args(["-NoLogo", "-NoProfile"]);
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        let pair = native_pty_system().openpty(PtySize {
            rows: size.0,
            cols: size.1,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        writer_init
            .send(writer)
            .map_err(|_| anyhow::anyhow!("Terminal input worker stopped"))?;
        reader_init
            .send(reader)
            .map_err(|_| anyhow::anyhow!("Terminal output worker stopped"))?;
        let mut child = match pair.slave.spawn_command(command) {
            Ok(child) => child,
            Err(error) => {
                closing.store(true, std::sync::atomic::Ordering::Release);
                drop(output);
                drop(input);
                drop(pair);
                return Err(error);
            }
        };
        drop(pair.slave);
        let tree = match crate::process_lifetime::own_pty_process(child.as_ref()) {
            Ok(tree) => tree,
            Err(error) => {
                let _ = child.kill();
                closing.store(true, std::sync::atomic::Ordering::Release);
                drop(output);
                drop(input);
                drop(pair.master);
                anyhow::bail!(error);
            }
        };
        Ok(Self {
            parser: vt100::Parser::new(
                NonZeroU16::new(size.0).unwrap(),
                NonZeroU16::new(size.1).unwrap(),
                SCROLLBACK,
            ),
            reply_size,
            closing,
            master: Some(pair.master),
            child,
            tree: Some(tree),
            input: Some(input),
            output: Some(output),
            phase: Phase::Running,
            pending: Default::default(),
            pending_bytes: 0,
            refused_input: false,
            name,
            size,
        })
    }
}

pub(super) fn command(app: &mut App, args: &str) {
    match args.trim() {
        "" | "open" => app.local_terminal.show(&app.cwd),
        "hide" => app.local_terminal.view = View::Chat,
        "close" => app.local_terminal.stop(),
        _ => app.flash("usage: /terminal [open|hide|close]"),
    }
}

pub(super) fn key(app: &mut App, key: &KeyEvent) -> bool {
    if key.kind == KeyEventKind::Release {
        return app.local_terminal.visible();
    }
    let ctrl =
        key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT);
    let alt =
        key.modifiers.contains(KeyModifiers::ALT) && !key.modifiers.contains(KeyModifiers::CONTROL);
    if key.code == KeyCode::F(11) && key.modifiers.is_empty()
        || ctrl && matches!(key.code, KeyCode::Char('`'))
    {
        if app.local_terminal.visible() {
            app.local_terminal.view = View::Chat;
        } else if app.rename_editor.is_none() && !worker_input::active(app) && !app.raster.active()
        {
            app.local_terminal.show(&app.cwd);
        }
        return true;
    }
    if !app.local_terminal.visible() {
        return false;
    }
    if ctrl
        && key.modifiers.contains(KeyModifiers::SHIFT)
        && matches!(key.code, KeyCode::Char('w' | 'W'))
    {
        app.local_terminal.stop();
        return true;
    }
    if key.code == KeyCode::F(6) && key.modifiers.is_empty() {
        app.local_terminal.view = View::Chat;
        return true;
    }
    if key.code == KeyCode::PageUp && key.modifiers.contains(KeyModifiers::SHIFT) {
        app.local_terminal.scroll(10);
        return true;
    }
    if key.code == KeyCode::PageDown && key.modifiers.contains(KeyModifiers::SHIFT) {
        app.local_terminal.scroll(-10);
        return true;
    }
    if (matches!(key.code, KeyCode::Char('v' | 'V')) && ctrl)
        || key.code == KeyCode::Insert && key.modifiers.contains(KeyModifiers::SHIFT)
    {
        match clipboard::get_text() {
            Ok(text) => app.local_terminal.paste(&text),
            Err(error) => app.local_terminal.notice = Some(format!("Paste failed: {error}")),
        }
        return true;
    }
    let application = app
        .local_terminal
        .session
        .as_ref()
        .is_some_and(|session| session.parser.screen().application_cursor());
    let modifier = 1
        + u8::from(key.modifiers.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(key.modifiers.contains(KeyModifiers::ALT))
        + 4 * u8::from(key.modifiers.contains(KeyModifiers::CONTROL));
    let navigation = |suffix: char| {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{suffix}").into_bytes()
        } else if application {
            format!("\x1bO{suffix}").into_bytes()
        } else {
            format!("\x1b[{suffix}").into_bytes()
        }
    };
    let tilde = |number| {
        if modifier > 1 {
            format!("\x1b[{number};{modifier}~").into_bytes()
        } else {
            format!("\x1b[{number}~").into_bytes()
        }
    };
    let bytes = match key.code {
        KeyCode::Char(character) if ctrl && character.is_ascii() => {
            let code = character.to_ascii_uppercase() as u8;
            if code == b' ' {
                vec![0]
            } else if (b'@'..=b'_').contains(&code) {
                vec![code & 0x1f]
            } else {
                Vec::new()
            }
        }
        KeyCode::Char(character) => {
            let mut bytes = character.to_string().into_bytes();
            if alt {
                bytes.insert(0, 0x1b);
            }
            bytes
        }
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Backspace if ctrl => vec![0x17],
        KeyCode::Backspace if alt => b"\x1b\x7f".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => navigation('A'),
        KeyCode::Down => navigation('B'),
        KeyCode::Right => navigation('C'),
        KeyCode::Left => navigation('D'),
        KeyCode::Home => navigation('H'),
        KeyCode::End => navigation('F'),
        KeyCode::Insert => tilde(2),
        KeyCode::Delete => tilde(3),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::F(number @ 1..=4) => {
            let suffix = char::from(b'P' + number - 1);
            if modifier > 1 {
                format!("\x1b[1;{modifier}{suffix}").into_bytes()
            } else {
                format!("\x1bO{suffix}").into_bytes()
            }
        }
        KeyCode::F(number @ 5..=12) => {
            tilde([15, 17, 18, 19, 20, 21, 23, 24][usize::from(number - 5)])
        }
        _ => Vec::new(),
    };
    if !bytes.is_empty() {
        app.local_terminal.write(bytes);
    }
    true
}

pub(super) fn mouse(app: &mut App, event: MouseEvent) -> bool {
    if !app.local_terminal.visible() {
        return false;
    }
    match event.kind {
        MouseEventKind::ScrollUp => app.local_terminal.scroll(3),
        MouseEventKind::ScrollDown => app.local_terminal.scroll(-3),
        _ => {}
    }
    true
}

fn color(value: vt100::Color) -> Color {
    match value {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(index) => Color::Indexed(index),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

pub(super) fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    app.areas = Areas::default();
    frame.render_widget(Clear, area);
    if area.height < 3 || area.width == 0 {
        return;
    }
    let footer = Rect::new(area.x, area.bottom() - 1, area.width, 1);
    let content = Rect::new(area.x, area.y + 1, area.width, area.height - 2);
    let mut title = " Local terminal".to_string();
    if let Some(session) = app.local_terminal.session.as_mut() {
        title = format!(" {} | local terminal", session.name);
        if session.size != (content.height, content.width) {
            let size = PtySize {
                rows: content.height,
                cols: content.width,
                pixel_width: 0,
                pixel_height: 0,
            };
            match session.master.as_ref().unwrap().resize(size) {
                Ok(()) => {
                    session.parser.set_size(
                        NonZeroU16::new(content.height).unwrap(),
                        NonZeroU16::new(content.width).unwrap(),
                    );
                    session.size = (content.height, content.width);
                    session.reply_size.store(
                        (u32::from(content.height) << 16) | u32::from(content.width),
                        std::sync::atomic::Ordering::Relaxed,
                    );
                }
                Err(error) => app.local_terminal.notice = Some(format!("Terminal resize: {error}")),
            }
        }
        let screen = session.parser.screen();
        for row in 0..content.height {
            for column in 0..content.width {
                let Some(cell) = screen.cell(row, column) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let mut style = Style::default()
                    .fg(color(cell.fgcolor()))
                    .bg(color(cell.bgcolor()));
                for (enabled, modifier) in [
                    (cell.bold(), Modifier::BOLD),
                    (cell.dim(), Modifier::DIM),
                    (cell.italic(), Modifier::ITALIC),
                    (cell.underline(), Modifier::UNDERLINED),
                    (cell.inverse(), Modifier::REVERSED),
                ] {
                    if enabled {
                        style = style.add_modifier(modifier);
                    }
                }
                let symbol = if cell.has_contents() {
                    cell.contents()
                } else {
                    " "
                };
                frame
                    .buffer_mut()
                    .set_string(content.x + column, content.y + row, symbol, style);
            }
        }
        if let Phase::Exited(status) = &session.phase {
            title.push_str(&format!(" | exited: {status}; Ctrl+Shift+W resets shell"));
        }
        if !screen.hide_cursor()
            && screen.scrollback() == 0
            && matches!(session.phase, Phase::Running)
        {
            let (row, column) = screen.cursor_position();
            if row < content.height && column < content.width {
                frame.set_cursor_position((content.x + column, content.y + row));
            }
        }
    }
    frame.render_widget(
        Paragraph::new(title).style(Style::default().fg(Color::Cyan)),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let hint = app.local_terminal.notice.as_deref().unwrap_or(
        "F11 / Ctrl+` chat | Ctrl+C interrupt | Shift+PgUp/PgDn scroll | Ctrl+Shift+W stop shell",
    );
    frame.render_widget(Paragraph::new(hint), footer);
}
