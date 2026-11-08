use super::*;
pub fn run_file_preview(path: &Path) -> Result<()> {
    preview::run(path)
}
#[path = "file_editor.rs"]
mod file_editor;
pub fn run_file_editor(arguments: &[String]) -> Result<()> {
    file_editor::run(arguments)
}

pub async fn run_chat_tui() -> Result<()> {
    run_chat_ui(false).await
}

#[cfg(windows)]
pub async fn run_chat_window() -> Result<()> {
    run_chat_ui(true).await
}

/// One conversation tab: its `App` and the channels its provider turns report on.
struct RunTab {
    app: App,
    turn_tx: mpsc::UnboundedSender<String>,
    turn_rx: mpsc::UnboundedReceiver<String>,
    done_tx: mpsc::UnboundedSender<TurnOutcome>,
    done_rx: mpsc::UnboundedReceiver<TurnOutcome>,
    handle_tx: mpsc::UnboundedSender<Arc<RunningTurn>>,
    handle_rx: mpsc::UnboundedReceiver<Arc<RunningTurn>>,
    progress_tx: mpsc::UnboundedSender<TurnProgress>,
    progress_rx: mpsc::UnboundedReceiver<TurnProgress>,
}

impl RunTab {
    fn new(app: App) -> Self {
        let (turn_tx, turn_rx) = mpsc::unbounded_channel();
        let (done_tx, done_rx) = mpsc::unbounded_channel();
        let (handle_tx, handle_rx) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();
        Self {
            app,
            turn_tx,
            turn_rx,
            done_tx,
            done_rx,
            handle_tx,
            handle_rx,
            progress_tx,
            progress_rx,
        }
    }

    /// Persist the draft and stop anything still running; used on close and on exit.
    fn shut_down(&mut self) {
        self.app.persist_draft_now();
        self.handle_rx.close();
        while let Ok(handle) = self.handle_rx.try_recv() {
            handle.cancel();
        }
        // Leaving mid-turn must not orphan the provider (it would keep running and spending).
        if let Some(cancel) = self.app.preparing_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if let Some(h) = self.app.cancel_handle.take() {
            h.cancel();
        }
        self.app.cancel_all_delegates();
    }
}

impl tabs::TabState for RunTab {
    fn needs_close_confirmation(&self) -> bool {
        tabs::TabState::needs_close_confirmation(&self.app)
    }
}

fn new_tab_app(state_root: &Path, cwd: &Path, use_window: bool) -> App {
    let mut app = App::new(state_root.to_path_buf(), cwd.to_path_buf());
    app.refresh_binding();
    landing::open(&mut app);
    app.prior_provider = app.provider;
    app.prior_home = app.continuity_home();
    app.workspace.set_native_windows(use_window);
    app
}

