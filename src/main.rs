mod declaration_meta;
mod execution;
mod expr;
mod graph;
mod parallel_execution;
mod parser;
mod scheduler;
mod scheduler_parallel;
mod scope;
mod stat_manager;
mod stmt;
mod task_graph;
mod token;
mod worker_pool;

use std::fs;
use std::sync::LazyLock;

use clap::Parser;

const ARGS: LazyLock<Args> = LazyLock::new(|| Args::parse());

#[derive(Parser)]
#[command(author, version, about = "Parallel script execution engine with adaptive scheduling", long_about = None)]
struct Args {
    /// Maximum number of tasks to schedule per scheduling round
    #[arg(short = 'l', long, default_value_t = 100)]
    schedule_task_limit: usize,

    /// Minimum estimated duration (in ms) for a block to be executed in parallel
    #[arg(short = 'b', long, default_value_t = 400)]
    min_block_duration_ms: u64,

    /// Minimum estimated duration per iteration (in ms) for a loop to be parallelized
    #[arg(short = 'i', long, default_value_t = 100)]
    min_loop_iter_duration_ms: u64,

    /// Disable parallel execution and run sequentially
    #[arg(short = 'S', long)]
    sequential: bool,

    /// Number of worker threads (defaults to number of CPU cores)
    #[arg(short = 'w', long)]
    workers: Option<usize>,

    /// Disable statistical performance prediction and machine learning
    #[arg(short = 'M', long)]
    disable_ml_predictor: bool,

    /// Path to the script file to execute
    #[arg(short, long)]
    file: String,
}

fn main() {
    colog::init();

    // let args = Args::parse();

    // if args.len() < 2 {
    //     eprintln!("Usage: {} <filename>", args[0]);
    //     std::process::exit(1);
    // }

    let source = match fs::read_to_string(&ARGS.file) {
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

    let graph = graph.map(|_, meta| meta.clone().unwrap(), |_, w| *w);

    if !ARGS.sequential {
        if ARGS.disable_ml_predictor {
            let stat_manager = stat_manager::StatManager::new(graph.node_count());
            stat_manager.run();
        }

        match parallel_execution::execute_plan(graph, &decls) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Parallel execution error: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        match execution::execute(&decls) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Execution error: {}", e);
                std::process::exit(1);
            }
        }
    }
}
