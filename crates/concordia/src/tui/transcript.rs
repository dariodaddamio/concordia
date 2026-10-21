//! Transcript as wrapped visual rows: scrolling, mouse hit-testing, drag selection, copy text.
//!
//! The view owns the rows built on the last draw so mouse events resolve against exactly what
//! is on screen. Selection is stored in row coordinates; it is cleared when the width changes
//! (rows re-wrap) and stays valid while new rows are appended at the end.

use super::markdown;
use super::text::{col_to_index, word_bounds, wrap_ranges_hang};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// Inline style over part of one line of a block: `(line, start, end, style)`, chars within the line.
pub type LineRun = (usize, usize, usize, Style);

/// One logical chunk of transcript (a message, a card line, …) before wrapping.
pub struct Block<'a> {
    pub identity: Option<usize>,
    pub gutter: &'a str,
    pub gutter_style: Style,
    pub style: Style,
    pub text: &'a str,
    /// Render the text as markdown (assistant replies).
    pub markdown: bool,
    /// Extra inline styles per line (plain blocks only), e.g. the coloured `/usage` block.
    pub runs: &'a [LineRun],
}

#[derive(Clone)]
pub struct Row {
    pub identity: Option<usize>,
    pub source_offset: usize,
    pub gutter: String,
    pub gutter_style: Style,
    pub text: Vec<char>,
    pub style: Style,
    /// Soft-wrap continuation of the previous row: no newline between them when copying.
    pub joins_prev: bool,
    /// Inline style runs `(start, end, style)` in chars of `text`, applied over `style`.
    pub runs: Vec<(usize, usize, Style)>,
    pub links: Vec<(usize, usize, crate::file_links::Candidate)>,
}

impl Row {
    fn gutter_width(&self) -> usize {
        self.gutter.chars().count()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
}

/// What a block looked like when its rows were last wrapped; if it still matches, its rows are
/// reused instead of being wrapped again.
#[derive(Clone, PartialEq, Eq)]
struct BlockKey {
    identity: Option<usize>,
    gutter: String,
    gutter_style: Style,
    style: Style,
    text: String,
    markdown: bool,
    runs: Vec<LineRun>,
}

impl BlockKey {
    fn of(b: &Block) -> Self {
        BlockKey {
            identity: b.identity,
            gutter: b.gutter.to_string(),
            gutter_style: b.gutter_style,
            style: b.style,
            text: b.text.to_string(),
            markdown: b.markdown,
            runs: b.runs.to_vec(),
        }
    }

    fn matches(&self, b: &Block) -> bool {
        self.identity == b.identity
            && self.markdown == b.markdown
            && self.style == b.style
            && self.gutter_style == b.gutter_style
            && self.gutter == b.gutter
            && self.text == b.text
            && self.runs == b.runs
    }
}

#[derive(Default)]
pub struct TranscriptView {
    pub rows: Vec<Row>,
    /// Per block: how it was wrapped and where its first row sits in `rows`.
    keys: Vec<(BlockKey, usize)>,
    /// Rows hidden below the viewport (0 = following the newest output).
    pub scroll: usize,
    /// `(anchor, head)`; may be reversed.
    pub sel: Option<(Pos, Pos)>,
    pub dragging: bool,
    width: usize,
    height: usize,
    first: usize,
    row_blocks: Vec<(Option<usize>, usize)>,
}

impl TranscriptView {
    /// Re-wrap the transcript for `width`. A long session has tens of thousands of rows and this
    /// runs on every frame and every mouse event, so only blocks from the first one that changed
    /// are wrapped again (normally just the newest); earlier blocks reuse their cached rows.
    pub fn rebuild(&mut self, blocks: &[Block], width: usize, height: usize) {
        let prev_rows = self.rows.len();
        let anchor = (self.scroll > 0)
            .then(|| self.row_blocks.get(self.first).copied())
            .flatten()
            .filter(|x| x.0.is_some());
        let width_changed = width != self.width;
        if width_changed {
            self.sel = None;
            self.dragging = false;
            self.rows.clear();
            self.row_blocks.clear();
            self.keys.clear();
        }

        // First block that is new or differs from what was cached.
        let keep = self
            .keys
            .iter()
            .zip(blocks)
            .take_while(|((key, _), b)| key.matches(b))
            .count();
        if keep < self.keys.len() {
            let cut = self.keys[keep].1;
            self.rows.truncate(cut);
            self.row_blocks.truncate(cut);
            self.keys.truncate(keep);
        }
        for b in &blocks[keep..] {
            let start = self.rows.len();
            self.keys.push((BlockKey::of(b), self.rows.len()));
            wrap_block(b, width, &mut self.rows);
            for row in &self.rows[start..] {
                self.row_blocks.push((b.identity, row.source_offset));
            }
        }

        if !width_changed && self.scroll > 0 && self.rows.len() > prev_rows {
            // Keep the viewport still while output streams in below it.
            self.scroll += self.rows.len() - prev_rows;
        }
        self.width = width;
        self.height = height;
        self.clamp_scroll();
        if let Some(anchor) = anchor {
            if let Some(row) = self
                .row_blocks
                .iter()
                .position(|b| *b == anchor)
                .or_else(|| {
                    self.row_blocks
                        .iter()
                        .enumerate()
                        .filter(|(_, b)| b.0 == anchor.0)
                        .min_by_key(|(_, b)| b.1.abs_diff(anchor.1))
                        .map(|(i, _)| i)
                })
            {
                self.scroll = self.rows.len().saturating_sub(row + height);
                self.clamp_scroll();
            }
        }
    }

