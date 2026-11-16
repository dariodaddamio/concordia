//! User settings and local provider customization inventory. Never executes discovered content.
use super::*;

fn row(id: &str, label: impl Into<String>, detail: impl Into<String>) -> PickerRow {
    PickerRow {
        id: id.into(),
        label: label.into(),
        detail: detail.into(),
    }
}

pub(super) const FOOTER: &str =
    "←/→ or Tab tabs · ↑/↓ rows · Enter or Space change · type to filter · Esc close";

const TABS: [&str; 6] = [
    "General",
    "Appearance",
    "Models and roles",
    "Accounts",
    "Updates",
    "Advanced",
];

fn setting(
    tab: usize,
    id: &str,
    label: &str,
    value: String,
    description: &str,
) -> (usize, SettingsRow) {
    (
        tab,
        SettingsRow {
            action: id.into(),
            label: label.into(),
            value,
            description: description.into(),
        },
    )
}

/// Every setting with its current value. Reads saved state, so call it on open and after a
/// change, never per frame.
fn settings_rows(app: &App) -> Vec<(usize, SettingsRow)> {
    let ui = &app.appearance.ui;
    let saved = crate::store::get_settings(&app.state_root);
    let config = crate::config::for_root(&app.state_root);
    let on_off = |on: bool| String::from(if on { "On" } else { "Off" });
    let opens = || String::from("Open ›");
    let fallbacks = config.continuation.fallback_models;
    let fallback_value = if fallbacks.is_empty() {
        String::from("Not set ›")
    } else {
        let mut text = fallbacks
            .into_iter()
            .map(|(role, model)| format!("{role} → {model}"))
            .collect::<Vec<_>>()
            .join(", ");
        text.push_str(" ›");
        text
    };
    let file_links = match saved.file_action {
        Some(crate::file_open::FileAction::Editor) => "External editor",
        _ => "Preview here",
    };
    let same_provider = crate::store::same_provider_automatic(&app.state_root)
        && crate::store::same_provider_terms_accepted(&app.state_root, app.provider);
    vec![
        setting(
            0,
            "permissions",
            "Permissions",
            format!("{} ›", app.trust.as_str()),
            "Default for future turns; a running turn keeps the permissions it started with",
        ),
        setting(
            0,
            "tokens",
            "Response usage line",
            on_off(ui.token_line),
            "Show provider-reported tokens under each reply",
        ),
        setting(
            0,
            "tips",
            "Idle tips",
            on_off(ui.tips),
            "Show a tip while nothing is running",
        ),
        setting(
            0,
            "stats",
            "CPU and memory",
            on_off(app.stats.is_some()),
            "Show this window's processor and memory use in the header",
        ),
        setting(
            0,
            "notifications",
            "Windows notifications",
            on_off(ui.notifications),
            "The taskbar status stays active when notifications are off",
        ),
        setting(
            0,
            "returns",
            "Worker result delivery",
            String::from(if saved.delegate_auto_return {
                "Automatic"
            } else {
                "Manual"
            }),
            "Automatic delivery may start a main-agent turn when the chat is idle",
        ),
        setting(
            1,
            "appearance",
            "Theme and colors",
            opens(),
            "Theme and window icon color for every window",
        ),
        setting(
            1,
            "motion",
            "Interface animation",
            on_off(ui.motion),
            "The Windows reduced-motion preference takes precedence",
        ),
        setting(
            1,
            "files",
            "File links open in",
            file_links.into(),
            "Where a clicked file link opens",
        ),
        setting(
            2,
            "models",
            "Model",
            format!("{} ›", app.model_label()),
            "Model for the current provider account",
        ),
        setting(
            2,
            "roles",
            "Roles",
            format!("{} configured ›", saved.roles.len()),
            "Provider, account, model and effort for delegated agents",
        ),
        setting(
            2,
            "fallback",
            "Fallback model per role",
            fallback_value,
            "First model offered after a provider usage limit; otherwise Concordia asks",
        ),
        setting(
            3,
            "accounts",
            "Account",
            format!("{} · {} ›", app.provider.as_str(), app.account_label()),
            "Provider and account for this chat; native sign-ins stay separate",
        ),
        setting(
            3,
            "continuation",
            "When a limit is reached",
            String::from(if app.continuation.automatic() {
                "Continue automatically"
            } else {
                "Pause and ask"
            }),
            "Applies to future sessions; proactive rollover is Codex-only and off",
        ),
        setting(
            3,
            "same-provider",
            "Continue on another account of the same provider",
            on_off(same_provider),
            "Turning this on confirms the provider's terms allow several accounts",
        ),
        setting(
            3,
            "session-handoff",
            "This session's handoff",
            opens(),
            "Session only: choose the next provider, or automatic or manual behavior",
        ),
        setting(
            4,
            "updates",
            "New versions",
            String::from(if config.updates.install_on_launch {
                "Install at next launch"
            } else {
                "Install in the background"
            }),
            "A background install loads by itself as soon as no work is running",
        ),
        setting(
            5,
            "native-controls",
            "Native provider controls",
            opens(),
            "Open the provider's own CLI for this account in a separate terminal",
        ),
        setting(
            5,
            "skills",
            "Skills",
            opens(),
            "Inspect project and account skill sources; nothing is run",
        ),
        setting(
            5,
            "hooks",
            "Hooks",
            opens(),
            "Inspect project and account hook sources; hooks run in their provider",
        ),
        setting(
            5,
            "integrations",
            "Integrations",
            opens(),
            "Inspect and manage connected tools",
        ),
        setting(
            5,
            "advanced",
            "config.toml",
            opens(),
            "Open the defaults file; manual edits apply after a restart",
        ),
    ]
}

