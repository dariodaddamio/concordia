use super::super::*;

impl App {
    pub(in crate::tui) fn open_resume_picker(&mut self) {
        self.refresh_store_warning();
        let sessions = list_recent_sessions(&self.state_root, 40);
        let tasks = crate::store::list_tasks(&self.state_root);
        let persisted_ids: std::collections::HashSet<_> = tasks
            .iter()
            .flat_map(|task| {
                task.sessions.iter().map(|session| {
                    (
                        session.provider,
                        session.account.clone(),
                        session.native_session_id.clone(),
                    )
                })
            })
            .collect();
        let native: Vec<_> = sessions
            .iter()
            .filter(|session| {
                !persisted_ids.contains(&(
                    session.provider,
                    session.account.clone(),
                    session.session_id.clone(),
                ))
            })
            .cloned()
            .collect();
        let mut rows = tasks_to_picker_rows(&tasks);
        for row in &mut rows {
            let Some(task_id) = row.id.strip_prefix("task:") else {
                continue;
            };
            if let Some(task) = tasks.iter().find(|task| task.id == task_id) {
                let diagnostic = crate::sessions::diagnose_task(&self.state_root, task);
                if let Some(cause) = diagnostic.cause() {
                    row.detail = format!(
                        "unopenable · {} · Enter to repair (fresh session from handoff map; draft kept)",
                        cause.label()
                    );
                }
            }
        }
        rows.extend(native_session_picker_rows(&native));
        let here = count_sessions_here(&sessions, Some(&self.cwd));
        let folder = self
            .cwd
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "cwd".into());
        let title = if here > 0 {
            format!("chats · {folder} ({here})")
        } else {
            format!("chats · {}", sessions.len())
        };
        if rows.is_empty() {
            rows.push(PickerRow {
                id: "none".into(),
                label: "no chats yet".into(),
                detail: "send a message first — then switch here".into(),
            });
        }
        self.show_picker(PickerKind::Resume, title, rows, true);
    }
    pub(in crate::tui) fn resume_explicit_session(&mut self, id: &str) {
        match crate::sessions::find_native_record(
            &self.state_root,
            &self.account,
            Some(self.provider),
            id,
        ) {
            Ok(Some((session, _))) => {
                self.request_resume_session(&session.account, &session.session_id);
            }
            Ok(None) => self.push_system("resume failed: exact native session was not found in the selected account; use /use to select its account"),
            Err(error) => self.push_system(format!("resume unavailable: {error}")),
        }
    }

    pub(in crate::tui) fn open_chat_repair(
        &mut self,
        task: crate::store::TaskRecord,
        diagnostic: crate::sessions::ChatDiagnostic,
    ) {
        let Some(cause) = diagnostic.cause() else {
            self.request_resume_session(&diagnostic.account, &diagnostic.native_session_id);
            return;
        };
        self.repair_task = Some(task.id);
        self.show_picker(
            PickerKind::ChatRepair,
            format!("repair chat · {}", diagnostic.title),
            vec![PickerRow {
                id: "chat-repair:start".into(),
                label: "Start fresh session from handoff map".into(),
                detail: format!("cause: {} · the current draft is kept", cause.label()),
            }],
            false,
        );
    }

    pub(in crate::tui) fn start_chat_repair(&mut self) {
        let Some(task_id) = self.repair_task.take() else {
            self.push_system("chat repair unavailable: task record is missing; draft kept");
            return;
        };
        let Some(task) = crate::store::task(&self.state_root, &task_id) else {
            self.push_system("chat repair unavailable: task record is missing; draft kept");
            return;
        };
        let Some(map) = task.last_map else {
            self.push_system("chat repair unavailable: handoff map is unreadable; draft kept");
            return;
        };
        let Some(map_text) = crate::threads::ThreadLog::open(&self.state_root, &map.thread_id)
            .read_map(crate::config::for_root(&self.state_root).threads.map_chars)
            .ok()
        else {
            self.push_system("chat repair unavailable: handoff map is unreadable; draft kept");
            return;
        };
        let draft = self.editor.text().to_owned();
        self.handle_slash("/new");
        self.editor.set_text(&draft);
        self.task_id = Some(task_id);
        self.pending_repair_map = Some(map_text);
        self.need_context_pack = true;
        self.landing = None;
        self.push_system("Started a fresh session from the handoff map. Your draft is kept.");
    }
    pub(in crate::tui) fn prepare_resume_delete(&mut self) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        let Some(idx) = picker.state.selected() else {
            return;
        };
        let filtered = picker.filtered();
        let Some(row) = filtered.get(idx) else { return };
        let Some(session) = row.id.strip_prefix("resume:") else {
            return;
        };
        let session = session.to_string();
        let Some((account, id)) = session.split_once(':') else {
            return;
        };
        self.show_picker(
            PickerKind::ResumeDeleteConfirm,
            format!("delete chat {id}?"),
            vec![
                PickerRow {
                    id: format!("resume-delete:{id}"),
                    label: "Delete this chat".into(),
                    detail: format!("{account} · this cannot be undone"),
                },
                PickerRow {
                    id: "resume-delete-cancel".into(),
                    label: "Cancel".into(),
                    detail: String::new(),
                },
            ],
            false,
        );
    }
    pub(in crate::tui) fn set_mode(&mut self, mode: TrustMode) -> bool {
        if let Err(e) = set_trust_mode(&self.state_root, mode) {
            self.push_system(format!("mode save failed - {e}"));
            return false;
        }
        self.trust = mode;
        true
    }
    pub(in crate::tui) fn cycle_mode(&mut self) {
        let next = next_mode(self.trust);
        if self.set_mode(next) {
            let note = if self.busy { " (next turn)" } else { "" };
            self.flash(format!("mode \u{2192} {}{note}", mode_label(next)));
        }
    }
    pub(in crate::tui) fn request_full_mode(&mut self) {
        if self.full_mode_confirmed {
            let _ = self.set_mode(TrustMode::Full);
            return;
        }
        self.show_picker(
            PickerKind::FullModeConfirm,
            "enable full access?".into(),
            vec![
                PickerRow {
                    id: "full-yes".into(),
                    label: "Yes, enable Full".into(),
                    detail: "Skip provider permission checks for this session.".into(),
                },
                PickerRow {
                    id: "full-no".into(),
                    label: "No, keep current mode".into(),
                    detail: String::new(),
                },
            ],
            false,
        );
    }
    pub(in crate::tui) fn run_mode_command(&mut self, arg: Option<&str>) {
        match arg {
            None => {
                self.push_system(format!(
                    "mode: {} - {}",
                    mode_label(self.trust),
                    mode_blurb(self.trust)
                ));
                self.push_system(
                    "set with /mode plan | ask | edit | full  (Shift+Tab cycles plan/ask/edit)",
                );
            }
            Some(a) => match parse_mode(a) {
                Some(mode) => {
                    if mode == TrustMode::Full && !self.full_mode_confirmed {
                        self.request_full_mode();
                        return;
                    }
                    if self.set_mode(mode) {
                        self.push_system(format!(
                            "mode \u{2192} {} - {}{}",
                            mode_label(mode),
                            mode_blurb(mode),
                            if self.busy {
                                " (next turn; the running turn keeps its original permissions)"
                            } else {
                                ""
                            }
                        ));
                    }
                }
                None => self.push_system(format!("unknown mode {a} - use plan, ask, edit or full")),
            },
        }
    }
    pub(in crate::tui) fn open_trust_picker(&mut self) {
        let rows = trust_picker_rows(self.trust);
        let idx = rows
            .iter()
            .position(|r| r.id == format!("trust:{}", self.trust.as_str()))
            .unwrap_or(0);
        self.show_picker_at(PickerKind::Trust, "permissions".into(), rows, false, idx);
    }
    pub(in crate::tui) fn refresh_account_continuity(&mut self) {
        let prior_home = self
            .home_path
            .as_ref()
            .map(|home| home.to_string_lossy().into_owned());
        let prior_provider = self.provider;
        let prior_session = self.native_session_id.clone();
        self.refresh_binding();
        let new_home = self
            .home_path
            .as_ref()
            .map(|home| home.to_string_lossy().into_owned());
        let messages = self.pack_messages();
        let decision = plan_continuity(&ContinuityRequest {
            from_provider: prior_provider,
            to_provider: self.provider,
            from_home: prior_home.as_deref(),
            to_home: new_home.as_deref(),
            session_id: prior_session.as_deref(),
            messages: &messages,
            thread_id: self.thread_id(),
            include_pack_if_needed: false,
        });
        self.native_session_id = decision.session_id.clone();
        if !decision.native_resume {
            self.session_chip = self.session_ui.title.clone();
        }
        self.need_context_pack = !decision.native_resume && !messages.is_empty();
        self.log_switch(prior_provider);
        if decision.pack.is_none()
            && !decision.native_resume
            && decision.reason != crate::context::NEW_SESSION
        {
            self.push_system(decision.reason);
        }
        self.prior_provider = self.provider;
        self.prior_home = self.continuity_home();
    }
    pub(in crate::tui) fn open_use_picker(&mut self) {
        self.request_account_usage();
        self.build_use_picker();
    }
    pub(in crate::tui) fn build_use_picker(&mut self) {
        self.refresh_store_warning();
        let ready = accounts_with_auth(&self.state_root);
        let duplicates = crate::accounts::duplicate_groups(&self.state_root);
        let mut rows = Vec::new();
        for a in &ready {
            let Some(home) = a.home_path.as_ref() else {
                continue;
            };
            let idn = inspect_provider(a.provider, home);
            let usage = self.usage_fetcher.snapshot(a.provider, &a.account, home);
            let mut urow = crate::usage::profiled_picker_row(&a.account, a.provider, &usage, &idn);
            urow.label =
                urow.label
                    .replacen(&display_account_name(a.provider, &a.account), &a.label, 1);
            self.picker_tones
                .insert(format!("use:{}", a.account), urow.tone);
            let mut detail = urow.detail;
            if let Some(other) = duplicates
                .iter()
                .find(|g| g.iter().any(|x| x.account == a.account))
                .and_then(|g| g.iter().find(|x| x.account != a.account))
            {
                detail.push_str(&format!(" · same login as {}", other.label));
            }
            rows.push(PickerRow {
                id: format!("use:{}", a.account),
                // The account's own name ("Codex 3"), so several accounts of one provider are
                // told apart at a glance.
                label: urow.label,
                detail,
            });
        }
        rows.push(PickerRow {
            id: "use-add".into(),
            label: "+ add account".into(),
            detail: if ready.is_empty() {
                "no logged-in accounts yet - connect Codex, Claude or Cursor".into()
            } else {
                "connect another Codex, Claude or Cursor account".into()
            },
        });
        if !ready.is_empty() {
            rows.push(PickerRow {
                id: "use-remove".into(),
                label: "- remove account".into(),
                detail: "take an account out of Concordia".into(),
            });
        }
        rows.push(PickerRow {
            id: "use-handoff".into(),
            label: "next provider for this session".into(),
            detail: self.handoff_description(),
        });
        rows.extend(
            crate::grok_bot::list(&self.state_root)
                .into_iter()
                .map(|routine| PickerRow {
                    id: format!("use-grok-bot:{}", routine.routine_id),
                    label: format!("Grok Bot · {}", routine.name),
                    detail: format!(
                        "account {} · routine {} · global",
                        routine.account, routine.routine_id
                    ),
                }),
        );
        rows.push(PickerRow {
            id: "use-grok-bot-setup".into(),
            label: "+ configure Grok Bot routine".into(),
            detail: "webhook key stays in Windows Credential Manager; see Grok Bot setup guide"
                .into(),
        });
        self.show_picker(PickerKind::Use, "accounts".into(), rows, true);
    }
    fn open_grok_bot_form(&mut self, existing: Option<crate::grok_bot::Routine>) {
        let form = existing.map_or_else(crate::tui::GrokBotForm::default, |r| crate::tui::GrokBotForm {
            account: r.account, routine_id: r.routine_id, name: r.name, webhook_url: r.webhook_url, ..Default::default()
        });
        self.grok_bot_form = Some(form);
        self.show_picker(PickerKind::GrokBotForm, "Grok Bot routine · Tab next · final Enter saves".into(), vec![
            PickerRow { id: "account".into(), label: "Account".into(), detail: String::new() },
            PickerRow { id: "routine".into(), label: "Routine ID".into(), detail: String::new() },
            PickerRow { id: "name".into(), label: "Display name".into(), detail: String::new() },
            PickerRow { id: "url".into(), label: "Webhook URL (HTTPS)".into(), detail: String::new() },
            PickerRow { id: "key".into(), label: "Webhook key ·••••••••".into(), detail: "stored only in Windows Credential Manager".into() },
        ], false);
    }

    pub(in crate::tui) fn grok_bot_form_key(&mut self, key: crossterm::event::KeyEvent) {
        let Some(form) = self.grok_bot_form.as_mut() else { return };
        let active = form.active;
        match key.code {
            crossterm::event::KeyCode::Esc => { self.grok_bot_form = None; self.dismiss_picker(); }
            crossterm::event::KeyCode::Tab => form.active = (active + 1) % 5,
            crossterm::event::KeyCode::Enter if active < 4 => form.active += 1,
            crossterm::event::KeyCode::Backspace => match active { 0 => { form.account.pop(); }, 1 => { form.routine_id.pop(); }, 2 => { form.name.pop(); }, 3 => { form.webhook_url.pop(); }, _ => { form.key.pop(); } },
            crossterm::event::KeyCode::Char(c) if !c.is_control() => match active { 0 => form.account.push(c), 1 => form.routine_id.push(c), 2 => form.name.push(c), 3 => form.webhook_url.push(c), _ => form.key.push(c) },
            crossterm::event::KeyCode::Enter => {
                let routine = crate::grok_bot::Routine { provider: "grok-bot".into(), account: form.account.trim().into(), routine_id: form.routine_id.trim().into(), name: form.name.trim().into(), description: String::new(), webhook_url: form.webhook_url.trim().into() };
                let result = crate::grok_bot::register(&self.state_root, routine, &form.key);
                self.grok_bot_form = None; self.dismiss_picker();
                match result { Ok(()) => self.push_system("Grok Bot routine saved; key is stored only in Credential Manager"), Err(e) => self.push_system(format!("Grok Bot setup failed · {e}")) }
            }
            _ => {}
        }
    }

    pub(in crate::tui) fn show_unfinished_request(&mut self) {
        if let Some(prompt) = self.continuation.pending_text() {
            let text = format!("Unfinished request (not sent):\n{prompt}");
            if !self
                .messages
                .last()
                .is_some_and(|message| message.role == MsgRole::System && message.text == text)
            {
                self.push_system(text);
            }
            self.open_continuation_picker();
        } else {
            self.flash("no unfinished request to continue");
        }
    }
    pub(in crate::tui) fn open_continuation_picker(&mut self) {
        let mut accounts = accounts_with_auth(&self.state_root)
            .into_iter()
            .filter(|a| {
                (a.account != self.account || self.continuation.same_account_allowed())
                    && a.home_path.is_some()
            })
            .collect::<Vec<_>>();
        accounts.sort_by(|a, b| {
            let remaining = |account: &crate::accounts::AccountDef| {
                account.home_path.as_ref().and_then(|home| {
                    crate::usage::peek_cached_usage(
                        &self.state_root,
                        account.provider,
                        &account.account,
                        home,
                    )
                    .and_then(|o| o.remaining_pct)
                })
            };
            remaining(b)
                .is_some()
                .cmp(&remaining(a).is_some())
                .then_with(|| {
                    remaining(b)
                        .partial_cmp(&remaining(a))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.account.cmp(&b.account))
        });
        let mut rows = accounts
            .into_iter()
            .map(|a| {
                let remaining = a.home_path.as_ref().and_then(|home| {
                    crate::usage::peek_cached_usage(&self.state_root, a.provider, &a.account, home)
                        .and_then(|o| o.remaining_pct)
                });
                let tier = crate::models::catalog_tier(&self.model);
                let tier = match tier {
                    crate::models::ModelTier::Frontier => "frontier",
                    crate::models::ModelTier::Standard => "standard",
                    crate::models::ModelTier::Fast => "fast",
                };
                PickerRow {
                    id: format!("continue:{}", a.account),
                    label: a.label,
                    detail: format!(
                        "5h {}% left · {tier} · continue the paused request",
                        remaining
                            .map(|p| p.round() as i64)
                            .map_or("??".into(), |p| p.to_string())
                    ),
                }
            })
            .collect::<Vec<_>>();
        if let Some(home) = &self.home_path {
            rows.extend(
                crate::models::ids_at(&self.state_root, self.provider, &self.account, home)
                    .into_iter()
                    .filter(|model| model != &self.model)
                    .map(|model| PickerRow {
                        id: format!("continue:{} {model}", self.account),
                        label: format!("{} \u{2192} {model}", self.account_label()),
                        detail: "continue on another model; account-wide quota may still apply"
                            .into(),
                    }),
            );
        }
        if rows.is_empty() {
            self.push_system("usage limit reached; /use add another account to continue");
        } else {
            self.show_picker(
                PickerKind::Continuation,
                "continue on which account?".into(),
                rows,
                true,
            );
        }
    }
    pub(in crate::tui) fn open_remove_picker(&mut self, named: Option<String>) {
        let all = crate::connect::removable(&self.state_root);
        if all.is_empty() {
            self.push_system("no accounts to remove");
            return;
        }
        if let Some(id) = named {
            let want = crate::accounts::normalize_account_id(&id);
            match all.into_iter().find(|r| r.id == want) {
                Some(r) => self.open_remove_confirm(r),
                None => self.push_system(format!("no account named {id} - try /use remove")),
            }
            return;
        }
        let rows = all
            .iter()
            .map(|r| {
                let what = if r.owned {
                    "created by Concordia - its login folder can be deleted"
                } else if r.stored {
                    "registered with Concordia - its files stay where they are"
                } else {
                    "your own login - it can only be hidden, never deleted"
                };
                let who = r
                    .home
                    .as_ref()
                    .map(|h| format_identity_label(&inspect_provider(r.provider, h)))
                    .unwrap_or_default();
                PickerRow {
                    id: format!("rmpick:{}", r.id),
                    label: format!("{}  {who}", self.named_account(r.provider, &r.id)),
                    detail: what.into(),
                }
            })
            .collect();
        self.show_picker(
            PickerKind::Remove,
            "remove which account?".into(),
            rows,
            true,
        );
    }
    pub(in crate::tui) fn open_remove_confirm(&mut self, r: crate::connect::Removable) {
        use crate::connect::RemoveMode;
        let mut rows: Vec<PickerRow> = r
            .modes()
            .into_iter()
            .map(|m| {
                let (key, label, detail) = match m {
                    RemoveMode::DeleteData => (
                        "delete",
                        "Remove and delete its login data",
                        r.home
                            .as_ref()
                            .map(|h| h.display().to_string())
                            .unwrap_or_default(),
                    ),
                    RemoveMode::Unregister => (
                        "unreg",
                        "Remove from Concordia, keep the files",
                        "nothing on disk changes".to_string(),
                    ),
                    RemoveMode::Hide => (
                        "hide",
                        "Hide from Concordia",
                        "your files are not touched; it simply stops being listed".to_string(),
                    ),
                };
                PickerRow {
                    id: format!("rm:{key}:{}", r.id),
                    label: label.into(),
                    detail,
                }
            })
            .collect();
        rows.push(PickerRow {
            id: "rm:cancel".into(),
            label: "Cancel".into(),
            detail: String::new(),
        });
        let title = format!("remove {}?", display_account_name(r.provider, &r.id));
        self.show_picker(PickerKind::RemoveConfirm, title, rows, false);
    }
    pub(in crate::tui) fn perform_remove(&mut self, id: &str, mode: crate::connect::RemoveMode) {
        if self.account_changes_blocked
            || self.busy
            || self.preparing_cancel.is_some()
            || self.cancel_handle.is_some()
            || self.deleg.d.any_active()
        {
            self.push_system("Finish or cancel running chats and workers before removing an account. No account or native history was changed.");
            return;
        }
        crate::logging::info(&format!("remove-account id={id} mode={mode:?}"));
        match crate::connect::remove_account(&self.state_root, id, mode) {
            Err(e) => self.push_system(format!("remove failed - {e}")),
            Ok(msg) => {
                self.push_system(msg);
                if crate::accounts::normalize_account_id(&self.account)
                    == crate::accounts::normalize_account_id(id)
                {
                    // The account in use is gone: move to another so the next turn has a home.
                    match accounts_with_auth(&self.state_root).into_iter().next() {
                        Some(next) => {
                            if select_account(&self.state_root, &next.account).is_ok() {
                                self.refresh_account_continuity();
                                self.push_system(format!("now using {}", self.account_label()));
                            }
                        }
                        None => {
                            self.prior_provider = self.provider;
                            self.prior_home = self.continuity_home();
                            self.account.clear();
                            self.home_path = None;
                            self.native_session_id = None;
                            self.native_owner = None;
                            self.native_accounting_owner = None;
                            self.native_usage_model = None;
                            self.native_usage_session = None;
                            self.account_email = None;
                            self.header_email_revealed = false;
                            self.model = "?".into();
                            self.session_ui.invalidate();
                            self.need_context_pack = self.messages.iter().any(|message| {
                                matches!(message.role, MsgRole::User | MsgRole::Assistant)
                            });
                            self.push_system("no accounts left - /use add to connect one");
                        }
                    }
                }
            }
        }
    }
    pub(in crate::tui) fn open_add_provider_picker(&mut self) {
        let rows = [
            (ProviderKind::Codex, "Codex", "codex"),
            (ProviderKind::Claude, "Claude", "claude"),
            (ProviderKind::Cursor, "Cursor", "agent"),
        ]
        .into_iter()
        .map(|(p, title, bin)| PickerRow {
            id: format!("add:{}", p.as_str()),
            label: title.to_string(),
            detail: if which::which(bin).is_ok()
                || (p == ProviderKind::Cursor && which::which("cursor-agent").is_ok())
            {
                "CLI found".into()
            } else {
                format!("{bin} not found on PATH")
            },
        })
        .collect();
        self.show_picker(PickerKind::AddProvider, "add account".into(), rows, false);
    }
    pub(in crate::tui) fn begin_add_account(&mut self, provider: ProviderKind) {
        if self.pending_login.is_some() {
            self.push_system("a login is already in progress - finish it first");
            return;
        }
        match (self.connect)(&self.state_root, provider) {
            Ok(AddResult {
                account,
                provider,
                label,
                home,
                message,
                watch,
            }) => {
                for l in message {
                    self.push_system(l);
                }
                if watch {
                    self.pending_login = Some(PendingLogin {
                        account,
                        provider,
                        label,
                        home,
                        ticks: 0,
                    });
                }
            }
            Err(e) => self.push_system(format!("add account failed - {e}")),
        }
    }
    pub(in crate::tui) fn open_model_picker(&mut self) {
        let chosen = Some(self.model.trim()).filter(|m| !m.is_empty() && *m != "?");
        let rows = model_picker_rows(
            crate::models::available_at(&self.state_root, self.provider, &self.account),
            chosen,
        );
        let selected = rows
            .iter()
            .position(|r| r.label.ends_with(" · current"))
            .unwrap_or(0);
        self.show_picker_at(PickerKind::Model, "model".into(), rows, true, selected);
        self.footer_left = "up/down Enter · r refresh · type to filter · Esc".into();
        if !cfg!(test)
            && crate::models::needs_refresh_at(&self.state_root, self.provider, &self.account)
        {
            self.refresh_model_picker();
        }
    }

    pub(in crate::tui) fn open_provider_picker(&mut self) {
        let rows = crate::accounts::accounts_with_auth(&self.state_root)
            .into_iter()
            .filter_map(|account| {
                let home = account.home_path?;
                let snap = self.usage_fetcher.snapshot(account.provider, &account.account, &home);
                Some(PickerRow {
                    id: format!("provider:{}", account.account),
                    label: format!("{} · {}", account.provider.as_str(), self.named_account(account.provider, &account.account)),
                    detail: snap.used_left_text(),
                })
            })
            .collect();
        self.show_picker(PickerKind::Provider, "providers and accounts".into(), rows, false);
    }

    pub(in crate::tui) fn open_effort_picker(&mut self) {
        let Some(model) = crate::models::find_at(
            &self.state_root,
            self.provider,
            &self.account,
            &self.model,
        ) else {
            self.push_system("effort - provider did not report options for this model");
            return;
        };
        let rows = model.efforts.into_iter().map(|effort| PickerRow {
            id: format!("effort:{}", effort.name),
            label: format!("{}{}", effort.name, if self.effort.as_deref() == Some(effort.name.as_str()) { " · current" } else { "" }),
            detail: "provider-reported model effort".into(),
        }).collect();
        self.show_picker(PickerKind::Effort, format!("effort · {}", model.display_name), rows, false);
    }
    pub(in crate::tui) fn refresh_model_picker(&mut self) {
        if self.model_refresh.is_some() {
            return;
        }
        let provider = self.provider;
        let account = self.account.clone();
        let state_root = self.state_root.clone();
        let (tx, rx) = std_mpsc::channel();
        std::thread::spawn({
            let account = account.clone();
            move || {
                let _ = tx.send(crate::models::refresh_at(&state_root, provider, &account));
            }
        });
        self.model_refresh = Some(ModelRefresh {
            provider,
            account,
            rx,
        });
        if let Some(picker) = self.picker.as_mut() {
            picker.title = "model · refreshing...".into();
        }
        self.footer_left = "refreshing... · Esc closes · UI remains responsive".into();
    }
    pub(in crate::tui) fn show_picker(
        &mut self,
        kind: PickerKind,
        title: String,
        rows: Vec<PickerRow>,
        filterable: bool,
    ) {
        self.show_picker_at(kind, title, rows, filterable, 0);
    }
    pub(in crate::tui) fn open_doctor(&mut self) {
        let checks = crate::doctor::run_fast_checks_at(&self.state_root);
        let fixes = crate::doctor::proposed_fixes(&self.state_root, &checks);
        self.doctor = Some(DoctorUi {
            checks,
            fixes,
            fixed: HashMap::new(),
        });
        self.refresh_doctor_picker();
    }
    pub(in crate::tui) fn refresh_doctor_picker(&mut self) {
        let Some(doctor) = self.doctor.as_ref() else {
            return;
        };
        let rows = doctor
            .checks
            .iter()
            .enumerate()
            .map(|(index, check)| {
                let tag = match check.level {
                    crate::doctor::Level::Ok => "ok",
                    crate::doctor::Level::Warn => "warn",
                    crate::doctor::Level::Fail => "FAIL",
                };
                let fix_index = doctor
                    .fixes
                    .iter()
                    .position(|fix| fix.finding == check.detail);
                let suffix = doctor
                    .fixed
                    .get(&index)
                    .map(|s| format!(" · fixed ({s})"))
                    .unwrap_or_default();
                PickerRow {
                    id: fix_index.map_or_else(
                        || format!("doctor:check:{index}"),
                        |i| format!("doctor:fix:{i}"),
                    ),
                    label: format!("{tag}  {}{suffix}", check.name),
                    detail: fix_index.map_or_else(
                        || check.detail.clone(),
                        |i| {
                            format!(
                                "before: {} · after: {} · Enter to review fix",
                                doctor.fixes[i].before, doctor.fixes[i].after
                            )
                        },
                    ),
                }
            })
            .collect();
        self.show_picker(PickerKind::Doctor, "doctor".into(), rows, false);
        self.footer_left = "↑↓ select · Enter review fix · Esc".into();
    }
    pub(in crate::tui) fn open_doctor_confirmation(&mut self, fix_index: usize) {
        let Some(fix) = self.doctor.as_ref().and_then(|d| d.fixes.get(fix_index)) else {
            return;
        };
        let row = PickerRow {
            id: format!("doctor-confirm:{fix_index}"),
            label: format!(
                "Apply {}?\nbefore: {}\nafter: {}",
                fix.finding, fix.before, fix.after
            ),
            detail: format!("impact: {} · y apply / n cancel", fix.impact),
        };
        self.show_picker(
            PickerKind::DoctorConfirm,
            "confirm doctor fix".into(),
            vec![row],
            false,
        );
        self.footer_left = "y apply · n cancel · Esc".into();
    }
    pub(in crate::tui) fn apply_doctor_fix(&mut self, fix_index: usize) {
        let Some(fix) = self
            .doctor
            .as_ref()
            .and_then(|d| d.fixes.get(fix_index))
            .cloned()
        else {
            return;
        };
        let result =
            crate::doctor::apply_fix_confirmed(&self.state_root, &fix, true, &*self.doctor_applier);
        match result {
            Ok(message) => {
                let backup = message
                    .split("undo from ")
                    .nth(1)
                    .unwrap_or("unknown backup")
                    .to_string();
                let checks = crate::doctor::run_fast_checks_at(&self.state_root);
                if let Some(doctor) = self.doctor.as_mut() {
                    doctor.checks = checks;
                    doctor.fixes = crate::doctor::proposed_fixes(&self.state_root, &doctor.checks);
                    if let Some(index) = doctor.checks.iter().position(|c| c.detail == fix.finding)
                    {
                        doctor.fixed.insert(index, backup.clone());
                    }
                }
                self.refresh_doctor_picker();
                self.flash(format!("fixed (backup: {backup})"));
            }
            Err(error) => {
                self.dismiss_picker();
                self.push_system(format!("doctor fix failed · {error}"));
            }
        }
    }
    pub(in crate::tui) fn show_picker_at(
        &mut self,
        kind: PickerKind,
        title: String,
        rows: Vec<PickerRow>,
        filterable: bool,
        idx: usize,
    ) {
        let mut state = ListState::default();
        if !rows.is_empty() {
            state.select(Some(idx.min(rows.len() - 1)));
        }
        self.picker = Some(Picker {
            kind,
            title,
            rows,
            filter: String::new(),
            state,
        });
        self.footer_left = if filterable {
            "↑↓ Enter · type to filter · Esc".into()
        } else {
            if kind == PickerKind::Usage {
                "↑↓ Enter · r refresh · Esc".into()
            } else {
                "↑↓ Enter · Esc".into()
            }
        };
    }
    pub(in crate::tui) fn update_progress(&mut self) {
        let showing = self
            .picker
            .as_ref()
            .is_some_and(|p| p.kind == PickerKind::Update);
        let Some(job) = self.job.as_ref().filter(|j| j.kind == JobKind::Update) else {
            return;
        };
        if !showing {
            return;
        }
        let status = job.status();
        let step = if status.is_empty() {
            "starting".to_string()
        } else {
            status.chars().take(120).collect()
        };
        use crate::update::PreviewLine as P;
        self.update_preview = Some(crate::update::UpdatePreview {
            history: Vec::new(),
            history_expanded: false,
            lines: vec![
                P::Title(format!("{} Updating Concordia", spinner(self.tick_count))),
                P::Field(
                    "Elapsed",
                    crate::delegate::fmt_elapsed(job.started.elapsed()),
                ),
                P::Field("Step", step),
                P::Blank,
                P::Text("Messages are paused until Concordia restarts on the new build.".into()),
                P::Blank,
                P::Hint("Esc cancels the update".into()),
            ],
            install_available: false,
            release_url: None,
        });
        self.footer_left = "Esc cancels the update".into();
    }
    pub(in crate::tui) fn close_update_screen(&mut self) {
        if self
            .picker
            .as_ref()
            .is_some_and(|p| p.kind == PickerKind::Update)
        {
            self.picker = None;
            self.update_preview = None;
            self.footer_left = FOOTER_HINT.into();
        }
    }
    pub(in crate::tui) fn dismiss_picker(&mut self) {
        let restore_handoff = self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.kind == PickerKind::Continuation)
            && self.continuation.pending_prompt().is_some()
            && self.editor.text().trim().is_empty();
        self.permission_row_pressed = None;
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.kind == PickerKind::Related)
        {
            self.session_ui.memory_pending = None;
            self.session_ui.memory_screen = None;
        }
        tokens::close(&mut self.token_reads);
        // Only the update screen owns a job: its check, or the update it started.
        if self
            .picker
            .as_ref()
            .is_some_and(|p| p.kind == PickerKind::Update)
        {
            if let Some(job) = &self.job {
                job.cancel();
            }
        }
        self.picker = None;
        self.update_preview = None;
        self.footer_left = FOOTER_HINT.into();
        if restore_handoff {
            if let Some(prompt) = self.continuation.pending_prompt().map(str::to_owned) {
                self.restore_composer(&prompt);
            }
        }
    }
    pub(in crate::tui) fn open_update_release(&mut self) {
        let Some(url) = self
            .update_preview
            .as_ref()
            .and_then(|p| p.release_url.clone())
        else {
            return;
        };
        if !url.starts_with("https://") {
            self.flash("refusing non-https release URL");
            return;
        }
        #[cfg(not(test))]
        {
            // Explorer is intentionally visible: this is the requested URL window.
            let _ = crate::process_ui::visible_command("explorer.exe")
                .arg(url)
                .spawn();
        }
        #[cfg(test)]
        self.flash("release page opened");
    }
    pub(in crate::tui) fn apply_picker_selection(
        &mut self,
        turn_tx: Option<&mpsc::UnboundedSender<String>>,
    ) {
        if self.picker.as_ref().is_some_and(|picker| {
            matches!(
                picker.kind,
                PickerKind::BrandCustom | PickerKind::ThemeImport | PickerKind::ThemeExport
            )
        }) {
            appearance::accept_custom(self);
            return;
        }
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        let filtered = picker.filtered();
        let Some(idx) = picker.state.selected() else {
            return;
        };
        let Some(row) = filtered.get(idx) else {
            return;
        };
        let id = row.id.clone();
        let label = row.label.clone();
        let kind = picker.kind;
        if kind == PickerKind::Doctor {
            if let Some(index) = id.strip_prefix("doctor:fix:").and_then(|s| s.parse().ok()) {
                self.open_doctor_confirmation(index);
            }
            return;
        }
        if kind == PickerKind::Model && id.starts_with("model-group:") {
            return;
        }
        if kind == PickerKind::Agents && id == crate::tui::native_agents::GROUP_ID {
            self.toggle_agents_row(&id);
            return;
        }
        self.dismiss_picker();
        if self.busy
            && matches!(
                kind,
                PickerKind::Resume | PickerKind::Use | PickerKind::Model | PickerKind::Usage
            )
        {
            self.push_system("finish or cancel the current turn before switching conversation, provider or model");
            return;
        }
        match kind {
            PickerKind::Settings | PickerKind::FilePreferences | PickerKind::Customization => {
                settings::choose(self, kind, &id)
            }
            PickerKind::Idle => match id.as_str() {
                "idle:wait" => {}
                "idle:cancel" => self.request_cancel(),
                "idle:handoff" => {
                    let has_other = accounts_with_auth(&self.state_root)
                        .iter()
                        .any(|a| a.account != self.account && a.home_path.is_some());
                    if has_other {
                        self.idle_handoff();
                    } else {
                        self.push_system("hand off disabled: no other signed-in account");
                    }
                }
                _ => {}
            },
            PickerKind::Appearance | PickerKind::Theme | PickerKind::BrandColor => {
                appearance::choose(self, kind, &id)
            }
            PickerKind::BrandCustom => {}
            PickerKind::ThemeImport | PickerKind::ThemeExport => {}
            PickerKind::CreditDetails => match id.as_str() {
                "credit-dashboard" => self.open_host_target(
                    crate::usage::CREDIT_DASHBOARD_URL.into(),
                    "credit balance dashboard",
                ),
                "credit-refresh" => self.show_credit_details(true),
                _ => self.show_credit_details(false),
            },
            PickerKind::Files => {
                if id == "artifact-catalog" {
                    artifacts::open(self);
                    return;
                }
                if let Some(index) = id
                    .strip_prefix("file:")
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    self.preview_related_file(index);
                }
            }
            PickerKind::FileActions => self.choose_file_action(&id),
            PickerKind::Artifacts => artifacts::choose(self, &id),
            PickerKind::ArtifactDetail => {}
            PickerKind::Agents => {
                if let Some(index) = id
                    .strip_prefix("native-agent:")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    if self.messages.get(index).is_some_and(|m| m.card.is_some()) {
                        self.activity.open(index);
                    }
                }
                if let Some(id) = id
                    .strip_prefix("worker-agent:")
                    .and_then(|id| id.parse::<u32>().ok())
                {
                    self.inspect_worker(id);
                }
            }
            PickerKind::NeedsYou => {
                if let Some(item_id) = id.strip_prefix("needs:") {
                    let item = self
                        .needs_you
                        .items()
                        .find(|item| item.id == item_id)
                        .cloned();
                    if let Some(item) = item {
                        landing::close(self);
                        self.activity.detail = None;
                        match item.kind {
                            crate::needs_you::ItemKind::Question
                            | crate::needs_you::ItemKind::Approval => {
                                if let Some(job) = item
                                    .id
                                    .strip_prefix("worker-native:")
                                    .and_then(|value| value.split(':').next())
                                    .and_then(|value| value.parse().ok())
                                {
                                    self.inspect_worker(job);
                                } else {
                                    self.queue.focused = false;
                                    self.questions.focused = true;
                                }
                            }
                            crate::needs_you::ItemKind::PausedQueue => {
                                self.queue.focused = true;
                                self.questions.focused = false;
                                self.flash("Queue paused. Ctrl+R resumes it; Enter edits the selected prompt.");
                            }
                            crate::needs_you::ItemKind::WorkerResult => {
                                if let Some(id) = item
                                    .id
                                    .strip_prefix("worker:")
                                    .and_then(|id| id.parse().ok())
                                {
                                    self.inspect_worker(id);
                                }
                            }
                            crate::needs_you::ItemKind::IdleNotice => self.open_idle_picker(),
                            crate::needs_you::ItemKind::SignedOut => self.open_use_picker(),
                        }
                    }
                }
            }
            PickerKind::Resume => {
                if id == "none" {
                    return;
                }
                if let Some(rest) = id.strip_prefix("resume:") {
                    let mut parts = rest.splitn(2, ':');
                    let account = parts.next().unwrap_or("").to_string();
                    let session_id = parts.next().unwrap_or("").to_string();
                    if session_id.is_empty() {
                        self.push_system("resume failed — missing session id");
                        return;
                    }
                    self.request_resume_session(&account, &session_id);
                }
                if let Some(task_id) = id.strip_prefix("task:") {
                    let Some(task) = crate::store::task(&self.state_root, task_id) else {
                        self.push_system("resume failed: task is no longer present");
                        return;
                    };
                    let diagnostic = crate::sessions::diagnose_task(&self.state_root, &task);
                    if diagnostic.cause().is_some() {
                        self.open_chat_repair(task, diagnostic);
                        return;
                    }
                    let Some(session) = task.sessions.last() else {
                        self.push_system("resume failed: task has no native sessions");
                        return;
                    };
                    if self.request_resume_session(&session.account, &session.native_session_id) {
                        if let Some(request) = self.session_ui.resume_request.as_mut() {
                            request.task = Some(task.id);
                        }
                    }
                }
            }
            PickerKind::ChatRepair => {
                if id == "chat-repair:start" {
                    self.start_chat_repair();
                }
            }
            PickerKind::Trust => {
                if let Some(mode) = id.strip_prefix("trust:") {
                    let trust = match mode {
                        "plan" => TrustMode::Plan,
                        "ask" => TrustMode::Ask,
                        "full" => TrustMode::Full,
                        _ => TrustMode::Edit,
                    };
                    if let Err(e) = set_trust_mode(&self.state_root, trust) {
                        self.push_system(format!("trust save failed — {e}"));
                        return;
                    }
                    self.trust = trust;
                    self.push_system(match trust {
                        TrustMode::Plan => "trust → plan (read-only: analyse and propose)",
                        TrustMode::Full => "trust → full (skip all permissions — careful)",
                        TrustMode::Edit => "trust → edit (cwd write, no trust dialogs)",
                        TrustMode::Ask => "trust → ask (may hang headless)",
                    });
                    if self.busy {
                        self.push_system("permission change applies to the next turn; the running turn keeps its original permissions");
                    }
                }
            }
            PickerKind::FullModeConfirm => {
                if id == "full-yes" {
                    self.full_mode_confirmed = true;
                    let _ = self.set_mode(TrustMode::Full);
                }
            }
            PickerKind::Remove => {
                if let Some(acct) = id.strip_prefix("rmpick:") {
                    self.open_remove_picker(Some(acct.to_string()));
                }
            }
            PickerKind::RemoveConfirm => {
                use crate::connect::RemoveMode;
                let mut parts = id.splitn(3, ':');
                let (_, mode, acct) = (parts.next(), parts.next(), parts.next());
                match (mode, acct) {
                    (Some("delete"), Some(a)) => self.perform_remove(a, RemoveMode::DeleteData),
                    (Some("unreg"), Some(a)) => self.perform_remove(a, RemoveMode::Unregister),
                    (Some("hide"), Some(a)) => self.perform_remove(a, RemoveMode::Hide),
                    _ => {}
                }
            }
            PickerKind::AddProvider => {
                if let Some(p) = id
                    .strip_prefix("add:")
                    .and_then(crate::commands::parse_provider)
                {
                    self.begin_add_account(p);
                }
            }
            PickerKind::Use => {
                if id == "use-handoff" {
                    self.open_handoff_settings();
                    return;
                }
                if id == "use-grok-bot-setup" {
                    self.open_grok_bot_form(None);
                    return;
                }
                if let Some(routine_id) = id.strip_prefix("use-grok-bot:") {
                    self.show_picker(PickerKind::GrokBotActions, "Grok Bot routine".into(), vec![
                        PickerRow { id: format!("edit:{routine_id}"), label: "edit routine".into(), detail: "replace URL or key".into() },
                        PickerRow { id: format!("rotate:{routine_id}"), label: "rotate callback token".into(), detail: "invalidate the previous token".into() },
                        PickerRow { id: format!("remove:{routine_id}"), label: "remove routine".into(), detail: "stop listening when this is the last routine".into() },
                    ], false);
                    return;
                }
                if id == "use-add" {
                    self.open_add_provider_picker();
                    return;
                }
                if id == "use-remove" {
                    self.open_remove_picker(None);
                    return;
                }
                if let Some(account) = id.strip_prefix("use:") {
                    if let Err(e) = select_account(&self.state_root, account) {
                        self.push_system(format!("account switch failed — {e}"));
                        return;
                    }
                    self.refresh_account_continuity();
                    let home = self.home_path.as_ref();
                    let idn = home
                        .map(|h| inspect_provider(self.provider, h))
                        .map(|i| format_identity_label(&i))
                        .unwrap_or_else(|| "unknown".into());
                    self.push_system(format!(
                        "using {} · {idn} · {}",
                        self.account_label(),
                        self.model
                    ));
                }
            }
            PickerKind::GrokBotActions => {
                let (action, routine_id) = id.split_once(':').unwrap_or(("", ""));
                if action == "edit" {
                    self.open_grok_bot_form(crate::grok_bot::list(&self.state_root).into_iter().find(|r| r.routine_id == routine_id));
                } else if action == "rotate" {
                    match crate::grok_bot::rotate_callback_token(&self.state_root) { Ok(_) => self.push_system("Grok Bot callback token rotated"), Err(e) => self.push_system(format!("callback token rotation failed · {e}")) }
                } else if action == "remove" {
                    if let Some(r) = crate::grok_bot::list(&self.state_root).into_iter().find(|r| r.routine_id == routine_id) { let _ = crate::grok_bot::remove(&self.state_root, &r.provider, &r.account, &r.routine_id); }
                    self.build_use_picker();
                }
            }
            PickerKind::GrokBotForm => {}
            PickerKind::Model => {
                if let Some(model) = id.strip_prefix("model:") {
                    if !model.is_empty() && !valid_model_id(model) {
                        self.push_system("model id must be letters, digits or . _ - : / [ ]");
                        return;
                    }
                    // A first run has no saved selection yet; save the account already shown.
                    if get_use_selection(&self.state_root).is_none() {
                        let shown = UseSelection {
                            provider: self.provider,
                            account: self.account.clone(),
                            home_path: self
                                .home_path
                                .as_ref()
                                .map(|p| p.to_string_lossy().into_owned()),
                            model: None,
                            effort: None,
                            updated_at: now_stamp(),
                        };
                        if let Err(e) = set_use_selection(&self.state_root, shown) {
                            self.push_system(format!("model save failed — {e}"));
                            return;
                        }
                    }
                    if let Err(e) = set_model(&self.state_root, model) {
                        self.push_system(format!("model save failed — {e}"));
                        return;
                    }
                    self.model = if model.is_empty() { "?" } else { model }.to_string();
                    if model.is_empty() {
                        self.push_system("model → provider default");
                    } else {
                        self.push_system(format!("model → {model}"));
                    }
                }
            }
            PickerKind::Effort => {
                if let Some(effort) = id.strip_prefix("effort:") {
                    self.effort = Some(effort.to_owned());
                    if let Some(entry) = crate::models::find_at(
                        &self.state_root,
                        self.provider,
                        &self.account,
                        &self.model,
                    )
                    {
                        let selected_id = entry.id_for_effort(Some(effort)).to_owned();
                        if selected_id != self.model && !selected_id.is_empty() {
                            let _ = set_model(&self.state_root, &selected_id);
                            self.model = selected_id;
                        }
                    }
                    if let Some(mut selection) = get_use_selection(&self.state_root) {
                        selection.effort = self.effort.clone();
                        let _ = set_use_selection(&self.state_root, selection);
                    }
                    self.push_system(format!("effort → {effort}"));
                }
            }
            PickerKind::Provider => {
                if let Some(account) = id.strip_prefix("provider:") {
                    if let Err(error) = select_account(&self.state_root, account) {
                        self.push_system(format!("provider switch failed · {error}"));
                    } else {
                        self.refresh_account_continuity();
                    }
                }
            }
            PickerKind::Usage => {
                if let Some(account) = id.strip_prefix("usage:") {
                    if account == self.account {
                        return;
                    }
                    if let Err(e) = select_account(&self.state_root, account) {
                        self.push_system(format!("usage switch failed — {e}"));
                        return;
                    }
                    self.refresh_account_continuity();
                    self.push_system(format!("using {} · {}", self.account_label(), self.model));
                }
            }
            PickerKind::Continuation => {
                if let Some(account) = id.strip_prefix("continue:") {
                    if let Some(tx) = turn_tx {
                        self.continue_usage(account, tx);
                    } else {
                        self.continuation_target = Some(account.to_string());
                    }
                }
            }
            PickerKind::HandoffSettings => {
                if let Some(value) = id.strip_prefix("handoff:") {
                    self.set_session_handoff(value);
                }
            }
            PickerKind::ResumeDeleteConfirm => {
                if let Some(id) = id.strip_prefix("resume-delete:") {
                    match crate::threads::resolve_prefix(&self.state_root, id) {
                        Ok(full) => match crate::threads::delete(&self.state_root, &full) {
                            Ok(()) => self.push_system(format!("deleted thread {full}")),
                            Err(e) => self.push_system(format!("delete failed: {e:#}")),
                        },
                        Err(e) => self.push_system(e.to_string()),
                    }
                }
            }
            PickerKind::Update
            | PickerKind::Tokens
            | PickerKind::Related
            | PickerKind::Integrations
            | PickerKind::Doctor
            | PickerKind::DoctorConfirm => {}
        }
        let _ = label;
    }
}
