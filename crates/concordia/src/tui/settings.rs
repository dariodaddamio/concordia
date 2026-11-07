//! User settings and local provider customization inventory. Never executes discovered content.
use super::*;

fn row(id: &str, label: impl Into<String>, detail: impl Into<String>) -> PickerRow {
    PickerRow {
        id: id.into(),
        label: label.into(),
        detail: detail.into(),
    }
}

pub(super) fn open(app: &mut App) {
    let ui = &app.appearance.ui;
    let settings = crate::store::get_settings(&app.state_root);
    let enabled = |on| if on { "On" } else { "Off" };
    let rows = vec![
        row(
            "appearance",
            "Appearance",
            "App-wide saved theme and window icon color",
        ),
        row(
            "tokens",
            format!("Response usage · {}", enabled(ui.token_line)),
            "App-wide saved preference; provider-reported tokens only",
        ),
        row(
            "tips",
            format!("Idle tips · {}", enabled(ui.tips)),
            "App-wide saved preference",
        ),
        row(
            "stats",
            format!("CPU and memory · {}", enabled(app.stats.is_some())),
            "App-wide saved preference",
        ),
        row(
            "motion",
            format!("Window icon animation · {}", enabled(ui.motion)),
            "Saved; Windows reduced-motion preferences take precedence",
        ),
        row(
            "permissions",
            "Permissions",
            "Saved default for future turns; running turns retain their permissions",
        ),
        row(
            "accounts",
            "Accounts",
            "Saved selected provider/account; native sign-ins stay separate",
        ),
        row(
            "models",
            "Model",
            "Saved selection for the current provider account",
        ),
        row(
            "continuation",
            format!(
                "Default limit handoff · {}",
                if app.continuation.automatic() {
                    "Automatic"
                } else {
                    "Pause"
                }
            ),
            format!("Saved for future sessions; context: {} at {}% (Cursor disables fresh when usage is unreported)", context_label(app), crate::config::for_root(&app.state_root).continuation.context.threshold),
        ),
        row(
            "session-handoff",
            "This session’s handoff",
            "Session only: choose the next provider or automatic/manual behavior",
        ),
        row(
            "returns",
            format!(
                "Worker result delivery · {}",
                if settings.delegate_auto_return {
                    "Automatic"
                } else {
                    "Manual"
                }
            ),
            "Saved; automatic delivery may start a main-agent turn when idle",
        ),
        row(
            "files",
            "File links",
            "Saved default: preview, split window or external editor",
        ),
        row(
            "skills",
            "Skills",
            "Inspect project and provider-account sources; opening does not invoke a skill",
        ),
        row(
            "hooks",
            "Hooks",
            "Inspect project and provider-account sources; hooks run in their native provider",
        ),
        row(
            "integrations",
            "Integrations",
            "Inspect and manage explicitly connected tools",
        ),
        row(
            "advanced",
            "Advanced config.toml",
            "Open the app-wide defaults file; manual changes apply after restarting",
        ),
    ];
    app.show_picker(
        PickerKind::Settings,
        "Settings · saved on this device".into(),
        rows,
        true,
    );
}

fn context_label(app: &App) -> String {
    let c = crate::config::for_root(&app.state_root)
        .continuation
        .context;
    let (mode, _) = c.for_provider(app.provider);
    mode.to_owned()
}

