//! Concordia's adapter for the generic `@dariodaddamio/harness` CLI.
//!
//! This deliberately lives in Concordia: Harness owns its generic CLI, while Concordia owns
//! the provider commands and the receipt identity derived from those commands.

use super::{parse_manifest, Manifest, RunOutcome};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

const HARNESS_NAME: &str = "@dariodaddamio/harness";

pub fn is_harness_program(program: &str) -> bool {
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("harness")
                || name.eq_ignore_ascii_case("harness.exe")
                || name.eq_ignore_ascii_case("harness.cmd")
                || name.eq_ignore_ascii_case("harness.bat")
        })
}

fn query(run: &dyn Fn(&[String]) -> RunOutcome, program: &str, args: &[&str]) -> RunOutcome {
    let mut argv = vec![program.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    run(&argv)
}

fn version(run: &dyn Fn(&[String]) -> RunOutcome, program: &str) -> Result<String, String> {
    let outcome = query(run, program, &["version", "--json"]);
    if outcome.timed_out || outcome.exit != Some(0) {
        return Err(format!(
            "`{program} version --json` failed ({})",
            if outcome.timed_out {
                "timed out".to_string()
            } else {
                outcome.stderr.trim().to_string()
            }
        ));
    }
    let value: Value = serde_json::from_str(&outcome.stdout)
        .map_err(|e| format!("`{program} version --json` returned invalid JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| format!("`{program} version --json` must return an object"))?;
    if object.get("name").and_then(Value::as_str) != Some(HARNESS_NAME) {
        return Err(format!(
            "`{program} version --json` returned an unexpected package name"
        ));
    }
    let version = object
        .get("version")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("`{program} version --json` returned no version"))?;
    Ok(version.to_string())
}

fn tools(run: &dyn Fn(&[String]) -> RunOutcome, program: &str) -> Result<Vec<Value>, String> {
    let outcome = query(run, program, &["tools", "--json"]);
    if outcome.timed_out || outcome.exit != Some(0) {
        return Err(format!(
            "`{program} tools --json` failed ({})",
            if outcome.timed_out {
                "timed out".to_string()
            } else {
                outcome.stderr.trim().to_string()
            }
        ));
    }
    let value: Value = serde_json::from_str(&outcome.stdout)
        .map_err(|e| format!("`{program} tools --json` returned invalid JSON: {e}"))?;
    value
        .as_array()
        .cloned()
        .ok_or_else(|| format!("`{program} tools --json` must return an array"))
}

/// Build the exact manifest used by both `/harness connect` and receipt refresh.
pub fn build(
    program: &str,
    cwd: &Path,
    run: &dyn Fn(&[String]) -> RunOutcome,
) -> Result<(Manifest, String), String> {
    let package_version = version(run, program)?;
    let mut providers = BTreeMap::new();
    for provider in ["claude", "codex", "cursor"] {
        let common = vec![
            "--provider".to_string(),
            provider.to_string(),
            "--home".to_string(),
            "{home}".to_string(),
            "--project".to_string(),
            "{project}".to_string(),
            "--json".to_string(),
        ];
        let mut connect = vec![program.to_string(), "connect".to_string()];
        connect.extend(common.clone());
        let mut verify = vec![
            program.to_string(),
            "enforce".to_string(),
            "verify".to_string(),
        ];
        verify.extend(common);
        providers.insert(
            provider.to_string(),
            serde_json::json!({"connect": connect, "verify": verify}),
        );
    }

    let mut registered = Vec::new();
    for item in tools(run, program)? {
        let object = item
            .as_object()
            .ok_or_else(|| "Harness tools must be objects".to_string())?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| "Harness tool has no name".to_string())?;
        let description = object
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("Harness registered tool");
        registered.push(serde_json::json!({
            "name": name,
            "description": description,
            "argv": [program, "tool", name]
        }));
    }
    registered.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));

    let mut root = BTreeMap::new();
    root.insert("name", Value::String("harness".into()));
    root.insert("version", Value::String(package_version));
    root.insert(
        "providers",
        Value::Object(providers.into_iter().collect::<Map<_, _>>()),
    );
    root.insert("tools", Value::Array(registered));
    let text = serde_json::to_string(&root)
        .map_err(|e| format!("could not encode Harness manifest: {e}"))?;
    let manifest = parse_manifest(&text, cwd)?;
    Ok((manifest, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(args: &[String]) -> RunOutcome {
        match args.get(1).map(String::as_str) {
            Some("version") => RunOutcome {
                exit: Some(0),
                stdout: r#"{"name":"@dariodaddamio/harness","version":"0.11.0"}"#.into(),
                ..Default::default()
            },
            Some("tools") => RunOutcome {
                exit: Some(0),
                stdout: r#"[{"name":"reference-search","description":"Search"}]"#.into(),
                ..Default::default()
            },
            _ => RunOutcome {
                exit: Some(1),
                ..Default::default()
            },
        }
    }

    #[test]
    fn builds_actual_program_commands_and_registered_tools() {
        let (manifest, text) = build("C:/bin/harness.cmd", Path::new("."), &fake).unwrap();
        assert!(text.contains("C:/bin/harness.cmd"));
        assert_eq!(manifest.version.as_deref(), Some("0.11.0"));
        assert_eq!(
            manifest.tools[0].argv,
            ["C:/bin/harness.cmd", "tool", "reference-search"]
        );
        assert_eq!(
            manifest.providers["cursor"].verify.as_ref().unwrap()[0],
            "C:/bin/harness.cmd"
        );
    }

    #[test]
    fn version_failure_is_not_silently_replaced_by_the_old_manifest() {
        let error = build("harness", Path::new("."), &|_| RunOutcome {
            exit: Some(1),
            stderr: "not found".into(),
            ..Default::default()
        })
        .unwrap_err();
        assert!(error.contains("version --json"));
    }

    #[test]
    fn tools_change_the_deterministic_manifest_text() {
        let (one, text_one) = build("harness", Path::new("."), &fake).unwrap();
        let (_, text_two) = build("harness", Path::new("."), &|args| {
            if args.get(1).map(String::as_str) == Some("tools") {
                return RunOutcome {
                    exit: Some(0),
                    stdout: r#"[{"name":"craft-clip","description":"Record"}]"#.into(),
                    ..Default::default()
                };
            }
            fake(args)
        })
        .unwrap();
        assert_ne!(text_one, text_two);
        assert_eq!(one.tools[0].name, "reference-search");
    }

    #[test]
    fn tools_lookup_failures_and_wrappers_fail_closed() {
        for outcome in [
            RunOutcome {
                exit: Some(1),
                stderr: "unavailable".into(),
                ..Default::default()
            },
            RunOutcome {
                timed_out: true,
                ..Default::default()
            },
            RunOutcome {
                exit: Some(0),
                stdout: r#"{"tools":[]}"#.into(),
                ..Default::default()
            },
        ] {
            let error = build("harness", Path::new("."), &|args| {
                if args.get(1).map(String::as_str) == Some("tools") {
                    return outcome.clone();
                }
                fake(args)
            })
            .unwrap_err();
            assert!(error.contains("tools --json"));
        }
    }
}
