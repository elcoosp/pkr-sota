use clap::Parser;
use pkr_abstraction::{KMeansAbstraction, load_centroids};
use pkr_cfr::Trainer;
use pkr_eval::NlheEvaluator;
use pkr_export::writer::write_blueprint;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "pkr-trainer")]
struct Cli {
    #[arg(long, default_value_t = 100000)]
    iterations: u32,

    #[arg(long, default_value = "blueprint.bin")]
    output: PathBuf,

    /// Default centroids (used for any street not explicitly provided)
    #[arg(long, default_value = "centroids.bin")]
    centroids: PathBuf,

    /// Optional: flop-specific centroids
    #[arg(long)]
    flop_centroids: Option<PathBuf>,

    /// Optional: turn-specific centroids
    #[arg(long)]
    turn_centroids: Option<PathBuf>,

    /// Optional: river-specific centroids
    #[arg(long)]
    river_centroids: Option<PathBuf>,

    /// Precomputed abstraction table for flop
    #[arg(long)]
    flop_table: Option<PathBuf>,

    /// Precomputed abstraction table for turn
    #[arg(long)]
    turn_table: Option<PathBuf>,

    /// Precomputed abstraction table for river
    #[arg(long)]
    river_table: Option<PathBuf>,

    #[arg(long)]
    threads: Option<usize>,
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    let num_threads = cli.threads
        .unwrap_or_else(|| std::thread::available_parallelism().map(|p| p.get()).unwrap_or(4));
    let evaluator = Arc::new(NlheEvaluator);

    // Load default centroids
    let store = load_centroids(cli.centroids.to_str().unwrap())
        .expect("Failed to load default centroids");
    let mut abstraction = KMeansAbstraction::from_store(store, evaluator.clone());

    // Load per-street centroids
    if let Some(path) = &cli.flop_centroids {
        abstraction.load_street_centroids(1, path.to_str().unwrap())
            .expect("Failed to load flop centroids");
    }
    if let Some(path) = &cli.turn_centroids {
        abstraction.load_street_centroids(2, path.to_str().unwrap())
            .expect("Failed to load turn centroids");
    }
    if let Some(path) = &cli.river_centroids {
        abstraction.load_street_centroids(3, path.to_str().unwrap())
            .expect("Failed to load river centroids");
    }

    // Load precomputed tables
    if let Some(path) = &cli.flop_table {
        abstraction.load_street_table(1, path.to_str().unwrap())
            .expect("Failed to load flop table");
    }
    if let Some(path) = &cli.turn_table {
        abstraction.load_street_table(2, path.to_str().unwrap())
            .expect("Failed to load turn table");
    }
    if let Some(path) = &cli.river_table {
        abstraction.load_street_table(3, path.to_str().unwrap())
            .expect("Failed to load river table");
    }

    let abstraction = Arc::new(abstraction);
    let mut trainer = Trainer::new(abstraction, evaluator, 6); // K=6 abstract actions

    for i in 0..cli.iterations {
        if i % 1000 == 0 {
            eprintln!("Iteration {}/{}", i, cli.iterations);
        }
        trainer.run_iteration_parallel(num_threads);
    }

    let output_path = cli.output.to_str().expect("invalid output path");
    write_blueprint(output_path, trainer.get_table());
    eprintln!("Blueprint written to {}", output_path);
}
