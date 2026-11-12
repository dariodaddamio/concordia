use crate::accounts::display_account_name;
use crate::store::{ProviderKind, TrustMode};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand {
    Use,
    UseAccount(String),
    /// `/use add [provider]`; `None` opens the provider picker.
    UseAdd(Option<String>),
    /// `/use remove [account]`; `None` opens the account picker.
    UseRemove(Option<String>),
    Model,
    ModelSet(String),
    /// `/usage`: limits for the current account.
    Usage,
    /// `/usage all`: limits for every ready account.
    UsageAll,
    UsageRefresh,
    /// `/help` or `/help --all` shows registry-backed commands.
    Help(bool),
    Resume,
    New,
    Config,
    /// `/mode [plan|ask|auto|full]`: show or set the permission mode.
    Mode(Option<String>),
    Update,
    Version,
    Copy,
    Doctor,
    Exit,
    /// `/workers`: list. Delegation commands run inside the TUI (`tui/workers.rs`).
    Workers,
    WorkersAdd(Option<String>),
    WorkersRemove(Option<String>),
    /// `/delegate <account|auto> <task...>` (the task may be empty: a usage error).
    Delegate {
        target: String,
        task: String,
    },
    DelegateAfter {
        after: Vec<u32>,
        target: String,
        task: String,
    },
    DelegateGraph,
    DelegateDelivery(String),
    DelegateRoutes(Option<u32>),
    DelegatePolicy(String),
    DelegateScope(String),
    DelegateWorktree {
        workspace: String,
        account: String,
        task: String,
    },
    DelegateScoped {
        account: String,
        declarations: Vec<crate::workspaces::Declaration>,
        task: String,
    },
    DelegateStop(Option<String>),
    DelegateUse(Option<String>),
    DelegateCollect,
    DelegateReturns(Option<bool>),
    DelegateModel {
        account: String,
        model: Option<String>,
    },
    DelegateHelp,
    /// `/delegate protocol [on|off]`.
    DelegateProtocol(Option<bool>),
    /// `/crew list|use NAME|clear` selects a saved local model arrangement.
    CrewList,
    CrewUse(String),
    CrewClear,
    /// `/harness ...` is executed by the TUI because it needs the live account home.
    Harness(String),
    Customization(bool),
    /// A known command received an unsupported argument. Keeping this distinct prevents
    /// misspelled destructive commands such as `/update typo` from running.
    Invalid(String),
    Unknown(String),
}

