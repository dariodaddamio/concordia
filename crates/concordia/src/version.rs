//! Build identity so users can tell which binary a pane is running.

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Names the changelog section this build belongs to; shown in header + `--version`.
pub const BUILD_STAMP: &str = "2026-11-19a";
/// Commit the binary was built from, with `+HHMM` (UTC build time) when the tree had
/// uncommitted changes. Empty when the build had no git checkout. Written by `build.rs`.
pub const BUILD_REV: &str = env!("CONCORDIA_BUILD_REV");

pub fn version_line() -> String {
    if BUILD_REV.is_empty() {
        format!("concordia {VERSION} ({BUILD_STAMP})")
    } else {
        format!("concordia {VERSION} ({BUILD_STAMP}, {BUILD_REV})")
    }
}

/// The stamp for the window header. A build with uncommitted changes carries its build
/// time, so a rebuilt binary never looks identical to the one it replaced.
pub fn short_stamp() -> String {
    match BUILD_REV.split_once('+') {
        Some((_, time)) => format!("{BUILD_STAMP}+{time}"),
        None => BUILD_STAMP.to_string(),
    }
}
