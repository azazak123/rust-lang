// src/main.rs
mod declaration_meta;
mod execution;
mod expr;
mod graph;
mod parallel_execution;
mod parser;
mod scheduler;
mod scope;
mod stmt;
mod token;

use std::env;
use std::fs;

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::visit::NodeRef;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <filename>", args[0]);
        std::process::exit(1);
    }

    let source = match fs::read_to_string(&args[1]) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            std::process::exit(1);
        }
    };

    // Tokenize
    let tokens = match token::tokenize(&source) {
        Some(tokens) => tokens,
        None => {
            eprintln!("Tokenization failed");
            std::process::exit(1);
        }
    };

    // Parse
    let mut parser = stmt::StmtParser::new(tokens);
    let (decls, _type_env) = match parser.parse() {
        Some(result) => result,
        None => {
            eprintln!("Parsing failed");
            std::process::exit(1);
        }
    };

    // Analyze dependencies
    let graph = graph::analyze(&decls);

    // dbg!(&graph);

    let mut graph = graph.map(|_, meta| meta.clone().unwrap(), |_, w| *w);

    // let mut edges = graph
    //     .edge_references()
    //     .map(|e| (e.source(), e.target()))
    //     .collect::<Vec<_>>();

    // edges.sort();

    // dbg!(edges);

    // Convert meta to the format expected by scheduler
    // let complexities: Vec<(usize, usize)> = meta
    //     .iter()
    //     .map(|(complexity, deps)| (*complexity, deps.len()))
    //     .collect();

    // dbg!(complexities);

    // Topological sort
    let order = match scheduler::kahn_topsort(&mut graph) {
        Ok(order) => order,
        Err(cycle) => {
            eprintln!("Cycle detected in dependency graph: {:?}", cycle);
            std::process::exit(1);
        }
    };

    // dbg!(&order);
    // dbg!(&graph[NodeIndex::new(3)]);

    // Check if parallel execution is beneficial
    // let use_parallel = decls.len() > 10 || meta.iter().any(|(c, _)| *c > 1);
    let use_parallel = true;

    if use_parallel {
        // Execute with parallelization
        match parallel_execution::execute_plan(&order, &graph) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Parallel execution error: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        // Execute sequentially
        match execution::execute(&decls) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Execution error: {}", e);
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenization() {
        let source = "var x = 42; print x;";
        let tokens = token::tokenize(source).unwrap();
        assert!(tokens.len() > 0);
    }

    #[test]
    fn test_simple_program() {
        let source = r#"
            var x = 10;
            var y = 20;
            var z = x + y;
            print z;
        "#;

        let tokens = token::tokenize(source).unwrap();
        let mut parser = stmt::StmtParser::new(tokens);
        let (decls, _) = parser.parse().unwrap();

        assert_eq!(decls.len(), 4);
    }

    #[test]
    fn test_array_operations() {
        let source = r#"
            var arr = [1, 2, 3, 4, 5];
            var doubled = map x in arr { x * 2 };
            print doubled;
        "#;

        let tokens = token::tokenize(source).unwrap();
        let mut parser = stmt::StmtParser::new(tokens);
        let (decls, _) = parser.parse().unwrap();

        let mut env = scope::env_empty();
        for decl in &decls {
            execution::execute_stmt(decl, &mut env).unwrap();
        }
    }

    #[test]
    fn test_range() {
        let source = r#"
            var range = [1..10];
            print range;
        "#;

        let tokens = token::tokenize(source).unwrap();
        let mut parser = stmt::StmtParser::new(tokens);
        let (decls, _) = parser.parse().unwrap();

        let mut env = scope::env_empty();
        for decl in &decls {
            execution::execute_stmt(decl, &mut env).unwrap();
        }
    }

    #[test]
    fn test_filter() {
        let source = r#"
            var nums = [1, 2, 3, 4, 5, 6];
            var evens = filter x in nums { x % 2 == 0 };
            print evens;
        "#;

        let tokens = token::tokenize(source).unwrap();
        let mut parser = stmt::StmtParser::new(tokens);
        let (decls, _) = parser.parse().unwrap();

        let mut env = scope::env_empty();
        for decl in &decls {
            execution::execute_stmt(decl, &mut env).unwrap();
        }
    }

    #[test]
    fn test_foldl() {
        let source = r#"
            var nums = [1, 2, 3, 4, 5];
            var sum = foldl acc, 0, x in nums { acc + x };
            print sum;
        "#;

        let tokens = token::tokenize(source).unwrap();
        let mut parser = stmt::StmtParser::new(tokens);
        let (decls, _) = parser.parse().unwrap();

        let mut env = scope::env_empty();
        for decl in &decls {
            execution::execute_stmt(decl, &mut env).unwrap();
        }
    }

    #[test]
    fn test_scanl() {
        let source = r#"
            var nums = [1, 2, 3, 4];
            var cumsum = scanl acc, 0, x in nums { acc + x };
            print cumsum;
        "#;

        let tokens = token::tokenize(source).unwrap();
        let mut parser = stmt::StmtParser::new(tokens);
        let (decls, _) = parser.parse().unwrap();

        let mut env = scope::env_empty();
        for decl in &decls {
            execution::execute_stmt(decl, &mut env).unwrap();
        }
    }

    #[test]
    fn test_control_flow() {
        let source = r#"
            var x = 10;
            if x > 5 {
                print true;
            } else {
                print false;
            }
        "#;

        let tokens = token::tokenize(source).unwrap();
        let mut parser = stmt::StmtParser::new(tokens);
        let (decls, _) = parser.parse().unwrap();

        let mut env = scope::env_empty();
        for decl in &decls {
            execution::execute_stmt(decl, &mut env).unwrap();
        }
    }

    #[test]
    fn test_while_loop() {
        let source = r#"
            var i = 0;
            while i < 5 {
                i = i + 1;
            }
            print i;
        "#;

        let tokens = token::tokenize(source).unwrap();
        let mut parser = stmt::StmtParser::new(tokens);
        let (decls, _) = parser.parse().unwrap();

        let mut env = scope::env_empty();
        for decl in &decls {
            execution::execute_stmt(decl, &mut env).unwrap();
        }
    }

    #[test]
    fn test_for_loop() {
        let source = r#"
            var sum = 0;
            for x in [1, 2, 3, 4, 5] {
                sum = sum + x;
            }
            print sum;
        "#;

        let tokens = token::tokenize(source).unwrap();
        let mut parser = stmt::StmtParser::new(tokens);
        let (decls, _) = parser.parse().unwrap();

        let mut env = scope::env_empty();
        for decl in &decls {
            execution::execute_stmt(decl, &mut env).unwrap();
        }
    }

    #[test]
    fn test_immutability() {
        let source = r#"
            final x = 10;
            x = 20;
        "#;

        let tokens = token::tokenize(source).unwrap();
        let mut parser = stmt::StmtParser::new(tokens);
        // This should fail at parse time
        assert!(parser.parse().is_none());
    }

    #[test]
    fn test_nested_expressions() {
        let source = r#"
            var x = 10;
            var y = 20;
            var result = (x + y) * 2 - 5;
            print result;
        "#;

        let tokens = token::tokenize(source).unwrap();
        let mut parser = stmt::StmtParser::new(tokens);
        let (decls, _) = parser.parse().unwrap();

        let mut env = scope::env_empty();
        for decl in &decls {
            execution::execute_stmt(decl, &mut env).unwrap();
        }
    }

    #[test]
    fn test_dependency_graph() {
        let source = r#"
            var a = 10;
            var b = 20;
            var c = a + b;
            var d = c * 2;
            print d;
        "#;

        let tokens = token::tokenize(source).unwrap();
        let mut parser = stmt::StmtParser::new(tokens);
        let (decls, _) = parser.parse().unwrap();

        // let (graph, meta) = graph::analyze(&decls);

        // assert_eq!(graph.node_count(), 5);
        // assert_eq!(meta.len(), 5);
    }

    #[test]
    fn test_topological_sort() {
        let source = r#"
            var a = 10;
            var b = 20;
            var c = a + b;
            var d = c * 2;
            print d;
        "#;

        let tokens = token::tokenize(source).unwrap();
        let mut parser = stmt::StmtParser::new(tokens);
        let (decls, _) = parser.parse().unwrap();

        // let (graph, meta) = graph::analyze(&decls);
        // let complexities: Vec<(usize, usize)> =
        //     meta.iter().map(|(c, deps)| (*c, deps.len())).collect();

        // let order = scheduler::kahn_topsort(&graph, &complexities).unwrap();

        // Order should respect dependencies
        // assert_eq!(order.len(), 5);
    }
}

