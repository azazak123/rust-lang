use std::{sync::Arc, time::Duration};

use parking_lot::Condvar;
use petgraph::{graph::NodeIndex, Direction, Graph};
use rustc_hash::FxHashMap as HashMap;

use crate::{expr::Expr, scope::Env};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug)]
pub struct Task {
    pub(crate) code_graph_id: NodeIndex,
    // pub(crate) id: TaskId,
    // work: WorkType,
    pub(crate) complexity: usize,
    pub(crate) estimated_duration: Option<Duration>,
    pub nested: Vec<NodeIndex>,
}

pub struct TaskGraph {
    pub tasks_status: HashMap<NodeIndex, Status>,
    pub tasks_results: HashMap<NodeIndex, Env>,
    pub tasks_tree: Graph<Task, ()>,
    pub result_notifier: Arc<Condvar>,
    // tasks_nested: HashMap<NodeIndex, Vec<NodeIndex>>,
}

impl TaskGraph {
    pub fn new() -> Self {
        TaskGraph {
            tasks_status: HashMap::default(),
            tasks_results: HashMap::default(),
            tasks_tree: Graph::new(),
            result_notifier: Arc::new(Condvar::new()),
            // tasks_nested: HashMap::default(),
        }
    }

    pub fn is_node_ready(&self, node_id: NodeIndex) -> bool {
        // dbg!(node_id);
        if self.tasks_status.get(&node_id) != Some(&Status::Pending) {
            return false;
        }
        self.tasks_tree
            .neighbors_directed(node_id, Direction::Incoming)
            .all(|dep_id| self.tasks_status.get(&dep_id) == Some(&Status::Done))
    }

    // pub fn all_subtasks_done(&self, parent_id: NodeIndex) -> bool {
    //     let Some(nested_tasks) = self.tasks_nested.get(&parent_id) else {
    //         return true;
    //     };
    //     nested_tasks
    //         .iter()
    //         .all(|node_index| self.tasks_status.get(node_index) == Some(&Status::Done))
    // }

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

    // pub fn add_nested_task(&mut self, parent_id: NodeIndex, child_id: NodeIndex) {
    //     self.tasks_nested
    //         .entry(parent_id)
    //         .or_insert_with(Vec::new)
    //         .push(child_id);
    // }

    pub fn mark_running(&mut self, node_id: NodeIndex) {
        self.tasks_status.insert(node_id, Status::Running);
    }

    pub fn mark_done(&mut self, node_id: NodeIndex, result: Env) {
        self.tasks_status.insert(node_id, Status::Done);
        self.tasks_results.insert(node_id, result);
        self.result_notifier.notify_all();
    }

    pub fn mark_failed(&mut self, node_id: NodeIndex) {
        self.tasks_status.insert(node_id, Status::Failed);
    }

    pub fn get_result(&self, node_id: NodeIndex) -> Option<Env> {
        self.tasks_results.get(&node_id).cloned()
    }

    // pub fn get_nested_results(&self, parent_id: NodeIndex) -> Vec<Env> {
    //     if let Some(nested_tasks) = self.tasks_nested.get(&parent_id) {
    //         nested_tasks
    //             .iter()
    //             .filter_map(|&task_id| self.tasks_results.get(&task_id).cloned())
    //             .collect()
    //     } else {
    //         Vec::new()
    //     }
    // }

    pub fn get_status(&self, node_id: NodeIndex) -> Option<Status> {
        self.tasks_status.get(&node_id).copied()
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

    // pub fn find_tasks_by_code_node(&self, code_node: NodeIndex) -> Vec<NodeIndex> {
    //     self.tasks_tree
    //         .node_indices()
    //         .filter(|&task_node| {
    //             if let Some(task) = self.tasks_tree.node_weight(task_node) {
    //                 task.code_graph_id == code_node
    //             } else {
    //                 false
    //             }
    //         })
    //         .collect()
    // }

    pub fn get_dependencies(&self, node_id: NodeIndex) -> Vec<NodeIndex> {
        self.tasks_tree
            .neighbors_directed(node_id, Direction::Incoming)
            .collect()
    }

    pub fn get_dependents(&self, node_id: NodeIndex) -> Vec<NodeIndex> {
        self.tasks_tree
            .neighbors_directed(node_id, Direction::Outgoing)
            .collect()
    }

    // pub fn build_task_id_map(&self) -> HashMap<TaskId, NodeIndex> {
    //     let mut map = HashMap::default();

    //     for node_id in self.tasks_tree.node_indices() {
    //         if let Some(task) = self.tasks_tree.node_weight(node_id) {
    //             map.insert(task.id.clone(), node_id);
    //         }
    //     }

    //     map
    // }
}
