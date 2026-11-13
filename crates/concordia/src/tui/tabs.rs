//! Browser-style conversation tabs.
//!
//! The runner owns the provider channels; this module owns tab identity and lifecycle.  Keeping
//! whole `App` values here is intentional: drafts, queues, selections and native session ids are
//! consequently never shared between conversations.

use super::App;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;

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

    /// Move a tab to `to` (an index in the final order). The active tab stays active.
    pub(crate) fn move_tab(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return false;
        }
        let active = self.active;
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = if active == from {
            to
        } else if from < active && active <= to {
            active - 1
        } else if to <= active && active < from {
            active + 1
        } else {
            active
        };
        true
    }

    /// Ctrl+Shift+PgUp/PgDn: move the active tab one place, without wrapping.
    pub(crate) fn move_active(&mut self, right: bool) -> bool {
        let to = if right {
            self.active + 1
        } else {
            match self.active.checked_sub(1) {
                Some(to) => to,
                None => return false,
            }
        };
        self.move_tab(self.active, to)
    }
}

/// Pointer drag that reorders items laid out in one row, such as the tab strip.
///
/// It knows only the row's rectangles, so a panel strip can reuse it; docking would add
/// further drop targets beside `slot`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DragReorder {
    pub source: usize,
    origin: u16,
    pointer: u16,
    moved: bool,
}

impl DragReorder {
    pub(crate) fn press(source: usize, x: u16) -> Self {
        Self {
            source,
            origin: x,
            pointer: x,
            moved: false,
        }
    }

    /// Track the pointer; a drag starts once it leaves the pressed column.
    pub(crate) fn hover(&mut self, x: u16) {
        self.pointer = x;
        self.moved |= x.abs_diff(self.origin) >= 2;
    }

    pub(crate) fn dragging(&self) -> bool {
        self.moved
    }

    /// The insertion slot under the pointer, `0..=items.len()`: before the item whose centre
    /// is right of the pointer.
    pub(crate) fn slot(&self, items: &[Rect]) -> usize {
        items
            .iter()
            .position(|rect| self.pointer < rect.x + rect.width / 2)
            .unwrap_or(items.len())
    }

    /// Where the dragged item lands, as an index in the final order, or `None` for no change.
    pub(crate) fn target(&self, items: &[Rect]) -> Option<usize> {
        if !self.moved || self.source >= items.len() {
            return None;
        }
        let slot = self.slot(items);
        let to = if slot > self.source { slot - 1 } else { slot };
        (to != self.source).then_some(to)
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
        KeyCode::PageUp if shift => TabAction::MoveLeft,
        KeyCode::PageDown if shift => TabAction::MoveRight,
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
    MoveLeft,
    MoveRight,
    /// A finished drag: (from, to) as indexes in the final order.
    Move(usize, usize),
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

    #[test]
    fn moving_tabs_keeps_the_active_tab_selected() {
        let mut set = TabSet::new(app());
        set.add(app());
        set.add(app());
        set.tabs[0].editor.set_text("first");
        set.active = 0;
        assert!(set.move_active(true));
        assert_eq!((set.active, set.tabs[1].editor.text()), (1, "first".into()));
        assert!(set.move_tab(2, 0));
        assert_eq!((set.active, set.tabs[2].editor.text()), (2, "first".into()));
        assert!(!set.move_active(true), "the last tab does not wrap");
        set.active = 0;
        assert!(!set.move_active(false), "the first tab does not wrap");
        assert_eq!(
            action_for(KeyEvent::new(
                KeyCode::PageUp,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            TabAction::MoveLeft
        );
        assert_eq!(
            action_for(KeyEvent::new(
                KeyCode::PageDown,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            TabAction::MoveRight
        );
        assert_eq!(
            action_for(KeyEvent::new(KeyCode::PageDown, KeyModifiers::CONTROL)),
            TabAction::Next
        );
    }

    #[test]
    fn dragging_a_tab_reports_the_insertion_slot_and_final_index() {
        let strip = [
            Rect::new(0, 0, 10, 1),
            Rect::new(11, 0, 10, 1),
            Rect::new(22, 0, 10, 1),
        ];
        let mut drag = DragReorder::press(0, 3);
        drag.hover(4);
        assert!(!drag.dragging(), "a one-column wobble is still a click");
        assert_eq!(drag.target(&strip), None);
        drag.hover(28);
        assert_eq!(drag.slot(&strip), 3);
        assert_eq!(drag.target(&strip), Some(2));
        drag.hover(14);
        assert_eq!(drag.target(&strip), None, "slot 1 is its own place");
        let mut back = DragReorder::press(2, 25);
        back.hover(1);
        assert_eq!((back.slot(&strip), back.target(&strip)), (0, Some(0)));
    }
}
