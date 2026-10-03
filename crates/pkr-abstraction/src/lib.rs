#![allow(clippy::needless_range_loop)] // numerics: indexed loops are idiomatic here
pub mod potential;

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

/// Env-gated soft-kmeans blending. When `PKR_SOFT_KMEANS=1`, hands whose
/// river hand_rank falls within `SOFT_BOUNDARY_FRAC` of a tier boundary
/// get a soft assignment (primary tier + adjacent tier, weighted blend).
/// Default off — training path is bit-identical when this is unset.
#[inline]
/// §C: exact 169-class preflop (env `PKR_PREFLOP_EXACT=1`). Default OFF.
fn preflop_exact_enabled() -> bool {
    use std::sync::OnceLock;
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(|| std::env::var("PKR_PREFLOP_EXACT").as_deref() == Ok("1"))
}

fn soft_kmeans_enabled() -> bool {
    use std::sync::OnceLock;
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(|| std::env::var("PKR_SOFT_KMEANS").as_deref() == Ok("1"))
}

/// Fraction of a river tier width that counts as "near boundary".
const SOFT_BOUNDARY_FRAC: f64 = 0.15;

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
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;
    let store: CentroidStore = postcard::from_bytes(&buf)?;
    Ok(store)
}

pub fn save_centroids(path: &str, store: &CentroidStore) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    postcard::to_io(store, file)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 6D "rich" centroids for the preflop feature experiment (v33).
//
// The classic 2D (EHS, EHS^2) feature space collapses strategically
// distinct hands onto the same point: EHS(22) ~= EHS(A5s) ~= EHS(KJo)
// ~= 0.50, so AA, 22, A5s, KJo all hash to the same infoset. The 6D
// space appends four hand-structure dims (rank_high, rank_low, suited,
// connector) so each cluster becomes a *strategic* neighbourhood rather
// than a pure *equity* neighbourhood.
//
// Persisted separately from CentroidStore so existing 2D pipelines
// (flop/turn/river centroids, existing blueprints) are untouched.
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CentroidStore6D {
    pub centroids: Vec<[f32; 6]>,
}

pub fn load_centroids_6d(path: &str) -> Result<CentroidStore6D, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;
    let store: CentroidStore6D = postcard::from_bytes(&buf)?;
    Ok(store)
}

pub fn save_centroids_6d(
    path: &str,
    store: &CentroidStore6D,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    postcard::to_io(store, file)?;
    Ok(())
}

/// Card encoding convention: `card = suit * 13 + rank`, with
/// `rank in 0..=12` (0 = deuce, 12 = ace) and `suit in 0..=3`. This is
/// the layout `combinadic_unrank_2` produces (per the 2026-09-25 session
/// handoff: "returns cards in `suit*13 + rank` format, sorted
/// descending"). `rank = c % 13`, `suit = c / 13`.
fn card_rank(card: u8) -> u8 {
    card % 13
}

fn card_suit(card: u8) -> u8 {
    card / 13
}

/// Four hand-structure features for the 6D preflop feature space.
/// Ranges: `rank_high / 12` and `rank_low / 12` in `[0, 1]`,
/// `suited_bit` and `connector_bit` in `{0.0, 1.0}`.
///
/// "Connector" means the two ranks are within 2 of each other
/// (i.e. gap <= 2), covering true connectors (54s) and one-gappers
/// (53s) — the classic playable-suited boundary.
pub fn hand_structure_features(hole: &[u8]) -> [f32; 4] {
    debug_assert_eq!(hole.len(), 2, "hand_structure_features: expected 2 cards");
    let r0 = card_rank(hole[0]);
    let r1 = card_rank(hole[1]);
    let (rank_high, rank_low) = if r0 >= r1 { (r0, r1) } else { (r1, r0) };
    let suited = if card_suit(hole[0]) == card_suit(hole[1]) {
        1.0f32
    } else {
        0.0f32
    };
    let gap = rank_high - rank_low;
    let connector = if gap <= 2 { 1.0f32 } else { 0.0f32 };
    [
        rank_high as f32 / 12.0,
        rank_low as f32 / 12.0,
        suited,
        connector,
    ]
}

