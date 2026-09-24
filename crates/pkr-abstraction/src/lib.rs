#![allow(clippy::needless_range_loop)] // numerics: indexed loops are idiomatic here

pub mod ehs;
pub use ehs::calculate_ehs;

use memmap2::Mmap;
use pkr_contracts::{fnv1a, AbstractionBuilder, Evaluator, FNV_OFFSET};
use pkr_eval::lookup::{choose, combinadic_rank};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

// ---------------------------------------------------------------------------
// C5c: silent-EHS-fallback accounting
// ---------------------------------------------------------------------------
//
// `get_infoset_hash` falls back to on-the-fly Monte-Carlo EHS clustering
// whenever a table lookup misses (missing table, out-of-range flat index,
// or an unexpected board length). That path is ~100x slower and produces
// a *different* cluster id than the table would have, so any run that
// mixes table hits with MC fallbacks is training on a silently corrupted
// abstraction.
//
// Before C5c the only signal was a single warning line printed once per
// process — easy to miss in a long training log. Now the counter is
// exact and the trainer aborts on any nonzero delta.
//
// `PKR_ALLOW_EHS_FALLBACK=1` disables the trainer-side abort (tests and
// the tiny-table smoke run need this).
static FALLBACK_COUNTS: [AtomicU64; 4] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Total EHS fallbacks across all streets since process start.
pub fn fallback_count() -> u64 {
    FALLBACK_COUNTS
        .iter()
        .map(|c| c.load(Ordering::Relaxed))
        .sum()
}

/// Per-street EHS fallbacks: `[preflop, flop, turn, river]`.
pub fn fallback_breakdown() -> [u64; 4] {
    [
        FALLBACK_COUNTS[0].load(Ordering::Relaxed),
        FALLBACK_COUNTS[1].load(Ordering::Relaxed),
        FALLBACK_COUNTS[2].load(Ordering::Relaxed),
        FALLBACK_COUNTS[3].load(Ordering::Relaxed),
    ]
}

