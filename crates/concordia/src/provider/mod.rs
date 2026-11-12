pub mod claude;
pub mod codex;
pub mod compat;
pub mod cursor;
pub mod grok;
pub mod enforcement;
pub mod failure;
pub mod interaction;
pub mod launch;
pub(crate) mod npm;
pub mod recording;
pub(crate) mod session_guard;
pub mod stream_cards;
pub mod stream_json;
pub mod turn;

use crate::store::ProviderKind;
use std::path::PathBuf;

/// A provider's interactive slash surface. Concordia launches providers in their documented
/// machine-readable mode, so these entries are discoverable but not executable as native
/// commands until that mode exposes a command transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub description: &'static str,
}

const CODEX_COMMANDS: &[CommandSpec] = &[
    CommandSpec { name: "/compact", description: "summarize the current conversation" },
    CommandSpec { name: "/diff", description: "show the current diff" },
    CommandSpec { name: "/fork", description: "fork the current thread" },
    CommandSpec { name: "/init", description: "create an AGENTS.md file" },
    CommandSpec { name: "/mcp", description: "list or manage MCP tools" },
    CommandSpec { name: "/plan", description: "switch to Plan mode" },
    CommandSpec { name: "/review", description: "review current changes" },
    CommandSpec { name: "/status", description: "inspect session status" },
    CommandSpec { name: "/theme", description: "choose a terminal theme" },
    CommandSpec { name: "/vim", description: "toggle Vim composer keys" },
    CommandSpec { name: "/clear", description: "clear the conversation" },
    CommandSpec { name: "/export", description: "export the conversation" },
];

const CLAUDE_COMMANDS: &[CommandSpec] = &[
    CommandSpec { name: "/compact", description: "compact the conversation" },
    CommandSpec { name: "/clear", description: "clear conversation history" },
    CommandSpec { name: "/init", description: "create a CLAUDE.md file" },
    CommandSpec { name: "/mcp", description: "manage MCP servers" },
    CommandSpec { name: "/memory", description: "view or edit memory files" },
    CommandSpec { name: "/review", description: "review the current changes" },
    CommandSpec { name: "/rewind", description: "rewind a conversation" },
    CommandSpec { name: "/vim", description: "toggle Vim mode" },
    CommandSpec { name: "/help", description: "show Claude Code help" },
];

const CURSOR_COMMANDS: &[CommandSpec] = &[
    CommandSpec { name: "/auto-run", description: "toggle automatic command approval" },
    CommandSpec { name: "/new-chat", description: "start a new chat" },
    CommandSpec { name: "/vim", description: "toggle Vim keys" },
    CommandSpec { name: "/feedback", description: "send feedback to Cursor" },
    CommandSpec { name: "/copy-req-id", description: "copy the last request id" },
    CommandSpec { name: "/logout", description: "sign out of Cursor" },
    CommandSpec { name: "/quit", description: "exit Cursor Agent" },
];
const GROK_COMMANDS: &[CommandSpec] = &[
    CommandSpec { name: "/model", description: "choose a Grok model" },
    CommandSpec { name: "/settings", description: "change Grok Build settings" },
    CommandSpec { name: "/mcp", description: "manage MCP servers" },
];

