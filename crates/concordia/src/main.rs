use anyhow::Result;
use concordia::store::default_store_root;
use concordia::tui;
use concordia::update::resolve_active_exe;
use concordia::version::version_line;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Window,
    Install,
    Shortcut,
    Doctor,
    Chat,
    FileEdit,
    Mcp,
    AskMcp,
    Help,
    Version,
}

#[derive(Debug, Clone, Copy)]
struct CommandSpec {
    name: &'static str,
    aliases: &'static [&'static str],
    args: &'static str,
    help: &'static str,
    command: Command,
}

const CLI_HELP: &[CommandSpec] = &[
    CommandSpec {
        name: "concordia",
        aliases: &[],
        args: "",
        help: "native app window (image paste and file drop)",
        command: Command::Window,
    },
    CommandSpec {
        name: "--window",
        aliases: &["window"],
        args: "",
        help: "native app window (image paste and file drop)",
        command: Command::Window,
    },
    CommandSpec {
        name: "install",
        aliases: &[],
        args: "",
        help: "install this exe on PATH",
        command: Command::Install,
    },
    CommandSpec {
        name: "shortcut",
        aliases: &[],
        args: "install|remove [--desktop]",
        help: "create or remove a Windows shortcut",
        command: Command::Shortcut,
    },
    CommandSpec {
        name: "doctor",
        aliases: &["--doctor"],
        args: "",
        help: "check install, provider CLIs, accounts and state",
        command: Command::Doctor,
    },
    CommandSpec {
        name: "chat",
        aliases: &[],
        args: "list|read|search|index|sources|excerpt|recall|lineage|link|detach",
        help: "bounded native CLI chats and history pointers",
        command: Command::Chat,
    },
    CommandSpec {
        name: "file-edit",
        aliases: &[],
        args: "FILE",
        help: "independent file editor",
        command: Command::FileEdit,
    },
    CommandSpec {
        name: "--help",
        aliases: &["-h", "help"],
        args: "",
        help: "show this help",
        command: Command::Help,
    },
    CommandSpec {
        name: "--version",
        aliases: &["-V", "version"],
        args: "",
        help: "show the build stamp",
        command: Command::Version,
    },
];

fn parse_command(args: &[String]) -> Result<(Command, &[String]), String> {
    let Some(name) = args.first() else {
        return Err("missing command".into());
    };
    let hidden = match name.as_str() {
        "mcp" => Some(Command::Mcp),
        "ask-mcp" => Some(Command::AskMcp),
        _ => None,
    };
    let Some(spec) = CLI_HELP
        .iter()
        .find(|s| s.name == name || s.aliases.contains(&name.as_str()))
    else {
        return Err(format!("unknown argument: {name} (try --help)"));
    };
    let rest = &args[1..];
    if let Some(command) = hidden {
        if !rest.is_empty() {
            return Err(format!("{name} takes no arguments (try --help)"));
        }
        return Ok((command, rest));
    }
    if spec.args.is_empty() && !rest.is_empty() {
        return Err(format!("{name} takes no arguments (try --help)"));
    }
    Ok((spec.command, rest))
}

fn print_help() {
    let commands = CLI_HELP
        .iter()
        .map(|spec| {
            let command = if spec.name == "concordia" {
                "window".to_string()
            } else if spec.args.is_empty() {
                spec.name.to_string()
            } else {
                format!("{} {}", spec.name, spec.args)
            };
            format!("  concordia {command:<43} {}", spec.help)
        })
        .collect::<Vec<_>>()
        .join("\n");
    println!("{}\n\n{commands}\n\n         \n\
         Slash: /use [add] /model /usage /resume /agents /config /new /threads /update /version /copy /doctor /exit\n\
         Files: /open PATH[:LINE[:COLUMN]] · /close ID\n\
         Delegate: /workers [add|remove] /delegate <account|auto> <task> /delegate stop|use|help (docs/delegation.md)\n\
         Auth mode shown bottom-right. Ctrl+C cancels (or copies a selection) · Tab slash-complete.\n\
         Connect: /use add [codex|claude|cursor] makes an isolated home and opens its login.\n\
         Mouse: click places the caret, drag selects, double/triple-click picks word/line,\n\
         wheel scrolls, right-click copies the selection or pastes.\n\
         Keys: Ctrl+A/X/V select-all/cut/paste · Shift+Enter or Ctrl+J newline · PgUp/PgDn scroll.\n\
         Queue: Tab queues while busy · Ctrl+Up/Down focuses queue · Ctrl+R resumes paused queue.\n\
         Edit: Ctrl+Z undo · Ctrl+Shift+Z/Ctrl+Y redo · Ctrl+E full activity detail · Ctrl+O expand all.\n\
         Never uses resume --last — bind an explicit session id via /resume.\n\
         \n\
         Active exe: {}\n\
         State: {}\n\
         Install: .\\scripts\\install-dev.ps1  or  /update",
        version_line(),
        resolve_active_exe().display(),
        default_store_root().display()
    );
}

