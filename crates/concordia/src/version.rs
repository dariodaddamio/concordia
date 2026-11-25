pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_STAMP: &str = "2026-11-25c";
pub const BUILD_REV: &str = env!("CONCORDIA_BUILD_REV");

pub fn version_line() -> String {
    if BUILD_REV.is_empty() {
        format!("concordia {VERSION} ({BUILD_STAMP})")
    } else {
        format!("concordia {VERSION} ({BUILD_STAMP}, {BUILD_REV})")
    }
}

pub fn short_stamp() -> String {
    match BUILD_REV.split_once('+') {
        Some((_, time)) => format!("{BUILD_STAMP}+{time}"),
        None => BUILD_STAMP.to_string(),
    }
}
