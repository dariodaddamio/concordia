//! Deterministic, local registry of sessions that need a user's attention.
//!
//! The registry deliberately contains no provider data or polling.  TUI and worker code publish
//! explicit transitions; consumers can render one queue regardless of which provider owns a run.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Running,
    Waiting,
    Idle,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
pub enum ItemKind {
    Approval,
    Question,
    PausedQueue,
    IdleNotice,
    WorkerResult,
    SignedOut,
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Approval => "permission",
            Self::Question => "question",
            Self::PausedQueue => "paused queue",
            Self::IdleNotice => "idle",
            Self::WorkerResult => "worker result",
            Self::SignedOut => "signed out",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub kind: ItemKind,
}

#[derive(Debug, Default, Clone)]
pub struct Queue {
    sessions: BTreeMap<String, SessionState>,
    items: BTreeMap<String, Item>,
}

impl Queue {
    pub fn set_state(&mut self, session_id: impl Into<String>, state: SessionState) {
        self.sessions.insert(session_id.into(), state);
    }

    pub fn state(&self, session_id: &str) -> Option<SessionState> {
        self.sessions.get(session_id).copied()
    }

    pub fn add(&mut self, item: Item) {
        self.items.insert(item.id.clone(), item);
    }

    pub fn remove(&mut self, id: &str) -> Option<Item> {
        self.items.remove(id)
    }

    pub fn remove_suffix(&mut self, suffix: &str) {
        let suffix = format!(":{suffix}");
        self.items.retain(|id, _| !id.ends_with(&suffix));
    }

    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.items.values()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn item(&self, index: usize) -> Option<&Item> {
        self.items.values().nth(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, session: &str, kind: ItemKind) -> Item {
        Item {
            id: id.into(),
            session_id: session.into(),
            title: id.into(),
            kind,
        }
    }

    #[test]
    fn needs_you_collects_each_item_kind() {
        let mut q = Queue::default();
        for (id, kind) in [
            ("a", ItemKind::Approval),
            ("q", ItemKind::Question),
            ("p", ItemKind::PausedQueue),
            ("i", ItemKind::IdleNotice),
            ("w", ItemKind::WorkerResult),
            ("s", ItemKind::SignedOut),
        ] {
            q.add(item(id, id, kind));
        }
        assert_eq!(q.len(), 6);
        assert_eq!(
            q.items().map(|i| i.kind).collect::<Vec<_>>(),
            vec![
                ItemKind::Approval,
                ItemKind::IdleNotice,
                ItemKind::PausedQueue,
                ItemKind::Question,
                ItemKind::SignedOut,
                ItemKind::WorkerResult,
            ]
        );
    }

    #[test]
    fn resolved_items_disappear() {
        let mut q = Queue::default();
        q.add(item("question", "chat", ItemKind::Question));
        assert!(q.remove("question").is_some());
        assert!(q.is_empty());
    }

    #[test]
    fn per_session_drafts_and_queues_are_independent() {
        let mut q = Queue::default();
        q.set_state("codex", SessionState::Running);
        q.set_state("claude", SessionState::Waiting);
        q.add(item("codex-q", "codex", ItemKind::PausedQueue));
        q.add(item("claude-q", "claude", ItemKind::Question));
        assert_eq!(q.state("codex"), Some(SessionState::Running));
        assert_eq!(q.state("claude"), Some(SessionState::Waiting));
        assert_eq!(q.item(0).unwrap().session_id, "claude");
        assert_eq!(q.item(1).unwrap().session_id, "codex");
    }
}
