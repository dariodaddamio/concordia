//! Lossless UTF-8 file editing with delta undo and compare-before-replace saves.
use std::io::{Read, Write};
use std::ops::Range;
use std::path::Path;
use unicode_width::UnicodeWidthChar;

pub const MAX_BYTES: usize = 2 * 1024 * 1024;
const HISTORY_BYTES: usize = 4 * 1024 * 1024;
const HISTORY_STEPS: usize = 1000;
const MAX_LINES: usize = 50_000;

struct Edit {
    start: usize,
    removed: String,
    inserted: String,
    before: usize,
    after: usize,
}

impl Edit {
    fn bytes(&self) -> usize {
        self.removed.len() + self.inserted.len() + std::mem::size_of::<Self>()
    }
}

pub struct Document {
    text: String,
    baseline: String,
    cursor: usize,
    starts: Vec<usize>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    history_bytes: usize,
    anchor: Option<usize>,
    revision: u64,
}

impl Document {
    pub fn new(text: String) -> Self {
        let cursor = if text.starts_with('\u{feff}') { 3 } else { 0 };
        let mut doc = Self {
            baseline: text.clone(),
            text,
            cursor,
            starts: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            history_bytes: 0,
            anchor: None,
            revision: 0,
        };
        doc.reindex();
        doc
    }

