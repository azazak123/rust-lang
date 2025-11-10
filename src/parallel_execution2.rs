// use log::{debug, error, info};
// use parking_lot::Mutex;
// use petgraph::{
//     data::DataMap,
//     graph::{DiGraph, NodeIndex},
//     visit::EdgeRef,
//     Direction, Graph,
// };
// use rustc_hash::{FxBuildHasher, FxHashMap as HashMap};
// use std::{
//     ops::Deref,
//     sync::{
//         atomic::{AtomicUsize, Ordering},
//         mpsc::{channel, Receiver, Sender},
//         Arc,
//     },
//     thread,
//     time::{Duration, Instant},
// };

// use crate::stat_manager::StatManager;
// use crate::stmt::{DeclType, Stmt};
// use crate::{
//     declaration_meta::Class,
//     scope::{env_add_scope, env_create_scope, env_remove_scope},
// };
// use crate::{declaration_meta::DeclarationMeta, scope::Env, stmt::Decl};
// use crate::{execution::execute_stmt, scope::create_env};
// use crate::{expr::Expr, scope::env_get_all_visible};

// // ====================================================================
// //                       КОНФІГУРАЦІЯ
// // ====================================================================

// const MIN_PARALLEL_SIZE: usize = 10;
// const COMPLEXITY_MULTIPLIER: u64 = 100;

// // Максимальна глибина паралелізації (None = необмежено)
// const MAX_PARALLEL_DEPTH: Option<usize> = None;

// // ====================================================================
// //                       БАЗОВІ СТРУКТУРИ
// // ====================================================================

// #[derive(Debug, Clone, PartialEq, Eq, Hash)]
// enum TaskId {
//     Node(usize),
//     SubTask(usize, usize), // (parent_id, iter_idx)
// }

// #[derive(Clone)]
// struct Task {
//     code_graph_id: NodeIndex,
//     id: TaskId,
//     work: WorkType,
//     env: Env,
//     complexity: u64,
//     estimated_duration: Option<Duration>,
// }

// #[derive(Clone)]
// enum WorkType {
//     ExecuteNode(Arc<Decl>),
//     ExecuteIteration {
//         var: String,
//         element: Expr,
//         body: Arc<Decl>,
//     },
// }

// struct TaskResult {
//     id: TaskId,
//     result: Result<Env, String>,
//     actual_duration: Duration,
//     worker_id: usize, // Додаємо це поле
// }

// // ====================================================================
// //                       WORKER POOL
// // ====================================================================

// struct WorkerPool {
//     workers: Vec<Sender<Option<Task>>>,
//     result_rx: Receiver<TaskResult>,
//     active_tasks: Arc<AtomicUsize>,
//     num_workers: usize,
// }

// impl WorkerPool {
//     fn new(num_workers: usize) -> Self {
//         let (result_tx, result_rx) = channel();
//         let active_tasks = Arc::new(AtomicUsize::new(0));
//         let mut workers = Vec::new();

//         for worker_id in 0..num_workers {
//             let (task_tx, task_rx) = channel::<Option<Task>>();
//             let result_tx = result_tx.clone();
//             let active = active_tasks.clone();

//             thread::spawn(move || {
//                 debug!("Worker {} started", worker_id);
//                 while let Ok(Some(task)) = task_rx.recv() {
//                     let start = Instant::now();
//                     let result = Self::execute_task(task.work, task.env);
//                     let duration = start.elapsed();

//                     let _ = result_tx.send(TaskResult {
//                         id: task.id,
//                         result,
//                         actual_duration: duration,
//                         worker_id, // Додаємо worker_id
//                     });

//                     active.fetch_sub(1, Ordering::Release);
//                 }
//                 debug!("Worker {} stopped", worker_id);
//             });

//             workers.push(task_tx);
//         }

//         WorkerPool {
//             workers,
//             result_rx,
//             active_tasks,
//             num_workers,
//         }
//     }

//     fn execute_task(work: WorkType, mut env: Env) -> Result<Env, String> {
//         match work {
//             WorkType::ExecuteNode(decl) => {
//                 execute_stmt(&decl, &mut env)?;
//                 Ok(env)
//             }
//             WorkType::ExecuteIteration { var, element, body } => {
//                 let mut scope = env_create_scope();
//                 scope.insert(var, element);
//                 env_add_scope(&mut env, scope);
//                 execute_stmt(&body, &mut env)?;
//                 env_remove_scope(&mut env);
//                 Ok(env)
//             }
//         }
//     }

//     fn submit(&self, task: Task, worker_id: usize) -> Result<(), String> {
//         self.active_tasks.fetch_add(1, Ordering::Release);

//         self.workers[worker_id].send(Some(task)).map_err(|e| {
//             self.active_tasks.fetch_sub(1, Ordering::Release);
//             format!("Failed to submit task: {}", e)
//         })
//     }

//     fn has_active_tasks(&self) -> bool {
//         self.active_tasks.load(Ordering::Acquire) > 0
//     }

//     fn shutdown(self) {
//         for worker in self.workers {
//             let _ = worker.send(None);
//         }
//     }
// }

// // ====================================================================
// //                       EXECUTION STATE
// // ====================================================================

// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
// enum Status {
//     Pending,
//     Running,
//     Done,
//     Failed,
// }

// struct ExecutionState {
//     tasks_status: HashMap<NodeIndex, Status>,
//     tasks_results: HashMap<NodeIndex, Env>,
//     tasks_tree: Graph<Task, ()>,
//     tasks_nested: HashMap<NodeIndex, Vec<NodeIndex>>,
// }

// impl ExecutionState {
//     fn new() -> Self {
//         ExecutionState {
//             tasks_status: HashMap::default(),
//             tasks_results: HashMap::default(),
//             tasks_tree: Graph::new(),
//             tasks_nested: HashMap::default(),
//         }
//     }

//     fn is_node_ready(&self, node_id: NodeIndex) -> bool {
//         if self.tasks_status.get(&node_id) != Some(&Status::Pending) {
//             return false;
//         }
//         self.tasks_tree
//             .neighbors_directed(node_id, Direction::Incoming)
//             .all(|dep_id| self.tasks_status.get(&dep_id) == Some(&Status::Done))
//     }

//     fn all_subtasks_done(&self, parent_id: NodeIndex) -> bool {
//         let Some(nested_tasks) = self.tasks_nested.get(&parent_id) else {
//             return true;
//         };
//         nested_tasks
//             .iter()
//             .all(|node_index| self.tasks_status.get(node_index) == Some(&Status::Done))
//     }

//     fn is_complete(&self) -> bool {
//         self.tasks_status
//             .values()
//             .all(|&s| s == Status::Done || s == Status::Failed)
//     }

//     fn add_task(&mut self, task: Task) -> NodeIndex {
//         let node_id = self.tasks_tree.add_node(task);
//         self.tasks_status.insert(node_id, Status::Pending);
//         node_id
//     }

//     fn add_dependency(&mut self, from: NodeIndex, to: NodeIndex) {
//         self.tasks_tree.add_edge(from, to, ());
//     }

