//! Multi-line prompt editor: cursor, selection, word/line movement, click hit-testing.
//!
//! Pure data + logic (no terminal I/O) so it can be unit tested. Indices are char indices into
//! `text`; layout is recomputed per width because rows soft-wrap.

use super::text::{col_to_index, next_word, prev_word, slice_width, word_bounds, wrap_ranges};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowSpan {
    pub start: usize,
    pub end: usize,
    /// Last visual row of its logical line (ends at a `\n` or end of text).
    pub last: bool,
}

/// Undo steps kept per editor.
const UNDO_CAP: usize = 200;

/// A restorable editor state: text, caret and selection anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Snap {
    text: Vec<char>,
    cursor: usize,
    anchor: Option<usize>,
}

/// Edits that merge into the previous undo step while they continue at the same caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Burst {
    /// Typing single characters.
    Insert,
    /// Backspace / Delete one character at a time.
    Delete,
    /// Consecutive chunks of a single terminal-delivered paste.
    Paste,
}

#[derive(Default)]
struct UndoStack {
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    /// The burst the last edit belongs to, and the caret it left behind.
    burst: Option<(Burst, usize)>,
}

#[derive(Default)]
pub struct Editor {
    text: Vec<char>,
    cursor: usize,
    anchor: Option<usize>,
    /// Display column to return to when moving up/down across rows of different length.
    goal_col: Option<usize>,
    hist: UndoStack,
    /// Bumped whenever the text changes (edit, undo, redo); lets callers notice a dirty draft.
    revision: u64,
}

impl Editor {
    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    pub fn chars(&self) -> &[char] {
        &self.text
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    #[cfg(test)]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Empty the buffer. One undo step, so Ctrl+Z after a send brings the sent text back.
    pub fn clear(&mut self) {
        self.edit(None, |e| {
            e.text.clear();
            e.cursor = 0;
            e.anchor = None;
            e.goal_col = None;
        });
    }

    /// Replace the whole buffer; cursor goes to the end. One undo step.
    pub fn set_text(&mut self, s: &str) {
        self.edit(None, |e| {
            e.text = s.chars().collect();
            e.cursor = e.text.len();
            e.anchor = None;
            e.goal_col = None;
        });
    }

    /// Forget all undo / redo steps (a fresh buffer, not an edit of an old one).
    pub fn reset_undo(&mut self) {
        self.hist = UndoStack::default();
    }

    /// Changes whenever the text does.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    // ---- undo / redo -----------------------------------------------------------------

    fn snap(&self) -> Snap {
        Snap {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        }
    }

    fn restore(&mut self, s: Snap) {
        self.cursor = s.cursor.min(s.text.len());
        self.anchor = s.anchor.filter(|a| *a <= s.text.len());
        self.text = s.text;
        self.goal_col = None;
        self.hist.burst = None;
        self.revision += 1;
    }

    /// Run a text-changing operation and record it as an undo step (merged into the previous
    /// step when it continues the same typing / deleting burst). A no-op records nothing.
    fn edit(&mut self, burst: Option<Burst>, f: impl FnOnce(&mut Self)) {
        let before = self.snap();
        let had_selection = self.selection().is_some();
        f(self);
        if self.text == before.text {
            return;
        }
        self.revision += 1;
        let merge =
            !had_selection && burst.is_some_and(|b| self.hist.burst == Some((b, before.cursor)));
        if !merge {
            self.hist.undo.push(before);
            if self.hist.undo.len() > UNDO_CAP {
                self.hist.undo.remove(0);
            }
        }
        self.hist.redo.clear();
        self.hist.burst = burst.map(|b| (b, self.cursor));
    }

    /// Step back. Returns whether anything changed.
    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.hist.undo.pop() else {
            return false;
        };
        let cur = self.snap();
        self.hist.redo.push(cur);
        self.restore(prev);
        true
    }

