use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::{
    expr::Expr,
    stmt::{Decl, Mutability, Stmt},
};

/// --- Minimal placeholder types (заміни на свої реальні типи) ---

/// --- Aliases matching Haskell version ---
type Variable = String;
type Scope = Vec<HashMap<Variable, (usize, Mutability)>>; // stack of scopes; head is nearest
pub(crate) type Meta = VecDeque<(usize, HashSet<usize>)>; // sequence of (complexity, set of mutable-dependencies)
type Graph = DiGraph<(), ()>;

/// --- Utilities for scope operations ---

fn env_lookup(scope: &Scope, name: &str) -> Option<(usize, Mutability)> {
    for s in scope.iter().rev() {
        if let Some(v) = s.get(name) {
            return Some(v.clone());
        }
    }
    None
}

fn env_add_scope(mut scope: Scope, init_scope: HashMap<Variable, (usize, Mutability)>) -> Scope {
    scope.push(init_scope);
    scope
}

fn env_declare(mut scope: Scope, name: Variable, info: (usize, Mutability)) -> Scope {
    if scope.is_empty() {
        let mut m = HashMap::new();
        m.insert(name, info);
        scope.push(m);
    } else {
        let last = scope.len() - 1;
        scope[last].insert(name, info);
    }
    scope
}

fn env_remove_scope(mut scope: Scope) -> Scope {
    if scope.is_empty() {
        scope
    } else {
        scope.pop();
        scope
    }
}

fn env_assign(scope: &Scope, name: &str, index: usize) -> Option<Scope> {
    // Must return new scope with the variable assigned (mutability set to Mutable)
    let mut new_scope = scope.clone();
    for map in new_scope.iter_mut().rev() {
        if map.contains_key(name) {
            if let Some((_, _)) = map.get(name).cloned() {
                map.insert(name.to_string(), (index, Mutability::Mutable));
                return Some(new_scope);
            }
        }
    }
    None
}

/// Merge two scopes vectors, aligning indices (zip-like).
/// For conflicting variable keys we choose the entry with the **larger index** (like unionWith max on Integer).
fn merge_scopes_preferring_larger(a: &Scope, b: &Scope) -> Scope {
    let max_len = a.len().max(b.len());
    let mut out: Scope = Vec::with_capacity(max_len);

    for i in 0..max_len {
        let mut merged = HashMap::new();
        if let Some(map_a) = a.get(i) {
            for (k, v) in map_a {
                merged.insert(k.clone(), *v);
            }
        }
        if let Some(map_b) = b.get(i) {
            for (k, v) in map_b {
                // if exists, choose entry with larger index
                match merged.get(k) {
                    Some((existing_idx, _)) if *existing_idx >= v.0 => {}
                    _ => {
                        merged.insert(k.clone(), *v);
                    }
                }
            }
        }
        out.push(merged);
    }
    out
}

/// Merge two scopes but for "after branches" we want to pick the entry with the **smaller** index
/// (this reflects `first (`min` index) <$> x` semantics in Haskell snippet).
fn merge_scopes_preferring_smaller(a: &Scope, b: &Scope) -> Scope {
    let max_len = a.len().max(b.len());
    let mut out: Scope = Vec::with_capacity(max_len);

    for i in 0..max_len {
        let mut merged = HashMap::new();
        if let Some(map_a) = a.get(i) {
            for (k, v) in map_a {
                merged.insert(k.clone(), *v);
            }
        }
        if let Some(map_b) = b.get(i) {
            for (k, v) in map_b {
                match merged.get(k) {
                    Some((existing_idx, _)) if *existing_idx <= v.0 => {}
                    _ => {
                        merged.insert(k.clone(), *v);
                    }
                }
            }
        }
        out.push(merged);
    }
    out
}

/// Extract variable names from an expression (simple traversal).
fn extract_vars(e: &Expr) -> Vec<Variable> {
    match e {
        Expr::Var(name) => vec![name.clone()],
        Expr::Unary(_, inner) => extract_vars(inner),
        Expr::Binary(l, _, r) => {
            let mut v = extract_vars(l);
            v.extend(extract_vars(r));
            v
        }
        Expr::MapExpr(_, a, b) | Expr::FilterExpr(_, a, b) => {
            let mut v = extract_vars(a);
            v.extend(extract_vars(b));
            v
        }
        Expr::ScanlExpr(_, a, _, b, c) | Expr::FoldlExpr(_, a, _, b, c) => {
            let mut v = extract_vars(a);
            v.extend(extract_vars(b));
            v.extend(extract_vars(c));
            v
        }
        Expr::Range(a, b) => {
            let mut v = extract_vars(a);
            v.extend(extract_vars(b));
            v
        }
        _ => vec![],
    }
}

