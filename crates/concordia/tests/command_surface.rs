//! Public command parser contract. These tests exercise parsing/dispatch only; no provider,
//! account mutation, host integration, updater, or login flow is started.

use concordia::commands::{
    dispatch_slash, parse_slash, CommandContext, CommandOutcome, SlashCommand,
};
use concordia::store::{ProviderKind, TrustMode};

#[test]
fn cli_commands_reject_extra_arguments_before_any_action() {
    for command in [
        "--version",
        "--help",
        "doctor",
        "install",
        "mcp",
        "--window",
        "window",
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_concordia"))
            .args([command, "unexpected"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{command}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("takes no arguments"));
    }
}
#[test]
fn documented_command_aliases_resolve_to_the_same_action() {
    let cases = [
        ("/use", SlashCommand::Use),
        ("/chats", SlashCommand::Resume),
        ("/resume", SlashCommand::Resume),
        ("/upgrade", SlashCommand::Update),
        ("/rebuild", SlashCommand::Update),
        ("/ver", SlashCommand::Version),
        ("/about", SlashCommand::Version),
        ("/cp", SlashCommand::Copy),
        ("/diag", SlashCommand::Doctor),
        ("/quit", SlashCommand::Exit),
        ("/q", SlashCommand::Exit),
        ("/worker", SlashCommand::Workers),
        ("/delegates help", SlashCommand::DelegateHelp),
    ];
    for (line, expected) in cases {
        assert_eq!(parse_slash(line).map(|(c, _)| c), Some(expected), "{line}");
    }
}

#[test]
fn invalid_arguments_do_not_fall_through_to_stateful_actions() {
    let cases = [
        ("/update typo", "update"),
        ("/upgrade check now", "update"),
        ("/usage typo", "usage"),
        ("/usage all now", "usage"),
    ];
    for (line, name) in cases {
        assert_eq!(
            parse_slash(line).map(|(c, _)| c),
            Some(SlashCommand::Invalid(name.into())),
            "{line}"
        );
    }

    let root = tempfile::tempdir().unwrap();
    let ctx = CommandContext {
        state_root: root.path(),
        cwd: root.path(),
        provider: ProviderKind::Codex,
        account: "codex-test",
        trust: TrustMode::Workspace,
    };
    for (_, name) in cases {
        let (outcome, messages) = dispatch_slash(SlashCommand::Invalid(name.into()), &ctx);
        assert_eq!(outcome, CommandOutcome::Ok);
        assert!(messages[0].contains("invalid arguments"));
    }
}