async fn run_chat_ui(use_window: bool) -> Result<()> {
    let state_root = default_store_root();
    crate::logging::init(&state_root);
    crate::logging::info(&format!(
        "start {} exe={}",
        crate::version::version_line(),
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    ));
    ensure_default_selection(&state_root)?;
    crate::config::loaded(); // read config.toml once, at start
    let cwd = std::env::current_dir()?;

    let mut app = App::new(state_root.clone(), cwd.clone());
    app.compat_rx = Some(crate::provider::compat::start_real_detection(
        state_root.clone(),
    ));
    app.refresh_binding();
    app.restore_state();
    if !app.recovery_choices().is_empty() {
        app.open_recovery_picker();
    }
    landing::open(&mut app);
    app.prior_provider = app.provider;
    app.prior_home = app.continuity_home();

    let terminal_restore = (!use_window).then(TerminalRestoreGuard::new);
    let (backend, mut terminal_input) = if use_window {
        #[cfg(windows)]
        {
            let (backend, events) = native_window::open_detailed()?;
            let input = input_stream::InputStream::from_native(events)?;
            // This process owns a native window now. Detach only its inherited console,
            // so another application's console control events cannot terminate its runs.
            unsafe {
                windows_sys::Win32::System::Console::FreeConsole();
            }
            (display_backend::DisplayBackend::Window(backend), input)
        }
        #[cfg(not(windows))]
        anyhow::bail!("Native windows are available on Windows")
    } else {
        enable_raw_mode()?;
        let mut output = stdout();
        execute!(output, EnterAlternateScreen)?;
        let _ = execute!(
            output,
            EnableMouseCapture,
            EnableBracketedPaste,
            EnableFocusChange
        );
        let _ = execute!(
            output,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        );
        let backend = ratatui::backend::CrosstermBackend::new(output);
        (
            display_backend::DisplayBackend::Terminal(backend),
            input_stream::InputStream::spawn()?,
        )
    };
    // A panic must not leave the terminal in raw/mouse-reporting mode.
    let default_hook = std::panic::take_hook();
    let panic_restore = terminal_restore
        .as_ref()
        .map(|restore| restore.0.clone())
        .unwrap_or_else(|| Arc::new(std::sync::atomic::AtomicBool::new(false)));
    std::panic::set_hook(Box::new(move |info| {
        if panic_restore.load(std::sync::atomic::Ordering::Acquire) {
            // Show the panic on the primary screen. The guard restores input modes after
            // the VT reader has stopped and restored its own saved console settings.
            let _ = execute!(
                std::io::stdout(),
                LeaveAlternateScreen,
                crossterm::cursor::Show
            );
        }
        persist::flush_on_panic();
        crate::logging::error(&format_panic_log_line(info.payload(), info.location()));
        eprintln!(
            "Concordia crashed; details are in {}",
            crate::logging::log_path(&default_store_root()).display()
        );
        default_hook(info);
    }));
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    app.workspace.set_native_windows(use_window);
    #[cfg(windows)]
    let mut detached_windows =
        use_window.then(|| detached::DetachedWindows::new(terminal_input.wake_handle()));

    let tick_rate = Duration::from_millis(200);
    let mut last_tick = Instant::now();
    let mut key_burst = input::BurstBuffer::default();
    let mut mouse_pointer = pointer::Pointer::default();
    let mut window_mouse = None;
    // Drawing is comparatively expensive: it rasterizes the whole frame and, on Windows,
    // extracts native images.  Keep the wait loop responsive, but only do that work after a
    // state change that can affect the screen.
    let mut dirty = true;
    let redraw_always = std::env::var_os("CONCORDIA_REDRAW_ALWAYS").is_some();

    let mut tabs = tabs::TabSet::new(RunTab::new(app));
    let mut close_armed: Option<Instant> = None;
    let mut previous_labels: Vec<String> = Vec::new();

    macro_rules! draw_if_needed {
        ($app:ident, $force:expr) => {
            if $force || (dirty && (!key_burst.active() || !terminal_input.poll(Duration::ZERO)?)) {
                let app = &mut *$app;
                let mut screen_text = String::new();
                if !use_window {
                    execute!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
                }
                app.raster.native = use_window;
                #[cfg(windows)]
                let mut image = Vec::new();
                let drawn = terminal
                    .draw(|f| {
                        raster::begin_frame(&app);
                        draw_ui(f, &mut *app);
                        if crate::config::for_root(&app.state_root).feed.screen {
                            let area = f.area();
                            for y in area.top()..area.bottom() {
                                for x in area.left()..area.right() {
                                    if let Some(cell) = f.buffer_mut().cell((x, y)) {
                                        screen_text.push_str(cell.symbol());
                                    }
                                }
                                screen_text.push('\n');
                            }
                        }
                        #[cfg(windows)]
                        {
                            image = raster::native_images(f, &app);
                        }
                    })
                    .map(|_| ());
                #[cfg(windows)]
                if drawn.is_ok() {
                    terminal.backend_mut().image(image)?;
                }
                let ended = if use_window {
                    Ok(())
                } else {
                    execute!(terminal.backend_mut(), EndSynchronizedUpdate)
                };
                drawn?;
                ended?;
                if !screen_text.is_empty() && crate::config::for_root(&app.state_root).feed.screen {
                    let writer = crate::feed::FeedWriter::new(
                        &app.state_root,
                        app.thread_id().unwrap_or("main"),
                        true,
                    );
                    let _ = writer.screen(&screen_text);
                }
                dirty = false;
                #[cfg(windows)]
                if let Some(windows) = &mut detached_windows {
                    windows.draw(&mut *app);
                }
                if use_window {
                    if let Some((x, y)) = window_mouse {
                        terminal.backend_mut().pointer(pointer::shape(&app, x, y))?;
                    }
                } else {
                    mouse_pointer.refresh(&app)?;
                }
            }
        };
    }

    let mut previous_title = String::new();
    let input_result = (|| -> Result<()> {
        loop {
            let active = tabs.active;
            let labels: Vec<String> = if tabs.has_multiple() {
                tabs.tabs.iter().map(|t| t.app.tab_label()).collect()
            } else {
                Vec::new()
            };
            if labels != previous_labels {
                dirty = true;
                previous_labels = labels.clone();
            }
            let mut tab_action = tabs::TabAction::Unhandled;
            {
                let RunTab { app, turn_tx, .. } = &mut tabs.tabs[active];
                app.tab_strip = labels;
                app.tab_index = active;
                #[cfg(windows)]
                if let Some(windows) = &mut detached_windows {
                    windows.drain(app);
                    windows.sync(app);
                }
                // Drain pending paste chunks before repainting the growing prompt.
                draw_if_needed!(app, redraw_always);
                let title = app.window_title();
                if title != previous_title {
                    terminal.backend_mut().title(&title)?;
                    previous_title = title;
                }

                let mut timeout = tick_rate.saturating_sub(last_tick.elapsed());
                // Input already wakes immediately. Provider channels used to wait for the
                // 200 ms animation tick; drain them at interactive latency during live turns.
                if app.busy || app.deleg.d.any_active() {
                    timeout = timeout.min(Duration::from_millis(33));
                }
                if terminal_input.poll(timeout)? {
                    // Drain a burst (mouse drags, pastes) before redrawing; bounded so a flood can't
                    // starve the turn/progress channels below.
                    let mut quit = false;
                    for _ in 0..256 {
                        let (event, captured_at, quiet_before) = terminal_input.read()?;
                        if quiet_before {
                            if let Some(burst) = key_burst.take_quiet() {
                                if dispatch_key_burst(&mut *app, burst, turn_tx) {
                                    quit = true;
                                    break;
                                }
                            }
                        }
                        let Some(event) = event else {
                            if !terminal_input.poll(Duration::ZERO)? {
                                break;
                            }
                            continue;
                        };
                        // Any input event may have changed a visible interaction state, including
                        // events that are intentionally no-ops for the current screen.
                        dirty = true;
                        if let Event::Key(key) = &event {
                            let action = tabs::action_for(*key);
                            // A focused file pane keeps Ctrl+W for closing itself.
                            let pane_close =
                                action == tabs::TabAction::Close && app.workspace.focused.is_some();
                            if action != tabs::TabAction::Unhandled && !pane_close {
                                if let Some(burst) = key_burst.take_pending() {
                                    dispatch_key_burst(app, burst, turn_tx);
                                }
                                tab_action = action;
                                break;
                            }
                        }
                        if let Event::Mouse(m) = &event {
                            if m.kind == MouseEventKind::Down(MouseButton::Left) {
                                if let Some(index) = app.tab_hits.iter().position(|r| {
                                    r.contains(ratatui::layout::Position::new(m.column, m.row))
                                }) {
                                    tab_action = tabs::TabAction::Select(index + 1);
                                    break;
                                }
                            }
                        }
                        match event {
                            Event::Key(key)
                                if use_window
                                    && key.code == KeyCode::F(12)
                                    && key.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                quit = true;
                                break;
                            }
                            Event::Key(key) if use_window => {
                                // The owned window delivers paste and drops explicitly. Its
                                // keyboard input needs no terminal paste-inference quiet gap.
                                if handle_key(&mut *app, key, turn_tx) {
                                    quit = true;
                                    break;
                                }
                            }
                            Event::Key(key) => {
                                if dispatch_key_event(
                                    &mut *app,
                                    key,
                                    &mut key_burst,
                                    turn_tx,
                                    captured_at,
                                ) {
                                    quit = true;
                                    break;
                                }
                            }
                            Event::Mouse(m) => {
                                if let Some(burst) = key_burst.take_pending() {
                                    dispatch_key_burst(&mut *app, burst, turn_tx);
                                }
                                if use_window {
                                    window_mouse = Some((m.column, m.row));
                                    terminal
                                        .backend_mut()
                                        .pointer(pointer::shape(app, m.column, m.row))?;
                                } else {
                                    mouse_pointer.update(app, m.column, m.row)?;
                                }
                                #[cfg(windows)]
                                if let Some((x, y)) = terminal_input.take_native_mouse_up() {
                                    if workspace::finish_tab_drag(&mut *app, x, y) {
                                        continue;
                                    }
                                }
                                handle_mouse(&mut *app, m);
                            }
                            Event::Paste(s) => {
                                if let Some(burst) = key_burst.take_pending() {
                                    dispatch_key_burst(&mut *app, burst, turn_tx);
                                }
                                if app.project_search.active() || !questions::paste(&mut *app, &s) {
                                    if s.is_empty() && clipboard::has_attachment() {
                                        app.paste_clipboard();
                                    } else {
                                        app.paste_text(&s)
                                    }
                                }
                            }
                            Event::FocusGained | Event::Resize(..) => {
                                // The host may have invalidated visible cells while our diff buffer
                                // still describes the old screen (tab switches and resizes).
                                // Query the current size: queued resize events may already be stale.
                                terminal.autoresize()?;
                                terminal.clear()?;
                            }
                            _ => {}
                        }
                        if !terminal_input.poll(Duration::ZERO)? {
                            break;
                        }
                    }
                    if quit {
                        break;
                    }
                }
            }

            match tab_action {
                tabs::TabAction::Unhandled => {}
                tabs::TabAction::New => {
                    tabs.add(RunTab::new(new_tab_app(&state_root, &cwd, use_window)));
                    close_armed = None;
                    dirty = true;
                }
                tabs::TabAction::Close if tabs.has_multiple() => {
                    let confirmed =
                        close_armed.is_some_and(|at| at.elapsed() < Duration::from_secs(3));
                    if tabs.needs_close_confirmation() && !confirmed {
                        close_armed = Some(Instant::now());
                        tabs.active_mut().app.flash(
                            "This tab has a running turn or an unsent draft. Ctrl+W again closes it.",
                        );
                    } else {
                        tabs.active_mut().shut_down();
                        tabs.close(true);
                        close_armed = None;
                    }
                    dirty = true;
                }
                tabs::TabAction::Close => {
                    tabs.active_mut().app.flash("This is the last tab.");
                    dirty = true;
                }
                tabs::TabAction::Reopen => {
                    if !tabs.reopen() {
                        tabs.active_mut().app.flash("No closed tab to reopen.");
                    }
                    dirty = true;
                }
                tabs::TabAction::Next => tabs.next(false),
                tabs::TabAction::Previous => tabs.next(true),
                tabs::TabAction::Select(n) => tabs.select_number(n),
            }
            if tabs.active != active {
                close_armed = None;
                dirty = true;
            }

            let tick_due = last_tick.elapsed() >= tick_rate;
            for tab in tabs.tabs.iter_mut() {
                let RunTab {
                    app,
                    turn_tx,
                    turn_rx,
                    done_tx,
                    done_rx,
                    handle_tx,
                    handle_rx,
                    progress_tx,
                    progress_rx,
                } = tab;
                while let Ok(h) = handle_rx.try_recv() {
                    app.accept_cancel_handle(h);
                    dirty = true;
                }

                // A finished turn can wait here for an inline queue edit to be committed.
                app.queue_advance(turn_tx);
                while let Ok(prompt) = turn_rx.try_recv() {
                    dirty = true;
                    let incoming = crate::attachments::Message::decode(&prompt);
                    let image_inputs = incoming.images();
                    if app.discard_cancelled_pending_turn() {
                        continue;
                    }
                    let state_root = app.state_root.clone();
                    let cwd = app.cwd.clone();
                    let provider = app.provider;
                    let account = app.account.clone();
                    let model = app.model.clone();
                    let trust = app.trust;
                    let owner = crate::provider::interaction::InteractionOwner {
                        conversation_id: app.deleg.d.conversation_id().to_owned(),
                        workspace: cwd.clone(),
                        execution_cwd: cwd.clone(),
                        run_id: app.continuation.run_id().to_owned(),
                        attempt_id: uuid::Uuid::new_v4().to_string(),
                        provider,
                        account: account.clone(),
                        home: app.home_path.clone(),
                    };
                    let owner = match owner.validate_for(provider, &cwd, app.home_path.as_deref()) {
                        Ok(owner) => owner,
                        Err(error) => {
                            app.on_turn_done(
                                TurnOutcome {
                                    stderr_tail: format!("native interaction owner: {error}"),
                                    ..Default::default()
                                },
                                turn_tx,
                            );
                            continue;
                        }
                    };
                    if let Err(error) = app.reserve_main_scope(&owner) {
                        app.on_turn_done(
                            TurnOutcome {
                                failed: true,
                                stderr_tail: format!("Workspace admission: {error}"),
                                ..Default::default()
                            },
                            turn_tx,
                        );
                        continue;
                    }
                    let cwd = owner.execution_cwd.clone();
                    let launch_home = owner.home.clone();
                    app.questions.cancel_native();
                    app.native_owner = Some(owner.clone());
                    app.native_accounting_owner = Some(owner.clone());
                    let original_reference_prompt =
                        app.last_sent.clone().unwrap_or_else(|| prompt.clone());
                    app.reference_journal.begin(
                        &owner,
                        &original_reference_prompt,
                        app.keep.history,
                    );
                    let reference_sender = app.reference_journal.sender();
                    let scope_invoked = app.scope_invocation_flag();
                    let scope_intent = app
                        .scopes
                        .snapshot(&owner.run_id)
                        .filter(|request| !request.whole_workspace_write())
                        .map(|request| request.describe());
                    app.native_usage_model =
                        (!model.is_empty() && model != "?").then(|| model.clone());
                    app.native_usage_session = app.native_session_id.clone();
                    let session = app.native_session_id.clone();
                    let prior_prov = app.prior_provider;
                    let prior_home = app.prior_home.clone();
                    // This dispatch carries any pending switch; later turns resume the new session.
                    app.prior_provider = app.provider;
                    app.prior_home = owner
                        .home
                        .as_ref()
                        .map(|home| home.to_string_lossy().into_owned());
                    let pack_msgs = app.pack_messages();
                    // Drop the just-typed user message from pack (already in prompt).
                    let pack_msgs: Vec<_> = if pack_msgs
                        .last()
                        .map(|m| matches!(m.role, PackRole::User) && m.text == prompt)
                        .unwrap_or(false)
                    {
                        pack_msgs[..pack_msgs.len().saturating_sub(1)].to_vec()
                    } else {
                        pack_msgs
                    };
                    let native_sources = app.native_sources.clone();
                    let preparing_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    app.preparing_cancel = Some(preparing_cancel.clone());
                    let need_pack_hint = app.need_context_pack;
                    let thread_id = app.thread_id().map(str::to_string);
                    let done_tx = done_tx.clone();
                    let handle_tx = handle_tx.clone();
                    let progress_tx = progress_tx.clone();
                    app.need_context_pack = false;
                    app.live_cards.clear();
                    app.live_text.clear();
                    app.live_thinking.clear();
                    app.phase = Phase::Waiting;
                    tokio::spawn(async move {
                        // Native-reference hydration and document reads must not block input/paint.
                        // Expand only composer text, before appending document contents or PDF inputs.
                        let reference_root = state_root.clone();
                        let prepared = tokio::task::spawn_blocking(move || {
                            let mut incoming = incoming;
                            let ex = chatref::expand(
                                &reference_root,
                                &incoming.text,
                                crate::config::for_root(&reference_root).threads.map_chars,
                            );
                            incoming.text = ex.sent;
                            let prompt = if incoming.has_pdfs() {
                                incoming.validate().map(|()| incoming.encode())
                            } else {
                                incoming.prompt_text()
                            }?;
                            Ok::<_, String>((incoming, prompt, ex.maps, ex.notices))
                        })
                        .await
                        .map_err(|_| "chat reference preparation worker stopped".to_owned())
                        .and_then(|prepared| prepared);
                        let (incoming, prompt, maps, notices) = match prepared {
                            Ok(prepared) => prepared,
                            Err(error) => {
                                let _ = done_tx.send(TurnOutcome {
                                    failed: true,
                                    stderr_tail: error,
                                    ..Default::default()
                                });
                                return;
                            }
                        };
                        if !maps.is_empty() || !notices.is_empty() {
                            let _ =
                                progress_tx.send(TurnProgress::ChatReferences { maps, notices });
                        }
                        let home = launch_home;
                        let home_str = home.as_ref().map(|p| p.to_string_lossy().into_owned());
                        let binding = home.as_ref().and_then(|h| {
                            crate::native_memory::Binding::new(
                                provider,
                                &account,
                                h,
                                session.as_deref().unwrap_or("context-policy"),
                            )
                            .ok()
                        });
                        let policy = match binding
                            .as_ref()
                            .map(|b| crate::capacity::load(&state_root, b))
                            .transpose()
                        {
                            Ok(p) => p.unwrap_or_default(),
                            Err(error) => {
                                let _ = done_tx.send(TurnOutcome {
                                    stderr_tail: format!("invalid context policy: {error}"),
                                    ..Default::default()
                                });
                                return;
                            }
                        };
                        let native_input = match crate::native_input::for_home(
                            &state_root,
                            provider,
                            &owner.account,
                            owner.home.as_deref(),
                        ) {
                            Ok(mode) => mode,
                            Err(error) => {
                                let _ = done_tx.send(TurnOutcome {
                                    stderr_tail: format!(
                                        "invalid native-input preference: {error}"
                                    ),
                                    ..Default::default()
                                });
                                return;
                            }
                        };
                        let mut rollover = false;
                        if policy.rollover_percent.is_some() && session.is_some() {
                            if let Some(binding) = binding {
                                let root = state_root.clone();
                                let selected_model = model.clone();
                                let measured = tokio::task::spawn_blocking(move || {
                                    crate::native_memory::context_observation(
                                        &root,
                                        &binding,
                                        &selected_model,
                                    )
                                })
                                .await;
                                match measured {
                                    Ok(Ok(observation)) => {
                                        rollover = policy.rollover(observation.measurement);
                                        let _ = progress_tx.send(TurnProgress::Notice(
                                            observation.notice(rollover),
                                        ));
                                    }
                                    _ => {
                                        let _ = progress_tx.send(TurnProgress::Notice("native context measurement unavailable; proactive rollover skipped".into()));
                                    }
                                }
                            }
                        }
                        let need_pack_hint = need_pack_hint || rollover;
                        if rollover && native_sources.is_empty() {
                            let _ = done_tx.send(TurnOutcome { stderr_tail: "proactive rollover stopped: no original source binding is available".into(), ..Default::default() });
                            return;
                        }
                        // Pack only after an explicit switch (need_pack_hint). Planner clears
                        // session id on cross-home/provider; omits pack when native resume applies.
                        let decision = plan_continuity(&ContinuityRequest {
                            from_provider: prior_prov,
                            to_provider: provider,
                            from_home: prior_home.as_deref().or(home_str.as_deref()),
                            to_home: home_str.as_deref(),
                            session_id: if rollover { None } else { session.as_deref() },
                            messages: &pack_msgs,
                            thread_id: thread_id.as_deref(),
                            include_pack_if_needed: need_pack_hint,
                        });
                        let session_out = if decision.native_resume {
                            decision.session_id.clone().or_else(|| {
                                if rollover {
                                    None
                                } else {
                                    session.clone()
                                }
                            })
                        } else {
                            // Never leak a foreign / cross-home session id.
                            None
                        };
                        let mut prefix = if need_pack_hint {
                            decision.pack.as_ref().map(|p| p.text.clone())
                        } else {
                            None
                        };
                        if need_pack_hint && !decision.native_resume && !native_sources.is_empty() {
                            let memory_root = state_root.clone();
                            let indexed = tokio::task::spawn_blocking(move || {
                                crate::native_memory::handoff_prefix(&memory_root, &native_sources)
                            })
                            .await;
                            match indexed {
                                Ok((sources, notices)) => {
                                    if rollover && (sources.is_empty() || !notices.is_empty()) {
                                        let _ = done_tx.send(TurnOutcome { stderr_tail: "proactive rollover stopped: original native sources could not all be verified".into(), ..Default::default() });
                                        return;
                                    }
                                    for notice in notices {
                                        let _ = progress_tx.send(TurnProgress::Notice(notice));
                                    }
                                    if !sources.is_empty() {
                                        prefix
                                            .get_or_insert_with(String::new)
                                            .push_str(&format!("\n{sources}"));
                                    }
                                }
                                Err(error) => {
                                    if rollover {
                                        let _ = done_tx.send(TurnOutcome {
                                            stderr_tail: format!(
                                                "proactive source preparation failed: {error}"
                                            ),
                                            ..Default::default()
                                        });
                                        return;
                                    }
                                    let _ = progress_tx.send(TurnProgress::Notice(format!(
                                        "native source indexing unavailable: {error}"
                                    )));
                                }
                            }
                        }
                        if preparing_cancel.load(std::sync::atomic::Ordering::SeqCst) {
                            let _ = done_tx.send(TurnOutcome {
                                cancelled: true,
                                ..Default::default()
                            });
                            return;
                        }
                        if let Some(scope) = scope_intent {
                            prefix.get_or_insert_with(String::new).push_str(&format!("\n[User-declared cooperative workspace scope]\n{scope}\nFollow these declared paths. Scheduler reservations grant no additional native permissions."));
                        }
                        let req = ProviderTurnRequest {
                            images: &image_inputs,
                            provider,
                            prompt: &prompt,
                            cwd: &cwd,
                            home_path: home.as_deref(),
                            model: if model == "?" {
                                None
                            } else {
                                Some(model.as_str())
                            },
                            native_session_id: session_out.as_deref(),
                            trust,
                            context_prefix: prefix.as_deref(),
                            local_time: Some(crate::prompt_time::now()),
                            compaction: policy.compaction,
                        };
                        // Validate plan even if spawn fails (argv safety).
                        let checked_plan = match plan_provider_turn(&req) {
                            Ok(plan) => plan,
                            Err(e) => {
                                let _ = done_tx.send(TurnOutcome {
                                    stderr_tail: format!("{e:#}"),
                                    ..Default::default()
                                });
                                return;
                            }
                        };
                        let original =
                            crate::attachments::Message::decode(&original_reference_prompt);
                        let raw_reference_prompt = if incoming.has_pdfs() {
                            crate::attachments::Message::decode(&prompt).text
                        } else {
                            prompt.clone()
                        };
                        let original_range = reference_journal::original_range(
                            &checked_plan.prompt,
                            &raw_reference_prompt,
                            &original.text,
                        );
                        let capture_root = state_root.clone();
                        let capture_account = account.clone();
                        let capture_home = home.clone();
                        let capture_session = session_out.clone();
                        let capture_scope = tokio::task::spawn_blocking(move || {
                            let Some(session) = capture_session else {
                                return Ok(None);
                            };
                            let home = capture_home
                                .as_deref()
                                .ok_or("native history home unavailable")?;
                            let binding = crate::native_memory::Binding::new(
                                provider,
                                &capture_account,
                                home,
                                &session,
                            )?;
                            crate::native_memory::references::begin_scope(&capture_root, &binding)
                                .map(Some)
                        })
                        .await
                        .unwrap_or_else(|_| Err("native history capture worker stopped".into()));
                        let reference_run = owner.run_id.clone();
                        let _ = reference_sender.send(reference_journal::Started {
                            run: reference_run.clone(),
                            scope: capture_scope.clone(),
                            input: checked_plan.prompt.clone(),
                            original: original_range.clone(),
                        });
                        // Admission is held before planning, but an input/launch failure is not
                        // evidence of a live process. Mark invocation only when transport reports
                        // its owned child, forwarding cancellation ownership before completion.
                        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
                        let forward_started = |handle: Arc<RunningTurn>| {
                            scope_invoked.store(true, std::sync::atomic::Ordering::SeqCst);
                            if handle_tx.send(handle.clone()).is_err() {
                                handle.cancel();
                            }
                        };
                        let turn = run_provider_turn_with_transport(
                            &req,
                            owner,
                            native_input,
                            Some(started_tx),
                            Some(progress_tx),
                        );
                        tokio::pin!(turn);
                        let result = loop {
                            tokio::select! {
                                biased;
                                Some(handle) = started_rx.recv() => forward_started(handle),
                                result = &mut turn => {
                                    while let Ok(handle) = started_rx.try_recv() {
                                        forward_started(handle);
                                    }
                                    break result;
                                }
                            }
                        };
                        match result {
                            Ok(outcome) => {
                                let seal_root = state_root.clone();
                                let seal_home = home.clone();
                                let seal_account = account.clone();
                                let seal_session = outcome.session_id.clone();
                                let sealed = tokio::task::spawn_blocking(move || {
                                    let scope = match capture_scope {
                                        Ok(Some(scope)) => scope,
                                        Ok(None) => {
                                            let home = seal_home
                                                .as_deref()
                                                .ok_or("native history home unavailable")?;
                                            let session = seal_session
                                                .as_deref()
                                                .ok_or("native history session unavailable")?;
                                            let binding = crate::native_memory::Binding::new(
                                                provider,
                                                &seal_account,
                                                home,
                                                session,
                                            )?;
                                            crate::native_memory::references::fresh_scope(
                                                &seal_root, &binding,
                                            )?
                                        }
                                        Err(error) => return Err(error),
                                    };
                                    crate::native_memory::references::seal_scope(&seal_root, &scope)
                                        .map(Some)
                                })
                                .await
                                .unwrap_or_else(|_| {
                                    Err("native history sealing worker stopped".into())
                                });
                                let _ = reference_sender.send(reference_journal::Started {
                                    run: reference_run,
                                    scope: sealed,
                                    input: checked_plan.prompt,
                                    original: original_range,
                                });
                                let _ = done_tx.send(outcome);
                            }
                            Err(e) => {
                                let _ = done_tx.send(TurnOutcome {
                                    stderr_tail: format!("{e:#}"),
                                    ..Default::default()
                                });
                            }
                        }
                    });
                }

                app.poll_delegates();
                app.flush_delegate_prompt(turn_tx);
                while let Ok(ev) = progress_rx.try_recv() {
                    app.apply_progress(ev);
                    dirty = true;
                }

                while let Ok(outcome) = done_rx.try_recv() {
                    // Progress is sent before completion, but may arrive between the earlier
                    // drain and this receive. Finish it before finalizing or switching sessions.
                    while let Ok(ev) = progress_rx.try_recv() {
                        app.apply_progress(ev);
                        dirty = true;
                    }
                    app.on_turn_done(outcome, turn_tx);
                    dirty = true;
                }

                if tick_due {
                    let tick_changed = app.on_tick();
                    // Running turns expose both elapsed time and an optional spinner.  The idle
                    // notice also changes on ticks; motion-off deliberately suppresses spinner
                    // cadence, while elapsed/status updates remain observable.
                    dirty |= tick_changed;
                }
            }
            if tick_due {
                last_tick = Instant::now();
            }

            if tabs
                .tabs
                .iter()
                .any(|t| t.app.should_quit || t.app.reexec_path.is_some())
            {
                break;
            }

            // Paint state changes handled above before waiting for the next input/tick. This is
            // intentionally after the event and channel drains: a key burst may defer one frame,
            // but must never defer the resulting frame until the next poll timeout.
            if !redraw_always {
                let app = &mut tabs.tabs[tabs.active].app;
                draw_if_needed!(app, false);
            }
        }
        Ok(())
    })();

    let mut reexec = None;
    for tab in tabs.tabs.iter_mut() {
        tab.shut_down();
        let _ = tab.app.workspace.flush_file_recovery();
        reexec = reexec.or(tab.app.reexec_path.take());
    }
    let cwd = tabs.active().app.cwd.clone();
    let _ = std::panic::take_hook(); // drop our hook; default behavior resumes
    drop(terminal_input);
    // Clear only on normal shutdown. The panic hook has already returned to the
    // primary screen to print diagnostics; unwinding must preserve those messages.
    if !use_window {
        let _ = terminal.clear();
        let _ = execute!(terminal.backend_mut(), crossterm::cursor::MoveTo(0, 0));
        if let Some(restore) = &terminal_restore {
            restore.restore();
        }
        terminal.show_cursor()?;
    }
    drop(terminal);
    input_result?;
    if let Some(path) = reexec {
        let code = crate::update::reexec_into_mode(&path, &cwd, !use_window)?;
        std::process::exit(code);
    }
    Ok(())
}

