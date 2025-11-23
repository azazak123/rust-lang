use std::{
    collections::BTreeMap,
    ops::Deref,
    sync::{mpsc::Receiver, Arc},
    thread,
};

use coarsetime::Duration;

use parking_lot::{Condvar, Mutex, RwLock};
use petgraph::graph::{DiGraph, NodeIndex};
use rustc_hash::FxHashMap as HashMap;

use crate::{
    declaration_meta::{Class, DeclarationMeta},
    expr::Expr,
    scheduler_parallel::Scheduler,
    scope::{env_empty, merge_scope},
    stat_manager::StatManager,
    stmt::{Decl, DeclType, Stmt},
    task_graph::{Task, TaskGraph},
    worker_pool::{TaskResult, WorkerPool},
    ARGS,
};

pub struct Executor {
    code_graph: DiGraph<DeclarationMeta, ()>,
    first_decls: Vec<NodeIndex>,
    state: Arc<RwLock<TaskGraph>>,
    scheduler: Arc<RwLock<Scheduler>>,
    workers_num: usize,
    wait: Arc<Condvar>,
}

impl Executor {
    pub fn new(graph: DiGraph<DeclarationMeta, ()>, decls: &[Arc<Decl>]) -> Self {
        let num_workers = ARGS
            .n_workers
            .unwrap_or_else(|| thread::available_parallelism().map_or(1, |n| n.get()));

        Executor {
            code_graph: graph,
            state: Arc::new(RwLock::new(TaskGraph::new())),
            first_decls: decls.iter().map(|x| NodeIndex::new(x.index)).collect(),
            workers_num: num_workers,
            scheduler: Arc::new(RwLock::new(Scheduler::new(num_workers))),
            wait: Arc::new(Condvar::new()),
        }
    }

    fn build_available_part_task_graph(&self) {
        let mut task_to_code = HashMap::default();
        let mut code_to_task = HashMap::default();

        for node_idx in &self.first_decls {
            (code_to_task, task_to_code, _) = Self::expand(
                &self.code_graph,
                &self.state,
                code_to_task,
                task_to_code,
                *node_idx,
                &None,
            );
        }
    }

    pub fn is_parallel_execution(duration: Option<Duration>, depth: usize) -> bool {
        duration.map_or(depth <= 1, |x| {
            x >= Duration::from_millis(ARGS.duration_to_par_ms)
        })
    }