/// Parse `/workers ...` and `/delegate ...` from the raw line, keeping the task text verbatim.
fn parse_delegation(cmd: &str, body: &str) -> Option<SlashCommand> {
    let rest = body.trim_start()[cmd.len()..].trim();
    let mut it = rest.splitn(2, char::is_whitespace);
    let first = it.next().unwrap_or("").trim();
    let tail = it.next().unwrap_or("").trim();
    let second = tail.split_whitespace().next().map(str::to_lowercase);
    let lower = first.to_lowercase();
    if matches!(cmd, "delegate" | "delegates") && lower == "delivery" {
        return Some(SlashCommand::DelegateDelivery(tail.into()));
    }
    if matches!(cmd, "delegate" | "delegates") && lower == "after" {
        let (ids, rest) = tail.split_once(char::is_whitespace).unwrap_or((tail, ""));
        let (target, task) = rest
            .trim_start()
            .split_once(char::is_whitespace)
            .unwrap_or((rest, ""));
        let target = target.trim().to_lowercase();
        let task = task.trim().to_string();
        let parsed: Result<Vec<u32>, _> = ids.split(',').map(str::parse).collect();
        return Some(match parsed {
            Ok(after)
                if !after.is_empty()
                    && after.len() <= 8
                    && !after.contains(&0)
                    && !target.is_empty()
                    && !task.is_empty() =>
            {
                SlashCommand::DelegateAfter {
                    after,
                    target,
                    task,
                }
            }
            _ => SlashCommand::Invalid("delegate after ID[,ID] ACCOUNT TASK".into()),
        });
    }
    if matches!(cmd, "delegate" | "delegates") && lower == "model" {
        let args: Vec<_> = tail.split_whitespace().collect();
        return Some(if args.len() > 2 {
            SlashCommand::Invalid("delegate model ACCOUNT [MODEL|default]".into())
        } else {
            SlashCommand::DelegateModel {
                account: args.first().copied().unwrap_or("").to_lowercase(),
                model: args.get(1).map(|m| m.to_string()),
            }
        });
    }
    let control = matches!(
        lower.as_str(),
        "help"
            | "?"
            | "stop"
            | "cancel"
            | "use"
            | "send"
            | "protocol"
            | "collect"
            | "returns"
            | "graph"
            | "routes"
    );
    if ((cmd == "workers" || cmd == "worker") || control) && tail.split_whitespace().count() > 1 {
        return Some(SlashCommand::Invalid(cmd.into()));
    }
    if matches!(lower.as_str(), "help" | "?" | "collect" | "graph") && !tail.is_empty() {
        return Some(SlashCommand::Invalid(cmd.into()));
    }
    match cmd {
        "workers" | "worker" => Some(match lower.as_str() {
            "add" | "new" => SlashCommand::WorkersAdd(second),
            "remove" | "rm" | "delete" => SlashCommand::WorkersRemove(second),
            "" | "list" if tail.is_empty() => SlashCommand::Workers,
            _ => SlashCommand::Invalid("workers".into()),
        }),
        "delegate" | "delegates" => Some(match lower.as_str() {
            "" | "help" | "?" => SlashCommand::DelegateHelp,
            "stop" | "cancel" => SlashCommand::DelegateStop(second),
            "use" | "send" => SlashCommand::DelegateUse(second),
            "collect" => SlashCommand::DelegateCollect,
            "graph" => SlashCommand::DelegateGraph,
            "routes" => match second {
                None => SlashCommand::DelegateRoutes(None),
                Some(id) => match id.parse::<u32>() {
                    Ok(id) if id > 0 => SlashCommand::DelegateRoutes(Some(id)),
                    _ => SlashCommand::Invalid("delegate routes [ID]".into()),
                },
            },
            "policy" => SlashCommand::DelegatePolicy(tail.to_owned()),
            "scope" => SlashCommand::DelegateScope(tail.to_owned()),
            "scoped" => match tail.split_once(char::is_whitespace) {
                Some((account, rest)) => match crate::workspaces::parse_declarations(rest, true) {
                    Ok((declarations, task)) => SlashCommand::DelegateScoped {
                        account: account.into(),
                        declarations,
                        task,
                    },
                    Err(error) => SlashCommand::Invalid(error),
                },
                None => SlashCommand::Invalid(
                    "delegate scoped ACCOUNT read:PATH write:PATH -- TASK".into(),
                ),
            },
            "worktree" => {
                let mut words = tail.splitn(3, char::is_whitespace);
                match (words.next(), words.next(), words.next()) {
                    (Some(workspace), Some(account), Some(task)) if !task.trim().is_empty() => {
                        SlashCommand::DelegateWorktree {
                            workspace: workspace.into(),
                            account: account.into(),
                            task: task.into(),
                        }
                    }
                    _ => {
                        SlashCommand::Invalid("delegate worktree WORKSPACE_ID ACCOUNT TASK".into())
                    }
                }
            }
            "returns" => match second.as_deref() {
                Some("auto") => SlashCommand::DelegateReturns(Some(true)),
                Some("manual") => SlashCommand::DelegateReturns(Some(false)),
                None => SlashCommand::DelegateReturns(None),
                _ => SlashCommand::Invalid("delegate returns auto|manual".into()),
            },
            "protocol" => match second.as_deref() {
                Some("on" | "true" | "yes") => SlashCommand::DelegateProtocol(Some(true)),
                Some("off" | "false" | "no") => SlashCommand::DelegateProtocol(Some(false)),
                None => SlashCommand::DelegateProtocol(None),
                _ => SlashCommand::Invalid("delegate protocol".into()),
            },
            _ => SlashCommand::Delegate {
                target: lower,
                task: tail.to_string(),
            },
        }),
        _ => None,
    }
}

