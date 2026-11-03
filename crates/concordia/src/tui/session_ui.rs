//! Session titles and bounded asynchronous resume scrollback. Native session ownership stays explicit.
use super::*;

#[derive(Default)]
pub(super) struct State {
    pub title: Option<String>,
    next: u64,
    pending: Option<u64>,
    jobs: Vec<Load>,
    memory_pending: Option<u64>,
    memory_jobs: Vec<(u64, std::sync::mpsc::Receiver<Result<String, String>>)>,
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
    #[cfg(test)]
    pub fn memory_reading(&self) -> bool {
        !self.memory_jobs.is_empty()
    }
    pub fn loading(&self) -> bool {
        self.pending.is_some()
    }
    pub fn invalidate(&mut self) {
        self.pending = None;
        self.memory_pending = None;
    }
    #[cfg(test)]
    pub fn reading(&self) -> bool {
        !self.jobs.is_empty()
    }
}

impl App {
    pub(super) fn memory_command(&mut self, argument: &str) {
        if argument.is_empty() {
            self.push_system("Memory: /memory recall TEXT · /memory recall-account ACCOUNT TEXT · /memory recall-source INDEX TEXT · /memory sources INDEX · /memory next INDEX · /memory excerpt INDEX --message N · /memory lineage INDEX · /memory link PARENT CHILD · /memory detach PARENT CHILD. Search suggestions never become links automatically. /memory index SESSION --account ID --scope whole starts a whole-history scan.");
            return;
        }
        if self.session_ui.memory_jobs.len() >= 2 {
            self.flash("memory reads are finishing; try again shortly");
            return;
        }
        let root = self.state_root.clone();
        let workspace = self.cwd.clone();
        let argument = argument.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        let result = std::thread::Builder::new().name("concordia-memory-read".into()).spawn(move || {
            let search = argument.strip_prefix("recall ").map(|q| (None, None, q))
                .or_else(|| argument.strip_prefix("recall-account ").and_then(|rest| rest.split_once(char::is_whitespace)).map(|(a,q)| (Some(a),None,q)))
                .or_else(|| argument.strip_prefix("recall-source ").and_then(|rest| rest.split_once(char::is_whitespace)).map(|(s,q)| (None,Some(s),q)));
            let output = if let Some((account, source, query)) = search {
                    crate::native_memory::retrieval::recall(&root, query, source, account, if source.is_none() { Some(&workspace) } else { None }).map(|r| {
                    let mut lines = vec![format!("Memory search: {} matches in {} bounded source windows{}", r.hits.len(), r.pages.len(), if r.incomplete { "; incomplete scan" } else { "" })];
                    for hit in r.hits {
                        lines.push(format!("{} / {} · {} · message {}\n{}\nSource: {}\nLexical suggestion; use /memory link to confirm a relationship.", hit.source.account, hit.source.session_id, hit.role, hit.message, hit.preview, hit.source.id));
                    }
                    for page in r.pages { lines.push(format!("Scanned source: {} / {} · {}",page.account,page.session_id,page.id)); }
                    lines.extend(r.unavailable);
                    lines.join("\n\n")
                })
            } else {
                let args: Vec<String> = argument.split_whitespace().map(str::to_owned).collect();
                if args.first().is_some_and(|s| s == "next") && args.len() == 2 {
                    crate::native_memory::history_from_pointer(&root, &args[1]).and_then(|s| serde_json::to_string_pretty(&s).map_err(|e| e.to_string()))
                } else {
                    crate::chat_cli::run(&root, &args).map_err(|(e, _)| e)
                }
            };
            let _ = tx.send(output);
        });
        if let Err(e) = result {
            self.push_system(format!("memory reader could not start: {e}"));
            return;
        }
        self.session_ui.next = self.session_ui.next.wrapping_add(1);
        let generation = self.session_ui.next;
        self.session_ui.memory_pending = Some(generation);
        self.session_ui.memory_jobs.push((generation, rx));
        self.flash("reading native memory…");
    }

    pub(super) fn poll_memory(&mut self) {
        let mut i = 0;
        while i < self.session_ui.memory_jobs.len() {
            let result = match self.session_ui.memory_jobs[i].1.try_recv() {
                Ok(r) => r,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    i += 1;
                    continue;
                }
                Err(_) => Err("memory reader stopped".into()),
            };
            let (generation, _) = self.session_ui.memory_jobs.swap_remove(i);
            if self.session_ui.memory_pending != Some(generation) {
                continue;
            }
            self.session_ui.memory_pending = None;
            self.push_system(crate::chat_cli::terminal_safe(
                &result.unwrap_or_else(|e| format!("memory unavailable: {e}")),
            ));
        }
    }

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
        self.continuation.invalidate();
        self.keep.turn_recovery_notice = false;
        self.session_ui.next = self.session_ui.next.wrapping_add(1);
        let generation = self.session_ui.next;
        self.session_ui.pending = Some(generation);
        self.session_ui.title =
            crate::session_titles::get(&self.state_root, provider, account, &home, id);
        self.session_chip = self.session_ui.title.clone();
        self.native_session_id = Some(id.to_string());
        self.native_sources.clear();
        self.remember_native_source();
        let checkpoint_cleared = self.save_turn_checkpoint(true);
        self.need_context_pack = false;
        self.prior_provider = provider;
        self.prior_home = Some(home.to_string_lossy().into_owned());
        self.project_search.close();
        self.native_owner = None;
        self.deleg.change_conversation();
        self.messages.clear();
        self.related_files.reset();
        self.questions.clear();
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
        if !checkpoint_cleared {
            self.push_system("could not clear the previous unfinished-request record; it has been kept for recovery");
        }
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
            self.related_files.reset();
            self.questions.clear();
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
