//! Slash-command suggestions shown while typing `/...` in the prompt.
//!
//! Pure logic (no terminal I/O): which commands match the text so far, and what selecting one
//! should put in the prompt. Drawing and key handling live in `tui/mod.rs`.

use crate::commands::command_registry;
use crate::store::ProviderKind;
use crate::user_commands::UserCommand;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub command: String,
    pub help: String,
    /// A user command from `<state>/commands` (drawn with a dim "custom" marker).
    pub custom: bool,
    /// Provider-native commands are discoverable but unavailable in print/exec mode.
    pub provider: bool,
    pub disabled: bool,
}

/// Commands whose full text starts with what the user has typed. Only a single-line prompt that
/// begins with `/` gets suggestions; everything else is ordinary chat text.
#[cfg(test)]
pub fn matching(input: &str) -> Vec<Entry> {
    matching_with(input, &[])
}

/// Like [`matching`], with the user's commands listed after the built-ins.
pub fn matching_with(input: &str, user: &[UserCommand]) -> Vec<Entry> {
    matching_for_provider(input, user, None)
}

pub fn matching_for_provider(
    input: &str,
    user: &[UserCommand],
    provider: Option<ProviderKind>,
) -> Vec<Entry> {
    if !input.starts_with('/') || input.contains('\n') {
        return Vec::new();
    }
    let typed = input.to_ascii_lowercase();
    let builtin = command_registry()
        .iter()
        .filter(|c| c.name.starts_with(&typed))
        .filter(|c| typed != "/" || !c.name.is_empty())
        .map(|c| Entry {
            command: c.name.to_string(),
            help: c.description.to_string(),
            custom: false,
            provider: false,
            disabled: false,
        });
    let provider_commands = provider
        .into_iter()
        .flat_map(crate::provider::command_specs)
        .filter(|c| c.name.starts_with(&typed))
        .map(|c| Entry {
            command: c.name.to_string(),
            help: format!("{} (unavailable in Concordia's provider mode)", c.description),
            custom: false,
            provider: true,
            disabled: true,
        });
    let custom = user
        .iter()
        .filter(|c| c.command.starts_with(&typed))
        .map(|c| Entry {
            command: c.command.to_string(),
            help: c.description.to_string(),
            custom: true,
            provider: false,
            disabled: false,
        });
    let mut entries: Vec<_> = builtin.chain(provider_commands).chain(custom).collect();
    if let Some(index) = entries.iter().position(|entry| entry.command == input) {
        let exact = entries.remove(index);
        entries.insert(0, exact);
    }
    entries
}

#[derive(Debug, Default)]
pub struct Palette {
    pub selected: usize,
    /// Closed with Esc; stays closed until the text changes.
    pub dismissed: bool,
}

impl Palette {
    pub fn reset(&mut self) {
        self.selected = 0;
        self.dismissed = false;
    }

