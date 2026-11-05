//! Display metadata derived solely from the retained unified diff, never current file bytes.
use ratatui::style::{Color, Modifier, Style};
use std::path::PathBuf;

pub(super) const ADDED: Color = Color::Rgb(25, 48, 38);
pub(super) const REMOVED: Color = Color::Rgb(54, 31, 36);

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Plain,
    Added,
    Removed,
    Context,
}

pub(super) struct Presentation {
    pub style: Style,
    pub runs: Vec<(usize, usize, Style)>,
    kind: Kind,
    old: Option<u32>,
    new: Option<u32>,
}

impl Presentation {
    pub fn gutter(&self, available: usize) -> String {
        if self.kind == Kind::Plain {
            return String::new();
        }
        let line = if self.kind == Kind::Removed {
            self.old
        } else {
            self.new
        }
        .map_or_else(|| "·".into(), |number| number.to_string());
        let digits = line.chars().count().max(4);
        if available >= digits + 7 {
            format!("{line:>digits$} ")
        } else {
            String::new()
        }
    }
}

struct Hunk<'a> {
    path: PathBuf,
    old: Vec<(usize, &'a str)>,
    new: Vec<(usize, &'a str)>,
}

#[derive(Default)]
struct HighlightBudget {
    bytes: usize,
    lines: usize,
    passes: usize,
}

/// Positions come from numeric hunk headers. A bare @@ retains unknown positions as dots.
pub(super) fn present(lines: &[&str]) -> Vec<Presentation> {
    let mut result = lines
        .iter()
        .map(|_| Presentation {
            style: Style::default(),
            runs: Vec::new(),
            kind: Kind::Plain,
            old: None,
            new: None,
        })
        .collect::<Vec<_>>();
    let mut path = PathBuf::from("diff.txt");
    let mut hunk: Option<Hunk<'_>> = None;
    let mut old = None;
    let mut new = None;
    let mut highlighted = HighlightBudget::default();
    for (index, &line) in lines.iter().enumerate() {
        if let Some(file) = line
            .strip_prefix("File: ")
            .or_else(|| line.strip_prefix("*** Update File: "))
            .or_else(|| line.strip_prefix("*** Add File: "))
            .or_else(|| line.strip_prefix("*** Delete File: "))
        {
            finish(hunk.take(), &mut result, &mut highlighted);
            if file.len() <= 4096 {
                path = PathBuf::from(file);
            }
            result[index].style = Style::default()
                .fg(Color::Indexed(252))
                .bg(Color::Indexed(235))
                .add_modifier(Modifier::BOLD);
            old = None;
            new = None;
            hunk = Some(Hunk {
                path: path.clone(),
                old: Vec::new(),
                new: Vec::new(),
            });
        } else if let Some(file) = line
            .strip_prefix("--- ")
            .or_else(|| line.strip_prefix("+++ "))
        {
            finish(hunk.take(), &mut result, &mut highlighted);
            let file = file
                .strip_prefix("b/")
                .or_else(|| file.strip_prefix("a/"))
                .unwrap_or(file)
                .split('\t')
                .next()
                .unwrap_or(file);
            if file.len() <= 4096 && file != "/dev/null" {
                path = PathBuf::from(file);
            }
            result[index].style = Style::default()
                .fg(Color::Indexed(244))
                .add_modifier(Modifier::DIM);
        } else if line.starts_with("@@") {
            finish(hunk.take(), &mut result, &mut highlighted);
            let positions = positions(line);
            old = positions.map(|positions| positions.0);
            new = positions.map(|positions| positions.1);
            hunk = Some(Hunk {
                path: path.clone(),
                old: Vec::new(),
                new: Vec::new(),
            });
            result[index].style = Style::default()
                .fg(Color::Indexed(244))
                .add_modifier(Modifier::DIM);
        } else if hunk.is_some() {
            let kind = match line.as_bytes().first() {
                Some(b'+') if !line.starts_with("+++") => Kind::Added,
                Some(b'-') if !line.starts_with("---") => Kind::Removed,
                Some(b' ') => Kind::Context,
                _ => {
                    if !line.starts_with("\\ No newline") {
                        finish(hunk.take(), &mut result, &mut highlighted);
                    }
                    continue;
                }
            };
            let hunk = hunk.as_mut().unwrap();
            let row = &mut result[index];
            row.kind = kind;
            row.old = (kind != Kind::Added).then_some(old).flatten();
            row.new = (kind != Kind::Removed).then_some(new).flatten();
            row.style = match kind {
                Kind::Added => Style::default().fg(Color::Indexed(252)).bg(ADDED),
                Kind::Removed => Style::default().fg(Color::Indexed(252)).bg(REMOVED),
                _ => Style::default().fg(Color::Indexed(250)),
            };
            if kind != Kind::Added {
                hunk.old.push((index, &line[1..]));
                old = old.and_then(|line| line.checked_add(1));
            }
            if kind != Kind::Removed {
                hunk.new.push((index, &line[1..]));
                new = new.and_then(|line| line.checked_add(1));
            }
        }
    }
    finish(hunk, &mut result, &mut highlighted);
    result
}

fn positions(line: &str) -> Option<(u32, u32)> {
    let mut fields = line.strip_prefix("@@ ")?.split_whitespace();
    let old = fields
        .next()?
        .strip_prefix('-')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    let new = fields
        .next()?
        .strip_prefix('+')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    (fields.next()? == "@@").then_some((old, new))
}

fn finish(hunk: Option<Hunk<'_>>, rows: &mut [Presentation], used: &mut HighlightBudget) {
    let Some(hunk) = hunk else { return };
    for source in [&hunk.old, &hunk.new] {
        let bytes = source.iter().map(|(_, text)| text.len() + 1).sum::<usize>();
        if used.lines.saturating_add(source.len()) > 4000
            || used.bytes.saturating_add(bytes) > 128 * 1024
            || used.passes >= 128
        {
            continue;
        }
        used.bytes += bytes;
        used.lines += source.len();
        used.passes += 1;
        let text = source
            .iter()
            .map(|(_, text)| *text)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let highlighted = super::super::syntax::highlight(&hunk.path, &text);
        for ((index, text), runs) in source.iter().zip(highlighted) {
            let chars = text.chars().collect::<Vec<_>>();
            let row = &mut rows[*index];
            row.runs = runs
                .into_iter()
                .filter_map(|(start, end, style)| {
                    let start = super::super::text::col_to_index(&chars, start);
                    let end = super::super::text::col_to_index(&chars, end);
                    (start < end).then_some((start + 1, end + 1, style))
                })
                .collect();
            row.runs.push((
                0,
                1,
                Style::default().fg(match row.kind {
                    Kind::Added => Color::Green,
                    Kind::Removed => Color::Red,
                    _ => Color::DarkGray,
                }),
            ));
        }
    }
}
