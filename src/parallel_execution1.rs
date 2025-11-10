use log::{debug, error, info};
use parking_lot::Mutex;
use petgraph::{
    graph::{DiGraph, NodeIndex},
    visit::EdgeRef,
    Direction,
};
use rustc_hash::{FxBuildHasher, FxHashMap as HashMap};
use std::{
    collections::HashSet,
    ops::Deref,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use crate::scope::{env_add_scope, env_create_scope, env_remove_scope};
use crate::stat_manager::StatManager;
use crate::stmt::{DeclType, Stmt};
use crate::{declaration_meta::DeclarationMeta, scope::Env, stmt::Decl};
use crate::{execution::execute_stmt, scope::create_env};
use crate::{expr::Expr, scope::env_get_all_visible};

// ====================================================================
//                       КОНФІГУРАЦІЯ
// ====================================================================

const MIN_PARALLEL_SIZE: usize = 10;
const COMPLEXITY_MULTIPLIER: u64 = 100;

// Максимальна глибина паралелізації (None = необмежено)
const MAX_PARALLEL_DEPTH: Option<usize> = None;

// ====================================================================
//                       БАЗОВІ СТРУКТУРИ
// ====================================================================

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum TaskId {
    Node(usize),
    SubTask(usize, usize), // (parent_id, iter_idx)
}

struct Task {
    id: TaskId,
    work: WorkType,
    env: Env,
    complexity: u64,
    estimated_duration: Option<Duration>,
}

enum WorkType {
    ExecuteNode(Arc<Decl>),
    ExecuteIteration {
        var: String,
        element: Expr,
        body: Arc<Decl>,
    },
}

struct TaskResult {
    id: TaskId,
    result: Result<Env, String>,
    actual_duration: Duration,
}

// ====================================================================
//                       WORKER POOL
// ====================================================================

struct WorkerPool {
    workers: Vec<Sender<Option<Task>>>,
    result_rx: Receiver<TaskResult>,
    active_tasks: Arc<AtomicUsize>,
    num_workers: usize,
}

impl WorkerPool {
    fn new(num_workers: usize) -> Self {
        let (result_tx, result_rx) = channel();
        let active_tasks = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();

        for worker_id in 0..num_workers {
            let (task_tx, task_rx) = channel::<Option<Task>>();
            let result_tx = result_tx.clone();
            let active = active_tasks.clone();

            thread::spawn(move || {
                debug!("Worker {} started", worker_id);
                while let Ok(Some(task)) = task_rx.recv() {
                    let start = Instant::now();
                    let result = Self::execute_task(task.work, task.env);
                    let duration = start.elapsed();

                    let _ = result_tx.send(TaskResult {
                        id: task.id,
                        result,
                        actual_duration: duration,
                    });

                    active.fetch_sub(1, Ordering::Release);
                }
                debug!("Worker {} stopped", worker_id);
            });

            workers.push(task_tx);
        }

        WorkerPool {
            workers,
            result_rx,
            active_tasks,
            num_workers,
        }
    }

    fn execute_task(work: WorkType, mut env: Env) -> Result<Env, String> {
        match work {
            WorkType::ExecuteNode(decl) => {
                execute_stmt(&decl, &mut env)?;
                Ok(env)
            }
            WorkType::ExecuteIteration { var, element, body } => {
                let mut scope = env_create_scope();
                scope.insert(var, element);
                env_add_scope(&mut env, scope);
                execute_stmt(&body, &mut env)?;
                env_remove_scope(&mut env);
                Ok(env)
            }
        }
    }

    fn submit(&self, task: Task, worker_id: usize) -> Result<(), String> {
        self.active_tasks.fetch_add(1, Ordering::Release);

        self.workers[worker_id].send(Some(task)).map_err(|e| {
            self.active_tasks.fetch_sub(1, Ordering::Release);
            format!("Failed to submit task: {}", e)
        })
    }

    fn has_active_tasks(&self) -> bool {
        self.active_tasks.load(Ordering::Acquire) > 0
    }

    fn shutdown(self) {
        for worker in self.workers {
            let _ = worker.send(None);
        }
    }
}

// ====================================================================
//                       EXECUTION STATE
// ====================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Pending,
    Running,
    Done,
    Failed,
}

struct SubTaskInfo {
    var: String,
    element: Expr,
    body: Arc<Decl>,
    body_idx: usize,
    env: Env,
    complexity: u64,
    depth: usize,
    estimated_duration: Option<Duration>, // ← ДОДАТИ
}

