//! Cached, background syntax highlighting; no filesystem reads or terminal escape output.
use super::*;
use std::sync::OnceLock;
use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

pub(super) type Runs = Vec<Vec<(usize, usize, Style)>>;
pub(super) fn highlight(path: &Path, text: &str) -> Runs {
    static SYNTAX: OnceLock<(SyntaxSet, Theme)> = OnceLock::new();
    let (syntaxes, theme) = SYNTAX.get_or_init(|| {
        (
            SyntaxSet::load_defaults_newlines(),
            ThemeSet::load_defaults().themes["base16-ocean.dark"].clone(),
        )
    });
    let syntax = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(|e| {
            syntaxes.find_syntax_by_extension(e).or_else(|| match e {
                "ts" | "tsx" | "jsx" | "mjs" | "cjs" => syntaxes.find_syntax_by_extension("js"),
                _ => None,
            })
        })
        .or_else(|| {
            text.lines()
                .next()
                .and_then(|l| syntaxes.find_syntax_by_first_line(l))
        })
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, theme);
    LinesWithEndings::from(text)
        .map(|line| {
            let mut column = 0;
            highlighter
                .highlight_line(line, syntaxes)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(color, span)| {
                    let start = column;
                    column += span
                        .chars()
                        .filter(|c| !matches!(c, '\r' | '\n'))
                        .map(crate::file_document::display_width)
                        .sum::<usize>();
                    (start < column).then(|| {
                        (
                            start,
                            column,
                            Style::default().fg(Color::Rgb(
                                color.foreground.r,
                                color.foreground.g,
                                color.foreground.b,
                            )),
                        )
                    })
                })
                .collect()
        })
        .collect()
}

pub(super) fn line(
    raw: &str,
    column: usize,
    width: usize,
    runs: &[(usize, usize, Style)],
    selection: Option<(usize, usize)>,
) -> Line<'static> {
    let visible = crate::file_document::visible_line(raw, column, width);
    let mut x = column;
    let mut token = runs.partition_point(|(_, end, _)| *end <= column);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for c in visible.chars() {
        while runs.get(token).is_some_and(|(_, end, _)| *end <= x) {
            token += 1;
        }
        let mut style = runs
            .get(token)
            .filter(|(a, b, _)| *a <= x && x < *b)
            .map(|(_, _, s)| *s)
            .unwrap_or_default();
        if selection.is_some_and(|(a, b)| a <= x && x < b) {
            style = style.add_modifier(Modifier::REVERSED);
        }
        if let Some(last) = spans.last_mut().filter(|last| last.style == style) {
            last.content.to_mut().push(c);
        } else {
            spans.push(Span::styled(c.to_string(), style));
        }
        x += text::char_width(c);
    }
    Line::from(spans)
}
