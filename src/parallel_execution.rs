use parking_lot::{Condvar, Mutex};
use petgraph::{
    graph::{DiGraph, NodeIndex},
    visit::EdgeRef,
};
use rayon::prelude::*;
use rustc_hash::{FxBuildHasher, FxHashMap as HashMap};
use std::{ops::Deref, sync::Arc, thread};

use crate::execution::execute_stmt;
use crate::expr::Expr;
use crate::scope::*;
use crate::stmt::{DeclType, Stmt};
use crate::{declaration_meta::DeclarationMeta, stmt::Decl};

/// Паралельне виконання графа задач із залежностями (оптимізовано)
pub fn execute_plan(
    order: &[usize],
    graph: &DiGraph<DeclarationMeta, ()>,
) -> Result<Vec<Env>, String> {
    let results: Arc<parking_lot::lock_api::Mutex<parking_lot::RawMutex, HashMap<usize, Env>>> =
        Arc::new(Mutex::new(HashMap::with_capacity_and_hasher(
            10,
            FxBuildHasher,
        )));
    let cv = Arc::new(Condvar::new());

    thread::scope(|s| {
        for &task_id in order {
            let DeclarationMeta {
                // index: _,
                complexity: _,
                mut_deps: deps_meta,
                class: _,
                decl,
                loops_decls_indexes: _,
            } = &graph[NodeIndex::new(task_id)];

            let results_clone: Arc<
                parking_lot::lock_api::Mutex<parking_lot::RawMutex, HashMap<usize, Env>>,
            > = Arc::clone(&results);
            let cv_clone = Arc::clone(&cv);

            s.spawn(move || {
                let node_idx = NodeIndex::new(task_id);

                // Знайти всі залежності
                let dep_ids: Vec<usize> = graph
                    .edges_directed(node_idx, petgraph::Direction::Incoming)
                    .map(|e| e.source().index())
                    .collect();

                // Очікуємо на залежності
                let dep_results: Vec<Env> = dep_ids
                    .iter()
                    .map(|&dep_id| loop {
                        let mut guard = results_clone.lock();
                        if let Some(env) = guard.get(&dep_id) {
                            return env.clone();
                        }
                        cv_clone.wait(&mut guard);
                    })
                    .collect();

                // Створюємо середовище
                let mut env = create_env(dep_results, dep_ids);

                // Виконання завдання
                let res_env = if deps_meta.is_empty() {
                    match &decl.v {
                        DeclType::Stmt(stmt) => match stmt.deref() {
                            Stmt::For(var, arr_expr, body) => {
                                parallel_for_optimized(var.clone(), arr_expr, body, &env)
                            }
                            _ => execute_stmt(decl, &mut env)
                                .map_err(|e| format!("Execution error: {}", e))
                                .map(|_| env),
                        },
                        _ => execute_stmt(decl, &mut env)
                            .map_err(|e| format!("Execution error: {}", e))
                            .map(|_| env),
                    }
                } else {
                    execute_stmt(decl, &mut env)
                        .map_err(|e| format!("Execution error: {}", e))
                        .map(|_| env)
                };

                if let Ok(final_env) = res_env {
                    results_clone.lock().insert(task_id, final_env);
                    cv_clone.notify_all();
                }
            });
        }

        Result::<(), String>::Ok(())
    })?;

    let final_results = results.lock();
    Ok(order
        .iter()
        .filter_map(|&id| final_results.get(&id).cloned())
        .collect())
}

/// Оптимізована паралельна версія For
fn parallel_for_optimized(
    var: String,
    arr_expr: &Expr,
    block: &Arc<Decl>,
    env: &Env,
) -> Result<Env, String> {
    if let Some(Expr::Array(arr)) = arr_expr.eval(&env) {
        if arr.is_empty() {
            return Ok(env.clone());
        }

        let results: Vec<Result<Env, String>> = arr
            .par_iter()
            .map(|el| {
                let mut local_env = env.clone();
                let mut scope = env_create_scope();
                scope.insert(var.clone(), el.clone());
                env_add_scope(&mut local_env, scope);

                let _ = execute_stmt(block, &mut local_env)?;
                env_remove_scope(&mut local_env);
                Ok(local_env)
            })
            .collect();

        results
            .into_iter()
            .next()
            .unwrap_or_else(|| Ok(env.clone()))
    } else {
        Err("For only works on arrays".into())
    }
}
