use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use log::debug;
use petgraph::graph::NodeIndex;

use crate::{
    execution::execute_stmt,
    expr::Expr,
    scope::{env_add_scope, env_create_scope, env_remove_scope, Env},
    stmt::Decl,
};

#[derive(Clone)]
pub struct WorkerTask {
    pub code_graph_id: NodeIndex,
    pub id: NodeIndex,
    pub work: WorkType,
    pub env: Env,
    pub complexity: u64,
    pub estimated_duration: Option<Duration>,
}

#[derive(Clone)]
pub enum WorkType {
    ExecuteNode(Arc<Decl>),
    ExecuteIteration {
        var: String,
        element: Expr,
        body: Arc<Decl>,
    },
}

pub struct TaskResult {
    pub id: NodeIndex,
    pub result: Result<Env, String>,
    pub actual_duration: Duration,
    pub worker_id: usize, // Додаємо це поле
}

// ====================================================================
//                       WORKER POOL
// ====================================================================

pub struct WorkerPool {
    workers: Vec<Sender<Option<WorkerTask>>>,
    // pub result_rx: Receiver<TaskResult>,
    active_tasks: Arc<AtomicUsize>,
    num_workers: usize,
}

impl WorkerPool {
    pub fn new(num_workers: usize) -> (Self, Receiver<TaskResult>) {
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
                    // dbg!(task.id);
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

        (
            WorkerPool {
                workers,
                // result_rx,
                active_tasks,
                num_workers,
            },
            result_rx,
        )
    }

    pub fn execute_task(work: WorkType, mut env: Env) -> Result<Env, String> {
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

    pub fn submit(&self, task: WorkerTask, worker_id: usize) -> Result<(), String> {
        self.active_tasks.fetch_add(1, Ordering::Release);

        self.workers[worker_id].send(Some(task)).map_err(|e| {
            self.active_tasks.fetch_sub(1, Ordering::Release);
            format!("Failed to submit task: {}", e)
        })
    }

    pub fn has_active_tasks(&self) -> bool {
        self.active_tasks.load(Ordering::Acquire) > 0
    }

    pub fn shutdown(self) {
        for worker in self.workers {
            let _ = worker.send(None);
        }
    }
}
