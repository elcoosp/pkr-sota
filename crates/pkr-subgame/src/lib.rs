//! River subgame solving POC.
//!
//! Concrete-card CFR on a river subgame with fixed ranges.
//!
//! Architecture: the public betting tree is enumerated once from the root
//! GameState. Each node caches the actions-by-bucket mapping. CFR walks the
//! tree by node_id; regrets/strategy sums live in flat `Vec<[f64;6]>`
//! indexed by `node_id * n_deals + deal_id`. No HashMap, no infoset
//! signature hashing on the hot path.

#![allow(clippy::needless_range_loop)]

use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::{Action, ActionKind, GameState};

pub const MAX_ACTIONS: usize = 8;
pub const ABSTRACT_BUCKETS: usize = 6;
const MAX_TREE_DEPTH: u32 = 50;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub struct Range {
    pub hands: Vec<[u8; 2]>,
    pub probs: Vec<f64>,
}

impl Range {
    pub fn uniform(hands: Vec<[u8; 2]>) -> Self {
        let n = hands.len().max(1) as f64;
        let probs = vec![1.0 / n; hands.len()];
        Range { hands, probs }
    }
}

pub struct POCConfig<'a> {
    pub root: GameState,
    pub p0_range: Range,
    pub p1_range: Range,
    pub iterations: u32,
    pub evaluator: &'a dyn Evaluator,
    pub blueprint: Option<(&'a dyn AbstractionBuilder, &'a CompactRegretTable)>,
}

pub struct POCResult {
    pub br_v1_vs_cfr: f64,
    pub br_v1_vs_blueprint: Option<f64>,
    pub iterations: u32,
    pub nodes_visited: u64,
}

pub fn run_poc(cfg: &POCConfig) -> POCResult {
    let mut solver = Solver::new(cfg);
    solver.solve();
    let br_v1_vs_cfr = solver.compute_br_v1(None);
    let br_v1_vs_blueprint = cfg
        .blueprint
        .map(|(abs, tbl)| solver.compute_br_v1(Some((abs, tbl))));
    POCResult {
        br_v1_vs_cfr,
        br_v1_vs_blueprint,
        iterations: cfg.iterations,
        nodes_visited: solver.nodes_visited,
    }
}

// ---------------------------------------------------------------------------
// Public tree
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum PublicNode {
    /// Fold terminal. `to_p0` is P0's chip value, already signed.
    Fold { to_p0: f64 },
    /// Showdown terminal. Value depends on deal ranks.
    Showdown { pot: f32, invested_p0: f32 },
    /// Decision node.
    Decision {
        actor: u8,
        /// Child node id per bucket, or -1 if that bucket is illegal here.
        bucket_child: [i32; ABSTRACT_BUCKETS],
    },
}

struct PublicTree {
    nodes: Vec<PublicNode>,
    root: u32,
}

fn build_tree(state: &mut GameState, nodes: &mut Vec<PublicNode>, depth: u32) -> u32 {
    let idx = nodes.len() as u32;
    nodes.push(PublicNode::Fold { to_p0: 0.0 });

    if state.is_terminal() || depth >= MAX_TREE_DEPTH {
        let n = if state.folded[0] {
            PublicNode::Fold {
                to_p0: -(state.total_invested[0] as f64),
            }
        } else if state.folded[1] {
            PublicNode::Fold {
                to_p0: state.pot as f64 - state.total_invested[0] as f64,
            }
        } else {
            PublicNode::Showdown {
                pot: state.pot,
                invested_p0: state.total_invested[0],
            }
        };
        nodes[idx as usize] = n;
        return idx;
    }

    let actor = state.actor;
    let mut buf = [Action { player: 0, kind: ActionKind::Fold }; MAX_ACTIONS];
    let n = state.legal_actions_into(&mut buf);
    if n == 0 {
        nodes[idx as usize] = PublicNode::Fold { to_p0: 0.0 };
        return idx;
    }

    let mut bucket_child = [-1i32; ABSTRACT_BUCKETS];
    for b in 0..ABSTRACT_BUCKETS {
        let pick = buf
            .iter()
            .take(n)
            .find(|act| bucket_of_state(state, &act.kind) as usize == b)
            .copied();
        if let Some(act) = pick {
            state.apply_action_in_place(&act);
            let child = build_tree(state, nodes, depth + 1);
            state.undo_action();
            bucket_child[b] = child as i32;
        }
    }
    nodes[idx as usize] = PublicNode::Decision { actor: actor as u8, bucket_child };
    idx
}

