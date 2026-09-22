use clap::Parser;
use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::Trainer;
use pkr_eval::TableEvaluator;
use pkr_export::writer::write_blueprint;
use rayon::ThreadPoolBuilder;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

    /// Path to a checkpoint file. If it exists on startup it is loaded and
    /// training resumes from the saved iteration. Written periodically.
    #[arg(long)]
    checkpoint: Option<PathBuf>,

    /// Write a checkpoint every N iterations (0 = never).
    #[arg(long, default_value_t = 10000)]
    checkpoint_every: u32,

    /// Maximum number of distinct infosets. Defaults to 5M (matches the
    /// production table). Lower for smoke tests.
    #[arg(long, default_value_t = 5_000_000)]
    capacity: usize,

    /// If > 0, ignore --iterations and run until this many seconds elapse.
    /// Used for throughput calibration.
    #[arg(long, default_value_t = 0)]
    bench_seconds: u64,
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    // On Apple Silicon, M1 has 4 P-cores + 4 slow E-cores. Scaling past
    // 4 threads makes wall time worse because the coordinator barrier
    // waits on the slowest (E-core) worker. Cap the default at 4.
    let num_threads = cli.threads.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|p| p.get().min(4))
            .unwrap_or(4)
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
        abstraction.load_street_centroids(1, path.to_str().unwrap()).expect("flop centroids");
    }
    if let Some(path) = &cli.turn_centroids {
        abstraction.load_street_centroids(2, path.to_str().unwrap()).expect("turn centroids");
    }
    if let Some(path) = &cli.river_centroids {
        abstraction.load_street_centroids(3, path.to_str().unwrap()).expect("river centroids");
    }
    if let Some(path) = &cli.preflop_table {
        abstraction.init_table(0, path.to_str().unwrap()).expect("preflop table");
    }
    if let Some(path) = &cli.flop_table {
        abstraction.init_table(1, path.to_str().unwrap()).expect("flop table");
    }
    if let Some(path) = &cli.turn_table {
        abstraction.init_table(2, path.to_str().unwrap()).expect("turn table");
    }
    if let Some(path) = &cli.flop_buckets {
        abstraction.load_flop_buckets(path.to_str().unwrap()).expect("flop buckets");
    }
    if let Some(path) = &cli.river_table {
        abstraction.init_table(3, path.to_str().unwrap()).expect("river table");
    }

    let abstraction = Arc::new(abstraction);
    let mut trainer = Trainer::with_capacity(abstraction, evaluator, cli.capacity);

    let start_iter = if let Some(ckpt) = &cli.checkpoint {
        if ckpt.exists() {
            match trainer.load_checkpoint(ckpt.to_str().unwrap()) {
                Ok(()) => {
                    let it = trainer.iteration();
                    eprintln!("Resumed from checkpoint at iteration {}", it);
                    it
                }
                Err(e) => {
                    eprintln!("WARNING: failed to load checkpoint: {} — starting fresh", e);
                    0
                }
            }
        } else {
            0
        }
    } else {
        0
    };

    let start = Instant::now();
    let mut last_ckpt_iter = start_iter;
    let mut last_report_iter = start_iter;
    let mut stopped_early = false;
    let bench_deadline = if cli.bench_seconds > 0 {
        Some(Duration::from_secs(cli.bench_seconds))
    } else {
        None
    };
    let max_iters = if bench_deadline.is_some() {
        u32::MAX
    } else {
        cli.iterations
    };

    // Logical CFR iterations per rayon dispatch. Larger batches amortize
    // the serial merge+flush further, at the cost of slightly staler
    // discount-schedule timing (DCFR tolerates this well). See
    // docs/status.md and the run_iterations_parallel doc comment.
    const ITERS_PER_SYNC: u32 = 64;

    let mut done = start_iter;
    while done < max_iters {
        if let Some(d) = bench_deadline {
            if start.elapsed() >= d {
                stopped_early = true;
                break;
            }
        }
        if trainer.is_near_capacity() {
            eprintln!(
                "WARN: table near capacity ({} infosets), stopping early",
                trainer.get_table().len()
            );
            stopped_early = true;
            break;
        }

        let batch = ITERS_PER_SYNC.min(max_iters - done);
        trainer.run_iterations_parallel(batch as usize);
        done += batch;

        if done.saturating_sub(last_report_iter) >= 1000 || done == max_iters {
            let elapsed = start.elapsed().as_secs_f64().max(1e-6);
            let iters_done = done.saturating_sub(start_iter).max(1) as f64;
            let rate = iters_done / elapsed;
            let remaining = (max_iters.saturating_sub(done)) as f64;
            let eta_s = remaining / rate.max(1e-6);
            let infosets = trainer.get_table().len();
            eprintln!(
                "iter {}/{} | infosets: {} | {:.1} it/s | ETA {:.2}h",
                done, max_iters, infosets, rate, eta_s / 3600.0
            );
            last_report_iter = done;
        }

        if cli.checkpoint_every > 0
            && done != last_ckpt_iter
            && (done - last_ckpt_iter) >= cli.checkpoint_every
        {
            if let Some(ckpt) = &cli.checkpoint {
                match trainer.save_checkpoint(ckpt.to_str().unwrap()) {
                    Ok(()) => {
                        eprintln!("Checkpoint written at iteration {}", done);
                        last_ckpt_iter = done;
                    }
                    Err(e) => eprintln!("WARNING: checkpoint failed: {}", e),
                }
            }
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    let total_iters = trainer.iteration().saturating_sub(start_iter);
    let rate = total_iters as f64 / elapsed.max(1e-6);
    eprintln!(
        "BENCH threads={} capacity={} elapsed={:.2}s iterations={} it/s={:.1} infosets={}",
        num_threads,
        cli.capacity,
        elapsed,
        total_iters,
        rate,
        trainer.get_table().len()
    );

    if bench_deadline.is_some() {
        // Skip export in bench mode — we only wanted the throughput number.
        return;
    }
    eprintln!("Training done in {:.1}s", elapsed);

    let mut keys = trainer.get_table().get_keys();
    keys.sort_unstable();
    eprintln!("Exporting {} infosets...", keys.len());

    let output_path = cli.output.to_str().expect("invalid output path");
    write_blueprint(output_path, trainer.get_table(), &keys);
    eprintln!("Blueprint written to {}", output_path);
}
