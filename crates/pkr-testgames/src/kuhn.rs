//! Kuhn poker CFR harness for measuring DCFR variants.
//!
//! Kuhn poker: 3-card deck (J, Q, K), 2 players, each antes 1 chip.
//! Six information sets per player (3 cards x 2 decision points).
//! Exact Nash value to player 0 is -1/18 ~= -0.0556.

use pkr_cfr::dcfr::{update_regret_full, DiscountMode, MomentumMode};

const N_INFOSETS: usize = 12;
const N_ACTIONS: usize = 2;

#[inline]
fn infoset_index(player: usize, card: u8, decision_point: usize) -> usize {
    (player * 3 + card as usize) * 2 + decision_point
}

/// Bit index in a player's 6-bit pure-strategy mask. Two infosets per card.
#[inline]
fn p_mask_bit(card: u8, dp: usize) -> u32 {
    (card as u32) * 2 + dp as u32
}

/// +stake if P0's card beats P1's, -stake otherwise. No ties (distinct cards).
#[inline]
fn showdown(cards: [u8; 2], stake: f32) -> f32 {
    if cards[0] > cards[1] {
        stake
    } else {
        -stake
    }
}

pub struct KuhnCfr {
    regrets: [[f32; N_ACTIONS]; N_INFOSETS],
    momentums: [[f32; N_ACTIONS]; N_INFOSETS],
    strategy_sum: [[f32; N_ACTIONS]; N_INFOSETS],
    iteration: u32,
    pub mode: DiscountMode,
    pub momentum: MomentumMode,
    pub nan_flag: bool,
}

impl KuhnCfr {
    pub fn new(mode: DiscountMode) -> Self {
        Self::new_full(mode, MomentumMode::On)
    }

    pub fn new_full(mode: DiscountMode, momentum: MomentumMode) -> Self {
        Self {
            regrets: [[0.0; N_ACTIONS]; N_INFOSETS],
            momentums: [[0.0; N_ACTIONS]; N_INFOSETS],
            strategy_sum: [[0.0; N_ACTIONS]; N_INFOSETS],
            iteration: 0,
            mode,
            momentum,
            nan_flag: false,
        }
    }

    pub fn iteration(&self) -> u32 {
        self.iteration
    }

    pub fn max_abs_regret(&self) -> f32 {
        let mut m = 0.0f32;
        for row in self.regrets.iter() {
            for &v in row.iter() {
                let a = v.abs();
                if a > m {
                    m = a;
                }
            }
        }
        m
    }

    fn strategy_at(&self, infoset: usize) -> [f32; N_ACTIONS] {
        let r = &self.regrets[infoset];
        let pos0 = r[0].max(0.0);
        let pos1 = r[1].max(0.0);
        let sum = pos0 + pos1;
        if sum > 0.0 {
            [pos0 / sum, pos1 / sum]
        } else {
            [0.5, 0.5]
        }
    }

    pub fn average_strategy_at(&self, infoset: usize) -> [f32; N_ACTIONS] {
        let s = &self.strategy_sum[infoset];
        let total = s[0] + s[1];
        if total > 0.0 {
            [s[0] / total, s[1] / total]
        } else {
            [0.5, 0.5]
        }
    }

