use std::{
    collections::VecDeque,
    ops::Deref,
    sync::{atomic::AtomicBool, mpsc::Receiver, Arc},
    thread,
    time::Duration,
};

use parking_lot::{Mutex, RwLock};
use petgraph::{
    acyclic::TopologicalPosition,
    graph::{DiGraph, NodeIndex},
    visit::EdgeRef,
};
use rustc_hash::FxHashMap as HashMap;

use crate::{
    declaration_meta::{Class, DeclarationMeta},
    expr::Expr,
    scheduler_parallel::Scheduler,
    scope::{env_create_scope, env_declare, env_empty, env_get_all_visible, merge_scope, Env},
    stat_manager::StatManager,
    stmt::{Decl, DeclType, Stmt},
    task_graph::{Status, Task, TaskGraph},
    worker_pool::{TaskResult, WorkerPool},
};

pub struct Executor {
    code_graph: DiGraph<DeclarationMeta, ()>,
    first_decls: Vec<NodeIndex>,
    // already_planned: HashSet<NodeIndex>,
    // last_planned: Option<(NodeIndex, Vec<NodeIndex>)>,
    state: Arc<RwLock<TaskGraph>>,
    // pool: WorkerPool,
    scheduler: Arc<RwLock<Scheduler>>,
    workers_num: usize,
    is_end: AtomicBool,
}

impl Executor {
    pub fn new(graph: DiGraph<DeclarationMeta, ()>, decls: &[Arc<Decl>]) -> Self {
        let num_workers = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        let state = TaskGraph::new();

        Executor {
            code_graph: graph,
            state: Arc::new(RwLock::new(state)),
            // pool:,
            first_decls: decls.iter().map(|x| NodeIndex::new(x.index)).collect(), // scheduler: Scheduler::new(num_workers),
            workers_num: num_workers,
            // last_planned: None,
            // already_planned: HashSet::default(),
            scheduler: Arc::new(RwLock::new(Scheduler::new(num_workers))),
            is_end: AtomicBool::new(false),
        }
    }

    fn build_available_part_task_graph(
        code_graph: &DiGraph<DeclarationMeta, ()>,
        state: &Arc<RwLock<TaskGraph>>,
        first_decls: &[NodeIndex],
        is_end: &AtomicBool,
    ) {
        // let mut in_degree: HashMap<usize, usize> = HashMap::default();

        // let mut blacklisted = HashSet::default();

        // for edge in self.code_graph.edge_references() {
        //     let target = edge.target();
        //     if target.index() <= edge.source().index()
        //         || self.code_graph[edge.source()].class == Class::Loop
        //     {
        //         blacklisted.insert(target.index());
        //     }
        // }

        // // Calculate in-degrees
        // for node in self.code_graph.node_indices().map(|n| n.index()) {
        //     if blacklisted.contains(&node) {
        //         continue;
        //     }
        //     in_degree.insert(node, 0);
        // }

        // for edge in self.code_graph.edge_references() {
        //     let target = edge.target();
        //     let source = edge.source();
        //     if blacklisted.contains(&target.index()) || blacklisted.contains(&source.index()) {
        //         continue;
        //     }
        //     *in_degree.entry(target.index()).or_insert(0) += 1;
        // }

        // // Priority queue based on complexity (higher complexity first)
        // let queue: VecDeque<(usize, usize)> = in_degree
        //     .iter()
        //     .filter(|(_, &deg)| deg == 0)
        //     .map(|(&node, _)| {
        //         let complexity = self.code_graph[NodeIndex::new(node)].complexity;
        //         (node, complexity)
        //     })
        //     .collect();

        let mut task_to_code = HashMap::default();
        let mut code_to_task = HashMap::default();

        // Sort by complexity (descending)
        // let mut sorted_queue: Vec<_> = queue.into_iter().collect();
        // sorted_queue.sort_by(|a, b| b.1.cmp(&a.1));
        // let mut queue: VecDeque<_> = sorted_queue.into_iter().collect();

        // let not_planned = self.first_decls.difference(&self.already_planned);

        for &node_idx in first_decls {
            (code_to_task, task_to_code, _) =
                Self::expand(code_graph, state, code_to_task, task_to_code, node_idx);

            // for edge in self.code_graph.edges(node_idx) {
            //     let target = edge.target().index();
            //     if blacklisted.contains(&target) {
            //         continue;
            //     }
            //     if let Some(deg) = in_degree.get_mut(&target) {
            //         // dbg!(*deg, target);
            //         *deg = deg.saturating_sub(1);
            //         if *deg == 0 {
            //             let complexity = self.code_graph[NodeIndex::new(target)].complexity;

            //             // Insert in sorted order
            //             let pos = queue
            //                 .iter()
            //                 .position(|(_, c)| *c < complexity)
            //                 .unwrap_or(queue.len());
            //             queue.insert(pos, (target, complexity));
            //         }
            //     }
            // }

            // in_degree.remove(&node);
        }
        is_end.store(true, std::sync::atomic::Ordering::SeqCst);

        // for i in result {

        // }
    }

