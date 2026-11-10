use super::super::*;

impl App {
    pub(in crate::tui) fn after_undo(&mut self) {
        self.palette.reset();
        self.hist_pos = None;
        self.view.clear_selection();
    }
    pub(in crate::tui) fn flash(&mut self, msg: impl Into<String>) {
        self.flash = Some((msg.into(), Instant::now()));
    }
    pub(in crate::tui) fn paste_text(&mut self, text: &str) {
        self.paste_text_chunk(text, false);
    }
    pub(in crate::tui) fn paste_text_chunk(&mut self, text: &str, continues: bool) {
        let title_focus = !worker_input::active(self)
            && !self.raster.active()
            && !self.project_search.active()
            && self.picker.is_none();
        if let Some(editor) = self.rename_editor.as_mut().filter(|_| title_focus) {
            let capacity = 256usize.saturating_sub(
                editor.chars().len() - editor.selection().map_or(0, |(a, b)| b - a),
            );
            let clean: String = text
                .chars()
                .filter(|c| !c.is_control())
                .take(capacity)
                .collect();
            editor.insert_paste_chunk(&clean, continues);
            return;
        }
        if self.local_terminal.visible() {
            self.local_terminal.paste(text);
            return;
        }
        self.busy_send_confirmation = None;
        if self.raster.active() {
            return;
        }
        if integrations::paste(self, text) {
            return;
        }
        if worker_input::active(self) {
            worker_input::paste(self, text);
            return;
        }
        if self.project_search.active() {
            self.project_search.paste(text);
            return;
        }
        // Paste can arrive as one Event::Paste or as a buffered burst split into overflow
        // chunks. Route every chunk here so a modal never edits the prompt or queue editor.
        if self.activity.detail.is_some() {
            self.activity.insert_query(text);
            return;
        }
        if let Some(picker) = self.picker.as_mut() {
            let clean: String = text::normalize_newlines(text)
                .chars()
                .flat_map(|c| if c == '\t' { vec![' '; 4] } else { vec![c] })
                .collect();
            picker.filter.push_str(&clean);
            picker.select_clamp();
            return;
        }
        if queue::paste(self, text, continues) {
            return;
        }
        if self.workspace.focused.is_some() {
            if workspace::paste(self, text) {
                return;
            }
            self.flash("read-only file pane; Esc returns to chat before pasting");
            return;
        }
        if landing::handle_paste(self, text) {
            return;
        }
        self.view.clear_selection();
        self.palette.reset();
        self.questions.focused = false;
        self.editor.insert_paste_chunk(text, continues);
        if !continues {
            self.offer_pasted_files(text);
        }
    }
    pub(in crate::tui) fn selected_text(&self) -> Option<String> {
        self.view
            .selected_text()
            .or_else(|| self.editor.selected_text())
    }
    pub(in crate::tui) fn has_selection(&self) -> bool {
        self.view.sel.is_some_and(|(a, b)| a != b) || self.editor.selection().is_some()
    }
    pub(in crate::tui) fn clear_selections(&mut self) {
        self.view.clear_selection();
        self.editor.clear_selection();
        self.drag = Drag::None;
    }
    pub(in crate::tui) fn last_assistant_text(&self) -> Option<String> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == MsgRole::Assistant && !m.text.trim().is_empty())
            .map(|m| m.text.clone())
    }
    pub(in crate::tui) fn navigate_activity_card(&mut self, older: bool) {
        let current = self.activity.detail.unwrap_or(self.messages.len());
        let next = if older {
            (0..current).rev().find(|&i| {
                self.messages
                    .get(i)
                    .is_some_and(|m| m.role == MsgRole::Card)
            })
        } else {
            (current + 1..self.messages.len()).find(|&i| {
                self.messages
                    .get(i)
                    .is_some_and(|m| m.role == MsgRole::Card)
            })
        };
        if let Some(i) = next {
            self.activity.open(i);
        }
    }
    pub(in crate::tui) fn open_tool_details(&mut self, index: usize) {
        if self
            .messages
            .get(index)
            .is_some_and(|message| message.role == MsgRole::Card)
        {
            self.activity.open(index);
        }
    }
    pub(in crate::tui) fn write_clipboard(&mut self, text: &str) -> bool {
        match clipboard::set_text(text) {
            Ok(()) => {
                self.flash(format!("copied {} chars", text.chars().count()));
                true
            }
            Err(e) => {
                self.flash(e);
                false
            }
        }
    }
    pub(in crate::tui) fn cut(&mut self) {
        if let Some(t) = self.editor.selected_text() {
            if self.write_clipboard(&t) {
                self.editor.cut();
                self.flash(format!("cut {} chars", t.chars().count()));
            }
        }
    }
}
