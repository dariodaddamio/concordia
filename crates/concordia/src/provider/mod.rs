pub mod claude;
pub mod codex;
pub mod compat;
pub mod cursor;
pub mod enforcement;
pub mod interaction;
pub mod launch;
pub(crate) mod npm;
mod session_guard;
pub mod stream_cards;
pub mod stream_json;
pub mod turn;

use crate::store::ProviderKind;
use std::path::PathBuf;

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
    let safe_path = env.path.as_deref().map(|raw| {
        std::env::split_paths(raw)
            .filter(|p| !p.as_os_str().is_empty() && p != std::path::Path::new("."))
            .filter(|dir| {
                let cwd = std::env::current_dir().ok();
                let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
                if cwd
                    .as_ref()
                    .is_some_and(|c| dir == c.canonicalize().unwrap_or_else(|_| c.clone()))
                {
                    return false;
                }
                !env.workspace_root.as_ref().is_some_and(|root| {
                    dir.starts_with(root.canonicalize().unwrap_or_else(|_| root.clone()))
                })
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
        let cwd = std::env::current_dir().ok();
        let unsafe_hit = cwd.as_ref().is_some_and(|c| parent == c)
            || env
                .workspace_root
                .as_ref()
                .is_some_and(|r| parent.starts_with(r));
        if !unsafe_hit {
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
    let override_var = match kind {
        ProviderKind::Codex => "CONCORDIA_CODEX_BIN",
        ProviderKind::Claude => "CONCORDIA_CLAUDE_BIN",
        ProviderKind::Cursor => "CONCORDIA_AGENT_BIN",
    };
    locate(
        kind,
        &LocateEnv {
            override_path: std::env::var(override_var).ok(),
            path: std::env::var("PATH").ok(),
            userprofile: std::env::var_os("USERPROFILE").map(PathBuf::from),
            appdata: std::env::var_os("APPDATA").map(PathBuf::from),
            localappdata: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            workspace_root: std::env::current_dir().ok(),
        },
    )
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
            };
            let second = match kind {
                ProviderKind::Codex => e.appdata.as_ref().unwrap().join("npm/codex.cmd"),
                ProviderKind::Claude => e.appdata.as_ref().unwrap().join("npm/claude.cmd"),
                ProviderKind::Cursor => e
                    .localappdata
                    .as_ref()
                    .unwrap()
                    .join("cursor-agent/cursor-agent.cmd"),
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
}
pub(crate) mod edit_display;
