//! Read-only discovery over existing Harness receipts and Concordia-owned MCP registrations.
use super::{connections, parse_manifest};
use crate::{store::ProviderKind, update::PreviewLine};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Serialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
}
#[derive(Debug, Serialize)]
pub struct Integration {
    pub id: String,
    pub name: String,
    pub source: String,
    pub revision: String,
    pub status: String,
    pub bindings: Vec<String>,
    pub tools: Vec<Tool>,
    pub omitted_bindings: usize,
}
#[derive(Debug, Serialize)]
pub struct Catalog {
    pub integrations: Vec<Integration>,
    pub native_registration: Vec<String>,
    pub incomplete: bool,
}

/// Capture a catalog row's exact mutation identity. The caller must display and confirm the
/// returned target before passing it to a reviewed command; no adapter is executed here.
pub fn action_target(
    root: &Path,
    id: &str,
    provider: &str,
    project: &Path,
    home: Option<&Path>,
) -> Result<connections::ActionTarget, String> {
    let all = connections::load(root)?;
    let connection = all
        .into_iter()
        .find(|connection| {
            format!(
                "harness-{}",
                &hash(format!("{}\n{}", connection.name, connection.manifest_path).as_bytes())
                    [..16]
            ) == id
        })
        .ok_or("unknown integration id")?;
    let home = home.map(|path| path.to_string_lossy().into_owned());
    let project = project.to_string_lossy().into_owned();
    let entry = connection
        .entries
        .iter()
        .find(|entry| entry.provider == provider && entry.project == project && entry.home == home)
        .ok_or("integration binding is not connected for this provider/project/home")?;
    Ok(connections::ActionTarget::from_connection(
        &connection,
        entry,
    ))
}
fn display(value: &str, max: usize) -> String {
    crate::chat_cli::terminal_safe(value)
        .chars()
        .take(max)
        .collect()
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn manifest(path: &Path, revision: &str) -> Result<super::Manifest, String> {
    if crate::workspace_paths::sensitive(path) {
        return Err("manifest path excluded by workspace policy".into());
    }
    let canonical = path.canonicalize().map_err(|_| "manifest unavailable")?;
    if crate::workspace_paths::sensitive(&canonical) {
        return Err("canonical manifest excluded by workspace policy".into());
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "manifest metadata unavailable")?;
    if !metadata.is_file() || crate::workspaces::linked(&metadata) || metadata.len() > 256 * 1024 {
        return Err("manifest must be a regular file of at most 256 KiB".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&canonical)
        .map_err(|_| "manifest unavailable")?
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "manifest read failed")?;
    if bytes.len() > 256 * 1024
        || hash(&bytes) != revision
        || path.canonicalize().ok().as_ref() != Some(&canonical)
    {
        return Err(
            "manifest revision changed; reconnect explicitly before trusting its tool list".into(),
        );
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "manifest is not UTF-8")?;
    let parsed = parse_manifest(text, canonical.parent().ok_or("manifest has no parent")?)?;
    if parsed.tools.len() > 64 {
        return Err("manifest exceeds the 64-tool discovery budget".into());
    }
    Ok(parsed)
}

pub fn inspect(root: &Path, cancel: &AtomicBool) -> Result<Catalog, String> {
    let all = connections::load(root)?;
    let mut result = Catalog {
        incomplete: all.len() > 16,
        integrations: Vec::new(),
        native_registration: Vec::new(),
    };
    for connection in all.into_iter().take(16) {
        if cancel.load(Ordering::Relaxed) {
            return Err("integration inspection cancelled".into());
        }
        let id = format!(
            "harness-{}",
            &hash(format!("{}\n{}", connection.name, connection.manifest_path).as_bytes())[..16]
        );
        let command_backed = connection.manifest_path.starts_with("command:");
        let inspected = if command_backed {
            Err(
                "command-backed manifest not refreshed; inspection never executes adapter commands"
                    .into(),
            )
        } else {
            manifest(
                Path::new(&connection.manifest_path),
                &connection.manifest_sha256,
            )
        };
        let (status, tools) = match inspected {
            Ok(manifest) if manifest.name == connection.name => (
                "manifest revision verified; connection verification remains per binding".into(),
                manifest
                    .tools
                    .into_iter()
                    .map(|tool| Tool {
                        name: format!("{id}::{}", display(&tool.name, 120)),
                        description: display(&tool.description, 240),
                    })
                    .collect(),
            ),
            Ok(_) => (
                "manifest identity differs from the saved connection; reconnect explicitly".into(),
                Vec::new(),
            ),
            Err(error) => (display(&error, 300), Vec::new()),
        };
        let omitted_bindings = connection.entries.len().saturating_sub(16);
        let bindings = connection
            .entries
            .into_iter()
            .take(16)
            .map(|entry| {
                format!(
                    "{} · home {} · project {} · connect {} · recorded verification {} at {}",
                    display(&entry.provider, 32),
                    display(
                        entry
                            .home
                            .as_deref()
                            .unwrap_or("native default; not an owned home"),
                        300
                    ),
                    display(&entry.project, 500),
                    if entry.connect_ok {
                        "recorded successful"
                    } else {
                        "failed/unconfirmed"
                    },
                    display(&entry.verify, 32),
                    entry.at
                )
            })
            .collect();
        result.incomplete |= omitted_bindings > 0;
        result.integrations.push(Integration {
            id,
            name: display(&connection.name, 120),
            source: if command_backed {
                "command-backed manifest (not executed)".into()
            } else {
                display(&connection.manifest_path, 1024)
            },
            revision: display(&connection.manifest_sha256, 64),
            status,
            bindings,
            tools,
            omitted_bindings,
        });
    }
    let homes = crate::integration::created_homes(root);
    result.incomplete |= homes.len() > 16;
    let want = crate::config::for_root(root).threads.mcp;
    let exe = crate::update::resolve_active_exe();
    for (account, provider, home) in homes.into_iter().take(16) {
        if cancel.load(Ordering::Relaxed) {
            return Err("integration inspection cancelled".into());
        }
        let configuration = home.join(if provider == ProviderKind::Claude {
            ".claude.json"
        } else {
            "config.toml"
        });
        let bounded = match std::fs::symlink_metadata(&configuration) {
            Ok(metadata) => {
                metadata.is_file()
                    && !crate::workspaces::linked(&metadata)
                    && metadata.len() <= 1024 * 1024
            }
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        };
        let status = if !bounded {
            "registration unknown: native configuration is linked, unavailable or exceeds 1 MiB"
                .into()
        } else {
            match crate::integration::sync_mcp(&home, provider, want, &exe, root, false) {
                crate::integration::Outcome::Current => {
                    "managed registration matches the configured preference".into()
                }
                crate::integration::Outcome::Absent => "managed registration absent".into(),
                crate::integration::Outcome::Updated => {
                    "registration needs an explicit native setup refresh".into()
                }
                crate::integration::Outcome::Removed => {
                    "registration present while preference is off; explicit refresh can remove it"
                        .into()
                }
                crate::integration::Outcome::Blocked(reason) => {
                    format!("registration unverified: {}", display(&reason, 240))
                }
            }
        };
        result.native_registration.push(format!(
            "{} / {} · {} · {}",
            provider.as_str(),
            display(&account, 120),
            display(&home.to_string_lossy(), 500),
            status
        ));
    }
    if result.native_registration.is_empty() {
        result.native_registration.push("No Concordia-owned native homes to inspect. User-owned homes and credentials are not inspected.".into());
    }
    Ok(result)
}

pub fn rows(catalog: &Catalog) -> Vec<PreviewLine> {
    let mut rows = vec![PreviewLine::Title("Integrations and agent tools".into()), PreviewLine::Text("Descriptions are untrusted manifest data. Discovery grants no permissions and runs no integration commands. Native provider approvals remain authoritative.".into())];
    if catalog.integrations.is_empty() {
        rows.push(PreviewLine::Text(
            "No Harness integrations connected.".into(),
        ));
    }
    for integration in &catalog.integrations {
        rows.push(PreviewLine::Blank);
        rows.push(PreviewLine::Kind(format!(
            "{} · {}",
            integration.name, integration.id
        )));
        rows.push(PreviewLine::Field("Source", integration.source.clone()));
        rows.push(PreviewLine::Field("Revision", integration.revision.clone()));
        rows.push(PreviewLine::Text(integration.status.clone()));
        for binding in &integration.bindings {
            rows.push(PreviewLine::Text(binding.clone()));
        }
        for tool in &integration.tools {
            rows.push(PreviewLine::Bullet(format!(
                "{} — {}",
                tool.name, tool.description
            )));
        }
        rows.push(PreviewLine::Hint("Explicit Harness connect / verify / disconnect actions remain available through /harness. Catalog discovery does not launch them.".into()));
    }
    rows.push(PreviewLine::Blank);
    rows.push(PreviewLine::Kind("Concordia MCP registration".into()));
    rows.extend(
        catalog
            .native_registration
            .iter()
            .cloned()
            .map(PreviewLine::Text),
    );
    if catalog.incomplete {
        rows.push(PreviewLine::Text("Incomplete catalog: showing at most 16 integrations, 16 bindings each and 16 native homes.".into()));
    }
    rows.push(PreviewLine::Hint("R refresh · Esc close".into()));
    rows
}

/// Minimal summaries keep discovery cheap; individual tool pages are requested explicitly.
pub fn tool(
    root: &Path,
    id: Option<&str>,
    offset: usize,
    revision: Option<&str>,
) -> Result<String, String> {
    let catalog = inspect(root, &AtomicBool::new(false))?;
    let value = if let Some(id) = id {
        let integration = catalog
            .integrations
            .iter()
            .find(|integration| integration.id == id)
            .ok_or("unknown integration id")?;
        if offset > integration.tools.len()
            || (offset > 0 && revision != Some(integration.revision.as_str()))
            || revision.is_some_and(|revision| revision != integration.revision)
        {
            return Err(
                "integration page revision changed or cursor invalid; inspect from offset zero"
                    .into(),
            );
        }
        serde_json::json!({"integration":id,"source":integration.source,"revision":integration.revision,"status":integration.status,"bindings":integration.bindings.iter().take(4).collect::<Vec<_>>(),"omitted_bindings":integration.omitted_bindings+integration.bindings.len().saturating_sub(4),"tools":integration.tools.iter().skip(offset).take(8).collect::<Vec<_>>(),"next_offset":(offset+8<integration.tools.len()).then_some(offset+8),"actions":"explicit Harness connect/verify/disconnect; discovery runs none"})
    } else {
        if offset != 0 || revision.is_some() {
            return Err("integration listing does not take a tool cursor".into());
        }
        serde_json::json!({"integrations":catalog.integrations.iter().map(|integration| serde_json::json!({"id":integration.id,"name":integration.name,"revision":integration.revision,"status":integration.status,"tools":integration.tools.len()})).collect::<Vec<_>>(),"incomplete":catalog.incomplete,"native_registration":catalog.native_registration.iter().take(8).collect::<Vec<_>>(),"omitted_native_homes":catalog.native_registration.len().saturating_sub(8)})
    };
    let text =
        serde_json::to_string(&value).map_err(|_| "integration result serialization failed")?;
    if text.chars().count() > crate::mcp::MAX_RESULT_CHARS {
        return Err("integration result exceeds the bounded display budget".into());
    }
    Ok(text)
}
