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

type WindowKey = (uuid::Uuid, u32);

enum Message {
    Input(WindowKey, native_window::NativeEvent),
    Closed(WindowKey),
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
    windows: BTreeMap<WindowKey, FileWindow>,
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

    fn open(&mut self, key: WindowKey) -> io::Result<()> {
        let (backend, receiver) = native_window::open()?;
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let sender = self.sender.clone();
        let wake = self.wake.clone();
        let input = thread::Builder::new()
            .name(format!("concordia-file-input-{}-{}", key.0, key.1))
            .spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    let message = match receiver.recv_timeout(Duration::from_millis(25)) {
                        Ok(event) => Message::Input(key, event),
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => {
                            send(&sender, Message::Closed(key), &stopped, &wake);
                            break;
                        }
                    };
                    if !send(&sender, message, &stopped, &wake) {
                        break;
                    }
                }
            })?;
        self.windows.insert(
            key,
            FileWindow {
                terminal,
                stop,
                input: Some(input),
            },
        );
        Ok(())
    }

    /// Reconcile HWND ownership after detach/dock/close. Opening failures restore the pane.
    pub(super) fn sync(&mut self, owner: uuid::Uuid, app: &mut App) {
        let ids = app.workspace.detached_ids();
        self.windows
            .retain(|(tab, id), _| *tab != owner || ids.contains(id));
        for id in ids {
            let key = (owner, id);
            if !self.windows.contains_key(&key) {
                if let Err(error) = self.open(key) {
                    app.workspace.request_dock(id);
                    app.flash(format!("Could not open file window: {error}"));
                }
            }
        }
    }

    pub(super) fn retain_tabs(&mut self, owners: &[uuid::Uuid]) {
        self.windows.retain(|(owner, _), _| owners.contains(owner));
    }

    /// Drain child input before root drawing. Closing a file HWND docks its existing document.
    pub(super) fn drain(
        &mut self,
        mut dispatch: impl FnMut(uuid::Uuid, u32, Option<Event>),
    ) -> bool {
        let mut changed = false;
        for _ in 0..1024 {
            let Ok(message) = self.receiver.try_recv() else {
                break;
            };
            let (key, event) = match message {
                Message::Closed(key) => {
                    if self.windows.contains_key(&key) {
                        dispatch(key.0, key.1, None);
                    }
                    changed = true;
                    continue;
                }
                Message::Input(key, native_window::NativeEvent::Event(event)) => (key, event),
                Message::Input(key, native_window::NativeEvent::MouseUp { event, .. }) => {
                    (key, Event::Mouse(event))
                }
                Message::Input(_key, native_window::NativeEvent::Attention(_)) => {
                    continue;
                }
            };
            // Messages already queued when a child was docked cannot affect another view.
            if !self.windows.contains_key(&key) {
                continue;
            }
            if matches!(event, Event::Key(key) if key.kind != KeyEventKind::Release && key.code == KeyCode::F(12) && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                dispatch(key.0, key.1, None);
                changed = true;
                continue;
            }
            dispatch(key.0, key.1, Some(event));
            changed = true;
        }
        changed
    }

    /// Render the same documents after the root view; workspace helpers preserve root hit areas.
    pub(super) fn draw(&mut self, owner: uuid::Uuid, app: &mut App) {
        let mut failed = Vec::new();
        for (&key, window) in &mut self.windows {
            if key.0 != owner {
                continue;
            }
            let id = key.1;
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
                failed.push((key, error));
            }
        }
        for (key, error) in failed {
            app.workspace.request_dock(key.1);
            self.windows.remove(&key);
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
