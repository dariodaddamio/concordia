//! A small markdown renderer for assistant replies: headings, bullets, numbered lists, quotes,
//! rules, fenced code, and inline `code`, **bold** and *italic*. It produces plain text plus style
//! runs, so selection and copy keep working on exactly what is on screen.
//!
//! Deliberately not a full CommonMark parser: tables, links and nested emphasis stay as typed.

use ratatui::style::{Color, Modifier, Style};

/// One rendered source line (before wrapping).
#[derive(Debug, Clone, PartialEq)]
pub struct MdLine {
    pub text: String,
    /// `(start, end, style)` in chars of `text`, applied over `base`.
    pub runs: Vec<(usize, usize, Style)>,
    /// Columns of left indent for every row of this line.
    pub indent: usize,
    /// Extra indent for wrapped continuation rows (list and quote hanging indents).
    pub hang: usize,
    pub base: Style,
}

fn code_style() -> Style {
    Style::default()
        .fg(Color::Indexed(223))
        .bg(Color::Indexed(236))
}

fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn plain(text: String) -> MdLine {
    MdLine {
        text,
        runs: Vec::new(),
        indent: 0,
        hang: 0,
        base: Style::default(),
    }
}

/// Render `text` into display lines.
pub fn render(text: &str) -> Vec<MdLine> {
    let mut out = Vec::new();
    let mut in_code = false;
    let mut prose_blank = false;
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if !in_code {
                let lang = trimmed.trim_start_matches('`').trim();
                if !lang.is_empty() {
                    out.push(MdLine {
                        base: dim().add_modifier(Modifier::ITALIC),
                        indent: 2,
                        ..plain(lang.to_string())
                    });
                }
            }
            in_code = !in_code;
            prose_blank = false;
            continue;
        }
        if in_code {
            out.push(MdLine {
                base: code_style(),
                indent: 2,
                ..plain(line.replace('\t', "    "))
            });
            continue;
        }
        if line.trim().is_empty() {
            if !prose_blank {
                out.push(render_prose(""));
                prose_blank = true;
            }
            continue;
        }
        prose_blank = false;
        out.push(render_prose(line));
    }
    out
}

fn render_prose(line: &str) -> MdLine {
    let trimmed = line.trim_start();
    let lead = line.len() - trimmed.len();
    let indent = (lead / 2) * 2;

    // Rule.
    let rule: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    if rule.len() >= 3
        && (rule.chars().all(|c| c == '-')
            || rule.chars().all(|c| c == '*')
            || rule.chars().all(|c| c == '_'))
    {
        return MdLine {
            base: dim(),
            ..plain("\u{2500}".repeat(24))
        };
    }

    // Heading.
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
        let (text, runs) = inline(trimmed[hashes..].trim());
        return MdLine {
            text,
            runs,
            indent: 0,
            hang: 0,
            base: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        };
    }

    // Quote.
    if let Some(rest) = trimmed.strip_prefix('>') {
        let (text, runs) = inline(rest.strip_prefix(' ').unwrap_or(rest));
        let mut shifted: Vec<_> = runs
            .into_iter()
            .map(|(a, b, s)| (a + 2, b + 2, s))
            .collect();
        shifted.insert(0, (0, 1, dim()));
        return MdLine {
            text: format!("\u{2502} {text}"),
            runs: shifted,
            indent,
            hang: 2,
            base: Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::ITALIC),
        };
    }

    // Bullet.
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            let (text, runs) = inline(rest);
            let mut shifted: Vec<_> = runs
                .into_iter()
                .map(|(a, b, s)| (a + 2, b + 2, s))
                .collect();
            shifted.insert(0, (0, 1, Style::default().fg(Color::Cyan)));
            return MdLine {
                text: format!("\u{2022} {text}"),
                runs: shifted,
                indent,
                hang: 2,
                base: Style::default(),
            };
        }
    }

    // Numbered list: keep the number as typed.
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if (1..=3).contains(&digits) {
        let after = &trimmed[digits..];
        if let Some(rest) = after
            .strip_prefix(". ")
            .or_else(|| after.strip_prefix(") "))
        {
            let marker_len = digits + 2;
            let (text, runs) = inline(rest);
            let mut shifted: Vec<_> = runs
                .into_iter()
                .map(|(a, b, s)| (a + marker_len, b + marker_len, s))
                .collect();
            shifted.insert(0, (0, digits + 1, Style::default().fg(Color::Cyan)));
            return MdLine {
                text: format!(
                    "{}{rest_marker}{text}",
                    &trimmed[..digits + 1],
                    rest_marker = " "
                ),
                runs: shifted,
                indent,
                hang: marker_len,
                base: Style::default(),
            };
        }
    }

    let (text, runs) = inline(line);
    MdLine {
        text,
        runs,
        indent: 0,
        hang: 0,
        base: Style::default(),
    }
}

