//! Concordia-owned MCP registry and per-launch projections.
//!
//! The registry is state owned by Concordia. Provider homes are never written. Secret-bearing
//! values are references, not values. Launch projections resolve environment references only in
//! memory and omit unresolved references so a missing sign-in is visible instead of leaking data.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

fn default_on() -> bool {
    true
}

pub fn path(state_root: &Path) -> PathBuf {
    state_root.join("mcp.toml")
}

pub fn load(state_root: &Path) -> Result<Registry, String> {
    let file = path(state_root);
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Registry {
                servers: BTreeMap::new(),
                projects: BTreeMap::new(),
            })
        }
        Err(e) => return Err(format!("cannot read {}: {e}", file.display())),
    };
    if text.len() as u64 > MAX_BYTES {
        return Err("MCP registry exceeds the 1 MiB limit".into());
    }
    toml::from_str(&text).map_err(|e| format!("MCP registry is invalid: {e}"))
}

pub fn save(state_root: &Path, registry: &Registry) -> Result<(), String> {
    for (name, server) in &registry.servers {
        validate_server(name, server)?;
    }
    let text = toml::to_string_pretty(registry).map_err(|e| format!("cannot encode MCP registry: {e}"))?;
    if text.len() as u64 > MAX_BYTES { return Err("MCP registry exceeds the 1 MiB limit".into()); }
    std::fs::create_dir_all(state_root).map_err(|e| format!("cannot create state directory: {e}"))?;
    let file = path(state_root);
    let temp = file.with_extension("toml.tmp");
    std::fs::write(&temp, text).map_err(|e| format!("cannot write MCP registry: {e}"))?;
    std::fs::rename(&temp, &file).map_err(|e| format!("cannot replace MCP registry: {e}"))
}

fn validate_server(name: &str, server: &Server) -> Result<(), String> {
    if has_inline_secret(server) {
        return Err(format!("MCP server {name} contains an inline secret; use a Credential Manager reference"));
    }
    Ok(())
}

fn has_inline_secret(server: &Server) -> bool {
    let Transport::Stdio { args, .. } = &server.transport else { return false };
    let secret_option = |arg: &str| matches!(arg.to_ascii_lowercase().as_str(), "--token" | "--access-token" | "--api-key" | "--apikey" | "--secret" | "--password" | "-t");
    args.iter().enumerate().any(|(index, arg)| {
        secret_option(arg) && args.get(index + 1).is_some()
            || secret_option(arg.split_once('=').map_or("", |(name, _)| name))
            || arg.to_ascii_uppercase().starts_with("TOKEN=")
            || arg.to_ascii_uppercase().starts_with("SECRET=")
            || arg.to_ascii_uppercase().starts_with("API_KEY=")
    })
}

pub fn effective<'a>(registry: &'a Registry, project: &Path, name: &str) -> Option<&'a Server> {
    let server = registry.servers.get(name)?;
    let project_key = project.to_string_lossy();
    if registry
        .projects
        .get(project_key.as_ref())
        .and_then(|p| p.get(name))
        .is_some_and(|on| !on)
    {
        return None;
    }
    server.enabled.then_some(server)
}

fn resolve(reference: &SecretRef) -> Option<String> {
    match reference {
        SecretRef::Env { env } => std::env::var(env).ok().filter(|v| !v.is_empty()),
        SecretRef::Credential { .. } => None,
    }
}

pub fn resolve_secret(reference: &SecretRef) -> Option<String> {
    match reference {
        SecretRef::Env { env } => std::env::var(env).ok().filter(|v| !v.is_empty()),
        SecretRef::Credential { credential } => credential_value(credential).ok(),
    }
}

#[cfg(windows)]
fn credential_value(name: &str) -> Result<String, String> {
    use std::iter::once;
    use windows_sys::Win32::Security::Credentials::*;
    let target: Vec<u16> = name.encode_utf16().chain(once(0)).collect();
    let mut ptr = std::ptr::null_mut();
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut ptr) } == 0 { return Err("credential not found".into()); }
    let cred = unsafe { &*ptr };
    let bytes = unsafe { std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize) };
    let value = String::from_utf8(bytes.to_vec()).map_err(|_| "credential is not UTF-8".to_string());
    unsafe { CredFree(ptr as _) };
    value
}

#[cfg(not(windows))]
fn credential_value(_: &str) -> Result<String, String> { Err("credentials require Windows Credential Manager".into()) }