struct ExecutionState {
    node_status: HashMap<usize, Status>,
    node_results: HashMap<usize, Env>,

    subtask_status: HashMap<(usize, usize), Status>,
    subtask_results: HashMap<(usize, usize), Env>,
    subtask_info: HashMap<(usize, usize), SubTaskInfo>,
    subtask_total: HashMap<usize, usize>,

    ignored: HashSet<usize>,
}

impl ExecutionState {
    fn new(graph: &DiGraph<DeclarationMeta, ()>) -> Self {
        let ignored: HashSet<usize> = graph
            .node_indices()
            .filter(|&n| {
                let meta = &graph[n];
                meta.class == crate::declaration_meta::Class::Block
                    || graph
                        .node_indices()
                        .any(|outer| graph[outer].loops_decls_indexes.contains(&n.index()))
            })
            .map(|n| n.index())
            .collect();

        let capacity = graph.node_count() - ignored.len();

        ExecutionState {
            node_status: HashMap::with_capacity_and_hasher(capacity, FxBuildHasher),
            node_results: HashMap::with_capacity_and_hasher(capacity, FxBuildHasher),
            subtask_status: HashMap::default(),
            subtask_results: HashMap::default(),
            subtask_info: HashMap::default(),
            subtask_total: HashMap::default(),
            ignored,
        }
    }

    fn is_node_ready(&self, node_id: usize, graph: &DiGraph<DeclarationMeta, ()>) -> bool {
        if self.ignored.contains(&node_id) {
            return false;
        }

        if self.node_status.get(&node_id) != Some(&Status::Pending) {
            return false;
        }

        graph
            .neighbors_directed(NodeIndex::new(node_id), Direction::Incoming)
            .all(|dep| {
                let dep_id = dep.index();
                self.ignored.contains(&dep_id)
                    || self.node_status.get(&dep_id) == Some(&Status::Done)
            })
    }

    fn all_subtasks_done(&self, parent_id: usize) -> bool {
        if let Some(&total) = self.subtask_total.get(&parent_id) {
            (0..total).all(|i| self.subtask_status.get(&(parent_id, i)) == Some(&Status::Done))
        } else {
            false
        }
    }

    fn is_complete(&self) -> bool {
        self.node_status
            .values()
            .all(|&s| s == Status::Done || s == Status::Failed)
    }
}

// ====================================================================
//                       WORKER LOAD BALANCER
// ====================================================================

#[derive(Clone)]
struct WorkerLoad {
    predicted_finish_time: Instant,
    assigned_complexity: u64,
}

impl WorkerLoad {
    fn new() -> Self {
        WorkerLoad {
            predicted_finish_time: Instant::now(),
            assigned_complexity: 0,
        }
    }

    fn add_task(&mut self, complexity: u64, estimated_duration: Option<Duration>) {
        let task_duration = estimated_duration
            .unwrap_or_else(|| Duration::from_micros(complexity * COMPLEXITY_MULTIPLIER));

        let now = Instant::now();

        if self.predicted_finish_time > now {
            self.predicted_finish_time += task_duration;
        } else {
            self.predicted_finish_time = now + task_duration;
        }

        self.assigned_complexity += complexity;
    }

    fn time_until_free(&self) -> Duration {
        let now = Instant::now();
        if self.predicted_finish_time > now {
            self.predicted_finish_time - now
        } else {
            Duration::ZERO
        }
    }
}

struct Scheduler {
    worker_loads: Vec<WorkerLoad>,
}

impl Scheduler {
    fn new(num_workers: usize) -> Self {
        Scheduler {
            worker_loads: vec![WorkerLoad::new(); num_workers],
        }
    }

    fn schedule_tasks(&mut self, mut tasks: Vec<Task>) -> Vec<(Task, usize)> {
        // Сортуємо за estimated_duration або complexity
        tasks.sort_by_key(|t| {
            std::cmp::Reverse(
                t.estimated_duration
                    .unwrap_or_else(|| Duration::from_micros(t.complexity * COMPLEXITY_MULTIPLIER))
                    .as_micros(),
            )
        });

        let mut assignments = Vec::new();

        for task in tasks {
            let worker_id = self.find_least_loaded_worker();
            self.worker_loads[worker_id].add_task(task.complexity, task.estimated_duration);

            let duration = task
                .estimated_duration
                .unwrap_or_else(|| Duration::from_micros(task.complexity * COMPLEXITY_MULTIPLIER));

            debug!(
                "Scheduled task {:?} (estimated={:?}) to worker {} (will be free at +{:?})",
                task.id,
                duration,
                worker_id,
                self.worker_loads[worker_id].time_until_free()
            );

            assignments.push((task, worker_id));
        }

        assignments
    }

