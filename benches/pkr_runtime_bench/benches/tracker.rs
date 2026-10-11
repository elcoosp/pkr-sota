//! Benchmark RangeTracker::apply_action — the per-action posterior
//! update on the live runtime path.
//!
//! The loop walks all 1326 hands, hashes each, reads the blueprint
//! average strategy, multiplies. It is the ~single hottest per-decision
//! cost in the bot after the lookup.
//!
//! Requires a smoke-built abstraction store + a matching blueprint.
//! Uses whatever `PKR_BLUEPRINT` and `PKR_TABLES` point at; if absent
//! or mis-sized, the bench skips with a clear message (mirrors
//! benches/lookup.rs).
//!
//! See docs/experiments/perf-P5-P18-2026-10-11.md (P8).

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::AbstractionBuilder;
use pkr_core::state::{Action, ActionKind, GameState};
use pkr_eval::NlheEvaluator;
use pkr_runtime::mmap::MmapReader;
use pkr_runtime::SolverHandle;
use pkr_subgame::range_tracker::RangeTracker;
use std::env;
use std::sync::Arc;

fn load_fixtures<'a>(
    abs: &'a mut KMeansAbstraction,
    tbl: &'a CompactRegretTable,
    ev: &'a NlheEvaluator,
) -> Option<(GameState, RangeTracker<'a>)> {
    let bp = env::var("PKR_BLUEPRINT").ok()?;
    let tables = env::var("PKR_TABLES").ok()?;
    let _ = bp;
    let _ = SolverHandle::new(MmapReader::new(&bp).ok()?);

    let store = load_centroids(&format!("{tables}/centroids.bin")).ok()?;
    *abs = KMeansAbstraction::from_store(store, Arc::new(NlheEvaluator));
    for (i, f) in [
        (0u8, "preflop_abstraction.bin"),
        (1u8, "abstraction.bin"),
        (2u8, "turn_abstraction.bin"),
        (3u8, "river_buckets.bin"),
    ] {
        abs.init_table(i, &format!("{tables}/{f}")).ok()?;
    }

    let mut st = GameState::new(200.0, 1.0, 2.0);
    st.set_hole_cards([12, 24], [10, 22]);
    // Preflop -> flop: call/check, deal flop.
    st.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
    st.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    st.advance_street_in_place(&[40, 3, 16]);
    // Postflop: check/check to reach the turn; separate decision points.
    st.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
    st.apply_action_in_place(&Action { player: 1, kind: ActionKind::Bet(6.0) });

    let tracker = RangeTracker::new(st.clone(), abs, tbl, ev);
    Some((st, tracker))
}

fn bench_tracker(c: &mut Criterion) {
    // The tracker needs the CHECKPOINT (train.ckpt), not the exported
    // blueprint.bin. Distinct env var to avoid confusing the two.
    let ckpt = match env::var("PKR_TRACKER_CKPT") {
        Ok(p) => p,
        Err(_) => {
            eprintln!("SKIP tracker bench: PKR_TRACKER_CKPT not set");
            return;
        }
    };
    let tables = match env::var("PKR_TABLES") {
        Ok(t) => t,
        Err(_) => {
            eprintln!("SKIP tracker bench: PKR_TABLES not set");
            return;
        }
    };
    // Load the checkpoint into a table.
    let mut abs = match load_centroids(&format!("{tables}/centroids.bin")) {
        Ok(s) => KMeansAbstraction::from_store(s, Arc::new(NlheEvaluator)),
        Err(_) => {
            eprintln!("SKIP tracker bench: cannot load centroids from {tables}");
            return;
        }
    };
    for (i, f) in [
        (0u8, "preflop_abstraction.bin"),
        (1u8, "abstraction.bin"),
        (2u8, "turn_abstraction.bin"),
        (3u8, "river_buckets.bin"),
    ] {
        if abs.init_table(i, &format!("{tables}/{f}")).is_err() {
            eprintln!("SKIP tracker bench: cannot init table {f}");
            return;
        }
    }
    // K=7 baseline has ~15M infosets; size the table to fit.
    let tbl = CompactRegretTable::with_capacity(20_000_000);
    let ev = NlheEvaluator;
    // The K=7 baseline was trained with sig v5. Set the env BEFORE
    // from_constants reads it (OnceLock, first call wins).
    if env::var("PKR_SIG_V5").is_err() {
        std::env::set_var("PKR_SIG_V5", "1");
    }
    let fp = pkr_core::abstraction::AbstractionFingerprint::from_constants(abs.k() as u32);
    if let Err(e) = tbl.load_checkpoint(&ckpt, &fp) {
        eprintln!("SKIP tracker bench: cannot load {ckpt}: {e}");
        return;
    }

    let abs_dyn: &dyn AbstractionBuilder = &abs;
    let mut st = GameState::new(200.0, 1.0, 2.0);
    st.set_hole_cards([12, 24], [10, 22]);
    st.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
    st.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    st.advance_street_in_place(&[40, 3, 16]);
    st.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
    st.apply_action_in_place(&Action { player: 1, kind: ActionKind::Bet(6.0) });

    let mut group = c.benchmark_group("tracker/apply_action");
    group.sample_size(30);
    // Call is the most common action and the one that iterates all
    // 1326 hands. Benchmark it on a fresh tracker each iteration.
    let call = Action { player: 0, kind: ActionKind::Call };
    group.bench_function("call_full_range", |b| {
        b.iter_batched(
            || RangeTracker::new(st.clone(), abs_dyn, &tbl, &ev),
            |mut t| {
                black_box(t.apply_action(black_box(call)).ok());
            },
            criterion::BatchSize::SmallInput,
        )
    });
    group.finish();
}

criterion_group!(benches, bench_tracker);
criterion_main!(benches);
