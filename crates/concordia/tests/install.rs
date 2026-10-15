//! Installing must always make `<bin>\concordia.exe` (what `concordia` on PATH runs) the newest build, even
//! when an older concordia is still running and holding that file open. This is the failure that left
//! `concordia` resolving to a months-old binary: the installer skipped the locked file and used another
//! name that nothing on PATH looked at.
//!
//! Scratch install dir via `CONCORDIA_INSTALL_DIR`; the real install, PATH and `CONCORDIA_EXE` are untouched.

use concordia::update::{current_pointer_path, install_binary};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

static LOCK: Mutex<()> = Mutex::new(());

const CONCORDIA: &str = env!("CARGO_BIN_EXE_concordia");
const FAKE: &str = env!("CARGO_BIN_EXE_concordia-fake-provider");

struct Sandbox {
    _guard: MutexGuard<'static, ()>,
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CONCORDIA_INSTALL_DIR", dir.path());
        Sandbox { _guard: guard, dir }
    }

    fn concordia_exe(&self) -> PathBuf {
        self.dir.path().join("concordia.exe")
    }

    fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::env::remove_var("CONCORDIA_INSTALL_DIR");
        std::env::remove_var("CONCORDIA_EXE");
    }
}

fn eventually(mut cond: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(8) {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    cond()
}

/// Run a copy of the fake provider *as* `path`, so Windows holds that file open.
fn run_as(path: &Path, pid_file: &Path) -> Child {
    std::fs::copy(FAKE, path).unwrap();
    let child = Command::new(path)
        .env("FAKE_PROVIDER_MODE", "hang")
        .env("FAKE_PROVIDER_PID", pid_file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    assert!(
        eventually(|| std::fs::read_to_string(pid_file).is_ok_and(|s| !s.trim().is_empty())),
        "the running stand-in never started"
    );
    child
}

#[test]
fn a_fresh_install_lands_at_concordia_exe_and_the_pointer_follows_it() {
    let sb = Sandbox::new();
    let dest = install_binary(Path::new(CONCORDIA)).unwrap();
    assert_eq!(dest, sb.concordia_exe());
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        std::fs::read(CONCORDIA).unwrap(),
        "installed bytes differ"
    );
    let pointer = std::fs::read_to_string(current_pointer_path()).unwrap();
    assert_eq!(PathBuf::from(pointer.trim()), dest);
    // No staging or swap debris.
    assert!(
        !sb.names()
            .iter()
            .any(|n| n.contains(".new-") || n.contains(".old-")),
        "{:?}",
        sb.names()
    );
    // And it runs.
    let out = Command::new(&dest).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("concordia "));
}

#[test]
fn an_update_replaces_concordia_exe_even_while_an_older_concordia_is_running_from_it() {
    let sb = Sandbox::new();
    let pid_file = sb.dir.path().join("pid.txt");
    let mut running = run_as(&sb.concordia_exe(), &pid_file);

    // Prove the premise: Windows really refuses to overwrite a running exe.
    assert!(
        std::fs::copy(CONCORDIA, sb.concordia_exe()).is_err(),
        "the stand-in should hold concordia.exe open; the test would prove nothing"
    );

    let dest = install_binary(Path::new(CONCORDIA))
        .expect("a locked concordia.exe must not block an update");
    assert_eq!(dest, sb.concordia_exe());
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        std::fs::read(CONCORDIA).unwrap(),
        "`concordia` must now resolve to the NEW build, not the one still running"
    );
    // The running old image was moved aside, not killed.
    assert!(
        sb.names()
            .iter()
            .any(|n| n.starts_with("concordia.exe.old-")),
        "{:?}",
        sb.names()
    );
    assert!(
        running.try_wait().unwrap().is_none(),
        "the old pane must keep running"
    );

    // Once the old pane exits, the next install sweeps its leftover file.
    running.kill().unwrap();
    running.wait().unwrap();
    install_binary(Path::new(CONCORDIA)).unwrap();
    assert!(
        eventually(|| !sb
            .names()
            .iter()
            .any(|n| n.starts_with("concordia.exe.old-"))),
        "leftover not swept: {:?}",
        sb.names()
    );
}

#[test]
fn installing_something_that_does_not_run_changes_nothing() {
    let sb = Sandbox::new();
    install_binary(Path::new(CONCORDIA)).unwrap();
    let before = std::fs::read(sb.concordia_exe()).unwrap();
    let names_before = sb.names();

    let junk = sb.dir.path().join("..").join("junk-not-installed.exe");
    let junk_dir = tempfile::tempdir().unwrap();
    let junk = junk_dir.path().join(junk.file_name().unwrap());
    std::fs::write(&junk, vec![0x4du8; 200_000]).unwrap();
    assert!(install_binary(&junk).is_err());

    assert_eq!(
        std::fs::read(sb.concordia_exe()).unwrap(),
        before,
        "a bad update must leave the working install alone"
    );
    assert_eq!(sb.names(), names_before);
}

#[test]
fn concordia_install_installs_this_exe_where_path_will_find_it() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(CONCORDIA)
        .arg("install")
        .env("CONCORDIA_INSTALL_DIR", dir.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("installed"), "{text}");
    assert!(text.contains("NEW terminal"), "{text}");
    let installed = dir.path().join("concordia.exe");
    assert!(installed.is_file());
    // With an explicit install dir concordia must not edit the user's PATH.
    assert!(!text.contains("to your PATH"), "{text}");
    let v = Command::new(&installed).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&v.stdout).starts_with("concordia "));

    // Running `install` again from the installed copy itself (replacing its own image) works.
    let again = Command::new(&installed)
        .arg("install")
        .env("CONCORDIA_INSTALL_DIR", dir.path())
        .output()
        .unwrap();
    assert!(
        again.status.success(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
}

#[test]
fn update_rebuilds_and_installs_from_any_directory_once_the_checkout_is_recorded() {
    use concordia::update::{record_source_root, update_auto, UpdateOutcome};
    let sb = Sandbox::new();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    record_source_root(&repo.canonicalize().unwrap());

    // A pane opened somewhere unrelated, as happens with a normal WezTerm tab.
    let elsewhere = tempfile::tempdir().unwrap();
    match update_auto(elsewhere.path()).expect("source update must work from any directory") {
        UpdateOutcome::Installed(r) => {
            assert_eq!(r.install_path, sb.concordia_exe());
            let out = Command::new(&r.install_path)
                .arg("--version")
                .output()
                .unwrap();
            assert!(String::from_utf8_lossy(&out.stdout).starts_with("concordia "));
        }
        UpdateOutcome::UpToDate(m) => panic!("a source update always installs: {m}"),
    }
    assert!(sb.concordia_exe().is_file());
}