pub fn acp_servers(state_root: &Path, project: &Path) -> Value {
    let Ok(registry) = load(state_root) else {
        return json!([]);
    };
    let mut out = Vec::new();
    for (name, _server) in &registry.servers {
        let Some(server) = effective(&registry, project, name) else { continue };
        if has_inline_secret(server) { continue; }
        match &server.transport {
            Transport::Stdio { command, args, env } => {
                let env = env
                    .iter()
                    .filter_map(|(name, reference)| {
                        resolve(reference).map(|value| json!({"name": name, "value": value}))
                    })
                    .collect::<Vec<_>>();
                out.push(json!({"name": name, "command": command, "args": args, "env": env}));
            }
            Transport::Remote { url, headers: _ } => {
                if supported_remote_url(url) {
                    out.push(proxy_entry(name, state_root));
                }
            }
        }
    }
    Value::Array(out)
}

pub fn codex_overrides(state_root: &Path, project: &Path) -> Vec<String> {
    let Ok(registry) = load(state_root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, _server) in &registry.servers {
        let Some(server) = effective(&registry, project, name) else { continue };
        if has_inline_secret(server) { continue; }
        match &server.transport {
            Transport::Stdio { command, args, env } => {
                out.push(format!("mcp_servers.{name}.command={command}"));
                out.push(format!(
                    "mcp_servers.{name}.args={}",
                    serde_json::to_string(args).unwrap_or_else(|_| "[]".into())
                ));
                for (key, reference) in env {
                    if let Some(value) = resolve(reference) {
                        out.push(format!("mcp_servers.{name}.env.{key}={value}"));
                    }
                }
            }
            Transport::Remote { url, .. } if supported_remote_url(url) => {
                let exe = std::env::current_exe().unwrap_or_default();
                out.push(format!("mcp_servers.{name}.command={}", exe.to_string_lossy()));
                out.push(format!("mcp_servers.{name}.args=[\"mcp\"]"));
                out.push(format!("mcp_servers.{name}.env.CONCORDIA_STATE_DIR={}", state_root.to_string_lossy()));
            }
            Transport::Remote { .. } => {}
        }
    }
    out
}

/// Build Claude's per-launch JSON argument. Secret values stay references and are never read.
pub fn claude_config(state_root: &Path, project: &Path) -> Option<String> {
    let registry = load(state_root).ok()?;
    let mut servers = serde_json::Map::new();
    for (name, _) in &registry.servers {
        let Some(server) = effective(&registry, project, name) else { continue };
        if has_inline_secret(server) { continue; }
        let value = match &server.transport {
            Transport::Stdio { command, args, env } => json!({
                "command": command,
                "args": args,
                "env": env.iter().map(|(key, reference)| (key.clone(), match reference {
                    SecretRef::Env { env } => format!("${{{env}}}"),
                    SecretRef::Credential { credential } => format!("${{CONCORDIA_CREDENTIAL_{credential}}}"),
                })).collect::<BTreeMap<_, _>>(),
            }),
            Transport::Remote { url, .. } if supported_remote_url(url) => proxy_json(state_root),
            Transport::Remote { .. } => continue,
        };
        servers.insert(name.clone(), value);
    }
    (!servers.is_empty()).then(|| json!({"mcpServers": servers}).to_string())
}

fn proxy_json(state_root: &Path) -> Value {
    json!({
        "command": std::env::current_exe().unwrap_or_default().to_string_lossy(),
        "args": ["mcp"],
        "env": {"CONCORDIA_STATE_DIR": state_root.to_string_lossy()}
    })
}

fn proxy_entry(name: &str, state_root: &Path) -> Value {
    json!({
        "name": name,
        "command": std::env::current_exe().unwrap_or_default().to_string_lossy(),
        "args": ["mcp"],
        "env": [{"name": "CONCORDIA_STATE_DIR", "value": state_root.to_string_lossy()}]
    })
}

