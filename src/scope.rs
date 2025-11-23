use std::collections::{BTreeMap, HashMap};

use nohash_hasher::{BuildNoHashHasher, IntMap};
// use rustc_hash::FxBuildHasher;
// use rustc_hash::FxHashMap as HashMap;
use ustr::Ustr;

use crate::expr::Expr;

// --- ЗМІНИ ТУТ ---
// Ми замінюємо Vec<Scope> на структуру.
// Clone потрібен, бо він використовується в create_env/merge_scope
#[derive(Clone, Debug, Default)]
pub struct Env {
    // Головна оптимізація: одне швидке звернення до мапи замість циклу.
    // Вектор діє як стек: останній елемент - це актуальне значення.
    values: IntMap<u64, Vec<Expr>>,

    // Історія змінних по скоупах. Потрібна, щоб знати,
    // які змінні видаляти при env_remove_scope та для merge_scope.
    scopes_history: Vec<Vec<u64>>,
}

// Scope залишаємо для сумісності сигнатур (використовується при додаванні/злитті)
pub type Scope = IntMap<u64, Expr>;

pub fn env_empty() -> Env {
    Env {
        values: HashMap::with_capacity_and_hasher(50, BuildNoHashHasher::default()),
        scopes_history: Vec::with_capacity(10),
    }
}

// --- НАЙВАЖЛИВІША ФУНКЦІЯ (HOT PATH) ---
pub fn env_lookup(env: &Env, name: &Ustr) -> Option<Expr> {
    env.values
        .get(&name.precomputed_hash())
        .and_then(|stack| stack.last().cloned())
}

pub fn env_add_scope(env: &mut Env, scope: Scope) {
    let mut current_scope_vars = Vec::with_capacity(scope.len());

    for (name, val) in scope {
        // Додаємо значення на вершину стеку для цієї змінної
        env.values.entry(name).or_default().push(val);

        // Запам'ятовуємо, що ця змінна була додана в поточному скоупі
        current_scope_vars.push(name);
    }

    env.scopes_history.push(current_scope_vars);
}

pub fn env_remove_scope(env: &mut Env) {
    if let Some(vars_to_remove) = env.scopes_history.pop() {
        for name in vars_to_remove {
            if let Some(stack) = env.values.get_mut(&name) {
                stack.pop();
                // Опціонально: чистимо ключ, якщо стек пустий, щоб економити пам'ять,
                // але для швидкості можна залишити пустий вектор.
                // if stack.is_empty() {
                //     env.values.remove(&name);
                // }
            }
        }
    }
}

pub fn env_declare(env: &mut Env, name: &Ustr, val: Expr) {
    // Якщо скоупів ще немає, створюємо кореневий
    if env.scopes_history.is_empty() {
        env.scopes_history.push(Vec::new());
    }

    // Додаємо значення
    env.values
        .entry(name.precomputed_hash())
        .or_default()
        .push(val);

    // Реєструємо змінну в поточному (останньому) скоупі
    if let Some(last_scope) = env.scopes_history.last_mut() {
        last_scope.push(name.precomputed_hash());
    }
}

pub fn env_assign(env: &mut Env, name: &Ustr, val: Expr) -> bool {
    // Шукаємо стек значень для змінної
    if let Some(stack) = env.values.get_mut(&name.precomputed_hash()) {
        if let Some(last) = stack.last_mut() {
            *last = val;
            return true;
        }
    }
    false
}

// Ця функція стала складнішою, бо нам треба емулювати поведінку "пошарового" злиття,
// маючи на руках "пласку" структуру.
// Але оскільки merge викликається рідше ніж lookup, це вигідний обмін.
pub fn merge_scope((id1, x): (usize, &Env), (id2, y): (usize, &Env)) -> (usize, Env) {
    let max_len = x.scopes_history.len().max(y.scopes_history.len());
    let mut merged_env = env_empty();

    // Нам потрібні лічильники, щоб знати, який саме по рахунку екземпляр змінної
    // брати зі стеку values (адже values містить всі версії змінної).
    let mut x_counters: HashMap<u64, usize, BuildNoHashHasher<u64>> =
        HashMap::with_hasher(BuildNoHashHasher::default());
    let mut y_counters: HashMap<u64, usize, BuildNoHashHasher<u64>> = HashMap::default();

    for i in 0..max_len {
        let mut map = env_create_scope();

        // --- Відновлення шару i для X ---
        if let Some(vars_in_scope) = x.scopes_history.get(i) {
            for name in vars_in_scope {
                let idx = *x_counters.entry(*name).or_insert(0);
                if let Some(val) = x.values.get(name).and_then(|v| v.get(idx)) {
                    map.insert(*name, val.clone());
                }
                // Зсуваємо лічильник, щоб наступного разу взяти наступну версію цієї змінної
                *x_counters.get_mut(name).unwrap() += 1;
            }
        }

        // --- Відновлення шару i для Y та злиття ---
        if let Some(vars_in_scope) = y.scopes_history.get(i) {
            for name in vars_in_scope {
                let idx = *y_counters.entry(*name).or_insert(0);
                // Отримуємо значення з Y
                if let Some(val_y) = y.values.get(name).and_then(|v| v.get(idx)) {
                    match map.get_mut(name) {
                        Some(existing) => {
                            // Логіка пріоритету з оригінальної функції
                            if id2 > id1 {
                                *existing = val_y.clone();
                            }
                        }
                        None => {
                            map.insert(*name, val_y.clone());
                        }
                    }
                }
                *y_counters.get_mut(name).unwrap() += 1;
            }
        }

        // Додаємо відновлений та злитий шар у нове середовище
        env_add_scope(&mut merged_env, map);
    }

    (id1.max(id2), merged_env)
}