/// Format panic diagnostics without copying arbitrary panic payloads into the log.
pub(crate) fn format_panic_log_line(
    payload_value: &(dyn std::any::Any + Send),
    location_value: Option<&std::panic::Location<'_>>,
) -> String {
    let payload = if payload_value.downcast_ref::<&str>().is_some() {
        ("&str", payload_value.downcast_ref::<&str>().copied())
    } else if let Some(message) = payload_value.downcast_ref::<String>() {
        ("String", Some(message.as_str()))
    } else {
        ("other", None)
    };
    let category = payload.1.map_or("panic", |message| {
        if message.contains("byte index") && message.contains("char boundary") {
            "char-boundary slice"
        } else {
            "panic"
        }
    });
    let location = location_value
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_else(|| "<unknown>".into());
    format!(
        "panic location={location} payload_type={} category={category}",
        payload.0
    )
}

/// Restores terminal modes on normal exit and unwinding.
struct TerminalRestoreGuard(Arc<std::sync::atomic::AtomicBool>);

impl TerminalRestoreGuard {
    fn new() -> Self {
        Self(Arc::new(std::sync::atomic::AtomicBool::new(true)))
    }

    fn restore(&self) {
        if self.0.swap(false, std::sync::atomic::Ordering::AcqRel) {
            restore_terminal();
        }
    }
}

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

