//! What Concordia leaves inside homes it created: the optional MCP entry.
//!
//! Rules (docs/mcp.md, AGENTS.md): only homes under `<state>/homes` that Concordia created are ever
//! touched, never a user's own `.claude` / `.codex`. Everything lives between explicit markers or in
//! one clearly named entry, is idempotent, preserves what the user wrote around it, and is left
//! alone (and reported by `concordia doctor`) when the file cannot be parsed.
//!
//! Formats (verified with `claude mcp add -s user` and `codex mcp add` against empty temp homes):
//! * Claude: `<CLAUDE_CONFIG_DIR>/.claude.json`, top-level `mcpServers.<name> =
//!   {type:"stdio", command, args, env}`; instructions in `<CLAUDE_CONFIG_DIR>/CLAUDE.md`.
//! * Codex: `<CODEX_HOME>/config.toml`, table `[mcp_servers.<name>]` with `command`, `args` and a
//!   `[mcp_servers.<name>.env]` table; instructions in `<CODEX_HOME>/AGENTS.md`.
//! * Cursor: skipped. Its extra accounts are API-key folders only; the CLI reads `~/.cursor`, which
//!   Concordia never writes.

use crate::store::ProviderKind;
use std::fs;
use std::path::{Path, PathBuf};

const TOML_BEGIN: &str =
    "# concordia:begin (managed by Concordia; remove with [threads] mcp = false)";
const TOML_BEGIN_KEY: &str = "# concordia:begin";
const TOML_END: &str = "# concordia:end";
/// Name of the server entry in each CLI's config.
pub const SERVER_NAME: &str = "concordia";
/// Env var that points the server at the state folder (the one `store::default_store_root` reads).
pub const STATE_ENV: &str = "CONCORDIA_STATE_DIR";
/// JSON object containing the account id and workspace root fixed at registration time.
pub const SCOPE_ENV: &str = "CONCORDIA_MCP_SCOPE";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Already as wanted.
    Current,
    /// Written (or, in a dry run, would be written).
    Updated,
    /// Our block or entry was removed (or would be).
    Removed,
    /// Off, and there is nothing of ours to remove.
    Absent,
    /// Left alone, with the reason (unparseable file, a foreign entry with our name, ...).
    Blocked(String),
}

#[derive(Debug, Clone)]
pub struct HomeReport {
    pub account: String,
    pub provider: ProviderKind,
    pub home: PathBuf,
    pub mcp: Outcome,
}

/// Homes Concordia created (registered, under `<state>/homes`) for the CLIs that read an
/// instructions file and an MCP config from their home. Hidden or user-owned homes never appear.
pub fn created_homes(state_root: &Path) -> Vec<(String, ProviderKind, PathBuf)> {
    crate::connect::removable(state_root)
        .into_iter()
        .filter(|r| r.owned && matches!(r.provider, ProviderKind::Claude | ProviderKind::Codex))
        .filter_map(|r| r.home.map(|h| (r.id, r.provider, h)))
        .collect()
}

/// Bring every Concordia-created home in line with `[threads] mcp`.
pub fn refresh(state_root: &Path, exe: &Path) -> Vec<HomeReport> {
    run(state_root, exe, true)
}

/// `/use`: refresh with the active exe and return the transcript lines for what changed.
pub fn refresh_summary(state_root: &Path) -> Vec<String> {
    summary_lines(&refresh(state_root, &crate::update::resolve_active_exe()))
}

/// Same decisions, nothing written (for `concordia doctor`).
pub fn check(state_root: &Path, exe: &Path) -> Vec<HomeReport> {
    run(state_root, exe, false)
}

fn run(state_root: &Path, exe: &Path, write: bool) -> Vec<HomeReport> {
    let cfg = crate::config::for_root(state_root).threads;
    created_homes(state_root)
        .into_iter()
        .map(|(account, provider, home)| HomeReport {
            mcp: sync_mcp_for_account(&home, provider, cfg.mcp, exe, state_root, &account, write),
            account,
            provider,
            home,
        })
        .collect()
}

