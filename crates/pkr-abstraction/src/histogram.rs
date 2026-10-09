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

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_eval::NlheEvaluator;

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
