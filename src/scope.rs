use rustc_hash::FxHashMap as HashMap;

use crate::expr::Expr;

pub type Env = Vec<HashMap<String, Expr>>;

pub fn env_empty() -> Env {
    Vec::new()
}

pub fn env_lookup(env: &Env, name: &str) -> Option<Expr> {
    for scope in env.iter().rev() {
        if let Some(val) = scope.get(name) {
            return Some(val.clone());
        }
    }
    None
}

pub fn env_add_scope(env: &mut Env, scope: HashMap<String, Expr>) {
    env.push(scope);
}

pub fn env_remove_scope(env: &mut Env) {
    if !env.is_empty() {
        env.pop();
    }
}

pub fn env_declare(env: &mut Env, name: String, val: Expr) {
    if env.is_empty() {
        env.push(HashMap::default());
    }

    let last_index = env.len() - 1;
    env[last_index].insert(name, val);
}

pub fn env_assign(env: &mut Env, name: &str, val: Expr) -> bool {
    for scope in env.iter_mut().rev() {
        if scope.contains_key(name) {
            scope.insert(name.to_string(), val);
            return true;
        }
    }
    false
}

pub fn merge_scope((id1, x): (usize, &Env), (id2, y): (usize, &Env)) -> (usize, Env) {
    let max_len = x.len().max(y.len());
    let mut merged = Vec::with_capacity(max_len);

    for i in 0..max_len {
        let mut map = HashMap::default();
        let scope_x = x.get(i);
        let scope_y = y.get(i);

        // Злиття двох мап поелементно з пріоритетом за більшим id
        if let Some(sx) = scope_x {
            for (k, v) in sx {
                map.insert(k.clone(), v.clone());
            }
        }

        if let Some(sy) = scope_y {
            for (k, v) in sy {
                match map.get_mut(k) {
                    Some(existing) => {
                        // Якщо id2 має вищий пріоритет — перезаписуємо
                        if id2 > id1 {
                            *existing = v.clone();
                        }
                    }
                    None => {
                        map.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        merged.push(map);
    }

    (id1.max(id2), merged)
}

pub fn create_env(dep_results: Vec<Env>, dep_ids: Vec<usize>) -> Env {
    // Допоміжна функція, щоб fold не споживав попереднє значення
    fn acc_ref(acc: &(usize, Env)) -> (usize, &Env) {
        (acc.0, &acc.1)
    }

    if dep_results.is_empty() {
        return vec![];
    }

    let iter = dep_ids.into_iter().zip(dep_results.into_iter());
    let result = iter
        .reduce(|acc, next| merge_scope(acc_ref(&acc), acc_ref(&next)))
        .map(|(_, r)| r)
        .unwrap_or(vec![]);

    result
}
