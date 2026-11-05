//! Separate native file windows share the root App and its document/save state.
//! Only input crosses threads; document mutation and drawing stay on the root UI thread.
use super::{input_stream::WakeHandle, native_window, workspace, App};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::Terminal;
use std::{
    collections::BTreeMap,
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

enum Message {
    Input(u32, native_window::NativeEvent),
    Closed(u32),
}

struct FileWindow {
    terminal: Terminal<native_window::WindowBackend>,
    stop: Arc<AtomicBool>,
    input: Option<JoinHandle<()>>,
}

impl Drop for FileWindow {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(input) = self.input.take() {
            let _ = input.join();
        }
        // The backend then cancels its native sender before destroying the HWND.
    }
}

pub(super) struct DetachedWindows {
    windows: BTreeMap<u32, FileWindow>,
    sender: SyncSender<Message>,
    receiver: Receiver<Message>,
    wake: WakeHandle,
}

impl DetachedWindows {
    pub(super) fn new(wake: WakeHandle) -> Self {
        let (sender, receiver) = mpsc::sync_channel(256);
        Self {
            windows: BTreeMap::new(),
            sender,
            receiver,
            wake,
        }
    }

    fn open(&mut self, id: u32) -> io::Result<()> {
        let (backend, receiver) = native_window::open_detailed()?;
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let sender = self.sender.clone();
        let wake = self.wake.clone();
        let input = thread::Builder::new()
            .name(format!("concordia-file-input-{id}"))
            .spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    let message = match receiver.recv_timeout(Duration::from_millis(25)) {
                        Ok(event) => Message::Input(id, event),
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => {
                            send(&sender, Message::Closed(id), &stopped, &wake);
                            break;
                        }
                    };
                    if !send(&sender, message, &stopped, &wake) {
                        break;
                    }
                }
            })?;
        self.windows.insert(
            id,
            FileWindow {
                terminal,
                stop,
                input: Some(input),
            },
        );
        Ok(())
    }

    /// Reconcile HWND ownership after detach/dock/close. Opening failures restore the pane.
    pub(super) fn sync(&mut self, app: &mut App) {
        let ids = app.workspace.detached_ids();
        self.windows.retain(|id, _| ids.contains(id));
        for id in ids {
            if !self.windows.contains_key(&id) {
                if let Err(error) = self.open(id) {
                    app.workspace.request_dock(id);
                    app.flash(format!("Could not open file window: {error}"));
                }
            }
        }
    }

    /// Drain child input before root drawing. Closing a file HWND docks its existing document.
    pub(super) fn drain(&mut self, app: &mut App) -> bool {
        let mut changed = false;
        for _ in 0..1024 {
            let Ok(message) = self.receiver.try_recv() else {
                break;
            };
            let (id, event) = match message {
                Message::Closed(id) => {
                    app.workspace.request_dock(id);
                    changed = true;
                    continue;
                }
                Message::Input(id, native_window::NativeEvent::Event(event)) => (id, event),
                Message::Input(id, native_window::NativeEvent::MouseUp { event, .. }) => {
                    (id, Event::Mouse(event))
                }
            };
            // Messages already queued when a child was docked cannot affect another view.
            if !self.windows.contains_key(&id) || !app.workspace.detached_ids().contains(&id) {
                continue;
            }
            if matches!(event, Event::Key(key) if key.kind != KeyEventKind::Release && key.code == KeyCode::F(12) && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                app.workspace.request_dock(id);
                changed = true;
                continue;
            }
            workspace::handle_window_event(app, id, event);
            changed = true;
        }
        changed
    }

    /// Render the same documents after the root view; workspace helpers preserve root hit areas.
    pub(super) fn draw(&mut self, app: &mut App) {
        let mut failed = Vec::new();
        for (&id, window) in &mut self.windows {
            let result = (|| -> io::Result<()> {
                if let Some(title) = app.workspace.detached_title(id, &app.cwd) {
                    window.terminal.backend_mut().set_title(&title)?;
                }
                window.terminal.autoresize()?;
                window.terminal.draw(|frame| {
                    workspace::draw_window(frame, app, id);
                    super::theme::apply(frame.buffer_mut(), app.no_color, true);
                })?;
                Ok(())
            })();
            if let Err(error) = result {
                failed.push((id, error));
            }
        }
        for (id, error) in failed {
            app.workspace.request_dock(id);
            self.windows.remove(&id);
            app.flash(format!("File window closed: {error}"));
        }
    }
}

fn send(
    sender: &SyncSender<Message>,
    mut message: Message,
    stopped: &AtomicBool,
    wake: &WakeHandle,
) -> bool {
    while !stopped.load(Ordering::Acquire) {
        match sender.try_send(message) {
            Ok(()) => {
                wake.wake();
                return true;
            }
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(pending)) => message = pending,
        }
        thread::sleep(Duration::from_millis(2));
    }
    false
}
