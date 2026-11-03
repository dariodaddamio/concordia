//! Bounded dependency metadata. Readiness is descriptive and never launches a provider.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_TASKS: usize = 64;
pub const MAX_EDGES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prerequisite {
    pub task_id: String,
    pub run_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Uncertain,
    PausedRecovery,
}

impl RunState {
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Uncertain
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    task_id: String,
    conversation_id: String,
    run_id: String,
    prerequisites: Vec<Prerequisite>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delivery: Option<super::delivery::Receipt>,
    state: RunState,
}

impl Task {
    pub fn new(
        task_id: String,
        conversation_id: String,
        run_id: String,
        prerequisites: Vec<Prerequisite>,
    ) -> Result<Self, String> {
        let task = Self {
            task_id,
            conversation_id,
            run_id,
            prerequisites,
            delivery: None,
            state: RunState::Pending,
        };
        task.validate_identity()?;
        Ok(task)
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn conversation_id(&self) -> &str {
        &self.conversation_id
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn prerequisites(&self) -> &[Prerequisite] {
        &self.prerequisites
    }

    pub fn state(&self) -> RunState {
        self.state
    }

    pub fn delivery(&self) -> Option<&super::delivery::Receipt> {
        self.delivery.as_ref()
    }

    fn validate_identity(&self) -> Result<(), String> {
        for id in [&self.task_id, &self.conversation_id, &self.run_id] {
            validate_id(id)?;
        }
        if self.prerequisites.len() > MAX_EDGES {
            return Err("dependency graph exceeds 256 edges".into());
        }
        if let Some(receipt) = &self.delivery {
            receipt.validate()?;
            if receipt.request.child != self.run_id
                || receipt.request.conversation != self.conversation_id
                || receipt.request.parents.iter().any(|parent| {
                    !self
                        .prerequisites
                        .iter()
                        .any(|prerequisite| prerequisite.run_id == parent.run)
                })
            {
                return Err(
                    "delivery receipt does not match the exact dependency task/edges".into(),
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockReason {
    Failed,
    Cancelled,
    Uncertain,
    PausedRecovery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Readiness {
    Ready,
    IntegrationRequired {
        reason: String,
    },
    WaitingDependencies {
        prerequisites: Vec<Prerequisite>,
    },
    BlockedDependency {
        prerequisite: Prerequisite,
        reason: BlockReason,
    },
    PausedRecovery,
    Running,
    Finished {
        state: RunState,
    },
}

/// Tasks retain submission order. Terminal prerequisite receipts survive normal viewer pruning.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    tasks: Vec<Task>,
}

impl Graph {
    pub fn new(tasks: Vec<Task>) -> Result<Self, String> {
        let graph = Self { tasks };
        graph.validate()?;
        Ok(graph)
    }

    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.tasks.len() > MAX_TASKS {
            return Err("dependency graph exceeds 64 tasks".into());
        }
        let mut indices = BTreeMap::new();
        let mut runs = BTreeSet::new();
        let mut edges = 0;
        for (index, task) in self.tasks.iter().enumerate() {
            task.validate_identity()?;
            if indices.insert(task.task_id.as_str(), index).is_some()
                || !runs.insert(task.run_id.as_str())
            {
                return Err("duplicate dependency task or run identity".into());
            }
            edges += task.prerequisites.len();
            if edges > MAX_EDGES {
                return Err("dependency graph exceeds 256 edges".into());
            }
        }
        for task in &self.tasks {
            let mut seen = BTreeSet::new();
            for prerequisite in &task.prerequisites {
                validate_id(&prerequisite.task_id)?;
                validate_id(&prerequisite.run_id)?;
                if prerequisite.task_id == task.task_id {
                    return Err("a task cannot depend on itself".into());
                }
                if !seen.insert(prerequisite.task_id.as_str()) {
                    return Err("duplicate prerequisite task".into());
                }
                let parent = indices
                    .get(prerequisite.task_id.as_str())
                    .map(|index| &self.tasks[*index])
                    .ok_or("unknown prerequisite task")?;
                if parent.run_id != prerequisite.run_id {
                    return Err("prerequisite run identity mismatch".into());
                }
                if parent.conversation_id != task.conversation_id {
                    return Err("dependency belongs to another conversation".into());
                }
            }
        }
        let mut marks = vec![0; self.tasks.len()];
        for index in 0..self.tasks.len() {
            self.visit(index, &indices, &mut marks)?;
        }
        Ok(())
    }

    fn visit(
        &self,
        index: usize,
        indices: &BTreeMap<&str, usize>,
        marks: &mut [u8],
    ) -> Result<(), String> {
        match marks[index] {
            1 => return Err("dependency graph contains a cycle".into()),
            2 => return Ok(()),
            _ => {}
        }
        marks[index] = 1;
        for prerequisite in &self.tasks[index].prerequisites {
            self.visit(indices[prerequisite.task_id.as_str()], indices, marks)?;
        }
        marks[index] = 2;
        Ok(())
    }

    pub fn insert(&mut self, task: Task) -> Result<(), String> {
        if task.state != RunState::Pending {
            return Err("new dependency tasks must be pending".into());
        }
        let mut candidate = self.clone();
        candidate.tasks.push(task);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub fn set_prerequisites(
        &mut self,
        task_id: &str,
        prerequisites: Vec<Prerequisite>,
    ) -> Result<(), String> {
        let mut candidate = self.clone();
        let task = candidate
            .tasks
            .iter_mut()
            .find(|task| task.task_id == task_id)
            .ok_or("unknown dependency task")?;
        if !matches!(task.state, RunState::Pending | RunState::PausedRecovery) {
            return Err("dependencies of a running or finished task cannot change".into());
        }
        task.prerequisites = prerequisites;
        task.delivery = None;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub(super) fn accept_delivery(
        &mut self,
        task_id: &str,
        receipt: super::delivery::Receipt,
    ) -> Result<(), String> {
        let mut candidate = self.clone();
        let task = candidate
            .tasks
            .iter_mut()
            .find(|task| task.task_id == task_id)
            .ok_or("unknown dependency task")?;
        if task.state != RunState::Pending {
            return Err("only pending dependency tasks accept delivery review".into());
        }
        task.delivery = Some(receipt);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    /// This says only whether prerequisites succeeded, never whether dispatch is authorized.
    pub fn readiness(&self, task_id: &str) -> Result<Readiness, String> {
        self.validate()?;
        let task = self
            .tasks
            .iter()
            .find(|task| task.task_id == task_id)
            .ok_or("unknown dependency task")?;
        match task.state {
            RunState::PausedRecovery => return Ok(Readiness::PausedRecovery),
            RunState::Running => return Ok(Readiness::Running),
            state if state.terminal() => return Ok(Readiness::Finished { state }),
            _ => {}
        }
        let mut waiting = Vec::new();
        for prerequisite in &task.prerequisites {
            // Validation binds both the exact task and run; a new run never inherits success.
            let parent = self
                .tasks
                .iter()
                .find(|parent| parent.task_id == prerequisite.task_id)
                .ok_or("unknown prerequisite task")?;
            let reason = match parent.state {
                RunState::Succeeded => continue,
                RunState::Pending | RunState::Running => {
                    waiting.push(prerequisite.clone());
                    continue;
                }
                RunState::Failed => BlockReason::Failed,
                RunState::Cancelled => BlockReason::Cancelled,
                RunState::Uncertain => BlockReason::Uncertain,
                RunState::PausedRecovery => BlockReason::PausedRecovery,
            };
            return Ok(Readiness::BlockedDependency {
                prerequisite: prerequisite.clone(),
                reason,
            });
        }
        if waiting.is_empty() {
            Ok(Readiness::Ready)
        } else {
            Ok(Readiness::WaitingDependencies {
                prerequisites: waiting,
            })
        }
    }

    pub(super) fn observe_run(
        &mut self,
        conversation_id: &str,
        run_id: &str,
        state: RunState,
    ) -> Result<(), String> {
        let mut candidate = self.clone();
        if let Some(task) = candidate
            .tasks
            .iter_mut()
            .find(|task| task.run_id == run_id)
        {
            if task.conversation_id != conversation_id {
                return Err("dependency run belongs to another conversation".into());
            }
            task.state = state;
        } else {
            let mut task = Task::new(
                run_id.into(),
                conversation_id.into(),
                run_id.into(),
                Vec::new(),
            )?;
            task.state = state;
            candidate.tasks.push(task);
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub(super) fn matches_run(&self, conversation_id: &str, run_id: &str, state: RunState) -> bool {
        self.tasks.iter().any(|task| {
            task.conversation_id == conversation_id && task.run_id == run_id && task.state == state
        })
    }

    pub(super) fn pause_interrupted(&mut self) {
        for task in &mut self.tasks {
            if matches!(task.state, RunState::Pending | RunState::Running) {
                task.state = RunState::PausedRecovery;
            }
        }
    }

    pub(super) fn retain_receipts(&mut self, visible_runs: &BTreeSet<&str>) {
        let mut keep: BTreeSet<String> = self
            .tasks
            .iter()
            .filter(|task| visible_runs.contains(task.run_id.as_str()) || !task.state.terminal())
            .map(|task| task.task_id.clone())
            .collect();
        loop {
            let before = keep.len();
            for task in &self.tasks {
                if keep.contains(&task.task_id) {
                    keep.extend(task.prerequisites.iter().map(|p| p.task_id.clone()));
                }
            }
            if keep.len() == before {
                break;
            }
        }
        self.tasks.retain(|task| keep.contains(&task.task_id));
    }
}

fn validate_id(id: &str) -> Result<(), String> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| "invalid dependency UUID")?;
    if parsed.to_string() != id {
        return Err("dependency UUID must use canonical lowercase hyphenated form".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> String {
        uuid::Uuid::from_u128(n).to_string()
    }

    fn task(n: u128, parents: &[u128]) -> Task {
        Task::new(
            id(n),
            id(1000),
            id(n + 100),
            parents
                .iter()
                .map(|p| Prerequisite {
                    task_id: id(*p),
                    run_id: id(*p + 100),
                })
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn rejected_mutations_preserve_graph_and_receipts_bind_exact_runs() {
        let mut graph = Graph::new(vec![task(1, &[]), task(2, &[1])]).unwrap();
        let before = graph.clone();
        assert!(graph
            .set_prerequisites(
                &id(1),
                vec![Prerequisite {
                    task_id: id(2),
                    run_id: id(102)
                }]
            )
            .is_err());
        assert_eq!(graph, before);
        assert!(graph.insert(task(3, &[99])).is_err());
        assert!(graph.insert(task(2, &[])).is_err());
        assert!(graph.insert(task(3, &[3])).is_err());
        let mut duplicate_run = task(3, &[]);
        duplicate_run.run_id = id(101);
        assert!(graph.insert(duplicate_run).is_err());
        assert!(graph
            .set_prerequisites(
                &id(2),
                vec![Prerequisite {
                    task_id: id(1),
                    run_id: id(777)
                }]
            )
            .is_err());
        assert!(graph
            .set_prerequisites(
                &id(2),
                vec![
                    Prerequisite {
                        task_id: id(1),
                        run_id: id(101)
                    };
                    2
                ]
            )
            .is_err());
        assert_eq!(graph, before);
        let mut foreign = task(3, &[1]);
        foreign.conversation_id = id(2000);
        assert!(graph.insert(foreign).is_err());
        assert_eq!(graph, before);
        assert!(matches!(
            graph.readiness(&id(2)).unwrap(),
            Readiness::WaitingDependencies { .. }
        ));
        for (state, reason) in [
            (RunState::PausedRecovery, BlockReason::PausedRecovery),
            (RunState::Failed, BlockReason::Failed),
            (RunState::Cancelled, BlockReason::Cancelled),
            (RunState::Uncertain, BlockReason::Uncertain),
        ] {
            graph.observe_run(&id(1000), &id(101), state).unwrap();
            assert!(
                matches!(graph.readiness(&id(2)).unwrap(), Readiness::BlockedDependency { reason: actual, .. } if actual == reason)
            );
        }
        graph
            .observe_run(&id(1000), &id(101), RunState::Succeeded)
            .unwrap();
        assert_eq!(graph.readiness(&id(2)).unwrap(), Readiness::Ready);
        // A new execution is a separate receipt and cannot satisfy an older edge.
        graph
            .observe_run(&id(1000), &id(101), RunState::Failed)
            .unwrap();
        graph
            .observe_run(&id(1000), &id(999), RunState::Succeeded)
            .unwrap();
        assert!(matches!(
            graph.readiness(&id(2)).unwrap(),
            Readiness::BlockedDependency {
                reason: BlockReason::Failed,
                ..
            }
        ));
        graph.retain_receipts(&BTreeSet::new());
        assert!(graph.tasks().iter().any(|task| task.task_id == id(1)));
        assert!(!graph.tasks().iter().any(|task| task.run_id == id(999)));
    }

    #[test]
    fn graph_enforces_node_edge_identity_and_recovery_limits() {
        let mut graph = Graph::new((1..=64).map(|n| task(n, &[])).collect()).unwrap();
        let before = graph.clone();
        assert!(graph
            .observe_run(&id(1000), &id(999), RunState::Running)
            .is_err());
        assert_eq!(graph, before);
        assert!(Task::new("invalid".into(), id(1000), id(90), Vec::new()).is_err());
        for n in 2..=23 {
            let parents: Vec<_> = (1..n).collect();
            graph
                .set_prerequisites(
                    &id(n),
                    parents
                        .iter()
                        .map(|p| Prerequisite {
                            task_id: id(*p),
                            run_id: id(*p + 100),
                        })
                        .collect(),
                )
                .unwrap();
        }
        let before = graph.clone();
        assert!(graph
            .set_prerequisites(
                &id(24),
                (1..24)
                    .map(|p| Prerequisite {
                        task_id: id(p),
                        run_id: id(p + 100)
                    })
                    .collect()
            )
            .is_err());
        assert_eq!(graph, before);
        graph.pause_interrupted();
        assert!(graph
            .tasks()
            .iter()
            .all(|task| task.state == RunState::PausedRecovery));
        assert_eq!(graph.readiness(&id(1)).unwrap(), Readiness::PausedRecovery);
    }
}
