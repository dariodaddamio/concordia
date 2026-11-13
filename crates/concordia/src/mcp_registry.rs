//! Concordia-owned MCP registry and per-launch projections.
//!
//! The registry is state owned by Concordia. Provider homes are never written. Secret-bearing
//! values are references, not values. Launch projections resolve environment references only in
//! memory and omit unresolved references so a missing sign-in is visible instead of leaking data.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub servers: BTreeMap<String, Server>,
    #[serde(default)]
    pub projects: BTreeMap<String, BTreeMap<String, bool>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    #[serde(default = "default_on")]
    pub enabled: bool,
    pub transport: Transport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Transport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, SecretRef>,
    },
    Remote {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, SecretRef>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SecretRef {
    Env { env: String },
    Credential { credential: String },
}

fn default_on() -> bool { true }

pub fn path(state_root: &Path) -> PathBuf { state_root.join("mcp.toml") }

pub fn load(state_root: &Path) -> Result<Registry, String> {
    let file = path(state_root);
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Registry { servers: BTreeMap::new(), projects: BTreeMap::new() }),
        Err(e) => return Err(format!("cannot read {}: {e}", file.display())),
    };
    if text.len() as u64 > MAX_BYTES { return Err("MCP registry exceeds the 1 MiB limit".into()); }
    toml::from_str(&text).map_err(|e| format!("MCP registry is invalid: {e}"))
}

pub fn save(state_root: &Path, registry: &Registry) -> Result<(), String> {
    let text = toml::to_string_pretty(registry).map_err(|e| format!("cannot encode MCP registry: {e}"))?;
    if text.len() as u64 > MAX_BYTES { return Err("MCP registry exceeds the 1 MiB limit".into()); }
    std::fs::create_dir_all(state_root).map_err(|e| format!("cannot create state directory: {e}"))?;
    let file = path(state_root);
    let temp = file.with_extension("toml.tmp");
    std::fs::write(&temp, text).map_err(|e| format!("cannot write MCP registry: {e}"))?;
    std::fs::rename(&temp, &file).map_err(|e| format!("cannot replace MCP registry: {e}"))
}

pub fn effective<'a>(registry: &'a Registry, project: &Path, name: &str) -> Option<&'a Server> {
    let server = registry.servers.get(name)?;
    let project_key = project.to_string_lossy();
    if registry.projects.get(project_key.as_ref()).and_then(|p| p.get(name)).is_some_and(|on| !on) { return None; }
    server.enabled.then_some(server)
}

fn resolve(reference: &SecretRef) -> Option<String> {
    match reference {
        SecretRef::Env { env } => std::env::var(env).ok().filter(|v| !v.is_empty()),
        SecretRef::Credential { .. } => None,
    }
}

pub fn acp_servers(state_root: &Path, project: &Path) -> Value {
    let Ok(registry) = load(state_root) else { return json!([]) };
    let mut out = Vec::new();
    for (name, _server) in &registry.servers {
        let Some(server) = effective(&registry, project, name) else { continue };
        match &server.transport {
            Transport::Stdio { command, args, env } => {
                let env = env.iter().filter_map(|(name, reference)| resolve(reference).map(|value| json!({"name": name, "value": value}))).collect::<Vec<_>>();
                out.push(json!({"name": name, "command": command, "args": args, "env": env}));
            }
            Transport::Remote { url, headers } => {
                let headers = headers.iter().filter_map(|(name, reference)| resolve(reference).map(|value| json!({name: value}))).collect::<Vec<_>>();
                let mut entry = json!({"name": name, "url": url});
                if !headers.is_empty() { entry["headers"] = Value::Array(headers); }
                out.push(entry);
            }
        }
    }
    Value::Array(out)
}

pub fn codex_overrides(state_root: &Path, project: &Path) -> Vec<String> {
    let Ok(registry) = load(state_root) else { return Vec::new() };
    let mut out = Vec::new();
    for (name, _server) in &registry.servers {
        let Some(server) = effective(&registry, project, name) else { continue };
        match &server.transport {
            Transport::Stdio { command, args, env } => {
                out.push(format!("mcp_servers.{name}.command={command}"));
                out.push(format!("mcp_servers.{name}.args={}", serde_json::to_string(args).unwrap_or_else(|_| "[]".into())));
                for (key, reference) in env { if let Some(value) = resolve(reference) { out.push(format!("mcp_servers.{name}.env.{key}={value}")); } }
            }
            Transport::Remote { url, .. } => out.push(format!("mcp_servers.{name}.url={url}")),
        }
    }
    out
}

