//! Bounded file references in user and assistant text. Ranges are UTF-8 byte offsets into the
//! original text; the TUI must translate its clicked character to a byte offset before matching.

use std::ops::Range;
use std::path::{Path, PathBuf};

const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_PATH_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Text displayed to the user (a Markdown label, or the literal path).
    pub display: String,
    /// Clickable range in the original text. Markdown uses the label range.
    pub range: Range<usize>,
    /// Local path text, without Markdown delimiters or a line/column fragment.
    pub target: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub markdown: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFile {
    pub path: PathBuf,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// Markdown syntax and label ranges in bytes, shared by discovery and display.
pub(crate) struct MarkdownLink {
    pub syntax: Range<usize>,
    pub label: Range<usize>,
    pub target: String,
}

pub(crate) fn markdown_links(text: &str) -> Vec<MarkdownLink> {
    let text = &text[..char_boundary_at_or_before(text, MAX_TEXT_BYTES)];
    let mut links = Vec::new();
    let mut cursor = 0;
    while let Some(open) = text[cursor..].find('[').map(|n| n + cursor) {
        let Some(mid) = text[open + 1..].find("](").map(|n| n + open + 1) else {
            break;
        };
        let Some(close) = markdown_destination_end(text, mid + 2) else {
            cursor = mid + 2;
            continue;
        };
        if mid > open + 1 && (open == 0 || !text[..open].ends_with(['\\', '!'])) {
            links.push(MarkdownLink {
                syntax: open..close + 1,
                label: open + 1..mid,
                target: decode_markdown_target(text[mid + 2..close].trim()),
            });
        }
        cursor = close + 1;
    }
    links
}

/// Find path-shaped references without touching the filesystem or interpreting them as commands.
/// Markdown destinations support `[label](target)`, including `<targets with spaces>`.
pub fn candidates(text: &str) -> Vec<Candidate> {
    let bounded = &text[..char_boundary_at_or_before(text, MAX_TEXT_BYTES)];
    let mut out = Vec::new();
    let mut masked = vec![false; bounded.len()];

    // Markdown links have an unambiguous destination, so record them first and suppress their
    // syntax from the plain-path pass. Keep only the visible label clickable.
    for link in markdown_links(bounded) {
        let label = &bounded[link.label.clone()];
        if let Some((path, line, column)) = split_reference(&link.target) {
            if is_local_path(&path) && valid_position(line, column) && !label.is_empty() {
                out.push(Candidate {
                    display: label.to_owned(),
                    range: link.label,
                    target: path,
                    line,
                    column,
                    markdown: true,
                });
            }
        }
        masked[link.syntax].fill(true);
    }

    // Native tool arguments quote JSON paths, including escaped Windows separators and spaces.
    // Decode the complete string instead of attempting to open its JSON spelling on disk.
    let mut quoted = 0;
    while let Some(start) = bounded[quoted..].find('"').map(|i| quoted + i) {
        let mut values =
            serde_json::Deserializer::from_str(&bounded[start..]).into_iter::<String>();
        if let Some(Ok(decoded)) = values.next() {
            let end = start + values.byte_offset();
            if !masked[start..end].iter().any(|m| *m) && plausible_path(&decoded) {
                if let Some(mut candidate) = explicit_reference(&decoded) {
                    candidate.range = start + 1..end - 1;
                    candidate.display = bounded[start + 1..end - 1].to_owned();
                    out.push(candidate);
                    masked[start..end].fill(true);
                }
            }
            quoted = end;
        } else {
            quoted = start + 1;
        }
    }

    // Inline code is a strong signal that whitespace belongs to the path. Produce every bounded
    // whitespace-delimited suffix endpoint so paths with spaces can be resolved exactly on click.
    let mut tick_cursor = 0;
    while let Some(start) = bounded[tick_cursor..].find('`').map(|n| n + tick_cursor) {
        let Some(end_rel) = bounded[start + 1..].find('`') else {
            break;
        };
        let end = start + 1 + end_rel;
        if !masked[start..=end].iter().any(|m| *m) {
            let raw = bounded[start + 1..end].trim();
            let leading =
                bounded[start + 1..end].len() - bounded[start + 1..end].trim_start().len();
            add_plain_variants(&mut out, raw, start + 1 + leading, true);
            masked[start..=end].fill(true);
        }
        tick_cursor = end + 1;
    }

    // Plain paths are recognized by a drive/root/relative prefix or a filename extension.
    // Candidate endpoints are bounded to a line and 1 KiB; resolution later requires an existing
    // file, which disambiguates prose from references containing spaces.
    let mut line_start = 0;
    for line in bounded.split_inclusive('\n') {
        let line_end = line_start + line.len();
        let content_end = if line.ends_with('\n') {
            line_end - 1
        } else {
            line_end
        };
        let mut i = line_start;
        while i < content_end {
            if masked[i] || !is_start_boundary(bounded, i) {
                i += bounded[i..].chars().next().map_or(1, char::len_utf8);
                continue;
            }
            let Some(ch) = bounded[i..].chars().next() else {
                break;
            };
            let looks_rooted = ch.is_ascii_alphabetic()
                && bounded[i + 1..content_end].starts_with(":\\")
                || ch == '/' && !bounded[i..content_end].starts_with("//")
                || bounded[i..content_end].starts_with("./")
                || bounded[i..content_end].starts_with(".\\")
                || bounded[i..content_end].starts_with("../")
                || bounded[i..content_end].starts_with("..\\");
            let word_end = scan_word_end(bounded, i, content_end);
            let word = &bounded[i..word_end];
            let looks_file = has_extension(word) || word.contains(['/', '\\']);
            let spaced_file = bounded[i..content_end]
                .split_whitespace()
                .skip(1)
                .take(3)
                .any(has_extension);
            if looks_rooted || looks_file || spaced_file {
                let mut end = word_end;
                while end <= content_end && end - i <= MAX_PATH_BYTES {
                    let raw = bounded[i..end].trim_end_matches(|c: char| ".,;!?)]}".contains(c));
                    let raw_end = i + raw.len();
                    if plausible_path(raw) && !masked[i..raw_end].iter().any(|m| *m) {
                        add_reference_candidate(&mut out, raw, i..raw_end, false);
                    }
                    if end == content_end {
                        break;
                    }
                    let Some(next) = bounded[end..content_end].chars().next() else {
                        break;
                    };
                    if !next.is_whitespace() {
                        break;
                    }
                    end += next.len_utf8();
                    let next_word = scan_word_end(bounded, end, content_end);
                    if next_word == end {
                        break;
                    }
                    end = next_word;
                }
            }
            i = if word_end > i {
                word_end
            } else {
                i + ch.len_utf8()
            };
        }
        line_start = line_end;
    }

    out.sort_by_key(|c| (c.range.start, c.range.end));
    out.dedup_by(|a, b| a.range == b.range && a.target == b.target);
    out
}

/// Explicit command operands can be extensionless and contain whitespace without guessing from prose.
pub fn explicit_reference(text: &str) -> Option<Candidate> {
    let text = text.trim();
    let (target, line, column) = split_reference(text)?;
    if target.len() > MAX_PATH_BYTES
        || !is_local_path(&target)
        || line == Some(0)
        || column == Some(0)
    {
        return None;
    }
    Some(Candidate {
        display: text.into(),
        range: 0..text.len(),
        target,
        line,
        column,
        markdown: false,
    })
}

/// Resolve one candidate for explicit navigation or bounded active-chat reference discovery.
/// Explicit Markdown basenames may use the workspace's direct `docs` directory when absent
/// at the root. No recursive search, subdirectory rewriting or file-content read occurs.
pub fn resolve(candidate: &Candidate, cwd: &Path) -> Result<ResolvedFile, String> {
    if !is_local_path(&candidate.target)
        || candidate.target.len() > MAX_PATH_BYTES
        || !valid_position(candidate.line, candidate.column)
    {
        return Err("not a supported local file reference".into());
    }
    // Chat renderers commonly spell an absolute Windows Markdown destination `/C:/...`.
    // Strip only that extra slash; preserve drive-relative, UNC and ordinary rooted paths.
    let target = candidate.target.as_str();
    #[cfg(windows)]
    let target = normalized_local_path(target);
    let raw = PathBuf::from(target);
    let path = if raw.is_absolute() {
        raw
    } else {
        cwd.join(raw)
    };
    let path = match std::fs::canonicalize(&path) {
        Ok(path) => path,
        Err(error) => {
            let basename = error.kind() == std::io::ErrorKind::NotFound
                && candidate.markdown
                && !target.contains(['/', '\\', ':'])
                && !matches!(target, "." | "..");
            let fallback = basename
                .then(|| cwd.join("docs").join(target))
                .and_then(|path| std::fs::canonicalize(path).ok())
                .filter(|path| std::fs::canonicalize(cwd).is_ok_and(|root| path.starts_with(root)));
            fallback.ok_or_else(|| format!("file is unavailable: {error}"))?
        }
    };
    if !path.is_file() {
        return Err("reference does not name a file".into());
    }
    Ok(ResolvedFile {
        path,
        line: candidate.line,
        column: candidate.column,
    })
}

/// Whether `candidate` names an existing file, remembered briefly so redraws do not stat the
/// same prose again. Rendering uses this to underline only references that open.
pub fn exists_cached(candidate: &Candidate, cwd: &Path) -> bool {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    type Cache = HashMap<(PathBuf, String, bool), (bool, Instant)>;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    const FRESH: Duration = Duration::from_secs(10);
    let key = (
        cwd.to_path_buf(),
        candidate.target.clone(),
        candidate.markdown,
    );
    let cache = CACHE.get_or_init(Default::default);
    if let Some((exists, at)) = cache.lock().ok().and_then(|c| c.get(&key).copied()) {
        if at.elapsed() < FRESH {
            return exists;
        }
    }
    let probe = Candidate {
        line: None,
        column: None,
        ..candidate.clone()
    };
    let exists = valid_position(candidate.line, candidate.column) && resolve(&probe, cwd).is_ok();
    if let Ok(mut c) = cache.lock() {
        if c.len() > 4096 {
            c.clear();
        }
        c.insert(key, (exists, Instant::now()));
    }
    exists
}

pub(crate) fn normalized_local_path(target: &str) -> &str {
    // Canonical Windows drive paths carry this prefix. Do not unwrap UNC/device namespaces.
    let target = target
        .strip_prefix(r"\\?\")
        .or_else(|| target.strip_prefix("//?/"))
        .filter(|path| {
            path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                && path.as_bytes().get(1) == Some(&b':')
                && path
                    .as_bytes()
                    .get(2)
                    .is_some_and(|byte| matches!(byte, b'/' | b'\\'))
        })
        .unwrap_or(target);
    if target.starts_with('/')
        && target
            .as_bytes()
            .get(1)
            .is_some_and(u8::is_ascii_alphabetic)
        && target.as_bytes().get(2) == Some(&b':')
        && target
            .as_bytes()
            .get(3)
            .is_some_and(|b| matches!(*b, b'/' | b'\\'))
    {
        &target[1..]
    } else {
        target
    }
}

/// Return the candidate covering an explicitly clicked byte offset, if any.
pub fn candidate_at(text: &str, byte: usize) -> Option<Candidate> {
    candidates(text)
        .into_iter()
        .filter(|c| c.range.contains(&byte))
        .min_by_key(|c| c.range.len())
}

fn markdown_destination_end(s: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    let mut in_angle = false;
    for (offset, ch) in s[start..].char_indices() {
        let i = start + offset;
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '<' && i == start {
            in_angle = true;
        } else if ch == '>' && in_angle {
            in_angle = false;
        } else if !in_angle && ch == '(' {
            depth += 1;
        } else if !in_angle && ch == ')' {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
    }
    None
}

fn decode_markdown_target(raw: &str) -> String {
    let raw = raw.trim();
    let raw = if raw.starts_with('<') && raw.ends_with('>') && raw.len() >= 2 {
        &raw[1..raw.len() - 1]
    } else {
        raw
    };
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.peek().copied() {
                if next.is_whitespace() || matches!(next, '(' | ')' | '<' | '>') {
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                    continue;
                }
            }
        }
        out.push(ch);
    }
    out
}

fn add_plain_variants(out: &mut Vec<Candidate>, raw: &str, start: usize, inline: bool) {
    // Variants are useful for `C:\dir\file with spaces.rs` while allowing the resolver to
    // reject shorter prefixes. End at each whitespace boundary and the complete code span.
    let mut ends = raw
        .char_indices()
        .filter_map(|(i, c)| c.is_whitespace().then_some(i))
        .collect::<Vec<_>>();
    ends.push(raw.len());
    for end in ends.into_iter().rev() {
        let part = raw[..end].trim_end();
        if plausible_path(part) {
            add_reference_candidate(out, part, start..start + part.len(), inline);
        }
    }
}

fn add_reference_candidate(
    out: &mut Vec<Candidate>,
    raw: &str,
    range: Range<usize>,
    markdown: bool,
) {
    let target = raw.strip_prefix('@').unwrap_or(raw);
    if let Some((path, line, column)) = split_reference(target) {
        if is_local_path(&path) {
            out.push(Candidate {
                display: raw.to_owned(),
                range,
                target: path,
                line,
                column,
                markdown,
            });
        }
    }
}

fn split_reference(raw: &str) -> Option<(String, Option<u32>, Option<u32>)> {
    let raw = raw.trim();
    if let Some((path, suffix)) = raw.rsplit_once('#') {
        let suffix = suffix
            .strip_prefix('L')
            .or_else(|| suffix.strip_prefix('l'))?;
        let line = suffix.parse().ok()?;
        if !path.is_empty() {
            return Some((path.to_owned(), Some(line), None));
        }
    }
    if let Some((path, tail)) = raw.rsplit_once(':') {
        if let Ok(column) = tail.parse::<u32>() {
            if let Some((path, line)) = path.rsplit_once(':') {
                if let Ok(line) = line.parse::<u32>() {
                    if !path.is_empty() {
                        return Some((path.to_owned(), Some(line), Some(column)));
                    }
                }
            }
            if !path.is_empty() {
                return Some((path.to_owned(), Some(column), None));
            }
        }
    }
    (!raw.is_empty()).then(|| (raw.to_owned(), None, None))
}

fn valid_position(line: Option<u32>, column: Option<u32>) -> bool {
    line != Some(0) && column != Some(0)
}

fn is_local_path(path: &str) -> bool {
    let p = normalized_local_path(path.trim());
    !p.is_empty()
        && !p.starts_with("//")
        && !p.starts_with("\\\\")
        && !p.contains("\0")
        && !p.contains("://")
        && !p
            .split(['/', '\\'])
            .any(|part| part.eq_ignore_ascii_case("http:") || part.eq_ignore_ascii_case("https:"))
        && !(p.find(':').is_some_and(|i| i > 1))
}

fn plausible_path(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty()
        || s.len() > MAX_PATH_BYTES
        || !(has_extension(s) || s.contains(['/', '\\']) || s.starts_with('.'))
    {
        return false;
    }
    split_reference(s).is_some_and(|(path, line, column)| {
        is_local_path(&path) && line != Some(0) && column != Some(0)
    })
}

fn has_extension(s: &str) -> bool {
    let s = s.trim_end_matches(|c: char| ".,;!?)]}".contains(c));
    let path = split_reference(s).map(|p| p.0).unwrap_or_default();
    let leaf = path.rsplit(['/', '\\']).next().unwrap_or(&path);
    // Bare decimal measurements are prose (12.3k tokens, 8.2s), not filenames.
    // Explicit paths such as ./8.2s remain addressable.
    if !path.contains(['/', '\\']) {
        let bare = leaf.trim_start_matches(['$', '\u{20ac}', '\u{a3}']);
        if let Some((whole, fraction)) = bare.split_once('.') {
            let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
            let unit = &fraction[digits..];
            if !whole.is_empty()
                && whole.bytes().all(|c| c.is_ascii_digit())
                && digits > 0
                && matches!(
                    unit,
                    "" | "k"
                        | "K"
                        | "M"
                        | "s"
                        | "ms"
                        | "us"
                        | "ns"
                        | "m"
                        | "h"
                        | "KB"
                        | "MB"
                        | "GB"
                        | "%"
                )
            {
                return false;
            }
        }
    }
    leaf.rfind('.').is_some_and(|i| {
        i > 0 && i + 1 < leaf.len() && leaf[i + 1..].chars().all(|c| c.is_ascii_alphanumeric())
    })
}

fn is_start_boundary(s: &str, i: usize) -> bool {
    i == 0
        || s[..i]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_whitespace() || "([<{\"'`=:,".contains(c))
}