    /// Витягує елементи з Expr::Array
    fn extract_array_elements(expr: &Expr) -> Result<Vec<Expr>, String> {
        match expr {
            Expr::Array(elements) => Ok(elements.clone()),
            _ => Err(format!("Expected array, got {:?}", expr)),
        }
    }

    fn expand(
        code_graph: &DiGraph<DeclarationMeta, ()>,
        state: &Arc<RwLock<TaskGraph>>,
        mut code_to_task: HashMap<NodeIndex, NodeIndex>,
        mut task_to_code: HashMap<NodeIndex, NodeIndex>,
        node_id: NodeIndex,
    ) -> (
        HashMap<NodeIndex, NodeIndex>,
        HashMap<NodeIndex, NodeIndex>,
        Option<usize>,
    ) {
        let meta = &code_graph[node_id];

        let mut to_skip = None;

        match &meta.decl.v {
            DeclType::Stmt(stmt) => match stmt.deref() {
                Stmt::For(loop_var, arr, _) if meta.mut_deps.len() == 0 => {
                    assert!(matches!(meta.class, Class::Loop));

                    // let mut loop_task = Task {
                    //     code_graph_id: node_id,
                    //     complexity: meta.complexity,
                    //     estimated_duration: None,
                    //     nested: vec![],
                    // };

                    // let inserted = state.write().add_task(loop_task);
                    // code_to_task.insert(node_id, inserted);
                    // task_to_code.insert(inserted, node_id);
                    // dbg!(node_id, &code_to_task, &task_to_code);

                    let needed = code_graph
                        .neighbors_directed(node_id, petgraph::Direction::Incoming)
                        .inspect(|x| {
                            // dbg!(x);
                        })
                        .map(|x| (x, code_to_task.get(&x).cloned().unwrap()))
                        .collect::<Vec<_>>();

                    //TODO: check smarter

                    // dbg!(&needed);
                    // dbg!(&code_to_task, &task_to_code);
                    // dbg!(&state.read().tasks_tree, &state.read().tasks_status);

                    let condvar = state.read().result_notifier.clone();
                    let dummy_mutex = Mutex::new(());

                    loop {
                        let all_ready = {
                            let state = state.read();
                            needed
                                .iter()
                                .all(|&(_code_id, task_id)| state.get_result(task_id).is_some())
                        };

                        if all_ready {
                            break;
                        }

                        // Чекаємо на сповіщення про нові результати
                        let mut guard = dummy_mutex.lock();
                        dbg!("waiting for loop dependencies");
                        let _ = condvar.wait_for(&mut guard, Duration::from_millis(10));
                    }

                    // dbg!(&state.read().tasks_results);

                    let env = needed
                        .into_iter()
                        .map(|(code_id, task_id)| {
                            (code_id, state.read().get_result(task_id).unwrap())
                        })
                        .reduce(|(code_id, env), (code_id2, env2)| {
                            (
                                NodeIndex::new(code_id.index().max(code_id2.index())),
                                merge_scope((code_id.index(), &env), (code_id2.index(), &env2)).1,
                            )
                        })
                        .unwrap_or((NodeIndex::default(), env_empty()))
                        .1;

                    let arr = arr.eval(&env).unwrap();

                    let Ok(elements) = Self::extract_array_elements(&arr) else {
                        //TODo: error handling
                        unreachable!();
                    };

                    // let mut estinated_time =
                    //     StatManager::predict(node_id.index(), &env_get_all_visible(&env));

                    // if estinated_time.is_none() {
                    //     let mut total_time = Duration::ZERO;
                    //     for el in &elements {
                    //         let mut env = env.clone();
                    //         env_declare(&mut env, loop_var.to_string(), el.clone());

                    //         let dur = StatManager::predict(
                    //             node_id.index() + 1,
                    //             &env_get_all_visible(&env),
                    //         );
                    //         if let Some(dur) = dur {
                    //             total_time += dur;
                    //         }
                    //     }

                    //     estinated_time = Some(total_time);
                    // }

                    // dbg!(estinated_time);

                    // if estinated_time.is_some()
                    //     && estinated_time.unwrap() < Duration::from_millis(100)
                    //     && estinated_time.unwrap() > Duration::ZERO
                    // {
                    //     dbg!("sequential loop", node_id);
                    //     // dbg!(node_id);
                    //     let task = Task {
                    //         code_graph_id: node_id,
                    //         complexity: meta.complexity,
                    //         estimated_duration: None,
                    //     };

                    //     let mut state = state.write();

                    //     let inserted = state.add_task(task);

                    //     let from =
                    //         code_graph.neighbors_directed(node_id, petgraph::Direction::Incoming);

                    //     for from in from {
                    //         if from >= node_id {
                    //             continue;
                    //         }

                    //         state.add_dependency(*code_to_task.get(&from).unwrap(), inserted);
                    //     }

                    //     task_to_code.insert(inserted, node_id);
                    //     code_to_task.insert(node_id, inserted);
                    // } else {
                    dbg!("par loop", node_id);
                    for el in elements.into_iter() {
                        let mut code_to_task = code_to_task.clone();
                        let mut task_to_code = task_to_code.clone();

                        let task = Task {
                            code_graph_id: node_id,
                            complexity: meta.complexity,
                            estimated_duration: None,
                            nested: vec![],
                        };

                        let inserted = {
                            let mut state = state.write();
                            let inserted = state.add_task(task);

                            let mut env = vec![HashMap::default(); meta.depth];
                            env[meta.depth - 1].insert(loop_var.clone(), el);

                            state.mark_done(inserted, env);

                            inserted
                        };

                        // if node_id.index() == 6 {
                        //     dbg!(inserted);
                        // }

                        code_to_task.insert(node_id, inserted);
                        task_to_code.insert(inserted, node_id);

                        (code_to_task, task_to_code, _) = Self::expand(
                            code_graph,
                            state,
                            code_to_task,
                            task_to_code,
                            NodeIndex::new(meta.loops_decls_indexes[0]),
                        );

                        to_skip = Some(meta.loops_decls_indexes.len());

                        // for id in meta.loops_decls_indexes.iter().map(|x| NodeIndex::new(*x)) {
                        //     dbg!(id);
                        //     (code_to_task, task_to_code) =
                        //         Self::expand(code_graph, state, code_to_task, task_to_code, id);
                        // }
                    }
                    // }
                }
                Stmt::Block(_) => {
                    assert!(matches!(meta.class, Class::Block));
                    // dbg!(node_id);

                    // let estimated_time = StatManager::predict(id, env);

                    // if estimated_time < Duration::from_millis(100) {
                    // } else {
                    let mut to_skip_nested = 0;

                    for i in &meta.loops_decls_indexes {
                        if to_skip_nested > 0 {
                            to_skip_nested -= 1;
                            continue;
                        }

                        let node_idx = NodeIndex::new(*i);

                        (code_to_task, task_to_code, to_skip) =
                            Self::expand(code_graph, state, code_to_task, task_to_code, node_idx);

                        if let Some(skip) = to_skip {
                            to_skip_nested += skip;
                        }
                    }
                    // }
                }
                _ => {
                    // dbg!(node_id);
                    let task = Task {
                        code_graph_id: node_id,
                        complexity: meta.complexity,
                        estimated_duration: None,
                        nested: vec![],
                    };

                    let mut state = state.write();

                    let inserted = state.add_task(task);

                    let from =
                        code_graph.neighbors_directed(node_id, petgraph::Direction::Incoming);

                    // dbg!(
                    //     &code_to_task,
                    //     &task_to_code,
                    //     // &state.tasks_tree.edges(node_id)
                    // );

                    for from in from {
                        if from >= node_id {
                            continue;
                        }
                        // dbg!(from);
                        state.add_dependency(*code_to_task.get(&from).unwrap(), inserted);
                    }

                    task_to_code.insert(inserted, node_id);
                    code_to_task.insert(node_id, inserted);

                    if meta.class == Class::Loop {
                        to_skip = Some(meta.loops_decls_indexes.len());
                    }
                }
            },
            _ => {
                // dbg!(node_id);
                let task = Task {
                    code_graph_id: node_id,
                    complexity: meta.complexity,
                    estimated_duration: None,
                    nested: vec![],
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
            }
        }

        return (code_to_task, task_to_code, to_skip);
    }

    /// Основний цикл виконання
    pub fn run(&mut self) -> Result<(), String> {
        let code_graph = &self.code_graph;
        let state = &self.state;
        let first_decls = &self.first_decls;
        let scheduler = &self.scheduler;

        let (pool, result_rx) = WorkerPool::new(self.workers_num);

        std::thread::scope(|s| {
            s.spawn(|| {
                Self::build_available_part_task_graph(code_graph, state, first_decls, &self.is_end);
            });

            s.spawn(|| {
                Self::collect_results(state, result_rx, scheduler).unwrap();
            });

            thread::sleep(Duration::from_millis(100));

            while !self.is_end.load(std::sync::atomic::Ordering::SeqCst) {
                let scheduled_tasks = {
                    let state = self.state.read();
                    self.scheduler.write().schedule(state.deref())
                };

                let mut min_duration = Duration::ZERO;

                // dbg!(&scheduled_tasks);

                for (worker_id, tasks) in scheduled_tasks.into_iter().enumerate() {
                    // dbg!("as");
                    for task_id in tasks {
                        let (complexity, estimated_duration) = {
                            let state = state.read();
                            let task = state.get_task(task_id).unwrap();
                            (task.complexity, task.estimated_duration)
                        };

                        if let Some(estimated_duration) = estimated_duration {
                            if estimated_duration < min_duration {
                                min_duration = estimated_duration;
                            }
                        }

                        let needed = state.read().get_dependencies(task_id);
                        // dbg!(&needed);
                        //TODO: check smarter

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

                        let worker_task = {
                            let state = state.read();
                            let code_node_id =
                                state.tasks_tree.node_weight(task_id).unwrap().code_graph_id;
                            let decl = &code_graph[code_node_id].decl;

                            crate::worker_pool::WorkerTask {
                                code_graph_id: code_node_id,
                                id: task_id,
                                work: crate::worker_pool::WorkType::ExecuteNode(decl.clone()),
                                env: env,
                                complexity: complexity as u64,
                                estimated_duration: estimated_duration,
                            }
                        };

                        state.write().mark_running(task_id);

                        pool.submit(worker_task, worker_id).unwrap();
                    }
                }

                // dbg!(state.read().is_complete());

                // // Обробка результатів виконання
                // while let Ok(result) = self.pool.result_rx.try_recv() {
                //     dbg!("hello");
                //     let mut state = state.write();

                //     match result.result {
                //         Ok(env) => {
                //             state.mark_done(result.id, env);
                //         }
                //         Err(err) => {
                //             state.mark_failed(result.id);
                //             return Err(err);
                //         }
                //     }

                //     self.scheduler
                //         .task_completed(result.worker_id, result.actual_duration, 1);
                // }

                thread::sleep(min_duration);
            }
        });

        pool.shutdown();

        Ok(())
    }

    fn collect_results(
        state: &Arc<RwLock<TaskGraph>>,
        result_rx: Receiver<TaskResult>,
        scheduler: &Arc<RwLock<Scheduler>>,
    ) -> Result<(), String> {
        while let Ok(result) = result_rx.recv() {
            // dbg!(result.id);
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
            scheduler
                .write()
                .task_completed(result.worker_id, result.actual_duration, 1);
        }
        Ok(())
    }

    // /// Запуск з автоматичним shutdown
    // pub fn execute(mut self) -> Result<(), String> {
    //     let result = self.run();
    //     self.pool.shutdown();
    //     result
    // }

    // Публічні методи для доступу до стану
    pub fn get_result(&self, node_id: NodeIndex) -> Option<Env> {
        let state = self.state.read();
        state.get_result(node_id)
    }

    // pub fn get_nested_results(&self, parent_id: NodeIndex) -> Vec<Env> {
    //     let state = self.state.lock();
    //     state.get_nested_results(parent_id)
    // }

    pub fn get_status(&self, node_id: NodeIndex) -> Option<Status> {
        let state = self.state.read();
        state.get_status(node_id)
    }

    // pub fn find_tasks_by_code_node(&self, code_node: NodeIndex) -> Vec<NodeIndex> {
    //     let state = self.state.lock();
    //     state.find_tasks_by_code_node(code_node)
    // }

    pub fn get_dependencies(&self, node_id: NodeIndex) -> Vec<NodeIndex> {
        let state = self.state.read();
        state.get_dependencies(node_id)
    }

    pub fn get_dependents(&self, node_id: NodeIndex) -> Vec<NodeIndex> {
        let state = self.state.read();
        state.get_dependents(node_id)
    }

    // pub fn all_subtasks_done(&self, parent_id: NodeIndex) -> bool {
    //     let state = self.state.lock();
    //     state.all_subtasks_done(parent_id)
    // }
}

pub fn execute_plan(
    graph: DiGraph<DeclarationMeta, ()>,
    decls: &[Arc<Decl>],
) -> Result<(), String> {
    Executor::new(graph, decls).run()
}
