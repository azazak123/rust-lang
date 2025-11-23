use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::declaration_meta::{Class, DeclarationMeta};

#[allow(dead_code)]
pub fn kahn_topsort(graph: &mut DiGraph<DeclarationMeta, ()>) -> Result<Vec<usize>, Vec<usize>> {
    let mut in_degree: HashMap<usize, usize> = HashMap::new();
    let mut result = Vec::new();

    let mut blacklisted = HashSet::new();

    for edge in graph.edge_references() {
        let target = edge.target();
        if target.index() <= edge.source().index() || graph[edge.source()].class == Class::Loop {
            blacklisted.insert(target.index());
        }
    }

    // Calculate in-degrees
    for node in graph.node_indices().map(|n| n.index()) {
        if blacklisted.contains(&node) {
            continue;
        }
        in_degree.insert(node, 0);
    }

    for edge in graph.edge_references() {
        let target = edge.target();
        let source = edge.source();
        if blacklisted.contains(&target.index()) || blacklisted.contains(&source.index()) {
            continue;
        }
        *in_degree.entry(target.index()).or_insert(0) += 1;
    }

    // Priority queue based on complexity (higher complexity first)
    let queue: VecDeque<(usize, usize)> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&node, _)| {
            let complexity = graph[NodeIndex::new(node)].complexity;
            (node, complexity)
        })
        .collect();

    // Sort by complexity (descending)
    let mut sorted_queue: Vec<_> = queue.into_iter().collect();
    sorted_queue.sort_by(|a, b| b.1.cmp(&a.1));
    let mut queue: VecDeque<_> = sorted_queue.into_iter().collect();

    while let Some((node, _)) = queue.pop_front() {
        result.push(node);

        // Find the node index
        if let Some(node_idx) = graph.node_indices().find(|&idx| idx.index() == node) {
            // Reduce in-degree for neighbors
            for edge in graph.edges(node_idx) {
                let target = edge.target().index();
                if blacklisted.contains(&target) {
                    continue;
                }
                if let Some(deg) = in_degree.get_mut(&target) {
                    // dbg!(*deg, target);
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        let complexity = graph[NodeIndex::new(target)].complexity;

                        // Insert in sorted order
                        let pos = queue
                            .iter()
                            .position(|(_, c)| *c < complexity)
                            .unwrap_or(queue.len());
                        queue.insert(pos, (target, complexity));
                    }
                }
            }
        }

        in_degree.remove(&node);
    }

    // let set = result.iter().copied().collect::<HashSet<_>>();

    // {
    //     for i in graph.node_indices().rev() {
    //         let mut decl = std::mem::take(&mut graph[i]);

    //         let block = match &mut decl.decl {
    //             DeclType::Stmt(Stmt::For(_, _, block)) => block,
    //             DeclType::Stmt(Stmt::While(_, block)) => block,
    //             _ => {
    //                 graph[i] = decl;
    //                 continue;
    //             }
    //         };

    //         let Stmt::Block(stmts) = block.as_mut() else {
    //             unreachable!();
    //         };

    //         let mut j = decl.loops_decls_indexes[0];
    //         while j <= decl.loops_decls_indexes[decl.loops_decls_indexes.len() - 1] {
    //             if !set.contains(&j) {
    //                 // читаємо graph іммутабельно — вже дозволено, бо decl тепер не позичений
    //                 let other = &graph[NodeIndex::new(j)];
    //                 stmts.push(other.decl.clone());
    //             }
    //             j += graph[NodeIndex::new(j)].loops_decls_indexes.len() + 1;
    //         }

    //         graph[i] = decl;
    //     }
    // }

    if in_degree.is_empty() {
        Ok(result)
    } else {
        Err(in_degree.keys().copied().collect())
    }
}