    pub fn identity_at(&self, rel_row: usize) -> Option<usize> {
        let row = self.first.checked_add(rel_row)?;
        self.rows.get(row)?.identity
    }

    fn clamp_scroll(&mut self) {
        let max = self.rows.len().saturating_sub(self.height);
        self.scroll = self.scroll.min(max);
        self.first = (self.rows.len() - self.scroll).saturating_sub(self.height);
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll += n;
        self.clamp_scroll();
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.scroll = self.scroll.saturating_sub(n);
        self.clamp_scroll();
    }

    pub fn page(&self) -> usize {
        self.height.saturating_sub(1).max(1)
    }

    pub fn follow_tail(&mut self) {
        self.scroll = 0;
        self.clamp_scroll();
    }

    pub fn visible(&self) -> std::ops::Range<usize> {
        self.first..(self.first + self.height).min(self.rows.len())
    }

    // ---- hit-testing -----------------------------------------------------------------

    /// Position under viewport cell `(rel_row, rel_col)`. `None` when the cell is below the last
    /// row; rows above/below the viewport clamp to its edge (for drags that leave the pane).
    pub fn pos_at(&self, rel_row: isize, rel_col: usize) -> Option<Pos> {
        if self.rows.is_empty() {
            return None;
        }
        let vis = self.visible();
        let row = if rel_row < 0 {
            vis.start
        } else {
            let r = self.first + rel_row as usize;
            if r >= vis.end {
                return if rel_row as usize >= self.height {
                    Some(self.end_of(vis.end - 1))
                } else {
                    None
                };
            }
            r
        };
        let r = &self.rows[row];
        let col = col_to_index(&r.text, rel_col.saturating_sub(r.gutter_width()));
        Some(Pos { row, col })
    }

    /// End of the last visible row (drag target when the pointer is below the content).
    pub fn last_pos(&self) -> Option<Pos> {
        let vis = self.visible();
        (vis.end > vis.start).then(|| self.end_of(vis.end - 1))
    }

    fn end_of(&self, row: usize) -> Pos {
        Pos {
            row,
            col: self.rows[row].text.len(),
        }
    }

    // ---- selection -------------------------------------------------------------------

    pub fn start_selection(&mut self, p: Pos) {
        self.sel = Some((p, p));
        self.dragging = true;
    }

    pub fn extend_selection(&mut self, p: Pos) {
        if let Some((a, _)) = self.sel {
            self.sel = Some((a, p));
        }
    }

    /// Mouse released: a click without movement selects nothing.
    pub fn finish_selection(&mut self) {
        self.dragging = false;
        if let Some((a, b)) = self.sel {
            if a == b {
                self.sel = None;
            }
        }
    }

    pub fn clear_selection(&mut self) {
        self.sel = None;
        self.dragging = false;
    }

    pub fn select_word(&mut self, p: Pos) {
        let Some(r) = self.rows.get(p.row) else {
            return;
        };
        let (s, e) = word_bounds(&r.text, p.col);
        self.sel = Some((Pos { row: p.row, col: s }, Pos { row: p.row, col: e }));
        self.dragging = false;
    }

