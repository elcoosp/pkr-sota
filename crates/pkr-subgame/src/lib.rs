//! River subgame solving — proof of concept.
//!
//! Concrete-card CFR on a river subgame with fixed ranges. The public tree
//! is enumerated by mutating a `GameState` (apply_action / undo_action), and
//! infosets are keyed by `(actor_hole, public_node_signature)`.
//!
//! The POC measures: can 100 iterations of CFR+ on the concrete subgame
//! produce a P0 strategy that is less exploitable than the blueprint's?
//!
//! See docs/ for the design.

#![allow(clippy::needless_range_loop)]

use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::{Action, ActionKind, GameState};
use std::collections::HashMap;
use foldhash::fast::RandomState as FxState;

type FastMap<K, V> = HashMap<K, V, FxState>;

pub const MAX_ACTIONS: usize = 8;
pub const ABSTRACT_BUCKETS: usize = 6;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub struct Range {
    /// One `[c0, c1]` per hand. Values are card ids 0..51.
    pub hands: Vec<[u8; 2]>,
    /// Un-normalized weights; must be non-negative. len == hands.len().
    pub probs: Vec<f64>,
}

impl Range {
    pub fn uniform(hands: Vec<[u8; 2]>) -> Self {
        let n = hands.len().max(1) as f64;
        let probs = vec![1.0 / n; hands.len()];
        Range { hands, probs }
    }
}

/// Everything the POC needs. `root` must already be at the river, with
/// `actor` being P0 (the player whose strategy we're solving).
pub struct POCConfig<'a> {
    pub root: GameState,
    pub p0_range: Range,
    pub p1_range: Range,
    pub iterations: u32,
    pub evaluator: &'a dyn Evaluator,
    /// Optional: if provided, we compare the CFR-solved P0 against the
    /// blueprint's P0. Requires the abstraction + table used in training.
    pub blueprint: Option<(&'a dyn AbstractionBuilder, &'a CompactRegretTable)>,
}

pub struct POCResult {
    /// P1 best-response value against the CFR-solved P0 strategy.
    pub br_v1_vs_cfr: f64,
    /// P1 best-response value against the blueprint's P0 strategy.
    /// `None` if no blueprint was provided.
    pub br_v1_vs_blueprint: Option<f64>,
    pub iterations: u32,
    pub nodes_visited: u64,
}

/// Main entry: solve, then measure exploitability via P1 BR against both
/// strategies (CFR-solved and blueprint).
pub fn run_poc(cfg: &POCConfig) -> POCResult {
    let (p0_strategy, nodes) = cfr_solve_p0(cfg);

    let br_v1_vs_cfr = compute_br_v1(cfg, |hole, state| {
        let k = (hole_key_of(hole), node_key_of(state));
        p0_strategy
            .get(&k)
            .copied()
            .unwrap_or_else(|| uniform_over_legal(state))
    });

    let br_v1_vs_blueprint = cfg.blueprint.map(|(abs, tbl)| {
        compute_br_v1(cfg, |hole, state| blueprint_p0_strategy(hole, state, abs, tbl))
    });

    POCResult {
        br_v1_vs_cfr,
        br_v1_vs_blueprint,
        iterations: cfg.iterations,
        nodes_visited: nodes,
    }
}

// ---------------------------------------------------------------------------
// CFR on the subgame
// ---------------------------------------------------------------------------

type RegretMap = FastMap<(u64, u64), [f64; MAX_ACTIONS]>;

struct CfrState<'a> {
    cfg: &'a POCConfig<'a>,
    reg0: RegretMap,
    reg1: RegretMap,
    sum0: RegretMap,
    sum1: RegretMap,
    nodes: u64,
    /// Linear averaging weight (iteration index + 1). CFR+ uses this in the
    /// strategy sum to get O(1/T) convergence on river subgames.
    iter_weight: f64,
    /// Cached hand ranks for the current deal. Recomputed once per deal
    /// per iteration to avoid evaluator calls at every showdown terminal.
    cur_rank0: u32,
    cur_rank1: u32,
}

