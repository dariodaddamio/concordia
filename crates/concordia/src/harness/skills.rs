//! Native skill directories point to one user-selected source; no copied skill trees.
use super::connections;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
fn junction_argument(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{unc}"))
    } else {
        PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
    }
}

fn bundles(source: &Path) -> Result<Vec<PathBuf>, String> {
    let source = std::fs::canonicalize(source)
        .map_err(|error| format!("skill source unavailable: {error}"))?;
    if source.join("SKILL.md").is_file() {
        return Ok(vec![source]);
    }
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if found.len() >= 256 {
            return Err("skill source exceeds 256 bundles".into());
        }
        if entry.path().join("SKILL.md").is_file() {
            found.push(entry.path());
        }
    }
    if found.is_empty() {
        return Err("no SKILL.md bundles found in that source".into());
    }
    found.sort();
    Ok(found)
}

pub fn validate_source(source: &Path) -> Result<PathBuf, String> {
    bundles(source)?;
    std::fs::canonicalize(source).map_err(|error| error.to_string())
}

fn destination(home: &Path, bundle: &Path) -> Result<PathBuf, String> {
    let name = bundle
        .file_name()
        .ok_or("skill bundle has no folder name")?;
    Ok(home.join("skills").join(name))
}

pub fn needs_setup(store: &Path, home: Option<&Path>) -> Result<bool, String> {
    let Some(home) = home else { return Ok(false) };
    for source in connections::shared_skill_sources(store)? {
        for bundle in bundles(Path::new(&source))? {
            // Existing user-owned bundles are preserved, including name collisions.
            if std::fs::symlink_metadata(destination(home, &bundle)?).is_err() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn install(source: &Path, home: &Path, cancel: &AtomicBool) -> Result<Vec<String>, String> {
    let bundles = bundles(source)?;
    let skills = home.join("skills");
    if std::fs::symlink_metadata(&skills)
        .is_ok_and(|meta| crate::workspaces::linked(&meta) || !meta.is_dir())
    {
        return Err("skills destination must be an ordinary directory".into());
    }
    std::fs::create_dir_all(&skills).map_err(|error| error.to_string())?;
    let mut lines = Vec::new();
    for bundle in bundles {
        if cancel.load(Ordering::SeqCst) {
            return Err("skill setup cancelled; inspect existing links before retrying".into());
        }
        let link = destination(home, &bundle)?;
        let current_source = std::fs::canonicalize(&bundle).map_err(|error| error.to_string())?;
        if std::fs::symlink_metadata(&link).is_ok() {
            if std::fs::canonicalize(&link).ok().as_ref() != Some(&current_source) {
                lines.push(format!(
                    "kept existing skill {}; name already belongs to another source",
                    link.display()
                ));
            }
            continue;
        }
        #[cfg(windows)]
        {
            let mut command = crate::process_ui::command("powershell.exe");
            command.args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:CONCORDIA_SKILL_LINK -Target $env:CONCORDIA_SKILL_SOURCE | Out-Null"])
                .env("CONCORDIA_SKILL_LINK", junction_argument(&link)).env("CONCORDIA_SKILL_SOURCE", junction_argument(&bundle));
            let output = crate::process_lifetime::capture_cancellable(
                command,
                super::run::DEFAULT_TIMEOUT,
                8192,
                cancel,
            )?;
            if !output.status.success() {
                return Err(format!(
                    "could not link skill {}: {}",
                    link.display(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&bundle, &link).map_err(|error| error.to_string())?;
        if std::fs::canonicalize(&link).ok().as_ref() != Some(&current_source) {
            return Err(format!(
                "skill link does not resolve to its source: {}",
                link.display()
            ));
        }
        lines.push(format!("linked skill {}", link.display()));
    }
    if lines.is_empty() {
        lines.push("skill sources already connected".into());
    }
    Ok(lines)
}

pub fn reconcile(
    store: &Path,
    home: Option<&Path>,
    cancel: &AtomicBool,
) -> Result<Vec<String>, String> {
    let Some(home) = home else {
        return Ok(Vec::new());
    };
    let mut lines = Vec::new();
    for source in connections::shared_skill_sources(store)? {
        lines.extend(install(Path::new(&source), home, cancel)?);
    }
    Ok(lines)
}

pub fn remove(source: &Path, home: &Path) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    for bundle in bundles(source)? {
        let link = destination(home, &bundle)?;
        let target = std::fs::canonicalize(bundle).map_err(|error| error.to_string())?;
        if std::fs::symlink_metadata(&link).is_ok_and(|meta| crate::workspaces::linked(&meta))
            && std::fs::canonicalize(&link).ok().as_ref() == Some(&target)
        {
            #[cfg(windows)]
            std::fs::remove_dir(&link).map_err(|error| error.to_string())?;
            #[cfg(unix)]
            std::fs::remove_file(&link).map_err(|error| error.to_string())?;
            lines.push(format!("removed skill link {}", link.display()));
        }
    }
    Ok(lines)
}
