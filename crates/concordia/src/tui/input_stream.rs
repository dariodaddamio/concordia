//! Read terminal events independently of drawing so idle gaps cannot disappear while the UI is
//! busy. The main loop consumes stamped events and flushes inference bursts at recorded gaps.

use crossterm::event::{self, Event};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(windows)]
mod windows_vt;

const EVENT_CAPACITY: usize = 4096;

enum Message {
    Event(Event, Instant),
    Quiet(Instant),
    Error(io::Error),
}

/// Owns the only Crossterm event reader for the TUI. Events and quiet-boundary markers are bounded
/// and retain their observed order and timestamps, independently of render-loop delays.
pub struct InputStream {
    receiver: Option<Receiver<Message>>,
    pending: Option<Message>,
    shutdown: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl InputStream {
    /// Start the reader. Call this only after terminal input modes have been configured.
    pub fn spawn() -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(EVENT_CAPACITY);
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = shutdown.clone();
        let reader = thread::Builder::new()
            .name("concordia-terminal-input".into())
            .spawn(move || read_events(sender, thread_shutdown, startup_tx))?;
        if let Err(error) = wait_for_startup(startup_rx) {
            shutdown.store(true, Ordering::Release);
            drop(receiver);
            let _ = reader.join();
            return Err(error);
        }
        Ok(Self {
            receiver: Some(receiver),
            pending: None,
            shutdown,
            reader: Some(reader),
        })
    }

    /// Wait for an event for at most `timeout`. A queued reader error is returned immediately.
    pub fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
        if let Some(message) = &self.pending {
            return match message {
                Message::Event(..) | Message::Quiet(..) => Ok(true),
                Message::Error(error) => Err(clone_error(error)),
            };
        }
        let receiver = self.receiver.as_ref().expect("reader receiver is alive");
        match receiver.recv_timeout(timeout) {
            Ok(message @ (Message::Event(..) | Message::Quiet(..))) => {
                self.pending = Some(message);
                Ok(true)
            }
            Ok(Message::Error(error)) => {
                let result = Err(clone_error(&error));
                self.pending = Some(Message::Error(error));
                result
            }
            Err(RecvTimeoutError::Timeout) => Ok(false),
            Err(RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "terminal input reader stopped",
            )),
        }
    }

    /// Return the next event or ordered idle marker, its reader timestamp, and whether it marks a
    /// quiet boundary. A boundary is `(None, at, true)`. Intended to follow `poll(true)`.
    pub fn read(&mut self) -> io::Result<(Option<Event>, Instant, bool)> {
        let message = match self.pending.take() {
            Some(message) => message,
            None => self
                .receiver
                .as_ref()
                .expect("reader receiver is alive")
                .recv()
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "terminal input reader stopped",
                    )
                })?,
        };
        match message {
            Message::Event(event, at) => Ok((Some(event), at, false)),
            Message::Quiet(at) => Ok((None, at, true)),
            Message::Error(error) => Err(error),
        }
    }
}

impl Drop for InputStream {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        // A full bounded channel can block the reader in send; dropping the receiver releases it.
        drop(self.receiver.take());
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[derive(Default)]
struct QuietBoundary {
    marker_sent: bool,
}

impl QuietBoundary {
    fn timed_out(&mut self) -> bool {
        if self.marker_sent {
            false
        } else {
            self.marker_sent = true;
            true
        }
    }

