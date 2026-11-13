//! A small markdown renderer for assistant replies: headings, bullets, numbered lists, quotes,
//! rules, fenced code, and inline `code`, **bold** and *italic*. It produces plain text plus style
//! runs, so selection and copy keep working on exactly what is on screen.
//!
//! Pipe tables adapt to the available width; link labels retain explicit destinations.

use ratatui::style::{Color, Modifier, Style};

#[path = "markdown_tables.rs"]
mod tables;

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
    /// Rendered character ranges and their explicit local-file or HTTP(S) destinations.
    pub links: Vec<(usize, usize, String)>,
    /// Decorative char ranges (quote bars, table borders) that selection and copy skip.
    pub decor: Vec<(usize, usize)>,
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
        links: Vec::new(),
        decor: Vec::new(),
    }
}

/// Render `text` into display lines.
#[cfg(test)]
pub fn render(text: &str) -> Vec<MdLine> {
    render_width(text, 100)
}

#[cfg(test)]
pub fn render_width(text: &str, width: usize) -> Vec<MdLine> {
    render_width_state(text, width).0
}

pub fn render_width_state(text: &str, width: usize) -> (Vec<MdLine>, bool) {
    let mut out = Vec::new();
    let mut pending = false;
    let mut diagrams = 0;
    let mut in_code = false;
    let mut prose_blank = false;
    let mut source = text.lines().peekable();
    while let Some(raw) = source.next() {
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if !in_code {
                let lang = trimmed.trim_start_matches('`').trim();
                if lang.eq_ignore_ascii_case("mermaid") {
                    diagrams += 1;
                    let mut diagram = String::new();
                    let mut closed = false;
                    for row in source.by_ref() {
                        if row.trim_start().starts_with("```") {
                            closed = true;
                            break;
                        }
                        diagram.push_str(row);
                        diagram.push('\n');
                    }
                    let result = if diagrams > 16 {
                        Some(Err(
                            "at most 16 diagrams per message; source retained".into()
                        ))
                    } else if closed {
                        super::diagrams::preview(&diagram, width.saturating_sub(2))
                    } else {
                        None
                    };
                    pending |= closed && result.is_none();
                    match result {
                        Some(Ok(rendered)) => out.extend(rendered.lines().map(|row| MdLine {
                            base: Style::default().fg(Color::Cyan),
                            ..plain(row.to_owned())
                        })),
                        status => {
                            let label = match status {
                                Some(Err(reason)) => format!("Mermaid: {reason}"),
                                _ if closed => "Rendering Mermaid diagram...".into(),
                                _ => "Mermaid diagram arriving...".into(),
                            };
                            out.push(MdLine {
                                base: dim(),
                                ..plain(label)
                            });
                            if closed {
                                out.extend(diagram.lines().map(|row| MdLine {
                                    base: code_style(),
                                    ..plain(row.to_owned())
                                }));
                            }
                        }
                    }
                    prose_blank = false;
                    continue;
                }
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
        if let Some(header) = tables::header(line, source.peek().copied().unwrap_or("")) {
            source.next();
            let mut rows = vec![header];
            while rows.len() < 257 {
                let Some(row) = source.peek().and_then(|s| tables::cells(s)) else {
                    break;
                };
                if row.len() != rows[0].len() {
                    break;
                }
                rows.push(row);
                source.next();
            }
            out.extend(tables::render(rows, width));
            prose_blank = false;
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
    (out, pending)
}

fn render_prose(line: &str) -> MdLine {
    let mut line = render_prose_inner(line);
    format_links(&mut line);
    line
}

/// Compact reported reasoning without cutting through Markdown markers or link destinations.
/// The original text remains available in the expanded transcript and activity review.
pub fn thinking_preview(raw: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let (clean, _) = super::text::terminal_text(raw);
    let text = clean
        .lines()
        .filter(|line| !line.trim_start().starts_with("```"))
        .map(|line| {
            render_prose(line)
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let count = text.chars().count();
    if count <= max_chars {
        return text;
    }
    if max_chars <= 3 {
        return "...".chars().take(max_chars).collect();
    }
    let tail = text
        .chars()
        .skip(count - (max_chars - 3))
        .collect::<String>();
    format!("...{tail}")
}

fn render_prose_inner(line: &str) -> MdLine {
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
            links: Vec::new(),
            decor: Vec::new(),
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
            links: Vec::new(),
            // The quote bar is decoration; the quoted words stay copyable.
            decor: vec![(0, 2)],
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
                links: Vec::new(),
                decor: Vec::new(),
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
                links: Vec::new(),
                decor: Vec::new(),
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
        links: Vec::new(),
        decor: Vec::new(),
    }
}

pub(super) fn web_target(target: &str) -> bool {
    target.len() <= 2048
        && target
            .strip_prefix("https://")
            .or_else(|| target.strip_prefix("http://"))
            .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
        && !target
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
}

fn format_links(line: &mut MdLine) {
    let old: Vec<char> = line.text.chars().collect();
    let mut map: Vec<usize> = (0..=old.len()).collect();
    let mut text = String::new();
    let mut source = 0;
    let mut count = 0;
    for link in crate::file_links::markdown_links(&line.text) {
        let start = line.text[..link.syntax.start].chars().count();
        let end = line.text[..link.syntax.end].chars().count();
        // Inline code remains literal, and unsupported URL schemes remain visible.
        if line
            .runs
            .iter()
            .any(|(a, b, s)| *a <= start && end <= *b && s.bg == code_style().bg)
            || !(web_target(&link.target)
                || crate::file_links::explicit_reference(&link.target).is_some())
        {
            continue;
        }
        let a = line.text[..link.label.start].chars().count();
        let b = line.text[..link.label.end].chars().count();
        for i in source..start {
            map[i] = count;
            text.push(old[i]);
            count += 1;
        }
        for m in &mut map[start..a] {
            *m = count;
        }
        let visible_start = count;
        for i in a..b {
            map[i] = count;
            text.push(old[i]);
            count += 1;
        }
        for m in &mut map[b..=end] {
            *m = count;
        }
        line.links.push((visible_start, count, link.target));
        source = end;
    }
    if line.links.is_empty() {
        return;
    }
    for i in source..old.len() {
        map[i] = count;
        text.push(old[i]);
        count += 1;
    }
    map[old.len()] = count;
    line.runs = line
        .runs
        .iter()
        .filter_map(|(a, b, s)| (map[*a] < map[*b]).then_some((map[*a], map[*b], *s)))
        .collect();
    line.runs.extend(line.links.iter().map(|(a, b, _)| {
        (
            *a,
            *b,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED),
        )
    }));
    line.text = text;
}

/// Inline `code`, **bold** and *italic*. Markers are removed; style runs index the result.
pub fn inline(s: &str) -> (String, Vec<(usize, usize, Style)>) {
    let chars: Vec<char> = s.chars().collect();
    let bytes: Vec<usize> = s
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(s.len()))
        .collect();
    let mut links = crate::file_links::markdown_links(s).into_iter().peekable();
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
        // Parse styles in a label, never in the explicit destination itself.
        while links
            .peek()
            .is_some_and(|link| link.syntax.start < bytes[i])
        {
            links.next();
        }
        if c == '['
            && links
                .peek()
                .is_some_and(|link| link.syntax.start == bytes[i])
        {
            if let Some(link) = links.next() {
                let (label, label_runs) = inline(&s[link.label.clone()]);
                out.push('[');
                count += 1;
                runs.extend(
                    label_runs
                        .into_iter()
                        .map(|(a, b, s)| (a + count, b + count, s)),
                );
                count += label.chars().count();
                out.push_str(&label);
                let tail = &s[link.label.end..link.syntax.end];
                out.push_str(tail);
                count += tail.chars().count();
                i = bytes
                    .binary_search(&link.syntax.end)
                    .expect("Markdown delimiter boundary");
                continue;
            }
        }
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

    #[test]
    fn link_labels_keep_exact_destinations_and_code_is_literal() {
        let rows = render("字 [**plan**](<docs/my plan.md>) and [plan](https://example.test/a_(b)) `code [x](file.md)`");
        assert_eq!(rows[0].text, "字 plan and plan code [x](file.md)");
        assert_eq!(rows[0].links.len(), 2);
        assert_eq!(rows[0].links[0], (2, 6, "docs/my plan.md".into()));
        assert_eq!(
            rows[0].links[1],
            (11, 15, "https://example.test/a_(b)".into())
        );
        assert!(render("[bad](javascript:alert(1))")[0].links.is_empty());
        assert!(!web_target("https://example.test\nattack"));
        let literal_target = render("[`source`](https://example.test/?q=*value*)");
        assert_eq!(literal_target[0].text, "source");
        assert_eq!(
            literal_target[0].links[0].2,
            "https://example.test/?q=*value*"
        );
        let relative = render("Read [E2E status](e2e-status.md).");
        assert_eq!(relative[0].text, "Read E2E status.");
        assert_eq!(relative[0].links, vec![(5, 15, "e2e-status.md".into())]);
        assert_eq!(
            thinking_preview(
                "**Inspecting source**\n\nRead [E2E status](e2e-status.md) and `main.rs`.",
                600
            ),
            "Inspecting source\nRead E2E status and main.rs."
        );
        let preview = thinking_preview(&format!("{}**Reported summary**", "雪".repeat(100)), 24);
        assert!(preview.starts_with("..."));
        assert!(preview.ends_with("Reported summary"));
        assert!(!preview.contains('*') && preview.chars().count() <= 24);
    }

    #[test]
    fn pipe_tables_wrap_and_stack_without_losing_links_or_unicode() {
        let table = "| Feature | Proof |\n| :--- | ---: |\n| Memory 字 | [Read exact originals](docs/native-memory.md) and `a|b` |\n| Files | New preview |";
        for width in [12, 24, 40, 100] {
            let rows = render_width(table, width);
            let combined = rows
                .iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(!combined.contains("| :---"), "{combined}");
            assert!(combined.contains("Memory"), "{combined}");
            if width >= 15 {
                assert!(
                    rows.iter().filter(|row| row.text.starts_with('├')).count() >= 2,
                    "body rows need separators: {combined}"
                );
            }
            assert!(rows
                .iter()
                .flat_map(|r| &r.links)
                .any(|(_, _, t)| t == "docs/native-memory.md"));
            if width >= 15 {
                assert!(
                    rows.iter().all(|r| super::super::text::slice_width(
                        &r.text.chars().collect::<Vec<_>>()
                    ) <= width),
                    "{combined}"
                );
            }
            for row in rows {
                for (a, b, _) in row.links {
                    assert!(a < b && b <= row.text.chars().count());
                }
            }
        }
        let literal = render("```md\n| a | b |\n| --- | --- |\n```\nnot a table | yet");
        assert_eq!(literal[0].text, "md");
        assert_eq!(literal[1].text, "| a | b |");
        assert_eq!(literal.last().unwrap().text, "not a table | yet");
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
