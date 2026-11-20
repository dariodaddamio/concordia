//! Strict, file-backed theme format. Theme files are data and never executable code.
use crate::config::{contrast, Rgb};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MAX_THEME_BYTES: usize = 16 * 1024;
const KEYS: &[&str] = &[
    "name",
    "author",
    "background",
    "foreground",
    "accent",
    "dim",
    "success",
    "warning",
    "error",
    "selection",
    "border",
    "portrait_tint",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeFile {
    pub name: String,
    pub author: String,
    pub background: Rgb,
    pub foreground: Rgb,
    pub accent: Rgb,
    pub dim: Rgb,
    pub success: Rgb,
    pub warning: Rgb,
    pub error: Rgb,
    pub selection: Rgb,
    pub border: Rgb,
    pub portrait_tint: Option<Rgb>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContrastWarning {
    pub foreground: f64,
    pub dim: f64,
}

impl ThemeFile {
    pub fn contrast_warning(&self) -> Option<ContrastWarning> {
        let foreground = contrast(self.foreground, self.background);
        let dim = contrast(self.dim, self.background);
        (foreground < 4.5 || dim < 3.0).then_some(ContrastWarning { foreground, dim })
    }
}

fn line_for(text: &str, key: &str) -> usize {
    text.lines()
        .position(|line| {
            line.split_once('=')
                .is_some_and(|(left, _)| left.trim() == key)
        })
        .map_or(1, |line| line + 1)
}

fn color(value: &toml::Value, key: &str) -> Result<Rgb, String> {
    let Some(raw) = value.as_str() else {
        return Err(format!("{key} must be #RRGGBB"));
    };
    if raw.len() != 7 || !raw.starts_with('#') || !raw[1..].bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{key} must be #RRGGBB"));
    }
    Ok([
        u8::from_str_radix(&raw[1..3], 16).unwrap(),
        u8::from_str_radix(&raw[3..5], 16).unwrap(),
        u8::from_str_radix(&raw[5..7], 16).unwrap(),
    ])
}

pub fn parse(text: &str) -> Result<ThemeFile, String> {
    if text.len() > MAX_THEME_BYTES {
        return Err(format!("theme file exceeds {MAX_THEME_BYTES} bytes"));
    }
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| format!("invalid theme TOML: {e}"))?;
    let unknown: BTreeSet<_> = table
        .keys()
        .filter(|key| !KEYS.contains(&key.as_str()))
        .collect();
    if let Some(key) = unknown.into_iter().next() {
        return Err(format!(
            "unknown theme key '{key}' at line {}",
            line_for(text, key)
        ));
    }
    let required = |key: &str| {
        table
            .get(key)
            .ok_or_else(|| format!("missing theme field '{key}'"))
    };
    let string = |key: &str| {
        required(key)?
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("{key} must be a non-empty string"))
    };
    Ok(ThemeFile {
        name: string("name")?,
        author: string("author")?,
        background: color(required("background")?, "background")?,
        foreground: color(required("foreground")?, "foreground")?,
        accent: color(required("accent")?, "accent")?,
        dim: color(required("dim")?, "dim")?,
        success: color(required("success")?, "success")?,
        warning: color(required("warning")?, "warning")?,
        error: color(required("error")?, "error")?,
        selection: color(required("selection")?, "selection")?,
        border: color(required("border")?, "border")?,
        portrait_tint: table
            .get("portrait_tint")
            .map(|v| color(v, "portrait_tint"))
            .transpose()?,
    })
}

pub fn load(path: &Path) -> Result<ThemeFile, String> {
    let metadata =
        std::fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if !metadata.is_file() {
        return Err("theme path is not a regular file".into());
    }
    if metadata.len() > MAX_THEME_BYTES as u64 {
        return Err(format!("theme file exceeds {MAX_THEME_BYTES} bytes"));
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|file| {
            file.take(MAX_THEME_BYTES as u64 + 1)
                .read_to_string(&mut text)
        })
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse(&text)
}

