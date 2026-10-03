//! Leduc hold'em harness driving the PRODUCTION CFR path (report §A).
//!
//! The Kuhn harness uses f32 vanilla CFR and does not touch the i64
//! `CompactRegretTable`, `flush_cpu_batch`, `apply_strategy_batch`, the
//! sorted batch dedup, epsilon exploration, or own-reach averaging. This
//! drives all of them on a small game whose exact exploitability is
//! computable, so a silent regression in `dcfr.rs`/`table.rs`/`traversal.rs`
//! shows up as rising exploitability.
//!
//! Run one config per process (`TrainConfig` is a per-process singleton):
//!   PKR_RM_PLUS=0 PKR_AVG_POWER=2 target/release/pkr-leduc-check 2000000 256 1 500000

use pkr_cfr::config::TrainConfig;
use pkr_cfr::gpu::BatchItem;
use pkr_cfr::metrics::LocalMetrics;
use pkr_cfr::table::{CompactRegretTable, StrategyOp};
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};

const K: usize = 6;

#[derive(Clone, Copy)]
struct S {
    c: [u8; 2],
    board: u8,
    round: u8,
    to: u8,
    contrib: [i32; 2],
    raises: u8,
    acts: u8,
    hist: u64,
    term: bool,
    folded: i8,
}

fn init(c0: u8, c1: u8, b: u8) -> S {
    S { c: [c0, c1], board: b, round: 0, to: 0, contrib: [1, 1], raises: 0,
        acts: 0, hist: 1, term: false, folded: -1 }
}
fn facing(s: &S) -> bool { s.contrib[0] != s.contrib[1] }
fn legal(s: &S) -> [bool; K] {
    let mut l = [false; K];
    if facing(s) { l[0] = true; }
    l[1] = true;
    if s.raises < 2 { l[2] = true; }
    l
}
fn end_round(n: &mut S) {
    if n.round == 0 { n.round = 1; n.to = 0; n.raises = 0; n.acts = 0; n.hist = n.hist * 4 + 3; }
    else { n.term = true; }
}
fn apply(s: &S, a: usize) -> S {
    let mut n = *s;
    n.hist = n.hist * 4 + a as u64;
    let (me, opp) = (s.to as usize, 1 - s.to as usize);
    match a {
        0 => { n.term = true; n.folded = me as i8; }
        1 => {
            if facing(s) { n.contrib[me] = n.contrib[opp]; end_round(&mut n); }
            else { n.acts += 1; if n.acts >= 2 { end_round(&mut n); } else { n.to = opp as u8; } }
        }
        _ => {
            let sz = if s.round == 0 { 2 } else { 4 };
            n.contrib[me] = n.contrib[opp].max(n.contrib[me]) + sz;
            n.raises += 1; n.acts += 1; n.to = opp as u8;
        }
    }
    n
}
fn payoff(s: &S, p: usize) -> f32 {
    let v0: i32 = if s.folded >= 0 {
        if s.folded == 0 { -s.contrib[0] } else { s.contrib[1] }
    } else {
        let sc = |c: u8| -> i32 { if c == s.board { 100 } else { c as i32 } };
        let (a, b) = (sc(s.c[0]), sc(s.c[1]));
        if a > b { s.contrib[1] } else if a < b { -s.contrib[0] } else { 0 }
    };
    if p == 0 { v0 as f32 } else { -(v0 as f32) }
}
fn key(s: &S) -> u64 {
    let me = s.to as usize;
    let vis = if s.round == 1 { s.board as u64 } else { 7 };
    let mut h: u64 = 0xcbf29ce484222325;
    for x in [s.c[me] as u64, vis, s.hist, me as u64] {
        h ^= x; h = h.wrapping_mul(0x100000001b3); h ^= h >> 29;
    }
    h
}
fn avg_weight(t: u32, p: f32) -> f32 {
    if p == 0.0 { 1.0 } else if p == 1.0 { t as f32 }
    else if p == 2.0 { (t as f32) * (t as f32) } else { (t as f32).powf(p) }
}
fn sample_eps(st: &[f32; K], l: &[bool; K], eps: f32, r: f32) -> usize {
    let n = l.iter().filter(|&&c| c).count() as f32;
    let (mut cdf, mut last) = (0.0, 0);
    for a in 0..K {
        if !l[a] { continue; }
        last = a;
        cdf += eps / n + (1.0 - eps) * st[a];
        if r < cdf { return a; }
    }
    last
}

