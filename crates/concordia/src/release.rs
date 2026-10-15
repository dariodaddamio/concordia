//! Update from GitHub Releases, for installs that have no source checkout.
//!
//! Flow: list releases -> pick the newest complete one -> download `SHA256SUMS` and the exe ->
//! verify size and checksum -> require that it runs and reports the expected version -> install
//! through the same slot/pointer logic as a source rebuild.
//!
//! Network access is PowerShell's `Invoke-WebRequest` (TLS 1.2 forced, constant script, values
//! passed through the environment), so concordia itself carries no HTTP stack. It only happens when the
//! user runs `/update`; concordia never phones home on its own.
//!
//! Integrity note: the checksum file comes from the same release as the binary, so it catches
//! corruption and truncation, not a compromised release. TLS authenticates GitHub; signed
//! releases are the next step.

use crate::update::{install_binary, verify_binary, UpdateResult};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

pub const ASSET_NAME: &str = "concordia-x86_64-pc-windows-msvc.exe";
pub const SUMS_NAME: &str = "SHA256SUMS";
const DEFAULT_API: &str = "https://api.github.com";
/// Anything outside these bounds is not our binary.
const MIN_EXE_BYTES: u64 = 100 * 1024;
const MAX_EXE_BYTES: u64 = 64 * 1024 * 1024;

// ---- versions --------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl Version {
    /// `v1.2.3`, `1.2.3`, `1.2.3-rc.1`, `1.2.3+build`.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim().trim_start_matches('v');
        let s = s.split('+').next()?;
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
            Some(_) => return None,
            None => (s, None),
        };
        let mut it = core.split('.');
        let major = it.next()?.parse().ok()?;
        let minor = it.next()?.parse().ok()?;
        let patch = it.next()?.parse().ok()?;
        if it.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(p) = &self.pre {
            write!(f, "-{p}")?;
        }
        Ok(())
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater, // 1.0.0 > 1.0.0-rc.1
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// ---- configuration ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// `owner/name`
    pub repo: String,
    pub api_base: String,
    /// Plain http is accepted only for a loopback API (tests, a local mirror).
    pub allow_http_loopback: bool,
    pub include_prerelease: bool,
}

fn valid_repo(r: &str) -> bool {
    let ok = |p: &str| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    match r.split_once('/') {
        Some((o, n)) => ok(o) && ok(n) && !n.contains('/'),
        None => false,
    }
}

/// The release feed, or `None` when updates from releases are not configured. The repo comes from
/// `CONCORDIA_UPDATE_REPO` or is baked in at build time with `CONCORDIA_RELEASE_REPO`.
pub fn config_from_env(current: &str) -> Option<Config> {
    config_from(
        std::env::var("CONCORDIA_UPDATE_REPO").ok().as_deref(),
        option_env!("CONCORDIA_RELEASE_REPO"),
        std::env::var("CONCORDIA_UPDATE_API").ok().as_deref(),
        std::env::var("CONCORDIA_UPDATE_PRERELEASE").ok().as_deref(),
        current,
    )
}

pub fn config_from(
    env_repo: Option<&str>,
    built_in_repo: Option<&str>,
    api: Option<&str>,
    prerelease_flag: Option<&str>,
    current: &str,
) -> Option<Config> {
    let repo = [env_repo, built_in_repo]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|r| !r.is_empty())?
        .to_string();
    if !valid_repo(&repo) {
        return None;
    }
    let api_base = api
        .map(|a| a.trim().trim_end_matches('/').to_string())
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| DEFAULT_API.to_string());
    let allow_http_loopback =
        api_base.starts_with("http://127.0.0.1") || api_base.starts_with("http://localhost");
    // Pre-1.0 releases are all marked pre-release on GitHub, so a 0.x install must see them.
    let cur = Version::parse(current);
    let include_prerelease = prerelease_flag == Some("1")
        || cur
            .map(|v| v.major == 0 || v.pre.is_some())
            .unwrap_or(false);
    Some(Config {
        repo,
        api_base,
        allow_http_loopback,
        include_prerelease,
    })
}