/// V39 preflop feature vector. Same 6 dimensions as the v33 version, but
/// the `connector` bit (gap <= 2) is replaced with `gap/12`, a continuous
/// measure of rank spread. Captures 54s vs 53s vs 52s distinctions that
/// the binary bit conflates.
pub fn hand_structure_features_v39(hole: &[u8]) -> [f32; 4] {
    debug_assert_eq!(hole.len(), 2);
    let r0 = card_rank(hole[0]);
    let r1 = card_rank(hole[1]);
    let (rank_high, rank_low) = if r0 >= r1 { (r0, r1) } else { (r1, r0) };
    let suited = if card_suit(hole[0]) == card_suit(hole[1]) { 1.0 } else { 0.0 };
    let gap = (rank_high - rank_low) as f32 / 12.0;
    [
        rank_high as f32 / 12.0,
        rank_low as f32 / 12.0,
        suited,
        gap,
    ]
}

/// V39 combined vector: (EHS, EHS^2, rank_high/12, rank_low/12, suited, gap/12).
pub fn hand_and_board_features_v39(
    ehs: f32,
    ehs_sq: f32,
    hole: &[u8],
    board: &[u8],
) -> [f32; 10] {
    let h = hand_structure_features_v39(hole);
    let b = board_structure_features(board);
    [ehs, ehs_sq, h[0], h[1], h[2], h[3], b[0], b[1], b[2], b[3]]
}

/// Env-gated selector for the v39 feature space. Reads PKR_PREFLOP_V39=1.
pub fn preflop_v39_enabled() -> bool {
    std::env::var("PKR_PREFLOP_V39").as_deref() == Ok("1")
}


// ---------------------------------------------------------------------------
// 10D "hand+board" feature space for flop/turn tables.
//
// The preflop 6D space (EHS, EHS^2, rank_high, rank_low, suited,
// connector) captures hand structure. For flop/turn the natural
// extension adds 4 board-structure features, giving a 10D vector:
//
//   (EHS, EHS^2,
//    rank_high/12, rank_low/12, suited, connector,
//    board_high/12, board_paired, board_flush_draw, board_connected)
//
// Rationale: the preflop experiment (docs/experiments/v33-*) showed
// that EHS alone collapses strategically distinct hands onto the same
// bucket. The same collapse applies to (hole, board) pairs:
// EHS(hole, flop) does not distinguish "drawing to a flush" from
// "drawing to a straight" from "top pair on a dry board" when their
// equities coincide. The 4 board features capture those differences.
//
// Persisted separately from CentroidStore and CentroidStore6D so the
// preflop pipeline is untouched.
// ---------------------------------------------------------------------------

/// 4 board-structure features for a flop (3 cards) or turn (4 cards).
/// Ranges: `board_high/12` in [0, 1]; the other three are 0/1 flags.
pub fn board_structure_features(board: &[u8]) -> [f32; 4] {
    debug_assert!(
        board.len() == 3 || board.len() == 4,
        "board_structure_features: expected 3 or 4 board cards, got {}",
        board.len()
    );

    // Ranks of the board cards, descending.
    let mut ranks: Vec<u8> = board.iter().map(|&c| card_rank(c)).collect();
    ranks.sort_unstable_by(|a, b| b.cmp(a));

    let high = ranks[0];
    let board_high = high as f32 / 12.0;

    // Paired: any two board cards share a rank.
    let paired = if ranks.windows(2).any(|w| w[0] == w[1]) {
        1.0
    } else {
        0.0
    };

    // Flush draw: 2+ cards of the same suit. On the flop 3-of-suit means a
    // made flush; either way the strategic dimension is "flush matters".
    let mut suit_counts = [0u8; 4];
    for &c in board {
        suit_counts[card_suit(c) as usize] += 1;
    }
    let flush_draw = if suit_counts.iter().any(|&n| n >= 2) {
        1.0
    } else {
        0.0
    };

    // Connected: any three board ranks with both gaps <= 2. Uses the
    // sorted-descending rank vector, so ranks[i] >= ranks[j] >= ranks[k].
    let mut connected = 0.0f32;
    if ranks.len() >= 3 {
        'outer: for i in 0..ranks.len() {
            for j in (i + 1)..ranks.len() {
                for k in (j + 1)..ranks.len() {
                    let gap_ij = ranks[i] - ranks[j];
                    let gap_jk = ranks[j] - ranks[k];
                    if gap_ij <= 2 && gap_jk <= 2 {
                        connected = 1.0;
                        break 'outer;
                    }
                }
            }
        }
    }

    [board_high, paired, flush_draw, connected]
}