pub fn command_specs(kind: ProviderKind) -> &'static [CommandSpec] {
    match kind {
        ProviderKind::Codex => CODEX_COMMANDS,
        ProviderKind::Claude => CLAUDE_COMMANDS,
        ProviderKind::Cursor => CURSOR_COMMANDS,
        ProviderKind::Grok => GROK_COMMANDS,
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn each_provider_exposes_documented_native_commands() {
        for (kind, expected) in [
            (ProviderKind::Codex, "/compact"),
            (ProviderKind::Claude, "/mcp"),
            (ProviderKind::Cursor, "/auto-run"),
        ] {
            assert!(command_specs(kind).iter().any(|command| command.name == expected));
            assert!(command_specs(kind).iter().all(|command| !command.description.is_empty()));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocateSource {
    Override,
    Path,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub path: PathBuf,
    pub source: LocateSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotFound {
    pub searched: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct LocateEnv {
    pub override_path: Option<String>,
    pub path: Option<String>,
    pub userprofile: Option<PathBuf>,
    pub appdata: Option<PathBuf>,
    pub localappdata: Option<PathBuf>,
    pub workspace_root: Option<PathBuf>,
}

impl LocateEnv {
    pub fn from_process(kind: ProviderKind) -> Self {
        let override_var = match kind {
            ProviderKind::Codex => "CONCORDIA_CODEX_BIN",
            ProviderKind::Claude => "CONCORDIA_CLAUDE_BIN",
            ProviderKind::Cursor => "CONCORDIA_AGENT_BIN",
            ProviderKind::Grok => "CONCORDIA_GROK_BIN",
        };
        Self {
            override_path: std::env::var(override_var).ok(),
            path: std::env::var("PATH").ok(),
            userprofile: std::env::var_os("USERPROFILE").map(PathBuf::from),
            appdata: std::env::var_os("APPDATA").map(PathBuf::from),
            localappdata: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            workspace_root: std::env::current_dir().ok(),
        }
    }
}

/// Whether a PATH directory must be excluded from executable discovery.
///
/// Project directories are untrusted because a checked-out project can plant a provider name,
/// but a user's home directory is a normal place for CLI installations. Relative PATH entries
/// are rejected by the caller before this predicate is used.
pub(crate) fn is_untrusted_path_dir(
    dir: &std::path::Path,
    cwd: &std::path::Path,
    workspace_root: Option<&std::path::Path>,
    home: Option<&std::path::Path>,
) -> bool {
    let normalize =
        |path: &std::path::Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let dir = normalize(dir);
    let cwd = normalize(cwd);
    if dir == cwd {
        return true;
    }
    let Some(root) = workspace_root.map(normalize) else {
        return false;
    };
    let root_is_home_or_ancestor = home
        .map(normalize)
        .is_some_and(|home| home == root || home.starts_with(&root));
    let root_is_drive = root.parent().is_none_or(|parent| parent == root);
    !root_is_home_or_ancestor && !root_is_drive && dir.starts_with(root)
}

pub fn locate(kind: ProviderKind, env: &LocateEnv) -> Result<Located, NotFound> {
    let (name, fallback): (&str, Vec<PathBuf>) = match kind {
        ProviderKind::Claude => (
            "claude",
            vec![
                env.userprofile
                    .as_ref()
                    .map(|p| p.join(".local/bin/claude.exe")),
                env.appdata.as_ref().map(|p| p.join("npm/claude.cmd")),
            ]
            .into_iter()
            .flatten()
            .collect(),
        ),
        ProviderKind::Codex => (
            "codex",
            vec![
                env.localappdata
                    .as_ref()
                    .map(|p| p.join("Programs/OpenAI/Codex/bin/codex.exe")),
                env.appdata.as_ref().map(|p| p.join("npm/codex.cmd")),
            ]
            .into_iter()
            .flatten()
            .collect(),
        ),
        ProviderKind::Cursor => (
            "agent",
            vec![
                env.localappdata
                    .as_ref()
                    .map(|p| p.join("cursor-agent/agent.cmd")),
                env.localappdata
                    .as_ref()
                    .map(|p| p.join("cursor-agent/cursor-agent.cmd")),
            ]
            .into_iter()
            .flatten()
            .collect(),
        ),
        ProviderKind::Grok => ("grok", vec![env.userprofile.as_ref().map(|p| p.join(".grok/bin/grok.exe"))].into_iter().flatten().collect()),
    };
    let mut searched = Vec::new();
    if let Some(p) = env
        .override_path
        .as_deref()
        .filter(|p| !p.trim().is_empty())
    {
        let p = PathBuf::from(p);
        return Ok(Located {
            path: p,
            source: LocateSource::Override,
        });
    }
    let names = if kind == ProviderKind::Cursor {
        ["agent", "cursor-agent"].as_slice()
    } else {
        &[name]
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let safe_path = env.path.as_deref().map(|raw| {
        std::env::split_paths(raw)
            .filter(|p| p.is_absolute() && !p.as_os_str().is_empty())
            .filter(|dir| {
                !is_untrusted_path_dir(
                    dir,
                    &cwd,
                    env.workspace_root.as_deref(),
                    env.userprofile.as_deref(),
                )
            })
            .collect::<Vec<_>>()
    });
    let safe_path = safe_path
        .as_ref()
        .map(|p| std::env::join_paths(p).unwrap_or_default());
    if let Some(candidate) = names
        .iter()
        .find_map(|n| which::which_in(n, safe_path.as_deref(), "").ok())
    {
        let parent = candidate.parent().unwrap_or(candidate.as_path());
        if !is_untrusted_path_dir(
            parent,
            &cwd,
            env.workspace_root.as_deref(),
            env.userprofile.as_deref(),
        ) {
            return Ok(Located {
                path: candidate,
                source: LocateSource::Path,
            });
        }
    }
    searched.extend(fallback.iter().cloned());
    for p in fallback {
        if p.is_file() {
            return Ok(Located {
                path: p.clone(),
                source: LocateSource::Fallback,
            });
        }
    }
    Err(NotFound { searched })
}

pub fn locate_real(kind: ProviderKind) -> Result<Located, NotFound> {
    locate(kind, &LocateEnv::from_process(kind))
}

pub use stream_cards::{format_card_line, upsert_card, CardStatus, StreamCard};
pub use turn::{
    plan_provider_turn, run_provider_turn, run_provider_turn_with_owner,
    run_provider_turn_with_progress, spawn_and_cancel_mock, PlannedTurn, ProviderTurnRequest,
    RunningTurn, TurnOutcome, TurnProgress,
};

/// Explicit provider CLI path from the environment (custom installs, and the fake provider in
/// tests). Empty values are ignored.
pub fn bin_override(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn path_fixture(dir: &std::path::Path, name: &str) -> PathBuf {
        #[cfg(windows)]
        let path = dir.join(format!("{name}.exe"));
        #[cfg(not(windows))]
        let path = dir.join(name);

        fs::write(&path, "").unwrap();
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions).unwrap();
        }
        path
    }

    fn env(root: &std::path::Path) -> LocateEnv {
        LocateEnv {
            path: Some(String::new()),
            userprofile: Some(root.join("user")),
            appdata: Some(root.join("app")),
            localappdata: Some(root.join("local")),
            ..Default::default()
        }
    }

    #[test]
    fn locate_finds_each_provider_at_its_standard_location_and_first_wins() {
        let root = tempfile::tempdir().unwrap();
        for kind in [
            ProviderKind::Codex,
            ProviderKind::Claude,
            ProviderKind::Cursor,
        ] {
            let e = env(root.path());
            let first = match kind {
                ProviderKind::Codex => e
                    .localappdata
                    .as_ref()
                    .unwrap()
                    .join("Programs/OpenAI/Codex/bin/codex.exe"),
                ProviderKind::Claude => e
                    .userprofile
                    .as_ref()
                    .unwrap()
                    .join(".local/bin/claude.exe"),
                ProviderKind::Cursor => e
                    .localappdata
                    .as_ref()
                    .unwrap()
                    .join("cursor-agent/agent.cmd"),
                ProviderKind::Grok => e.userprofile.as_ref().unwrap().join(".grok/bin/grok.exe"),
            };
            let second = match kind {
                ProviderKind::Codex => e.appdata.as_ref().unwrap().join("npm/codex.cmd"),
                ProviderKind::Claude => e.appdata.as_ref().unwrap().join("npm/claude.cmd"),
                ProviderKind::Cursor => e
                    .localappdata
                    .as_ref()
                    .unwrap()
                    .join("cursor-agent/cursor-agent.cmd"),
                ProviderKind::Grok => e.userprofile.as_ref().unwrap().join(".grok/bin/grok.cmd"),
            };
            fs::create_dir_all(first.parent().unwrap()).unwrap();
            fs::write(&first, "").unwrap();
            fs::create_dir_all(second.parent().unwrap()).unwrap();
            fs::write(&second, "").unwrap();
            let found = locate(kind, &e).unwrap();
            assert_eq!(found.path, first);
            assert_eq!(found.source, LocateSource::Fallback);
        }
    }

    #[test]
    fn locate_precedence_is_override_then_path_then_fallback_and_missing_lists_all() {
        let root = tempfile::tempdir().unwrap();
        let mut e = env(root.path());
        e.override_path = Some("claude".into());
        assert_eq!(
            locate(ProviderKind::Claude, &e).unwrap().path,
            PathBuf::from("claude")
        );
        e.override_path = None;
        let path = root.path().join("bin");
        fs::create_dir_all(&path).unwrap();
        let claude = path_fixture(&path, "claude");
        e.path = Some(path.to_string_lossy().into());
        assert_eq!(
            locate(ProviderKind::Claude, &e).unwrap(),
            Located {
                path: claude,
                source: LocateSource::Path,
            }
        );
        e.path = Some(String::new());
        let missing = locate(ProviderKind::Claude, &e).unwrap_err();
        assert_eq!(missing.searched.len(), 2);
    }

    #[test]
    fn locate_cursor_tries_agent_then_cursor_agent_on_path() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("bin");
        fs::create_dir_all(&dir).unwrap();
        let cursor_agent = path_fixture(&dir, "cursor-agent");
        let mut e = env(root.path());
        e.path = Some(dir.to_string_lossy().into());
        assert_eq!(locate(ProviderKind::Cursor, &e).unwrap().path, cursor_agent);

        let agent = path_fixture(&dir, "agent");
        assert_eq!(locate(ProviderKind::Cursor, &e).unwrap().path, agent);
    }

    #[test]
    fn planted_binary_in_workspace_is_not_used() {
        let root = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let project_bin = root.path().join("bin");
        fs::create_dir_all(&project_bin).unwrap();
        for name in ["codex", "claude", "agent"] {
            path_fixture(root.path(), name);
        }
        let relative = root.path().join("relative");
        fs::create_dir_all(&relative).unwrap();
        let path = std::env::join_paths([
            root.path().as_os_str(),
            relative.as_os_str(),
            Path::new(".").as_os_str(),
        ])
        .unwrap();
        let mut e = env(root.path());
        e.userprofile = Some(home.path().to_path_buf());
        e.path = Some(path.to_string_lossy().into_owned());
        e.workspace_root = Some(root.path().to_path_buf());
        for kind in [
            ProviderKind::Codex,
            ProviderKind::Claude,
            ProviderKind::Cursor,
        ] {
            assert!(
                locate(kind, &e).is_err(),
                "{kind:?} was planted in the project"
            );
        }
    }

    #[test]
    fn home_as_workspace_still_finds_clis_on_path() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let claude = path_fixture(&bin, "claude");
        let mut e = env(home.path());
        e.userprofile = Some(home.path().to_path_buf());
        e.workspace_root = Some(home.path().to_path_buf());
        e.path = Some(bin.to_string_lossy().into_owned());
        assert_eq!(locate(ProviderKind::Claude, &e).unwrap().path, claude);
    }

    #[test]
    fn is_untrusted_path_dir_rules() {
        let root = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();
        let cwd = root.path().join("project");
        let project = cwd.clone();
        let inside = project.join("bin");
        let home = home_dir.path().to_path_buf();
        let home_bin = home.join("bin");
        let home_parent = home.parent().unwrap().to_path_buf();
        let drive_root = Path::new(std::path::MAIN_SEPARATOR_STR).to_path_buf();
        let cases = [
            (cwd.clone(), true),
            (inside, true),
            (home.clone(), false),
            (home_bin, false),
            (home_parent, false),
            (drive_root, false),
            (Path::new("relative").to_path_buf(), false),
            (Path::new(".").to_path_buf(), false),
        ];
        for (dir, expected) in cases {
            let actual =
                dir.is_absolute() && is_untrusted_path_dir(&dir, &cwd, Some(&project), Some(&home));
            assert_eq!(actual, expected, "{dir:?}");
        }
    }

    #[test]
    fn missing_cli_fails_instead_of_spawning_a_bare_name() {
        let env = LocateEnv {
            override_path: None,
            path: Some(String::new()),
            userprofile: None,
            appdata: None,
            localappdata: None,
            workspace_root: None,
        };
        let cwd = tempfile::tempdir().unwrap();
        let codex = crate::provider::codex::plan_turn_argv_with(
            &crate::provider::codex::TurnRequest {
                prompt: "x",
                cwd: cwd.path(),
                codex_home: None,
                model: None,
                native_session_id: None,
                trust: crate::store::TrustMode::Plan,
            },
            &env,
        );
        let claude = crate::provider::claude::plan_claude_turn_with(
            &crate::provider::claude::ClaudeTurnRequest {
                prompt: "x",
                cwd: cwd.path(),
                config_dir: None,
                model: None,
                native_session_id: None,
                trust: crate::store::TrustMode::Plan,
            },
            &env,
        );
        let cursor = crate::provider::cursor::plan_cursor_turn_with(
            &crate::provider::cursor::CursorTurnRequest {
                prompt: "x",
                cwd: cwd.path(),
                home_path: None,
                model: None,
                native_session_id: None,
                trust: crate::store::TrustMode::Plan,
            },
            &env,
        );
        assert!(codex.is_err() && claude.is_err() && cursor.is_err());
        assert!(!format!("{codex:?}{claude:?}{cursor:?}").contains("command: \"codex\""));
    }
}
pub(crate) mod edit_display;
