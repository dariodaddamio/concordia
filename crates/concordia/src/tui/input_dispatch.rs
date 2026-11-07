use super::*;

/// Releases are identified by the backend. Never infer duplicates from processing time:
/// queued, distinct presses can be drained together after a large paste or slow frame.
pub(super) fn should_accept_key(key: &KeyEvent) -> bool {
    if matches!(key.kind, KeyEventKind::Release) {
        return false;
    }
    if key.kind == KeyEventKind::Repeat && key.code == KeyCode::Enter {
        return false;
    }
    // Enter also arrives as Char('\r') / '\n' on some hosts — ignore those chars.
    if matches!(key.code, KeyCode::Char('\r') | KeyCode::Char('\n')) {
        return false;
    }
    true
}

pub(super) fn dispatch_key_burst(
    app: &mut App,
    burst: input::KeyBurst,
    turn_tx: &mpsc::UnboundedSender<String>,
) -> bool {
    // File drops can arrive as a whole unframed burst of character keys.
    if !burst.text && burst.keys.len() > 1 {
        let dropped: Option<String> = burst
            .keys
            .iter()
            .map(|key| match key.code {
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    Some(character)
                }
                _ => None,
            })
            .collect();
        if let Some(text) = dropped.filter(|text| crate::attachments::pasted_paths(text).is_some())
        {
            app.paste_text(&text);
            return false;
        }
    }
    if burst.text {
        let text = burst.text_content.as_deref().unwrap_or_default();
        if !app.project_search.active() && questions::paste(app, text) {
            return false;
        }
        app.paste_text_chunk(
            burst.text_content.as_deref().unwrap_or_default(),
            burst.continues_paste,
        );
        return false;
    }
    burst
        .keys
        .into_iter()
        .any(|key| handle_key(app, key, turn_tx))
}

pub(super) fn dispatch_key_event(
    app: &mut App,
    key: KeyEvent,
    key_burst: &mut input::BurstBuffer,
    turn_tx: &mpsc::UnboundedSender<String>,
    now: Instant,
) -> bool {
    if matches!(key.kind, KeyEventKind::Release)
        || key.kind == KeyEventKind::Repeat && key.code == KeyCode::Enter
    {
        return false;
    }
    let palette_action = matches!(key.code, KeyCode::Enter | KeyCode::Tab)
        && !app.palette_entries().is_empty()
        && !key_burst.active();
    if input::BurstBuffer::is_candidate(&key)
        && app.picker.is_none()
        && !app.project_search.active()
        && !palette_action
    {
        if let Some(burst) = key_burst.push(key, now) {
            return dispatch_key_burst(app, burst, turn_tx);
        }
        return false;
    }
    if !should_accept_key(&key) {
        return false;
    }
    if let Some(burst) = key_burst.take_pending() {
        if dispatch_key_burst(app, burst, turn_tx) {
            return true;
        }
    }
    key_burst.end_continuation();
    handle_key(app, key, turn_tx)
}

