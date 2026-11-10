//! Browser-style conversation tabs.
//!
//! The runner owns the provider channels; this module owns tab identity and lifecycle.  Keeping
//! whole `App` values here is intentional: drafts, queues, selections and native session ids are
//! consequently never shared between conversations.

use super::App;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// What a tab must answer before it closes.
pub(crate) trait TabState {
    fn needs_close_confirmation(&self) -> bool;
    fn can_evict_closed(&self) -> bool {
        false
    }
}

impl TabState for App {
    fn needs_close_confirmation(&self) -> bool {
        self.busy
            || self.local_terminal.running()
            || self.deleg.d.any_active()
            || self.deleg.has_pending_prompt()
            || !self.editor.text().is_empty()
            || self.composer_images().count() > 0
            || self.composer_images().importing()
            || !self.queue.items.is_empty()
            || self.queue.edit.is_some()
            || self.rename_editor.is_some()
            || super::landing::editing_goal(self)
            || self.questions.waiting()
            || super::workers::overlay_open(self)
            || self.workspace.has_unsaved()
    }

    fn can_evict_closed(&self) -> bool {
        !self.needs_close_confirmation()
            && self.preparing_cancel.is_none()
            && self.cancel_handle.is_none()
            && !self.cancel_requested
            && self.job.is_none()
            && self.picker.is_none()
            && !self.session_ui.loading()
            && !self.continuation.waiting()
            && self.continuation_target.is_none()
            && !self.keep.turn_recovery_notice
            && !self.reference_journal.pending()
            && !self.reference_journal.backlogged()
            && self.recovery_warning().is_none()
            && self
                .parked_draft
                .as_ref()
                .is_none_or(|draft| draft.is_empty())
            && self.hist_draft.is_empty()
            && self
                .keep
                .lease
                .as_ref()
                .is_some_and(|lease| matches!(lease.turn(), Ok(None)))
            && (self.native_session_id.is_some()
                || !self.messages.iter().any(|message| {
                    matches!(
                        message.role,
                        super::MsgRole::User | super::MsgRole::Assistant
                    )
                }))
    }
}

pub(crate) struct TabSet<T = App> {
    pub tabs: Vec<T>,
    pub active: usize,
    pub closed: Vec<T>,
}

impl<T: TabState> TabSet<T> {
    pub(crate) fn new(first: T) -> Self {
        Self {
            tabs: vec![first],
            active: 0,
            closed: Vec::new(),
        }
    }

    pub(crate) fn active(&self) -> &T {
        &self.tabs[self.active]
    }
    pub(crate) fn active_mut(&mut self) -> &mut T {
        &mut self.tabs[self.active]
    }
    pub(crate) fn has_multiple(&self) -> bool {
        self.tabs.len() > 1
    }

    pub(crate) fn add(&mut self, tab: T) {
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
    }

    /// Returns whether the tab may close immediately.  The caller owns the confirmation UI.
    pub(crate) fn needs_close_confirmation(&self) -> bool {
        self.active().needs_close_confirmation()
    }

    pub(crate) fn close(&mut self, confirmed: bool) -> bool {
        if self.tabs.len() == 1 || (!confirmed && self.needs_close_confirmation()) {
            return false;
        }
        let index = self.active;
        let tab = self.tabs.remove(index);
        self.closed.push(tab);
        self.prune_closed();
        self.active = index.min(self.tabs.len() - 1);
        true
    }

    /// Keep a small reopen cache of clean conversations. Unsaved or unfinished
    /// closed tabs stay owned in memory; eviction never deletes native/recovery files.
    pub(crate) fn prune_closed(&mut self) {
        let mut clean = self
            .closed
            .iter()
            .filter(|tab| tab.can_evict_closed())
            .count();
        if clean <= 8 {
            return;
        }
        self.closed.retain(|tab| {
            if clean > 8 && tab.can_evict_closed() {
                clean -= 1;
                false
            } else {
                true
            }
        });
    }

    pub(crate) fn reopen(&mut self) -> bool {
        let Some(tab) = self.closed.pop() else {
            return false;
        };
        self.add(tab);
        true
    }

