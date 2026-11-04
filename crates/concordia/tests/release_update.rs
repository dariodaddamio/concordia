//! Release-based self-update, end to end: real concordia binary as the payload, a real local HTTP
//! server, real PowerShell downloads and hashing for the happy path, and an in-memory fetcher to
//! inject every failure. The install dir is a scratch directory (`CONCORDIA_INSTALL_DIR`), so these
//! tests never touch the real install pointer, `CONCORDIA_EXE`, or the npm shim.
//!
//! The invariant behind every failure case: if any check fails, **nothing is installed**.

use anyhow::{anyhow, Result};
use concordia::release::{
    check, config_from, install_release, Check, Config, Fetcher, PowerShellFetcher, Release,
    ASSET_NAME, SUMS_NAME,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

const CONCORDIA_EXE: &str = env!("CARGO_BIN_EXE_concordia");
const FAKE_EXE: &str = env!("CARGO_BIN_EXE_concordia-fake-provider");

struct Sandbox {
    _guard: MutexGuard<'static, ()>,
    install: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let install = tempfile::tempdir().unwrap();
        std::env::set_var("CONCORDIA_INSTALL_DIR", install.path());
        Sandbox {
            _guard: guard,
            install,
        }
    }

    fn installed_files(&self) -> Vec<String> {
        std::fs::read_dir(self.install.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect()
    }

    fn assert_nothing_installed(&self) {
        let files = self.installed_files();
        assert!(files.is_empty(), "a failed update installed {files:?}");
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::env::remove_var("CONCORDIA_INSTALL_DIR");
        // install_binary sets CONCORDIA_EXE in this process.
        std::env::remove_var("CONCORDIA_EXE");
    }
}

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn real_concordia() -> Vec<u8> {
    std::fs::read(CONCORDIA_EXE).unwrap()
}

/// `concordia X.Y.Z (stamp)` as the real binary reports it, so tags can match it.
fn real_version() -> String {
    concordia::version::VERSION.to_string()
}

fn release_for(tag: &str, base: &str) -> Release {
    Release {
        tag: tag.to_string(),
        version: concordia::release::Version::parse(tag).unwrap(),
        asset_url: format!("{base}/dl/{tag}/{ASSET_NAME}"),
        sums_url: format!("{base}/dl/{tag}/{SUMS_NAME}"),
        notes: String::new(),
        published_at: String::new(),
        html_url: String::new(),
    }
}

fn sums_for(payload: &[u8]) -> Vec<u8> {
    format!("{}  {ASSET_NAME}\n", sha_hex(payload)).into_bytes()
}

// ---- an in-memory fetcher ----------------------------------------------------------------

#[derive(Default)]
struct Mock {
    /// file name (last URL segment) -> bytes
    files: HashMap<String, Vec<u8>>,
}

impl Mock {
    fn serving(payload: Vec<u8>, sums: Vec<u8>) -> Self {
        let mut m = Mock::default();
        m.files.insert(ASSET_NAME.into(), payload);
        m.files.insert(SUMS_NAME.into(), sums);
        m
    }
    fn honest(payload: Vec<u8>) -> Self {
        let sums = sums_for(&payload);
        Mock::serving(payload, sums)
    }
}

impl Fetcher for Mock {
    fn get_text(&self, _url: &str) -> Result<String> {
        unreachable!("install tests do not list releases")
    }
    fn download(&self, url: &str, dest: &Path) -> Result<()> {
        let name = url.rsplit('/').next().unwrap();
        let bytes = self.files.get(name).ok_or_else(|| anyhow!("404 {url}"))?;
        std::fs::write(dest, bytes)?;
        Ok(())
    }
    fn sha256(&self, path: &Path) -> Result<String> {
        Ok(sha_hex(&std::fs::read(path)?))
    }
}

fn cfg() -> Config {
    config_from(Some("o/n"), None, None, None, "0.0.0").unwrap()
}

fn install_with(mock: &Mock, tag: &str) -> Result<concordia::update::UpdateResult> {
    install_release(
        &cfg(),
        &release_for(tag, "https://dl.example"),
        "0.0.0",
        mock,
    )
}

// ---- failure cases: nothing may be installed ----------------------------------------------

#[test]
fn a_tampered_download_is_rejected_by_checksum_and_nothing_is_installed() {
    let sb = Sandbox::new();
    let good = real_concordia();
    let mut tampered = good.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xff;
    // The release's checksum describes the good file; the download was altered.
    let mock = Mock::serving(tampered, sums_for(&good));
    let err = install_with(&mock, &format!("v{}", real_version())).unwrap_err();
    assert!(err.to_string().contains("checksum mismatch"), "{err:#}");
    sb.assert_nothing_installed();
}

#[test]
fn a_matching_checksum_does_not_excuse_a_file_that_does_not_run() {
    let sb = Sandbox::new();
    let junk = vec![0x4du8; 300_000]; // big enough to look plausible, but not an executable
    let err = install_with(&Mock::honest(junk), &format!("v{}", real_version())).unwrap_err();
    assert!(err.to_string().contains("does not run"), "{err:#}");
    sb.assert_nothing_installed();
}

#[test]
fn a_valid_executable_that_is_not_concordia_is_refused() {
    let sb = Sandbox::new();
    let other = std::fs::read(FAKE_EXE).unwrap();
    let err = install_with(&Mock::honest(other), &format!("v{}", real_version())).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("not an concordia binary") || msg.contains("does not run"),
        "{msg}"
    );
    sb.assert_nothing_installed();
}

