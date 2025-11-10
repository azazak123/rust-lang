use petgraph::{
    graph::{DiGraph, NodeIndex},
    visit::EdgeRef,
};
use serde_json::de;
use std::{
    cmp::max,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::{
    declaration_meta::{Class, DeclarationMeta},
    expr::Expr,
    stmt::{Decl, DeclType, Mutability, Stmt},
};

/// --- Minimal placeholder types (заміни на свої реальні типи) ---

/// --- Aliases matching Haskell version ---
type Variable = String;
type Scope = Vec<HashMap<Variable, (usize, Mutability)>>; // stack of scopes; head is nearest
type Graph = DiGraph<Option<DeclarationMeta>, ()>;

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
#[allow(dead_code)]
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
pub fn analyze(decls: &[Arc<Decl>]) -> Graph {
    let (g, _, _) = analyze_rec(Vec::new(), DiGraph::new(), decls, 0);
    g
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
    mut scope: Scope,
    mut graph: Graph,
    decls: &[Arc<Decl>],
    depth: usize,
) -> (Graph, Scope, usize) {
    let mut last_index = 0;
    for decl in decls.into_iter() {
        let (g2, s2, index) = add_stmt(Arc::clone(decl), graph.clone(), scope.clone(), depth);
        graph = g2;
        scope = s2;
        last_index = index;

        // dbg!(&graph, &decl);
    }

    // for i in initial_meta.len()..graph.node_count() {
    //     graph.remove_node(NodeIndex::new(i));
    // }

    (graph, scope, last_index)
}

fn add_stmt(
    decl: Arc<Decl>,
    mut graph: Graph,
    scope: Scope,
    depth: usize,
) -> (Graph, Scope, usize) {
    // Допоміжна функція для створення/оновлення вузла
    let update_node = |graph: &mut Graph, meta: DeclarationMeta| {
        let index = meta.decl.index;
        if graph.raw_nodes().get(index).is_none() {
            graph.add_node(Some(meta));
        } else {
            graph[NodeIndex::new(index)] = Some(meta);
        }
    };

    // Допоміжна функція для додавання залежностей до графа
    let add_deps_to_graph =
        |graph: &mut Graph, indexes: &[(usize, Mutability)], current_index: usize| {
            for (dep, _) in indexes {
                // Забезпечуємо, що вузол dep існує
                while graph.node_count() - 1 < max(*dep, current_index) {
                    graph.add_node(None);
                }
                graph.update_edge(NodeIndex::new(*dep), NodeIndex::new(current_index), ());
            }
        };

    match &decl.v {
        // --- 1. VarDecl (Оголошення Змінної) ---
        DeclType::VarDecl(name, expr, mutability) => {
            let deps = expr.extract_vars();
            let indexes: Vec<(usize, Mutability)> =
                deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

            let mut_deps: HashSet<usize> = indexes
                .iter()
                .filter(|(_, m)| *m == Mutability::Mutable)
                .map(|(i, _)| *i)
                .collect();

            let meta = DeclarationMeta {
                complexity: get_expr_complexity(expr),
                mut_deps,
                class: Class::Ordinary,
                decl: Arc::clone(&decl),
                loops_decls_indexes: vec![],
                depth,
            };

            update_node(&mut graph, meta);
            add_deps_to_graph(&mut graph, &indexes, decl.index);

            let new_scope = env_declare(scope, name.to_string(), (decl.index, *mutability));
            (graph, new_scope, decl.index)
        }

        // --- 2. Stmt (Всі Оператори, обгорнуті в Arc<Stmt>) ---
        DeclType::Stmt(arc_stmt) => {
            match arc_stmt.as_ref() {
                // 2.1. PRINT Statement
                Stmt::Print(expr) => {
                    let deps = expr.extract_vars();
                    let indexes: Vec<(usize, Mutability)> =
                        deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

                    let mut_deps = indexes
                        .iter()
                        .filter(|(_, m)| *m == Mutability::Mutable)
                        .map(|(i, _)| *i)
                        .collect();

                    let meta = DeclarationMeta {
                        complexity: 4,
                        mut_deps,
                        class: Class::Ordinary,
                        decl: decl.clone(),
                        loops_decls_indexes: vec![],
                        depth,
                    };

                    update_node(&mut graph, meta);
                    add_deps_to_graph(&mut graph, &indexes, decl.index);

                    (graph, scope, decl.index)
                }

                // 2.2. ASSIGN Statement
                Stmt::Assign(name, expr) => {
                    let deps = expr.extract_vars();
                    let mut indexes: Vec<(usize, Mutability)> =
                        deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();
                    if let Some(lhs) = env_lookup(&scope, &name) {
                        indexes.push(lhs);
                    }

                    let mut_deps: HashSet<usize> = indexes
                        .iter()
                        .filter(|(_, m)| *m == Mutability::Mutable)
                        .map(|(i, _)| *i)
                        .collect();

                    let meta = DeclarationMeta {
                        complexity: get_expr_complexity(expr),
                        mut_deps,
                        class: Class::Ordinary,
                        decl: decl.clone(),
                        loops_decls_indexes: vec![],
                        depth,
                    };

                    update_node(&mut graph, meta);
                    add_deps_to_graph(&mut graph, &indexes, decl.index);

                    let new_scope = env_assign(&scope, &name, decl.index)
                        .unwrap_or_else(|| panic!("Variable {} should be declared", name));

                    (graph, new_scope, decl.index)
                }

                // 2.3. BLOCK Statement
                Stmt::Block(stmts) => {
                    let meta = DeclarationMeta {
                        complexity: 0,
                        mut_deps: HashSet::new(),
                        class: Class::Block,
                        decl: decl.clone(),
                        loops_decls_indexes: vec![],
                        depth: depth + 1,
                    };

                    update_node(&mut graph, meta);
                    add_deps_to_graph(&mut graph, &[], decl.index);

                    let new_scope = env_add_scope(scope.clone(), HashMap::new());

                    let (mut g2, scope_after_block, index_after_block) =
                        analyze_rec(new_scope, graph.clone(), stmts, depth + 1);

                    let scope_after = env_remove_scope(scope_after_block);

                    for n in decl.index + 1..=index_after_block {
                        g2[NodeIndex::new(decl.index)]
                            .as_mut()
                            .unwrap()
                            .loops_decls_indexes
                            .push(n);
                    }

                    //     let inner_deps = g2
                    //         .neighbors_directed(NodeIndex::new(n), petgraph::Direction::Incoming)
                    //         .collect::<Vec<_>>();
                    //     for source in inner_deps {
                    //         if source.index() < decl.index {
                    //             g2.update_edge(source, NodeIndex::new(decl.index), ());
                    //         }
                    //     }
                    // }

                    (g2, scope_after, index_after_block)
                }

                // 2.4. CONDITION Statement (Then/Else - Arc<Stmt>)
                Stmt::Condition(cond, then_block, maybe_else) => {
                    let deps = cond.extract_vars();
                    let indexes: Vec<(usize, Mutability)> =
                        deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

                    let mut_deps: HashSet<usize> = indexes
                        .iter()
                        .filter(|(_, m)| *m == Mutability::Mutable)
                        .map(|(i, _)| *i)
                        .collect();

                    let meta = DeclarationMeta {
                        complexity: 1,
                        mut_deps,
                        class: Class::Ordinary,
                        decl: decl.clone(),
                        loops_decls_indexes: vec![],
                        depth: depth + 1,
                    };

                    update_node(&mut graph, meta);
                    add_deps_to_graph(&mut graph, &indexes, decl.index);

                    // --- Аналіз гілки THEN ---
                    let then_scope = env_add_scope(scope.clone(), HashMap::new());
                    let (g_then, scope_then_after, i_after_than) = analyze_rec(
                        then_scope,
                        graph.clone(),
                        &[Arc::clone(then_block)],
                        depth + 1,
                    );

                    // --- Аналіз гілки ELSE ---
                    let (g_else, scope_else_after, i_after_block) =
                        if let Some(else_stmt) = maybe_else {
                            let else_scope = env_add_scope(scope.clone(), HashMap::new());
                            let (g_e, s_e_after, i_after_else) = analyze_rec(
                                else_scope,
                                g_then.clone(),
                                &[Arc::clone(else_stmt)],
                                depth + 1,
                            );
                            (g_e, s_e_after, i_after_else)
                        } else {
                            (
                                g_then.clone(),
                                env_add_scope(scope.clone(), HashMap::new()),
                                i_after_than,
                            )
                        };

                    let merged_scope =
                        merge_scopes_preferring_larger(&scope_then_after, &scope_else_after);
                    let clamped = apply_min_index_to_scope(&merged_scope, decl.index);
                    let next_scope = env_remove_scope(clamped);

                    let mut next_graph = g_else;
                    redirect_edges_to_target(&mut next_graph, decl.index);
                    next_graph.retain_edges(|g, e| {
                        let edge = g.edge_endpoints(e).unwrap();
                        edge.0 != edge.1
                    });

                    (next_graph, next_scope, i_after_block)
                }

                // 2.5. WHILE Loop (Тіло - Arc<Stmt>)
                Stmt::While(cond, block) => {
                    let deps = cond.extract_vars();
                    let indexes: Vec<(usize, Mutability)> =
                        deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

                    let mut_deps: HashSet<usize> = indexes
                        .iter()
                        .filter(|(_, m)| *m == Mutability::Mutable)
                        .map(|(i, _)| *i)
                        .collect();

                    let meta = DeclarationMeta {
                        complexity: 2,
                        mut_deps,
                        class: Class::Loop,
                        decl: Arc::clone(&decl),
                        loops_decls_indexes: vec![],
                        depth: depth + 1,
                    };

                    update_node(&mut graph, meta);
                    add_deps_to_graph(&mut graph, &indexes, decl.index);

                    // --- Аналіз тіла циклу ---
                    let new_scope = env_add_scope(scope.clone(), HashMap::new());
                    let initial_index = decl.index;

                    let block_stmts = &[Arc::clone(block)];

                    let (g_block, scope_after_block, _) =
                        analyze_rec(new_scope.clone(), graph.clone(), block_stmts, depth + 1);

                    let (mut g_block2, scope_after_block, index_after_block) = analyze_rec(
                        scope_after_block.clone(),
                        graph.clone(),
                        block_stmts,
                        depth + 1,
                    );

                    for e in g_block.edge_references() {
                        g_block2.update_edge(e.source(), e.target(), ());
                    }

                    let deps = cond.extract_vars();
                    let indexes: Vec<(usize, Mutability)> = deps
                        .iter()
                        .filter_map(|d| env_lookup(&scope_after_block, d))
                        .collect();

                    for (dep, _) in &indexes {
                        while g_block2.node_count() - 1 < max(*dep, decl.index) {
                            g_block2.add_node(None);
                        }
                        g_block2.update_edge(
                            NodeIndex::new(*dep),
                            NodeIndex::new(initial_index),
                            (),
                        );
                    }

                    let mut next_graph = g_block2;
                    for n in initial_index + 1..=index_after_block {
                        next_graph[NodeIndex::new(initial_index)]
                            .as_mut()
                            .unwrap()
                            .loops_decls_indexes
                            .push(n);
                        let inner_deps = next_graph
                            .neighbors_directed(NodeIndex::new(n), petgraph::Direction::Incoming)
                            .collect::<Vec<_>>();
                        for source in inner_deps {
                            if source.index() < initial_index {
                                next_graph.update_edge(source, NodeIndex::new(initial_index), ());
                            }
                        }
                        // if next_graph[NodeIndex::new(n)].as_ref().unwrap().class == Class::Loop {
                        //     next_graph.update_edge(
                        //         NodeIndex::new(initial_index),
                        //         NodeIndex::new(n),
                        //         (),
                        //     );
                        // }
                    }

                    let next_scope = env_remove_scope(scope_after_block);
                    (next_graph, next_scope, index_after_block)
                }

                // 2.6. FOR Loop (Тіло - Arc<Stmt>)
                Stmt::For(name, arr, block) => {
                    let deps = arr.extract_vars();
                    let indexes: Vec<(usize, Mutability)> =
                        deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

                    let mut_deps: HashSet<usize> = indexes
                        .iter()
                        .filter(|(_, m)| *m == Mutability::Mutable)
                        .map(|(i, _)| *i)
                        .collect();

                    let meta = DeclarationMeta {
                        complexity: 2,
                        mut_deps,
                        class: Class::Loop,
                        decl: Arc::clone(&decl),
                        loops_decls_indexes: vec![],
                        depth: depth + 1,
                    };

                    update_node(&mut graph, meta);
                    add_deps_to_graph(&mut graph, &indexes, decl.index);

                    // --- Аналіз тіла циклу ---
                    let mut top = HashMap::new();
                    top.insert(name.to_string(), (decl.index, Mutability::Immutable));
                    let s_with_loopvar = env_add_scope(scope.clone(), top);

                    let initial_index = decl.index;

                    let block_stmts = &[Arc::clone(block)];

                    let (g_block, scope_after_block, _) =
                        analyze_rec(s_with_loopvar.clone(), graph.clone(), block_stmts, depth + 1);

                    let (mut g_block2, scope_after_block, index_after_block) =
                        analyze_rec(scope_after_block.clone(), graph.clone(), block_stmts, depth + 1);

                    let next_scope = env_remove_scope(scope_after_block);

                    for e in g_block.edge_references() {
                        g_block2.update_edge(e.source(), e.target(), ());
                    }

                    let mut next_graph = g_block2;

                    for n in initial_index + 1..=index_after_block {
                        next_graph[NodeIndex::new(initial_index)]
                            .as_mut()
                            .unwrap()
                            .loops_decls_indexes
                            .push(n);
                        let inner_deps = next_graph
                            .neighbors_directed(NodeIndex::new(n), petgraph::Direction::Incoming)
                            .collect::<Vec<_>>();
                        for source in inner_deps {
                            if source.index() < initial_index {
                                next_graph.update_edge(source, NodeIndex::new(initial_index), ());
                            }
                        }
                        // next_graph.update_edge(
                        //     NodeIndex::new(initial_index),
                        //     NodeIndex::new(n),
                        //     (),
                        // );
                    }

                    (next_graph, next_scope, index_after_block)
                }

                // 2.7. EXPRESSION Statement
                Stmt::Expression(expr) => {
                    let deps = expr.extract_vars();
                    let indexes: Vec<(usize, Mutability)> =
                        deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

                    let mut_deps: HashSet<usize> = indexes
                        .iter()
                        .filter(|(_, m)| *m == Mutability::Mutable)
                        .map(|(i, _)| *i)
                        .collect();

                    let meta = DeclarationMeta {
                        complexity: get_expr_complexity(expr),
                        mut_deps,
                        class: Class::Ordinary,
                        decl: decl.clone(),
                        loops_decls_indexes: vec![],
                        depth
                    };

                    update_node(&mut graph, meta);
                    add_deps_to_graph(&mut graph, &indexes, decl.index);

                    (graph, scope, decl.index)
                }
            }
        }
        // --- 3. NONE ---
        DeclType::None => unreachable!(),
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

fn redirect_edges_to_target(graph: &mut Graph, target_index: usize) {
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

// use petgraph::{
//     graph::{DiGraph, NodeIndex},
//     visit::EdgeRef,
// };
// use std::{
//     cmp::max,
//     collections::{HashMap, HashSet},
//     sync::Arc,
// };

// use crate::{
//     declaration_meta::{Class, DeclarationMeta},
//     expr::Expr,
//     stmt::{Decl, DeclType, Mutability, Stmt},
// };

// type Variable = String;
// type Scope = Vec<HashMap<Variable, (usize, Mutability)>>;
// type Graph = DiGraph<Option<DeclarationMeta>, ()>;

// // ============================================================================
// // SCOPE UTILITIES
// // ============================================================================

// fn env_lookup(scope: &Scope, name: &str) -> Option<(usize, Mutability)> {
//     for s in scope.iter().rev() {
//         if let Some(v) = s.get(name) {
//             return Some(v.clone());
//         }
//     }
//     None
// }

// fn env_add_scope(mut scope: Scope, init_scope: HashMap<Variable, (usize, Mutability)>) -> Scope {
//     scope.push(init_scope);
//     scope
// }

// fn env_declare(mut scope: Scope, name: Variable, info: (usize, Mutability)) -> Scope {
//     if scope.is_empty() {
//         let mut m = HashMap::new();
//         m.insert(name, info);
//         scope.push(m);
//     } else {
//         let last = scope.len() - 1;
//         scope[last].insert(name, info);
//     }
//     scope
// }

// fn env_remove_scope(mut scope: Scope) -> Scope {
//     if !scope.is_empty() {
//         scope.pop();
//     }
//     scope
// }

// fn env_assign(scope: &Scope, name: &str, index: usize) -> Option<Scope> {
//     let mut new_scope = scope.clone();
//     for map in new_scope.iter_mut().rev() {
//         if map.contains_key(name) {
//             if let Some((_, _)) = map.get(name).cloned() {
//                 map.insert(name.to_string(), (index, Mutability::Mutable));
//                 return Some(new_scope);
//             }
//         }
//     }
//     None
// }

// fn merge_scopes_preferring_larger(a: &Scope, b: &Scope) -> Scope {
//     let max_len = a.len().max(b.len());
//     let mut out: Scope = Vec::with_capacity(max_len);

//     for i in 0..max_len {
//         let mut merged = HashMap::new();
//         if let Some(map_a) = a.get(i) {
//             for (k, v) in map_a {
//                 merged.insert(k.clone(), *v);
//             }
//         }
//         if let Some(map_b) = b.get(i) {
//             for (k, v) in map_b {
//                 match merged.get(k) {
//                     Some((existing_idx, _)) if *existing_idx >= v.0 => {}
//                     _ => {
//                         merged.insert(k.clone(), *v);
//                     }
//                 }
//             }
//         }
//         out.push(merged);
//     }
//     out
// }

// fn get_expr_complexity(e: &Expr) -> usize {
//     match e {
//         Expr::MapExpr { .. } => 2,
//         Expr::FilterExpr { .. } => 2,
//         Expr::ScanlExpr { .. } => 2,
//         Expr::FoldlExpr { .. } => 2,
//         _ => 1,
//     }
// }

// // ============================================================================
// // FLATTEN HELPERS - Створення плоских вузлів для циклів
// // ============================================================================

// /// Створює спеціальний вузол для For-циклу БЕЗ вкладених інструкцій
// fn create_flat_for_node(
//     decl: Arc<Decl>,
//     var: &str,
//     arr: &Expr,
//     body: &Arc<Decl>,
//     scope: &Scope,
// ) -> DeclarationMeta {
//     let deps = arr.extract_vars();
//     let indexes: Vec<(usize, Mutability)> =
//         deps.iter().filter_map(|d| env_lookup(scope, d)).collect();

//     let mut_deps: HashSet<usize> = indexes
//         .iter()
//         .filter(|(_, m)| *m == Mutability::Mutable)
//         .map(|(i, _)| *i)
//         .collect();

//     // Створюємо новий Decl який містить тільки голову FOR без тіла
//     let flat_for_decl = Arc::new(Decl {
//         index: decl.index,
//         v: DeclType::Stmt(Arc::new(Stmt::For(
//             var.to_string(),
//             arr.clone(),
//             body.clone(),
//         ))),
//     });

//     DeclarationMeta {
//         complexity: 2,
//         mut_deps,
//         class: Class::Loop,
//         decl: flat_for_decl,
//         loops_decls_indexes: vec![], // ПОРОЖНЬО! Немає вкладених інструкцій
//     }
// }

// /// Створює спеціальний вузол для While-циклу БЕЗ вкладених інструкцій
// fn create_flat_while_node(
//     decl: Arc<Decl>,
//     cond: &Expr,
//     body: &Arc<Decl>,
//     scope: &Scope,
// ) -> DeclarationMeta {
//     let deps = cond.extract_vars();
//     let indexes: Vec<(usize, Mutability)> =
//         deps.iter().filter_map(|d| env_lookup(scope, d)).collect();

//     let mut_deps: HashSet<usize> = indexes
//         .iter()
//         .filter(|(_, m)| *m == Mutability::Mutable)
//         .map(|(i, _)| *i)
//         .collect();

//     let flat_while_decl = Arc::new(Decl {
//         index: decl.index,
//         v: DeclType::Stmt(Arc::new(Stmt::While(cond.clone(), body.clone()))),
//     });

//     DeclarationMeta {
//         complexity: 2,
//         mut_deps,
//         class: Class::Loop,
//         decl: flat_while_decl,
//         loops_decls_indexes: vec![], // ПОРОЖНЬО!
//     }
// }

// // ============================================================================
// // MAIN ANALYZE
// // ============================================================================

// pub fn analyze(decls: &[Arc<Decl>]) -> Graph {
//     let (g, _, _) = analyze_rec(Vec::new(), DiGraph::new(), decls);
//     g
// }

// fn analyze_rec(mut scope: Scope, mut graph: Graph, decls: &[Arc<Decl>]) -> (Graph, Scope, usize) {
//     let mut last_index = 0;
//     for decl in decls.into_iter() {
//         let (g2, s2, index) = add_stmt(Arc::clone(decl), graph.clone(), scope.clone());
//         graph = g2;
//         scope = s2;
//         last_index = index;
//     }

//     (graph, scope, last_index)
// }

// fn add_stmt(decl: Arc<Decl>, mut graph: Graph, scope: Scope) -> (Graph, Scope, usize) {
//     let update_node = |graph: &mut Graph, meta: DeclarationMeta| {
//         let index = meta.decl.index;
//         if graph.raw_nodes().get(index).is_none() {
//             graph.add_node(Some(meta));
//         } else {
//             graph[NodeIndex::new(index)] = Some(meta);
//         }
//     };

//     let add_deps_to_graph =
//         |graph: &mut Graph, indexes: &[(usize, Mutability)], current_index: usize| {
//             for (dep, _) in indexes {
//                 while graph.node_count() - 1 < max(*dep, current_index) {
//                     graph.add_node(None);
//                 }
//                 graph.update_edge(NodeIndex::new(*dep), NodeIndex::new(current_index), ());
//             }
//         };

//     match &decl.v {
//         DeclType::VarDecl(name, expr, mutability) => {
//             let deps = expr.extract_vars();
//             let indexes: Vec<(usize, Mutability)> =
//                 deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

//             let mut_deps: HashSet<usize> = indexes
//                 .iter()
//                 .filter(|(_, m)| *m == Mutability::Mutable)
//                 .map(|(i, _)| *i)
//                 .collect();

//             let meta = DeclarationMeta {
//                 complexity: get_expr_complexity(expr),
//                 mut_deps,
//                 class: Class::Ordinary,
//                 decl: Arc::clone(&decl),
//                 loops_decls_indexes: vec![],
//             };

//             update_node(&mut graph, meta);
//             add_deps_to_graph(&mut graph, &indexes, decl.index);

//             let new_scope = env_declare(scope, name.to_string(), (decl.index, *mutability));
//             (graph, new_scope, decl.index)
//         }

//         DeclType::Stmt(arc_stmt) => {
//             match arc_stmt.as_ref() {
//                 Stmt::Print(expr) => {
//                     let deps = expr.extract_vars();
//                     let indexes: Vec<(usize, Mutability)> =
//                         deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

//                     let mut_deps = indexes
//                         .iter()
//                         .filter(|(_, m)| *m == Mutability::Mutable)
//                         .map(|(i, _)| *i)
//                         .collect();

//                     let meta = DeclarationMeta {
//                         complexity: 4,
//                         mut_deps,
//                         class: Class::Ordinary,
//                         decl: decl.clone(),
//                         loops_decls_indexes: vec![],
//                     };

//                     update_node(&mut graph, meta);
//                     add_deps_to_graph(&mut graph, &indexes, decl.index);

//                     (graph, scope, decl.index)
//                 }

//                 Stmt::Assign(name, expr) => {
//                     let deps = expr.extract_vars();
//                     let mut indexes: Vec<(usize, Mutability)> =
//                         deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();
//                     if let Some(lhs) = env_lookup(&scope, &name) {
//                         indexes.push(lhs);
//                     }

//                     let mut_deps: HashSet<usize> = indexes
//                         .iter()
//                         .filter(|(_, m)| *m == Mutability::Mutable)
//                         .map(|(i, _)| *i)
//                         .collect();

//                     let meta = DeclarationMeta {
//                         complexity: get_expr_complexity(expr),
//                         mut_deps,
//                         class: Class::Ordinary,
//                         decl: decl.clone(),
//                         loops_decls_indexes: vec![],
//                     };

//                     update_node(&mut graph, meta);
//                     add_deps_to_graph(&mut graph, &indexes, decl.index);

//                     let new_scope = env_assign(&scope, &name, decl.index)
//                         .unwrap_or_else(|| panic!("Variable {} should be declared", name));

//                     (graph, new_scope, decl.index)
//                 }

//                 Stmt::Block(stmts) => {
//                     let meta = DeclarationMeta {
//                         complexity: 0,
//                         mut_deps: HashSet::new(),
//                         class: Class::Ordinary,
//                         decl: decl.clone(),
//                         loops_decls_indexes: vec![],
//                     };

//                     update_node(&mut graph, meta);

//                     let new_scope = env_add_scope(scope.clone(), HashMap::new());
//                     let (g2, scope_after_block, index_after_block) =
//                         analyze_rec(new_scope, graph.clone(), stmts);

//                     let scope_after = env_remove_scope(scope_after_block);
//                     (g2, scope_after, index_after_block)
//                 }

//                 Stmt::Condition(cond, then_block, maybe_else) => {
//                     let deps = cond.extract_vars();
//                     let indexes: Vec<(usize, Mutability)> =
//                         deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

//                     let mut_deps: HashSet<usize> = indexes
//                         .iter()
//                         .filter(|(_, m)| *m == Mutability::Mutable)
//                         .map(|(i, _)| *i)
//                         .collect();

//                     let meta = DeclarationMeta {
//                         complexity: 1,
//                         mut_deps,
//                         class: Class::Ordinary,
//                         decl: decl.clone(),
//                         loops_decls_indexes: vec![],
//                     };

//                     update_node(&mut graph, meta);
//                     add_deps_to_graph(&mut graph, &indexes, decl.index);

//                     let then_scope = env_add_scope(scope.clone(), HashMap::new());
//                     let (g_then, scope_then_after, i_after_than) =
//                         analyze_rec(then_scope, graph.clone(), &[Arc::clone(then_block)]);

//                     let (g_else, scope_else_after, i_after_block) =
//                         if let Some(else_stmt) = maybe_else {
//                             let else_scope = env_add_scope(scope.clone(), HashMap::new());
//                             let (g_e, s_e_after, i_after_else) =
//                                 analyze_rec(else_scope, g_then.clone(), &[Arc::clone(else_stmt)]);
//                             (g_e, s_e_after, i_after_else)
//                         } else {
//                             (
//                                 g_then.clone(),
//                                 env_add_scope(scope.clone(), HashMap::new()),
//                                 i_after_than,
//                             )
//                         };

//                     let merged_scope =
//                         merge_scopes_preferring_larger(&scope_then_after, &scope_else_after);
//                     let clamped = apply_min_index_to_scope(&merged_scope, decl.index);
//                     let next_scope = env_remove_scope(clamped);

//                     let mut next_graph = g_else;
//                     redirect_edges_to_target(&mut next_graph, decl.index);
//                     next_graph.retain_edges(|g, e| {
//                         let edge = g.edge_endpoints(e).unwrap();
//                         edge.0 != edge.1
//                     });

//                     (next_graph, next_scope, i_after_block)
//                 }

//                 // ============================================================
//                 // WHILE - ПЛОСКИЙ ВУЗОЛ
//                 // ============================================================
//                 Stmt::While(cond, body) => {
//                     // 1. Створюємо плоский вузол для самого циклу
//                     let flat_meta = create_flat_while_node(decl.clone(), cond, body, &scope);
//                     let loop_index = decl.index;

//                     update_node(&mut graph, flat_meta.clone());

//                     let deps = cond.extract_vars();
//                     let indexes: Vec<(usize, Mutability)> =
//                         deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();
//                     add_deps_to_graph(&mut graph, &indexes, loop_index);

//                     // 2. Аналізуємо тіло циклу окремо (створює свої вузли в графі)
//                     let new_scope = env_add_scope(scope.clone(), HashMap::new());
//                     let (mut g_body, scope_after, last_body_index) =
//                         analyze_rec(new_scope, graph.clone(), &[Arc::clone(body)]);

//                     // 3. Додаємо залежності: всі вузли тіла залежать від циклу
//                     for node_idx in (loop_index + 1)..=last_body_index {
//                         if g_body.node_weight(NodeIndex::new(node_idx)).is_some() {
//                             g_body.update_edge(
//                                 NodeIndex::new(loop_index),
//                                 NodeIndex::new(node_idx),
//                                 (),
//                             );
//                         }
//                     }

//                     // 4. Додаємо зворотні залежності від змінних після ітерації
//                     let deps_after = cond.extract_vars();
//                     let indexes_after: Vec<(usize, Mutability)> = deps_after
//                         .iter()
//                         .filter_map(|d| env_lookup(&scope_after, d))
//                         .collect();

//                     for (dep, _) in &indexes_after {
//                         if *dep > loop_index && *dep <= last_body_index {
//                             g_body.update_edge(
//                                 NodeIndex::new(*dep),
//                                 NodeIndex::new(loop_index),
//                                 (),
//                             );
//                         }
//                     }

//                     let next_scope = env_remove_scope(scope_after);
//                     (g_body, next_scope, last_body_index)
//                 }

//                 // ============================================================
//                 // FOR - ПЛОСКИЙ ВУЗОЛ
//                 // ============================================================
//                 Stmt::For(var, arr, body) => {
//                     // 1. Створюємо плоский вузол для самого циклу
//                     let flat_meta = create_flat_for_node(decl.clone(), var, arr, body, &scope);
//                     let loop_index = decl.index;

//                     update_node(&mut graph, flat_meta);

//                     let deps = arr.extract_vars();
//                     let indexes: Vec<(usize, Mutability)> =
//                         deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();
//                     add_deps_to_graph(&mut graph, &indexes, loop_index);

//                     // 2. Аналізуємо тіло циклу окремо
//                     let mut top = HashMap::new();
//                     top.insert(var.to_string(), (loop_index, Mutability::Immutable));
//                     let s_with_loopvar = env_add_scope(scope.clone(), top);

//                     let (mut g_body, scope_after, last_body_index) =
//                         analyze_rec(s_with_loopvar, graph.clone(), &[Arc::clone(body)]);

//                     // 3. Додаємо залежності: всі вузли тіла залежать від циклу
//                     for node_idx in (loop_index + 1)..=last_body_index {
//                         if g_body.node_weight(NodeIndex::new(node_idx)).is_some() {
//                             g_body.update_edge(
//                                 NodeIndex::new(loop_index),
//                                 NodeIndex::new(node_idx),
//                                 (),
//                             );
//                         }
//                     }

//                     let next_scope = env_remove_scope(scope_after);
//                     (g_body, next_scope, last_body_index)
//                 }

//                 Stmt::Expression(expr) => {
//                     let deps = expr.extract_vars();
//                     let indexes: Vec<(usize, Mutability)> =
//                         deps.iter().filter_map(|d| env_lookup(&scope, d)).collect();

//                     let mut_deps: HashSet<usize> = indexes
//                         .iter()
//                         .filter(|(_, m)| *m == Mutability::Mutable)
//                         .map(|(i, _)| *i)
//                         .collect();

//                     let meta = DeclarationMeta {
//                         complexity: get_expr_complexity(expr),
//                         mut_deps,
//                         class: Class::Ordinary,
//                         decl: decl.clone(),
//                         loops_decls_indexes: vec![],
//                     };

//                     update_node(&mut graph, meta);
//                     add_deps_to_graph(&mut graph, &indexes, decl.index);

//                     (graph, scope, decl.index)
//                 }
//             }
//         }

//         DeclType::None => unreachable!(),
//     }
// }

// fn apply_min_index_to_scope(scope: &Scope, idx: usize) -> Scope {
//     scope
//         .iter()
//         .map(|map| {
//             map.iter()
//                 .map(|(k, (i, m))| (k.clone(), (std::cmp::min(*i, idx), m.clone())))
//                 .collect::<HashMap<Variable, (usize, Mutability)>>()
//         })
//         .collect::<Scope>()
// }

// fn redirect_edges_to_target(graph: &mut Graph, target_index: usize) {
//     let target = NodeIndex::new(target_index);

//     let edges_to_redirect: Vec<(NodeIndex, NodeIndex)> = graph
//         .edge_indices()
//         .filter_map(|eidx| {
//             let (src, dst) = graph.edge_endpoints(eidx)?;
//             if dst.index() > target_index || src.index() > target_index {
//                 Some((src, dst))
//             } else {
//                 None
//             }
//         })
//         .collect();

//     for (src, dst) in &edges_to_redirect {
//         if let Some(eidx) = graph.find_edge(*src, *dst) {
//             graph.remove_edge(eidx);
//         }
//     }

//     for (src, dst) in &edges_to_redirect {
//         graph.remove_node(*dst);
//         if *src < target {
//             graph.update_edge(*src, target, ());
//         } else if *src > target {
//             graph.remove_node(*src);
//         }
//     }
// }
