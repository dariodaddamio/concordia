//! Bypass only a verified stock npm bridge, using its own unmodified native executable.
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

const BRIDGE_HASH: &str = "61b0194f3bb6534439c8d26a3ed57d0805f84b884588b761795323eeb92fcf70";
const SHIM: &str = "@ECHO off\nGOTO start\n:find_dp0\nSET dp0=%~dp0\nEXIT /b\n:start\nSETLOCAL\nCALL :find_dp0\n\nIF EXIST \"%dp0%\\node.exe\" (\n  SET \"_prog=%dp0%\\node.exe\"\n) ELSE (\n  SET \"_prog=node\"\n  SET PATHEXT=%PATHEXT:;.JS;=;%\n)\n\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*";

pub(crate) struct Native {
    pub program: PathBuf,
    pub env: Vec<(String, String)>,
}
fn read(path: &Path) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= 128 * 1024).then_some(bytes)
}
pub(crate) fn codex(program: &str) -> Option<Native> {
    if !cfg!(windows) {
        return None;
    }
    resolve(Path::new(program), BRIDGE_HASH)
}
fn resolve(shim: &Path, expected_hash: &str) -> Option<Native> {
    if !shim.file_name()?.eq_ignore_ascii_case("codex.cmd") {
        return None;
    }
    let text = String::from_utf8(read(shim)?).ok()?.replace('\r', "");
    if text.trim_end() != SHIM {
        return None;
    }
    let package = shim.parent()?.join("node_modules/@openai/codex");
    let manifest: serde_json::Value =
        serde_json::from_slice(&read(&package.join("package.json"))?).ok()?;
    if manifest["name"] != "@openai/codex" || manifest["bin"]["codex"] != "bin/codex.js" {
        return None;
    }
    let bridge = read(&package.join("bin/codex.js"))?;
    if format!("{:x}", Sha256::digest(&bridge)) != expected_hash {
        return None;
    }
    let (platform, triple) = if cfg!(target_arch = "aarch64") {
        ("codex-win32-arm64", "aarch64-pc-windows-msvc")
    } else {
        ("codex-win32-x64", "x86_64-pc-windows-msvc")
    };
    let vendor = [
        package.join(format!("node_modules/@openai/{platform}/vendor")),
        shim.parent()?
            .join(format!("node_modules/@openai/{platform}/vendor")),
        package.join("vendor"),
    ];
    let binary = vendor
        .iter()
        .map(|v| v.join(triple).join("bin/codex.exe"))
        .find(|p| p.is_file())?;
    Some(Native {
        program: binary,
        env: vec![
            (
                "CODEX_MANAGED_PACKAGE_ROOT".into(),
                package.to_string_lossy().into_owned(),
            ),
            ("CODEX_MANAGED_BY_NPM".into(), "1".into()),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stock_shim_resolves_native_but_custom_bridges_and_missing_binaries_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("codex.cmd");
        let package = dir.path().join("node_modules/@openai/codex");
        std::fs::create_dir_all(package.join("bin")).unwrap();
        let platform = if cfg!(target_arch = "aarch64") {
            "codex-win32-arm64/vendor/aarch64-pc-windows-msvc"
        } else {
            "codex-win32-x64/vendor/x86_64-pc-windows-msvc"
        };
        let binary = package.join(format!("node_modules/@openai/{platform}/bin/codex.exe"));
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&shim, SHIM.replace('\n', "\r\n")).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@openai/codex","bin":{"codex":"bin/codex.js"}}"#,
        )
        .unwrap();
        std::fs::write(package.join("bin/codex.js"), "synthetic bridge").unwrap();
        let hash = format!("{:x}", Sha256::digest(b"synthetic bridge"));
        assert!(resolve(&shim, &hash).is_none());
        std::fs::write(&binary, "synthetic executable marker").unwrap();
        assert_eq!(resolve(&shim, &hash).unwrap().program, binary);
        assert!(resolve(&shim, "different bridge hash").is_none());
        std::fs::write(&shim, format!("{SHIM}\necho custom launch step")).unwrap();
        assert!(resolve(&shim, &hash).is_none());
    }
}
