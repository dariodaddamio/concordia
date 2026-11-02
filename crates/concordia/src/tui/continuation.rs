//! Bounded, opt-in handoff after a provider explicitly reports exhausted usage.
use super::*;
use std::collections::HashSet;

pub(super) struct State {
    automatic: bool,
    accounts: Vec<String>,
    models: Vec<String>,
    tried: HashSet<String>,
    pub errors: Vec<String>,
    prompt: Option<String>,
    pending: Option<String>,
    retry_draft: Option<String>,
    dispatching: bool,
    deferred: bool,
    blocked: bool,
    recovered: bool,
    run_id: String,
    context_limited: bool,
    context_rollovers: u8,
}

impl State {
    pub fn new(root: &Path) -> Self {
        let config = crate::config::for_root(root).continuation;
        Self {
            automatic: crate::store::get_settings(root)
                .continuation_automatic
                .unwrap_or(config.automatic),
            accounts: config.accounts,
            models: config.models,
            tried: HashSet::new(),
            errors: Vec::new(),
            prompt: None,
            pending: None,
            retry_draft: None,
            dispatching: false,
            deferred: false,
            blocked: false,
            recovered: false,
            run_id: uuid::Uuid::new_v4().to_string(),
            context_limited: false,
            context_rollovers: 0,
        }
    }

    pub fn begin(&mut self, account: &str, model: &str, prompt: &str) {
        if !self.dispatching {
            self.tried.clear();
            self.run_id = uuid::Uuid::new_v4().to_string();
            self.recovered = false;
            self.context_rollovers = 0;
            self.prompt = Some(prompt.to_owned());
        }
        self.tried.insert(format!("{account} {model}"));
        self.deferred = false;
        self.errors.clear();
        self.pending = None;
        self.retry_draft = None;
        self.blocked = false;
        self.context_limited = false;
    }

    pub(super) fn checkpoint(
        &self,
        title: Option<String>,
        sources: Vec<crate::native_memory::Binding>,
        workspace: PathBuf,
    ) -> Option<turn_checkpoint::Checkpoint> {
        self.prompt
            .as_ref()
            .map(|prompt| turn_checkpoint::Checkpoint {
                version: 1,
                run_id: self.run_id.clone(),
                workspace,
                context_rollovers: self.context_rollovers,
                prompt: prompt.clone(),
                title,
                sources,
                tried: self.tried.iter().take(4).cloned().collect(),
                reason: if self.pending.is_some() {
                    "usage-limited"
                } else {
                    "interrupted"
                }
                .into(),
            })
    }

    pub(super) fn restore_checkpoint(&mut self, checkpoint: turn_checkpoint::Checkpoint) {
        self.prompt = Some(checkpoint.prompt.clone());
        self.pending = Some(checkpoint.prompt);
        self.retry_draft = None;
        self.tried = checkpoint.tried.into_iter().collect();
        self.run_id = checkpoint.run_id;
        self.context_limited = checkpoint.reason == "context-limited";
        self.context_rollovers = checkpoint.context_rollovers;
        self.recovered = true;
        self.blocked = true; // A restart cannot prove completion of the previous tools.
    }

    pub(super) fn pending_text(&self) -> Option<&str> {
        self.pending
            .as_deref()
            .map(crate::attachments::Message::text)
    }

    pub(super) fn same_account_allowed(&self) -> bool {
        self.recovered || self.context_limited
    }

    pub fn invalidate(&mut self) {
        self.deferred = false;
        self.pending = None;
        self.prompt = None;
        self.recovered = false;
        self.errors.clear();
    }

    pub fn waiting(&self) -> bool {
        self.pending.is_some()
    }
    pub(super) fn dispatch_request(&self) -> Option<&str> {
        self.dispatching.then_some(self.prompt.as_deref()).flatten()
    }

    pub(super) fn automatic(&self) -> bool {
        self.automatic
    }

    pub(super) fn toggle_automatic(&mut self) -> bool {
        self.automatic = !self.automatic;
        self.automatic
    }
}

fn usage_exhausted(text: &str) -> bool {
    crate::provider::stream_cards::usage_exhausted(text)
}

