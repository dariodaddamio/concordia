//! Session titles and bounded asynchronous resume scrollback. Native session ownership stays explicit.
use super::*;

pub(super) const HISTORY_OMITTED: &str =
    "Showing recent conversation history. Earlier messages remain saved in the original chat.";

#[derive(Default)]
pub(super) struct State {
    pub title: Option<String>,
    pub(super) resume_request: Option<ResumeRequest>,
    next: u64,
    pending: Option<u64>,
    jobs: Vec<Load>,
    pub(super) memory_pending: Option<u64>,
    pub(super) memory_screen: Option<u64>,
    memory_jobs: Vec<(u64, std::sync::mpsc::Receiver<Result<String, String>>)>,
}

pub(super) struct ResumeRequest {
    pub account: String,
    pub session: String,
    pub task: Option<String>,
    pub key: super::chat_ownership::Key,
}

struct Load {
    generation: u64,
    provider: ProviderKind,
    account: String,
    home: PathBuf,
    expected_session: Option<String>,
    restoring: bool,
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
        self.resume_request = None;
        self.pending = None;
        self.memory_pending = None;
        self.memory_screen = None;
    }
    #[cfg(test)]
    pub fn reading(&self) -> bool {
        !self.jobs.is_empty()
    }
}

fn file_facts_memory(value: serde_json::Value) -> String {
    let source = value["source"]["id"].as_str().unwrap_or_default();
    let mut rows = vec![format!("Historical file facts · source {source}{}\nPaths in verified native tool requests; current disk state is unverified.", if value["incomplete"] == true { " · incomplete scope" } else { "" })];
    if let Some(facts) = value["facts"].as_array() {
        for fact in facts {
            rows.push(format!(
                "{} · message {}\n/memory excerpt {source} --message {}",
                fact["path"].as_str().unwrap_or_default(),
                fact["message"],
                fact["message"]
            ));
        }
    }
    if let Some(next) = value["next_offset"].as_u64() {
        rows.push(format!(
            "Next: /memory file-facts {source} {next} {}",
            value["revision"].as_str().unwrap_or_default()
        ));
    }
    for key in ["limits", "unavailable"] {
        if let Some(values) = value[key].as_array() {
            rows.extend(
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned)),
            );
        }
    }
    rows.join("\n\n")
}

