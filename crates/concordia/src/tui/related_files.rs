//! Chat references, recent previews and bounded workspace discovery without reading file contents.
use super::*;
use crate::file_links::{self, Candidate};
use std::collections::HashSet;

const MAX_SOURCES: usize = 128;
const MAX_SOURCE_BYTES: usize = 256 * 1024;
const MAX_CANDIDATES: usize = 512;
const MAX_FILES: usize = 4096;
const TTL: Duration = Duration::from_secs(5);

pub(super) fn heading(path: &Path, width: usize) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let budget = width.saturating_sub(7);
    if path.chars().map(text::char_width).sum::<usize>() <= budget {
        return format!("Open \u{b7} {path}");
    }
    let mut suffix = Vec::new();
    let mut used = 1;
    for c in path.chars().rev() {
        let size = text::char_width(c);
        if used + size > budget {
            break;
        }
        suffix.push(c);
        used += size;
    }
    format!(
        "Open \u{b7} \u{2026}{}",
        suffix.into_iter().rev().collect::<String>()
    )
}

#[derive(Clone)]
pub(super) struct RelatedFile {
    pub candidate: Candidate,
    pub label: String,
    pub origin: &'static str,
    pub directory: bool,
}

#[derive(Default)]
pub(super) struct State {
    pub entries: Vec<RelatedFile>,
    job: Option<std::thread::JoinHandle<(u64, Vec<RelatedFile>)>>,
    epoch: u64,
    loaded: Option<Instant>,
    pub targets: Vec<(Rect, usize)>,
    pub pressed: Option<usize>,
    pub base: Option<PathBuf>,
    pub recent: Vec<PathBuf>,
    history: Option<PathBuf>,
    history_job: Option<std::thread::JoinHandle<()>>,
    history_dirty: bool,
}

impl State {
    pub fn new(state: &Path, cwd: &Path) -> Self {
        use sha2::{Digest, Sha256};
        let workspace = cwd.canonicalize().unwrap_or_else(|_| cwd.into());
        let hash = Sha256::digest(workspace.to_string_lossy().as_bytes());
        let history = state.join("file-recent").join(format!("{hash:x}.json"));
        let recent = std::fs::File::open(&history)
            .ok()
            .and_then(|f| {
                use std::io::Read;
                let mut bytes = Vec::new();
                f.take(64 * 1024).read_to_end(&mut bytes).ok()?;
                serde_json::from_slice::<Vec<PathBuf>>(&bytes).ok()
            })
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.is_absolute() && !sensitive(p))
            .take(64)
            .collect();
        Self {
            history: Some(history),
            recent,
            ..Self::default()
        }
    }
    pub fn remember(&mut self, path: &Path) {
        if sensitive(path) {
            return;
        }
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.into());
        self.recent.truncate(64);
        self.loaded = None;
        self.epoch = self.epoch.wrapping_add(1);
        self.history_dirty = true;
    }
    pub fn browse(&mut self, dir: PathBuf) {
        self.reset();
        self.base = Some(dir);
    }
    pub fn reset(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.entries.clear();
        self.loaded = None;
        self.targets.clear();
        self.base = None;
    }

    pub fn invalidate(&mut self) {
        self.loaded = None;
    }

    pub fn loading(&self) -> bool {
        self.job.is_some()
    }

    pub fn request(&mut self, cwd: &Path, sources: impl FnOnce() -> Vec<String>) {
        if self.job.is_some() || self.loaded.is_some_and(|t| t.elapsed() < TTL) {
            return;
        }
        let epoch = self.epoch;
        let cwd = cwd.to_path_buf();
        let base = self.base.clone().unwrap_or_else(|| cwd.clone());
        let recent = self.recent.clone();
        let sources = sources();
        match std::thread::Builder::new()
            .name("concordia-related-files".into())
            .spawn(move || (epoch, discover_workspace(&cwd, &base, &sources, &recent)))
        {
            Ok(job) => {
                self.job = Some(job);
                self.loaded = Some(Instant::now());
            }
            Err(_) => self.loaded = Some(Instant::now()),
        }
    }

    pub fn poll(&mut self) -> bool {
        if self.history_job.as_ref().is_some_and(|j| j.is_finished()) {
            let _ = self.history_job.take().unwrap().join();
        }
        if self.history_dirty && self.history_job.is_none() {
            if let Some(history) = self.history.clone() {
                let recent = self.recent.clone();
                self.history_job = std::thread::Builder::new()
                    .name("concordia-file-history".into())
                    .spawn(move || {
                        if let Some(parent) = history.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Ok(bytes) = serde_json::to_vec(&recent) {
                            let _ = crate::state_io::write_atomic(&history, &bytes);
                        }
                    })
                    .ok();
                self.history_dirty = self.history_job.is_none();
            }
        }
        if !self.job.as_ref().is_some_and(|job| job.is_finished()) {
            return false;
        }
        if let Ok((epoch, entries)) = self.job.take().unwrap().join() {
            if epoch == self.epoch {
                self.entries = entries;
                return true;
            }
        }
        self.loaded = None;
        false
    }
}

