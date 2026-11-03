//! What Concordia leaves inside homes it created: the awareness block and the optional MCP entry.
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

pub const AWARE_BEGIN: &str = "<!-- concordia:begin -->";
pub const AWARE_END: &str = "<!-- concordia:end -->";
const TOML_BEGIN: &str =
    "# concordia:begin (managed by Concordia; remove with [threads] mcp = false)";
const TOML_BEGIN_KEY: &str = "# concordia:begin";
const TOML_END: &str = "# concordia:end";
/// Name of the server entry in each CLI's config.
pub const SERVER_NAME: &str = "concordia";
/// Env var that points the server at the state folder (the one `store::default_store_root` reads).
pub const STATE_ENV: &str = "CONCORDIA_STATE_DIR";

/// The awareness text, at most 45 words.
pub const AWARE_TEXT: &str = "You are running inside Concordia, a terminal app that runs several accounts. A [concordia map] block is a pointer to an earlier conversation; run `concordia thread read <id>` (add --grep TEXT or --tail N) to read details. Do not paste whole threads.";

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
    pub awareness: Outcome,
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

/// Bring every Concordia-created home in line with `[threads] awareness` / `mcp`.
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
            awareness: sync_awareness(&home, provider, cfg.awareness, write),
            mcp: sync_mcp(&home, provider, cfg.mcp, exe, state_root, write),
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
        for (what, o) in [("awareness", &r.awareness), ("mcp", &r.mcp)] {
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
        for (name, o) in [("awareness", &r.awareness), ("mcp", &r.mcp)] {
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
// Awareness block
// ---------------------------------------------------------------------------------------------

fn instructions_file(home: &Path, provider: ProviderKind) -> Option<PathBuf> {
    match provider {
        ProviderKind::Claude => Some(home.join("CLAUDE.md")),
        ProviderKind::Codex => Some(home.join("AGENTS.md")),
        ProviderKind::Cursor => None,
    }
}

pub fn sync_awareness(home: &Path, provider: ProviderKind, want: bool, write: bool) -> Outcome {
    let Some(file) = instructions_file(home, provider) else {
        return Outcome::Blocked("not supported for this provider".into());
    };
    sync_text_block(&file, AWARE_BEGIN, AWARE_END, &[AWARE_TEXT], want, write)
}

/// Replace, add or remove the marker-delimited block in a text file. User text outside it is kept
/// byte for byte. `validate` runs on the new text before anything is written.
fn sync_text_block(
    file: &Path,
    begin: &str,
    end: &str,
    body: &[&str],
    want: bool,
    write: bool,
) -> Outcome {
    sync_text_block_checked(file, begin, end, body, want, write, &|_| Ok(()))
}

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
        let res = if !want && new.trim().is_empty() && existed {
            // We only ever left our own block: do not leave an empty file behind.
            fs::remove_file(file)
        } else {
            atomic_write(file, new.as_bytes())
        };
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

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if bytes.len() > 1024 * 1024 {
        return Err(std::io::Error::other(
            "native configuration exceeds the 1 MiB write bound; original retained",
        ));
    }
    let tmp = path.with_extension(format!("concordia-tmp-{}", std::process::id()));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
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
    let found = text.find(begin_key(begin));
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
    let Some(rel) = text[after_begin..].find(end) else {
        return Err("a concordia:begin marker has no matching end marker; left alone".into());
    };
    let stop = after_begin + rel + end.len();
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
    match provider {
        ProviderKind::Claude => sync_claude_json(home, want, exe, state_root, write),
        ProviderKind::Codex => sync_codex_toml(home, want, exe, state_root, write),
        ProviderKind::Cursor => Outcome::Blocked("not supported for this provider".into()),
    }
}

fn entry_json(exe: &Path, state_root: &Path) -> serde_json::Value {
    serde_json::json!({
        "type": "stdio",
        "command": exe.to_string_lossy(),
        "args": ["mcp"],
        "env": { STATE_ENV: state_root.to_string_lossy() },
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
    write: bool,
) -> Outcome {
    let file = home.join(".claude.json");
    let mut root = match read_configuration(&file) {
        Ok(b) => match serde_json::from_slice::<serde_json::Value>(&b) {
            Ok(v) if v.is_object() => v,
            _ => return Outcome::Blocked(".claude.json is not a JSON object; left alone".into()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Outcome::Blocked(format!("cannot read .claude.json: {e}")),
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
        let desired = entry_json(exe, state_root);
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
        if let Err(e) = atomic_write(&file, text.as_bytes()) {
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

fn sync_codex_toml(home: &Path, want: bool, exe: &Path, state_root: &Path, write: bool) -> Outcome {
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

    fn word_count(s: &str) -> usize {
        s.split_whitespace().count()
    }

    #[test]
    fn the_awareness_text_stays_short_and_names_the_pointer() {
        assert!(word_count(AWARE_TEXT) <= 45, "{}", word_count(AWARE_TEXT));
        assert!(AWARE_TEXT.contains("concordia thread read <id>"));
        assert!(AWARE_TEXT.contains("[concordia map]"));
    }

    #[test]
    fn splice_adds_replaces_removes_and_keeps_user_text() {
        let b = ["one"];
        let added = splice("my notes\n", AWARE_BEGIN, AWARE_END, Some(&b)).unwrap();
        assert_eq!(
            added,
            format!("my notes\n\n{AWARE_BEGIN}\none\n{AWARE_END}\n")
        );
        let again = splice(&added, AWARE_BEGIN, AWARE_END, Some(&b)).unwrap();
        assert_eq!(again, added, "idempotent");
        let changed = splice(&added, AWARE_BEGIN, AWARE_END, Some(&["two"])).unwrap();
        assert!(
            changed.contains("two") && !changed.contains("one") && changed.starts_with("my notes")
        );
        let gone = splice(&changed, AWARE_BEGIN, AWARE_END, None).unwrap();
        assert_eq!(gone, "my notes\n");
        assert!(splice(
            &format!("{AWARE_BEGIN}\nx"),
            AWARE_BEGIN,
            AWARE_END,
            Some(&b)
        )
        .is_err());
        let crlf = splice("a\r\nb\r\n", AWARE_BEGIN, AWARE_END, Some(&b)).unwrap();
        assert!(crlf.contains(&format!("{AWARE_BEGIN}\r\none\r\n{AWARE_END}\r\n")));
    }

    #[test]
    fn text_after_the_block_survives_a_replace_and_a_removal() {
        let t = format!("top\n{AWARE_BEGIN}\nold\n{AWARE_END}\nbottom\n");
        let r = splice(&t, AWARE_BEGIN, AWARE_END, Some(&["new"])).unwrap();
        assert_eq!(r, format!("top\n{AWARE_BEGIN}\nnew\n{AWARE_END}\nbottom\n"));
        assert_eq!(
            splice(&t, AWARE_BEGIN, AWARE_END, None).unwrap(),
            "top\nbottom\n"
        );
    }

    fn home(tmp: &Path, name: &str) -> PathBuf {
        let h = tmp.join(name);
        fs::create_dir_all(&h).unwrap();
        h
    }

    #[test]
    fn awareness_create_update_preserve_and_off() {
        let tmp = tempfile::tempdir().unwrap();
        let h = home(tmp.path(), "claude-2");
        let f = h.join("CLAUDE.md");
        assert_eq!(
            sync_awareness(&h, ProviderKind::Claude, true, false),
            Outcome::Updated
        );
        assert!(!f.exists(), "dry run writes nothing");
        assert_eq!(
            sync_awareness(&h, ProviderKind::Claude, true, true),
            Outcome::Updated
        );
        assert!(fs::read_to_string(&f).unwrap().contains(AWARE_TEXT));
        assert_eq!(
            sync_awareness(&h, ProviderKind::Claude, true, true),
            Outcome::Current
        );
        // User text outside the block, and a stale block, both handled.
        fs::write(
            &f,
            format!("# mine\n\n{AWARE_BEGIN}\nstale\n{AWARE_END}\n\nafter\n"),
        )
        .unwrap();
        assert_eq!(
            sync_awareness(&h, ProviderKind::Claude, true, true),
            Outcome::Updated
        );
        let s = fs::read_to_string(&f).unwrap();
        assert!(s.starts_with("# mine") && s.contains("after") && !s.contains("stale"));
        // Off removes only the block.
        assert_eq!(
            sync_awareness(&h, ProviderKind::Claude, false, true),
            Outcome::Removed
        );
        let s = fs::read_to_string(&f).unwrap();
        assert!(s.contains("# mine") && s.contains("after") && !s.contains("concordia:"));
        assert_eq!(
            sync_awareness(&h, ProviderKind::Claude, false, true),
            Outcome::Absent
        );
        // A file that held only our block is deleted again.
        let h2 = home(tmp.path(), "codex-3");
        sync_awareness(&h2, ProviderKind::Codex, true, true);
        assert!(h2.join("AGENTS.md").exists());
        sync_awareness(&h2, ProviderKind::Codex, false, true);
        assert!(!h2.join("AGENTS.md").exists());
        // Cursor has no supported file.
        assert!(matches!(
            sync_awareness(&h, ProviderKind::Cursor, true, true),
            Outcome::Blocked(_)
        ));
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
