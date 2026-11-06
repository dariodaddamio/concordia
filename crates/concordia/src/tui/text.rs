//! Display-width aware wrapping and hit-testing shared by the prompt editor and transcript.

use unicode_width::UnicodeWidthChar;

/// Preserve logical lines from Unix, Windows and CR-only clipboard sources.
pub fn normalize_newlines(input: &str) -> std::borrow::Cow<'_, str> {
    if input.contains('\r') {
        std::borrow::Cow::Owned(input.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        std::borrow::Cow::Borrowed(input)
    }
}

/// Provider output is text, never terminal instructions. Return a source-character
/// boundary map as well so callers can preserve styles after removing escape sequences.
pub fn terminal_text(input: &str) -> (String, Vec<usize>) {
    #[derive(Clone, Copy)]
    enum State {
        Text,
        Escape,
        Csi,
        String,
        StringEscape,
        Intermediate,
    }
    let mut state = State::Text;
    let mut out = String::with_capacity(input.len());
    let mut offsets = vec![0];
    let mut count = 0;
    for c in input.chars() {
        if is_terminal_unsafe(c) {
            offsets.push(count);
            continue;
        }
        match state {
            State::Text => match c {
                '\u{1b}' => state = State::Escape,
                '\u{9b}' => state = State::Csi,
                '\u{90}' | '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}' => state = State::String,
                '\t' => {
                    out.push_str("    ");
                    count += 4;
                }
                '\n' => {
                    out.push(c);
                    count += 1;
                }
                _ if c.is_control() => {}
                _ => {
                    out.push(c);
                    count += 1;
                }
            },
            State::Escape => {
                state = match c {
                    '[' => State::Csi,
                    ']' | 'P' | 'X' | '^' | '_' => State::String,
                    '\u{20}'..='\u{2f}' => State::Intermediate,
                    _ => State::Text,
                }
            }
            State::Csi => {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    state = State::Text;
                }
            }
            State::String => match c {
                '\u{7}' | '\u{9c}' => state = State::Text,
                '\u{1b}' => state = State::StringEscape,
                _ => {}
            },
            State::StringEscape => {
                state = match c {
                    '\\' | '\u{7}' | '\u{9c}' => State::Text,
                    '\u{1b}' => State::StringEscape,
                    _ => State::String,
                }
            }
            State::Intermediate => {
                if ('\u{30}'..='\u{7e}').contains(&c) {
                    state = State::Text;
                }
            }
        }
        offsets.push(count);
    }
    (out, offsets)
}

fn is_terminal_unsafe(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{061c}' | '\u{feff}')
}

/// Terminal columns a char occupies (control chars count as 0, wide CJK/emoji as 2).
pub fn char_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

pub fn slice_width(chars: &[char]) -> usize {
    chars.iter().map(|c| char_width(*c)).sum()
}

/// Soft-wrap one logical line (no `\n`) into `[start, end)` char ranges that each fit `width`
/// columns. Prefers breaking after a space; falls back to a hard break inside a long word.
/// Always returns at least one range, so an empty line still occupies a row.
pub fn wrap_ranges(chars: &[char], width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut cols = 0;
        let mut end = start;
        let mut last_space = None;
        while end < chars.len() {
            let w = char_width(chars[end]);
            if cols + w > width && end > start {
                break;
            }
            cols += w;
            if chars[end] == ' ' {
                last_space = Some(end + 1);
            }
            end += 1;
        }
        if end < chars.len() {
            // Row is full and more text follows. A space right at the margin hangs off this
            // row (invisible); otherwise break after the last space if one exists past the
            // start of the row.
            if chars[end] == ' ' {
                end += 1;
            } else if let Some(sp) = last_space {
                if sp > start {
                    end = sp;
                }
            }
        }
        out.push((start, end));
        start = end;
    }
    if out.is_empty() {
        out.push((0, 0));
    }
    out
}

/// Like `wrap_ranges`, but the first row may be wider than the rest (a list bullet sits in the
/// first row's text while continuation rows are indented past it).
pub fn wrap_ranges_hang(chars: &[char], first: usize, rest: usize) -> Vec<(usize, usize)> {
    let head = wrap_ranges(chars, first);
    let (s0, e0) = head[0];
    let mut out = vec![(s0, e0)];
    if e0 < chars.len() {
        for (a, b) in wrap_ranges(&chars[e0..], rest) {
            out.push((e0 + a, e0 + b));
        }
    }
    out
}

