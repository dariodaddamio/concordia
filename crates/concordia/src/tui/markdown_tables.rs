//! Bounded pipe-table layout shared by chat and formatted file panes.
use super::*;
use crate::tui::text::{slice_width, wrap_ranges};

pub(super) fn cells(line: &str) -> Option<Vec<String>> {
    let line = line.trim();
    if !line.contains('|') || line.len() > 64 * 1024 {
        return None;
    }
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    let mut code = false;
    for c in line.chars() {
        if escaped {
            if c != '|' {
                cell.push('\\');
            }
            cell.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '`' => {
                code = !code;
                cell.push(c);
            }
            '|' if !code => {
                cells.push(cell.trim().to_owned());
                cell.clear();
            }
            _ => cell.push(c),
        }
    }
    if escaped {
        cell.push('\\');
    }
    cells.push(cell.trim().to_owned());
    if line.starts_with('|') {
        cells.remove(0);
    }
    if line.ends_with('|') && !line.ends_with("\\|") {
        cells.pop();
    }
    (cells.len() >= 2 && cells.len() <= 16).then_some(cells)
}

pub(super) fn header(line: &str, separator: &str) -> Option<Vec<String>> {
    let header = cells(line)?;
    let sep = cells(separator)?;
    (sep.len() == header.len()
        && sep.iter().all(|cell| {
            let dashes = cell.trim_matches(':');
            dashes.len() >= 3 && dashes.chars().all(|c| c == '-')
        }))
    .then_some(header)
}

fn append(to: &mut MdLine, from: &MdLine, chars: &[char], start: usize, end: usize) {
    let offset = to.text.chars().count();
    to.text.extend(&chars[start..end]);
    to.runs.extend(from.runs.iter().filter_map(|(a, b, s)| {
        let (a, b) = ((*a).max(start), (*b).min(end));
        (a < b).then(|| (a - start + offset, b - start + offset, *s))
    }));
    to.links
        .extend(from.links.iter().filter_map(|(a, b, target)| {
            let (a, b) = ((*a).max(start), (*b).min(end));
            (a < b).then(|| (a - start + offset, b - start + offset, target.clone()))
        }));
}

fn rule(widths: &[usize], left: char, join: char, right: char) -> MdLine {
    let mut line = plain(format!(
        "{left}{}{right}",
        widths
            .iter()
            .map(|w| "─".repeat(w + 2))
            .collect::<Vec<_>>()
            .join(&join.to_string())
    ));
    line.decor.push((0, line.text.chars().count()));
    line
}

pub(super) fn render(rows: Vec<Vec<String>>, width: usize) -> Vec<MdLine> {
    let cells: Vec<Vec<MdLine>> = rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|s| {
                    // Cell markup is inline, not a heading/list nested in the table.
                    let (text, runs) = inline(&s);
                    let mut cell = MdLine {
                        text,
                        runs,
                        ..plain(String::new())
                    };
                    format_links(&mut cell);
                    cell
                })
                .collect()
        })
        .collect();
    let columns = cells[0].len();
    let characters: Vec<Vec<Vec<char>>> = cells
        .iter()
        .map(|row| row.iter().map(|cell| cell.text.chars().collect()).collect())
        .collect();
    let mut out = Vec::new();
    // Very narrow panes use labelled rows so no column is silently clipped.
    if width < columns * 7 + 1 {
        'narrow: for (r, row) in cells.iter().enumerate().skip(1) {
            for (i, cell) in row.iter().enumerate() {
                let mut line = plain(String::new());
                let header = &cells[0][i];
                append(
                    &mut line,
                    header,
                    &characters[0][i],
                    0,
                    characters[0][i].len(),
                );
                line.text.push_str(": ");
                let n = line.text.chars().count();
                line.runs
                    .push((0, n, Style::default().add_modifier(Modifier::BOLD)));
                append(
                    &mut line,
                    cell,
                    &characters[r][i],
                    0,
                    characters[r][i].len(),
                );
                out.push(line);
                if out.len() >= 4096 {
                    out.push(plain(
                        "Table preview is truncated; original source retained.".into(),
                    ));
                    break 'narrow;
                }
            }
            out.push(plain(String::new()));
        }
        if out.is_empty() {
            out.push(plain(
                cells[0]
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" · "),
            ));
        }
        return out;
    }
    let room = width.saturating_sub(columns * 3 + 1);
    let mut widths: Vec<usize> = (0..columns)
        .map(|i| {
            cells
                .iter()
                .enumerate()
                .map(|(r, _)| slice_width(&characters[r][i]))
                .max()
                .unwrap_or(1)
                .max(3)
                .min(room)
        })
        .collect();
    while widths.iter().sum::<usize>() > room {
        let (i, _) = widths.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
        widths[i] -= 1;
    }
    out.push(rule(&widths, '┌', '┬', '┐'));
    'rows: for (r, row) in cells.iter().enumerate() {
        let ranges: Vec<_> = row
            .iter()
            .enumerate()
            .zip(&widths)
            .map(|((i, _), w)| wrap_ranges(&characters[r][i], *w))
            .collect();
        for part in 0..ranges.iter().map(Vec::len).max().unwrap_or(1) {
            let mut line = plain("│ ".into());
            line.decor.push((0, 1));
            for (i, cell) in row.iter().enumerate() {
                let (a, mut b) = ranges[i].get(part).copied().unwrap_or((0, 0));
                let chars = &characters[r][i];
                // Transcript wrapping lets an invisible space hang over the margin;
                // a table cell must leave room for the next visible separator.
                while b > a && chars[b - 1].is_whitespace() {
                    b -= 1;
                }
                let start = line.text.chars().count();
                append(&mut line, cell, chars, a, b);
                if r == 0 {
                    line.runs.push((
                        start,
                        line.text.chars().count(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ));
                }
                let used = slice_width(&chars[a..b]);
                line.text
                    .push_str(&" ".repeat(widths[i].saturating_sub(used)));
                let bar = line.text.chars().count() + 1;
                line.decor.push((bar, bar + 1));
                line.text
                    .push_str(if i + 1 == columns { " │" } else { " │ " });
            }
            out.push(line);
            if out.len() >= 4096 {
                out.push(plain(
                    "Table preview is truncated; original source retained.".into(),
                ));
                break 'rows;
            }
        }
        // Keep wrapped body rows distinct. Without a rule here, a tall cell in one
        // row runs directly into the next row and the table loses its row structure.
        out.push(rule(&widths, '├', '┼', '┤'));
    }
    out.push(rule(&widths, '└', '┴', '┘'));
    out
}
