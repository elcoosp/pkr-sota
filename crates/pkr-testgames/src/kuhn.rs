//! Kuhn poker CFR harness for measuring DCFR variants.
//!
//! Kuhn poker: 3-card deck (J, Q, K), 2 players, each antes 1 chip.
//! Six information sets per player (3 cards × 2 decision points).
//! Exact Nash value to player 0 is -1/18 ≈ -0.0556.
//!
//! The tree:
//!   []         P0: check (0) | bet (1)
//!   [0]        P1: check (0) | bet (1)
//!   [1]        P1: fold (0)  | call (1)
//!   [0,0]      showdown, stakes 1 each
//!   [0,1]      P0: fold (0)  | call (1)
//!   [0,1,0]    P0 folds, P0 -1
//!   [0,1,1]    showdown, stakes 2 each
//!   [1,0]      P1 folds, P1 -1  ->  P0 +1
//!   [1,1]      showdown, stakes 2 each

use pkr_cfr::dcfr::{update_regret_full, DiscountMode, MomentumMode};

const N_INFOSETS: usize = 12;
const N_ACTIONS: usize = 2;

/// Infoset index: ((player * 3 + card) * 2 + decision_point)
/// P0 decision_point 0 = initial, 1 = after check-bet
/// P1 decision_point 0 = after P0 check, 1 = after P0 bet
#[inline]
fn infoset_index(player: usize, card: u8, decision_point: usize) -> usize {
    (player * 3 + card as usize) * 2 + decision_point
}

/// Value to player 0 at a showdown given the two cards.
/// Higher card wins. Winner takes `stake` from the loser's view.
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

    /// One full-tree CFR iteration. Both players updated. All six
    /// distinct (c0, c1) card pairs evaluated, weighted 1/6.
    pub fn iterate(&mut self) {
        self.iteration += 1;
        // Snapshot the current iteration number; dcfr uses it for both
        // momentum and the discount schedule.
        let t = self.iteration;
        for c0 in 0..3u8 {
            for c1 in 0..3u8 {
                if c0 == c1 {
                    continue;
                }
                self.traverse([c0, c1], &[], [1.0, 1.0], t);
            }
        }
        // NaN check: only once per outer iteration to avoid overhead.
        if !self.nan_flag {
            for row in self.regrets.iter() {
                for &v in row.iter() {
                    if !v.is_finite() {
                        self.nan_flag = true;
                    }
                }
            }
        }
    }

    /// Returns value to player 0 at this history.
    fn traverse(
        &mut self,
        cards: [u8; 2],
        history: &[u8],
        reach: [f32; 2],
        t: u32,
    ) -> f32 {
        match history.len() {
            0 => {
                // P0 acts: check (0) or bet (1)
                let infoset = infoset_index(0, cards[0], 0);
                let strat = self.strategy_at(infoset);

                let mut vs = [0.0f32; 2];
                for a in 0..2 {
                    let mut h = history.to_vec();
                    h.push(a as u8);
                    let child_reach = [reach[0] * strat[a], reach[1]];
                    vs[a] = self.traverse(cards, &h, child_reach, t);
                }
                let v = strat[0] * vs[0] + strat[1] * vs[1];

                let delta = [vs[0] - v, vs[1] - v];
                self.apply_regret_update(infoset, delta, reach[1], t);
                self.add_strategy_sum(infoset, strat, reach[0]);
                v
            }
            1 => {
                // P1 acts, decision point depends on history[0]
                let dp = if history[0] == 0 { 0 } else { 1 };
                let infoset = infoset_index(1, cards[1], dp);
                let strat = self.strategy_at(infoset);

                let mut vs = [0.0f32; 2];
                for a in 0..2 {
                    let mut h = history.to_vec();
                    h.push(a as u8);
                    let child_reach = [reach[0], reach[1] * strat[a]];
                    vs[a] = self.traverse(cards, &h, child_reach, t);
                }
                let v = strat[0] * vs[0] + strat[1] * vs[1];

                // traverse returns value-to-P0. P1 maximizes their own
                // value = minimizes v_to_P0, so regret[a] is negated.
                let delta = [v - vs[0], v - vs[1]];
                self.apply_regret_update(infoset, delta, reach[0], t);
                self.add_strategy_sum(infoset, strat, reach[1]);
                v
            }
            2 => {
                match (history[0], history[1]) {
                    (0, 0) => showdown(cards, 1.0),
                    (0, 1) => {
                        // P0 fold (0) or call (1)
                        let infoset = infoset_index(0, cards[0], 1);
                        let strat = self.strategy_at(infoset);
                        let v_fold = -1.0;
                        let v_call = showdown(cards, 2.0);
                        let vs = [v_fold, v_call];
                        let v = strat[0] * vs[0] + strat[1] * vs[1];
                        let delta = [vs[0] - v, vs[1] - v];
                        self.apply_regret_update(infoset, delta, reach[1], t);
                        self.add_strategy_sum(infoset, strat, reach[0]);
                        v
                    }
                    (1, 0) => 1.0,           // P1 folded
                    (1, 1) => showdown(cards, 2.0),
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
    }

    fn apply_regret_update(
        &mut self,
        infoset: usize,
        delta: [f32; 2],
        opp_reach: f32,
        t: u32,
    ) {
        for a in 0..N_ACTIONS {
            let cur = self.regrets[infoset][a];
            let mom = self.momentums[infoset][a];
            let d = delta[a] * opp_reach;
            let (new_r, new_m) =
                update_regret_full(cur, mom, t, d, self.mode, self.momentum);
            self.regrets[infoset][a] = new_r;
            self.momentums[infoset][a] = new_m;
        }
    }

    fn add_strategy_sum(&mut self, infoset: usize, strat: [f32; 2], reach: f32) {
        for a in 0..N_ACTIONS {
            self.strategy_sum[infoset][a] += strat[a] * reach;
        }
    }

    // --- Exploitability --------------------------------------------------

    /// Value to player 0 when both play their average strategies.
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

    /// Best-response value to player 0 against fixed average strategy of
    /// player 1. P0 maximizes at every one of his decision points.
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

    /// Value to player 0 when P1 best-responds against P0's average
    /// strategy. P1 minimizes at every one of his decision points.
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

    /// (BR0 + BR1_negated) / 2 where BR1 is from P0's perspective.
    /// Zero at Nash. Positive otherwise.
    pub fn exploitability(&self) -> f32 {
        let br0 = self.br_value_p0();
        let br1_p0 = self.br_value_p0_given_br1();
        (br0 - br1_p0) / 2.0
    }
}
