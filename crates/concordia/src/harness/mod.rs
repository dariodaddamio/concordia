//! Connect any user harness to the providers Concordia launches.
//!
//! A harness is any tool a user wants wired into a provider's isolated home (hooks, rules,
//! skills). It describes itself with a small manifest of argv commands; Concordia never assumes
//! a particular harness exists. Commands run without a shell, so placeholders and arguments are
//! never interpreted as shell syntax.

pub mod command;
pub mod connections;
pub mod run;
pub mod verify;

use crate::store::ProviderKind;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAX_ARGV: usize = 32;
const MAX_ARG_LEN: usize = 4096;
const PLACEHOLDERS: [&str; 3] = ["{provider}", "{home}", "{project}"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCommands {
    /// Wires the harness into one provider home. Required.
    pub connect: Vec<String>,
    /// Proves, with a real attempt, that the harness blocks what it claims to block. Optional.
    pub verify: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: Option<String>,
    /// Directory the manifest came from; relative programs and the skill resolve against it.
    pub dir: PathBuf,
    pub skill: Option<PathBuf>,
    pub providers: BTreeMap<String, ProviderCommands>,
}

/// How a command ended. Output is truncated by the runner.
#[derive(Debug, Clone, Default)]
pub struct RunOutcome {
    pub exit: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// What a command's placeholders expand to.
pub struct Context<'a> {
    pub provider: ProviderKind,
    pub home: Option<&'a Path>,
    pub project: &'a Path,
}

fn check_fields(
    obj: &serde_json::Map<String, Value>,
    allowed: &[&str],
    what: &str,
) -> Result<(), String> {
    match obj.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(extra) => Err(format!(
            "{what}: unknown field `{extra}` (allowed: {})",
            allowed.join(", ")
        )),
        None => Ok(()),
    }
}

fn parse_argv(value: &Value, field: &str) -> Result<Vec<String>, String> {
    let items = value.as_array().ok_or_else(|| {
        format!("{field} must be an array of strings (a command and its arguments)")
    })?;
    if items.is_empty() || items.len() > MAX_ARGV {
        return Err(format!("{field} needs 1 to {MAX_ARGV} entries"));
    }
    let mut argv = Vec::new();
    for item in items {
        let text = item
            .as_str()
            .ok_or_else(|| format!("{field} must contain only strings"))?;
        if text.is_empty() {
            return Err(format!("{field} has an empty entry"));
        }
        if text.len() > MAX_ARG_LEN || text.contains('\0') {
            return Err(format!(
                "{field} has an entry that is too long or contains a NUL"
            ));
        }
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else {
                break;
            };
            let token = &rest[open..open + close + 1];
            if !PLACEHOLDERS.contains(&token) {
                return Err(format!(
                    "{field} uses {token}, which is not a placeholder (use {{provider}}, {{home}} or {{project}})"
                ));
            }
            rest = &rest[open + close + 1..];
        }
        argv.push(text.to_string());
    }
    Ok(argv)
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 40
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Parse and validate a manifest. Errors are short and name the field to fix.
pub fn parse_manifest(text: &str, dir: &Path) -> Result<Manifest, String> {
    let root: Value =
        serde_json::from_str(text).map_err(|e| format!("manifest is not valid JSON: {e}"))?;
    let obj = root.as_object().ok_or("manifest must be a JSON object")?;
    check_fields(obj, &["name", "version", "skill", "providers"], "manifest")?;
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| valid_name(n))
        .ok_or("name must be 1 to 40 characters: lowercase letters, digits and dashes")?
        .to_string();
    let version = match obj.get("version") {
        None => None,
        Some(v) => Some(
            v.as_str()
                .filter(|v| !v.is_empty() && v.len() <= 64)
                .ok_or("version must be a short string")?
                .to_string(),
        ),
    };
    let skill = match obj.get("skill") {
        None => None,
        Some(v) => {
            let rel = v.as_str().ok_or("skill must be a relative path string")?;
            let path = Path::new(rel);
            let unsafe_path = rel.is_empty()
                || path.is_absolute()
                || rel.starts_with('/')
                || rel.starts_with('\\')
                || rel.chars().nth(1) == Some(':')
                || rel.split(['/', '\\']).any(|part| part == "..");
            if unsafe_path {
                return Err("skill must be a relative path inside the harness folder".into());
            }
            Some(dir.join(path))
        }
    };
    let raw = obj
        .get("providers")
        .and_then(Value::as_object)
        .filter(|p| !p.is_empty())
        .ok_or("providers must list at least one provider: codex, claude or cursor")?;
    let mut providers = BTreeMap::new();
    for (key, value) in raw {
        if !matches!(key.as_str(), "codex" | "claude" | "cursor") {
            return Err(format!(
                "provider `{key}` is not supported (codex, claude, cursor)"
            ));
        }
        let entry = value
            .as_object()
            .ok_or_else(|| format!("providers.{key} must be an object"))?;
        check_fields(entry, &["connect", "verify"], &format!("providers.{key}"))?;
        let connect = parse_argv(
            entry
                .get("connect")
                .ok_or_else(|| format!("providers.{key}.connect is required"))?,
            &format!("providers.{key}.connect"),
        )?;
        let verify = entry
            .get("verify")
            .map(|v| parse_argv(v, &format!("providers.{key}.verify")))
            .transpose()?;
        providers.insert(key.clone(), ProviderCommands { connect, verify });
    }
    Ok(Manifest {
        name,
        version,
        dir: dir.to_path_buf(),
        skill,
        providers,
    })
}