fn context_exhausted(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    let lower = lower.strip_prefix("error: ").unwrap_or(&lower);
    ["context_length_exceeded", "context_window_exceeded"]
        .iter()
        .any(|code| lower == *code || lower.starts_with(&format!("{code}:")))
        || lower.starts_with("maximum context length exceeded")
}

impl App {
    pub(super) fn continuation_command(&mut self, arg: &str) {
        match arg {
            "automatic" => self.continuation.automatic = true,
            "manual" => self.continuation.automatic = false,
            "" => {}
            _ => {
                self.push_system("usage: /continuation [manual|automatic]; /continue ACCOUNT continues an interrupted request");
                return;
            }
        }
        let mode = if self.continuation.automatic {
            "automatic"
        } else {
            "manual"
        };
        let accounts = self.continuation.accounts.join(", ");
        self.push_system(format!("usage continuation: {mode}; accounts: {}; model preferences: {}. /use saves the handoff toggle; config.toml can order accounts and models.", if accounts.is_empty() { "connected accounts, then cached models" } else { &accounts }, self.continuation.models.join(", ")));
    }

    /// Called before finalization while the completed turn's cards and draft still exist.
    pub(super) fn prepare_usage_continuation(&mut self, outcome: &TurnOutcome) -> Option<String> {
        if outcome.cancelled || self.cancel_requested {
            self.continuation.pending = None;
            return None;
        }
        if !outcome.failed && outcome.exit_code == Some(0) {
            self.continuation.pending = None;
            return None;
        }
        if crate::provider::turn::thread_recording_failed(self.provider, &outcome.stderr_tail) {
            self.continuation.pending = self.continuation.prompt.clone();
            self.continuation.retry_draft = self.last_sent.clone();
            self.continuation.blocked = true;
            self.continuation.recovered = true; // Explicit continuation must start a fresh session.
            self.push_system(format!("Codex could not record its native thread. Saved history may still exist; this is not a quota signal. Your request is kept and queued work is paused. Inspect partial edits, then /continue {} starts a fresh native session with source pointers. No automatic replay was attempted.", self.account));
            return None;
        }
        let exhausted = self.continuation.errors.iter().any(|e| usage_exhausted(e))
            || (outcome.exit_code.is_some_and(|code| code != 0)
                && usage_exhausted(&outcome.stderr_tail));
        let context_full = self
            .continuation
            .errors
            .iter()
            .any(|error| context_exhausted(error))
            || (outcome.exit_code.is_some_and(|code| code != 0)
                && context_exhausted(&outcome.stderr_tail));
        self.continuation.context_limited = context_full;
        if !exhausted && !context_full {
            self.continuation.pending = None;
            return None;
        }
        self.continuation.pending = self.continuation.prompt.clone();
        self.continuation.retry_draft = self.last_sent.clone();
        self.continuation.blocked = outcome.exit_code.is_none()
            || self
                .live_cards
                .iter()
                .chain(&outcome.cards)
                .any(|card| card.status == CardStatus::Running);
        self.push_system(
            if context_full { "provider reported context capacity exhausted; a new native session can continue using exact-source pointers. Queued prompts stay paused." }
            else { "usage limit reached; choose another account or model to continue. Queued prompts stay paused." },
        );
        if !self.continuation.automatic {
            self.open_continuation_picker();
        }
        if self.continuation.blocked {
            self.push_system("automatic continuation stopped: provider or tool completion is uncertain. Inspect its output before continuing.");
            return None;
        }
        if !self.continuation.automatic {
            return None;
        }
        if self.continuation.tried.len() >= 4 {
            self.continuation.deferred = false;
            self.push_system("automatic continuation stopped after four account/model pairs; choose /continue to review another target");
            self.open_continuation_picker();
            return None;
        }
        let paused = if self.queue.edit.is_some() {
            Some("a queued message is being edited; handoff resumes after the edit closes")
        } else if self.picker.is_some() {
            Some("a command screen is open; handoff resumes when it closes")
        } else if self.job.is_some() {
            Some("a background command is running; handoff resumes when it finishes")
        } else {
            None
        };
        if let Some(reason) = paused {
            self.continuation.deferred = true;
            self.push_system(format!("automatic continuation waiting: {reason}. The unfinished request and queue are kept."));
            return None;
        }
        self.next_usage_target()
    }