    fn find_least_loaded_worker(&self) -> usize {
        self.worker_loads
            .iter()
            .enumerate()
            .min_by_key(|(_, load)| load.predicted_finish_time)
            .map(|(id, _)| id)
            .unwrap_or(0)
    }

    fn next_scheduling_time(&self) -> Duration {
        let max_finish = self
            .worker_loads
            .iter()
            .map(|load| load.time_until_free())
            .max()
            .unwrap_or(Duration::ZERO);

        max_finish + Duration::from_millis(10)
    }

    fn reset(&mut self) {
        for load in &mut self.worker_loads {
            *load = WorkerLoad::new();
        }
    }
}

// ====================================================================
//                       EXECUTOR
// ====================================================================

pub struct Executor {
    graph: DiGraph<DeclarationMeta, ()>,
    state: Arc<Mutex<ExecutionState>>,
    pool: WorkerPool,
    scheduler: Scheduler,
}

impl Executor {
    pub fn new(graph: DiGraph<DeclarationMeta, ()>) -> Self {
        let num_workers = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        let state = ExecutionState::new(&graph);

        Executor {
            graph,
            state: Arc::new(Mutex::new(state)),
            pool: WorkerPool::new(num_workers),
            scheduler: Scheduler::new(num_workers),
        }
    }

    pub fn execute(mut self) -> Result<Vec<Env>, String> {
        {
            let mut state = self.state.lock();
            for node_id in self.graph.node_indices().map(|n| n.index()) {
                if !state.ignored.contains(&node_id) {
                    state.node_status.insert(node_id, Status::Pending);
                }
            }
        }

        let num_workers = self.pool.num_workers;
        info!("Starting execution with {} workers", num_workers);

        loop {
            self.collect_results()?;

            let ready_tasks = self.gather_ready_tasks()?;

            if ready_tasks.is_empty() {
                let state = self.state.lock();

                if state.is_complete() && !self.pool.has_active_tasks() {
                    info!("✅ All tasks completed successfully");
                    break;
                }

                if !self.pool.has_active_tasks() {
                    error!("❌ Deadlock detected: no ready tasks and no active tasks");
                    return Err("Execution deadlock".into());
                }

                drop(state);
                thread::sleep(Duration::from_millis(10));
                continue;
            }

            info!("📋 Scheduling {} ready tasks", ready_tasks.len());
            let assignments = self.scheduler.schedule_tasks(ready_tasks);

            for (task, worker_id) in assignments {
                self.pool.submit(task, worker_id)?;
            }

            let next_schedule = self.scheduler.next_scheduling_time();
            info!("⏰ Next scheduling in {:?}", next_schedule);

            thread::sleep(next_schedule);
            self.scheduler.reset();
        }

        let final_results = self.collect_results_final()?;
        self.pool.shutdown();

        Ok(final_results)
    }