/// One transcript line per change or problem (nothing for homes that are already right).
pub fn summary_lines(reports: &[HomeReport]) -> Vec<String> {
    let mut out = Vec::new();
    for r in reports {
        for (what, o) in [("mcp", &r.mcp)] {
            match o {
                Outcome::Updated => out.push(format!("{}: {what} set up", r.account)),
                Outcome::Removed => out.push(format!("{}: {what} removed", r.account)),
                Outcome::Blocked(why) => out.push(format!("{}: {what} skipped - {why}", r.account)),
                Outcome::Current | Outcome::Absent => {}
            }
        }
    }
    out
}

/// Rows for `concordia doctor`: (is a warning, name, detail).
pub fn doctor_rows(state_root: &Path, exe: &Path) -> Vec<(bool, &'static str, String)> {
    let mut out = Vec::new();
    for r in check(state_root, exe) {
        for (name, o) in [("mcp", &r.mcp)] {
            let (warn, text) = match o {
                Outcome::Current => (false, "set up".to_string()),
                Outcome::Updated => (false, "out of date; /use refreshes it".to_string()),
                Outcome::Removed => (
                    false,
                    "still present but turned off; /use removes it".into(),
                ),
                Outcome::Absent => (false, "off".to_string()),
                Outcome::Blocked(why) => (true, format!("left alone - {why}")),
            };
            out.push((warn, name, format!("{}: {text}", r.account)));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Shared marker-block editing
// ---------------------------------------------------------------------------------------------

fn sync_text_block_checked(
    file: &Path,
    begin: &str,
    end: &str,
    body: &[&str],
    want: bool,
    write: bool,
    validate: &dyn Fn(&str) -> Result<(), String>,
) -> Outcome {
    let (old, existed) = match read_configuration(file) {
        Ok(b) => match String::from_utf8(b) {
            Ok(s) => (s, true),
            Err(_) => return Outcome::Blocked(format!("{} is not UTF-8", name_of(file))),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), false),
        Err(e) => return Outcome::Blocked(format!("cannot read {}: {e}", name_of(file))),
    };
    let new = match splice(&old, begin, end, want.then_some(body)) {
        Ok(s) => s,
        Err(m) => return Outcome::Blocked(format!("{}: {m}", name_of(file))),
    };
    if new == old {
        return if want {
            Outcome::Current
        } else {
            Outcome::Absent
        };
    }
    if want {
        if let Err(m) = validate(&new) {
            return Outcome::Blocked(format!("{}: {m}", name_of(file)));
        }
    }
    if write {
        let res = atomic_write(file, new.as_bytes(), existed.then_some(old.as_bytes()));
        if let Err(e) = res {
            return Outcome::Blocked(format!("cannot write {}: {e}", name_of(file)));
        }
    }
    if want {
        Outcome::Updated
    } else {
        Outcome::Removed
    }
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn atomic_write(path: &Path, bytes: &[u8], expected: Option<&[u8]>) -> std::io::Result<()> {
    if bytes.len() > 1024 * 1024 {
        return Err(std::io::Error::other(
            "native configuration exceeds the 1 MiB write bound; original retained",
        ));
    }
    match expected {
        Some(expected) => crate::file_document::save(
            path,
            std::str::from_utf8(expected).map_err(std::io::Error::other)?,
            std::str::from_utf8(bytes).map_err(std::io::Error::other)?,
        )
        .map_err(std::io::Error::other),
        None => crate::state_io::write_atomic_new(path, bytes),
    }
}

/// Pure text edit behind [`sync_text_block`]. `begin` may be a prefix of the line (the TOML marker
/// carries a trailing comment); the block is the lines from the one starting with `begin` through
/// the one starting with `end`.
fn splice(text: &str, begin: &str, end: &str, body: Option<&[&str]>) -> Result<String, String> {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let block = |b: &[&str]| -> String {
        let mut lines: Vec<&str> = vec![begin];
        lines.extend_from_slice(b);
        lines.push(end);
        lines.join(nl)
    };
    let markers = |marker: &str| {
        text.match_indices(marker)
            .filter(|(index, _)| *index == 0 || text.as_bytes()[index - 1] == b'\n')
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    };
    let begins = markers(begin_key(begin));
    let ends = markers(end);
    if begins.len() > 1 || ends.len() > 1 || begins.is_empty() && !ends.is_empty() {
        return Err("ambiguous concordia markers; left alone".into());
    }
    let found = begins.first().copied();
    let Some(start) = found else {
        return Ok(match body {
            None => text.to_string(),
            Some(b) => {
                let mut out = text.to_string();
                if !out.is_empty() {
                    if !out.ends_with('\n') {
                        out.push_str(nl);
                    }
                    out.push_str(nl);
                }
                out.push_str(&block(b));
                out.push_str(nl);
                out
            }
        });
    };
    let after_begin = start + begin_key(begin).len();
    let Some(end_start) = ends.first().copied().filter(|end| *end >= after_begin) else {
        return Err("a concordia:begin marker has no matching end marker; left alone".into());
    };
    let stop = end_start + end.len();
    match body {
        Some(b) => Ok(format!("{}{}{}", &text[..start], block(b), &text[stop..])),
        None => {
            let mut before = text[..start].to_string();
            let after = text[stop..]
                .strip_prefix("\r\n")
                .or_else(|| text[stop..].strip_prefix('\n'))
                .unwrap_or(&text[stop..]);
            if after.is_empty() {
                let trimmed = before.trim_end_matches(['\r', '\n']).len();
                before.truncate(trimmed);
                if !before.is_empty() {
                    before.push_str(nl);
                }
            }
            Ok(format!("{before}{after}"))
        }
    }
}

/// The marker text to search for: the begin marker without any trailing comment.
fn begin_key(begin: &str) -> &str {
    if begin.starts_with(TOML_BEGIN_KEY) {
        TOML_BEGIN_KEY
    } else {
        begin
    }
}

// ---------------------------------------------------------------------------------------------
// MCP registration
// ---------------------------------------------------------------------------------------------

pub fn sync_mcp(
    home: &Path,
    provider: ProviderKind,
    want: bool,
    exe: &Path,
    state_root: &Path,
    write: bool,
) -> Outcome {
    sync_mcp_for_account(home, provider, want, exe, state_root, "unknown", write)
}

fn registration_workspace() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|path| fs::canonicalize(path).ok())
        .unwrap_or_else(|| PathBuf::from(".").to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn scope_json(account: &str) -> String {
    serde_json::json!({"account": account, "workspace": registration_workspace()}).to_string()
}

fn sync_mcp_for_account(
    home: &Path,
    provider: ProviderKind,
    want: bool,
    exe: &Path,
    state_root: &Path,
    account: &str,
    write: bool,
) -> Outcome {
    match provider {
        ProviderKind::Claude => sync_claude_json(home, want, exe, state_root, account, write),
        ProviderKind::Codex => sync_codex_toml(home, want, exe, state_root, account, write),
        ProviderKind::Cursor => Outcome::Blocked("not supported for this provider".into()),
        ProviderKind::Grok | ProviderKind::Opencode => {
            Outcome::Blocked("ACP provider integrations are provider-owned".into())
        }
    }
}

fn entry_json(exe: &Path, state_root: &Path, account: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "stdio",
        "command": exe.to_string_lossy(),
        "args": ["mcp"],
        "env": { STATE_ENV: state_root.to_string_lossy(), SCOPE_ENV: scope_json(account) },
    })
}

fn read_configuration(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || crate::workspaces::linked(&metadata) {
        return Err(std::io::Error::other(
            "native configuration must be a regular file",
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(std::io::Error::other(
            "native configuration exceeds the 1 MiB inspection bound; left unchanged",
        ));
    }
    Ok(bytes)
}

/// An entry we wrote: stdio, args exactly ["mcp"]. Anything else under our name is the user's.
fn is_ours(entry: &serde_json::Value) -> bool {
    entry.get("args") == Some(&serde_json::json!(["mcp"]))
        && entry
            .get("env")
            .and_then(|e| e.get(STATE_ENV))
            .is_some_and(|v| v.is_string())
}

fn sync_claude_json(
    home: &Path,
    want: bool,
    exe: &Path,
    state_root: &Path,
    account: &str,
    write: bool,
) -> Outcome {
    let file = home.join(".claude.json");
    let original = match read_configuration(&file) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Outcome::Blocked(format!("cannot read .claude.json: {error}")),
    };
    let mut root = match original.as_ref() {
        Some(b) => match serde_json::from_slice::<serde_json::Value>(b) {
            Ok(v) if v.is_object() => v,
            _ => return Outcome::Blocked(".claude.json is not a JSON object; left alone".into()),
        },
        None => serde_json::json!({}),
    };
    let existing = root.pointer("/mcpServers/concordia").cloned();
    if let Some(e) = &existing {
        if !is_ours(e) {
            return Outcome::Blocked(
                "mcpServers.concordia exists and was not written by Concordia; left alone".into(),
            );
        }
    }
    if root.get("mcpServers").is_some_and(|m| !m.is_object()) {
        return Outcome::Blocked("mcpServers is not an object; left alone".into());
    }
    if want {
        let desired = entry_json(exe, state_root, account);
        if existing.as_ref() == Some(&desired) {
            return Outcome::Current;
        }
        root["mcpServers"][SERVER_NAME] = desired;
    } else {
        if existing.is_none() {
            return Outcome::Absent;
        }
        let servers = root["mcpServers"].as_object_mut().expect("checked above");
        servers.remove(SERVER_NAME);
        if servers.is_empty() {
            root.as_object_mut().expect("object").remove("mcpServers");
        }
    }
    if write {
        let Ok(text) = serde_json::to_string_pretty(&root) else {
            return Outcome::Blocked("could not serialise .claude.json".into());
        };
        if let Err(e) = atomic_write(&file, text.as_bytes(), original.as_deref()) {
            return Outcome::Blocked(format!("cannot write .claude.json: {e}"));
        }
    }
    if want {
        Outcome::Updated
    } else {
        Outcome::Removed
    }
}

fn toml_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn sync_codex_toml(
    home: &Path,
    want: bool,
    exe: &Path,
    state_root: &Path,
    account: &str,
    write: bool,
) -> Outcome {
    let file = home.join("config.toml");
    let text = match read_configuration(&file) {
        Ok(b) => match String::from_utf8(b) {
            Ok(s) => s,
            Err(_) => return Outcome::Blocked("config.toml is not UTF-8; left alone".into()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Outcome::Blocked(format!("cannot read config.toml: {e}")),
    };
    let table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            return Outcome::Blocked(format!(
                "config.toml does not parse ({}); left alone",
                e.message()
            ))
        }
    };
    let ours = text.contains(TOML_BEGIN_KEY);
    let foreign = table
        .get("mcp_servers")
        .and_then(|m| m.as_table())
        .is_some_and(|m| m.contains_key(SERVER_NAME));
    if foreign && !ours {
        return Outcome::Blocked(
            "[mcp_servers.concordia] exists and was not written by Concordia; left alone".into(),
        );
    }
    let body = [
        "[mcp_servers.concordia]".to_string(),
        format!("command = {}", toml_str(&exe.to_string_lossy())),
        "args = [\"mcp\"]".to_string(),
        String::new(),
        "[mcp_servers.concordia.env]".to_string(),
        format!("{STATE_ENV} = {}", toml_str(&state_root.to_string_lossy())),
        format!("{SCOPE_ENV} = {}", toml_str(&scope_json(account))),
    ];
    let body_refs: Vec<&str> = body.iter().map(String::as_str).collect();
    // The new text must still be valid TOML with a single concordia server, or nothing is written.
    let validate = |t: &str| -> Result<(), String> {
        match t.parse::<toml::Table>() {
            Ok(_) => Ok(()),
            Err(e) => Err(format!(
                "would not parse after the edit ({}); left alone",
                e.message()
            )),
        }
    };
    sync_text_block_checked(
        &file, TOML_BEGIN, TOML_END, &body_refs, want, write, &validate,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(tmp: &Path, name: &str) -> PathBuf {
        let h = tmp.join(name);
        fs::create_dir_all(&h).unwrap();
        h
    }

    #[test]
    fn claude_json_merges_keeps_everything_else_and_removes_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let h = home(tmp.path(), "claude-2");
        let f = h.join(".claude.json");
        let exe = Path::new("C:/bin/concordia.exe");
        let st = Path::new("C:/state");
        fs::write(
            &f,
            r#"{"theme":"dark","mcpServers":{"other":{"command":"x"}}}"#,
        )
        .unwrap();
        assert_eq!(
            sync_mcp(&h, ProviderKind::Claude, true, exe, st, true),
            Outcome::Updated
        );
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(
            v["mcpServers"]["concordia"]["args"],
            serde_json::json!(["mcp"])
        );
        assert_eq!(v["mcpServers"]["concordia"]["env"][STATE_ENV], "C:/state");
        assert_eq!(
            sync_mcp(&h, ProviderKind::Claude, true, exe, st, true),
            Outcome::Current
        );
        // A moved exe updates the entry.
        let moved = Path::new("C:/new/concordia.exe");
        assert_eq!(
            sync_mcp(&h, ProviderKind::Claude, true, moved, st, true),
            Outcome::Updated
        );
        assert_eq!(
            sync_mcp(&h, ProviderKind::Claude, false, moved, st, true),
            Outcome::Removed
        );
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&f).unwrap()).unwrap();
        assert!(v["mcpServers"].get("concordia").is_none() && v["mcpServers"]["other"].is_object());
        assert_eq!(
            sync_mcp(&h, ProviderKind::Claude, false, moved, st, true),
            Outcome::Absent
        );
    }

    #[test]
    fn registration_writes_the_scope() {
        let tmp = tempfile::tempdir().unwrap();
        let codex3_home = home(tmp.path(), "codex-3");
        let exe = Path::new("C:/bin/concordia.exe");
        let state = Path::new("C:/state");
        assert_eq!(
            sync_mcp_for_account(
                &codex3_home,
                ProviderKind::Claude,
                true,
                exe,
                state,
                "codex-3",
                true
            ),
            Outcome::Updated
        );
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(codex3_home.join(".claude.json")).unwrap())
                .unwrap();
        let scope: serde_json::Value = serde_json::from_str(
            value["mcpServers"][SERVER_NAME]["env"][SCOPE_ENV]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(scope["account"], "codex-3");
        assert!(scope["workspace"]
            .as_str()
            .is_some_and(|workspace| !workspace.is_empty()));
        let codex_home = home(tmp.path(), "codex-config");
        assert_eq!(
            sync_mcp_for_account(
                &codex_home,
                ProviderKind::Codex,
                true,
                exe,
                state,
                "codex-3",
                true
            ),
            Outcome::Updated
        );
        let codex: toml::Table = fs::read_to_string(codex_home.join("config.toml"))
            .unwrap()
            .parse()
            .unwrap();
        let codex_scope: serde_json::Value = serde_json::from_str(
            codex["mcp_servers"][SERVER_NAME]["env"][SCOPE_ENV]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(codex_scope["account"], "codex-3");
    }

    #[test]
    fn claude_json_that_cannot_be_parsed_or_is_foreign_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let h = home(tmp.path(), "claude-2");
        let f = h.join(".claude.json");
        let exe = Path::new("c.exe");
        let st = Path::new("s");
        fs::write(&f, "{ not json").unwrap();
        assert!(matches!(
            sync_mcp(&h, ProviderKind::Claude, true, exe, st, true),
            Outcome::Blocked(_)
        ));
        assert_eq!(fs::read_to_string(&f).unwrap(), "{ not json");
        fs::write(
            &f,
            r#"{"mcpServers":{"concordia":{"command":"mine","args":["serve"]}}}"#,
        )
        .unwrap();
        let before = fs::read_to_string(&f).unwrap();
        assert!(matches!(
            sync_mcp(&h, ProviderKind::Claude, true, exe, st, true),
            Outcome::Blocked(_)
        ));
        assert!(matches!(
            sync_mcp(&h, ProviderKind::Claude, false, exe, st, true),
            Outcome::Blocked(_)
        ));
        assert_eq!(fs::read_to_string(&f).unwrap(), before);
    }

    #[test]
    fn codex_toml_gets_a_marked_block_and_keeps_the_users_content() {
        let tmp = tempfile::tempdir().unwrap();
        let h = home(tmp.path(), "codex-3");
        let f = h.join("config.toml");
        let exe = Path::new(r"C:\bin\concordia.exe");
        let st = Path::new(r"C:\state dir");
        let mine = "# my settings\nmodel = \"x\"\n\n[mcp_servers.other]\ncommand = \"y\"\n";
        fs::write(&f, mine).unwrap();
        assert_eq!(
            sync_mcp(&h, ProviderKind::Codex, true, exe, st, true),
            Outcome::Updated
        );
        let s = fs::read_to_string(&f).unwrap();
        assert!(s.starts_with(mine));
        let t: toml::Table = s.parse().unwrap();
        let c = &t["mcp_servers"]["concordia"];
        assert_eq!(c["command"].as_str(), Some(r"C:\bin\concordia.exe"));
        assert_eq!(c["args"][0].as_str(), Some("mcp"));
        assert_eq!(c["env"][STATE_ENV].as_str(), Some(r"C:\state dir"));
        assert_eq!(t["mcp_servers"]["other"]["command"].as_str(), Some("y"));
        assert_eq!(
            sync_mcp(&h, ProviderKind::Codex, true, exe, st, true),
            Outcome::Current
        );
        assert_eq!(
            sync_mcp(&h, ProviderKind::Codex, false, exe, st, true),
            Outcome::Removed
        );
        assert_eq!(fs::read_to_string(&f).unwrap(), mine);
    }

    #[test]
    fn codex_toml_foreign_or_broken_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let h = home(tmp.path(), "codex-3");
        let f = h.join("config.toml");
        let (exe, st) = (Path::new("c.exe"), Path::new("s"));
        fs::write(&f, "[mcp_servers.concordia]\ncommand = \"mine\"\n").unwrap();
        assert!(matches!(
            sync_mcp(&h, ProviderKind::Codex, true, exe, st, true),
            Outcome::Blocked(_)
        ));
        assert_eq!(
            fs::read_to_string(&f).unwrap(),
            "[mcp_servers.concordia]\ncommand = \"mine\"\n"
        );
        fs::write(&f, "[oops\n").unwrap();
        assert!(matches!(
            sync_mcp(&h, ProviderKind::Codex, true, exe, st, true),
            Outcome::Blocked(_)
        ));
        assert_eq!(fs::read_to_string(&f).unwrap(), "[oops\n");
    }

    #[test]
    fn cursor_mcp_is_not_supported() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(
            sync_mcp(
                tmp.path(),
                ProviderKind::Cursor,
                true,
                Path::new("c"),
                Path::new("s"),
                true
            ),
            Outcome::Blocked(_)
        ));
    }
}
