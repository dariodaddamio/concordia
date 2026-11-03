use anyhow::Result;
use concordia::store::default_store_root;
use concordia::tui;
use concordia::update::resolve_active_exe;
use concordia::version::version_line;

fn print_help() {
    println!(
        "{}\n\
         \n\
           concordia           chat TUI in this terminal\n\
           concordia --help    this message\n\
           concordia --version build stamp (confirm pane is not stale)\n\
           concordia install   install this exe as the `concordia` on your PATH (new panes get it)\n\
           concordia wezterm   launch WezTerm running Concordia (split with Ctrl+Shift+D/E)\n\
           concordia doctor    check install, provider CLIs, accounts and state (exit 1 on a problem)\n\
           concordia chat list|read|search  bounded native CLI chats across your own accounts\n\
           concordia chat index|sources|excerpt  revision-checked native history pointers\n\
           concordia chat recall|lineage|link|detach  bounded retrieval and explicit source links\n\
           concordia preview FILE          read-only Markdown preview in this terminal\n\
           concordia file-edit FILE        independent file editor (Ctrl+Q closes)\n\
           concordia thread list [--json] | map <id> | read <id> [--tail N] [--grep TEXT] [--range A-B] [--max-chars N]\n\
                               read recorded threads (opt-in: CONCORDIA_THREADS=1); read-only\n\
           concordia mcp       stdio MCP server: threads, thread_map, thread_read, native_read, usage, delegate (docs/mcp.md)\n\
         \n\
         Slash: /use [add] /model /usage /resume /agents /trust /config /new /threads /split /update /version /copy /doctor /exit\n\
         Files: /open PATH[:LINE[:COLUMN]] · /preview PATH (internal read-only panes) · /close ID · /edit PATH (external editor)\n\
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

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        tui::run_chat_tui().await?;
        return Ok(());
    }
    if args.len() > 1
        && matches!(
            args[0].as_str(),
            "--host"
                | "host"
                | "--help"
                | "-h"
                | "help"
                | "install"
                | "wezterm"
                | "doctor"
                | "--doctor"
                | "mcp"
                | "--version"
                | "-V"
                | "version"
        )
    {
        eprintln!("{} takes no arguments (try --help)", args[0]);
        std::process::exit(2);
    }
    match args[0].as_str() {
        "chat" => match concordia::chat_cli::run(&default_store_root(), &args[1..]) {
            Ok(output) => print!("{output}"),
            Err((message, code)) => {
                eprintln!("{message}");
                std::process::exit(code);
            }
        },
        "file-edit" => tui::run_file_editor(&args[1..])?,
        "preview" => {
            if args.len() != 2 {
                anyhow::bail!("usage: concordia preview FILE");
            }
            tui::run_file_preview(std::path::Path::new(&args[1]))?;
        }
        "--host" | "host" => {
            // Alias: same as bare concordia (TUI in current pane).
            tui::run_chat_tui().await?;
        }
        "--help" | "-h" | "help" => print_help(),
        "install" => {
            match concordia::update::install_self() {
                Ok(r) => {
                    println!("installed {}", r.installed.display());
                    if r.path_added {
                        println!(
                            "added {} to your PATH",
                            concordia::update::install_bin_dir().display()
                        );
                    }
                    if r.state_adopted {
                        println!("carried over your settings and registered accounts from the old orc state");
                    }
                    if let Some(old) = r.legacy_retired {
                        println!(
                        "removed the old `orc` install from your PATH ({}); delete that folder when you are ready",
                        old.display()
                    );
                    }
                    println!("open a NEW terminal, then run: concordia --version");
                }
                Err(e) => {
                    eprintln!("install failed: {e:#}");
                    std::process::exit(1);
                }
            }
        }
        "wezterm" => {
            if let Err(e) = concordia::host::wezterm::launch_window() {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        "doctor" | "--doctor" => {
            let (report, healthy) = concordia::doctor::run_cli_report();
            println!("{report}");
            if !healthy {
                std::process::exit(1);
            }
        }
        "thread" => match concordia::threads::run_cli(&default_store_root(), &args[1..]) {
            Ok(out) => print!("{out}"),
            Err((msg, code)) => {
                eprintln!("{msg}");
                std::process::exit(code);
            }
        },
        "mcp" => {
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            if let Err(e) =
                concordia::mcp::serve(stdin.lock(), stdout.lock(), &default_store_root())
            {
                eprintln!("mcp: {e}");
                std::process::exit(1);
            }
        }
        "--version" | "-V" | "version" => {
            println!("{}", version_line());
            // Read-only: reporting a version must never repoint the install at this exe.
            if let Ok(exe) = std::env::current_exe() {
                println!("exe: {}", exe.display());
            }
        }
        other => {
            eprintln!("unknown argument: {other} (try --help)");
            std::process::exit(2);
        }
    }
    Ok(())
}