/* fn ranked_memory(
    root: &Path,
    workspace: &Path,
    argument: &str,
    cross_project: bool,
) -> Option<Result<String, String>> {
    let (mode, input) = [
        "ranked",
        "ranked-file",
        "ranked-goal",
        "ranked-from",
        "ranked-at",
        "ranked-source",
    ]
    .into_iter()
    .find_map(|mode| {
        argument
            .strip_prefix(mode)
            .filter(|rest| rest.is_empty() || rest.starts_with(' '))
            .map(|rest| (mode, rest.trim_start()))
    })?;
    Some((|| {
        let mut file = None;
        let mut goal = None;
        let mut from = None;
        let mut at = None;
        let mut source = None;
        let text = if mode == "ranked" {
            input
        } else {
            let (context, consumed) = crate::workspaces::word(input)?.ok_or(
                "use ranked-file PATH TEXT, ranked-goal GOAL TEXT, ranked-from SOURCE TEXT, ranked-source SOURCE TEXT or ranked-at RFC3339 TEXT; quote paths containing spaces",
            )?;
            if mode == "ranked-file" {
                file = Some(context);
            } else if mode == "ranked-goal" {
                goal = Some(context);
            } else if mode == "ranked-from" {
                from = Some(context);
            } else if mode == "ranked-at" {
                at = Some(context);
            } else {
                source = Some(context);
            }
            input[consumed..].trim_start()
        };
        let report = crate::native_memory::rank::recall(
            root,
            crate::native_memory::rank::Request {
                text,
                source: source.as_deref(),
                account: None,
                workspace: (!cross_project).then_some(workspace),
                goal: goal.as_deref(),
                file: file.as_deref(),
                from: from.as_deref(),
                at: at.as_deref(),
            },
        )?;
        let mut lines = vec![format!("Query: {text:?} · goal {goal:?} · file {file:?} · from {from:?} · reference time {at:?}"), format!(
            "Ranked memory: {} matches in {} source pages{}",
            report.hits.len(),
            report.pages.len(),
            if report.incomplete {
                "; incomplete scope"
            } else {
                ""
            }
        )];
        if cross_project {
            lines.push("Cross-project suggestions enabled. Check each recorded workspace; suggestions never confirm membership.".into());
        }
        for hit in report.hits {
            let reasons = hit
                .reasons
                .iter()
                .map(|reason| {
                    let label = match reason.kind {
                        "confirmed_goal_source" => "Confirmed goal",
                        "explicit_relationship" => "Context link",
                        "exact_historical_file" => "Historical file",
                        "lexical" => "Text match",
                        "temporal_distance" => "Native time",
                        _ => "Evidence",
                    };
                    format!(
                        "{} +{}: {}\nEvidence: {}",
                        label, reason.points, reason.detail, reason.evidence
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            lines.push(format!("score {} · {} / {} · message {}\n{}\n{}\nSource: {}\n/memory excerpt {} --message {}", hit.score, hit.hit.source.account, hit.hit.source.session_id, hit.hit.message, hit.hit.preview, reasons, hit.hit.source.id, hit.hit.source.id, hit.hit.message));
            if let Some(workspace) = hit.recorded_workspace {
                let full = workspace.to_string_lossy();
                let shown: String = full.chars().take(180).collect();
                lines.push(format!(
                    "Recorded workspace: {shown}{}",
                    if full.chars().count() > 180 {
                        "… (path display truncated)"
                    } else {
                        ""
                    }
                ));
                if cross_project {
                    lines.push("Context suggestion; inspect the recorded workspace before using it. Membership and native ancestry are unconfirmed.".into());
                }
            }
        }
        lines.extend(
            report
                .limits
                .into_iter()
                .map(|limit| format!("Limit: {limit}")),
        );
        lines.extend(report.unavailable);
        Ok(lines.join("\n\n"))
    })())
}

*/
impl App {
    /// Refresh a restored tab's exact native history without changing selection, draft,
    /// conversation ownership or paused unfinished-request state.
    pub(super) fn reload_restored_history(&mut self, session: &str) {
        let Some(home) = self.home_path.clone() else {
            return;
        };
        let root = self.state_root.clone();
        let account = self.account.clone();
        let provider = self.provider;
        let requested = session.to_owned();
        let worker_session = requested.clone();
        let worker_account = account.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        if let Err(error) = std::thread::Builder::new()
            .name("concordia-restored-history".into())
            .spawn(move || {
                let _ = tx.send(crate::chat_cli::read_history(
                    &root,
                    &worker_account,
                    provider,
                    &worker_session,
                ));
            })
        {
            self.push_system(format!(
                "Saved conversation kept; history refresh unavailable: {error}"
            ));
            return;
        }
        self.session_ui.next = self.session_ui.next.wrapping_add(1);
        let generation = self.session_ui.next;
        self.session_ui.pending = Some(generation);
        self.session_ui.jobs.push(Load {
            generation,
            provider,
            account,
            home,
            expected_session: self.native_session_id.clone(),
            restoring: true,
            rx,
        });
    }