async fn run_default_chat() -> Result<()> {
    #[cfg(windows)]
    {
        tui::run_chat_window().await
    }
    #[cfg(not(windows))]
    {
        tui::run_chat_tui().await
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    if let Some(warning) = concordia::store::outside_profile_warning(&default_store_root()) {
        eprintln!("warn: {warning}");
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        run_default_chat().await?;
        return Ok(());
    }
    let (command, rest) = match parse_command(&args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    match command {
        Command::Chat => match concordia::chat_cli::run(&default_store_root(), rest) {
            Ok(output) => print!("{output}"),
            Err((message, code)) => {
                eprintln!("{message}");
                std::process::exit(code);
            }
        },
        Command::FileEdit => tui::run_file_editor(rest)?,
        Command::Window => {
            #[cfg(windows)]
            tui::run_chat_window().await?;
            #[cfg(not(windows))]
            anyhow::bail!("Native app windows require Windows");
        }
        Command::Help => print_help(),
        Command::Install => match concordia::update::install_self() {
            Ok(r) => {
                println!("installed {}", r.installed.display());
                if r.path_added {
                    println!(
                        "added {} to your PATH",
                        concordia::update::install_bin_dir().display()
                    );
                }
                println!("open a NEW terminal, then run: concordia --version");
            }
            Err(e) => {
                eprintln!("install failed: {e:#}");
                std::process::exit(1);
            }
        },
        Command::Shortcut => {
            if rest.is_empty()
                || rest.len() > 2
                || !matches!(rest[0].as_str(), "install" | "remove")
            {
                anyhow::bail!("usage: concordia shortcut install|remove [--desktop]");
            }
            let desktop = rest.get(1).is_some_and(|arg| arg == "--desktop");
            if rest.get(1).is_some_and(|arg| arg != "--desktop") {
                anyhow::bail!("usage: concordia shortcut install|remove [--desktop]");
            }
            let path = if rest[0] == "install" {
                concordia::update::install_shortcut(desktop)?
            } else {
                concordia::update::remove_shortcut(desktop)?
            };
            println!("{} {}", rest[0], path.display());
        }
        Command::Doctor => {
            let (report, healthy) = concordia::doctor::run_cli_report();
            println!("{report}");
            if !healthy {
                std::process::exit(1);
            }
        }
        Command::Mcp => {
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            if let Err(e) =
                concordia::mcp::serve(stdin.lock(), stdout.lock(), &default_store_root())
            {
                eprintln!("mcp: {e}");
                std::process::exit(1);
            }
        }
        Command::AskMcp => {
            concordia::ask_bridge::serve(std::io::stdin().lock(), std::io::stdout().lock())?;
        }
        Command::Version => {
            println!("{}", version_line());
            // Read-only: reporting a version must never repoint the install at this exe.
            if let Ok(exe) = std::env::current_exe() {
                println!("exe: {}", exe.display());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_command, CLI_HELP};
    use std::collections::HashSet;

    #[test]
    fn help_table_lists_each_command_once() {
        let commands: HashSet<_> = CLI_HELP.iter().map(|spec| spec.name).collect();
        assert_eq!(commands.len(), CLI_HELP.len());
        assert!(commands.contains("chat"));
        assert!(commands.contains("chat"));
    }

    #[test]
    fn every_parsed_command_has_a_help_row_and_vice_versa() {
        for spec in CLI_HELP {
            let args = if spec.args.is_empty() {
                vec![spec.name.to_string()]
            } else {
                vec![spec.name.to_string(), "fixture".into()]
            };
            let (command, _) = parse_command(&args).unwrap();
            assert_eq!(command, spec.command, "{}", spec.name);
        }
    }

    #[test]
    fn unknown_and_extra_args_exit_2_with_hint() {
        let unknown = parse_command(&["nope".into()]).unwrap_err();
        assert!(unknown.contains("try --help"));
        let extra = parse_command(&["--help".into(), "extra".into()]).unwrap_err();
        assert!(extra.contains("takes no arguments"));
        assert!(extra.contains("try --help"));
    }
}