    pub fn clamp(&mut self, len: usize) {
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    pub fn up(&mut self, len: usize) {
        if len > 0 {
            self.selected = (self.selected + len - 1) % len;
        }
    }

    pub fn down(&mut self, len: usize) {
        if len > 0 {
            self.selected = (self.selected + 1) % len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds(input: &str) -> Vec<String> {
        matching(input).into_iter().map(|e| e.command).collect()
    }

    #[test]
    fn a_bare_slash_lists_every_command() {
        let all = cmds("/");
        assert_eq!(all.len(), command_registry().len());
        assert_eq!(
            all,
            command_registry()
                .iter()
                .map(|c| c.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn typing_narrows_by_prefix_case_insensitively() {
        assert_eq!(cmds("/up"), ["/update"]);
        assert_eq!(cmds("/UP"), ["/update"]);
        assert_eq!(cmds("/use a"), Vec::<&str>::new());
        assert_eq!(cmds("/zzz"), Vec::<&str>::new());
    }

    #[test]
    fn only_a_single_line_slash_prompt_gets_suggestions() {
        assert!(cmds("hello /use").is_empty());
        assert!(cmds("").is_empty());
        assert!(cmds("/use\nsecond line").is_empty());
    }

    #[test]
    fn every_command_has_a_description_and_is_unique() {
        let mut seen = std::collections::HashSet::new();
        for command in command_registry() {
            assert!(command.name.starts_with('/'), "{}", command.name);
            assert!(
                !command.description.trim().is_empty(),
                "{} needs a description",
                command.name
            );
            assert!(seen.insert(command.name), "duplicate {}", command.name);
        }
    }

    #[test]
    fn user_commands_are_listed_after_built_ins_and_marked_custom() {
        let dir = tempfile::tempdir().unwrap();
        let cmds = crate::user_commands::commands_dir(dir.path());
        std::fs::create_dir_all(&cmds).unwrap();
        std::fs::write(
            cmds.join("ship.md"),
            "---
description: ship it
---
go",
        )
        .unwrap();
        let user = crate::user_commands::scan(dir.path()).commands;
        let all = matching_with("/", &user);
        assert_eq!(all.len(), command_registry().len() + 1);
        let last = all.last().unwrap();
        assert_eq!(
            (last.command.as_str(), last.help.as_str(), last.custom),
            ("/ship", "ship it", true)
        );
        assert!(all[..all.len() - 1].iter().all(|e| !e.custom));
        assert_eq!(matching_with("/sh", &user).len(), 1);
        assert!(matching_with("/sh", &[]).is_empty());
    }

    #[test]
    fn active_provider_commands_are_listed_and_disabled_with_transport_reason() {
        let entries = matching_for_provider("/", &[], Some(ProviderKind::Cursor));
        let entry = entries.iter().find(|entry| entry.command == "/auto-run").unwrap();
        assert!(entry.provider);
        assert!(entry.disabled);
        assert!(entry.help.contains("unavailable"));
    }

    #[test]
    fn selection_wraps_and_clamps() {
        let mut p = Palette::default();
        p.up(3);
        assert_eq!(p.selected, 2);
        p.down(3);
        assert_eq!(p.selected, 0);
        p.selected = 9;
        p.clamp(4);
        assert_eq!(p.selected, 3);
        p.clamp(0);
        assert_eq!(p.selected, 0);
        p.dismissed = true;
        p.reset();
        assert!(!p.dismissed);
    }
}

/// Branches reveal their children once the complete command has been entered.
pub fn is_branch(input: &str) -> bool {
    matches!(
        input.to_ascii_lowercase().as_str(),
        "/use"
            | "/use add"
            | "/use remove"
            | "/use rename"
            | "/model"
            | "/mode"
            | "/update"
            | "/workers"
            | "/workers add"
            | "/workers remove"
            | "/delegate"
            | "/delegate protocol"
            | "/delegate model"
            | "/delegate returns"
            | "/harness"
            | "/skills"
            | "/hooks"
    )
}

pub fn has_argument_menu(input: &str) -> bool {
    let root = input
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        root.as_str(),
        "/use"
            | "/model"
            | "/mode"
            | "/update"
            | "/workers"
            | "/delegate"
            | "/harness"
            | "/skills"
            | "/hooks"
            | "/usage"
    )
}

/// Dynamic choices come only from connected accounts and locally cached models.
pub fn matching_arguments(
    input: &str,
    accounts: &[(String, String)],
    models: &[String],
) -> Vec<Entry> {
    if input.contains('\n') {
        return Vec::new();
    }
    let typed = input.to_ascii_lowercase();
    let mut entries = Vec::new();
    let mut add = |command: String, help: String| {
        if command.to_ascii_lowercase().starts_with(&typed) {
            entries.push(Entry {
                command,
                help,
                custom: false,
                provider: false,
                disabled: false,
            });
        }
    };
    match input
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "/use" => {
            add(
                "/use".into(),
                "ACCOUNT | add PROVIDER | remove ACCOUNT - Enter opens accounts".into(),
            );
            add(
                "/use add".into(),
                "connect a provider account - Enter opens provider choices".into(),
            );
            add(
                "/use remove".into(),
                "remove an account - Enter opens removal choices".into(),
            );
            add(
                "/use rename".into(),
                "ACCOUNT NICKNAME - rename for display; omit nickname to reset".into(),
            );
            for provider in ["codex", "claude", "cursor", "grok"]
                .into_iter()
                .filter(|_| typed.starts_with("/use add"))
            {
                add(
                    format!("/use add {provider}"),
                    "connect with the provider's own login".into(),
                );
            }
            for (id, label) in accounts {
                if !typed.starts_with("/use add")
                    && !typed.starts_with("/use remove")
                    && !typed.starts_with("/use rename")
                {
                    add(format!("/use {id}"), format!("switch to {label}"));
                }
                if typed.starts_with("/use rename") {
                    add(
                        format!("/use rename {id}"),
                        format!("type a nickname for {label}; Enter resets the name"),
                    );
                }
                if typed.starts_with("/use remove") {
                    add(
                        format!("/use remove {id}"),
                        format!("open removal options for {label}"),
                    );
                }
            }
        }
        "/model" => {
            add(
                "/model".into(),
                "MODEL - choose from this account's catalog; Enter opens models".into(),
            );
            for model in models {
                add(
                    format!("/model {model}"),
                    "use this model for the active account".into(),
                );
            }
        }
        "/mode" => {
            add(
                "/mode".into(),
                "plan | ask | edit | full - Enter shows the permission mode".into(),
            );
            for (mode, help) in [
                ("plan", "read-only analysis"),
                ("ask", "use provider approvals"),
                ("edit", "allow workspace edits"),
                ("full", "full access using provider-supported controls"),
            ] {
                add(format!("/mode {mode}"), help.into());
            }
        }
        "/update" => {
            add("/update".into(), "Enter opens the update preview".into());
        }
        "/workers" => {
            add(
                "/workers".into(),
                "list | add ACCOUNT | remove ACCOUNT".into(),
            );
            add(
                "/workers add".into(),
                "choose an account to use as a worker".into(),
            );
            add(
                "/workers remove".into(),
                "choose a worker account to remove".into(),
            );
            for (id, label) in accounts {
                if typed.starts_with("/workers add") {
                    add(
                        format!("/workers add {id}"),
                        format!("add {label} as a worker"),
                    );
                }
                if typed.starts_with("/workers remove") {
                    add(
                        format!("/workers remove {id}"),
                        format!("remove {label} from workers"),
                    );
                }
            }
        }
        "/delegate" => {
            add(
                "/delegate".into(),
                "ACCOUNT TASK | auto TASK | use | stop | protocol".into(),
            );
            for (suffix, help) in [
                ("use", "include a completed worker result"),
                ("returns", "view automatic result handback settings"),
                (
                    "returns auto",
                    "return finished worker batches when the main agent is idle",
                ),
                ("returns manual", "send results only on request"),
                (
                    "collect",
                    "send up to three new results to the main agent together",
                ),
                (
                    "model",
                    "ACCOUNT [MODEL|default] - configure a worker model",
                ),
                ("stop", "stop worker tasks"),
                ("help", "show delegation syntax and limits"),
                ("protocol", "view proposal confirmation settings"),
                ("protocol on", "allow proposals that require confirmation"),
                ("protocol off", "disable delegation proposals"),
            ] {
                if !suffix.starts_with("protocol ") || typed.starts_with("/delegate protocol") {
                    add(format!("/delegate {suffix}"), help.into());
                }
            }
            if typed.starts_with("/delegate model") {
                for (id, label) in accounts {
                    add(
                        format!("/delegate model {id}"),
                        format!("show models for {label}; register it as a worker first"),
                    );
                }
            }
        }
        cmd @ ("/skills" | "/hooks") => {
            add(
                cmd.into(),
                "Enter inspects this account's sources; connect applies shared setup".into(),
            );
            add(
                format!("{cmd} list"),
                "inspect this account's local sources".into(),
            );
            add(
                format!("{cmd} connect"),
                "PATH [--current] - connect setup across accounts".into(),
            );
            add(
                format!("{cmd} status"),
                "show Harness setup and verification results".into(),
            );
            if cmd == "/skills" {
                add(
                    "/skills remove".into(),
                    "PATH - stop sharing and remove matching links across accounts".into(),
                );
                add(
                    format!("{cmd} install"),
                    "[PATH] [--current] - share skill folders across accounts".into(),
                );
            } else {
                add(
                    format!("{cmd} verify"),
                    "[NAME] [--current] - run verification across accounts".into(),
                );
            }
        }
        "/harness" => {
            add(
                "/harness".into(),
                "connect | status | tools | verify | disconnect | skill".into(),
            );
            for (suffix, help) in [
                (
                    "connect",
                    "PATH [--current] - connect setup across accounts",
                ),
                ("status", "show connected harnesses and verification state"),
                (
                    "tools",
                    "NAME or harness/tool - list or run a declared tool",
                ),
                (
                    "verify",
                    "[NAME] [--current] - verify all accounts; may use provider usage",
                ),
                ("disconnect", "NAME - remove a harness connection"),
                (
                    "skill",
                    "[--current] - install the connection skill across accounts",
                ),
            ] {
                add(format!("/harness {suffix}"), help.into());
            }
        }
        "/usage" => {
            add(
                "/usage".into(),
                "Enter opens limits; r refreshes and t shows recorded token usage".into(),
            );
        }
        _ => {}
    }
    if let Some(index) = entries
        .iter()
        .position(|e| e.command.eq_ignore_ascii_case(input))
    {
        let exact = entries.remove(index);
        entries.insert(0, exact);
    }
    entries
}

#[cfg(test)]
mod argument_tests {
    use super::*;
    #[test]
    fn account_provider_and_model_arguments_complete_by_prefix() {
        let accounts = [("codex-2".into(), "Codex 2".into())];
        assert_eq!(
            matching_arguments("/use co", &accounts, &[])[0].command,
            "/use codex-2"
        );
        assert_eq!(
            matching_arguments("/use add cu", &accounts, &[])[0].command,
            "/use add cursor"
        );
        assert_eq!(
            matching_arguments("/use remove co", &accounts, &[])[0].command,
            "/use remove codex-2"
        );
        assert_eq!(
            matching_arguments("/model fi", &[], &["fixture-model".into()])[0].command,
            "/model fixture-model"
        );
        assert!(
            matching_arguments("/model unavailable", &[], &["fixture-model".into()]).is_empty()
        );
    }
}

#[cfg(test)]
mod discovery_tests {
    use super::*;
    #[test]
    fn only_the_completed_branch_reveals_its_immediate_children() {
        let accounts = [("codex-2".into(), "Codex 2".into())];
        let root = matching_arguments("/use", &accounts, &[]);
        assert_eq!(root[0].command, "/use");
        assert!(root.iter().any(|e| e.command == "/use add"));
        assert!(!root.iter().any(|e| e.command == "/use add codex"));
        let add = matching_arguments("/use add", &accounts, &[]);
        assert_eq!(add[0].command, "/use add");
        assert!(add.iter().any(|e| e.command == "/use add codex"));
        assert!(!add.iter().any(|e| e.command == "/use codex-2"));
    }

    #[test]
    fn advertised_children_use_supported_command_syntax() {
        for root in [
            "/use",
            "/model",
            "/mode",
            "/update",
            "/harness",
            "/skills",
            "/hooks",
            "/usage",
        ] {
            let entries = matching_arguments(root, &[], &[]);
            assert_eq!(entries[0].command, root);
            for entry in entries {
                let (parsed, _) = crate::commands::parse_slash(&entry.command).unwrap();
                assert!(
                    !matches!(
                        parsed,
                        crate::commands::SlashCommand::Invalid(_)
                            | crate::commands::SlashCommand::Unknown(_)
                    ),
                    "{}",
                    entry.command
                );
            }
        }
    }

    #[test]
    fn update_has_no_redundant_check_child() {
        let entries = matching_arguments("/update", &[], &[]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "/update");
        assert!(matching_arguments("/update check", &[], &[]).is_empty());
    }
}