    /// Batched CFR iteration: strategy frozen at start, deltas accumulated
    /// across card pairs, applied once at the end.
    pub fn iterate(&mut self) {
        self.iteration += 1;
        let t = self.iteration;

        let mut strat = [[0.0f32; N_ACTIONS]; N_INFOSETS];
        for i in 0..N_INFOSETS {
            strat[i] = self.strategy_at(i);
        }

        let mut delta_accum = [[0.0f32; N_ACTIONS]; N_INFOSETS];
        for c0 in 0..3u8 {
            for c1 in 0..3u8 {
                if c0 == c1 {
                    continue;
                }
                Self::traverse(
                    &strat,
                    &mut self.strategy_sum,
                    &mut delta_accum,
                    [c0, c1],
                    &[],
                    [1.0, 1.0],
                );
            }
        }

        for i in 0..N_INFOSETS {
            for a in 0..N_ACTIONS {
                let (new_r, new_m) = update_regret_full(
                    self.regrets[i][a],
                    self.momentums[i][a],
                    t,
                    delta_accum[i][a],
                    self.mode,
                    self.momentum,
                );
                self.regrets[i][a] = new_r;
                self.momentums[i][a] = new_m;
            }
        }

        if !self.nan_flag {
            'outer: for row in self.regrets.iter() {
                for &v in row.iter() {
                    if !v.is_finite() {
                        self.nan_flag = true;
                        break 'outer;
                    }
                }
            }
        }
    }

    fn traverse(
        strat: &[[f32; N_ACTIONS]; N_INFOSETS],
        strategy_sum: &mut [[f32; N_ACTIONS]; N_INFOSETS],
        delta_accum: &mut [[f32; N_ACTIONS]; N_INFOSETS],
        cards: [u8; 2],
        history: &[u8],
        reach: [f32; 2],
    ) -> f32 {
        match history.len() {
            0 => {
                let infoset = infoset_index(0, cards[0], 0);
                let s = strat[infoset];
                let v0 = Self::traverse(
                    strat,
                    strategy_sum,
                    delta_accum,
                    cards,
                    &[0],
                    [reach[0] * s[0], reach[1]],
                );
                let v1 = Self::traverse(
                    strat,
                    strategy_sum,
                    delta_accum,
                    cards,
                    &[1],
                    [reach[0] * s[1], reach[1]],
                );
                let v = s[0] * v0 + s[1] * v1;
                let w = reach[1];
                delta_accum[infoset][0] += (v0 - v) * w;
                delta_accum[infoset][1] += (v1 - v) * w;
                for a in 0..N_ACTIONS {
                    strategy_sum[infoset][a] += s[a] * reach[0];
                }
                v
            }
            1 => {
                let dp = if history[0] == 0 { 0 } else { 1 };
                let infoset = infoset_index(1, cards[1], dp);
                let s = strat[infoset];
                let v0 = Self::traverse(
                    strat,
                    strategy_sum,
                    delta_accum,
                    cards,
                    &[history[0], 0],
                    [reach[0], reach[1] * s[0]],
                );
                let v1 = Self::traverse(
                    strat,
                    strategy_sum,
                    delta_accum,
                    cards,
                    &[history[0], 1],
                    [reach[0], reach[1] * s[1]],
                );
                let v = s[0] * v0 + s[1] * v1;
                // P1 minimizes v_to_P0, so regret for action a is v - v_a.
                let w = reach[0];
                delta_accum[infoset][0] += (v - v0) * w;
                delta_accum[infoset][1] += (v - v1) * w;
                for a in 0..N_ACTIONS {
                    strategy_sum[infoset][a] += s[a] * reach[1];
                }
                v
            }
            2 => match (history[0], history[1]) {
                (0, 0) => showdown(cards, 1.0),
                (0, 1) => {
                    let infoset = infoset_index(0, cards[0], 1);
                    let s = strat[infoset];
                    let v_fold: f32 = -1.0;
                    let v_call: f32 = showdown(cards, 2.0);
                    let vs = [v_fold, v_call];
                    let v = s[0] * vs[0] + s[1] * vs[1];
                    let w = reach[1];
                    delta_accum[infoset][0] += (vs[0] - v) * w;
                    delta_accum[infoset][1] += (vs[1] - v) * w;
                    for a in 0..N_ACTIONS {
                        strategy_sum[infoset][a] += s[a] * reach[0];
                    }
                    v
                }
                (1, 0) => 1.0,
                (1, 1) => showdown(cards, 2.0),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    // --- Exploitability via brute-force pure-strategy enumeration ---------
    //
    // Six infosets per player, two actions each => 2^6 = 64 pure
    // strategies. A best response is one of these. Enumerate and pick the
    // best. This is correct by construction; the earlier recursive
    // br_tree_* computed max per card pair, which is not the same as
    // max per infoset, and inflated exploitability by ~0.27.

    /// Value to P0 when P0 commits to a specific pure strategy (6-bit mask)
    /// and P1 plays their average strategy.
    fn value_p0_pure(&self, mask: u32) -> f32 {
        let mut total = 0.0f32;
        for c0 in 0..3u8 {
            for c1 in 0..3u8 {
                if c0 == c1 {
                    continue;
                }
                let a0_dp0 = ((mask >> p_mask_bit(c0, 0)) & 1) as u8;
                let a0_dp1 = ((mask >> p_mask_bit(c0, 1)) & 1) as u8;

                let v = match a0_dp0 {
                    // P0 checks; P1 acts at their dp=0.
                    0 => {
                        let s1 = self.average_strategy_at(infoset_index(1, c1, 0));
                        let v_p1_check = showdown([c0, c1], 1.0);
                        let v_p1_bet = match a0_dp1 {
                            0 => -1.0,
                            1 => showdown([c0, c1], 2.0),
                            _ => unreachable!(),
                        };
                        s1[0] * v_p1_check + s1[1] * v_p1_bet
                    }
                    // P0 bets; P1 acts at their dp=1.
                    1 => {
                        let s1 = self.average_strategy_at(infoset_index(1, c1, 1));
                        let v_fold: f32 = 1.0;
                        let v_call = showdown([c0, c1], 2.0);
                        s1[0] * v_fold + s1[1] * v_call
                    }
                    _ => unreachable!(),
                };
                total += v / 6.0;
            }
        }
        total
    }

    /// Value to P0 when P1 commits to a specific pure strategy (6-bit mask)
    /// and P0 plays their average strategy. P1 minimizes this.
    fn value_p1_pure(&self, mask: u32) -> f32 {
        let mut total = 0.0f32;
        for c0 in 0..3u8 {
            for c1 in 0..3u8 {
                if c0 == c1 {
                    continue;
                }
                let a1_dp0 = ((mask >> p_mask_bit(c1, 0)) & 1) as u8;
                let a1_dp1 = ((mask >> p_mask_bit(c1, 1)) & 1) as u8;

                let s0_dp0 = self.average_strategy_at(infoset_index(0, c0, 0));
                let s0_dp1 = self.average_strategy_at(infoset_index(0, c0, 1));

                // P0 checks branch:
                let v_if_p0_check = match a1_dp0 {
                    0 => showdown([c0, c1], 1.0),
                    1 => -s0_dp1[0] + s0_dp1[1] * showdown([c0, c1], 2.0),
                    _ => unreachable!(),
                };
                // P0 bets branch:
                let v_if_p0_bet = match a1_dp1 {
                    0 => 1.0,
                    1 => showdown([c0, c1], 2.0),
                    _ => unreachable!(),
                };
                let v = s0_dp0[0] * v_if_p0_check + s0_dp0[1] * v_if_p0_bet;
                total += v / 6.0;
            }
        }
        total
    }

    /// P0's best-response value against P1's average strategy.
    pub fn br_value_p0_legacy(&self) -> f32 {
        let mut best = f32::NEG_INFINITY;
        for mask in 0..64u32 {
            let v = self.value_p0_pure(mask);
            if v > best {
                best = v;
            }
        }
        best
    }

    /// Value to P0 when P1 best-responds against P0's average strategy.
    /// P1 minimizes, so this is a lower bound on the true value.
    pub fn br_value_p0_given_br1_legacy(&self) -> f32 {
        let mut best = f32::INFINITY;
        for mask in 0..64u32 {
            let v = self.value_p1_pure(mask);
            if v < best {
                best = v;
            }
        }
        best
    }

    pub fn value_of_avg(&self) -> f32 {
        let mut total = 0.0f32;
        for c0 in 0..3u8 {
            for c1 in 0..3u8 {
                if c0 == c1 {
                    continue;
                }
                total += self.avg_value_tree([c0, c1], &[]) / 6.0;
            }
        }
        total
    }

    fn avg_value_tree(&self, cards: [u8; 2], history: &[u8]) -> f32 {
        match history.len() {
            0 => {
                let s = self.average_strategy_at(infoset_index(0, cards[0], 0));
                let v0 = self.avg_value_tree(cards, &[0]);
                let v1 = self.avg_value_tree(cards, &[1]);
                s[0] * v0 + s[1] * v1
            }
            1 => {
                let dp = if history[0] == 0 { 0 } else { 1 };
                let s = self.average_strategy_at(infoset_index(1, cards[1], dp));
                let v0 = self.avg_value_tree(cards, &[history[0], 0]);
                let v1 = self.avg_value_tree(cards, &[history[0], 1]);
                s[0] * v0 + s[1] * v1
            }
            2 => match (history[0], history[1]) {
                (0, 0) => showdown(cards, 1.0),
                (0, 1) => {
                    let s = self.average_strategy_at(infoset_index(0, cards[0], 1));
                    -s[0] + s[1] * showdown(cards, 2.0)
                }
                (1, 0) => 1.0,
                (1, 1) => showdown(cards, 2.0),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    /// Tree walk with pluggable strategy sources. `s0` is looked up at
    /// P0's decision nodes, `s1` at P1's. Both return [prob_action0, prob_action1].
    /// Returns the value to P0 at `history`.
    fn walk_with_strats(
        &self,
        c0: u8,
        c1: u8,
        s0: &dyn Fn(usize) -> [f32; 2],
        s1: &dyn Fn(usize) -> [f32; 2],
        history: &[u8],
    ) -> f32 {
        match history.len() {
            0 => {
                let infoset = infoset_index(0, c0, 0);
                let p = s0(infoset);
                let v0 = self.walk_with_strats(c0, c1, s0, s1, &[0]);
                let v1 = self.walk_with_strats(c0, c1, s0, s1, &[1]);
                p[0] * v0 + p[1] * v1
            }
            1 => {
                let dp = if history[0] == 0 { 0 } else { 1 };
                let infoset = infoset_index(1, c1, dp);
                let p = s1(infoset);
                let v0 = self.walk_with_strats(c0, c1, s0, s1, &[history[0], 0]);
                let v1 = self.walk_with_strats(c0, c1, s0, s1, &[history[0], 1]);
                p[0] * v0 + p[1] * v1
            }
            2 => match (history[0], history[1]) {
                (0, 0) => {
                    if c0 > c1 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                (0, 1) => {
                    let infoset = infoset_index(0, c0, 1);
                    let p = s0(infoset);
                    let v_fold: f32 = -1.0;
                    let v_call: f32 = if c0 > c1 { 2.0 } else { -2.0 };
                    p[0] * v_fold + p[1] * v_call
                }
                (1, 0) => 1.0,
                (1, 1) => {
                    if c0 > c1 {
                        2.0
                    } else {
                        -2.0
                    }
                }
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    /// Value to P0 under the given per-seat strategies, averaged over the
    /// 6 equiprobable deals.
    fn value_over_deals(
        &self,
        s0: &dyn Fn(usize) -> [f32; 2],
        s1: &dyn Fn(usize) -> [f32; 2],
    ) -> f32 {
        let mut total = 0.0f32;
        for c0 in 0..3u8 {
            for c1 in 0..3u8 {
                if c0 == c1 {
                    continue;
                }
                total += self.walk_with_strats(c0, c1, s0, s1, &[]) / 6.0;
            }
        }
        total
    }

    /// Value to P0 when P0 best-responds against the average strategy of P1.
    /// Enumerates all 2^6 = 64 pure strategies of P0; per-infoset choice is
    /// thus consistent across all histories in the infoset (the crucial
    /// property the previous per-card-pair version lacked).
    pub fn br_value_p0(&self) -> f32 {
        let avg_s1 = |infoset: usize| self.average_strategy_at(infoset);
        let mut best = f32::NEG_INFINITY;
        for mask in 0u8..64 {
            let s0_pure = move |infoset: usize| -> [f32; 2] {
                let bit = (infoset % 6) as u8;
                if (mask >> bit) & 1 == 0 {
                    [1.0, 0.0]
                } else {
                    [0.0, 1.0]
                }
            };
            let v = self.value_over_deals(&s0_pure, &avg_s1);
            if v > best {
                best = v;
            }
        }
        best
    }

    /// Value to P0 when P1 best-responds against the average strategy of P0.
    /// P1 minimizes; the value reported is still to P0.
    pub fn br_value_p0_given_br1(&self) -> f32 {
        let avg_s0 = |infoset: usize| self.average_strategy_at(infoset);
        let mut best = f32::INFINITY;
        for mask in 0u8..64 {
            let s1_pure = move |infoset: usize| -> [f32; 2] {
                let bit = (infoset % 6) as u8;
                if (mask >> bit) & 1 == 0 {
                    [1.0, 0.0]
                } else {
                    [0.0, 1.0]
                }
            };
            let v = self.value_over_deals(&avg_s0, &s1_pure);
            if v < best {
                best = v;
            }
        }
        best
    }

    pub fn exploitability(&self) -> f32 {
        let br0 = self.br_value_p0();
        let br1_p0 = self.br_value_p0_given_br1();
        (br0 - br1_p0) / 2.0
    }
}

#[cfg(test)]
mod t03_tests {
    use super::*;

    /// Uniform strategy: exploitability is a fixed positive number,
    /// approximately (BR0 - BR1_to_p0)/2 for uniform play.
    #[test]
    fn uniform_strategy_is_exploitable() {
        let k = KuhnCfr::new(pkr_cfr::dcfr::DiscountMode::None);
        let e = k.exploitability();
        assert!(
            e > 0.05,
            "uniform exploitability should be positive, got {e}"
        );
        assert!(e < 0.5, "uniform exploitability should be bounded, got {e}");
    }

    /// After convergence, exploitability should be strictly lower than
    /// uniform. Convergence here = 50k iterations of vanilla CFR on Kuhn,
    /// which is trivial (~50k * 18 infosets ops).
    #[test]
    fn cfr_converges_on_kuhn() {
        let mut k = KuhnCfr::new(pkr_cfr::dcfr::DiscountMode::None);
        let e0 = k.exploitability();
        for _ in 0..50_000 {
            k.iterate();
        }
        let e1 = k.exploitability();
        assert!(
            e1 < e0 * 0.5,
            "vanilla CFR on Kuhn should halve exploitability in 50k iters: {e0} -> {e1}"
        );
    }

    /// P0's BR value against uniform-P1 must be >= P0's value under uniform.
    #[test]
    fn br_dominates_own_strategy() {
        let k = KuhnCfr::new(pkr_cfr::dcfr::DiscountMode::None);
        let avg_val = k.value_of_avg();
        let br = k.br_value_p0();
        assert!(
            br >= avg_val - 1e-4,
            "BR ({br}) should dominate avg ({avg_val})"
        );
    }
}

#[cfg(test)]
mod f5_grid_tests {
    //! F5 grid: run the Kuhn CFR harness under a few combinations of
    //! the update-rule flags and report the convergence.
    //!
    //! The full 24-config grid is documented in
    //! `docs/experiments/f5-grid.md`. This test runs the two configs
    //! that matter most:
    //!
    //!   baseline: neg_floor=true, momentum off, avg_power=2
    //!   variant:  neg_floor=false, momentum off, avg_power=2
    //!
    //! and prints the exploitability at 1e5 and 1e6 iterations for each.
    //!
    //! Marked `#[ignore]` because it takes ~30s (Kuhn is fast but the
    //! full iteration count is high). Run manually:
    //!
    //!   cargo test --release -p pkr-testgames --lib \
    //!       f5_grid_tests -- --ignored --nocapture

    use super::*;

    fn run_kuhn(iters: u32) -> (f32, u32) {
        let mut cfr = KuhnCfr::new_full(DiscountMode::PRODUCTION, MomentumMode::Off);
        for _ in 0..iters {
            cfr.iterate();
        }
        (cfr.exploitability(), iters)
    }

    #[test]
    #[ignore]
    fn kuhn_floor_grid() {
        // Reads whatever `TrainConfig` resolved at process start.
        // To A/B the two floor modes, run this test twice:
        //
        //   cargo test --release -p pkr-testgames --lib \
        //       kuhn_floor_grid -- --ignored --nocapture
        //   PKR_RM_PLUS=0 cargo test --release -p pkr-testgames --lib \
        //       kuhn_floor_grid -- --ignored --nocapture
        //
        // `TrainConfig` is a per-process singleton, so a single run
        // cannot test both. The test prints whichever mode it saw so
        // the output is unambiguous.
        let mode = if std::env::var("PKR_RM_PLUS").as_deref() == Ok("0") {
            "neg_floor=false (DCFR beta)"
        } else {
            "neg_floor=true (RM+)"
        };
        let (e5, _) = run_kuhn(100_000);
        let (e6, _) = run_kuhn(1_000_000);

        println!();
        println!("=== F5 Kuhn grid: {mode} ===");
        println!("    1e5 iters: expl = {:.6}", e5);
        println!("    1e6 iters: expl = {:.6}", e6);

        // Both modes should converge. Kuhn's exact Nash is
        // -1/18, exploitability of the converged strategy tends to 0.
        assert!(
            e6 < 1e-2,
            "Kuhn should converge below 1e-2, got {e6}"
        );
    }
}
