//! Bounded, opt-in handoff after a provider explicitly reports exhausted usage.
use super::*;
use std::collections::HashSet;

pub(super) struct State {
    automatic: bool,
    accounts: Vec<String>,
    tried: HashSet<String>,
    pub errors: Vec<String>,
    prompt: Option<String>,
    pending: Option<String>,
    retry_draft: Option<String>,
    dispatching: bool,
    blocked: bool,
}

impl State {
    pub fn new(root: &Path) -> Self {
        let config = crate::config::for_root(root).continuation;
        Self {
            automatic: config.automatic,
            accounts: config.accounts,
            tried: HashSet::new(),
            errors: Vec::new(),
            prompt: None,
            pending: None,
            retry_draft: None,
            dispatching: false,
            blocked: false,
        }
    }

    pub fn begin(&mut self, account: &str, prompt: &str) {
        if !self.dispatching {
            self.tried.clear();
            self.prompt = Some(prompt.to_owned());
        }
        self.tried.insert(account.to_owned());
        self.errors.clear();
        self.pending = None;
        self.retry_draft = None;
        self.blocked = false;
    }

    pub fn invalidate(&mut self) {
        self.pending = None;
        self.errors.clear();
    }

    pub fn waiting(&self) -> bool {
        self.pending.is_some()
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
    let lower = text.to_ascii_lowercase();
    [
        "usage limit",
        "usage_limit",
        "quota exceeded",
        "quota_exceeded",
        "insufficient_quota",
        "you've hit your limit",
        "you have hit your limit",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase))
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
        self.push_system(format!("usage continuation: {mode}; automatic targets: {}. Set [continuation] mode and accounts in config.toml for future launches.", if accounts.is_empty() { "none configured" } else { &accounts }));
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
        let exhausted = self.continuation.errors.iter().any(|e| usage_exhausted(e))
            || (outcome.exit_code.is_some_and(|code| code != 0)
                && usage_exhausted(&outcome.stderr_tail));
        if !exhausted {
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
                .any(|card| card.status != CardStatus::Done);
        self.push_system(
            "usage limit reached; choose another account to continue. Queued prompts stay paused.",
        );
        if !self.continuation.automatic {
            self.open_continuation_picker();
        }
        if self.continuation.blocked {
            self.push_system("automatic continuation stopped: provider or tool completion is uncertain. Inspect its output before continuing.");
            return None;
        }
        if !self.continuation.automatic
            || !self.editor.text().trim().is_empty()
            || self.queue.edit.is_some()
            || self.picker.is_some()
            || self.job.is_some()
            || self.continuation.tried.len() >= 4
        {
            return None;
        }
        // Ordered allowlist, one attempt per account, at most three handoffs per request.
        let target = self
            .continuation
            .accounts
            .iter()
            .filter_map(|id| get_account(&self.state_root, id))
            .find(|account| {
                account.home_path.is_some()
                    && account.account != self.account
                    && !self.continuation.tried.contains(&account.account)
            })
            .map(|account| account.account);
        if target.is_none() {
            self.push_system("automatic continuation stopped: no untried configured account; /use lists accounts");
        }
        target
    }

    pub(super) fn continue_usage(&mut self, target: &str, tx: &mpsc::UnboundedSender<String>) {
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
        if target.is_empty() || target == self.account {
            self.push_system("usage: /continue ACCOUNT (choose another account from /use)");
            return;
        }
        // A draft typed while the turn was running belongs to the user.
        if !get_account(&self.state_root, target)
            .is_some_and(|account| account.home_path.is_some() && account.account != self.account)
        {
            self.push_system(
                "choose a different account with an explicit home from /use before continuing",
            );
            return;
        }
        if !self.editor.text().trim().is_empty()
            && self.editor.text() != prompt
            && self.continuation.retry_draft.as_deref() != Some(self.editor.text().as_str())
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
        self.trust = trust;
        self.native_session_id = None;
        self.native_agents = native_agents::State::default();
        self.prior_provider = previous_provider;
        self.prior_home = previous_home;
        self.need_context_pack = true;
        self.log_switch(previous_provider);
        self.editor.clear();
        self.persist_draft_now();
        self.push_system(format!("continuing with {} using the conversation map; native tool and approval state stay with the previous account", self.account_label()));
        let instruction = format!("Continue the request interrupted by a provider usage limit. First inspect the workspace and recorded completed actions. Reconcile any uncertain tool result before proceeding; do not blindly repeat commands or edits. Follow the current provider's approval rules. Original request:\n{prompt}");
        self.continuation.dispatching = true;
        self.run_line(instruction, tx);
        self.continuation.dispatching = false;
        // The failure finalizer paused the queue; continuation must finish before explicit resume.
        self.queue.advance = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn ordinary_errors_cancellation_uncertain_tools_and_unsent_drafts_never_auto_send() {
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
            assert!(rx.try_recv().is_err(), "{case}");
            if case == "draft" {
                assert_eq!(app.editor.text(), "my unsent draft");
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
