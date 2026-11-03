//! Pane-owned unfinished request recovery. It never dispatches a turn during restoration.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Checkpoint {
    pub version: u32,
    pub run_id: String,
    pub workspace: PathBuf,
    pub context_rollovers: u8,
    pub prompt: String,
    pub title: Option<String>,
    pub sources: Vec<crate::native_memory::Binding>,
    pub tried: Vec<String>,
    pub reason: String,
}

impl App {
    pub(super) fn remember_native_source(&mut self) {
        let (Some(home), Some(session)) = (&self.home_path, &self.native_session_id) else {
            return;
        };
        let Ok(binding) =
            crate::native_memory::Binding::new(self.provider, &self.account, home, session)
        else {
            return;
        };
        if !self.native_sources.contains(&binding) {
            if let Some(parent) = self.native_sources.last() {
                if let Err(error) = crate::native_memory::retrieval::observe_handoff(
                    &self.state_root,
                    parent,
                    &binding,
                    &self.cwd,
                ) {
                    self.flash(format!("source relationship could not be saved: {error}"));
                }
            }
            self.native_sources.push(binding);
            if self.native_sources.len() > 8 {
                self.native_sources.remove(0);
            }
        }
    }

    pub(super) fn save_turn_checkpoint(&mut self, closed: bool) -> bool {
        self.remember_native_source();
        let Some(lease) = &self.keep.lease else {
            #[cfg(test)]
            return true; // Unit harnesses can dispatch into fake channels without startup persistence.
            #[cfg(not(test))]
            {
                self.keep.turn_recovery_notice = true;
                self.push_system("unfinished-request recovery is unavailable; request was not dispatched. Fix local storage and use /recover retry.");
                return false;
            }
        };
        let checkpoint = if closed {
            None
        } else {
            self.continuation.checkpoint(
                self.session_ui.title.clone(),
                self.native_sources.clone(),
                self.cwd.clone(),
            )
        };
        if let Err(error) = lease.save_turn(checkpoint.as_ref()) {
            self.keep.turn_recovery_notice = true;
            self.push_system(format!(
                "could not save unfinished request: {error}; local recovery may be unavailable"
            ));
            return false;
        }
        true
    }

    pub(super) fn restore_turn_checkpoint(&mut self) {
        self.continuation.invalidate();
        let result = self.keep.lease.as_ref().map(|lease| lease.turn());
        self.keep.turn_recovery_notice = matches!(&result, Some(Ok(Some(_))) | Some(Err(_)));
        match result {
            Some(Ok(Some(checkpoint))) => {
                if checkpoint.version != 1
                    || uuid::Uuid::parse_str(&checkpoint.run_id).is_err()
                    || checkpoint.prompt.len() > 1024 * 1024
                    || checkpoint.sources.len() > 8
                    || checkpoint.tried.len() > 4
                    || checkpoint.context_rollovers > 1
                    || std::fs::canonicalize(&checkpoint.workspace).ok()
                        != std::fs::canonicalize(&self.cwd).ok()
                {
                    self.push_system(
                        "unfinished-request record is unsupported; it has been kept for inspection",
                    );
                    return;
                }
                self.native_sources = checkpoint.sources.clone();
                self.native_session_id = None;
                self.session_ui.invalidate();
                self.session_ui.title = checkpoint.title.clone();
                self.deleg.change_conversation();
                self.messages.clear();
                self.related_files.reset();
                self.questions.clear();
                self.activity = activity::Activity::default();
                self.native_agents = native_agents::State::default();
                self.view = TranscriptView::default();
                self.usage_block = None;
                self.thread = crate::threads::start(&self.state_root);
                self.thread_meta_done = false;
                self.continuation.restore_checkpoint(checkpoint);
                self.queue.paused = true;
                self.queue.advance = false;
                self.push_system("restored your unsent draft and queued prompts; unfinished request recovered and paused; /continue ACCOUNT [MODEL] starts a new native session with source pointers. Inspect uncertain actions first. Your newer draft is separate.");
            }
            Some(Err(error)) => self.push_system(format!(
                "unfinished-request recovery unavailable: {error}; existing record kept"
            )),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restart_keeps_original_request_and_newer_draft_separate_without_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("fixture-home");
        std::fs::create_dir_all(&home).unwrap();
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
        let (tx, mut rx) = mpsc::unbounded_channel();
        {
            let mut app = App::new(dir.path().into(), dir.path().into());
            app.refresh_binding();
            app.restore_state();
            app.run_line("Finish the parser fix".into(), &tx);
            rx.try_recv().unwrap();
            app.apply_progress(TurnProgress::Session("explicit-original-session".into()));
            app.editor.set_text("newer unsent draft");
            app.queue.items.push("later work".into());
            app.persist_draft_now();
            assert_eq!(
                app.keep
                    .lease
                    .as_ref()
                    .unwrap()
                    .turn()
                    .unwrap()
                    .unwrap()
                    .prompt,
                "Finish the parser fix"
            );
        }
        let mut app = App::new(dir.path().into(), dir.path().into());
        app.refresh_binding();
        app.restore_state();
        assert!(
            rx.try_recv().is_err(),
            "restoration cannot send a provider turn"
        );
        assert!(app.continuation.waiting());
        assert_eq!(app.editor.text(), "newer unsent draft");
        assert_eq!(
            app.native_sources[0].session_id,
            "explicit-original-session"
        );
        assert!(
            app.native_session_id.is_none(),
            "restart does not assume old native writer is gone"
        );
        assert!(app.queue.paused && !app.queue.advance);
        app.run_line("/continue fixture".into(), &tx);
        assert!(
            rx.try_recv().is_err(),
            "newer draft must block continuation"
        );
        app.editor.clear();
        app.run_line("/continue fixture".into(), &tx);
        assert!(rx
            .try_recv()
            .unwrap()
            .contains("Original request:\nFinish the parser fix"));
        assert!(app.need_context_pack);
        app.on_turn_done(
            TurnOutcome {
                exit_code: Some(0),
                ..Default::default()
            },
            &tx,
        );
        assert!(app.keep.lease.as_ref().unwrap().turn().unwrap().is_none());
    }

    #[test]
    fn failure_to_record_the_request_prevents_dispatch_and_keeps_the_draft() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().into(), dir.path().into());
        app.restore_state();
        // Create a directory at the checkpoint path, preserving it rather than overwriting it.
        let root = app.keep.recovery_root.as_ref().unwrap();
        let record = std::fs::read_dir(root)
            .unwrap()
            .flatten()
            .find(|e| e.file_type().unwrap().is_dir())
            .unwrap()
            .path();
        std::fs::create_dir(record.join("turn.json")).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.run_line("must not dispatch".into(), &tx);
        assert!(rx.try_recv().is_err());
        assert_eq!(app.editor.text(), "must not dispatch");
        assert!(!app.busy);
    }
}