fn bucket_of_state(state: &GameState, kind: &ActionKind) -> u8 {
    let actor = state.actor;
    pkr_core::abstraction::action_bucket(
        kind,
        state.stacks[actor],
        state.street_bets[actor],
        state.street_bets[1 - actor],
        state.pot,
    )
}

// ---------------------------------------------------------------------------
// Solver
// ---------------------------------------------------------------------------

struct Deal {
    h0: [u8; 2],
    h1: [u8; 2],
    prior: f64,
}

struct Solver<'a> {
    cfg: &'a POCConfig<'a>,
    tree: PublicTree,
    deals: Vec<Deal>,
    n_nodes: usize,
    n_deals: usize,
    /// Per-(node, deal) regret & strategy-sum arrays. Index = node_id * n_deals + deal_id.
    reg0: Vec<[f64; ABSTRACT_BUCKETS]>,
    reg1: Vec<[f64; ABSTRACT_BUCKETS]>,
    sum0: Vec<[f64; ABSTRACT_BUCKETS]>,
    sum1: Vec<[f64; ABSTRACT_BUCKETS]>,
    /// Precomputed terminal values indexed the same way.
    term_val: Vec<f64>,
    iter_weight: f64,
    nodes_visited: u64,
}

impl<'a> Solver<'a> {
    fn new(cfg: &'a POCConfig<'a>) -> Self {
        // --- Build public tree ---
        let mut tree_state = cfg.root.clone();
        let mut nodes = Vec::new();
        let root = build_tree(&mut tree_state, &mut nodes, 0);
        let n_nodes = nodes.len();

        // --- Build deal list ---
        let mut deals = Vec::new();
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
                deals.push(Deal { h0, h1, prior });
            }
        }
        let n_deals = deals.len();

        // --- Precompute hand ranks per deal ---
        let board = &cfg.root.board[..cfg.root.board_len as usize];
        let mut rank0 = Vec::with_capacity(n_deals);
        let mut rank1 = Vec::with_capacity(n_deals);
        for d in &deals {
            rank0.push(cfg.evaluator.evaluate_hand(&d.h0, board));
            rank1.push(cfg.evaluator.evaluate_hand(&d.h1, board));
        }

        // --- Precompute terminal values ---
        let mut term_val = vec![0.0f64; n_nodes * n_deals];
        for node_id in 0..n_nodes {
            match nodes[node_id] {
                PublicNode::Fold { to_p0 } => {
                    for deal_idx in 0..n_deals {
                        term_val[node_id * n_deals + deal_idx] = to_p0;
                    }
                }
                PublicNode::Showdown { pot, invested_p0 } => {
                    for deal_idx in 0..n_deals {
                        let r0 = rank0[deal_idx];
                        let r1 = rank1[deal_idx];
                        let v = if r0 == r1 {
                            (pot / 2.0 - invested_p0) as f64
                        } else if r0 < r1 {
                            (pot - invested_p0) as f64
                        } else {
                            -(invested_p0 as f64)
                        };
                        term_val[node_id * n_deals + deal_idx] = v;
                    }
                }
                PublicNode::Decision { .. } => {}
            }
        }

        Solver {
            cfg,
            tree: PublicTree { nodes, root },
            deals,
            n_nodes,
            n_deals,
            reg0: vec![[0.0; ABSTRACT_BUCKETS]; n_nodes * n_deals],
            reg1: vec![[0.0; ABSTRACT_BUCKETS]; n_nodes * n_deals],
            sum0: vec![[0.0; ABSTRACT_BUCKETS]; n_nodes * n_deals],
            sum1: vec![[0.0; ABSTRACT_BUCKETS]; n_nodes * n_deals],
            term_val,
            iter_weight: 1.0,
            nodes_visited: 0,
        }
    }

    #[inline]
    fn idx(&self, node_id: u32, deal_idx: u32) -> usize {
        node_id as usize * self.n_deals + deal_idx as usize
    }

    fn walk(&mut self, node_id: u32, deal_idx: u32, reach0: f64, reach1: f64) -> f64 {
        self.nodes_visited += 1;
        let node = self.tree.nodes[node_id as usize];
        match node {
            PublicNode::Fold { .. } | PublicNode::Showdown { .. } => {
                self.term_val[self.idx(node_id, deal_idx)]
            }
            PublicNode::Decision { actor, bucket_child } => {
                let i = self.idx(node_id, deal_idx);
                let prior = self.deals[deal_idx as usize].prior;

                let regrets = if actor == 0 { &self.reg0[i] } else { &self.reg1[i] };
                let strat = regret_matching(regrets, &bucket_child);

                let mut cfv = [0.0f64; ABSTRACT_BUCKETS];
                let mut avg_p0 = 0.0f64;
                let mut avg_actor = 0.0f64;
                for b in 0..ABSTRACT_BUCKETS {
                    if bucket_child[b] < 0 { continue; }
                    let child = bucket_child[b] as u32;
                    let v_p0 = if actor == 0 {
                        self.walk(child, deal_idx, reach0 * strat[b], reach1)
                    } else {
                        self.walk(child, deal_idx, reach0, reach1 * strat[b])
                    };
                    cfv[b] = if actor == 0 { v_p0 } else { -v_p0 };
                    avg_p0 += strat[b] * v_p0;
                    avg_actor += strat[b] * cfv[b];
                }

                let regs = if actor == 0 { &mut self.reg0[i] } else { &mut self.reg1[i] };
                let reach_opp = if actor == 0 { reach1 } else { reach0 };
                for b in 0..ABSTRACT_BUCKETS {
                    if bucket_child[b] < 0 { continue; }
                    let r = regs[b] + reach_opp * prior * (cfv[b] - avg_actor);
                    regs[b] = if r > 0.0 { r } else { 0.0 };
                }

                let sums = if actor == 0 { &mut self.sum0[i] } else { &mut self.sum1[i] };
                let reach_self = if actor == 0 { reach0 } else { reach1 };
                let w = self.iter_weight;
                for b in 0..ABSTRACT_BUCKETS {
                    if bucket_child[b] < 0 { continue; }
                    sums[b] += w * reach_self * strat[b];
                }

                avg_p0
            }
        }
    }

    fn solve(&mut self) {
        let root = self.tree.root;
        let n_deals = self.n_deals;
        for iter in 0..self.cfg.iterations {
            self.iter_weight = (iter + 1) as f64;
            for deal_idx in 0..n_deals {
                self.walk(root, deal_idx as u32, 1.0, 1.0);
            }
        }
    }

    /// P0 strategy at every node. None => uniform fallback.
    fn p0_strategy(&self) -> Vec<Option<[f64; ABSTRACT_BUCKETS]>> {
        let mut out = vec![None; self.n_nodes * self.n_deals];
        for node_id in 0..self.n_nodes {
            if let PublicNode::Decision { actor: 0, bucket_child } = self.tree.nodes[node_id] {
                for deal_idx in 0..self.n_deals {
                    let i = node_id * self.n_deals + deal_idx;
                    let sum: f64 = self.sum0[i].iter().sum();
                    if sum > 1e-12 {
                        let mut s = [0.0; ABSTRACT_BUCKETS];
                        for b in 0..ABSTRACT_BUCKETS {
                            if bucket_child[b] >= 0 {
                                s[b] = self.sum0[i][b] / sum;
                            }
                        }
                        out[i] = Some(s);
                    }
                }
            }
        }
        out
    }

    fn br_v1(&self, p0_strategy: &[Option<[f64; ABSTRACT_BUCKETS]>]) -> f64 {
        // br_walk returns value to P0; we want P1's best-response value
        // (the number that goes into exploitability), so negate once here.
        let mut total = 0.0f64;
        for deal_idx in 0..self.n_deals {
            let prior = self.deals[deal_idx].prior;
            let v_p0 = self.br_walk(self.tree.root, deal_idx as u32, p0_strategy);
            total += prior * (-v_p0);
        }
        total
    }

    fn br_walk(
        &self,
        node_id: u32,
        deal_idx: u32,
        p0_strategy: &[Option<[f64; ABSTRACT_BUCKETS]>],
    ) -> f64 {
        let node = self.tree.nodes[node_id as usize];
        match node {
            PublicNode::Fold { .. } | PublicNode::Showdown { .. } => {
                self.term_val[self.idx(node_id, deal_idx)]
            }
            PublicNode::Decision { actor, bucket_child } => {
                if actor == 0 {
                    let i = self.idx(node_id, deal_idx);
                    let strat = match &p0_strategy[i] {
                        Some(s) => *s,
                        None => uniform_over_legal(&bucket_child),
                    };
                    let mut v = 0.0f64;
                    for b in 0..ABSTRACT_BUCKETS {
                        if bucket_child[b] < 0 { continue; }
                        v += strat[b]
                            * self.br_walk(bucket_child[b] as u32, deal_idx, p0_strategy);
                    }
                    v
                } else {
                    let mut best = f64::NEG_INFINITY;
                    for b in 0..ABSTRACT_BUCKETS {
                        if bucket_child[b] < 0 { continue; }
                        let v_p0 =
                            self.br_walk(bucket_child[b] as u32, deal_idx, p0_strategy);
                        let v_p1 = -v_p0;
                        if v_p1 > best {
                            best = v_p1;
                        }
                    }
                    if best.is_finite() { -best } else { 0.0 }
                }
            }
        }
    }

    fn compute_br_v1(
        &self,
        blueprint: Option<(&dyn AbstractionBuilder, &CompactRegretTable)>,
    ) -> f64 {
        match blueprint {
            None => {
                let strat = self.p0_strategy();
                self.br_v1(&strat)
            }
            Some((abs, tbl)) => {
                let mut strat: Vec<Option<[f64; ABSTRACT_BUCKETS]>> =
                    vec![None; self.n_nodes * self.n_deals];
                let mut state = self.cfg.root.clone();
                self.fill_blueprint_strat(
                    self.tree.root,
                    &mut state,
                    &mut strat,
                    abs,
                    tbl,
                );
                self.br_v1(&strat)
            }
        }
    }

    fn fill_blueprint_strat(
        &self,
        node_id: u32,
        state: &mut GameState,
        strat: &mut [Option<[f64; ABSTRACT_BUCKETS]>],
        abs: &dyn AbstractionBuilder,
        tbl: &CompactRegretTable,
    ) {
        let node = self.tree.nodes[node_id as usize];
        if let PublicNode::Decision { actor, bucket_child } = node {
            if actor == 0 {
                let mut sig_buf = [0u8; 8];
                let sig_len = state.infoset_signature_into(&mut sig_buf);
                let board = &state.board[..state.board_len as usize];
                for deal_idx in 0..self.n_deals {
                    let h0 = self.deals[deal_idx].h0;
                    let hash = abs.get_infoset_hash(
                        &h0,
                        board,
                        &sig_buf[..sig_len],
                        state.street as u8,
                    );
                    let mut cluster = [0.0f32; ABSTRACT_BUCKETS];
                    tbl.get_average_strategy_into(hash, &mut cluster);
                    let mut s = [0.0; ABSTRACT_BUCKETS];
                    let mut sum = 0.0f64;
                    for b in 0..ABSTRACT_BUCKETS {
                        if bucket_child[b] >= 0 {
                            s[b] = cluster[b] as f64;
                            sum += s[b];
                        }
                    }
                    if sum > 1e-12 {
                        for b in 0..ABSTRACT_BUCKETS {
                            s[b] /= sum;
                        }
                        let i = self.idx(node_id, deal_idx as u32);
                        strat[i] = Some(s);
                    }
                }
            }
            let mut buf = [Action { player: 0, kind: ActionKind::Fold }; MAX_ACTIONS];
            let n = state.legal_actions_into(&mut buf);
            for b in 0..ABSTRACT_BUCKETS {
                if bucket_child[b] < 0 { continue; }
                let pick = buf
                    .iter()
                    .take(n)
                    .find(|act| bucket_of_state(state, &act.kind) as usize == b)
                    .copied();
                if let Some(act) = pick {
                    state.apply_action_in_place(&act);
                    self.fill_blueprint_strat(
                        bucket_child[b] as u32,
                        state,
                        strat,
                        abs,
                        tbl,
                    );
                    state.undo_action();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn uniform_over_legal(bucket_child: &[i32; ABSTRACT_BUCKETS]) -> [f64; ABSTRACT_BUCKETS] {
    let n = bucket_child.iter().filter(|&&c| c >= 0).count().max(1);
    let u = 1.0 / n as f64;
    let mut s = [0.0; ABSTRACT_BUCKETS];
    for b in 0..ABSTRACT_BUCKETS {
        if bucket_child[b] >= 0 {
            s[b] = u;
        }
    }
    s
}

fn regret_matching(
    regrets: &[f64; ABSTRACT_BUCKETS],
    bucket_child: &[i32; ABSTRACT_BUCKETS],
) -> [f64; ABSTRACT_BUCKETS] {
    let mut sum = 0.0f64;
    for b in 0..ABSTRACT_BUCKETS {
        if bucket_child[b] >= 0 && regrets[b] > 0.0 {
            sum += regrets[b];
        }
    }
    let mut s = [0.0; ABSTRACT_BUCKETS];
    if sum > 1e-12 {
        for b in 0..ABSTRACT_BUCKETS {
            if bucket_child[b] >= 0 && regrets[b] > 0.0 {
                s[b] = regrets[b] / sum;
            }
        }
    } else {
        return uniform_over_legal(bucket_child);
    }
    s
}

fn incompatible(a: &[u8; 2], b: &[u8; 2]) -> bool {
    a[0] == b[0] || a[0] == b[1] || a[1] == b[0] || a[1] == b[1]
}

// ---------------------------------------------------------------------------
// Blueprint adapter (kept for tests / compat)
// ---------------------------------------------------------------------------

pub fn blueprint_p0_strategy(
    hole: &[u8; 2],
    state: &GameState,
    abstraction: &dyn AbstractionBuilder,
    table: &CompactRegretTable,
) -> [f64; MAX_ACTIONS] {
    let mut sig_buf = [0u8; 8];
    let sig_len = state.infoset_signature_into(&mut sig_buf);
    let board = &state.board[..state.board_len as usize];
    let hash = abstraction.get_infoset_hash(
        hole,
        board,
        &sig_buf[..sig_len],
        state.street as u8,
    );
    let mut cluster = [0.0f32; ABSTRACT_BUCKETS];
    table.get_average_strategy_into(hash, &mut cluster);
    let _actor = state.actor;
    let mut buf = [Action { player: 0, kind: ActionKind::Fold }; MAX_ACTIONS];
    let n = state.legal_actions_into(&mut buf);
    let mut out = [0.0; MAX_ACTIONS];
    for a in 0..n {
        let b = bucket_of_state(state, &buf[a].kind) as usize;
        if b < ABSTRACT_BUCKETS {
            out[a] = cluster[b] as f64;
        }
    }
    out
}
