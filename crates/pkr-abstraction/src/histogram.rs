//! Distribution-aware river/turn features (Task 2.3b).
//!
//! Replaces the single (EHS, EHS^2) pair with an EQUITY-DISTRIBUTION
//! histogram: for a river board the feature is the exact equity vs a
//! uniform random hand; for a turn board it is the histogram of river
//! equities over the 46 remaining river cards. Clustering these
//! histograms (e.g. k-means on CDFs) is the standard card-abstraction
//! approach.
//!
//! Card = suit*13 + rank; evaluator LOWER u32 = STRONGER.

use pkr_contracts::Evaluator;

/// Exact equity (0..1) of `hole` vs one uniformly random opponent hand on
/// the COMPLETE 5-card `board5`. Ties count 1/2. Exact enumeration.
pub fn river_equity(hole: &[u8; 2], board5: &[u8], ev: &dyn Evaluator) -> f32 {
    debug_assert_eq!(board5.len(), 5, "river_equity needs a 5-card board");
    let mine = ev.evaluate_hand(hole, board5);
    let (mut w, mut t, mut n) = (0u32, 0u32, 0u32);
    for a in 0..52u8 {
        if hole.contains(&a) || board5.contains(&a) {
            continue;
        }
        for b in (a + 1)..52u8 {
            if hole.contains(&b) || board5.contains(&b) {
                continue;
            }
            let o = ev.evaluate_hand(&[a, b], board5);
            n += 1;
            if mine < o {
                w += 1;
            } else if mine == o {
                t += 1;
            }
        }
    }
    (w as f32 + 0.5 * t as f32) / n as f32
}

/// Histogram (len `bins`) of river equities over the 46 river cards
/// completing a turn `board4`. Each bin is a fraction in [0,1].
pub fn turn_hist(hole: &[u8; 2], board4: &[u8], ev: &dyn Evaluator, bins: usize) -> Vec<f32> {
    debug_assert_eq!(board4.len(), 4, "turn_hist needs a 4-card board");
    let mut h = vec![0f32; bins];
    let mut n = 0f32;
    for r in 0..52u8 {
        if hole.contains(&r) || board4.contains(&r) {
            continue;
        }
        let mut b5 = [0u8; 5];
        b5[..4].copy_from_slice(board4);
        b5[4] = r;
        let e = river_equity(hole, &b5, ev);
        let idx = ((e * bins as f32) as usize).min(bins - 1);
        h[idx] += 1.0;
        n += 1.0;
    }
    for x in &mut h {
        *x /= n;
    }
    h
}

/// Flop feature: histogram (len `bins`) over the 47 turn cards, where
/// each turn's equity is estimated from `samples_per_turn` random rivers
/// (the full 46-river enumeration per turn is too costly at the flop).
/// Deterministic given `seed`. Each bin is a fraction in [0,1].
pub fn flop_hist(
    hole: &[u8; 2],
    board3: &[u8],
    ev: &dyn Evaluator,
    bins: usize,
    samples_per_turn: usize,
    seed: u64,
) -> Vec<f32> {
    debug_assert_eq!(board3.len(), 3, "flop_hist needs a 3-card board");
    let mut h = vec![0f32; bins];
    let mut n = 0f32;
    let mut rng = seed | 1; // nonzero LCG state
    let mut next = || {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (rng >> 33) as u32
    };
    for turn in 0..52u8 {
        if hole.contains(&turn) || board3.contains(&turn) {
            continue;
        }
        let mut b4 = [0u8; 4];
        b4[..3].copy_from_slice(board3);
        b4[3] = turn;
        // Estimate this turn's equity by sampling rivers.
        let mut eq = 0f32;
        let mut used = 0u32;
        let mut tries = 0u32;
        while used < samples_per_turn as u32 && tries < 1000 {
            tries += 1;
            let r = (next() % 52) as u8;
            if hole.contains(&r) || b4.contains(&r) {
                continue;
            }
            let mut b5 = [0u8; 5];
            b5[..4].copy_from_slice(&b4);
            b5[4] = r;
            eq += river_equity(hole, &b5, ev);
            used += 1;
        }
        if used > 0 {
            eq /= used as f32;
            let idx = ((eq * bins as f32) as usize).min(bins - 1);
            h[idx] += 1.0;
            n += 1.0;
        }
    }
    for x in &mut h {
        *x /= n;
    }
    h
}