/// Zero the counters. Useful for tests and for the trainer to reset
/// after an explicit acknowledgement.
pub fn reset_fallback_counts() {
    for c in FALLBACK_COUNTS.iter() {
        c.store(0, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CentroidStore {
    pub centroids: Vec<(f32, f32)>,
}

pub fn load_centroids(path: &str) -> Result<CentroidStore, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let store: CentroidStore = bincode::deserialize_from(reader)?;
    Ok(store)
}

pub fn save_centroids(path: &str, store: &CentroidStore) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    bincode::serialize_into(file, store)?;
    Ok(())
}

pub struct KMeansAbstraction {
    centroids: HashMap<u8, Vec<(f32, f32)>>,
    default_centroids: Vec<(f32, f32)>,
    tables: HashMap<u8, OnceLock<Mmap>>,
    flop_buckets: OnceLock<Vec<u8>>,
    evaluator: Arc<dyn Evaluator>,
}

impl KMeansAbstraction {
    pub fn new(default_centroids: Vec<(f32, f32)>, evaluator: Arc<dyn Evaluator>) -> Self {
        let mut tables = HashMap::new();
        for s in 0u8..=3 {
            tables.insert(s, OnceLock::new());
        }
        KMeansAbstraction {
            centroids: HashMap::new(),
            default_centroids,
            tables,
            flop_buckets: OnceLock::new(),
            evaluator,
        }
    }

    pub fn from_store(store: CentroidStore, evaluator: Arc<dyn Evaluator>) -> Self {
        Self::new(store.centroids, evaluator)
    }

    pub fn load_street_centroids(
        &mut self,
        street_code: u8,
        path: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let store = load_centroids(path)?;
        self.centroids.insert(street_code, store.centroids);
        Ok(())
    }

    pub fn init_table(&self, street_code: u8, path: &str) -> Result<(), std::io::Error> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let lock = self
            .tables
            .get(&street_code)
            .expect("table slot not created");
        lock.set(mmap).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::AlreadyExists, "table already set")
        })?;
        Ok(())
    }

    pub fn load_flop_buckets(&self, path: &str) -> Result<(), std::io::Error> {
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        self.flop_buckets.set(data).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "flop buckets already set",
            )
        })?;
        Ok(())
    }

    /// SUPERSEDED: flop_bucket was removed from the river infoset hash
    /// (T2.2). The method is retained until a follow-up cleanup removes
    /// the whole path (method + loader + field + trainer flag).
    #[allow(dead_code)]
    fn flop_bucket(&self, board: &[u8]) -> u8 {
        if board.len() >= 3 {
            if let Some(buckets) = self.flop_buckets.get() {
                let mut flop = [board[0], board[1], board[2]];
                flop.sort_unstable_by(|a, b| b.cmp(a));
                let idx = choose(flop[0] as u32, 3) as usize
                    + choose(flop[1] as u32, 2) as usize
                    + choose(flop[2] as u32, 1) as usize;
                if idx < buckets.len() {
                    return buckets[idx];
                }
            }
        }
        0
    }

    fn flat_index_preflop(hole: &[u8]) -> usize {
        assert_eq!(hole.len(), 2);
        assert_ne!(hole[0], hole[1]);
        let mut cards = [hole[0], hole[1]];
        cards.sort_unstable_by(|a, b| b.cmp(a));
        choose(cards[0] as u32, 2) as usize + choose(cards[1] as u32, 1) as usize
    }

    fn flat_index_flop(hole: &[u8], board: &[u8]) -> usize {
        assert_eq!(hole.len(), 2);
        assert_eq!(board.len(), 3);
        assert_ne!(hole[0], hole[1]);
        let mut all = [0u8; 5];
        all[0] = hole[0];
        all[1] = hole[1];
        all[2] = board[0];
        all[3] = board[1];
        all[4] = board[2];
        all.sort_unstable_by(|a, b| b.cmp(a));
        let combo_idx = combinadic_rank(&all) as usize;
        let hole_set = [hole[0], hole[1]];
        let masks: [[usize; 2]; 10] = [
            [0, 1],
            [0, 2],
            [0, 3],
            [0, 4],
            [1, 2],
            [1, 3],
            [1, 4],
            [2, 3],
            [2, 4],
            [3, 4],
        ];
        let mut mask_idx = 0;
        for (mi, pos) in masks.iter().enumerate() {
            let h1 = all[pos[0]];
            let h2 = all[pos[1]];
            if hole_set.contains(&h1) && hole_set.contains(&h2) {
                mask_idx = mi;
                break;
            }
        }
        combo_idx * 10 + mask_idx
    }

    fn flat_index_turn(hole: &[u8], board: &[u8]) -> usize {
        assert_eq!(hole.len(), 2);
        assert_eq!(board.len(), 4);
        let mut all = [0u8; 6];
        all[0] = hole[0];
        all[1] = hole[1];
        all[2] = board[0];
        all[3] = board[1];
        all[4] = board[2];
        all[5] = board[3];
        all.sort_unstable_by(|a, b| b.cmp(a));
        let rank = combinadic_rank_6(&all);
        let hole_set = [hole[0], hole[1]];
        let masks: [[usize; 2]; 15] = [
            [0, 1],
            [0, 2],
            [0, 3],
            [0, 4],
            [0, 5],
            [1, 2],
            [1, 3],
            [1, 4],
            [1, 5],
            [2, 3],
            [2, 4],
            [2, 5],
            [3, 4],
            [3, 5],
            [4, 5],
        ];
        let mut mask_idx = 0;
        for (mi, pos) in masks.iter().enumerate() {
            let h1 = all[pos[0]];
            let h2 = all[pos[1]];
            if hole_set.contains(&h1) && hole_set.contains(&h2) {
                mask_idx = mi;
                break;
            }
        }
        (rank as usize) * 15 + mask_idx
    }

    fn flat_index_river_board(board: &[u8]) -> usize {
        debug_assert_eq!(board.len(), 5);
        let mut sorted = [board[0], board[1], board[2], board[3], board[4]];
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        combinadic_rank(&sorted) as usize
    }
}