fn restore_terminal() {
    let mut out = stdout();
    // Each command is independent. In particular, keyboard enhancement is unsupported on
    // Windows; batching it with screen restoration aborts cleanup before leaving the TUI.
    let _ = execute!(out, EndSynchronizedUpdate);
    let _ = execute!(out, PopKeyboardEnhancementFlags);
    let _ = execute!(out, DisableBracketedPaste);
    let _ = execute!(out, DisableFocusChange);
    let _ = execute!(out, DisableMouseCapture);
    let _ = pointer::reset(&mut out);
    let _ = execute!(out, crossterm::style::ResetColor);
    let _ = execute!(
        out,
        crossterm::style::SetAttribute(crossterm::style::Attribute::Reset)
    );
    let _ = execute!(out, LeaveAlternateScreen);
    let _ = execute!(out, crossterm::cursor::Show);
    let _ = disable_raw_mode();
}

pub(super) fn write_osc_title(title: &str) -> io::Result<()> {
    let clean: String = title.chars().filter(|c| !c.is_control()).collect();
    execute!(stdout(), crossterm::terminal::SetTitle(clean))
}

#[cfg(test)]
mod redraw_tests {
    #[derive(Default)]
    struct RedrawHarness {
        dirty: bool,
        draws: usize,
    }