#[test]
fn a_release_that_ships_the_wrong_version_is_refused() {
    let sb = Sandbox::new();
    // The tag claims 9.9.9 but the binary inside is this build.
    let err = install_with(&Mock::honest(real_concordia()), "v9.9.9").unwrap_err();
    assert!(err.to_string().contains("nothing was installed"), "{err:#}");
    assert!(err.to_string().contains("reports"), "{err:#}");
    sb.assert_nothing_installed();
}

#[test]
fn an_implausibly_small_or_large_download_is_refused() {
    let sb = Sandbox::new();
    let tiny = vec![1u8; 10];
    let err = install_with(&Mock::honest(tiny), &format!("v{}", real_version())).unwrap_err();
    assert!(err.to_string().contains("not a plausible"), "{err:#}");
    sb.assert_nothing_installed();
}

#[test]
fn a_missing_or_malformed_checksum_entry_stops_the_update() {
    let sb = Sandbox::new();
    let payload = real_concordia();
    let tag = format!("v{}", real_version());

    let no_entry = Mock::serving(payload.clone(), b"deadbeef  something-else.exe\n".to_vec());
    let err = install_with(&no_entry, &tag).unwrap_err();
    assert!(err.to_string().contains("no valid entry"), "{err:#}");

    let garbage = Mock::serving(payload.clone(), b"not a checksum file".to_vec());
    assert!(install_with(&garbage, &tag).is_err());

    let mut no_sums = Mock::default();
    no_sums.files.insert(ASSET_NAME.into(), payload);
    let err = install_with(&no_sums, &tag).unwrap_err();
    assert!(format!("{err:#}").contains("404"), "{err:#}");
    sb.assert_nothing_installed();
}

#[test]
fn a_missing_asset_is_a_clear_error() {
    let sb = Sandbox::new();
    let mut m = Mock::default();
    m.files.insert(SUMS_NAME.into(), sums_for(b"x"));
    let err = install_with(&m, &format!("v{}", real_version())).unwrap_err();
    assert!(format!("{err:#}").contains("404"), "{err:#}");
    sb.assert_nothing_installed();
}

#[test]
fn non_https_asset_urls_are_refused_before_any_download() {
    let sb = Sandbox::new();
    let rel = release_for(&format!("v{}", real_version()), "http://evil.example");
    let err = install_release(&cfg(), &rel, "0.0.0", &Mock::honest(real_concordia())).unwrap_err();
    assert!(err.to_string().contains("non-https"), "{err:#}");
    sb.assert_nothing_installed();
}

// ---- success ----------------------------------------------------------------------------------

