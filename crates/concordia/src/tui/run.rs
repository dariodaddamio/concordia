use super::*;
#[path = "file_editor.rs"]
mod file_editor;
#[path = "window_session.rs"]
mod window_session;
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
    chat_lease: Option<chat_ownership::Lease>,
    conversation_lease: Option<chat_ownership::Lease>,
    id: uuid::Uuid,
    home_origin: Option<uuid::Uuid>,
    app: App,
    turn_tx: mpsc::UnboundedSender<String>,
    turn_rx: mpsc::UnboundedReceiver<String>,
    done_tx: mpsc::UnboundedSender<TurnOutcome>,
    done_rx: mpsc::UnboundedReceiver<TurnOutcome>,
    pending_done: Option<TurnOutcome>,
    handle_tx: mpsc::UnboundedSender<Arc<RunningTurn>>,
    handle_rx: mpsc::UnboundedReceiver<Arc<RunningTurn>>,
    progress_tx: mpsc::Sender<TurnProgress>,
    progress_rx: mpsc::Receiver<TurnProgress>,
}

impl RunTab {
    fn new(app: App) -> Self {
        let (turn_tx, turn_rx) = mpsc::unbounded_channel();
        let (done_tx, done_rx) = mpsc::unbounded_channel();
        let (handle_tx, handle_rx) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::channel(crate::provider::turn::PROGRESS_CAPACITY);
        Self {
            chat_lease: None,
            conversation_lease: None,
            id: uuid::Uuid::new_v4(),
            home_origin: None,
            app,
            turn_tx,
            turn_rx,
            done_tx,
            done_rx,
            pending_done: None,
            handle_tx,
            handle_rx,
            progress_tx,
            progress_rx,
        }
    }

    /// Persist the draft and stop anything still running; used on close and on exit.
    fn shut_down(&mut self) {
        self.app.local_terminal.stop();
        // A closed tab can be reopened. Retain a pending completion and queued
        // progress together; finalizing here could dispatch another queued turn.
        // On exit, the unfinished checkpoint and native history remain available.
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
        self.chat_lease = None;
        self.conversation_lease = None;
    }

    fn reopen_handle_receiver(&mut self) {
        // A retired turn cannot publish handles into the replacement receiver.
        let (sender, receiver) = mpsc::unbounded_channel();
        self.handle_tx = sender;
        self.handle_rx = receiver;
    }

    fn acquire_chat_ownership(&mut self) -> Result<(), String> {
        let conversation = chat_ownership::Key::conversation(&self.app);
        let next_conversation =
            if self.conversation_lease.as_ref().map(|lease| lease.key()) != Some(&conversation) {
                Some(chat_ownership::Lease::acquire(
                    &self.app.state_root,
                    &conversation,
                )?)
            } else {
                None
            };
        let key = chat_ownership::Key::for_app(&self.app);
        if self.app.native_session_id.is_some() && key.is_none() {
            return Err("The chat's exact native account/home identity is unavailable.".into());
        }
        let changed = self.chat_lease.as_ref().map(|lease| lease.key()) != key.as_ref();
        let next_native = if changed {
            key.as_ref()
                .map(|key| chat_ownership::Lease::acquire(&self.app.state_root, key))
                .transpose()?
        } else {
            None
        };
        if let Some(lease) = next_conversation {
            self.conversation_lease = Some(lease);
        }
        if changed {
            self.chat_lease = next_native;
        }
        Ok(())
    }

    fn refresh_chat_ownership(&mut self) -> bool {
        let error = self.acquire_chat_ownership().err();
        if error == self.app.chat_ownership_error {
            return false;
        }
        if let Some(message) = &error {
            if self.app.busy
                && (self.app.cancel_handle.is_some() || self.app.preparing_cancel.is_some())
            {
                self.app.request_cancel();
            }
            self.app
                .push_system(format!("{message} Sending is blocked; saved work is kept."));
        }
        self.app.chat_ownership_error = error;
        true
    }
}

impl tabs::TabState for RunTab {
    fn needs_close_confirmation(&self) -> bool {
        tabs::TabState::needs_close_confirmation(&self.app)
    }

    fn can_evict_closed(&self) -> bool {
        self.pending_done.is_none()
            && self.progress_rx.is_empty()
            && self.done_rx.is_empty()
            && self.handle_rx.is_empty()
            && self.turn_rx.is_empty()
            && tabs::TabState::can_evict_closed(&self.app)
    }
}