    pub(super) fn memory_command(&mut self, argument: &str) {
        if argument.is_empty() {
            self.push_system("Files: /memory file-facts INDEX [OFFSET REVISION] reads historical paths from verified native tool records; it does not open referenced project files.");
            self.push_system("Memory: /memory recall TEXT · /memory recall-account ACCOUNT TEXT · /memory recall-source INDEX TEXT · /memory sources INDEX · /memory next INDEX · /memory excerpt INDEX --message N · /memory index SESSION --account ID --scope whole starts a whole-history scan.");
            return;
        }
        if self.session_ui.memory_jobs.len() >= 2 {
            self.flash("memory reads are finishing; try again shortly");
            return;
        }
        let root = self.state_root.clone();
        let workspace = self.cwd.clone();
        let screen = argument.starts_with("file-facts") || argument.starts_with("sources");
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
                        lines.push(format!("{} / {} · {} · message {}\n{}\nSource: {}", hit.source.account, hit.source.session_id, hit.role, hit.message, hit.preview, hit.source.id));
                    }
                    for page in r.pages { lines.push(format!("Scanned source: {} / {} · {}",page.account,page.session_id,page.id)); }
                    lines.extend(r.unavailable);
                    lines.join("\n\n")
                })
            } else {
                let args: Vec<String> = argument.split_whitespace().map(str::to_owned).collect();
                if args.first().is_some_and(|operation| operation == "file-facts") {
                    crate::native_memory::files::command(&root, &args).map(file_facts_memory)
                } else if args.first().is_some_and(|s| s == "next") && args.len() == 2 {
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
        self.session_ui.memory_screen = screen.then_some(generation);
        self.session_ui.memory_jobs.push((generation, rx));
        if screen {
            self.update_preview = Some(crate::update::UpdatePreview {
                history: Vec::new(),
                history_expanded: false,
                lines: vec![crate::update::PreviewLine::Text(
                    "Loading related work…".into(),
                )],
                install_available: false,
                release_url: None,
            });
            self.show_picker(
                PickerKind::Related,
                "related work".into(),
                Vec::new(),
                false,
            );
        }
        self.flash("reading native memory…");
    }

    pub(super) fn poll_memory(&mut self) -> bool {
        let mut changed = false;
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
            changed = true;
            let output = crate::chat_cli::terminal_safe(
                &result.unwrap_or_else(|e| format!("memory unavailable: {e}")),
            );
            if self.session_ui.memory_screen == Some(generation) {
                if self
                    .picker
                    .as_ref()
                    .is_some_and(|picker| picker.kind == PickerKind::Related)
                {
                    self.update_preview = Some(crate::update::UpdatePreview {
                        history: Vec::new(),
                        history_expanded: false,
                        lines: output
                            .lines()
                            .map(|line| crate::update::PreviewLine::Text(line.into()))
                            .collect(),
                        install_available: false,
                        release_url: None,
                    });
                }
            } else if self.session_ui.memory_screen.is_none()
                && !self
                    .picker
                    .as_ref()
                    .is_some_and(|picker| picker.kind == PickerKind::Related)
            {
                self.push_system(output);
            }
        }
        changed
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

    pub(super) fn rename_session(&mut self, raw: &str) -> bool {
        let title = crate::session_titles::sanitize(raw);
        if let (Some(id), Some(home)) = (&self.native_session_id, &self.home_path) {
            if let Err(error) = crate::session_titles::set(
                &self.state_root,
                self.provider,
                &self.account,
                home,
                id,
                title.as_deref(),
            ) {
                self.flash(format!("Chat title was not changed: {error}"));
                return false;
            }
        }
        self.session_ui.title = title;
        self.session_chip = self.session_ui.title.clone();
        true
    }

    /// Request navigation without replacing this tab before the runner checks existing owners.
    pub(super) fn request_resume_session(&mut self, account: &str, id: &str) -> bool {
        let key = match super::chat_ownership::Key::for_resume(&self.state_root, account, id) {
            Ok(key) => key,
            Err(error) => {
                self.push_system(format!("resume unavailable: {error}"));
                return false;
            }
        };
        self.session_ui.resume_request = Some(ResumeRequest {
            account: account.into(),
            session: id.into(),
            task: None,
            key,
        });
        true
    }

    /// Low-level application after window-level routing has excluded another owning tab.
    pub(super) fn resume_session(&mut self, account: &str, id: &str) -> bool {
        if self.busy {
            self.flash("finish or cancel the active turn before resuming");
            return false;
        }
        if self.session_ui.jobs.len() >= 2 {
            self.flash("previous history reads are finishing; try resume again shortly");
            return false;
        }
        let Some(target) = get_account(&self.state_root, account) else {
            self.push_system("resume failed: account is no longer registered");
            return false;
        };
        let Some(home) = target.home_path.clone() else {
            self.push_system("resume failed: account has no native home");
            return false;
        };
        if let Err(error) = select_account(&self.state_root, account) {
            self.push_system(format!("resume account switch failed: {error}"));
            return false;
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
            return false;
        }
        super::landing::close(self);
        self.continuation.invalidate();
        self.keep.turn_recovery_notice = false;
        self.session_ui.next = self.session_ui.next.wrapping_add(1);
        let generation = self.session_ui.next;
        self.session_ui.pending = Some(generation);
        self.session_ui.title =
            crate::session_titles::get(&self.state_root, provider, account, &home, id);
        self.session_chip = self.session_ui.title.clone();
        self.native_session_id = Some(id.to_string());
        self.task_id = None;
        self.native_sources.clear();
        self.remember_native_source();
        let checkpoint_cleared = self.save_turn_checkpoint(true);
        self.need_context_pack = false;
        self.prior_provider = provider;
        self.prior_home = home
            .canonicalize()
            .ok()
            .map(|home| home.to_string_lossy().into_owned());
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
        if crate::usage::peek_cached_usage(&self.state_root, provider, account, &home)
            .is_some_and(|usage| usage.exhausted)
        {
            self.push_system("This account's recorded usage is exhausted. You can still read this chat. Use /use to choose another account, /model to choose a model, or /usage refresh to check the limit before sending.");
        }
        self.session_ui.jobs.push(Load {
            generation,
            provider,
            account: account.into(),
            home,
            expected_session: Some(id.into()),
            restoring: false,
            rx,
        });
        true
    }

    pub(super) fn poll_session_history(&mut self) -> bool {
        let mut changed = false;
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
            if self.session_ui.pending != Some(job.generation) {
                continue;
            }
            if self.provider != job.provider
                || self.account != job.account
                || self.home_path.as_ref() != Some(&job.home)
                || self.native_session_id != job.expected_session
            {
                self.session_ui.pending = None;
                changed = true;
                let notice = "History loading stopped because the account or native session changed. Select the intended chat again.";
                if let Some(message) = self.messages.iter_mut().find(|message| {
                    message.role == MsgRole::System
                        && message.text == "Loading recent messages for the selected session…"
                }) {
                    message.text = notice.into();
                } else {
                    self.push_system(notice);
                }
                continue;
            }
            self.session_ui.pending = None;
            changed = true;
            let unfinished_prompt = (job.restoring && self.keep.turn_recovery_notice)
                .then(|| self.continuation.pending_text().map(str::to_owned))
                .flatten();
            if job.restoring {
                if let Err(error) = &result {
                    self.push_system(format!(
                        "Saved conversation kept; native history unavailable: {error}"
                    ));
                    continue;
                }
                if result
                    .as_ref()
                    .is_ok_and(|history| history.messages.is_empty())
                    && self
                        .messages
                        .iter()
                        .any(|message| matches!(message.role, MsgRole::User | MsgRole::Assistant))
                {
                    self.push_system("Saved conversation kept; the native source contains no readable messages yet.");
                    continue;
                }
            }
            let pending_notices = if job.restoring {
                self.messages
                    .iter()
                    .filter(|message| {
                        message.role == MsgRole::System && message.text != HISTORY_OMITTED
                    })
                    .cloned()
                    .collect()
            } else {
                self.messages.split_off(1.min(self.messages.len()))
            };
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
                    if history.truncated {
                        self.push_system(HISTORY_OMITTED);
                    } else if !job.restoring {
                        self.push_system(format!("Resumed · {count} messages loaded. Scroll up to review the conversation."));
                    }
                    let meta = Some(self.reply_meta());
                    for message in history.messages {
                        self.messages.push(ChatLine {
                            role: match message.role.as_str() {
                                "user" => MsgRole::User,
                                "thinking" => MsgRole::Thinking,
                                "card" => MsgRole::Card,
                                _ => MsgRole::Assistant,
                            },
                            text: message.text,
                            card: message.card,
                            meta: if message.role != "user" {
                                meta.clone()
                            } else {
                                None
                            },
                        });
                        if let Some(card) = self
                            .messages
                            .last()
                            .and_then(|message| message.card.as_ref())
                        {
                            self.native_agents.record(card, self.messages.len() - 1);
                        }
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
            if let Some(prompt) = unfinished_prompt {
                let already_last = self
                    .messages
                    .iter()
                    .rev()
                    .find(|message| message.role == MsgRole::User)
                    .is_some_and(|message| message.text == prompt);
                if !already_last {
                    self.messages.push(ChatLine {
                        role: MsgRole::User,
                        text: prompt,
                        card: None,
                        meta: None,
                    });
                }
            }
            let notice_offset = self.messages.len();
            self.messages.extend(pending_notices);
            self.usage_block = pending_usage.map(|index| notice_offset + index);
            self.view.follow_tail();
        }
        changed
    }
}