pub(super) fn choose(app: &mut App, kind: PickerKind, id: &str) {
    if kind == PickerKind::Customization {
        if let Some(path) = id.strip_prefix("file:") {
            app.file_command(path, false, true);
        }
        return;
    }
    if kind == PickerKind::FilePreferences {
        let action = match id {
            "preview" => crate::file_open::FileAction::Preview,
            "split" => crate::file_open::FileAction::Split,
            "editor" => crate::file_open::FileAction::Editor,
            _ => return,
        };
        match crate::store::set_file_action(&app.state_root, action) {
            Ok(()) => app.flash("File preference saved"),
            Err(error) => app.flash(format!("Not saved: {error}")),
        }
        open(app);
        return;
    }
    match id {
        "appearance" => appearance::open(app),
        "tokens" | "tips" | "stats" | "motion" => {
            let current = match id {
                "tokens" => app.appearance.ui.token_line,
                "tips" => app.appearance.ui.tips,
                "motion" => app.appearance.ui.motion,
                _ => app.stats.is_some(),
            };
            appearance::save(app, id, if current { "off" } else { "on" });
            open(app);
        }
        "permissions" => app.open_trust_picker(),
        "accounts" => {
            if app.busy {
                app.flash("Finish or cancel the current turn before changing account");
            } else {
                app.build_use_picker();
            }
        }
        "models" => {
            if app.busy {
                app.flash("Finish or cancel the current turn before changing model");
            } else {
                app.open_model_picker();
            }
        }
        "continuation" => {
            let next = !app.continuation.automatic();
            match crate::store::set_continuation_automatic(&app.state_root, next) {
                Ok(()) => {
                    app.continuation.toggle_automatic();
                    app.flash("Continuation preference saved");
                }
                Err(error) => app.flash(format!("Not saved: {error}")),
            }
            open(app);
        }
        "session-handoff" => app.open_handoff_settings(),
        "returns" => {
            let automatic = crate::store::get_settings(&app.state_root).delegate_auto_return;
            app.handle_slash(if automatic {
                "/delegate returns manual"
            } else {
                "/delegate returns auto"
            });
            open(app);
        }
        "files" => app.show_picker(
            PickerKind::FilePreferences,
            "File links · saved default".into(),
            vec![
                row("preview", "Preview here", "Open inside the current window"),
                row("split", "Split off", "Open in an independent window"),
                row(
                    "editor",
                    "External editor",
                    "Use the configured editor executable",
                ),
            ],
            false,
        ),
        "skills" => catalog(app, false),
        "hooks" => catalog(app, true),
        "integrations" => integrations::open(app),
        "advanced" => {
            let path = app.state_root.join("config.toml");
            if path.is_file() {
                app.file_command(&path.to_string_lossy(), false, true);
            } else {
                app.flash("No config.toml yet; defaults are active. See docs/config.md for available settings.");
            }
        }
        _ => {}
    }
}

fn bounded_text(path: &Path) -> Result<String, &'static str> {
    if path
        .ancestors()
        .any(|part| std::fs::symlink_metadata(part).is_ok_and(|m| m.file_type().is_symlink()))
    {
        return Err("linked source; inspect through the native provider");
    }
    let meta = std::fs::symlink_metadata(path).map_err(|_| "unreadable source")?;
    if !meta.is_file() || meta.len() > 64 * 1024 {
        return Err("source is linked, non-file or exceeds 64 KiB");
    }
    let mut file = std::fs::File::open(path).map_err(|_| "unreadable source")?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(&mut file, 64 * 1024 + 1),
        &mut bytes,
    )
    .map_err(|_| "unreadable source")?;
    if bytes.len() > 64 * 1024 {
        return Err("source exceeds 64 KiB");
    }
    String::from_utf8(bytes).map_err(|_| "source is not UTF-8")
}

fn source_row(path: &Path, scope: &str, name: &str, detail: &str) -> PickerRow {
    row(
        &format!("file:{}", path.display()),
        format!("{scope} · {name}"),
        format!("{detail} · {}", path.display()),
    )
}

fn skill_rows(directory: &Path, scope: &str, rows: &mut Vec<PickerRow>, visited: &mut usize) {
    if std::fs::symlink_metadata(directory).is_ok_and(|m| m.file_type().is_symlink()) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.take(256).flatten() {
        if *visited >= 512 {
            break;
        }
        *visited += 1;
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let path = entry.path().join("SKILL.md");
        if !path.exists() {
            continue;
        }
        let folder = entry.file_name().to_string_lossy().into_owned();
        match bounded_text(&path) {
            Ok(text) => {
                let description = text
                    .lines()
                    .take(60)
                    .find_map(|line| line.strip_prefix("description:"))
                    .unwrap_or("Local skill definition")
                    .trim()
                    .trim_matches(['\'', '"']);
                rows.push(source_row(
                    &path,
                    scope,
                    &folder,
                    &crate::sessions::clean_preview(Some(description), 120),
                ));
            }
            Err(error) => rows.push(row("notice", format!("{scope} · {folder}"), error)),
        }
    }
}

