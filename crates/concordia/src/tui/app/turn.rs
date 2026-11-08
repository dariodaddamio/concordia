use super::super::*;

impl App {
    pub(in crate::tui) fn apply_progress(&mut self, ev: TurnProgress) {
        let feed = crate::feed::FeedWriter::new(
            &self.state_root,
            self.thread_id().unwrap_or("main"),
            crate::config::for_root(&self.state_root).feed.enabled,
        );
        match &ev {
            TurnProgress::Text(text) => {
                let _ = feed.emit("text", text);
            }
            TurnProgress::TextDelta(text) => {
                let _ = feed.emit_text_delta(text);
            }
            TurnProgress::Card(card) => {
                let _ = feed.emit("tool", &card.title);
            }
            TurnProgress::Interaction(_) => {
                let _ = feed.emit("question", "native approval/question waiting");
            }
            TurnProgress::Error(error) => {
                let _ = feed.emit("error", error);
            }
            TurnProgress::Usage(usage) => {
                let _ = feed.emit("usage", format!("{:?}", usage));
            }
            _ => {}
        }
        match ev {
            TurnProgress::Artifact(record) => {
                if self.native_owner.as_ref() == Some(&record.owner)
                    && record.owner.conversation_id == self.deleg.d.conversation_id()
                {
                    self.commit_live_thinking();
                    self.commit_live_text();
                    if !artifacts::record_live(self, &record) {
                        self.push_card(record.card());
                    }
                }
            }
            TurnProgress::Interaction(request) => {
                if self.cancel_requested
                    || !self.busy
                    || !self
                        .native_owner
                        .as_ref()
                        .is_some_and(|owner| request.is_owned_by(owner))
                {
                    request.invalidate();
                    return;
                }
                let plan = matches!(
                    request.kind(),
                    crate::provider::interaction::NativeInteraction::PlanApproval { .. }
                );
                let codex_review = matches!(
                    request.kind(),
                    crate::provider::interaction::NativeInteraction::CodexPermission { .. }
                );
                if codex_review {
                    let evidence = self
                        .messages
                        .iter()
                        .rev()
                        .filter_map(|message| message.card.as_ref())
                        .find(|card| Some(card.id.as_str()) == request.params["itemId"].as_str())
                        .and_then(|card| card.detail.as_deref())
                        .unwrap_or("");
                    let detail = format!(
                        "{evidence}\n\nNative request:\n{}",
                        serde_json::to_string_pretty(&request.params).unwrap_or_default()
                    );
                    self.push_card(StreamCard {
                        id: format!(
                            "native-review:{}:{}",
                            request.envelope().connection_id,
                            request.envelope().request_id
                        ),
                        title: "Native action awaiting your review".into(),
                        detail: Some(detail),
                        status: CardStatus::Running,
                    });
                    if self.picker.is_none() && self.activity.detail.is_none() {
                        self.activity.open(self.messages.len() - 1);
                    }
                }
                if plan {
                    self.push_card(StreamCard {
                        id: format!(
                            "native-plan:{}:{}",
                            request.envelope().connection_id,
                            request.envelope().request_id
                        ),
                        title: "Plan awaiting your review".into(),
                        detail: Some(format!(
                            "{}\n\n{}\n\nTasks:\n{}",
                            request.params["overview"].as_str().unwrap_or(""),
                            request.params["plan"].as_str().unwrap_or(""),
                            serde_json::to_string_pretty(&request.params["todos"])
                                .unwrap_or_default()
                        )),
                        status: CardStatus::Running,
                    });
                    if self.picker.is_none() && self.activity.detail.is_none() {
                        self.activity.open(self.messages.len() - 1);
                    }
                }
                self.questions.offer_native(
                    *request,
                    !plan && !codex_review && self.editor.text().is_empty(),
                );
                self.pause_queue();
            }
            TurnProgress::Session(id) => {
                self.native_usage_session = Some(id.clone());
                self.native_session_id = Some(id);
                self.remember_native_source();
                self.save_turn_checkpoint(false);
                self.persist_session_title();
                if let (Some(task_id), Some(session_id)) =
                    (self.task_id.clone(), self.native_session_id.clone())
                {
                    let map = Some(crate::store::TaskMapPointer {
                        version: 2,
                        thread_id: self.thread_id().unwrap_or(task_id.as_str()).to_owned(),
                    });
                    if let Err(error) = crate::store::record_task_session(
                        &self.state_root,
                        &task_id,
                        self.session_ui.title.as_deref().unwrap_or("untitled task"),
                        &self.cwd.to_string_lossy(),
                        self.provider,
                        &self.account,
                        &session_id,
                        map,
                    ) {
                        self.flash(format!("task link could not be saved: {error}"));
                    }
                }
            }
            TurnProgress::Thinking(t) => {
                self.clear_idle_notice();
                self.commit_live_text();
                self.phase = Phase::Thinking;
                self.live_thinking.push_str(&t);
                if !t.ends_with('\n') {
                    self.live_thinking.push('\n');
                }
            }
            TurnProgress::ThinkingDelta(t) => {
                self.clear_idle_notice();
                self.commit_live_text();
                self.phase = Phase::Thinking;
                self.live_thinking.push_str(&t);
            }
            TurnProgress::Text(t) => {
                self.clear_idle_notice();
                self.commit_live_thinking();
                self.phase = Phase::Writing;
                self.live_text.push_str(&t);
                if !t.ends_with('\n') {
                    self.live_text.push('\n');
                }
            }
            TurnProgress::TextDelta(t) => {
                self.clear_idle_notice();
                self.commit_live_thinking();
                self.phase = Phase::Writing;
                self.live_text.push_str(&t);
            }
            TurnProgress::Card(card) => {
                self.clear_idle_notice();
                self.commit_live_thinking();
                self.commit_live_text();
                self.phase = if card.status == CardStatus::Running {
                    Phase::Tool(card.title.clone())
                } else {
                    Phase::Waiting
                };
                let id = card.id.clone();
                upsert_card(&mut self.live_cards, card);
                if let Some(merged) = self.live_cards.iter().find(|c| c.id == id).cloned() {
                    self.push_card(merged);
                }
            }
            TurnProgress::Notice(n) => self.push_system(n),
            TurnProgress::Idle {
                provider, minutes, ..
            } => {
                let text = format!(
                    "No output from {} for {} minutes. Esc cancels; click here (or press Ctrl+K) for options.",
                    Self::idle_provider_name(provider),
                    minutes
                );
                if let Some(line) = self.messages.iter_mut().find(|line| {
                    line.meta
                        .as_ref()
                        .is_some_and(|(_, label)| label == "__concordia_idle_notice__")
                }) {
                    line.text = text;
                } else {
                    self.messages.push(ChatLine {
                        role: MsgRole::System,
                        text,
                        card: None,
                        meta: Some((provider, "__concordia_idle_notice__".into())),
                    });
                    self.idle_notice = Some(provider);
                }
            }
            TurnProgress::ChatReferences { maps, notices } => {
                self.show_chat_refs(chatref::Expansion {
                    maps,
                    notices,
                    ..Default::default()
                });
            }
            TurnProgress::Error(e) => {
                if self.continuation.errors.len() < 12 {
                    self.continuation
                        .errors
                        .push(e.chars().take(2000).collect());
                }
                self.push_system(e);
            }
            TurnProgress::Usage(u) => {
                self.turn_usage.update_snapshot(&u);
            }
        }
    }
    pub(in crate::tui) fn commit_live_text(&mut self) {
        let text = std::mem::take(&mut self.live_text);
        if !text.trim().is_empty() {
            self.push_assistant(text.trim().to_string());
            self.streamed_text_committed = true;
        }
    }
    pub(in crate::tui) fn commit_live_thinking(&mut self) {
        let text = std::mem::take(&mut self.live_thinking);
        if !text.is_empty() {
            let meta = Some(self.reply_meta());
            self.messages.push(ChatLine {
                role: MsgRole::Thinking,
                text,
                card: None,
                meta,
            });
        }
    }
    pub(in crate::tui) fn visible_tip(&mut self) -> Option<&'static str> {
        let quiet = !self.busy
            && self.picker.is_none()
            && self.job.is_none()
            && self.pending_login.is_none()
            && self.editor.chars().is_empty();
        let fresh = self.messages.iter().all(|m| m.role == MsgRole::System);
        if !quiet
            || !(fresh || self.idle_ticks >= tips::IDLE_TICKS)
            || !tips::enabled(self.appearance.ui.tips)
        {
            return None;
        }
        let tip = tips::contextual_tip(
            !self.queue.items.is_empty(),
            self.busy,
            self.usage_warning.is_some(),
            self.tip_seed,
        );
        self.tip_limiter.take(tip)
    }
    pub(in crate::tui) fn record_tokens(
        &mut self,
        usage: &crate::provider::stream_cards::TurnUsage,
        outcome: &TurnOutcome,
    ) {
        let owner = self
            .native_accounting_owner
            .as_ref()
            .or(self.native_owner.as_ref());
        let provider = owner.map_or(self.provider, |owner| owner.provider);
        let (new_input, cached_input, output) = crate::token_log::split(provider, usage);
        let entry = crate::token_log::Entry {
            at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            provider: provider.as_str().to_string(),
            account: owner.map_or_else(|| self.account.clone(), |owner| owner.account.clone()),
            model: if owner.is_some() {
                self.native_usage_model.clone()
            } else {
                (!self.model.is_empty() && self.model != "?").then(|| self.model.clone())
            },
            project: owner
                .map_or(self.cwd.as_path(), |owner| owner.execution_cwd.as_path())
                .to_string_lossy()
                .into_owned(),
            chat: outcome.session_id.clone().or_else(|| {
                if owner.is_some() {
                    self.native_usage_session.clone()
                } else {
                    self.native_session_id.clone()
                }
            }),
            new_input,
            cached_input,
            output,
            cost_usd: usage.cost_usd,
            attribution: owner.map(|owner| {
                let (status, complete) = crate::token_log::attempt_outcome(
                    outcome.exit_code,
                    outcome.cancelled,
                    outcome.failed,
                );
                crate::token_log::Attribution::new(
                    crate::token_log::Origin::Main,
                    &owner.run_id,
                    &owner.attempt_id,
                    owner.home.as_deref(),
                    usage,
                    status,
                    complete,
                )
            }),
        };
        if let Err(e) = crate::token_log::record(&self.state_root, &entry) {
            crate::logging::info(&format!("token log not written: {e}"));
            self.push_system(format!(
                "Usage could not be saved: {e}. Native usage is not assumed to be zero."
            ));
        }
    }
    pub(in crate::tui) fn finalize_turn(&mut self, outcome: TurnOutcome) {
        let feed = crate::feed::FeedWriter::new(
            &self.state_root,
            self.thread_id().unwrap_or("main"),
            crate::config::for_root(&self.state_root).feed.enabled,
        );
        let _ = feed.flush_text_delta();
        let _ = feed.emit(
            "turn_end",
            if outcome.failed {
                "failed"
            } else {
                "completed"
            },
        );
        self.clear_idle_notice();
        self.busy_send_confirmation = None;
        self.submitted_prompt = None;
        let elapsed = self.turn_started.map(|t| t.elapsed());
        let usage = std::mem::take(&mut self.turn_usage);
        if !usage.is_empty()
            || self.native_owner.is_some()
            || self.native_accounting_owner.is_some()
        {
            self.record_tokens(&usage, &outcome);
        }
        self.session_input = self.session_input.saturating_add(usage.input.unwrap_or(0));
        self.session_output = self
            .session_output
            .saturating_add(usage.output.unwrap_or(0));
        self.session_cost += usage.cost_usd.unwrap_or(0.0);
        // Events only: never the prompt or the reply.
        crate::logging::info(&format!(
            "turn provider={} account={} exit={:?} cancelled={} ms={} stderr_lines={}",
            self.provider.as_str(),
            self.account,
            outcome.exit_code,
            outcome.cancelled,
            self.turn_started
                .take()
                .map(|t| t.elapsed().as_millis())
                .unwrap_or(0),
            outcome.stderr_tail.lines().count()
        ));
        if let Some(sid) = outcome.session_id {
            self.native_session_id = Some(sid);
            self.persist_session_title();
        }
        let had_cards = !self.live_cards.is_empty() || !outcome.cards.is_empty();
        let failed = !outcome.cancelled
            && (outcome.failed
                || match outcome.exit_code {
                    Some(code) => code != 0,
                    None => !outcome.stderr_tail.trim().is_empty(),
                });
        let finish_incomplete = |mut card: StreamCard| {
            card.status = CardStatus::Error;
            let reason = if outcome.cancelled {
                "Interrupted: turn cancelled before the provider reported tool completion."
                    .to_string()
            } else if failed {
                match outcome.exit_code {
                    Some(code) => format!(
                        "Provider failed with exit code {code} before reporting tool completion."
                    ),
                    None => "Provider failed before reporting tool completion.".to_string(),
                }
            } else {
                "Incomplete: provider stream ended before reporting tool completion.".to_string()
            };
            card.detail = Some(match card.detail.take() {
                Some(detail) if !detail.is_empty() => format!("{detail}\n{reason}"),
                _ => reason,
            });
            card
        };
        // Final output may contain edits completed after the last progress event was drained.
        // Reconcile by id even when some cards have already streamed.
        for card in outcome.cards {
            let id = card.id.clone();
            upsert_card(&mut self.live_cards, card);
            if let Some(card) = self.live_cards.iter().find(|card| card.id == id).cloned() {
                self.push_card(card);
            }
        }
        let running: Vec<_> = self
            .live_cards
            .iter()
            .filter(|card| card.status == CardStatus::Running)
            .cloned()
            .collect();
        for card in running {
            self.push_card(finish_incomplete(card));
        }
        let text = if !self.live_text.trim().is_empty() {
            self.live_text.trim().to_string()
        } else if !self.streamed_text_committed && !outcome.cancelled {
            outcome.text.trim().to_string()
        } else {
            String::new()
        };
        let scan_text = if outcome.cancelled {
            String::new()
        } else if !outcome.text.is_empty() {
            outcome.text.clone()
        } else {
            text.clone()
        };
        // A provider that failed to start, or exited non-zero, must say why. Its stderr is the
        // only place the reason (expired login, bad flag, missing CLI) ever appears.
        let thought = self.live_thinking.clone();
        if !thought.is_empty() {
            let meta = Some(self.reply_meta());
            self.messages.push(ChatLine {
                role: MsgRole::Thinking,
                text: thought,
                card: None,
                meta,
            });
        }
        let has_reply = !text.is_empty() || self.streamed_text_committed;
        if has_reply && !failed && !outcome.cancelled {
            self.questions.offer(&scan_text);
        }
        if !text.is_empty() {
            self.push_assistant(text);
        } else if !had_cards && !failed && !outcome.cancelled && !self.streamed_text_committed {
            self.push_system("(no text in provider stream)");
        }
        if has_reply || had_cards {
            let tokens = if token_line::enabled(self.appearance.ui.token_line) {
                token_line::format_line(&usage, None)
            } else {
                String::new()
            };
            let line = format!(
                "Worked for {} · stopped {}{}",
                crate::delegate::fmt_elapsed(elapsed.unwrap_or_default()),
                crate::usage::local_time_label(),
                if tokens.is_empty() {
                    String::new()
                } else {
                    format!(" · {tokens}")
                }
            );
            if !line.is_empty() {
                self.messages.push(ChatLine {
                    role: MsgRole::Meta,
                    text: line,
                    card: None,
                    meta: None,
                });
            }
        }
        if outcome.cancelled {
            self.push_system("cancelled - queued work paused; completed actions were not undone");
        }
        if failed && outcome.stderr_tail.contains("already has an active writer") {
            self.push_system("this Codex session is open in another client or pane. Close its active turn, then retry the restored prompt; /new starts a separate conversation. Concordia will not replay it automatically.");
        }
        if failed {
            self.push_system(match outcome.exit_code {
                Some(0) => "provider reported a turn failure".to_string(),
                Some(code) => format!("provider exited with code {code}"),
                None => "provider could not run".to_string(),
            });
            let tail = outcome.stderr_tail.trim();
            if tail.contains("apply_patch verification failed") {
                self.push_card(StreamCard {
                    id: format!("provider-diagnostics:{}", self.messages.len()),
                    title: "Provider diagnostics: patch verification failed".into(),
                    detail: Some(format!("These are tool diagnostics retained by the provider; they may predate its exit. They do not establish why the process stopped.\n\n{tail}")),
                    status: CardStatus::Error,
                });
                self.push_system("A patch did not match the current file. Inspect the diagnostics and current files before retrying; completed edits remain in the workspace.");
            }
            let lines: Vec<&str> = tail.lines().collect();
            let said: Vec<String> = self
                .messages
                .iter()
                .rev()
                .take(8)
                .filter(|m| m.role == MsgRole::System)
                .map(|m| m.text.trim().to_string())
                .collect();
            for l in lines.iter().skip(lines.len().saturating_sub(6)) {
                let t = l.trim();
                // Skip CLI chatter and anything already reported above.
                if tail.contains("apply_patch verification failed")
                    || t.is_empty()
                    || is_cli_noise(t)
                    || said.iter().any(|s| s == t)
                {
                    continue;
                }
                self.push_system(format!("  {}", l.trim_end()));
            }
        }
        self.live_cards.clear();
        self.live_text.clear();
        self.streamed_text_committed = false;
        self.live_thinking.clear();
        self.turn_finished(outcome.cancelled, failed);
        self.scan_delegation(&scan_text);
        // The turn used some of this account's limits: re-check them in the background.
        self.usage_fetcher.invalidate(&self.account);
        self.refresh_usage();
        self.update_usage_block();
    }
    pub(in crate::tui) fn threads_command(&mut self, arg: &str) -> Vec<String> {
        use crate::threads;
        if let Some(id) = arg.strip_prefix("delete").map(str::trim) {
            if id.is_empty() {
                return vec!["usage: /threads delete <id>".into()];
            }
            return match threads::resolve_prefix(&self.state_root, id) {
                Ok(full) if Some(full.as_str()) == self.thread_id() => {
                    vec!["that is the current conversation; /new first".into()]
                }
                Ok(full) => match threads::delete(&self.state_root, &full) {
                    Ok(()) => vec![format!("deleted thread {full}")],
                    Err(e) => vec![format!("delete failed: {e:#}")],
                },
                Err(e) => vec![e.to_string()],
            };
        }
        let metas = threads::list(&self.state_root);
        if metas.is_empty() {
            return vec![if threads::enabled(&self.state_root) {
                "no recorded threads yet".into()
            } else {
                "thread log is off (set CONCORDIA_THREADS=1 to record conversations)".into()
            }];
        }
        metas
            .iter()
            .take(15)
            .map(|m| {
                format!(
                    "{}  {:>4}  {}  {}",
                    m.id.chars().take(8).collect::<String>(),
                    threads::age_label(m.started),
                    m.account.as_deref().unwrap_or("-"),
                    m.title
                )
            })
            .collect()
    }
    pub(in crate::tui) fn pack_messages(&self) -> Vec<PackMessage> {
        self.messages
            .iter()
            .filter_map(|m| match m.role {
                MsgRole::User => Some(PackMessage {
                    role: PackRole::User,
                    text: m.text.clone(),
                }),
                MsgRole::Assistant => Some(PackMessage {
                    role: PackRole::Assistant,
                    text: m.text.clone(),
                }),
                MsgRole::Card => m.card.as_ref().map(|c| PackMessage {
                    role: PackRole::Card,
                    text: crate::threads::encode_card(
                        &c.title,
                        c.detail.as_deref(),
                        match c.status {
                            CardStatus::Error => "error",
                            CardStatus::Running => "running",
                            CardStatus::Done => "done",
                        },
                    ),
                }),
                MsgRole::System | MsgRole::Command | MsgRole::Thinking | MsgRole::Meta => None,
            })
            .collect()
    }
    pub(in crate::tui) fn on_tick(&mut self) -> bool {
        let mut visible_change = self.busy && self.appearance.ui.motion;
        self.poll_reference_journal();
        self.questions.refresh_native();
        self.poll_images();
        self.raster.poll();
        self.tick_count = self.tick_count.wrapping_add(1);
        if self.busy {
            self.pulse_on = self.tick_count.is_multiple_of(2);
        } else {
            self.pulse_on = false;
        }
        if self.busy || !self.editor.chars().is_empty() || self.picker.is_some() {
            self.idle_ticks = 0;
        } else {
            self.idle_ticks = self.idle_ticks.saturating_add(1);
        }
        visible_change |= self.idle_ticks == tips::IDLE_TICKS;
        self.persist_tick();
        visible_change |= self.poll_file_jobs();
        self.poll_session_history();
        self.poll_memory();
        self.poll_project_search();
        visible_change |= self.workspace.poll();
        if let Some(notice) = self.workspace.take_notice() {
            self.flash(notice);
        }
        if self.workspace.file_quit_requested() {
            self.should_quit = true;
        }
        if self
            .picker
            .as_ref()
            .is_some_and(|p| p.kind == PickerKind::Files)
        {
            self.request_related_files();
        }
        if self.related_files.poll()
            && self
                .picker
                .as_ref()
                .is_some_and(|p| p.kind == PickerKind::Files)
        {
            let old = self.picker.take().unwrap();
            let selected = old
                .state
                .selected()
                .and_then(|i| old.filtered().get(i).map(|r| r.label.clone()));
            self.build_related_files_picker();
            if let Some(picker) = &mut self.picker {
                picker.filter = old.filter;
                picker.state = old.state;
                if let Some(label) = selected {
                    if let Some(i) = picker.filtered().iter().position(|r| r.label == label) {
                        picker.state.select(Some(i));
                    }
                }
                picker.select_clamp();
            }
        }
        self.poll_pending_login();
        self.poll_model_refresh();
        self.poll_job();
        landing::poll(self);
        tokens::poll(self);
        integrations::poll(self);
        artifacts::poll(self);
        self.update_progress();
        self.poll_compat();
        self.poll_proposals();
        // Polling alone never ages a completed cache. Re-read on a cheap UI timer so idle
        // panes notice resets and start the fetcher's bounded TTL refresh.
        if self.usage_fetcher.poll() || self.tick_count.is_multiple_of(50) {
            self.refresh_usage();
            self.update_usage_block();
            self.update_usage_picker();
            visible_change = true;
        }
        visible_change
    }
}