    fn event_arrived(&mut self) {
        self.marker_sent = false;
    }
}

fn read_events(
    sender: SyncSender<Message>,
    shutdown: Arc<AtomicBool>,
    startup: SyncSender<io::Result<()>>,
) {
    #[cfg(windows)]
    {
        match windows_vt::WindowsVt::start() {
            Ok(mut input) => {
                let _ = startup.send(Ok(()));
                if let Err(error) = input.run(&sender, &shutdown) {
                    let _ = sender.send(Message::Error(error));
                }
                return;
            }
            Err(windows_vt::StartError::Unsupported) => {
                // VT input is unsupported by this host/handle. No bytes have been read, so it is
                // safe to fall back to Crossterm's native Windows INPUT_RECORD reader.
            }
            Err(windows_vt::StartError::Failed(error)) => {
                let _ = startup.send(Err(io::Error::new(error.kind(), error.to_string())));
                return;
            }
        }
    }

    let _ = startup.send(Ok(()));
    read_crossterm_events(sender, shutdown);
}

fn wait_for_startup(startup: Receiver<io::Result<()>>) -> io::Result<()> {
    startup
        .recv()
        .map_err(|_| io::Error::other("terminal input reader exited before initialization"))?
}

fn read_crossterm_events(sender: SyncSender<Message>, shutdown: Arc<AtomicBool>) {
    let mut boundary = QuietBoundary::default();
    while !shutdown.load(Ordering::Acquire) {
        match event::poll(super::input::BURST_QUIET) {
            Ok(false) => {
                if boundary.timed_out() && sender.send(Message::Quiet(Instant::now())).is_err() {
                    break;
                }
            }
            Ok(true) => match event::read() {
                Ok(event) => {
                    #[cfg(windows)]
                    let event = windows_vt::normalize_event(event);
                    let message = Message::Event(event, Instant::now());
                    boundary.event_arrived();
                    if sender.send(message).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Message::Error(error));
                    break;
                }
            },
            Err(error) => {
                let _ = sender.send(Message::Error(error));
                break;
            }
        }
    }
}

fn clone_error(error: &io::Error) -> io::Error {
    io::Error::new(error.kind(), error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn startup_waits_for_source_commit_and_propagates_initialization_failure() {
        let (tx, rx) = mpsc::sync_channel(1);
        let committed = Arc::new(AtomicBool::new(false));
        let reader_committed = committed.clone();
        let reader = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            reader_committed.store(true, Ordering::Release);
            tx.send(Ok(())).unwrap();
        });
        wait_for_startup(rx).unwrap();
        assert!(committed.load(Ordering::Acquire));
        reader.join().unwrap();

        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "input mode unavailable",
        )))
        .unwrap();
        assert_eq!(
            wait_for_startup(rx).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }

    #[test]
    fn an_idle_transition_emits_one_marker_until_input_arrives() {
        let mut boundary = QuietBoundary::default();
        assert!(boundary.timed_out());
        assert!(!boundary.timed_out());
        boundary.event_arrived();
        assert!(boundary.timed_out());
    }

    #[test]
    fn text_quiet_tab_quiet_text_order_survives_a_delayed_consumer() {
        let (sender, receiver) = mpsc::sync_channel(8);
        let first_at = Instant::now();
        let second_at = first_at + super::super::input::BURST_QUIET;
        let third_at = second_at + super::super::input::BURST_QUIET;
        let first = Event::Paste("queued original".into());
        let tab = Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let last = Event::Paste("main draft\nstays here".into());
        for message in [
            Message::Event(first.clone(), first_at),
            Message::Quiet(second_at),
            Message::Event(tab.clone(), second_at),
            Message::Quiet(third_at),
            Message::Event(last.clone(), third_at),
        ] {
            assert!(sender.send(message).is_ok());
        }
        let mut stream = InputStream {
            receiver: Some(receiver),
            pending: None,
            shutdown: Arc::new(AtomicBool::new(true)),
            reader: None,
        };
        let expected = [
            (Some(first), first_at, false),
            (None, second_at, true),
            (Some(tab), second_at, false),
            (None, third_at, true),
            (Some(last), third_at, false),
        ];
        for item in expected {
            assert!(stream.poll(Duration::ZERO).unwrap());
            assert_eq!(stream.read().unwrap(), item);
        }
        assert!(!stream.poll(Duration::ZERO).unwrap());
    }

    #[test]
    fn dropping_stream_closes_a_full_channel_before_joining_the_reader() {
        let (sender, receiver) = mpsc::sync_channel(1);
        assert!(sender.send(Message::Quiet(Instant::now())).is_ok());
        let (started_tx, started_rx) = mpsc::channel();
        let reader = thread::spawn(move || {
            started_tx.send(()).unwrap();
            let _ = sender.send(Message::Quiet(Instant::now()));
        });
        started_rx.recv().unwrap();
        let stream = InputStream {
            receiver: Some(receiver),
            pending: None,
            shutdown: Arc::new(AtomicBool::new(false)),
            reader: Some(reader),
        };
        drop(stream);
    }
}
