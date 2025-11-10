use std::{
    sync::{atomic::AtomicUsize, Arc},
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use petgraph::graph::NodeIndex;

use crate::task_graph::{self, Task, TaskGraph};

const COMPLEXITY_MULTIPLIER: usize = 100; // мікросекунди на одиницю складності

#[derive(Clone)]
struct WorkerLoad {
    predicted_finish_time: Instant,
    assigned_complexity: usize,
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

    fn add_task(&mut self, complexity: usize, estimated_duration: Option<Duration>) {
        let task_duration = estimated_duration
            .unwrap_or_else(|| Duration::from_micros((complexity * COMPLEXITY_MULTIPLIER) as u64));

        let now = Instant::now();
        if self.predicted_finish_time > now {
            self.predicted_finish_time += task_duration;
        } else {
            self.predicted_finish_time = now + task_duration;
        }

        self.assigned_complexity += complexity;
        self.tasks_count += 1;
    }

    fn complete_task(&mut self, actual_duration: Duration, complexity: usize) {
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

pub struct Scheduler {
    worker_loads: Arc<Mutex<Vec<WorkerLoad>>>,
    num_workers: usize,
    last_used: usize,
}

impl Scheduler {
    pub fn new(num_workers: usize) -> Self {
        Scheduler {
            worker_loads: Arc::new(Mutex::new(vec![WorkerLoad::new(); num_workers])),
            num_workers,
            last_used: 0,
        }
    }

    pub fn schedule(&mut self, task_graph: &TaskGraph) -> Vec<Vec<NodeIndex>> {
        let mut scheduled_tasks = vec![vec![]; self.num_workers];

        // dbg!(task_graph.get_ready_tasks());

        for (id, task) in task_graph.get_ready_tasks() {
            let worker_id = self.select_worker_with_policy(SchedulingPolicy::LeastLoaded);

            self.schedule_task(&task, worker_id);
            scheduled_tasks[worker_id].push(id);
        }

        scheduled_tasks
    }

    /// Планує задачу на воркер і оновлює статистику
    fn schedule_task(&self, task: &Task, worker_id: usize) {
        let mut loads = self.worker_loads.lock();
        loads[worker_id].add_task(task.complexity, task.estimated_duration);

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
    pub fn task_completed(&self, worker_id: usize, actual_duration: Duration, complexity: usize) {
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
    fn get_worker_stats(&self) -> Vec<(usize, Duration, usize, usize)> {
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
                let next = self.last_used + 1 % self.num_workers;
                self.last_used = next;
                next
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SchedulingPolicy {
    LeastLoaded, // Вибирає воркер з найменшим predicted_finish_time
    FewestTasks, // Вибирає воркер з найменшою кількістю задач
    RoundRobin,  // По черзі
                 // WorkStealing, // З крадіжкою роботи
}