    pub(crate) fn next(&mut self, backwards: bool) {
        if self.tabs.len() < 2 {
            return;
        }
        let n = self.tabs.len();
        self.active = if backwards {
            (self.active + n - 1) % n
        } else {
            (self.active + 1) % n
        };
    }

    pub(crate) fn select_number(&mut self, number: usize) {
        if (1..=self.tabs.len()).contains(&number) {
            self.active = number - 1;
        }
    }
}

/// Tab keys, recognised before any other dispatch so pickers and the editor never see them.
pub(crate) fn action_for(key: KeyEvent) -> TabAction {
    if key.kind == KeyEventKind::Release
        || !key.modifiers.contains(KeyModifiers::CONTROL)
        || key.modifiers.contains(KeyModifiers::ALT)
    {
        return TabAction::Unhandled;
    }
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Char('t' | 'T') if shift => TabAction::Reopen,
        KeyCode::Char('t') => TabAction::New,
        KeyCode::Char('w') if !shift => TabAction::Close,
        KeyCode::BackTab => TabAction::Previous,
        KeyCode::Tab if shift => TabAction::Previous,
        KeyCode::Tab | KeyCode::PageDown => TabAction::Next,
        KeyCode::PageUp => TabAction::Previous,
        KeyCode::Char(c @ '1'..='9') if !shift => TabAction::Select(c as usize - '0' as usize),
        _ => TabAction::Unhandled,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabAction {
    Unhandled,
    New,
    Home,
    BackToChat,
    Close,
    CloseAt(usize),
    Reopen,
    Next,
    Previous,
    Select(usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn app() -> App {
        App::new(
            PathBuf::from("tabs-test-state"),
            PathBuf::from("tabs-test-workspace"),
        )
    }
    fn key(c: char, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), mods)
    }

    #[test]
    fn ctrl_t_opens_home_tab() {
        assert_eq!(action_for(key('t', KeyModifiers::CONTROL)), TabAction::New);
    }
    #[test]
    fn ctrl_w_closes_and_asks_only_when_needed() {
        let mut set = TabSet::new(app());
        set.add(app());
        assert!(!set.needs_close_confirmation());
        assert!(set.close(false));
    }
    #[test]
    fn ctrl_shift_t_reopens_last_closed() {
        let mut set = TabSet::new(app());
        set.add(app());
        set.close(false);
        assert!(set.reopen());
        assert_eq!(set.tabs.len(), 2);
    }
    #[test]
    fn tabs_keep_independent_drafts_and_queues() {
        let mut set = TabSet::new(app());
        set.active_mut().editor.set_text("one");
        set.add(app());
        assert!(set.active().editor.is_empty());
        set.next(true);
        assert_eq!(set.active().editor.text(), "one");
    }
    #[test]
    fn background_tab_turn_keeps_running() {
        let mut set = TabSet::new(app());
        set.add(app());
        set.tabs[0].busy = true;
        set.next(false);
        assert!(set.tabs[0].busy);
    }
    #[test]
    fn tab_strip_only_with_two_or_more_tabs() {
        let mut set = TabSet::new(app());
        assert!(!set.has_multiple());
        set.add(app());
        assert!(set.has_multiple());
    }
    #[test]
    fn keys_registered_without_conflicts() {
        assert_eq!(
            action_for(key('t', KeyModifiers::CONTROL | KeyModifiers::SHIFT)),
            TabAction::Reopen
        );
        assert_eq!(
            action_for(key('T', KeyModifiers::CONTROL | KeyModifiers::SHIFT)),
            TabAction::Reopen
        );
        assert_eq!(
            action_for(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL)),
            TabAction::Next
        );
        assert_eq!(
            action_for(KeyEvent::new(
                KeyCode::BackTab,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            TabAction::Previous
        );
        let mut release = key('t', KeyModifiers::CONTROL);
        release.kind = KeyEventKind::Release;
        assert_eq!(action_for(release), TabAction::Unhandled);
        assert_eq!(
            action_for(key('t', KeyModifiers::NONE)),
            TabAction::Unhandled
        );
    }
}
