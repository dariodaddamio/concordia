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

/// Run the declared entrypoint of a stock npm shim without cmd.exe. Unlike bypassing the
/// bridge, this preserves the installed JavaScript's behavior across package updates.
pub(crate) fn node_bridge(
    program: &str,
    provider: crate::store::ProviderKind,
) -> Option<(PathBuf, PathBuf)> {
    if !cfg!(windows) {
        return None;
    }
    let shim = Path::new(program);
    let (name, package_name, entrypoint) = match provider {
        crate::store::ProviderKind::Codex => ("codex", "@openai/codex", "bin/codex.js"),
        crate::store::ProviderKind::Claude => ("claude", "@anthropic-ai/claude-code", "cli.js"),
        crate::store::ProviderKind::Cursor => return None,
    };
    if !shim
        .file_name()?
        .eq_ignore_ascii_case(format!("{name}.cmd"))
    {
        return None;
    }
    let declared = format!(
        "node_modules\\{}\\{}",
        package_name.replace('/', "\\"),
        entrypoint.replace('/', "\\")
    );
    let expected = SHIM.replace("node_modules\\@openai\\codex\\bin\\codex.js", &declared);
    let text = String::from_utf8(read(shim)?).ok()?.replace('\r', "");
    if text.trim_end() != expected {
        return None;
    }
    let package = shim.parent()?.join("node_modules").join(package_name);
    let manifest: serde_json::Value =
        serde_json::from_slice(&read(&package.join("package.json"))?).ok()?;
    if manifest["name"] != package_name || manifest["bin"][name] != entrypoint {
        return None;
    }
    let script = package.join(entrypoint);
    if !script.is_file() {
        return None;
    }
    let local_node = shim.parent()?.join("node.exe");
    let node = if local_node.is_file() {
        local_node
    } else {
        which::which("node.exe").ok()?
    };
    if !node.extension()?.eq_ignore_ascii_case("exe") {
        return None;
    }
    Some((node, script))
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
        if cfg!(windows) {
            // A newer bridge is still run verbatim through native Node, never bypassed.
            let node = dir.path().join("node.exe");
            std::fs::write(&node, "synthetic node marker").unwrap();
            let (resolved_node, script) =
                node_bridge(&shim.to_string_lossy(), crate::store::ProviderKind::Codex).unwrap();
            assert_eq!(resolved_node, node);
            assert_eq!(script, package.join("bin/codex.js"));
            let prompt = "image context % & \"quoted\"\nUnicode λ";
            let plan = crate::provider::turn::PlannedTurn {
                command: shim.to_string_lossy().into_owned(),
                argv: vec!["exec".into(), "--json".into(), prompt.into()],
                prompt: prompt.into(),
                new_session_id: None,
                env: vec![],
                cwd: ".".into(),
                provider: crate::store::ProviderKind::Codex,
            };
            let launch = crate::provider::launch::prepare(&plan).unwrap();
            assert_eq!(launch.program, node.to_string_lossy());
            assert_eq!(launch.args[0], script.to_string_lossy());
            assert_eq!(&launch.args[1..], ["exec", "--json", "-"]);
            assert_eq!(launch.stdin.as_deref(), Some(prompt));
            // A package claiming a different entrypoint cannot redirect this resolution.
            std::fs::write(
                package.join("package.json"),
                r#"{"name":"@openai/codex","bin":{"codex":"other.js"}}"#,
            )
            .unwrap();
            assert!(
                node_bridge(&shim.to_string_lossy(), crate::store::ProviderKind::Codex).is_none()
            );
            std::fs::write(
                package.join("package.json"),
                r#"{"name":"@openai/codex","bin":{"codex":"bin/codex.js"}}"#,
            )
            .unwrap();
        }
        std::fs::write(&shim, format!("{SHIM}\necho custom launch step")).unwrap();
        assert!(resolve(&shim, &hash).is_none());
        assert!(node_bridge(&shim.to_string_lossy(), crate::store::ProviderKind::Codex).is_none());
    }
}
