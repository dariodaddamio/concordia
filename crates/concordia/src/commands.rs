use crate::accounts::display_account_name;
use crate::store::{ProviderKind, TrustMode};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand {
    Use,
    /// `/use add [provider]`; `None` opens the provider picker.
    UseAdd(Option<String>),
    /// `/use remove [account]`; `None` opens the account picker.
    UseRemove(Option<String>),
    Model,
    /// `/usage`: limits for the current account.
    Usage,
    /// `/usage all`: limits for every ready account.
    UsageAll,
    Resume,
    New,
    Config,
    Trust,
    Split,
    Update,
    /// `/update check`: report only.
    UpdateCheck,
    Version,
    Copy,
    Doctor,
    Exit,
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
    let vertical = rest.eq_ignore_ascii_case("v") || rest.eq_ignore_ascii_case("vertical");
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
            _ => Some((SlashCommand::Use, false)),
        },
        "login" | "connect" => Some((
            SlashCommand::UseAdd(Some(rest.to_lowercase()).filter(|r| !r.is_empty())),
            false,
        )),
        "model" => Some((SlashCommand::Model, false)),
        "usage" if rest.eq_ignore_ascii_case("all") => Some((SlashCommand::UsageAll, false)),
        "usage" => Some((SlashCommand::Usage, false)),
        "chats" | "resume" => Some((SlashCommand::Resume, false)),
        "new" => Some((SlashCommand::New, false)),
        "config" => Some((SlashCommand::Config, false)),
        "trust" => Some((SlashCommand::Trust, false)),
        "split" => Some((SlashCommand::Split, vertical)),
        "update" | "upgrade" | "rebuild" if rest.eq_ignore_ascii_case("check") => {
            Some((SlashCommand::UpdateCheck, false))
        }
        "update" | "upgrade" | "rebuild" => Some((SlashCommand::Update, false)),
        "version" | "ver" | "about" => Some((SlashCommand::Version, false)),
        "copy" | "cp" => Some((SlashCommand::Copy, false)),
        "doctor" | "diag" => Some((SlashCommand::Doctor, false)),
        "exit" | "quit" | "q" => Some((SlashCommand::Exit, false)),
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
    ("/use", "switch account or provider"),
    ("/use add", "connect a new account"),
    ("/use remove", "remove an account"),
    ("/model", "choose the model"),
    ("/usage", "limits for this account"),
    ("/usage all", "limits for every account"),
    ("/resume", "continue an earlier session"),
    ("/trust", "approvals: ask, workspace or full"),
    ("/config", "how to connect providers"),
    ("/new", "start a fresh conversation"),
    ("/split", "open another pane"),
    ("/update", "update Concordia and restart"),
    ("/update check", "look for a newer version"),
    ("/version", "build and install info"),
    ("/copy", "copy the last reply"),
    ("/doctor", "check your setup"),
    ("/exit", "quit"),
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
        "in concordia  /use · /use add · /model · /usage · /resume · /trust · /update".into(),
    ]
    .join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    Ok,
    Exit,
    Update,
    UpdateCheck,
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
        SlashCommand::Usage => (CommandOutcome::ShowUsage, Vec::new()),
        SlashCommand::UsageAll => (CommandOutcome::OpenUsage, Vec::new()),
        SlashCommand::Copy => (CommandOutcome::CopyLastReply, Vec::new()),
        SlashCommand::Doctor => (
            CommandOutcome::Ok,
            crate::doctor::run_fast_report()
                .lines()
                .map(str::to_string)
                .collect(),
        ),
        SlashCommand::Update => (CommandOutcome::Update, vec!["updating…".into()]),
        SlashCommand::UpdateCheck => (
            CommandOutcome::UpdateCheck,
            vec!["checking for updates…".into()],
        ),
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
        SlashCommand::Unknown(name) => (
            CommandOutcome::Ok,
            vec![format!("unknown command: /{name} — try /config")],
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
        assert_eq!(parse_slash("/usage all").unwrap().0, SlashCommand::UsageAll);
        assert_eq!(parse_slash("/usage ALL").unwrap().0, SlashCommand::UsageAll);
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
            CommandOutcome::ShowUsage
        );
        assert_eq!(
            dispatch_slash(SlashCommand::UsageAll, &ctx).0,
            CommandOutcome::OpenUsage
        );
        let help = |c: &str| SLASH_COMMANDS.iter().find(|(n, _)| *n == c).unwrap().1;
        assert_eq!(help("/usage"), "limits for this account");
        assert_eq!(help("/usage all"), "limits for every account");
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;

    #[test]
    fn update_and_update_check_parse_separately() {
        assert_eq!(parse_slash("/update").unwrap().0, SlashCommand::Update);
        assert_eq!(parse_slash("/upgrade").unwrap().0, SlashCommand::Update);
        assert_eq!(
            parse_slash("/update check").unwrap().0,
            SlashCommand::UpdateCheck
        );
        assert_eq!(
            parse_slash("/update CHECK").unwrap().0,
            SlashCommand::UpdateCheck
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
