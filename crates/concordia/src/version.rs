//! Build identity so users can tell which binary a pane is running.

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Bumped when everyday behavior changes; shown in header + `--version`.
pub const BUILD_STAMP: &str = "2026-11-02j";

pub fn version_line() -> String {
    format!("concordia {VERSION} ({BUILD_STAMP})")
}

pub fn short_stamp() -> &'static str {
    BUILD_STAMP
}
