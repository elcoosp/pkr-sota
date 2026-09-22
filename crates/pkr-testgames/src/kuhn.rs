//! Kuhn poker CFR harness for measuring DCFR variants.
//!
//! Kuhn poker: 3-card deck (J, Q, K), 2 players, each antes 1 chip.
//! Six information sets per player (3 cards x 2 decision points).
//! Exact Nash value to player 0 is -1/18 ~= -0.0556.
//!
//! Tree:
//!   []         P0: check (0) | bet (1)
//!   [0]        P1: check (0) | bet (1)
//!   [1]        P1: fold (0)  | call (1)
//!   [0,0]      showdown, stakes 1 each
//!   [0,1]      P0: fold (0)  | call (1)
//!   [0,1,0]    P0 folds, P0 -1
//!   [0,1,1]    showdown, stakes 2 each
//!   [1,0]      P1 folds, P1 -1 -> P0 +1
//!   [1,1]      showdown, stakes 2 each

use pkr_cfr::dcfr::{update_regret_full, DiscountMode, MomentumMode};

const N_INFOSETS: usize = 12;
const N_ACTIONS: usize = 2;

#[inline]
fn infoset_index(player: usize, card: u8, decision_point: usize) -> usize {
    (player * 3 + card as usize) * 2 + decision_point
}

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
    /// Set if any regret ever becomes non-finite; indicates numerical
    /// blow-up of the discount formula. Reported by the experiment.
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

    /// Largest |regret| across all infosets and actions. Used by the
    /// experiment to reveal multiplicative blow-up before it becomes NaN.
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

    /// One full CFR iteration with batched regret updates.
    ///
    /// Critically: the strategy is snapshotted at the start of the
    /// iteration and used unchanged throughout the six card-pair
    /// traversals. Regret deltas accumulate into a local buffer and are
    /// applied once at the end. This is textbook CFR. The prior
    /// implementation updated regrets in place during the traversal,
    /// which made the second card pair use a partially-updated strategy
    /// -- not CFR, and empirically non-convergent on Kuhn.
    pub fn iterate(&mut self) {
        self.iteration += 1;
        let t = self.iteration;

        // 1. Snapshot regret-matching strategy for every infoset.
        let mut strat = [[0.0f32; N_ACTIONS]; N_INFOSETS];
        for i in 0..N_INFOSETS {
            strat[i] = self.strategy_at(i);
        }

        // 2. Accumulate deltas and strategy_sum across all card pairs.
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

        // 3. Apply accumulated deltas once.
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

        // 4. NaN check once per iteration.
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

    /// Traverse the tree for a fixed card pair. Reads `strat` (frozen),
    /// writes `strategy_sum` and `delta_accum`. Returns value-to-P0 at
    /// this history.
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
                    strat, strategy_sum, delta_accum, cards, &[0],
                    [reach[0] * s[0], reach[1]],
                );
                let v1 = Self::traverse(
                    strat, strategy_sum, delta_accum, cards, &[1],
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
                    strat, strategy_sum, delta_accum, cards, &[history[0], 0],
                    [reach[0], reach[1] * s[0]],
                );
                let v1 = Self::traverse(
                    strat, strategy_sum, delta_accum, cards, &[history[0], 1],
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

    // --- Exploitability --------------------------------------------------

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
                    s[0] * -1.0 + s[1] * showdown(cards, 2.0)
                }
                (1, 0) => 1.0,
                (1, 1) => showdown(cards, 2.0),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    pub fn br_value_p0(&self) -> f32 {
        let mut total = 0.0f32;
        for c0 in 0..3u8 {
            for c1 in 0..3u8 {
                if c0 == c1 {
                    continue;
                }
                total += self.br_tree_p0([c0, c1], &[]) / 6.0;
            }
        }
        total
    }

    fn br_tree_p0(&self, cards: [u8; 2], history: &[u8]) -> f32 {
        match history.len() {
            0 => {
                let v0 = self.br_tree_p0(cards, &[0]);
                let v1 = self.br_tree_p0(cards, &[1]);
                v0.max(v1)
            }
            1 => {
                let dp = if history[0] == 0 { 0 } else { 1 };
                let s1 = self.average_strategy_at(infoset_index(1, cards[1], dp));
                let v0 = self.br_tree_p0(cards, &[history[0], 0]);
                let v1 = self.br_tree_p0(cards, &[history[0], 1]);
                s1[0] * v0 + s1[1] * v1
            }
            2 => match (history[0], history[1]) {
                (0, 0) => showdown(cards, 1.0),
                (0, 1) => {
                    let v_fold: f32 = -1.0;
                    let v_call: f32 = showdown(cards, 2.0);
                    v_fold.max(v_call)
                }
                (1, 0) => 1.0,
                (1, 1) => showdown(cards, 2.0),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    pub fn br_value_p0_given_br1(&self) -> f32 {
        let mut total = 0.0f32;
        for c0 in 0..3u8 {
            for c1 in 0..3u8 {
                if c0 == c1 {
                    continue;
                }
                total += self.br_tree_p1([c0, c1], &[]) / 6.0;
            }
        }
        total
    }

    fn br_tree_p1(&self, cards: [u8; 2], history: &[u8]) -> f32 {
        match history.len() {
            0 => {
                let s0 = self.average_strategy_at(infoset_index(0, cards[0], 0));
                let v0 = self.br_tree_p1(cards, &[0]);
                let v1 = self.br_tree_p1(cards, &[1]);
                s0[0] * v0 + s0[1] * v1
            }
            1 => {
                let v0 = self.br_tree_p1(cards, &[history[0], 0]);
                let v1 = self.br_tree_p1(cards, &[history[0], 1]);
                v0.min(v1)
            }
            2 => match (history[0], history[1]) {
                (0, 0) => showdown(cards, 1.0),
                (0, 1) => {
                    let s0 = self.average_strategy_at(infoset_index(0, cards[0], 1));
                    s0[0] * -1.0 + s0[1] * showdown(cards, 2.0)
                }
                (1, 0) => 1.0,
                (1, 1) => showdown(cards, 2.0),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    pub fn exploitability(&self) -> f32 {
        let br0 = self.br_value_p0();
        let br1_p0 = self.br_value_p0_given_br1();
        (br0 - br1_p0) / 2.0
    }
}