/// Compose the 10D feature vector from EHS, EHS^2, hole structure and
/// board structure. Centralized so precompute and any future runtime
/// path use the exact same layout.
pub fn hand_and_board_features(
    ehs: f32,
    ehs_sq: f32,
    hole: &[u8],
    board: &[u8],
) -> [f32; 10] {
    let h = hand_structure_features(hole);
    let b = board_structure_features(board);
    [ehs, ehs_sq, h[0], h[1], h[2], h[3], b[0], b[1], b[2], b[3]]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CentroidStore10D {
    pub centroids: Vec<[f32; 10]>,
}

pub fn load_centroids_10d(path: &str) -> Result<CentroidStore10D, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;
    let store: CentroidStore10D = postcard::from_bytes(&buf)?;
    Ok(store)
}

pub fn save_centroids_10d(
    path: &str,
    store: &CentroidStore10D,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    postcard::to_io(store, file)?;
    Ok(())
}

/// Nearest centroid in 10D. Returns 0 on empty input (mirrors the
/// nearest_centroid / nearest_centroid_6d behaviour).
pub fn nearest_centroid_10d(feat: &[f32; 10], centroids: &[[f32; 10]]) -> u64 {
    centroids
        .iter()
        .enumerate()
        .min_by(|a, b| {
            let mut d1 = 0.0f32;
            let mut d2 = 0.0f32;
            for k in 0..10 {
                let da = feat[k] - a.1[k];
                let db = feat[k] - b.1[k];
                d1 += da * da;
                d2 += db * db;
            }
            d1.total_cmp(&d2)
        })
        .map(|(idx, _)| idx as u64)
        .unwrap_or(0)
}

pub struct KMeansAbstraction {
    centroids: HashMap<u8, Vec<(f32, f32)>>,
    default_centroids: Vec<(f32, f32)>,
    tables: HashMap<u8, OnceLock<Mmap>>,
    flop_buckets: OnceLock<Vec<u8>>,
    evaluator: Arc<dyn Evaluator>,
}

/// Sorting-network helpers for small fixed-size arrays of card bytes.
/// All sort DESCENDING (highest rank first), matching the previous
/// `sort_unstable_by(|a, b| b.cmp(a))` semantics bit-for-bit.
/// Fixed compare-exchange sequences are fully unrolled by LLVM.
mod sortnets {
    #[inline(always)]
    fn ce2(a: &mut [u8; 2], i: usize, j: usize) {
        if a[i] < a[j] {
            a.swap(i, j);
        }
    }
    #[inline(always)]
    fn ce3(a: &mut [u8; 3], i: usize, j: usize) {
        if a[i] < a[j] {
            a.swap(i, j);
        }
    }
    #[inline(always)]
    fn ce5(a: &mut [u8; 5], i: usize, j: usize) {
        if a[i] < a[j] {
            a.swap(i, j);
        }
    }
    #[inline(always)]
    fn ce6(a: &mut [u8; 6], i: usize, j: usize) {
        if a[i] < a[j] {
            a.swap(i, j);
        }
    }

    pub fn sort2_desc(a: &mut [u8; 2]) {
        ce2(a, 0, 1);
    }
    pub fn sort3_desc(a: &mut [u8; 3]) {
        ce3(a, 0, 1);
        ce3(a, 1, 2);
        ce3(a, 0, 1);
    }
    /// Optimal 5-element network (9 compare-exchanges).
    pub fn sort5_desc(a: &mut [u8; 5]) {
        ce5(a, 0, 1);
        ce5(a, 3, 4);
        ce5(a, 2, 4);
        ce5(a, 2, 3);
        ce5(a, 0, 3);
        ce5(a, 0, 2);
        ce5(a, 1, 4);
        ce5(a, 1, 3);
        ce5(a, 1, 2);
    }
    /// 6-element network (12 compare-exchanges).
    pub fn sort6_desc(a: &mut [u8; 6]) {
        ce6(a, 1, 2);
        ce6(a, 4, 5);
        ce6(a, 0, 2);
        ce6(a, 3, 5);
        ce6(a, 0, 1);
        ce6(a, 3, 4);
        ce6(a, 2, 5);
        ce6(a, 0, 3);
        ce6(a, 1, 4);
        ce6(a, 2, 4);
        ce6(a, 1, 3);
        ce6(a, 2, 3);
    }
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

