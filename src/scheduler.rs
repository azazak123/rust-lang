use petgraph::graph::DiGraph;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, VecDeque};

pub fn kahn_topsort(
    graph: &DiGraph<(), ()>,
    complexities: &[(usize, usize)],
) -> Result<Vec<usize>, Vec<usize>> {
    let mut in_degree: HashMap<usize, usize> = HashMap::new();
    let mut result = Vec::new();

    // Calculate in-degrees
    for node in graph.node_indices() {
        in_degree.insert(node.index(), 0);
    }

    for edge in graph.edge_references() {
        let target = edge.target();
        *in_degree.entry(target.index()).or_insert(0) += 1;
    }

    // Priority queue based on complexity (higher complexity first)
    let queue: VecDeque<(usize, usize)> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&node, _)| {
            let complexity = if node < complexities.len() {
                complexities[node].0
            } else {
                0
            };
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
                if let Some(deg) = in_degree.get_mut(&target) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        let complexity = if target < complexities.len() {
                            complexities[target].0
                        } else {
                            0
                        };

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

    if in_degree.is_empty() {
        Ok(result)
    } else {
        Err(in_degree.keys().copied().collect())
    }
}