    /// Step forward again after an undo.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.hist.redo.pop() else {
            return false;
        };
        let cur = self.snap();
        self.hist.undo.push(cur);
        self.restore(next);
        true
    }

    // ---- selection -------------------------------------------------------------------

    /// Ordered, non-empty selection.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        if a == self.cursor {
            return None;
        }
        Some((a.min(self.cursor), a.max(self.cursor)))
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection()
            .map(|(a, b)| self.text[a..b].iter().collect())
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    pub fn select_all(&mut self) {
        self.hist.burst = None;
        self.anchor = Some(0);
        self.cursor = self.text.len();
        self.goal_col = None;
    }

    pub fn select_range(&mut self, a: usize, b: usize) {
        let n = self.text.len();
        self.hist.burst = None;
        self.anchor = Some(a.min(n));
        self.cursor = b.min(n);
        self.goal_col = None;
    }

    fn begin_move(&mut self, extend: bool) {
        self.hist.burst = None;
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
    }

    /// Delete the selection if any; returns whether something was deleted.
    fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        self.text.drain(a..b);
        self.cursor = a;
        self.anchor = None;
        self.goal_col = None;
        true
    }

    /// Remove and return the selected text (cut).
    pub fn cut(&mut self) -> Option<String> {
        let t = self.selected_text()?;
        self.edit(None, |e| {
            e.delete_selection();
        });
        Some(t)
    }

    // ---- editing ---------------------------------------------------------------------

    /// Insert at the cursor, replacing any selection. Normalize line endings; tabs become 4 spaces.
    pub fn insert_str(&mut self, s: &str) {
        let single = s.chars().count() == 1 && s != "\n" && s != "\r";
        self.edit(single.then_some(Burst::Insert), |e| e.insert_raw(s));
    }

    pub fn insert_paste_chunk(&mut self, s: &str, continues: bool) {
        if !continues {
            self.hist.burst = None;
        }
        self.edit(Some(Burst::Paste), |e| e.insert_raw(s));
    }

    fn insert_raw(&mut self, s: &str) {
        self.delete_selection();
        let clean: Vec<char> = super::text::normalize_newlines(s)
            .chars()
            .flat_map(|c| if c == '\t' { vec![' '; 4] } else { vec![c] })
            .collect();
        let n = clean.len();
        self.text.splice(self.cursor..self.cursor, clean);
        self.cursor += n;
        self.goal_col = None;
    }

    pub fn insert_char(&mut self, c: char) {
        let mut buf = [0u8; 4];
        self.insert_str(c.encode_utf8(&mut buf));
    }

    pub fn backspace(&mut self) {
        self.edit(Some(Burst::Delete), |e| {
            if e.delete_selection() || e.cursor == 0 {
                return;
            }
            e.cursor -= 1;
            e.text.remove(e.cursor);
            e.goal_col = None;
        });
    }

    pub fn delete(&mut self) {
        self.edit(Some(Burst::Delete), |e| {
            if e.delete_selection() || e.cursor >= e.text.len() {
                return;
            }
            e.text.remove(e.cursor);
            e.goal_col = None;
        });
    }

    pub fn delete_word_left(&mut self) {
        self.edit(None, |e| {
            if e.delete_selection() {
                return;
            }
            let to = prev_word(&e.text, e.cursor);
            e.text.drain(to..e.cursor);
            e.cursor = to;
            e.goal_col = None;
        });
    }

    pub fn delete_word_right(&mut self) {
        self.edit(None, |e| {
            if e.delete_selection() {
                return;
            }
            let to = next_word(&e.text, e.cursor);
            e.text.drain(e.cursor..to);
            e.goal_col = None;
        });
    }

    // ---- movement --------------------------------------------------------------------

    pub fn move_left(&mut self, extend: bool) {
        // Collapsing a selection with a plain arrow jumps to its edge, like every editor.
        if !extend {
            if let Some((a, _)) = self.selection() {
                self.hist.burst = None;
                self.cursor = a;
                self.anchor = None;
                self.goal_col = None;
                return;
            }
        }
        self.begin_move(extend);
        self.cursor = self.cursor.saturating_sub(1);
        self.goal_col = None;
    }

    pub fn move_right(&mut self, extend: bool) {
        if !extend {
            if let Some((_, b)) = self.selection() {
                self.hist.burst = None;
                self.cursor = b;
                self.anchor = None;
                self.goal_col = None;
                return;
            }
        }
        self.begin_move(extend);
        self.cursor = (self.cursor + 1).min(self.text.len());
        self.goal_col = None;
    }

    pub fn word_left(&mut self, extend: bool) {
        self.begin_move(extend);
        self.cursor = prev_word(&self.text, self.cursor);
        self.goal_col = None;
    }

    pub fn word_right(&mut self, extend: bool) {
        self.begin_move(extend);
        self.cursor = next_word(&self.text, self.cursor);
        self.goal_col = None;
    }

    pub fn move_to(&mut self, idx: usize, extend: bool) {
        self.begin_move(extend);
        self.cursor = idx.min(self.text.len());
        self.goal_col = None;
    }

    pub fn home(&mut self, width: usize, extend: bool) {
        let rows = self.layout(width);
        let r = self.row_of(&rows, self.cursor);
        self.move_to(rows[r].start, extend);
    }

    pub fn end(&mut self, width: usize, extend: bool) {
        let rows = self.layout(width);
        let r = self.row_of(&rows, self.cursor);
        let mut to = rows[r].end;
        // On a soft-wrapped row, stay before the hanging space / on the row.
        if !rows[r].last && to > rows[r].start {
            to -= 1;
        }
        self.move_to(to, extend);
    }

    pub fn up(&mut self, width: usize, extend: bool) {
        self.vertical(width, extend, -1);
    }

    pub fn down(&mut self, width: usize, extend: bool) {
        self.vertical(width, extend, 1);
    }

    fn vertical(&mut self, width: usize, extend: bool, dir: isize) {
        let rows = self.layout(width);
        let r = self.row_of(&rows, self.cursor);
        let col = self
            .goal_col
            .unwrap_or_else(|| slice_width(&self.text[rows[r].start..self.cursor]));
        let target = r as isize + dir;
        self.begin_move(extend);
        if target < 0 {
            self.cursor = 0;
            self.goal_col = None;
            return;
        }
        if target as usize >= rows.len() {
            self.cursor = self.text.len();
            self.goal_col = None;
            return;
        }
        self.cursor = self.index_in_row(&rows[target as usize], col);
        self.goal_col = Some(col);
    }

    // ---- layout & hit-testing --------------------------------------------------------

    /// Visual rows for the given text width (always at least one).
    pub fn layout(&self, width: usize) -> Vec<RowSpan> {
        let mut out = Vec::new();
        let mut line_start = 0;
        loop {
            let line_end = self.text[line_start..]
                .iter()
                .position(|c| *c == '\n')
                .map(|p| line_start + p)
                .unwrap_or(self.text.len());
            let ranges = wrap_ranges(&self.text[line_start..line_end], width);
            let n = ranges.len();
            for (i, (a, b)) in ranges.into_iter().enumerate() {
                out.push(RowSpan {
                    start: line_start + a,
                    end: line_start + b,
                    last: i + 1 == n,
                });
            }
            if line_end >= self.text.len() {
                break;
            }
            line_start = line_end + 1;
        }
        out
    }

    /// Row containing char index `idx` (a boundary shared by a wrap belongs to the later row;
    /// the end of a logical line belongs to that line).
    pub fn row_of(&self, rows: &[RowSpan], idx: usize) -> usize {
        for (i, r) in rows.iter().enumerate() {
            if idx < r.end || (idx == r.end && r.last) {
                return i;
            }
        }
        rows.len().saturating_sub(1)
    }

    /// `(row, display column)` of the cursor.
    pub fn cursor_pos(&self, width: usize) -> (usize, usize) {
        let rows = self.layout(width);
        let r = self.row_of(&rows, self.cursor);
        (r, slice_width(&self.text[rows[r].start..self.cursor]))
    }

    fn index_in_row(&self, row: &RowSpan, col: usize) -> usize {
        let slice = &self.text[row.start..row.end];
        let mut i = col_to_index(slice, col);
        // Never place the caret after a hanging soft-wrap space (it would render on the next row).
        if !row.last && i >= slice.len() && !slice.is_empty() {
            i = slice.len() - 1;
        }
        row.start + i
    }

    /// Char index for a click at `(row, col)` in layout space.
    pub fn index_at(&self, width: usize, row: usize, col: usize) -> usize {
        let rows = self.layout(width);
        let r = row.min(rows.len() - 1);
        self.index_in_row(&rows[r], col)
    }

    /// Double-click: select the word at `idx`.
    pub fn select_word_at(&mut self, idx: usize) {
        let (a, b) = word_bounds(&self.text, idx.min(self.text.len().saturating_sub(1)));
        self.select_range(a, b);
    }

    /// Triple-click: select the logical line (between newlines) at `idx`.
    pub fn select_line_at(&mut self, idx: usize) {
        let idx = idx.min(self.text.len());
        let a = self.text[..idx]
            .iter()
            .rposition(|c| *c == '\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let b = self.text[idx..]
            .iter()
            .position(|c| *c == '\n')
            .map(|p| idx + p)
            .unwrap_or(self.text.len());
        self.select_range(a, b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(s: &str) -> Editor {
        let mut e = Editor::default();
        e.set_text(s);
        e
    }

    #[test]
    fn insert_and_backspace_at_cursor() {
        let mut e = ed("helo");
        e.move_to(3, false);
        e.insert_char('l');
        assert_eq!(e.text(), "hello");
        assert_eq!(e.cursor(), 4);
        e.backspace();
        assert_eq!(e.text(), "helo");
        e.move_to(0, false);
        e.backspace();
        assert_eq!(e.text(), "helo");
        e.delete();
        assert_eq!(e.text(), "elo");
    }

    #[test]
    fn typing_replaces_selection() {
        let mut e = ed("hello world");
        e.select_range(0, 5);
        e.insert_str("bye");
        assert_eq!(e.text(), "bye world");
        assert_eq!(e.cursor(), 3);
        assert!(e.selection().is_none());
    }

    #[test]
    fn shift_arrows_extend_and_plain_arrows_collapse() {
        let mut e = ed("abcd");
        e.move_to(1, false);
        e.move_right(true);
        e.move_right(true);
        assert_eq!(e.selected_text().as_deref(), Some("bc"));
        e.move_left(false);
        assert_eq!(e.cursor(), 1);
        assert!(e.selection().is_none());
        e.move_to(3, false);
        e.move_left(true);
        assert_eq!(e.selection(), Some((2, 3)));
        e.move_right(false);
        assert_eq!(e.cursor(), 3);
    }

    #[test]
    fn cut_returns_and_removes() {
        let mut e = ed("one two");
        e.select_range(4, 7);
        assert_eq!(e.cut().as_deref(), Some("two"));
        assert_eq!(e.text(), "one ");
        assert!(e.cut().is_none());
    }

    #[test]
    fn word_motion_and_delete() {
        let mut e = ed("alpha beta gamma");
        e.word_left(false);
        assert_eq!(e.cursor(), 11);
        e.delete_word_left();
        assert_eq!(e.text(), "alpha gamma");
        e.move_to(0, false);
        e.delete_word_right();
        assert_eq!(e.text(), " gamma");
    }

    #[test]
    fn paste_normalizes_cr_and_tabs() {
        let mut e = Editor::default();
        e.insert_str("a\r\nb\tc");
        assert_eq!(e.text(), "a\nb    c");
    }

    #[test]
    fn layout_wraps_and_tracks_newlines() {
        let e = ed("abcd efgh\nxy");
        let rows = e.layout(5);
        let got: Vec<(usize, usize, bool)> =
            rows.iter().map(|r| (r.start, r.end, r.last)).collect();
        assert_eq!(got, vec![(0, 5, false), (5, 9, true), (10, 12, true)]);
    }

    #[test]
    fn layout_trailing_newline_gives_empty_last_row() {
        let e = ed("ab\n");
        let rows = e.layout(10);
        assert_eq!(rows.len(), 2);
        assert_eq!(e.cursor_pos(10), (1, 0));
    }

    #[test]
    fn click_places_cursor() {
        let e = ed("abcd efgh\nxy");
        assert_eq!(e.index_at(5, 0, 2), 2);
        assert_eq!(e.index_at(5, 1, 0), 5);
        // past end of a soft-wrapped row stays on that row, before the hanging space
        assert_eq!(e.index_at(5, 0, 40), 4);
        // past end of the last row of a line lands at the line end
        assert_eq!(e.index_at(5, 1, 40), 9);
        assert_eq!(e.index_at(5, 2, 1), 11);
        // row beyond layout clamps to the last row
        assert_eq!(e.index_at(5, 99, 0), 10);
    }

    #[test]
    fn up_down_keep_goal_column() {
        let mut e = ed("abcdef\nab\nabcdef");
        e.move_to(5, false); // col 5 on row 0
        e.down(80, false); // short row clamps to its end
        assert_eq!(e.cursor(), 9);
        e.down(80, false); // goal column restored
        assert_eq!(e.cursor(), 10 + 5);
        e.up(80, false);
        e.up(80, false);
        assert_eq!(e.cursor(), 5);
        e.up(80, false); // up from the first row goes to the start
        assert_eq!(e.cursor(), 0);
    }

    #[test]
    fn home_end_are_per_visual_row() {
        let mut e = ed("abcd efgh");
        e.move_to(7, false);
        e.home(5, false);
        assert_eq!(e.cursor(), 5);
        e.end(5, false);
        assert_eq!(e.cursor(), 9);
        e.move_to(2, false);
        e.end(5, false);
        assert_eq!(e.cursor(), 4);
    }

    #[test]
    fn double_and_triple_click_selection() {
        let mut e = ed("foo bar\nbaz qux");
        e.select_word_at(5);
        assert_eq!(e.selected_text().as_deref(), Some("bar"));
        e.select_line_at(9);
        assert_eq!(e.selected_text().as_deref(), Some("baz qux"));
        e.select_line_at(2);
        assert_eq!(e.selected_text().as_deref(), Some("foo bar"));
    }

    #[test]
    fn select_all_then_delete() {
        let mut e = ed("gone");
        e.select_all();
        e.backspace();
        assert!(e.is_empty());
    }

    // ---- undo / redo ----

    fn typed(e: &mut Editor, s: &str) {
        for c in s.chars() {
            e.insert_char(c);
        }
    }

    #[test]
    fn a_typing_burst_is_one_undo_step() {
        let mut e = Editor::default();
        typed(&mut e, "hello world");
        assert!(e.undo());
        assert_eq!(e.text(), "");
        assert!(!e.undo());
        assert!(e.redo());
        assert_eq!(e.text(), "hello world");
        assert_eq!(e.cursor(), 11);
        assert!(!e.redo());
    }

    #[test]
    fn moving_the_caret_splits_typing_bursts() {
        let mut e = Editor::default();
        typed(&mut e, "abc");
        e.move_to(1, false);
        typed(&mut e, "XY");
        assert_eq!(e.text(), "aXYbc");
        e.undo();
        assert_eq!(e.text(), "abc");
        assert_eq!(e.cursor(), 1);
        e.undo();
        assert_eq!(e.text(), "");
    }

    #[test]
    fn newline_paste_cut_word_delete_and_clear_are_their_own_steps() {
        let mut e = Editor::default();
        typed(&mut e, "one");
        e.insert_char('\n');
        typed(&mut e, "two");
        e.insert_str("PASTED");
        e.select_range(0, 3);
        e.cut();
        e.move_to(usize::MAX, false);
        e.delete_word_left();
        assert_eq!(e.text(), "\n");
        e.clear();
        assert_eq!(e.text(), "");
        let mut seen = Vec::new();
        while e.undo() {
            seen.push(e.text());
        }
        assert_eq!(
            seen,
            vec![
                "\n".to_string(),
                "\ntwoPASTED".to_string(),
                "one\ntwoPASTED".to_string(),
                "one\ntwo".to_string(),
                "one\n".to_string(),
                "one".to_string(),
                String::new(),
            ]
        );
    }

    #[test]
    fn backspace_bursts_merge_and_do_not_merge_with_typing() {
        let mut e = Editor::default();
        typed(&mut e, "hello");
        e.backspace();
        e.backspace();
        e.backspace();
        assert_eq!(e.text(), "he");
        typed(&mut e, "y");
        e.undo();
        assert_eq!(e.text(), "he");
        e.undo();
        assert_eq!(e.text(), "hello");
        assert_eq!(e.cursor(), 5);
    }

    #[test]
    fn forward_delete_bursts_merge() {
        let mut e = ed("abcdef");
        e.move_to(1, false);
        e.delete();
        e.delete();
        assert_eq!(e.text(), "adef");
        e.undo();
        assert_eq!(e.text(), "abcdef");
    }

    #[test]
    fn a_new_edit_invalidates_redo() {
        let mut e = Editor::default();
        typed(&mut e, "abc");
        e.undo();
        typed(&mut e, "z");
        assert!(!e.redo());
        assert_eq!(e.text(), "z");
    }

    #[test]
    fn undo_stack_is_capped_at_200_steps() {
        let mut e = Editor::default();
        for i in 0..300 {
            e.insert_str(&format!("{}\n", i % 7)); // each paste is its own step
        }
        let mut n = 0;
        while e.undo() {
            n += 1;
        }
        assert_eq!(n, UNDO_CAP);
        // the oldest 100 pastes could not be undone
        assert_eq!(e.text().lines().count(), 100);
    }

    #[test]
    fn unicode_round_trips() {
        let mut e = Editor::default();
        typed(&mut e, "héllo ✓");
        e.insert_str("日本語 👍");
        e.backspace();
        assert_eq!(e.text(), "héllo ✓日本語 ");
        e.undo();
        assert_eq!(e.text(), "héllo ✓日本語 👍");
        e.undo();
        assert_eq!(e.text(), "héllo ✓");
        e.undo();
        assert_eq!(e.text(), "");
        e.redo();
        e.redo();
        e.redo();
        assert_eq!(e.text(), "héllo ✓日本語 ");
    }

    #[test]
    fn undo_restores_selection_and_caret() {
        let mut e = ed("hello world");
        e.select_range(6, 11);
        e.insert_str("there");
        assert_eq!(e.text(), "hello there");
        e.undo();
        assert_eq!(e.text(), "hello world");
        assert_eq!(e.selection(), Some((6, 11)));
        e.redo();
        assert_eq!(e.text(), "hello there");
        assert_eq!(e.cursor(), 11);
        assert!(e.selection().is_none());
    }

    #[test]
    fn undo_after_send_restores_the_sent_text() {
        let mut e = Editor::default();
        typed(&mut e, "ship it");
        e.clear(); // what sending does
        assert!(e.is_empty());
        assert!(e.undo());
        assert_eq!(e.text(), "ship it");
        assert_eq!(e.cursor(), 7);
        // and a second send is still undoable
        e.clear();
        typed(&mut e, "next");
        e.undo();
        assert_eq!(e.text(), "");
        e.undo();
        assert_eq!(e.text(), "ship it");
    }

    #[test]
    fn no_op_edits_record_nothing() {
        let mut e = ed("abc");
        e.move_to(0, false);
        e.backspace();
        e.set_text("abc");
        e.clear_selection();
        let rev = e.revision();
        assert!(e.undo()); // only the initial set_text step exists
        assert_eq!(e.text(), "");
        assert!(e.revision() > rev);
        assert!(!e.undo());
    }
}
