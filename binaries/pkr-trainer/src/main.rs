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

/// Global allocator: mimalloc. Under heavy parallel allocation
/// (BatchItem/StrategyOp buffers, papaya map, large arrays) it
/// substantially outperforms the system allocator and lowers RSS.
///
/// B18: when the `dhat-profiling` feature is enabled, dhat's allocator
/// replaces mimalloc so heap activity is recorded.
#[cfg(not(feature = "dhat-profiling"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(feature = "dhat-profiling")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Fixed seed for the exploitability evaluator. Using a constant rather
/// than `done` makes successive EVAL points directly comparable
/// (common-random-numbers comparison). The training RNG is separate.
const EVAL_SEED: u64 = 0xE7A1_0000_0000_0001;

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

    /// Hand evaluator backend. "table" = 21-subset LUT read (default,
    /// reference implementation). "fast7" = rank-count LUT + flush fast
    /// path (T1.3, ~20-60x faster per eval, bit-identical output).
    #[arg(long, default_value = "table")]
    evaluator: String,

    #[arg(long)]
    threads: Option<usize>,

    #[arg(long)]
    checkpoint: Option<PathBuf>,

    /// Iterations between rolling checkpoint saves. Default is 500,000
    /// (~5-10 minutes at typical throughput). A 186 MB checkpoint every
    /// 10,000 iterations is ~6 MB/s sustained I/O which triggers macOS
    /// Spotlight / Time Machine resource storms on full disks (see
    /// docs/experiments/v33-rich-preflop-confirmed.md §recommendations).
    #[arg(long, default_value_t = 500_000)]
    checkpoint_every: u32,

    /// Regret-table capacity (number of infosets). v36 capacity sweep
    /// (docs/experiments/v36-capacity-sweep.md) showed 60M beats 5M by
    /// ~56 mbb pooled across 2 seeds. The extra memory is virtual address
    /// space only (lazily allocated), not RSS.
    #[arg(long, default_value_t = 60_000_000)]
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

    /// Fire one sampled best-response exploitability check immediately
    /// after loading the checkpoint, before any training. Measures an
    /// existing checkpoint without needing --eval-every to wait for a
    /// report boundary.
    #[arg(long, default_value_t = false)]
    eval_now: bool,

    /// Discard an existing checkpoint instead of resuming.
    #[arg(long, default_value_t = false)]
    fresh: bool,

    /// Tolerance (in mbb/hand) for the promotion gate: a new checkpoint
    /// is allowed to be worse than the current best by up to this much
    /// before it is rejected. Larger = more permissive. Zero means
    /// strict monotone improvement is required (usually too tight given
    /// BR sampling noise). Ignored when --eval-every == 0.
    #[arg(long, default_value_t = 3.0)]
    promote_gate: f64,

    /// Path to the exploitability CSV. If unset, derived from
    /// --output's directory as `exploitability.csv`. Only written when
    /// --eval-every > 0.
    #[arg(long)]
    exploitability_csv: Option<PathBuf>,

    /// Deals sampled per exploitability check. Accuracy ~ 1/sqrt(deals).
    #[arg(long, default_value_t = 10000)]
    eval_deals: u32,

    /// Skip exporting infosets whose reach-weighted strategy mass is below
    /// this many visits. 0 = export everything. Reduces blueprint size
    /// and removes uniform-fallback infosets from the shipped file.
    #[arg(long, default_value_t = 0.0)]
    min_visits: f32,

    /// Seed for the worker RNGs. Same seed + same inputs = identical training run.
    #[arg(long, default_value_t = 0x5EED_1F70u64)]
    seed: u64,
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
    fingerprint: &pkr_core::abstraction::AbstractionFingerprint,
) -> std::io::Result<()> {
    let tmp = ckpt.with_extension("ckpt.tmp");
    let prev = ckpt.with_extension("ckpt.prev");
    trainer.save_checkpoint(tmp.to_str().unwrap(), fingerprint)?;
    if ckpt.exists() {
        std::fs::rename(ckpt, &prev)?;
    }
    std::fs::rename(&tmp, ckpt)?;
    Ok(())
}
fn export_blueprint(
    trainer: &pkr_cfr::Trainer,
    output: &std::path::Path,
    min_visits: f32,
    fingerprint: &pkr_core::abstraction::AbstractionFingerprint,
) -> std::io::Result<usize> {
    let table = trainer.get_table();
    let mut keys = table.get_keys();
    keys.sort_unstable();
    if min_visits > 0.0 {
        keys.retain(|k| {
            table
                .get_average_strategy_slice(*k)
                .is_some_and(|s| s.iter().sum::<f32>() >= min_visits)
        });
    }
    let path_str = output.to_str().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid output path")
    })?;
    write_blueprint(path_str, table, &keys, fingerprint)?;
    Ok(keys.len())
}