/// Read provider configuration without changing it. The returned entries contain only transport
/// shape and secret references. Literal secret values are rejected instead of copied.
pub fn import_provider_file(path: &Path, provider: &str) -> Result<Vec<(String, Transport)>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if text.len() as u64 > MAX_BYTES { return Err("provider MCP file exceeds the 1 MiB limit".into()); }
    let mut out = Vec::new();
    if provider.eq_ignore_ascii_case("codex") {
        let root: toml::Table = text.parse().map_err(|e| format!("Codex MCP file is invalid: {e}"))?;
        for (name, value) in root.get("mcp_servers").and_then(toml::Value::as_table).into_iter().flat_map(|t| t.iter()) {
            let Some(table) = value.as_table() else { continue };
            let command = table.get("command").and_then(toml::Value::as_str).ok_or_else(|| format!("Codex server {name} has no command"))?;
            let args = table.get("args").and_then(toml::Value::as_array).map(|a| a.iter().filter_map(toml::Value::as_str).map(str::to_owned).collect()).unwrap_or_default();
            let env = table.get("env").and_then(toml::Value::as_table).map(|e| e.keys().map(|key| (key.clone(), SecretRef::Env { env: key.clone() })).collect()).unwrap_or_default();
            out.push((name.clone(), Transport::Stdio { command: command.into(), args, env }));
        }
    } else {
        let root: Value = serde_json::from_str(&text).map_err(|e| format!("provider MCP file is invalid: {e}"))?;
        for (name, value) in root.get("mcpServers").and_then(Value::as_object).into_iter().flat_map(|o| o.iter()) {
            if let Some(url) = value.get("url").and_then(Value::as_str) {
                out.push((name.clone(), Transport::Remote { url: url.into(), headers: BTreeMap::new() }));
            } else if let Some(command) = value.get("command").and_then(Value::as_str) {
                let args = value.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default();
                let env = value.get("env").and_then(Value::as_object).map(|e| e.keys().map(|key| (key.clone(), SecretRef::Env { env: key.clone() })).collect()).unwrap_or_default();
                out.push((name.clone(), Transport::Stdio { command: command.into(), args, env }));
            }
        }
    }
    Ok(out)
}

pub fn status_lines(state_root: &Path, project: &Path) -> Vec<String> {
    let Ok(registry) = load(state_root) else { return vec!["MCP registry cannot be read".into()] };
    registry.servers.iter().map(|(name, server)| {
        let state = if !server.enabled { "off" } else if effective(&registry, project, name).is_none() { "project off" } else { "enabled" };
        format!("{name}: {state} · Claude Code: launch config required · Codex: delivered · Cursor/Grok Build/opencode: ACP")
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_round_trip_and_project_override_keep_secret_as_reference() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry { servers: BTreeMap::new(), projects: BTreeMap::new() };
        registry.servers.insert("jira".into(), Server { enabled: true, transport: Transport::Stdio { command: "jira-mcp".into(), args: vec!["--stdio".into()], env: BTreeMap::from([("TOKEN".into(), SecretRef::Env { env: "JIRA_TOKEN".into() })]) } });
        registry.projects.insert(dir.path().join("off").to_string_lossy().into(), BTreeMap::from([("jira".into(), false)]));
        save(dir.path(), &registry).unwrap();
        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded, registry);
        assert!(effective(&loaded, &dir.path().join("off"), "jira").is_none());
        let text = std::fs::read_to_string(path(dir.path())).unwrap();
        assert!(!text.contains("secret") && text.contains("JIRA_TOKEN"));
    }

    #[test]
    fn import_reads_synthetic_provider_files_as_references() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude.json");
        std::fs::write(&claude, r#"{"mcpServers":{"jira":{"command":"jira-mcp","args":["--stdio"],"env":{"TOKEN":"do-not-copy"}}}}"#).unwrap();
        let imported = import_provider_file(&claude, "claude").unwrap();
        assert_eq!(imported[0].0, "jira");
        assert_eq!(imported[0].1, Transport::Stdio { command: "jira-mcp".into(), args: vec!["--stdio".into()], env: BTreeMap::from([("TOKEN".into(), SecretRef::Env { env: "TOKEN".into() })]) });
        let codex = dir.path().join("codex.toml");
        std::fs::write(&codex, "[mcp_servers.jira]\ncommand='jira-mcp'\nargs=['--stdio']\n[mcp_servers.jira.env]\nTOKEN='do-not-copy'\n").unwrap();
        assert_eq!(import_provider_file(&codex, "codex").unwrap().len(), 1);
        assert!(!toml::to_string(&imported).unwrap_or_default().contains("do-not-copy"));
    }
}