#[allow(clippy::too_many_arguments)]
fn trav(s: &S, trv: usize, reach: f32, t: u32, table: &CompactRegretTable,
        rng: &mut SmallRng, batch: &mut Vec<BatchItem>, sbatch: &mut Vec<StrategyOp>,
        m: &mut LocalMetrics) -> f32 {
    if s.term { return payoff(s, trv); }
    let cfg = TrainConfig::global();
    let l = legal(s);
    let actor = s.to as usize;
    let h = key(s);
    let mut strat = [0f32; K];
    let (idx, is_trv) = if actor == trv {
        (table.get_strategy_and_idx(h, &mut strat, m), true)
    } else if !cfg.avg_at_traverser {
        (table.get_strategy_and_idx(h, &mut strat, m), false)
    } else { table.get_strategy_into(h, &mut strat); (0, false) };
    let tot: f32 = (0..K).filter(|&a| l[a]).map(|a| strat[a]).sum();
    let nl = l.iter().filter(|&&x| x).count() as f32;
    for a in 0..K {
        strat[a] = if !l[a] { 0.0 } else if tot > 0.0 { strat[a] / tot } else { 1.0 / nl };
    }
    let w = avg_weight(t, cfg.avg_power);
    if cfg.avg_at_traverser {
        if is_trv { for a in 0..K { if strat[a] > 0.0 {
            sbatch.push(StrategyOp { index: idx as u32, action: a as u8, prob: strat[a] * reach * w }); } } }
    } else if !is_trv {
        for a in 0..K { if strat[a] > 0.0 {
            sbatch.push(StrategyOp { index: idx as u32, action: a as u8, prob: strat[a] * w }); } }
    }
    if is_trv {
        let mut v = [f32::NAN; K];
        for a in 0..K { if l[a] {
            v[a] = trav(&apply(s, a), trv, reach * strat[a], t, table, rng, batch, sbatch, m); } }
        let vs: f32 = (0..K).filter(|&a| l[a]).map(|a| strat[a] * v[a]).sum();
        for a in 0..K { if !l[a] { continue; }
            let mut d = v[a] - vs;
            if cfg.linear_cfr { d = (d as f64 * (t as f64 / 1e6)) as f32; }
            batch.push(BatchItem { index: idx as u32, action: a as u32, iteration: t, delta: d });
        }
        vs
    } else {
        let a = sample_eps(&strat, &l, cfg.explore_epsilon, rng.random::<f32>());
        trav(&apply(s, a), trv, reach, t, table, rng, batch, sbatch, m)
    }
}

fn policy(table: &CompactRegretTable, s: &S) -> [f32; K] {
    let mut p = [0f32; K];
    table.get_average_strategy_into(key(s), &mut p);
    let l = legal(s);
    let tot: f32 = (0..K).filter(|&a| l[a]).map(|a| p[a]).sum();
    let nl = l.iter().filter(|&&x| x).count() as f32;
    for a in 0..K { p[a] = if !l[a] { 0.0 } else if tot > 1e-9 { p[a] / tot } else { 1.0 / nl }; }
    p
}
fn br(p: usize, states: Vec<(S, f64)>, table: &CompactRegretTable) -> f64 {
    if states.is_empty() { return 0.0; }
    if states[0].0.round == 1 && !states[0].0.term {
        let mut g: [Vec<(S, f64)>; 3] = [vec![], vec![], vec![]];
        for st in &states { g[st.0.board as usize].push(*st); }
        if g.iter().filter(|x| !x.is_empty()).count() > 1 {
            return g.into_iter().map(|x| br(p, x, table)).sum();
        }
    }
    if states[0].0.term { return states.iter().map(|(s, w)| *w * payoff(s, p) as f64).sum(); }
    let l = legal(&states[0].0);
    if states[0].0.to as usize == p {
        let mut best = f64::NEG_INFINITY;
        for a in 0..K { if !l[a] { continue; }
            best = best.max(br(p, states.iter().map(|(s, w)| (apply(s, a), *w)).collect(), table)); }
        best
    } else {
        let mut tot = 0.0;
        for a in 0..K { if !l[a] { continue; }
            let ch: Vec<(S, f64)> = states.iter().filter_map(|(s, w)| {
                let pr = policy(table, s)[a] as f64;
                if pr > 0.0 { Some((apply(s, a), *w * pr)) } else { None } }).collect();
            tot += br(p, ch, table); }
        tot
    }
}
fn exploitability(table: &CompactRegretTable) -> f64 {
    let mut deals = vec![];
    for c0 in 0..6u8 { for c1 in 0..6u8 { for b in 0..6u8 {
        if c0 == c1 || c0 == b || c1 == b { continue; }
        deals.push(init(c0 / 2, c1 / 2, b / 2)); } } }
    let w = 1.0 / deals.len() as f64;
    let mut sum = 0.0;
    for p in 0..2 {
        let mut g: [Vec<(S, f64)>; 3] = [vec![], vec![], vec![]];
        for d in &deals { g[d.c[p] as usize].push((*d, w)); }
        sum += g.into_iter().map(|x| br(p, x, table)).sum::<f64>();
    }
    sum / 2.0
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let iters: u64 = a.get(1).and_then(|x| x.parse().ok()).unwrap_or(1_000_000);
    let sync: u64 = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(256);
    let seed: u64 = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(1);
    let every: u64 = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(iters);
    let table = CompactRegretTable::with_capacity(200_000);
    let mut rng = SmallRng::seed_from_u64(seed);
    let (mut batch, mut sbatch, mut m) = (Vec::new(), Vec::new(), LocalMetrics::default());
    let (mut t, mut done) = (0u32, 0u64);
    while done < iters {
        let n = sync.min(iters - done);
        for _ in 0..n {
            t += 1;
            let mut d = [0u8, 0, 1, 1, 2, 2];
            for i in 0..3 { let j = i + rng.random_range(0..(6 - i)); d.swap(i, j); }
            for trv in 0..2 {
                trav(&init(d[0], d[1], d[2]), trv, 1.0, t, &table, &mut rng, &mut batch, &mut sbatch, &mut m);
            }
        }
        table.apply_strategy_batch(&mut sbatch);
        table.flush_cpu_batch(&mut batch);
        sbatch.clear(); batch.clear();
        let prev = done; done += n;
        if done / every != prev / every || done == iters {
            println!("iter={} expl_mchips={:.2} infosets={}", done, exploitability(&table) * 1000.0, table.len());
        }
    }
}