    fn gather_ready_tasks(&mut self) -> Result<Vec<Task>, String> {
        let mut state = self.state.lock();
        let mut tasks = Vec::new();

        // 1. Збираємо готові вузли верхнього рівня
        let ready_nodes: Vec<usize> = self
            .graph
            .node_indices()
            .map(|n| n.index())
            .filter(|&id| state.is_node_ready(id, &self.graph))
            .collect();

        for node_id in ready_nodes {
            let meta = &self.graph[NodeIndex::new(node_id)];
            let env = self.create_env_for_node(node_id, &state);

            if let DeclType::Stmt(stmt) = &meta.decl.v {
                if let Stmt::For(var, arr_expr, body) = stmt.deref() {
                    if self.should_parallelize(arr_expr, body, &env, 0) {
                        self.initialize_loop_subtasks(
                            node_id,
                            var.clone(),
                            arr_expr,
                            body,
                            &env,
                            0,
                            &mut state,
                        )?;

                        state.node_status.insert(node_id, Status::Running);
                        continue;
                    }
                }
            }

            state.node_status.insert(node_id, Status::Running);

            // Використовуємо StatManager::predict для прогнозу
            let estimated_duration = StatManager::predict(node_id, &env_get_all_visible(&env));

            tasks.push(Task {
                id: TaskId::Node(node_id),
                work: WorkType::ExecuteNode(meta.decl.clone()),
                env,
                complexity: meta.complexity as u64,
                estimated_duration,
            });
        }

        let ready_subtasks: Vec<(usize, usize)> = state
            .subtask_status
            .iter()
            .filter(|(_, &status)| status == Status::Pending)
            .filter(|(&(parent_id, _), _)| {
                // Якщо це вкладений цикл
                if parent_id >= 1000000 {
                    let actual_parent = parent_id / 1000000;
                    let parent_iter = parent_id % 1000000;
                    // Перевіряємо що батьківська ітерація Running
                    state.subtask_status.get(&(actual_parent, parent_iter))
                        == Some(&Status::Running)
                } else {
                    true
                }
            })
            .map(|(&key, _)| key)
            .collect();
        for (parent_id, iter_idx) in ready_subtasks {
            let key = (parent_id, iter_idx);
            let info = state
                .subtask_info
                .get(&key)
                .ok_or_else(|| format!("Missing subtask info for {:?}", key))?;

            let var = info.var.clone();
            let element = info.element.clone();
            let body = info.body.clone();
            let body_idx = info.body_idx;
            let env = info.env.clone();
            let complexity = info.complexity;
            let depth = info.depth;
            let estimated_duration = info.estimated_duration; // ← ДОДАТИ

            // Перевіряємо чи тіло ітерації містить цикл для паралелізації
            if let DeclType::Stmt(stmt) = &body.v {
                if let Stmt::For(nested_var, nested_arr_expr, nested_body) = stmt.deref() {
                    debug!(
                        "Found nested loop in iteration {}[{}] at depth {}",
                        parent_id, iter_idx, depth
                    );

                    let mut test_env = env.clone();
                    let mut scope = env_create_scope();
                    scope.insert(var.clone(), element.clone());
                    env_add_scope(&mut test_env, scope);

                    if let Some(Expr::Array(arr)) = nested_arr_expr.eval(&test_env) {
                        debug!(
                            "Nested loop array size: {}, MIN_PARALLEL_SIZE: {}",
                            arr.len(),
                            MIN_PARALLEL_SIZE
                        );
                    }

                    if self.should_parallelize(nested_arr_expr, nested_body, &test_env, depth) {
                        state.subtask_status.insert(key, Status::Running);

                        self.initialize_nested_loop_subtasks(
                            parent_id,
                            iter_idx,
                            var.clone(),
                            element.clone(),
                            nested_var.clone(),
                            nested_arr_expr,
                            nested_body,
                            &env,
                            depth,
                            &mut state,
                        )?;

                        continue;
                    }
                }
            }

            state.subtask_status.insert(key, Status::Running);

            tasks.push(Task {
                id: TaskId::SubTask(parent_id, iter_idx),
                work: WorkType::ExecuteIteration { var, element, body },
                env,
                complexity,
                estimated_duration, // ← ВИКОРИСТОВУВАТИ збережене значення
            });
        }

        // debug!(
        //     "Gathered {} ready tasks ({} nodes, {} subtasks)",
        //     tasks.len(),
        //     tasks
        //         .iter()
        //         .filter(|t| matches!(t.id, TaskId::Node(_)))
        //         .count(),
        //     tasks
        //         .iter()
        //         .filter(|t| matches!(t.id, TaskId::SubTask(_, _)))
        //         .count()
        // );

        Ok(tasks)
    }