/// Substitute placeholders. A command that needs `{home}` for an account with no isolated home
/// is refused instead of guessing a directory.
pub fn expand(argv: &[String], ctx: &Context<'_>) -> Result<Vec<String>, String> {
    argv.iter()
        .map(|arg| {
            let mut out = arg.replace("{provider}", ctx.provider.as_str());
            out = out.replace("{project}", &ctx.project.to_string_lossy());
            if out.contains("{home}") {
                let home = ctx.home.ok_or(
                    "this account has no isolated home, so a command that needs {home} cannot run",
                )?;
                out = out.replace("{home}", &home.to_string_lossy());
            }
            Ok(out)
        })
        .collect()
}

/// A program with a path separator is relative to the manifest; a bare name is looked up on PATH.
pub fn resolve_program(program: &str, dir: &Path) -> PathBuf {
    let path = Path::new(program);
    if path.is_absolute() || !(program.contains('/') || program.contains('\\')) {
        path.to_path_buf()
    } else {
        dir.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ProviderKind;
    use std::path::Path;

    const VALID: &str = r#"{
        "name": "my-harness",
        "version": "1.2.0",
        "skill": "skills/connect",
        "providers": {
            "claude": {
                "connect": ["./bin/h", "connect", "{provider}", "{home}", "{project}"],
                "verify": ["./bin/h", "verify", "{provider}", "{home}", "{project}"]
            },
            "codex": { "connect": ["h", "connect", "codex", "{home}"] }
        }
    }"#;

    #[test]
    fn a_valid_manifest_parses() {
        let m = parse_manifest(VALID, Path::new("/opt/h")).unwrap();
        assert_eq!(m.name, "my-harness");
        assert_eq!(m.version.as_deref(), Some("1.2.0"));
        assert_eq!(m.skill.as_deref(), Some(Path::new("/opt/h/skills/connect")));
        let claude = &m.providers["claude"];
        assert_eq!(claude.connect[0], "./bin/h");
        assert!(claude.verify.is_some());
        assert!(m.providers["codex"].verify.is_none());
    }

    fn rejected(text: &str) -> String {
        parse_manifest(text, Path::new("/opt/h")).unwrap_err()
    }

    #[test]
    fn typos_and_unknown_fields_are_refused_not_ignored() {
        let e = rejected(r#"{"name":"x","providers":{"claude":{"connect":["h"]}},"extra":1}"#);
        assert!(e.contains("extra"), "{e}");
        let e = rejected(r#"{"name":"x","providers":{"claude":{"conect":["h"]}}}"#);
        assert!(e.contains("conect"), "{e}");
    }

    #[test]
    fn names_providers_and_argv_are_validated() {
        assert!(
            rejected(r#"{"name":"Bad Name","providers":{"claude":{"connect":["h"]}}}"#)
                .contains("name")
        );
        assert!(
            rejected(r#"{"name":"x","providers":{"gemini":{"connect":["h"]}}}"#).contains("gemini")
        );
        assert!(rejected(r#"{"name":"x","providers":{}}"#).contains("provider"));
        assert!(
            rejected(r#"{"name":"x","providers":{"claude":{"connect":[]}}}"#).contains("connect")
        );
        assert!(
            rejected(r#"{"name":"x","providers":{"claude":{"connect":["h",""]}}}"#)
                .contains("empty")
        );
        assert!(
            rejected(r#"{"name":"x","providers":{"claude":{"connect":"h --go"}}}"#)
                .contains("connect")
        );
        assert!(rejected("not json").contains("JSON"));
    }

    #[test]
    fn unknown_placeholders_and_unsafe_skill_paths_are_refused() {
        let e = rejected(r#"{"name":"x","providers":{"claude":{"connect":["h","{secret}"]}}}"#);
        assert!(e.contains("{secret}"), "{e}");
        for skill in ["../up", "/abs/path", "C:/abs", "a/../../b"] {
            let text = format!(
                r#"{{"name":"x","skill":"{skill}","providers":{{"claude":{{"connect":["h"]}}}}}}"#
            );
            assert!(rejected(&text).contains("skill"), "{skill}");
        }
    }

    fn ctx<'a>(home: Option<&'a Path>) -> Context<'a> {
        Context {
            provider: ProviderKind::Claude,
            home,
            project: Path::new("/work/app"),
        }
    }

    #[test]
    fn placeholders_expand_inside_arguments_and_never_reach_a_shell() {
        let argv: Vec<String> = ["h", "--to={home}", "{provider}", "a;b && c", "{project}"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let out = expand(&argv, &ctx(Some(Path::new("/homes/c1")))).unwrap();
        assert_eq!(out[1], "--to=/homes/c1");
        assert_eq!(out[2], "claude");
        assert_eq!(out[3], "a;b && c", "metacharacters stay literal");
        assert_eq!(out[4], "/work/app");
    }

    #[test]
    fn a_home_placeholder_needs_an_isolated_home() {
        let argv = vec!["h".to_string(), "{home}".to_string()];
        let e = expand(&argv, &ctx(None)).unwrap_err();
        assert!(e.contains("isolated home"), "{e}");
        let argv = vec!["h".to_string(), "{provider}".to_string()];
        assert!(expand(&argv, &ctx(None)).is_ok());
    }

    #[test]
    fn a_relative_program_resolves_against_the_manifest_directory() {
        let base = Path::new("/opt/h");
        assert_eq!(
            resolve_program("./bin/h", base),
            Path::new("/opt/h").join("./bin/h")
        );
        assert_eq!(resolve_program("tool", base), Path::new("tool"));
        assert_eq!(
            resolve_program("/usr/bin/tool", base),
            Path::new("/usr/bin/tool")
        );
    }
}
