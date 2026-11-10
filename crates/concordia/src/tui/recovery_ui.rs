//! Explicit selection of abandoned pane recovery; active panes never appear here.
use super::{recovery, App, PickerKind, PickerRow};

impl App {
    pub(super) fn open_recovery_picker(&mut self) {
        self.show_recovery_picker(true);
    }

    pub(super) fn offer_startup_recovery(&mut self) {
        self.show_recovery_picker(false);
    }

    fn show_recovery_picker(&mut self, refresh: bool) {
        if self.busy {
            self.push_system("finish or cancel the current turn before recovering a draft");
            return;
        }
        if let Some(warning) = self.recovery_warning() {
            self.push_system(format!(
                "{warning}; use /recover retry after fixing local storage"
            ));
            return;
        }
        if let Some(root) = self.keep.recovery_root.as_ref().filter(|_| refresh) {
            match recovery::available(root) {
                Ok(choices) => self.keep.recovery_choices = choices,
                Err(error) => {
                    self.push_system(format!("could not list saved drafts: {error}"));
                    return;
                }
            }
        }
        let mut rows: Vec<PickerRow> = self
            .recovery_choices()
            .iter()
            .map(|choice| {
                let preview: String = choice
                    .draft
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(80)
                    .collect();
                PickerRow {
                    id: format!("recover:{}", choice.id),
                    label: if preview.is_empty() {
                        "queued prompts".into()
                    } else {
                        preview
                    },
                    detail: format!(
                        "{} queued - {}",
                        choice.queued,
                        saved_age(choice.updated_unix_ms)
                    ),
                }
            })
            .collect();
        rows.extend(self.recovery_choices().iter().map(|choice| PickerRow {
            id: format!("recover-preview:{}", choice.id),
            label: format!("Preview saved draft {}", choice.id),
            detail: "read the draft and queued prompts without restoring".into(),
        }));
        if recovery::legacy_available(&self.state_root) {
            rows.push(PickerRow {
                id: "recover:legacy".into(),
                label: "Import older shared draft and queue".into(),
                detail: "copies into this pane; original files are kept".into(),
            });
        }
        if rows.is_empty() {
            self.push_system("no abandoned drafts or queues for this workspace");
            return;
        }
        self.show_picker(
            PickerKind::Recovery,
            "saved drafts - Esc dismisses; /recover opens them later".into(),
            rows,
            true,
        );
    }

    pub(super) fn preview_recovery(&mut self, id: &str) {
        super::landing::close(self);
        let result = self
            .keep
            .recovery_root
            .as_ref()
            .ok_or_else(|| std::io::Error::other("recovery storage unavailable"))
            .and_then(|root| recovery::claim(root, id))
            .and_then(|lease| {
                let data = lease.data()?;
                let mut text = format!(
                    "Saved draft {id}\n{}",
                    crate::attachments::Message::text(&data.draft)
                );
                for (i, prompt) in data.queue.iter().enumerate() {
                    text.push_str(&format!(
                        "\n\nQueued {}\n{}",
                        i + 1,
                        crate::attachments::Message::text(prompt)
                    ));
                }
                if let Some(turn) = lease.turn()? {
                    text.push_str(&format!(
                        "\n\nUnfinished request\n{}",
                        crate::attachments::Message::text(&turn.prompt)
                    ));
                }
                if let Some(edit) = data.pending_edit {
                    text.push_str(&format!(
                        "\n\nUnfinished queue edit\n{}",
                        crate::attachments::Message::text(&edit.text)
                    ));
                }
                Ok(text)
            });
        self.push_system(
            result.unwrap_or_else(|error| format!("could not preview saved draft: {error}")),
        );
    }

    pub(super) fn choose_recovery(&mut self, id: &str) {
        if self.busy {
            self.push_system("finish or cancel the current turn before recovering a draft");
            return;
        }
        if self.queue.edit.is_some() {
            self.push_system("save or cancel the queued edit before recovering another draft");
            return;
        }
        // Switching ownership leaves the prior record intact for later recovery.
        self.persist_draft_now();
        if self.recovery_warning().is_some() {
            self.push_system(
                "could not save this pane; recovery switch cancelled to preserve your work",
            );
            return;
        }
        let result = if id == "legacy" {
            self.import_legacy_recovery()
        } else {
            self.claim_recovery(id)
        };
        if let Err(error) = result {
            self.push_system(format!(
                "could not recover that draft; current draft kept: {error}"
            ));
        }
    }
}

