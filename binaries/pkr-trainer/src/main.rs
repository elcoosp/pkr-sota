use clap::Parser;
use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::Trainer;
use pkr_eval::TableEvaluator;
use pkr_export::writer::write_blueprint;
use rayon::ThreadPoolBuilder;
use std::io::Write;
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

    #[arg(long)]
    checkpoint: Option<PathBuf>,

    #[arg(long, default_value_t = 10000)]
    checkpoint_every: u32,

    #[arg(long, default_value_t = 5_000_000)]
    capacity: usize,

    #[arg(long, default_value_t = 0)]
    bench_seconds: u64,

    /// Write a CSV row per report interval with CFR health + timing.
    #[arg(long)]
    metrics_csv: Option<PathBuf>,

    /// Write a JSON summary with strategy analysis and sampled infosets
    /// at the end of training.
    #[arg(long)]
    stats_json: Option<PathBuf>,

    /// Iterations between reports (CSV row + progress print).
    #[arg(long, default_value_t = 10000)]
    report_every: u32,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("FATAL: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
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

    let store = load_centroids(cli.centroids.to_str().unwrap())
        .expect("Failed to load default centroids");
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
    let t_init = Instant::now();
    let mut trainer = Trainer::with_capacity(abstraction, evaluator, cli.capacity);
    eprintln!(
        "init: table + abstraction ready in {:.2}s (capacity={})",
        t_init.elapsed().as_secs_f64(),
        cli.capacity
    );

    // Optional resume.
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

    // CSV writer for live metrics.
    let mut csv_writer: Option<std::io::BufWriter<std::fs::File>> = match &cli.metrics_csv {
        Some(path) => {
            let f = std::fs::File::create(path)?;
            let mut w = std::io::BufWriter::new(f);
            writeln!(
                w,
                "iter,wall_s,it_per_s,infosets,cap_pct,max_abs_regret,\
                 mean_abs_regret,nonfinite,strat_mass,\
                 nodes,nodes_per_iter,avg_depth,max_depth,cache_hit_rate,\
                 regret_in,regret_out,regret_dedup,strategy_applied,\
                 traverse_ms,merge_ms,flush_ms,wall_ms"
            )?;
            w.flush()?;
            Some(w)
        }
        None => None,
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
    // the serial merge + flush further, at the cost of slightly staler
    // discount-schedule timing (DCFR tolerates this well).
    const ITERS_PER_SYNC: u32 = 256;

    let mut done = start_iter;

    // Reset the global metrics counters so window deltas start at zero.
    let mut prev_metrics_snapshot = pkr_cfr::metrics::global().snapshot();

    while done < max_iters {
        if let Some(d) = bench_deadline {
            if start.elapsed() >= d {
                stopped_early = true;
                break;
            }
        }

        let batch = ITERS_PER_SYNC.min(max_iters - done);
        trainer.run_iterations_parallel(batch as usize);
        done += batch;

        let should_report =
            done.saturating_sub(last_report_iter) >= cli.report_every || done == max_iters;

        if should_report {
            let elapsed = start.elapsed().as_secs_f64().max(1e-6);
            let iters_done = done.saturating_sub(start_iter).max(1) as f64;
            let rate = iters_done / elapsed;
            let remaining = (max_iters.saturating_sub(done)) as f64;
            let eta_s = remaining / rate.max(1e-6);

            let snap = trainer.get_table().snapshot();
            let cap_pct = if snap.capacity > 0 {
                100.0 * snap.infosets as f64 / snap.capacity as f64
            } else {
                0.0
            };

            // Window delta of the global metrics since last report.
            let cur_metrics = pkr_cfr::metrics::global().snapshot();
            let delta = cur_metrics.delta(&prev_metrics_snapshot);

            let nodes_per_iter = if delta.iterations > 0 {
                delta.nodes as f64 / delta.iterations as f64
            } else {
                0.0
            };
            let cache_hit_rate = delta.cache_hit_rate();
            let regret_dedup = delta.regret_dedup_ratio();
            let traverse_ms = delta.traverse_ns as f64 / 1.0e6;
            let merge_ms = delta.merge_ns as f64 / 1.0e6;
            let flush_ms = delta.flush_ns as f64 / 1.0e6;
            let wall_ms = delta.wall_ns as f64 / 1.0e6;

            eprintln!(
                "iter {}/{} | infosets: {} ({:.1}%) | {:.1} it/s | ETA {:.2}h | \
                 max|r|={:.2e} nonfinite={} | cache_hit={:.3} dedup={:.3} | \
                 nodes/it={:.0} depth_avg={:.1}",
                done,
                max_iters,
                snap.infosets,
                cap_pct,
                rate,
                eta_s / 3600.0,
                snap.max_abs_regret,
                snap.nonfinite_count,
                cache_hit_rate,
                regret_dedup,
                nodes_per_iter,
                delta.avg_depth(),
            );

            if let Some(w) = csv_writer.as_mut() {
                writeln!(
                    w,
                    "{},{:.3},{:.1},{},{:.3},{:.6e},{:.6e},{},{:.6e},\
                     {},{:.1},{:.3},{},{:.4},\
                     {},{},{:.4},{},\
                     {:.3},{:.3},{:.3},{:.3}",
                    done,
                    elapsed,
                    rate,
                    snap.infosets,
                    cap_pct,
                    snap.max_abs_regret,
                    snap.mean_abs_regret,
                    snap.nonfinite_count,
                    snap.strategy_sum_mass,
                    delta.nodes,
                    nodes_per_iter,
                    delta.avg_depth(),
                    delta.max_depth,
                    cache_hit_rate,
                    delta.regret_input,
                    delta.regret_unique,
                    regret_dedup,
                    delta.strategy_applied,
                    traverse_ms,
                    merge_ms,
                    flush_ms,
                    wall_ms,
                )?;
                w.flush()?;
            }

            prev_metrics_snapshot = cur_metrics;
            last_report_iter = done;

            if trainer.is_near_capacity() {
                eprintln!(
                    "WARN: table near capacity ({} infosets), stopping early",
                    snap.infosets
                );
                stopped_early = true;
                break;
            }
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

    if !stopped_early {
        if let Some(ckpt) = &cli.checkpoint {
            if let Err(e) = trainer.save_checkpoint(ckpt.to_str().unwrap()) {
                eprintln!("WARNING: final checkpoint failed: {}", e);
            }
        }
    }

    let elapsed_total = start.elapsed().as_secs_f64();
    let total_iters = trainer.iteration().saturating_sub(start_iter);
    eprintln!(
        "Training done: {} iterations in {:.1}s ({:.1} it/s)",
        total_iters,
        elapsed_total,
        total_iters as f64 / elapsed_total.max(1e-6)
    );

    if let Some(path) = &cli.stats_json {
        eprintln!("computing final stats ...");
        let snap = trainer.get_table().snapshot();
        let analysis = trainer.get_table().analyze_strategies();
        let samples = trainer.get_table().sample_infosets(200);
        let cumulative = pkr_cfr::metrics::global().snapshot();

        let mut json = String::with_capacity(1 << 20);
        json.push_str("{\n");
        json.push_str("  \"config\": {\n");
        json.push_str(&format!("    \"iterations\": {},\n", cli.iterations));
        json.push_str(&format!("    \"threads\": {},\n", num_threads));
        json.push_str(&format!("    \"capacity\": {},\n", cli.capacity));
        json.push_str(&format!("    \"iters_per_sync\": {},\n", ITERS_PER_SYNC));
        json.push_str(&format!("    \"report_every\": {},\n", cli.report_every));
        json.push_str(&format!("    \"start_iter\": {},\n", start_iter));
        json.push_str(&format!("    \"end_iter\": {},\n", trainer.iteration()));
        json.push_str(&format!("    \"stopped_early\": {}\n", stopped_early));
        json.push_str("  },\n");

        json.push_str(&format!("  \"wall_seconds\": {:.3},\n", elapsed_total));

        json.push_str("  \"snapshot\": {\n");
        json.push_str(&format!("    \"infosets\": {},\n", snap.infosets));
        json.push_str(&format!("    \"capacity\": {},\n", snap.capacity));
        json.push_str(&format!(
            "    \"capacity_pct\": {:.3},\n",
            if snap.capacity > 0 {
                100.0 * snap.infosets as f64 / snap.capacity as f64
            } else {
                0.0
            }
        ));
        json.push_str(&format!(
            "    \"max_abs_regret\": {:.6e},\n",
            snap.max_abs_regret
        ));
        json.push_str(&format!(
            "    \"mean_abs_regret\": {:.6e},\n",
            snap.mean_abs_regret
        ));
        json.push_str(&format!(
            "    \"nonfinite_count\": {},\n",
            snap.nonfinite_count
        ));
        json.push_str(&format!(
            "    \"strategy_sum_mass\": {:.6e}\n",
            snap.strategy_sum_mass
        ));
        json.push_str("  },\n");

        json.push_str("  \"cumulative_metrics\": {\n");
        json.push_str(&format!("    \"nodes\": {},\n", cumulative.nodes));
        json.push_str(&format!(
            "    \"nodes_per_iteration\": {:.3},\n",
            if cumulative.iterations > 0 {
                cumulative.nodes as f64 / cumulative.iterations as f64
            } else {
                0.0
            }
        ));
        json.push_str(&format!(
            "    \"avg_depth\": {:.4},\n",
            cumulative.avg_depth()
        ));
        json.push_str(&format!("    \"max_depth\": {},\n", cumulative.max_depth));
        json.push_str(&format!(
            "    \"cache_hit_rate\": {:.4},\n",
            cumulative.cache_hit_rate()
        ));
        json.push_str(&format!(
            "    \"infosets_created\": {},\n",
            cumulative.infosets_created
        ));
        json.push_str(&format!(
            "    \"strategy_ops_pushed\": {},\n",
            cumulative.strategy_pushed
        ));
        json.push_str(&format!(
            "    \"strategy_ops_applied\": {},\n",
            cumulative.strategy_applied
        ));
        json.push_str(&format!(
            "    \"regret_ops_input\": {},\n",
            cumulative.regret_input
        ));
        json.push_str(&format!(
            "    \"regret_ops_unique\": {},\n",
            cumulative.regret_unique
        ));
        json.push_str(&format!(
            "    \"regret_dedup_ratio\": {:.4},\n",
            cumulative.regret_dedup_ratio()
        ));
        json.push_str(&format!(
            "    \"batches\": {},\n",
            cumulative.batches
        ));
        json.push_str(&format!(
            "    \"total_traverse_s\": {:.4},\n",
            cumulative.traverse_ns as f64 / 1.0e9
        ));
        json.push_str(&format!(
            "    \"total_merge_s\": {:.4},\n",
            cumulative.merge_ns as f64 / 1.0e9
        ));
        json.push_str(&format!(
            "    \"total_flush_s\": {:.4},\n",
            cumulative.flush_ns as f64 / 1.0e9
        ));
        json.push_str(&format!(
            "    \"total_wall_s\": {:.4},\n",
            cumulative.wall_ns as f64 / 1.0e9
        ));
        json.push_str("    \"depth_histogram\": [");
        for (i, v) in cumulative.depth_hist.iter().enumerate() {
            if i > 0 {
                json.push_str(", ");
            }
            json.push_str(&format!("{}", v));
        }
        json.push_str("]\n");
        json.push_str("  },\n");

        json.push_str("  \"strategy_analysis\": {\n");
        json.push_str(&format!("    \"total\": {},\n", analysis.total));
        json.push_str(&format!("    \"empty\": {},\n", analysis.empty));
        json.push_str(&format!("    \"pure\": {},\n", analysis.pure));
        json.push_str(&format!("    \"mixed\": {},\n", analysis.mixed));
        json.push_str(&format!(
            "    \"mean_entropy_bits\": {:.4},\n",
            analysis.mean_entropy
        ));
        json.push_str("    \"entropy_histogram_0p25bit\": [");
        for (i, v) in analysis.entropy_histogram.iter().enumerate() {
            if i > 0 {
                json.push_str(", ");
            }
            json.push_str(&format!("{}", v));
        }
        json.push_str("],\n");
        json.push_str("    \"dominant_action_counts\": [");
        for (i, v) in analysis.dominant_counts.iter().enumerate() {
            if i > 0 {
                json.push_str(", ");
            }
            json.push_str(&format!("{}", v));
        }
        json.push_str("]\n");
        json.push_str("  },\n");

        json.push_str("  \"sample_infosets\": [\n");
        for (i, d) in samples.iter().enumerate() {
            json.push_str("    {");
            json.push_str(&format!("\"hash\": \"0x{:016x}\", ", d.hash));
            json.push_str("\"strategy\": [");
            for (j, p) in d.strategy.iter().enumerate() {
                if j > 0 {
                    json.push_str(", ");
                }
                json.push_str(&format!("{:.6}", p));
            }
            json.push_str("], \"regrets\": [");
            for (j, r) in d.regrets.iter().enumerate() {
                if j > 0 {
                    json.push_str(", ");
                }
                json.push_str(&format!("{:.6e}", r));
            }
            json.push_str("]");
            if i + 1 < samples.len() {
                json.push_str(",");
            }
            json.push('\n');
        }
        json.push_str("  ]\n");
        json.push_str("}\n");

        if let Err(e) = std::fs::write(path, json) {
            eprintln!("WARNING: failed to write stats JSON: {}", e);
        } else {
            eprintln!("stats JSON written to {}", path.display());
        }
    }

    // Export blueprint.
    let mut keys = trainer.get_table().get_keys();
    keys.sort_unstable();
    eprintln!("Exporting {} infosets...", keys.len());

    let output_path = cli.output.to_str().expect("invalid output path");
    write_blueprint(output_path, trainer.get_table(), &keys);
    eprintln!("Blueprint written to {}", output_path);

    Ok(())
}
