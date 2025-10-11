// src/execution.rs
use crate::expr::Expr;
use crate::scope::*;
use crate::stmt::{Decl, Stmt};
use std::collections::HashMap;

pub fn execute_stmt(decl: &Decl, env: &mut Env) -> Result<(), String> {
    match decl {
        Decl::Stmt(Stmt::Print(expr)) => {
            if let Some(val) = expr.eval(env) {
                println!("{}", val.pretty_print());
            } else {
                return Err("Error evaluating expression".to_string());
            }
        }
        Decl::VarDecl(name, expr, _mutability) => {
            if let Some(val) = expr.eval(env) {
                env_declare(env, name.clone(), val);
            } else {
                return Err(format!("Error: cannot evaluate {}", name));
            }
        }
        Decl::Stmt(Stmt::Assign(name, expr)) => {
            if let Some(val) = expr.eval(env) {
                if !env_assign(env, name, val) {
                    return Err(format!("Error: variable {} not found", name));
                }
            } else {
                return Err(format!("Error: cannot evaluate {}", name));
            }
        }
        Decl::Stmt(Stmt::Block(stmts)) => {
            env_add_scope(env, HashMap::new());
            for stmt in stmts {
                execute_stmt(stmt, env)?;
            }
            env_remove_scope(env);
        }
        Decl::Stmt(Stmt::Condition(cond, then_stmt, else_stmt)) => {
            if let Some(Expr::Bool(b)) = cond.eval(env) {
                if b {
                    execute_stmt(&Decl::Stmt(*then_stmt.clone()), env)?;
                } else if let Some(else_branch) = else_stmt {
                    execute_stmt(&Decl::Stmt(*else_branch.clone()), env)?;
                }
            } else {
                return Err("Error: condition must be boolean".to_string());
            }
        }
        Decl::Stmt(Stmt::While(cond, body)) => {
            let body = &Decl::Stmt(*body.clone());
            while let Some(Expr::Bool(true)) = cond.eval(env) {
                execute_stmt(body, env)?;
            }
        }
        Decl::Stmt(Stmt::For(var, arr_expr, body)) => {
            let body = &Decl::Stmt(*body.clone());
            if let Some(Expr::Array(arr)) = arr_expr.eval(env) {
                for val in arr {
                    let mut scope = HashMap::new();
                    scope.insert(var.clone(), val);
                    env_add_scope(env, scope);

                    execute_stmt(body, env)?;

                    env_remove_scope(env);
                }
            } else {
                return Err("Error: for loop requires array".to_string());
            }
        }
        Decl::Stmt(Stmt::Expression(_)) => {}
        Decl::None => unreachable!(),
    }

    Ok(())
}

pub fn execute(decls: &[Decl]) -> Result<(), String> {
    let mut env = env_empty();
    for decl in decls {
        execute_stmt(decl, &mut env)?;
    }
    Ok(())
}