    pub fn expand(
        code_graph: &DiGraph<DeclarationMeta, ()>,
        state: &Arc<RwLock<TaskGraph>>,
        mut code_to_task: HashMap<NodeIndex, NodeIndex>,
        mut task_to_code: HashMap<NodeIndex, NodeIndex>,
        node_id: NodeIndex,
        scope: &Option<BTreeMap<u64, Expr>>,
    ) -> (
        HashMap<NodeIndex, NodeIndex>,
        HashMap<NodeIndex, NodeIndex>,
        Option<usize>,
    ) {
        let meta = &code_graph[node_id];

        let mut to_skip = None;

        let duration = scope
            .as_ref()
            .and_then(|s| StatManager::predict(node_id.index(), s));

        let par_block = Self::is_parallel_execution(duration, meta.depth);

        match &meta.decl.v {
            DeclType::Stmt(stmt) => match stmt.deref() {
                Stmt::For(_, _, _) if meta.mut_deps.len() == 0 && par_block => {
                    assert!(matches!(meta.class, Class::Loop));

                    let loop_task = Task::MaybeExpand {
                        code_graph_id: node_id,
                        complexity: meta.complexity,
                        estimated_duration: Some(
                            duration.unwrap_or(Duration::from_millis(0 as u64)),
                        ),
                        loo: None,
                    };

                    let mut state = state.write();

                    let inserted = state.add_task(loop_task);

                    let from =
                        code_graph.neighbors_directed(node_id, petgraph::Direction::Incoming);

                    for from in from {
                        if from >= node_id {
                            continue;
                        }

                        state.add_dependency(*code_to_task.get(&from).unwrap(), inserted);
                    }

                    task_to_code.insert(inserted, node_id);
                    code_to_task.insert(node_id, inserted);

                    if let Some(Task::MaybeExpand { loo, .. }) =
                        state.tasks_tree.node_weight_mut(inserted)
                    {
                        *loo = Some((code_to_task.clone(), task_to_code.clone(), meta.clone()));
                    } else {
                        unreachable!();
                    };

                    to_skip = Some(meta.loops_decls_indexes.len());
                }
                Stmt::Block(_) if par_block => {
                    assert!(matches!(meta.class, Class::Block));
                    let mut to_skip_nested = 0;

                    for i in &meta.loops_decls_indexes {
                        if to_skip_nested > 0 {
                            to_skip_nested -= 1;
                            continue;
                        }

                        let node_idx = NodeIndex::new(*i);

                        (code_to_task, task_to_code, to_skip) = Self::expand(
                            code_graph,
                            state,
                            code_to_task,
                            task_to_code,
                            node_idx,
                            &None,
                        );

                        if let Some(skip) = to_skip {
                            to_skip_nested = skip;
                        }
                    }

                    to_skip = Some(meta.loops_decls_indexes.len());
                }
                _ => {
                    let task = Task::ExecuteNode {
                        code_graph_id: node_id,
                        complexity: meta.complexity,
                        estimated_duration: Some(
                            duration.unwrap_or(Duration::from_millis(meta.complexity as u64)),
                        ),
                        loo: None,
                    };

                    let mut state = state.write();

                    let inserted = state.add_task(task);

                    let from =
                        code_graph.neighbors_directed(node_id, petgraph::Direction::Incoming);

                    for from in from {
                        if from >= node_id {
                            continue;
                        }

                        state.add_dependency(*code_to_task.get(&from).unwrap(), inserted);
                    }

                    task_to_code.insert(inserted, node_id);
                    code_to_task.insert(node_id, inserted);

                    if meta.loops_decls_indexes.len() > 0 {
                        to_skip = Some(meta.loops_decls_indexes.len());
                    }

                    if let Some(Task::ExecuteNode { loo, .. }) =
                        state.tasks_tree.node_weight_mut(inserted)
                    {
                        *loo = Some((code_to_task.clone(), task_to_code.clone(), meta.clone()));
                    } else {
                        unreachable!();
                    };
                }
            },
            _ => {
                let task = Task::ExecuteNode {
                    code_graph_id: node_id,
                    complexity: meta.complexity,
                    estimated_duration: Some(
                        duration.unwrap_or(Duration::from_millis(meta.complexity as u64)),
                    ),
                    loo: None,
                };

                let mut state = state.write();

                let inserted = state.add_task(task);

                let from = code_graph.neighbors_directed(node_id, petgraph::Direction::Incoming);

                for from in from {
                    if from >= node_id {
                        continue;
                    }

                    state.add_dependency(*code_to_task.get(&from).unwrap(), inserted);
                }

                task_to_code.insert(inserted, node_id);
                code_to_task.insert(node_id, inserted);

                if let Some(Task::ExecuteNode { loo, .. }) =
                    state.tasks_tree.node_weight_mut(inserted)
                {
                    *loo = Some((code_to_task.clone(), task_to_code.clone(), meta.clone()));
                } else {
                    unreachable!();
                };
            }
        }

        return (code_to_task, task_to_code, to_skip);
    }