pub(super) fn handle_key(
    app: &mut App,
    key: KeyEvent,
    turn_tx: &mpsc::UnboundedSender<String>,
) -> bool {
    // Native windows distinguish auto-repeat from a fresh press. Navigation and deletion
    // must keep moving while held; action shortcuts (send, cancel, toggles) run once.
    match key.kind {
        KeyEventKind::Release => return false,
        KeyEventKind::Repeat
            if !matches!(
                key.code,
                KeyCode::Backspace
                    | KeyCode::Delete
                    | KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::PageUp
                    | KeyCode::PageDown
            ) =>
        {
            return false;
        }
        _ => {}
    }
    if key.code != KeyCode::Enter || !key.modifiers.is_empty() {
        app.busy_send_confirmation = None;
    }
    if raster::key(app, &key) {
        return false;
    }
    if worker_input::key(app, &key) {
        return false;
    }
    if app.project_search.active() {
        if (matches!(key.code, KeyCode::Char('v' | 'V'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT))
            || (key.code == KeyCode::Insert && key.modifiers.contains(KeyModifiers::SHIFT))
        {
            match clipboard::get_text() {
                Ok(text) => app.project_search.paste(&text),
                Err(error) => app.project_search.notice(&format!("paste failed: {error}")),
            }
            return false;
        }
        let action = app.project_search.key(key);
        app.apply_search_action(action);
        return false;
    }
    if key.code == KeyCode::F(9)
        && app.picker.is_none()
        && app.workspace.focused.is_none()
        && app.queue.edit.is_none()
        && app.activity.detail.is_none()
        && !app.questions.focused
    {
        if !app.accept_pasted_files() {
            app.start_image_import(None, clipboard::has_attachment());
        }
        return false;
    }
    if key.code == KeyCode::F(10)
        && app.picker.is_none()
        && app.workspace.focused.is_none()
        && app.queue.edit.is_none()
        && app.activity.detail.is_none()
        && !app.questions.focused
    {
        landing::close(app);
        integrations::open(app);
        return false;
    }
    if key.code == KeyCode::Char('f')
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && app
            .picker
            .as_ref()
            .is_some_and(|p| p.kind == PickerKind::Files)
    {
        app.open_project_search();
        return false;
    }
    if key.code == KeyCode::F(4)
        && app
            .picker
            .as_ref()
            .is_some_and(|picker| picker.kind == PickerKind::Files)
    {
        let index = app.picker.as_ref().and_then(|picker| {
            picker.state.selected().and_then(|index| {
                picker
                    .filtered()
                    .get(index)
                    .and_then(|row| row.id.strip_prefix("file:"))
                    .and_then(|id| id.parse::<usize>().ok())
            })
        });
        if let Some(entry) = index
            .and_then(|index| app.related_files.entries.get(index))
            .filter(|entry| !entry.directory)
            .cloned()
        {
            app.show_file_actions(entry.candidate, app.cwd.clone());
        }
        return false;
    }
    if key.code == KeyCode::F(4) && app.picker.is_none() {
        if let Some(file) = app.workspace.selected_file() {
            let target = file.path.to_string_lossy().into_owned();
            app.show_file_actions(
                crate::file_links::Candidate {
                    display: target.clone(),
                    range: 0..target.len(),
                    target,
                    line: file.line,
                    column: file.column,
                    markdown: false,
                },
                app.cwd.clone(),
            );
            return false;
        }
    }
    if landing::navigation_key(app, key) {
        return false;
    }
    if landing::visible(app) && landing::handle_key(app, key) {
        return false;
    }
    if tokens::handle_key(app, key) {
        return false;
    }
    if integrations::handle_key(app, key) {
        return false;
    }
    if artifacts::key(app, key) {
        return false;
    }
    if let Some(picker) = app.picker.as_mut() {
        // Keep the long-standing command-history affordance when `/doctor` was just submitted.
        // Once the user has typed or selected a row, the doctor picker owns navigation normally.
        if picker.kind == PickerKind::Doctor
            && app.editor.text().is_empty()
            && app.history.last().is_some_and(|line| line == "/doctor")
            && matches!(key.code, KeyCode::Up | KeyCode::Down)
        {
            app.dismiss_picker();
            if key.code == KeyCode::Up {
                app.history_prev();
            } else {
                app.history_next();
            }
            return false;
        }
        if picker.kind == PickerKind::DoctorConfirm {
            match key.code {
                KeyCode::Char('y' | 'Y') => {
                    let index = picker
                        .rows
                        .first()
                        .and_then(|row| row.id.strip_prefix("doctor-confirm:"))
                        .and_then(|s| s.parse().ok());
                    if let Some(index) = index {
                        app.apply_doctor_fix(index);
                    }
                }
                KeyCode::Char('n' | 'N') | KeyCode::Esc => app.dismiss_picker(),
                _ => {}
            }
            return false;
        }
        if picker.kind == PickerKind::Doctor
            && matches!(key.code, KeyCode::Char(_) | KeyCode::Backspace)
        {
            app.dismiss_picker();
            if let KeyCode::Char(c) = key.code {
                app.editor.insert_char(c);
            } else {
                app.editor.backspace();
            }
            return false;
        }
        if matches!(
            picker.kind,
            PickerKind::Update
                | PickerKind::Tokens
                | PickerKind::Related
                | PickerKind::Integrations
                | PickerKind::ArtifactDetail
        ) {
            let visible = app.update_view.max(1);
            let last = app.update_rows.saturating_sub(visible);
            match key.code {
                KeyCode::Char('h' | 'H')
                    if picker.kind == PickerKind::Update
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    if let Some(preview) = app.update_preview.as_mut() {
                        preview.history_expanded = !preview.history_expanded;
                        picker.state.select(Some(0));
                    }
                }
                KeyCode::Esc => app.dismiss_picker(),
                KeyCode::Enter
                    if picker.kind == PickerKind::Update
                        && app.job.is_none()
                        && app
                            .update_preview
                            .as_ref()
                            .is_some_and(|p| !p.install_available) =>
                {
                    app.dismiss_picker();
                }
                KeyCode::Enter
                    if app
                        .update_preview
                        .as_ref()
                        .is_some_and(|p| p.install_available) =>
                {
                    app.start_job(JobKind::Update);
                    if app.job.as_ref().is_some_and(|j| j.kind == JobKind::Update) {
                        app.update_progress();
                    }
                }
                KeyCode::Char('o') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.open_update_release()
                }
                KeyCode::Up => picker
                    .state
                    .select(Some(picker.state.selected().unwrap_or(0).saturating_sub(1))),
                KeyCode::Down => picker
                    .state
                    .select(Some((picker.state.selected().unwrap_or(0) + 1).min(last))),
                KeyCode::PageUp => picker.state.select(Some(
                    picker.state.selected().unwrap_or(0).saturating_sub(visible),
                )),
                KeyCode::PageDown => picker.state.select(Some(
                    (picker.state.selected().unwrap_or(0) + visible).min(last),
                )),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.dismiss_picker()
                }
                _ => {}
            }
            return false;
        }
        match key.code {
            KeyCode::Esc => app.dismiss_picker(),
            KeyCode::Enter if picker.kind == PickerKind::Doctor => {
                app.apply_picker_selection(Some(turn_tx))
            }
            KeyCode::Enter => app.apply_picker_selection(Some(turn_tx)),
            KeyCode::Delete if picker.kind == PickerKind::Resume => app.prepare_resume_delete(),
            KeyCode::Char('t') if picker.kind == PickerKind::Usage => tokens::open(app),
            KeyCode::Char('r') if picker.kind == PickerKind::Usage => {
                for account in crate::accounts::accounts_with_auth(&app.state_root) {
                    app.usage_fetcher.invalidate(&account.account);
                }
                app.open_usage_picker();
            }
            KeyCode::Char('r') if picker.kind == PickerKind::Model && picker.filter.is_empty() => {
                app.refresh_model_picker()
            }
            KeyCode::Up => {
                let n = picker.filtered().len();
                if n > 0 {
                    let i = picker.state.selected().unwrap_or(0);
                    picker
                        .state
                        .select(Some(if i == 0 { n - 1 } else { i - 1 }));
                }
            }
            KeyCode::Down => {
                let n = picker.filtered().len();
                if n > 0 {
                    let i = picker.state.selected().unwrap_or(0);
                    picker.state.select(Some((i + 1) % n));
                }
            }
            KeyCode::Backspace => {
                picker.filter.pop();
                picker.select_clamp();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                picker.filter.push(c);
                picker.select_clamp();
            }
            // Like Esc: close the picker. It must not quit the app (and lose the draft), which
            // would contradict the press-twice rule for exiting.
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.dismiss_picker();
            }
            _ => {}
        }
        return false;
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if ctrl && !alt && matches!(key.code, KeyCode::Char('g' | 'G')) {
        app.open_agents_picker();
        return false;
    }
    if ctrl && !alt && matches!(key.code, KeyCode::Char('n' | 'N')) {
        app.open_needs_you_picker();
        return false;
    }
    let tw = app.areas.input_text_w.max(1);

    if app.activity.detail.is_none() && ctrl && !alt && matches!(key.code, KeyCode::Char('o' | 'O'))
    {
        app.expanded_activity = !app.expanded_activity;
        app.activity.collapsed.clear();
        app.flash(if app.expanded_activity {
            "full activity - Ctrl+O collapses"
        } else {
            "compact activity - Ctrl+O expands"
        });
        return false;
    }
    if app.idle_notice.is_some()
        && app.activity.detail.is_none()
        && ctrl
        && !alt
        && matches!(key.code, KeyCode::Char('k' | 'K'))
    {
        app.open_idle_picker();
        return false;
    }
    if app.activity.detail.is_none() && ctrl && !alt && matches!(key.code, KeyCode::Char('e' | 'E'))
    {
        if let Some(i) = app.messages.iter().rposition(|m| m.role == MsgRole::Card) {
            app.open_tool_details(i);
        } else {
            app.flash("no tool output yet; expand thinking in the chat");
        }
        return false;
    }
    if app.activity.detail.is_some() {
        match app.activity.handle_key(key) {
            activity::ModalAction::Close => {
                app.activity.detail = None;
                app.activity.detail_text = None;
                app.activity.query.clear();
            }
            activity::ModalAction::Copy => {
                if let Some(i) = app.activity.detail {
                    if let Some(text) = app
                        .activity
                        .detail_text
                        .as_ref()
                        .map(|(title, body)| activity::full_output(title, Some(body)))
                        .or_else(|| {
                            app.messages
                                .get(i)
                                .and_then(|m| m.card.as_ref())
                                .map(|c| activity::full_output(&c.title, c.detail.as_deref()))
                        })
                    {
                        app.write_clipboard(&text);
                    }
                }
            }
            activity::ModalAction::PreviousCard => app.navigate_activity_card(true),
            activity::ModalAction::NextCard => app.navigate_activity_card(false),
            activity::ModalAction::Paste => app.paste_clipboard(),
            // Modal owns every other key; editing, undo, queue actions and cancellation do not
            // fall through to the prompt or turn handlers.
            activity::ModalAction::Consumed => {}
        }
        return false;
    }
    if questions::handle_key(app, &key) {
        return false;
    }
    if queue_focus::handle_key(app, &key) {
        app.workspace.focused = None;
        return false;
    }
    if app.workspace.focused.is_some()
        && !app.queue.focused
        && app.queue.edit.is_none()
        && workspace::handle_key(app, &key)
    {
        return false;
    }
    if queue_controls::handle_key(app, &key) || queue::handle_key(app, &key) {
        app.workspace.focused = None;
        return false;
    }
    if workers::handle_key(app, &key) {
        app.workspace.focused = None;
        return false;
    }
    if workspace::handle_key(app, &key) {
        return false;
    }

    // `@chat` completion list (own mode; the slash palette below is unchanged).
    if chatref::handle_key(app, &key) {
        return false;
    }
    // Slash-command suggestions take the navigation keys while they are showing.
    let entries = app.palette_entries();
    if !entries.is_empty() && !ctrl && !alt {
        app.palette.clamp(entries.len());
        let chosen = entries[app.palette.selected].clone();
        match key.code {
            KeyCode::Up | KeyCode::BackTab => {
                app.palette.up(entries.len());
                return false;
            }
            KeyCode::Down => {
                app.palette.down(entries.len());
                return false;
            }
            KeyCode::Tab => {
                app.complete_palette(chosen);
                return false;
            }
            // Enter completes a partial command; once it is exactly the chosen one, it runs.
            KeyCode::Enter if app.editor.text() != chosen.command => {
                app.complete_palette(chosen);
                return false;
            }
            KeyCode::Esc => {
                app.palette.dismissed = true;
                return false;
            }
            _ => {}
        }
    }

    // Anything that edits the prompt drops a transcript selection (like a normal document).
    let edits = matches!(
        key.code,
        KeyCode::Backspace | KeyCode::Delete | KeyCode::Enter | KeyCode::Tab
    ) || matches!(key.code, KeyCode::Char(_)) && (!ctrl || alt);
    if edits {
        app.view.clear_selection();
        app.hist_pos = None;
        // Tab/Enter are handled by the palette above; any other edit reopens and re-ranks it.
        app.palette.reset();
    }

    match key.code {
        // Copy when something is selected (Windows Terminal convention); otherwise Ctrl+C keeps
        // its meaning: cancel the running turn, or quit when idle. Ctrl+Shift+C always copies
        // (falling back to the last reply) on hosts that deliver it to the app.
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'c') => {
            if let Some(i) = app.activity.detail {
                if let Some(text) = app
                    .messages
                    .get(i)
                    .and_then(|m| m.card.as_ref())
                    .map(|c| activity::full_output(&c.title, c.detail.as_deref()))
                {
                    app.write_clipboard(&text);
                }
                return false;
            }
            if shift {
                app.copy(true);
                false
            } else if app.has_selection() {
                app.copy(false);
                false
            } else if app.busy {
                app.request_cancel();
                false
            } else if app.quit_armed.is_some_and(|t| t.elapsed() <= QUIT_WINDOW) {
                app.workspace.request_file_exit()
            } else {
                app.quit_armed = Some(Instant::now());
                app.flash("press Ctrl+C again to exit");
                false
            }
        }
        // Ctrl+Z undo; Ctrl+Shift+Z and Ctrl+Y redo (hosts report Shift as the modifier or as an
        // uppercase letter).
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'z') => {
            if shift || c == 'Z' {
                app.editor.redo();
            } else {
                app.editor.undo();
            }
            app.after_undo();
            false
        }
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'y') => {
            app.editor.redo();
            app.after_undo();
            false
        }
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'v') => {
            app.paste_clipboard();
            false
        }
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'x') => {
            app.cut();
            false
        }
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'a') => {
            app.view.clear_selection();
            app.editor.select_all();
            false
        }
        // Ctrl+J / Alt+Enter / Shift+Enter: newline without sending.
        KeyCode::Char(c) if ctrl && !alt && c.eq_ignore_ascii_case(&'j') => {
            app.editor.insert_char('\n');
            false
        }
        KeyCode::Enter if ctrl || alt || shift => {
            app.editor.insert_char('\n');
            false
        }
        KeyCode::Enter => {
            app.submit_input(turn_tx);
            false
        }
        KeyCode::Tab => {
            if !queue::tab(app) {
                app.tab_complete_slash();
            }
            false
        }
        // Shift+Tab (palette closed): cycle the permission mode.
        KeyCode::BackTab => {
            app.cycle_mode();
            false
        }
        KeyCode::Backspace if ctrl => {
            app.editor.delete_word_left();
            false
        }
        KeyCode::Backspace => {
            app.editor.backspace();
            false
        }
        KeyCode::Delete if ctrl => {
            app.editor.delete_word_right();
            false
        }
        KeyCode::Delete => {
            app.editor.delete();
            false
        }
        KeyCode::Left if ctrl => {
            app.editor.word_left(shift);
            false
        }
        KeyCode::Right if ctrl => {
            app.editor.word_right(shift);
            false
        }
        KeyCode::Left => {
            app.editor.move_left(shift);
            false
        }
        KeyCode::Right => {
            app.editor.move_right(shift);
            false
        }
        KeyCode::PageUp if alt => {
            app.scroll_input(-(app.areas.input.height.max(1) as isize));
            false
        }
        KeyCode::PageDown if alt => {
            app.scroll_input(app.areas.input.height.max(1) as isize);
            false
        }
        KeyCode::Home if ctrl => {
            app.editor.move_to(0, shift);
            false
        }
        KeyCode::End if ctrl => {
            app.editor.move_to(usize::MAX, shift);
            false
        }
        KeyCode::Home => {
            app.editor.home(tw, shift);
            false
        }
        KeyCode::End => {
            app.editor.end(tw, shift);
            false
        }
        // Up on the first prompt row (and Down on the last, while browsing) walks sent prompts.
        KeyCode::Up => {
            if !shift
                && !ctrl
                && !alt
                && app.editor.selection().is_none()
                && app.editor.cursor_pos(tw).0 == 0
            {
                if !queue::focus_on_up(app) {
                    app.history_prev();
                }
            } else {
                app.editor.up(tw, shift);
            }
            false
        }
        KeyCode::Down => {
            let last_row = app.editor.layout(tw).len() - 1;
            if !shift
                && !ctrl
                && !alt
                && app.editor.selection().is_none()
                && app.editor.cursor_pos(tw).0 == last_row
            {
                app.history_next();
            } else {
                app.editor.down(tw, shift);
            }
            false
        }
        KeyCode::PageUp => {
            let n = app.view.page();
            app.view.scroll_up(n);
            false
        }
        KeyCode::PageDown => {
            let n = app.view.page();
            app.view.scroll_down(n);
            false
        }
        // AltGr arrives as Ctrl+Alt on Windows; those chars are real input.
        KeyCode::Char(c) if !ctrl || alt => {
            app.editor.insert_char(c);
            false
        }
        KeyCode::Esc => {
            // Esc never quits (an accidental press must not lose a draft): it clears a
            // selection, or interrupts a running turn. Exit is /exit or Ctrl+C twice.
            if let Some(job) = &app.job {
                job.cancel();
                app.flash("cancelling...");
            } else if app.has_selection() {
                app.clear_selections();
            } else if app.busy {
                app.request_cancel();
            }
            false
        }
        _ => false,
    }
}