impl Config {
    pub fn releases_url(&self) -> String {
        format!("{}/repos/{}/releases?per_page=20", self.api_base, self.repo)
    }

    /// https always; http only for a loopback API.
    pub fn check_url(&self, url: &str) -> Result<()> {
        if url.starts_with("https://") {
            return Ok(());
        }
        if self.allow_http_loopback
            && (url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost"))
        {
            return Ok(());
        }
        bail!("refusing non-https update URL: {url}")
    }
}

// ---- release selection -----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub version: Version,
    pub asset_url: String,
    pub sums_url: String,
}

/// The newest non-draft release that has both the exe and the checksum file attached. A release
/// whose assets are still uploading is skipped rather than half-installed.
pub fn find_release(json: &str, include_prerelease: bool) -> Result<Option<Release>> {
    let v: Value = serde_json::from_str(json).context("releases response is not JSON")?;
    let list = v
        .as_array()
        .context("releases response is not a list (wrong repo, or rate limited?)")?;
    let mut best: Option<Release> = None;
    for r in list {
        if r.get("draft").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        if !include_prerelease
            && r.get("prerelease")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            continue;
        }
        let Some(tag) = r.get("tag_name").and_then(Value::as_str) else {
            continue;
        };
        let Some(version) = Version::parse(tag) else {
            continue;
        };
        let url_of = |name: &str| -> Option<String> {
            r.get("assets")?
                .as_array()?
                .iter()
                .find(|a| a.get("name").and_then(Value::as_str) == Some(name))
                .and_then(|a| a.get("browser_download_url")?.as_str())
                .map(str::to_string)
        };
        let (Some(asset_url), Some(sums_url)) = (url_of(ASSET_NAME), url_of(SUMS_NAME)) else {
            continue;
        };
        if best.as_ref().is_none_or(|b| version > b.version) {
            best = Some(Release {
                tag: tag.to_string(),
                version,
                asset_url,
                sums_url,
            });
        }
    }
    Ok(best)
}