/// Char index (relative to `chars`) of the cell at display column `col`. A column past the end
/// of the text maps to `chars.len()`; a click on the right half of a wide char lands after it.
pub fn col_to_index(chars: &[char], col: usize) -> usize {
    let mut cols = 0;
    for (i, c) in chars.iter().enumerate() {
        let w = char_width(*c);
        if col < cols + w.max(1) {
            // Right half of a wide char counts as "after".
            return if w > 1 && col >= cols + w / 2 {
                i + 1
            } else {
                i
            };
        }
        cols += w;
    }
    chars.len()
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[derive(PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Punct,
}

fn class(c: char) -> Class {
    if is_word_char(c) {
        Class::Word
    } else if c.is_whitespace() {
        Class::Space
    } else {
        Class::Punct
    }
}

/// `[start, end)` of the run of same-class chars around `idx` (double-click selection).
pub fn word_bounds(chars: &[char], idx: usize) -> (usize, usize) {
    if chars.is_empty() {
        return (0, 0);
    }
    let i = idx.min(chars.len() - 1);
    let cls = class(chars[i]);
    let mut s = i;
    while s > 0 && class(chars[s - 1]) == cls {
        s -= 1;
    }
    let mut e = i + 1;
    while e < chars.len() && class(chars[e]) == cls {
        e += 1;
    }
    (s, e)
}

/// Previous word start strictly before `idx` (Ctrl+Left).
pub fn prev_word(chars: &[char], idx: usize) -> usize {
    let mut i = idx.min(chars.len());
    while i > 0 && class(chars[i - 1]) == Class::Space {
        i -= 1;
    }
    if i == 0 {
        return 0;
    }
    let cls = class(chars[i - 1]);
    while i > 0 && class(chars[i - 1]) == cls {
        i -= 1;
    }
    i
}

/// Next word end strictly after `idx` (Ctrl+Right).
pub fn next_word(chars: &[char], idx: usize) -> usize {
    let mut i = idx.min(chars.len());
    while i < chars.len() && class(chars[i]) == Class::Space {
        i += 1;
    }
    if i >= chars.len() {
        return chars.len();
    }
    let cls = class(chars[i]);
    while i < chars.len() && class(chars[i]) == cls {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tui_text_strips_bidi_and_zero_width() {
        let spoofed = "invoice\u{202e}fdp.exe\u{200b}\u{2066}🙂";
        assert_eq!(terminal_text(spoofed).0, "invoicefdp.exe🙂");
    }

    fn c(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn wrap_prefers_spaces() {
        let t = c("hello brave new world");
        let r = wrap_ranges(&t, 11);
        let rows: Vec<String> = r.iter().map(|(a, b)| t[*a..*b].iter().collect()).collect();
        assert_eq!(rows, vec!["hello brave ", "new world"]);
        let r = wrap_ranges(&t, 8);
        let rows: Vec<String> = r.iter().map(|(a, b)| t[*a..*b].iter().collect()).collect();
        assert_eq!(rows, vec!["hello ", "brave ", "new ", "world"]);
    }

    #[test]
    fn wrap_hard_breaks_long_word() {
        let t = c("abcdefghij");
        assert_eq!(wrap_ranges(&t, 4), vec![(0, 4), (4, 8), (8, 10)]);
    }

    #[test]
    fn wrap_empty_is_one_row() {
        assert_eq!(wrap_ranges(&[], 10), vec![(0, 0)]);
    }

    #[test]
    fn wrap_is_lossless() {
        let t = c("the quick  brown fox jumps over the lazy dog");
        for w in 1..30 {
            let joined: String = wrap_ranges(&t, w)
                .iter()
                .flat_map(|(a, b)| t[*a..*b].iter().copied())
                .collect();
            assert_eq!(joined, t.iter().collect::<String>(), "width {w}");
        }
    }

    #[test]
    fn wrap_respects_wide_chars() {
        let t = c("日本語です");
        let rows = wrap_ranges(&t, 4);
        for (a, b) in rows {
            assert!(slice_width(&t[a..b]) <= 4);
        }
    }

    #[test]
    fn col_hit_testing() {
        let t = c("abc");
        assert_eq!(col_to_index(&t, 0), 0);
        assert_eq!(col_to_index(&t, 2), 2);
        assert_eq!(col_to_index(&t, 99), 3);
        let w = c("日a");
        assert_eq!(col_to_index(&w, 0), 0);
        assert_eq!(col_to_index(&w, 1), 1); // right half of wide char = after it
        assert_eq!(col_to_index(&w, 2), 1);
    }

    #[test]
    fn word_navigation() {
        let t = c("foo  bar_baz, qux");
        assert_eq!(word_bounds(&t, 1), (0, 3));
        assert_eq!(word_bounds(&t, 3), (3, 5));
        assert_eq!(word_bounds(&t, 6), (5, 12));
        assert_eq!(word_bounds(&t, 12), (12, 13));
        assert_eq!(next_word(&t, 0), 3);
        assert_eq!(next_word(&t, 3), 12);
        assert_eq!(prev_word(&t, 12), 5);
        assert_eq!(prev_word(&t, 5), 0);
        assert_eq!(prev_word(&t, 0), 0);
        assert_eq!(next_word(&t, 17), 17);
    }

    #[test]
    fn a_hanging_wrap_gives_the_first_row_more_room() {
        let t = c("aaaa bbbb cccc dddd");
        let rows: Vec<String> = wrap_ranges_hang(&t, 10, 5)
            .iter()
            .map(|(a, b)| t[*a..*b].iter().collect())
            .collect();
        assert_eq!(rows, ["aaaa bbbb ", "cccc ", "dddd"]);
        // Lossless and never empty.
        let joined: String = wrap_ranges_hang(&t, 10, 5)
            .iter()
            .flat_map(|(a, b)| t[*a..*b].iter().copied())
            .collect();
        assert_eq!(joined, "aaaa bbbb cccc dddd");
        assert_eq!(wrap_ranges_hang(&[], 5, 3), vec![(0, 0)]);
    }
}