pub fn parse_slash(line: &str) -> Option<(SlashCommand, bool)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let body = trimmed.trim_start_matches('/').trim();
    let mut parts = body.split_whitespace();
    let cmd = parts.next().unwrap_or("").to_lowercase();
    let rest = parts.next().unwrap_or("");
    let words = body.split_whitespace().count();
    if matches!(
        cmd.as_str(),
        "delegate" | "delegates" | "worker" | "workers" | "crew" | "worker-panel" | "scope"
    ) {
        return Some((SlashCommand::Unknown(cmd), false));
    }
    let max_words = match cmd.as_str() {
        "chats" | "resume" | "new" | "config" | "version" | "ver" | "about" | "copy" | "cp"
        | "doctor" | "diag" | "exit" | "quit" | "q" | "agents" | "stats" | "close" => Some(1),
        "model" | "mode" => Some(2),
        "use" => Some(3),
        _ => None,
    };
    if max_words.is_some_and(|max| words > max) {
        return Some((SlashCommand::Invalid(cmd), false));
    }
    match cmd.as_str() {
        "" => None,
        "crew" => {
            let mut crew_words = body[cmd.len()..].split_whitespace();
            let action = crew_words.next().unwrap_or("").to_ascii_lowercase();
            if matches!(action.as_str(), "list" | "clear") && crew_words.clone().next().is_some() {
                return Some((
                    SlashCommand::Invalid("crew use NAME|list|clear".into()),
                    false,
                ));
            }
            if action == "list" || action.is_empty() {
                Some((SlashCommand::CrewList, false))
            } else if action == "clear" {
                Some((SlashCommand::CrewClear, false))
            } else if action == "use" {
                match (crew_words.next(), crew_words.next()) {
                    (Some(name), None) => Some((SlashCommand::CrewUse(name.to_owned()), false)),
                    _ => Some((
                        SlashCommand::Invalid("crew use NAME|list|clear".into()),
                        false,
                    )),
                }
            } else {
                Some((
                    SlashCommand::Invalid("crew use NAME|list|clear".into()),
                    false,
                ))
            }
        }
        "use" => match rest.to_lowercase().as_str() {
            "remove" | "rm" | "delete" | "forget" => Some((
                SlashCommand::UseRemove(parts.next().map(str::to_lowercase)),
                false,
            )),
            "add" | "new" | "connect" => Some((
                SlashCommand::UseAdd(parts.next().map(str::to_lowercase)),
                false,
            )),
            "" | "list" if words <= 2 => Some((SlashCommand::Use, false)),
            _ if words == 2 => Some((SlashCommand::UseAccount(rest.to_lowercase()), false)),
            _ => Some((SlashCommand::Invalid("use".into()), false)),
        },
        "model" if rest.is_empty() => Some((SlashCommand::Model, false)),
        "model" => Some((SlashCommand::ModelSet(rest.to_string()), false)),
        "usage"
            if (rest.eq_ignore_ascii_case("all") || rest.eq_ignore_ascii_case("refresh"))
                && parts.next().is_none() =>
        {
            Some((SlashCommand::Usage, false))
        }
        "usage" if !rest.is_empty() => Some((SlashCommand::Invalid("usage".into()), false)),
        "usage" => Some((SlashCommand::Usage, false)),
        "help" | "?" => Some((
            SlashCommand::Help(rest.eq_ignore_ascii_case("--all")),
            false,
        )),
        "chats" | "resume" => Some((SlashCommand::Resume, false)),
        "new" => Some((SlashCommand::New, false)),
        "config" => Some((SlashCommand::Config, false)),
        "mode" => Some((
            SlashCommand::Mode(Some(rest.to_lowercase()).filter(|r| !r.is_empty())),
            false,
        )),
        "update" | "upgrade" | "rebuild" if rest.is_empty() => Some((SlashCommand::Update, false)),
        "update" | "upgrade" | "rebuild" => Some((SlashCommand::Invalid("update".into()), false)),
        "version" | "ver" | "about" => Some((SlashCommand::Version, false)),
        "copy" | "cp" => Some((SlashCommand::Copy, false)),
        "doctor" | "diag" => Some((SlashCommand::Doctor, false)),
        "skills" | "hooks" => {
            let args = body[cmd.len()..].trim();
            let (action, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
            if matches!(action, "" | "list" | "catalog" | "status") && !rest.trim().is_empty() {
                return Some((
                    SlashCommand::Invalid(format!(
                        "{cmd}: unsupported arguments; use /{cmd} to inspect sources"
                    )),
                    false,
                ));
            }
            let routed = match (cmd.as_str(), action) {
                (_, "" | "list" | "catalog") => {
                    return Some((SlashCommand::Customization(cmd == "hooks"), false))
                }
                (_, "connect" | "status") => format!("{action} {rest}"),
                ("skills", "install") => format!("skills {rest}"),
                ("skills", "remove") => format!("skills-remove {rest}"),
                ("hooks", "verify") => format!("verify {rest}"),
                _ => return Some((SlashCommand::Invalid(cmd), false)),
            };
            Some((SlashCommand::Harness(routed), false))
        }
        "harness" => Some((
            SlashCommand::Harness(body["harness".len()..].trim().to_string()),
            false,
        )),
        "exit" | "quit" | "q" => Some((SlashCommand::Exit, false)),
        "workers" | "worker" | "delegate" | "delegates" => {
            parse_delegation(&cmd, body).map(|c| (c, false))
        }
        other => Some((SlashCommand::Unknown(other.to_string()), false)),
    }
}

