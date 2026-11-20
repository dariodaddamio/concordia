//! Bounded, evidence-labelled inventory for provider-owned skills and hooks.
//!
//! This module deliberately separates local configuration evidence from provider exposure.
//! Reading a source is never treated as enabling it, and no provider trust or registration is
//! changed here.

use super::{App, PickerKind, PickerRow, ProviderKind};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, SystemTime};

fn row(id: impl Into<String>, label: impl Into<String>, detail: impl Into<String>) -> PickerRow {
    PickerRow {
        id: id.into(),
        label: label.into(),
        detail: detail.into(),
    }
}

fn source_row(path: &Path, scope: &str, name: &str, evidence: &str) -> PickerRow {
    row(
        format!("file:{}", path.display()),
        format!("{scope} · {name}"),
        format!("{evidence} · source: {}", path.display()),
    )
}

/// Compact age, so the state text still fits at the default window width.
fn age(path: &Path) -> String {
    let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) else {
        return "age ?".into();
    };
    let seconds = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default()
        .as_secs();
    match seconds {
        0..=59 => format!("{seconds}s ago"),
        60..=3599 => format!("{}m ago", seconds / 60),
        3600..=86399 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86400),
    }
}

/// The five states in a fixed order. `?` means no evidence either way.
fn states(enabled: &str, exposed: &str, proven: &str, age: &str) -> String {
    format!(
        "configured · discovered · enabled {enabled} · exposed {exposed} · proven {proven} · {age}"
    )
}

fn skill_rows(dir: &Path, scope: &str, rows: &mut Vec<PickerRow>, visited: &mut usize) {
    if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten().take(256) {
        if *visited >= 512 {
            break;
        }
        *visited += 1;
        let folder = entry.path();
        let source = folder.join("SKILL.md");
        let linked =
            |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink());
        if linked(&folder) || linked(&source) || !folder.is_dir() || !source.is_file() {
            continue;
        }
        rows.push(source_row(
            &source,
            scope,
            &entry.file_name().to_string_lossy(),
            &states("?", "?", "no", &age(&source)),
        ));
    }
}

/// One hook from a Codex `hooks/list` answer.
struct NativeHook {
    event: String,
    source_path: String,
    source: String,
    enabled: bool,
    trust: String,
}

/// Answer of the read-only Codex query: `Err` when it could not be asked or answered.
type Native = Result<Vec<NativeHook>, String>;

fn norm_path(path: &str) -> String {
    path.trim_start_matches(r"\\?\")
        .replace('\\', "/")
        .to_lowercase()
}

fn hook_row(path: &Path, scope: &str, native: Option<&Native>, rows: &mut Vec<PickerRow>) {
    if std::fs::symlink_metadata(path).is_err() {
        return;
    }
    let text = match super::settings::bounded_text(path) {
        Ok(text) => text,
        Err(reason) => {
            rows.push(row(
                "notice",
                format!("{scope} · source unavailable"),
                format!(
                    "{reason}; enabled/exposed/proven unavailable · {}",
                    path.display()
                ),
            ));
            return;
        }
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        rows.push(row(
            "notice",
            format!("{scope} · source unavailable"),
            format!(
                "invalid JSON; configured/discovered unknown; enabled/exposed/proven unavailable · {}",
                path.display()
            ),
        ));
        return;
    };
    let disabled = value
        .get("disableAllHooks")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let age = age(path);
    let want = norm_path(&path.display().to_string());
    let line = |event: &str| match native {
        // The native answer decides exposure and enablement for a matching hook.
        Some(Ok(list)) => {
            let hit = list
                .iter()
                .find(|h| h.event.eq_ignore_ascii_case(event) && norm_path(&h.source_path) == want);
            match hit {
                Some(h) => states(if h.enabled { "yes" } else { "no" }, "yes", "now", &age),
                None => states("?", "none", "now", &age),
            }
        }
        _ => states(
            if disabled { "off in source" } else { "?" },
            "?",
            "no",
            &age,
        ),
    };
    let Some(hooks) = value.get("hooks").and_then(Value::as_object) else {
        rows.push(source_row(path, scope, "configured source", &line("")));
        return;
    };
    for (event, _) in hooks.iter().take(128) {
        rows.push(source_row(path, scope, event, &line(event)));
    }
}

/// Stops and reaps the app-server child on every path out of `codex_hooks`.
struct Reaped(std::process::Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Ask a read-only Codex app-server which hooks it would run for `project`.
/// Nothing is trusted, enabled or written; the child dies when this returns.
fn codex_hooks(home: Option<&Path>, project: &Path) -> Native {
    let home = home.ok_or("selected Codex account home unavailable")?;
    let bin = crate::provider::codex::find_codex_bin()?;
    let mut child = Reaped(
        crate::process_ui::command(bin)
            .arg("app-server")
            .env("CODEX_HOME", home)
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "could not start read-only Codex app-server".to_owned())?,
    );
    let mut input = child
        .0
        .stdin
        .take()
        .ok_or("Codex app-server stdin unavailable")?;
    let stdout = child
        .0
        .stdout
        .take()
        .ok_or("Codex app-server stdout unavailable")?;
    let init = json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"concordia_inventory","version":crate::version::VERSION}}});
    writeln!(input, "{init}").map_err(|_| "Codex initialization failed")?;
    writeln!(input, "{{\"method\":\"initialized\"}}").map_err(|_| "Codex initialization failed")?;
    let request =
        json!({"id":2,"method":"hooks/list","params":{"cwds":[project.display().to_string()]}});
    writeln!(input, "{request}").map_err(|_| "Codex hooks/list request failed")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut lines = BufReader::new(stdout).lines();
        let mut result = None;
        for _ in 0..128 {
            let Some(Ok(line)) = lines.next() else { break };
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value.get("id") == Some(&json!(2)) {
                result = Some(if value.get("error").is_some() {
                    Err("Codex rejected read-only hooks/list".to_owned())
                } else {
                    Ok(value.get("result").cloned().unwrap_or(Value::Null))
                });
                break;
            }
        }
        let _ = tx.send(result);
    });
    let result = rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "Codex hooks/list timed out")?
        .ok_or("Codex app-server closed before hooks/list")??;
    let text = |hook: &Value, key: &str| {
        hook.get(key)
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_owned()
    };
    Ok(result
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            entry
                .get("hooks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .take(128)
        .map(|hook| NativeHook {
            event: text(hook, "eventName"),
            source_path: text(hook, "sourcePath"),
            source: text(hook, "source"),
            enabled: hook
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            trust: text(hook, "trustStatus"),
        })
        .collect())
}

