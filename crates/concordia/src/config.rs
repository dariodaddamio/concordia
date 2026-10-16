//! `<state>/config.toml`: user-written defaults and switches. Read-only to Concordia.
//!
//! Loading never fails and never writes: a missing file is the defaults, a malformed file is the
//! defaults plus a warning (with line and column), unknown keys and invalid values are warnings
//! (invalid values fall back to the default or are clamped). Warnings are surfaced by
//! `concordia doctor`. Concordia-owned state stays in `store.json`; a value the user set there
//! wins over the config default (see `docs/config.md`).

use crate::store::{default_store_root, TrustMode};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Hard ceiling on delegated workers (the store and `/workers add` enforce it too).
pub const MAX_WORKERS_CAP: usize = 3;
pub const MAP_CHARS_RANGE: std::ops::RangeInclusive<i64> = 200..=8000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub ui: Ui,
    pub threads: Threads,
    pub mode: Mode,
    pub delegation: Delegation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ui {
    pub token_line: bool,
    pub tips: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Threads {
    pub enabled: bool,
    pub map_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mode {
    pub default: TrustMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegation {
    pub protocol: bool,
    pub max_workers: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ui: Ui {
                token_line: true,
                tips: true,
            },
            threads: Threads {
                enabled: false,
                map_chars: 1600,
            },
            mode: Mode {
                default: TrustMode::Ask,
            },
            delegation: Delegation {
                protocol: false,
                max_workers: MAX_WORKERS_CAP,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConfig {
    pub config: Config,
    pub warnings: Vec<String>,
    /// File that was consulted (it may not exist).
    pub path: PathBuf,
    pub exists: bool,
}

/// Location of the config file: `CONCORDIA_CONFIG` if set, else `<state>/config.toml`.
pub fn config_path(state_root: &Path) -> PathBuf {
    match std::env::var("CONCORDIA_CONFIG") {
        Ok(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => state_root.join("config.toml"),
    }
}

fn line_col(text: &str, byte: usize) -> (usize, usize) {
    let mut end = byte.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let upto = &text[..end];
    let line = upto.matches('\n').count() + 1;
    let col = upto.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

impl Config {
    /// Read `<state>/config.toml`. Never fails, never writes.
    pub fn load(state_root: &Path) -> LoadedConfig {
        let path = config_path(state_root);
        let mut out = LoadedConfig {
            config: Config::default(),
            warnings: Vec::new(),
            path: path.clone(),
            exists: false,
        };
        let raw = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return out,
            Err(e) => {
                out.exists = true;
                out.warnings.push(format!(
                    "cannot read {}: {e}; using defaults",
                    path.display()
                ));
                return out;
            }
        };
        out.exists = true;
        let Ok(text) = String::from_utf8(raw) else {
            out.warnings
                .push("config.toml is not valid UTF-8; using defaults".into());
            return out;
        };
        let (config, warnings) = Config::parse(&text);
        out.config = config;
        out.warnings = warnings;
        out
    }

    /// Parse config text into a config plus warnings (pure; used by `load` and the tests).
    pub fn parse(text: &str) -> (Config, Vec<String>) {
        let mut warnings = Vec::new();
        let table: toml::Table = match text.parse() {
            Ok(t) => t,
            Err(e) => {
                let at = e
                    .span()
                    .map(|s| {
                        let (l, c) = line_col(text, s.start);
                        format!("line {l}, column {c}: ")
                    })
                    .unwrap_or_default();
                warnings.push(format!(
                    "config.toml is malformed ({at}{}); using defaults",
                    e.message()
                ));
                return (Config::default(), warnings);
            }
        };
        let mut cfg = Config::default();
        let mut unknown = Vec::new();
        for (section, value) in &table {
            let Some(sub) = value.as_table() else {
                if matches!(section.as_str(), "ui" | "threads" | "mode" | "delegation") {
                    warnings.push(format!("[{section}] must be a table; ignored"));
                } else {
                    unknown.push(section.clone());
                }
                continue;
            };
            for (key, v) in sub {
                let full = format!("{section}.{key}");
                match full.as_str() {
                    "ui.token_line" => set_bool(v, &full, &mut cfg.ui.token_line, &mut warnings),
                    "ui.tips" => set_bool(v, &full, &mut cfg.ui.tips, &mut warnings),
                    "threads.enabled" => {
                        set_bool(v, &full, &mut cfg.threads.enabled, &mut warnings)
                    }
                    "threads.map_chars" => match v.as_integer() {
                        Some(n) if MAP_CHARS_RANGE.contains(&n) => {
                            cfg.threads.map_chars = n as usize
                        }
                        Some(n) => {
                            let c = n.clamp(*MAP_CHARS_RANGE.start(), *MAP_CHARS_RANGE.end());
                            cfg.threads.map_chars = c as usize;
                            warnings.push(format!(
                                "threads.map_chars = {n} is outside 200..=8000; using {c}"
                            ));
                        }
                        None => warnings
                            .push("threads.map_chars must be an integer; using 1600".to_string()),
                    },
                    "mode.default" => match v.as_str().map(str::to_ascii_lowercase).as_deref() {
                        Some("plan") => cfg.mode.default = TrustMode::Plan,
                        Some("ask") => cfg.mode.default = TrustMode::Ask,
                        Some("workspace") => cfg.mode.default = TrustMode::Workspace,
                        Some("full") => cfg.mode.default = TrustMode::Full,
                        _ => warnings.push(format!(
                            "mode.default = {} is not one of plan, ask, workspace, full; using \"ask\"",
                            v.as_str().map_or_else(|| v.type_str().to_string(), |s| format!("{s:?}"))
                        )),
                    },
                    "delegation.protocol" => {
                        set_bool(v, &full, &mut cfg.delegation.protocol, &mut warnings)
                    }
                    "delegation.max_workers" => match v.as_integer() {
                        Some(n) if (1..=MAX_WORKERS_CAP as i64).contains(&n) => {
                            cfg.delegation.max_workers = n as usize
                        }
                        Some(n) => {
                            let c = n.clamp(1, MAX_WORKERS_CAP as i64);
                            cfg.delegation.max_workers = c as usize;
                            warnings.push(format!(
                                "delegation.max_workers = {n} is outside 1..={MAX_WORKERS_CAP}; using {c}"
                            ));
                        }
                        None => warnings.push(format!(
                            "delegation.max_workers must be an integer; using {MAX_WORKERS_CAP}"
                        )),
                    },
                    _ => unknown.push(full),
                }
            }
        }
        if !unknown.is_empty() {
            warnings.push(format!(
                "unknown config keys ignored: {}",
                unknown.join(", ")
            ));
        }
        (cfg, warnings)
    }
}

fn set_bool(v: &toml::Value, key: &str, slot: &mut bool, warnings: &mut Vec<String>) {
    match v.as_bool() {
        Some(b) => *slot = b,
        None => warnings.push(format!("{key} must be true or false; using {slot}")),
    }
}

static CONFIG: OnceLock<LoadedConfig> = OnceLock::new();

/// Load (once) from the default state directory. Call at TUI start and from CLI subcommands.
pub fn loaded() -> &'static LoadedConfig {
    CONFIG.get_or_init(|| Config::load(&default_store_root()))
}

/// The process-wide config, loaded once.
pub fn get() -> &'static Config {
    &loaded().config
}

/// Whether the per-turn token line is shown. Wired by stage 1b.
pub fn ui_token_line() -> bool {
    get().ui.token_line
}

/// Whether idle tips are shown. Wired by stage 1b.
pub fn ui_tips() -> bool {
    get().ui.tips
}

/// Whether conversations are recorded to `<state>/threads`. Wired by stage 1a.
pub fn threads_enabled() -> bool {
    get().threads.enabled
}

/// Size of the `[concordia map]` block in characters. Wired by stage 1a.
pub fn threads_map_chars() -> usize {
    get().threads.map_chars
}

/// Permission mode for a store that has none saved yet.
pub fn default_mode() -> TrustMode {
    get().mode.default
}

/// Config default for the delegation protocol (a value saved in the store wins).
pub fn delegation_protocol() -> bool {
    get().delegation.protocol
}

/// Config cap on delegated workers (never above [`MAX_WORKERS_CAP`]).
pub fn max_workers() -> usize {
    get().delegation.max_workers
}

/// The same config for an explicit state root (store code and tests with temp roots).
pub fn for_root(state_root: &Path) -> Config {
    Config::load(state_root).config
}

#[cfg(test)]
pub(crate) fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("CONCORDIA_CONFIG");
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str) -> (Config, Vec<String>) {
        Config::parse(text)
    }

    #[test]
    fn empty_text_is_all_defaults_without_warnings() {
        let (c, warn) = w("");
        assert_eq!(c, Config::default());
        assert!(warn.is_empty());
        assert!(c.ui.token_line && c.ui.tips && !c.threads.enabled);
        assert_eq!(c.threads.map_chars, 1600);
        assert_eq!(c.mode.default, TrustMode::Ask);
        assert!(!c.delegation.protocol);
        assert_eq!(c.delegation.max_workers, 3);
    }

    #[test]
    fn every_key_is_read() {
        let (c, warn) = w(r#"
[ui]
token_line = false
tips = false
[threads]
enabled = true
map_chars = 800
[mode]
default = "Full"
[delegation]
protocol = true
max_workers = 2
"#);
        assert!(warn.is_empty(), "{warn:?}");
        assert!(!c.ui.token_line && !c.ui.tips && c.threads.enabled);
        assert_eq!(c.threads.map_chars, 800);
        assert_eq!(c.mode.default, TrustMode::Full);
        assert!(c.delegation.protocol);
        assert_eq!(c.delegation.max_workers, 2);
    }

    #[test]
    fn missing_file_is_defaults_and_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let _g = env_guard();
        let l = Config::load(dir.path());
        assert_eq!(l.config, Config::default());
        assert!(l.warnings.is_empty() && !l.exists);
        assert!(!dir.path().join("config.toml").exists());
    }

    #[test]
    fn malformed_file_gives_defaults_a_position_and_is_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let _g = env_guard();
        let text = "[ui]\ntips = false\n[mode\ndefault = \"plan\"\n";
        std::fs::write(dir.path().join("config.toml"), text).unwrap();
        let l = Config::load(dir.path());
        assert_eq!(l.config, Config::default());
        assert_eq!(l.warnings.len(), 1);
        assert!(l.warnings[0].contains("line 3"), "{}", l.warnings[0]);
        assert!(l.warnings[0].contains("column"), "{}", l.warnings[0]);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("config.toml")).unwrap(),
            text
        );
    }

    #[test]
    fn unknown_keys_and_sections_are_listed() {
        let (c, warn) = w("[ui]\ntips = false\ncolour = 1\n[extra]\nx = 1\n");
        assert!(!c.ui.tips);
        assert_eq!(warn.len(), 1);
        assert!(warn[0].contains("ui.colour") && warn[0].contains("extra.x"));
        let (_, warn) = w("stray = 1\n");
        assert!(warn[0].contains("stray"));
    }

    #[test]
    fn invalid_values_are_defaulted_or_clamped_with_a_warning() {
        let (c, warn) = w("[mode]\ndefault = \"banana\"\n");
        assert_eq!(c.mode.default, TrustMode::Ask);
        assert!(warn[0].contains("banana"));
        let (c, warn) = w("[threads]\nmap_chars = 50\n");
        assert_eq!(c.threads.map_chars, 200);
        assert_eq!(warn.len(), 1);
        let (c, _) = w("[threads]\nmap_chars = 99999\n");
        assert_eq!(c.threads.map_chars, 8000);
        let (c, warn) = w("[delegation]\nmax_workers = 0\n");
        assert_eq!(c.delegation.max_workers, 1);
        assert_eq!(warn.len(), 1);
        let (c, warn) = w("[delegation]\nmax_workers = 9\n");
        assert_eq!(c.delegation.max_workers, 3);
        assert_eq!(warn.len(), 1);
        let (c, warn) = w("[ui]\ntips = \"yes\"\n[threads]\nmap_chars = \"big\"\n");
        assert!(c.ui.tips);
        assert_eq!(c.threads.map_chars, 1600);
        assert_eq!(warn.len(), 2);
        let (_, warn) = w("ui = 3\n");
        assert!(warn[0].contains("[ui]"));
    }

    #[test]
    fn env_override_selects_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("elsewhere.toml");
        std::fs::write(&other, "[ui]\ntips = false\n").unwrap();
        let _g = env_guard();
        std::env::set_var("CONCORDIA_CONFIG", &other);
        let l = Config::load(dir.path());
        std::env::remove_var("CONCORDIA_CONFIG");
        assert!(!l.config.ui.tips && l.exists && l.path == other);
    }
}
