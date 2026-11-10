//! Filesystem checks used by update, release, and native-state readers.
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn read_text_bounded(path: &Path, limit: usize) -> Option<String> {
    use std::io::Read;
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > limit {
        return None;
    }
    String::from_utf8(bytes).ok()
}

pub(crate) fn regular_owned_file(path: &Path, root: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    let linked = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        linked || metadata.file_attributes() & 0x400 != 0
    };
    metadata.is_file()
        && !linked
        && fs::canonicalize(path)
            .ok()
            .is_some_and(|path| path.parent() == Some(root))
}

pub(crate) fn resolved_directory(path: &Path) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(path).ok()?;
    let linked = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        linked || metadata.file_attributes() & 0x400 != 0
    };
    (metadata.is_dir() && !linked)
        .then(|| fs::canonicalize(path).ok())
        .flatten()
}

pub fn resolve_active_exe() -> PathBuf {
    let bin = std::env::var("CONCORDIA_INSTALL_DIR")
        .ok()
        .filter(|dir| !dir.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("concordia")
                .join("bin")
        });
    if let Some(raw) = read_text_bounded(&bin.join("current"), 4096) {
        let p = PathBuf::from(raw.trim());
        if p.is_file() {
            return p;
        }
    }
    if let Ok(raw) = std::env::var("CONCORDIA_EXE") {
        let p = PathBuf::from(raw);
        if p.is_file() {
            return p;
        }
    }
    let primary = bin.join("concordia.exe");
    if primary.is_file() {
        primary
    } else {
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("concordia.exe"))
    }
}