/// Opens the settings screen, or rebuilds it in place after a change, keeping the tab, the
/// selected row of every tab and the filter.
pub(super) fn open(app: &mut App) {
    let previous = app.settings_view.take();
    let mut tabs: Vec<SettingsTab> = TABS
        .iter()
        .map(|name| SettingsTab {
            name: (*name).into(),
            rows: Vec::new(),
        })
        .collect();
    for (tab, row) in settings_rows(app) {
        tabs[tab].rows.push(row);
    }
    let mut view = SettingsView {
        selected: vec![0; tabs.len()],
        tabs,
        active: 0,
        filter: String::new(),
        filter_selected: 0,
        stale: false,
        tab_areas: Vec::new(),
        row_areas: Vec::new(),
    };
    if let Some(previous) = previous {
        view.active = previous.active.min(view.tabs.len() - 1);
        for (tab, selected) in previous.selected.iter().enumerate().take(view.tabs.len()) {
            view.selected[tab] = (*selected).min(view.tabs[tab].rows.len().saturating_sub(1));
        }
        view.filter = previous.filter;
        view.filter_selected = previous.filter_selected;
    }
    app.settings_view = Some(view);
    app.picker = None;
}

/// Rebuilds the rows once a popup opened from the screen has closed.
pub(super) fn refresh_if_stale(app: &mut App) {
    if app.picker.is_none() && app.settings_view.as_ref().is_some_and(|view| view.stale) {
        open(app);
    }
}

/// The rows on screen as (tab, row): the active tab, or every match while a filter is typed.
pub(super) fn visible(view: &SettingsView) -> Vec<(usize, usize)> {
    if view.filter.is_empty() {
        return (0..view.tabs[view.active].rows.len())
            .map(|row| (view.active, row))
            .collect();
    }
    let query = view.filter.to_lowercase();
    let mut out = Vec::new();
    for (tab, group) in view.tabs.iter().enumerate() {
        for (row, item) in group.rows.iter().enumerate() {
            if [&group.name, &item.label, &item.value, &item.description]
                .iter()
                .any(|text| text.to_lowercase().contains(&query))
            {
                out.push((tab, row));
            }
        }
    }
    out
}

pub(super) fn selected_position(view: &SettingsView) -> usize {
    if view.filter.is_empty() {
        view.selected[view.active]
    } else {
        view.filter_selected
    }
}

fn select_position(view: &mut SettingsView, position: usize) {
    if view.filter.is_empty() {
        view.selected[view.active] = position;
    } else {
        view.filter_selected = position;
    }
}

fn switch_tab(view: &mut SettingsView, forward: bool) {
    let count = view.tabs.len();
    view.filter.clear();
    view.filter_selected = 0;
    view.active = if forward {
        (view.active + 1) % count
    } else {
        (view.active + count - 1) % count
    };
}