//     fn add_nested_task(&mut self, parent_id: NodeIndex, child_id: NodeIndex) {
//         self.tasks_nested
//             .entry(parent_id)
//             .or_insert_with(Vec::new)
//             .push(child_id);
//     }

//     fn mark_running(&mut self, node_id: NodeIndex) {
//         self.tasks_status.insert(node_id, Status::Running);
//     }

//     fn mark_done(&mut self, node_id: NodeIndex, result: Env) {
//         self.tasks_status.insert(node_id, Status::Done);
//         self.tasks_results.insert(node_id, result);
//     }

//     fn mark_failed(&mut self, node_id: NodeIndex) {
//         self.tasks_status.insert(node_id, Status::Failed);
//     }

//     fn get_result(&self, node_id: NodeIndex) -> Option<Env> {
//         self.tasks_results.get(&node_id).cloned()
//     }

//     fn get_nested_results(&self, parent_id: NodeIndex) -> Vec<Env> {
//         if let Some(nested_tasks) = self.tasks_nested.get(&parent_id) {
//             nested_tasks
//                 .iter()
//                 .filter_map(|&task_id| self.tasks_results.get(&task_id).cloned())
//                 .collect()
//         } else {
//             Vec::new()
//         }
//     }

//     fn get_status(&self, node_id: NodeIndex) -> Option<Status> {
//         self.tasks_status.get(&node_id).copied()
//     }

//     fn get_task(&self, node_id: NodeIndex) -> Option<&Task> {
//         self.tasks_tree.node_weight(node_id)
//     }

//     fn get_ready_tasks(&self) -> Vec<(NodeIndex, Task)> {
//         self.tasks_tree
//             .node_indices()
//             .filter(|&node_id| self.is_node_ready(node_id))
//             .filter_map(|node_id| {
//                 let t = self
//                     .tasks_tree
//                     .node_weight(node_id)
//                     .map(|task| (node_id, task.clone()));
//                 t
//             })
//             .collect()
//     }

//     fn find_tasks_by_code_node(&self, code_node: NodeIndex) -> Vec<NodeIndex> {
//         self.tasks_tree
//             .node_indices()
//             .filter(|&task_node| {
//                 if let Some(task) = self.tasks_tree.node_weight(task_node) {
//                     task.code_graph_id == code_node
//                 } else {
//                     false
//                 }
//             })
//             .collect()
//     }

//     fn get_dependencies(&self, node_id: NodeIndex) -> Vec<NodeIndex> {
//         self.tasks_tree
//             .neighbors_directed(node_id, Direction::Incoming)
//             .collect()
//     }

//     fn get_dependents(&self, node_id: NodeIndex) -> Vec<NodeIndex> {
//         self.tasks_tree
//             .neighbors_directed(node_id, Direction::Outgoing)
//             .collect()
//     }

//     fn build_task_id_map(&self) -> HashMap<TaskId, NodeIndex> {
//         let mut map = HashMap::default();

//         for node_id in self.tasks_tree.node_indices() {
//             if let Some(task) = self.tasks_tree.node_weight(node_id) {
//                 map.insert(task.id.clone(), node_id);
//             }
//         }

//         map
//     }
// }

// // ====================================================================
// //                       WORKER LOAD BALANCER
// // ====================================================================

// #[derive(Clone)]
// struct WorkerLoad {
//     predicted_finish_time: Instant,
//     assigned_complexity: u64,
//     tasks_count: usize,
// }

// impl WorkerLoad {
//     fn new() -> Self {
//         WorkerLoad {
//             predicted_finish_time: Instant::now(),
//             assigned_complexity: 0,
//             tasks_count: 0,
//         }
//     }

//     fn add_task(&mut self, complexity: u64, estimated_duration: Option<Duration>) {
//         let task_duration = estimated_duration
//             .unwrap_or_else(|| Duration::from_micros(complexity * COMPLEXITY_MULTIPLIER));

//         let now = Instant::now();
//         if self.predicted_finish_time > now {
//             self.predicted_finish_time += task_duration;
//         } else {
//             self.predicted_finish_time = now + task_duration;
//         }

//         self.assigned_complexity += complexity;
//         self.tasks_count += 1;
//     }

//     fn complete_task(&mut self, actual_duration: Duration, complexity: u64) {
//         self.assigned_complexity = self.assigned_complexity.saturating_sub(complexity);
//         self.tasks_count = self.tasks_count.saturating_sub(1);

//         // Коригуємо predicted_finish_time на основі фактичного часу
//         let now = Instant::now();
//         if self.predicted_finish_time > now {
//             // Якщо виконання було швидше, зменшуємо час
//             let predicted_left = self.predicted_finish_time - now;
//             if actual_duration < predicted_left {
//                 self.predicted_finish_time = now + (predicted_left - actual_duration);
//             }
//         }
//     }

//     fn time_until_free(&self) -> Duration {
//         let now = Instant::now();
//         if self.predicted_finish_time > now {
//             self.predicted_finish_time - now
//         } else {
//             Duration::ZERO
//         }
//     }

//     fn is_idle(&self) -> bool {
//         self.tasks_count == 0 && self.predicted_finish_time <= Instant::now()
//     }
// }

// struct Scheduler {
//     worker_loads: Arc<Mutex<Vec<WorkerLoad>>>,
//     num_workers: usize,
// }

// impl Scheduler {
//     fn new(num_workers: usize) -> Self {
//         Scheduler {
//             worker_loads: Arc::new(Mutex::new(vec![WorkerLoad::new(); num_workers])),
//             num_workers,
//         }
//     }

//     /// Вибирає найкращий воркер для однієї задачі
//     fn select_worker(&self, task: &Task, pool: &WorkerPool) -> usize {
//         let loads = self.worker_loads.lock();

//         // Стратегія: вибираємо воркер з найменшим predicted_finish_time
//         let worker_id = loads
//             .iter()
//             .enumerate()
//             .min_by_key(|(_, load)| load.predicted_finish_time)
//             .map(|(id, _)| id)
//             .unwrap_or(0);

//         worker_id
//     }

//     /// Планує задачу на воркер і оновлює статистику
//     fn schedule_task(&self, task: &Task, worker_id: usize) {
//         let mut loads = self.worker_loads.lock();
//         loads[worker_id].add_task(task.complexity, task.estimated_duration);

//         debug!(
//             "Scheduled task {:?} (complexity={}, estimated={:?}) to worker {} (will be free in {:?})",
//             task.id,
//             task.complexity,
//             task.estimated_duration,
//             worker_id,
//             loads[worker_id].time_until_free()
//         );
//     }

//     /// Повідомляє scheduler про завершення задачі
//     fn task_completed(&self, worker_id: usize, actual_duration: Duration, complexity: u64) {
//         let mut loads = self.worker_loads.lock();
//         if worker_id < loads.len() {
//             loads[worker_id].complete_task(actual_duration, complexity);
//         }
//     }

//     /// Знаходить найменш завантажений воркер
//     fn find_least_loaded_worker(&self) -> usize {
//         let loads = self.worker_loads.lock();
//         loads
//             .iter()
//             .enumerate()
//             .min_by_key(|(_, load)| load.predicted_finish_time)
//             .map(|(id, _)| id)
//             .unwrap_or(0)
//     }