/// Prompt text index under screen cell `(x, y)`; coordinates outside the box clamp to its edge
/// so a drag that leaves the prompt keeps extending the selection.
fn input_index_at(app: &App, x: u16, y: u16) -> usize {
    let r = app.areas.input;
    let rel_y = (y.saturating_sub(r.y) as usize).min(r.height.saturating_sub(1) as usize);
    let rel_x = (x.saturating_sub(r.x) as usize).saturating_sub(PROMPT_W);
    app.editor.index_at(
        app.areas.input_text_w.max(1),
        rel_y + app.input_scroll,
        rel_x,
    )
}

pub(super) fn handle_mouse(app: &mut App, m: MouseEvent) {
    if matches!(m.kind, MouseEventKind::Down(_) | MouseEventKind::Drag(_)) {
        app.card_header_pressed = None;
        app.busy_send_confirmation = None;
    }
    if app.raster.active() && !worker_input::active(app) {
        modal::mouse(app, &m);
        return;
    }
    if worker_input::active(app) {
        if !modal::mouse(app, &m) {
            worker_input::mouse(app, &m);
        }
        return;
    }
    if app.project_search.active() {
        return;
    }
    let permissions_hit = contains(app.areas.permissions, m.column, m.row);
    let permissions_allowed = app
        .picker
        .as_ref()
        .is_none_or(|picker| picker.kind == PickerKind::Trust)
        && app.activity.detail.is_none()
        && !workers::overlay_open(app);
    if m.kind == MouseEventKind::Down(MouseButton::Left) {
        app.permissions_pressed = permissions_hit && permissions_allowed;
        if app.permissions_pressed {
            return;
        }
    }
    if m.kind == MouseEventKind::Up(MouseButton::Left)
        && std::mem::take(&mut app.permissions_pressed)
    {
        if permissions_hit && permissions_allowed {
            if app
                .picker
                .as_ref()
                .is_some_and(|picker| picker.kind == PickerKind::Trust)
            {
                app.dismiss_picker();
            } else {
                app.open_trust_picker();
            }
        }
        return;
    }
    if modal::mouse(app, &m) {
        return;
    }
    if app
        .picker
        .as_ref()
        .is_some_and(|picker| picker.kind == PickerKind::Trust)
        && m.kind == MouseEventKind::Down(MouseButton::Left)
        && !contains(app.areas.permissions_popup, m.column, m.row)
    {
        app.dismiss_picker();
    }
    if app
        .picker
        .as_ref()
        .is_some_and(|picker| picker.kind == PickerKind::Trust)
    {
        let hit = app
            .permission_rows
            .iter()
            .find(|(rect, _)| contains(*rect, m.column, m.row))
            .map(|(_, index)| *index);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                app.permission_row_pressed = hit;
                if let Some(index) = hit {
                    app.picker.as_mut().unwrap().state.select(Some(index));
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(index) = app
                    .permission_row_pressed
                    .take()
                    .filter(|index| Some(*index) == hit)
                {
                    app.picker.as_mut().unwrap().state.select(Some(index));
                    app.apply_picker_selection(None);
                }
            }
            MouseEventKind::Drag(_) => app.permission_row_pressed = None,
            _ => {}
        }
        return;
    }
    let home_hit = app
        .areas
        .header_home
        .is_some_and(|rect| contains(rect, m.column, m.row));
    let credits_hit = app
        .areas
        .header_credits
        .is_some_and(|rect| contains(rect, m.column, m.row));
    if m.kind == MouseEventKind::Down(MouseButton::Left) {
        app.header_home_pressed = home_hit;
        app.header_credits_pressed = credits_hit;
        app.header_email_pressed = false;
        if home_hit || credits_hit {
            return;
        }
    }
    if m.kind == MouseEventKind::Up(MouseButton::Left)
        && std::mem::take(&mut app.header_home_pressed)
    {
        if home_hit {
            if landing::visible(app) {
                landing::close(app);
            } else {
                landing::open_home(app);
            }
        }
        return;
    }
    if m.kind == MouseEventKind::Up(MouseButton::Left)
        && std::mem::take(&mut app.header_credits_pressed)
    {
        if credits_hit {
            app.show_credit_details(false);
        }
        return;
    }
    let email_hit = app
        .areas
        .header_email
        .is_some_and(|rect| contains(rect, m.column, m.row));
    if m.kind == MouseEventKind::Down(MouseButton::Left) {
        app.header_email_pressed = email_hit;
        if email_hit {
            return;
        }
    }
    if m.kind == MouseEventKind::Up(MouseButton::Left)
        && std::mem::take(&mut app.header_email_pressed)
    {
        if email_hit {
            app.header_email_revealed = !app.header_email_revealed;
        }
        return;
    }
    if chatref::handle_mouse(app, m) {
        return;
    }
    if landing::visible(app) && landing::handle_mouse(app, m) {
        return;
    }
    if app.activity.detail.is_some() {
        let file = app
            .activity
            .file_targets
            .iter()
            .find(|(rect, _)| contains(*rect, m.column, m.row))
            .map(|(_, file)| file.clone());
        if m.kind == MouseEventKind::Down(MouseButton::Left) {
            app.activity.pressed_file = file;
            return;
        }
        if m.kind == MouseEventKind::Up(MouseButton::Left) {
            if let Some(pressed) = app
                .activity
                .pressed_file
                .take()
                .filter(|pressed| Some(pressed) == file.as_ref())
            {
                app.activity.detail = None;
                app.open_file_candidate(pressed, true);
            }
            return;
        }
        match m.kind {
            MouseEventKind::ScrollUp => {
                app.activity.detail_scroll = app.activity.detail_scroll.saturating_sub(3);
                app.activity.follow_match = false;
            }
            MouseEventKind::ScrollDown => {
                app.activity.detail_scroll = app.activity.detail_scroll.saturating_add(3);
                app.activity.follow_match = false;
            }
            _ => {}
        }
        return;
    }
    if app
        .picker
        .as_ref()
        .is_some_and(|p| p.kind == PickerKind::Files)
    {
        if let Some(index) = app
            .related_files
            .targets
            .iter()
            .find(|(rect, _)| contains(*rect, m.column, m.row))
            .map(|(_, index)| *index)
        {
            if m.kind == MouseEventKind::Down(MouseButton::Left) {
                app.picker.as_mut().unwrap().state.select(Some(index));
                app.related_files.pressed = Some(index);
            }
            if m.kind == MouseEventKind::Up(MouseButton::Left)
                && app.related_files.pressed.take() == Some(index)
            {
                app.apply_picker_selection(None);
            }
        }
        if matches!(m.kind, MouseEventKind::Drag(_) | MouseEventKind::Up(_)) {
            app.related_files.pressed = None;
        }
        return;
    }
    if app.picker.is_some() {
        let max = app.update_rows.saturating_sub(app.update_view.max(1));
        if let Some(picker) = app.picker.as_mut().filter(|p| {
            matches!(
                p.kind,
                PickerKind::Update
                    | PickerKind::Tokens
                    | PickerKind::Related
                    | PickerKind::Integrations
                    | PickerKind::ArtifactDetail
            )
        }) {
            match m.kind {
                MouseEventKind::ScrollUp => picker
                    .state
                    .select(Some(picker.state.selected().unwrap_or(0).saturating_sub(3))),
                MouseEventKind::ScrollDown => picker
                    .state
                    .select(Some((picker.state.selected().unwrap_or(0) + 3).min(max))),
                _ => {}
            }
        }
        return;
    }
    if images::mouse(app, &m) || questions::mouse(app, &m) {
        return;
    }
    if workers::handle_mouse(app, &m) || queue::handle_mouse(app, &m) {
        app.workspace.focused = None;
        return;
    }
    if workspace::handle_mouse(app, &m) {
        return;
    }
    let (x, y) = (m.column, m.row);
    let t = app.areas.transcript;
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            app.file_click = None;
            if let Some((_, id)) = app.areas.pinned_card.filter(|(r, _)| contains(*r, x, y)) {
                app.activity.toggle_card(id, app.expanded_activity);
                app.view.clear_selection();
                app.drag = Drag::None;
                return;
            }
            if contains(app.areas.run_cancel, x, y) {
                app.request_cancel();
                return;
            }
            if contains(app.areas.bottom_button, x, y) {
                app.view.follow_tail();
                app.view.clear_selection();
                app.drag = Drag::None;
                app.workspace.focused = None;
                return;
            }
            let clicks = app.clicks.register(x, y, Instant::now());
            let pal = app.areas.palette;
            if pal.height > 0 && contains(pal, x, y) {
                let entries = app.palette_entries();
                let visible = pal.height.saturating_sub(2) as usize;
                let first = app
                    .palette
                    .selected
                    .saturating_sub(visible.saturating_sub(1));
                let row = (y.saturating_sub(pal.y + 1)) as usize;
                if let Some(e) = entries.get(first + row) {
                    let e = e.clone();
                    app.complete_palette(e);
                }
                return;
            }
            if contains(t, x, y) {
                if let Some(i) = app.view.identity_at((y - t.y) as usize) {
                    if app.messages.get(i).is_some_and(|line| {
                        line.meta
                            .as_ref()
                            .is_some_and(|(_, label)| label == "__concordia_idle_notice__")
                    }) {
                        app.open_idle_picker();
                        return;
                    }
                }
                if x.saturating_sub(t.x) < 2 {
                    if let Some(i) = app.view.identity_at((y - t.y) as usize) {
                        if app.messages.get(i).is_some_and(ChatLine::toggles) {
                            app.activity.toggle_card(i, app.expanded_activity);
                            app.drag = Drag::None;
                            return;
                        }
                    }
                }
                app.card_header_pressed = activity::card_header_at(app, x, y);
                app.editor.clear_selection();
                app.drag = Drag::Transcript;
                let pos = app.view.pos_at((y - t.y) as isize, (x - t.x) as usize);
                match (pos, clicks) {
                    (None, _) => {
                        app.view.clear_selection();
                        app.drag = Drag::None;
                    }
                    (Some(p), 1) => {
                        app.file_click = Some(p);
                        app.view.start_selection(p);
                    }
                    (Some(p), 2) => {
                        app.view.select_word(p);
                        app.drag = Drag::None;
                    }
                    (Some(p), _) => {
                        app.view.select_line(p);
                        app.drag = Drag::None;
                    }
                }
            } else if contains(app.areas.input, x, y) {
                app.questions.focused = false;
                app.view.clear_selection();
                app.drag = Drag::Input;
                let idx = input_index_at(app, x, y);
                match clicks {
                    1 => app.editor.move_to(idx, false),
                    2 => app.editor.select_word_at(idx),
                    _ => app.editor.select_line_at(idx),
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            app.file_click = None;
            match app.drag {
                Drag::Transcript => {
                    // Dragging past the top/bottom edge scrolls while extending the selection.
                    if y < t.y {
                        app.view.scroll_up(1);
                    } else if y >= t.y + t.height {
                        app.view.scroll_down(1);
                    }
                    let rel_row = y as isize - t.y as isize;
                    let rel_col = x.saturating_sub(t.x) as usize;
                    let p = app
                        .view
                        .pos_at(rel_row, rel_col)
                        .or_else(|| app.view.last_pos());
                    if let Some(p) = p {
                        app.view.extend_selection(p);
                    }
                }
                Drag::Input => {
                    let idx = input_index_at(app, x, y);
                    app.editor.move_to(idx, true);
                }
                Drag::None => {}
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if let Some(id) = app.card_header_pressed.take() {
                if activity::card_header_at(app, x, y) == Some(id) {
                    app.activity.toggle_card(id, app.expanded_activity);
                    app.view.clear_selection();
                    app.drag = Drag::None;
                    app.file_click = None;
                    return;
                }
            }
            let clicked = app.file_click.take();
            if app.drag == Drag::Transcript {
                if let Some((anchor, head)) = app.view.sel.filter(|(a, b)| a == b) {
                    let release = app
                        .view
                        .pos_at(y as isize - t.y as isize, x.saturating_sub(t.x) as usize);
                    if clicked == Some(head) && contains(t, x, y) && release == Some(head) {
                        if let Some(row) = app.view.rows.get(anchor.row) {
                            let mut candidates: Vec<_> = row
                                .links
                                .iter()
                                .filter(|(a, b, _)| *a <= head.col && head.col < *b)
                                .map(|(_, _, c)| c.clone())
                                .collect();
                            candidates.sort_by_key(|c| std::cmp::Reverse(c.range.len()));
                            let fallback = candidates.first().cloned();
                            if let Some(candidate) = candidates
                                .into_iter()
                                .find(|c| crate::file_links::resolve(c, &app.cwd).is_ok())
                                .or(fallback)
                            {
                                if m.modifiers.contains(KeyModifiers::ALT) {
                                    app.show_file_actions(candidate, app.cwd.clone());
                                } else {
                                    app.open_file_candidate(candidate, false);
                                }
                            } else if let Some(url) = row
                                .web_links
                                .iter()
                                .find(|(a, b, _)| *a <= head.col && head.col < *b)
                                .map(|(_, _, url)| url.clone())
                            {
                                app.open_web_link(url);
                            }
                        }
                    }
                }
                app.view.finish_selection();
            }
            app.drag = Drag::None;
        }
        // Right click: copy the selection, or paste when there is none (Windows Terminal style).
        MouseEventKind::Down(MouseButton::Right) => {
            if app.has_selection() {
                app.copy(false);
            } else {
                app.paste_clipboard();
            }
        }
        MouseEventKind::ScrollUp if contains(app.areas.input, x, y) => app.scroll_input(-3),
        MouseEventKind::ScrollDown if contains(app.areas.input, x, y) => app.scroll_input(3),
        MouseEventKind::ScrollUp => app.view.scroll_up(3),
        MouseEventKind::ScrollDown => app.view.scroll_down(3),
        _ => {}
    }
}