    fn should_parallelize(
        &self,
        arr_expr: &Expr,
        body: &Arc<Decl>,
        env: &Env,
        depth: usize,
    ) -> bool {
        if let Some(max_depth) = MAX_PARALLEL_DEPTH {
            if depth >= max_depth {
                debug!("Loop at depth {} exceeds MAX_PARALLEL_DEPTH", depth);
                return false;
            }
        }

        if let Some(Expr::Array(arr)) = arr_expr.eval(env) {
            let size = arr.len();
            if size < MIN_PARALLEL_SIZE {
                debug!(
                    "Loop has size {} < MIN_PARALLEL_SIZE={}",
                    size, MIN_PARALLEL_SIZE
                );
                return false;
            }

            // Прогнозуємо час виконання однієї ітерації
            let estimated_duration = StatManager::predict(body.index, &env_get_all_visible(env));
            let iteration_time = estimated_duration.unwrap_or_else(|| {
                let body_meta = &self.graph[NodeIndex::new(body.index)];
                Duration::from_micros(body_meta.complexity as u64 * COMPLEXITY_MULTIPLIER)
            });

            // Паралелізуємо якщо одна ітерація займає >= 100ms
            const MIN_ITERATION_TIME: Duration = Duration::from_millis(100);

            if iteration_time < MIN_ITERATION_TIME {
                debug!(
                    "Loop at depth {} with iteration time {:?} < 100ms will run sequentially",
                    depth, iteration_time
                );
                return false;
            }

            info!(
                "Loop at depth {} with size {} and iteration time {:?} will be parallelized",
                depth, size, iteration_time
            );
            true
        } else {
            false
        }
    }

    fn initialize_loop_subtasks(
        &self,
        parent_id: usize,
        var: String,
        arr_expr: &Expr,
        body: &Arc<Decl>,
        env: &Env,
        depth: usize,
        state: &mut ExecutionState,
    ) -> Result<(), String> {
        let arr = match arr_expr.eval(env) {
            Some(Expr::Array(arr)) => arr,
            _ => return Err("For requires array".into()),
        };

        let body_meta = &self.graph[NodeIndex::new(body.index)];
        let body_complexity = body_meta.complexity;
        let total = arr.len();

        // Прогнозуємо час для однієї ітерації
        let estimated_duration = StatManager::predict(body.index, &env_get_all_visible(env));

        state.subtask_total.insert(parent_id, total);

        info!(
            "🔄 Initializing loop {} at depth {} with {} parallel iterations (estimated={:?})",
            parent_id, depth, total, estimated_duration
        );

        for (i, element) in arr.into_iter().enumerate() {
            let key = (parent_id, i);

            state.subtask_status.insert(key, Status::Pending);
            state.subtask_info.insert(
                key,
                SubTaskInfo {
                    var: var.clone(),
                    element,
                    body: body.clone(),
                    body_idx: body.index,
                    env: env.clone(),
                    complexity: body_complexity as u64,
                    depth: depth + 1,
                    estimated_duration,
                },
            );
        }

        Ok(())
    }

    fn initialize_nested_loop_subtasks(
        &self,
        parent_id: usize,
        parent_iter: usize,
        outer_var: String,
        outer_element: Expr,
        inner_var: String,
        inner_arr_expr: &Expr,
        inner_body: &Arc<Decl>,
        env: &Env,
        depth: usize,
        state: &mut ExecutionState,
    ) -> Result<(), String> {
        let mut nested_env = env.clone();
        let mut scope = env_create_scope();
        scope.insert(outer_var, outer_element);
        env_add_scope(&mut nested_env, scope);

        let arr = match inner_arr_expr.eval(&nested_env) {
            Some(Expr::Array(arr)) => arr,
            _ => return Err("Nested for requires array".into()),
        };

        let body_meta = &self.graph[NodeIndex::new(inner_body.index)];
        let body_complexity = body_meta.complexity;
        let total = arr.len();

        // Прогнозуємо час для вкладеної ітерації
        let estimated_duration =
            StatManager::predict(inner_body.index, &env_get_all_visible(&nested_env));

        let nested_parent_id = parent_id * 1000000 + parent_iter;
        state.subtask_total.insert(nested_parent_id, total);

        info!(
            "🔄 Initializing nested loop {}[{}] at depth {} with {} iterations (estimated={:?})",
            parent_id,
            parent_iter,
            depth + 1,
            total,
            estimated_duration
        );

        for (i, element) in arr.into_iter().enumerate() {
            let nested_key = (nested_parent_id, i);

            state.subtask_status.insert(nested_key, Status::Pending);
            state.subtask_info.insert(
                nested_key,
                SubTaskInfo {
                    var: inner_var.clone(),
                    element,
                    body: inner_body.clone(),
                    body_idx: inner_body.index,
                    env: nested_env.clone(),
                    complexity: body_complexity as u64,
                    depth: depth + 2,
                    estimated_duration,
                },
            );
        }

        Ok(())
    }

    fn collect_results(&self) -> Result<(), String> {
        while let Ok(result) = self.pool.result_rx.try_recv() {
            self.handle_result(result)?;
        }
        Ok(())
    }