pub fn claude_config_path(state_root: &Path, project: &Path) -> Option<PathBuf> {
    let config = claude_config(state_root, project)?;
    std::fs::create_dir_all(state_root).ok()?;
    let path = state_root.join(format!("mcp-claude-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&path, config).ok()?;
    Some(path)
}

pub fn has_credential_reference(server: &Server) -> bool {
    match &server.transport {
        Transport::Stdio { env, .. } => env.values().any(|r| matches!(r, SecretRef::Credential { .. })),
        Transport::Remote { headers, .. } => headers.values().any(|r| matches!(r, SecretRef::Credential { .. })),
    }
}

pub fn provider_status_lines(state_root: &Path, project: &Path) -> Vec<String> {
    let Ok(registry) = load(state_root) else { return vec!["MCP registry cannot be read".into()] };
    registry.servers.iter().flat_map(|(name, server)| {
        let base = if !server.enabled { "off" } else if effective(&registry, project, name).is_none() { "project off" } else if has_inline_secret(server) { "unsupported" } else { "delivered" };
        let providers = [
            ("Claude Code", true),
            ("Codex", true),
            ("Cursor", true),
            ("Grok Build", true),
            ("opencode", true),
        ].iter().map(|(provider, supports)| {
            let state = if base != "delivered" { base } else if !supports_transport(server, *supports) { "unsupported" } else if has_missing_reference(server) { "needs sign-in" } else { "delivered" };
            let next = if state == "unsupported" { "next: use stdio or HTTP(S)" } else if state == "needs sign-in" { "next: sign in or set the referenced environment variable" } else if state == "off" || state == "project off" { "next: enable the server" } else { "next: none" };
            format!("{name} · {provider}: {state} · {next}")
        }).collect::<Vec<_>>();
        providers
    }).collect()
}

fn supports_transport(server: &Server, _provider_supports_remote: bool) -> bool {
    match &server.transport {
        Transport::Stdio { .. } => true,
        Transport::Remote { url, .. } => supported_remote_url(url),
    }
}

fn has_missing_reference(server: &Server) -> bool {
    match &server.transport {
        Transport::Stdio { env, .. } => env.values().any(|reference| resolve_secret(reference).is_none()),
        Transport::Remote { headers, .. } => headers.values().any(|reference| resolve_secret(reference).is_none()),
    }
}

fn supported_remote_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEntry {
    pub provider: String,
    pub path: PathBuf,
    pub name: String,
    pub transport: Transport,
}

/// Read provider configuration without changing it. The returned entries contain only transport
/// shape and secret references. Literal secret values are rejected instead of copied.
pub fn import_provider_file(path: &Path, provider: &str) -> Result<Vec<(String, Transport)>, String> {
    let text = std::fs::read_to_string(path).map_err(|_| format!("cannot read provider MCP file {}", path.display()))?;
    if text.len() as u64 > MAX_BYTES { return Err("provider MCP file exceeds the 1 MiB limit".into()); }
    let mut out = Vec::new();
    if provider.eq_ignore_ascii_case("codex") {
        let root: toml::Table = text.parse().map_err(|_| "Codex MCP file is invalid".to_string())?;
        for (name, value) in root.get("mcp_servers").and_then(toml::Value::as_table).into_iter().flat_map(|t| t.iter()) {
            let Some(table) = value.as_table() else { continue };
            if let Some(url) = table.get("url").and_then(toml::Value::as_str) {
                let mut headers = BTreeMap::new();
                if let Some(env) = table.get("bearer_token_env_var").and_then(toml::Value::as_str) {
                    headers.insert("Authorization".into(), SecretRef::Env { env: env.into() });
                }
                out.push((name.clone(), Transport::Remote { url: url.into(), headers }));
                continue;
            }
            let command = table.get("command").and_then(toml::Value::as_str).ok_or_else(|| format!("Codex server {name} has no command"))?;
            let args = table.get("args").and_then(toml::Value::as_array).map(|a| a.iter().filter_map(toml::Value::as_str).map(str::to_owned).collect()).unwrap_or_default();
            let env = table.get("env").and_then(toml::Value::as_table).map(|e| e.iter().map(|(key, value)| (key.clone(), imported_secret_ref(value.as_str().unwrap_or_default(), provider, name, key))).collect()).unwrap_or_default();
            let server = Server { enabled: true, transport: Transport::Stdio { command: command.into(), args, env } };
            validate_server(name, &server)?;
            out.push((name.clone(), server.transport));
        }
    } else {
        let json_text = if provider.eq_ignore_ascii_case("opencode") { strip_jsonc(&text) } else { text.clone() };
        let root: Value = serde_json::from_str(&json_text).map_err(|_| "provider MCP file is invalid".to_string())?;
        for (name, value) in root.get("mcpServers").and_then(Value::as_object).into_iter().flat_map(|o| o.iter()) {
            if let Some(url) = value.get("url").and_then(Value::as_str) {
                out.push((
                    name.clone(),
                    Transport::Remote {
                        url: url.into(),
                        headers: BTreeMap::new(),
                    },
                ));
            } else if let Some(command) = value.get("command").and_then(Value::as_str) {
                let args = value.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default();
                let env = value.get("env").and_then(Value::as_object).map(|e| e.iter().map(|(key, value)| (key.clone(), imported_secret_ref(value.as_str().unwrap_or_default(), provider, name, key))).collect()).unwrap_or_default();
                let server = Server { enabled: true, transport: Transport::Stdio { command: command.into(), args, env } };
                validate_server(name, &server)?;
                out.push((name.clone(), server.transport));
            }
        }
        if provider.eq_ignore_ascii_case("opencode") {
            if let Some(servers) = root.get("mcp").and_then(Value::as_object) {
                for (name, value) in servers {
                    let kind = value.get("type").and_then(Value::as_str).unwrap_or("local");
                    if kind == "remote" {
                        if let Some(url) = value.get("url").and_then(Value::as_str) {
                            out.push((name.clone(), Transport::Remote { url: url.into(), headers: BTreeMap::new() }));
                        }
                    } else if let Some(command) = value.get("command").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str) {
                        let args = value.get("command").and_then(Value::as_array).map(|a| a.iter().skip(1).filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default();
                        let env = value.get("environment").and_then(Value::as_object).map(|e| e.iter().map(|(key, value)| (key.clone(), imported_secret_ref(value.as_str().unwrap_or_default(), provider, name, key))).collect()).unwrap_or_default();
                        let server = Server { enabled: true, transport: Transport::Stdio { command: command.into(), args, env } };
                        validate_server(name, &server)?;
                        out.push((name.clone(), server.transport));
                    }
                }
            }
        }
    }
    Ok(out)
}

fn strip_jsonc(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if in_string {
            output.push(ch);
            if escaped { escaped = false; } else if ch == '\\' { escaped = true; } else if ch == '"' { in_string = false; }
            continue;
        }
        match ch {
            '"' => { in_string = true; output.push(ch); }
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                for comment in chars.by_ref() { if comment == '\n' { output.push('\n'); break; } }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                for comment in chars.by_ref() {
                    if previous == '*' && comment == '/' { break; }
                    if comment == '\n' { output.push('\n'); }
                    previous = comment;
                }
            }
            _ => output.push(ch),
        }
    }
    let mut cleaned = String::with_capacity(output.len());
    let mut chars = output.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if in_string {
            cleaned.push(ch);
            if escaped { escaped = false; } else if ch == '\\' { escaped = true; } else if ch == '"' { in_string = false; }
        } else if ch == '"' {
            in_string = true;
            cleaned.push(ch);
        } else if ch == ',' {
            let mut whitespace = String::new();
            while chars.peek().is_some_and(|next| next.is_whitespace()) { whitespace.push(chars.next().unwrap()); }
            if chars.peek() != Some(&'}') && chars.peek() != Some(&']') {
                cleaned.push(ch);
                cleaned.push_str(&whitespace);
            }
        } else {
            cleaned.push(ch);
        }
    }
    cleaned
}