fn cfr_solve_p0(cfg: &POCConfig) -> (RegretMap, u64) {
    // Pre-compute the deal list once. Each entry is a GameState at the
    // subgame root with holes set, plus the joint prior for the pair.
    // Reused across iterations — walk() is apply/undo-symmetric so the
    // state is unchanged after a full pass.
    struct Deal {
        state: GameState,
        h0: [u8; 2],
        h1: [u8; 2],
        prior: f64,
    }

    let mut deals: Vec<Deal> = Vec::new();
    for i in 0..cfg.p0_range.hands.len() {
        for j in 0..cfg.p1_range.hands.len() {
            let h0 = cfg.p0_range.hands[i];
            let h1 = cfg.p1_range.hands[j];
            if incompatible(&h0, &h1) || h0[0] == h0[1] || h1[0] == h1[1] {
                continue;
            }
            let prior = cfg.p0_range.probs[i] * cfg.p1_range.probs[j];
            if prior <= 0.0 {
                continue;
            }
            let mut state = cfg.root.clone();
            state.set_hole_cards(h0, h1);
            deals.push(Deal { state, h0, h1, prior });
        }
    }

    eprintln!(
        "[cfr_solve_p0] iterations={} p0_hands={} p1_hands={} valid_deals={}",
        cfg.iterations,
        cfg.p0_range.hands.len(),
        cfg.p1_range.hands.len(),
        deals.len(),
    );

    let mut st = CfrState {
        cfg,
        reg0: FastMap::default(),
        reg1: FastMap::default(),
        sum0: FastMap::default(),
        sum1: FastMap::default(),
        nodes: 0,
        iter_weight: 1.0,
        cur_rank0: 0,
        cur_rank1: 0,
    };

    // Dump the first deal state once so we can verify the subgame root.
    if let Some(d) = deals.first() {
        eprintln!(
            "[root dump] street={:?} board_len={} actor={} pot={:.2} street_bets={:?} total_invested={:?}",
            d.state.street, d.state.board_len, d.state.actor, d.state.pot,
            d.state.street_bets, d.state.total_invested,
        );
        eprintln!(
            "[root dump] is_terminal={} is_street_complete={}",
            d.state.is_terminal(), d.state.is_street_complete(),
        );
    }

    let board_slice = &cfg.root.board[..cfg.root.board_len as usize];
    for iter in 0..cfg.iterations {
        st.iter_weight = (iter + 1) as f64;
        for d in deals.iter_mut() {
            st.cur_rank0 = cfg.evaluator.evaluate_hand(&d.h0, board_slice);
            st.cur_rank1 = cfg.evaluator.evaluate_hand(&d.h1, board_slice);
            st.walk(&mut d.state, &d.h0, &d.h1, 1.0, 1.0, d.prior, 0);
        }
    }

    eprintln!("[cfr_solve_p0] st.nodes={} total_deals_walked={}",
        st.nodes, deals.len() * cfg.iterations as usize);

    // P0's averaged strategy: normalize strat_sum.
    let mut out: RegretMap = FastMap::default();
    for (k, ss) in &st.sum0 {
        let sum: f64 = ss.iter().sum();
        let mut s = [0.0; MAX_ACTIONS];
        if sum > 1e-12 {
            for a in 0..MAX_ACTIONS {
                s[a] = ss[a] / sum;
            }
        } else {
            for a in 0..MAX_ACTIONS {
                s[a] = 1.0 / MAX_ACTIONS as f64;
            }
        }
        out.insert(*k, s);
    }
    (out, st.nodes)
}

impl<'a> CfrState<'a> {
    /// Terminal value to P0 using precomputed hand ranks. Caller is
    /// responsible for setting `cur_rank0` / `cur_rank1` before each walk.
    fn terminal_value_cached(&self, state: &GameState) -> f64 {
        let to_p0 = if state.folded[0] {
            -state.total_invested[0]
        } else if state.folded[1] {
            state.pot - state.total_invested[0]
        } else {
            let r0 = self.cur_rank0;
            let r1 = self.cur_rank1;
            if r0 == r1 {
                state.pot / 2.0 - state.total_invested[0]
            } else if r0 < r1 {
                state.pot - state.total_invested[0]
            } else {
                -state.total_invested[0]
            }
        };
        to_p0 as f64
    }

    /// Returns value to P0 from the current node.
    fn walk(
        &mut self,
        state: &mut GameState,
        h0: &[u8; 2],
        h1: &[u8; 2],
        reach0: f64,
        reach1: f64,
        prior: f64,
        depth: u32,
    ) -> f64 {
        self.nodes += 1;
        if self.nodes <= 5 {
            eprintln!("[walk] nodes={} depth={} terminal={} actor={}",
                self.nodes, depth, state.is_terminal(), state.actor);
        }
        if depth > 30 {
            return 0.0;
        }
        if state.is_terminal() {
            return self.terminal_value_cached(state);
        }

        let actor = state.actor;
        let mut buf = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; MAX_ACTIONS];
        let n = state.legal_actions_into(&mut buf);
        if n == 0 {
            return 0.0;
        }

