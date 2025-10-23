use std::ops::Deref;
use std::sync::Arc;
use std::time::Instant;

use crate::expr::Expr;
use crate::scope::*;
use crate::stat_manager::StatManager;
use crate::stmt::{Decl, DeclType, Stmt};

pub fn execute_stmt(decl: &Arc<Decl>, env: &mut Env) -> Result<(), String> {
    // Match the inner DeclType by reference

    let start = Instant::now();
    match &decl.v {
        // 1. VARIABLE DECLARATION (No Arc involved here)
        DeclType::VarDecl(name, expr, _mutability) => {
            let val = eval_or_err(expr, env, Some(name))?;
            env_declare(env, name.clone(), val);
        }

        // 2. STATEMENT: Match the inner Arc<Stmt>
        DeclType::Stmt(arc_stmt) => {
            // Dereference the Arc<Stmt> to get a reference to the inner Stmt
            match arc_stmt.deref() {
                // 2.1. PRINT Statement
                Stmt::Print(expr) => {
                    let val = eval_or_err(expr, env, None)?;
                    println!("{}", val.pretty_print());
                }

                // 2.2. ASSIGNMENT Statement
                Stmt::Assign(name, expr) => {
                    let val = eval_or_err(expr, env, Some(name))?;
                    if !env_assign(env, name, val) {
                        return Err(format!(
                            "Error: variable {} not found or is immutable",
                            name
                        ));
                    }
                }

                // 2.3. BLOCK Statement
                Stmt::Block(stmts) => {
                    env_add_scope(env, env_create_scope());
                    for stmt in stmts {
                        execute_stmt(stmt, env)?;
                    }
                    env_remove_scope(env);
                }

                // 2.4. CONDITIONAL Statement (Then/Else branches are Arc<Stmt>)
                Stmt::Condition(cond, then_branch, else_branch_opt) => {
                    let cond_val = eval_or_err(cond, env, Some("condition expression"))?;

                    let b = match cond_val {
                        Expr::Bool(b) => b,
                        _ => return Err("Error: condition must be boolean".to_string()),
                    };

                    if b {
                        execute_stmt(then_branch, env)?; // Use helper for Arc<Stmt>
                    } else if let Some(else_branch) = else_branch_opt {
                        execute_stmt(else_branch, env)?; // Use helper for Arc<Stmt>
                    }
                }

                // 2.5. WHILE Loop (Body is Arc<Stmt>)
                Stmt::While(cond, body) => {
                    let stmts = match &body.deref().v {
                        DeclType::Stmt(stmt) => match stmt.deref() {
                            Stmt::Block(stmts) => Some(stmts),
                            _ => None,
                        },
                        _ => return Err("Error: for loop requires an statement".to_string()),
                    };

                    loop {
                        let cond_val = match eval_or_err(cond, env, Some("while condition"))? {
                            Expr::Bool(b) => b,
                            _ => return Err("Error: while condition must be boolean".to_string()),
                        };

                        if !cond_val {
                            break;
                        }

                        if let Some(stmts) = stmts {
                            for stmt in stmts {
                                execute_stmt(stmt, env)?;
                            }
                        } else {
                            execute_stmt(body, env)?;
                        }
                    }
                }

                // 2.6. FOR Loop (Body is Arc<Stmt>)
                Stmt::For(var, arr_expr, body) => {
                    let arr_val = eval_or_err(arr_expr, env, Some("for loop array"))?;
                    let stmts = match &body.deref().v {
                        DeclType::Stmt(stmt) => match stmt.deref() {
                            Stmt::Block(stmts) => Some(stmts),
                            _ => None,
                        },
                        _ => return Err("Error: for loop requires an statement".to_string()),
                    };

                    if let Expr::Array(arr) = arr_val {
                        for val in arr {
                            // Create and manage a new scope for each iteration
                            let mut scope = env_create_scope();
                            scope.insert(var.clone(), val);
                            env_add_scope(env, scope);

                            if let Some(stmts) = stmts {
                                for stmt in stmts {
                                    execute_stmt(stmt, env)?;
                                }
                            } else {
                                execute_stmt(body, env)?;
                            }

                            env_remove_scope(env);
                        }
                    } else {
                        return Err("Error: for loop requires an array expression".to_string());
                    }
                }

                // 2.7. EXPRESSION Statement
                Stmt::Expression(expr) => {
                    // Evaluate expression for side effects, but ignore the result
                    let _ = expr.eval(env);
                }
            }
        }

        // 3. NONE (Should not be reached)
        DeclType::None => unreachable!(),
    }
    let duration = start.elapsed();

    if duration.as_millis() > 100 {
        StatManager::send_data(decl.index, env_get_all_visible(&env), duration);
    }

    Ok(())
}

// Helper to reduce repetitive evaluation and error boilerplate
#[inline]
fn eval_or_err(expr: &Expr, env: &Env, name: Option<&str>) -> Result<Expr, String> {
    expr.eval(env).ok_or_else(|| {
        if let Some(n) = name {
            format!("Error: cannot evaluate expression for {}", n)
        } else {
            "Error evaluating expression".to_string()
        }
    })
}

pub fn execute(decls: &[Arc<Decl>]) -> Result<(), String> {
    let mut env = env_empty();
    for decl in decls {
        execute_stmt(decl, &mut env)?;
    }
    Ok(())
}