/// Inline `code`, **bold** and *italic*. Markers are removed; style runs index the result.
pub fn inline(s: &str) -> (String, Vec<(usize, usize, Style)>) {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut count = 0usize;
    let mut runs = Vec::new();
    let mut i = 0;
    let find = |from: usize, pat: &[char]| -> Option<usize> {
        (from..chars.len().saturating_sub(pat.len().saturating_sub(1)))
            .find(|&j| chars[j..j + pat.len()] == *pat)
    };
    while i < chars.len() {
        let c = chars[i];
        // `code`
        if c == '`' {
            if let Some(end) = find(i + 1, &['`']) {
                if end > i + 1 {
                    let inner: String = chars[i + 1..end].iter().collect();
                    let n = inner.chars().count();
                    runs.push((count, count + n, code_style()));
                    out.push_str(&inner);
                    count += n;
                    i = end + 1;
                    continue;
                }
            }
        }
        // **bold**
        if c == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(end) = find(i + 2, &['*', '*']) {
                if end > i + 2 {
                    let (inner, inner_runs) = inline(&chars[i + 2..end].iter().collect::<String>());
                    let n = inner.chars().count();
                    runs.push((
                        count,
                        count + n,
                        Style::default().add_modifier(Modifier::BOLD),
                    ));
                    runs.extend(
                        inner_runs
                            .into_iter()
                            .map(|(a, b, st)| (a + count, b + count, st)),
                    );
                    out.push_str(&inner);
                    count += n;
                    i = end + 2;
                    continue;
                }
            }
        }
        // *italic* (not a bullet, not "a * b")
        if c == '*'
            && chars
                .get(i + 1)
                .is_some_and(|n| !n.is_whitespace() && *n != '*')
            && (i == 0 || !chars[i - 1].is_alphanumeric())
        {
            if let Some(end) = find(i + 1, &['*']) {
                if end > i + 1
                    && !chars[end - 1].is_whitespace()
                    && chars.get(end + 1) != Some(&'*')
                {
                    let inner: String = chars[i + 1..end].iter().collect();
                    let n = inner.chars().count();
                    runs.push((
                        count,
                        count + n,
                        Style::default().add_modifier(Modifier::ITALIC),
                    ));
                    out.push_str(&inner);
                    count += n;
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(c);
        count += 1;
        i += 1;
    }
    (out, runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(md: &str) -> Vec<String> {
        render(md).into_iter().map(|l| l.text).collect()
    }

    fn styled(l: &MdLine, pat: &str) -> Option<Style> {
        let chars: Vec<char> = l.text.chars().collect();
        let p: Vec<char> = pat.chars().collect();
        (0..chars.len().saturating_sub(p.len().saturating_sub(1)))
            .find(|&i| chars[i..i + p.len()] == p[..])
            .and_then(|i| {
                l.runs
                    .iter()
                    .rev()
                    .find(|(a, b, _)| *a <= i && i + p.len() <= *b)
                    .map(|(_, _, s)| *s)
            })
    }

    #[test]
    fn inline_markers_are_removed_and_styled() {
        let l = &render("use `cargo test` and **really** *do* it")[0];
        assert_eq!(l.text, "use cargo test and really do it");
        assert_eq!(styled(l, "cargo test"), Some(code_style()));
        assert!(styled(l, "really")
            .unwrap()
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(styled(l, "do")
            .unwrap()
            .add_modifier
            .contains(Modifier::ITALIC));
        assert!(styled(l, "use ").is_none());
    }

    #[test]
    fn unmatched_or_spaced_markers_stay_literal() {
        assert_eq!(texts("a * b * c"), ["a * b * c"]);
        assert_eq!(
            texts("2 * 3 = 6 and `unclosed"),
            ["2 * 3 = 6 and `unclosed"]
        );
        assert_eq!(
            texts("snake_case_name and **unclosed"),
            ["snake_case_name and **unclosed"]
        );
        assert_eq!(texts("empty `` ticks"), ["empty `` ticks"]);
    }

    #[test]
    fn bold_can_contain_code() {
        let l = &render("**run `x` now**")[0];
        assert_eq!(l.text, "run x now");
        assert!(styled(l, "run")
            .unwrap()
            .add_modifier
            .contains(Modifier::BOLD));
        assert_eq!(styled(l, "x"), Some(code_style()));
    }

    #[test]
    fn headings_drop_their_hashes_and_get_a_style() {
        let l = &render("## Plan")[0];
        assert_eq!(l.text, "Plan");
        assert!(l.base.add_modifier.contains(Modifier::BOLD));
        assert_eq!(texts("#hashtag no space"), ["#hashtag no space"]);
    }

    #[test]
    fn bullets_and_numbers_hang_their_continuation_lines() {
        let b = &render("- first item")[0];
        assert_eq!(b.text, "\u{2022} first item");
        assert_eq!((b.indent, b.hang), (0, 2));
        let nested = &render("    - nested")[0];
        assert_eq!(nested.indent, 4);
        let n = &render("12. twelfth")[0];
        assert_eq!(n.text, "12. twelfth");
        assert_eq!(n.hang, 4);
        // A plain sentence starting with a number is not a list.
        assert_eq!(texts("2024 was a year"), ["2024 was a year"]);
    }

    #[test]
    fn code_fences_render_their_body_verbatim_and_drop_the_fences() {
        let lines = render("before\n```rust\nlet x = `1`; // **raw**\n\n    indented\n```\nafter");
        let t: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            t,
            [
                "before",
                "rust",
                "let x = `1`; // **raw**",
                "",
                "    indented",
                "after"
            ]
        );
        assert_eq!(lines[2].base, code_style());
        assert_eq!(lines[2].indent, 2);
        assert!(lines[2].runs.is_empty(), "no inline parsing inside code");
        assert_eq!(lines[5].base, Style::default());
    }

    #[test]
    fn an_unclosed_fence_treats_the_rest_as_code() {
        let lines = render("```\nstill code\n- not a bullet");
        assert!(lines.iter().all(|l| l.base == code_style()));
    }

    #[test]
    fn quotes_and_rules() {
        let q = &render("> wise words")[0];
        assert_eq!(q.text, "\u{2502} wise words");
        assert_eq!(q.hang, 2);
        let r = &render("---")[0];
        assert!(r.text.chars().all(|c| c == '\u{2500}'));
        assert_eq!(texts("a - b"), ["a - b"]);
    }

    #[test]
    fn runs_never_exceed_the_text_and_never_overlap_backwards() {
        for md in [
            "**a** `b` *c* **d `e` f** `g`",
            "- **bold** and `code` and *it*",
            "> *quoted* `code`",
            "# H **x** `y`",
            "9. **n** `m`",
        ] {
            for l in render(md) {
                let n = l.text.chars().count();
                for (a, b, _) in &l.runs {
                    assert!(
                        a <= b && *b <= n,
                        "{md:?}: run {a}..{b} in {n} chars of {:?}",
                        l.text
                    );
                }
            }
        }
    }
}
