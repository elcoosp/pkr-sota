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

    #[arg(long, default_value = "centroids.bin")]
    centroids: PathBuf,

    #[arg(long, default_value = "abstraction.bin")]
    abstraction_table: PathBuf,

    #[arg(long)]
    threads: Option<usize>,
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    let num_threads = cli.threads
        .unwrap_or_else(|| std::thread::available_parallelism().map(|p| p.get()).unwrap_or(4));
    let evaluator = Arc::new(NlheEvaluator);

    // Load centroids
    let store = load_centroids(cli.centroids.to_str().unwrap())
        .expect("Failed to load centroids");
    let mut abstraction = KMeansAbstraction::from_store(store, evaluator.clone());

    // Load precomputed flop abstraction table
    if let Err(e) = abstraction.load_table(cli.abstraction_table.to_str().unwrap()) {
        eprintln!("Warning: couldn't load abstraction table: {}. Falling back to MC.", e);
    }

    let abstraction = Arc::new(abstraction);
    let mut trainer = Trainer::new(abstraction, evaluator, 4);

    let mut rng = rand::rng();
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
