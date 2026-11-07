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
}

impl TabState for App {
    fn needs_close_confirmation(&self) -> bool {
        self.is_busy() || self.has_unsent_work()
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
        self.active = index.min(self.tabs.len() - 1);
        true
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

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> TabAction {
        let action = action_for(key);
        match action {
            TabAction::Next => self.next(false),
            TabAction::Previous => self.next(true),
            TabAction::Select(n) => self.select_number(n),
            _ => {}
        }
        action
    }
}

/// Tab keys, recognised before any other dispatch so pickers and the editor never see them.
pub(crate) fn action_for(key: KeyEvent) -> TabAction {
    if key.kind == KeyEventKind::Release || !key.modifiers.contains(KeyModifiers::CONTROL) {
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
    Close,
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
        let mut set = TabSet::new(app());
        assert_eq!(
            set.handle_key(key('t', KeyModifiers::CONTROL)),
            TabAction::New
        );
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
        assert!(set.tabs[0].is_busy());
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