fn new_tab_app(state_root: &Path, cwd: &Path, use_window: bool) -> App {
    let mut app = App::new(state_root.to_path_buf(), cwd.to_path_buf());
    app.refresh_binding();
    // App::new already owns the tab's recovery lease. Retrying it synchronously here forced a
    // durable draft write for every Ctrl+T, even though a new Home tab has no draft to recover.
    // The normal persist tick still retries a real dirty lease when the tab is used.
    app.prior_provider = app.provider;
    app.prior_home = app.continuity_home();
    app.workspace.set_native_windows(use_window);
    app
}

fn navigate_home(tabs: &mut tabs::TabSet<RunTab>, state_root: &Path, cwd: &Path, use_window: bool) {
    let current = &tabs.active().app;
    if landing::can_host_home(current)
        && (current.landing.is_some()
            || (!tabs::TabState::needs_close_confirmation(current)
                && current.composer_images().count() == 0
                && !current.composer_images().importing()))
    {
        landing::open_home_tab(&mut tabs.active_mut().app);
        return;
    }
    let origin = tabs.active().id;
    if let Some(index) = tabs
        .tabs
        .iter()
        .position(|tab| tab.app.landing.is_some() && landing::can_host_home(&tab.app))
    {
        tabs.active = index;
    } else {
        tabs.add(RunTab::new(new_tab_app(state_root, cwd, use_window)));
    }
    landing::open_home_tab(&mut tabs.active_mut().app);
    tabs.active_mut().home_origin = Some(origin);
}

fn return_from_home(tabs: &mut tabs::TabSet<RunTab>) {
    let origin = tabs.active_mut().home_origin.take();
    if let Some(index) = origin.and_then(|id| tabs.tabs.iter().position(|tab| tab.id == id)) {
        tabs.active = index;
    } else {
        landing::close(&mut tabs.active_mut().app);
    }
}

fn matches_resume(app: &App, request: &session_ui::ResumeRequest) -> bool {
    if chat_ownership::Key::for_app(app).as_ref() == Some(&request.key) {
        return true;
    }
    // A task can span several native sessions, but never use its name/id to cross accounts.
    request.task.as_ref().is_some_and(|task| {
        app.task_id.as_ref() == Some(task)
            && app.account == request.account
            && get_account(&app.state_root, &request.account).is_some_and(|account| {
                account.provider == app.provider
                    && account.home_path.as_ref().zip(app.home_path.as_ref()).is_some_and(
                        |(registered, home)| {
                            matches!((std::fs::canonicalize(registered), std::fs::canonicalize(home)),
                                (Ok(left), Ok(right)) if left == right)
                        },
                    )
            })
    })
}

fn show_existing_chat(app: &mut App) {
    if !landing::editing_goal(app) {
        landing::close(app);
    }
    app.flash("This chat is already open; switched to its owning tab.");
}

