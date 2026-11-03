//! Buffers short plain-key bursts because Windows terminals may report bracketed
//! paste as individual key events instead of `Event::Paste`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

pub const BURST_QUIET: Duration = Duration::from_millis(16);
const MAX_BURST_KEYS: usize = 1024;

#[derive(Debug)]
pub struct KeyBurst {
    pub keys: Vec<KeyEvent>,
    /// True once a newline was seen or the safety bound forced a flush.
    pub text: bool,
    /// Normalized while the buffer still has cross-chunk CRLF state.
    pub text_content: Option<String>,
    pub continues_paste: bool,
}

#[derive(Default)]
pub struct BurstBuffer {
    keys: Vec<KeyEvent>,
    last_at: Option<Instant>,
    force_text: bool,
    paste_latched: bool,
    last_char_was_cr: bool,
    emitted_paste: bool,
    after_quiet: bool,
}

impl BurstBuffer {
    pub fn is_candidate(key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(_) => {
                key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT
            }
            // ConPTY translates pasted LF into Ctrl+Enter, including for bracketed pastes.
            KeyCode::Enter => {
                key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::CONTROL
            }
            KeyCode::Tab => key.modifiers == KeyModifiers::NONE,
            _ => false,
        }
    }

    /// Add an eligible key. A returned chunk is safe to dispatch immediately.
    pub fn push(&mut self, key: KeyEvent, now: Instant) -> Option<KeyBurst> {
        // Only the caller can establish that the terminal input queue was quiet. Time
        // spent drawing a frame between drain batches must not split a pending paste.
        self.add(key, now);
        if self.keys.len() >= MAX_BURST_KEYS {
            self.force_text = true; // On overflow, preserve every key as draft text.
            self.paste_latched = true;
            Some(self.take())
        } else {
            None
        }
    }

    fn add(&mut self, key: KeyEvent, now: Instant) {
        self.keys.push(key);
        self.last_at = Some(now);
    }

    #[cfg(test)]
    pub fn due(&self, now: Instant) -> bool {
        self.last_at
            .is_some_and(|at| now.duration_since(at) >= BURST_QUIET)
    }

    pub fn active(&self) -> bool {
        self.last_at.is_some() || self.paste_latched
    }

    #[cfg(test)]
    pub fn take_if_due(&mut self, now: Instant) -> Option<KeyBurst> {
        if self.due(now) {
            let burst = self.take();
            self.last_at = None;
            self.paste_latched = false;
            self.last_char_was_cr = false;
            self.emitted_paste = false;
            self.after_quiet = false;
            (!burst.keys.is_empty()).then_some(burst)
        } else {
            None
        }
    }

    pub fn take_pending(&mut self) -> Option<KeyBurst> {
        if self.last_at.is_some() {
            let burst = self.take();
            self.last_at = None;
            self.paste_latched = false;
            self.last_char_was_cr = false;
            self.emitted_paste = false;
            self.after_quiet = false;
            (!burst.keys.is_empty()).then_some(burst)
        } else {
            None
        }
    }

    /// Flush keys at an input-source quiet marker without ending a paste that the bounded
    /// buffer already identified. Terminal delivery can pause between chunks of one paste.
    pub fn take_quiet(&mut self) -> Option<KeyBurst> {
        if self.last_at.is_some() {
            let burst = self.take();
            self.last_at = None;
            self.after_quiet = self.continuation_active();
            (!burst.keys.is_empty()).then_some(burst)
        } else {
            None
        }
    }

    pub fn continuation_active(&self) -> bool {
        self.paste_latched || self.emitted_paste || self.last_char_was_cr
    }

    /// End cross-chunk text normalization and undo grouping at an explicit key action.
    pub fn end_continuation(&mut self) {
        self.paste_latched = false;
        self.last_char_was_cr = false;
        self.emitted_paste = false;
        self.after_quiet = false;
    }

    pub fn take(&mut self) -> KeyBurst {
        let has_newline = self.keys.iter().any(|key| {
            key.code == KeyCode::Enter || matches!(key.code, KeyCode::Char('\n' | '\r'))
        });
        let raw_linebreak = self
            .keys
            .iter()
            .any(|key| matches!(key.code, KeyCode::Char('\n' | '\r')));
        let separate_action = self.after_quiet
            && self.keys.len() == 1
            && matches!(
                (self.keys[0].code, self.keys[0].modifiers),
                (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Tab, KeyModifiers::NONE)
            );
        let text = !separate_action
            && (self.force_text
                || self.paste_latched
                || self.emitted_paste
                || (self.last_char_was_cr && has_newline)
                || (has_newline && (self.keys.len() > 1 || raw_linebreak)));
        self.force_text = false;
        let keys = std::mem::take(&mut self.keys);
        let text_content = text.then(|| self.render_text(&keys));
        let continues_paste = self.emitted_paste;
        self.emitted_paste |= text;
        if separate_action {
            self.end_continuation();
        } else {
            self.after_quiet = false;
        }
        KeyBurst {
            keys,
            text,
            text_content,
            continues_paste,
        }
    }

    pub fn render_text(&mut self, keys: &[KeyEvent]) -> String {
        let mut out = String::new();
        for key in keys {
            let ch = match key.code {
                KeyCode::Char(c) => Some(c),
                KeyCode::Enter if key.modifiers == KeyModifiers::NONE => Some('\r'),
                KeyCode::Enter => Some('\n'),
                KeyCode::Tab => Some('\t'),
                _ => None,
            };
            if let Some(ch) = ch {
                if ch == '\n' && self.last_char_was_cr {
                    self.last_char_was_cr = false;
                    continue;
                }
                if ch == '\r' {
                    out.push('\n');
                    self.last_char_was_cr = true;
                } else {
                    out.push(ch);
                    self.last_char_was_cr = false;
                }
            }
        }
        out
    }
}

