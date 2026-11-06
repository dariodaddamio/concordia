//! Independent file surface; it never enters the chat dispatcher or starts a provider.
use super::*;
use crate::file_links;
use anyhow::Context;
use ratatui::backend::CrosstermBackend;

pub(super) fn run(arguments: &[String]) -> Result<()> {
    let path = arguments.first().context(
        "usage: concordia file-edit FILE [--line N] [--column N] [--recover ID | --list-recovery]",
    )?;
    let requested = PathBuf::from(path);
    let mut line = None;
    let mut column = None;
    let mut recover = None;
    let mut list = false;
    let mut args = arguments.iter().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--line" if line.is_none() => {
                line = Some(args.next().context("Missing line")?.parse::<u32>()?)
            }
            "--column" if column.is_none() => {
                column = Some(args.next().context("Missing column")?.parse::<u32>()?)
            }
            "--recover" if recover.is_none() => {
                recover = Some(args.next().context("Missing recovery ID")?.clone())
            }
            "--list-recovery" if !list => list = true,
            _ => anyhow::bail!("Unknown or repeated file-editor option"),
        }
    }
    anyhow::ensure!(
        line != Some(0) && column != Some(0),
        "File locations are one-based"
    );
    let state = default_store_root();
    let cwd = std::env::current_dir()?.canonicalize()?;
    let path = if (recover.is_some() || list) && !requested.exists() {
        requested
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(&cwd)
            .canonicalize()?
            .join(requested.file_name().context("Missing file name")?)
    } else {
        requested.canonicalize()?
    };
    anyhow::ensure!(
        !crate::workspace_paths::reserved_device_name(&path),
        "reserved device name is not a file"
    );
    anyhow::ensure!(
        !crate::workspace_paths::sensitive(&path),
        "File is excluded by workspace policy"
    );
    let root = recovery::recovery_root(&state.join("file-editors"), &cwd)?;
    if list {
        anyhow::ensure!(recover.is_none(), "Choose listing or one exact recovery ID");
        for choice in recovery::available(&root)? {
            println!("{}", choice.id);
        }
        return Ok(());
    }
    let lease = if let Some(id) = recover.as_deref() {
        let lease = recovery::claim(&root, id)?;
        let value: serde_json::Value = serde_json::from_slice(&lease.files()?)?;
        anyhow::ensure!(
            value["panes"].as_array().is_some_and(|panes| panes
                .iter()
                .any(|pane| pane["path"].as_str() == Some(path.to_string_lossy().as_ref()))),
            "Recovery record does not contain this exact file; original retained"
        );
        anyhow::ensure!(
            value["panes"]
                .as_array()
                .is_some_and(|panes| panes.iter().all(|pane| pane["path"]
                    .as_str()
                    .is_some_and(|path| !crate::workspace_paths::sensitive(Path::new(path))))),
            "Recovered file is excluded by workspace policy; original retained"
        );
        lease
    } else {
        recovery::create(&root)?
    };
    lease.save("", &[])?;
    let record_id = lease.record_id();
    let mut app = App::new(state, cwd.clone());
    enable_raw_mode()?;
    let restore = TerminalRestoreGuard::new();
    execute!(
        stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        EnableFocusChange
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let _ = execute!(
        stdout(),
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
    );
    let mut terminal_input = input_stream::InputStream::spawn()?;
    let size = terminal.size()?;
    app.workspace = workspace::Workspace::standalone(Rect::new(0, 0, size.width, size.height));
    app.workspace
        .set_file_recovery(Arc::new(lease), &cwd)
        .map_err(anyhow::Error::msg)?;
    app.workspace
        .open(file_links::ResolvedFile { path, line, column })
        .map_err(anyhow::Error::msg)?;
    let result = (|| -> Result<()> {
        loop {
            app.workspace.poll();
            app.poll_file_jobs();
            if app.workspace.file_quit_requested() {
                break;
            }
            if let Some(notice) = app.workspace.take_notice() {
                app.flash(notice);
            }
            if app.workspace.focused.is_none() {
                app.workspace.focused = app.workspace.panes.first().map(|pane| pane.id);
            }
            terminal.draw(|frame| {
                workspace::draw(frame, &mut app);
                if let Some((message, at)) = &app.flash {
                    if at.elapsed() < Duration::from_secs(4) {
                        let area = frame.area();
                        frame.render_widget(
                            Paragraph::new(text::terminal_text(message).0),
                            Rect::new(
                                area.x,
                                area.bottom().saturating_sub(1),
                                area.width,
                                1.min(area.height),
                            ),
                        );
                    }
                }
                if app.workspace.panes.is_empty() {
                    frame.render_widget(
                        Paragraph::new("File panes closed · Ctrl+Q exits"),
                        frame.area(),
                    );
                }
            })?;
            if !terminal_input.poll(Duration::from_millis(35))? {
                continue;
            }
            let (event, _, _) = terminal_input.read()?;
            let Some(event) = event else {
                continue;
            };
            match event {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if key.code == KeyCode::Char('q')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        if app.workspace.request_file_exit() {
                            break;
                        }
                    } else {
                        workspace::handle_key(&mut app, &key);
                    }
                }
                Event::Paste(text) => {
                    workspace::paste(&mut app, &text);
                }
                Event::Mouse(mouse) => {
                    workspace::handle_mouse(&mut app, &mouse);
                }
                Event::FocusGained | Event::Resize(..) => {
                    terminal.autoresize()?;
                    terminal.clear()?;
                }
                _ => {}
            }
        }
        Ok(())
    })();
    let recovery_result = app
        .workspace
        .flush_file_recovery()
        .map_err(anyhow::Error::msg);
    drop(terminal_input);
    drop(terminal);
    drop(restore);
    println!("File editor recovery: {record_id} (concordia file-edit FILE --recover ID from the same workspace)");
    result?;
    recovery_result?;
    Ok(())
}
