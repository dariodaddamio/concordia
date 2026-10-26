//! Session titles and bounded asynchronous resume scrollback. Native session ownership stays explicit.
use super::*;

#[derive(Default)]
pub(super) struct State {
    pub title: Option<String>,
    next: u64,
    pending: Option<u64>,
    jobs: Vec<Load>,
}

struct Load {
    generation: u64,
    provider: ProviderKind,
    account: String,
    home: PathBuf,
    session: String,
    rx: std::sync::mpsc::Receiver<Result<crate::chat_cli::History, String>>,
}

impl State {
    pub fn loading(&self) -> bool {
        self.pending.is_some()
    }
    pub fn invalidate(&mut self) {
        self.pending = None;
    }
    #[cfg(test)]
    pub fn reading(&self) -> bool {
        !self.jobs.is_empty()
    }
}

impl App {
    pub(super) fn ensure_session_title(&mut self, prompt: &str) {
        if self.session_ui.title.is_none() {
            let title = crate::session_titles::title_from_prompt(prompt);
            if !title.is_empty() {
                self.session_chip = Some(title.clone());
                self.session_ui.title = Some(title);
            }
        }
        self.persist_session_title();
    }

    pub(super) fn persist_session_title(&mut self) {
        if let (Some(title), Some(id), Some(home)) = (
            &self.session_ui.title,
            &self.native_session_id,
            &self.home_path,
        ) {
            if let Err(error) = crate::session_titles::set_if_absent(
                &self.state_root,
                self.provider,
                &self.account,
                home,
                id,
                title,
            ) {
                self.flash(format!("session title could not be saved: {error}"));
            }
        }
    }

    pub(super) fn resume_session(&mut self, account: &str, id: &str) {
        self.continuation.invalidate();
        if self.busy {
            self.flash("finish or cancel the active turn before resuming");
            return;
        }
        if self.session_ui.jobs.len() >= 2 {
            self.flash("previous history reads are finishing; try resume again shortly");
            return;
        }
        let Some(target) = get_account(&self.state_root, account) else {
            self.push_system("resume failed: account is no longer registered");
            return;
        };
        let Some(home) = target.home_path.clone() else {
            self.push_system("resume failed: account has no native home");
            return;
        };
        if let Err(error) = select_account(&self.state_root, account) {
            self.push_system(format!("resume account switch failed: {error}"));
            return;
        }
        self.refresh_binding();
        let root = self.state_root.clone();
        let worker_account = account.to_string();
        let worker_id = id.to_string();
        let provider = target.provider;
        let (tx, rx) = std::sync::mpsc::channel();
        if let Err(error) = std::thread::Builder::new()
            .name("concordia-resume-history".into())
            .spawn(move || {
                let _ = tx.send(crate::chat_cli::read_history(
                    &root,
                    &worker_account,
                    provider,
                    &worker_id,
                ));
            })
        {
            self.native_session_id = None;
            self.push_system(format!("could not start history reader: {error}"));
            return;
        }
        self.session_ui.next = self.session_ui.next.wrapping_add(1);
        let generation = self.session_ui.next;
        self.session_ui.pending = Some(generation);
        self.session_ui.title =
            crate::session_titles::get(&self.state_root, provider, account, &home, id);
        self.session_chip = self.session_ui.title.clone();
        self.native_session_id = Some(id.to_string());
        self.need_context_pack = false;
        self.prior_provider = provider;
        self.prior_home = Some(home.to_string_lossy().into_owned());
        self.messages.clear();
        self.native_agents = native_agents::State::default();
        self.activity = activity::Activity::default();
        self.expanded_activity = false;
        self.view = TranscriptView::default();
        self.clear_selections();
        self.usage_block = None;
        self.live_cards.clear();
        self.live_text.clear();
        self.live_thinking.clear();
        self.streamed_text_committed = false;
        self.session_input = 0;
        self.session_output = 0;
        self.session_cost = 0.0;
        self.pause_queue();
        self.thread = crate::threads::start(&self.state_root);
        self.thread_meta_done = false;
        self.push_system("Loading recent messages for the selected session…");
        self.session_ui.jobs.push(Load {
            generation,
            provider,
            account: account.into(),
            home,
            session: id.into(),
            rx,
        });
    }

    pub(super) fn poll_session_history(&mut self) {
        let mut i = 0;
        while i < self.session_ui.jobs.len() {
            let result = match self.session_ui.jobs[i].rx.try_recv() {
                Ok(result) => result,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    i += 1;
                    continue;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Err("history reader stopped unexpectedly".into())
                }
            };
            let job = self.session_ui.jobs.swap_remove(i);
            if self.session_ui.pending != Some(job.generation)
                || self.provider != job.provider
                || self.account != job.account
                || self.home_path.as_ref() != Some(&job.home)
                || self.native_session_id.as_deref() != Some(job.session.as_str())
            {
                continue;
            }
            self.session_ui.pending = None;
            let pending_notices = self.messages.split_off(1.min(self.messages.len()));
            let pending_usage = self
                .usage_block
                .take()
                .and_then(|index| index.checked_sub(1));
            self.messages.clear();
            self.view = TranscriptView::default();
            match result {
                Ok(history) => {
                    if self.session_ui.title.is_none() {
                        let source = history.title.as_deref().or_else(|| {
                            history
                                .messages
                                .iter()
                                .find(|message| message.role == "user")
                                .map(|message| message.text.as_str())
                        });
                        if let Some(source) = source {
                            self.ensure_session_title(source);
                        }
                        if self.session_ui.title.is_none() {
                            if let Some(source) = history
                                .messages
                                .iter()
                                .find(|message| message.role == "user")
                            {
                                self.ensure_session_title(&source.text);
                            }
                        }
                    }
                    let count = history.messages.len();
                    self.push_system(if history.truncated {
                        format!("Resumed · {count} recent messages loaded; older history is omitted from this bounded view.")
                    } else { format!("Resumed · {count} messages loaded. Scroll up to review the conversation.") });
                    let meta = Some(self.reply_meta());
                    for message in history.messages {
                        self.messages.push(ChatLine {
                            role: if message.role == "user" {
                                MsgRole::User
                            } else {
                                MsgRole::Assistant
                            },
                            text: message.text,
                            card: None,
                            meta: if message.role != "user" {
                                meta.clone()
                            } else {
                                None
                            },
                        });
                    }
                    if count == 0 {
                        self.push_system(
                            "No readable messages were found in the selected native session.",
                        );
                    }
                }
                Err(error) => {
                    self.push_system(format!(
                        "Session selected; previous messages unavailable: {error}"
                    ));
                }
            }
            let notice_offset = self.messages.len();
            self.messages.extend(pending_notices);
            self.usage_block = pending_usage.map(|index| notice_offset + index);
            self.view.follow_tail();
        }
    }
}
