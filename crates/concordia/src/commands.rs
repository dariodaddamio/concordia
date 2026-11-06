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
    Resume,
    New,
    Config,
    Trust,
    /// `/mode [plan|ask|auto|full]`: show or set the permission mode.
    Mode(Option<String>),
    Split,
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
    /// `/harness ...` is executed by the TUI because it needs the live account home.
    Harness(String),
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
    let vertical = rest.eq_ignore_ascii_case("v") || rest.eq_ignore_ascii_case("vertical");
    let words = body.split_whitespace().count();
    let max_words = match cmd.as_str() {
        "chats" | "resume" | "new" | "config" | "version" | "ver" | "about" | "copy" | "cp"
        | "doctor" | "diag" | "exit" | "quit" | "q" | "agents" | "stats" | "close" => Some(1),
        "model" | "mode" | "trust" | "split" | "login" | "connect" => Some(2),
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
        "login" | "connect" => Some((
            SlashCommand::UseAdd(Some(rest.to_lowercase()).filter(|r| !r.is_empty())),
            false,
        )),
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
        "chats" | "resume" => Some((SlashCommand::Resume, false)),
        "new" => Some((SlashCommand::New, false)),
        "config" => Some((SlashCommand::Config, false)),
        "trust" => Some((SlashCommand::Trust, false)),
        "mode" => Some((
            SlashCommand::Mode(Some(rest.to_lowercase()).filter(|r| !r.is_empty())),
            false,
        )),
        "split"
            if rest.is_empty()
                || vertical
                || rest.eq_ignore_ascii_case("h")
                || rest.eq_ignore_ascii_case("horizontal") =>
        {
            Some((SlashCommand::Split, vertical))
        }
        "split" => Some((SlashCommand::Invalid("split".into()), false)),
        "update" | "upgrade" | "rebuild" if rest.is_empty() => Some((SlashCommand::Update, false)),
        "update" | "upgrade" | "rebuild" => Some((SlashCommand::Invalid("update".into()), false)),
        "version" | "ver" | "about" => Some((SlashCommand::Version, false)),
        "copy" | "cp" => Some((SlashCommand::Copy, false)),
        "doctor" | "diag" => Some((SlashCommand::Doctor, false)),
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

/// Every slash command with a one-line description, in the order the palette shows them.
pub const SLASH_COMMANDS: &[(&str, &str)] = &[
    (
        "/worktree",
        "Create or inspect explicitly managed Git worktrees; no automatic integration or deletion",
    ),
    ("/use", "switch, add or remove accounts"),
    ("/model", "choose the model"),
    (
        "/config",
        "settings: display, permissions, accounts and behavior",
    ),
    (
        "/skills",
        "inspect project and user skills for this provider",
    ),
    ("/hooks", "inspect project and user hooks for this provider"),
    ("/mode", "how much the agent may do without asking"),
    ("/usage", "limits for every account"),
    (
        "/integrations",
        "inspect connected integrations and agent tools (F10)",
    ),
    ("/new", "start a fresh chat"),
    ("/resume", "continue an earlier chat"),
    ("/memory", "search original chats and inspect source links"),
    (
        "/context",
        "inspect context rollover and native compaction policy",
    ),
    ("/delegate", "run tasks on worker accounts"),
    (
        "/scope",
        "declare task paths and inspect workspace reservations",
    ),
    ("/harness", "connect and run harness tools"),
    (
        "/open",
        "preview files mentioned in this chat, or /open PATH",
    ),
    ("/search", "search project file contents"),
    ("/copy", "copy the last reply"),
    ("/doctor", "check your setup"),
    ("/update", "update Concordia"),
];

pub fn format_config_help() -> String {
    let home = dirs::home_dir()
        .unwrap_or_else(|| Path::new(".").to_path_buf())
        .display()
        .to_string();
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
    OpenTrust,
    OpenUse,
    OpenAddProvider,
    OpenRemovePicker,
    /// Connect a new account for this provider.
    AddAccount(ProviderKind),
    OpenModel,
    /// `/usage`: show the current account's limits in the transcript.
    ShowUsage,
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
    pub split_vertical: bool,
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
        SlashCommand::New => (
            CommandOutcome::Ok,
            vec![format!(
                "new chat · {}",
                display_account_name(ctx.provider, ctx.account)
            )],
        ),
        SlashCommand::Resume => (CommandOutcome::OpenResume, Vec::new()),
        SlashCommand::Trust => (CommandOutcome::OpenTrust, Vec::new()),
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
        SlashCommand::Doctor => (
            CommandOutcome::Ok,
            crate::doctor::run_fast_report()
                .lines()
                .map(str::to_string)
                .collect(),
        ),
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
        SlashCommand::Split => {
            match crate::host::wezterm::split_concordia_pane(ctx.split_vertical) {
                Ok(()) => (
                    CommandOutcome::Ok,
                    vec![format!(
                        "split → {} concordia pane",
                        if ctx.split_vertical {
                            "vertical"
                        } else {
                            "horizontal"
                        }
                    )],
                ),
                Err(e) => (CommandOutcome::Ok, vec![format!("split failed — {e}")]),
            }
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
        SlashCommand::Unknown(name) => (
            CommandOutcome::Ok,
            vec![format!(
                "unknown command: /{name}; type / to see the commands"
            )],
        ),
        SlashCommand::Harness(_) => (
            CommandOutcome::Ok,
            vec!["/harness runs inside the Concordia TUI".into()],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            cmd("/use connect claude"),
            SlashCommand::UseAdd(Some("claude".into()))
        );
        assert_eq!(cmd("/login"), SlashCommand::UseAdd(None));
        assert_eq!(
            cmd("/login cursor"),
            SlashCommand::UseAdd(Some("cursor".into()))
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
            split_vertical: false,
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
            split_vertical: false,
        };
        assert_eq!(
            dispatch_slash(SlashCommand::Usage, &ctx).0,
            CommandOutcome::OpenUsage
        );
        let help = |c: &str| SLASH_COMMANDS.iter().find(|(n, _)| *n == c).unwrap().1;
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