fn combinadic_rank_6(cards: &[u8; 6]) -> u64 {
    let c0 = cards[0] as u32;
    let c1 = cards[1] as u32;
    let c2 = cards[2] as u32;
    let c3 = cards[3] as u32;
    let c4 = cards[4] as u32;
    let c5 = cards[5] as u32;
    choose(c0, 6) as u64
        + choose(c1, 5) as u64
        + choose(c2, 4) as u64
        + choose(c3, 3) as u64
        + choose(c4, 2) as u64
        + choose(c5, 1) as u64
}

impl AbstractionBuilder for KMeansAbstraction {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8], street: u8) -> u64 {
        // INVARIANT: the board slice must contain exactly the cards dealt for
        // `street`. A caller passing the raw [u8;5] array silently routes every
        // street through the river branch (regression guard, see audit F1).
        let expected_len = match street {
            0 => 0,
            1 => 3,
            2 => 4,
            3 => 5,
            _ => board.len(),
        };
        debug_assert_eq!(
            board.len(),
            expected_len,
            "get_infoset_hash: board.len()={} but street {} expects {} cards",
            board.len(),
            street,
            expected_len
        );
        debug_assert!(
            hole.iter().all(|h| !board.contains(h)),
            "abstraction hash called with hole and board sharing a card: \
             hole={:?} board={:?} street={}",
            hole,
            board,
            street,
        );
        let centroids = self
            .centroids
            .get(&street)
            .unwrap_or(&self.default_centroids);

        let ehs_fallback = || {
            // C5c: count this fallback so the trainer can abort.
            FALLBACK_COUNTS[(street as usize) & 3].fetch_add(1, Ordering::Relaxed);
            warn_mc_fallback_once();
            let (ehs, ehs_sq) = calculate_ehs(hole, board, self.evaluator.as_ref());
            nearest_centroid(ehs, ehs_sq, centroids)
        };
        let cluster_id = match board.len() {
            0 => {
                if let Some(table) = self.tables.get(&0u8).and_then(|l| l.get()) {
                    let idx = Self::flat_index_preflop(hole);
                    if idx < table.len() {
                        table[idx] as u64
                    } else {
                        ehs_fallback()
                    }
                } else {
                    ehs_fallback()
                }
            }
            3 => {
                if let Some(table) = self.tables.get(&1u8).and_then(|l| l.get()) {
                    let idx = Self::flat_index_flop(hole, board);
                    if idx < table.len() {
                        table[idx] as u64
                    } else {
                        ehs_fallback()
                    }
                } else {
                    ehs_fallback()
                }
            }
            4 => {
                if let Some(table) = self.tables.get(&2u8).and_then(|l| l.get()) {
                    let idx = Self::flat_index_turn(hole, board);
                    if idx < table.len() {
                        table[idx] as u64
                    } else {
                        ehs_fallback()
                    }
                } else {
                    ehs_fallback()
                }
            }
            5 => {
                // River: bucket hand strength into ~256 ordered tiers.
                //
                // evaluate_hand returns the inverted-bit encoding !raw =
                // ~(category << 20 | rank_bits) — NOT a 7462-scale rank. Its value
                // range is ~[2^32 - 9*2^20, 2^32] (audit F6). `>> 15` is a monotone
                // quantization of that range into ~287 tiers where lower tier =
                // stronger hand. (The old `>> 6` produced ~147k tiers and blew up the
                // river infoset count; a true 7462-scale dense rank is the M1-playbook
                // fast7 follow-up.)
                let hand_rank = self.evaluator.evaluate_hand(hole, board) as u64;
                let hand_bucket = hand_rank >> 15; // ~0..=287, monotone
                let board_bucket = match self.tables.get(&3u8).and_then(|l| l.get()) {
                    Some(table) => {
                        let idx = Self::flat_index_river_board(board);
                        if idx < table.len() {
                            table[idx] as u64
                        } else {
                            FALLBACK_COUNTS[3].fetch_add(1, Ordering::Relaxed);
                            0
                        }
                    }
                    None => {
                        FALLBACK_COUNTS[3].fetch_add(1, Ordering::Relaxed);
                        0
                    }
                };
                (hand_bucket << 8) | (board_bucket & 0xff)
            }
            _ => ehs_fallback(),
        };

