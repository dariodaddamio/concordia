//! Bounded, read-only workspace facts used by continuity handoffs.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceState {
    pub branch: String,
    pub head: String,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub files: Vec<String>,
    pub omitted_files: usize,
    pub added: u32,
    pub removed: u32,
    pub workers: Vec<String>,
}

/// A deliberately small projection so handoff code cannot serialize prompts or credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningWorker<'a> {
    pub id: &'a str,
    pub account: &'a str,
    pub state: &'a str,
}

pub fn attach_workers<'a>(
    state: &mut WorkspaceState,
    workers: impl IntoIterator<Item = RunningWorker<'a>>,
) {
    state.workers = workers
        .into_iter()
        .map(|w| format!("{} {} {}", w.id, w.account, w.state))
        .collect();
}

const FILE_CAP: usize = 20;

fn git(root: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let root = root.to_owned();
    let args = args.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut c = crate::process_ui::command("git");
        c.args(&args)
            .current_dir(root)
            .env("GIT_OPTIONAL_LOCKS", "0");
        let result = c.output().map_err(|e| e.to_string()).and_then(|o| {
            if o.status.success() {
                Ok(String::from_utf8_lossy(&o.stdout).into_owned())
            } else {
                Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
            }
        });
        let _ = tx.send(result);
    });
    rx.recv_timeout(timeout)
        .map_err(|_| String::from("timed out"))?
}

pub fn collect(root: &Path) -> Option<WorkspaceState> {
    collect_with_git(root, Duration::from_secs(2), |root, args, timeout| {
        git(root, args, timeout)
    })
}

fn collect_with_git<F>(root: &Path, timeout: Duration, mut run_git: F) -> Option<WorkspaceState>
where
    F: FnMut(&Path, &[&str], Duration) -> Result<String, String>,
{
    let branch = run_git(root, &["branch", "--show-current"], timeout)
        .ok()?
        .trim()
        .to_owned();
    let head = run_git(root, &["rev-parse", "--short", "HEAD"], timeout)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let status = run_git(
        root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
        timeout,
    )
    .ok()?;
    let mut state = WorkspaceState {
        branch,
        head,
        ..Default::default()
    };
    for line in status.lines() {
        if line.len() < 3 {
            continue;
        }
        let path = line[3..].replace('\n', "");
        if state.files.len() < FILE_CAP {
            state.files.push(format!("{} {}", &line[..2], path));
        } else {
            state.omitted_files += 1;
        }
    }
    if let Ok(upstream) = run_git(
        root,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
        timeout,
    ) {
        let p = upstream.split_whitespace().collect::<Vec<_>>();
        if p.len() == 2 {
            state.ahead = p[0].parse().ok();
            state.behind = p[1].parse().ok();
        }
    }
    if let Ok(diff) = run_git(root, &["diff", "--numstat", "HEAD"], timeout) {
        for l in diff.lines() {
            let p = l.split('\t').collect::<Vec<_>>();
            if p.len() >= 2 {
                state.added += p[0].parse::<u32>().unwrap_or(0);
                state.removed += p[1].parse::<u32>().unwrap_or(0);
            }
        }
    }
    Some(state)
}

pub fn summary(state: &WorkspaceState) -> String {
    let mut s = format!(
        "workspace: {}branch {} @{} ?? {} files uncommitted (+{}/-{})",
        if state.files.is_empty() {
            "clean; "
        } else {
            ""
        },
        state.branch,
        state.head,
        state.files.len() + state.omitted_files,
        state.added,
        state.removed
    );
    if let (Some(a), Some(b)) = (state.ahead, state.behind) {
        s.push_str(&format!(" ?? ahead {a}/behind {b}"));
    }
    if !state.workers.is_empty() {
        s.push_str(&format!(" ?? workers: {}", state.workers.join(", ")));
    }
    s
}

pub fn render(state: &WorkspaceState, max_chars: usize) -> String {
    let mut lines = vec![summary(state)];
    lines.extend(state.files.iter().map(|f| format!("workspace file: {f}")));
    if state.omitted_files > 0 {
        lines.push(format!("workspace files: +{} more", state.omitted_files));
    }
    let mut out = lines.join("\n");
    if out.chars().count() > max_chars {
        out = out.chars().take(max_chars).collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn git(dir: &Path, args: &[&str]) {
        let mut c = crate::process_ui::command("git");
        c.args(args).current_dir(dir);
        assert!(c.status().unwrap().success());
    }

    #[test]
    fn handoff_pack_includes_branch_and_dirty_files() {
        let d = tempdir().unwrap();
        git(d.path(), &["init", "-q", "-b", "fixture"]);
        fs::write(d.path().join("a.rs"), "x\n").unwrap();
        // The parallel suite can starve git past the interactive 2 s budget.
        let s = collect_with_git(d.path(), Duration::from_secs(30), super::git).unwrap();
        assert_eq!(s.branch, "fixture");
        assert!(s.files.iter().any(|f| f.contains("a.rs")));
    }

    #[test]
    fn workspace_state_is_capped() {
        let mut s = WorkspaceState {
            branch: "b".into(),
            head: "h".into(),
            ..Default::default()
        };
        for i in 0..25 {
            s.files.push(format!("?? f{i}"));
        }
        s.omitted_files = 5;
        assert!(render(&s, 120).chars().count() <= 120);
    }

    #[test]
    fn non_git_workspace_omits_state() {
        assert!(collect(Path::new("target/definitely-not-a-repo")).is_none());
    }

    #[test]
    fn running_workers_listed() {
        let mut s = WorkspaceState::default();
        attach_workers(
            &mut s,
            [RunningWorker {
                id: "7",
                account: "fixture",
                state: "thinking",
            }],
        );
        assert!(summary(&s).contains("7 fixture thinking"));
    }

    #[test]
    fn git_timeout_omits_state_without_failing() {
        let slow_git = |_root: &Path, _args: &[&str], timeout: Duration| {
            std::thread::sleep(timeout + Duration::from_millis(20));
            Err(String::from("timed out"))
        };
        assert!(collect_with_git(
            Path::new("fake-slow-git-fixture"),
            Duration::from_millis(1),
            slow_git,
        )
        .is_none());
    }
}