/// Runs the selected row: a switch flips in place, anything larger opens its popup over the
/// screen, and a file or terminal leaves the screen.
fn act(app: &mut App) {
    let Some(view) = app.settings_view.as_ref() else {
        return;
    };
    let Some(&(tab, row)) = visible(view).get(selected_position(view)) else {
        return;
    };
    let action = view.tabs[tab].rows[row].action.clone();
    match action.as_str() {
        "files" => {
            let next = match crate::store::get_settings(&app.state_root).file_action {
                Some(crate::file_open::FileAction::Editor) => crate::file_open::FileAction::Preview,
                _ => crate::file_open::FileAction::Editor,
            };
            if let Err(error) = crate::store::set_file_action(&app.state_root, next) {
                app.flash(format!("Not saved: {error}"));
            }
            open(app);
        }
        "advanced" | "native-controls" => {
            app.settings_view = None;
            app.footer_left = FOOTER_HINT.into();
            choose(app, PickerKind::SettingsView, &action);
        }
        _ => {
            choose(app, PickerKind::SettingsView, &action);
            let popup = app.picker.is_some();
            if let Some(view) = app.settings_view.as_mut() {
                view.stale = popup;
            }
        }
    }
}

pub(super) fn key(app: &mut App, key: KeyEvent) {
    let Some(view) = app.settings_view.as_mut() else {
        return;
    };
    let count = visible(view).len();
    let position = selected_position(view).min(count.saturating_sub(1));
    match key.code {
        KeyCode::Esc if !view.filter.is_empty() => {
            view.filter.clear();
            view.filter_selected = 0;
        }
        KeyCode::Esc => {
            app.settings_view = None;
            app.footer_left = FOOTER_HINT.into();
        }
        KeyCode::Right => switch_tab(view, true),
        KeyCode::Left | KeyCode::BackTab => switch_tab(view, false),
        KeyCode::Tab => switch_tab(view, !key.modifiers.contains(KeyModifiers::SHIFT)),
        KeyCode::Up if count > 0 => select_position(
            view,
            if position == 0 {
                count - 1
            } else {
                position - 1
            },
        ),
        KeyCode::Down if count > 0 => select_position(view, (position + 1) % count),
        KeyCode::Home => select_position(view, 0),
        KeyCode::End => select_position(view, count.saturating_sub(1)),
        KeyCode::Enter | KeyCode::Char(' ') => act(app),
        KeyCode::Backspace => {
            view.filter.pop();
            view.filter_selected = 0;
        }
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            view.filter.push(c);
            view.filter_selected = 0;
        }
        _ => {}
    }
}

/// A click selects a tab or a row; a click on the row that is already selected runs it.
pub(super) fn mouse(app: &mut App, event: MouseEvent) {
    let Some(view) = app.settings_view.as_mut() else {
        return;
    };
    let count = visible(view).len();
    match event.kind {
        MouseEventKind::ScrollUp if count > 0 => {
            let position = selected_position(view);
            select_position(view, position.saturating_sub(1));
        }
        MouseEventKind::ScrollDown if count > 0 => {
            let position = selected_position(view);
            select_position(view, (position + 1).min(count - 1));
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let at = Position::new(event.column, event.row);
            if let Some((_, tab)) = view.tab_areas.iter().find(|(area, _)| area.contains(at)) {
                view.filter.clear();
                view.filter_selected = 0;
                view.active = *tab;
            } else if let Some(&(_, position)) =
                view.row_areas.iter().find(|(area, _)| area.contains(at))
            {
                if position == selected_position(view) {
                    act(app);
                } else {
                    select_position(view, position);
                }
            }
        }
        _ => {}
    }
}