    fn reindex(&mut self) {
        self.starts.clear();
        self.starts.push(0);
        self.starts.extend(
            self.text
                .bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| i + 1),
        );
    }

    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn baseline(&self) -> &str {
        &self.baseline
    }
    pub fn dirty(&self) -> bool {
        self.text != self.baseline
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn acknowledge_save(&mut self, saved: String) {
        self.baseline = saved;
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn restored(baseline: String, text: String) -> Result<Self, String> {
        if baseline.len() > MAX_BYTES || text.len() > MAX_BYTES || text.contains('\0') {
            return Err("file recovery exceeds its bounds".into());
        }
        let mut doc = Self::new(text);
        doc.baseline = baseline;
        Ok(doc)
    }
    pub fn begin_selection(&mut self, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
    }
    pub fn select_all(&mut self) {
        self.anchor = Some(if self.text.starts_with('\u{feff}') {
            3
        } else {
            0
        });
        self.cursor = self.text.len();
    }
    fn selection(&self) -> Option<Range<usize>> {
        self.anchor
            .filter(|a| *a != self.cursor)
            .map(|a| a.min(self.cursor)..a.max(self.cursor))
    }
    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|r| &self.text[r])
    }
    pub fn selection_columns(&self, row: usize) -> Option<(usize, usize)> {
        let selection = self.selection()?;
        let line = self.line_range(row);
        let a = selection.start.max(line.start);
        let b = selection.end.min(line.end);
        (a < b).then(|| {
            (
                self.text[line.start..a].chars().map(display_width).sum(),
                self.text[line.start..b].chars().map(display_width).sum(),
            )
        })
    }
    pub fn line_count(&self) -> usize {
        self.starts.len()
    }
    fn line_range(&self, row: usize) -> Range<usize> {
        let start = self.starts[row.min(self.starts.len() - 1)];
        let mut end = self.starts.get(row + 1).copied().unwrap_or(self.text.len());
        if end > start && self.text.as_bytes()[end - 1] == b'\n' {
            end -= 1;
        }
        if end > start && self.text.as_bytes()[end - 1] == b'\r' {
            end -= 1;
        }
        start..end
    }
    pub fn line(&self, row: usize) -> &str {
        &self.text[self.line_range(row)]
    }
    pub fn position(&self) -> (usize, usize) {
        let row = self
            .starts
            .partition_point(|start| *start <= self.cursor)
            .saturating_sub(1);
        let col = self.text[self.starts[row]..self.cursor]
            .chars()
            .map(display_width)
            .sum();
        (row, col)
    }
    pub fn move_to(&mut self, row: usize, column: usize) {
        let range = self.line_range(row.min(self.starts.len() - 1));
        let mut cols = 0;
        self.cursor = range.end;
        for (offset, c) in self.text[range.clone()].char_indices() {
            if cols + display_width(c) > column {
                self.cursor = range.start + offset;
                break;
            }
            cols += display_width(c);
        }
    }
    pub fn horizontal(&mut self, right: bool) {
        if !right
            && self.cursor
                <= if self.text.starts_with('\u{feff}') {
                    3
                } else {
                    0
                }
        {
            return;
        }
        if right {
            if self.text[self.cursor..].starts_with("\r\n") {
                self.cursor += 2;
            } else if let Some(c) = self.text[self.cursor..].chars().next() {
                self.cursor += c.len_utf8();
            }
        } else if self.cursor > 0 {
            if self.text[..self.cursor].ends_with("\r\n") {
                self.cursor -= 2;
            } else if let Some(c) = self.text[..self.cursor].chars().next_back() {
                self.cursor -= c.len_utf8();
            }
        }
    }
    pub fn vertical(&mut self, down: bool) {
        let (row, column) = self.position();
        self.move_to(
            if down {
                (row + 1).min(self.starts.len() - 1)
            } else {
                row.saturating_sub(1)
            },
            column,
        );
    }
    pub fn line_end(&mut self, end: bool) {
        let (row, _) = self.position();
        let range = self.line_range(row);
        self.cursor = if end {
            range.end
        } else if row == 0 && self.text.starts_with('\u{feff}') {
            3
        } else {
            range.start
        };
    }

    fn replace(
        &mut self,
        range: Range<usize>,
        inserted: &str,
        cursor: usize,
    ) -> Result<(), String> {
        if self.text.len() - range.len() + inserted.len() > MAX_BYTES {
            return Err("file edit limit is 2 MiB".into());
        }
        let removed_newlines = self.text[range.clone()]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        let added_newlines = inserted.bytes().filter(|b| *b == b'\n').count();
        let ends_with_newline = if range.end == self.text.len() {
            inserted
                .as_bytes()
                .last()
                .or_else(|| self.text.as_bytes()[..range.start].last())
                == Some(&b'\n')
        } else {
            self.text.ends_with('\n')
        };
        if self.starts.len() - removed_newlines + added_newlines - usize::from(ends_with_newline)
            > MAX_LINES
        {
            return Err("file edit limit is 50,000 lines".into());
        }
        let edit = Edit {
            start: range.start,
            removed: self.text[range.clone()].into(),
            inserted: inserted.into(),
            before: self.cursor,
            after: cursor,
        };
        self.text.replace_range(range, inserted);
        self.cursor = cursor;
        self.anchor = None;
        self.revision = self.revision.wrapping_add(1);
        self.history_bytes -= self.redo.iter().map(Edit::bytes).sum::<usize>();
        self.redo.clear();
        self.history_bytes += edit.bytes();
        self.undo.push(edit);
        while self.history_bytes > HISTORY_BYTES || self.undo.len() > HISTORY_STEPS {
            let removed = self.undo.remove(0);
            self.history_bytes -= removed.bytes();
        }
        self.reindex();
        Ok(())
    }
    pub fn insert(&mut self, text: &str) -> Result<(), String> {
        if text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err("file paste contains unsupported control characters".into());
        }
        let range = self.selection().unwrap_or(self.cursor..self.cursor);
        let cursor = range.start + text.len();
        self.replace(range, text, cursor)
    }
    pub fn newline(&mut self) -> Result<(), String> {
        let newline = if self.baseline.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        self.insert(newline)
    }
    pub fn delete(&mut self, forward: bool) -> Result<(), String> {
        if let Some(range) = self.selection() {
            let cursor = range.start;
            return self.replace(range, "", cursor);
        }
        let cursor = self.cursor;
        self.horizontal(forward);
        let edge = self.cursor;
        self.cursor = cursor;
        if edge == cursor {
            return Ok(());
        }
        self.replace(cursor.min(edge)..cursor.max(edge), "", cursor.min(edge))
    }
    pub fn undo(&mut self, redo: bool) {
        self.anchor = None;
        self.revision = self.revision.wrapping_add(1);
        if redo {
            if let Some(edit) = self.redo.pop() {
                self.text
                    .replace_range(edit.start..edit.start + edit.removed.len(), &edit.inserted);
                self.cursor = edit.after;
                self.undo.push(edit);
            }
        } else if let Some(edit) = self.undo.pop() {
            self.text
                .replace_range(edit.start..edit.start + edit.inserted.len(), &edit.removed);
            self.cursor = edit.before;
            self.redo.push(edit);
        }
        self.reindex();
    }
}

pub fn display_width(c: char) -> usize {
    if c == '\t' {
        4
    } else if c == '\u{feff}' {
        0
    } else if c.is_control() {
        1
    } else {
        UnicodeWidthChar::width(c).unwrap_or(0)
    }
}

pub fn visible_line(text: &str, column: usize, width: usize) -> String {
    let mut output = String::new();
    let mut col = 0;
    for c in text.chars() {
        let cells = display_width(c);
        if col >= column + width {
            break;
        }
        if c == '\t' {
            for offset in 0..4 {
                if (column..column + width).contains(&(col + offset)) {
                    output.push(' ');
                }
            }
        } else if col >= column {
            output.push(if c.is_control() { '�' } else { c });
        }
        col += cells;
    }
    output
}