fn imported_secret_ref(value: &str, provider: &str, server: &str, key: &str) -> SecretRef {
    let env = value.trim().trim_start_matches("${").trim_end_matches('}');
    if env == value.trim() || env.is_empty() {
        SecretRef::Credential { credential: format!("concordia/mcp/{provider}/{server}/{key}") }
    } else {
        SecretRef::Env { env: env.to_owned() }
    }
}

pub fn discover_provider_files(state_root: &Path) -> Vec<(String, PathBuf)> {
    let mut files = Vec::new();
    for account in crate::accounts::list_all_accounts(state_root) {
        let Some(home) = account.home_path else { continue };
        let candidates: Vec<PathBuf> = match account.provider {
            crate::store::ProviderKind::Codex => vec![home.join("config.toml")],
            crate::store::ProviderKind::Claude => vec![home.join("mcp.json"), home.join(".mcp.json"), home.parent().unwrap_or(&home).join(".claude.json")],
            crate::store::ProviderKind::Opencode => vec![home.join("opencode.json"), home.join("opencode.jsonc")],
            _ => vec![home.join("mcp.json"), home.join(".mcp.json"), home.join("config.json")],
        };
        for path in candidates {
            if path.is_file() { files.push((account.provider.as_str().to_owned(), path)); }
        }
    }
    files
}

pub fn import_summary(state_root: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    for (provider, path) in discover_provider_files(state_root) {
        match import_provider_file(&path, &provider) {
            Ok(entries) if entries.is_empty() => {}
            Ok(entries) => lines.push(format!("{provider}: {} · read-only · Enter an entry to add it to Concordia", entries.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", "))),
            Err(error) => lines.push(format!("{provider}: cannot inspect {} · {error}" , path.display())),
        }
    }
    if lines.is_empty() { lines.push("No provider MCP files found. Nothing changed.".into()); }
    lines
}

pub fn import_entries(state_root: &Path) -> Vec<ImportEntry> {
    discover_provider_files(state_root).into_iter().flat_map(|(provider, path)| {
        import_provider_file(&path, &provider).unwrap_or_default().into_iter().map(move |(name, transport)| ImportEntry { provider: provider.clone(), path: path.clone(), name, transport })
    }).collect()
}