pub(super) fn choose(app: &mut App, kind: PickerKind, id: &str) {
    if kind == PickerKind::Customization {
        if let Some(path) = id.strip_prefix("file:") {
            app.settings_view = None;
            app.file_command(path, false, true);
        }
        return;
    }
    if kind == PickerKind::FilePreferences {
        let action = match id {
            "preview" => crate::file_open::FileAction::Preview,
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
        "native-controls" => local_terminal::native_controls(app),
        "tokens" | "tips" | "stats" | "motion" | "notifications" => {
            let current = match id {
                "tokens" => app.appearance.ui.token_line,
                "tips" => app.appearance.ui.tips,
                "motion" => app.appearance.ui.motion,
                "notifications" => app.appearance.ui.notifications,
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
        "roles" => {
            let rows = crate::store::get_settings(&app.state_root)
                .roles
                .into_iter()
                .map(|(name, role)| {
                    row(&format!("role:{name}"), name, {
                        let effective = |own: Option<&str>, inherited: &str| match own {
                            Some(value) => value.to_string(),
                            None if inherited.is_empty() || inherited == "?" => "not set".into(),
                            None => format!("{inherited} (inherited)"),
                        };
                        format!(
                            "{} · {} · {} · effort {}",
                            effective(role.provider.as_deref(), app.provider.as_str()),
                            effective(role.account.as_deref(), &app.account),
                            effective(role.model.as_deref(), &app.model),
                            effective(role.effort.as_deref(), app.effort.as_deref().unwrap_or("")),
                        )
                    })
                })
                .collect();
            app.show_picker(PickerKind::SettingsView, "Roles".into(), rows, true);
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
        "updates" => {
            let current = crate::config::for_root(&app.state_root)
                .updates
                .install_on_launch;
            match crate::store::set_update_install_on_launch(&app.state_root, !current) {
                Ok(()) => app.flash(if current {
                    "Updates will prepare in the background"
                } else {
                    "Updates will install on the next launch"
                }),
                Err(error) => app.flash(format!("Not saved: {error}")),
            }
            open(app);
        }
        "fallback" => {
            let mut roles = crate::store::get_settings(&app.state_root)
                .roles
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            roles.push("general".into());
            roles.sort();
            roles.dedup();
            let configured = crate::config::for_root(&app.state_root)
                .continuation
                .fallback_models;
            app.show_picker(
                PickerKind::SettingsView,
                "Fallback model · choose a role".into(),
                roles
                    .into_iter()
                    .map(|role| {
                        row(
                            format!("fallback-role:{role}").as_str(),
                            role.clone(),
                            configured
                                .get(&role)
                                .map_or("not configured".into(), |model| {
                                    format!("fallback model · {model}")
                                }),
                        )
                    })
                    .collect(),
                true,
            );
        }
        "same-provider" => {
            let root = app.state_root.clone();
            let on = crate::store::same_provider_automatic(&root)
                && crate::store::same_provider_terms_accepted(&root, app.provider);
            let saved = if on {
                crate::store::withdraw_same_provider_terms(&root, app.provider)
            } else {
                crate::store::accept_same_provider_terms(&root, app.provider).and_then(|()| {
                    if app.continuation.automatic() {
                        Ok(())
                    } else {
                        crate::store::set_continuation_automatic(&root, true).map(|()| {
                            app.continuation.toggle_automatic();
                        })
                    }
                })
            };
            match saved {
                Ok(()) if on => app.flash("Same-provider handoff off"),
                Ok(()) => app.flash("Same-provider handoff on; limit handoff is automatic"),
                Err(error) => app.flash(format!("Not saved: {error}")),
            }
            open(app);
        }
        "session-handoff" => app.open_handoff_settings(),
        "returns" => {
            let automatic = crate::store::get_settings(&app.state_root).delegate_auto_return;
            match crate::store::set_delegate_auto_return(&app.state_root, !automatic) {
                Ok(()) => app.flash(if automatic {
                    "Worker results return manually"
                } else {
                    "Worker results return automatically"
                }),
                Err(error) => app.flash(format!("Not saved: {error}")),
            }
            open(app);
        }
        "files" => app.show_picker(
            PickerKind::FilePreferences,
            "File links · saved default".into(),
            vec![
                row("preview", "Preview here", "Open inside the current window"),
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
        id if id.starts_with("fallback-role:") => {
            let role = id.trim_start_matches("fallback-role:").to_owned();
            let rows = model_picker_rows(
                crate::models::available_at(&app.state_root, app.provider, &app.account),
                None,
            );
            app.fallback_role = Some(role.clone());
            app.show_picker(
                PickerKind::FallbackModel,
                format!("Fallback model · {role}"),
                rows,
                true,
            );
        }
        id if id.starts_with("role:") => {
            if app.busy {
                app.flash("Finish or cancel the current turn before changing role model");
            } else {
                app.open_role_model_picker(id.trim_start_matches("role:").to_owned());
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

/// Read a skill front-matter `description`, including YAML folded (`>`) and literal (`|`) blocks.
fn skill_description(text: &str) -> String {
    let lines: Vec<&str> = text.lines().take(80).collect();
    let Some((index, rest)) = lines.iter().enumerate().find_map(|(index, line)| {
        line.strip_prefix("description:")
            .map(|rest| (index, rest.trim()))
    }) else {
        return "Local skill definition".into();
    };
    let inline = rest.trim_matches(['\'', '"']).trim();
    let block = match inline {
        ">" | ">-" | ">+" => Some(false),
        "|" | "|-" | "|+" => Some(true),
        _ if inline.is_empty() => {
            // Bare `description:` then an indented block on the next lines.
            let next = lines.get(index + 1).copied().unwrap_or("");
            if next.starts_with(' ') || next.starts_with('\t') {
                Some(false)
            } else {
                None
            }
        }
        _ => None,
    };
    let Some(literal) = block else {
        return if inline.is_empty() {
            "Local skill definition".into()
        } else {
            inline.to_string()
        };
    };
    let mut body = Vec::new();
    for line in lines.iter().skip(index + 1) {
        if line.is_empty() {
            if body.is_empty() {
                continue;
            }
            body.push(String::new());
            continue;
        }
        if !(line.starts_with(' ') || line.starts_with('\t')) {
            break;
        }
        body.push(line.trim().to_string());
    }
    let joined = if literal {
        body.join("\n")
    } else {
        body.into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    };
    if joined.is_empty() {
        "Local skill definition".into()
    } else {
        joined
    }
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
        if !entry.path().is_dir() {
            continue;
        }
        let path = entry.path().join("SKILL.md");
        if !path.exists() {
            continue;
        }
        let folder = entry.file_name().to_string_lossy().into_owned();
        let native_source = std::fs::canonicalize(entry.path())
            .map(|folder| folder.join("SKILL.md"))
            .unwrap_or_else(|_| path.clone());
        match bounded_text(&native_source) {
            Ok(text) => {
                let description = skill_description(&text);
                rows.push(source_row(
                    &native_source,
                    scope,
                    &folder,
                    &crate::sessions::clean_preview(Some(&description), 120),
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
        ProviderKind::Grok => ".grok",
        ProviderKind::Opencode => ".opencode",
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
                ProviderKind::Codex => {
                    hook_rows(&root.join(".codex/hooks.json"), "Project", &mut rows)
                }
                ProviderKind::Grok | ProviderKind::Opencode => {}
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
                ProviderKind::Claude => hook_rows(
                    &home.join("settings.json"),
                    "User · selected account",
                    &mut rows,
                ),
                ProviderKind::Cursor => hook_rows(
                    &home.join("hooks.json"),
                    "User · selected account",
                    &mut rows,
                ),
                ProviderKind::Codex => {
                    hook_rows(
                        &home.join("hooks.json"),
                        "User · selected account",
                        &mut rows,
                    );
                    rows.push(row("notice", "Native hook trust", "This inventory shows configured sources. Review native /hooks in this provider home; only native consent can enable untrusted hooks."));
                }
                ProviderKind::Grok | ProviderKind::Opencode => hook_rows(
                    &home.join("hooks.json"),
                    "User · selected account",
                    &mut rows,
                ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_provider_handoff_toggles_consent_and_turns_on_automatic_handoff() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().into(), dir.path().into());
        let root = dir.path();
        assert!(!crate::store::same_provider_terms_accepted(
            root,
            app.provider
        ));
        choose(&mut app, PickerKind::SettingsView, "same-provider");
        assert!(crate::store::same_provider_automatic(root));
        assert!(crate::store::same_provider_terms_accepted(
            root,
            app.provider
        ));
        assert!(app.continuation.automatic());
        choose(&mut app, PickerKind::SettingsView, "same-provider");
        assert!(!crate::store::same_provider_terms_accepted(
            root,
            app.provider
        ));
        assert!(!crate::store::same_provider_automatic(root));
    }
}
