use std::{
    collections::HashMap,
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
        LazyLock, OnceLock,
    },
    thread,
    time::Duration,
};

use parking_lot::RwLock;

use crate::{expr::Expr, scope::Scope};

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
        match self {
            Model::LGBM(Booster(model)) => {
                model.predict(args, args.len() as i32, true).unwrap()[0] as f32
            }
            Model::L2(ridge_regression) => ridge_regression
                .predict(&DenseMatrix::from_2d_array(&[args]).unwrap())
                .unwrap()[0] as f32,
        }
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
    AddEntries((usize, Scope, Duration)),
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

    pub fn send_data(n: usize, scope: Scope, time: Duration) {
        if let Some(tx) = STAT_MANAGER_TX.get() {
            tx.send(MessageType::AddEntries((n, scope, time))).unwrap();
        }
    }

    fn inner(&mut self) {
        loop {
            match self.rx.recv_timeout(Duration::from_millis(500)) {
                Ok(MessageType::AddEntries((n, env, time))) => {
                    let e = self.stats.entry(n).or_insert((vec![], vec![]));

                    let v = env
                        .into_iter()
                        .filter_map(|(_, v)| match v {
                            Expr::Number(n) => Some(n),
                            Expr::Bool(b) => Some(b as u8 as f64),
                            _ => None,
                        })
                        .collect();

                    e.0.push(v);

                    e.1.push(time.as_millis() as f32);
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

        for (k, v) in &self.stats {
            if v.0.is_empty() {
                continue;
            }
            dbg!(v.0.len(), k);
            if v.0.len() > 100 {
                let dataset = Dataset::from_vec_of_vec(v.0.clone(), v.1.clone(), true).unwrap();
                let params = json! {
                    {
                        "num_iterations":10,
                        "verbose":0,
                        "learning_rate": 0.5,
                        "objective": "regression"
                    }
                };
                let bst = lightgbm3::Booster::train(dataset, &params).unwrap();
                let mut models = MODELS.write();
                models.insert(*k, Model::LGBM(Booster(bst)));
            } else if v.0.len() > v.0[0].len() {
                let m = DenseMatrix::from_2d_vec(&v.0).unwrap();
                let model = RidgeRegression::fit(
                    &m,
                    &v.1,
                    RidgeRegressionParameters::default()
                        .with_normalize(false)
                        .with_alpha(0.7),
                )
                .unwrap();
                let mut models = MODELS.write();
                models.insert(*k, Model::L2(model));
            }
        }
    }

    pub fn predict(id: usize, env: &Scope) -> Option<f32> {
        let v = env
            .into_iter()
            .filter_map(|(_, v)| match v {
                Expr::Number(n) => Some(*n),
                Expr::Bool(b) => Some(*b as u8 as f64),
                _ => None,
            })
            .collect::<Vec<_>>();

        let models = MODELS.read();
        let model = models.get(&id)?;

        Some(model.predict(&v))
    }
}
