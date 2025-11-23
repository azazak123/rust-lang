use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
        LazyLock, OnceLock,
    },
    thread,
};

use coarsetime::Duration;

use parking_lot::RwLock;

use crate::expr::Expr;

use smartcore::{
    self,
    linalg::basic::matrix::DenseMatrix,
    linear::ridge_regression::{RidgeRegression, RidgeRegressionParameters},
};

struct Booster(lightgbm3::Booster);

unsafe impl Send for Booster {}
unsafe impl Sync for Booster {}

enum Model {
    LGBM(Booster),
    L2(smartcore::linear::ridge_regression::RidgeRegression<f64, f32, DenseMatrix<f64>, Vec<f32>>),
}

impl Model {
    pub fn predict(&self, args: &[f64]) -> f32 {
        // Прогноз повертається в логарифмічній шкалі ln(ms + 1)
        let log_prediction = match self {
            Model::LGBM(Booster(model)) => {
                // dbg!("Using LGBM model for prediction");
                model.predict(args, args.len() as i32, true).unwrap()[0] as f32
            }
            Model::L2(ridge_regression) => {
                // dbg!("Using Ridge Regression model for prediction");
                ridge_regression
                    .predict(&DenseMatrix::from_2d_array(&[args]).unwrap())
                    .unwrap()[0] as f32
            }
        };

        // Відновлюємо час: e^y - 1
        // .max(0.0) гарантує, що ми не повернемо від'ємний час через шуми моделі
        (log_prediction.exp() - 1.0).max(0.0)
    }
}

static STAT_MANAGER_TX: OnceLock<Sender<MessageType>> = OnceLock::new();
static MODELS: RwLock<LazyLock<HashMap<usize, Model>>> =
    RwLock::new(LazyLock::new(|| HashMap::new()));

fn init_channel() -> Receiver<MessageType> {
    let (tx, rx) = mpsc::channel();
    STAT_MANAGER_TX.set(tx).ok().unwrap();
    rx
}

pub enum MessageType {
    AddEntries((usize, BTreeMap<u64, Expr>, Duration)),
}

pub struct StatManager {
    stats: HashMap<usize, (Vec<Vec<f64>>, Vec<f32>)>,
    rx: Receiver<MessageType>,
}

impl StatManager {
    pub fn new(decl_len: usize) -> Self {
        let rx = init_channel();
        StatManager {
            stats: HashMap::with_capacity(decl_len),
            rx,
        }
    }

    pub fn run(mut self) {
        thread::spawn(move || self.inner());
    }

    pub fn send_data(n: usize, scope: BTreeMap<u64, Expr>, time: Duration) {
        if let Some(tx) = STAT_MANAGER_TX.get() {
            tx.send(MessageType::AddEntries((n, scope, time))).unwrap();
        }
    }

    fn inner(&mut self) {
        loop {
            match self.rx.recv_timeout(std::time::Duration::from_millis(500)) {
                Ok(MessageType::AddEntries((n, env, time))) => {
                    let e = self.stats.entry(n).or_insert((vec![], vec![]));

                    let mut v: Vec<_> = env
                        .iter()
                        .filter_map(|(_, v)| match v {
                            Expr::Number(n) => Some(*n),
                            Expr::Bool(b) => Some(*b as u8 as f64),
                            _ => None,
                        })
                        .collect();

                    if v.is_empty() {
                        v.push(1.0);
                    }

                    e.0.push(v);

                    e.1.push(time.as_millis() as f32);

                    self.process();
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.process();
                }
                Err(RecvTimeoutError::Disconnected) => {
                    eprintln!("channel's sending half has become disconnected!");
                    break;
                }
            }
        }
    }

    fn process(&mut self) {
        use lightgbm3::Dataset;
        use serde_json::json;

        // dbg!(self.stats.keys());
        for (k, v) in &self.stats {
            if v.0.is_empty() {
                continue;
            }

            // КРОК 1: Уніфікована підготовка даних (Log1p Transform)
            // Використовуємо ln(x + 1), щоб уникнути -inf для 0ms
            let log_targets: Vec<f32> = v.1.iter().map(|&t| (t + 1.0).ln()).collect();

            // Якщо даних мало, Ridge теж має вчитися на логарифмах!
            if v.0.len() > 100 {
                let dataset = Dataset::from_vec_of_vec(v.0.clone(), log_targets, true).unwrap();
                let params = json! {
                    {
                        "objective": "regression_l2", // L2 стабільніша з логарифмом
                        "verbose": -1,
                        "learning_rate": 0.05,
                        "num_iterations": 100,
                        "max_depth": 3,
                        "num_leaves": 15,
                        "min_data_in_leaf": 2,
                        "bagging_fraction": 1.0,
                        "feature_fraction": 1.0, // Вимкніть семплінг колонок на малих даних
                    }
                };
                // Додаємо обробку помилок, щоб не падало
                if let Ok(bst) = lightgbm3::Booster::train(dataset, &params) {
                    let mut models = MODELS.write();
                    models.insert(*k, Model::LGBM(Booster(bst)));
                }
            } else if v.0.len() > v.0[0].len() {
                // Ridge Regression тепер теж вчиться на log_targets
                let m = DenseMatrix::from_2d_vec(&v.0).unwrap();
                // Alpha (регуляризація) має бути меншою для логарифмічних даних (наприклад 0.1 - 1.0)
                if let Ok(model) = RidgeRegression::fit(
                    &m,
                    &log_targets,
                    RidgeRegressionParameters::default()
                        .with_normalize(true) // Бажано нормалізувати вхідні фічі
                        .with_alpha(0.5),
                ) {
                    let mut models = MODELS.write();
                    models.insert(*k, Model::L2(model));
                }
            }
        }
    }

    pub fn predict(id: usize, env: &BTreeMap<u64, Expr>) -> Option<Duration> {
        let mut v = env
            .iter()
            .filter_map(|(_, v)| match v {
                Expr::Number(n) => Some(*n),
                Expr::Bool(b) => Some(*b as u8 as f64),
                _ => None,
            })
            .collect::<Vec<_>>();

        // ВАЖЛИВО: Повторюємо логіку з inner().
        // Якщо змінних немає, модель все одно очікує bias-терм (1.0),
        // на якому вона тренувалася.
        if v.is_empty() {
            v.push(1.0);
        }

        let models = MODELS.read();
        let model = models.get(&id)?;

        Some(Duration::from_millis(model.predict(&v) as u64))
    }
}