        let node_key = node_key_of(state);

        if actor == 0 {
            let hole_key = hole_key_of(h0);
            let info_key = (hole_key, node_key);

            // Copy regrets out so we don't hold a borrow across self.walk.
            let regs_snapshot = *self.reg0.entry(info_key).or_insert([0.0; MAX_ACTIONS]);
            let strat = regret_matching(&regs_snapshot, n);

            let mut cfv = [0.0f64; MAX_ACTIONS];
            let mut avg = 0.0f64;
            for a in 0..n {
                state.apply_action_in_place(&buf[a]);
                let v = self.walk(state, h0, h1, reach0 * strat[a], reach1, prior, depth + 1);
                state.undo_action();
                cfv[a] = v;
                avg += strat[a] * v;
            }

            // Write regrets back
            {
                let regs = self.reg0.entry(info_key).or_insert([0.0; MAX_ACTIONS]);
                for a in 0..n {
                    let r = regs[a] + reach1 * prior * (cfv[a] - avg);
                    regs[a] = if r > 0.0 { r } else { 0.0 };
                }
            }
            // Strategy sum
            {
                let w = self.iter_weight;
                let ss = self.sum0.entry(info_key).or_insert([0.0; MAX_ACTIONS]);
                for a in 0..n {
                    ss[a] += w * reach0 * strat[a];
                }
            }
            avg
        } else {
            let hole_key = hole_key_of(h1);
            let info_key = (hole_key, node_key);

            let regs_snapshot = *self.reg1.entry(info_key).or_insert([0.0; MAX_ACTIONS]);
            let strat = regret_matching(&regs_snapshot, n);

            let mut cfv_p1 = [0.0f64; MAX_ACTIONS];
            let mut avg_p1 = 0.0f64;
            let mut avg_p0 = 0.0f64;
            for a in 0..n {
                state.apply_action_in_place(&buf[a]);
                let v_p0 = self.walk(state, h0, h1, reach0, reach1 * strat[a], prior, depth + 1);
                state.undo_action();
                let v_p1 = -v_p0;
                cfv_p1[a] = v_p1;
                avg_p1 += strat[a] * v_p1;
                avg_p0 += strat[a] * v_p0;
            }

            {
                let regs = self.reg1.entry(info_key).or_insert([0.0; MAX_ACTIONS]);
                for a in 0..n {
                    let r = regs[a] + reach0 * prior * (cfv_p1[a] - avg_p1);
                    regs[a] = if r > 0.0 { r } else { 0.0 };
                }
            }
            {
                let w = self.iter_weight;
                let ss = self.sum1.entry(info_key).or_insert([0.0; MAX_ACTIONS]);
                for a in 0..n {
                    ss[a] += w * reach1 * strat[a];
                }
            }
            avg_p0
        }
    }
}

// ---------------------------------------------------------------------------
// P1 best response against a fixed P0 strategy
// ---------------------------------------------------------------------------

fn compute_br_v1<F>(cfg: &POCConfig, p0_fn: F) -> f64
where
    F: Fn(&[u8; 2], &GameState) -> [f64; MAX_ACTIONS],
{
    let mut total = 0.0f64;
    for i in 0..cfg.p0_range.hands.len() {
        for j in 0..cfg.p1_range.hands.len() {
            let h0 = cfg.p0_range.hands[i];
            let h1 = cfg.p1_range.hands[j];
            if incompatible(&h0, &h1) || h0[0] == h0[1] || h1[0] == h1[1] {
                continue;
            }
            let prior = cfg.p0_range.probs[i] * cfg.p1_range.probs[j];
            if prior <= 0.0 {
                continue;
            }
            let mut state = cfg.root.clone();
            state.set_hole_cards(h0, h1);
            let v_p1 = br_walk_p1(&mut state, &h0, &h1, &p0_fn, cfg.evaluator, 0);
            total += prior * v_p1;
        }
    }
    total
}

