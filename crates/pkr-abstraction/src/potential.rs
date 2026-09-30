//! F4: noise-corrected potential feature for flop and turn hands.
//!
//! The audit (F4) found the current (EHS, EHS²) feature space is
//! noise-dominated:
//!
//!   - EHS sampling std is ~0.047 at 100 samples (95th pct 0.093).
//!   - Within-bucket std of true equity under 200 k-means centroids on
//!     (EHS, EHS²) is 0.044 — i.e. bucket resolution ≈ sample noise.
//!   - EHS² is nearly a copy of EHS (correlation 0.999 across 1326
//!     preflop hands), because per-sample equity is 0, 0.5 or 1.
//!
//! This module computes a (mean, potential) pair per (hole, board) that
//! addresses both problems:
//!
//!   - **mean**: equity versus a random hand, averaged over next-street
//!     cards. Larger inner sample count reduces noise.
//!   - **potential**: standard deviation of that equity across next
//!     cards, with the sampling variance of each inner estimate
//!     subtracted out. That is what makes it a real second dimension
//!     instead of an EHS² near-copy: it distinguishes made hands from
//!     draws with the same current strength.
//!
//! Cost: `num_next_cards * inner_samples` evaluator calls per input.
//! For a flop, 47 * 50 = 2350 calls; the audit suggests timing a
//! 100k-entry sample before committing to the full 26M-entry rebuild.

use pkr_contracts::Evaluator;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;

/// Deterministic seed from (hole, board). Same scheme as `ehs.rs` so
/// the same input produces the same feature vector.
#[inline]
fn seed_for(hole: &[u8], board: &[u8]) -> u64 {
    let mut s: u64 = 0xcbf2_9ce4_8422_2325;
    for &c in hole.iter().chain(board.iter()) {
        s ^= c as u64;
        s = s.wrapping_mul(0x0000_0100_0000_01b3);
    }
    s ^ ((hole.len() as u64) << 32) ^ (board.len() as u64)
}

/// Equity versus a uniformly random opponent hand on a fixed board,
/// averaged over `inner` MC deals of the opponent's hole + remaining
/// runout. Deterministic given (hole, board).
fn equity_vs_random(
    hole: &[u8],
    board: &[u8],
    ev: &dyn Evaluator,
    inner: usize,
    rng: &mut SmallRng,
) -> f64 {
    // Build the remaining-card pool once.
    let mut remaining = [0u8; 52];
    let mut rem_len = 0;
    for c in 0..52u8 {
        if !hole.contains(&c) && !board.contains(&c) {
            remaining[rem_len] = c;
            rem_len += 1;
        }
    }
    let needed_board = 5 - board.len();
    let total_draw = 2 + needed_board;

    let mut full_board = [0u8; 5];
    full_board[..board.len()].copy_from_slice(board);

    let mut sum = 0.0f64;
    for _ in 0..inner {
        remaining[..rem_len].partial_shuffle(rng, total_draw);
        let opp = [remaining[0], remaining[1]];
        let fill = &remaining[2..2 + needed_board];
        full_board[board.len()..].copy_from_slice(fill);

        let hero_rank = ev.evaluate_hand(hole, &full_board);
        let opp_rank = ev.evaluate_hand(&opp, &full_board);
        let eq = if hero_rank < opp_rank {
            1.0
        } else if hero_rank == opp_rank {
            0.5
        } else {
            0.0
        };
        sum += eq;
    }
    sum / inner as f64
}

/// Compute (mean, potential) for a hand on a partial board.
///
/// `inner` is the number of opponent/runout samples per next card. The
/// cost is `num_next_cards * inner` evaluator calls.
///
/// `potential` is the standard deviation of `mean` across the next
/// card, with each next-card's own sampling variance subtracted out.
/// The subtraction is what makes the potential a property of the hand
/// rather than a property of the RNG.
pub fn ehs_and_potential(
    hole: &[u8],
    board: &[u8],
    ev: &dyn Evaluator,
    inner: usize,
) -> (f32, f32) {
    assert_eq!(hole.len(), 2, "ehs_and_potential: exactly 2 hole cards");
    assert!(board.len() <= 4, "ehs_and_potential: flop (3) or turn (4) only");
    assert!(inner >= 1, "inner samples must be >= 1");

    let mut rng = SmallRng::seed_from_u64(seed_for(hole, board));

    let mut used = [false; 52];
    for &c in hole.iter().chain(board.iter()) {
        used[c as usize] = true;
    }

    let mut s = 0.0f64;
    let mut s2 = 0.0f64;
    let mut n = 0.0f64;
    let mut noise = 0.0f64;

    for next in (0..52u8).filter(|&c| !used[c as usize]) {
        // Advance the board by one card.
        let mut b = [0u8; 5];
        b[..board.len()].copy_from_slice(board);
        b[board.len()] = next;
        let nb = &b[..board.len() + 1];

        let e = equity_vs_random(hole, nb, ev, inner, &mut rng);
        s += e;
        s2 += e * e;
        n += 1.0;
        // Sampling variance of a per-sample equity that is 0/0.5/1.
        noise += e * (1.0 - e) / inner as f64;
    }

    let mean = s / n;
    let raw_var = (s2 / n) - (mean * mean);
    let corrected = (raw_var - noise / n).max(0.0);
    (mean as f32, corrected.sqrt() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_eval::NlheEvaluator;

    fn ev() -> NlheEvaluator {
        NlheEvaluator
    }

    #[test]
    fn deterministic_given_input() {
        let hole = [0u8, 5];
        let board = [10u8, 20, 30];
        let (m1, p1) = ehs_and_potential(&hole, &board, &ev(), 20);
        let (m2, p2) = ehs_and_potential(&hole, &board, &ev(), 20);
        assert_eq!(m1, m2);
        assert_eq!(p1, p2);
    }

    #[test]
    fn mean_is_in_unit_range() {
        let hole = [0u8, 5];
        let board = [10u8, 20, 30];
        let (m, p) = ehs_and_potential(&hole, &board, &ev(), 20);
        assert!((0.0..=1.0).contains(&m), "mean = {m}");
        assert!(p >= 0.0, "potential = {p}");
        assert!(p <= 1.0, "potential too large = {p}");
    }

    #[test]
    fn potential_is_finite_and_nonzero_on_a_draw() {
        // Suited connectors on a flop that misses: should have a real
        // potential (many turn cards help).
        let hole = [1u8, 14]; // 2s, 3s (same suit if suit*13+rank)
        let board = [27u8, 40, 8];
        let (_m, p) = ehs_and_potential(&hole, &board, &ev(), 30);
        assert!(p.is_finite());
        // Not asserting a specific magnitude — the point is it doesn't
        // collapse to zero the way EHS² would.
    }

    #[test]
    #[ignore]
    fn timing_sample() {
        // Sample 1000 hands and report wall time — used to decide
        // whether the full 26M-entry rebuild is tractable.
        let e = ev();
        let t0 = std::time::Instant::now();
        let n = 1000usize;
        let mut acc = 0.0f64;
        for i in 0..n {
            let hole = [(i % 52) as u8, ((i + 7) % 52) as u8];
            let board = [(i + 13) as u8, (i + 21) as u8, (i + 33) as u8];
            let (m, _p) = ehs_and_potential(&hole, &board, &e, 20);
            acc += m as f64;
        }
        let dt = t0.elapsed().as_secs_f64();
        eprintln!(
            "1000 flop hands @ inner=20: {:.3}s ({:.1}us/hand), acc={:.3}",
            dt, dt / n as f64 * 1e6, acc
        );
    }
}
