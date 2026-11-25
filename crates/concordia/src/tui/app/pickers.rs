use super::super::*;

impl App {
    pub(in crate::tui) fn open_guided_command(&mut self, command: &str, choices: Vec<PickerRow>) {
        let purpose = match command {
            "/terminal" => "This panel opens, hides, or closes a terminal.",
            "/rename" => "This panel changes the current chat title.",
            "/native" => "This panel changes native provider transport for an account.",
            "/mcp" => "This panel manages the local MCP server registry.",
            "/help" => "This panel shows the available slash commands.",
            "/close" => "This panel closes an open file pane.",
            "/queue" => "This panel reviews and controls queued prompts.",
            _ => "This panel guides the command and previews its effect.",
        };
        self.show_picker(
            PickerKind::Guided,
            format!("{command} · {purpose}"),
            choices,
            false,
        );
        self.footer_left = "↑↓ move · Enter select · Esc close".into();
    }

    pub(in crate::tui) fn guide_open_picker(&mut self, command: &str, does: &str, undo: &str) {
        if let Some(picker) = self.picker.as_mut() {
            picker.title = format!("{command} · This panel lets you {does}.");
            if command != "/harness" {
                self.footer_left = if undo.starts_with("Esc") {
                    "↑↓ move · Enter select · Esc close".into()
                } else {
                    format!("↑↓ move · Enter select · Esc close · undo: {undo}")
                };
            }
        }
        if command == "/model" {
            self.footer_left =
                "↑↓ move · Enter select · R refresh · type to filter · Esc close".into();
        }
    }

    pub(in crate::tui) fn open_resume_picker(&mut self) {
        self.refresh_store_warning();
        self.show_picker(
            PickerKind::Resume,
            "chats".into(),
            vec![PickerRow {
                id: "resume:loading".into(),
                label: "Loading chats…".into(),
                detail: String::new(),
            }],
            true,
        );
        self.footer_left = "↑↓ move · Enter select · type to filter · Esc close".into();
        let state_root = self.state_root.clone();
        let cwd = self.cwd.clone();
        self.start_picker_load(PickerKind::Resume, "concordia-resume-list", move || {
            let (title, rows) = build_resume_picker_payload(&state_root, &cwd);
            PickerLoadPayload::Resume { title, rows }
        });
    }

    fn start_picker_load(
        &mut self,
        kind: PickerKind,
        thread_name: &str,
        work: impl FnOnce() -> PickerLoadPayload + Send + 'static,
    ) {
        let (tx, rx) = std_mpsc::channel();
        self.picker_load_generation = self.picker_load_generation.wrapping_add(1);
        let generation = self.picker_load_generation;
        let _ = std::thread::Builder::new()
            .name(thread_name.into())
            .spawn(move || {
                let _ = tx.send(work());
            });
        self.picker_load = Some(PickerLoad {
            generation,
            kind,
            rx,
        });
    }

