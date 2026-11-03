//! Bounded literal project search. No writes, shell commands, providers or background indexing.
use crate::workspace_paths;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const MAX_QUERY_BYTES: usize = 1024;
pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_FILES: usize = 4096;
const MAX_ENTRIES: usize = 12_000;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_MATCHES: usize = 1000;
const MAX_LINES: usize = 50_000;
const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    pub root: PathBuf,
    pub conversation: String,
    pub generation: u64,
    pub query: String,
    pub case_sensitive: bool,
}

impl SearchRequest {
    pub fn new(
        root: &Path,
        conversation: &str,
        generation: u64,
        query: &str,
        case_sensitive: bool,
    ) -> Result<Self, String> {
        if query.is_empty() || query.len() > MAX_QUERY_BYTES || query.chars().any(char::is_control)
        {
            return Err("search requires a single-line literal of 1–1024 UTF-8 bytes".into());
        }
        let root = root
            .canonicalize()
            .map_err(|_| "search root is unavailable")?;
        if !root.is_dir() || workspace_paths::sensitive(&root) {
            return Err("search root is not an allowed workspace directory".into());
        }
        if conversation.is_empty() || conversation.len() > 256 {
            return Err("search requires an explicit conversation identity".into());
        }
        Ok(Self {
            root,
            conversation: conversation.into(),
            generation,
            query: query.into(),
            case_sensitive,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: PathBuf,
    pub range: Range<usize>,
    pub line: u32,
    /// One-based Unicode scalar column, never a byte offset or terminal-cell column.
    pub column: u32,
    pub hash: [u8; 32],
    pub preview: String,
}

#[derive(Debug)]
pub struct VerifiedHit {
    pub hit: SearchHit,
    pub text: String,
    pub modified: Option<std::time::SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Complete,
    Incomplete(String),
    Cancelled,
    Error(String),
}

#[derive(Debug)]
pub struct SearchSummary {
    pub request: SearchRequest,
    pub hits: Vec<SearchHit>,
    pub status: Status,
    pub files_examined: usize,
    pub bytes_read: usize,
    pub skipped_nontext: usize,
}

fn stopped(cancel: &AtomicBool, started: Instant) -> Option<Status> {
    if cancel.load(Ordering::Relaxed) {
        Some(Status::Cancelled)
    } else if started.elapsed() >= DEADLINE {
        Some(Status::Incomplete("five-second deadline".into()))
    } else {
        None
    }
}

fn read_file(
    root: &Path,
    path: &Path,
    cancel: &AtomicBool,
    started: Instant,
    budget: usize,
) -> Result<Vec<u8>, String> {
    workspace_paths::regular_file(root, path)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Open the entry itself, never follow a last-component reparse point swapped in
        // between path validation and open. Nonregular entries are rejected below.
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let mut file = options.open(path).map_err(|_| "file could not be opened")?;
    let metadata = file
        .metadata()
        .map_err(|_| "file metadata is unavailable")?;
    if !metadata.is_file() {
        return Err("workspace entry is not a regular file".into());
    }
    workspace_paths::regular_file(root, path)?;
    if metadata.len() > MAX_FILE_BYTES as u64 {
        return Err("2 MiB per-file limit".into());
    }
    let mut bytes = Vec::with_capacity((metadata.len() as usize).min(MAX_FILE_BYTES));
    let mut buffer = [0; 64 * 1024];
    loop {
        if let Some(state) = stopped(cancel, started) {
            return Err(match state {
                Status::Cancelled => "cancelled".into(),
                _ => "five-second deadline".into(),
            });
        }
        let remaining = (MAX_FILE_BYTES + 1)
            .saturating_sub(bytes.len())
            .min(budget.saturating_sub(bytes.len()));
        if remaining == 0 {
            return Err("64 MiB aggregate read limit".into());
        }
        let take = remaining.min(buffer.len());
        let n = file
            .read(&mut buffer[..take])
            .map_err(|_| "file read failed")?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..n]);
        if bytes.len() > MAX_FILE_BYTES {
            return Err("2 MiB per-file limit".into());
        }
    }
    workspace_paths::regular_file(root, path)?;
    Ok(bytes)
}

/// Lowercase with an explicit original-byte map, preserving ranges for expanding Unicode chars.
fn lowercase(
    text: &str,
    cancel: &AtomicBool,
    started: Instant,
) -> Result<(String, Vec<(usize, usize)>), Status> {
    let mut folded = String::new();
    let mut origins = Vec::new();
    for (index, (start, c)) in text.char_indices().enumerate() {
        if index % 4096 == 0 {
            if let Some(state) = stopped(cancel, started) {
                return Err(state);
            }
        }
        for lower in c.to_lowercase() {
            folded.push(lower);
            for _ in 0..lower.len_utf8() {
                origins.push((start, start + c.len_utf8()));
            }
        }
    }
    Ok((folded, origins))
}

fn match_ranges(
    text: &str,
    query: &str,
    case_sensitive: bool,
    cancel: &AtomicBool,
    started: Instant,
) -> Result<Vec<Range<usize>>, Status> {
    if case_sensitive {
        return Ok(text
            .match_indices(query)
            .take(MAX_MATCHES + 1)
            .map(|(start, m)| start..start + m.len())
            .collect());
    }
    let (folded, origins) = lowercase(text, cancel, started)?;
    let query = query.to_lowercase();
    let mut ranges = Vec::new();
    for (start, m) in folded.match_indices(&query).take(MAX_MATCHES + 1) {
        let range = origins[start].0..origins[start + m.len() - 1].1;
        if ranges.last() != Some(&range) {
            ranges.push(range);
        }
    }
    Ok(ranges)
}

fn location(text: &str, offset: usize) -> (u32, u32, String) {
    let prefix = &text[..offset];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
    let start = prefix.rfind('\n').map_or(0, |p| p + 1);
    let content_start = if start == 0 && text.starts_with('\u{feff}') {
        3
    } else {
        start
    };
    let column = text[content_start.min(offset)..offset].chars().count() + 1;
    let end = text[offset..].find('\n').map_or(text.len(), |p| offset + p);
    let excerpt_start = text[start..offset]
        .char_indices()
        .rev()
        .nth(80)
        .map_or(start, |(p, _)| start + p);
    let preview = text[excerpt_start..end]
        .trim_end_matches('\r')
        .chars()
        .take(240)
        .collect();
    (line as u32, column as u32, preview)
}

pub fn search(request: SearchRequest, cancel: &AtomicBool) -> SearchSummary {
    let mut summary = SearchSummary {
        request,
        hits: Vec::new(),
        status: Status::Complete,
        files_examined: 0,
        bytes_read: 0,
        skipped_nontext: 0,
    };
    let started = Instant::now();
    if let Err(error) = SearchRequest::new(
        &summary.request.root,
        &summary.request.conversation,
        summary.request.generation,
        &summary.request.query,
        summary.request.case_sensitive,
    ) {
        summary.status = Status::Error(error);
        return summary;
    }
    let root = summary.request.root.clone();
    let walk = ignore::WalkBuilder::new(&root)
        .hidden(false)
        .follow_links(false)
        .require_git(false)
        .max_depth(Some(14))
        .filter_entry(|entry| {
            workspace_paths::discoverable(
                entry.path(),
                entry.file_type().is_some_and(|t| t.is_dir()),
            )
        })
        .build();
    for (examined, entry) in walk.enumerate() {
        if let Some(state) = stopped(cancel, started) {
            summary.status = state;
            break;
        }
        if examined >= MAX_ENTRIES {
            summary.status = Status::Incomplete("12,000 entry traversal limit".into());
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                summary.status = Status::Incomplete("workspace traversal error".into());
                break;
            }
        };
        if entry.depth() >= 14 && entry.file_type().is_some_and(|t| t.is_dir()) {
            summary.status = Status::Incomplete("14-level directory depth limit".into());
            break;
        }
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if summary.files_examined >= MAX_FILES {
            summary.status = Status::Incomplete("4,096 file limit".into());
            break;
        }
        summary.files_examined += 1;
        let path = entry.path().to_path_buf();
        let bytes = match read_file(
            &root,
            &path,
            cancel,
            started,
            MAX_TOTAL_BYTES.saturating_sub(summary.bytes_read),
        ) {
            Ok(bytes) => bytes,
            Err(reason) => {
                summary.status = if cancel.load(Ordering::Relaxed) {
                    Status::Cancelled
                } else {
                    Status::Incomplete(reason)
                };
                break;
            }
        };
        summary.bytes_read += bytes.len();
        if bytes.contains(&0) {
            summary.skipped_nontext += 1;
            continue;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            summary.skipped_nontext += 1;
            continue;
        };
        if text.bytes().filter(|b| *b == b'\n').count() + 1 > MAX_LINES {
            summary.status = Status::Incomplete("50,000 lines per-file limit".into());
            break;
        }
        let hash = Sha256::digest(&bytes).into();
        let ranges = match match_ranges(
            text,
            &summary.request.query,
            summary.request.case_sensitive,
            cancel,
            started,
        ) {
            Ok(ranges) => ranges,
            Err(status) => {
                summary.status = status;
                break;
            }
        };
        for range in ranges {
            if let Some(state) = stopped(cancel, started) {
                summary.status = state;
                break;
            }
            if summary.hits.len() >= MAX_MATCHES {
                summary.status = Status::Incomplete("1,000 match limit".into());
                break;
            }
            let (line, column, preview) = location(text, range.start);
            summary.hits.push(SearchHit {
                path: path.clone(),
                range,
                line,
                column,
                hash,
                preview,
            });
        }
        if summary.status != Status::Complete {
            break;
        }
    }
    summary
}