pub(super) fn sensitive(path: &Path) -> bool {
    crate::workspace_paths::sensitive(path)
}

fn discover(cwd: &Path, sources: &[String]) -> Vec<RelatedFile> {
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut examined = 0;
    for source in sources {
        let mut candidates = file_links::candidates(source);
        if let Some(candidate) = file_links::explicit_reference(source) {
            candidates.push(candidate);
        }
        // Prefer exact Markdown links and longest candidates before ambiguous prose suffixes.
        candidates.sort_by_key(|c| {
            (
                std::cmp::Reverse(c.markdown),
                std::cmp::Reverse(c.range.len()),
            )
        });
        for candidate in candidates {
            examined += 1;
            if examined > MAX_CANDIDATES || entries.len() >= MAX_FILES {
                return entries;
            }
            if sensitive(Path::new(&candidate.target)) {
                continue;
            }
            let Ok(file) = file_links::resolve(&candidate, &cwd) else {
                continue;
            };
            if sensitive(&file.path) {
                continue;
            }
            let key = file.path.to_string_lossy().to_string();
            #[cfg(windows)]
            let key = key.to_lowercase();
            if !seen.insert(key) {
                continue;
            }
            let label = file
                .path
                .strip_prefix(&cwd)
                .unwrap_or(&file.path)
                .to_string_lossy()
                .replace('\\', "/");
            let mut candidate = candidate;
            let target = file.path.to_string_lossy();
            candidate.target = target.strip_prefix(r"\\?\").unwrap_or(&target).to_owned();
            entries.push(RelatedFile {
                candidate,
                label,
                origin: "chat",
                directory: false,
            });
        }
    }
    entries
}

fn discover_workspace(
    cwd: &Path,
    base: &Path,
    sources: &[String],
    recent: &[PathBuf],
) -> Vec<RelatedFile> {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.into());
    let base = base.canonicalize().unwrap_or_else(|_| base.into());
    let mut entries = discover(&cwd, sources);
    let mut seen: HashSet<_> = entries
        .iter()
        .map(|e| e.candidate.target.replace('\\', "/").to_lowercase())
        .collect();
    let mut add = |path: PathBuf, directory: bool, origin: &'static str| {
        if entries.len() >= MAX_FILES || sensitive(&path) {
            return;
        }
        let target = path
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('\\', "/");
        if !seen.insert(target.to_lowercase()) {
            return;
        }
        let label = path
            .strip_prefix(&cwd)
            .unwrap_or(&path)
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('\\', "/");
        entries.push(RelatedFile {
            candidate: Candidate {
                display: label.clone(),
                target,
                range: 0..0,
                line: None,
                column: None,
                markdown: false,
            },
            label: if directory {
                format!("{label}/")
            } else {
                label
            },
            origin,
            directory,
        });
    };
    for path in recent {
        if path.is_file() {
            add(path.clone(), false, "recent");
        }
    }
    if base != cwd {
        if let Some(parent) = base.parent() {
            add(parent.into(), true, "parent folder");
        }
    }
    let project = [
        ".git",
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "go.mod",
    ]
    .iter()
    .any(|p| base.join(p).exists());
    let walk = ignore::WalkBuilder::new(&base)
        .hidden(false)
        .follow_links(false)
        .require_git(false)
        .max_depth(Some(if project { 14 } else { 1 }))
        .sort_by_file_path(|a, b| a.cmp(b))
        .filter_entry(|entry| {
            crate::workspace_paths::discoverable(
                entry.path(),
                entry.file_type().is_some_and(|t| t.is_dir()),
            )
        })
        .build();
    for e in walk.take(12_000).flatten() {
        if e.depth() == 0 {
            continue;
        }
        if let Some(typ) = e.file_type() {
            if typ.is_file() || typ.is_dir() && e.depth() == 1 {
                add(e.path().to_path_buf(), typ.is_dir(), "workspace");
            }
        }
    }
    entries
}

