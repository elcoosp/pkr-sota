//! Vector-form subgame solver (Phase 4.1) — port of the plan's Appendix A.
//! Player 0 = OOP (acts first postflop). Card = suit*13+rank; hand index
//! = C(hi,2)+lo; evaluator LOWER u32 = STRONGER (matches `Evaluator`).
//! Carries the full 1326-hand range through every node (unlike the
//! sampled-hand solver). Exact river enumeration; turn via chance nodes.

pub const N: usize = 1326;

pub fn index_of(h: [u8; 2]) -> usize {
    let (hi, lo) = if h[0] > h[1] { (h[0] as usize, h[1] as usize) } else { (h[1] as usize, h[0] as usize) };
    hi * (hi - 1) / 2 + lo
}
pub fn hole_table() -> Vec<[u8; 2]> {
    let mut t = vec![[0u8; 2]; N];
    for hi in 1..52usize { for lo in 0..hi { t[hi * (hi - 1) / 2 + lo] = [hi as u8, lo as u8]; } }
    t
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub board: Vec<u8>,
    pub dead: f32,
    pub stack: f32,
    pub root_actor: u8,
    pub root_inv: [f32; 2],
    pub bet_fracs: Vec<f32>,
    pub max_raises: u8,
    pub min_bet: f32,
    pub allin_snap: f32,
    pub ranges: [Vec<f32>; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act { Fold, Check, Call, Bet(f32) }

struct ActNode { player: usize, acts: Vec<Act>, kids: Vec<Node>, regret: Vec<f32>, ssum: Vec<f32> }
struct ShowNode { inv: f32, worst_first: Vec<u16>, rank: Vec<u32> }
struct ChanceNode { cards: Vec<u8>, kids: Vec<Node>, n_deck: f32 }
enum Node { Act(ActNode), Fold { folder: usize, inv: [f32; 2] }, Show(ShowNode), Chance(ChanceNode) }

#[derive(Clone, Copy)]
struct St { inv: [f32; 2], actor: usize, n_act: u8, raises: u8, last_raise: f32 }

pub struct Solver<'a> {
    spec: Spec,
    holes: Vec<[u8; 2]>,
    eval: &'a (dyn Fn(&[u8; 2], &[u8]) -> u32 + Sync),
    root: Node,
    pub iters: u32,
    valid_root: Vec<bool>,
}

impl<'a> Solver<'a> {
    pub fn new(spec: Spec, eval: &'a (dyn Fn(&[u8; 2], &[u8]) -> u32 + Sync)) -> Self {
        let holes = hole_table();
        let mut s = Solver { spec, holes, eval, root: Node::Fold { folder: 0, inv: [0.0; 2] }, iters: 0, valid_root: vec![] };
        s.valid_root = (0..N).map(|h| !s.holes[h].iter().any(|c| s.spec.board.contains(c))).collect();
        for p in 0..2 { for h in 0..N { if !s.valid_root[h] { s.spec.ranges[p][h] = 0.0; } } }
        let st = St { inv: s.spec.root_inv, actor: s.spec.root_actor as usize, n_act: 0, raises: 0, last_raise: 0.0 };
        let mut board = s.spec.board.clone();
        let st = if (st.inv[0] - st.inv[1]).abs() > 1e-6 { St { raises: 1, last_raise: (st.inv[0] - st.inv[1]).abs(), n_act: 1, ..st } } else { st };
        s.root = s.build(st, &mut board);
        s
    }

    fn pot(&self, inv: [f32; 2]) -> f32 { self.spec.dead + inv[0] + inv[1] }

    fn build(&self, st: St, board: &mut Vec<u8>) -> Node {
        let a = st.actor; let o = 1 - a; let sp = &self.spec;
        let pot = self.pot(st.inv);
        let facing = st.inv[o] > st.inv[a] + 1e-6;
        let mut acts: Vec<Act> = Vec::new();
        let mut kids: Vec<Node> = Vec::new();
        let can_raise = st.raises < sp.max_raises && sp.stack > st.inv[o] + 1e-6;
        if facing { acts.push(Act::Fold); acts.push(Act::Call); } else { acts.push(Act::Check); }
        if can_raise {
            let to_call = (st.inv[o] - st.inv[a]).max(0.0);
            let min_to = st.inv[o] + st.last_raise.max(sp.min_bet);
            let mut tos: Vec<f32> = Vec::new();
            for &f in &sp.bet_fracs {
                let mut t = st.inv[o] + f * (pot + to_call);
                if t < min_to { t = min_to; }
                let pot_after = pot + (t - st.inv[a]);
                if t >= sp.stack - 1e-6 || sp.stack - t < sp.allin_snap * pot_after { t = sp.stack; }
                if t > st.inv[o] + 1e-6 { tos.push(t); }
            }
            tos.push(sp.stack);
            tos.sort_by(|x, y| x.partial_cmp(y).unwrap());
            tos.dedup_by(|x, y| (*x - *y).abs() < 1e-4);
            for t in tos { acts.push(Act::Bet(t)); }
        }
        for act in &acts {
            let mut ni = st.inv; let kid;
            match *act {
                Act::Fold => { kid = Node::Fold { folder: a, inv: st.inv }; }
                Act::Call => { ni[a] = st.inv[o]; kid = self.end_street(ni, board); }
                Act::Check => {
                    if st.n_act >= 1 { kid = self.end_street(ni, board); }
                    else { kid = self.build(St { inv: ni, actor: o, n_act: 1, raises: st.raises, last_raise: st.last_raise }, board); }
                }
                Act::Bet(t) => {
                    ni[a] = t;
                    let lr = t - st.inv[o];
                    kid = self.build(St { inv: ni, actor: o, n_act: st.n_act + 1, raises: st.raises + 1, last_raise: lr }, board);
                }
            }
            kids.push(kid);
        }
        let n = acts.len();
        Node::Act(ActNode { player: a, acts, kids, regret: vec![0.0; n * N], ssum: vec![0.0; n * N] })
    }

    fn end_street(&self, inv: [f32; 2], board: &mut Vec<u8>) -> Node {
        if board.len() == 5 { return self.showdown(inv[0], board); }
        let n_deck = (52 - board.len() - 4) as f32;
        let allin = inv[0] >= self.spec.stack - 1e-6 || inv[1] >= self.spec.stack - 1e-6;
        let mut cards = Vec::new(); let mut kids = Vec::new();
        for c in 0..52u8 {
            if board.contains(&c) { continue; }
            board.push(c);
            let kid = if allin { self.end_street(inv, board) }
                      else { self.build(St { inv, actor: 0, n_act: 0, raises: 0, last_raise: 0.0 }, board) };
            board.pop();
            cards.push(c); kids.push(kid);
        }
        Node::Chance(ChanceNode { cards, kids, n_deck })
    }

    fn showdown(&self, inv: f32, board: &[u8]) -> Node {
        let mut rank = vec![0u32; N];
        let mut ids: Vec<u16> = Vec::new();
        for h in 0..N {
            if self.holes[h].iter().any(|c| board.contains(c)) { continue; }
            rank[h] = (self.eval)(&self.holes[h], board);
            ids.push(h as u16);
        }
        ids.sort_by(|&x, &y| rank[y as usize].cmp(&rank[x as usize]));
        Node::Show(ShowNode { inv, worst_first: ids, rank })
    }
}

fn compat_sums(holes: &[[u8; 2]], r: &[f32], out_total: &mut f32, card: &mut [f32; 52]) {
    *out_total = 0.0; *card = [0.0; 52];
    for h in 0..N { let w = r[h]; if w == 0.0 { continue; } *out_total += w; card[holes[h][0] as usize] += w; card[holes[h][1] as usize] += w; }
}

fn fold_cfv(holes: &[[u8; 2]], r_opp: &[f32], value: f32, valid: &[bool]) -> Vec<f32> {
    let mut tot = 0.0; let mut card = [0.0f32; 52];
    compat_sums(holes, r_opp, &mut tot, &mut card);
    let mut out = vec![0.0; N];
    for h in 0..N { if !valid[h] { continue; }
        let c = tot - card[holes[h][0] as usize] - card[holes[h][1] as usize] + r_opp[h];
        out[h] = value * c; }
    out
}

fn show_cfv(holes: &[[u8; 2]], sn: &ShowNode, r_opp: &[f32], v: f32) -> Vec<f32> {
    let mut below = vec![0.0f32; N]; let mut above = vec![0.0f32; N];
    for pass in 0..2 {
        let mut tot = 0.0f32; let mut card = [0.0f32; 52];
        let ids: Vec<u16> = if pass == 0 { sn.worst_first.clone() } else { sn.worst_first.iter().rev().cloned().collect() };
        let mut i = 0;
        while i < ids.len() {
            let rk = sn.rank[ids[i] as usize]; let mut j = i;
            while j < ids.len() && sn.rank[ids[j] as usize] == rk { j += 1; }
            for k in i..j { let h = ids[k] as usize;
                let s = tot - card[holes[h][0] as usize] - card[holes[h][1] as usize];
                if pass == 0 { below[h] = s } else { above[h] = s } }
            for k in i..j { let h = ids[k] as usize; let w = r_opp[h]; tot += w;
                card[holes[h][0] as usize] += w; card[holes[h][1] as usize] += w; }
            i = j;
        }
    }
    let mut out = vec![0.0; N];
    for &h in &sn.worst_first { let h = h as usize; out[h] = v * (below[h] - above[h]); }
    out
}

fn rm_plus(regret: &[f32], n: usize, strat: &mut [f32]) {
    for h in 0..N {
        let mut s = 0.0; for a in 0..n { s += regret[a * N + h].max(0.0); }
        if s > 1e-12 { for a in 0..n { strat[a * N + h] = regret[a * N + h].max(0.0) / s; } }
        else { for a in 0..n { strat[a * N + h] = 1.0 / n as f32; } }
    }
}
fn avg_strat(ssum: &[f32], n: usize, strat: &mut [f32]) {
    for h in 0..N {
        let mut s = 0.0; for a in 0..n { s += ssum[a * N + h]; }
        if s > 1e-20 { for a in 0..n { strat[a * N + h] = ssum[a * N + h] / s; } }
        else { for a in 0..n { strat[a * N + h] = 1.0 / n as f32; } }
    }
}

impl<'a> Solver<'a> {
    fn cfr(&self, node: *mut Node, p: usize, reach: [&[f32]; 2], t: f32) -> Vec<f32> {
        let node = unsafe { &mut *node };
        let sp = &self.spec;
        match node {
            Node::Fold { folder, inv } => {
                let val = if *folder == p { -(sp.dead * 0.5 + inv[p]) } else { sp.dead * 0.5 + inv[1 - p] };
                fold_cfv(&self.holes, reach[1 - p], val, &self.valid_root)
            }
            Node::Show(sn) => show_cfv(&self.holes, sn, reach[1 - p], sp.dead * 0.5 + sn.inv),
            Node::Chance(cn) => {
                let mut out = vec![0.0f32; N];
                let n_deck = cn.n_deck;
                for (i, &c) in cn.cards.iter().enumerate() {
                    let mut r0 = reach[0].to_vec(); let mut r1 = reach[1].to_vec();
                    for h in 0..N { if self.holes[h][0] == c || self.holes[h][1] == c { r0[h] = 0.0; r1[h] = 0.0; } }
                    let kid: *mut Node = &mut cn.kids[i];
                    let v = self.cfr(kid, p, [&r0, &r1], t);
                    for h in 0..N { if !(self.holes[h][0] == c || self.holes[h][1] == c) { out[h] += v[h] / n_deck; } }
                }
                out
            }
            Node::Act(an) => {
                let n = an.acts.len();
                let mut strat = vec![0.0f32; n * N];
                rm_plus(&an.regret, n, &mut strat);
                if an.player == p {
                    let mut vals: Vec<Vec<f32>> = Vec::with_capacity(n);
                    for a in 0..n {
                        let mut rp = reach[p].to_vec();
                        for h in 0..N { rp[h] *= strat[a * N + h]; }
                        let kid: *mut Node = &mut an.kids[a];
                        let rr = if p == 0 { [&rp[..], reach[1]] } else { [reach[0], &rp[..]] };
                        vals.push(self.cfr(kid, p, rr, t));
                    }
                    let mut node_v = vec![0.0f32; N];
                    for a in 0..n { for h in 0..N { node_v[h] += strat[a * N + h] * vals[a][h]; } }
                    for a in 0..n { for h in 0..N {
                        let r = &mut an.regret[a * N + h];
                        *r = (*r + vals[a][h] - node_v[h]).max(0.0);
                        an.ssum[a * N + h] += t * reach[p][h] * strat[a * N + h];
                    } }
                    node_v
                } else {
                    let mut out = vec![0.0f32; N];
                    for a in 0..n {
                        let mut ro = reach[1 - p].to_vec();
                        for h in 0..N { ro[h] *= strat[a * N + h]; }
                        let kid: *mut Node = &mut an.kids[a];
                        let rr = if p == 0 { [reach[0], &ro[..]] } else { [&ro[..], reach[1]] };
                        let v = self.cfr(kid, p, rr, t);
                        for h in 0..N { out[h] += v[h]; }
                    }
                    out
                }
            }
        }
    }

    pub fn iterate(&mut self, n: u32) {
        for _ in 0..n {
            self.iters += 1; let t = self.iters as f32;
            for p in 0..2 {
                let r0 = self.spec.ranges[0].clone(); let r1 = self.spec.ranges[1].clone();
                let root: *mut Node = &mut self.root;
                let _ = self.cfr(root, p, [&r0, &r1], t);
            }
        }
    }

    fn br(&self, node: &Node, p: usize, r_opp: &[f32]) -> Vec<f32> {
        let sp = &self.spec;
        match node {
            Node::Fold { folder, inv } => {
                let val = if *folder == p { -(sp.dead * 0.5 + inv[p]) } else { sp.dead * 0.5 + inv[1 - p] };
                fold_cfv(&self.holes, r_opp, val, &self.valid_root)
            }
            Node::Show(sn) => show_cfv(&self.holes, sn, r_opp, sp.dead * 0.5 + sn.inv),
            Node::Chance(cn) => {
                let n_deck = cn.n_deck;
                let mut out = vec![0.0f32; N];
                for (i, &c) in cn.cards.iter().enumerate() {
                    let mut ro = r_opp.to_vec();
                    for h in 0..N { if self.holes[h][0] == c || self.holes[h][1] == c { ro[h] = 0.0; } }
                    let v = self.br(&cn.kids[i], p, &ro);
                    for h in 0..N { if !(self.holes[h][0] == c || self.holes[h][1] == c) { out[h] += v[h] / n_deck; } }
                }
                out
            }
            Node::Act(an) => {
                let n = an.acts.len();
                if an.player == p {
                    let mut best = vec![f32::NEG_INFINITY; N];
                    for a in 0..n { let v = self.br(&an.kids[a], p, r_opp);
                        for h in 0..N { if v[h] > best[h] { best[h] = v[h]; } } }
                    for h in 0..N { if best[h] == f32::NEG_INFINITY { best[h] = 0.0; } }
                    best
                } else {
                    let mut st = vec![0.0f32; n * N]; avg_strat(&an.ssum, n, &mut st);
                    let mut out = vec![0.0f32; N];
                    for a in 0..n {
                        let mut ro = r_opp.to_vec();
                        for h in 0..N { ro[h] *= st[a * N + h]; }
                        let v = self.br(&an.kids[a], p, &ro);
                        for h in 0..N { out[h] += v[h]; }
                    }
                    out
                }
            }
        }
    }

    pub fn exploitability(&self) -> f32 {
        let mut tot = 0.0;
        for p in 0..2 {
            let cf = self.br(&self.root, p, &self.spec.ranges[1 - p]);
            let mut z = 0.0f64; let mut v = 0.0f64;
            let mut tot_o = 0.0; let mut card = [0.0f32; 52];
            compat_sums(&self.holes, &self.spec.ranges[1 - p], &mut tot_o, &mut card);
            for h in 0..N { if !self.valid_root[h] { continue; }
                let w = self.spec.ranges[p][h] as f64; if w == 0.0 { continue; }
                let comp = tot_o - card[self.holes[h][0] as usize] - card[self.holes[h][1] as usize] + self.spec.ranges[1 - p][h];
                z += w * comp as f64; v += w * cf[h] as f64; }
            if z > 0.0 { tot += (v / z) as f32; }
        }
        tot * 0.5
    }

    pub fn root_strategy(&self, hand: [u8; 2]) -> Vec<(Act, f32)> {
        if let Node::Act(an) = &self.root {
            let n = an.acts.len(); let mut st = vec![0.0; n * N]; avg_strat(&an.ssum, n, &mut st);
            let h = index_of(hand);
            (0..n).map(|a| (an.acts[a], st[a * N + h])).collect()
        } else { vec![] }
    }
    pub fn n_root_actions(&self) -> usize { if let Node::Act(an) = &self.root { an.acts.len() } else { 0 } }

    // -----------------------------------------------------------------
    // Safety-bound primitives (root-level blend toward the blueprint).
    //
    // The raw vector solve has no safety bound. The 2026-10-06 measurement
    // showed it helps (LBR -376, H2H +1.25 chips/deal), but an
    // off-distribution opponent could still punish it. These methods let
    // `solve_root_bounded` clamp the ROOT to the largest convex blend with
    // the blueprint whose opponent-BR value is no worse than the pure
    // blueprint's. Every non-root node keeps the search continuation.
    //
    // This is the vector-path analog of `safe_solve_root_p0_strategy`
    // (sampled path). It is an in-sample heuristic, NOT the CFR-D gadget
    // (plan Task 4.4); the gadget remains follow-up.
    // -----------------------------------------------------------------

    /// The solved root average strategy as per-action, per-hand
    /// probabilities: `out[a][h] = P(play action a | hand h)`.
    /// `None` if the root is not a decision node.
    pub fn root_strategy_all(&self) -> Option<Vec<Vec<f32>>> {
        if let Node::Act(an) = &self.root {
            let n = an.acts.len();
            let mut st = vec![0.0f32; n * N];
            avg_strat(&an.ssum, n, &mut st);
            let mut out = Vec::with_capacity(n);
            for a in 0..n {
                out.push(st[a * N..(a + 1) * N].to_vec());
            }
            Some(out)
        } else {
            None
        }
    }

    /// Overwrite the root's average-strategy sums so the root plays
    /// exactly `probs[a][h]`. `avg_strat` normalizes, so the scale is
    /// irrelevant; 1e6 keeps the arithmetic clean. Only the root is
    /// touched.
    pub fn set_root_strategy(&mut self, probs: &[Vec<f32>]) {
        if let Node::Act(an) = &mut self.root {
            let n = an.acts.len();
            for a in 0..n.min(probs.len()) {
                for h in 0..N {
                    an.ssum[a * N + h] = probs[a][h].max(0.0) * 1e6;
                }
            }
        }
    }

    /// Best-response value in chips for the **opponent of the root
    /// actor**, against the tree's *current* strategy (root average
    /// strategy + all downstream search continuations). This is the
    /// security number the bound uses: how much a best-responding
    /// opponent wins given the strategy we would ship.
    ///
    /// Correct for EITHER root actor. (`p1_br_value` — the earlier name —
    /// was only right when the root actor was OOP; `spec_from_state` sets
    /// `root_actor` from the live actor, so IP-rooted solves need the
    /// `p == 0` term instead.)
    ///
    /// Mirrors the appropriate term of `exploitability()`:
    /// `br(root, opp, ranges[1-opp])`, weighted by `ranges[opp]` with its
    /// compatibility against `ranges[1-opp]`.
    pub fn opp_br_value(&self) -> f32 {
        let root_actor = match &self.root {
            Node::Act(an) => an.player,
            _ => return 0.0,
        };
        let opp = 1 - root_actor;
        let opp_range = &self.spec.ranges[1 - opp];
        let cf = self.br(&self.root, opp, opp_range);
        let mut z = 0.0f64;
        let mut v = 0.0f64;
        let mut tot_o = 0.0f32;
        let mut card = [0.0f32; 52];
        compat_sums(&self.holes, opp_range, &mut tot_o, &mut card);
        let my_range = &self.spec.ranges[opp];
        for h in 0..N {
            if !self.valid_root[h] {
                continue;
            }
            let w = my_range[h] as f64;
            if w == 0.0 {
                continue;
            }
            let comp = tot_o
                - card[self.holes[h][0] as usize]
                - card[self.holes[h][1] as usize]
                + opp_range[h];
            z += w * comp as f64;
            v += w * cf[h] as f64;
        }
        if z > 0.0 {
            (v / z) as f32
        } else {
            0.0
        }
    }

    /// Backwards-compatible alias (P1-only). Prefer `opp_br_value`.
    pub fn p1_br_value(&self) -> f32 {
        self.opp_br_value()
    }

    /// The blueprint's strategy at the root, per action per hand, mapped
    /// onto the vector tree's root actions. `st` supplies the root
    /// scalars `action_bucket` needs; it must be the same state
    /// `spec_from_state` was built from.
    pub fn blueprint_root_strategy(
        &self,
        abs: &dyn pkr_contracts::AbstractionBuilder,
        tbl: &pkr_cfr::table::CompactRegretTable,
        history_sig: &[u8],
        street: u8,
        st: &pkr_core::state::GameState,
    ) -> Option<Vec<Vec<f32>>> {
        let an = match &self.root {
            Node::Act(an) => an,
            _ => return None,
        };
        let n = an.acts.len();
        let actor = an.player;
        let a_stacks = st.stacks[actor];
        let a_street = st.street_bets[actor];
        let o_street = st.street_bets[1 - actor];
        let pot = st.pot;
        let bucket_of: Vec<usize> = an
            .acts
            .iter()
            .map(|act| {
                let kind = match act {
                    Act::Fold => pkr_core::state::ActionKind::Fold,
                    Act::Check => pkr_core::state::ActionKind::Check,
                    Act::Call => pkr_core::state::ActionKind::Call,
                    Act::Bet(t) => pkr_core::state::ActionKind::Bet(*t),
                };
                pkr_core::abstraction::action_bucket(&kind, a_stacks, a_street, o_street, pot)
                    as usize
            })
            .collect();
        let mut out = vec![vec![0.0f32; N]; n];
        for h in 0..N {
            if !self.valid_root[h] {
                continue;
            }
            let hole = self.holes[h];
            let hash = abs.get_infoset_hash(&hole, &self.spec.board, history_sig, street);
            let mut strat = [0.0f32; 6];
            tbl.get_average_strategy_into(hash, &mut strat);
            let mut bucket_count = [0u8; 6];
            for &b in &bucket_of {
                if b < 6 {
                    bucket_count[b] = bucket_count[b].saturating_add(1);
                }
            }
            for a in 0..n {
                let b = bucket_of[a];
                if b < 6 && bucket_count[b] > 0 {
                    out[a][h] = strat[b] / bucket_count[b] as f32;
                }
            }
        }
        Some(out)
    }

    /// Convex blend of two root strategies: `alpha * a + (1 - alpha) * b`.
    pub fn blend_root(a: &[Vec<f32>], b: &[Vec<f32>], alpha: f32) -> Vec<Vec<f32>> {
        let n = a.len().min(b.len());
        (0..n)
            .map(|ai| {
                (0..N)
                    .map(|h| alpha * a[ai][h] + (1.0 - alpha) * b[ai][h])
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_contracts::Evaluator;
    use pkr_eval::NlheEvaluator;

    fn eval_fn(h: &[u8; 2], b: &[u8]) -> u32 { NlheEvaluator.evaluate_hand(h, b) }

    fn river_spec() -> Spec {
        // Board: As Kd 7c 2h 9d (suit*13+rank).
        let board = vec![12u8, 13 + 11, 39 + 5, 13 + 0, 26 + 7];
        // Uniform-ish ranges over board-free hands.
        let holes = hole_table();
        let mut r = vec![0.0f32; N];
        for h in 0..N {
            if !holes[h].iter().any(|c| board.contains(c)) { r[h] = 1.0; }
        }
        Spec { board, dead: 100.0, stack: 150.0, root_actor: 0, root_inv: [0.0, 0.0],
               bet_fracs: vec![0.5, 1.0], max_raises: 2, min_bet: 2.0, allin_snap: 0.2,
               ranges: [r.clone(), r] }
    }

    #[test]
    fn river_exploitability_converges() {
        let s = river_spec();
        let mut solver = Solver::new(s, &eval_fn);
        assert!(solver.n_root_actions() >= 2, "river root must have actions");
        let e1 = { solver.iterate(10); solver.exploitability() };
        let e2 = { solver.iterate(190); solver.exploitability() };
        eprintln!("river exploitability: 10 iters={e1:.4} 200 iters={e2:.4} chips");
        assert!(e2.is_finite() && e2 >= 0.0);
        assert!(e2 < e1.max(1e-3), "exploitability must fall: {e1} -> {e2}");
        assert!(e2 < 1.0, "200-iter river solve should be < 1 chip exploitable, got {e2}");
    }

    #[test]
    fn root_strategy_is_a_distribution() {
        let s = river_spec();
        let mut solver = Solver::new(s, &eval_fn);
        solver.iterate(50);
        // Any board-free hand.
        let holes = hole_table();
        let hand = holes.iter().find(|h| !river_spec().board.contains(&h[0]) && !river_spec().board.contains(&h[1])).copied().unwrap();
        let strat = solver.root_strategy(hand);
        assert!(!strat.is_empty());
        let sum: f32 = strat.iter().map(|(_, p)| *p).sum();
        assert!((sum - 1.0).abs() < 1e-3, "root strategy must sum to 1, got {sum}");
    }

    #[test]
    fn root_strategy_all_sums_to_one_and_set_roundtrips() {
        // Trivial evaluator: every hand ties. Keeps the test independent
        // of pkr-eval and fast.
        fn trivial_eval(_h: &[u8; 2], _b: &[u8]) -> u32 { 0 }
        let board: Vec<u8> = vec![20, 25, 30, 35, 44];
        let mut r = vec![0.0f32; N];
        for h in 0..N {
            let hh = hole_table()[h];
            if !hh.iter().any(|c| board.contains(c)) {
                r[h] = 1.0;
            }
        }
        let spec = Spec {
            board,
            dead: 100.0,
            stack: 150.0,
            root_actor: 0,
            root_inv: [0.0, 0.0],
            bet_fracs: vec![0.5, 1.0],
            max_raises: 2,
            min_bet: 2.0,
            allin_snap: 0.2,
            ranges: [r.clone(), r],
        };
        let mut s = Solver::new(spec, &trivial_eval);
        s.iterate(20);

        let full = s.root_strategy_all().expect("root is a decision node");
        for h in 0..N {
            if !s.valid_root[h] { continue; }
            let sum: f32 = full.iter().map(|a| a[h]).sum();
            assert!((sum - 1.0).abs() < 1e-3, "hand {h} sums to {sum}");
        }

        // set_root_strategy then re-read round-trips.
        let n_actions = full.len();
        let forced: Vec<Vec<f32>> = (0..n_actions)
            .map(|a| {
                let mut v = vec![0.0f32; N];
                for h in 0..N {
                    v[h] = if a == 0 { 1.0 } else { 0.0 };
                }
                v
            })
            .collect();
        s.set_root_strategy(&forced);
        let after = s.root_strategy_all().expect("still a decision node");
        for h in 0..N {
            if !s.valid_root[h] { continue; }
            assert!((after[0][h] - 1.0).abs() < 1e-3, "h={h} after[0]={}", after[0][h]);
            for a in 1..n_actions {
                assert!(after[a][h].abs() < 1e-3, "h={h} a={a} = {}", after[a][h]);
            }
        }

        assert!(s.opp_br_value().is_finite());

        let blended = Solver::blend_root(&full, &full, 0.3);
        for a in 0..n_actions {
            for h in 0..N {
                assert!((blended[a][h] - full[a][h]).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn hand_index_roundtrip() {
        let holes = hole_table();
        for h in 0..N {
            assert_eq!(index_of(holes[h]), h);
            assert!(holes[h][0] > holes[h][1], "hole must be sorted hi,lo");
        }
    }
}

/// Build a `Spec` from a real `GameState` at our decision, plus the two
/// players' hole-card ranges (indexed by `index_of`). Postflop, solver
/// player 0 = OOP (acts first = `1 - dealer`), player 1 = IP.
///
/// `ranges` is `[oop_range, ip_range]` already ordered to solver seats.
pub fn spec_from_state(
    st: &pkr_core::state::GameState,
    ranges: [Vec<f32>; 2],
    bet_fracs: Vec<f32>,
    max_raises: u8,
) -> Spec {
    let board: Vec<u8> = st.board[..st.board_len as usize].to_vec();
    let dead = (st.pot - st.street_bets[0] - st.street_bets[1]).max(0.0);
    let stack = (st.stacks[0] + st.street_bets[0])
        .min(st.stacks[1] + st.street_bets[1]);
    // OOP is the non-dealer postflop.
    let oop = 1 - st.dealer;
    let root_actor = if st.actor == oop { 0 } else { 1 };
    let root_inv = [st.street_bets[oop], st.street_bets[1 - oop]];
    Spec {
        board,
        dead,
        stack,
        root_actor: root_actor as u8,
        root_inv,
        bet_fracs,
        max_raises,
        min_bet: 2.0,
        allin_snap: 0.2,
        ranges,
    }
}
