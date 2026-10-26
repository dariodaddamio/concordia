//! Explicit selection of abandoned pane recovery; active panes never appear here.
use super::{recovery, App, PickerKind, PickerRow};

impl App {
    pub(super) fn open_recovery_picker(&mut self) {
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
        if let Some(root) = &self.keep.recovery_root {
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
            "recover a saved draft - Esc keeps this pane".into(),
            rows,
            true,
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
        assert_eq!(picker.rows.len(), 1);
        assert_eq!(picker.rows[0].id, format!("recover:{id}"));
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
