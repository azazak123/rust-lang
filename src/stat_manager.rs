use std::{
    collections::HashMap,
    sync::{
        mpsc::{self, channel, Receiver, RecvTimeoutError, Sender},
        OnceLock,
    },
    thread,
    time::Duration,
};

use crate::{expr::Expr, scope::Env};

static STAT_MANAGER_TX: OnceLock<Sender<MessageType>> = OnceLock::new();
// static CU_ESTIMATOR: RwLock<Option<CuEstimator>> = RwLock::const_new(None);

fn init_channel() -> Receiver<MessageType> {
    let (tx, rx) = mpsc::channel();
    STAT_MANAGER_TX.set(tx).ok().unwrap();
    rx
}

pub enum MessageType {
    AddEntries((usize, Env, Duration)),
}

pub struct StatManager {
    stats: HashMap<usize, (Vec<Env>, Vec<Duration>)>,
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

    pub fn send_data(n: usize, env: Env, time: Duration) {
        if let Some(tx) = STAT_MANAGER_TX.get() {
            tx.send(MessageType::AddEntries((n, env, time))).unwrap();
        }
    }

    fn inner(&mut self) {
        loop {
            match self.rx.recv_timeout(Duration::from_millis(1000)) {
                Ok(MessageType::AddEntries((n, env, time))) => {
                    let e = self.stats.entry(n).or_insert((vec![], vec![]));
                    e.0.push(env);
                    e.1.push(time);
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.process();
                    println!("ogo")
                }
                Err(RecvTimeoutError::Disconnected) => {
                    eprintln!("channel's sending half has become disconnected!");
                    break;
                }
            }
        }
    }

    fn process(&mut self) {}
}
