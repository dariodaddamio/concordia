use super::super::*;

impl App {
    /// Trim the resting projection only after source capture and indexed UI work
    /// settle. Native sources, composer, queues and conversation identity remain owned.
    pub(in crate::tui) fn prune_transcript_projection(&mut self) -> bool {
        const ROWS: usize = 2048;
        const BYTES: usize = 16 * 1024 * 1024;
        if self.busy
            || self.preparing_cancel.is_some()
            || self.cancel_requested
            || self.deleg.d.any_active()
            || self.deleg.has_pending_prompt()
            || self.questions.waiting()
            || self.picker.is_some()
            || self.activity.detail.is_some()
            || self.card_header_pressed.is_some()
            || self.view.scroll != 0
            || self.view.sel.is_some()
            || self.view.dragging
            || self.session_ui.loading()
            || self.native_session_id.is_none()
            || self.native_sources.is_empty()
            || self.continuation.waiting()
            || self.reference_journal.pending()
            || self.reference_journal.backlogged()
            || artifacts::projection_pending(self)
        {
            return false;
        }
        let size = |message: &ChatLine| {
            message
                .text
                .len()
                .saturating_add(message.card.as_ref().map_or(0, |card| {
                    card.id.len() + card.title.len() + card.detail.as_ref().map_or(0, String::len)
                }))
        };
        let mut bytes = 0usize;
        let mut keep = 0usize;
        for message in self.messages.iter().rev().take(ROWS) {
            let next = bytes.saturating_add(size(message));
            if keep > 0 && next > BYTES {
                break;
            }
            bytes = next;
            keep += 1;
        }
        let removed = self.messages.len().saturating_sub(keep);
        const OMITTED: &str = "Older messages are omitted from this bounded view. Original native history is kept; reopen the chat or use exact-source memory retrieval to inspect it.";
        if removed == 0
            || (removed == 1
                && self
                    .messages
                    .first()
                    .is_some_and(|message| message.text == OMITTED))
            || !self
                .keep
                .lease
                .as_ref()
                .is_some_and(|lease| matches!(lease.turn(), Ok(None)))
        {
            return false;
        }
        let (Some(home), Some(session)) = (&self.home_path, &self.native_session_id) else {
            return false;
        };
        let Ok(binding) =
            crate::native_memory::Binding::new(self.provider, &self.account, home, session)
        else {
            return false;
        };
        if !self.native_sources.contains(&binding)
            || !self
                .reference_journal
                .projection_verified(&self.state_root, &binding)
        {
            return false;
        }
        self.messages.drain(..removed);
        self.messages.insert(
            0,
            ChatLine {
                role: MsgRole::System,
                text: OMITTED.into(),
                card: None,
                meta: None,
            },
        );
        self.usage_block = self
            .usage_block
            .and_then(|index| index.checked_sub(removed))
            .map(|index| index + 1);
        self.activity = activity::Activity::default();
        self.artifacts = artifacts::State::default();
        self.native_agents = native_agents::State::default();
        for (index, message) in self.messages.iter().enumerate() {
            if let Some(card) = &message.card {
                self.native_agents.record(card, index);
            }
        }
        self.view = TranscriptView::default();
        self.related_files.invalidate();
        if self.messages.capacity() > ROWS * 2 {
            self.messages.shrink_to_fit();
        }
        true
    }