//     /// Знаходит воркер з найменшою кількістю задач
//     fn find_worker_with_fewest_tasks(&self) -> usize {
//         let loads = self.worker_loads.lock();
//         loads
//             .iter()
//             .enumerate()
//             .min_by_key(|(_, load)| load.tasks_count)
//             .map(|(id, _)| id)
//             .unwrap_or(0)
//     }

//     /// Отримує статистику по воркерах
//     fn get_worker_stats(&self) -> Vec<(usize, Duration, u64, usize)> {
//         let loads = self.worker_loads.lock();
//         loads
//             .iter()
//             .enumerate()
//             .map(|(id, load)| {
//                 (
//                     id,
//                     load.time_until_free(),
//                     load.assigned_complexity,
//                     load.tasks_count,
//                 )
//             })
//             .collect()
//     }

//     /// Перевіряє чи всі воркери простоюють
//     fn all_workers_idle(&self) -> bool {
//         let loads = self.worker_loads.lock();
//         loads.iter().all(|load| load.is_idle())
//     }

//     /// Час до наступного можливого планування
//     fn next_scheduling_time(&self) -> Duration {
//         let loads = self.worker_loads.lock();
//         let min_finish = loads
//             .iter()
//             .map(|load| load.time_until_free())
//             .min()
//             .unwrap_or(Duration::ZERO);

//         if min_finish > Duration::ZERO {
//             min_finish
//         } else {
//             Duration::from_millis(1)
//         }
//     }

//     /// Скидає статистику scheduler
//     fn reset(&self) {
//         let mut loads = self.worker_loads.lock();
//         for load in loads.iter_mut() {
//             *load = WorkerLoad::new();
//         }
//     }

//     /// Стратегія вибору воркера з різними політиками
//     fn select_worker_with_policy(&self, task: &Task, policy: SchedulingPolicy) -> usize {
//         match policy {
//             SchedulingPolicy::LeastLoaded => self.find_least_loaded_worker(),
//             SchedulingPolicy::FewestTasks => self.find_worker_with_fewest_tasks(),
//             SchedulingPolicy::RoundRobin => {
//                 // Простий round-robin
//                 let loads = self.worker_loads.lock();
//                 (task.id.hash() % self.num_workers) as usize
//             }
//             SchedulingPolicy::WorkStealing => {
//                 // Для work-stealing потрібна складніша логіка
//                 self.find_least_loaded_worker()
//             }
//         }
//     }
// }

// #[derive(Debug, Clone, Copy)]
// enum SchedulingPolicy {
//     LeastLoaded,  // Вибирає воркер з найменшим predicted_finish_time
//     FewestTasks,  // Вибирає воркер з найменшою кількістю задач
//     RoundRobin,   // По черзі
//     WorkStealing, // З крадіжкою роботи
// }

// // Додаємо простий hash для TaskId
// impl TaskId {
//     fn hash(&self) -> usize {
//         match self {
//             TaskId::Node(n) => *n,
//             TaskId::SubTask(p, i) => p.wrapping_mul(1000).wrapping_add(*i),
//         }
//     }
// }

// // ====================================================================
// //                       EXECUTOR
// // ====================================================================

// pub struct Executor {
//     code_graph: Graph<DeclarationMeta, ()>,
//     state: Arc<Mutex<ExecutionState>>,
//     pool: WorkerPool,
//     scheduler: Scheduler,
// }

// impl Executor {
//     pub fn new(graph: DiGraph<DeclarationMeta, ()>) -> Self {
//         let num_workers = thread::available_parallelism()
//             .map(|n| n.get())
//             .unwrap_or(4);

//         let state = ExecutionState::new();

//         Executor {
//             code_graph: graph,
//             state: Arc::new(Mutex::new(state)),
//             pool: WorkerPool::new(num_workers),
//             scheduler: Scheduler::new(num_workers),
//         }
//     }

//     /// Створює задачу для виконання декларації
//     fn create_task_from_decl(
//         &self,
//         code_node: NodeIndex,
//         decl: Arc<Decl>,
//         env: Env,
//         complexity: u64,
//     ) -> Task {
//         Task {
//             code_graph_id: code_node,
//             id: TaskId::Node(decl.index),
//             work: WorkType::ExecuteNode(decl),
//             env,
//             complexity,
//             estimated_duration: None,
//         }
//     }

//     /// Створює задачу для ітерації циклу
//     fn create_iteration_task(
//         &self,
//         parent_code_node: NodeIndex,
//         parent_index: usize,
//         iter_idx: usize,
//         var: String,
//         element: Expr,
//         body: Arc<Decl>,
//         env: Env,
//         complexity: u64,
//     ) -> Task {
//         Task {
//             code_graph_id: parent_code_node,
//             id: TaskId::SubTask(parent_index, iter_idx),
//             work: WorkType::ExecuteIteration { var, element, body },
//             env,
//             complexity,
//             estimated_duration: None,
//         }
//     }

//     fn expand_for_loop(
//         &self,
//         loop_code_node: NodeIndex,
//         loop_meta: &DeclarationMeta,
//         var: String,
//         iterable_expr: &Expr,
//         body: Arc<Decl>,
//         base_env: &Env,
//     ) -> Result<Vec<NodeIndex>, String> {
//         // Обчислюємо ітеровану колекцію
//         let iterable_value = iterable_expr
//             .eval(base_env)
//             .ok_or_else(|| "Failed to evaluate iterable expression".to_string())?;

//         let elements = self.extract_array_elements(&iterable_value)?;

//         let mut state = self.state.lock();

//         let sequential = self.should_be_sequential(loop_meta);

//         // Якщо цикл послідовний або має мало ітерацій - виконуємо як одну задачу
//         let execute_as_single = sequential || elements.len() < 4; // поріг можна налаштувати

//         if execute_as_single {
//             // Виконуємо весь цикл як одну задачу
//             let task = self.create_task_from_decl(
//                 loop_code_node,
//                 loop_meta.decl.clone(),
//                 base_env.clone(),
//                 loop_meta.complexity as u64,
//             );
//             let task_id = state.add_task(task);
//             return Ok(vec![task_id]);
//         }

//         // Розгортаємо цикл у окремі задачі для паралельного виконання
//         let parent_task = self.create_task_from_decl(
//             loop_code_node,
//             loop_meta.decl.clone(),
//             base_env.clone(),
//             loop_meta.complexity as u64,
//         );
//         let parent_id = state.add_task(parent_task);

//         let mut task_ids = Vec::new();

//         for (iter_idx, element) in elements.into_iter().enumerate() {
//             let iter_task = self.create_iteration_task(
//                 loop_code_node,
//                 loop_meta.decl.index,
//                 iter_idx,
//                 var.clone(),
//                 element,
//                 body.clone(),
//                 base_env.clone(),
//                 1,
//             );

//             let task_id = state.add_task(iter_task);
//             task_ids.push(task_id);
//             state.add_nested_task(parent_id, task_id);

