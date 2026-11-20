//! Reconcile explicitly shared project setup before an account's first native turn.
use super::{command, connections, Context, RunOutcome};
use crate::store::ProviderKind;
use std::path::Path;

pub fn needs_setup(
    store: &Path,
    provider: ProviderKind,
    home: Option<&Path>,
    project: &Path,
) -> Result<bool, String> {
    let project_key = connections::path_key(project);
    let home_key = home.map(connections::path_key);
    Ok(connections::load(store)?.iter().any(|connection| {
        connection
            .all_account_projects
            .iter()
            .any(|path| connections::path_key(Path::new(path)) == project_key)
            && !connection.entries.iter().any(|entry| {
                entry.provider == provider.as_str()
                    && entry
                        .home
                        .as_deref()
                        .map(Path::new)
                        .map(connections::path_key)
                        == home_key
                    && connections::path_key(Path::new(&entry.project)) == project_key
            })
    }))
}

pub fn reconcile_account(
    store: &Path,
    provider: ProviderKind,
    home: Option<&Path>,
    project: &Path,
    run: &dyn Fn(&[String]) -> RunOutcome,
) -> Result<Vec<String>, String> {
    let project_key = connections::path_key(project);
    let home_key = home.map(connections::path_key);
    let mut lines = Vec::new();
    for connection in connections::load(store)? {
        if !connection
            .all_account_projects
            .iter()
            .any(|path| connections::path_key(Path::new(path)) == project_key)
        {
            continue;
        }
        if let Some(entry) = connection.entries.iter().find(|entry| {
            entry.provider == provider.as_str()
                && entry
                    .home
                    .as_deref()
                    .map(Path::new)
                    .map(connections::path_key)
                    == home_key
                && connections::path_key(Path::new(&entry.project)) == project_key
        }) {
            if !entry.connect_ok {
                lines.push(format!(
                    "{} setup failed previously; /harness connect to retry",
                    connection.name
                ));
            }
            continue;
        }
        let loaded = command::reload(&connection.manifest_path, project, run)?;
        if loaded.sha != connection.manifest_sha256 || loaded.manifest.name != connection.name {
            return Err(format!(
                "{} setup changed; /harness connect to review the new source",
                connection.name
            ));
        }
        if !loaded.manifest.providers.contains_key(provider.as_str()) {
            lines.push(format!(
                "{} does not support {}",
                connection.name,
                provider.as_str()
            ));
            continue;
        }
        let entry = command::connect_in(
            store,
            &loaded.manifest,
            Path::new(&loaded.path),
            &loaded.sha,
            &Context {
                provider,
                home,
                project,
            },
            run,
        );
        lines.push(if entry.connect_ok {
            format!(
                "{} shared setup connected for {}",
                connection.name,
                provider.as_str()
            )
        } else {
            format!(
                "{} shared setup failed: {}",
                connection.name, entry.connect_note
            )
        });
    }
    Ok(lines)
}
