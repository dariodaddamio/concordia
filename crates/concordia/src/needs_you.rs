//! Deterministic, local registry of sessions that need a user's attention.
//!
//! The registry deliberately contains no provider data or polling.  TUI and worker code publish
//! explicit transitions; consumers can render one queue regardless of which provider owns a run.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

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
    pub project: String,
    pub chat: String,
    pub agent: String,
    pub request: String,
    pub context: String,
    pub source: String,
    pub created: Instant,
}

static GLOBAL: OnceLock<Mutex<Queue>> = OnceLock::new();

fn global() -> &'static Mutex<Queue> {
    GLOBAL.get_or_init(|| Mutex::new(Queue::default()))
}

/// Publish a live request for panels belonging to any chat in this process.
pub fn publish(item: Item) {
    global().lock().unwrap_or_else(|e| e.into_inner()).add(item);
}

pub fn withdraw(id: &str) {
    global().lock().unwrap_or_else(|e| e.into_inner()).remove(id);
}

pub fn snapshot() -> Queue {
    global().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[derive(Debug, Default, Clone)]
pub struct Queue {
    items: BTreeMap<String, Item>,
}

impl Queue {
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

    pub fn remove_kind(&mut self, kind: ItemKind) {
        self.items.retain(|_, item| item.kind != kind);
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
            project: "project".into(),
            chat: session.into(),
            agent: "agent".into(),
            request: id.into(),
            context: String::new(),
            source: "conversation".into(),
            created: Instant::now(),
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
    fn attention_items_keep_their_session_owner_when_another_session_resolves() {
        let mut q = Queue::default();
        q.add(item("codex-q", "codex", ItemKind::PausedQueue));
        q.add(item("claude-q", "claude", ItemKind::Question));
        assert_eq!(q.item(0).unwrap().session_id, "claude");
        assert_eq!(q.item(1).unwrap().session_id, "codex");
        q.remove_kind(ItemKind::Question);
        assert_eq!(q.len(), 1);
        assert_eq!(q.item(0).unwrap().session_id, "codex");
    }
}