/// True when the eval should fire at the current iteration.
///
/// Fires on the regular schedule (done advanced by >= eval_every since
/// the last eval) AND on the final iteration. The final-iteration case
/// matters because `done` advances in `iters_per_sync` batches, so the
/// observed `last_eval_iter` can be a few hundred iters past the
/// theoretical multiple; without this clause, a run that stops exactly
/// at `max_iters` loses its final data point entirely (v23a bug: 5M
/// run with eval_every=2.5M only produced the 2.5M row).
#[inline]
fn should_eval(done: u32, last_eval_iter: u32, eval_every: u32, max_iters: u32) -> bool {
    eval_every > 0 && (done >= last_eval_iter.saturating_add(eval_every) || done == max_iters)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    // B18: dhat heap profiling (opt-in via --features dhat-profiling).
    // The guard spans the whole run; dhat writes `dhat-heap.json` to the
    // process CWD when it drops. ci/scripts/run-dhat.sh moves it into
    // $PROF_DIR/dhat-out/ afterwards.
    #[cfg(feature = "dhat-profiling")]
    let _dhat_profiler = dhat::Profiler::new_heap();
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

    let evaluator: Arc<dyn pkr_contracts::Evaluator> = match cli.evaluator.as_str() {
        "fast7" => Arc::new(
            pkr_eval::Fast7Evaluator::new(&cli.rank_table)
                .expect("Failed to load hand_ranks.bin (Fast7Evaluator)"),
        ),
        "table" => {
            Arc::new(TableEvaluator::new(&cli.rank_table).expect("Failed to load hand_ranks.bin"))
        }
        other => {
            eprintln!(
                "FATAL: unknown --evaluator '{}'. Expected 'table' or 'fast7'.",
                other
            );
            std::process::exit(2);
        }
    };
    eprintln!("evaluator: {}", cli.evaluator);

    let store =
        load_centroids(cli.centroids.to_str().unwrap()).expect("Failed to load default centroids");
    let k = store.centroids.len();
    eprintln!("centroids: k={}", k);
    if k < 100 && std::env::var("PKR_ALLOW_SMALL_K").as_deref() != Ok("1") {
        return Err(format!(
            "centroids.bin has k={k} - below the production floor of 100. \
             This is the smoke-test config that caused the v9-v13 incident. \
             Regenerate with run.sh (CENTROID_K=200), or set PKR_ALLOW_SMALL_K=1 \
             to explicitly acknowledge the small-k config."
        )
        .into());
    }
    let mut abstraction = KMeansAbstraction::from_store(store, evaluator.clone());

    // F2b: fingerprint the semantic configuration. Every checkpoint
    // carries this; mismatch on load aborts (see table.rs).
    let fingerprint = pkr_core::abstraction::AbstractionFingerprint::from_constants(k as u32);
    eprintln!(
        "fingerprint: k={} sizings=[{:.2},{:.2},{:.2}] thresholds=[{:.2},{:.2}] sig_v={} hash_algo={}",
        fingerprint.preflop_k,
        fingerprint.sizing_small,
        fingerprint.sizing_medium,
        fingerprint.sizing_large,
        fingerprint.threshold_small,
        fingerprint.threshold_large,
        fingerprint.sig_version,
        fingerprint.hash_algo,
    );

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
    trainer.set_run_seed(cli.seed);
    eprintln!(
        "init: table + abstraction ready in {:.2}s (capacity={})",
        t_init.elapsed().as_secs_f64(),
        cli.capacity
    );

    let start_iter: u32 = match &cli.checkpoint {
        Some(ckpt) if ckpt.exists() && !cli.fresh => {
            match trainer.load_checkpoint(ckpt.to_str().unwrap(), &fingerprint) {
                Ok(()) => {
                    let it = trainer.iteration();
                    eprintln!("Resumed from checkpoint at iteration {}", it);
                    it
                }
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                    return Err(format!(
                        "checkpoint {} is incompatible: {e}. Delete it or pass --fresh.",
                        ckpt.display()
                    )
                    .into());
                }
                Err(e) => {
                    let prev = ckpt.with_extension("ckpt.prev");
                    if !prev.exists() {
                        return Err(format!(
                            "checkpoint {} unreadable ({e}) and no .prev. Pass --fresh.",
                            ckpt.display()
                        )
                        .into());
                    }
                    eprintln!("WARNING: primary checkpoint failed ({e}), trying .prev");
                    trainer
                        .load_checkpoint(prev.to_str().unwrap(), &fingerprint)
                        .map_err(|e2| format!("both checkpoint and .prev failed: {e2}"))?;
                    let it = trainer.iteration();
                    eprintln!("Resumed from .prev checkpoint at iteration {}", it);
                    it
                }
            }
        }
        Some(ckpt) if ckpt.exists() && cli.fresh => {
            eprintln!(
                "WARNING: --fresh: existing checkpoint {} will be overwritten",
                ckpt.display()
            );
            0
        }
        _ => 0,
    };

    // Metrics CSV: append on resume, truncate on --fresh. Before v33 this
    // used File::create unconditionally, which silently erased the CSV
    // history on every resume (observed during the seed-43 A OOM recovery).
    let mut csv_writer: Option<std::io::BufWriter<std::fs::File>> = match &cli.metrics_csv {
        Some(path) => {
            let is_new = cli.fresh
                || !path.exists()
                || std::fs::metadata(path).map(|m| m.len() == 0).unwrap_or(true);
            let f = if cli.fresh {
                std::fs::File::create(path)?
            } else {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)?
            };
            let mut w = std::io::BufWriter::new(f);
            if is_new {
                writeln!(
                    w,
                    "iter,wall_s,it_per_s,infosets,cap_pct,max_abs_regret,\
                     mean_abs_regret,nonfinite,strat_mass,\
                     nodes,nodes_per_iter,avg_depth,max_depth,cache_hit_rate,\
                     regret_in,regret_out,regret_dedup,strategy_applied,\
                     traverse_ms,merge_ms,flush_ms,wall_ms"
                )?;
            }
            w.flush()?;
            Some(w)
        }
        None => None,
    };

    if cli.eval_now {
        eprintln!(
            "EVAL-NOW: firing initial exploitability check at iter {}",
            start_iter
        );
        let br = pkr_exploit::best_response::sampled_exploitability(
            trainer.get_table(),
            abstraction_for_eval.as_ref(),
            evaluator_for_eval.as_ref(),
            cli.eval_deals,
            EVAL_SEED ^ (start_iter as u64),
        );
        eprintln!(
            "EVAL iter={} expl_mbb={:.2}+/-{:.2} insample={:.2} br0={:.4} br1={:.4} deals={}",
            start_iter,
            br.exploitability_mbb,
            br.expl_std_err_mbb,
            br.expl_insample_mbb,
            br.br0,
            br.br1,
            br.deals_sampled
        );
    }

    let start = Instant::now();
    let mut last_ckpt_iter = start_iter;
    let mut last_report_iter = start_iter;
    let mut last_eval_iter = start_iter;
    let mut stopped_early = false;
    let mut hit_capacity = false;
    let mut interrupted = false;
    let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let f = std::sync::Arc::clone(&stop_flag);
        ctrlc::set_handler(move || {
            f.store(true, std::sync::atomic::Ordering::SeqCst);
        })
        .expect("install ctrlc handler");
    }

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
    // C5c: track EHS-fallback count across the run.
    let mut prev_fallbacks = pkr_abstraction::fallback_count();
    // C5d: track depth/deck overflows across the run.
    let mut prev_depth_overflows: u64 = 0;
    let mut prev_deck_overflows: u64 = 0;

    // E1: exploitability CSV writer + promotion-gate state.
    // Exploitability CSV: same append-on-resume semantics as the metrics
    // CSV. Before v33 this also used File::create unconditionally, which
    // erased the seed-43 A readings when the run was resumed after OOM.
    let mut expl_writer: Option<std::io::BufWriter<std::fs::File>> = if cli.eval_every > 0 {
        let path = cli
            .exploitability_csv
            .clone()
            .unwrap_or_else(|| cli.output.with_file_name("exploitability.csv"));
        let is_new = cli.fresh
            || !path.exists()
            || std::fs::metadata(&path).map(|m| m.len() == 0).unwrap_or(true);
        let f = if cli.fresh {
            std::fs::File::create(&path)?
        } else {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?
        };
        let mut w = std::io::BufWriter::new(f);
        if is_new {
            writeln!(w, "iter,expl_mbb,expl_stderr_mbb,br0,br1,deals")?;
        }
        w.flush()?;
        eprintln!("exploitability CSV: {}", path.display());
        Some(w)
    } else {
        None
    };
    let mut best_expl_mbb: Option<f64> = None;
    // Whether we promoted a checkpoint inside the loop. If false (e.g.
    // --eval-every 0, or no eval fired), the end-of-run export runs
    // as before. If true, we skip the end-of-run export to avoid
    // clobbering the promoted blueprint with a possibly-worse one.
    let mut promoted = false;

    while done < max_iters {
        if stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
            eprintln!("Signal received: stopping after iteration {done}");
            interrupted = true;
            break;
        }
        if trainer.is_near_capacity() {
            eprintln!(
                "WARN: table >=95% capacity ({} slots), stopping early",
                trainer.get_table().allocated()
            );
            stopped_early = true;
            hit_capacity = true;
            break;
        }
        if let Some(d) = bench_deadline {
            if start.elapsed() >= d {
                stopped_early = true;
                break;
            }
        }

        let batch = cli.iters_per_sync.min(max_iters - done);
        trainer.run_iterations_parallel(batch as usize);
        done += batch;

        // C5c: abort if the abstraction silently fell back to
        // Monte-Carlo EHS during this batch. Any fallback means at
        // least one infoset hashed through a *different* cluster id
        // than the table would have produced — silent corruption.
        // `PKR_ALLOW_EHS_FALLBACK=1` disables (tests, small-table smoke).
        let cur_fallbacks = pkr_abstraction::fallback_count();
        if cur_fallbacks > prev_fallbacks
            && std::env::var("PKR_ALLOW_EHS_FALLBACK").as_deref() != Ok("1")
        {
            let breakdown = pkr_abstraction::fallback_breakdown();
            eprintln!(
                "FATAL: {} EHS fallback(s) in batch ending at iter {}; \
                 per-street (preflop/flop/turn/river) = {:?}. \
                 Aborting to prevent silent mixed-abstraction training. \
                 Set PKR_ALLOW_EHS_FALLBACK=1 to override.",
                cur_fallbacks - prev_fallbacks,
                done,
                breakdown,
            );
            return Err("abstraction fell back to Monte-Carlo EHS".into());
        }
        prev_fallbacks = cur_fallbacks;

        // C5d: any depth or deck overflow silently corrupts regret
        // math via a spurious 0.0 return. Abort on first occurrence.
        let cur_metrics = pkr_cfr::metrics::global().snapshot();
        let cur_depth = cur_metrics.depth_overflows;
        let cur_deck = cur_metrics.deck_overflows;
        if cur_depth > prev_depth_overflows || cur_deck > prev_deck_overflows {
            eprintln!(
                "FATAL: traversal overflow at iter {} — depth_overflows={} (+{}), \
                 deck_overflows={} (+{}). Training would silently corrupt \
                 regrets. Aborting.",
                done,
                cur_depth,
                cur_depth - prev_depth_overflows,
                cur_deck,
                cur_deck - prev_deck_overflows,
            );
            return Err("traversal depth/deck overflow".into());
        }
        prev_depth_overflows = cur_depth;
        prev_deck_overflows = cur_deck;

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
            if should_eval(done, last_eval_iter, cli.eval_every, max_iters) {
                let br = pkr_exploit::best_response::sampled_exploitability(
                    trainer.get_table(),
                    abstraction_for_eval.as_ref(),
                    evaluator_for_eval.as_ref(),
                    cli.eval_deals,
                    done as u64,
                );
                eprintln!(
            "EVAL iter={} expl_mbb={:.2}+/-{:.2} insample={:.2} br0={:.4} br1={:.4} deals={}",
            done, br.exploitability_mbb, br.expl_std_err_mbb,
            br.expl_insample_mbb, br.br0, br.br1, br.deals_sampled
        );

                // E1: append to the exploitability CSV.
                if let Some(w) = expl_writer.as_mut() {
                    writeln!(
                        w,
                        "{},{:.4},{:.4},{:.4},{:.4},{}",
                        done,
                        br.exploitability_mbb,
                        br.expl_std_err_mbb,
                        br.br0,
                        br.br1,
                        br.deals_sampled,
                    )?;
                    w.flush()?;
                }

                // E1: promotion gate. Reject a checkpoint whose exploitability
                // Promote ONLY on a new historical minimum. The old logic
                // allowed a sliding upward gate (`best + gate` where best
                // was the LAST accepted reading): once a worse reading
                // entered the accept window, `best` updated to that worse
                // value and the next accept threshold climbed further.
                // That is how v25final shipped the 200M reading (6257 mbb)
                // instead of the 120M floor (5450 mbb).
                //
                // The C3 "gate must exceed measurement noise" concern is
                // still respected by the caller: when best is unset, we
                // accept unconditionally; when best is set, we require
                // strictly better (any improvement counts, but the sliding
                // threshold is gone). For winner's-curse protection we
                // keep `promote_gate` as a *significance* margin that the
                // improvement must clear.
                let min_improvement = cli.promote_gate.max(0.0);
                let rejected = match best_expl_mbb {
                    Some(b) => br.exploitability_mbb >= b - min_improvement,
                    None => false,
                };
                if rejected {
                    eprintln!(
                        "SKIP-PROMOTE iter={} expl_mbb={:.2} not a new minimum (best {:.2}, need < {:.2})",
                        done,
                        br.exploitability_mbb,
                        best_expl_mbb.unwrap_or(0.0),
                        best_expl_mbb.map(|b| b - min_improvement).unwrap_or(0.0),
                    );
                } else {
                    // Export the current table as the promoted blueprint.
                    match export_blueprint(&trainer, &cli.output, cli.min_visits, &fingerprint) {
                        Ok(n) => {
                            eprintln!(
                                "PROMOTE iter={} expl_mbb={:.2} (prev best {:?}) -> {} ({} infosets)",
                                done,
                                br.exploitability_mbb,
                                best_expl_mbb,
                                cli.output.display(),
                                n,
                            );
                            // Defensive: also write a "best-ever" copy. If a
                            // later run or a bug overwrites `cli.output`,
                            // `best.bin` remains the historical minimum.
                            let best_path = cli.output.with_file_name(format!(
                                "{}.best.bin",
                                cli.output
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("blueprint")
                            ));
                            if let Err(e) = export_blueprint(
                                &trainer,
                                &best_path,
                                cli.min_visits,
                                &fingerprint,
                            ) {
                                eprintln!(
                                    "WARNING: best-blueprint export failed at iter {done}: {e}"
                                );
                            } else {
                                eprintln!("         (also saved to {})", best_path.display());
                            }
                            best_expl_mbb = Some(br.exploitability_mbb);
                            promoted = true;
                        }
                        Err(e) => {
                            eprintln!("WARNING: blueprint export failed at iter {done}: {e}");
                        }
                    }
                }

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
        }

        if cli.checkpoint_every > 0
            && done != last_ckpt_iter
            && (done - last_ckpt_iter) >= cli.checkpoint_every
        {
            if let Some(ckpt) = &cli.checkpoint {
                match save_checkpoint_rolling(&trainer, ckpt, &fingerprint) {
                    Ok(()) => {
                        eprintln!("Checkpoint written at iteration {}", done);
                        last_ckpt_iter = done;
                    }
                    Err(e) => {
                        // Do NOT silently continue. If a checkpoint save
                        // fails (typically ENOSPC on a full disk) and we
                        // retry every 10K iterations, we fill the disk with
                        // partial .tmp files and can trigger a macOS
                        // resource storm. Abort on first failure so the
                        // operator can free space and resume from the
                        // previous good checkpoint.
                        eprintln!(
                            "FATAL: checkpoint failed at iteration {} ({}); aborting to preserve disk",
                            done, e
                        );
                        return Err(format!(
                            "checkpoint failed at iteration {}: {}",
                            done, e
                        )
                        .into());
                    }
                }
            }
        }
    }

    if !stopped_early || hit_capacity || interrupted {
        if let Some(ckpt) = &cli.checkpoint {
            if let Err(e) = save_checkpoint_rolling(&trainer, ckpt, &fingerprint) {
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

    // E1: if we promoted inside the loop, the blueprint on disk is
    // already the best checkpoint; do not overwrite it here.
    if !promoted {
        // Fallback: --eval-every == 0 (no gate) or no eval fired.
        // T2.4: min-visits filter applied inside export_blueprint.
        let n = export_blueprint(&trainer, &cli.output, cli.min_visits, &fingerprint)?;
        eprintln!(
            "Blueprint written to {} ({} infosets)",
            cli.output.display(),
            n
        );
    }

    // B18: the `_dhat_profiler` guard (declared at the top of run())
    // writes dhat-heap.json on drop. See the NOTE there for the API
    // deviation from the plan draft.

    Ok(())
}

#[cfg(test)]
mod should_eval_tests {
    use super::should_eval;

    #[test]
    fn fires_on_regular_schedule() {
        assert!(should_eval(5_000_000, 0, 5_000_000, 20_000_000));
        assert!(should_eval(10_000_000, 5_000_000, 5_000_000, 20_000_000));
    }

    #[test]
    fn does_not_fire_early() {
        assert!(!should_eval(4_999_999, 0, 5_000_000, 20_000_000));
        assert!(!should_eval(9_999_999, 5_000_000, 5_000_000, 20_000_000));
    }

    #[test]
    fn fires_on_final_iteration_even_if_batch_misaligned() {
        // Reproduces the v23a bug: eval fired at 2_501_120 (a multiple of 512),
        // so last_eval_iter=2_501_120 and the next threshold is 5_001_120,
        // but the run stops at max_iters=5_000_000.
        assert!(should_eval(5_000_000, 2_501_120, 2_500_000, 5_000_000));
    }

    #[test]
    fn disabled_when_eval_every_is_zero() {
        assert!(!should_eval(5_000_000, 0, 0, 20_000_000));
        assert!(!should_eval(20_000_000, 0, 0, 20_000_000));
    }

    #[test]
    fn fires_at_very_first_iteration_when_max_is_zero() {
        // Degenerate; must not panic.
        assert!(!should_eval(0, 0, 0, 0));
    }
}
