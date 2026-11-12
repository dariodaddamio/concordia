//! Exact rules supplied by explicitly connected file-backed harness manifests.
#[cfg(test)]
use super::parse_manifest;
use super::{connections, Manifest};
use crate::store::ProviderKind;
#[cfg(test)]
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::Path};

pub(super) fn manifest_at(path: &Path, expected: &str) -> Result<Manifest, String> {
    let selection = path
        .to_str()
        .ok_or("pinned-decision manifest path is not UTF-8")?;
    let (manifest, digest, _) =
        super::command::load_local_manifest_for_action(selection, Path::new("."))?;
    if digest != expected {
        return Err("pinned-decision manifest changed; reconnect the harness before starting a new native session".into());
    }
    Ok(manifest)
}

pub fn for_new_session(
    root: &Path,
    provider: ProviderKind,
    home: Option<&Path>,
    project: &Path,
) -> Result<String, String> {
    let connections = connections::load(root)?;
    if !connections.iter().any(|connection| {
        connection.has_pinned_decisions
            && connection
                .entries
                .iter()
                .any(|entry| entry.connect_ok && entry.provider == provider.as_str())
    }) {
        return Ok(String::new());
    }
    let project = std::fs::canonicalize(project).map_err(|_| "project directory is unavailable")?;
    let home = home
        .map(std::fs::canonicalize)
        .transpose()
        .map_err(|_| "account home is unavailable")?;
    let mut decisions = Vec::new();
    let mut seen = HashSet::new();
    for connection in connections
        .iter()
        .filter(|connection| connection.has_pinned_decisions)
    {
        if !connection.entries.iter().any(|entry| {
            entry.connect_ok
                && entry.provider == provider.as_str()
                && std::fs::canonicalize(&entry.project).ok().as_ref() == Some(&project)
                && entry
                    .home
                    .as_ref()
                    .map(std::fs::canonicalize)
                    .transpose()
                    .ok()
                    == Some(home.clone())
        }) {
            continue;
        }
        let manifest = manifest_at(
            Path::new(&connection.manifest_path),
            &connection.manifest_sha256,
        )?;
        for decision in manifest.pinned_decisions {
            if seen.insert(decision.clone()) {
                decisions.push(decision);
            }
        }
    }
    if decisions.is_empty() {
        return Ok(String::new());
    }
    if decisions.iter().map(String::len).sum::<usize>() > 4096 {
        return Err(
            "combined pinned decisions exceed 4096 bytes; reduce the connected harness rules"
                .into(),
        );
    }
    Ok(format!(
        "[pinned decisions from connected harness]\n{}",
        decisions.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rules_are_delivered_verbatim_for_the_bound_target_and_changed_manifests_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let path = dir.path().join("harness.json");
        let text = r#"{"name":"fixture","providers":{"codex":{"connect":["fake-provider"]}},"pinned_decisions":["Keep this exact  spacing.","Preserve original logs."]}"#;
        std::fs::write(&path, text).unwrap();
        let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
        connections::record(
            dir.path(),
            &connections::Identity {
                name: "fixture",
                version: None,
                manifest_path: path.to_str().unwrap(),
                manifest_sha256: &digest,
                skill: None,
            },
            connections::Entry {
                provider: "codex".into(),
                home: Some(home.to_string_lossy().into_owned()),
                project: project.to_string_lossy().into_owned(),
                connect_ok: true,
                connect_note: String::new(),
                verify: "not-run".into(),
                verify_note: String::new(),
                at: 0,
            },
        )
        .unwrap();
        let prefix =
            for_new_session(dir.path(), ProviderKind::Codex, Some(&home), &project).unwrap();
        assert!(prefix.contains("Keep this exact  spacing.\nPreserve original logs."));
        assert!(
            for_new_session(dir.path(), ProviderKind::Claude, Some(&home), &project)
                .unwrap()
                .is_empty()
        );
        std::fs::write(&path, text.replace("Keep", "Change")).unwrap();
        assert!(
            for_new_session(dir.path(), ProviderKind::Codex, Some(&home), &project)
                .unwrap_err()
                .contains("changed")
        );
        assert!(parse_manifest(
            r#"{"name":"fixture","providers":{"codex":{"connect":["fake-provider"]}},"pinned_decisions":[7]}"#,
            dir.path()
        )
        .is_err());
    }
}

