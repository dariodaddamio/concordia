use crate::accounts::display_account_name;
use crate::store::{ProviderKind, TrustMode};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand {
    Use,
    UseAccount(String),
    UseAdd(Option<String>),
    UseRemove(Option<String>),
    Model,
    ModelSet(String),
    Effort(Option<String>),
    Usage,
    UsageAll,
    UsageRefresh,
    Help(bool),
    Resume,
    New,
    Config,
    Mode(Option<String>),
    Update,
    Version,
    Copy,
    Doctor,
    Exit,
    Harness(String),
    Customization(bool),
    Mcp(String),
    Invalid(String),
    Unknown(String),
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
        "effort" => Some((
            SlashCommand::Effort((!rest.is_empty()).then(|| rest.to_string())),
            false,
        )),
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
        "mcp" => Some((
            SlashCommand::Mcp(body[cmd.len()..].trim().to_string()),
            false,
        )),
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
                (_, "" | "list" | "catalog" | "status") => {
                    return Some((SlashCommand::Customization(cmd == "hooks"), false))
                }
                ("skills", "install") => format!("skills {rest}"),
                ("skills", "remove") => format!("skills-remove {rest}"),
                _ => return Some((SlashCommand::Invalid(cmd), false)),
            };
            Some((SlashCommand::Harness(routed), false))
        }
        "harness" => Some((
            SlashCommand::Harness(body["harness".len()..].trim().to_string()),
            false,
        )),
        "exit" | "quit" | "q" => Some((SlashCommand::Exit, false)),
        "workers" | "worker" | "delegate" | "delegates" | "crew" | "worker-panel" | "scope" => {
            Some((SlashCommand::Unknown(cmd), false))
        }
        other => Some((SlashCommand::Unknown(other.to_string()), false)),
    }
}

pub fn is_provider_passthrough(line: &str) -> bool {
    matches!(parse_slash(line), Some((SlashCommand::Unknown(_), _)))
}

const REUSES_PROVIDER: &[&str] = &["/model", "/effort"];

pub fn shared_command_name(
    line: &str,
    provider: ProviderKind,
    advertised: Option<&[crate::provider::AvailableCommand]>,
) -> Option<String> {
    let name = line.split_whitespace().next()?.to_ascii_lowercase();
    if !name.starts_with('/') || REUSES_PROVIDER.contains(&name.as_str()) {
        return None;
    }
    let concordia = COMMAND_REGISTRY.iter().any(|command| {
        command.name.eq_ignore_ascii_case(&name)
            || command
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(&name))
    });
    let provider = match advertised {
        Some(commands) => commands
            .iter()
            .any(|command| command.name.eq_ignore_ascii_case(&name)),
        None => crate::provider::command_specs(provider)
            .iter()
            .any(|command| command.name.eq_ignore_ascii_case(&name)),
    };
    concordia.then_some(name).filter(|_| provider)
}

