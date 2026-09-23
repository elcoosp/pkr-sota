#![allow(clippy::needless_range_loop)] // numerics: indexed loops are idiomatic here

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

    /// Iterations per rayon dispatch. Larger amortizes the serial
    /// merge/flush work across more logical iterations; smaller reduces
    /// staleness of the regrets each traversal sees. 512 measured best
    /// on M1 in v11 (55K it/s vs 38K at 256).
    #[arg(long, default_value_t = 512)]
    iters_per_sync: u32,

    /// After training, run preflop chart sanity checks against the
    /// trained strategy and print the results. Validates BU open
    /// frequency against published ranges.
    #[arg(long, default_value_t = false)]
    preflop_check: bool,

    /// Sampled best-response exploitability check every N iterations
    /// (0 = off). Reports in milli-big-blinds per game.
    #[arg(long, default_value_t = 0)]
    eval_every: u32,

    /// Deals sampled per exploitability check. Accuracy ~ 1/sqrt(deals).
    #[arg(long, default_value_t = 2000)]
    eval_deals: u32,

    /// Skip exporting infosets whose reach-weighted strategy mass is below
    /// this many visits. 0 = export everything. Reduces blueprint size
    /// and removes uniform-fallback infosets from the shipped file.
    #[arg(long, default_value_t = 0.0)]
    min_visits: f32,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("FATAL: {}", e);
        std::process::exit(1);
    }
}