//             // Завжди додаємо залежність від попередньої ітерації
//             // для збереження порядку (навіть для паралельних циклів це може бути корисно)
//             // if iter_idx > 0 {
//             //     state.add_dependency(task_ids[iter_idx - 1], task_id);
//             // }
//         }

//         Ok(task_ids)
//     }

//     /// Розгортає Block у задачі
//     fn expand_block(
//         &self,
//         block_code_node: NodeIndex,
//         block_meta: &DeclarationMeta,
//         statements: &[Arc<Decl>],
//         base_env: &Env,
//         sequential: bool,
//     ) -> Result<Vec<NodeIndex>, String> {
//         let mut state = self.state.lock();
//         let mut task_ids = Vec::new();

//         for (idx, stmt_decl) in statements.iter().enumerate() {
//             let task = Task {
//                 code_graph_id: block_code_node,
//                 id: TaskId::SubTask(block_meta.decl.index, idx),
//                 work: WorkType::ExecuteNode(stmt_decl.clone()),
//                 env: base_env.clone(),
//                 complexity: 1,
//                 estimated_duration: None,
//             };

//             let task_id = state.add_task(task);
//             task_ids.push(task_id);

//             // Додаємо послідовні залежності якщо потрібно
//             if sequential && idx > 0 {
//                 state.add_dependency(task_ids[idx - 1], task_id);
//             }
//         }

//         Ok(task_ids)
//     }

//     /// Визначає чи має бути цикл послідовним на основі mut_deps
//     fn should_be_sequential(&self, meta: &DeclarationMeta) -> bool {
//         !meta.mut_deps.is_empty()
//     }

//     /// Витягує елементи з Expr::Array
//     fn extract_array_elements(&self, expr: &Expr) -> Result<Vec<Expr>, String> {
//         match expr {
//             Expr::Array(elements) => Ok(elements.clone()),
//             _ => Err(format!("Expected array, got {:?}", expr)),
//         }
//     }

//     /// Будує задачі з code_graph
//     pub fn build_tasks_from_code_graph(&self, root_env: Env) -> Result<(), String> {
//         let mut code_to_tasks: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::default();

//         // Топологічний обхід
//         let mut topo = petgraph::visit::Topo::new(&self.code_graph);

//         while let Some(code_node) = topo.next(&self.code_graph) {
//             let meta = &self.code_graph[code_node];

//             // Створюємо задачі залежно від класу та типу декларації
//             let task_nodes = self.create_tasks_from_meta(code_node, meta, &root_env)?;

//             // Додаємо залежності з code_graph
//             let mut state = self.state.lock();
//             for dep_code_node in self
//                 .code_graph
//                 .neighbors_directed(code_node, Direction::Incoming)
//             {
//                 if let Some(dep_task_nodes) = code_to_tasks.get(&dep_code_node) {
//                     // Перші задачі поточного вузла залежать від останніх задач залежностей
//                     if let (Some(&first_task), Some(&last_dep_task)) =
//                         (task_nodes.first(), dep_task_nodes.last())
//                     {
//                         state.add_dependency(last_dep_task, first_task);
//                     }
//                 }
//             }
//             drop(state);

//             code_to_tasks.insert(code_node, task_nodes);
//         }

//         Ok(())
//     }

//     /// Створює задачі з DeclarationMeta
//     fn create_tasks_from_meta(
//         &self,
//         code_node: NodeIndex,
//         meta: &DeclarationMeta,
//         base_env: &Env,
//     ) -> Result<Vec<NodeIndex>, String> {
//         match meta.class {
//             Class::Loop => self.create_loop_tasks(code_node, meta, base_env),
//             Class::Block => self.create_block_tasks(code_node, meta, base_env),
//             Class::Ordinary => self.create_ordinary_task(code_node, meta, base_env),
//         }
//     }

//     /// Створює задачі для циклу
//     fn create_loop_tasks(
//         &self,
//         code_node: NodeIndex,
//         meta: &DeclarationMeta,
//         base_env: &Env,
//     ) -> Result<Vec<NodeIndex>, String> {
//         let decl = &meta.decl;

//         match &decl.v {
//             DeclType::Stmt(stmt) => {
//                 match stmt.as_ref() {
//                     Stmt::For(var, iterable_expr, body) => {
//                         let child_ids = self.expand_for_loop(
//                             code_node,
//                             meta,
//                             var.clone(),
//                             iterable_expr,
//                             body.clone(),
//                             base_env,
//                         )?;

//                         // Батьківська задача не додається в tasks_tree окремо,
//                         // вона вже є там після expand_for_loop
//                         // Повертаємо лише дочірні задачі для залежностей
//                         Ok(child_ids)
//                     }
//                     Stmt::While(_condition, _body) => {
//                         // While виконується як одна задача
//                         let mut state = self.state.lock();
//                         let task = self.create_task_from_decl(
//                             code_node,
//                             meta.decl.clone(),
//                             base_env.clone(),
//                             meta.complexity as u64,
//                         );
//                         let task_id = state.add_task(task);
//                         Ok(vec![task_id])
//                     }
//                     _ => Err(format!("Expected loop statement, got {:?}", stmt)),
//                 }
//             }
//             _ => Err(format!("Loop class requires Stmt, got {:?}", decl.v)),
//         }
//     }

//     /// Створює задачі для блоку
//     fn create_block_tasks(
//         &self,
//         code_node: NodeIndex,
//         meta: &DeclarationMeta,
//         base_env: &Env,
//     ) -> Result<Vec<NodeIndex>, String> {
//         let decl = &meta.decl;

//         match &decl.v {
//             DeclType::Stmt(stmt) => match stmt.as_ref() {
//                 Stmt::Block(statements) => {
//                     let sequential = self.should_be_sequential(meta);
//                     let task_ids =
//                         self.expand_block(code_node, meta, statements, base_env, sequential)?;
//                     Ok(task_ids)
//                 }
//                 _ => Err(format!("Expected block statement, got {:?}", stmt)),
//             },
//             _ => Err(format!("Block class requires Stmt, got {:?}", decl.v)),
//         }
//     }

//     /// Створює задачу для звичайної декларації
//     fn create_ordinary_task(
//         &self,
//         code_node: NodeIndex,
//         meta: &DeclarationMeta,
//         base_env: &Env,
//     ) -> Result<Vec<NodeIndex>, String> {
//         let mut state = self.state.lock();
//         let task = self.create_task_from_decl(
//             code_node,
//             meta.decl.clone(),
//             base_env.clone(),
//             meta.complexity as u64,
//         );
//         let task_id = state.add_task(task);
//         Ok(vec![task_id])
//     }

//     /// Обробляє результат задачі
//     fn process_task_result(&self, result: TaskResult, task_id_map: &HashMap<TaskId, NodeIndex>) {
//         if let Some(&node_id) = task_id_map.get(&result.id) {
//             let mut state = self.state.lock();
//             match result.result {
//                 Ok(env) => {
//                     state.mark_done(node_id, env);
//                     debug!(
//                         "Task {:?} completed in {:?}",
//                         result.id, result.actual_duration
//                     );
//                 }
//                 Err(err) => {
//                     state.mark_failed(node_id);
//                     error!("Task {:?} failed: {}", result.id, err);
//                 }
//             }
//         }
//     }
//     /// Основний цикл виконання
//     pub fn run(&self) -> Result<(), String> {
//         let task_id_map = {
//             let state = self.state.lock();
//             state.build_task_id_map()
//         };