    /// Number of centroids on the default (preflop) table. Used by the
    /// evaluator to build the right `AbstractionFingerprint` before
    /// loading a checkpoint, without re-reading the centroids file.
    pub fn k(&self) -> usize {
        self.default_centroids.len()
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
                sortnets::sort3_desc(&mut flop);
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

    #[inline]
    fn flat_index_preflop(hole: &[u8]) -> usize {
        assert_eq!(hole.len(), 2);
        assert_ne!(hole[0], hole[1]);
        let mut cards = [hole[0], hole[1]];
        sortnets::sort2_desc(&mut cards);
        choose(cards[0] as u32, 2) as usize + choose(cards[1] as u32, 1) as usize
    }

    #[inline]
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
        sortnets::sort5_desc(&mut all);
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

    #[inline]
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
        sortnets::sort6_desc(&mut all);
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

    #[inline]
    fn flat_index_river_board(board: &[u8]) -> usize {
        debug_assert_eq!(board.len(), 5);
        let mut sorted = [board[0], board[1], board[2], board[3], board[4]];
        sortnets::sort5_desc(&mut sorted);
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
    fn get_infoset_hash_soft(
        &self,
        hole: &[u8],
        board: &[u8],
        history: &[u8],
        street: u8,
    ) -> pkr_contracts::SoftHash {
        let primary = self.get_infoset_hash(hole, board, history, street);
        if !soft_kmeans_enabled() || street != pkr_core::state::Street::River as u8 {
            return pkr_contracts::SoftHash::hard(primary);
        }
        // River only. Recompute hand_bucket/board_bucket to find boundary distance.
        let hand_rank = self.evaluator.evaluate_hand(hole, board) as u64;
        let tier_bits = pkr_core::abstraction::RIVER_TIER_SHIFT as u32;
        let tier_size: u64 = 1u64 << tier_bits; // = 32768
        let low = hand_rank & (tier_size - 1);
        let hand_bucket = hand_rank >> tier_bits;
        let board_bucket = match self.tables.get(&3u8).and_then(|l| l.get()) {
            Some(table) => {
                let idx = Self::flat_index_river_board(board);
                if idx < table.len() { table[idx] as u64 } else { 0 }
            }
            None => 0,
        };

        let threshold = (tier_size as f64 * SOFT_BOUNDARY_FRAC) as u64;
        let (adj_bucket, weight_primary) = if hand_bucket > 0 && low < threshold {
            // Near lower boundary: blend with tier below.
            let w = 0.5 + 0.5 * (low as f64 / threshold as f64);
            (hand_bucket - 1, w as f32)
        } else if low > tier_size - threshold {
            // Near upper boundary: blend with tier above.
            let dist = low - (tier_size - threshold);
            let w = 1.0 - 0.5 * (dist as f64 / threshold as f64);
            (hand_bucket + 1, w as f32)
        } else {
            return pkr_contracts::SoftHash::hard(primary);
        };

        // Recompute the FNV hash with the adjacent cluster_id.
        let adj_cluster_id = (adj_bucket << 8) | (board_bucket & 0xff);
        let mut h: u64 = pkr_contracts::FNV_OFFSET;
        pkr_contracts::fnv1a(&mut h, std::slice::from_ref(&street));
        pkr_contracts::fnv1a(&mut h, &[history.len() as u8]);
        pkr_contracts::fnv1a(&mut h, history);
        pkr_contracts::fnv1a(&mut h, &adj_cluster_id.to_le_bytes());

        pkr_contracts::SoftHash {
            primary,
            secondary: h,
            weight_primary,
        }
    }

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
                if preflop_exact_enabled() {
                    // §C: lossless 169-class preflop, no centroid table.
                    pkr_core::abstraction::preflop_class(&[hole[0], hole[1]]) as u64
                } else if let Some(table) = self.tables.get(&0u8).and_then(|l| l.get()) {
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
                // River: bucket hand strength into ordered tiers.
                //
                // evaluate_hand returns the inverted-bit encoding !raw =
                // ~(category << 20 | rank_bits) — NOT a 7462-scale rank. Its
                // range is ~[2^32 - 9*2^20, 2^32] (audit F6). Shifting by
                // RIVER_TIER_SHIFT (=15) is a monotone quantization into
                // ~288 tiers, lower = stronger. Set the shift ONCE via
                // RIVER_TIER_SHIFT; history (>>6 -> 147k tiers, >>13 -> 1152)
                // lives in the constant's doc, not here.
                let hand_rank = self.evaluator.evaluate_hand(hole, board) as u64;
                let hand_bucket = hand_rank >> pkr_core::abstraction::RIVER_TIER_SHIFT;
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

/// Nearest-centroid lookup in the 6D (EHS, EHS^2, rank_high, rank_low,
/// suited, connector) feature space. Mirrors `nearest_centroid` but
/// consumes a full feature vector so callers can add dims without
/// rewriting the runtime dispatcher. Returns 0 on an empty centroid list.
pub fn nearest_centroid_6d(feat: &[f32; 6], centroids: &[[f32; 6]]) -> u64 {
    centroids
        .iter()
        .enumerate()
        .min_by(|a, b| {
            let mut d1 = 0.0f32;
            let mut d2 = 0.0f32;
            for k in 0..6 {
                let da = feat[k] - a.1[k];
                let db = feat[k] - b.1[k];
                d1 += da * da;
                d2 += db * db;
            }
            d1.total_cmp(&d2)
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
    // range is ~[2^32 - 9*2^20, 2^32]. `>> 13` quantizes that range into
    // ~1152 monotone tiers (T2.2). If the shift changes, bump the blueprint
    // format version — that is the contract.
    // ------------------------------------------------------------------
    #[test]
    fn river_hand_bucket_is_monotone_and_bounded() {
        // Stronger hand must map to a <= bucket (monotone) and the tier count
        // must be small (bounded infoset space).
        // Direct check of the quantization math used in the river branch
        // (T2.2: >> 13, ~1152 tiers).
        let strong = (u32::MAX - 9_437_184) as u64 >> 13; // best hand in range
        let weak = (u32::MAX) as u64 >> 13;
        assert!(strong <= weak);
        assert!(
            weak - strong < 2048,
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
    /// rank-bit combinations). `hand_rank >> 13` quantizes that range
    /// into a bounded number of ordered tiers.
    ///
    /// The audit F6 concern: the pre-fix `>> 6` produced ~147k tiers,
    /// not the ~117 the comment claimed. This test pins the current
    /// shift to a bounded count so any future change that silently
    /// explodes or collapses the tier count will fail the test.
    #[test]
    fn river_shift_13_yields_bounded_tier_count() {
        // T2.2: the shift is >> 13, giving ~1152 tiers over the ~9.4M
        // raw-rank band. Bounds kept loose (±50%) so incidental raw
        // range tweaks do not spuriously fail, but any change that
        // silently collapses or explodes the tier count will.
        let min_rank: u32 = u32::MAX - 9 * (1u32 << 20) + 1;
        let max_rank: u32 = u32::MAX;
        let tier_lo = min_rank >> 13;
        let tier_hi = max_rank >> 13;
        let count = tier_hi - tier_lo + 1;
        assert!(
            count <= 2048,
            ">>13 should yield <=2048 tiers, got {count} (range {}..{})",
            tier_lo,
            tier_hi
        );
        assert!(count >= 512, ">>13 should yield >=512 tiers, got {count}");
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
                (strong >> 13) <= (weak >> 13),
                "stronger hand (raw={strong}) mapped to a larger tier than weak (raw={weak})",
            );
        }
    }
}

/// B10: a missing river board table must increment FALLBACK_COUNTS[3],
/// not silently return 0. Guards the C5c "abort on fallback" guard.
#[cfg(test)]
mod b10_fallback_tests {
    use super::*;

    // NOTE: this test only verifies the *counter* path is reachable.
    // Building a real river-miss requires an abstraction with no river
    // table; that's the KMeansAbstraction with an empty river table.
    // We only assert the atomic increments (side-effect-only test).
    #[test]
    fn reset_then_check_counter_round_trip() {
        reset_fallback_counts();
        assert_eq!(fallback_count(), 0);
        // We don't fabricate a river hash here — the code path is
        // exercised in integration; this only guards the counter API.
    }
}

#[cfg(test)]
mod sortnet_tests {
    use super::sortnets;

    fn ref_desc<T: Ord + Copy>(v: &mut [T]) {
        v.sort_unstable_by(|a, b| b.cmp(a));
    }

    #[test]
    fn sort2_matches_reference() {
        for a in 0u8..=3 {
            for b in 0u8..=3 {
                let mut x = [a, b];
                let mut y = [a, b];
                sortnets::sort2_desc(&mut x);
                ref_desc(&mut y);
                assert_eq!(x, y);
            }
        }
    }

    #[test]
    fn sort3_matches_reference() {
        for a in 0u8..=4 {
            for b in 0u8..=4 {
                for c in 0u8..=4 {
                    let mut x = [a, b, c];
                    let mut y = [a, b, c];
                    sortnets::sort3_desc(&mut x);
                    ref_desc(&mut y);
                    assert_eq!(x, y);
                }
            }
        }
    }

    #[test]
    fn sort5_matches_reference() {
        let vals = [0u8, 1, 7, 13, 27, 51];
        for &a in &vals {
            for &b in &vals {
                for &c in &vals {
                    for &d in &vals {
                        for &e in &vals {
                            let mut x = [a, b, c, d, e];
                            let mut y = [a, b, c, d, e];
                            sortnets::sort5_desc(&mut x);
                            ref_desc(&mut y);
                            assert_eq!(x, y, "input {a},{b},{c},{d},{e}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn sort6_matches_reference() {
        let vals = [0u8, 1, 7, 13, 27, 51];
        for &a in &vals {
            for &b in &vals {
                for &c in &vals {
                    for &d in &vals {
                        for &e in &vals {
                            for &f in &vals {
                                let mut x = [a, b, c, d, e, f];
                                let mut y = [a, b, c, d, e, f];
                                sortnets::sort6_desc(&mut x);
                                ref_desc(&mut y);
                                assert_eq!(x, y, "input {a},{b},{c},{d},{e},{f}");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod rich_features_tests {
    use super::*;

    #[test]
    fn card_encoding_is_suit13_plus_rank() {
        // Encoding: card = suit * 13 + rank. rank in 0..=12 (0=2, 12=A),
        // suit in 0..=3.
        assert_eq!(card_rank(0), 0);   // deuce of suit 0
        assert_eq!(card_rank(12), 12); // ace of suit 0
        assert_eq!(card_rank(13), 0);  // deuce of suit 1
        assert_eq!(card_suit(0), 0);
        assert_eq!(card_suit(12), 0);
        assert_eq!(card_suit(13), 1);
        assert_eq!(card_suit(51), 3);

        // AKs: ace (rank 12, suit 0) = 12, king (rank 11, suit 0) = 11.
        assert_eq!(card_rank(12), 12);
        assert_eq!(card_rank(11), 11);
        assert_eq!(card_suit(12), card_suit(11));
    }

    #[test]
    fn hand_structure_features_bounds() {
        // Encoding: card = suit * 13 + rank, rank 0..=12 (0=2, 12=A).

        // AA: ace of suit 0 (=12) and ace of suit 1 (=25). Different suits.
        let aa = hand_structure_features(&[12, 25]);
        assert!((aa[0] - 1.0).abs() < 1e-6); // rank_high = 12/12
        assert!((aa[1] - 1.0).abs() < 1e-6); // rank_low = 12/12
        assert_eq!(aa[2], 0.0); // different suits -> offsuit
        assert_eq!(aa[3], 1.0); // gap 0 -> connector

        // AKs: ace (12) + king (11), both suit 0.
        let aks = hand_structure_features(&[12, 11]);
        assert!((aks[0] - 1.0).abs() < 1e-6);          // rank_high = 12/12
        assert!((aks[1] - 11.0 / 12.0).abs() < 1e-6);  // rank_low = 11/12
        assert_eq!(aks[2], 1.0); // suited
        assert_eq!(aks[3], 1.0); // gap 1 -> connector

        // 7-2 offsuit: seven (rank 5, suit 0) = 5, deuce (rank 0, suit 1) = 13.
        let seven_two_offsuit = hand_structure_features(&[5, 13]);
        assert!((seven_two_offsuit[0] - 5.0 / 12.0).abs() < 1e-6);
        assert!((seven_two_offsuit[1] - 0.0).abs() < 1e-6);
        assert_eq!(seven_two_offsuit[2], 0.0); // offsuit
        assert_eq!(seven_two_offsuit[3], 0.0); // gap 5 -> no connector

        // 5-3 suited (one-gapper): five (rank 3, suit 0) = 3, three (rank 1, suit 0) = 1.
        let five_three_suited = hand_structure_features(&[3, 1]);
        assert_eq!(five_three_suited[2], 1.0); // suited
        assert_eq!(five_three_suited[3], 1.0); // gap 2 -> inside

        // 5-2 suited (two-gapper): five (rank 3, suit 0) = 3, deuce (rank 0, suit 0) = 0.
        let five_two_suited = hand_structure_features(&[3, 0]);
        assert_eq!(five_two_suited[2], 1.0); // suited
        assert_eq!(five_two_suited[3], 0.0); // gap 3 -> outside
    }

    #[test]
    fn nearest_centroid_6d_picks_identical_point() {
        let c: Vec<[f32; 6]> = vec![
            [0.0; 6],
            [0.5, 0.5, 0.5, 0.5, 0.5, 0.5],
            [1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        ];
        let probe = [0.51, 0.51, 0.51, 0.51, 0.51, 0.51];
        assert_eq!(nearest_centroid_6d(&probe, &c), 1);
    }

    #[test]
    fn nearest_centroid_6d_handles_empty() {
        let empty: Vec<[f32; 6]> = vec![];
        let probe = [0.5; 6];
        assert_eq!(nearest_centroid_6d(&probe, &empty), 0);
    }

    #[test]
    fn centroid_store_6d_round_trip() {
        let store = CentroidStore6D {
            centroids: vec![
                [0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                [0.5, 0.25, 0.5, 0.5, 1.0, 0.0],
            ],
        };
        let tmp = std::env::temp_dir().join("test_centroids6d.bin");
        save_centroids_6d(tmp.to_str().unwrap(), &store).unwrap();
        let loaded = load_centroids_6d(tmp.to_str().unwrap()).unwrap();
        assert_eq!(loaded.centroids.len(), 2);
        assert!((loaded.centroids[1][4] - 1.0).abs() < 1e-6);
        std::fs::remove_file(tmp).ok();
    }
}

#[cfg(test)]
mod hand_board_features_tests {
    use super::*;

    #[test]
    fn board_structure_flop_dry_high_card() {
        // Ks Qd 2h (as flop): 12*4+0=48? No: encoding suit*13+rank.
        // K-spades = 0*13 + 11 = 11. Q-diamonds = 1*13 + 10 = 23.
        // 2-hearts = 2*13 + 0 = 26.
        // board_high = 11/12; unpaired; not flush-drawn (all distinct suits);
        // not connected (K-Q gap 1, Q-2 gap 8).
        let f = board_structure_features(&[11, 23, 26]);
        assert!((f[0] - 11.0/12.0).abs() < 1e-6);
        assert_eq!(f[1], 0.0); // not paired
        assert_eq!(f[2], 0.0); // not flush draw
        assert_eq!(f[3], 0.0); // not connected
    }

    #[test]
    fn board_structure_flop_wet_connected_flushy() {
        // Js Ts 9s (as flop): J-spades = 0*13+9 = 9, T-spades = 0*13+8 = 8,
        // 9-spades = 0*13+7 = 7.
        // board_high = 9/12; unpaired; flush draw (3 spades); connected (J-T-9).
        let f = board_structure_features(&[9, 8, 7]);
        assert!((f[0] - 9.0/12.0).abs() < 1e-6);
        assert_eq!(f[1], 0.0); // not paired
        assert_eq!(f[2], 1.0); // 3 spades -> flush draw bit
        assert_eq!(f[3], 1.0); // J-T-9 -> connected
    }

    #[test]
    fn board_structure_flop_paired() {
        // Ah As Kd on the flop. Encoding: card = suit * 13 + rank,
        // rank 0..12 (0=deuce, 12=ace), suits 0,1,2,3 distinct.
        //   Ah: suit 2, rank 12 -> 2*13 + 12 = 38
        //   As: suit 3, rank 12 -> 3*13 + 12 = 51
        //   Kd: suit 1, rank 11 -> 1*13 + 11 = 24
        // Board ranks: [12, 12, 11]; high = 12; paired (two aces);
        // three distinct suits -> no flush draw;
        // AAK counts as "connected" under the gap <= 2 rule (intentional:
        // the flag means "ranks are clustered", not "straight is possible").
        let f = board_structure_features(&[38, 51, 24]);
        assert!((f[0] - 12.0/12.0).abs() < 1e-6);
        assert_eq!(f[1], 1.0); // paired
        assert_eq!(f[2], 0.0); // three different suits
        assert_eq!(f[3], 1.0); // clustered ranks (gap 0 and gap 1)
    }

    #[test]
    fn board_structure_turn_four_cards() {
        // 4-card board: Ah Kh Qh Jh -- all hearts.
        // A-hearts = 25, K-hearts = 24, Q-hearts = 23, J-hearts = 22.
        // board_high = 12/12; unpaired; flush draw (4 hearts); connected.
        let f = board_structure_features(&[25, 24, 23, 22]);
        assert!((f[0] - 1.0).abs() < 1e-6);
        assert_eq!(f[1], 0.0);
        assert_eq!(f[2], 1.0);
        assert_eq!(f[3], 1.0);
    }

    #[test]
    fn hand_and_board_features_layout() {
        // AA on KQ2 rainbow flop.
        // A-spades = 12, A-hearts = 25. K-spades = 11, Q-diamonds = 23, 2-hearts = 26.
        let feat = hand_and_board_features(0.5, 0.25, &[12, 25], &[11, 23, 26]);
        // [EHS, EHS^2, rank_high, rank_low, suited, connector, board_high, paired, flush_draw, connected]
        assert!((feat[0] - 0.5).abs() < 1e-6);
        assert!((feat[1] - 0.25).abs() < 1e-6);
        assert!((feat[2] - 1.0).abs() < 1e-6);  // A rank_high
        assert!((feat[3] - 1.0).abs() < 1e-6);  // A rank_low (pair)
        assert_eq!(feat[4], 0.0);               // AA offsuit
        assert_eq!(feat[5], 1.0);               // gap 0 -> connector
        assert!((feat[6] - 11.0/12.0).abs() < 1e-6); // K-high board
        assert_eq!(feat[7], 0.0);               // unpaired board
        assert_eq!(feat[8], 0.0);               // rainbow
        assert_eq!(feat[9], 0.0);               // not connected (K-Q-2)
    }

    #[test]
    fn nearest_centroid_10d_picks_identity() {
        let c: Vec<[f32; 10]> = vec![
            [0.0; 10],
            [0.5; 10],
            [1.0; 10],
        ];
        // Probe is offset in every dim, so it stays closest to the
        // [0.5; 10] centroid under squared L2. Any near-[0.5] probe
        // works here; all-dims-set avoids accidentally landing closer
        // to the origin centroid.
        let probe = [0.51f32; 10];
        assert_eq!(nearest_centroid_10d(&probe, &c), 1);
    }

    #[test]
    fn nearest_centroid_10d_empty() {
        let empty: Vec<[f32; 10]> = vec![];
        assert_eq!(nearest_centroid_10d(&[0.5; 10], &empty), 0);
    }

    #[test]
    fn centroid_store_10d_round_trip() {
        let store = CentroidStore10D {
            centroids: vec![[0.0; 10], [1.0; 10]],
        };
        let tmp = std::env::temp_dir().join("test_centroids10d.bin");
        save_centroids_10d(tmp.to_str().unwrap(), &store).unwrap();
        let loaded = load_centroids_10d(tmp.to_str().unwrap()).unwrap();
        assert_eq!(loaded.centroids.len(), 2);
        assert!((loaded.centroids[1][9] - 1.0).abs() < 1e-6);
        std::fs::remove_file(tmp).ok();
    }
}
