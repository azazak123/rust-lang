use crate::declaration_meta::DeclarationMeta;
use crate::execution::execute_stmt;
use crate::expr::Expr;
use crate::graph::Meta;
use crate::scope::*;
use crate::stmt::{Decl, Stmt};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;

/// Паралельне виконання графа задач із залежностями.
/// Повний аналог функції `executePlan` з Haskell.
pub fn execute_plan(
    order: &[usize],
    graph: &DiGraph<DeclarationMeta, ()>,
) -> Result<Vec<Env>, String> {
    let results: Arc<RwLock<HashMap<usize, Env>>> = Arc::new(RwLock::new(HashMap::new()));

    thread::scope(|s| {
        for &task_id in order {
            // let decl = decls
            //     .get(task_id)
            //     .ok_or_else(|| format!("Invalid decl index {}", task_id))?;
            let DeclarationMeta {
                index: _,
                complexity: _,
                mut_deps: deps_meta,
                class: _,
                decl,
                loops_decls_indexes: _,
            } = &graph[NodeIndex::new(task_id)];
            // .get(task_id)
            // .ok_or_else(|| format!("Invalid meta index {}", task_id))?;

            // dbg!(decl);

            let results_clone = Arc::clone(&results);

            // Запускаємо кожне завдання в окремому потоці
            s.spawn(move || {
                let node_idx = NodeIndex::new(task_id);

                // Знайти всі залежності (preSet)
                let dep_ids: Vec<usize> = graph
                    .edges_directed(node_idx, petgraph::Direction::Incoming)
                    .map(|e| e.source().index())
                    .collect();

                // Очікуємо на завершення усіх залежностей
                let dep_results: Vec<Env> = dep_ids
                    .iter()
                    .map(|&dep_id| loop {
                        if let Some(env) = results_clone.read().unwrap().get(&dep_id) {
                            return env.clone();
                        }
                        thread::sleep(std::time::Duration::from_millis(1));
                    })
                    .collect();

                // Створюємо середовище
                let mut env = create_env(dep_results, dep_ids);

                // Виконання самого завдання
                let res_env = if deps_meta.is_empty() {
                    match &decl {
                        Decl::Stmt(Stmt::For(var, arr_expr, body)) => {
                            parallel_for(var.clone(), arr_expr, body, &env)
                        }
                        _ => execute_stmt(&decl, &mut env)
                            .map_err(|e| format!("Execution error: {}", e))
                            .map(|_| env),
                    }
                } else {
                    execute_stmt(&decl, &mut env)
                        .map_err(|e| format!("Execution error: {}", e))
                        .map(|_| env)
                };

                if let Ok(final_env) = res_env {
                    results_clone.write().unwrap().insert(task_id, final_env);
                }
            });
        }

        return Result::<(), String>::Ok(());
    })?;

    // Повертаємо результати в порядку order
    let final_results = results.read().unwrap();
    Ok(order
        .iter()
        .filter_map(|&id| final_results.get(&id).cloned())
        .collect())
}

/// Паралельне виконання циклу `For`, аналог `parallelFor` з Haskell
fn parallel_for(var: String, arr_expr: &Expr, block: &Stmt, env: &Env) -> Result<Env, String> {
    let body = &Decl::Stmt(block.clone());
    if let Some(Expr::Array(arr)) = arr_expr.eval(&env) {
        let results = arr
            .par_iter()
            .map(|el| {
                let mut local_env = env.clone();
                let mut scope = HashMap::new();
                scope.insert(var.clone(), el.clone());
                env_add_scope(&mut local_env, scope);
                // dbg!(&body);
                let _ = execute_stmt(body, &mut local_env)?;
                env_remove_scope(&mut local_env);
                Ok(local_env)
            })
            .collect::<Vec<Result<Env, String>>>();

        // Аналог `head envs` у Haskell
        let res = results
            .into_iter()
            .next()
            .unwrap_or_else(|| Ok(env.clone()))?;

        Ok(res)
    } else {
        Err("For only works on arrays".into())
    }
}
