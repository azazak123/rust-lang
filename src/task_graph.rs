use coarsetime::Duration;
use petgraph::{graph::NodeIndex, Direction, Graph};
use rustc_hash::FxHashMap as HashMap;

use crate::{declaration_meta::DeclarationMeta, scope::Env};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug)]
pub enum Task {
    ExecuteNode {
        code_graph_id: NodeIndex,
        complexity: usize,
        estimated_duration: Option<coarsetime::Duration>,
        loo: Option<(
            HashMap<NodeIndex, NodeIndex>,
            HashMap<NodeIndex, NodeIndex>,
            DeclarationMeta,
        )>,
    },
    MaybeExpand {
        code_graph_id: NodeIndex,
        complexity: usize,
        estimated_duration: Option<coarsetime::Duration>,
        loo: Option<(
            HashMap<NodeIndex, NodeIndex>,
            HashMap<NodeIndex, NodeIndex>,
            DeclarationMeta,
        )>,
    },
}

impl Task {
    pub fn get_estimated_duration(&self) -> Option<Duration> {
        match self {
            Task::ExecuteNode {
                estimated_duration, ..
            } => *estimated_duration,
            Task::MaybeExpand {
                estimated_duration, ..
            } => *estimated_duration,
        }
    }

    pub fn get_complexity(&self) -> usize {
        match self {
            Task::ExecuteNode { complexity, .. } => *complexity,
            Task::MaybeExpand { complexity, .. } => *complexity,
        }
    }

    pub fn get_code_graph_id(&self) -> NodeIndex {
        match self {
            Task::ExecuteNode { code_graph_id, .. } => *code_graph_id,
            Task::MaybeExpand { code_graph_id, .. } => *code_graph_id,
        }
    }
}

pub struct TaskGraph {
    pub tasks_status: HashMap<NodeIndex, Status>,
    pub tasks_results: HashMap<NodeIndex, Env>,
    pub tasks_tree: Graph<Task, ()>,
}

impl TaskGraph {
    pub fn new() -> Self {
        TaskGraph {
            tasks_status: HashMap::default(),
            tasks_results: HashMap::default(),
            tasks_tree: Graph::new(),
        }
    }

    pub fn is_node_ready(&self, node_id: NodeIndex) -> bool {
        if self.tasks_status.get(&node_id) != Some(&Status::Pending) {
            return false;
        }
        self.tasks_tree
            .neighbors_directed(node_id, Direction::Incoming)
            .all(|dep_id| self.tasks_status.get(&dep_id) == Some(&Status::Done))
    }

    pub fn is_complete(&self) -> bool {
        self.tasks_status
            .values()
            .all(|&s| s == Status::Done || s == Status::Failed)
    }

    pub fn add_task(&mut self, task: Task) -> NodeIndex {
        let node_id = self.tasks_tree.add_node(task);
        self.tasks_status.insert(node_id, Status::Pending);
        node_id
    }

    pub fn add_dependency(&mut self, from: NodeIndex, to: NodeIndex) {
        self.tasks_tree.add_edge(from, to, ());
    }

    pub fn mark_running(&mut self, node_id: NodeIndex) {
        self.tasks_status.insert(node_id, Status::Running);
    }

    pub fn mark_done(&mut self, node_id: NodeIndex, result: Env) {
        self.tasks_status.insert(node_id, Status::Done);
        self.tasks_results.insert(node_id, result);
    }

    pub fn mark_failed(&mut self, node_id: NodeIndex) {
        self.tasks_status.insert(node_id, Status::Failed);
    }

    pub fn get_result(&self, node_id: NodeIndex) -> Option<Env> {
        self.tasks_results.get(&node_id).cloned()
    }

    pub fn get_task(&self, node_id: NodeIndex) -> Option<&Task> {
        self.tasks_tree.node_weight(node_id)
    }

    pub fn get_ready_tasks(&self) -> Vec<(NodeIndex, Task)> {
        self.tasks_tree
            .node_indices()
            .filter(|&node_id| self.is_node_ready(node_id))
            .filter_map(|node_id| {
                let t = self
                    .tasks_tree
                    .node_weight(node_id)
                    .map(|task| (node_id, task.clone()));
                t
            })
            .collect()
    }

    pub fn get_dependencies(&self, node_id: NodeIndex) -> Vec<NodeIndex> {
        self.tasks_tree
            .neighbors_directed(node_id, Direction::Incoming)
            .collect()
    }
}