fn get_expr_complexity(e: &Expr) -> usize {
    match e {
        Expr::MapExpr { .. } => 2,
        Expr::FilterExpr { .. } => 2,
        Expr::ScanlExpr { .. } => 2,
        Expr::FoldlExpr { .. } => 2,
        _ => 1,
    }
}

/// Main analyze entrypoint.
/// Returns (graph, meta)
pub fn analyze(decls: &[Decl]) -> (Graph, Meta) {
    let (g, _, meta, _) = analyze_rec(0, Vec::new(), VecDeque::new(), DiGraph::new(), decls);
    (g, meta)
}

/// Recursive analyzer mirroring Haskell `analyze'`.
/// Parameters:
/// - from: starting index
/// - scope: current scope stack
/// - initial_meta: meta collected so far (usually empty when descending)
/// - graph: current graph
/// - decls: list of declarations to analyze
///
/// Returns (graph, scope_after, meta_collected, next_index)
fn analyze_rec(
    from: usize,
    mut scope: Scope,
    mut initial_meta: Meta,
    mut graph: Graph,
    decls: &[Decl],
) -> (Graph, Scope, Meta, usize) {
    let mut index = from;

    for decl in decls.into_iter() {
        let (g2, s2, m2, next_index) = add_stmt(
            index,
            decl,
            graph.clone(),
            scope.clone(),
            initial_meta.clone(),
        );
        graph = g2;
        scope = s2;
        initial_meta = m2;
        index = next_index;

        // dbg!(&graph, &decl);
    }

    (graph, scope, initial_meta, index)
}