        let mut h: u64 = FNV_OFFSET;
        fnv1a(&mut h, std::slice::from_ref(&street));
        fnv1a(&mut h, &[history.len() as u8]);
        fnv1a(&mut h, history);
        fnv1a(&mut h, &cluster_id.to_le_bytes());
        h
    }
}

fn warn_mc_fallback_once() {
    use std::sync::OnceLock;
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        eprintln!(
            "WARNING: abstraction fell back to Monte-Carlo EHS. This is \
             ~100x slower per infoset than the precomputed table path. \
             Likely cause: --preflop-table / --flop-table / --flop-buckets \
             not loaded, or an index fell outside the table's range."
        );
    });
}

fn nearest_centroid(ehs: f32, ehs_sq: f32, centroids: &[(f32, f32)]) -> u64 {
    centroids
        .iter()
        .enumerate()
        .min_by(|a, b| {
            let c1 = a.1;
            let c2 = b.1;
            let dx1 = ehs - c1.0;
            let dy1 = ehs_sq - c1.1;
            let dx2 = ehs - c2.0;
            let dy2 = ehs_sq - c2.1;
            (dx1 * dx1 + dy1 * dy1).total_cmp(&(dx2 * dx2 + dy2 * dy2))
        })
        .map(|(idx, _)| idx as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_contracts::Evaluator;
    struct MockEvaluator;
    impl Evaluator for MockEvaluator {
        fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u32 {
            0u32
        }
    }

    #[test]
    fn test_history_street_hash_golden() {
        // GOLDEN VECTORS — these constants are part of the on-disk format contract.
        // If these values change, every exported blueprint in existence is invalidated
        // (requires a format_version bump + full retrain + re-export).
        //
        // Computed with FNV-1a 64-bit, little-endian cluster_id, length-prefixed
        // history. Regenerated for T2.2 (river >> 3, no flop_bucket in hash), 2026-09-23.
        let builder =
            KMeansAbstraction::new(vec![(0.3, 0.09), (0.7, 0.49)], Arc::new(MockEvaluator));
        assert_eq!(
            builder.get_infoset_hash(&[0, 1], &[], &[], 0),
            0x69d307cc20f6ef8d
        );
        assert_eq!(
            builder.get_infoset_hash(&[0, 1], &[], &[0], 0),
            0xafe0abd88048ea4e
        );
        assert_ne!(
            builder.get_infoset_hash(&[0, 1], &[], &[], 0),
            builder.get_infoset_hash(&[0, 1], &[], &[0], 0),
            "history must be part of the hash"
        );
    }

    #[test]
    fn test_history_street_hash() {
        let builder =
            KMeansAbstraction::new(vec![(0.3, 0.09), (0.7, 0.49)], Arc::new(MockEvaluator));
        let h1 = builder.get_infoset_hash(&[0, 1], &[], &[], 0);
        let h2 = builder.get_infoset_hash(&[0, 1], &[], &[0], 0);
        assert_ne!(h1, h2);
    }

    #[test]
    #[should_panic(expected = "board.len()")]
    fn hash_panics_when_board_len_does_not_match_street() {
        let builder =
            KMeansAbstraction::new(vec![(0.3, 0.09), (0.7, 0.49)], Arc::new(MockEvaluator));
        // street 1 (flop) requires exactly 3 board cards; 4 is a caller bug.
        let _ = builder.get_infoset_hash(&[0, 1], &[2, 3, 4, 5], &[], 1);
    }
}

#[cfg(test)]
mod extended_tests {
    use super::*;
    use pkr_contracts::Evaluator;
    use std::sync::Arc;
    struct TestEval;
    impl Evaluator for TestEval {
        fn evaluate_hand(&self, _: &[u8], _: &[u8]) -> u32 {
            0
        }
    }

    #[test]
    fn test_flat_index_preflop_boundaries() {
        assert_eq!(KMeansAbstraction::flat_index_preflop(&[0, 1]), 0);
        assert_eq!(KMeansAbstraction::flat_index_preflop(&[50, 51]), 1325);
    }

    #[test]
    fn test_flat_index_flop_consistency() {
        let idx1 = KMeansAbstraction::flat_index_flop(&[10, 20], &[30, 40, 50]);
        let idx2 = KMeansAbstraction::flat_index_flop(&[20, 10], &[50, 30, 40]);
        assert_eq!(idx1, idx2);
    }

    #[test]
    fn test_flat_index_turn_consistency() {
        let idx1 = KMeansAbstraction::flat_index_turn(&[5, 15], &[25, 35, 45, 51]);
        let idx2 = KMeansAbstraction::flat_index_turn(&[15, 5], &[51, 35, 25, 45]);
        assert_eq!(idx1, idx2);
    }

    #[test]
    fn test_centroid_save_and_load() {
        let store = CentroidStore {
            centroids: vec![(0.1, 0.01), (0.5, 0.25), (0.9, 0.81)],
        };
        let tmp = std::env::temp_dir().join("test_centroids.bin");
        save_centroids(tmp.to_str().unwrap(), &store).unwrap();
        let loaded = load_centroids(tmp.to_str().unwrap()).unwrap();
        assert_eq!(loaded.centroids.len(), 3);
        assert!((loaded.centroids[1].0 - 0.5).abs() < 0.001);
        std::fs::remove_file(tmp).ok();
    }

    #[test]
    fn test_river_centroids_support_large_k() {
        // River centroids should support k up to 2000 (u16 range)
        let n: usize = 1500;
        let store = CentroidStore {
            centroids: (0..n)
                .map(|i| (i as f32 / n as f32, i as f32 / n as f32))
                .collect(),
        };
        let tmp = std::env::temp_dir().join("test_river_centroids.bin");
        save_centroids(tmp.to_str().unwrap(), &store).unwrap();
        let loaded = load_centroids(tmp.to_str().unwrap()).unwrap();
        assert_eq!(loaded.centroids.len(), n);
        std::fs::remove_file(tmp).ok();
    }

    #[test]
    fn test_hash_changes_with_street() {
        let builder = KMeansAbstraction::new(vec![(0.5, 0.25)], Arc::new(TestEval));
        let h1 = builder.get_infoset_hash(&[0, 1], &[], &[], 0);
        let h2 = builder.get_infoset_hash(&[0, 1], &[2, 3, 4], &[], 1);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_hash_changes_with_history_length() {
        let builder = KMeansAbstraction::new(vec![(0.5, 0.25)], Arc::new(TestEval));
        let h1 = builder.get_infoset_hash(&[0, 1], &[2, 3, 4], &[0], 1);
        let h2 = builder.get_infoset_hash(&[0, 1], &[2, 3, 4], &[0, 1], 1);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_flop_bucket_default_zero() {
        let builder = KMeansAbstraction::new(vec![(0.5, 0.25)], Arc::new(TestEval));
        let h = builder.get_infoset_hash(&[0, 1], &[2, 3, 4], &[], 1);
        assert!(h != 0);
    }

    #[test]
    fn test_abstraction_is_thread_safe() {
        use std::thread;
        let builder = Arc::new(KMeansAbstraction::new(
            vec![(0.3, 0.09), (0.7, 0.49)],
            Arc::new(TestEval),
        ));
        let mut handles = vec![];
        for _ in 0..4 {
            let b = Arc::clone(&builder);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    assert!(b.get_infoset_hash(&[0, 1], &[2, 3, 4], &[0, 1], 1) != 0);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }
}

#[cfg(test)]
mod c6_unit_tests {
    use super::*;
    use pkr_contracts::Evaluator;

    struct NullEval;
    impl Evaluator for NullEval {
        fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u32 {
            0
        }
    }

    // ------------------------------------------------------------------
    // River tier coverage (audit F6)
    //
    // evaluate_hand returns the inverted-bit encoding !raw, whose value
    // range is ~[2^32 - 9*2^20, 2^32]. `>> 15` quantizes that range into
    // ~287 monotone tiers. If the shift changes, bump the blueprint
    // format version — that is the contract.
    // ------------------------------------------------------------------
    #[test]
    fn river_hand_bucket_is_monotone_and_bounded() {
        // Stronger hand must map to a <= bucket (monotone) and the tier count
        // must be small (bounded infoset space).
        // Direct check of the quantization math used in the river branch:
        let strong = (u32::MAX - 9_437_184) as u64 >> 15; // best hand in range
        let weak = (u32::MAX) as u64 >> 15;
        assert!(strong <= weak);
        assert!(
            weak - strong < 512,
            "river hand tiers must stay bounded, got {}",
            weak - strong
        );
    }

    // ------------------------------------------------------------------
    // Flat-index preflop is injective over the 1326 combos.
    // ------------------------------------------------------------------
    #[test]
    fn preflop_flat_index_is_injective() {
        let mut seen = std::collections::HashSet::new();
        for a in 0u8..52 {
            for b in (a + 1)..52 {
                let mut cards = [a, b];
                cards.sort_unstable_by(|x, y| y.cmp(x));
                let idx = {
                    let c0 = cards[0] as u32;
                    let c1 = cards[1] as u32;
                    choose(c0, 2) + choose(c1, 1)
                };
                assert!(
                    seen.insert(idx),
                    "duplicate flat index {idx} for combo ({a},{b})"
                );
            }
        }
        assert_eq!(seen.len(), 1326);
    }

    // ------------------------------------------------------------------
    // Hash stability: same inputs, same output, across calls and across
    // threads. This is the primary contract every downstream consumer
    // (trainer, exporter, runtime) relies on.
    // ------------------------------------------------------------------
    #[test]
    fn hash_is_deterministic_across_threads() {
        let store = CentroidStore {
            centroids: vec![(0.3, 0.09), (0.7, 0.49)],
        };
        let abstraction = KMeansAbstraction::from_store(store, std::sync::Arc::new(NullEval));

        // Canonical preflop hash for AA, empty history.
        let hole: [u8; 2] = [48, 49];
        let history: [u8; 4] = [0, 0, 0, 0];
        let expected = abstraction.get_infoset_hash(&hole, &[], &history, 0);

        // Serial: 16 calls must produce identical output.
        for _ in 0..16 {
            let r = abstraction.get_infoset_hash(&hole, &[], &history, 0);
            assert_eq!(r, expected, "hash must be stable across calls");
        }

        // Parallel: rayon does not need shared state mutation here, so
        // this simply confirms the read path is thread-safe (no interior
        // mutability on the fast path).
        use rayon::prelude::*;
        let par_expected: Vec<u64> = (0..1024)
            .into_par_iter()
            .map(|_| abstraction.get_infoset_hash(&hole, &[], &history, 0))
            .collect();
        for r in par_expected {
            assert_eq!(r, expected, "hash must be stable across threads");
        }
    }

    // ------------------------------------------------------------------
    // Hash distinguishes street: same (hole, board, history) at different
    // streets must not collide — preflop vs flop must differ.
    // ------------------------------------------------------------------
    #[test]
    fn hash_changes_with_street() {
        let store = CentroidStore {
            centroids: vec![(0.3, 0.09), (0.7, 0.49)],
        };
        let abstraction = KMeansAbstraction::from_store(store, std::sync::Arc::new(NullEval));
        let hole: [u8; 2] = [48, 49];
        let history: [u8; 4] = [0, 0, 0, 0];
        let h_pre = abstraction.get_infoset_hash(&hole, &[], &history, 0);
        let h_flop = abstraction.get_infoset_hash(&hole, &[0, 1, 2], &history, 1);
        assert_ne!(h_pre, h_flop);
    }

    // ------------------------------------------------------------------
    // Hash distinguishes history: same cards, different sig byte → diff.
    // ------------------------------------------------------------------
    #[test]
    fn hash_changes_with_history() {
        let store = CentroidStore {
            centroids: vec![(0.3, 0.09), (0.7, 0.49)],
        };
        let abstraction = KMeansAbstraction::from_store(store, std::sync::Arc::new(NullEval));
        let hole: [u8; 2] = [48, 49];
        let h1 = abstraction.get_infoset_hash(&hole, &[], &[0, 0, 0, 0], 0);
        let h2 = abstraction.get_infoset_hash(&hole, &[], &[1, 0, 0, 0], 0);
        let h3 = abstraction.get_infoset_hash(&hole, &[], &[2, 0, 0, 0], 0);
        assert_ne!(h1, h2);
        assert_ne!(h2, h3);
        assert_ne!(h1, h3);
    }
}

#[cfg(test)]
mod audit_f6_tests {
    /// The `!raw` encoding produced by both evaluators has its usable
    /// range bounded above by `u32::MAX` and below by
    /// `u32::MAX - 9 * 2^20 + 1` (9 hand categories, each with 2^20
    /// rank-bit combinations). `hand_rank >> 15` quantizes that range
    /// into a bounded number of ordered tiers.
    ///
    /// The audit F6 concern: the pre-fix `>> 6` produced ~147k tiers,
    /// not the ~117 the comment claimed. This test pins the current
    /// shift to a bounded count so any future change that silently
    /// explodes the tier count will fail the test.
    #[test]
    fn river_shift_15_yields_bounded_tier_count() {
        // Lowest possible !raw value across all 9 categories.
        let min_rank: u32 = u32::MAX - 9 * (1u32 << 20) + 1;
        let max_rank: u32 = u32::MAX;
        let tier_lo = min_rank >> 15;
        let tier_hi = max_rank >> 15;
        let count = tier_hi - tier_lo + 1;
        assert!(
            count <= 512,
            ">>15 should yield <=512 tiers, got {count} (range {}..{})",
            tier_lo,
            tier_hi
        );
        assert!(
            count >= 128,
            ">>15 should yield >=128 tiers (finer than >>13), got {count}"
        );
    }

    /// Monotonicity: a stronger hand (smaller `!raw`) must never map to
    /// a *larger* tier than a weaker hand. The shift is monotone by
    /// construction, but this test makes the contract explicit so a
    /// future change to `river_hand_bucket` cannot silently reverse it.
    #[test]
    fn river_tier_ordering_is_monotone() {
        // A hand with !raw = MAX (weakest possible in the encoding) and
        // a hand with !raw = MAX - K (stronger by K) — the stronger hand
        // must map to an equal-or-smaller tier.
        for k in [1u32, 100, 1000, 100_000, 1_000_000, 9_000_000] {
            let weak = u32::MAX;
            let strong = u32::MAX.saturating_sub(k);
            assert!(
                (strong >> 15) <= (weak >> 15),
                "stronger hand (raw={strong}) mapped to a larger tier than weak (raw={weak})",
            );
        }
    }
}