/// The hash for `name` in a `sha256sum`-style file (`<hex>  <name>` or `<hex> *<name>`).
pub fn parse_sums(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (hash, rest) = l.trim().split_once(char::is_whitespace)?;
        let file = rest.trim().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

// ---- fetching --------------------------------------------------------------------------------

pub trait Fetcher {
    fn get_text(&self, url: &str) -> Result<String>;
    fn download(&self, url: &str, dest: &Path) -> Result<()>;
    fn sha256(&self, path: &Path) -> Result<String>;
}

/// Real network and hashing through Windows PowerShell. Scripts are constants; URLs and paths
/// arrive in the environment, so nothing is ever interpolated into a command.
pub struct PowerShellFetcher;

const PS_PREAMBLE: &str = "$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; \
    [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12; \
    [Console]::OutputEncoding=[Text.Encoding]::UTF8; ";

fn run_ps(script: &str, env: &[(&str, &str)]) -> Result<String> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &format!("{PS_PREAMBLE}{script}"),
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().context("run powershell.exe")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let first = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("failed");
        bail!("{}", first.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

impl Fetcher for PowerShellFetcher {
    fn get_text(&self, url: &str) -> Result<String> {
        run_ps(
            // .Content is a byte[] when the server does not label the body as text.
            "$c=(Invoke-WebRequest -UseBasicParsing -TimeoutSec 30 \
             -Headers @{'User-Agent'='concordia-updater';'Accept'='application/vnd.github+json'} \
             -Uri $env:CONCORDIA_URL).Content; \
             if ($c -is [byte[]]) { [Text.Encoding]::UTF8.GetString($c) } else { $c }",
            &[("CONCORDIA_URL", url)],
        )
        .with_context(|| format!("GET {url}"))
    }

    fn download(&self, url: &str, dest: &Path) -> Result<()> {
        run_ps(
            "Invoke-WebRequest -UseBasicParsing -TimeoutSec 180 \
             -Headers @{'User-Agent'='concordia-updater'} -Uri $env:CONCORDIA_URL -OutFile $env:CONCORDIA_OUT",
            &[("CONCORDIA_URL", url), ("CONCORDIA_OUT", &dest.to_string_lossy())],
        )
        .with_context(|| format!("download {url}"))
        .map(drop)
    }

    fn sha256(&self, path: &Path) -> Result<String> {
        // .NET directly: `Get-FileHash` lives in a module that is not always loadable (it was
        // missing for the PowerShell on a clean CI runner), and a checksum must never be skipped.
        let out = run_ps(
            "$s=[IO.File]::OpenRead($env:CONCORDIA_FILE); \
             try { $h=[Security.Cryptography.SHA256]::Create(); \
                   ([BitConverter]::ToString($h.ComputeHash($s)) -replace '-','').ToLower() } \
             finally { $s.Dispose() }",
            &[("CONCORDIA_FILE", &path.to_string_lossy())],
        )?;
        Ok(out.trim().to_string())
    }
}

// ---- the update ------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    UpToDate { current: String, latest: String },
    Available(Release),
}

pub fn check(cfg: &Config, current: &str, fetch: &dyn Fetcher) -> Result<Check> {
    let cur = Version::parse(current).context("this build has no valid version")?;
    let url = cfg.releases_url();
    cfg.check_url(&url)?;
    let body = fetch.get_text(&url)?;
    match find_release(&body, cfg.include_prerelease)? {
        None => bail!("no published release found in {}", cfg.repo),
        Some(rel) if rel.version > cur => Ok(Check::Available(rel)),
        Some(rel) => Ok(Check::UpToDate {
            current: cur.to_string(),
            latest: rel.version.to_string(),
        }),
    }
}

/// Removes the scratch directory on drop, including on error paths.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Download, verify and install `rel`. Nothing is installed unless every check passes.
pub fn install_release(
    cfg: &Config,
    rel: &Release,
    current: &str,
    fetch: &dyn Fetcher,
) -> Result<UpdateResult> {
    cfg.check_url(&rel.asset_url)?;
    cfg.check_url(&rel.sums_url)?;

    let scratch = Scratch(std::env::temp_dir().join(format!(
        "concordia-update-{}-{}",
        std::process::id(),
        rel.version
    )));
    let _ = fs::remove_dir_all(&scratch.0);
    fs::create_dir_all(&scratch.0).context("create scratch dir")?;

    let sums_path = scratch.0.join(SUMS_NAME);
    fetch.download(&rel.sums_url, &sums_path)?;
    let sums = fs::read_to_string(&sums_path).context("read checksums")?;
    let expected = parse_sums(&sums, ASSET_NAME)
        .with_context(|| format!("{SUMS_NAME} has no valid entry for {ASSET_NAME}"))?;

    let exe = scratch.0.join("concordia.exe");
    fetch.download(&rel.asset_url, &exe)?;
    let len = fs::metadata(&exe).context("downloaded file missing")?.len();
    if !(MIN_EXE_BYTES..=MAX_EXE_BYTES).contains(&len) {
        bail!("downloaded file is {len} bytes, which is not a plausible concordia binary");
    }
    let actual = fetch.sha256(&exe)?;
    if !actual.eq_ignore_ascii_case(&expected) {
        bail!("checksum mismatch (expected {expected}, got {actual}); nothing was installed");
    }

    // It must run and be the version the release claims, or every launcher would break.
    let reported = verify_binary(&exe)?;
    let want = format!("concordia {} ", rel.version);
    if !reported.starts_with(&want) {
        bail!(
            "release {} contains a binary that reports {reported:?}; nothing was installed",
            rel.tag
        );
    }

    let install_path = install_binary(&exe)?;
    Ok(UpdateResult {
        install_path,
        built_from: exe,
        log_tail: String::new(),
        summary: format!("updated {current} -> {}", rel.version),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn versions_parse_and_order_like_semver() {
        assert_eq!(v("v1.2.3").to_string(), "1.2.3");
        assert_eq!(v("1.2.3-rc.1+build5").to_string(), "1.2.3-rc.1");
        for bad in ["", "1.2", "1.2.3.4", "a.b.c", "1.2.3-", "v"] {
            assert!(Version::parse(bad).is_none(), "{bad:?}");
        }
        assert!(v("0.1.0") < v("0.1.1"));
        assert!(v("0.9.9") < v("0.10.0"), "numeric, not lexicographic");
        assert!(
            v("1.0.0-rc.1") < v("1.0.0"),
            "a release beats its pre-release"
        );
        assert!(v("1.0.0-rc.1") < v("1.0.0-rc.2"));
        assert_eq!(v("1.0.0").cmp(&v("1.0.0")), Ordering::Equal);
    }

    #[test]
    fn repo_names_are_validated() {
        for ok in ["owner/name", "a-b/c.d_e"] {
            assert!(valid_repo(ok), "{ok}");
        }
        for bad in ["", "owner", "o/n/x", "o /n", "o/n;rm", "/n", "o/"] {
            assert!(!valid_repo(bad), "{bad:?}");
        }
    }

    #[test]
    fn config_needs_a_valid_repo_and_defaults_sensibly() {
        assert!(config_from(None, None, None, None, "0.1.0").is_none());
        assert!(config_from(Some("not a repo"), None, None, None, "0.1.0").is_none());
        let c = config_from(
            Some(" me/concordia "),
            Some("built/in"),
            None,
            None,
            "0.1.0",
        )
        .unwrap();
        assert_eq!(c.repo, "me/concordia", "env wins over the built-in repo");
        assert_eq!(c.api_base, "https://api.github.com");
        assert!(!c.allow_http_loopback);
        let c = config_from(None, Some("built/in"), None, None, "1.0.0").unwrap();
        assert_eq!(c.repo, "built/in");
        assert!(
            !c.include_prerelease,
            "a stable install ignores pre-releases"
        );
        assert!(
            config_from(None, Some("b/i"), None, Some("1"), "1.0.0")
                .unwrap()
                .include_prerelease
        );
        assert!(
            config_from(None, Some("b/i"), None, None, "0.3.0")
                .unwrap()
                .include_prerelease
        );
        assert!(
            config_from(None, Some("b/i"), None, None, "1.0.0-rc.1")
                .unwrap()
                .include_prerelease
        );
    }

    #[test]
    fn plain_http_is_only_ever_allowed_for_a_loopback_api() {
        let local = config_from(
            Some("o/n"),
            None,
            Some("http://127.0.0.1:9/"),
            None,
            "0.1.0",
        )
        .unwrap();
        assert!(local.allow_http_loopback);
        assert_eq!(local.api_base, "http://127.0.0.1:9");
        assert!(local.check_url("http://127.0.0.1:9/dl/x").is_ok());
        assert!(local.check_url("http://evil.example/x").is_err());
        assert!(local
            .check_url("https://objects.githubusercontent.com/x")
            .is_ok());

        let remote = config_from(Some("o/n"), None, None, None, "0.1.0").unwrap();
        assert!(remote.check_url("http://127.0.0.1/x").is_err());
        assert!(remote.check_url("ftp://x").is_err());
        // An attacker-controlled API base cannot opt into http for a remote host.
        let sneaky = config_from(
            Some("o/n"),
            None,
            Some("http://evil.example"),
            None,
            "0.1.0",
        )
        .unwrap();
        assert!(!sneaky.allow_http_loopback);
        assert!(sneaky.check_url("http://evil.example/x").is_err());
    }

    fn release_json(entries: &[(&str, bool, bool, &[&str])]) -> String {
        let items: Vec<Value> = entries
            .iter()
            .map(|(tag, draft, pre, assets)| {
                serde_json::json!({
                    "tag_name": tag,
                    "draft": draft,
                    "prerelease": pre,
                    "assets": assets.iter().map(|n| serde_json::json!({
                        "name": n,
                        "browser_download_url": format!("https://dl.example/{tag}/{n}")
                    })).collect::<Vec<_>>()
                })
            })
            .collect();
        serde_json::to_string(&items).unwrap()
    }

    const BOTH: &[&str] = &[ASSET_NAME, SUMS_NAME];

    #[test]
    fn the_newest_complete_published_release_wins() {
        let json = release_json(&[
            ("v0.2.0", true, false, BOTH),           // draft
            ("v0.1.5", false, false, &[ASSET_NAME]), // still uploading: no sums
            ("v0.1.4", false, false, BOTH),
            ("v0.1.3", false, false, BOTH),
            ("v0.1.9-rc.1", false, true, BOTH), // pre-release
            ("not-a-version", false, false, BOTH),
        ]);
        let r = find_release(&json, false).unwrap().unwrap();
        assert_eq!(r.tag, "v0.1.4");
        assert_eq!(
            r.asset_url,
            format!("https://dl.example/v0.1.4/{ASSET_NAME}")
        );
        let r = find_release(&json, true).unwrap().unwrap();
        assert_eq!(r.tag, "v0.1.9-rc.1", "pre-releases count when allowed");
    }

    #[test]
    fn selection_does_not_depend_on_list_order() {
        let a = release_json(&[
            ("v0.1.0", false, false, BOTH),
            ("v0.3.0", false, false, BOTH),
            ("v0.2.0", false, false, BOTH),
        ]);
        assert_eq!(find_release(&a, false).unwrap().unwrap().tag, "v0.3.0");
    }

    #[test]
    fn odd_responses_are_errors_or_none_never_panics() {
        assert!(find_release("[]", true).unwrap().is_none());
        assert!(find_release("{\"message\":\"Not Found\"}", true).is_err());
        assert!(find_release("<html>rate limited</html>", true).is_err());
        assert!(find_release("[{\"tag_name\":5}]", true).unwrap().is_none());
    }

    #[test]
    fn checksum_files_parse_in_both_sha256sum_styles() {
        let h = "a".repeat(64);
        let text = format!("{h}  {ASSET_NAME}\n{} *other.exe\n", "b".repeat(64));
        assert_eq!(parse_sums(&text, ASSET_NAME).as_deref(), Some(h.as_str()));
        assert_eq!(
            parse_sums(&text, "other.exe").as_deref(),
            Some("b".repeat(64).as_str())
        );
        assert!(parse_sums(&text, "missing.exe").is_none());
        // Wrong length / non-hex hashes are not accepted.
        assert!(parse_sums(&format!("abc  {ASSET_NAME}"), ASSET_NAME).is_none());
        assert!(parse_sums(&format!("{}  {ASSET_NAME}", "z".repeat(64)), ASSET_NAME).is_none());
        // Upper-case is normalized.
        assert_eq!(
            parse_sums(&format!("{}  {ASSET_NAME}", "A".repeat(64)), ASSET_NAME).unwrap(),
            "a".repeat(64)
        );
    }

    struct FeedOnly(String);
    impl Fetcher for FeedOnly {
        fn get_text(&self, _: &str) -> Result<String> {
            Ok(self.0.clone())
        }
        fn download(&self, _: &str, _: &Path) -> Result<()> {
            unreachable!("check() must not download")
        }
        fn sha256(&self, _: &Path) -> Result<String> {
            unreachable!()
        }
    }

    fn cfg() -> Config {
        config_from(Some("o/n"), None, None, None, "0.1.0").unwrap()
    }

    #[test]
    fn check_reports_available_or_up_to_date_and_downloads_nothing() {
        let json = release_json(&[("v0.1.4", false, false, BOTH)]);
        match check(&cfg(), "0.1.0", &FeedOnly(json.clone())).unwrap() {
            Check::Available(r) => assert_eq!(r.version.to_string(), "0.1.4"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            check(&cfg(), "0.1.4", &FeedOnly(json.clone())).unwrap(),
            Check::UpToDate {
                current: "0.1.4".into(),
                latest: "0.1.4".into()
            }
        );
        // Never "update" backwards to an older release.
        assert!(matches!(
            check(&cfg(), "0.2.0", &FeedOnly(json)).unwrap(),
            Check::UpToDate { .. }
        ));
        assert!(check(&cfg(), "0.1.0", &FeedOnly("[]".into())).is_err());
        assert!(check(&cfg(), "garbage", &FeedOnly("[]".into())).is_err());
    }
}
