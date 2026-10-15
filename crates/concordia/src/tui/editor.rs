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

#[derive(Default)]
pub struct Editor {
    text: Vec<char>,
    cursor: usize,
    anchor: Option<usize>,
    /// Display column to return to when moving up/down across rows of different length.
    goal_col: Option<usize>,
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

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.anchor = None;
        self.goal_col = None;
    }

    /// Replace the whole buffer; cursor goes to the end.
    pub fn set_text(&mut self, s: &str) {
        self.text = s.chars().collect();
        self.cursor = self.text.len();
        self.anchor = None;
        self.goal_col = None;
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
        self.anchor = Some(0);
        self.cursor = self.text.len();
        self.goal_col = None;
    }

    pub fn select_range(&mut self, a: usize, b: usize) {
        let n = self.text.len();
        self.anchor = Some(a.min(n));
        self.cursor = b.min(n);
        self.goal_col = None;
    }

    fn begin_move(&mut self, extend: bool) {
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
        self.delete_selection();
        Some(t)
    }

    // ---- editing ---------------------------------------------------------------------

    /// Insert at the cursor, replacing any selection. `\r` is dropped, tabs become 4 spaces.
    pub fn insert_str(&mut self, s: &str) {
        self.delete_selection();
        let clean: Vec<char> = s
            .chars()
            .filter(|c| *c != '\r')
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
        if self.delete_selection() || self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        self.text.remove(self.cursor);
        self.goal_col = None;
    }

    pub fn delete(&mut self) {
        if self.delete_selection() || self.cursor >= self.text.len() {
            return;
        }
        self.text.remove(self.cursor);
        self.goal_col = None;
    }

    pub fn delete_word_left(&mut self) {
        if self.delete_selection() {
            return;
        }
        let to = prev_word(&self.text, self.cursor);
        self.text.drain(to..self.cursor);
        self.cursor = to;
        self.goal_col = None;
    }

    pub fn delete_word_right(&mut self) {
        if self.delete_selection() {
            return;
        }
        let to = next_word(&self.text, self.cursor);
        self.text.drain(self.cursor..to);
        self.goal_col = None;
    }

    // ---- movement --------------------------------------------------------------------

    pub fn move_left(&mut self, extend: bool) {
        // Collapsing a selection with a plain arrow jumps to its edge, like every editor.
        if !extend {
            if let Some((a, _)) = self.selection() {
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
}