/// Rows for the native answer: one per hook with its enabled and trust state, or `none`.
fn native_rows(native: &Native, rows: &mut Vec<PickerRow>) {
    match native {
        Err(why) => rows.push(row(
            "notice",
            "Native Codex exposure",
            format!("exposed ?; proven no: {why}"),
        )),
        Ok(list) if list.is_empty() => rows.push(row(
            "notice",
            "Native Codex exposure",
            "exposed none (0 hooks); proven now (hooks/list for this project)",
        )),
        Ok(list) => {
            for hook in list {
                rows.push(row(
                    "notice",
                    format!("Native · {} · {}", hook.event, hook.source),
                    format!(
                        "enabled {} · trust {} · exposed yes · proven now (hooks/list)",
                        if hook.enabled { "yes" } else { "no" },
                        hook.trust
                    ),
                ));
            }
        }
    }
}

pub(super) fn open(app: &mut App, hooks: bool) {
    let provider = app.provider;
    let subdir = match provider {
        ProviderKind::Codex => ".agents",
        ProviderKind::Claude => ".claude",
        ProviderKind::Cursor => ".cursor",
        ProviderKind::Grok => ".grok",
        ProviderKind::Opencode => ".opencode",
    };
    let mut rows = vec![
        row(
            "notice",
            "Opening a source proves nothing",
            "Enter opens the file and proves none of these states; ? means unknown.",
        ),
        row(
            "notice",
            format!("Provider: {}", provider.as_str()),
            format!("project: {}", app.cwd.display()),
        ),
    ];
    let mut roots = vec![app.cwd.clone()];
    for parent in app.cwd.ancestors().take(32) {
        if parent.join(".git").exists() {
            roots = app
                .cwd
                .ancestors()
                .take_while(|p| *p != parent)
                .map(Path::to_path_buf)
                .chain([parent.to_path_buf()])
                .collect();
            break;
        }
    }
    let mut visited = 0;
    let native = (hooks && provider == ProviderKind::Codex)
        .then(|| codex_hooks(app.home_path.as_deref(), &app.cwd));
    for root in roots {
        if hooks {
            match provider {
                ProviderKind::Claude => {
                    hook_row(
                        &root.join(".claude/settings.json"),
                        "Project · shared",
                        native.as_ref(),
                        &mut rows,
                    );
                    hook_row(
                        &root.join(".claude/settings.local.json"),
                        "Project · local",
                        native.as_ref(),
                        &mut rows,
                    );
                }
                ProviderKind::Cursor => hook_row(
                    &root.join(".cursor/hooks.json"),
                    "Project",
                    native.as_ref(),
                    &mut rows,
                ),
                ProviderKind::Codex => hook_row(
                    &root.join(".codex/hooks.json"),
                    "Project",
                    native.as_ref(),
                    &mut rows,
                ),
                _ => {}
            }
        } else {
            skill_rows(
                &root.join(subdir).join("skills"),
                "Project",
                &mut rows,
                &mut visited,
            );
        }
    }
    if let Some(home) = app.home_path.as_deref() {
        if hooks {
            match provider {
                ProviderKind::Claude => hook_row(
                    &home.join("settings.json"),
                    "User",
                    native.as_ref(),
                    &mut rows,
                ),
                ProviderKind::Cursor => {
                    hook_row(&home.join("hooks.json"), "User", native.as_ref(), &mut rows)
                }
                ProviderKind::Codex => {
                    hook_row(&home.join("hooks.json"), "User", native.as_ref(), &mut rows);
                    if let Some(native) = &native {
                        native_rows(native, &mut rows);
                    }
                }
                _ => hook_row(&home.join("hooks.json"), "User", native.as_ref(), &mut rows),
            }
        } else {
            skill_rows(
                &home.join("skills"),
                "User · selected account",
                &mut rows,
                &mut visited,
            );
            if provider == ProviderKind::Codex {
                skill_rows(
                    &home.join("skills/.system"),
                    "System",
                    &mut rows,
                    &mut visited,
                );
            }
        }
    } else {
        rows.push(row("notice", "Selected account unavailable", "configured/discovered may be visible at project scope; enabled/exposed/freshly proven unavailable without the provider home"));
    }
    if !hooks {
        rows.push(row("notice", "Provider exposure", "Skills are local source evidence only; the provider's plugin/managed catalog is not enumerated here, so exposed and freshly proven remain unavailable."));
    }
    app.show_picker(
        PickerKind::Customization,
        format!(
            "{} · {} · evidence",
            if hooks { "Hooks" } else { "Skills" },
            provider.as_str()
        ),
        rows,
        true,
    );
}