pub fn status_lines(state_root: &Path, project: &Path) -> Vec<String> {
    let Ok(registry) = load(state_root) else {
        return vec!["MCP registry cannot be read".into()];
    };
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
        let mut registry = Registry {
            servers: BTreeMap::new(),
            projects: BTreeMap::new(),
        };
        registry.servers.insert(
            "jira".into(),
            Server {
                enabled: true,
                transport: Transport::Stdio {
                    command: "jira-mcp".into(),
                    args: vec!["--stdio".into()],
                    env: BTreeMap::from([(
                        "TOKEN".into(),
                        SecretRef::Env {
                            env: "JIRA_TOKEN".into(),
                        },
                    )]),
                },
            },
        );
        registry.projects.insert(
            dir.path().join("off").to_string_lossy().into(),
            BTreeMap::from([("jira".into(), false)]),
        );
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
        assert_eq!(imported[0].1, Transport::Stdio { command: "jira-mcp".into(), args: vec!["--stdio".into()], env: BTreeMap::from([("TOKEN".into(), SecretRef::Credential { credential: "concordia/mcp/claude/jira/TOKEN".into() })]) });
        let codex = dir.path().join("codex.toml");
        std::fs::write(&codex, "[mcp_servers.jira]\ncommand='jira-mcp'\nargs=['--stdio']\n[mcp_servers.jira.env]\nTOKEN='do-not-copy'\n").unwrap();
        assert_eq!(import_provider_file(&codex, "codex").unwrap().len(), 1);
        assert!(!toml::to_string(&imported)
            .unwrap_or_default()
            .contains("do-not-copy"));
    }

    #[test]
    fn import_rejects_inline_token_arguments_without_echoing_the_value() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("claude.json");
        std::fs::write(&file, r#"{"mcpServers":{"jira":{"command":"jira-mcp","args":["--token","SECRET_CANARY"]}}}"#).unwrap();
        let error = import_provider_file(&file, "claude").unwrap_err();
        assert!(error.contains("inline secret"));
        assert!(!error.contains("SECRET_CANARY"));
    }

    #[test]
    fn https_upstream_is_explicitly_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry { servers: BTreeMap::new(), projects: BTreeMap::new() };
        registry.servers.insert("remote".into(), Server { enabled: true, transport: Transport::Remote { url: "https://example.invalid/mcp".into(), headers: BTreeMap::new() } });
        save(dir.path(), &registry).unwrap();
        let lines = provider_status_lines(dir.path(), Path::new("."));
        assert!(lines[0].contains("delivered"));
    }

    #[test]
    fn missing_environment_reference_is_visible_as_sign_in() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry { servers: BTreeMap::new(), projects: BTreeMap::new() };
        registry.servers.insert("remote".into(), Server { enabled: true, transport: Transport::Remote { url: "https://example.invalid/mcp".into(), headers: BTreeMap::from([("Authorization".into(), SecretRef::Env { env: "Y5_MISSING".into() })]) } });
        save(dir.path(), &registry).unwrap();
        let lines = provider_status_lines(dir.path(), Path::new("."));
        assert!(lines[0].contains("needs sign-in"));
        assert!(lines[0].contains("set the referenced environment variable"));
    }

    #[test]
    fn provider_discovery_includes_native_files() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude");
        let opencode = dir.path().join("opencode");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::create_dir_all(&opencode).unwrap();
        std::fs::write(dir.path().join(".claude.json"), r#"{"mcpServers":{"claude-native":{"command":"x"}}}"#).unwrap();
        std::fs::write(opencode.join("opencode.json"), r#"{"mcp":{"open-native":{"type":"local","command":["x","--stdio"]}}}"#).unwrap();
        let claude_entries = import_provider_file(&dir.path().join(".claude.json"), "claude").unwrap();
        let open_entries = import_provider_file(&opencode.join("opencode.json"), "opencode").unwrap();
        assert_eq!(claude_entries[0].0, "claude-native");
        assert_eq!(open_entries[0].0, "open-native");
    }

    #[test]
    fn opencode_jsonc_preserves_urls_and_allows_trailing_commas() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("opencode.jsonc");
        std::fs::write(&file, r#"{
          // provider comments are valid JSONC
          "mcp": { "remote": { "type": "remote", "url": "https://example.invalid/mcp", }, },
        }"#).unwrap();
        let entries = import_provider_file(&file, "opencode").unwrap();
        assert_eq!(entries[0].0, "remote");
        assert!(matches!(&entries[0].1, Transport::Remote { url, .. } if url == "https://example.invalid/mcp"));
    }
}