fn scan_word_end(s: &str, start: usize, end: usize) -> usize {
    let mut i = start;
    while i < end {
        let Some(c) = s[i..end].chars().next() else {
            break;
        };
        if c.is_whitespace() || "()<>\"'`".contains(c) {
            break;
        }
        i += c.len_utf8();
    }
    i
}

fn char_boundary_at_or_before(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_one_character_inputs_do_not_panic() {
        assert!(candidates("").is_empty());
        assert!(candidates("x").is_empty());
        assert!(candidates(" ").is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn absolute_windows_markdown_link_accepts_chat_leading_slash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preview.md");
        std::fs::write(&path, "# Exact file").unwrap();
        let source = format!(
            "[preview](/{}:1)",
            path.to_string_lossy().replace('\\', "/")
        );
        let link = candidates(&source).into_iter().next().unwrap();
        let resolved = resolve(&link, dir.path()).unwrap();
        assert_eq!(resolved.path, path.canonicalize().unwrap());
        assert_eq!(resolved.line, Some(1));
        let canonical = Candidate {
            target: resolved.path.to_string_lossy().into_owned(),
            ..link.clone()
        };
        assert_eq!(resolve(&canonical, dir.path()).unwrap().path, resolved.path);
        for target in [
            r"\\?\UNC\server\share\file.txt",
            r"\\.\C:\file.txt",
            r"\\?\C:relative.txt",
        ] {
            assert!(resolve(
                &Candidate {
                    target: target.into(),
                    ..link.clone()
                },
                dir.path()
            )
            .is_err());
        }
    }

    #[test]
    fn markdown_label_keeps_spaces_unicode_and_position_reference() {
        let s = "See [résumé draft](docs/éducation plan.md#L12) now";
        let c = candidates(s).into_iter().find(|c| c.markdown).unwrap();
        assert_eq!(c.display, "résumé draft");
        assert_eq!(&s[c.range.clone()], "résumé draft");
        assert_eq!(c.target, "docs/éducation plan.md");
        assert_eq!(c.line, Some(12));
        assert_eq!(c.column, None);
    }

    #[test]
    fn markdown_destination_handles_nested_parentheses_and_escaped_spaces() {
        let s = r"See [draft](docs/review\ notes/(final\ copy).md:12:4)";
        let c = candidates(s).into_iter().find(|c| c.markdown).unwrap();
        assert_eq!(c.target, "docs/review notes/(final copy).md");
        assert_eq!((c.line, c.column), (Some(12), Some(4)));
        assert_eq!(&s[c.range.clone()], "draft");
    }

    #[test]
    fn inline_code_and_plain_paths_include_spaces_and_line_column() {
        let quoted = r#"{"file_path":"C:\\work\\folder name\\Dockerfile"}"#;
        let decoded = candidates(quoted)
            .into_iter()
            .find(|c| c.target == r"C:\work\folder name\Dockerfile")
            .unwrap();
        assert_eq!(&quoted[decoded.range], r"C:\\work\\folder name\\Dockerfile");
        assert!(candidates("Open src/Dockerfile")
            .iter()
            .any(|c| c.target == "src/Dockerfile"));
        let s = "Open `src/Unicode file.rs:8:3` or C:\\work\\another file.rs.";
        let cs = candidates(s);
        assert!(cs.iter().any(|c| c.target == "src/Unicode file.rs"
            && c.line == Some(8)
            && c.column == Some(3)));
        assert!(cs.iter().any(|c| c.target == r"C:\work\another file.rs"));
    }

    #[test]
    fn schemes_and_unc_are_rejected() {
        let s = "[web](https://example.test/a.md) [net](\\\\server\\share\\a.md) \\\\server\\share\\a.md";
        assert!(candidates(s).is_empty());
    }

    #[test]
    fn plain_relative_path_with_spaces_and_unicode_resolves_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("folder name");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("café file.rs"), "fn main() {}").unwrap();
        let s = "Open folder name/café file.rs:8:3.";
        let candidate = candidates(s)
            .into_iter()
            .filter(|c| c.range.start == s.find("folder").unwrap())
            .find(|c| resolve(c, dir.path()).is_ok())
            .unwrap();
        assert_eq!(candidate.target, "folder name/café file.rs");
        let resolved = resolve(&candidate, dir.path()).unwrap();
        assert_eq!((resolved.line, resolved.column), (Some(8), Some(3)));
    }

    #[test]
    fn zero_positions_are_rejected_for_markdown_plain_and_explicit_operands() {
        for reference in ["src/a.rs#L0", "src/a.rs:0", "src/a.rs:2:0"] {
            assert!(explicit_reference(reference).is_none(), "{reference}");
            let s = format!("[{reference}]({reference})");
            assert!(candidates(&s).is_empty(), "{reference}");
        }
    }

    #[test]
    fn resolve_requires_exact_existing_local_file_under_cwd() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hello world.md"), "# Hi").unwrap();
        let c = Candidate {
            display: "friendly".into(),
            range: 0..8,
            target: "hello world.md".into(),
            line: Some(2),
            column: None,
            markdown: true,
        };
        let r = resolve(&c, dir.path()).unwrap();
        assert_eq!(
            r.path,
            std::fs::canonicalize(dir.path().join("hello world.md")).unwrap()
        );
        assert_eq!(r.line, Some(2));
        let mut missing = c.clone();
        missing.target = "hello.md".into();
        assert!(resolve(&missing, dir.path()).is_err());
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        let doc = dir.path().join("docs/e2e-status.md");
        std::fs::write(&doc, "reported status").unwrap();
        let mut explicit = c.clone();
        explicit.target = "e2e-status.md".into();
        assert_eq!(
            resolve(&explicit, dir.path()).unwrap().path,
            std::fs::canonicalize(&doc).unwrap()
        );
        let mut plain = explicit.clone();
        plain.markdown = false;
        assert!(!exists_cached(&plain, dir.path()));
        assert!(exists_cached(&explicit, dir.path()));
        assert!(resolve(&plain, dir.path()).is_err());
        let mut nested = explicit.clone();
        nested.target = "missing/e2e-status.md".into();
        assert!(resolve(&nested, dir.path()).is_err());
        let root_doc = dir.path().join("e2e-status.md");
        std::fs::write(&root_doc, "root wins").unwrap();
        assert_eq!(
            resolve(&explicit, dir.path()).unwrap().path,
            std::fs::canonicalize(root_doc).unwrap()
        );
    }
}