/// add_stmt returns (graph, scope, meta, next_index)
fn add_stmt(
    index: usize,
    decl: &Decl,
    mut graph: Graph,
    scope: Scope,
    mut meta: Meta,
) -> (Graph, Scope, Meta, usize) {
    match decl {
        Decl::VarDecl(name, expr, mutability) => {
            let deps = extract_vars(&expr);
            let indexes: Vec<(usize, Mutability)> =
                deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

            if indexes.is_empty() {
                graph.add_node(());
            } else {
                graph.add_node(());
                for (dep, _) in &indexes {
                    graph.update_edge(NodeIndex::new(*dep), NodeIndex::new(index), ());
                }
            }

            let mut_deps: HashSet<usize> = indexes
                .iter()
                .filter(|(_, m)| *m == Mutability::Mutable)
                .map(|(i, _)| *i)
                .collect();
            let new_scope = env_declare(scope, name.to_string(), (index, *mutability));
            meta.push_back((get_expr_complexity(&expr), mut_deps));
            (graph, new_scope, meta, index + 1)
        }

        Decl::Stmt(Stmt::Print(expr)) => {
            let deps = extract_vars(&expr);
            let indexes: Vec<(usize, Mutability)> =
                deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

            if indexes.is_empty() {
                graph.add_node(());
            } else {
                graph.add_node(());
                for (dep, _) in &indexes {
                    graph.update_edge(NodeIndex::new(*dep), NodeIndex::new(index), ());
                }
            }
            let mut_deps = indexes
                .iter()
                .filter(|(_, m)| *m == Mutability::Mutable)
                .map(|(i, _)| *i)
                .collect();
            meta.push_back((4, mut_deps));
            (graph, scope, meta, index + 1)
        }

        Decl::Stmt(Stmt::Assign(name, expr)) => {
            let deps = extract_vars(&expr);
            let mut indexes: Vec<(usize, Mutability)> =
                deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();
            if let Some(lhs) = env_lookup(&scope, &name) {
                indexes.push(lhs);
            }

            if indexes.is_empty() {
                graph.add_node(());
            } else {
                graph.add_node(());
                for (dep, _) in &indexes {
                    graph.update_edge(NodeIndex::new(*dep), NodeIndex::new(index), ());
                }
            }

            let mut_deps: HashSet<usize> = indexes
                .iter()
                .filter(|(_, m)| *m == Mutability::Mutable)
                .map(|(i, _)| *i)
                .collect();

            let new_scope = env_assign(&scope, &name, index)
                .unwrap_or_else(|| panic!("Variable {} should be declared", name));

            meta.push_back((get_expr_complexity(&expr), mut_deps));
            (graph, new_scope, meta, index + 1)
        }

        Decl::Stmt(Stmt::Block(stmts)) => {
            // create new inner scope
            let new_scope = {
                let mut s = scope.clone();
                s.insert(0, HashMap::new());
                s
            };

            // analyze block starting with same index (matching Haskell semantics)
            let (g2, scope_after_block, mut new_meta, i_after_block) = analyze_rec(
                index,
                new_scope.clone(),
                VecDeque::new(),
                graph.clone(),
                stmts,
            );
            // after exploring block, remove the innermost scope
            let scope_after = env_remove_scope(scope_after_block);
            // append block meta to outer meta
            meta.append(&mut new_meta);

            // In Haskell code they returned i + 1 after block; we keep same convention:
            (g2, scope_after, meta, i_after_block + 1)
        }

        Decl::Stmt(Stmt::Condition(cond, then_block, maybe_else)) => {
            // deps from cond
            let deps = extract_vars(&cond);
            let indexes: Vec<(usize, Mutability)> =
                deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

            if indexes.is_empty() {
                graph.add_node(());
            } else {
                graph.add_node(());
                for (dep, _) in &indexes {
                    graph.update_edge(NodeIndex::new(*dep), NodeIndex::new(index), ());
                }
            }
            let mut_deps: HashSet<usize> = indexes
                .iter()
                .filter(|(_, m)| *m == Mutability::Mutable)
                .map(|(i, _)| *i)
                .collect();

            // analyze then branch
            let then_scope = {
                let mut s = scope.clone();
                s.insert(0, HashMap::new());
                s
            };
            let (g_then, scope_then_after, mut meta_then, _i_then_end) = analyze_rec(
                index,
                then_scope.clone(),
                VecDeque::new(),
                graph.clone(),
                match then_block.as_ref() {
                    Stmt::Block(stmts) => stmts,
                    _ => &[],
                },
            );

            // analyze else branch if present
            let (g_else, scope_else_after, mut meta_else) = if let Some(else_stmt) = maybe_else {
                let else_scope = {
                    let mut s = scope.clone();
                    s.insert(0, HashMap::new());
                    s
                };
                let (g_e, s_e_after, m_e, _i_e_end) = analyze_rec(
                    index,
                    else_scope.clone(),
                    VecDeque::new(),
                    g_then.clone(),
                    match else_stmt.as_ref() {
                        Stmt::Block(stmts) => stmts,
                        _ => &[],
                    },
                );
                (g_e, s_e_after, m_e)
            } else {
                // no else: treat as empty branch
                (
                    g_then.clone(),
                    {
                        let mut s = scope.clone();
                        s.insert(0, HashMap::new());
                        s
                    },
                    {
                        let mut d = VecDeque::new();
                        d.push_back((1, HashSet::new()));
                        d
                    },
                )
            };

            // merge with "max" semantics (unionWith max)
            let merged_scope = merge_scopes_preferring_larger(&scope_then_after, &scope_else_after);

            // then clamp ids by index: first (`min` index) <$> merged_scope
            let clamped = apply_min_index_to_scope(&merged_scope, index);

            // remove innermost scope (як в Haskell)
            let next_scope = env_remove_scope(clamped);

            // merge graphs: use graph with edges from else analysis, then remove self-loop index->index if any
            let mut next_graph = g_else;
            redirect_edges_to_target(&mut next_graph, index);
            next_graph.retain_edges(|g, e| {
                let edge = g.edge_endpoints(e).unwrap();
                edge.0 != edge.1
            });

            // stmtDeps: mutable deps from condition and both metas, filtered < index
            let mut stmt_deps = mut_deps;
            for (_, set) in meta_then.iter().chain(meta_else.iter()) {
                for v in set {
                    if *v < index {
                        stmt_deps.insert(*v);
                    }
                }
            }

            // final complexity: max of maximum complexities from both metas
            let max_then = meta_then.iter().map(|(c, _)| *c).max().unwrap_or(1);
            let max_else = meta_else.iter().map(|(c, _)| *c).max().unwrap_or(1);
            let final_complex = std::cmp::max(max_then, max_else);

            meta.push_back((final_complex, stmt_deps));

            (next_graph, next_scope, meta, index + 1)
        }

        Decl::Stmt(Stmt::While(cond, block)) => {
            let deps = extract_vars(&cond);
            let indexes: Vec<(usize, Mutability)> =
                deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

            if indexes.is_empty() {
                graph.add_node(());
            } else {
                graph.add_node(());
                for (dep, _) in &indexes {
                    graph.update_edge(NodeIndex::new(*dep), NodeIndex::new(index), ());
                }
            }
            let mut_deps: HashSet<usize> = indexes
                .iter()
                .filter(|(_, m)| *m == Mutability::Mutable)
                .map(|(i, _)| *i)
                .collect();

            let new_scope = {
                let mut s = scope.clone();
                s.insert(0, HashMap::new());
                s
            };
            let (g_block, scope_after_block, mut block_meta, _i_end) = analyze_rec(
                index,
                new_scope.clone(),
                VecDeque::new(),
                graph.clone(),
                match block.as_ref() {
                    Stmt::Block(stmts) => stmts,
                    _ => &[],
                },
            );

            let clamped = apply_min_index_to_scope(&scope_after_block, index);
            let next_scope = env_remove_scope(clamped);

            let mut next_graph = g_block;
            redirect_edges_to_target(&mut next_graph, index);
            next_graph.retain_edges(|g, e| {
                let edge = g.edge_endpoints(e).unwrap();
                edge.0 != edge.1
            });

            // stmtDeps: mut_deps and meta from block
            let mut stmt_deps = mut_deps;
            for (_, set) in block_meta.iter() {
                for v in set {
                    if *v < index {
                        stmt_deps.insert(*v);
                    }
                }
            }

            meta.push_back((2, stmt_deps));

            // Return next index incremented (match Haskell pattern)
            (next_graph, next_scope, meta, index + 1)
        }

        Decl::Stmt(Stmt::For(name, arr, block)) => {
            let deps = extract_vars(&arr);
            let indexes: Vec<(usize, Mutability)> =
                deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

            if indexes.is_empty() {
                graph.add_node(());
            } else {
                graph.add_node(());
                for (dep, _) in &indexes {
                    graph.update_edge(NodeIndex::new(*dep), NodeIndex::new(index), ());
                }
            }
            let mut_deps: HashSet<usize> = indexes
                .iter()
                .filter(|(_, m)| *m == Mutability::Mutable)
                .map(|(i, _)| *i)
                .collect();

            // new scope where loop var is declared immutable at index
            let mut top = HashMap::new();
            top.insert(name.to_string(), (index, Mutability::Immutable));
            let s_with_loopvar = env_add_scope(scope.clone(), top);

            let (g_block, scope_after_block, block_meta, _i_end) = analyze_rec(
                index,
                s_with_loopvar.clone(),
                VecDeque::new(),
                graph.clone(),
                match block.as_ref() {
                    Stmt::Block(stmts) => stmts,
                    _ => &[],
                },
            );

            // next_scope: prefer smaller indices when merging
            let clamped = apply_min_index_to_scope(&scope_after_block, index);
            let next_scope = env_remove_scope(clamped);

            let mut next_graph = g_block;
            redirect_edges_to_target(&mut next_graph, index);
            next_graph.retain_edges(|g, e| {
                let edge = g.edge_endpoints(e).unwrap();
                edge.0 != edge.1
            });

            // stmtDeps from mut_deps and block_meta values < index
            let mut stmt_deps = mut_deps;
            for (_, set) in block_meta.iter() {
                for v in set {
                    if *v < index {
                        stmt_deps.insert(*v);
                    }
                }
            }

            meta.push_back((2, stmt_deps));
            (next_graph, next_scope, meta, index + 1)
        }

        Decl::Stmt(Stmt::Expression(expr)) => {
            let deps = extract_vars(&expr);
            let indexes: Vec<(usize, Mutability)> =
                deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

            if indexes.is_empty() {
                graph.add_node(());
            } else {
                graph.add_node(());
                for (dep, _) in &indexes {
                    graph.update_edge(NodeIndex::new(*dep), NodeIndex::new(index), ());
                }
            }
            let mut_deps: HashSet<usize> = indexes
                .iter()
                .filter(|(_, m)| *m == Mutability::Mutable)
                .map(|(i, _)| *i)
                .collect();
            meta.push_back((get_expr_complexity(&expr), mut_deps));
            (graph, scope, meta, index + 1)
        }

        other => {
            panic!("todo: unhandled decl: {:?}", other);
        }
    }
}

