use std::sync::Arc;

use coarsetime::Duration;

use parking_lot::RwLock;
use petgraph::graph::NodeIndex;

use crate::{
    task_graph::{Task, TaskGraph},
    ARGS,
};

#[derive(Clone, Debug)]
struct WorkerLoad {
    assigned_time: Duration,
    assigned_complexity: usize,
    tasks_count: usize,
}

impl WorkerLoad {
    fn new() -> Self {
        WorkerLoad {
            assigned_time: Duration::from_days(0),
            assigned_complexity: 0,
            tasks_count: 0,
        }
    }

    fn add_task(&mut self, complexity: usize, estimated_duration: Option<Duration>) {
        if let Some(estimated_duration) = estimated_duration {
            self.assigned_time += estimated_duration;
        }

        self.assigned_complexity += complexity;
        self.tasks_count += 1;
    }

    fn complete_task(&mut self, actual_duration: Option<Duration>, complexity: usize) {
        self.assigned_complexity = self.assigned_complexity.saturating_sub(complexity);
        self.tasks_count = self.tasks_count.saturating_sub(1);

        if let Some(actual_duration) = actual_duration {
            self.assigned_time.saturating_sub(actual_duration);
        }
    }
}

pub struct Scheduler {
    worker_loads: Arc<RwLock<Vec<WorkerLoad>>>,
    num_workers: usize,
    last_used: usize,
}

impl Scheduler {
    pub fn new(num_workers: usize) -> Self {
        Scheduler {
            worker_loads: Arc::new(RwLock::new(vec![WorkerLoad::new(); num_workers])),
            num_workers,
            last_used: 0,
        }
    }

    pub fn schedule(&mut self, task_graph: &TaskGraph) -> Vec<Vec<NodeIndex>> {
        let mut scheduled_tasks = vec![vec![]; self.num_workers];

        let mut ready_tasks = task_graph.get_ready_tasks();

        let use_complexity = false;

        let policy = SchedulingPolicy::LeastLoaded(if use_complexity {
            Load::Complexity
        } else {
            Load::Time
        });

        ready_tasks.sort_unstable_by(|(_, x1), (_, x2)| {
            x2.get_estimated_duration()
                .unwrap()
                .cmp(&x1.get_estimated_duration().unwrap())
        });

        ready_tasks.truncate(ARGS.schedule_task_limit);

        for (id, task) in ready_tasks {
            let worker_id = self.select_worker_with_policy(policy);

            self.schedule_task(&task, worker_id);
            scheduled_tasks[worker_id].push(id);
        }

        scheduled_tasks
    }

    /// Планує задачу на воркер і оновлює статистику
    fn schedule_task(&self, task: &Task, worker_id: usize) {
        let mut loads = self.worker_loads.write();
        loads[worker_id].add_task(task.get_complexity(), task.get_estimated_duration());

        // debug!(
        //     "Scheduled task {:?} (complexity={}, estimated={:?}) to worker {} (will be free in {:?})",
        //     task.id,
        //     task.complexity,
        //     task.estimated_duration,
        //     worker_id,
        //     loads[worker_id].time_until_free()
        // );
    }

    /// Повідомляє scheduler про завершення задачі
    pub fn task_completed(
        &self,
        worker_id: usize,
        actual_duration: Option<Duration>,
        complexity: usize,
    ) {
        let mut loads = self.worker_loads.write();
        if worker_id < loads.len() {
            loads[worker_id].complete_task(actual_duration, complexity);
        }
    }

    /// Знаходить найменш завантажений воркер
    fn find_least_loaded_worker(&self, load_type: Load) -> usize {
        let loads = self.worker_loads.read();
        loads
            .iter()
            .enumerate()
            .min_by_key(|(_, load)| match load_type {
                Load::Complexity => load.assigned_complexity,
                Load::Time => load.assigned_time.as_ticks() as usize,
            })
            .map(|(id, _)| id)
            .unwrap_or(0)
    }

    /// Знаходит воркер з найменшою кількістю задач
    fn find_worker_with_fewest_tasks(&self) -> usize {
        let loads = self.worker_loads.read();
        loads
            .iter()
            .enumerate()
            .min_by_key(|(_, load)| load.tasks_count)
            .map(|(id, _)| id)
            .unwrap_or(0)
    }

    #[allow(dead_code)]
    fn reset(&self) {
        let mut loads = self.worker_loads.write();
        for load in loads.iter_mut() {
            *load = WorkerLoad::new();
        }
    }

    /// Стратегія вибору воркера з різними політиками
    fn select_worker_with_policy(&mut self, policy: SchedulingPolicy) -> usize {
        match policy {
            SchedulingPolicy::LeastLoaded(load_type) => self.find_least_loaded_worker(load_type),
            SchedulingPolicy::FewestTasks => self.find_worker_with_fewest_tasks(),
            SchedulingPolicy::RoundRobin => {
                // Простий round-robin
                let next = self.last_used + 1 % self.num_workers;
                self.last_used = next;
                next
            }
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
enum SchedulingPolicy {
    LeastLoaded(Load), // Вибирає воркер з найменшим predicted_finish_time
    FewestTasks,       // Вибирає воркер з найменшою кількістю задач
    RoundRobin,        // По черзі
                       // WorkStealing, // З крадіжкою роботи
}

#[derive(Debug, Clone, Copy)]
enum Load {
    Time,
    Complexity,
}