    pub(super) fn resume_deferred_usage(&mut self) -> Option<String> {
        if !self.continuation.deferred
            || !self.continuation.automatic
            || !self.continuation.waiting()
            || self.continuation.blocked
            || self.busy
            || self.picker.is_some()
            || self.job.is_some()
            || self.queue.edit.is_some()
            || self.session_ui.loading()
        {
            return None;
        }
        self.continuation.deferred = false;
        self.next_usage_target()
    }

    fn next_usage_target(&mut self) -> Option<String> {
        if self.continuation.context_limited {
            if self.continuation.context_rollovers < 1 {
                return Some(self.account.clone());
            }
            self.push_system("automatic context rollover stopped after one attempt; inspect the source and choose a target explicitly");
            self.open_continuation_picker();
            return None;
        }
        // At most three handoffs, each account/model pair once.
        let known_models = self
            .home_path
            .as_ref()
            .map(|home| crate::catalog::list_models(self.provider, home))
            .unwrap_or_default();
        if let Some(model) = self.continuation.models.iter().find(|model| {
            *model != &self.model
                && known_models.contains(model)
                && !self
                    .continuation
                    .tried
                    .contains(&format!("{} {model}", self.account))
        }) {
            return Some(format!("{} {model}", self.account));
        }
        let accounts = if self.continuation.accounts.is_empty() {
            accounts_with_auth(&self.state_root)
                .into_iter()
                .map(|a| a.account)
                .collect()
        } else {
            self.continuation.accounts.clone()
        };
        let target = accounts
            .iter()
            .filter_map(|id| get_account(&self.state_root, id))
            .find(|account| {
                account.home_path.is_some()
                    && account.account != self.account
                    && !self.continuation.tried.iter().any(|key| {
                        key.split_once(' ')
                            .is_some_and(|(id, _)| id == account.account)
                    })
            })
            .map(|account| account.account);
        if target.is_none() {
            if self.continuation.accounts.is_empty() && self.continuation.models.is_empty() {
                if let Some(model) = known_models.iter().find(|model| {
                    *model != &self.model
                        && !self
                            .continuation
                            .tried
                            .contains(&format!("{} {model}", self.account))
                }) {
                    return Some(format!("{} {model}", self.account));
                }
            }
            self.push_system("automatic continuation stopped: no untried eligible account/model; /use lists accounts");
            self.open_continuation_picker();
        }
        target
    }

