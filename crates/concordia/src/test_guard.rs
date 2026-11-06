//! Test-only protection against accidentally using a user's real state.
use std::path::Path;

pub fn check_not_real_state(path: &Path) {
    if !cfg!(test) && std::env::var_os("CONCORDIA_TEST_GUARD").is_none() {
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
    use super::check_not_real_state;
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
}