pub fn parse_provider(s: &str) -> Option<ProviderKind> {
    match s.trim().to_lowercase().as_str() {
        "codex" => Some(ProviderKind::Codex),
        "claude" => Some(ProviderKind::Claude),
        "cursor" | "agent" => Some(ProviderKind::Cursor),
        "grok" | "grok-build" => Some(ProviderKind::Grok),
        "opencode" | "open-code" => Some(ProviderKind::Opencode),
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

pub const COMMAND_REGISTRY: &[CommandSpec] = &[
    command!(
        "/terminal",
        [],
        "[open|native|help|hide|close]",
        "open the local terminal or the selected provider's native interface",
        "Run local shell commands, or use native to open the selected provider's own interface on this chat's exact session when available. Native works for every supported provider, including opencode. Hide keeps it alive; close stops it. Chat drafts stay separate.",
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
        "/effort", [], "[LEVEL]", "choose reasoning effort",
        "Choose a provider-reported effort for the active model.",
        true, CommandCategory::Settings, None, None
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
        "[plan|ask|edit|full]",
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
        "/harness",
        [],
        "ACTION",
        "integrations and agent tools",
        "Inspect connected integrations and agent tools.",
        false,
        CommandCategory::Diagnostics,
        Some("F10"),
        None
    ),
    command!(
        "/worktree",
        [],
        "[list|create BASE|inspect ID|delegate ID ACCOUNT TASK|review ID ACTION|integrate ID RECEIPT|reconcile ID|cancel]",
        "create, delegate into, review and explicitly integrate managed Git workspaces",
        "Managed worktrees are explicit Git workspaces for delegated changes. create makes one from a clean committed base; delegate runs one worker in it; review ID integrate|pick|retire|delete-branch returns a receipt; integrate ID RECEIPT applies exactly that reviewed change. Receipts expire when heads, files, reservations or references change. Concordia never merges, resets, cleans or runs checks on its own.",
        false,
        CommandCategory::Workspace,
        None,
        None
    ),
    command!(
        "/skills", [], "[list|install [PATH]|remove PATH]",
        "inspect skills and toggle their availability",
        "List selected account sources. Install a skill folder/collection across accounts; default source: ~/.agents/skills. Use --current to limit setup.",
        false, CommandCategory::Settings, None, None
    ),
    command!(
        "/hooks", [], "[list]",
        "inspect native hooks",
        "List native hook sources. Connect and verify harness setup through /harness. Native trust remains separate.",
        false, CommandCategory::Settings, None, None
    ),
    command!(
        "/mcp", [], "[add|remove|on|off|import] [NAME]",
        "manage Concordia-wide MCP servers",
        "List MCP servers and their delivery state. Add, remove, enable, disable or import provider configurations without writing provider homes.",
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
        "        /use add codex      (or claude, cursor, grok)".into(),
        String::new(),
        "by hand, if the login window cannot open:".into(),
        format!("Codex   $env:CODEX_HOME='{home}\\.codex'; codex login"),
        "Claude  claude login   (optional CLAUDE_CONFIG_DIR)".into(),
        "Cursor  agent login   (extra accounts: an concordia-api-key file in the account home)".into(),
        "Grok    grok login    (uses the selected GROK_HOME)".into(),
        String::new(),
        "in concordia  /use · /model · /config · /mode · /usage · /new · /resume · /harness · /open · /copy · /doctor · /update".into(),
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
    AddAccount(ProviderKind),
    OpenModel,
    ShowUsage,
    OpenDoctor,
    ShowUsageRefresh,
    OpenUsage,
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
        SlashCommand::Mode(_)
        | SlashCommand::UseAccount(_)
        | SlashCommand::ModelSet(_)
        | SlashCommand::Effort(_) => (CommandOutcome::Ok, Vec::new()),
        SlashCommand::Use => (CommandOutcome::OpenUse, Vec::new()),
        SlashCommand::UseAdd(None) => (CommandOutcome::OpenAddProvider, Vec::new()),
        SlashCommand::UseRemove(_) => (CommandOutcome::OpenRemovePicker, Vec::new()),
        SlashCommand::UseAdd(Some(p)) => match parse_provider(&p) {
            Some(provider) => (CommandOutcome::AddAccount(provider), Vec::new()),
            None => (
                CommandOutcome::Ok,
                vec![format!(
                    "unknown provider '{p}' - use codex, claude, cursor or grok"
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
        SlashCommand::Unknown(_) => (CommandOutcome::Ok, Vec::new()),
        SlashCommand::Harness(_) | SlashCommand::Customization(_) => (
            CommandOutcome::Ok,
            vec!["/harness runs inside the Concordia TUI".into()],
        ),
        SlashCommand::Mcp(_) => (
            CommandOutcome::Ok,
            vec!["/mcp runs inside the Concordia TUI".into()],
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
        assert!(matches!(
            parse_slash("/hooks verify").unwrap().0,
            SlashCommand::Invalid(_)
        ));
    }

    fn cmd(line: &str) -> SlashCommand {
        parse_slash(line).unwrap().0
    }

    #[test]
    fn unknown_commands_are_not_a_dispatch_error() {
        let ctx = CommandContext {
            state_root: Path::new("."),
            cwd: Path::new("."),
            provider: ProviderKind::Codex,
            account: "test",
            trust: TrustMode::Plan,
        };
        assert_eq!(
            dispatch_slash(cmd("/provider-native"), &ctx).1,
            Vec::<String>::new()
        );
    }

    #[test]
    fn use_add_parses_with_and_without_provider() {
        assert_eq!(cmd("/use"), SlashCommand::Use);
        assert_eq!(cmd("/use add"), SlashCommand::UseAdd(None));
        assert_eq!(
            cmd("/use add Codex"),
            SlashCommand::UseAdd(Some("codex".into()))
        );
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
            trust: TrustMode::Edit,
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
            trust: TrustMode::Edit,
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
    fn removed_delegation_commands_are_unknown() {
        let c = |l: &str| parse_slash(l).unwrap().0;
        assert_eq!(c("/workers"), SlashCommand::Unknown("workers".into()));
        assert_eq!(c("/delegate"), SlashCommand::Unknown("delegate".into()));
        assert_eq!(c("/crew list"), SlashCommand::Unknown("crew".into()));
        assert_eq!(c("/scope"), SlashCommand::Unknown("scope".into()));
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