fn saved_age(saved_ms: u128) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let seconds = now.saturating_sub(saved_ms) / 1000;
    match seconds {
        0..=59 => "saved just now".into(),
        60..=3599 => format!("saved {}m ago", seconds / 60),
        3600..=86399 => format!("saved {}h ago", seconds / 3600),
        _ => format!("saved {}d ago", seconds / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_offer_keeps_dismissed_records_hidden_and_home_escape_dismisses() {
        let state = tempfile::tempdir().unwrap();
        let root = recovery::recovery_root(state.path(), state.path()).unwrap();
        let old = recovery::create(&root).unwrap();
        let old_id = old.record_id();
        old.save("previously declined", &[]).unwrap();
        drop(old);
        recovery::dismiss(&root, &recovery::available(&root).unwrap()).unwrap();
        for draft in ["new draft one", "new draft two"] {
            let saved = recovery::create(&root).unwrap();
            saved.save(draft, &[]).unwrap();
        }
        let mut app = App::new(state.path().into(), state.path().into());
        app.restore_state();
        app.offer_startup_recovery();
        assert_eq!(app.keep.recovery_choices.len(), 2);
        assert!(!app
            .picker
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.id.ends_with(&old_id)));
        super::super::landing::open_home_tab(&mut app);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        super::super::input_dispatch::handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            ),
            &tx,
        );
        assert!(app.picker.is_none());
        assert!(super::super::landing::visible(&app));
        assert!(rx.try_recv().is_err());
        drop(app);
        let mut restarted = App::new(state.path().into(), state.path().into());
        restarted.restore_state();
        assert!(restarted.keep.recovery_choices.is_empty());
        restarted.open_recovery_picker();
        assert_eq!(restarted.keep.recovery_choices.len(), 3);
    }

    #[test]
    fn picker_hides_active_panes_and_failed_claim_preserves_current_work() {
        let state = tempfile::tempdir().unwrap();
        let mut app = App::new(state.path().into(), state.path().into());
        app.restore_state();
        app.editor.set_text("current draft");
        app.queue.items = vec!["current queue".into()];
        app.persist_draft_now();
        let root = app.keep.recovery_root.clone().unwrap();
        let active = recovery::create(&root).unwrap();
        active.save("active pane", &[]).unwrap();
        let abandoned = recovery::create(&root).unwrap();
        let id = abandoned.id().to_owned();
        abandoned
            .save("abandoned draft", &["abandoned queue".into()])
            .unwrap();
        drop(abandoned);
        app.open_recovery_picker();
        let picker = app.picker.as_ref().unwrap();
        assert_eq!(picker.rows.len(), 2);
        assert_eq!(picker.rows[0].id, format!("recover:{id}"));
        app.picker.as_mut().unwrap().state.select(Some(1));
        app.apply_picker_selection(None);
        assert_eq!(app.editor.text(), "current draft");
        assert_eq!(app.queue.items, ["current queue"]);
        let preview = &app.messages.last().unwrap().text;
        assert!(preview.contains("abandoned draft") && preview.contains("abandoned queue"));
        assert_eq!(
            recovery::claim(&root, &id).unwrap().data().unwrap().draft,
            "abandoned draft"
        );
        app.open_recovery_picker();
        let claimed_elsewhere = recovery::claim(&root, &id).unwrap();
        app.choose_recovery(&id);
        assert_eq!(app.editor.text(), "current draft");
        assert_eq!(app.queue.items, ["current queue"]);
        drop(claimed_elsewhere);
        app.choose_recovery(&id);
        assert_eq!(app.editor.text(), "abandoned draft");
        assert_eq!(app.queue.items, ["abandoned queue"]);
        assert!(app.queue.paused);
        let prior = recovery::available(&root).unwrap();
        assert!(prior.iter().any(|c| c.draft == "current draft"));
        assert!(!prior.iter().any(|c| c.draft == "active pane"));
    }
}