    pub(in crate::tui) fn thread_id(&self) -> Option<&str> {
        self.thread.as_ref().map(|t| t.id())
    }
    pub(in crate::tui) fn log_event(&mut self, mut ev: crate::threads::ThreadEvent) {
        if let Some(t) = &self.thread {
            if matches!(ev.k.as_str(), "user" | "assistant" | "card") {
                self.reference_journal.buffer(t.id(), ev, None);
            } else {
                ev.text = None;
                if let Some(card) = &mut ev.card {
                    card.detail = None;
                }
                let _ = t.append(&ev);
            }
        }
    }
    pub(in crate::tui) fn log_who(
        &self,
        ev: crate::threads::ThreadEvent,
    ) -> crate::threads::ThreadEvent {
        ev.who(self.provider.as_str(), &self.account, &self.model)
    }
    pub(in crate::tui) fn log_user(&mut self, text: &str) {
        if let Some(t) = &self.thread {
            if !self.thread_meta_done {
                self.thread_meta_done = true;
                let _ = t.write_meta(&crate::threads::ThreadMeta {
                    id: t.id().to_string(),
                    started: crate::threads::now_secs(),
                    title: self
                        .session_ui
                        .title
                        .clone()
                        .unwrap_or_else(|| crate::threads::title_from_prompt(text)),
                    cwd: self.cwd.to_string_lossy().into_owned(),
                    account: Some(self.account.clone()),
                });
            }
        }
        self.log_event(crate::threads::ThreadEvent::with_text("user", text));
    }
    pub(in crate::tui) fn log_switch(&mut self, from: ProviderKind) {
        let ev = crate::threads::ThreadEvent::with_text(
            "switch",
            &format!("{} -> {}", from.as_str(), self.account),
        );
        let ev = self.log_who(ev);
        self.log_event(ev);
    }
    pub(in crate::tui) fn push_user(&mut self, mut text: String) {
        self.related_files.invalidate();
        self.log_user(&text);
        if text.len() > crate::provider::turn::VISIBLE_TEXT_MAX {
            let full = std::mem::take(&mut text);
            crate::provider::turn::append_visible_text(&mut text, &full);
        }
        self.messages.push(ChatLine {
            role: MsgRole::User,
            text,
            card: None,
            meta: None,
        });
    }
    pub(in crate::tui) fn reply_meta(&self) -> (ProviderKind, String) {
        let mut label = self.account_label();
        if !self.model.is_empty() && self.model != "?" {
            label.push_str(&format!(" \u{b7} {}", self.model));
        }
        (self.provider, label)
    }
    pub(in crate::tui) fn push_command(&mut self, line: &str) {
        self.messages.push(ChatLine {
            role: MsgRole::Command,
            text: line.trim().to_string(),
            card: None,
            meta: None,
        });
    }
    pub(in crate::tui) fn push_assistant(&mut self, mut text: String) {
        self.related_files.invalidate();
        let ev = self.log_who(crate::threads::ThreadEvent::with_text("assistant", &text));
        self.log_event(ev);
        if text.len() > crate::provider::turn::VISIBLE_TEXT_MAX {
            let full = std::mem::take(&mut text);
            crate::provider::turn::append_visible_text(&mut text, &full);
        }
        let meta = Some(self.reply_meta());
        self.messages.push(ChatLine {
            role: MsgRole::Assistant,
            text,
            card: None,
            meta,
        });
    }
    pub(in crate::tui) fn push_card(&mut self, card: StreamCard) {
        let name = card.title.to_ascii_lowercase();
        if !self.queue.dispatch.is_explicit()
            && (name.contains("request_user_input") || name.contains("askuserquestion"))
        {
            if let Some(detail) = &card.detail {
                self.questions.offer_tool(detail);
            }
        }
        self.related_files.invalidate();
        let turn_start = self
            .messages
            .iter()
            .rposition(|m| m.role == MsgRole::User)
            .unwrap_or(0);
        let existing_index = (turn_start..self.messages.len()).rev().find(|&i| {
            let m = &self.messages[i];
            m.role == MsgRole::Card
                && m.card.as_ref().map(|c| c.id.as_str()) == Some(card.id.as_str())
        });
        if existing_index.is_some_and(|i| self.messages[i].card.as_ref() == Some(&card)) {
            return;
        }
        if card.status != CardStatus::Running {
            let status = if card.status == CardStatus::Error {
                "error"
            } else {
                "done"
            };
            let ev = self.log_who(crate::threads::ThreadEvent::card(
                &card.title,
                card.detail.as_deref(),
                status,
            ));
            if let Some(thread) = &self.thread {
                self.reference_journal
                    .buffer(thread.id(), ev, Some(&card.id));
            }
        }
        let text = format_card_line(&card);
        if let Some(i) = existing_index {
            let existing = &mut self.messages[i];
            existing.card = Some(card);
            existing.text = text;
            self.native_agents
                .record(existing.card.as_ref().unwrap(), i);
            if self.activity.detail == Some(i) {
                self.activity.invalidate_search();
            }
            self.refresh_agents_picker();
            return;
        }
        let meta = Some(self.reply_meta());
        self.messages.push(ChatLine {
            role: MsgRole::Card,
            text,
            card: Some(card),
            meta,
        });
        if let Some(card) = self.messages.last().and_then(|m| m.card.as_ref()) {
            self.native_agents.record(card, self.messages.len() - 1);
        }
        self.refresh_agents_picker();
    }
}
