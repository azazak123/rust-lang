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
struct Args {
    #[arg(short, long, default_value_t = 100)]
    schedule_task_limit: usize,

    #[arg(short, long, default_value_t = 400)]
    duration_to_par_ms: u64,

    #[arg(short, long, default_value_t = 100)]
    duration_to_par_loop_iter_ms: u64,

    #[arg(short, long)]
    no_par: bool,

    #[arg(short, long)]
    n_workers: Option<usize>,

    #[arg(short, long)]
    no_use_stat_manager: bool,

    #[arg(short, long)]
    script: String,
}

fn main() {
    colog::init();

    // let args = Args::parse();

    // if args.len() < 2 {
    //     eprintln!("Usage: {} <filename>", args[0]);
    //     std::process::exit(1);
    // }

    let source = match fs::read_to_string(&ARGS.script) {
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

    if !ARGS.no_par {
        if ARGS.no_use_stat_manager {
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