    pub(super) fn continue_usage(&mut self, target: &str, tx: &mpsc::UnboundedSender<String>) {
        let (target, model) = target
            .split_once(' ')
            .map(|(a, m)| (a, Some(m.trim())))
            .unwrap_or((target, None));
        if self.busy
            || self.session_ui.loading()
            || self.job.is_some()
            || self.queue.edit.is_some()
            || self.picker.is_some()
        {
            self.push_system(
                "finish the active turn, history load, update or queued edit before continuing",
            );
            return;
        }
        let Some(prompt) = self.continuation.pending.clone() else {
            self.push_system("no usage-limited request to continue; /continuation [manual|automatic] controls handoff");
            return;
        };
        if target.is_empty()
            || (target == self.account
                && !self.continuation.same_account_allowed()
                && model.is_none_or(|m| m == self.model))
        {
            self.push_system("usage: /continue ACCOUNT [MODEL] (choose another account or model)");
            return;
        }
        if let Some(model) = model {
            let valid = get_account(&self.state_root, target).is_some_and(|account| {
                account.home_path.as_ref().is_some_and(|home| {
                    crate::catalog::list_models(account.provider, home)
                        .iter()
                        .any(|m| m == model)
                })
            });
            if !valid {
                self.push_system("continuation model is absent from that account's native catalog; request kept paused");
                return;
            }
        }
        // A draft typed while the turn was running belongs to the user.
        if !get_account(&self.state_root, target).is_some_and(|account| account.home_path.is_some())
        {
            self.push_system(
                "choose a different account with an explicit home from /use before continuing",
            );
            return;
        }
        let draft = self.composer_value();
        if !self.continuation.automatic
            && (!self.editor.text().trim().is_empty() || !self.images.draft.is_empty())
            && draft != prompt
            && self.continuation.retry_draft.as_deref() != Some(draft.as_str())
        {
            self.push_system("save or clear your unsent draft before continuing; it has been kept");
            return;
        }
        let previous_provider = self.provider;
        let previous_home = self
            .home_path
            .as_ref()
            .map(|h| h.to_string_lossy().into_owned());
        let trust = self.trust;
        if let Err(error) = select_account(&self.state_root, target) {
            self.push_system(format!("continuation account unavailable: {error}"));
            return;
        }
        self.refresh_binding();
        if let Some(model) = model {
            if let Err(error) = set_model(&self.state_root, model) {
                self.push_system(format!(
                    "could not select continuation model: {error}; request kept paused"
                ));
                return;
            }
            self.model = model.to_owned();
        }
        self.trust = trust;
        self.native_session_id = None;
        self.native_agents = native_agents::State::default();
        self.prior_provider = previous_provider;
        self.prior_home = previous_home;
        self.need_context_pack = true;
        self.log_switch(previous_provider);
        let unsent = self.composer_value();
        let preserve_draft =
            !unsent.is_empty() && self.continuation.retry_draft.as_deref() != Some(unsent.as_str());
        self.editor.clear();
        self.images.draft.clear();
        self.persist_draft_now();
        self.push_system(format!("continuing with {} using the conversation map; native tool and approval state stay with the previous account", self.account_label()));
        let original = crate::attachments::Message::decode(&prompt);
        let instruction = crate::attachments::Message{text:format!("Continue the interrupted request in this new native session. First inspect the workspace and recorded completed actions. Reconcile any uncertain tool result before proceeding; do not blindly repeat commands or edits. Follow the current provider's approval rules. Original request:\n{}",original.text),images:original.images}.encode();
        if self.continuation.context_limited {
            self.continuation.context_rollovers =
                self.continuation.context_rollovers.saturating_add(1).min(1);
        }
        self.continuation.dispatching = true;
        self.run_line(instruction, tx);
        self.continuation.dispatching = false;
        if preserve_draft {
            self.restore_composer(&unsent);
            self.persist_draft_now();
        }
        // The failure finalizer paused the queue; continuation must finish before explicit resume.
        self.queue.advance = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closing_a_screen_cannot_bypass_the_four_pair_handoff_limit() {
        let (mut app, dir) = setup();
        for id in ["fourth", "fifth"] {
            let home = dir.path().join(id);
            std::fs::create_dir_all(&home).unwrap();
            crate::store::upsert_stored_account(
                dir.path(),
                crate::store::StoredAccount {
                    id: id.into(),
                    provider: ProviderKind::Codex,
                    home_path: Some(home.to_string_lossy().into_owned()),
                    label: id.into(),
                    created_at: "0".into(),
                },
            )
            .unwrap();
            app.continuation.accounts.push(id.into());
        }
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.run_line("Finish it".into(), &tx);
        rx.try_recv().unwrap();
        for target in ["backup", "third", "fourth"] {
            exhausted(&mut app, &tx);
            assert!(rx.try_recv().is_ok());
            assert_eq!(app.account, target);
        }
        exhausted(&mut app, &tx);
        assert!(rx.try_recv().is_err());
        assert!(!app.continuation.deferred);
        app.dismiss_picker();
        app.queue_advance(&tx);
        assert!(rx.try_recv().is_err());
        assert_eq!(app.account, "fourth");
    }
    #[test]
    fn a_finished_error_card_can_handoff_and_a_command_screen_defers_until_closed() {
        for screen in [false, true] {
            let (mut app, _dir) = setup();
            let (tx, mut rx) = mpsc::unbounded_channel();
            app.run_line("Finish the changes".into(), &tx);
            rx.try_recv().unwrap();
            app.live_cards.push(StreamCard {
                id: "finished".into(),
                title: "test".into(),
                detail: Some("exit 1".into()),
                status: CardStatus::Error,
            });
            if screen {
                app.show_picker(
                    PickerKind::Usage,
                    "usage".into(),
                    vec![PickerRow {
                        id: "none".into(),
                        label: "Usage".into(),
                        detail: String::new(),
                    }],
                    false,
                );
            }
            exhausted(&mut app, &tx);
            if screen {
                assert!(rx.try_recv().is_err());
                assert!(app.continuation.deferred);
                app.dismiss_picker();
                app.queue_advance(&tx);
            }
            assert!(rx
                .try_recv()
                .unwrap()
                .contains("Original request:\nFinish the changes"));
            assert_eq!(app.account, "backup");
            app.queue_advance(&tx);
            assert!(rx.try_recv().is_err());
        }
    }

    #[test]
    fn native_history_failure_keeps_request_and_allows_only_explicit_fresh_session_recovery() {
        let (mut app, _dir) = setup();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.run_line("Finish the original task".into(), &tx);
        rx.try_recv().unwrap();
        app.native_session_id = Some("11111111-2222-3333-4444-555555555555".into());
        app.editor.set_text("my newer draft");
        app.on_turn_done(TurnOutcome { exit_code: Some(1), stderr_tail: "ERROR codex_core::session: failed to record rollout items: thread fixture-thread not found".into(), ..Default::default() }, &tx);
        assert!(
            rx.try_recv().is_err(),
            "storage failures must never automatically replay tools"
        );
        assert!(app.continuation.waiting() && app.continuation.blocked);
        assert_eq!(app.editor.text(), "my newer draft");
        let account = app.account.clone();
        app.continue_usage(&account, &tx);
        let prompt = rx.try_recv().unwrap();
        assert!(prompt.contains("Original request:\nFinish the original task"));
        assert!(app.native_session_id.is_none());
        assert_eq!(app.editor.text(), "my newer draft");
    }

    fn setup() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "[continuation]\nmode='automatic'\naccounts=['backup','third']\n",
        )
        .unwrap();
        for id in ["backup", "third"] {
            let home = dir.path().join(id);
            std::fs::create_dir_all(&home).unwrap();
            crate::store::upsert_stored_account(
                dir.path(),
                crate::store::StoredAccount {
                    id: id.into(),
                    provider: ProviderKind::Codex,
                    home_path: Some(home.to_string_lossy().into_owned()),
                    label: id.into(),
                    created_at: "0".into(),
                },
            )
            .unwrap();
        }
        (App::new(dir.path().into(), dir.path().into()), dir)
    }

    fn exhausted(app: &mut App, tx: &mpsc::UnboundedSender<String>) {
        app.apply_progress(TurnProgress::Error("You've hit your usage limit".into()));
        app.on_turn_done(
            TurnOutcome {
                exit_code: Some(1),
                ..Default::default()
            },
            tx,
        );
    }

    #[test]
    fn automatic_handoff_preserves_thread_title_trust_and_queue_and_never_cycles() {
        let (mut app, _dir) = setup();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.run_line("Fix the graph".into(), &tx);
        rx.try_recv().unwrap();
        app.native_session_id = Some("source-native-id".into());
        app.queue.items.push("later task".into());
        let title = app.session_ui.title.clone();
        let trust = app.trust;
        exhausted(&mut app, &tx);
        assert!(rx
            .try_recv()
            .unwrap()
            .contains("Original request:\nFix the graph"));
        assert_eq!(app.account, "backup");
        assert!(app.native_session_id.is_none());
        assert!(app.need_context_pack);
        assert_eq!(app.session_ui.title, title);
        assert_eq!(app.trust, trust);
        assert!(app.queue.paused);
        assert_eq!(
            app.messages
                .iter()
                .filter(|m| m.role == MsgRole::User)
                .count(),
            1
        );
        assert!(app.messages.iter().all(|m| !m
            .text
            .starts_with("Continue the interrupted request in this new native session.")));
        exhausted(&mut app, &tx);
        assert_eq!(app.account, "third");
        rx.try_recv().unwrap();
        exhausted(&mut app, &tx);
        assert!(rx.try_recv().is_err());
        assert!(!app.busy);
        assert_eq!(app.queue.items, ["later task"]);
    }

    #[test]
    fn a_recovered_provider_error_does_not_handoff_but_terminal_failure_does() {
        for terminal_failed in [false, true] {
            let (mut app, _dir) = setup();
            let (tx, mut rx) = mpsc::unbounded_channel();
            app.run_line("Fix the graph".into(), &tx);
            rx.try_recv().unwrap();
            app.apply_progress(TurnProgress::Error("usage limit reached".into()));
            app.on_turn_done(
                TurnOutcome {
                    exit_code: Some(0),
                    failed: terminal_failed,
                    ..Default::default()
                },
                &tx,
            );
            assert_eq!(rx.try_recv().is_ok(), terminal_failed);
            assert_eq!(app.account == "backup", terminal_failed);
        }
    }

    #[test]
    fn structured_quota_codes_trigger_handoff_even_with_a_generic_message_and_zero_exit() {
        for event in [
            serde_json::json!({"type":"error","code":-32000,"error":{"message":"Request failed","codexErrorInfo":"usageLimitExceeded"}}),
            serde_json::json!({"type":"turn.failed","error":{"message":"Request failed","codexErrorInfo":{"UsageLimitExceeded":{}}}}),
            serde_json::json!({"type":"turn.failed","error":{"code":"usage_limit_reached","message":"Request cannot be completed"}}),
            serde_json::json!({"type":"turn.failed","error":{"code":"insufficient_quota"}}),
            serde_json::json!({"type":"error","error":{"code":"quota_exceeded","message":"Unavailable"}}),
        ] {
            let (mut app, _dir) = setup();
            let (tx, mut rx) = mpsc::unbounded_channel();
            app.run_line("Finish the changes".into(), &tx);
            rx.try_recv().unwrap();
            for part in crate::provider::stream_cards::classify_codex_event(&event) {
                if let crate::provider::stream_cards::StreamPart::Error(error) = part {
                    app.apply_progress(TurnProgress::Error(error));
                }
            }
            app.on_turn_done(
                TurnOutcome {
                    failed: true,
                    exit_code: Some(0),
                    ..Default::default()
                },
                &tx,
            );
            assert_eq!(app.account, "backup", "{event}");
            assert!(rx
                .try_recv()
                .unwrap()
                .contains("Original request:\nFinish the changes"));
        }
    }

    #[test]
    fn ordinary_errors_cancellation_and_uncertain_tools_pause_but_drafts_survive_handoff() {
        for case in [
            "ordinary",
            "cancel",
            "tool",
            "draft",
            "manual",
            "unknown_exit",
        ] {
            let (mut app, _dir) = setup();
            let (tx, mut rx) = mpsc::unbounded_channel();
            app.run_line("Fix the graph".into(), &tx);
            rx.try_recv().unwrap();
            if case == "tool" {
                app.live_cards.push(StreamCard {
                    id: "uncertain".into(),
                    title: "shell".into(),
                    detail: None,
                    status: CardStatus::Running,
                });
            }
            if case == "draft" {
                app.editor.set_text("my unsent draft");
            }
            if case == "manual" {
                app.continuation.automatic = false;
            }
            if case == "unknown_exit" {
                app.queue.items.push("later task".into());
            }
            app.apply_progress(TurnProgress::Error(
                if case == "ordinary" {
                    "connection failed"
                } else {
                    "usage limit reached"
                }
                .into(),
            ));
            app.on_turn_done(
                TurnOutcome {
                    exit_code: if case == "unknown_exit" {
                        None
                    } else {
                        Some(1)
                    },
                    cancelled: case == "cancel",
                    ..Default::default()
                },
                &tx,
            );
            if case == "draft" {
                assert!(rx.try_recv().unwrap().contains("Fix the graph"));
                assert_eq!(app.account, "backup");
                assert_eq!(app.editor.text(), "my unsent draft");
            } else {
                assert!(rx.try_recv().is_err(), "{case}");
            }
            if case == "unknown_exit" {
                assert!(app.queue.paused);
                assert_eq!(app.queue.items, ["later task"]);
            }
            if case == "manual" {
                app.run_line("/continue backup".into(), &tx);
                assert!(rx.try_recv().unwrap().contains("Fix the graph"));
            }
        }
    }
}