/// k-means over CDF-transformed histograms (Task 2.3c). Each input
/// histogram is converted to its CDF, then clustered with L2 k-means.
/// Returns (centroids-as-CDFs, assignment per input). Deterministic:
/// init = first k distinct points (no RNG), 20 iterations max.
///
/// `emd_l1(a, centroid_cdf)` is the natural distance, but k-means on
/// CDFs with L2 is the standard cheap surrogate; we report the final
/// within-cluster mean L1 (EMD) so the caller can judge quality.
pub fn kmeans_cdf(data: &[Vec<f32>], k: usize, iters: usize) -> (Vec<Vec<f32>>, Vec<usize>) {
    assert!(!data.is_empty(), "kmeans_cdf: empty data");
    let dim = data[0].len();
    let cdfs: Vec<Vec<f32>> = data.iter().map(|h| cdf(h)).collect();
    let k = k.min(cdfs.len()).max(1);
    // Deterministic init: spread picks (i * n / k).
    let mut cents: Vec<Vec<f32>> = (0..k).map(|i| cdfs[i * cdfs.len() / k].clone()).collect();
    let mut assign = vec![0usize; cdfs.len()];
    for _ in 0..iters.max(1) {
        // assign
        for (i, p) in cdfs.iter().enumerate() {
            let mut best = 0usize;
            let mut bd = f32::INFINITY;
            for (ci, c) in cents.iter().enumerate() {
                let d: f32 = p.iter().zip(c).map(|(a, b)| (a - b) * (a - b)).sum();
                if d < bd { bd = d; best = ci; }
            }
            assign[i] = best;
        }
        // update
        let mut sums = vec![vec![0f32; dim]; k];
        let mut counts = vec![0usize; k];
        for (i, p) in cdfs.iter().enumerate() {
            counts[assign[i]] += 1;
            for d in 0..dim { sums[assign[i]][d] += p[d]; }
        }
        for ci in 0..k {
            if counts[ci] > 0 {
                for d in 0..dim { cents[ci][d] = sums[ci][d] / counts[ci] as f32; }
            }
        }
    }
    (cents, assign)
}

/// Mean within-cluster EMD (L1 of CDFs) of a clustering — the quality
/// metric Task 2.3(c) compares against the legacy buckets.
pub fn mean_within_cluster_emd(data: &[Vec<f32>], cents: &[Vec<f32>], assign: &[usize]) -> f32 {
    if data.is_empty() { return 0.0; }
    let mut tot = 0f32;
    for (i, h) in data.iter().enumerate() {
        tot += emd_l1(h, &cents[assign[i]]);
    }
    tot / data.len() as f32
}

/// Cumulative distribution of a histogram (each bin = fraction).
pub fn cdf(h: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(h.len());
    let mut acc = 0.0f32;
    for &x in h {
        acc += x;
        out.push(acc);
    }
    out
}