    /// Select the whole logical line (all soft-wrapped rows) at `p`.
    pub fn select_line(&mut self, p: Pos) {
        if p.row >= self.rows.len() {
            return;
        }
        let mut a = p.row;
        while a > 0 && self.rows[a].joins_prev {
            a -= 1;
        }
        let mut b = p.row;
        while b + 1 < self.rows.len() && self.rows[b + 1].joins_prev {
            b += 1;
        }
        self.sel = Some((Pos { row: a, col: 0 }, self.end_of(b)));
        self.dragging = false;
    }

    fn ordered(&self) -> Option<(Pos, Pos)> {
        let (a, b) = self.sel?;
        Some(if a <= b { (a, b) } else { (b, a) })
    }

    /// Selected char range within `row`, if any.
    fn row_selection(&self, row: usize) -> Option<(usize, usize)> {
        let (a, b) = self.ordered()?;
        if row < a.row || row > b.row {
            return None;
        }
        let len = self.rows.get(row)?.text.len();
        let from = if row == a.row { a.col.min(len) } else { 0 };
        let to = if row == b.row { b.col.min(len) } else { len };
        (from < to).then_some((from, to))
    }

    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.ordered()?;
        if self.rows.is_empty() {
            return None;
        }
        let mut out = String::new();
        for row in a.row..=b.row.min(self.rows.len().saturating_sub(1)) {
            let r = &self.rows[row];
            if row > a.row && !r.joins_prev {
                out.push('\n');
            }
            let len = r.text.len();
            let from = if row == a.row { a.col.min(len) } else { 0 };
            let to = if row == b.row { b.col.min(len) } else { len };
            if from < to {
                out.extend(&r.text[from..to]);
            }
        }
        (!out.is_empty()).then_some(out)
    }

    // ---- rendering -------------------------------------------------------------------

    pub fn lines(&self) -> Vec<Line<'static>> {
        self.visible()
            .map(|i| {
                let r = &self.rows[i];
                let mut spans = Vec::new();
                if !r.gutter.is_empty() {
                    spans.push(Span::styled(r.gutter.clone(), r.gutter_style));
                }
                spans.extend(row_spans(r, self.row_selection(i)));
                Line::from(spans)
            })
            .collect()
    }

    /// Display width of the widest visible row (tests).
    #[cfg(test)]
    fn max_row_width(&self) -> usize {
        self.visible()
            .map(|i| self.rows[i].gutter_width() + super::text::slice_width(&self.rows[i].text))
            .max()
            .unwrap_or(0)
    }
}

/// The styled segments of one row: its text split at every style-run and selection boundary.
fn row_spans(r: &Row, selection: Option<(usize, usize)>) -> Vec<Span<'static>> {
    let n = r.text.len();
    let mut cuts = vec![0, n];
    for (a, b, _) in &r.runs {
        cuts.extend([(*a).min(n), (*b).min(n)]);
    }
    if let Some((a, b)) = selection {
        cuts.extend([a.min(n), b.min(n)]);
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut out = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a == b {
            continue;
        }
        let mut style = r.style;
        // Later runs win, so nested emphasis (bold containing code) layers correctly.
        for (ra, rb, rs) in &r.runs {
            if *ra <= a && b <= *rb {
                style = style.patch(*rs);
            }
        }
        if selection.is_some_and(|(sa, sb)| sa <= a && b <= sb) {
            style = style.add_modifier(Modifier::REVERSED);
        }
        out.push(Span::styled(r.text[a..b].iter().collect::<String>(), style));
    }
    out
}