#[cfg(test)]
mod model_tests {
    use super::*;
    #[test]
    fn automatic_model_handoff_keeps_request_and_never_retries_a_pair() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("models_cache.json"),
            r#"["model-a","model-b","model-c"]"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("config.toml"), "[continuation]\nmode='automatic'\naccounts=['synthetic']\nmodels=['missing-model','model-b','model-c']\n").unwrap();
        crate::store::upsert_stored_account(
            dir.path(),
            crate::store::StoredAccount {
                id: "synthetic".into(),
                provider: ProviderKind::Codex,
                home_path: Some(home.to_string_lossy().into()),
                label: "synthetic".into(),
                created_at: "0".into(),
            },
        )
        .unwrap();
        select_account(dir.path(), "synthetic").unwrap();
        set_model(dir.path(), "model-a").unwrap();
        let mut app = App::new(dir.path().into(), dir.path().into());
        app.refresh_binding();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.run_line("Finish the existing changes".into(), &tx);
        rx.try_recv().unwrap();
        for expected in ["model-b", "model-c"] {
            app.apply_progress(TurnProgress::Error("rate_limit_exceeded".into()));
            app.on_turn_done(
                TurnOutcome {
                    exit_code: Some(1),
                    ..Default::default()
                },
                &tx,
            );
            let prompt = rx.try_recv().unwrap();
            assert!(prompt.contains("Original request:\nFinish the existing changes"));
            assert_eq!(app.model, expected);
            assert_eq!(app.account, "synthetic");
            assert!(app.need_context_pack);
        }
        app.apply_progress(TurnProgress::Error("usage limit".into()));
        app.on_turn_done(
            TurnOutcome {
                exit_code: Some(1),
                ..Default::default()
            },
            &tx,
        );
        assert!(rx.try_recv().is_err());
        assert!(!app.busy);
    }

    #[test]
    fn automatic_preference_survives_a_new_app() {
        let dir = tempfile::tempdir().unwrap();
        crate::store::set_continuation_automatic(dir.path(), true).unwrap();
        assert!(State::new(dir.path()).automatic());
        crate::store::set_continuation_automatic(dir.path(), false).unwrap();
        assert!(!State::new(dir.path()).automatic());
    }
}

