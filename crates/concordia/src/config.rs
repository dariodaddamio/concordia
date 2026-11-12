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

pub const MAP_CHARS_RANGE: std::ops::RangeInclusive<i64> = 200..=8000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub ui: Ui,
    pub threads: Threads,
    pub mode: Mode,
    pub delegation: Delegation,
    pub editor: EditorConfig,
    pub continuation: Continuation,
    pub turn: TurnConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnConfig {
    /// Minutes without provider output before Concordia notifies the user. Zero disables it.
    pub idle_timeout_minutes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Continuation {
    pub automatic: bool,
    /// Codex-only proactive context rollover; off unless explicitly enabled.
    pub proactive_rollover: bool,
    /// Same-provider automatic exhaustion handoff is deliberately opt-in.
    pub same_provider_automatic: bool,
    /// Maximum automatic handoffs into one account per local day.
    pub automatic_handoff_cap: usize,
    /// Optional ordered allowlist; empty uses connected accounts in automatic mode.
    pub accounts: Vec<String>,
    /// Ordered models on the current account, validated against its native catalog.
    pub models: Vec<String>,
}

impl Default for Continuation {
    fn default() -> Self {
        Self {
            automatic: false,
            proactive_rollover: false,
            same_provider_automatic: false,
            automatic_handoff_cap: 3,
            accounts: Vec::new(),
            models: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorConfig {
    /// One executable, never a shell command.
    pub program: String,
    pub args: Vec<String>,
    pub launch: crate::file_open::EditorLaunch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ui {
    pub file_action: crate::file_open::FileAction,
    pub token_line: bool,
    /// Add the local-time header to provider turns.
    pub time_header: bool,
    pub tips: bool,
    pub persist_history: bool,
    /// Footer memory and CPU figures (`mem 41 MB - cpu 1%`); off by default.
    pub stats: bool,
    pub theme: Theme,
    pub background: Option<Rgb>,
    pub foreground: Option<Rgb>,
    pub accent: Option<Rgb>,
    pub brand_color: Option<Rgb>,
    pub motion: bool,
    pub library_theme: Option<crate::theme_library::ThemeFile>,
}

pub type Rgb = [u8; 3];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Default,
    Dark,
    Light,
    Terminal,
    Opifex,
}

impl Theme {
    pub(crate) fn colors(self) -> [Rgb; 3] {
        match self {
            Self::Opifex => crate::theme_library::builtin("opifex")
                .map(|t| [t.background, t.foreground, t.accent])
                .unwrap_or([[0, 0, 0], [216, 208, 192], [255, 176, 0]]),
            Self::Light => crate::theme_library::builtin("light")
                .map(|t| [t.background, t.foreground, t.accent])
                .unwrap_or([[247, 247, 244], [36, 40, 47], [28, 86, 180]]),
            Self::Dark => crate::theme_library::builtin("dark")
                .map(|t| [t.background, t.foreground, t.accent])
                .unwrap_or([[11, 13, 17], [232, 234, 240], [107, 180, 245]]),
            Self::Default | Self::Terminal => [[0, 0, 0], [224, 226, 235], [58, 150, 221]],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Threads {
    /// Whether Concordia writes its local thread log.
    pub logging: bool,
    pub map_chars: usize,
    /// Register `concordia mcp` in homes Concordia created (default off: schemas cost tokens).
    pub mcp: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mode {
    pub default: TrustMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegation {
    pub protocol: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ui: Ui {
                token_line: true,
                time_header: true,
                tips: true,
                persist_history: true,
                stats: false,
                file_action: crate::file_open::FileAction::Preview,
                theme: Theme::Opifex,
                background: None,
                foreground: None,
                accent: None,
                brand_color: None,
                motion: true,
                library_theme: None,
            },
            threads: Threads {
                logging: false,
                map_chars: 1600,
                mcp: false,
            },
            mode: Mode {
                default: TrustMode::Ask,
            },
            delegation: Delegation {
                protocol: true,
            },
            editor: EditorConfig::default(),
            continuation: Continuation::default(),
            turn: TurnConfig {
                idle_timeout_minutes: 10,
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
        Self::load_from_path(path)
    }

    fn load_from_path(path: PathBuf) -> LoadedConfig {
        let mut out = LoadedConfig {
            config: Config::default(),
            warnings: Vec::new(),
            path: path.clone(),
            exists: false,
        };
        use std::io::Read;
        let raw = match std::fs::File::open(&path).and_then(|file| {
            let mut raw = Vec::new();
            file.take(1024 * 1024 + 1).read_to_end(&mut raw)?;
            if raw.len() > 1024 * 1024 {
                return Err(std::io::Error::other("config.toml exceeds 1 MiB"));
            }
            Ok(raw)
        }) {
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
                if matches!(
                    section.as_str(),
                    "ui" | "threads"
                        | "mode"
                        | "delegation"
                        | "editor"
                        | "continuation"
                        | "turn"
                        | "crews"
                ) {
                    warnings.push(format!("[{section}] must be a table; ignored"));
                } else {
                    unknown.push(section.clone());
                }
                continue;
            };
            if section == "crews" {
                continue;
            }
            for (key, v) in sub {
                let full = format!("{section}.{key}");
                match full.as_str() {
                    "continuation.mode" => match v.as_str() {
                        Some("automatic") => cfg.continuation.automatic = true,
                        Some("manual") => cfg.continuation.automatic = false,
                        _ => warnings.push("continuation.mode must be manual or automatic; using manual".into()),
                    },
                    "continuation.proactive_rollover" => set_bool(v, &full, &mut cfg.continuation.proactive_rollover, &mut warnings),
                    "continuation.same_provider_automatic" => set_bool(v, &full, &mut cfg.continuation.same_provider_automatic, &mut warnings),
                    "continuation.automatic_handoff_cap" => match v.as_integer() {
                        Some(n) if (1..=32).contains(&n) => cfg.continuation.automatic_handoff_cap = n as usize,
                        _ => warnings.push("continuation.automatic_handoff_cap must be between 1 and 32; using 3".into()),
                    },
                    "continuation.models" => match v.as_array() {
                        Some(models) if models.iter().all(|m| m.as_str().is_some()) => {
                            cfg.continuation.models = models.iter().filter_map(|m| m.as_str().map(str::to_owned)).collect();
                        }
                        _ => warnings.push("continuation.models must be an array of model ids".into()),
                    },
                    "continuation.accounts" => match v.as_array() {
                        Some(accounts) if accounts.iter().all(|a| a.as_str().is_some_and(|s| !s.trim().is_empty())) => {
                            cfg.continuation.accounts = accounts.iter().filter_map(|a| a.as_str().map(str::to_owned)).collect();
                        }
                        _ => warnings.push("continuation.accounts must be an array of account ids".into()),
                    },
                    "turn.idle_timeout_minutes" => match v.as_integer() {
                        Some(n) if n >= 0 => cfg.turn.idle_timeout_minutes = n as u64,
                        _ => warnings.push("turn.idle_timeout_minutes must be a nonnegative integer; using 10".into()),
                    },
                    "editor.program" => match v.as_str() {
                        Some(program) if !program.trim().is_empty() => cfg.editor.program = program.into(),
                        _ => warnings.push("editor.program must be a nonempty executable path".into()),
                    },
                    "editor.args" => match v.as_array() {
                        Some(args) if args.iter().all(|arg| arg.is_str()) => cfg.editor.args = args.iter().filter_map(|arg| arg.as_str().map(str::to_owned)).collect(),
                        _ => warnings.push("editor.args must be an array of argument strings".into()),
                    },
                    "ui.file_action"=>match v.as_str().and_then(crate::file_open::FileAction::parse) {
                        Some(action)=>cfg.ui.file_action=action,
                        None=>warnings.push("ui.file_action must be 'preview' or 'editor'".into()),
                    },
                    "ui.token_line" => set_bool(v, &full, &mut cfg.ui.token_line, &mut warnings),
                    "ui.time_header" => set_bool(v, &full, &mut cfg.ui.time_header, &mut warnings),
                    "ui.tips" => set_bool(v, &full, &mut cfg.ui.tips, &mut warnings),
                    "ui.persist_history" => {
                        set_bool(v, &full, &mut cfg.ui.persist_history, &mut warnings)
                    }
                    "ui.stats" => set_bool(v, &full, &mut cfg.ui.stats, &mut warnings),
                    "ui.theme" => match v.as_str().map(str::to_ascii_lowercase).as_deref() {
                        Some("default") => cfg.ui.theme = Theme::Default,
                        Some("dark") => cfg.ui.theme = Theme::Dark,
                        Some("light") => cfg.ui.theme = Theme::Light,
                        Some("terminal") => cfg.ui.theme = Theme::Terminal,
                        Some("opifex") | Some("opifex studio") => cfg.ui.theme = Theme::Opifex,
                        _ => warnings.push("ui.theme must be default, dark, light, terminal or opifex; using default".into()),
                    },
                    "ui.background" => set_color(v, &full, &mut cfg.ui.background, &mut warnings),
                    "ui.foreground" => set_color(v, &full, &mut cfg.ui.foreground, &mut warnings),
                    "ui.accent" => set_color(v, &full, &mut cfg.ui.accent, &mut warnings),
                    "ui.brand_color" => set_color(v, &full, &mut cfg.ui.brand_color, &mut warnings),
                    "ui.motion" => set_bool(v, &full, &mut cfg.ui.motion, &mut warnings),
                    "threads.logging" => match v.as_str().map(str::to_ascii_lowercase).as_deref() {
                        Some("on") => cfg.threads.logging = true,
                        Some("off") => cfg.threads.logging = false,
                        _ => warnings.push("threads.logging must be 'on' or 'off'; using off".into()),
                    },
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
                    "threads.mcp" => set_bool(v, &full, &mut cfg.threads.mcp, &mut warnings),
                    "mode.default" => match v.as_str().map(str::to_ascii_lowercase).as_deref() {
                        Some("plan") => cfg.mode.default = TrustMode::Plan,
                        Some("ask") => cfg.mode.default = TrustMode::Ask,
                        Some("edit") => cfg.mode.default = TrustMode::Edit,
                        Some("full") => cfg.mode.default = TrustMode::Full,
                        _ => warnings.push(format!(
                            "mode.default = {} is not one of plan, ask, edit, full; using \"ask\"",
                            v.as_str().map_or_else(|| v.type_str().to_string(), |s| format!("{s:?}"))
                        )),
                    },
                    "delegation.protocol" => {
                        set_bool(v, &full, &mut cfg.delegation.protocol, &mut warnings)
                    }
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
        let background = cfg.ui.background.unwrap_or(cfg.ui.theme.colors()[0]);
        let known_background = cfg.ui.theme != Theme::Terminal || cfg.ui.background.is_some();
        if known_background
            && cfg
                .ui
                .foreground
                .is_some_and(|foreground| contrast(foreground, background) < 4.5)
        {
            cfg.ui.foreground = None;
            warnings.push("ui.foreground has insufficient contrast against the background; using a readable foreground".into());
        }
        if known_background
            && cfg
                .ui
                .accent
                .is_some_and(|accent| contrast(accent, background) < 3.0)
        {
            cfg.ui.accent = None;
            warnings.push("ui.accent has insufficient contrast against the background; using a readable accent".into());
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

fn set_color(v: &toml::Value, key: &str, slot: &mut Option<Rgb>, warnings: &mut Vec<String>) {
    let parsed = v
        .as_str()
        .filter(|value| {
            value.len() == 7
                && value.starts_with('#')
                && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .and_then(|value| {
            Some([
                u8::from_str_radix(&value[1..3], 16).ok()?,
                u8::from_str_radix(&value[3..5], 16).ok()?,
                u8::from_str_radix(&value[5..7], 16).ok()?,
            ])
        });
    match parsed {
        Some(color) => *slot = Some(color),
        None => warnings.push(format!(
            "{key} must be a quoted #RRGGBB color; using the theme default"
        )),
    }
}

pub(crate) fn luminance(color: Rgb) -> f64 {
    let channel = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color[0]) + 0.7152 * channel(color[1]) + 0.0722 * channel(color[2])
}

pub(crate) fn contrast(first: Rgb, second: Rgb) -> f64 {
    let first = luminance(first);
    let second = luminance(second);
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
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

/// Config default for the delegation protocol (a value saved in the store wins).
pub fn delegation_protocol() -> bool {
    get().delegation.protocol
}

/// The same config for an explicit state root (store code and tests with temp roots).
pub fn for_root(state_root: &Path) -> Config {
    Config::load(state_root).config
}

#[cfg(test)]
pub(crate) fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_is_manual_unless_explicitly_configured() {
        assert!(!Config::default().continuation.automatic);
        let (cfg, warnings) =
            Config::parse("[continuation]\nmode='automatic'\naccounts=['backup','third']\n");
        assert!(warnings.is_empty());
        assert!(cfg.continuation.automatic);
        assert_eq!(cfg.continuation.accounts, ["backup", "third"]);
        let (cfg, warnings) = Config::parse("[continuation]\nmode='anything'\naccounts=[42]\n");
        assert!(!cfg.continuation.automatic);
        assert!(cfg.continuation.accounts.is_empty());
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn editor_configuration_uses_one_program_and_argument_array() {
        let (config, warnings) =
            Config::parse("[editor]\nprogram='nvim'\nargs=['+{line}','--','{file}']\n");
        assert!(warnings.is_empty());
        assert_eq!(config.editor.program, "nvim");
        assert_eq!(config.editor.args, ["+{line}", "--", "{file}"]);
        let (gui, warnings) = Config::parse(
            "[editor]\nlaunch='gui'\nprogram='native-editor'\n[ui]\nfile_action='editor'\n",
        );
        assert!(warnings.is_empty());
        assert_eq!(gui.editor.launch, crate::file_open::EditorLaunch::Gui);
        assert_eq!(gui.ui.file_action, crate::file_open::FileAction::Editor);
        let (bad, warnings) = Config::parse("[editor]\nargs=['valid',12]\nprogram=3\n");
        assert_eq!(bad.editor, EditorConfig::default());
        assert_eq!(warnings.len(), 2);
    }

    fn w(text: &str) -> (Config, Vec<String>) {
        Config::parse(text)
    }

    #[test]
    fn empty_text_is_all_defaults_without_warnings() {
        let (c, warn) = w("");
        assert_eq!(c, Config::default());
        assert!(warn.is_empty());
        assert!(c.ui.token_line && c.ui.tips && !c.threads.logging);
        assert_eq!(c.threads.map_chars, 1600);
        assert_eq!(c.mode.default, TrustMode::Ask);
    }

    #[test]
    fn mcp_defaults_off() {
        let (c, warn) = w("");
        assert!(warn.is_empty() && !c.threads.mcp);
        let (c, warn) = w("[threads]\nmcp = true\n");
        assert!(warn.is_empty(), "{warn:?}");
        assert!(c.threads.mcp);
        let (c, warn) = w("[threads]\nmcp = \"yes\"\n");
        assert!(!c.threads.mcp && warn[0].contains("threads.mcp must be true or false"));
    }

    #[test]
    fn thread_logging_accepts_on_and_off() {
        let (c, warnings) = w("[threads]\nlogging = 'on'\n");
        assert!(warnings.is_empty());
        assert!(c.threads.logging);
        let (c, warnings) = w("[threads]\nlogging = 'off'\n");
        assert!(warnings.is_empty());
        assert!(!c.threads.logging);
    }

    #[test]
    fn every_key_is_read() {
        let (c, warn) = w(r#"
[ui]
token_line = false
tips = false
theme = 'light'
background = '#fbf9f1'
foreground = '#232729'
accent = '#1b5a9e'
brand_color = '#c9a5ff'
motion = false
[threads]
logging = "on"
map_chars = 800
[mode]
default = "Full"
[delegation]
protocol = true
"#);
        assert!(warn.is_empty(), "{warn:?}");
        assert!(!c.ui.token_line && !c.ui.tips && c.threads.logging);
        assert_eq!(c.ui.theme, Theme::Light);
        assert_eq!(c.ui.background, Some([251, 249, 241]));
        assert_eq!(c.ui.foreground, Some([35, 39, 41]));
        assert_eq!(c.ui.accent, Some([27, 90, 158]));
        assert_eq!(c.ui.brand_color, Some([201, 165, 255]));
        assert!(!c.ui.motion);
        for (name, theme) in [
            ("default", Theme::Default),
            ("dark", Theme::Dark),
            ("light", Theme::Light),
            ("terminal", Theme::Terminal),
            ("opifex", Theme::Opifex),
        ] {
            let (themed, warnings) = w(&format!("[ui]\ntheme='{name}'\n"));
            assert!(warnings.is_empty(), "{warnings:?}");
            assert_eq!(themed.ui.theme, theme);
        }
        let (invalid, warnings) =
            w("[ui]\ntheme='missing'\nbackground='#fff'\nforeground='blue'\naccent=42\n");
        assert_eq!(warnings.len(), 4);
        assert_eq!(invalid.ui.theme, Theme::Opifex);
        assert!(
            invalid.ui.background.is_none()
                && invalid.ui.foreground.is_none()
                && invalid.ui.accent.is_none()
        );
        let (unreadable, warnings) =
            w("[ui]\nbackground='#101010'\nforeground='#111111'\naccent='#121212'\n");
        assert_eq!(warnings.len(), 2);
        assert_eq!(unreadable.ui.background, Some([16, 16, 16]));
        assert!(unreadable.ui.foreground.is_none() && unreadable.ui.accent.is_none());
        let (host, warnings) = w("[ui]\ntheme='terminal'\nforeground='#111111'\n");
        assert!(warnings.is_empty(), "terminal host background is unknown");
        assert_eq!(host.ui.foreground, Some([17, 17, 17]));
        assert_eq!(c.threads.map_chars, 800);
        assert_eq!(c.mode.default, TrustMode::Full);
        assert!(c.delegation.protocol);
    }

    #[test]
    fn missing_file_is_defaults_and_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let l = Config::load_from_path(dir.path().join("config.toml"));
        assert_eq!(l.config, Config::default());
        assert!(l.warnings.is_empty() && !l.exists);
        assert!(!dir.path().join("config.toml").exists());
    }

    #[test]
    fn malformed_file_gives_defaults_a_position_and_is_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let text = "[ui]\ntips = false\n[mode\ndefault = \"plan\"\n";
        std::fs::write(dir.path().join("config.toml"), text).unwrap();
        let l = Config::load_from_path(dir.path().join("config.toml"));
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
        let l = Config::load_from_path(other.clone());
        assert!(!l.config.ui.tips && l.exists && l.path == other);
    }

    #[test]
    fn crew_configuration_is_ignored_after_removal() {
        let (_, warnings) = Config::parse("[crews.default]\nlead = \"claude:opus\"\nworkers = []\n");
        assert!(warnings.is_empty());
    }
}