fn assert_installed_and_active(sb: &Sandbox, payload: &[u8]) -> PathBuf {
    let pointer = sb.install.path().join("current");
    let target = PathBuf::from(std::fs::read_to_string(&pointer).unwrap().trim());
    assert!(target.starts_with(sb.install.path()), "{target:?}");
    assert_eq!(
        std::fs::read(&target).unwrap(),
        payload,
        "installed bytes differ"
    );
    // No temp files left behind by the atomic pointer write.
    assert!(
        !sb.installed_files()
            .iter()
            .any(|f| f.starts_with("current.tmp")),
        "{:?}",
        sb.installed_files()
    );
    // The installed binary really runs and is the version we asked for.
    let out = std::process::Command::new(&target)
        .arg("--version")
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&out.stdout).starts_with(&format!("concordia {}", real_version()))
    );
    target
}

#[test]
fn a_good_release_installs_flips_the_pointer_and_runs() {
    let sb = Sandbox::new();
    let payload = real_concordia();
    let tag = format!("v{}", real_version());
    let res = install_with(&Mock::honest(payload.clone()), &tag).unwrap();
    assert_eq!(res.summary, format!("updated 0.0.0 -> {}", real_version()));
    let target = assert_installed_and_active(&sb, &payload);
    assert_eq!(res.install_path, target);
    // The scratch download directory was cleaned up.
    assert!(!res.built_from.exists(), "scratch dir should be removed");
}

#[test]
fn updating_twice_keeps_a_working_install() {
    let sb = Sandbox::new();
    let payload = real_concordia();
    let tag = format!("v{}", real_version());
    install_with(&Mock::honest(payload.clone()), &tag).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let active = PathBuf::from(
            std::fs::read_to_string(sb.install.path().join("current"))
                .unwrap()
                .trim(),
        );
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let held = std::thread::spawn(move || {
            let _file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(3)
                .open(active)
                .unwrap();
            ready_tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(100));
        });
        ready_rx.recv().unwrap();
        install_with(&Mock::honest(payload.clone()), &tag).unwrap();
        held.join().unwrap();
    }
    #[cfg(not(windows))]
    install_with(&Mock::honest(payload.clone()), &tag).unwrap();
    assert_installed_and_active(&sb, &payload);
}

// ---- real HTTP + real PowerShell ----------------------------------------------------------------

struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn start(routes: HashMap<String, (u16, Vec<u8>)>) -> Server {
        Server::start_with("application/json", |_| routes)
    }

    /// `make_routes` receives the bound port so a feed can embed absolute URLs to itself.
    fn start_with(
        feed_content_type: &'static str,
        make_routes: impl FnOnce(u16) -> HashMap<String, (u16, Vec<u8>)>,
    ) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let routes = make_routes(port);
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            while !stop2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut conn, _)) => {
                        conn.set_nonblocking(false).ok();
                        let mut buf = [0u8; 4096];
                        let n = conn.read(&mut buf).unwrap_or(0);
                        let req = String::from_utf8_lossy(&buf[..n]).to_string();
                        let path = req
                            .lines()
                            .next()
                            .and_then(|l| l.split_whitespace().nth(1))
                            .unwrap_or("/")
                            .split('?')
                            .next()
                            .unwrap()
                            .to_string();
                        let (status, body) = routes
                            .get(&path)
                            .cloned()
                            .unwrap_or((404, b"not found".to_vec()));
                        let ctype = if path.ends_with("/releases") {
                            feed_content_type
                        } else {
                            "application/octet-stream"
                        };
                        let head = format!(
                            "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nContent-Type: {ctype}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = conn.write_all(head.as_bytes());
                        let _ = conn.write_all(&body);
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
                }
            }
        });
        Server {
            port,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn local_cfg(port: u16) -> Config {
    config_from(
        Some("o/n"),
        None,
        Some(&format!("http://127.0.0.1:{port}")),
        None,
        "0.0.0",
    )
    .unwrap()
}

fn feed_json(port: u16, tags: &[&str]) -> Vec<u8> {
    let items: Vec<_> = tags
        .iter()
        .map(|t| {
            serde_json::json!({
                "tag_name": t, "draft": false, "prerelease": true,
                "assets": [
                    {"name": ASSET_NAME, "browser_download_url": format!("http://127.0.0.1:{port}/dl/{t}/{ASSET_NAME}")},
                    {"name": SUMS_NAME,  "browser_download_url": format!("http://127.0.0.1:{port}/dl/{t}/{SUMS_NAME}")},
                ]
            })
        })
        .collect();
    serde_json::to_vec(&items).unwrap()
}

#[test]
fn full_update_over_real_http_and_powershell() {
    full_update("application/json");
}

/// Mirrors and proxies sometimes serve JSON unlabeled; PowerShell then hands back raw bytes.
#[test]
fn the_feed_still_parses_when_served_as_octet_stream() {
    full_update("application/octet-stream");
}

fn full_update(feed_content_type: &'static str) {
    let sb = Sandbox::new();
    let payload = real_concordia();
    let tag = format!("v{}", real_version());

    let server = Server::start_with(feed_content_type, |port| {
        let mut r = HashMap::new();
        r.insert(
            "/repos/o/n/releases".to_string(),
            (200, feed_json(port, &[&tag])),
        );
        r.insert(format!("/dl/{tag}/{ASSET_NAME}"), (200, payload.clone()));
        r.insert(format!("/dl/{tag}/{SUMS_NAME}"), (200, sums_for(&payload)));
        r
    });
    let cfg = local_cfg(server.port);

    match check(&cfg, "0.0.0", &PowerShellFetcher).unwrap() {
        Check::Available(rel) => {
            assert_eq!(rel.tag, tag);
            let res = install_release(&cfg, &rel, "0.0.0", &PowerShellFetcher).unwrap();
            assert!(
                res.summary.starts_with("updated 0.0.0 ->"),
                "{}",
                res.summary
            );
        }
        other => panic!("expected an available release, got {other:?}"),
    }
    assert_installed_and_active(&sb, &payload);

    // Already current: nothing to do.
    let current = real_version();
    assert!(matches!(
        check(&cfg, &current, &PowerShellFetcher).unwrap(),
        Check::UpToDate { .. }
    ));
}

#[test]
fn a_dead_or_wrong_feed_fails_cleanly_without_installing() {
    let sb = Sandbox::new();
    // Nothing listens on this port.
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let err = check(&local_cfg(port), "0.0.0", &PowerShellFetcher).unwrap_err();
    assert!(format!("{err:#}").contains("GET"), "{err:#}");

    // A server that answers 404 (wrong repo).
    let server = Server::start(HashMap::new());
    let err = check(&local_cfg(server.port), "0.0.0", &PowerShellFetcher).unwrap_err();
    assert!(!format!("{err:#}").is_empty());
    sb.assert_nothing_installed();
}

// ---- /update decision logic -----------------------------------------------------------------

use concordia::update::{check_for_update, update_auto, UpdateOutcome};

/// A directory that is not inside any source checkout.
fn outside_checkout() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

struct FeedEnv;

impl FeedEnv {
    fn point_at(port: u16) -> FeedEnv {
        std::env::set_var("CONCORDIA_UPDATE_REPO", "o/n");
        std::env::set_var("CONCORDIA_UPDATE_API", format!("http://127.0.0.1:{port}"));
        std::env::set_var("CONCORDIA_UPDATE_PRERELEASE", "1");
        FeedEnv
    }
}

impl Drop for FeedEnv {
    fn drop(&mut self) {
        for k in [
            "CONCORDIA_UPDATE_REPO",
            "CONCORDIA_UPDATE_API",
            "CONCORDIA_UPDATE_PRERELEASE",
        ] {
            std::env::remove_var(k);
        }
    }
}

fn next_patch_tag() -> String {
    let v = concordia::release::Version::parse(&real_version()).unwrap();
    format!("v{}.{}.{}", v.major, v.minor, v.patch + 1)
}

fn feed_server(tags: &'static [&str]) -> Server {
    Server::start_with("application/json", move |port| {
        let mut r = HashMap::new();
        r.insert(
            "/repos/o/n/releases".to_string(),
            (200, feed_json(port, tags)),
        );
        r
    })
}

#[test]
fn update_without_a_checkout_or_a_feed_says_exactly_what_to_configure() {
    let _sb = Sandbox::new();
    std::env::remove_var("CONCORDIA_UPDATE_REPO");
    let dir = outside_checkout();
    let err = update_auto(dir.path()).unwrap_err();
    assert!(
        format!("{err:#}").contains("CONCORDIA_UPDATE_REPO=owner/name"),
        "{err:#}"
    );
    // The /update screen shows the same instruction and offers nothing to install.
    let preview = check_for_update(dir.path()).unwrap();
    assert!(!preview.install_available);
    assert!(
        preview.plain_lines()[0].contains("CONCORDIA_UPDATE_REPO=owner/name"),
        "{:?}",
        preview.plain_lines()
    );
}

#[test]
fn inside_a_source_checkout_check_describes_the_rebuild_and_never_touches_the_network() {
    let _sb = Sandbox::new();
    // No feed configured and no server: if this tried the network it would fail.
    std::env::remove_var("CONCORDIA_UPDATE_REPO");
    let msg = check_for_update(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    assert!(
        msg.lines
            .iter()
            .any(|line| line.plain().contains("source checkout")),
        "{msg:?}"
    );
}

#[test]
fn check_reports_an_available_update_without_installing_it() {
    let sb = Sandbox::new();
    let tag: &'static str = Box::leak(next_patch_tag().into_boxed_str());
    let tags: &'static [&'static str] = Box::leak(Box::new([tag]));
    let server = feed_server(tags);
    let _feed = FeedEnv::point_at(server.port);
    let msg = check_for_update(outside_checkout().path()).unwrap();
    assert!(
        msg.lines
            .iter()
            .any(|line| line.plain().contains(&tag[1..])),
        "{msg:?}"
    );
    sb.assert_nothing_installed();
}

#[test]
fn update_when_already_newest_changes_nothing() {
    let sb = Sandbox::new();
    let tag: &'static str = Box::leak(format!("v{}", real_version()).into_boxed_str());
    let tags: &'static [&'static str] = Box::leak(Box::new([tag]));
    let server = feed_server(tags);
    let _feed = FeedEnv::point_at(server.port);

    match update_auto(outside_checkout().path()).unwrap() {
        UpdateOutcome::UpToDate(msg) => assert!(msg.contains("already up to date"), "{msg}"),
        UpdateOutcome::Installed(r) => panic!("installed {r:?} when already current"),
    }
    assert!(
        !check_for_update(outside_checkout().path())
            .unwrap()
            .install_available
    );
    sb.assert_nothing_installed();
}