#[cfg(test)]
mod context_tests {
    use super::*;
    #[test]
    fn explicit_context_failure_rolls_over_once_without_treating_usage_totals_as_capacity() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("fixture-home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "[continuation]\nmode='automatic'\n",
        )
        .unwrap();
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
        select_account(dir.path(), "fixture").unwrap();
        let mut app = App::new(dir.path().into(), dir.path().into());
        app.refresh_binding();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.run_line("Finish existing work".into(), &tx);
        rx.try_recv().unwrap();
        app.queue.items.push("later queued request".into());
        app.native_session_id = Some("old-explicit-session".into());
        app.apply_progress(TurnProgress::Error("context_length_exceeded".into()));
        app.on_turn_done(
            TurnOutcome {
                exit_code: Some(1),
                ..Default::default()
            },
            &tx,
        );
        assert!(rx
            .try_recv()
            .unwrap()
            .contains("Original request:\nFinish existing work"));
        assert_eq!(app.account, "fixture");
        assert!(app.native_session_id.is_none());
        assert!(app.need_context_pack);
        assert_eq!(app.native_sources[0].session_id, "old-explicit-session");
        app.apply_progress(TurnProgress::Error("context_length_exceeded".into()));
        app.on_turn_done(
            TurnOutcome {
                exit_code: Some(1),
                ..Default::default()
            },
            &tx,
        );
        assert!(
            rx.try_recv().is_err(),
            "second automatic context rollover must stop"
        );
        assert!(app.queue.paused);
        for text in [
            "token usage 1000000",
            "context size unknown",
            "context_length_exceeded in normal explanatory prose",
        ] {
            let mut app = App::new(dir.path().into(), dir.path().into());
            app.refresh_binding();
            app.run_line("work".into(), &tx);
            rx.try_recv().unwrap();
            app.on_turn_done(
                TurnOutcome {
                    exit_code: Some(0),
                    text: text.into(),
                    ..Default::default()
                },
                &tx,
            );
            assert!(rx.try_recv().is_err());
        }
    }
    #[test]
    fn cancel_requested_after_provider_failure_still_prevents_handoff() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().into(), dir.path().into());
        app.continuation.automatic = true;
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.run_line("original request".into(), &tx);
        rx.try_recv().unwrap();
        app.apply_progress(TurnProgress::Error("context_length_exceeded".into()));
        app.request_cancel();
        app.on_turn_done(
            TurnOutcome {
                exit_code: Some(1),
                ..Default::default()
            },
            &tx,
        );
        assert!(rx.try_recv().is_err());
        assert!(!app.continuation.waiting());
        assert!(!app.busy);
    }
}