//         while !{
//             let state = self.state.lock();
//             state.is_complete()
//         } {
//             let ready_tasks = {
//                 let state = self.state.lock();
//                 state.get_ready_tasks()
//             };

//             for (node_id, task) in ready_tasks {
//                 {
//                     let mut state = self.state.lock();
//                     state.mark_running(node_id);
//                 }

//                 // Вибираємо воркер через scheduler
//                 let worker_id = self.scheduler.select_worker(&task, &self.pool);

//                 // Повідомляємо scheduler про планування
//                 self.scheduler.schedule_task(&task, worker_id);

//                 if let Err(e) = self.pool.submit(task, worker_id) {
//                     error!("Failed to submit task: {}", e);
//                     let mut state = self.state.lock();
//                     state.mark_failed(node_id);
//                 }
//             }

//             // Обробляємо результати
//             while let Ok(result) = self.pool.result_rx.try_recv() {
//                 let id = result.id.clone();
//                 self.process_task_result(result, &task_id_map);

//                 // Повідомляємо scheduler про завершення задачі
//                 if let Some(&node_id) = task_id_map.get(&id) {
//                     let state = self.state.lock();
//                     if let Some(task) = state.get_task(node_id) {
//                         // Отримуємо worker_id з результату або з іншого джерела
//                         // Можна додати worker_id в TaskResult
//                         // self.scheduler.task_completed(worker_id, result.actual_duration, task.complexity);
//                     }
//                 }
//             }

//             let has_active = self.pool.has_active_tasks();
//             let has_ready = {
//                 let state = self.state.lock();
//                 !state.get_ready_tasks().is_empty()
//             };

//             if !has_active && !has_ready {
//                 let state = self.state.lock();
//                 if !state.is_complete() {
//                     return Err("Execution deadlock detected".to_string());
//                 }
//                 break;
//             }

//             thread::sleep(Duration::from_millis(1));
//         }

//         // Очікуємо завершення всіх активних задач
//         while self.pool.has_active_tasks() {
//             if let Ok(result) = self.pool.result_rx.recv_timeout(Duration::from_millis(100)) {
//                 self.process_task_result(result, &task_id_map);
//             }
//         }

//         Ok(())
//     }

//     /// Запуск з автоматичним shutdown
//     pub fn execute(self) -> Result<(), String> {
//         let result = self.run();
//         self.pool.shutdown();
//         result
//     }

//     // Публічні методи для доступу до стану

//     pub fn get_result(&self, node_id: NodeIndex) -> Option<Env> {
//         let state = self.state.lock();
//         state.get_result(node_id)
//     }

//     pub fn get_nested_results(&self, parent_id: NodeIndex) -> Vec<Env> {
//         let state = self.state.lock();
//         state.get_nested_results(parent_id)
//     }

//     pub fn get_status(&self, node_id: NodeIndex) -> Option<Status> {
//         let state = self.state.lock();
//         state.get_status(node_id)
//     }

//     pub fn find_tasks_by_code_node(&self, code_node: NodeIndex) -> Vec<NodeIndex> {
//         let state = self.state.lock();
//         state.find_tasks_by_code_node(code_node)
//     }

//     pub fn get_dependencies(&self, node_id: NodeIndex) -> Vec<NodeIndex> {
//         let state = self.state.lock();
//         state.get_dependencies(node_id)
//     }

//     pub fn get_dependents(&self, node_id: NodeIndex) -> Vec<NodeIndex> {
//         let state = self.state.lock();
//         state.get_dependents(node_id)
//     }

//     pub fn all_subtasks_done(&self, parent_id: NodeIndex) -> bool {
//         let state = self.state.lock();
//         state.all_subtasks_done(parent_id)
//     }
// }

// // ====================================================================
// //                       PUBLIC API
// // ====================================================================

// pub fn execute_plan(graph: DiGraph<DeclarationMeta, ()>) -> Result<(), String> {
//     Executor::new(graph).run()
// }

// executor.rs
use log::{debug, error, info};
use parking_lot::Mutex;
use petgraph::{
    data::DataMap,
    graph::{DiGraph, NodeIndex},
    visit::EdgeRef,
    Direction, Graph,
};
use rustc_hash::{FxBuildHasher, FxHashMap as HashMap};
use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use crate::stmt::{DeclType, Stmt};
use crate::{
    declaration_meta::Class,
    scope::{env_add_scope, env_create_scope, env_remove_scope},
};
use crate::{declaration_meta::DeclarationMeta, scope::Env, stmt::Decl};
use crate::{execution::execute_stmt, scope::create_env};
use crate::{expr::Expr, scope::env_get_all_visible};
use crate::{scope::env_empty, stat_manager::StatManager};

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

#[derive(Clone)]
struct WorkerTask {
    code_graph_id: NodeIndex,
    id: TaskId,
    work: WorkType,
    env: Env,
    complexity: u64,
    estimated_duration: Option<Duration>,
}

#[derive(Clone)]
struct Task {
    code_graph_id: NodeIndex,
    id: TaskId,
    work: WorkType,
    complexity: u64,
    estimated_duration: Option<Duration>,
}

#[derive(Clone)]
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
    worker_id: usize, // Додаємо це поле
}

// ====================================================================
//                       WORKER POOL
// ====================================================================

struct WorkerPool {
    workers: Vec<Sender<Option<WorkerTask>>>,
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
            let (task_tx, task_rx) = channel::<Option<WorkerTask>>();
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
                        worker_id, // Додаємо worker_id
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