pub fn env_create_scope() -> Scope {
    // with_capacity_and_hasher тепер приймає BuildNoHashHasher
    HashMap::with_capacity_and_hasher(5, BuildNoHashHasher::default())
}

pub fn env_get_all_visible(env: &Env) -> BTreeMap<u64, Expr> {
    let mut visible_vars = BTreeMap::new();

    for (name, stack) in &env.values {
        if let Some(val) = stack.last() {
            // ОПТИМІЗАЦІЯ: Не клонуйте складні структури (String, List, Object),
            // якщо StatManager все одно їх відфільтрує.
            match val {
                Expr::Number(_) | Expr::Bool(_) => {
                    visible_vars.insert(*name, val.clone());
                }
                // Якщо StatManager ігнорує інші типи, не додавайте їх сюди взагалі.
                // Це зекономить пам'ять і час на clone().
                _ => {}
            }
        }
    }
    visible_vars
}

// use crate::expr::Expr;
// use nohash_hasher::IntMap;
// use ustr::Ustr;

// const SMALL_SIZE: usize = 16;

// #[derive(Clone, Debug)]
// pub enum Scope {
//     // Малий розмір: вектор пар (Key=usize, Value)
//     // usize тут — це адреса вказівника ustr
//     Small(Vec<(u64, Expr)>),

//     // Великий розмір: спеціалізована IntMap
//     // Вона працює з u64, тому нам доведеться кастити usize -> u64
//     Big(IntMap<u64, Expr>),
// }

// // Env залишається тим самим
// pub type Env = Vec<Scope>;

// // Отримуємо ID як usize (адреса пам'яті)
// // #[inline(always)]
// // fn get_key(name: &Ustr) -> usize {
// //     name.precomputed_hash()
// //     name.as_char_ptr() as usize
// // }

// impl Scope {
//     pub fn new() -> Self {
//         Scope::Small(Vec::with_capacity(4))
//     }

//     #[inline(always)]
//     pub fn get(&self, key: u64) -> Option<&Expr> {
//         match self {
//             Scope::Small(vec) => {
//                 // Лінійний пошук (SIMD-friendly)
//                 for (k, v) in vec {
//                     if *k == key {
//                         return Some(v);
//                     }
//                 }
//                 None
//             }
//             Scope::Big(map) => {
//                 // IntMap вимагає u64. На 64-бітних системах це безкоштовно.
//                 map.get(&key)
//             }
//         }
//     }

//     #[inline(always)]
//     pub fn get_mut(&mut self, key: u64) -> Option<&mut Expr> {
//         match self {
//             Scope::Small(vec) => {
//                 for (k, v) in vec {
//                     if *k == key {
//                         return Some(v);
//                     }
//                 }
//                 None
//             }
//             Scope::Big(map) => map.get_mut(&key),
//         }
//     }

//     pub fn insert(&mut self, key: u64, val: Expr) {
//         match self {
//             Scope::Small(vec) => {
//                 // 1. Спроба перезапису
//                 for (k, v) in vec.iter_mut() {
//                     if *k == key {
//                         *v = val;
//                         return;
//                     }
//                 }

//                 // 2. Вставка або Апгрейд
//                 if vec.len() < SMALL_SIZE {
//                     vec.push((key, val));
//                 } else {
//                     // --- ПЕРЕХІД НА INTMAP ---
//                     let mut map = IntMap::default();

//                     // Переносимо старі дані (кастимо usize -> u64)
//                     for (k, v) in vec.drain(..) {
//                         map.insert(k as u64, v);
//                     }
//                     // Додаємо нове
//                     map.insert(key as u64, val);

//                     *self = Scope::Big(map);
//                 }
//             }
//             Scope::Big(map) => {
//                 map.insert(key as u64, val);
//             }
//         }
//     }