fn resume_in_tabs(
    tabs: &mut tabs::TabSet<RunTab>,
    request: session_ui::ResumeRequest,
    state_root: &Path,
    native: bool,
) {
    match chat_ownership::Key::for_resume(state_root, &request.account, &request.session) {
        Ok(current) if current == request.key => {}
        _ => {
            tabs.active_mut().app.push_system("resume unavailable: the selected account/home changed; existing chats and drafts were kept");
            return;
        }
    }
    if let Some(index) = tabs
        .tabs
        .iter()
        .position(|tab| matches_resume(&tab.app, &request))
    {
        tabs.active = index;
        show_existing_chat(&mut tabs.active_mut().app);
        return;
    }

    if let Err(error) = chat_ownership::Lease::check_resume_writer(state_root, &request.key) {
        tabs.active_mut().app.push_system(format!(
            "Cannot open this chat yet: {error}. Existing drafts were kept."
        ));
        return;
    }
    if let Some(index) = tabs
        .closed
        .iter()
        .position(|tab| matches_resume(&tab.app, &request))
    {
        if let Err(error) = tabs.closed[index].acquire_chat_ownership() {
            tabs.active_mut().app.push_system(error);
            return;
        }
        let mut tab = tabs.closed.remove(index);
        tab.reopen_handle_receiver();
        show_existing_chat(&mut tab.app);
        tabs.add(tab);
        return;
    }

    let native_lease = match chat_ownership::Lease::acquire(state_root, &request.key) {
        Ok(lease) => lease,
        Err(error) => {
            tabs.active_mut().app.push_system(error);
            return;
        }
    };
    let app = &tabs.active().app;
    let has_work = tabs::TabState::needs_close_confirmation(app)
        || app.native_session_id.is_some()
        || !app.native_sources.is_empty()
        || app.task_id.is_some()
        || app.keep.turn_recovery_notice
        || app.continuation.waiting()
        || app.session_ui.loading()
        || app.messages.iter().any(|message| {
            matches!(
                message.role,
                MsgRole::User | MsgRole::Assistant | MsgRole::Card | MsgRole::Thinking
            )
        });
    if has_work {
        let workspace = tabs.active().app.cwd.clone();
        let mut app = new_tab_app(state_root, &workspace, native);
        if app.resume_session(&request.account, &request.session) {
            app.task_id = request.task;
            let mut tab = RunTab::new(app);
            tab.chat_lease = Some(native_lease);
            if let Err(error) = tab.acquire_chat_ownership() {
                tabs.active_mut().app.push_system(error);
                return;
            }
            tabs.add(tab);
        } else {
            tabs.active_mut().app.push_system(
                "Could not open the selected chat; the current conversation and draft were kept.",
            );
        }
    } else {
        let tab = tabs.active_mut();
        if tab.app.resume_session(&request.account, &request.session) {
            tab.app.task_id = request.task;
            tab.chat_lease = Some(native_lease);
            if let Err(error) = tab.acquire_chat_ownership() {
                tab.app.chat_ownership_error = Some(error.clone());
                tab.app.push_system(error);
            }
        }
    }
}

#[cfg(test)]
pub(super) fn inspect_requested_resume(mut app: App, inspect: impl FnOnce(&mut App)) {
    let request = app
        .session_ui
        .resume_request
        .take()
        .expect("queued resume request");
    let root = app.state_root.clone();
    let mut tabs = tabs::TabSet::new(RunTab::new(app));
    resume_in_tabs(&mut tabs, request, &root, false);
    inspect(&mut tabs.active_mut().app);
    assert!(
        tabs.tabs
            .iter_mut()
            .all(|tab| tab.turn_rx.try_recv().is_err()),
        "history navigation must not dispatch a model turn"
    );
}