    pub(in crate::tui) fn poll_picker_load(&mut self) -> bool {
        let Some(load) = self.picker_load.as_ref() else {
            return false;
        };
        let generation = load.generation;
        let expected = load.kind;
        let result = match load.rx.try_recv() {
            Ok(payload) => payload,
            Err(std_mpsc::TryRecvError::Empty) => return false,
            Err(std_mpsc::TryRecvError::Disconnected) => {
                self.picker_load = None;
                return false;
            }
        };
        self.picker_load = None;
        if generation != self.picker_load_generation {
            return false;
        }
        if self
            .picker
            .as_ref()
            .is_none_or(|picker| picker.kind != expected)
        {
            return false;
        }
        match result {
            PickerLoadPayload::Resume { title, rows } => {
                let Some(picker) = self.picker.as_mut() else {
                    return false;
                };
                let filter = picker.filter.clone();
                picker.title = title;
                picker.rows = rows;
                picker.filter = filter;
                picker.select_clamp();
                true
            }
            PickerLoadPayload::Doctor { checks, fixes } => {
                self.doctor = Some(DoctorUi {
                    checks,
                    fixes,
                    fixed: HashMap::new(),
                });
                self.refresh_doctor_picker();
                true
            }
        }
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
        let conversation = self.deleg.d.conversation_id().to_owned();
        let mut questions = self.questions.saved(&conversation);
        let previous_recovery = self.keep.lease.clone();
        self.handle_slash("/new");
        if self.deleg.d.conversation_id() == conversation {
            return;
        }
        if questions.waiting() {
            questions.conversation = self.deleg.d.conversation_id().into();
            if let Err(error) = self
                .questions
                .restore(questions, self.deleg.d.conversation_id())
            {
                self.push_system(error);
                return;
            }
            if let Err(error) = self.persist_draft_checked() {
                self.push_system(error.to_string());
                return;
            }
            if let Some(previous) = previous_recovery {
                let _ = previous.dismiss_current();
            }
        }
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
        self.show_picker_at(
            PickerKind::Trust,
            "permissions · all providers".into(),
            rows,
            false,
            idx,
        );
    }
    pub(in crate::tui) fn refresh_account_continuity(&mut self) {
        let prior_home = self
            .home_path
            .as_ref()
            .map(|home| home.to_string_lossy().into_owned());
        let prior_provider = self.prior_provider;
        let prior_account = self.prior_account.clone();
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
        self.prior_provider = prior_provider;
        self.prior_account = prior_account;
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
        let accounts: Vec<_> = ready
            .iter()
            .filter_map(|a| {
                a.home_path
                    .as_ref()
                    .map(|home| (a.account.clone(), a.provider, home.clone()))
            })
            .collect();
        let rows_data =
            usage_rows_for_accounts(&mut self.usage_fetcher, &accounts, &self.account, false);
        let mut rows = Vec::new();
        for urow in rows_data {
            let Some(a) = ready.iter().find(|a| a.account == urow.id) else {
                continue;
            };
            let heading = crate::accounts::format_account_heading(
                a.provider,
                &a.account,
                self.account_names.get(&a.account).map(|name| name.as_str()),
            );
            let default_name = display_account_name(a.provider, &a.account);
            let label = urow.label.replacen(&default_name, &heading, 1);
            if urow.tone != UsageTone::Mute {
                self.picker_tones
                    .insert(format!("use:{}", a.account), urow.tone);
            } else {
                self.picker_tones.remove(&format!("use:{}", a.account));
            }
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
                label,
                detail,
            });
        }
        rows.push(PickerRow {
            id: "use-add".into(),
            label: "Add account".into(),
            detail: if ready.is_empty() {
                "connect a provider account to start chatting".into()
            } else {
                "connect another provider account".into()
            },
        });
        if !ready.is_empty() {
            rows.push(PickerRow {
                id: "use-remove".into(),
                label: "Remove account".into(),
                detail: "take an account out of Concordia".into(),
            });
        }
        rows.push(PickerRow {
            id: "use-handoff".into(),
            label: "Choose next provider".into(),
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
            label: "Set up Grok Bot routine".into(),
            detail: "webhook key stays in Windows Credential Manager; see Grok Bot setup guide"
                .into(),
        });
        self.show_picker(PickerKind::Use, "accounts".into(), rows, true);
    }
    fn open_grok_bot_form(&mut self, existing: Option<crate::grok_bot::Routine>) {
        let form = existing.map_or_else(crate::tui::GrokBotForm::default, |r| {
            crate::tui::GrokBotForm {
                account: r.account,
                routine_id: r.routine_id,
                name: r.name,
                webhook_url: r.webhook_url,
                ..Default::default()
            }
        });
        self.grok_bot_form = Some(form);
        self.show_picker(
            PickerKind::GrokBotForm,
            "Grok Bot routine · Tab next · final Enter saves".into(),
            vec![
                PickerRow {
                    id: "account".into(),
                    label: "Account".into(),
                    detail: String::new(),
                },
                PickerRow {
                    id: "routine".into(),
                    label: "Routine ID".into(),
                    detail: String::new(),
                },
                PickerRow {
                    id: "name".into(),
                    label: "Display name".into(),
                    detail: String::new(),
                },
                PickerRow {
                    id: "url".into(),
                    label: "Webhook URL (HTTPS)".into(),
                    detail: String::new(),
                },
                PickerRow {
                    id: "key".into(),
                    label: "Webhook key ·••••••••".into(),
                    detail: "stored only in Windows Credential Manager".into(),
                },
            ],
            false,
        );
    }

    pub(in crate::tui) fn grok_bot_form_key(&mut self, key: crossterm::event::KeyEvent) {
        let Some(form) = self.grok_bot_form.as_mut() else {
            return;
        };
        let active = form.active;
        match key.code {
            crossterm::event::KeyCode::Esc => {
                self.grok_bot_form = None;
                self.dismiss_picker();
            }
            crossterm::event::KeyCode::Tab => form.active = (active + 1) % 5,
            crossterm::event::KeyCode::Enter if active < 4 => form.active += 1,
            crossterm::event::KeyCode::Backspace => match active {
                0 => {
                    form.account.pop();
                }
                1 => {
                    form.routine_id.pop();
                }
                2 => {
                    form.name.pop();
                }
                3 => {
                    form.webhook_url.pop();
                }
                _ => {
                    form.key.pop();
                }
            },
            crossterm::event::KeyCode::Char(c) if !c.is_control() => match active {
                0 => form.account.push(c),
                1 => form.routine_id.push(c),
                2 => form.name.push(c),
                3 => form.webhook_url.push(c),
                _ => form.key.push(c),
            },
            crossterm::event::KeyCode::Enter => {
                let routine = crate::grok_bot::Routine {
                    provider: "grok-bot".into(),
                    account: form.account.trim().into(),
                    routine_id: form.routine_id.trim().into(),
                    name: form.name.trim().into(),
                    description: String::new(),
                    webhook_url: form.webhook_url.trim().into(),
                };
                let result = crate::grok_bot::register(&self.state_root, routine, &form.key);
                self.grok_bot_form = None;
                self.dismiss_picker();
                match result {
                    Ok(()) => self.push_system(
                        "Grok Bot routine saved; key is stored only in Credential Manager",
                    ),
                    Err(e) => self.push_system(format!("Grok Bot setup failed · {e}")),
                }
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
            .filter(|a| a.home_path.is_some())
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
        let current_account = self.account.clone();
        let current_model = self.model.clone();
        let same_account_allowed = self.continuation.same_account_allowed();
        let rows = accounts
            .into_iter()
            .flat_map(|a| {
                let state_root = self.state_root.clone();
                let models = a
                    .home_path
                    .as_ref()
                    .map(|home| {
                        crate::models::ids_at(&self.state_root, a.provider, &a.account, home)
                    })
                    .unwrap_or_default();
                let candidate_account = a.account.clone();
                let current_account = current_account.clone();
                let current_model = current_model.clone();
                models
                    .into_iter()
                    .filter(move |model| {
                        candidate_account != current_account
                            || *model != current_model
                            || same_account_allowed
                    })
                    .map(move |model| {
                        let observation = a.home_path.as_ref().and_then(|home| {
                            crate::usage::peek_cached_usage_for_model(
                                &state_root,
                                a.provider,
                                &a.account,
                                home,
                                Some(&model),
                            )
                        });
                        let remaining = observation.as_ref().and_then(|o| o.remaining_pct);
                        let available = observation.as_ref().is_some_and(|o| {
                            !o.exhausted && o.remaining_pct.is_some_and(|remaining| remaining > 0.0)
                        });
                        let reset = observation.and_then(|o| o.reset);
                        PickerRow {
                            id: format!(
                                "{}{} {model}",
                                if available {
                                    "continue:"
                                } else {
                                    "continue-disabled:"
                                },
                                a.account
                            ),
                            label: format!("{} → {model}", a.label),
                            detail: if !available && reset.is_some() {
                                format!(
                                    "at limit · {}",
                                    reset.as_deref().unwrap_or(crate::usage::RESET_UNKNOWN)
                                )
                            } else if !available {
                                "usage unavailable · capacity must be confirmed before continuing"
                                    .into()
                            } else {
                                remaining.map_or_else(
                                    || "usage unavailable · continue the paused request".into(),
                                    |p| {
                                        format!(
                                            "{}% left · continue the paused request",
                                            p.round() as i64
                                        )
                                    },
                                )
                            },
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let (mut exhausted_rows, mut available_rows): (Vec<_>, Vec<_>) =
            rows.into_iter().partition(PickerRow::disabled);
        available_rows.sort_by(|a, b| {
            let left = a
                .detail
                .split('%')
                .next()
                .and_then(|value| value.parse::<f64>().ok());
            let right = b
                .detail
                .split('%')
                .next()
                .and_then(|value| value.parse::<f64>().ok());
            right
                .partial_cmp(&left)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut rows = available_rows;
        let fallback = crate::config::for_root(&self.state_root)
            .continuation
            .fallback_models
            .get("general")
            .cloned()
            .filter(|model| {
                rows.iter().any(|row| {
                    row.id.starts_with("continue:") && row.id.ends_with(&format!(" {model}"))
                })
            });
        if let Some(model) = fallback {
            if let Some(index) = rows.iter().position(|row| {
                row.id.starts_with("continue:") && row.id.ends_with(&format!(" {model}"))
            }) {
                let mut row = rows.remove(index);
                row.label = format!("Fallback · {}", row.label);
                row.detail = format!("configured fallback for the general role · {}", row.detail);
                rows.insert(0, row);
            }
        }
        rows.append(&mut exhausted_rows);
        let reset = self
            .home_path
            .as_ref()
            .and_then(|home| {
                crate::usage::peek_cached_usage(
                    &self.state_root,
                    self.provider,
                    &self.account,
                    home,
                )
            })
            .and_then(|o| o.reset);
        rows.push(PickerRow {
            id: "continue:wait".into(),
            label: "Wait for the reset".into(),
            detail: reset.map_or_else(
                || "resume when the provider reports capacity again".into(),
                |r| format!("resume automatically when {r} passes"),
            ),
        });
        rows.push(PickerRow {
            id: "continue:stop".into(),
            label: "Stop here".into(),
            detail: "keep the interrupted request for later".into(),
        });
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
                    match accounts_with_auth(&self.state_root).into_iter().next() {
                        Some(next) => {
                            if select_account(&self.state_root, &next.account).is_ok() {
                                self.refresh_account_continuity();
                                self.push_system(format!("now using {}", self.account_label()));
                            }
                        }
                        None => {
                            self.prior_provider = self.provider;
                            self.prior_account = self.account.clone();
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
            Err(e) => {
                if let Some(command) = crate::connect::official_install_command(provider)
                    .filter(|_| crate::connect::cli_missing(provider))
                {
                    self.show_picker(
                        PickerKind::AddProvider,
                        format!("install {} CLI?", provider.as_str()),
                        vec![PickerRow {
                            id: format!("install:{}", provider.as_str()),
                            label: format!("Install {} CLI", provider.as_str()),
                            detail: command.into(),
                        }],
                        false,
                    );
                    self.footer_left = "Enter run official installer · Esc close".into();
                    self.push_system(format!(
                        "{e}. Install it with `{command}` in your own terminal, or confirm below to open the installer window."
                    ));
                } else {
                    self.push_system(format!("add account failed - {e}"));
                }
            }
        }
    }
    pub(in crate::tui) fn open_model_picker(&mut self) {
        let chosen = Some(self.model.clone()).filter(|model| !model.is_empty() && model != "?");
        self.open_model_picker_for(chosen.as_deref(), "model");
    }

    pub(in crate::tui) fn open_role_model_picker(&mut self, role: String) {
        let chosen = crate::store::get_settings(&self.state_root)
            .roles
            .get(&role)
            .and_then(|role| role.model.clone())
            .or_else(|| Some(self.model.clone()).filter(|model| !model.is_empty() && model != "?"));
        self.role_editing = Some(role);
        self.open_model_picker_for(chosen.as_deref(), "role model");
    }

    fn open_model_picker_for(&mut self, chosen: Option<&str>, title: &str) {
        let rows = model_picker_rows(
            crate::models::available_at(&self.state_root, self.provider, &self.account),
            chosen,
        );
        let selected = rows
            .iter()
            .position(|r| r.label.ends_with(" · current"))
            .unwrap_or(0);
        self.show_picker_at(PickerKind::Model, title.into(), rows, true, selected);
        self.footer_left = "↑↓ move · Enter select · R refresh · type to filter · Esc close".into();
        if !cfg!(test)
            && crate::models::needs_refresh_at(&self.state_root, self.provider, &self.account)
        {
            self.refresh_model_picker();
        }
    }

    pub(in crate::tui) fn open_provider_picker(&mut self) {
        let signed_in = crate::accounts::accounts_with_auth(&self.state_root);
        let mut rows = Vec::new();
        let mut offered = std::collections::HashSet::new();
        for account in crate::accounts::list_all_accounts(&self.state_root) {
            let Some(home) = account.home_path else {
                continue;
            };
            let connected = signed_in.iter().any(|a| a.account == account.account);
            let provider_has_login = signed_in.iter().any(|a| a.provider == account.provider);
            if !connected
                && !account.custom
                && (provider_has_login || !offered.insert(account.provider))
            {
                continue;
            }
            let id = format!("provider:{}", account.account);
            let (tone, detail) = if connected {
                let snap = self
                    .usage_fetcher
                    .snapshot(account.provider, &account.account, &home);
                let detail = snap.used_left_text();
                let detail = if detail.is_empty() {
                    crate::usage::NO_USAGE_DATA.into()
                } else {
                    detail
                };
                (crate::usage::usage_tone(&snap), detail)
            } else {
                (UsageTone::Mute, "not connected".into())
            };
            if !connected || tone != UsageTone::Mute {
                self.picker_tones.insert(id.clone(), tone);
            } else {
                self.picker_tones.remove(&id);
            }
            let name = self.named_account(account.provider, &account.account);
            let current = account.account == self.account;
            rows.push(PickerRow {
                id,
                label: format!(
                    "{} · {}{}",
                    account.provider.as_str(),
                    name,
                    if current { " · current" } else { "" }
                ),
                detail,
            });
        }
        let current = format!("provider:{}", self.account);
        let selected = rows.iter().position(|row| row.id == current).unwrap_or(0);
        self.show_picker_at(
            PickerKind::Provider,
            "providers and accounts".into(),
            rows,
            false,
            selected,
        );
    }

    pub(in crate::tui) fn toggle_header_dropdown(&mut self, kind: PickerKind, chip: Rect) {
        let open = self.header_dropdown.is_some_and(|(open, _)| open == kind)
            && self
                .picker
                .as_ref()
                .is_some_and(|picker| picker.kind == kind);
        if open {
            self.dismiss_picker();
            return;
        }
        match kind {
            PickerKind::Model => self.open_model_picker(),
            PickerKind::Effort => self.open_effort_picker(),
            PickerKind::Provider => self.open_provider_picker(),
            _ => return,
        }
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.kind == kind)
        {
            self.header_dropdown = Some((kind, chip));
        }
    }

    pub(in crate::tui) fn open_effort_picker(&mut self) {
        if self.busy {
            self.push_system("Finish or cancel the current turn before changing effort.");
            return;
        }
        let model = self.model.clone();
        self.open_model_efforts(&model);
    }

    fn open_model_efforts(&mut self, model_id: &str) {
        let Some(model) =
            crate::models::find_at(&self.state_root, self.provider, &self.account, model_id)
        else {
            self.flash("No provider-reported effort options for this model. Refresh /model to check its catalog.");
            return;
        };
        if model.efforts.is_empty() {
            self.flash(format!(
                "{} does not report adjustable effort.",
                model.display_name
            ));
            return;
        }
        let role_effort = self.role_editing.as_ref().and_then(|role| {
            crate::store::get_settings(&self.state_root)
                .roles
                .get(role)
                .and_then(|role| role.effort.clone())
        });
        let saved_effort = get_use_selection(&self.state_root).and_then(|sel| sel.effort);
        let preferred_owned = model
            .efforts
            .iter()
            .find(|effort| effort.model_id.as_deref() == Some(model_id))
            .map(|effort| effort.name.clone())
            .or(role_effort)
            .or_else(|| {
                model.effort_for_preference(self.effort.as_deref().or(saved_effort.as_deref()))
            });
        let preferred = preferred_owned.as_deref();
        let selected = model
            .efforts
            .iter()
            .position(|effort| Some(effort.name.as_str()) == preferred)
            .unwrap_or(0);
        let rows = model
            .efforts
            .iter()
            .map(|effort| PickerRow {
                id: format!("effort:{}", effort.name),
                label: effort.name.clone(),
                detail: if self.role_editing.as_ref().is_some_and(|role| {
                    crate::store::get_settings(&self.state_root)
                        .roles
                        .get(role)
                        .and_then(|role| role.effort.as_deref())
                        == Some(&effort.name)
                }) || self.model == model_id
                    && self.effort.as_deref() == Some(&effort.name)
                {
                    "Current effort".into()
                } else if model.default_effort.as_deref() == Some(&effort.name) {
                    "Provider default".into()
                } else {
                    String::new()
                },
            })
            .collect();
        self.show_picker_at(
            PickerKind::Effort,
            format!("{} \u{b7} Choose effort", model.display_name),
            rows,
            false,
            selected,
        );
        self.effort_model = Some(model_id.to_owned());
        self.footer_left = "\u{2191}\u{2193} move \u{b7} Enter select \u{b7} Esc close".into();
    }

    pub(in crate::tui) fn choose_effort(&mut self, effort: &str) {
        if self.busy {
            self.push_system("Finish or cancel the current turn before changing effort.");
            return;
        }
        let Some(model) =
            crate::models::find_at(&self.state_root, self.provider, &self.account, &self.model)
        else {
            self.push_system("Choose a provider-reported model with /model before setting effort.");
            return;
        };
        if let Err(error) = self.commit_model_effort(&model, Some(effort)) {
            self.push_system(error);
        }
    }

    fn commit_model_effort(
        &mut self,
        model: &crate::models::ModelEntry,
        effort: Option<&str>,
    ) -> Result<(), String> {
        if effort.is_some_and(|selected| {
            !model
                .efforts
                .iter()
                .any(|candidate| candidate.name == selected)
        }) {
            return Err(format!(
                "Unsupported effort for {}. Available: {}",
                model.display_name,
                model.effort_names().join(", ")
            ));
        }
        let selected_model = model.id_for_effort(effort).to_owned();
        let selection = UseSelection {
            provider: self.provider,
            account: self.account.clone(),
            home_path: self
                .home_path
                .as_ref()
                .map(|home| home.to_string_lossy().into_owned()),
            model: Some(selected_model.clone()),
            effort: effort.map(str::to_owned),
            updated_at: now_stamp(),
        };
        set_use_selection(&self.state_root, selection)
            .map_err(|error| format!("Model and effort were not changed: {error}"))?;
        self.model = selected_model;
        self.effort = effort.map(str::to_owned);
        self.messages.retain(|message| {
            !(message.role == MsgRole::System && message.text.starts_with("using "))
        });
        self.push_system(format!(
            "using {} \u{b7} {}{}",
            self.account_label(),
            self.model_label(),
            effort
                .map(|effort| format!(" \u{b7} effort {effort}"))
                .unwrap_or_default()
        ));
        Ok(())
    }

    fn commit_role_effort(
        &mut self,
        role: &str,
        model: &crate::models::ModelEntry,
        effort: Option<&str>,
    ) -> Result<(), String> {
        if effort.is_some_and(|selected| {
            !model
                .efforts
                .iter()
                .any(|candidate| candidate.name == selected)
        }) {
            return Err(format!(
                "Unsupported effort for {}. Available: {}",
                model.display_name,
                model.effort_names().join(", ")
            ));
        }
        let selected_model = effort.map_or_else(
            || model.id.clone(),
            |selected| model.id_for_effort(Some(selected)).to_owned(),
        );
        crate::store::set_role_selection(
            &self.state_root,
            role,
            self.provider.as_str(),
            &self.account,
            &selected_model,
            effort,
        )
        .map_err(|error| format!("Role configuration was not changed: {error}"))?;
        self.role_editing = None;
        self.push_system(format!(
            "role {role} → {selected_model}{}",
            effort
                .map(|effort| format!(" · effort {effort}"))
                .unwrap_or_default()
        ));
        crate::tui::settings::open(self);
        Ok(())
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
        self.footer_left = "Refreshing models… · Esc close".into();
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
        self.doctor = None;
        self.show_picker(
            PickerKind::Doctor,
            "doctor".into(),
            vec![PickerRow {
                id: "doctor:loading".into(),
                label: "Checking…".into(),
                detail: String::new(),
            }],
            false,
        );
        self.footer_left = "Esc close".into();
        let state_root = self.state_root.clone();
        self.start_picker_load(PickerKind::Doctor, "concordia-doctor", move || {
            let checks = crate::doctor::run_fast_checks_at(&state_root);
            let fixes = crate::doctor::proposed_fixes(&state_root, &checks);
            PickerLoadPayload::Doctor { checks, fixes }
        });
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
        self.footer_left = "↑↓ move · Enter review fix · Esc close".into();
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
        self.footer_left = "Y apply · N / Esc close".into();
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
        self.header_dropdown = None;
        self.picker = Some(Picker {
            kind,
            title,
            rows,
            filter: String::new(),
            filterable,
            state,
            usage_scroll: 0,
            usage_cols: 1,
        });
        self.footer_left = if filterable {
            if kind == PickerKind::Files {
                "↑↓ move · Enter open · F4 actions · type to filter · Esc close".into()
            } else {
                "↑↓ move · Enter select · type to filter · Esc close".into()
            }
        } else {
            if kind == PickerKind::Usage {
                "↑↓ move · Enter select · r refresh · Esc close".into()
            } else {
                "↑↓ move · Enter select · Esc close".into()
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
        self.update_preview = Some(update_ui::running(
            &job.snapshot(),
            job.started.elapsed(),
            self.update_details,
        ));
        self.footer_left = "D build details \u{b7} Esc cancel update".into();
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
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.kind == PickerKind::SharedCommand)
        {
            self.shared_command = None;
            self.provider_command = None;
        }
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
        if self
            .picker
            .as_ref()
            .is_some_and(|p| p.kind == PickerKind::Update)
        {
            if let Some(job) = &self.job {
                job.cancel();
            }
        }
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| matches!(picker.kind, PickerKind::Model | PickerKind::Effort))
        {
            self.role_editing = None;
        }
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| matches!(picker.kind, PickerKind::Resume | PickerKind::Doctor))
        {
            self.picker_load_generation = self.picker_load_generation.wrapping_add(1);
            self.picker_load = None;
        }
        self.effort_model = None;
        self.header_dropdown = None;
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
            let _ = crate::process_ui::visible_command("explorer.exe")
                .arg(url)
                .spawn();
        }
        #[cfg(test)]
        self.flash("release page opened");
    }
    pub(in crate::tui) fn end_tab_rename(&mut self) {
        if !self.mcp_add_editor {
            self.rename_editor = None;
        }
    }

    pub(in crate::tui) fn begin_mcp_add(&mut self) {
        let mut editor = Editor::default();
        editor.set_text("NAME COMMAND [ARGS...]");
        editor.select_all();
        self.mcp_add_editor = true;
        self.rename_editor = Some(editor);
        self.footer_left = "type NAME COMMAND [ARGS...] · Enter preview · Esc close".into();
    }

    pub(in crate::tui) fn begin_mcp_import(&mut self) {
        let entries = crate::mcp_registry::import_entries(&self.state_root);
        if entries.is_empty() {
            self.push_system("MCP import found no readable provider servers. Nothing changed.");
            return;
        }
        let rows = entries.iter().enumerate().map(|(index, entry)| PickerRow {
            id: format!("mcp:import:{index}"),
            label: format!("Add {} from {}", entry.name, entry.provider),
            detail: format!("read-only source {}; adds a registry reference with no provider-home write; undo: /mcp remove {}", entry.path.display(), entry.name),
        }).collect();
        self.mcp_import_entries = Some(entries);
        self.show_picker(
            PickerKind::Guided,
            "/mcp import · Choose a provider server to add".into(),
            rows,
            false,
        );
        self.footer_left = "↑↓ move · Enter add server · Esc close".into();
    }

    fn apply_mcp_import(&mut self, index: usize) {
        let Some(entry) = self
            .mcp_import_entries
            .as_ref()
            .and_then(|entries| entries.get(index))
            .cloned()
        else {
            self.push_system("MCP import selection is no longer available. Nothing changed.");
            return;
        };
        let result = crate::mcp_registry::mutate(&self.state_root, |registry| {
            if registry.servers.contains_key(&entry.name) {
                return Err(format!("MCP server {} already exists", entry.name));
            }
            registry.servers.insert(
                entry.name.clone(),
                crate::mcp_registry::Server {
                    enabled: true,
                    transport: entry.transport,
                },
            );
            Ok(())
        });
        match result {
            Ok(()) => self.push_system(format!(
                "MCP import added {} from {}. Provider homes were not changed.",
                entry.name, entry.provider
            )),
            Err(error) => self.push_system(format!("MCP import failed: {error}; nothing changed.")),
        }
    }

    pub(in crate::tui) fn finish_mcp_add_editor(&mut self, text: String) {
        let words = text
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if words.len() < 2
            || words[0]
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
        {
            self.push_system(
                "MCP add validation failed: enter a server name and command; nothing changed.",
            );
            return;
        }
        let name = words[0].clone();
        let command = words[1].clone();
        let args = words[2..].to_vec();
        self.show_picker(PickerKind::Guided, "/mcp add · This panel previews the server change.".into(), vec![PickerRow {
            id: "mcp:add:confirm".into(),
            label: format!("Add {name} using {command}"),
            detail: format!("Adds an enabled local stdio server with arguments [{}]. Undo with /mcp remove {name}.", args.join(" ")),
        }], false);
        self.mcp_add_preview = Some((name, command, args));
        self.footer_left = "Enter confirm · Esc close".into();
    }

    pub(in crate::tui) fn apply_mcp_add(&mut self) {
        let Some((name, command, args)) = self.mcp_add_preview.take() else {
            self.push_system("MCP add validation failed: preview is missing; nothing changed.");
            return;
        };
        let result = crate::mcp_registry::mutate(&self.state_root, |registry| {
            if registry.servers.contains_key(&name) {
                return Err(format!("MCP server {name} already exists"));
            }
            registry.servers.insert(
                name.clone(),
                crate::mcp_registry::Server {
                    enabled: true,
                    transport: crate::mcp_registry::Transport::Stdio {
                        command,
                        args,
                        env: Default::default(),
                    },
                },
            );
            Ok(())
        });
        match result {
            Ok(()) => self.push_system(format!(
                "Added the enabled {name} server to the local registry. Undo: /mcp remove {name}."
            )),
            Err(error) => self.push_system(format!("MCP add failed: {error}; nothing changed.")),
        }
    }

    fn toggle_guided_mcp_server(&mut self) {
        let result = crate::mcp_registry::mutate(&self.state_root, |registry| {
            let (name, server) = registry
                .servers
                .iter_mut()
                .next()
                .ok_or("No local MCP server was detected")?;
            server.enabled = !server.enabled;
            Ok((name.clone(), server.enabled))
        });
        match result {
            Ok((name, enabled)) => self.push_system(format!(
                "{name} is now {}. Choose Enable or disable a server again to undo.",
                if enabled { "enabled" } else { "disabled" }
            )),
            Err(error) => self.push_system(format!("MCP toggle failed: {error}; nothing changed.")),
        }
    }

    fn remove_guided_mcp_server(&mut self) {
        let result = crate::mcp_registry::mutate(&self.state_root, |registry| {
            let name = registry
                .servers
                .keys()
                .next()
                .cloned()
                .ok_or("No local MCP server was detected")?;
            registry.servers.remove(&name);
            Ok(name)
        });
        match result {
            Ok(name) => self.push_system(format!(
                "Removed {name}. Add the server again with its command to undo."
            )),
            Err(error) => self.push_system(format!("MCP remove failed: {error}; nothing changed.")),
        }
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
        let shared_command = self.shared_command.clone();
        if row.disabled() {
            return;
        }
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
            self.set_agents_row_open(&id, None);
            return;
        }
        let effort_model = self.effort_model.take();
        let role_editing = self.role_editing.clone();
        let header_anchor = self.header_dropdown;
        self.dismiss_picker();
        if matches!(kind, PickerKind::Model | PickerKind::Effort) {
            self.role_editing = role_editing;
        }
        if self.busy
            && matches!(
                kind,
                PickerKind::Resume
                    | PickerKind::Use
                    | PickerKind::Model
                    | PickerKind::Effort
                    | PickerKind::Usage
            )
        {
            self.push_system("finish or cancel the current turn before switching conversation, provider or model");
            return;
        }
        match kind {
            PickerKind::SharedCommand => {
                let Some(command) = shared_command else {
                    self.push_system("shared command choice expired; nothing was sent");
                    return;
                };
                if id == "shared:concordia" {
                    self.shared_command = Some(command.clone());
                    if let Some(tx) = turn_tx {
                        self.run_line(command, tx);
                    } else {
                        self.handle_slash(&command);
                    }
                    return;
                }
                let Some(account) = id.strip_prefix("shared:account:") else {
                    self.push_system("shared command choice was invalid; nothing was sent");
                    return;
                };
                let Some(def) = accounts_with_auth(&self.state_root)
                    .into_iter()
                    .find(|candidate| candidate.account == account)
                else {
                    self.push_system("shared command account is no longer registered; nothing was sent");
                    return;
                };
                let name = command.split_whitespace().next().unwrap_or_default();
                if def.provider == self.provider && def.account == self.account
                    && self.available_provider_commands().is_some_and(|commands| {
                        !commands.iter().any(|candidate| candidate.name.eq_ignore_ascii_case(name))
                    })
                {
                    self.push_system("The provider no longer advertises this command; nothing was sent.");
                    return;
                }
                if let Err(error) = select_account(&self.state_root, account) {
                    self.push_system(format!("shared command account switch failed · {error}"));
                    return;
                }
                self.refresh_account_continuity();
                if crate::provider::command_supported(def.provider, &command.split_whitespace().next().unwrap_or_default()) {
                    self.provider_command = Some(command.clone());
                    if let Some(tx) = turn_tx {
                        self.run_line(command, tx);
                    } else {
                        self.push_system("provider command selected; press Enter to run it");
                    }
                } else {
                    self.push_command(&command);
                    crate::tui::local_terminal::native_command(self, &command);
                }
            }
            PickerKind::Guided => {
                match id.as_str() {
                    "terminal:open" => {
                        local_terminal::command(self, "open");
                        self.push_system("Opened a local PowerShell pane. Undo: run /terminal close.");
                    }
                    "terminal:native" => {
                        local_terminal::command(self, "native");
                        self.push_system("Requested the provider's native interface. Undo: run /terminal close.");
                    }
                    "terminal:hide" => {
                        local_terminal::command(self, "hide");
                        self.push_system("Hid the terminal; its process is still running. Undo: run /terminal open.");
                    }
                    "terminal:close" => {
                        local_terminal::command(self, "close");
                        self.push_system("Closed the terminal and stopped its process. Undo: run /terminal open.");
                    }
                    "rename:set" => {
                        let mut editor = Editor::default();
                        editor.set_text(self.session_ui.title.as_deref().unwrap_or_default());
                        editor.select_all();
                        self.rename_editor = Some(editor);
                    }
                    "rename:clear" => {
                        if self.rename_session("") {
                            self.push_system("Restored the automatic title; the custom title was cleared. Undo: set the previous title again with /rename.");
                        }
                    }
                    id if id.starts_with("delegate:") => {
                        let mut parts = id.split(':');
                        let _ = parts.next();
                        match (parts.next(), parts.next()) {
                            (Some("fallback"), Some(mode @ ("on" | "off"))) => {
                                self.worker_recovery_command(&format!("fallback {mode}"));
                            }
                            (Some("input"), Some(id)) => match id.parse() {
                                Ok(id) => worker_input::open(self, id),
                                Err(_) => self.push_system("delegate input validation failed; worker id is invalid"),
                            },
                            (Some("recover"), Some(id)) => self.worker_recovery_command(&format!("recover {id}")),
                            (Some("retry"), Some(id)) => self.worker_recovery_command(&format!("retry {id} reconciled")),
                            _ => self.push_system("delegate choice validation failed; nothing changed"),
                        }
                    }
                    "help:everyday" => {
                        self.push_system(format!("Commands (everyday view):\n{}", crate::commands::format_slash_help(false)));
                    }
                    "help:all" => {
                        self.push_system(format!("Commands (all view):\n{}", crate::commands::format_slash_help(true)));
                    }
                    "mcp:add" => self.begin_mcp_add(),
                    "mcp:add:confirm" => self.apply_mcp_add(),
                    "mcp:toggle" => self.toggle_guided_mcp_server(),
                    "mcp:remove" => self.remove_guided_mcp_server(),
                    "mcp:import" => self.begin_mcp_import(),
                    id if id.starts_with("mcp:import:") => {
                        if let Ok(index) = id["mcp:import:".len()..].parse::<usize>() { self.apply_mcp_import(index); }
                    }
                    id if id.starts_with("native:") => {
                        let parts = id.split(':').collect::<Vec<_>>();
                        if let [_, account, action] = parts.as_slice() {
                            let result = crate::native_input::command(&self.state_root, account, action);
                            self.push_system(result.map_or_else(|error| format!("Native transport change failed: {error}"), |message| format!("Updated native transport for {account}; {message}. Undo: choose the opposite transport.")));
                        }
                    }
                    id if id.starts_with("queue:") => match id {
                        "queue:list" => for line in self.queue_command("") { self.push_system(line); },
                        "queue:resume" => for line in queue_controls::resume(self) { self.push_system(line); },
                        "queue:clear" => for line in self.queue_command("clear") { self.push_system(line); },
                        _ => {}
                    },
                    id if id.starts_with("close:") => {
                        if let Some(id) = id.strip_prefix("close:").and_then(|id| id.parse::<u32>().ok()) {
                            self.handle_slash(&format!("/close {id}"));
                        }
                    }
                    "close:none" => self.push_system("No open file panes were found; nothing changed. Undo: not needed."),
                    _ => self.push_system(format!("{label}: no change was made. Esc cancels and leaves the current state unchanged.")),
                }
            }
            PickerKind::SettingsView | PickerKind::FilePreferences | PickerKind::Customization => {
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
                    self.focus_needs_you_item(item_id);
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
                    if trust == TrustMode::Full {
                        self.request_full_mode();
                        return;
                    }
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
                if let Some(provider) = id
                    .strip_prefix("install:")
                    .and_then(crate::commands::parse_provider)
                {
                    match crate::connect::install_provider(provider) {
                        Ok(()) => self.push_system(format!(
                            "{} installer opened. Finish it there, then return and choose Add account again; Concordia will not sign you in automatically or assume the version is compatible.",
                            provider.as_str()
                        )),
                        Err(error) => self.push_system(format!(
                            "{} installer could not start: {error}. Nothing changed; install it yourself with `{}` and retry.",
                            provider.as_str(),
                            crate::connect::official_install_command(provider).unwrap_or_default()
                        )),
                    }
                    return;
                }
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
                    self.show_picker(
                        PickerKind::GrokBotActions,
                        "Grok Bot routine".into(),
                        vec![
                            PickerRow {
                                id: format!("edit:{routine_id}"),
                                label: "edit routine".into(),
                                detail: "replace URL or key".into(),
                            },
                            PickerRow {
                                id: format!("rotate:{routine_id}"),
                                label: "rotate callback token".into(),
                                detail: "invalidate the previous token".into(),
                            },
                            PickerRow {
                                id: format!("remove:{routine_id}"),
                                label: "remove routine".into(),
                                detail: "stop listening when this is the last routine".into(),
                            },
                        ],
                        false,
                    );
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
                    if self.busy {
                        self.push_system("an active turn is still running; finish it before switching accounts");
                        return;
                    }
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
                        self.model_label()
                    ));
                }
            }
            PickerKind::GrokBotActions => {
                let (action, routine_id) = id.split_once(':').unwrap_or(("", ""));
                if action == "edit" {
                    self.open_grok_bot_form(
                        crate::grok_bot::list(&self.state_root)
                            .into_iter()
                            .find(|r| r.routine_id == routine_id),
                    );
                } else if action == "rotate" {
                    match crate::grok_bot::rotate_callback_token(&self.state_root) {
                        Ok(_) => self.push_system("Grok Bot callback token rotated"),
                        Err(e) => self.push_system(format!("callback token rotation failed · {e}")),
                    }
                } else if action == "remove" {
                    if let Some(r) = crate::grok_bot::list(&self.state_root)
                        .into_iter()
                        .find(|r| r.routine_id == routine_id)
                    {
                        let _ = crate::grok_bot::remove(
                            &self.state_root,
                            &r.provider,
                            &r.account,
                            &r.routine_id,
                        );
                    }
                    self.build_use_picker();
                }
            }
            PickerKind::GrokBotForm => {}
            PickerKind::FallbackModel => {
                if let (Some(role), Some(model)) = (
                    self.fallback_role.take(),
                    id.strip_prefix("model:"),
                ) {
                    match crate::config::set_fallback_model(&self.state_root, &role, model) {
                        Ok(()) => self.push_system(format!("fallback for {role} → {model}")),
                        Err(error) => self.push_system(format!("fallback save failed · {error}")),
                    }
                }
            }
            PickerKind::Model => {
                let anchor = header_anchor
                    .filter(|(kind, _)| *kind == PickerKind::Model)
                    .map(|(_, chip)| chip);
                if let Some(model_id) = id.strip_prefix("model:") {
                    let Some(model) = crate::models::find_at(&self.state_root, self.provider, &self.account, model_id) else {
                        self.push_system("Model is no longer in this account's catalog. Refresh /model and choose again.");
                        return;
                    };
                    if let Some(role) = self.role_editing.clone() {
                        if model.efforts.len() > 1 {
                            self.open_model_efforts(model_id);
                        } else if let Err(error) = self.commit_role_effort(
                            &role,
                            &model,
                            model.efforts.first().map(|effort| effort.name.as_str()),
                        ) {
                            self.push_system(error);
                        }
                    } else if model.efforts.len() > 1 {
                        self.open_model_efforts(model_id);
                        if let Some(chip) = anchor.filter(|_| {
                            self.picker.as_ref().is_some_and(|p| p.kind == PickerKind::Effort)
                        }) {
                            self.header_dropdown = Some((PickerKind::Effort, chip));
                        }
                    } else {
                        let effort = model.efforts.first().map(|effort| effort.name.as_str());
                        if let Err(error) = self.commit_model_effort(&model, effort) { self.push_system(error); }
                    }
                }
            }
            PickerKind::Effort => {
                if let Some(effort) = id.strip_prefix("effort:") {
                    let model_id = effort_model.as_deref().unwrap_or(&self.model);
                    if let Some(model) = crate::models::find_at(&self.state_root, self.provider, &self.account, model_id) {
                        if let Some(role) = self.role_editing.clone() {
                            if let Err(error) = self.commit_role_effort(&role, &model, Some(effort)) {
                                self.push_system(error);
                            }
                        } else if let Err(error) = self.commit_model_effort(&model, Some(effort)) {
                            self.push_system(error);
                        }
                    } else {
                        self.push_system("Model choices changed. Open /model again; the previous selection was kept.");
                    }
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
                if id == "usage-add" {
                    self.open_add_provider_picker();
                } else if let Some(account) = id.strip_prefix("usage:") {
                    if account == self.account {
                        return;
                    }
                    if let Err(e) = select_account(&self.state_root, account) {
                        self.push_system(format!("usage switch failed — {e}"));
                        return;
                    }
                    self.refresh_account_continuity();
                    self.push_system(format!("using {} · {}", self.account_label(), self.model_label()));
                }
            }
            PickerKind::Continuation => {
                if id == "continue:wait" {
                    self.wait_for_usage_reset();
                } else if id == "continue:stop" {
                    self.continuation.invalidate();
                    self.push_system("stopped here; the interrupted request remains unsent");
                } else if let Some(account) = id.strip_prefix("continue:") {
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

fn build_resume_picker_payload(state_root: &Path, cwd: &Path) -> (String, Vec<PickerRow>) {
    let sessions = list_recent_sessions(state_root, 40);
    let tasks = crate::store::list_tasks(state_root);
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
    let diagnostics = crate::sessions::diagnose_tasks(state_root, &tasks);
    for row in &mut rows {
        let Some(task_id) = row.id.strip_prefix("task:") else {
            continue;
        };
        if let Some(diagnostic) = diagnostics.iter().find(|d| d.task_id == task_id) {
            if let Some(cause) = diagnostic.cause() {
                row.detail = format!(
                    "unopenable · {} · Enter to repair (or run chat repair; draft kept)",
                    cause.label()
                );
            }
        }
    }
    rows.extend(native_session_picker_rows(&native));
    let here = count_sessions_here(&sessions, Some(cwd));
    let folder = cwd
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
    (title, rows)
}
