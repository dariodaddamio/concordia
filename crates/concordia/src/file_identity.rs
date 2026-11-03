//! Platform file identity, shared by native sources and workspace bindings.
use std::fs::File;

#[cfg(windows)]
pub(crate) fn of(file: &File) -> Result<String, String> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    #[derive(Default)]
    struct Time {
        low: u32,
        high: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Info {
        attributes: u32,
        created: Time,
        accessed: Time,
        written: Time,
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(handle: *mut std::ffi::c_void, info: *mut Info) -> i32;
    }
    let mut info = Info::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err("native file identity is unavailable".into());
    }
    Ok(format!(
        "{}:{}:{}:{}:{}",
        info.volume, info.index_high, info.index_low, info.created.high, info.created.low
    ))
}
#[cfg(unix)]
pub(crate) fn of(file: &File) -> Result<String, String> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    Ok(format!("{}:{}", meta.dev(), meta.ino()))
}
#[cfg(not(any(windows, unix)))]
pub(crate) fn of(_: &File) -> Result<String, String> {
    Err("native file identity is unsupported on this host".into())
}