fn hook_rows(path: &Path, scope: &str, rows: &mut Vec<PickerRow>) {
    if !path.exists() {
        return;
    }
    match bounded_text(path).and_then(|text| {
        serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|_| "invalid JSON; native provider decides whether it can load")
    }) {
        Ok(value) => {
            let disabled = value
                .get("disableAllHooks")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if let Some(hooks) = value.get("hooks").and_then(|v| v.as_object()) {
                for (event, entries) in hooks.iter().take(128) {
                    let count = entries.as_array().map_or(1, Vec::len);
                    rows.push(source_row(
                        path,
                        scope,
                        event,
                        &format!(
                            "{count} configured group(s){}; provider-owned",
                            if disabled {
                                "; disabled in this source"
                            } else {
                                ""
                            }
                        ),
                    ));
                }
            }
        }
        Err(error) => rows.push(row(
            "notice",
            format!("{scope} · source unavailable"),
            format!("{error} · {}", path.display()),
        )),
    }
}

pub(super) fn catalog(app: &mut App, hooks: bool) {
    let mut rows = vec![row("notice", "Project vs user scope", "Project: this repository. User: selected provider home across projects. Native managed/plugin rules can add or override entries.")];
    let mut roots = vec![app.cwd.clone()];
    for parent in app.cwd.ancestors().take(32) {
        if parent.join(".git").exists() {
            roots = app
                .cwd
                .ancestors()
                .take_while(|p| *p != parent)
                .map(Path::to_path_buf)
                .chain(std::iter::once(parent.to_path_buf()))
                .collect();
            break;
        }
    }
    let mut visited = 0;
    let provider = app.provider;
    let subdir = match provider {
        ProviderKind::Codex => ".agents",
        ProviderKind::Claude => ".claude",
        ProviderKind::Cursor => ".cursor",
    };
    for root in roots {
        if hooks {
            match provider {
                ProviderKind::Claude => {
                    hook_rows(
                        &root.join(".claude/settings.json"),
                        "Project · shared",
                        &mut rows,
                    );
                    hook_rows(
                        &root.join(".claude/settings.local.json"),
                        "Project · local",
                        &mut rows,
                    );
                }
                ProviderKind::Cursor => {
                    hook_rows(&root.join(".cursor/hooks.json"), "Project", &mut rows)
                }
                ProviderKind::Codex => {}
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
    if let Some(home) = &app.home_path {
        if hooks {
            match provider {
                ProviderKind::Claude => hook_rows(&home.join("settings.json"), "User · selected account", &mut rows),
                ProviderKind::Cursor => hook_rows(&home.join("hooks.json"), "User · selected account", &mut rows),
                ProviderKind::Codex => rows.push(row("notice", "Codex native hooks", "No verified native hook catalog is exposed here. Concordia does not install or emulate hooks.")),
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
                    "System · selected installation",
                    &mut rows,
                    &mut visited,
                );
            }
        }
    }
    if !hooks && provider == ProviderKind::Codex {
        if let Some(home) = Some(crate::test_guard::test_home()) {
            skill_rows(
                &home.join(".agents/skills"),
                "User · shared",
                &mut rows,
                &mut visited,
            );
        }
    }
    rows.push(row("notice", "Local source inventory", "Enter opens a source; nothing is executed. Configured does not prove enabled in a running turn. Plugin/managed/remote sources are not fully enumerated. Reopen to refresh; no provider call."));
    app.show_picker(
        PickerKind::Customization,
        format!(
            "{} · {} · local sources",
            if hooks { "Hooks" } else { "Skills" },
            provider.as_str()
        ),
        rows,
        true,
    );
}