fn br_walk_p1<F>(
    state: &mut GameState,
    h0: &[u8; 2],
    h1: &[u8; 2],
    p0_fn: &F,
    evaluator: &dyn Evaluator,
    depth: u32,
) -> f64
where
    F: Fn(&[u8; 2], &GameState) -> [f64; MAX_ACTIONS],
{
    if depth > 30 {
        return 0.0;
    }
    if state.is_terminal() {
        return -terminal_value(state, h0, h1, evaluator);
    }
    let actor = state.actor;
    let mut buf = [Action {
        player: 0,
        kind: ActionKind::Fold,
    }; MAX_ACTIONS];
    let n = state.legal_actions_into(&mut buf);
    if n == 0 {
        return 0.0;
    }

    if actor == 0 {
        let strat = p0_fn(h0, state);
        let mut v = 0.0f64;
        for a in 0..n {
            state.apply_action_in_place(&buf[a]);
            let cv = br_walk_p1(state, h0, h1, p0_fn, evaluator, depth + 1);
            state.undo_action();
            v += strat[a] * cv;
        }
        v
    } else {
        let mut best = f64::NEG_INFINITY;
        for a in 0..n {
            state.apply_action_in_place(&buf[a]);
            let cv = br_walk_p1(state, h0, h1, p0_fn, evaluator, depth + 1);
            state.undo_action();
            if cv > best {
                best = cv;
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// Blueprint strategy adapter
// ---------------------------------------------------------------------------

pub fn blueprint_p0_strategy(
    hole: &[u8; 2],
    state: &GameState,
    abstraction: &dyn AbstractionBuilder,
    table: &CompactRegretTable,
) -> [f64; MAX_ACTIONS] {
    let mut sig_buf = [0u8; 8];
    let sig_len = state.infoset_signature_into(&mut sig_buf);
    let history = &sig_buf[..sig_len];
    let board = &state.board[..state.board_len as usize];
    let hash = abstraction.get_infoset_hash(hole, board, history, state.street as u8);

    let mut cluster_strat = [0.0f32; ABSTRACT_BUCKETS];
    table.get_average_strategy_into(hash, &mut cluster_strat);

    let actor = state.actor;
    let mut buf = [Action {
        player: 0,
        kind: ActionKind::Fold,
    }; MAX_ACTIONS];
    let n = state.legal_actions_into(&mut buf);

    let mut out = [0.0; MAX_ACTIONS];
    for a in 0..n {
        let bucket = pkr_core::abstraction::action_bucket(
            &buf[a].kind,
            state.stacks[actor],
            state.street_bets[actor],
            state.street_bets[1 - actor],
            state.pot,
        ) as usize;
        if bucket < ABSTRACT_BUCKETS {
            out[a] = cluster_strat[bucket] as f64;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn uniform_over_legal(state: &GameState) -> [f64; MAX_ACTIONS] {
    let mut buf = [Action {
        player: 0,
        kind: ActionKind::Fold,
    }; MAX_ACTIONS];
    let n = state.legal_actions_into(&mut buf).max(1);
    let mut out = [0.0; MAX_ACTIONS];
    let u = 1.0 / n as f64;
    for a in 0..n {
        out[a] = u;
    }
    out
}

fn regret_matching(regrets: &[f64; MAX_ACTIONS], n: usize) -> [f64; MAX_ACTIONS] {
    let mut sum = 0.0f64;
    for a in 0..n {
        if regrets[a] > 0.0 {
            sum += regrets[a];
        }
    }
    let mut s = [0.0; MAX_ACTIONS];
    if sum > 1e-12 {
        for a in 0..n {
            if regrets[a] > 0.0 {
                s[a] = regrets[a] / sum;
            }
        }
    } else {
        let u = 1.0 / n.max(1) as f64;
        for a in 0..n {
            s[a] = u;
        }
    }
    s
}

fn terminal_value(
    state: &GameState,
    h0: &[u8; 2],
    h1: &[u8; 2],
    evaluator: &dyn Evaluator,
) -> f64 {
    let to_p0 = if state.folded[0] {
        -state.total_invested[0]
    } else if state.folded[1] {
        state.pot - state.total_invested[0]
    } else {
        let board = &state.board[..state.board_len as usize];
        let r0 = evaluator.evaluate_hand(h0, board);
        let r1 = evaluator.evaluate_hand(h1, board);
        if r0 == r1 {
            state.pot / 2.0 - state.total_invested[0]
        } else if r0 < r1 {
            state.pot - state.total_invested[0]
        } else {
            -state.total_invested[0]
        }
    };
    to_p0 as f64
}

fn node_key_of(state: &GameState) -> u64 {
    let mut sig_buf = [0u8; 8];
    let sig_len = state.infoset_signature_into(&mut sig_buf);
    fnv1a_bytes(&sig_buf[..sig_len])
}

fn hole_key_of(hole: &[u8; 2]) -> u64 {
    (hole[0] as u64) | ((hole[1] as u64) << 8)
}

fn fnv1a_bytes(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn incompatible(a: &[u8; 2], b: &[u8; 2]) -> bool {
    a[0] == b[0] || a[0] == b[1] || a[1] == b[0] || a[1] == b[1]
}