/// Existing bytes must still match exactly. Failures never retry or truncate the target.
pub fn save(path: &Path, expected: &str, replacement: &str) -> Result<(), String> {
    let read_current = || -> Result<Vec<u8>, String> {
        let meta = std::fs::symlink_metadata(path).map_err(|e| format!("save: {e}"))?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err("save target is no longer a regular file".into());
        }
        if meta.permissions().readonly() {
            return Err("save target is read-only".into());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| format!("save: {e}"))?
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("save: {e}"))?;
        Ok(bytes)
    };
    if replacement.len() > MAX_BYTES {
        return Err("file save limit is 2 MiB".into());
    }
    if read_current()? != expected.as_bytes() {
        return Err("file changed on disk; save refused, edits retained".into());
    }
    let suffix = uuid::Uuid::new_v4();
    let temp = path.with_file_name(format!(".concordia-{suffix}.tmp"));
    let backup = path.with_file_name(format!(".concordia-{suffix}.bak"));
    let prepare = || -> Result<(), String> {
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| format!("save staging: {e}"))?;
        std::fs::set_permissions(
            &temp,
            std::fs::metadata(path)
                .map_err(|e| e.to_string())?
                .permissions(),
        )
        .map_err(|e| e.to_string())?;
        out.write_all(replacement.as_bytes())
            .and_then(|_| out.sync_all())
            .map_err(|e| format!("save staging: {e}"))?;
        if read_current()? != expected.as_bytes() {
            return Err("file changed during save; edits retained".into());
        }
        Ok(())
    };
    if let Err(error) = prepare() {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    if let Err(error) = replace_file(path, &temp, &backup) {
        // ReplaceFileW can fail after moving files. Retain staging/backup and report uncertainty.
        return Err(format!(
            "save replacement failed ({error}); inspect {} and {}; no automatic retry",
            temp.display(),
            backup.display()
        ));
    }
    #[cfg(windows)]
    if std::fs::read(&backup)
        .map_err(|e| format!("save backup check: {e}; retain {}", backup.display()))?
        != expected.as_bytes()
    {
        return Err(format!(
            "file changed at replacement; external contents retained at {}; inspect before retry",
            backup.display()
        ));
    }
    let _ = std::fs::remove_file(&backup);
    Ok(())
}

#[cfg(windows)]
fn replace_file(path: &Path, temp: &Path, backup: &Path) -> std::io::Result<()> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn ReplaceFileW(
            original: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut c_void,
            reserved: *mut c_void,
        ) -> i32;
    }
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<u16>>()
    };
    let original = wide(path);
    let replacement = wide(temp);
    let saved = wide(backup);
    // SAFETY: live nul-terminated UTF-16 buffers; reserved arguments null; merge errors not ignored.
    let result = unsafe {
        ReplaceFileW(
            original.as_ptr(),
            replacement.as_ptr(),
            saved.as_ptr(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(path: &Path, temp: &Path, _backup: &Path) -> std::io::Result<()> {
    std::fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lossless_edits_and_undo_preserve_bom_tabs_and_crlf() {
        let original = "\u{feff}one\tline\r\nsecond\r\n";
        let mut doc = Document::new(original.into());
        assert!(!doc.dirty());
        doc.move_to(1, 6);
        doc.insert("!").unwrap();
        assert_eq!(doc.text, "\u{feff}one\tline\r\nsecond!\r\n");
        doc.undo(false);
        assert_eq!(doc.text, original);
        doc.undo(true);
        assert!(doc.dirty());
        doc.newline().unwrap();
        assert!(doc.text.contains("second!\r\n\r\n"));
        doc.delete(false).unwrap();
        doc.undo(false);
        assert_eq!(doc.line_count(), 4);
        assert!(visible_line(doc.line(0), 0, 40).contains("one    line"));
        doc.horizontal(false);
        doc.vertical(false);
        doc.line_end(true);
        assert!(doc.position().0 < doc.line_count());
    }
    #[test]
    fn undo_history_and_document_size_are_bounded() {
        let mut doc = Document::new(String::new());
        for _ in 0..2000 {
            doc.insert("x").unwrap();
        }
        assert!(doc.undo.len() <= HISTORY_STEPS && doc.history_bytes <= HISTORY_BYTES);
        let before = doc.text.clone();
        assert!(doc.insert(&"x".repeat(MAX_BYTES)).is_err());
        assert_eq!(doc.text, before);
        assert!(doc
            .insert(&"line\n".repeat(50_001))
            .unwrap_err()
            .contains("50,000"));
        assert_eq!(doc.text, before);
    }
    #[test]
    fn exact_byte_conflicts_and_readonly_files_cannot_be_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, "same\r\n").unwrap();
        assert!(save(&path, "same\n", "replacement")
            .unwrap_err()
            .contains("changed"));
        assert_eq!(std::fs::read(&path).unwrap(), b"same\r\n");
        save(&path, "same\r\n", "\u{feff}new\ttext\r\n").unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            "\u{feff}new\ttext\r\n".as_bytes()
        );
        let original_permissions = std::fs::metadata(&path).unwrap().permissions();
        let mut perms = original_permissions.clone();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms).unwrap();
        assert!(save(&path, "\u{feff}new\ttext\r\n", "another")
            .unwrap_err()
            .contains("read-only"));
        std::fs::set_permissions(&path, original_permissions).unwrap();
    }
}