    impl RedrawHarness {
        fn draw_if_dirty(&mut self) {
            if self.dirty {
                self.draws += 1;
                self.dirty = false;
            }
        }
        fn input(&mut self) {
            self.dirty = true;
        }
        fn resize(&mut self) {
            self.dirty = true;
        }
        fn tick(&mut self, running: bool, motion: bool) {
            self.dirty |= running && motion;
        }

        fn handle_event_then_draw(&mut self) {
            self.input();
            self.draw_if_dirty();
        }
    }

    #[test]
    fn idle_screen_does_not_redraw() {
        let mut h = RedrawHarness {
            dirty: true,
            ..Default::default()
        };
        h.draw_if_dirty();
        for _ in 0..25 {
            h.draw_if_dirty();
        }
        assert_eq!(h.draws, 1);
    }

    #[test]
    fn input_triggers_exactly_one_redraw() {
        let mut h = RedrawHarness {
            dirty: true,
            ..Default::default()
        };
        h.draw_if_dirty();
        h.input();
        h.draw_if_dirty();
        h.draw_if_dirty();
        assert_eq!(h.draws, 2);
    }

    #[test]
    fn handled_event_draws_before_the_next_wait_iteration() {
        let mut h = RedrawHarness {
            dirty: true,
            ..Default::default()
        };
        h.draw_if_dirty();
        h.handle_event_then_draw();
        assert_eq!(h.draws, 2);
        assert!(
            !h.dirty,
            "the handled event was painted in its own iteration"
        );
    }

