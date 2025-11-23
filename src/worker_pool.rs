use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    thread,
};

use coarsetime::Duration;

use log::debug;
use parking_lot::RwLock;
use petgraph::graph::{DiGraph, NodeIndex};
use rustc_hash::FxHashMap as HashMap;

use crate::{
    declaration_meta::{Class, DeclarationMeta},
    execution::execute_stmt,
    expr::Expr,
    parallel_execution::Executor,
    scope::{env_declare, env_get_all_visible, Env},
    stat_manager::StatManager,
    stmt::{Decl, DeclType, Stmt},
    task_graph::{Task, TaskGraph},
    ARGS,
};

#[derive(Clone)]
pub struct WorkerTask {
    pub code_graph_id: NodeIndex,
    pub id: NodeIndex,
    pub work: WorkType,
    pub env: Env,
    pub complexity: usize,
    pub estimated_duration: Option<Duration>,
}

#[derive(Clone)]
pub enum WorkType {
    ExecuteNode {
        code_to_task: HashMap<NodeIndex, NodeIndex>,
        task_to_code: HashMap<NodeIndex, NodeIndex>,
        body: Arc<Decl>,
        meta: DeclarationMeta,
    },
    MaybeExpand {
        code_to_task: HashMap<NodeIndex, NodeIndex>,
        task_to_code: HashMap<NodeIndex, NodeIndex>,
        body: Arc<Decl>,
        meta: DeclarationMeta,
    },
}

pub struct TaskResult {
    pub id: NodeIndex,
    pub result: Result<Env, String>,
    pub actual_duration: Option<Duration>,
    pub worker_id: usize, // Додаємо це поле
    pub complexity: usize,
}

// ====================================================================
//                       WORKER POOL
// ====================================================================

pub struct WorkerPool {
    workers: Vec<Sender<Option<WorkerTask>>>,
    active_tasks: Arc<AtomicUsize>,
}

impl WorkerPool {
    pub fn new(
        num_workers: usize,
        state: &Arc<RwLock<TaskGraph>>,
        code_graph: DiGraph<DeclarationMeta, ()>,
    ) -> (Self, Receiver<TaskResult>) {
        let (result_tx, result_rx) = channel();
        let active_tasks = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();

        for worker_id in 0..num_workers {
            let (task_tx, task_rx) = channel::<Option<WorkerTask>>();
            let result_tx = result_tx.clone();
            let active = active_tasks.clone();
            let state = Arc::clone(state);
            let code_graph = code_graph.clone();

            thread::spawn(move || {
                debug!("Worker {} started", worker_id);
                while let Ok(Some(task)) = task_rx.recv() {
                    let result = Self::execute_task(
                        task.work,
                        task.env,
                        task.code_graph_id,
                        &code_graph,
                        state.clone(),
                    );

                    result_tx
                        .send(TaskResult {
                            id: task.id,
                            result,
                            actual_duration: task.estimated_duration,
                            worker_id,
                            complexity: task.complexity,
                        })
                        .unwrap();

                    active.fetch_sub(1, Ordering::Release);
                }
                debug!("Worker {} stopped", worker_id);
            });

            workers.push(task_tx);
        }

        (
            WorkerPool {
                workers,
                active_tasks,
            },
            result_rx,
        )
    }

    pub fn execute_task(
        work: WorkType,
        mut env: Env,
        node_id: NodeIndex,
        code_graph: &DiGraph<DeclarationMeta, ()>,
        state: Arc<RwLock<TaskGraph>>,
    ) -> Result<Env, String> {
        match work {
            WorkType::ExecuteNode {
                code_to_task,
                task_to_code,
                body,
                meta,
            } => {
                let estinated_time =
                    StatManager::predict(node_id.index(), &env_get_all_visible(&env));

                if !matches!(meta.class, Class::Block | Class::Loop)
                    || !Executor::is_parallel_execution(estinated_time, meta.depth)
                {
                    execute_stmt::<true>(&body, &mut env).unwrap();
                } else {
                    // dbg!("par_block");
                    Executor::expand(
                        code_graph,
                        &state,
                        code_to_task,
                        task_to_code,
                        node_id,
                        &Some(env_get_all_visible(&env)),
                    );
                }

                Ok(env)
            }
            WorkType::MaybeExpand {
                code_to_task,
                task_to_code,
                body,
                meta,
            } => {
                let DeclType::Stmt(stmt) = &body.v else {
                    return Err("Body of MaybeExpand must be a statement".to_string());
                };

                let Stmt::For(loop_var, arr, _) = stmt.deref() else {
                    return Err("Body of MaybeExpand must be a for loop".to_string());
                };

                let elements = extract_array_elements(&arr.eval(&env).unwrap()).unwrap();

                let estinated_time =
                    StatManager::predict(node_id.index(), &env_get_all_visible(&env));

                if estinated_time.is_some_and(|x| {
                    (x / elements.len() as u32)
                        < Duration::from_millis(ARGS.duration_to_par_loop_iter_ms)
                }) {
                    execute_stmt::<true>(&body, &mut env).unwrap();
                } else {
                    // dbg!("par_loop");
                    for el in elements.into_iter() {
                        let mut env = env.clone();

                        let mut code_to_task = code_to_task.clone();
                        let mut task_to_code = task_to_code.clone();

                        let task = Task::ExecuteNode {
                            code_graph_id: node_id,
                            complexity: meta.complexity,
                            estimated_duration: None,
                            loo: None,
                        };

                        env_declare(&mut env, loop_var, el);

                        let all_visible = &Some(env_get_all_visible(&env));

                        let inserted = {
                            let mut state = state.write();
                            let inserted = state.add_task(task);

                            state.mark_done(inserted, env);

                            inserted
                        };

                        code_to_task.insert(node_id, inserted);
                        task_to_code.insert(inserted, node_id);

                        Executor::expand(
                            code_graph,
                            &state,
                            code_to_task,
                            task_to_code,
                            NodeIndex::new(meta.loops_decls_indexes[0]),
                            all_visible,
                        );
                    }
                }

                Ok(env)
            }
        }
    }

    pub fn submit(&self, task: WorkerTask, worker_id: usize) -> Result<(), String> {
        self.active_tasks.fetch_add(1, Ordering::Release);

        self.workers[worker_id].send(Some(task)).map_err(|e| {
            self.active_tasks.fetch_sub(1, Ordering::Release);
            format!("Failed to submit task: {}", e)
        })
    }

    pub fn shutdown(self) {
        for worker in self.workers {
            let _ = worker.send(None);
        }
    }
}

fn extract_array_elements(expr: &Expr) -> Result<Vec<Expr>, String> {
    match expr {
        Expr::Array(elements) => Ok(elements.clone()),
        _ => Err(format!("Expected array, got {:?}", expr)),
    }
}
