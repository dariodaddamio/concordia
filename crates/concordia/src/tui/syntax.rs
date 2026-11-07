//! Cached, background syntax highlighting; no filesystem reads or terminal escape output.
use super::*;
use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::sync::OnceLock;
use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

pub(super) type Runs = Vec<Vec<(usize, usize, Style)>>;

const MAX_HIGHLIGHT_BYTES: usize = 2 * 1024 * 1024;
const MAX_HIGHLIGHT_LINES: usize = 20_000;
const MAX_CACHE_ENTRIES: usize = 32;
const MAX_CACHE_BYTES: usize = 8 * 1024 * 1024;

struct CacheEntry {
    key: (String, u64),
    runs: Runs,
    bytes: usize,
}

#[derive(Default)]
struct HighlightCache {
    entries: VecDeque<CacheEntry>,
    bytes: usize,
}

static CACHE: OnceLock<Mutex<HighlightCache>> = OnceLock::new();

fn cache() -> &'static Mutex<HighlightCache> {
    CACHE.get_or_init(|| Mutex::new(HighlightCache::default()))
}

fn cache_key(path: &Path, text: &str) -> (String, u64) {
    let mut hash = DefaultHasher::new();
    text.hash(&mut hash);
    (path.to_string_lossy().into_owned(), hash.finish())
}

fn run_bytes(runs: &Runs) -> usize {
    runs.iter()
        .map(|line| line.len() * std::mem::size_of::<(usize, usize, Style)>() + 24)
        .sum()
}

pub(super) fn highlight(path: &Path, text: &str) -> Runs {
    if text.len() > MAX_HIGHLIGHT_BYTES || text.lines().count() > MAX_HIGHLIGHT_LINES {
        return Vec::new();
    }
    let key = cache_key(path, text);
    if let Some(runs) = cache().lock().ok().and_then(|mut cache| {
        cache
            .entries
            .iter()
            .position(|entry| entry.key == key)
            .map(|index| {
                let entry = cache.entries.remove(index).expect("cache index");
                let runs = entry.runs.clone();
                cache.entries.push_front(entry);
                runs
            })
    }) {
        return runs;
    }
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
    let runs: Runs = LinesWithEndings::from(text)
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
        .collect();
    let bytes = text.len().saturating_add(run_bytes(&runs));
    if bytes <= MAX_CACHE_BYTES {
        if let Ok(mut cache) = cache().lock() {
            cache.bytes = cache.bytes.saturating_add(bytes);
            cache.entries.push_front(CacheEntry {
                key,
                runs: runs.clone(),
                bytes,
            });
            while cache.entries.len() > MAX_CACHE_ENTRIES || cache.bytes > MAX_CACHE_BYTES {
                if let Some(entry) = cache.entries.pop_back() {
                    cache.bytes = cache.bytes.saturating_sub(entry.bytes);
                } else {
                    break;
                }
            }
        }
    }
    runs
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_cache_is_bounded() {
        for i in 0..MAX_CACHE_ENTRIES * 3 {
            let text = format!("fn item_{i}() {{ return {i}; }}\n");
            let _ = highlight(Path::new("fixture.rs"), &text);
        }
        let cache = cache().lock().unwrap();
        assert!(cache.entries.len() <= MAX_CACHE_ENTRIES);
        assert!(cache.bytes <= MAX_CACHE_BYTES);
        drop(cache);
        assert!(highlight(
            Path::new("generated.min.js"),
            &"x".repeat(MAX_HIGHLIGHT_BYTES + 1)
        )
        .is_empty());
    }
}
