use rustc_hash::FxHashMap as HashMap;

use crate::scope::*;

#[derive(Debug, Clone)]
pub enum Expr {
    Number(f64),
    String(String),
    Bool(bool),
    Nil,
    Var(String),
    Unary(UnaryOp, Box<Expr>),
    Binary(Box<Expr>, BinaryOp, Box<Expr>),
    Array(Vec<Expr>),
    Range(Box<Expr>, Box<Expr>),
    MapExpr(String, Box<Expr>, Box<Expr>),
    FilterExpr(String, Box<Expr>, Box<Expr>),
    ScanlExpr(String, Box<Expr>, String, Box<Expr>, Box<Expr>),
    FoldlExpr(String, Box<Expr>, String, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOp {
    Bang,
    Minus,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOp {
    Plus,
    Minus,
    Star,
    Slash,
    EqualEqual,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
    Get,
    Modulo,
}

impl Expr {
    pub fn eval(&self, env: &Env) -> Option<Expr> {
        match self {
            Expr::Number(n) => Some(Expr::Number(*n)),
            Expr::String(s) => Some(Expr::String(s.clone())),
            Expr::Bool(b) => Some(Expr::Bool(*b)),
            Expr::Nil => Some(Expr::Nil),

            Expr::Var(name) => env_lookup(env, name),

            Expr::Unary(op, e) => {
                let val = e.eval(env)?;
                match (op, val) {
                    (UnaryOp::Bang, Expr::Bool(b)) => Some(Expr::Bool(!b)),
                    (UnaryOp::Minus, Expr::Number(n)) => Some(Expr::Number(-n)),
                    _ => None,
                }
            }

            Expr::Binary(left, op, right) => {
                let l = left.eval(env)?;
                match (l, op) {
                    (Expr::Number(n1), _) => {
                        let Expr::Number(n2) = right.eval(env)? else {
                            return None;
                        };
                        match op {
                            BinaryOp::Plus => Some(Expr::Number(n1 + n2)),
                            BinaryOp::Minus => Some(Expr::Number(n1 - n2)),
                            BinaryOp::Star => Some(Expr::Number(n1 * n2)),
                            BinaryOp::Slash => Some(Expr::Number(n1 / n2)),
                            BinaryOp::EqualEqual => Some(Expr::Bool((n1 - n2).abs() < 1e-10)),
                            BinaryOp::BangEqual => Some(Expr::Bool((n1 - n2).abs() >= 1e-10)),
                            BinaryOp::Less => Some(Expr::Bool(n1 < n2)),
                            BinaryOp::LessEqual => Some(Expr::Bool(n1 <= n2)),
                            BinaryOp::Greater => Some(Expr::Bool(n1 > n2)),
                            BinaryOp::GreaterEqual => Some(Expr::Bool(n1 >= n2)),
                            BinaryOp::Modulo => Some(Expr::Number(n1 % n2)),
                            _ => None,
                        }
                    }
                    (Expr::Bool(b1), BinaryOp::And) => {
                        if !b1 {
                            Some(Expr::Bool(false))
                        } else {
                            let Expr::Bool(b2) = right.eval(env)? else {
                                return None;
                            };
                            Some(Expr::Bool(b2))
                        }
                    }
                    (Expr::Bool(b1), BinaryOp::Or) => {
                        if b1 {
                            Some(Expr::Bool(true))
                        } else {
                            let Expr::Bool(b2) = right.eval(env)? else {
                                return None;
                            };
                            Some(Expr::Bool(b2))
                        }
                    }
                    (Expr::Bool(b1), BinaryOp::EqualEqual) => {
                        let Expr::Bool(b2) = right.eval(env)? else {
                            return None;
                        };
                        Some(Expr::Bool(b1 == b2))
                    }
                    (Expr::Bool(b1), BinaryOp::BangEqual) => {
                        let Expr::Bool(b2) = right.eval(env)? else {
                            return None;
                        };
                        Some(Expr::Bool(b1 != b2))
                    }
                    (Expr::Array(arr), BinaryOp::Get) => {
                        let Expr::Number(idx) = right.eval(env)? else {
                            return None;
                        };
                        let index = idx as usize;
                        arr.get(index).cloned()
                    }
                    _ => None,
                }
            }

            Expr::Array(exprs) => {
                let vals: Option<Vec<_>> = exprs.iter().map(|e| e.eval(env)).collect();
                Some(Expr::Array(vals?))
            }

            Expr::Range(start, end) => {
                let Expr::Number(s) = start.eval(env)? else {
                    return None;
                };
                let Expr::Number(e) = end.eval(env)? else {
                    return None;
                };
                let start_int = s as i64;
                let end_int = e as i64;

                let range: Vec<Expr> = if start_int <= end_int {
                    (start_int..=end_int)
                        .map(|i| Expr::Number(i as f64))
                        .collect()
                } else {
                    (end_int..=start_int)
                        .rev()
                        .map(|i| Expr::Number(i as f64))
                        .collect()
                };
                Some(Expr::Array(range))
            }

            Expr::MapExpr(var, arr_expr, body) => {
                let Expr::Array(arr) = arr_expr.eval(env)? else {
                    return None;
                };
                let mut results = Vec::with_capacity(arr.len());

                for val in arr {
                    let mut new_env = env.clone();
                    let mut scope = HashMap::default();
                    scope.insert(var.clone(), val);
                    env_add_scope(&mut new_env, scope);

                    results.push(body.eval(&new_env)?);
                    env_remove_scope(&mut new_env);
                }
                Some(Expr::Array(results))
            }

            Expr::FilterExpr(var, arr_expr, body) => {
                let Expr::Array(arr) = arr_expr.eval(env)? else {
                    return None;
                };
                let mut results = Vec::new();

                for val in arr {
                    let mut new_env = env.clone();
                    let mut scope = HashMap::default();
                    scope.insert(var.clone(), val.clone());
                    env_add_scope(&mut new_env, scope);

                    let Expr::Bool(cond) = body.eval(&new_env)? else {
                        return None;
                    };
                    if cond {
                        results.push(val);
                    }
                    env_remove_scope(&mut new_env);
                }
                Some(Expr::Array(results))
            }

            Expr::FoldlExpr(acc_name, init, var, arr_expr, body) => {
                let Expr::Array(arr) = arr_expr.eval(env)? else {
                    return None;
                };
                let mut acc = init.eval(env)?;

                for val in arr {
                    let mut new_env = env.clone();
                    let mut scope = HashMap::default();
                    scope.insert(acc_name.clone(), acc);
                    scope.insert(var.clone(), val);
                    env_add_scope(&mut new_env, scope);

                    acc = body.eval(&new_env)?;
                    env_remove_scope(&mut new_env);
                }
                Some(acc)
            }

            Expr::ScanlExpr(acc_name, init, var, arr_expr, body) => {
                let Expr::Array(arr) = arr_expr.eval(env)? else {
                    return None;
                };
                let mut acc = init.eval(env)?;
                let mut results = vec![acc.clone()];

                for val in arr {
                    let mut new_env = env.clone();
                    let mut scope = HashMap::default();
                    scope.insert(acc_name.clone(), acc);
                    scope.insert(var.clone(), val);
                    env_add_scope(&mut new_env, scope);

                    acc = body.eval(&new_env)?;
                    results.push(acc.clone());
                    env_remove_scope(&mut new_env);
                }
                Some(Expr::Array(results))
            }
        }
    }

    pub fn pretty_print(&self) -> String {
        match self {
            Expr::Number(n) => {
                if n.fract() == 0.0 {
                    format!("{:.0}", n)
                } else {
                    n.to_string()
                }
            }
            Expr::String(s) => s.clone(),
            Expr::Bool(b) => b.to_string(),
            Expr::Nil => "nil".to_string(),
            Expr::Array(arr) => {
                let items: Vec<_> = arr.iter().map(Self::pretty_print).collect();
                format!("[{}]", items.join(", "))
            }
            Expr::Var(name) => name.clone(),
            _ => format!("{:?}", self),
        }
    }

    /// Extract variable names from an expression (simple traversal).
    pub fn extract_vars(&self) -> Vec<String> {
        match self {
            Expr::Var(name) => vec![name.clone()],
            Expr::Unary(_, inner) => inner.extract_vars(),
            Expr::Binary(l, _, r) => {
                let mut v = l.extract_vars();
                v.extend(r.extract_vars());
                v
            }
            Expr::MapExpr(_, a, b) | Expr::FilterExpr(_, a, b) => {
                let mut v = a.extract_vars();
                v.extend(b.extract_vars());
                v
            }
            Expr::ScanlExpr(_, a, _, b, c) | Expr::FoldlExpr(_, a, _, b, c) => {
                let mut v = a.extract_vars();
                v.extend(b.extract_vars());
                v.extend(c.extract_vars());
                v
            }
            Expr::Range(a, b) => {
                let mut v = a.extract_vars();
                v.extend(b.extract_vars());
                v
            }
            _ => vec![],
        }
    }
}