    #[test]
    fn running_turn_redraws_at_spinner_cadence_only() {
        let mut h = RedrawHarness {
            dirty: true,
            ..Default::default()
        };
        h.draw_if_dirty();
        for _ in 0..5 {
            h.tick(true, true);
            h.draw_if_dirty();
        }
        assert_eq!(h.draws, 6);
    }

    #[test]
    fn motion_off_has_no_spinner_redraws() {
        let mut h = RedrawHarness {
            dirty: true,
            ..Default::default()
        };
        h.draw_if_dirty();
        for _ in 0..25 {
            h.tick(true, false);
            h.draw_if_dirty();
        }
        assert_eq!(h.draws, 1);
    }

    #[test]
    fn resize_redraws() {
        let mut h = RedrawHarness {
            dirty: true,
            ..Default::default()
        };
        h.draw_if_dirty();
        h.resize();
        h.draw_if_dirty();
        assert_eq!(h.draws, 2);
    }
}

#[cfg(test)]
mod panic_log_tests {
    #[test]
    fn panic_log_line_never_contains_payload_text() {
        let secret = "SECRET-PANIC-PAYLOAD";
        let payload = secret.to_string();
        let line = super::format_panic_log_line(&payload, Some(std::panic::Location::caller()));
        assert!(!line.contains(secret));
        assert!(line.contains("location="));
    }
}