/// Earth-mover's distance between two 1-D histograms. For 1-D
/// distributions EMD == L1 distance of the CDFs (Task 2.3c).
pub fn emd_l1(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len(), "emd_l1: histogram lengths differ");
    let (ca, cb) = (cdf(a), cdf(b));
    ca.iter().zip(cb.iter()).map(|(x, y)| (x - y).abs()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_eval::NlheEvaluator;

    #[test]
    fn flop_hist_sums_to_one_and_deterministic() {
        let ev = NlheEvaluator;
        let board3 = [12u8, 24, 36];
        let hole = [0u8, 5];
        let a = flop_hist(&hole, &board3, &ev, 8, 4, 42);
        let b = flop_hist(&hole, &board3, &ev, 8, 4, 42);
        assert_eq!(a, b, "flop_hist must be deterministic for a fixed seed");
        let sum: f32 = a.iter().sum();
        assert!((sum - 1.0).abs() < 1e-3, "must sum to 1, got {sum}");
        assert!(a.iter().any(|&x| x > 0.0));
    }

    #[test]
    fn kmeans_cdf_separates_two_clusters() {
        // Cluster A: mass in low bins. Cluster B: mass in high bins.
        let mut data = Vec::new();
        for _ in 0..10 { data.push(vec![1.0, 0.0, 0.0, 0.0, 0.0]); }
        for _ in 0..10 { data.push(vec![0.0, 0.0, 0.0, 0.0, 1.0]); }
        let (cents, assign) = kmeans_cdf(&data, 2, 20);
        assert_eq!(cents.len(), 2);
        // All A's share one label, all B's the other.
        let a = assign[0];
        assert!(assign[..10].iter().all(|&x| x == a), "A not homogeneous");
        assert!(assign[10..].iter().all(|&x| x != a), "B not separated from A");
    }

    #[test]
    fn kmeans_cdf_quality_improves_or_is_finite() {
        let mut data = Vec::new();
        let mut rng = 12345u64;
        let mut next = || { rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1); (rng >> 40) as f32 / (1u64 << 24) as f32 };
        for _ in 0..50 { data.push(vec![next(), next(), next(), next()]); }
        let (cents, assign) = kmeans_cdf(&data, 4, 20);
        let q = mean_within_cluster_emd(&data, &cents, &assign);
        assert!(q.is_finite() && q >= 0.0, "quality {q}");
    }

    #[test]
    fn emd_zero_for_identical() {
        let h = [0.1, 0.2, 0.4, 0.3];
        assert!(emd_l1(&h, &h).abs() < 1e-6);
    }

    #[test]
    fn emd_positive_and_symmetric_for_shifted() {
        let a = [1.0, 0.0, 0.0, 0.0];
        let b = [0.0, 0.0, 0.0, 1.0];
        let d = emd_l1(&a, &b);
        assert!(d > 0.0);
        assert!((d - emd_l1(&b, &a)).abs() < 1e-6, "EMD must be symmetric");
    }

    #[test]
    fn cdf_is_monotone_and_ends_at_one() {
        let h = [0.25, 0.25, 0.25, 0.25];
        let c = cdf(&h);
        assert!((c.last().unwrap() - 1.0).abs() < 1e-6);
        for w in c.windows(2) {
            assert!(w[1] >= w[0] - 1e-6, "CDF must be monotone");
        }
    }

    #[test]
    fn river_equity_extremes() {
        let ev = NlheEvaluator;
        // Rainbow-ish board. Find the actual nuts and worst hole by brute
        // force (don't hand-guess card indices).
        let board = [12u8, 24, 36, 48, 1];
        let mut best: (u32, [u8; 2]) = (u32::MAX, [0, 0]);
        let mut worst: (u32, [u8; 2]) = (0, [0, 0]);
        for a in 0..52u8 {
            if board.contains(&a) { continue; }
            for b in (a + 1)..52u8 {
                if board.contains(&b) { continue; }
                let r = ev.evaluate_hand(&[a, b], &board);
                if r < best.0 { best = (r, [a, b]); }
                if r > worst.0 { worst = (r, [a, b]); }
            }
        }
        let e_nuts = river_equity(&best.1, &board, &ev);
        let e_worst = river_equity(&worst.1, &board, &ev);
        assert!(e_nuts > 0.8, "nuts {:?} equity {e_nuts} should be high", best.1);
        assert!(e_worst < 0.2, "worst {:?} equity {e_worst} should be low", worst.1);
        assert!(e_nuts > e_worst);
    }

    #[test]
    fn turn_hist_sums_to_one_and_has_46_support() {
        let ev = NlheEvaluator;
        let board4 = [12u8, 25, 38, 1];
        let hole = [0u8, 5];
        let h = turn_hist(&hole, &board4, &ev, 10);
        let sum: f32 = h.iter().sum();
        assert!((sum - 1.0).abs() < 1e-3, "histogram must sum to 1, got {sum}");
        assert_eq!(h.len(), 10);
        // 46 river cards -> some bin gets mass; not all zero.
        assert!(h.iter().any(|&x| x > 0.0));
    }

    #[test]
    fn river_equity_matches_bruteforce_sum() {
        // Property: summing (win + 0.5 tie) over all opp hands equals
        // n * equity.
        let ev = NlheEvaluator;
        let board = [12u8, 25, 38, 51, 1];
        let hole = [8u8, 0];
        let e = river_equity(&hole, &board, &ev);
        let mine = ev.evaluate_hand(&hole, &board);
        let (mut w, mut t, mut n) = (0u32, 0u32, 0u32);
        for a in 0..52u8 {
            if hole.contains(&a) || board.contains(&a) { continue; }
            for b in (a + 1)..52u8 {
                if hole.contains(&b) || board.contains(&b) { continue; }
                let o = ev.evaluate_hand(&[a, b], &board);
                n += 1;
                if mine < o { w += 1 } else if mine == o { t += 1 }
            }
        }
        let want = (w as f32 + 0.5 * t as f32) / n as f32;
        assert!((e - want).abs() < 1e-6);
    }
}
