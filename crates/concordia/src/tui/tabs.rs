//! Browser-style conversation tabs.
//!
//! The runner owns the provider channels; this module owns tab identity and lifecycle.  Keeping
//! whole `App` values here is intentional: drafts, queues, selections and native session ids are
//! consequently never shared between conversations.

use super::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TabId(u64);

pub(crate) struct ClosedTab {
    pub id: TabId,
    pub app: App,
}

pub(crate) struct TabSet {
    pub tabs: Vec<App>,
    pub active: usize,
    pub closed: Vec<ClosedTab>,
    next_id: u64,
    ids: Vec<TabId>,
}

impl TabSet {
    pub(crate) fn new(app: App) -> Self {
        Self {
            tabs: vec![app],
            active: 0,
            closed: Vec::new(),
            next_id: 1,
            ids: vec![TabId(0)],
        }
    }

    pub(crate) fn active(&self) -> &App {
        &self.tabs[self.active]
    }
    pub(crate) fn active_mut(&mut self) -> &mut App {
        &mut self.tabs[self.active]
    }
    pub(crate) fn ids(&self) -> &[TabId] {
        &self.ids
    }
    pub(crate) fn has_multiple(&self) -> bool {
        self.tabs.len() > 1
    }

    pub(crate) fn add(&mut self, app: App) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;
        self.tabs.push(app);
        self.ids.push(id);
        self.active = self.tabs.len() - 1;
        id
    }

    /// Returns whether the tab may close immediately.  The caller owns the confirmation UI.
    pub(crate) fn needs_close_confirmation(&self) -> bool {
        let app = self.active();
        app.is_busy() || app.has_unsent_work()
    }

    pub(crate) fn close(&mut self, confirmed: bool) -> bool {
        if self.tabs.len() == 1 || (!confirmed && self.needs_close_confirmation()) {
            return false;
        }
        let index = self.active;
        let app = self.tabs.remove(index);
        let id = self.ids.remove(index);
        self.closed.push(ClosedTab { id, app });
        self.active = index.min(self.tabs.len() - 1);
        true
    }

    pub(crate) fn reopen(&mut self) -> bool {
        let Some(closed) = self.closed.pop() else {
            return false;
        };
        self.tabs.push(closed.app);
        self.ids.push(closed.id);
        self.active = self.tabs.len() - 1;
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
        if !key.modifiers.contains(KeyModifiers::CONTROL) {
            return TabAction::Unhandled;
        }
        match key.code {
            KeyCode::Char('t') if !key.modifiers.contains(KeyModifiers::SHIFT) => TabAction::New,
            KeyCode::Char('w') => TabAction::Close,
            KeyCode::Char('t') => TabAction::Reopen,
            KeyCode::Tab | KeyCode::PageDown => {
                self.next(key.modifiers.contains(KeyModifiers::SHIFT));
                TabAction::Handled
            }
            KeyCode::PageUp => {
                self.next(true);
                TabAction::Handled
            }
            KeyCode::Char(c @ '1'..='9') if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.select_number(c as usize - '0' as usize);
                TabAction::Handled
            }
            _ => TabAction::Unhandled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabAction {
    Unhandled,
    Handled,
    New,
    Close,
    Reopen,
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
            TabSet::new(app()).handle_key(key('t', KeyModifiers::CONTROL | KeyModifiers::SHIFT)),
            TabAction::Reopen
        );
    }
}