/// Wrap one block into display rows. The gutter glyph appears once, on the block's first row;
/// every other row gets blank space of the same width, plus any indent the line asks for.
fn wrap_block(b: &Block, width: usize, rows: &mut Vec<Row>) {
    let gw = b.gutter.chars().count();
    let lines: Vec<markdown::MdLine> = if b.markdown {
        markdown::render(b.text)
    } else {
        b.text
            .lines()
            .enumerate()
            .map(|(i, l)| markdown::MdLine {
                text: l.to_string(),
                runs: b
                    .runs
                    .iter()
                    .filter(|r| r.0 == i)
                    .map(|r| (r.1, r.2, r.3))
                    .collect(),
                indent: 0,
                hang: 0,
                base: Style::default(),
            })
            .collect()
    };
    let mut first_row = true;
    let mut source_offset = 0usize;
    for ml in lines {
        let chars: Vec<char> = ml.text.chars().collect();
        let links: Vec<_> = crate::file_links::candidates(&ml.text)
            .into_iter()
            .map(|candidate| {
                (
                    ml.text[..candidate.range.start].chars().count(),
                    ml.text[..candidate.range.end].chars().count(),
                    candidate,
                )
            })
            .collect();
        let first_w = width.saturating_sub(gw + ml.indent).max(1);
        let rest_w = width.saturating_sub(gw + ml.indent + ml.hang).max(1);
        for (i, (s, e)) in wrap_ranges_hang(&chars, first_w, rest_w)
            .into_iter()
            .enumerate()
        {
            let pad = if i == 0 {
                ml.indent
            } else {
                ml.indent + ml.hang
            };
            let lead = if first_row {
                b.gutter.to_string()
            } else if b.gutter.starts_with('|') {
                // Command/system blocks keep their left bar on wrapped rows.
                format!("|{}", " ".repeat(gw.saturating_sub(1)))
            } else {
                " ".repeat(gw)
            };
            first_row = false;
            let mut runs: Vec<_> = ml
                .runs
                .iter()
                .filter_map(|&(ra, rb, st)| {
                    let (lo, hi) = (ra.max(s), rb.min(e));
                    (lo < hi).then(|| (lo - s, hi - s, st))
                })
                .collect();
            let row_links: Vec<_> = links
                .iter()
                .filter_map(|(a, z, candidate)| {
                    let (lo, hi) = ((*a).max(s), (*z).min(e));
                    (lo < hi).then(|| (lo - s, hi - s, candidate.clone()))
                })
                .collect();
            runs.extend(
                row_links
                    .iter()
                    .map(|(a, z, _)| (*a, *z, Style::default().add_modifier(Modifier::UNDERLINED))),
            );
            rows.push(Row {
                identity: b.identity,
                source_offset: source_offset + s,
                gutter: format!("{lead}{}", " ".repeat(pad)),
                gutter_style: b.gutter_style,
                text: chars[s..e].to_vec(),
                style: b.style.patch(ml.base),
                joins_prev: i > 0,
                runs,
                links: row_links,
            });
        }
        source_offset += ml.text.chars().count() + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blk<'a>(gutter: &'a str, text: &'a str) -> Block<'a> {
        Block {
            identity: None,
            gutter,
            gutter_style: Style::default(),
            style: Style::default(),
            text,
            markdown: false,
            runs: &[],
        }
    }

    fn view(blocks: &[Block], w: usize, h: usize) -> TranscriptView {
        let mut v = TranscriptView::default();
        v.rebuild(blocks, w, h);
        v
    }

    #[test]
    fn rows_wrap_within_width_with_hanging_gutter() {
        let v = view(&[blk("you ", "one two three four five six")], 14, 10);
        assert!(v.rows.len() > 1);
        assert!(v.rows[1].joins_prev);
        assert_eq!(v.rows[1].gutter, "    ");
        assert!(v.max_row_width() <= 15); // hanging space may overhang by one cell
    }

    #[test]
    fn copy_rejoins_soft_wraps_but_keeps_hard_newlines() {
        let v0 = view(&[blk("", "alpha beta gamma\ndelta")], 8, 20);
        let mut v = v0;
        v.sel = Some((
            Pos { row: 0, col: 0 },
            Pos {
                row: v.rows.len() - 1,
                col: 5,
            },
        ));
        assert_eq!(
            v.selected_text().as_deref(),
            Some("alpha beta gamma\ndelta")
        );
    }

    #[test]
    fn partial_selection_across_rows() {
        let mut v = view(&[blk("a ", "hello"), blk("b ", "world")], 40, 10);
        v.sel = Some((Pos { row: 0, col: 3 }, Pos { row: 1, col: 2 }));
        assert_eq!(v.selected_text().as_deref(), Some("lo\nwo"));
        // reversed drag selects the same text
        v.sel = Some((Pos { row: 1, col: 2 }, Pos { row: 0, col: 3 }));
        assert_eq!(v.selected_text().as_deref(), Some("lo\nwo"));
    }

    #[test]
    fn stale_selection_over_empty_transcript_is_safe() {
        let mut v = view(&[blk("", "hello")], 40, 10);
        v.sel = Some((Pos { row: 0, col: 0 }, Pos { row: 0, col: 5 }));
        v.rebuild(&[], 40, 10);
        assert!(v.selected_text().is_none());
        assert!(v.lines().is_empty());
    }

    #[test]
    fn click_without_drag_selects_nothing() {
        let mut v = view(&[blk("", "hello")], 40, 10);
        let p = v.pos_at(0, 2).unwrap();
        v.start_selection(p);
        v.finish_selection();
        assert!(v.sel.is_none());
        assert!(v.selected_text().is_none());
    }

    #[test]
    fn drag_selects_range() {
        let mut v = view(&[blk("> ", "hello world")], 40, 10);
        let a = v.pos_at(0, 2).unwrap(); // gutter is 2 wide: col 2 -> char 0
        let b = v.pos_at(0, 7).unwrap();
        assert_eq!((a.col, b.col), (0, 5));
        v.start_selection(a);
        v.extend_selection(b);
        v.finish_selection();
        assert_eq!(v.selected_text().as_deref(), Some("hello"));
    }

    #[test]
    fn click_in_gutter_maps_to_line_start() {
        let v = view(&[blk("you ", "hello")], 40, 10);
        assert_eq!(v.pos_at(0, 1).unwrap().col, 0);
    }

    #[test]
    fn double_click_word_and_triple_click_line() {
        let mut v = view(&[blk("", "foo bar baz qux quux")], 10, 10);
        assert!(v.rows.len() >= 2);
        let p = v.pos_at(0, 5).unwrap();
        v.select_word(p);
        assert_eq!(v.selected_text().as_deref(), Some("bar"));
        v.select_line(p);
        assert_eq!(v.selected_text().as_deref(), Some("foo bar baz qux quux"));
    }

    #[test]
    fn scrolling_clamps_and_follows_bottom() {
        let text = (0..30)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut v = view(&[blk("", &text)], 40, 10);
        assert_eq!(v.visible(), 20..30);
        v.scroll_up(5);
        assert_eq!(v.visible(), 15..25);
        v.scroll_up(1000);
        assert_eq!(v.visible(), 0..10);
        v.scroll_down(3);
        assert_eq!(v.visible(), 3..13);
        v.follow_tail();
        assert_eq!(v.visible(), 20..30);
    }

    #[test]
    fn viewport_stays_put_when_output_streams_below() {
        let t1 = (0..30)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut v = view(&[blk("", &t1)], 40, 10);
        v.scroll_up(4);
        let before: Vec<char> = v.rows[v.visible().start].text.clone();
        let t2 = format!("{t1}\nnew1\nnew2");
        v.rebuild(&[blk("", &t2)], 40, 10);
        assert_eq!(v.rows[v.visible().start].text, before);
    }

    #[test]
    fn viewport_anchor_uses_stable_block_identity_and_row_offset() {
        let a0 = (0..10)
            .map(|i| format!("a{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let b = (0..30)
            .map(|i| format!("b{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        fn mk<'a>(a: &'a str, b: &'a str) -> [Block<'a>; 2] {
            [
                Block {
                    identity: Some(11),
                    text: a,
                    ..blk("", a)
                },
                Block {
                    identity: Some(22),
                    text: b,
                    ..blk("", b)
                },
            ]
        }
        let mut v = TranscriptView::default();
        v.rebuild(&mk(&a0, &b), 40, 5);
        v.scroll_up(15);
        let prior = v.row_blocks[v.first];
        assert_eq!(prior.0, Some(22));
        let a1 = (0..15)
            .map(|i| format!("expanded{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        v.rebuild(&mk(&a1, &b), 40, 5);
        assert_eq!(v.row_blocks[v.first], prior);
        v.rebuild(&mk(&a1, &b), 12, 5);
        let resized = v.row_blocks[v.first];
        assert_eq!(resized.0, Some(22));
        assert!(
            resized.1.abs_diff(prior.1) <= 12,
            "resize should keep the same source location within one wrapped row"
        );
    }

    #[test]
    fn resize_clears_selection() {
        let mut v = view(&[blk("", "hello world")], 40, 10);
        v.sel = Some((Pos { row: 0, col: 0 }, Pos { row: 0, col: 5 }));
        v.rebuild(&[blk("", "hello world")], 20, 10);
        assert!(v.sel.is_none());
    }

    #[test]
    fn drag_below_or_above_viewport_clamps() {
        let text = (0..30)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let v = view(&[blk("", &text)], 40, 10);
        assert_eq!(v.pos_at(-3, 0).unwrap().row, 20);
        let below = v.pos_at(15, 0).unwrap();
        assert_eq!(below.row, 29);
        assert_eq!(below.col, v.rows[29].text.len());
    }

    #[test]
    fn highlight_marks_selected_cells_only() {
        let mut v = view(&[blk("> ", "hello")], 40, 10);
        v.sel = Some((Pos { row: 0, col: 1 }, Pos { row: 0, col: 3 }));
        let line = &v.lines()[0];
        let sel: Vec<&str> = line
            .spans
            .iter()
            .filter(|s| s.style.add_modifier.contains(Modifier::REVERSED))
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(sel, vec!["el"]);
    }

    fn rows_text(v: &TranscriptView) -> Vec<(String, String, bool)> {
        v.rows
            .iter()
            .map(|r| (r.gutter.clone(), r.text.iter().collect(), r.joins_prev))
            .collect()
    }

    /// The incremental result must always equal wrapping everything from scratch.
    fn assert_matches_fresh(v: &TranscriptView, blocks: &[Block], w: usize, h: usize) {
        let fresh = view(blocks, w, h);
        assert_eq!(rows_text(v), rows_text(&fresh));
    }

    #[test]
    fn incremental_rebuild_equals_a_fresh_wrap_through_edits_appends_and_resizes() {
        let texts: Vec<String> = (0..12)
            .map(|i| {
                format!(
                    "message {i} {}\nsecond line {i}",
                    "lorem ipsum ".repeat(i + 1)
                )
            })
            .collect();
        let mk = |n: usize, tweak: Option<(usize, &str)>| -> Vec<(String, String)> {
            (0..n)
                .map(|i| {
                    let t = match tweak {
                        Some((j, new)) if j == i => new.to_string(),
                        _ => texts[i].clone(),
                    };
                    (
                        if i % 2 == 0 {
                            "you ".to_string()
                        } else {
                            "concordia ".to_string()
                        },
                        t,
                    )
                })
                .collect()
        };
        fn as_blocks(owned: &[(String, String)]) -> Vec<Block<'_>> {
            owned.iter().map(|(g, t)| blk(g, t)).collect()
        }

        let mut v = TranscriptView::default();
        // append one at a time
        for n in 1..=12 {
            let owned = mk(n, None);
            let blocks = as_blocks(&owned);
            v.rebuild(&blocks, 30, 10);
            assert_matches_fresh(&v, &blocks, 30, 10);
        }
        // edit a block in the middle in place (a tool card flipping to done does this)
        let owned = mk(
            12,
            Some((
                5,
                "EDITED and now quite a lot longer than before, wrapping more",
            )),
        );
        let blocks = as_blocks(&owned);
        v.rebuild(&blocks, 30, 10);
        assert_matches_fresh(&v, &blocks, 30, 10);
        // shrink it again, then edit the first block
        let owned = mk(12, Some((0, "x")));
        let blocks = as_blocks(&owned);
        v.rebuild(&blocks, 30, 10);
        assert_matches_fresh(&v, &blocks, 30, 10);
        // resize
        let blocks = as_blocks(&owned);
        v.rebuild(&blocks, 17, 10);
        assert_matches_fresh(&v, &blocks, 17, 10);
        // truncate (a /new clears the transcript)
        let owned = mk(3, None);
        let blocks = as_blocks(&owned);
        v.rebuild(&blocks, 17, 10);
        assert_matches_fresh(&v, &blocks, 17, 10);
        v.rebuild(&[], 17, 10);
        assert!(v.rows.is_empty());
    }

    #[test]
    fn an_unchanged_transcript_is_not_rewrapped() {
        let blocks = [
            blk("you ", "hello there"),
            blk("concordia ", "general kenobi"),
        ];
        let mut v = view(&blocks, 40, 10);
        // Poison the cached rows: if the next rebuild reused them untouched they stay poisoned.
        v.rows[0].text = vec!['#'];
        v.rebuild(&blocks, 40, 10);
        assert_eq!(
            v.rows[0].text,
            vec!['#'],
            "unchanged block was wrapped again"
        );
        // Changing the *last* block re-wraps only it.
        let changed = [blk("you ", "hello there"), blk("concordia ", "changed")];
        v.rebuild(&changed, 40, 10);
        assert_eq!(v.rows[0].text, vec!['#'], "earlier block must be reused");
        assert_eq!(v.rows[1].text.iter().collect::<String>(), "changed");
    }

    #[test]
    fn a_style_change_alone_invalidates_the_cached_block() {
        let mut v = TranscriptView::default();
        let a = Block {
            identity: None,
            gutter: "",
            gutter_style: Style::default(),
            style: Style::default(),
            text: "card",
            markdown: false,
            runs: &[],
        };
        v.rebuild(&[a], 40, 10);
        let red = Style::default().fg(ratatui::style::Color::Red);
        let b = Block {
            identity: None,
            gutter: "",
            gutter_style: Style::default(),
            style: red,
            text: "card",
            markdown: false,
            runs: &[],
        };
        v.rebuild(&[b], 40, 10);
        assert_eq!(v.rows[0].style, red);
    }

    fn md<'a>(gutter: &'a str, text: &'a str) -> Block<'a> {
        Block {
            identity: None,
            markdown: true,
            ..blk(gutter, text)
        }
    }

    #[test]
    fn markdown_shows_the_marker_once_hangs_bullets_and_styles_inline_code() {
        let v = view(
            &[md(
                "\u{25cf} ",
                "intro\n\n- a long bullet item that has to wrap onto another row\n\nuse `cargo test` now",
            )],
            24,
            20,
        );
        let g: Vec<&str> = v.rows.iter().map(|r| r.gutter.as_str()).collect();
        assert_eq!(g[0], "\u{25cf} ", "marker on the first row only");
        assert!(g[1..].iter().all(|x| !x.contains('\u{25cf}')));
        // The wrapped bullet's continuation rows are indented past the bullet.
        let bullet = v
            .rows
            .iter()
            .position(|r| r.text.first() == Some(&'\u{2022}'))
            .unwrap();
        assert!(v.rows[bullet + 1].joins_prev);
        assert!(v.rows[bullet + 1].gutter.len() > v.rows[bullet].gutter.len() - 1);
        assert_eq!(
            v.rows[bullet + 1].gutter.chars().count(),
            2 + 2,
            "marker width + hang"
        );
        // Inline code carries a style run over exactly its characters.
        let code = v
            .rows
            .iter()
            .find(|r| r.text.iter().collect::<String>().contains("cargo test"))
            .unwrap();
        let text: String = code.text.iter().collect();
        let (a, b, _) = code.runs[0];
        assert_eq!(
            &text.chars().skip(a).take(b - a).collect::<String>(),
            "cargo test"
        );
    }

    #[test]
    fn style_runs_that_end_before_a_wrapped_row_do_not_underflow() {
        // A run at the very start, then enough text to wrap several rows past it.
        let v = view(
            &[md(
                "",
                "`start` and then a long tail of ordinary words that keeps going and going",
            )],
            14,
            20,
        );
        assert!(v.rows.len() > 3);
        assert!(!v.rows[0].runs.is_empty());
        assert!(v.rows[2..].iter().all(|r| r.runs.is_empty()));
    }

    #[test]
    fn selecting_styled_text_layers_the_highlight_over_the_style() {
        let mut v = view(&[md("", "see `code` here")], 40, 10);
        v.sel = Some((Pos { row: 0, col: 0 }, Pos { row: 0, col: 8 }));
        let line = &v.lines()[0];
        let reversed: Vec<(String, bool)> = line
            .spans
            .iter()
            .map(|s| {
                (
                    s.content.to_string(),
                    s.style.add_modifier.contains(Modifier::REVERSED),
                )
            })
            .collect();
        // "see " plain-reversed, "code" code-style-reversed, " here" untouched.
        assert!(reversed.iter().any(|(t, r)| t == "see " && *r));
        assert!(reversed.iter().any(|(t, r)| t == "code" && *r));
        assert!(reversed.iter().any(|(t, r)| t.trim() == "here" && !*r));
        // Copy gives the visible text, without markdown markers.
        assert_eq!(v.selected_text().as_deref(), Some("see code"));
    }
}
