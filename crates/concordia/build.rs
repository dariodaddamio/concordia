//! Embed the same Concordia mark used by the owned window in the Windows executable.
//! The resource compiler ships with the Windows SDK; no build crate is required.
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn resource_compiler() -> Option<PathBuf> {
    if let Some(explicit) = env::var_os("RC") {
        let path = PathBuf::from(explicit);
        return path.is_file().then_some(path);
    }
    if let Some(path) = env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|path| path.join("rc.exe"))
            .find(|path| path.is_file())
    }) {
        return Some(path);
    }
    let root = PathBuf::from(env::var_os("ProgramFiles(x86)")?).join("Windows Kits/10/bin");
    let mut versions: Vec<_> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    versions.sort_by_key(|path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .split('.')
            .map(|part| part.parse::<u32>().unwrap_or_default())
            .collect::<Vec<_>>()
    });
    let host = env::var("HOST").unwrap_or_default();
    let architecture = if host.starts_with("aarch64") {
        "arm64"
    } else if host.starts_with("i686") {
        "x86"
    } else {
        "x64"
    };
    versions
        .into_iter()
        .rev()
        .flat_map(|version| {
            [
                version.join(architecture).join("rc.exe"),
                version.join("x64/rc.exe"),
            ]
        })
        .find(|path| path.is_file())
}

fn main() {
    println!("cargo:rerun-if-changed=assets/concordia.ico");
    println!("cargo:rerun-if-changed=assets/concordia.manifest");
    println!("cargo:rerun-if-env-changed=RC");
    println!("cargo:rerun-if-env-changed=ProgramFiles(x86)");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"));
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"))
        .join("concordia.res");
    let version = env::var("CARGO_PKG_VERSION").expect("package version");
    let version_parts = version
        .split('.')
        .map(|part| {
            part.split('-')
                .next()
                .unwrap_or(part)
                .parse::<u16>()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    let (major, minor, patch) = (
        *version_parts.first().unwrap_or(&0),
        *version_parts.get(1).unwrap_or(&0),
        *version_parts.get(2).unwrap_or(&0),
    );
    let rc_source = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"))
        .join("concordia-generated.rc");
    let rc = format!(
        r#"1 ICON "{icon}"
1 RT_MANIFEST "{manifest}"
1 VERSIONINFO
 FILEVERSION {major},{minor},{patch},0
 PRODUCTVERSION {major},{minor},{patch},0
 FILEFLAGSMASK 0x3fL
 FILEFLAGS 0
 FILEOS 0x40004L
 FILETYPE 0x1L
 BEGIN
  BLOCK "StringFileInfo"
  BEGIN
   BLOCK "040904B0"
   BEGIN
    VALUE "CompanyName", "Concordia contributors\0"
    VALUE "FileDescription", "Local provider orchestration TUI\0"
    VALUE "FileVersion", "{version}\0"
    VALUE "InternalName", "concordia.exe\0"
    VALUE "OriginalFilename", "concordia.exe\0"
    VALUE "ProductName", "Concordia\0"
    VALUE "ProductVersion", "{version}\0"
   END
  END
  BLOCK "VarFileInfo"
  BEGIN
   VALUE "Translation", 0x409, 1200
  END
 END
"#,
        manifest = manifest_path(&manifest)
            .display()
            .to_string()
            .replace('\\', "/"),
        icon = manifest
            .join("assets/concordia.ico")
            .display()
            .to_string()
            .replace('\\', "/"),
    );
    fs::write(&rc_source, rc).expect("write generated Windows resource");
    let compiler = resource_compiler().expect("Windows SDK rc.exe is required to embed Concordia resources; install the Windows SDK or set RC to its resource compiler");
    let mut command = Command::new(compiler);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let status = command
        .current_dir(manifest.join("assets"))
        .args([
            OsString::from("/nologo"),
            OsString::from("/fo"),
            output.as_os_str().to_owned(),
            rc_source.as_os_str().to_owned(),
        ])
        .status()
        .expect("start Windows SDK resource compiler");
    assert!(status.success(), "Windows SDK resource compiler failed");
    println!("cargo:rustc-link-arg-bin=concordia={}", output.display());
}

fn manifest_path(manifest: &Path) -> PathBuf {
    manifest.join("assets/concordia.manifest")
}