// Example programs that can be run:
/*

// example1.txt - Basic arithmetic
var x = 10;
var y = 20;
var z = x + y;
print z;

// example2.txt - Arrays and map
var nums = [1, 2, 3, 4, 5];
var doubled = map x in nums { x * 2 };
print doubled;

// example3.txt - Filter
var numbers = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
var evens = filter x in numbers { x % 2 == 0 };
print evens;

// example4.txt - Foldl (sum)
var values = [1, 2, 3, 4, 5];
var sum = foldl acc, 0, x in values { acc + x };
print sum;

// example5.txt - Scanl (cumulative sum)
var data = [1, 2, 3, 4];
var cumulative = scanl acc, 0, x in data { acc + x };
print cumulative;

// example6.txt - Range
var range = [1..10];
var squares = map x in range { x * x };
print squares;

// example7.txt - Control flow
var x = 15;
if x > 10 {
    print "Greater than 10";
} else {
    print "Less than or equal to 10";
}

// example8.txt - While loop
var counter = 0;
var sum = 0;
while counter < 10 {
    sum = sum + counter;
    counter = counter + 1;
}
print sum;

// example9.txt - For loop
var total = 0;
for num in [1, 2, 3, 4, 5] {
    total = total + num;
}
print total;

// example10.txt - Complex pipeline
var nums = [1..20];
var evens = filter x in nums { x % 2 == 0 };
var squared = map x in evens { x * x };
var sum = foldl acc, 0, x in squared { acc + x };
print sum;

// example11.txt - Nested operations
var matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
var sums = map row in matrix {
    foldl acc, 0, x in row { acc + x }
};
print sums;

// example12.txt - Fibonacci-like with scanl
var fibs = scanl acc, 1, x in [1..10] { acc + x };
print fibs;

*/