    fn handle_result(&self, result: TaskResult) -> Result<(), String> {
        let mut state = self.state.lock();

        match result.id {
            TaskId::Node(id) => match result.result {
                Ok(env) => {
                    state.node_status.insert(id, Status::Done);
                    state.node_results.insert(id, env);
                    debug!("✅ Node {} completed in {:?}", id, result.actual_duration);
                }
                Err(e) => {
                    state.node_status.insert(id, Status::Failed);
                    error!("❌ Node {} failed: {}", id, e);
                    return Err(e);
                }
            },
            TaskId::SubTask(parent, iter) => {
                let key = (parent, iter);
                match result.result {
                    Ok(env) => {
                        state.subtask_status.insert(key, Status::Done);
                        state.subtask_results.insert(key, env);

                        // Перевіряємо чи це звичайний цикл чи вкладений
                        let is_nested = parent >= 1000000;

                        if is_nested {
                            // Це ітерація вкладеного циклу
                            let actual_parent = parent / 1000000;
                            let parent_iter = parent % 1000000;

                            if state.all_subtasks_done(parent) {
                                let total = state.subtask_total[&parent];
                                let final_env = state.subtask_results[&(parent, total - 1)].clone();

                                // Позначаємо батьківську ітерацію як завершену
                                let parent_key = (actual_parent, parent_iter);
                                state.subtask_status.insert(parent_key, Status::Done);
                                state.subtask_results.insert(parent_key, final_env);

                                info!(
                                    "✅ Nested loop {}[{}] completed all {} iterations",
                                    actual_parent, parent_iter, total
                                );

                                // Перевіряємо чи всі ітерації батьківського циклу завершені
                                if state.all_subtasks_done(actual_parent) {
                                    let parent_total = state.subtask_total[&actual_parent];
                                    let parent_final_env = state.subtask_results
                                        [&(actual_parent, parent_total - 1)]
                                        .clone();

                                    state.node_status.insert(actual_parent, Status::Done);
                                    state.node_results.insert(actual_parent, parent_final_env);

                                    info!(
                                        "✅ Parent loop {} completed all {} iterations",
                                        actual_parent, parent_total
                                    );
                                }
                            }
                        } else {
                            // Звичайний цикл верхнього рівня
                            if state.all_subtasks_done(parent) {
                                let total = state.subtask_total[&parent];
                                let final_env = state.subtask_results[&(parent, total - 1)].clone();

                                state.node_status.insert(parent, Status::Done);
                                state.node_results.insert(parent, final_env);

                                info!("✅ Loop {} completed all {} iterations", parent, total);
                            }
                        }
                    }
                    Err(e) => {
                        state.subtask_status.insert(key, Status::Failed);

                        // Позначаємо батьківський цикл як Failed
                        let actual_parent = if parent >= 1000000 {
                            parent / 1000000
                        } else {
                            parent
                        };
                        state.node_status.insert(actual_parent, Status::Failed);

                        error!("❌ Subtask {}[{}] failed: {}", parent, iter, e);
                        return Err(e);
                    }
                }
            }
        }

        Ok(())
    }

    fn create_env_for_node(&self, node_id: usize, state: &ExecutionState) -> Env {
        let dep_ids: Vec<usize> = self
            .graph
            .neighbors_directed(NodeIndex::new(node_id), Direction::Incoming)
            .map(|n| n.index())
            .filter(|&id| !state.ignored.contains(&id))
            .collect();

        let dep_envs: Vec<Env> = dep_ids
            .iter()
            .filter_map(|&id| state.node_results.get(&id).cloned())
            .collect();

        create_env(dep_envs, dep_ids)
    }

    fn collect_results_final(&self) -> Result<Vec<Env>, String> {
        let state = self.state.lock();
        let mut results = Vec::new();

        for node_id in self.graph.node_indices().map(|n| n.index()) {
            if state.ignored.contains(&node_id) {
                continue;
            }

            if let Some(env) = state.node_results.get(&node_id) {
                results.push(env.clone());
            }
        }

        info!("Collected {} final results", results.len());
        Ok(results)
    }
}

// ====================================================================
//                       PUBLIC API
// ====================================================================

pub fn execute_plan(graph: DiGraph<DeclarationMeta, ()>) -> Result<Vec<Env>, String> {
    Executor::new(graph).execute()
}
