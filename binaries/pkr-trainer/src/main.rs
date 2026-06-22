use clap::Parser;
use pkr_abstraction::{KMeansAbstraction, load_centroids};
use pkr_cfr::Trainer;
use pkr_eval::lookup::TableEvaluator;
use pkr_export::writer::write_blueprint;
use rayon::ThreadPoolBuilder;
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

    #[arg(long)]
    flop_centroids: Option<PathBuf>,

    #[arg(long)]
    turn_centroids: Option<PathBuf>,

    #[arg(long)]
    river_centroids: Option<PathBuf>,

    #[arg(long)]
    preflop_table: Option<PathBuf>,

    #[arg(long)]
    flop_table: Option<PathBuf>,

    #[arg(long)]
    turn_table: Option<PathBuf>,

    #[arg(long)]
    river_table: Option<PathBuf>,
    #[arg(long)]
    flop_buckets: Option<PathBuf>,


    #[arg(long, default_value = "hand_ranks.bin")]
    rank_table: PathBuf,

    #[arg(long)]
    threads: Option<usize>,
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    // 1. Initialize the Rayon thread pool ONCE.
    let num_threads = cli.threads
        .unwrap_or_else(|| std::thread::available_parallelism().map(|p| p.get()).unwrap_or(8));

    ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build_global()
        .expect("Failed to initialize global Rayon pool");

    eprintln!("Running with {} threads", num_threads);

    // 2. Load the FAST evaluator instead of NlheEvaluator
    let evaluator = Arc::new(
        TableEvaluator::new(&cli.rank_table).expect("Failed to load hand_ranks.bin")
    );

    let store = load_centroids(cli.centroids.to_str().unwrap())
        .expect("Failed to load default centroids");
    let mut abstraction = KMeansAbstraction::from_store(store, evaluator.clone());

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

    // Load tables using &self init_table
    if let Some(path) = &cli.preflop_table {
        abstraction.init_table(0, path.to_str().unwrap())
            .expect("Failed to load preflop table");
    }
    if let Some(path) = &cli.flop_table {
        abstraction.init_table(1, path.to_str().unwrap())
            .expect("Failed to load flop table");
    }
    if let Some(path) = &cli.turn_table {
        abstraction.init_table(2, path.to_str().unwrap())
            .expect("Failed to load turn table");
    }
    if let Some(path) = &cli.flop_buckets {
        abstraction.load_flop_buckets(path.to_str().unwrap())
            .expect("Failed to load flop buckets");
    }

    if let Some(path) = &cli.river_table {
        abstraction.init_table(3, path.to_str().unwrap())
            .expect("Failed to load river table");
    }

    let abstraction = Arc::new(abstraction);

    // 3. Trainer no longer takes num_threads; it uses the global Rayon pool.
    let mut trainer = Trainer::new(abstraction, evaluator);

    for i in 0..cli.iterations {
        if i % 1000 == 0 {
            eprintln!("Iteration {}/{}", i, cli.iterations);
        }
        trainer.run_iteration_parallel();
    }

    let output_path = cli.output.to_str().expect("invalid output path");
    write_blueprint(output_path, trainer.get_table());
    eprintln!("Blueprint written to {}", output_path);
}