/// Convert a text burst to editor content; CRLF and bare CR become one logical newline.
#[cfg(test)]
pub fn burst_text(keys: &[KeyEvent]) -> String {
    let mut out = String::new();
    let mut previous_cr = false;
    for key in keys {
        let ch = match key.code {
            KeyCode::Char('\r') => Some('\r'),
            KeyCode::Char('\n') | KeyCode::Enter => Some('\n'),
            KeyCode::Char(c) => Some(c),
            KeyCode::Tab => Some('\t'),
            _ => None,
        };
        if let Some(ch) = ch {
            if ch == '\n' && previous_cr {
                previous_cr = false;
                continue;
            }
            if ch == '\r' {
                out.push('\n');
                previous_cr = true;
            } else {
                out.push(ch);
                previous_cr = false;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn windows_pasted_lf_is_ctrl_enter_and_crlf_is_one_newline() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = super::super::App::new(dir.path().into(), dir.path().into());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut buffer = BurstBuffer::default();
        let start = Instant::now();
        let keys = [
            key(KeyCode::Char('a')),
            key(KeyCode::Enter),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
            key(KeyCode::Char('b')),
        ];
        for (i, key) in keys.into_iter().enumerate() {
            super::super::dispatch_key_event(
                &mut app,
                key,
                &mut buffer,
                &tx,
                start + Duration::from_millis(i as u64),
            );
        }
        let burst = buffer.take_if_due(start + Duration::from_secs(1)).unwrap();
        super::super::dispatch_key_burst(&mut app, burst, &tx);
        assert_eq!(app.editor.text(), "a\n\nb");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn one_undo_restores_the_draft_before_a_paste_with_several_overflow_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = super::super::App::new(dir.path().into(), dir.path().into());
        app.editor.set_text("existing draft: ");
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut buffer = BurstBuffer::default();
        let start = Instant::now();
        for i in 0..MAX_BURST_KEYS * 3 + 17 {
            if let Some(burst) = buffer.push(
                key(KeyCode::Char('x')),
                start + Duration::from_micros(i as u64),
            ) {
                super::super::dispatch_key_burst(&mut app, burst, &tx);
            }
        }
        let burst = buffer.take_if_due(start + Duration::from_secs(1)).unwrap();
        super::super::dispatch_key_burst(&mut app, burst, &tx);
        assert!(app.editor.undo());
        assert_eq!(app.editor.text(), "existing draft: ");
        assert!(app.editor.redo());
        assert_eq!(
            app.editor.text(),
            format!("existing draft: {}", "x".repeat(MAX_BURST_KEYS * 3 + 17))
        );
    }

    #[test]
    fn newline_burst_is_text_and_preserves_tabs_unicode_blank_and_trailing_lines() {
        let keys = [
            key(KeyCode::Char('a')),
            key(KeyCode::Tab),
            key(KeyCode::Char('界')),
            key(KeyCode::Enter),
            key(KeyCode::Enter),
            key(KeyCode::Char('z')),
            key(KeyCode::Enter),
        ];
        assert_eq!(burst_text(&keys), "a\t界\n\nz\n");
    }

    #[test]
    fn quiet_enter_is_a_single_key_submission_candidate() {
        let start = Instant::now();
        let mut buffer = BurstBuffer::default();
        assert!(buffer.push(key(KeyCode::Enter), start).is_none());
        let burst = buffer.take_if_due(start + BURST_QUIET).unwrap();
        assert!(!burst.text);
        assert_eq!(burst.keys[0].kind, KeyEventKind::Press);
    }

    #[test]
    fn crlf_is_one_line_break_and_repeated_chars_are_retained() {
        let keys = [
            key(KeyCode::Char('x')),
            key(KeyCode::Char('x')),
            key(KeyCode::Char('\r')),
            key(KeyCode::Char('\n')),
        ];
        assert_eq!(burst_text(&keys), "xx\n");
    }

    #[test]
    fn multiline_paste_stays_in_draft_until_a_later_enter_submits_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = super::super::App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let start = Instant::now();
        let mut buffer = BurstBuffer::default();
        let pasted = "first\n\nsecond\n";
        for (i, ch) in pasted.chars().enumerate() {
            let key = if ch == '\n' {
                key(KeyCode::Enter)
            } else {
                key(KeyCode::Char(ch))
            };
            assert!(buffer
                .push(key, start + Duration::from_millis(i as u64))
                .is_none());
        }
        let burst = buffer
            .take_if_due(start + Duration::from_millis(pasted.chars().count() as u64 + 20))
            .unwrap();
        assert!(burst.text);
        assert!(!super::super::dispatch_key_burst(&mut app, burst, &tx));
        assert_eq!(app.editor.text(), pasted);
        assert!(rx.try_recv().is_err());

        let enter_at = start + Duration::from_secs(1);
        buffer.push(key(KeyCode::Enter), enter_at);
        let burst = buffer.take_if_due(enter_at + BURST_QUIET).unwrap();
        assert!(!burst.text);
        assert!(!super::super::dispatch_key_burst(&mut app, burst, &tx));
        assert_eq!(rx.try_recv().unwrap(), pasted);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn overflow_flush_keeps_paste_mode_until_quiet_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = super::super::App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let start = Instant::now();
        let mut buffer = BurstBuffer::default();
        let mut text = String::new();
        let mut first_chunk = None;
        for i in 0..MAX_BURST_KEYS - 1 {
            text.push('a');
            if let Some(chunk) = buffer.push(
                key(KeyCode::Char('a')),
                start + Duration::from_micros(i as u64),
            ) {
                first_chunk = Some(chunk);
            }
        }
        if let Some(chunk) = buffer.push(
            key(KeyCode::Enter),
            start + Duration::from_micros(MAX_BURST_KEYS as u64),
        ) {
            first_chunk = Some(chunk);
        }
        assert!(first_chunk.as_ref().unwrap().text);
        assert!(!super::super::dispatch_key_burst(
            &mut app,
            first_chunk.unwrap(),
            &tx
        ));
        assert!(rx.try_recv().is_err());

        buffer.push(
            key(KeyCode::Enter),
            start + Duration::from_micros(MAX_BURST_KEYS as u64 + 1),
        );
        let tail = buffer
            .take_if_due(start + Duration::from_millis(30))
            .unwrap();
        assert!(tail.text);
        assert!(!super::super::dispatch_key_burst(&mut app, tail, &tx));
        text.push('\n');
        text.push('\n');
        assert_eq!(app.editor.text(), text);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn source_quiet_keeps_overflow_paste_undo_and_crlf_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = super::super::App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        app.editor.set_text("draft: ");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut buffer = BurstBuffer::default();
        let start = Instant::now();
        for i in 0..MAX_BURST_KEYS {
            if let Some(burst) = buffer.push(
                key(KeyCode::Char('x')),
                start + Duration::from_micros(i as u64),
            ) {
                super::super::dispatch_key_burst(&mut app, burst, &tx);
            }
        }
        assert!(buffer.take_quiet().is_none());
        for i in 0..MAX_BURST_KEYS {
            if let Some(burst) = buffer.push(
                key(KeyCode::Char('y')),
                start + Duration::from_secs(1) + Duration::from_micros(i as u64),
            ) {
                assert!(burst.continues_paste);
                super::super::dispatch_key_burst(&mut app, burst, &tx);
            }
        }
        assert!(buffer.take_quiet().is_none());
        assert!(app.editor.undo());
        assert_eq!(app.editor.text(), "draft: ");
        assert!(rx.try_recv().is_err());

        let mut crlf = BurstBuffer::default();
        let cr = [key(KeyCode::Char('a')), key(KeyCode::Enter)];
        for (i, key) in cr.into_iter().enumerate() {
            crlf.push(key, start + Duration::from_millis(i as u64));
        }
        let first = crlf.take_quiet().unwrap();
        assert!(first.text);
        assert_eq!(first.text_content.as_deref(), Some("a\n"));
        let tail_at = start + Duration::from_secs(2);
        crlf.push(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
            tail_at,
        );
        let tail = crlf.take_quiet().unwrap();
        assert!(tail.text);
        assert_eq!(tail.text_content.as_deref(), Some(""));

        let mut send_app =
            super::super::App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        let (send_tx, mut send_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut send_buffer = BurstBuffer::default();
        for i in 0..MAX_BURST_KEYS {
            if let Some(burst) = send_buffer.push(
                key(KeyCode::Char('z')),
                start + Duration::from_micros(i as u64),
            ) {
                super::super::dispatch_key_burst(&mut send_app, burst, &send_tx);
            }
        }
        assert!(send_buffer.take_quiet().is_none());
        assert!(!super::super::dispatch_key_event(
            &mut send_app,
            key(KeyCode::Enter),
            &mut send_buffer,
            &send_tx,
            start + Duration::from_secs(3),
        ));
        let submit = send_buffer.take_quiet().unwrap();
        assert!(!submit.text);
        super::super::dispatch_key_burst(&mut send_app, submit, &send_tx);
        assert_eq!(send_rx.try_recv().unwrap(), "z".repeat(MAX_BURST_KEYS));
    }

    #[test]
    fn plain_enter_after_quiet_waits_for_the_next_quiet_before_submitting() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = super::super::App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut buffer = BurstBuffer::default();
        let start = Instant::now();

        for (i, key) in [key(KeyCode::Char('a')), key(KeyCode::Enter)]
            .into_iter()
            .enumerate()
        {
            super::super::dispatch_key_event(
                &mut app,
                key,
                &mut buffer,
                &tx,
                start + Duration::from_millis(i as u64),
            );
        }
        let first = buffer.take_quiet().unwrap();
        super::super::dispatch_key_burst(&mut app, first, &tx);

        // A lone plain Enter is ambiguous until the next quiet marker. A Ctrl+Enter arriving
        // first proves that the first Enter belonged to the split paste; neither sends a turn.
        super::super::dispatch_key_event(
            &mut app,
            key(KeyCode::Enter),
            &mut buffer,
            &tx,
            start + Duration::from_secs(1),
        );
        super::super::dispatch_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
            &mut buffer,
            &tx,
            start + Duration::from_secs(1) + Duration::from_millis(1),
        );
        let continuation = buffer.take_quiet().unwrap();
        assert!(continuation.text);
        super::super::dispatch_key_burst(&mut app, continuation, &tx);
        assert!(rx.try_recv().is_err());
        assert_eq!(app.editor.text(), "a\n\n");

        super::super::dispatch_key_event(
            &mut app,
            key(KeyCode::Enter),
            &mut buffer,
            &tx,
            start + Duration::from_secs(2),
        );
        let submit = buffer.take_quiet().unwrap();
        assert!(!submit.text);
        super::super::dispatch_key_burst(&mut app, submit, &tx);
        // Submission preserves both pasted newlines as well as the draft.
        assert_eq!(rx.try_recv().unwrap(), "a\n\n");
    }

    #[test]
    fn exact_overflow_chunk_quiet_clears_mode_before_next_enter() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = super::super::App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let start = Instant::now();
        let mut buffer = BurstBuffer::default();
        let mut chunk = None;
        for i in 0..MAX_BURST_KEYS {
            chunk = buffer
                .push(
                    key(KeyCode::Char('q')),
                    start + Duration::from_micros(i as u64),
                )
                .or(chunk);
        }
        let chunk = chunk.expect("the full buffer should flush");
        assert!(chunk.text);
        assert!(!super::super::dispatch_key_burst(&mut app, chunk, &tx));
        assert_eq!(app.editor.text(), "q".repeat(MAX_BURST_KEYS));
        assert!(rx.try_recv().is_err());

        assert!(buffer
            .take_if_due(start + Duration::from_millis(30))
            .is_none());
        let enter_at = start + Duration::from_secs(1);
        buffer.push(key(KeyCode::Enter), enter_at);
        let enter = buffer.take_if_due(enter_at + BURST_QUIET).unwrap();
        assert!(!enter.text);
        assert!(!super::super::dispatch_key_burst(&mut app, enter, &tx));
        assert_eq!(rx.try_recv().unwrap(), "q".repeat(MAX_BURST_KEYS));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn crlf_at_overflow_boundary_is_normalized_once_through_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = super::super::App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut buffer = BurstBuffer::default();
        let start = Instant::now();
        for i in 0..MAX_BURST_KEYS - 1 {
            assert!(!super::super::dispatch_key_event(
                &mut app,
                key(KeyCode::Char('a')),
                &mut buffer,
                &tx,
                start + Duration::from_micros(i as u64),
            ));
        }
        assert!(!super::super::dispatch_key_event(
            &mut app,
            key(KeyCode::Char('\r')),
            &mut buffer,
            &tx,
            start + Duration::from_micros(MAX_BURST_KEYS as u64),
        ));
        assert_eq!(
            app.editor.text(),
            format!("{}\n", "a".repeat(MAX_BURST_KEYS - 1))
        );

        assert!(!super::super::dispatch_key_event(
            &mut app,
            key(KeyCode::Char('\n')),
            &mut buffer,
            &tx,
            start + Duration::from_micros(MAX_BURST_KEYS as u64 + 1),
        ));
        let tail = buffer
            .take_if_due(start + Duration::from_millis(30))
            .unwrap();
        assert!(!super::super::dispatch_key_burst(&mut app, tail, &tx));
        assert_eq!(
            app.editor.text(),
            format!("{}\n", "a".repeat(MAX_BURST_KEYS - 1))
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn key_dispatch_ignores_releases_and_accepts_shifted_text() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = super::super::App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut buffer = BurstBuffer::default();
        let start = Instant::now();
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('B'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert!(!super::super::dispatch_key_event(
            &mut app,
            release,
            &mut buffer,
            &tx,
            start,
        ));
        assert!(!buffer.active());
        let shifted = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);
        assert!(!super::super::dispatch_key_event(
            &mut app,
            shifted,
            &mut buffer,
            &tx,
            start + Duration::from_millis(1),
        ));
        let burst = buffer
            .take_if_due(start + Duration::from_millis(20))
            .unwrap();
        assert_eq!(burst.keys, vec![shifted]);
        assert!(!super::super::dispatch_key_burst(&mut app, burst, &tx));
        assert_eq!(app.editor.text(), "A");
    }

    #[test]
    fn slow_redraw_between_pending_input_batches_does_not_split_paste() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = super::super::App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut buffer = BurstBuffer::default();
        let start = Instant::now();
        for i in 0..256 {
            super::super::dispatch_key_event(
                &mut app,
                key(KeyCode::Char('x')),
                &mut buffer,
                &tx,
                start + Duration::from_micros(i),
            );
        }
        // More input is already queued, but a slow redraw delayed its processing.
        let after_draw = start + Duration::from_millis(50);
        super::super::dispatch_key_event(
            &mut app,
            key(KeyCode::Enter),
            &mut buffer,
            &tx,
            after_draw,
        );
        let burst = buffer.take_if_due(after_draw + BURST_QUIET).unwrap();
        super::super::dispatch_key_burst(&mut app, burst, &tx);
        assert_eq!(app.editor.text(), format!("{}\n", "x".repeat(256)));
        assert!(
            rx.try_recv().is_err(),
            "queued paste must not submit after redraw"
        );
    }

    #[test]
    fn slash_looking_multiline_paste_is_kept_as_text_not_dispatched_as_command() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = super::super::App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut buffer = BurstBuffer::default();
        let start = Instant::now();
        let pasted = "/doctor\nsecond\n";
        for (i, ch) in pasted.chars().enumerate() {
            let code = if ch == '\n' {
                KeyCode::Enter
            } else {
                KeyCode::Char(ch)
            };
            assert!(!super::super::dispatch_key_event(
                &mut app,
                KeyEvent::new(code, KeyModifiers::NONE),
                &mut buffer,
                &tx,
                start + Duration::from_millis(i as u64),
            ));
        }
        let burst = buffer
            .take_if_due(start + Duration::from_millis(pasted.chars().count() as u64 + 20))
            .unwrap();
        assert!(burst.text);
        assert!(!super::super::dispatch_key_burst(&mut app, burst, &tx));
        assert_eq!(app.editor.text(), pasted);
        assert!(rx.try_recv().is_err());
    }
}
