//! Test-only protection against accidentally using a user's real state.
use std::path::{Path, PathBuf};

fn enabled() -> bool {
    cfg!(test) || std::env::var_os("CONCORDIA_TEST_GUARD").is_some()
}

pub(crate) fn is_enabled() -> bool {
    enabled()
}

fn configured_root(variable: &str, label: &str) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| {
            panic!("CONCORDIA_TEST_GUARD=1 requires {variable} to point at the test {label}")
        })
}

/// Isolated home used for provider path defaults while tests are running.
pub fn test_home() -> PathBuf {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    if !enabled() {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    }
    ROOT.get_or_init(|| {
        let root = if cfg!(test) && std::env::var_os("CONCORDIA_TEST_GUARD").is_none() {
            std::env::temp_dir()
                .join(format!("concordia-unit-{}", std::process::id()))
                .join("home")
        } else {
            configured_root("CONCORDIA_TEST_HOME", "home")
        };
        std::fs::create_dir_all(&root).expect("create test provider home");
        root
    })
    .clone()
}

/// Isolated Concordia state root used when no explicit state directory was supplied.
pub fn test_state_root() -> PathBuf {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    if !enabled() {
        return dirs::data_local_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
            .join("concordia");
    }
    ROOT.get_or_init(|| {
        let root = if cfg!(test) && std::env::var_os("CONCORDIA_TEST_GUARD").is_none() {
            std::env::temp_dir()
                .join(format!("concordia-unit-{}", std::process::id()))
                .join("state")
        } else {
            configured_root("CONCORDIA_STATE_DIR", "state")
        };
        std::fs::create_dir_all(&root).expect("create test state root");
        root
    })
    .clone()
}

pub fn check_not_real_state(path: &Path) {
    if !enabled() {
        return;
    }
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut roots = Vec::new();
    if let Some(data) = dirs::data_local_dir() {
        roots.push(data.join("concordia"));
    }
    if let Some(home) = dirs::home_dir() {
        roots.extend([
            home.join(".codex"),
            home.join(".codex-2"),
            home.join(".claude"),
            home.join(".cursor"),
        ]);
        #[cfg(unix)]
        roots.push(home.join(".local/share/concordia"));
    }
    if roots
        .iter()
        .any(|root| path == *root || path.starts_with(root))
    {
        panic!("test touched real user state: {}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::{check_not_real_state, test_home, test_state_root};
    #[test]
    fn guard_rejects_real_state_paths() {
        let home = dirs::home_dir().expect("test profile home");
        let data = dirs::data_local_dir().expect("test local data dir");
        for path in [data.join("concordia"), home.join(".claude")] {
            assert!(std::panic::catch_unwind(|| check_not_real_state(&path)).is_err());
        }
        let temp = tempfile::tempdir().unwrap();
        check_not_real_state(temp.path());
    }

    #[test]
    fn default_roots_are_redirected_under_the_guard() {
        let state = crate::store::default_store_root();
        let home = crate::accounts::profile_home();
        assert!(state.starts_with(test_state_root()));
        assert!(home.starts_with(test_home()));
        for name in [".codex", ".codex-2", ".claude", ".cursor"] {
            assert!(home.join(name).starts_with(test_home()));
        }
        assert!(!state.starts_with(dirs::data_local_dir().unwrap().join("concordia")));
        let real_home = dirs::home_dir().unwrap();
        assert_ne!(home.join(".codex"), real_home.join(".codex"));
        assert_ne!(home.join(".codex-2"), real_home.join(".codex-2"));
        assert_ne!(home.join(".claude"), real_home.join(".claude"));
        assert_ne!(home.join(".cursor"), real_home.join(".cursor"));
    }
}