pub fn parse_provider(s: &str) -> Option<ProviderKind> {
    match s.trim().to_lowercase().as_str() {
        "codex" => Some(ProviderKind::Codex),
        "claude" => Some(ProviderKind::Claude),
        "cursor" | "agent" => Some(ProviderKind::Cursor),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCategory {
    Chat,
    Workspace,
    Accounts,
    Workers,
    Settings,
    Diagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub args: &'static str,
    pub description: &'static str,
    pub long_help: &'static str,
    pub everyday: bool,
    pub category: CommandCategory,
    pub key_binding: Option<&'static str>,
    pub contextual_tip: Option<&'static str>,
}

macro_rules! command {
    ($name:literal, [$($alias:literal),* $(,)?], $args:literal, $description:literal, $long:literal, $everyday:expr, $category:expr, $key:expr, $tip:expr) => {
        CommandSpec { name: $name, aliases: &[$($alias),*], args: $args, description: $description, long_help: $long, everyday: $everyday, category: $category, key_binding: $key, contextual_tip: $tip }
    };
}

/// The single source for slash command presentation, documentation and contextual teaching.
pub const COMMAND_REGISTRY: &[CommandSpec] = &[
    command!(
        "/terminal",
        [],
        "[open|hide|close]",
        "open the local PowerShell terminal",
        "Run local shell commands in the current workspace. Hide keeps the shell alive; close stops it. Chat drafts stay separate.",
        true,
        CommandCategory::Workspace,
        Some("F11"),
        None
    ),
    command!(
        "/use",
        [],
        "ACCOUNT|add PROVIDER|remove ACCOUNT",
        "switch, add or remove accounts",
        "Choose an account or connect through the provider's own login.",
        true,
        CommandCategory::Accounts,
        None,
        Some("Near a usage limit? Try /use to switch accounts.")
    ),
    command!(
        "/model",
        [],
        "[MODEL]",
        "choose the model",
        "Choose a model from the active provider's local catalog.",
        true,
        CommandCategory::Settings,
        None,
        None
    ),
    command!(
        "/rename",
        [],
        "[TITLE]",
        "rename the current chat",
        "Set a title for the current chat or clear it to restore automatic naming.",
        false,
        CommandCategory::Chat,
        None,
        None
    ),
    command!(
        "/config",
        [],
        "[KEY VALUE]",
        "settings: display, permissions, accounts and behavior",
        "Inspect or change local Concordia settings.",
        true,
        CommandCategory::Settings,
        None,
        None
    ),
    command!(
        "/mode",
        [],
        "[plan|ask|auto|full]",
        "how much the agent may do without asking",
        "Set the permission mode for the active conversation.",
        true,
        CommandCategory::Settings,
        Some("Shift+Tab"),
        None
    ),
    command!(
        "/usage",
        [],
        "[all|refresh]",
        "limits for every account",
        "Show provider-reported limits without making an LLM call.",
        true,
        CommandCategory::Accounts,
        None,
        None
    ),
    command!(
        "/new",
        [],
        "",
        "start a fresh chat",
        "Start a fresh conversation in this folder.",
        true,
        CommandCategory::Chat,
        None,
        None
    ),
    command!(
        "/resume",
        ["/chats"],
        "",
        "continue an earlier chat",
        "Choose an earlier recorded conversation by explicit identity.",
        true,
        CommandCategory::Chat,
        None,
        Some("Use @ to find an earlier chat while writing.")
    ),
    command!(
        "/delegate",
        ["/delegates"],
        "ACCOUNT TASK",
        "run tasks on worker accounts",
        "Send a bounded task to a worker account and review its result.",
        true,
        CommandCategory::Workers,
        None,
        Some("Tab queues your message while a turn runs; Ctrl+R resumes a paused queue.")
    ),
    command!(
        "/workers",
        ["/worker"],
        "[add|remove]",
        "manage worker accounts",
        "List or manage the accounts available to workers.",
        true,
        CommandCategory::Workers,
        None,
        Some("Tab queues your message while a turn runs; Ctrl+R resumes a paused queue.")
    ),
    command!(
        "/crew",
        [],
        "[list|use NAME|clear]",
        "choose a saved lead and worker preset",
        "Apply a local crew from config.toml without making a provider call.",
        true,
        CommandCategory::Workers,
        None,
        None
    ),
    command!(
        "/worker-panel",
        [],
        "",
        "toggle the worker side panel",
        "Show live worker state in a read-only right-hand panel on wide terminals.",
        true,
        CommandCategory::Workers,
        Some("Ctrl+Shift+G"),
        None
    ),
    command!(
        "/harness",
        [],
        "ACTION",
        "connect and run harness tools",
        "Inspect the explicitly connected local harness.",
        false,
        CommandCategory::Diagnostics,
        Some("F10"),
        None
    ),
    command!(
        "/skills", [], "[list|install [PATH]|remove PATH|connect PATH|status]",
        "inspect skills and connect shared setup",
        "List selected account sources. Install a skill folder/collection across accounts; default source: ~/.agents/skills. Use --current to limit setup.",
        false, CommandCategory::Settings, None, None
    ),
    command!(
        "/hooks", [], "[list|connect PATH|verify|status]",
        "inspect hooks and verify shared setup",
        "List native hook sources; connect shared setup or run its verification. Native trust remains separate.",
        false, CommandCategory::Settings, None, None
    ),
    command!(
        "/open",
        [],
        "[PATH]",
        "preview files mentioned in this chat",
        "Open a formatted internal file preview.",
        true,
        CommandCategory::Workspace,
        None,
        None
    ),
    command!(
        "/search",
        [],
        "TEXT",
        "search project file contents",
        "Search the current project without leaving the conversation.",
        false,
        CommandCategory::Workspace,
        Some("Ctrl+K"),
        None
    ),
    command!(
        "/scope",
        [],
        "...",
        "declare task paths and reservations",
        "Declare workspace paths for a task and inspect reservations.",
        false,
        CommandCategory::Workspace,
        None,
        None
    ),
    command!(
        "/worktree",
        [],
        "...",
        "manage explicit Git workspaces",
        "Create or inspect an explicitly managed worktree.",
        false,
        CommandCategory::Workspace,
        None,
        None
    ),
    command!(
        "/context",
        [],
        "",
        "inspect context rollover policy",
        "Inspect context capacity and native compaction policy.",
        false,
        CommandCategory::Chat,
        None,
        None
    ),
    command!(
        "/recover",
        [],
        "[retry|legacy]",
        "inspect saved drafts and interrupted requests",
        "Preview or restore saved work for this workspace without automatically sending it.",
        true,
        CommandCategory::Chat,
        None,
        None
    ),
    command!(
        "/copy",
        ["/cp"],
        "",
        "copy the last reply",
        "Copy the last assistant reply to the clipboard.",
        true,
        CommandCategory::Chat,
        None,
        None
    ),
    command!(
        "/doctor",
        ["/diag"],
        "",
        "check your setup",
        "Check provider CLIs, logins, accounts and local state.",
        true,
        CommandCategory::Diagnostics,
        None,
        None
    ),
    command!(
        "/update",
        ["/upgrade"],
        "",
        "update Concordia",
        "Review and apply a local Concordia update.",
        true,
        CommandCategory::Diagnostics,
        None,
        None
    ),
    command!(
        "/exit",
        ["/quit", "/q"],
        "",
        "exit Concordia",
        "Close the current Concordia session.",
        true,
        CommandCategory::Chat,
        None,
        None
    ),
];

pub fn command_registry() -> &'static [CommandSpec] {
    COMMAND_REGISTRY
}

pub fn format_slash_help(all: bool) -> String {
    COMMAND_REGISTRY
        .iter()
        .filter(|c| all || c.everyday)
        .map(|c| {
            let args = if c.args.is_empty() {
                String::new()
            } else {
                format!(" {}", c.args)
            };
            format!("{:<34} {}", format!("{}{}", c.name, args), c.description)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Markdown table used by README generation and its synchronization test.
pub fn markdown_command_table() -> String {
    let mut out =
        String::from("| Command | Arguments | Description | View |\n| --- | --- | --- | --- |\n");
    for c in COMMAND_REGISTRY {
        let args = c.args.replace('|', "\\|");
        out.push_str(&format!(
            "| `{}` | `{}` | {} | {} |\n",
            c.name,
            args,
            c.description,
            if c.everyday { "everyday" } else { "advanced" }
        ));
    }
    out
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn palette_help_and_readme_come_from_one_registry() {
        assert!(format_slash_help(false).contains("/use"));
    }

    #[test]
    fn readme_command_table_in_sync() {
        let readme = include_str!("../../../README.md");
        let start = "<!-- COMMANDS:START -->";
        let end = "<!-- COMMANDS:END -->";
        let body = readme
            .split_once(start)
            .unwrap()
            .1
            .split_once(end)
            .unwrap()
            .0
            .trim();
        assert_eq!(body, markdown_command_table().trim());
    }

    #[test]
    fn key_sheet_lists_every_binding_once() {
        let bindings: Vec<_> = COMMAND_REGISTRY
            .iter()
            .filter_map(|c| c.key_binding)
            .collect();
        let unique: std::collections::HashSet<_> = bindings.iter().copied().collect();
        assert_eq!(bindings.len(), unique.len());
        assert!(bindings.contains(&"F10"));
    }
}

pub fn format_config_help() -> String {
    let home = crate::test_guard::test_home().display().to_string();
    [
        "config  local CLIs — no concordia cloud".to_string(),
        "connect /use add            pick a provider; concordia makes an isolated home and opens its login".into(),
        "        /use add codex      (or claude, cursor)".into(),
        String::new(),
        "by hand, if the login window cannot open:".into(),
        format!("Codex   $env:CODEX_HOME='{home}\\.codex'; codex login"),
        "Claude  claude login   (optional CLAUDE_CONFIG_DIR)".into(),
        "Cursor  agent login   (extra accounts: an concordia-api-key file in the account home)".into(),
        String::new(),
        "in concordia  /use · /model · /config · /mode · /usage · /new · /resume · /delegate · /harness · /open · /copy · /doctor · /update".into(),
    ]
    .join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    Ok,
    Exit,
    Update,
    OpenResume,
    OpenUse,
    OpenAddProvider,
    OpenRemovePicker,
    /// Connect a new account for this provider.
    AddAccount(ProviderKind),
    OpenModel,
    /// `/usage`: show the current account's limits in the transcript.
    ShowUsage,
    /// `/doctor`: open the interactive local diagnostics screen.
    OpenDoctor,
    ShowUsageRefresh,
    /// `/usage all`: picker with every ready account.
    OpenUsage,
    /// Copy the last assistant reply to the clipboard.
    CopyLastReply,
}

pub struct CommandContext<'a> {
    pub state_root: &'a Path,
    pub cwd: &'a Path,
    pub provider: ProviderKind,
    pub account: &'a str,
    pub trust: TrustMode,
}

pub fn dispatch_slash(
    cmd: SlashCommand,
    ctx: &CommandContext<'_>,
) -> (CommandOutcome, Vec<String>) {
    match cmd {
        SlashCommand::Exit => (CommandOutcome::Exit, Vec::new()),
        SlashCommand::Invalid(name) => (
            CommandOutcome::Ok,
            vec![format!(
                "invalid arguments for /{name}; type / to see the commands"
            )],
        ),
        SlashCommand::Config => (CommandOutcome::Ok, vec![format_config_help()]),
        SlashCommand::Help(all) => (
            CommandOutcome::Ok,
            vec![format!(
                "Commands ({} view):\n{}",
                if all { "all" } else { "everyday" },
                format_slash_help(all)
            )],
        ),
        SlashCommand::New => (
            CommandOutcome::Ok,
            vec![format!(
                "new chat · {}",
                display_account_name(ctx.provider, ctx.account)
            )],
        ),
        SlashCommand::Resume => (CommandOutcome::OpenResume, Vec::new()),
        // Handled by the TUI (it owns the live mode); nothing to do headless.
        SlashCommand::Mode(_) | SlashCommand::UseAccount(_) | SlashCommand::ModelSet(_) => {
            (CommandOutcome::Ok, Vec::new())
        }
        SlashCommand::Use => (CommandOutcome::OpenUse, Vec::new()),
        SlashCommand::UseAdd(None) => (CommandOutcome::OpenAddProvider, Vec::new()),
        SlashCommand::UseRemove(_) => (CommandOutcome::OpenRemovePicker, Vec::new()),
        SlashCommand::UseAdd(Some(p)) => match parse_provider(&p) {
            Some(provider) => (CommandOutcome::AddAccount(provider), Vec::new()),
            None => (
                CommandOutcome::Ok,
                vec![format!(
                    "unknown provider '{p}' - use codex, claude or cursor"
                )],
            ),
        },
        SlashCommand::Model => (CommandOutcome::OpenModel, Vec::new()),
        SlashCommand::Usage => (CommandOutcome::OpenUsage, Vec::new()),
        SlashCommand::UsageAll => (CommandOutcome::OpenUsage, Vec::new()),
        SlashCommand::UsageRefresh => (CommandOutcome::ShowUsageRefresh, Vec::new()),
        SlashCommand::Copy => (CommandOutcome::CopyLastReply, Vec::new()),
        SlashCommand::Doctor => (CommandOutcome::OpenDoctor, Vec::new()),
        SlashCommand::Update => (CommandOutcome::Update, Vec::new()),
        SlashCommand::Version => {
            let exe = std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "?".into());
            let active = crate::update::resolve_active_exe();
            (
                CommandOutcome::Ok,
                vec![
                    crate::version::version_line(),
                    format!("this pane  {exe}"),
                    format!("active     {}", active.display()),
                ],
            )
        }
        // Delegation needs the running TUI (workers, strip); `tui/workers.rs` handles these
        // before dispatch. Reaching here means a caller without a UI.
        SlashCommand::Workers
        | SlashCommand::WorkersAdd(_)
        | SlashCommand::WorkersRemove(_)
        | SlashCommand::Delegate { .. }
        | SlashCommand::DelegateAfter { .. }
        | SlashCommand::DelegateGraph
        | SlashCommand::DelegateDelivery(_)
        | SlashCommand::DelegateRoutes(_)
        | SlashCommand::DelegatePolicy(_)
        | SlashCommand::DelegateScope(_)
        | SlashCommand::DelegateScoped { .. }
        | SlashCommand::DelegateWorktree { .. }
        | SlashCommand::DelegateStop(_)
        | SlashCommand::DelegateUse(_)
        | SlashCommand::DelegateCollect
        | SlashCommand::DelegateReturns(_)
        | SlashCommand::DelegateModel { .. }
        | SlashCommand::DelegateHelp
        | SlashCommand::DelegateProtocol(_) => (
            CommandOutcome::Ok,
            vec!["delegation commands run inside the Concordia TUI".into()],
        ),
        SlashCommand::CrewList | SlashCommand::CrewUse(_) | SlashCommand::CrewClear => (
            CommandOutcome::Ok,
            vec!["crew commands run inside the Concordia TUI".into()],
        ),
        SlashCommand::Unknown(name) => (
            CommandOutcome::Ok,
            vec![format!(
                "unknown command: /{name}; type / to see the commands"
            )],
        ),
        SlashCommand::Harness(_) | SlashCommand::Customization(_) => (
            CommandOutcome::Ok,
            vec!["/harness runs inside the Concordia TUI".into()],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customization_routes_validate_arguments_before_setup() {
        for command in [
            "/skills list extra",
            "/hooks catalog extra",
            "/hooks status extra",
        ] {
            assert!(
                matches!(parse_slash(command).unwrap().0, SlashCommand::Invalid(_)),
                "{command}"
            );
        }
        assert_eq!(
            parse_slash("/skills install \"shared skills\" --current")
                .unwrap()
                .0,
            SlashCommand::Harness("skills \"shared skills\" --current".into())
        );
        assert_eq!(
            parse_slash("/hooks verify").unwrap().0,
            SlashCommand::Harness("verify ".into())
        );
    }

    fn cmd(line: &str) -> SlashCommand {
        parse_slash(line).unwrap().0
    }

    #[test]
    fn use_add_parses_with_and_without_provider() {
        assert_eq!(cmd("/use"), SlashCommand::Use);
        assert_eq!(cmd("/use add"), SlashCommand::UseAdd(None));
        assert_eq!(
            cmd("/use add Codex"),
            SlashCommand::UseAdd(Some("codex".into()))
        );
        // /use with some other word is still the plain picker.
        assert_eq!(cmd("/use list"), SlashCommand::Use);
    }

    #[test]
    fn use_add_dispatches_to_picker_or_provider() {
        let dir = std::env::temp_dir();
        let ctx = CommandContext {
            state_root: &dir,
            cwd: &dir,
            provider: ProviderKind::Codex,
            account: "codex",
            trust: TrustMode::Workspace,
        };
        assert_eq!(
            dispatch_slash(SlashCommand::UseAdd(None), &ctx).0,
            CommandOutcome::OpenAddProvider
        );
        assert_eq!(
            dispatch_slash(SlashCommand::UseAdd(Some("agent".into())), &ctx).0,
            CommandOutcome::AddAccount(ProviderKind::Cursor)
        );
        let (o, lines) = dispatch_slash(SlashCommand::UseAdd(Some("gemini".into())), &ctx);
        assert_eq!(o, CommandOutcome::Ok);
        assert!(lines[0].contains("unknown provider 'gemini'"));
    }
}

#[cfg(test)]
mod usage_tests {
    use super::*;

    #[test]
    fn usage_is_current_account_and_usage_all_lists_everyone() {
        assert_eq!(parse_slash("/usage").unwrap().0, SlashCommand::Usage);
        assert_eq!(parse_slash("/usage all").unwrap().0, SlashCommand::Usage);
        assert_eq!(parse_slash("/usage ALL").unwrap().0, SlashCommand::Usage);
        let dir = std::env::temp_dir();
        let ctx = CommandContext {
            state_root: &dir,
            cwd: &dir,
            provider: ProviderKind::Codex,
            account: "codex",
            trust: TrustMode::Workspace,
        };
        assert_eq!(
            dispatch_slash(SlashCommand::Usage, &ctx).0,
            CommandOutcome::OpenUsage
        );
        let help = |c: &str| {
            command_registry()
                .iter()
                .find(|spec| spec.name == c)
                .unwrap()
                .description
        };
        assert_eq!(help("/usage"), "limits for every account");
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;

    #[test]
    fn update_opens_the_page_and_rejects_removed_check_argument() {
        assert_eq!(parse_slash("/update").unwrap().0, SlashCommand::Update);
        assert_eq!(parse_slash("/upgrade").unwrap().0, SlashCommand::Update);
        assert_eq!(
            parse_slash("/update check").unwrap().0,
            SlashCommand::Invalid("update".into())
        );
        assert_eq!(
            parse_slash("/update CHECK").unwrap().0,
            SlashCommand::Invalid("update".into())
        );
    }

    #[test]
    fn workers_and_delegate_commands_parse() {
        let c = |l: &str| parse_slash(l).unwrap().0;
        assert_eq!(c("/workers"), SlashCommand::Workers);
        assert_eq!(
            c("/workers add Codex-2"),
            SlashCommand::WorkersAdd(Some("codex-2".into()))
        );
        assert_eq!(c("/workers add"), SlashCommand::WorkersAdd(None));
        assert_eq!(
            c("/workers rm codex-2"),
            SlashCommand::WorkersRemove(Some("codex-2".into()))
        );
        assert_eq!(c("/crew list"), SlashCommand::CrewList);
        assert_eq!(
            c("/crew use office"),
            SlashCommand::CrewUse("office".into())
        );
        assert_eq!(c("/crew clear"), SlashCommand::CrewClear);
        assert_eq!(c("/delegate"), SlashCommand::DelegateHelp);
        assert_eq!(c("/delegate help"), SlashCommand::DelegateHelp);
        assert_eq!(c("/delegate graph"), SlashCommand::DelegateGraph);
        assert_eq!(c("/delegate routes"), SlashCommand::DelegateRoutes(None));
        assert_eq!(
            c("/delegate routes 7"),
            SlashCommand::DelegateRoutes(Some(7))
        );
        assert_eq!(
            c("/delegate policy set primary:Native-Model backup"),
            SlashCommand::DelegatePolicy("set primary:Native-Model backup".into())
        );
        assert_eq!(
            c("/delegate after 1,2   Codex-2   Preserve  exact\nTASK"),
            SlashCommand::DelegateAfter {
                after: vec![1, 2],
                target: "codex-2".into(),
                task: "Preserve  exact\nTASK".into()
            }
        );
        for invalid in [
            "/delegate after",
            "/delegate after 0 codex task",
            "/delegate after 1,,2 codex task",
            "/delegate graph typo",
            "/delegate routes 0",
            "/delegate routes invalid",
            "/delegate routes 1 2",
        ] {
            assert!(matches!(c(invalid), SlashCommand::Invalid(_)));
        }
        assert_eq!(
            c("/delegate stop Claude"),
            SlashCommand::DelegateStop(Some("claude".into()))
        );
        assert_eq!(c("/delegate use"), SlashCommand::DelegateUse(None));
        assert_eq!(
            c("/delegate protocol ON"),
            SlashCommand::DelegateProtocol(Some(true))
        );
        assert_eq!(
            c("/delegate protocol"),
            SlashCommand::DelegateProtocol(None)
        );
        // The task keeps its case, spacing inside and punctuation.
        assert_eq!(
            c("/delegate Codex-2   Fix the BUG in  src/a.rs, please"),
            SlashCommand::Delegate {
                target: "codex-2".into(),
                task: "Fix the BUG in  src/a.rs, please".into()
            }
        );
        assert_eq!(
            c("/delegate auto"),
            SlashCommand::Delegate {
                target: "auto".into(),
                task: String::new()
            }
        );
    }

    #[test]
    fn use_remove_parses_with_and_without_an_account() {
        assert_eq!(
            parse_slash("/use remove").unwrap().0,
            SlashCommand::UseRemove(None)
        );
        assert_eq!(
            parse_slash("/use rm codex-3").unwrap().0,
            SlashCommand::UseRemove(Some("codex-3".into()))
        );
        assert_eq!(
            parse_slash("/use forget Claude").unwrap().0,
            SlashCommand::UseRemove(Some("claude".into()))
        );
    }
}