/// Apply the shared confirmation/close path after the target tab is active.
fn confirm_and_close_tab(
    tabs: &mut tabs::TabSet<RunTab>,
    close_armed: &mut Option<(uuid::Uuid, Instant)>,
    prompt: &str,
) {
    let owner = tabs.active().id;
    let confirmed =
        close_armed.is_some_and(|(id, at)| id == owner && at.elapsed() < Duration::from_secs(3));
    if tabs.needs_close_confirmation() && !confirmed {
        *close_armed = Some((owner, Instant::now()));
        tabs.active_mut().app.flash(prompt);
        return;
    }
    tabs.active_mut().shut_down();
    tabs.close(true);
    *close_armed = None;
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

    let (mut saved_window, mut tabs) = window_session::open(&state_root, &cwd, use_window);
    tabs.active_mut().app.compat_rx = Some(crate::provider::compat::start_real_detection(
        state_root.clone(),
    ));

    let terminal_restore = (!use_window).then(TerminalRestoreGuard::new);
    let (backend, mut terminal_input) = if use_window {
        #[cfg(windows)]
        {
            let (backend, events) = native_window::open()?;
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
    let mut diagram_revision = diagrams::revision();
    let redraw_always = std::env::var_os("CONCORDIA_REDRAW_ALWAYS").is_some();

    let mut close_armed: Option<(uuid::Uuid, Instant)> = None;
    let mut previous_labels: Vec<String> = Vec::new();
    let mut tab_click: Option<(usize, u16, u16, Instant)> = None;

    macro_rules! draw_if_needed {
        ($app:ident, $force:expr) => {
            if $force || (dirty && (!key_burst.active() || !terminal_input.poll(Duration::ZERO)?)) {
                let app = &mut *$app;
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
                dirty = false;
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
            for tab in tabs.tabs.iter_mut() {
                dirty |= tab.refresh_chat_ownership();
            }
            let revision = diagrams::revision();
            dirty |= revision != diagram_revision;
            diagram_revision = revision;
            let account_changes_blocked = tabs.tabs.iter().chain(&tabs.closed).any(|tab| {
                tab.app.busy
                    || tab.app.preparing_cancel.is_some()
                    || tab.app.cancel_handle.is_some()
                    || tab.app.deleg.d.any_active()
                    || tab.pending_done.is_some()
                    || !tab.turn_rx.is_empty()
                    || !tab.handle_rx.is_empty()
                    || !tab.done_rx.is_empty()
            });
            for tab in &mut tabs.tabs {
                tab.app.account_changes_blocked = account_changes_blocked;
            }
            #[cfg(windows)]
            if let Some(windows) = &mut detached_windows {
                let owners: Vec<_> = tabs.tabs.iter().map(|tab| tab.id).collect();
                windows.retain_tabs(&owners);
                dirty |= windows.drain(|owner, id, event| {
                    let Some(tab) = tabs.tabs.iter_mut().find(|tab| tab.id == owner) else {
                        return;
                    };
                    if !tab.app.workspace.detached_ids().contains(&id) {
                        return;
                    }
                    if let Some(event) = event {
                        workspace::handle_window_event(&mut tab.app, id, event);
                    } else {
                        tab.app.workspace.request_dock(id);
                    }
                });
                for tab in &mut tabs.tabs {
                    windows.sync(tab.id, &mut tab.app);
                    windows.draw(tab.id, &mut tab.app);
                }
            }
            let active = tabs.active;
            #[cfg(windows)]
            if use_window {
                let mut needs_attention = false;
                let mut working = false;
                for tab in &mut tabs.tabs {
                    tab.app.sync_needs_you();
                    needs_attention |= !tab.app.needs_you.is_empty();
                    working |= tab.app.busy || tab.app.deleg.d.any_active();
                }
                terminal.backend_mut().status_dot(if needs_attention {
                    super::native_window::StatusDot::NeedsAttention
                } else if working {
                    super::native_window::StatusDot::Working
                } else {
                    super::native_window::StatusDot::Idle
                });
            }
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
            let mut rename_tab = None;
            {
                let RunTab { app, turn_tx, .. } = &mut tabs.tabs[active];
                app.tab_strip = labels;
                app.tab_index = active;
                // Drain pending paste chunks before repainting the growing prompt.
                draw_if_needed!(app, redraw_always);
                let title = app.window_title();
                if use_window {
                    let overlays = app
                        .picker
                        .as_ref()
                        .map(|picker| picker.title.clone())
                        .into_iter()
                        .chain(
                            app.workspace
                                .focused
                                .as_ref()
                                .map(|id| format!("workspace:{id}")),
                        )
                        .collect();
                    terminal.backend_mut().test_state(app.tab_label(), overlays);
                }
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
                            tab_click = None;
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
                                    if app.tab_close_hits.get(index).is_some_and(|r| {
                                        r.contains(ratatui::layout::Position::new(m.column, m.row))
                                    }) {
                                        tab_click = None;
                                        tab_action = tabs::TabAction::CloseAt(index);
                                    } else {
                                        #[cfg(windows)]
                                        let interval = Duration::from_millis(u64::from(unsafe {
                                            windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime()
                                        }));
                                        #[cfg(not(windows))]
                                        let interval = MULTI_CLICK;
                                        if tab_click.is_some_and(|(previous, x, y, at)| {
                                            previous == index
                                                && x.abs_diff(m.column) <= 1
                                                && y == m.row
                                                && captured_at.saturating_duration_since(at)
                                                    <= interval
                                        }) {
                                            rename_tab = Some(index);
                                            tab_click = None;
                                        } else {
                                            tab_click = Some((index, m.column, m.row, captured_at));
                                        }
                                        tab_action = tabs::TabAction::Select(index + 1);
                                    }
                                    break;
                                }
                                tab_click = None;
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
                                if app.local_terminal.visible() {
                                    app.local_terminal.paste(&s);
                                } else if app.rename_editor.is_some() {
                                    app.paste_text(&s);
                                } else if !questions::paste(&mut *app, &s) {
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
                        // Apply navigation before draining the next event: it belongs to
                        // the destination tab, not the tab that requested the switch.
                        if app.tab_request.is_some()
                            || app.session_ui.resume_request.is_some()
                            || !terminal_input.poll(Duration::ZERO)?
                        {
                            break;
                        }
                    }
                    if quit {
                        break;
                    }
                }
            }

            if let Some(request) = tabs.active_mut().app.tab_request.take() {
                tab_action = request;
            }
            let resume_request = tabs.active_mut().app.session_ui.resume_request.take();
            if !matches!(
                tab_action,
                tabs::TabAction::Select(_) | tabs::TabAction::Unhandled
            ) {
                tab_click = None;
            }
            match tab_action {
                tabs::TabAction::Unhandled => {}
                tabs::TabAction::New => {
                    let mut app = new_tab_app(&state_root, &cwd, use_window);
                    landing::open_home_tab(&mut app);
                    tabs.add(RunTab::new(app));
                    close_armed = None;
                    dirty = true;
                }
                tabs::TabAction::Home => {
                    navigate_home(&mut tabs, &state_root, &cwd, use_window);
                    close_armed = None;
                    dirty = true;
                }
                tabs::TabAction::BackToChat => {
                    return_from_home(&mut tabs);
                    close_armed = None;
                    dirty = true;
                }
                tabs::TabAction::Close if tabs.has_multiple() => {
                    confirm_and_close_tab(
                        &mut tabs,
                        &mut close_armed,
                        "This tab has a running turn or an unsent draft. Ctrl+W again closes it.",
                    );
                    dirty = true;
                }
                tabs::TabAction::Close => {
                    tabs.active_mut().app.flash("This is the last tab.");
                    dirty = true;
                }
                tabs::TabAction::CloseAt(index) if tabs.has_multiple() => {
                    tabs.select_number(index + 1);
                    confirm_and_close_tab(
                        &mut tabs,
                        &mut close_armed,
                        "This tab has a running turn or unsent draft. Click \u{00d7} again to close.",
                    );
                    dirty = true;
                }
                tabs::TabAction::CloseAt(_) => {
                    tabs.active_mut().app.flash("This is the last tab.");
                    dirty = true;
                }
                tabs::TabAction::Reopen => {
                    let existing = tabs.closed.last().and_then(|closed| {
                        let key = chat_ownership::Key::for_app(&closed.app);
                        tabs.tabs.iter().position(|open| {
                            open.app.deleg.d.conversation_id()
                                == closed.app.deleg.d.conversation_id()
                                || key.as_ref().is_some_and(|key| {
                                    chat_ownership::Key::for_app(&open.app).as_ref() == Some(key)
                                })
                        })
                    });
                    if let Some(index) = existing {
                        tabs.active = index;
                        show_existing_chat(&mut tabs.active_mut().app);
                    } else if let Some(error) = tabs
                        .closed
                        .last_mut()
                        .and_then(|tab| tab.acquire_chat_ownership().err())
                    {
                        tabs.active_mut().app.push_system(error);
                    } else if tabs.reopen() {
                        // Shutdown closes the previous receiver so late starts from
                        // that turn are cancelled. Future turns need their own channel.
                        tabs.active_mut().reopen_handle_receiver();
                    } else {
                        tabs.active_mut().app.flash("No closed tab to reopen.");
                    }
                    dirty = true;
                }
                tabs::TabAction::Next => tabs.next(false),
                tabs::TabAction::Previous => tabs.next(true),
                tabs::TabAction::Select(n) => tabs.select_number(n),
            }
            if let Some(request) = resume_request {
                resume_in_tabs(&mut tabs, request, &state_root, use_window);
                close_armed = None;
                tab_click = None;
                dirty = true;
            }
            if rename_tab == Some(tabs.active) {
                let RunTab { app, turn_tx, .. } = tabs.active_mut();
                local_terminal::command(app, "hide");
                handle_key(
                    app,
                    KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE),
                    turn_tx,
                );
                dirty = true;
            }
            if tabs.active != active {
                for (index, tab) in tabs.tabs.iter_mut().enumerate() {
                    if index != tabs.active {
                        tab.app.rename_editor = None;
                    }
                }
                if close_armed.is_some_and(|(owner, _)| owner != tabs.active().id) {
                    close_armed = None;
                }
                dirty = true;
            }

            let tick_due = last_tick.elapsed() >= tick_rate;
            for tab in tabs.tabs.iter_mut() {
                dirty |= tab.refresh_chat_ownership();
                let RunTab {
                    app,
                    turn_tx,
                    turn_rx,
                    done_tx,
                    done_rx,
                    pending_done,
                    handle_tx,
                    handle_rx,
                    progress_tx,
                    progress_rx,
                    ..
                } = tab;
                if app.worker_panel_explicit.is_none() && app.deleg.d.any_active() {
                    app.worker_panel_open = true;
                }
                while let Ok(h) = handle_rx.try_recv() {
                    app.accept_cancel_handle(h);
                    dirty = true;
                }

                // A finished turn can wait here for an inline queue edit to be committed.
                app.queue_advance(turn_tx);
                while let Ok(prompt) = turn_rx.try_recv() {
                    dirty = true;
                    if let Some(error) = app.chat_ownership_error.clone() {
                        app.queue.items.insert(0, prompt);
                        app.pause_queue();
                        app.queue_changed();
                        app.submitted_prompt = None;
                        app.busy = false;
                        app.keep.turn_recovery_notice = true;
                        app.push_system(format!("Request kept in the paused queue: {error}"));
                        continue;
                    }
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
                    let repair_map = app.pending_repair_map.take();
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
                            let _ = progress_tx
                                .send(TurnProgress::ChatReferences { maps, notices })
                                .await;
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
                        let mut policy = match binding
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
                        if crate::config::for_root(&state_root)
                            .continuation
                            .proactive_rollover
                            && provider == crate::store::ProviderKind::Codex
                        {
                            policy.rollover_percent = Some(85);
                        }
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
                                        let _ = progress_tx
                                            .send(TurnProgress::Notice(
                                                observation.notice(rollover),
                                            ))
                                            .await;
                                    }
                                    _ => {
                                        let _ = progress_tx.send(TurnProgress::Notice("native context measurement unavailable; proactive rollover skipped".into())).await;
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
                        if let Some(map) = repair_map {
                            prefix = Some(match prefix {
                                Some(prefix) => format!("{prefix}\n\n[repair handoff map]\n{map}"),
                                None => format!("[repair handoff map]\n{map}"),
                            });
                        }
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
                                        let _ =
                                            progress_tx.send(TurnProgress::Notice(notice)).await;
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
                                    let _ = progress_tx
                                        .send(TurnProgress::Notice(format!(
                                            "native source indexing unavailable: {error}"
                                        )))
                                        .await;
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
                        if let Some(context) = prefix.as_ref().filter(|text| !text.is_empty()) {
                            let _ = progress_tx.send(TurnProgress::Notice(format!(
                                "Context for this message · {}\nConversation references are included for the selected provider. Native tool and approval state stays with its original session.\n\n{context}",
                                account
                            ))).await;
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
                            local_time: crate::config::for_root(&state_root)
                                .ui
                                .time_header
                                .then(crate::prompt_time::now),
                            compaction: Default::default(),
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

                dirty |= app.poll_delegates();
                app.flush_delegate_prompt(turn_tx);
                // A fast producer must yield to input and painting. Keep every event,
                // including approvals; this is a consumer scheduling budget, not a cap
                // on the provider's wire traffic or retained history.
                let drain_started = Instant::now();
                for _ in 0..256 {
                    let Ok(ev) = progress_rx.try_recv() else {
                        break;
                    };
                    app.apply_progress(ev);
                    dirty = true;
                    if drain_started.elapsed() >= Duration::from_millis(8) {
                        break;
                    }
                }

                if pending_done.is_none() {
                    *pending_done = done_rx.try_recv().ok();
                }
                // Progress is sent before completion. A completion received after a
                // partial drain waits for the remaining events, across frames, before
                // finalization can switch sessions or start the next queued turn.
                if pending_done.is_some() && progress_rx.is_empty() {
                    app.on_turn_done(pending_done.take().unwrap(), turn_tx);
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

            if let Some(saved) = &mut saved_window {
                if let Err(error) = saved.save(&tabs, false) {
                    tabs.active_mut()
                        .app
                        .flash(format!("Could not save open tabs: {error}"));
                }
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

    // Capture the visible conversations before cancellation changes live request state.
    for tab in &mut tabs.tabs {
        tab.app.persist_draft_now();
    }
    if let Some(saved) = &mut saved_window {
        if let Err(error) = saved.save(&tabs, true) {
            crate::logging::error(&format!("Could not save open tabs on exit: {error}"));
        }
    }
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
    // The replacement must be able to claim this window and each pane's recovery lease.
    // reexec waits for its child, so keeping these owners alive would lock out restoration.
    drop(tabs);
    drop(saved_window);
    input_result?;
    if let Some(path) = reexec {
        let code = crate::update::reexec_into(&path, &cwd)?;
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
    fn resuming_an_open_chat_focuses_its_busy_owner_and_keeps_both_drafts() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("fixture-home");
        std::fs::create_dir_all(home.join("sessions")).unwrap();
        let session = "8facabf0-7af5-4701-85bd-76ac30604475";
        std::fs::write(
            home.join("sessions/fixture.jsonl"),
            format!("{}\n", serde_json::json!({"type":"session_meta", "payload":{"id":session,"cwd":dir.path()}})),
        ).unwrap();
        crate::store::upsert_stored_account(
            dir.path(),
            crate::store::StoredAccount {
                id: "fixture".into(),
                provider: ProviderKind::Codex,
                home_path: Some(home.to_string_lossy().into_owned()),
                label: "Fixture".into(),
                created_at: "0".into(),
            },
        )
        .unwrap();
        let mut owner = new_tab_app(dir.path(), dir.path(), false);
        owner.provider = ProviderKind::Codex;
        owner.account = "fixture".into();
        owner.home_path = Some(home);
        owner.native_session_id = Some(session.into());
        owner.editor.set_text("owning tab draft");
        owner.busy = true;
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        owner.preparing_cancel = Some(cancel.clone());
        let conversation = owner.deleg.d.conversation_id().to_owned();
        let mut tabs = tabs::TabSet::new(RunTab::new(owner));
        let mut origin = new_tab_app(dir.path(), dir.path(), false);
        origin.editor.set_text("other tab draft");
        assert!(origin.request_resume_session("fixture", session));
        let request = origin.session_ui.resume_request.take().unwrap();
        tabs.add(RunTab::new(origin));
        resume_in_tabs(&mut tabs, request, dir.path(), false);
        assert_eq!(tabs.tabs.len(), 2);
        assert_eq!(tabs.active, 0);
        assert!(tabs.active().app.busy);
        assert_eq!(
            tabs.active().app.native_session_id.as_deref(),
            Some(session)
        );
        assert_eq!(tabs.active().app.deleg.d.conversation_id(), conversation);
        assert_eq!(tabs.tabs[0].app.editor.text(), "owning tab draft");
        assert_eq!(tabs.tabs[1].app.editor.text(), "other tab draft");
        assert!(!cancel.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!tabs.active().app.session_ui.loading());
        assert!(tabs
            .tabs
            .iter_mut()
            .all(|tab| tab.turn_rx.try_recv().is_err()));
    }

    #[test]
    fn home_tab_keeps_the_previous_chat_draft_and_owns_new_recovery() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let mut original = new_tab_app(dir.path(), dir.path(), false);
        original.editor.set_text("draft belongs to previous chat");
        original.queue.items.push("queued in previous chat".into());
        let mut tabs = tabs::TabSet::new(RunTab::new(original));
        landing::open_home(&mut tabs.active_mut().app);
        assert_eq!(
            tabs.active_mut().app.tab_request.take(),
            Some(tabs::TabAction::Home)
        );
        let mut home = new_tab_app(dir.path(), dir.path(), false);
        landing::open_home_tab(&mut home);
        assert!(home.editor.is_empty());
        assert!(home.queue.items.is_empty());
        tabs.add(RunTab::new(home));
        assert!(landing::visible(&tabs.active().app));
        tabs.next(true);
        assert_eq!(
            tabs.active().app.editor.text(),
            "draft belongs to previous chat"
        );
        assert_eq!(tabs.active().app.queue.items[0], "queued in previous chat");
    }

    #[test]
    fn panic_log_line_never_contains_payload_text() {
        let secret = "SECRET-PANIC-PAYLOAD";
        let payload = secret.to_string();
        let line = super::format_panic_log_line(&payload, Some(std::panic::Location::caller()));
        assert!(!line.contains(secret));
        assert!(line.contains("location="));
    }
}