    fn submit(&self, task: WorkerTask, worker_id: usize) -> Result<(), String> {
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

struct ExecutionState {
    tasks_status: HashMap<NodeIndex, Status>,
    tasks_results: HashMap<NodeIndex, Env>,
    tasks_tree: Graph<Task, ()>,
    tasks_nested: HashMap<NodeIndex, Vec<NodeIndex>>,
}

impl ExecutionState {
    fn new() -> Self {
        ExecutionState {
            tasks_status: HashMap::default(),
            tasks_results: HashMap::default(),
            tasks_tree: Graph::new(),
            tasks_nested: HashMap::default(),
        }
    }

    fn is_node_ready(&self, node_id: NodeIndex) -> bool {
        if self.tasks_status.get(&node_id) != Some(&Status::Pending) {
            return false;
        }
        self.tasks_tree
            .neighbors_directed(node_id, Direction::Incoming)
            .all(|dep_id| self.tasks_status.get(&dep_id) == Some(&Status::Done))
    }

    fn all_subtasks_done(&self, parent_id: NodeIndex) -> bool {
        let Some(nested_tasks) = self.tasks_nested.get(&parent_id) else {
            return true;
        };
        nested_tasks
            .iter()
            .all(|node_index| self.tasks_status.get(node_index) == Some(&Status::Done))
    }

    fn is_complete(&self) -> bool {
        self.tasks_status
            .values()
            .all(|&s| s == Status::Done || s == Status::Failed)
    }

    fn add_task(&mut self, task: Task) -> NodeIndex {
        let node_id = self.tasks_tree.add_node(task);
        self.tasks_status.insert(node_id, Status::Pending);
        node_id
    }

    fn add_dependency(&mut self, from: NodeIndex, to: NodeIndex) {
        self.tasks_tree.add_edge(from, to, ());
    }

    fn add_nested_task(&mut self, parent_id: NodeIndex, child_id: NodeIndex) {
        self.tasks_nested
            .entry(parent_id)
            .or_insert_with(Vec::new)
            .push(child_id);
    }

    fn mark_running(&mut self, node_id: NodeIndex) {
        self.tasks_status.insert(node_id, Status::Running);
    }

    fn mark_done(&mut self, node_id: NodeIndex, result: Env) {
        self.tasks_status.insert(node_id, Status::Done);
        self.tasks_results.insert(node_id, result);
    }

    fn mark_failed(&mut self, node_id: NodeIndex) {
        self.tasks_status.insert(node_id, Status::Failed);
    }

    fn get_result(&self, node_id: NodeIndex) -> Option<Env> {
        self.tasks_results.get(&node_id).cloned()
    }

    fn get_nested_results(&self, parent_id: NodeIndex) -> Vec<Env> {
        if let Some(nested_tasks) = self.tasks_nested.get(&parent_id) {
            nested_tasks
                .iter()
                .filter_map(|&task_id| self.tasks_results.get(&task_id).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    fn get_status(&self, node_id: NodeIndex) -> Option<Status> {
        self.tasks_status.get(&node_id).copied()
    }

    fn get_task(&self, node_id: NodeIndex) -> Option<&Task> {
        self.tasks_tree.node_weight(node_id)
    }

    fn get_ready_tasks(&self) -> Vec<(NodeIndex, Task)> {
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

    fn find_tasks_by_code_node(&self, code_node: NodeIndex) -> Vec<NodeIndex> {
        self.tasks_tree
            .node_indices()
            .filter(|&task_node| {
                if let Some(task) = self.tasks_tree.node_weight(task_node) {
                    task.code_graph_id == code_node
                } else {
                    false
                }
            })
            .collect()
    }

    fn get_dependencies(&self, node_id: NodeIndex) -> Vec<NodeIndex> {
        self.tasks_tree
            .neighbors_directed(node_id, Direction::Incoming)
            .collect()
    }

    fn get_dependents(&self, node_id: NodeIndex) -> Vec<NodeIndex> {
        self.tasks_tree
            .neighbors_directed(node_id, Direction::Outgoing)
            .collect()
    }

    fn build_task_id_map(&self) -> HashMap<TaskId, NodeIndex> {
        let mut map = HashMap::default();

        for node_id in self.tasks_tree.node_indices() {
            if let Some(task) = self.tasks_tree.node_weight(node_id) {
                map.insert(task.id.clone(), node_id);
            }
        }

        map
    }
}

// ====================================================================
//                       WORKER LOAD BALANCER
// ====================================================================

#[derive(Clone)]
struct WorkerLoad {
    predicted_finish_time: Instant,
    assigned_complexity: u64,
    tasks_count: usize,
}

impl WorkerLoad {
    fn new() -> Self {
        WorkerLoad {
            predicted_finish_time: Instant::now(),
            assigned_complexity: 0,
            tasks_count: 0,
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
        self.tasks_count += 1;
    }

    fn complete_task(&mut self, actual_duration: Duration, complexity: u64) {
        self.assigned_complexity = self.assigned_complexity.saturating_sub(complexity);
        self.tasks_count = self.tasks_count.saturating_sub(1);

        // Коригуємо predicted_finish_time на основі фактичного часу
        let now = Instant::now();
        if self.predicted_finish_time > now {
            // Якщо виконання було швидше, зменшуємо час
            let predicted_left = self.predicted_finish_time - now;
            if actual_duration < predicted_left {
                self.predicted_finish_time = now + (predicted_left - actual_duration);
            }
        }
    }

    fn time_until_free(&self) -> Duration {
        let now = Instant::now();
        if self.predicted_finish_time > now {
            self.predicted_finish_time - now
        } else {
            Duration::ZERO
        }
    }

    fn is_idle(&self) -> bool {
        self.tasks_count == 0 && self.predicted_finish_time <= Instant::now()
    }
}

struct Scheduler {
    worker_loads: Arc<Mutex<Vec<WorkerLoad>>>,
    num_workers: usize,
    last_used: usize,
}

impl Scheduler {
    fn new(num_workers: usize) -> Self {
        Scheduler {
            worker_loads: Arc::new(Mutex::new(vec![WorkerLoad::new(); num_workers])),
            num_workers,
            last_used: 0,
        }
    }

    /// Вибирає найкращий воркер для однієї задачі
    fn select_worker(&self) -> usize {
        let loads = self.worker_loads.lock();

        // Стратегія: вибираємо воркер з найменшим predicted_finish_time
        let worker_id = loads
            .iter()
            .enumerate()
            .min_by_key(|(_, load)| load.predicted_finish_time)
            .map(|(id, _)| id)
            .unwrap_or(0);

        worker_id
    }

    /// Планує задачу на воркер і оновлює статистику
    fn schedule_task(&self, task: &Task, worker_id: usize) {
        let mut loads = self.worker_loads.lock();
        loads[worker_id].add_task(task.complexity, task.estimated_duration);

        debug!(
            "Scheduled task {:?} (complexity={}, estimated={:?}) to worker {} (will be free in {:?})",
            task.id,
            task.complexity,
            task.estimated_duration,
            worker_id,
            loads[worker_id].time_until_free()
        );
    }

    /// Повідомляє scheduler про завершення задачі
    fn task_completed(&self, worker_id: usize, actual_duration: Duration, complexity: u64) {
        let mut loads = self.worker_loads.lock();
        if worker_id < loads.len() {
            loads[worker_id].complete_task(actual_duration, complexity);
        }
    }

    /// Знаходить найменш завантажений воркер
    fn find_least_loaded_worker(&self) -> usize {
        let loads = self.worker_loads.lock();
        loads
            .iter()
            .enumerate()
            .min_by_key(|(_, load)| load.predicted_finish_time)
            .map(|(id, _)| id)
            .unwrap_or(0)
    }

    /// Знаходит воркер з найменшою кількістю задач
    fn find_worker_with_fewest_tasks(&self) -> usize {
        let loads = self.worker_loads.lock();
        loads
            .iter()
            .enumerate()
            .min_by_key(|(_, load)| load.tasks_count)
            .map(|(id, _)| id)
            .unwrap_or(0)
    }

    /// Отримує статистику по воркерах
    fn get_worker_stats(&self) -> Vec<(usize, Duration, u64, usize)> {
        let loads = self.worker_loads.lock();
        loads
            .iter()
            .enumerate()
            .map(|(id, load)| {
                (
                    id,
                    load.time_until_free(),
                    load.assigned_complexity,
                    load.tasks_count,
                )
            })
            .collect()
    }

    /// Перевіряє чи всі воркери простоюють
    fn all_workers_idle(&self) -> bool {
        let loads = self.worker_loads.lock();
        loads.iter().all(|load| load.is_idle())
    }

    /// Час до наступного можливого планування
    fn next_scheduling_time(&self) -> Duration {
        let loads = self.worker_loads.lock();
        let min_finish = loads
            .iter()
            .map(|load| load.time_until_free())
            .min()
            .unwrap_or(Duration::ZERO);

        if min_finish > Duration::ZERO {
            min_finish
        } else {
            Duration::from_millis(1)
        }
    }

    /// Скидає статистику scheduler
    fn reset(&self) {
        let mut loads = self.worker_loads.lock();
        for load in loads.iter_mut() {
            *load = WorkerLoad::new();
        }
    }

    /// Стратегія вибору воркера з різними політиками
    fn select_worker_with_policy(&mut self, policy: SchedulingPolicy) -> usize {
        match policy {
            SchedulingPolicy::LeastLoaded => self.find_least_loaded_worker(),
            SchedulingPolicy::FewestTasks => self.find_worker_with_fewest_tasks(),
            SchedulingPolicy::RoundRobin => {
                // Простий round-robin
                let res = (self.last_used % self.num_workers) as usize;
                self.last_used += 1;
                res
            }
            SchedulingPolicy::WorkStealing => {
                // Для work-stealing потрібна складніша логіка
                self.find_least_loaded_worker()
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SchedulingPolicy {
    LeastLoaded,  // Вибирає воркер з найменшим predicted_finish_time
    FewestTasks,  // Вибирає воркер з найменшою кількістю задач
    RoundRobin,   // По черзі
    WorkStealing, // З крадіжкою роботи
}

// ====================================================================
//                       EXECUTOR
// ====================================================================

pub struct Executor {
    code_graph: DiGraph<DeclarationMeta, ()>,

    state: Arc<Mutex<ExecutionState>>,
    pool: WorkerPool,
    scheduler: Scheduler,
}

impl Executor {
    pub fn new(graph: DiGraph<DeclarationMeta, ()>) -> Self {
        let num_workers = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        let state = ExecutionState::new();

        Executor {
            code_graph: graph,
            state: Arc::new(Mutex::new(state)),
            pool: WorkerPool::new(num_workers),
            scheduler: Scheduler::new(num_workers),
        }
    }

    /// Створює задачу для виконання декларації
    fn create_task_from_decl(
        &self,
        code_node: NodeIndex,
        decl: Arc<Decl>,
        complexity: u64,
    ) -> Task {
        Task {
            code_graph_id: code_node,
            id: TaskId::Node(decl.index),
            work: WorkType::ExecuteNode(decl),
            complexity,
            estimated_duration: None,
        }
    }

    /// Створює задачу для ітерації циклу
    fn create_iteration_task(
        &self,
        parent_code_node: NodeIndex,
        parent_index: usize,
        iter_idx: usize,
        var: String,
        element: Expr,
        body: Arc<Decl>,
        complexity: u64,
    ) -> Task {
        Task {
            code_graph_id: parent_code_node,
            id: TaskId::SubTask(parent_index, iter_idx),
            work: WorkType::ExecuteIteration { var, element, body },
            complexity,
            estimated_duration: None,
        }
    }

    fn create_tasks(&self, code_node: NodeIndex) -> Result<Vec<NodeIndex>, String> {
        let meta = &self.code_graph[code_node];

        match &meta.decl.v {
            DeclType::Stmt(stmt) => match stmt.deref() {
                Stmt::Block(decls) => self.create_block_tasks(code_node, meta),
                Stmt::For(_, expr, decl) => todo!(),
                _ => self
                    .create_or_execute_ordinary_task(code_node)
                    .map(|x| vec![x.unwrap()]),
            },
            _ => self
                .create_or_execute_ordinary_task(code_node)
                .map(|x| vec![x.unwrap()]),
        }
    }

    // fn expand_for_loop(
    //     &self,
    //     loop_code_node: NodeIndex,
    //     loop_meta: &DeclarationMeta,
    //     var: String,
    //     iterable_expr: &Expr,
    //     body: Arc<Decl>,
    //     base_env: &Env,
    // ) -> Result<Vec<NodeIndex>, String> {
    //     // Обчислюємо ітеровану колекцію
    //     let iterable_value = iterable_expr
    //         .eval(base_env)
    //         .ok_or_else(|| "Failed to evaluate iterable expression".to_string())?;

    //     let elements = self.extract_array_elements(&iterable_value)?;

    //     let mut state = self.state.lock();

    //     let sequential = self.should_be_sequential(loop_meta);

    //     // Якщо цикл послідовний або має мало ітерацій - виконуємо як одну задачу
    //     let execute_as_single = sequential; // поріг можна налаштувати

    //     if execute_as_single {
    //         // Виконуємо весь цикл як одну задачу
    //         let task = self.create_task_from_decl(
    //             loop_code_node,
    //             loop_meta.decl.clone(),
    //             base_env.clone(),
    //             loop_meta.complexity as u64,
    //         );
    //         let task_id = state.add_task(task);
    //         return Ok(vec![task_id]);
    //     }

    //     // Розгортаємо цикл у окремі задачі для паралельного виконання
    //     let parent_task = self.create_task_from_decl(
    //         loop_code_node,
    //         loop_meta.decl.clone(),
    //         base_env.clone(),
    //         loop_meta.complexity as u64,
    //     );
    //     let parent_id = state.add_task(parent_task);

    //     let mut task_ids = Vec::new();

    //     for (iter_idx, element) in elements.into_iter().enumerate() {
    //         let iter_task = self.create_iteration_task(
    //             loop_code_node,
    //             loop_meta.decl.index,
    //             iter_idx,
    //             var.clone(),
    //             element,
    //             body.clone(),
    //             base_env.clone(),
    //             1,
    //         );

    //         let task_id = state.add_task(iter_task);
    //         task_ids.push(task_id);
    //         state.add_nested_task(parent_id, task_id);
    //     }

    //     Ok(task_ids)
    // }

    /// Розгортає Block у задачі
    fn expand_block(
        &self,
        block_code_node: NodeIndex,
        block_meta: &DeclarationMeta,
        statements: &[Arc<Decl>],
        sequential: bool,
    ) -> Result<Vec<NodeIndex>, String> {
        let mut state = self.state.lock();
        let mut task_ids = Vec::new();

        for id in block_meta.loops_decls_indexes {
            self.create_tasks(NodeIndex::new(id));

            // let task = Task {
            //     code_graph_id: block_code_node,
            //     id: TaskId::SubTask(block_meta.decl.index, idx),
            //     work: WorkType::ExecuteNode(stmt_decl.clone()),
            //     complexity: 1,
            //     estimated_duration: None,
            // };

            let task_id = state.add_task(task);
            task_ids.push(task_id);

            // Додаємо послідовні залежності якщо потрібно
            if sequential && idx > 0 {
                state.add_dependency(task_ids[idx - 1], task_id);
            }
        }

        Ok(task_ids)
    }

    /// Визначає чи має бути цикл послідовним на основі mut_deps
    fn should_be_sequential(&self, meta: &DeclarationMeta) -> bool {
        !meta.mut_deps.is_empty()
    }

    /// Витягує елементи з Expr::Array
    fn extract_array_elements(&self, expr: &Expr) -> Result<Vec<Expr>, String> {
        match expr {
            Expr::Array(elements) => Ok(elements.clone()),
            _ => Err(format!("Expected array, got {:?}", expr)),
        }
    }

    /// Створює задачі з DeclarationMeta
    // fn create_tasks_from_meta(
    //     &self,
    //     code_node: NodeIndex,
    //     meta: &DeclarationMeta,
    //     base_env: &Env,
    // ) -> Result<Vec<NodeIndex>, String> {
    //     match meta.class {
    //         Class::Loop => self.create_loop_tasks(code_node, meta, base_env),
    //         Class::Block => Ok(vec![]), // self.create_block_tasks(code_node, meta, base_env),
    //         Class::Ordinary => self.create_ordinary_task(code_node, meta, base_env),
    //     }
    // }

    // /// Створює задачі для циклу
    // fn create_loop_tasks(
    //     &self,
    //     code_node: NodeIndex,
    //     meta: &DeclarationMeta,
    //     base_env: &Env,
    // ) -> Result<Vec<NodeIndex>, String> {
    //     let decl = &meta.decl;

    //     match &decl.v {
    //         DeclType::Stmt(stmt) => {
    //             match stmt.as_ref() {
    //                 Stmt::For(var, iterable_expr, body) => {
    //                     let child_ids = self.expand_for_loop(
    //                         code_node,
    //                         meta,
    //                         var.clone(),
    //                         iterable_expr,
    //                         body.clone(),
    //                         base_env,
    //                     )?;

    //                     // Батьківська задача не додається в tasks_tree окремо,
    //                     // вона вже є там після expand_for_loop
    //                     // Повертаємо лише дочірні задачі для залежностей
    //                     Ok(child_ids)
    //                 }
    //                 Stmt::While(_condition, _body) => {
    //                     // While виконується як одна задача
    //                     let mut state = self.state.lock();
    //                     let task = self.create_task_from_decl(
    //                         code_node,
    //                         meta.decl.clone(),
    //                         base_env.clone(),
    //                         meta.complexity as u64,
    //                     );
    //                     let task_id = state.add_task(task);
    //                     Ok(vec![task_id])
    //                 }
    //                 _ => Err(format!("Expected loop statement, got {:?}", stmt)),
    //             }
    //         }
    //         _ => Err(format!("Loop class requires Stmt, got {:?}", decl.v)),
    //     }
    // }

    /// Створює задачі для блоку
    fn create_block_tasks(
        &self,
        code_node: NodeIndex,
        meta: &DeclarationMeta,
    ) -> Result<Vec<NodeIndex>, String> {
        let decl = &meta.decl;

        match &decl.v {
            DeclType::Stmt(stmt) => match stmt.as_ref() {
                Stmt::Block(statements) => {
                    // let sequential = self.should_be_sequential(meta);
                    let sequential = true;
                    let task_ids = self.expand_block(code_node, meta, statements, sequential)?;
                    Ok(task_ids)
                }
                _ => Err(format!("Expected block statement, got {:?}", stmt)),
            },
            _ => Err(format!("Block class requires Stmt, got {:?}", decl.v)),
        }
    }

    /// Створює задачу для звичайної декларації
    fn create_or_execute_ordinary_task(
        &self,
        code_node: NodeIndex,
    ) -> Result<Option<NodeIndex>, String> {
        let meta = &self.code_graph[code_node];

        // let estimated_time =
        //     StatManager::predict(code_node.index(), &env_get_all_visible(base_env));

        // if estimated_time.is_none() || estimated_time.unwrap() < Duration::from_millis(100) {
        //     let mut env = base_env.clone();
        //     execute_stmt(&meta.decl, &mut env)?;

        //     return Ok(None);
        // }

        let mut state = self.state.lock();
        let task = self.create_task_from_decl(code_node, meta.decl.clone(), meta.complexity as u64);
        let task_id = state.add_task(task);

        Ok(Some(task_id))
    }

    /// Обробляє результат задачі
    // fn process_task_result(&self, result: TaskResult, task_id_map: &HashMap<TaskId, NodeIndex>) {
    //     if let Some(&node_id) = task_id_map.get(&result.id) {
    //         let mut state = self.state.lock();
    //         match result.result {
    //             Ok(env) => {
    //                 state.mark_done(node_id, env);
    //                 debug!(
    //                     "Task {:?} completed in {:?}",
    //                     result.id, result.actual_duration
    //                 );
    //                 // повідомляємо scheduler
    //                 self.scheduler
    //                     .task_completed(result.worker_id, result.actual_duration, {
    //                         if let Some(task) = state.get_task(node_id) {
    //                             task.complexity
    //                         } else {
    //                             0
    //                         }
    //                     });
    //             }
    //             Err(err) => {
    //                 state.mark_failed(node_id);
    //                 error!("Task {:?} failed: {}", result.id, err);
    //             }
    //         }
    //     }
    // }

    /// Основний цикл виконання
    pub fn run(&self) -> Result<(), String> {
        Ok(())
    }

    /// Запуск з автоматичним shutdown
    pub fn execute(self) -> Result<(), String> {
        let result = self.run();
        self.pool.shutdown();
        result
    }

    // Публічні методи для доступу до стану

    pub fn get_result(&self, node_id: NodeIndex) -> Option<Env> {
        let state = self.state.lock();
        state.get_result(node_id)
    }

    pub fn get_nested_results(&self, parent_id: NodeIndex) -> Vec<Env> {
        let state = self.state.lock();
        state.get_nested_results(parent_id)
    }

    pub fn get_status(&self, node_id: NodeIndex) -> Option<Status> {
        let state = self.state.lock();
        state.get_status(node_id)
    }

    pub fn find_tasks_by_code_node(&self, code_node: NodeIndex) -> Vec<NodeIndex> {
        let state = self.state.lock();
        state.find_tasks_by_code_node(code_node)
    }

    pub fn get_dependencies(&self, node_id: NodeIndex) -> Vec<NodeIndex> {
        let state = self.state.lock();
        state.get_dependencies(node_id)
    }

    pub fn get_dependents(&self, node_id: NodeIndex) -> Vec<NodeIndex> {
        let state = self.state.lock();
        state.get_dependents(node_id)
    }

    pub fn all_subtasks_done(&self, parent_id: NodeIndex) -> bool {
        let state = self.state.lock();
        state.all_subtasks_done(parent_id)
    }
}

// ====================================================================
//                       PUBLIC API
// ====================================================================

pub fn execute_plan(graph: DiGraph<DeclarationMeta, ()>) -> Result<(), String> {
    Executor::new(graph).run()
}