pub fn builtin(name: &str) -> Result<ThemeFile, String> {
    let text = match name.to_ascii_lowercase().as_str() {
        "dark" => include_str!("themes/dark.toml"),
        "light" => include_str!("themes/light.toml"),
        "opifex" => include_str!("themes/opifex.toml"),
        _ => return Err(format!("unknown built-in theme '{name}'")),
    };
    parse(text)
}

pub fn library_dir(root: &Path) -> PathBuf {
    root.join("themes")
}

pub fn import(root: &Path, source: &Path) -> Result<ThemeFile, String> {
    let theme = load(source)?;
    std::fs::create_dir_all(library_dir(root))
        .map_err(|e| format!("cannot create theme library: {e}"))?;
    let destination = library_dir(root).join(format!("{}.toml", safe_name(&theme.name)));
    crate::state_io::write_atomic(&destination, serialize(&theme).as_bytes())
        .map_err(|e| format!("cannot copy theme: {e}"))?;
    Ok(theme)
}

pub fn export(
    root: &Path,
    theme: &ThemeFile,
    destination: Option<&Path>,
) -> Result<PathBuf, String> {
    let path = destination.map_or_else(
        || library_dir(root).join(format!("{}.toml", safe_name(&theme.name))),
        PathBuf::from,
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    crate::state_io::write_atomic(&path, serialize(theme).as_bytes()).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn serialize(theme: &ThemeFile) -> String {
    let hex = |c: Rgb| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
    let quote = |text: &str| {
        serde_json::to_string(text)
            .expect("string serialization")
            .replace('\u{7f}', "\\u007f")
    };
    format!("name = {}\nauthor = {}\nbackground = \"{}\"\nforeground = \"{}\"\naccent = \"{}\"\ndim = \"{}\"\nsuccess = \"{}\"\nwarning = \"{}\"\nerror = \"{}\"\nselection = \"{}\"\nborder = \"{}\"{}\n", quote(&theme.name), quote(&theme.author), hex(theme.background), hex(theme.foreground), hex(theme.accent), hex(theme.dim), hex(theme.success), hex(theme.warning), hex(theme.error), hex(theme.selection), hex(theme.border), theme.portrait_tint.map_or(String::new(), |c| format!("\nportrait_tint = \"{}\"", hex(c))))
}

fn safe_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn valid() -> String {
        "name='Test'\nauthor='A'\nbackground='#000000'\nforeground='#FFFFFF'\naccent='#FF0000'\ndim='#AAAAAA'\nsuccess='#00FF00'\nwarning='#FFFF00'\nerror='#FF00FF'\nselection='#111111'\nborder='#222222'\n".into()
    }
    #[test]
    fn valid_and_invalid_theme_files() {
        for (bad, needle) in [
            ("foreground='nope'", "foreground"),
            ("mystery=1", "unknown theme key"),
            (&"x".repeat(MAX_THEME_BYTES + 1), "exceeds"),
            ("", "missing theme field"),
        ] {
            let mut text = valid();
            if bad == "foreground='nope'" {
                text = text.replace("foreground='#FFFFFF'", bad);
            } else if bad.starts_with('x') {
                text = bad.to_string();
            } else if !bad.is_empty() {
                text.push_str(bad);
            } else {
                text.clear();
            }
            assert!(parse(&text).unwrap_err().contains(needle));
        }
    }
    #[test]
    fn low_contrast_theme_loads_with_warning() {
        let mut t = valid();
        t = t
            .replace("foreground='#FFFFFF'", "foreground='#010101'")
            .replace("dim='#AAAAAA'", "dim='#020202'");
        let parsed = parse(&t).unwrap();
        assert!(parsed.contrast_warning().is_some());
    }
    #[test]
    fn builtin_themes_parse_through_the_same_loader() {
        for name in ["dark", "light", "opifex"] {
            assert!(builtin(name).unwrap().name.len() > 2);
        }
    }

    #[test]
    fn import_export_round_trip() {
        let source_root = tempfile::tempdir().unwrap();
        let destination_root = tempfile::tempdir().unwrap();
        let theme = builtin("opifex").unwrap();
        let exported = export(
            source_root.path(),
            &theme,
            Some(&source_root.path().join("exported.toml")),
        )
        .unwrap();

        let imported = import(destination_root.path(), &exported).unwrap();
        assert_eq!(imported, theme);
    }
}