fn apply_min_index_to_scope(scope: &Scope, idx: usize) -> Scope {
    scope
        .iter()
        .map(|map| {
            map.iter()
                .map(|(k, (i, m))| (k.clone(), (std::cmp::min(*i, idx), m.clone())))
                .collect::<HashMap<Variable, (usize, Mutability)>>()
        })
        .collect::<Scope>()
}

fn redirect_edges_to_target(graph: &mut DiGraph<(), ()>, target_index: usize) {
    let target = NodeIndex::new(target_index);

    // 1️⃣ Збираємо ребра, які треба перенаправити (не можна змінювати граф під час ітерації!)
    let edges_to_redirect: Vec<(NodeIndex, NodeIndex)> = graph
        .edge_indices()
        .filter_map(|eidx| {
            let (src, dst) = graph.edge_endpoints(eidx)?;
            if dst.index() > target_index || src.index() > target_index {
                Some((src, dst))
            } else {
                None
            }
        })
        .collect();

    // 2️⃣ Видаляємо старі ребра
    for (src, dst) in &edges_to_redirect {
        if let Some(eidx) = graph.find_edge(*src, *dst) {
            graph.remove_edge(eidx);
        }
    }

    // 3️⃣ Додаємо нові ребра → target
    for (src, dst) in &edges_to_redirect {
        graph.remove_node(*dst);
        if *src < target {
            graph.update_edge(*src, target, ());
        } else if *src > target {
            graph.remove_node(*src);
        }
    }
}
