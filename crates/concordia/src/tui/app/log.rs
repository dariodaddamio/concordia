use super::super::*;

impl App {
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
    pub(in crate::tui) fn push_user(&mut self, text: String) {
        self.related_files.invalidate();
        self.log_user(&text);
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
    pub(in crate::tui) fn push_assistant(&mut self, text: String) {
        self.related_files.invalidate();
        let ev = self.log_who(crate::threads::ThreadEvent::with_text("assistant", &text));
        self.log_event(ev);
        let meta = Some(self.reply_meta());
        self.messages.push(ChatLine {
            role: MsgRole::Assistant,
            text,
            card: None,
            meta,
        });
    }
    pub(in crate::tui) fn push_map(&mut self, pack: &crate::context::ContextPack) {
        let rest = pack.text.split_once('\n').map_or("", |(_, rest)| rest);
        self.push_system(format!(
            "{} · ~{} tok\n{}",
            pack.labeled,
            pack.est_tokens,
            rest.trim_end()
        ));
    }
    pub(in crate::tui) fn push_card(&mut self, card: StreamCard) {
        let name = card.title.to_ascii_lowercase();
        if !self.queue.force_pending
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