impl App {
    pub(super) fn request_related_files(&mut self) {
        if let Some((_, query)) = chatref::reference_query(&self.editor.text()) {
            if !query.contains('/') && self.picker.is_none() && self.related_files.base.is_some() {
                self.related_files.base = None;
                self.related_files.epoch = self.related_files.epoch.wrapping_add(1);
                self.related_files.loaded = None;
            }
            if let Some((dir, _)) = query.rsplit_once('/') {
                let dir = if dir.ends_with(':') {
                    format!("{dir}/")
                } else {
                    dir.to_owned()
                };
                let path = Path::new(&dir);
                let path = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    self.cwd.join(path)
                };
                if path.is_dir() && self.related_files.base.as_ref() != Some(&path) {
                    self.related_files.base = Some(path);
                    self.related_files.epoch = self.related_files.epoch.wrapping_add(1);
                    self.related_files.loaded = None;
                }
            }
        }
        let messages = &self.messages;
        let live = &self.live_text;
        let pending = self.continuation.pending_text();
        self.related_files.request(&self.cwd, || {
            let mut remaining = MAX_SOURCE_BYTES;
            std::iter::once(live.as_str())
                .chain(pending)
                .chain(
                    messages
                        .iter()
                        .rev()
                        .filter(|m| {
                            matches!(m.role, MsgRole::User | MsgRole::Assistant | MsgRole::Card)
                        })
                        .map(|m| {
                            m.card
                                .as_ref()
                                .and_then(|c| c.detail.as_deref())
                                .unwrap_or(&m.text)
                        }),
                )
                .take(MAX_SOURCES)
                .filter_map(|source| {
                    if remaining == 0 {
                        return None;
                    }
                    let mut end = source.len().min(remaining).min(64 * 1024);
                    while !source.is_char_boundary(end) {
                        end -= 1;
                    }
                    remaining -= end;
                    Some(source[..end].to_owned())
                })
                .collect()
        });
    }

    pub(super) fn open_related_files(&mut self) {
        self.related_files.base = None;
        self.related_files.invalidate();
        self.request_related_files();
        self.build_related_files_picker();
    }

    pub(super) fn build_related_files_picker(&mut self) {
        let mut rows: Vec<_> = self
            .related_files
            .entries
            .iter()
            .enumerate()
            .map(|(i, entry)| PickerRow {
                id: format!("file:{i}"),
                label: entry.label.clone(),
                detail: entry
                    .candidate
                    .line
                    .map(|line| format!("line {line} \u{b7} preview"))
                    .unwrap_or_else(|| {
                        if entry.directory {
                            format!("{} · open folder", entry.origin)
                        } else {
                            format!("{} · preview", entry.origin)
                        }
                    }),
            })
            .collect();
        if rows.is_empty() {
            rows.push(PickerRow {
                id: "none".into(),
                label: if self.related_files.loading() {
                    "Finding files mentioned in this chat..."
                } else {
                    "No matches. Type a relative path (src/main.rs) or an absolute path (C:/project/file.md)."
                }
                .into(),
                detail: String::new(),
            });
        }
        self.show_picker(
            PickerKind::Files,
            format!(
                "Open · {} · files in this chat + recent + workspace",
                self.related_files
                    .base
                    .as_deref()
                    .unwrap_or(&self.cwd)
                    .display()
            ),
            rows,
            true,
        );
    }

    pub(super) fn preview_related_file(&mut self, index: usize) {
        if let Some(entry) = self.related_files.entries.get(index).cloned() {
            if entry.directory {
                self.related_files
                    .browse(PathBuf::from(&entry.candidate.target));
                self.request_related_files();
                self.build_related_files_picker();
                return;
            }
            self.open_file_candidate(entry.candidate, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn narrow_browser_headings_keep_the_current_folder_visible() {
        let title = heading(Path::new("C:/long/workspace/root/current-project"), 26);
        assert!(title.ends_with("current-project"));
        assert!(title.contains('\u{2026}'));
        assert!(title.chars().map(text::char_width).sum::<usize>() <= 26);
        assert_eq!(
            heading(Path::new("C:/project"), 60),
            "Open \u{b7} C:/project"
        );
    }
    #[test]
    fn discovery_deduplicates_aliases_and_locations_and_only_uses_chat_references() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        for file in [
            "src/code.rs",
            "notes with spaces.md",
            "unrelated.txt",
            ".env",
            "auth.json",
        ] {
            std::fs::write(dir.path().join(file), "fixture").unwrap();
        }
        let cwd = std::fs::canonicalize(dir.path()).unwrap();
        let entries = discover(&cwd, &["[notes](<notes with spaces.md>) @src/code.rs:7 src/./code.rs:2 src/CODE.rs:3 .env auth.json missing.txt".into()]);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.label == "notes with spaces.md"));
        assert!(entries.iter().any(|e| e.label == "src/code.rs"));
    }

    #[test]
    fn late_discovery_cannot_leak_files_into_another_conversation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.txt"), "old").unwrap();
        let mut state = State::default();
        state.request(dir.path(), || vec!["old.txt".into()]);
        state.reset();
        let deadline = Instant::now() + Duration::from_secs(5);
        while state.loading() {
            state.poll();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(state.entries.is_empty());
    }
}