/// Atomic checkpoint write: write to `.tmp`, rotate existing to `.prev`,
/// then rename. A crash mid-write can never corrupt the main checkpoint.
fn save_checkpoint_rolling(
    trainer: &pkr_cfr::Trainer,
    ckpt: &std::path::Path,
) -> std::io::Result<()> {
    let tmp = ckpt.with_extension("ckpt.tmp");
    let prev = ckpt.with_extension("ckpt.prev");
    trainer.save_checkpoint(tmp.to_str().unwrap())?;
    if ckpt.exists() {
        std::fs::rename(ckpt, &prev)?;
    }
    std::fs::rename(&tmp, ckpt)?;
    Ok(())
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

    let store =
        load_centroids(cli.centroids.to_str().unwrap()).expect("Failed to load default centroids");
    let mut abstraction = KMeansAbstraction::from_store(store, evaluator.clone());

    if let Some(path) = &cli.flop_centroids {
        abstraction
            .load_street_centroids(1, path.to_str().unwrap())
            .expect("flop centroids");
    }
    if let Some(path) = &cli.turn_centroids {
        abstraction
            .load_street_centroids(2, path.to_str().unwrap())
            .expect("turn centroids");
    }
    if let Some(path) = &cli.river_centroids {
        abstraction
            .load_street_centroids(3, path.to_str().unwrap())
            .expect("river centroids");
    }
    if let Some(path) = &cli.preflop_table {
        abstraction
            .init_table(0, path.to_str().unwrap())
            .expect("preflop table");
    }
    if let Some(path) = &cli.flop_table {
        abstraction
            .init_table(1, path.to_str().unwrap())
            .expect("flop table");
    }
    if let Some(path) = &cli.turn_table {
        abstraction
            .init_table(2, path.to_str().unwrap())
            .expect("turn table");
    }
    if let Some(path) = &cli.flop_buckets {
        abstraction
            .load_flop_buckets(path.to_str().unwrap())
            .expect("flop buckets");
    }
    if let Some(path) = &cli.river_table {
        abstraction
            .init_table(3, path.to_str().unwrap())
            .expect("river table");
    }

    let abstraction = Arc::new(abstraction);
    let t_init = Instant::now();
    let abstraction_for_eval = Arc::clone(&abstraction);
    let evaluator_for_eval = Arc::clone(&evaluator);
    let mut trainer = Trainer::with_capacity(abstraction, evaluator, cli.capacity);
    eprintln!(
        "init: table + abstraction ready in {:.2}s (capacity={})",
        t_init.elapsed().as_secs_f64(),
        cli.capacity
    );

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
    let mut last_eval_iter = start_iter;
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

    let mut done = start_iter;
    let mut prev_metrics_snapshot = pkr_cfr::metrics::global().snapshot();

    while done < max_iters {
        if let Some(d) = bench_deadline {
            if start.elapsed() >= d {
                stopped_early = true;
                break;
            }
        }

        let batch = cli.iters_per_sync.min(max_iters - done);
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

            // Sampled best-response exploitability check.
            // Fire when done has advanced by at least eval_every since the
            // last eval (done increments by ITERS_PER_SYNC, not by 1).
            if cli.eval_every > 0 && done >= last_eval_iter.saturating_add(cli.eval_every) {
                let br = pkr_exploit::best_response::sampled_exploitability(
                    trainer.get_table(),
                    abstraction_for_eval.as_ref(),
                    evaluator_for_eval.as_ref(),
                    cli.eval_deals,
                    done as u64,
                );
                eprintln!(
                    "EVAL iter={} expl_mbb={:.2} br0={:.4} br1_p0={:.4} deals={}",
                    done, br.exploitability_mbb, br.br0, br.br1_to_p0, br.deals_sampled
                );
                last_eval_iter = done;
            }

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
                match save_checkpoint_rolling(&trainer, ckpt) {
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
            if let Err(e) = save_checkpoint_rolling(&trainer, ckpt) {
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

        let depth_hist: Vec<u64> = cumulative.depth_hist.to_vec();
        let entropy_hist: Vec<usize> = analysis.entropy_histogram.to_vec();
        let dominant: Vec<usize> = analysis.dominant_counts.to_vec();

        let stats = serde_json::json!({
            "config": {
                "iterations": cli.iterations,
                "threads": num_threads,
                "capacity": cli.capacity,
                "iters_per_sync": cli.iters_per_sync,
                "report_every": cli.report_every,
                "start_iter": start_iter,
                "end_iter": trainer.iteration(),
                "stopped_early": stopped_early,
            },
            "wall_seconds": elapsed_total,
            "snapshot": {
                "infosets": snap.infosets,
                "capacity": snap.capacity,
                "capacity_pct": if snap.capacity > 0 {
                    100.0 * snap.infosets as f64 / snap.capacity as f64
                } else { 0.0 },
                "max_abs_regret": snap.max_abs_regret,
                "mean_abs_regret": snap.mean_abs_regret,
                "nonfinite_count": snap.nonfinite_count,
                "strategy_sum_mass": snap.strategy_sum_mass,
            },
            "cumulative_metrics": {
                "nodes": cumulative.nodes,
                "nodes_per_iteration": if cumulative.iterations > 0 {
                    cumulative.nodes as f64 / cumulative.iterations as f64
                } else { 0.0 },
                "avg_depth": cumulative.avg_depth(),
                "max_depth": cumulative.max_depth,
                "cache_hit_rate": cumulative.cache_hit_rate(),
                "infosets_created": cumulative.infosets_created,
                "strategy_ops_pushed": cumulative.strategy_pushed,
                "strategy_ops_applied": cumulative.strategy_applied,
                "regret_ops_input": cumulative.regret_input,
                "regret_ops_unique": cumulative.regret_unique,
                "regret_dedup_ratio": cumulative.regret_dedup_ratio(),
                "batches": cumulative.batches,
                "total_traverse_s": cumulative.traverse_ns as f64 / 1.0e9,
                "total_merge_s": cumulative.merge_ns as f64 / 1.0e9,
                "total_flush_s": cumulative.flush_ns as f64 / 1.0e9,
                "total_wall_s": cumulative.wall_ns as f64 / 1.0e9,
                "depth_histogram": depth_hist,
            },
            "strategy_analysis": {
                "total": analysis.total,
                "empty": analysis.empty,
                "pure": analysis.pure,
                "mixed": analysis.mixed,
                "mean_entropy_bits": analysis.mean_entropy,
                "entropy_histogram_0p25bit": entropy_hist,
                "dominant_action_counts": dominant,
                "nonzero_strategy_sum_cells": analysis.nonzero_strategy_sum_cells,
                "uniform_fallback": analysis.uniform_fallback,
            },
            "sample_infosets": samples.iter().map(|d| {
                let strategy: Vec<f32> = d.strategy.to_vec();
                let regrets: Vec<f32> = d.regrets.to_vec();
                serde_json::json!({
                    "hash": format!("0x{:016x}", d.hash),
                    "strategy": strategy,
                    "regrets": regrets,
                })
            }).collect::<Vec<_>>(),
        });

        match serde_json::to_string_pretty(&stats) {
            Ok(s) => {
                if let Err(e) = std::fs::write(path, s) {
                    eprintln!("WARNING: failed to write stats JSON: {}", e);
                } else {
                    eprintln!("stats JSON written to {}", path.display());
                }
            }
            Err(e) => eprintln!("WARNING: failed to serialize stats JSON: {}", e),
        }
    }

    if cli.preflop_check {
        eprintln!();
        eprintln!("=== preflop chart validation ===");
        let lookup = pkr_cfr::preflop_validate::lookup_from_table(
            trainer.get_table(),
            abstraction_for_eval.as_ref(),
        );
        let r = pkr_cfr::preflop_validate::validate_preflop_opening(&lookup);
        eprintln!("  category: {}", r.category);
        eprintln!("  passed:   {}", r.passed);
        eprintln!("  score:    {:.3}", r.score);
        for issue in &r.issues {
            eprintln!("  issue:    {}", issue);
        }
        eprintln!();
    }

    let mut keys = trainer.get_table().get_keys();
    keys.sort_unstable();

    // T2.4: filter infosets whose accumulated reach-weighted strategy
    // mass is below min_visits. These are the ones that would export as
    // uniform fallback (never reached with meaningful probability) and
    // contribute nothing but size to the blueprint. The runtime's
    // host-app fallback handles them at inference time.
    if cli.min_visits > 0.0 {
        let before = keys.len();
        keys.retain(
            |k| match trainer.get_table().get_average_strategy_slice(*k) {
                Some(strat) => {
                    let mass: f32 = strat.iter().sum();
                    mass >= cli.min_visits
                }
                None => false,
            },
        );
        eprintln!(
            "min-visits filter ({:.1}): {} -> {} infosets",
            cli.min_visits,
            before,
            keys.len()
        );
    }

    eprintln!("Exporting {} infosets...", keys.len());

    let output_path = cli.output.to_str().expect("invalid output path");
    write_blueprint(output_path, trainer.get_table(), &keys);
    eprintln!("Blueprint written to {}", output_path);

    Ok(())
}