//     // Уніфікований ітератор, який повертає (usize, &Expr)
//     // Це потрібно для merge_scope, щоб не думати про типи ключів
//     pub fn iter(&self) -> impl Iterator<Item = (u64, &Expr)> {
//         match self {
//             Scope::Small(vec) => EitherIter::Left(vec.iter().map(|(k, v)| (*k, v))),
//             Scope::Big(map) => EitherIter::Right(
//                 // IntMap повертає (&u64, &V), кастимо назад в usize
//                 map.iter().map(|(k, v)| (*k, v)),
//             ),
//         }
//     }
// }

// // --- Helper Iterator Boilerplate ---
// // Щоб не алокувати Box<dyn Iterator>
// enum EitherIter<L, R> {
//     Left(L),
//     Right(R),
// }

// impl<L, R, Item> Iterator for EitherIter<L, R>
// where
//     L: Iterator<Item = Item>,
//     R: Iterator<Item = Item>,
// {
//     type Item = Item;
//     fn next(&mut self) -> Option<Self::Item> {
//         match self {
//             EitherIter::Left(l) => l.next(),
//             EitherIter::Right(r) => r.next(),
//         }
//     }
// }

// // --- GLOBAL API (майже без змін) ---

// pub fn env_lookup(env: &Env, name: &Ustr) -> Option<Expr> {
//     let key = name.precomputed_hash();
//     for scope in env.iter().rev() {
//         if let Some(val) = scope.get(key) {
//             return Some(val.clone());
//         }
//     }
//     None
// }

// pub fn env_declare(env: &mut Env, name: &Ustr, val: Expr) {
//     if env.is_empty() {
//         env.push(Scope::new());
//     }
//     let last_index = env.len() - 1;
//     env[last_index].insert(name.precomputed_hash(), val);
// }

// pub fn env_assign(env: &mut Env, name: &Ustr, val: Expr) -> bool {
//     let key = name.precomputed_hash();
//     for scope in env.iter_mut().rev() {
//         if let Some(v) = scope.get_mut(key) {
//             *v = val;
//             return true;
//         }
//     }
//     false
// }

// pub fn env_add_scope(env: &mut Env, scope: Scope) {
//     env.push(scope);
// }

// pub fn env_remove_scope(env: &mut Env) {
//     if !env.is_empty() {
//         env.pop();
//     }
// }

// // Функцію merge_scope треба трохи адаптувати під ітератор
// pub fn merge_scope((id1, x): (usize, &Env), (id2, y): (usize, &Env)) -> (usize, Env) {
//     let max_len = x.len().max(y.len());
//     let mut merged = Vec::with_capacity(max_len);

//     for i in 0..max_len {
//         let mut new_scope = env_create_scope();
//         let scope_x = x.get(i);
//         let scope_y = y.get(i);

//         if let Some(sx) = scope_x {
//             for (k, v) in sx.iter() {
//                 new_scope.insert(k, v.clone());
//             }
//         }

//         if let Some(sy) = scope_y {
//             for (k, v) in sy.iter() {
//                 // Тут ми використовуємо get_mut самої Scope, яка сховає деталі реалізації
//                 match new_scope.get_mut(k) {
//                     Some(existing) => {
//                         if id2 > id1 {
//                             *existing = v.clone();
//                         }
//                     }
//                     None => {
//                         new_scope.insert(k, v.clone());
//                     }
//                 }
//             }
//         }
//         merged.push(new_scope);
//     }

//     (id1.max(id2), merged)
// }

// // create_env залишається без змін, бо використовує merge_scope
// pub fn create_env(dep_results: Vec<Env>, dep_ids: Vec<usize>) -> Env {
//     fn acc_ref(acc: &(usize, Env)) -> (usize, &Env) {
//         (acc.0, &acc.1)
//     }
//     if dep_results.is_empty() {
//         return vec![];
//     }
//     let iter = dep_ids.into_iter().zip(dep_results.into_iter());
//     iter.reduce(|acc, next| merge_scope(acc_ref(&acc), acc_ref(&next)))
//         .map(|(_, r)| r)
//         .unwrap_or(vec![])
// }

// pub fn env_create_scope() -> Scope {
//     Scope::new()
// }

// pub fn env_get_all_visible(env: &Env) -> Scope {
//     // Починаємо з малого вектора (Scope::Small)
//     let mut visible_vars = Scope::new();

//     // Ітеруємось від найстарішого скоупу до найновішого (від 0 до кінця).
//     // Це важливо: значення з новіших скоупів будуть перезаписувати (shadow)
//     // значення зі старих скоупів, якщо ключі (адреси змінних) збігаються.
//     for scope in env {
//         // scope.iter() повертає (usize, &Expr), де usize - це ID/адреса
//         for (key, val) in scope.iter() {
//             visible_vars.insert(key, val.clone());
//         }
//     }

//     visible_vars
// }

// pub fn env_empty() -> Env {
//     // Резервуємо місце під 16 вкладених скоупів.
//     // Це покриває більшість реальних сценаріїв без необхідності
//     // розширювати вектор (reallocation) під час виконання.
//     Vec::with_capacity(16)
// }
