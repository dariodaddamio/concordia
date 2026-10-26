//! Colour for the `/usage` block and the `/usage all` picker rows.
//!
//! The text itself is plain (`usage::window_lines`); this module reads it back and returns style
//! runs, so a block keeps its colour when it is rewritten in place and old blocks stay coloured.
//! Only the left percentage changes colour (green below 80 % used, yellow from 80, red from 95 or
//! when the limit is reached); everything else is dim. `NO_COLOR` strips the colours later and
//! keeps the emphasis (bold left percentage, dim rest).

use super::transcript::LineRun;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

fn red_bold() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}

/// Colour of a left percentage given how much is used.
pub fn tone_color(used: i64, limit_reached: bool) -> Color {
    if limit_reached || used >= 95 {
        Color::Red
    } else if used >= 80 {
        Color::Yellow
    } else {
        Color::Green
    }
}

fn left_style(used: i64, limit_reached: bool) -> Style {
    Style::default()
        .fg(tone_color(used, limit_reached))
        .add_modifier(Modifier::BOLD)
}

fn chars_before(s: &str, byte: usize) -> usize {
    s[..byte].chars().count()
}

/// The ASCII number ending at byte `end`: its start and value.
fn number_before(s: &str, end: usize) -> Option<(usize, i64)> {
    let b = s.as_bytes();
    let mut start = end;
    while start > 0 && b[start - 1].is_ascii_digit() {
        start -= 1;
    }
    s[start..end].parse().ok().map(|n| (start, n))
}

/// The provider colour of a block header (`Claude 2 - limits`), from the account's first word.
fn header_color(line: &str) -> Color {
    let p = match line.split_whitespace().next().unwrap_or("") {
        "Codex" => crate::store::ProviderKind::Codex,
        "Claude" => crate::store::ProviderKind::Claude,
        "Cursor" => crate::store::ProviderKind::Cursor,
        _ => return Color::Indexed(252),
    };
    super::provider_color(p)
}

/// Style runs `(line, start, end, style)` for a `/usage` block; empty for any other text.
pub fn runs(text: &str) -> Vec<LineRun> {
    let mut lines = text.lines();
    let Some(head) = lines.next() else {
        return Vec::new();
    };
    if !head.ends_with(" - limits") {
        return Vec::new();
    }
    let mut out = vec![(
        0,
        0,
        head.chars().count(),
        Style::default()
            .fg(header_color(head))
            .add_modifier(Modifier::BOLD),
    )];
    for (i, line) in lines.enumerate() {
        let i = i + 1;
        let n = line.chars().count();
        if let Some(pos) = line.find("% left") {
            let Some((start, _)) = number_before(line, pos) else {
                continue;
            };
            let end = pos + "% left".len();
            let used = line[end..]
                .find("% used")
                .and_then(|p| number_before(line, end + p))
                .map_or(0, |(_, u)| u);
            let reached = line.contains("limit reached");
            let (a, b) = (chars_before(line, start), chars_before(line, end));
            out.push((i, 0, a, dim()));
            out.push((i, a, b, left_style(used, reached)));
            out.push((i, b, n, dim()));
            if let Some(p) = line.find("limit reached") {
                let x = chars_before(line, p);
                out.push((i, x, x + "limit reached".len(), red_bold()));
            }
        } else if line.starts_with("limit reached") {
            out.push((i, 0, n, red_bold()));
        } else if line.starts_with("checked ") || line.contains("as of your last chat") {
            out.push((i, 0, n, dim().add_modifier(Modifier::ITALIC)));
        }
    }
    out
}

/// Spans for a `/use` or `/usage all` row label (`Codex  5h 59% left (41% used) · ...`). Each
/// left percentage is bold and toned on its own; `None` when the label has none, so the caller
/// keeps its single-style rendering.
pub fn picker_spans(label: &str) -> Option<Vec<Span<'static>>> {
    const LEFT: &str = "% left";
    let mut spans = Vec::new();
    let mut rest = label;
    let mut found = false;
    while let Some(p) = rest.find("% left (") {
        let Some((start, _)) = number_before(rest, p) else {
            break;
        };
        let used_at = p + "% left (".len();
        let Some(close) = rest[used_at..].find(')') else {
            break;
        };
        let close = used_at + close + 1;
        let used = rest[used_at..close]
            .trim_end_matches(')')
            .trim_end_matches("% used")
            .parse()
            .unwrap_or(0);
        found = true;
        if start > 0 {
            spans.push(plain_or_limit(&rest[..start]));
        }
        spans.push(Span::styled(
            rest[start..p + LEFT.len()].to_string(),
            left_style(used, used >= 100),
        ));
        spans.push(Span::styled(rest[p + LEFT.len()..close].to_string(), dim()));
        rest = &rest[close..];
    }
    if !found {
        return None;
    }
    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_string(), dim()));
    }
    Some(spans)
}

fn plain_or_limit(s: &str) -> Span<'static> {
    if s.contains("LIMIT HIT") {
        Span::styled(s.to_string(), red_bold())
    } else {
        Span::raw(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style_at(runs: &[LineRun], line: usize, col: usize) -> Style {
        runs.iter()
            .filter(|r| r.0 == line && r.1 <= col && col < r.2)
            .fold(Style::default(), |s, r| s.patch(r.3))
    }

    const BLOCK: &str = "Codex 2 - limits\n5h   78% left · 22% used · resets in 3h\n7d   12% left · 88% used · resets Oct 21, 9am\n7d Opus    0% left · 100% used · limit reached\nas of your last chat, 3m ago";

    #[test]
    fn left_percentage_tone_follows_used() {
        let r = runs(BLOCK);
        assert_eq!(style_at(&r, 1, 5).fg, Some(Color::Green));
        assert_eq!(style_at(&r, 2, 5).fg, Some(Color::Yellow));
        assert_eq!(style_at(&r, 3, 12).fg, Some(Color::Red));
        assert!(style_at(&r, 1, 5).add_modifier.contains(Modifier::BOLD));
        // label and reset phrase are dim, never coloured
        assert!(style_at(&r, 1, 0).add_modifier.contains(Modifier::DIM));
        assert_eq!(style_at(&r, 1, 0).fg, None);
        assert!(style_at(&r, 1, 35).add_modifier.contains(Modifier::DIM));
        assert!(style_at(&r, 4, 0).add_modifier.contains(Modifier::ITALIC));
        assert_eq!(
            style_at(&r, 0, 0).fg,
            Some(super::super::provider_color(
                crate::store::ProviderKind::Codex
            ))
        );
    }

    #[test]
    fn other_text_gets_no_runs() {
        assert!(runs("usage - no account selected").is_empty());
    }

    #[test]
    fn picker_label_splits_per_window() {
        let s =
            picker_spans("Codex  5h 59% left (41% used) · 7d 5% left (95% used) · resets in 3h")
                .unwrap();
        let reds = s.iter().filter(|x| x.style.fg == Some(Color::Red)).count();
        let greens: Vec<_> = s
            .iter()
            .filter(|x| x.style.fg == Some(Color::Green))
            .collect();
        assert_eq!(reds, 1);
        assert_eq!(greens.len(), 1);
        assert_eq!(greens[0].content, "59% left");
        assert!(picker_spans("Claude  checking limits...").is_none());
    }
}
