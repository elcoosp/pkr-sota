use clap::Parser;
use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::Trainer;
use pkr_eval::TableEvaluator;
use pkr_export::writer::write_blueprint;
use rayon::ThreadPoolBuilder;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

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

    let num_threads = cli.threads.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(8)
    });

    ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .stack_size(32 * 1024 * 1024)
        .build_global()
        .expect("Failed to initialize global Rayon pool");

    eprintln!("Running with {} threads", num_threads);

    let evaluator =
        Arc::new(TableEvaluator::new(&cli.rank_table).expect("Failed to load hand_ranks.bin"));

    let store =
        load_centroids(cli.centroids.to_str().unwrap()).expect("Failed to load default centroids");
    let mut abstraction = KMeansAbstraction::from_store(store, evaluator.clone());

    if let Some(path) = &cli.flop_centroids {
        abstraction
            .load_street_centroids(1, path.to_str().unwrap())
            .expect("Failed to load flop centroids");
    }
    if let Some(path) = &cli.turn_centroids {
        abstraction
            .load_street_centroids(2, path.to_str().unwrap())
            .expect("Failed to load turn centroids");
    }
    if let Some(path) = &cli.river_centroids {
        abstraction
            .load_street_centroids(3, path.to_str().unwrap())
            .expect("Failed to load river centroids");
    }

    if let Some(path) = &cli.preflop_table {
        abstraction
            .init_table(0, path.to_str().unwrap())
            .expect("Failed to load preflop table");
    }
    if let Some(path) = &cli.flop_table {
        abstraction
            .init_table(1, path.to_str().unwrap())
            .expect("Failed to load flop table");
    }
    if let Some(path) = &cli.turn_table {
        abstraction
            .init_table(2, path.to_str().unwrap())
            .expect("Failed to load turn table");
    }
    if let Some(path) = &cli.flop_buckets {
        abstraction
            .load_flop_buckets(path.to_str().unwrap())
            .expect("Failed to load flop buckets");
    }
    if let Some(path) = &cli.river_table {
        abstraction
            .init_table(3, path.to_str().unwrap())
            .expect("Failed to load river table");
    }

    let abstraction = Arc::new(abstraction);
    let mut trainer = Trainer::new(abstraction, evaluator);

    let start = Instant::now();
    for i in 0..cli.iterations {
        if i % 1000 == 0 {
            let elapsed = start.elapsed().as_secs_f64();
            let iters_done = i.max(1) as f64;
            let rate = iters_done / elapsed.max(1e-6);
            let remaining = (cli.iterations - i) as f64;
            let eta_s = remaining / rate.max(1e-6);
            let infosets = trainer.get_table().len();
            eprintln!(
                "iter {}/{} | infosets: {} | {:.1} it/s | ETA {:.2}h",
                i,
                cli.iterations,
                infosets,
                rate,
                eta_s / 3600.0
            );
        }
        trainer.run_iteration_parallel();
    }

    eprintln!("Training done in {:.1}s", start.elapsed().as_secs_f64());

    let mut keys = trainer.get_table().get_keys();
    keys.sort_unstable();
    eprintln!("Exporting {} infosets...", keys.len());

    let output_path = cli.output.to_str().expect("invalid output path");
    write_blueprint(output_path, trainer.get_table(), &keys);
    eprintln!("Blueprint written to {}", output_path);
}