/// Return the exact verified snapshot for opening. The UI must not re-read another revision.
pub fn verify_hit(
    request: &SearchRequest,
    hit: &SearchHit,
    cancel: &AtomicBool,
) -> Result<VerifiedHit, String> {
    SearchRequest::new(
        &request.root,
        &request.conversation,
        request.generation,
        &request.query,
        request.case_sensitive,
    )?;
    let before = std::fs::metadata(&hit.path).map_err(|_| "search result is unavailable")?;
    let bytes = read_file(
        &request.root,
        &hit.path,
        cancel,
        Instant::now(),
        MAX_FILE_BYTES + 1,
    )?;
    let after = std::fs::metadata(&hit.path).map_err(|_| "search result is unavailable")?;
    if before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || after.len() != bytes.len() as u64
    {
        return Err("search result changed during verification".into());
    }
    let hash: [u8; 32] = Sha256::digest(&bytes).into();
    if hash != hit.hash {
        return Err("search result is stale; run the search again".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "search result is no longer UTF-8 text")?;
    let value = text
        .get(hit.range.clone())
        .ok_or("search result range is unavailable")?;
    let matches = if request.case_sensitive {
        value == request.query
    } else {
        value.to_lowercase().contains(&request.query.to_lowercase())
    };
    if !matches {
        return Err("search result no longer matches the query".into());
    }
    let (line, column, _) = location(&text, hit.range.start);
    if line != hit.line || column != hit.column {
        return Err("search location is stale".into());
    }
    Ok(VerifiedHit {
        hit: hit.clone(),
        text,
        modified: before.modified().ok(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(dir: &Path, query: &str, case: bool) -> SearchRequest {
        SearchRequest::new(dir, "fixture-conversation", 1, query, case).unwrap()
    }
    #[test]
    fn original_bytes_locations_and_unicode_case_mapping_survive_bom_crlf() {
        let dir = tempfile::tempdir().unwrap();
        let text = "\u{feff}alpha\r\nİstanbul ΑΒΓ mixed\r\n";
        std::fs::write(dir.path().join("notes.txt"), text).unwrap();
        let req = request(dir.path(), "αβγ", false);
        let result = search(req.clone(), &AtomicBool::new(false));
        assert_eq!(result.status, Status::Complete);
        let hit = &result.hits[0];
        assert_eq!(&text[hit.range.clone()], "ΑΒΓ");
        assert_eq!((hit.line, hit.column), (2, 10));
        assert_eq!(
            verify_hit(&req, hit, &AtomicBool::new(false)).unwrap().text,
            text
        );
        let expanded = search(
            request(dir.path(), "i\u{307}", false),
            &AtomicBool::new(false),
        );
        assert_eq!(&text[expanded.hits[0].range.clone()], "İ");
    }
    #[test]
    fn ignores_credentials_builds_gitignore_and_rejects_stale_results() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::create_dir(dir.path().join(".codex-2")).unwrap();
        for file in [
            ".env",
            "auth.json",
            "target/code.rs",
            ".codex-2/history.jsonl",
            "ignored.txt",
            "visible.txt",
        ] {
            std::fs::write(dir.path().join(file), "needle").unwrap();
        }
        std::fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        let req = request(dir.path(), "needle", true);
        let result = search(req.clone(), &AtomicBool::new(false));
        assert_eq!(result.hits.len(), 1);
        assert!(result.hits[0].path.ends_with("visible.txt"));
        std::fs::write(&result.hits[0].path, "replacement").unwrap();
        assert!(verify_hit(&req, &result.hits[0], &AtomicBool::new(false))
            .unwrap_err()
            .contains("stale"));
    }
    #[test]
    fn cancellation_and_match_limits_are_explicit_and_bounded() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("many.txt"), "needle\n".repeat(1100)).unwrap();
        let result = search(request(dir.path(), "needle", true), &AtomicBool::new(false));
        assert_eq!(result.hits.len(), MAX_MATCHES);
        assert!(matches!(result.status, Status::Incomplete(_)));
        let result = search(request(dir.path(), "needle", true), &AtomicBool::new(true));
        assert_eq!(result.status, Status::Cancelled);
        assert!(result.hits.is_empty());
    }
    #[test]
    fn forged_paths_and_ranges_cannot_open_unrelated_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "needle").unwrap();
        let req = request(dir.path(), "needle", true);
        let result = search(req.clone(), &AtomicBool::new(false));
        let mut hit = result.hits[0].clone();
        hit.range = 1..2;
        assert!(verify_hit(&req, &hit, &AtomicBool::new(false)).is_err());
        hit.path = req.root.join("../escape.txt");
        assert!(verify_hit(&req, &hit, &AtomicBool::new(false)).is_err());
    }

    #[test]
    fn oversized_files_and_invalid_requests_never_claim_complete_scans() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("large.txt"), vec![b'x'; MAX_FILE_BYTES + 1]).unwrap();
        let result = search(request(dir.path(), "x", true), &AtomicBool::new(false));
        assert!(matches!(result.status, Status::Incomplete(reason) if reason.contains("2 MiB")));
        assert!(result.hits.is_empty());
        let mut req = request(dir.path(), "x", false);
        req.query.clear();
        assert!(matches!(
            search(req, &AtomicBool::new(false)).status,
            Status::Error(_)
        ));
    }
}