    pub fn run(&mut self) -> Result<(), String> {
        let code_graph = &self.code_graph;
        let state = &self.state;
        let scheduler = &self.scheduler;

        let (pool, result_rx) =
            WorkerPool::new(self.workers_num, &self.state, self.code_graph.clone());

        self.build_available_part_task_graph();

        std::thread::scope(|s| {
            s.spawn(|| {
                Self::collect_results(state, result_rx, scheduler, &self.wait).unwrap();
            });

            let dummy = Mutex::new(());

            while !state.read().is_complete() {
                let scheduled_tasks = {
                    let state = self.state.read();
                    self.scheduler.write().schedule(state.deref())
                };

                for (worker_id, tasks) in scheduled_tasks.into_iter().enumerate() {
                    for task_id in tasks {
                        let task = {
                            let state = state.read();
                            let task = state.get_task(task_id).unwrap().clone();
                            task
                        };

                        let needed = state.read().get_dependencies(task_id);

                        let env = needed
                            .into_iter()
                            .map(|task_id| (task_id, state.read().get_result(task_id).unwrap()))
                            .reduce(|(task_id, env), (task_id2, env2)| {
                                (
                                    NodeIndex::new(task_id.index().max(task_id2.index())),
                                    merge_scope((task_id.index(), &env), (task_id2.index(), &env2))
                                        .1,
                                )
                            })
                            .unwrap_or((NodeIndex::default(), env_empty()))
                            .1;

                        let worker_task = match task {
                            Task::MaybeExpand {
                                loo,
                                complexity,
                                estimated_duration,
                                ..
                            } => {
                                let (code_to_task, task_to_code, meta) = loo.unwrap();

                                let state = state.read();
                                let code_node_id = state
                                    .tasks_tree
                                    .node_weight(task_id)
                                    .unwrap()
                                    .get_code_graph_id();
                                let decl = &code_graph[code_node_id].decl;

                                crate::worker_pool::WorkerTask {
                                    code_graph_id: code_node_id,
                                    id: task_id,
                                    work: crate::worker_pool::WorkType::MaybeExpand {
                                        code_to_task,
                                        task_to_code,
                                        body: decl.clone(),
                                        meta,
                                    },
                                    env: env,
                                    complexity,
                                    estimated_duration,
                                }
                            }

                            Task::ExecuteNode {
                                loo,
                                complexity,
                                estimated_duration,
                                ..
                            } => {
                                let (code_to_task, task_to_code, meta) = loo.unwrap();

                                let state = state.read();
                                let code_node_id = state
                                    .tasks_tree
                                    .node_weight(task_id)
                                    .unwrap()
                                    .get_code_graph_id();
                                let decl = &code_graph[code_node_id].decl;

                                crate::worker_pool::WorkerTask {
                                    code_graph_id: code_node_id,
                                    id: task_id,
                                    work: crate::worker_pool::WorkType::ExecuteNode {
                                        code_to_task,
                                        task_to_code,
                                        body: decl.clone(),
                                        meta,
                                    },
                                    env: env,
                                    complexity,
                                    estimated_duration,
                                }
                            }
                        };
                        state.write().mark_running(task_id);

                        pool.submit(worker_task, worker_id).unwrap();
                    }
                }

                self.wait.wait(&mut dummy.lock());
            }

            pool.shutdown();
        });

        Ok(())
    }

    fn collect_results(
        state: &Arc<RwLock<TaskGraph>>,
        result_rx: Receiver<TaskResult>,
        scheduler: &Arc<RwLock<Scheduler>>,
        wait: &Arc<Condvar>,
    ) -> Result<(), String> {
        while let Ok(result) = result_rx.recv() {
            let mut state = state.write();
            match result.result {
                Ok(env) => {
                    state.mark_done(result.id, env); // Тепер це сповістить чекаючі потоки
                }
                Err(err) => {
                    state.mark_failed(result.id);
                    return Err(err);
                }
            }
            scheduler.write().task_completed(
                result.worker_id,
                result.actual_duration,
                result.complexity,
            );

            wait.notify_all();
        }
        Ok(())
    }
}

pub fn execute_plan(
    graph: DiGraph<DeclarationMeta, ()>,
    decls: &[Arc<Decl>],
) -> Result<(), String> {
    Executor::new(graph, decls).run()
}