#[test]
fn an_update_that_fails_verification_is_an_error_and_installs_nothing() {
    // The feed advertises a newer tag, but the real binary it would serve reports an older
    // version, so verification must refuse it.
    let sb = Sandbox::new();
    let tag: &'static str = Box::leak(next_patch_tag().into_boxed_str());
    let payload = real_concordia();
    let server = Server::start_with("application/json", {
        let payload = payload.clone();
        move |port| {
            let mut r = HashMap::new();
            r.insert(
                "/repos/o/n/releases".to_string(),
                (200, feed_json(port, &[tag])),
            );
            r.insert(format!("/dl/{tag}/{ASSET_NAME}"), (200, payload.clone()));
            r.insert(format!("/dl/{tag}/{SUMS_NAME}"), (200, sums_for(&payload)));
            r
        }
    });
    let _feed = FeedEnv::point_at(server.port);
    let err = update_auto(outside_checkout().path()).unwrap_err();
    assert!(
        format!("{err:#}").contains("nothing was installed"),
        "{err:#}"
    );
    sb.assert_nothing_installed();
}

#[test]
fn the_powershell_hasher_matches_a_known_sha256_and_handles_odd_paths() {
    let dir = tempfile::tempdir().unwrap();
    // SHA-256("abc"), the standard test vector.
    let p = dir.path().join("with space [and] 'quote'.bin");
    std::fs::write(&p, b"abc").unwrap();
    assert_eq!(
        PowerShellFetcher.sha256(&p).unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    // The empty file, and agreement with the independent Rust implementation on real bytes.
    let e = dir.path().join("empty.bin");
    std::fs::write(&e, b"").unwrap();
    assert_eq!(
        PowerShellFetcher.sha256(&e).unwrap(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    let big = real_concordia();
    let f = dir.path().join("orc.bin");
    std::fs::write(&f, &big).unwrap();
    assert_eq!(PowerShellFetcher.sha256(&f).unwrap(), sha_hex(&big));
    // A missing file is an error, never an empty hash.
    assert!(PowerShellFetcher.sha256(&dir.path().join("nope")).is_err());
}
