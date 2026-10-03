I'll start by checking how to read the uploaded file.Reconstructing the repo from the dump and checking whether a Rust toolchain can be installed here, so I can compile and test my proposed fixes instead of drafting blind.The workspace compiles here with Rust 1.91, so I can run the tests and verify claims by experiment. Checking the baseline test status first.While that runs, I'm reviewing the runtime path (translation, session, subgame) that the bot actually plays with.I couldn't save a file because I ran out of tool calls, so the full report is below in markdown. Treat it as a reviewed diagnosis plus draft patches. Only the Leduc harness (section 5, §A) was compiled and run; the other patches are uncompiled.

# pkr-sota: competitiveness review (M1 Mac mini, 16 GB)

## 0. Bottom line

- **Your own docs grade the bot "weak-bot, not competitive."** `status.md` puts superhuman HUNL bots near 50 mbb and competent ones under 500. Your best readings are 1,222 mbb at 40k deals and still falling. I do not trust any of them as a measure of real-game strength (§3).
- **Memory and compute are not the constraint.** Your measured throughput is ~10.2k it/s, so 200M iterations is about 5.5 h. Per-infoset storage is 96 B (`table.rs`: regret `RM_STRIDE=6` i64 plus strategy sum 6 f64), while you use about 2% of a 60M-slot table.
- **The binding problems are:**
  - **Measurement:** the abstract-game best-response numbers are not bounds on real exploitability.
  - **Real-game robustness:** the translation code exists but nothing calls it.
  - **Abstraction fidelity:** the abstraction is coarse.
  - **Validation:** the production CFR path was never checked on a game with known Nash.
- **What I did not find:** any recorded held-out, LBR or V3-run result in the docs.
- **Realistic target:** a bot with a low, measured real-game LBR that stays robust to off-tree bet sizes. Matching a full-tree Nash solver on all of HUNL is not achievable on this hardware.

## 1. Scope and limits

- I reconstructed the repo from `dump.txt` (221 files). With Rust 1.91 installed via apt it compiles (`cargo check -p pkr-core -p pkr-cfr -p pkr-exploit`), and `pkr-core` tests pass (78/78). The two reviews inside your dump say "no toolchain"; that no longer holds.
- The sandbox has 1 CPU and 3 GB of RAM, so I could not train NLHE. I ran a Leduc proxy for the update rule only; it says nothing about the abstraction.
- **I read closely:** `status.md`, the handoffs, both in-repo review reports, the F4, v36, v38, v40, v42 and v43 docs, `config.rs`, `traversal.rs`, `dcfr.rs`, the `table.rs` flush, the `get_infoset_hash` hashing, V3 signatures in `state.rs`, `action_bucket`, `best_response.rs`, `translate_live.rs`, `session.rs`, `bot_loop.rs`, `blend_p0_strategy`, `safe_solve` and the trainer scheduler.
- **I did not read:** `precompute.rs`, the evaluators, `pkr-export` writer and reader, the scripted bots in `pkr-fuzz`, the `range_tracker` and subgame solver internals, and most of the trainer `main.rs`. The `tests/` directories are not in the dump.
- **The Leduc matrix is incomplete.** Only 3 of 12 configs finished (§2).

## 2. The dump is newer than the reviews inside it

Several items in `docs/reviews/*` are already fixed in the code. Use the code, not those reports.

| In-repo review says | Current code |
|---|---|
| V3 size-aware key is off and never run | `sig_v3_size_aware()` is **default ON** (`PKR_SIG_V3=0` opts out), with half-octave pot class and previous-aggressor bit. I found **no doc reporting a V3 training run**, so it is unmeasured. |
| Need held-out BR | `heldout_exploitability` exists, and the trainer has an `--eval-mode heldout` option. No readings are recorded. |
| Need LBR | `lbr.rs` and `lbr_eval.rs` exist. `status.md` still lists LBR as open and no results are recorded. |
| `blend_p0_strategy` bug | Fixed (`normalize_or_uniform`). |
| Momentum plane wastes memory | Removed (`RM_FIELDS=1`). |
| Linear CFR / anneal ε | Implemented behind flags. |
| avg_power | Default is now 1.0. |
| Pseudo-harmonic translation | Implemented in `translate_live.rs`, but see finding F2. |

`docs/status.md` (dated 09-30) is stale and still says "no bot binary" and "nothing has played."

## 3. Findings, ranked by impact on competitiveness

### F1. The exploitability numbers are not bounds on real exploitability
**Verified in code** (`best_response.rs`): the BR policy is a map keyed by the abstract `infoset_hash`, and it only chooses in-tree actions.

- In-sample fitting inflates the reading. Your own data shows it: one checkpoint reads 3796 mbb at 5k deals, 1707 at 20k and 1222 at 40k.
- Restricting the BR to bucket-level infosets and in-tree actions deflates it, because a real opponent sees exact cards and can bet any size.
- The docs and handoff say every number is an "upper bound." That is only true of the *abstract-game* exploitability. Relative to real exploitability the sign is unknown.
- Consequences:
  - Conclusions made with the in-sample metric are not safe. These include "30M is the sweet spot," the plateau-stop at 5 evals, the 160M "spike," and the abstraction "equivalent" results (F4 and F5). The 100M-vs-30M v38 comparison and the 160M spike were also measured before the F1 estimator fix.
  - Plateau-stop will systematically cut bigger-table runs.
- **Fix:** score checkpoints with held-out BR plus LBR with off-tree sizes. Save distinct checkpoints per eval point (`train.ckpt` is overwritten today). Set `--stop-on-plateau 0` until that exists.

### F2. Translation is not wired into anything
**Verified by grep:** `translate_bet` and `pseudo_harmonic_prob_lower` have no callers outside their own module and doc comments.

- `bot_loop.rs` and `RuntimeSession` apply the concrete action and hash it. The V3 key then takes the action bucket from hard thresholds (0.6 and 1.2 in `action_bucket`) and the pot class from real chips.
- The arena result (+210 bb/100, 99.9% blueprint hit rate) probably says nothing about off-tree play. I did not open the scripted bots, but if they pick from `legal_actions_into`, they only choose in-tree sizes.
- Any opponent using 0.33×, 0.75×, 1.5× or 3× pot is mapped crudely.
- The action space is only `{0.5, 1, 2}× + jam`, six buckets. Sizing is raise-above-call over the pre-call pot, which is smaller than the solver "pot raise" convention.
- **Fix:** §5, §B.

### F3. The production CFR path has never been validated on a game with known Nash
The only exact-Nash harness is Kuhn (`pkr-testgames/src/kuhn.rs`). It is full-width, uses f32 vanilla CFR, and calls only `update_regret_full`. It does not exercise the i64 table, sampled traversal, sorted batch flush, ε-exploration or own-reach averaging.

I built a Leduc harness (§5, §A) that drives the production `CompactRegretTable`, `flush_cpu_batch` and `apply_strategy_batch`. It mirrors `traverse()`'s sampling, averaging, ε-mixing and the `TrainConfig` weights, and computes exact exploitability.

Results from the 3 configs that finished (2M iterations, 3 seeds, exploitability in milli-chips per game):

| config | seeds | mean |
|---|---|---|
| base (RM+, p=1, own-reach site, ε=0.01, sync 256) | 21.02 / 19.90 / 22.91 | **21.28** |
| base, sync 2048 | 20.34 / 19.53 / 21.17 | 20.35 |
| RM+ floor off | 19.23 / 20.75 / 20.21 | 20.06 |

- The production machinery converges, and the staleness from batching at 2048 and the RM+ floor make no visible difference here.
- The avg_power, averaging-site, ε and linear-CFR configs did not finish, so I make no claims about them. The run does not contradict your docs' conclusion that these knobs are not the bottleneck.
- Limits: Leduc has 288 infosets, and the BR code has no independent check beyond converging toward zero.

### F4. The subgame +2.40 chips/deal evidence is narrow
- The test deals are a forced prelude (preflop SB-call/BB-check, flop and turn check-check, fixed runout), then river play. That is a check-check-check river spot only.
- Subgame-P0 scored +2.07 chips/deal against blueprint-P0's −0.33, and 16,288 of 20,000 deals diverged from the blueprint. The doc excerpt I read does not state the opponent policy, and the test file is not in the dump.
- A concrete-card solver exploiting a coarse bucketed blueprint is expected. That is not evidence of lower exploitability.
- `safe_solve`'s own comment says its safety check is in-sample and "not the theorem-backed" gadget.
- Turn solving is negative or flat (−0.67 at 10 iterations, −0.16 at 50).
- **Action:** re-measure against an independent opponent (a different-seed blueprint or LBR), not the blueprint itself.

### F5. Abstraction fidelity
- Flop and turn keys are k=200 buckets from a 2D (EHS, EHS²) fit. The tables are `u8`, so k ≤ 255.
- The F4 doc records a within-bucket equity std of **0.044** against a target below 0.01.
- Preflop is fit on Monte Carlo EHS with 1000 samples. The shipped "rich 6D" preflop probably mitigates this, but I did not inspect the tables. Exact preflop needs only 169 classes.
- River key is `(evaluate_hand >> 15) << 8 | board_bucket`, an absolute-rank tier. I did not measure its coarseness.
- The F4 and k=250 negative results came from the biased metric (F1) and from a run with drifted config. They do not show that finer abstraction has no headroom.

### F6. Smaller verified issues
- **Iteration counter is `AtomicU32`.** With `fetch_add(n as u32)` and no guard, it wraps at 4.29B iterations (about 4.9 days at 10.2k it/s). A wrap corrupts the averaging weights.
- **ε-exploration** is uncorrected (bias of order ε). `PKR_ANNEAL_EPS` exists.
- **Averaging at the traverser's nodes** weights by own reach on top of opponent-reach visit frequency. Theory prefers the opponent site. Your A/B was within 1σ on the biased metric, so re-test it under held-out scoring.
- **Doc drift:** `status.md` is out of date.

## 4. Numbers from your docs (all carry the F1 caveat)

| Fact | Value |
|---|---|
| Throughput | 10,228 it/s (v36, sync 2048, 60M slots) |
| Iterations per hour / 200M | 36.7M / about 5.5 h |
| Memory per infoset | 96 B plus map overhead (unmeasured) |
| RSS | ~700 MB (v36); 2.4 GB (v42 doc) |
| v42 curve @5k deals | 3313 @3M → 3780 @18M |
| Same checkpoint @5k / 20k / 40k deals | 3796 / 1707 / 1222 |
| Seed SD | about 40–90 mbb |
| avg_power 1 vs 2 | −81…−127 mbb at paired points (2 seeds, handoff) |
| Capacity 5M → 60M | −56 mbb pooled (z=−1.11) |
| Potential feature (F4) | −11 mbb, SE about 192 (equivalent) |
| Arena vs scripted bots | +210 bb/100 (5k hands) |

## 5. Code solutions

### §A. Leduc harness driving the production table (compiled, run)
Add `crates/pkr-leduc-check` to the workspace members.

`Cargo.toml`:
```toml
[package]
name = "pkr-leduc-check"
version.workspace = true
edition.workspace = true
[dependencies]
pkr-cfr = { workspace = true }
rand = { workspace = true }
```

`src/main.rs`:
```rust
use pkr_cfr::config::TrainConfig;
use pkr_cfr::gpu::BatchItem;
use pkr_cfr::metrics::LocalMetrics;
use pkr_cfr::table::{CompactRegretTable, StrategyOp};
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};

const K: usize = 6;

#[derive(Clone, Copy)]
struct S { c: [u8; 2], board: u8, round: u8, to: u8, contrib: [i32; 2],
           raises: u8, acts: u8, hist: u64, term: bool, folded: i8 }

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
```
Run it once per config, because `TrainConfig` is read once per process:
```bash
cargo build --release -p pkr-leduc-check
PKR_RM_PLUS=0 PKR_AVG_POWER=2 target/release/pkr-leduc-check 2000000 256 1 500000
```
Add it to CI as a regression gate: after any change to `dcfr.rs`, `table.rs` or `traversal.rs`, exploitability must stay near 20 mchips/g.

### §B. Wire translation into the live bot (uncompiled draft)
The bot keeps the real chips state for payments and an abstract mirror state to hash and track. The observed bet is rewritten to an in-tree action before it is applied to the mirror and the `RangeTracker`.

```rust
// pkr-runtime/src/translate_live.rs
use pkr_core::abstraction::action_bucket;
use pkr_core::state::{Action, ActionKind, GameState};

/// Map an observed bet (total street commitment) to an in-tree concrete action.
pub fn translate_observed_bet(
    state: &GameState,
    observed_total: f32,
    rng: &mut impl rand::Rng,
) -> Option<Action> {
    let a = state.actor;
    let committed = state.street_bets[a].max(state.street_bets[1 - a]);
    let pot = state.pot.max(1.2);
    let x = (observed_total - committed).max(0.0) / pot;
    let jam_frac = (state.stacks[a] + state.street_bets[a] - committed) / pot;
    let want = translate_bet(x, jam_frac, rng) as i32;

    let mut buf = [Action { player: a, kind: ActionKind::Fold }; 8];
    let n = state.legal_actions_into(&mut buf);
    let bucket = |k: &ActionKind| action_bucket(
        k, state.stacks[a], state.street_bets[a], state.street_bets[1 - a], state.pot) as i32;
    buf[..n].iter()
        .filter(|c| matches!(c.kind, ActionKind::Bet(_)))
        .min_by_key(|c| (bucket(&c.kind) - want).abs())
        .copied()
}
```
Two details matter:
1. Sample once per decision and feed the same mapped action to the hash, the tracker and any subgame root.
2. Measure with LBR using extra off-tree sizes (`[0.33, 0.75, 1.5, 3.0]`) before and after.

### §C. Exact preflop classes (uncompiled)
Card ids are `suit*13 + rank`. This is lossless, needs no table, and is not subject to Monte Carlo noise.

```rust
/// 169 strategically distinct preflop classes: 0..12 pairs, 13..90 suited, 91..168 offsuit.
#[inline]
pub fn preflop_class(hole: &[u8]) -> u8 {
    let (r0, s0) = (hole[0] % 13, hole[0] / 13);
    let (r1, s1) = (hole[1] % 13, hole[1] / 13);
    let (hi, lo) = if r0 >= r1 { (r0, r1) } else { (r1, r0) };
    if hi == lo { return hi; }
    let tri = hi * (hi - 1) / 2 + lo;
    if s0 == s1 { 13 + tri } else { 91 + tri }
}
```
Use it in the `0 =>` arm of `KMeansAbstraction::get_infoset_hash` behind a flag (for example `PKR_PREFLOP_EXACT`). It changes every hash. So bump the fingerprint (a new `centroid_feature_v` value) and regenerate the golden hash tests. Check the A/B with held-out BR and LBR.

### §D. River key from exact equity percentile (hypothesis; A/B before adopting)
A river hole+board has 990 opponent hands. At about 32 ns per evaluation (your `table_eval` bench) that is roughly 30 µs per `(hole, board)`. With 2 holes per deal and a per-thread cache, it is about 60 µs per iteration, an estimate of ~8% of your per-thread iteration time.

```rust
// pkr-abstraction/src/lib.rs
const RIVER_PCT_BUCKETS: f32 = 48.0;
thread_local! {
    static PCT_CACHE: std::cell::RefCell<[(u64, u8); 64]> =
        std::cell::RefCell::new([(u64::MAX, 0); 64]);
}
fn river_pct_bucket(ev: &dyn pkr_contracts::Evaluator, hole: &[u8], board: &[u8]) -> u64 {
    let mut key = 0u64;
    for &c in hole.iter().chain(board.iter()) { key = (key << 6) | c as u64; }
    let slot = (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 58) as usize;
    if let Some(v) = PCT_CACHE.with(|c| { let e = c.borrow()[slot]; (e.0 == key).then_some(e.1) }) {
        return v as u64;
    }
    let mine = ev.evaluate_hand(hole, board);              // lower = stronger
    let mut dead = [false; 52];
    for &c in hole.iter().chain(board.iter()) { dead[c as usize] = true; }
    let (mut win2, mut n) = (0u32, 0u32);
    for a in 0..52u8 { if dead[a as usize] { continue; }
        for b in (a + 1)..52u8 { if dead[b as usize] { continue; }
            let o = ev.evaluate_hand(&[a, b], board);
            n += 1;
            if mine < o { win2 += 2 } else if mine == o { win2 += 1 }
        } }
    let pct = win2 as f32 / (2 * n.max(1)) as f32;
    let b = ((pct * RIVER_PCT_BUCKETS) as u8).min(RIVER_PCT_BUCKETS as u8 - 1);
    PCT_CACHE.with(|c| c.borrow_mut()[slot] = (key, b));
    b as u64
}
// river arm: cluster_id = (river_pct_bucket(..) << 8) | board_bucket
```
Cost is higher in the `RangeTracker`, which hashes about 1,000 hands per action, so expect tens of milliseconds per river action. This also changes the fingerprint.

### §E. Overflow guard
```rust
// Trainer::run_iterations_parallel, before fetch_add
let prev = self.iteration.load(Ordering::Relaxed);
assert!(prev.checked_add(n as u32).is_some(),
    "u32 iteration counter would overflow at {prev}+{n}; widen to u64 before >4.29B iterations");
```

## 6. Plan with decision gates

1. **Measurement (days, mostly existing code).**
   - Run held-out BR and LBR (with off-tree sizes) on the v42 blueprint and on a V3 run.
   - Save distinct checkpoints per eval.
   - Set `--stop-on-plateau 0`.
   - Check the trainer `--help` for the exact fit/score deal flags.
   - Add the Leduc gate to CI.
   - **Gate:** a held-out curve that descends with iterations.
2. **Robustness.**
   - Wire §B and re-run LBR with off-tree sizes.
   - **Gate:** off-tree LBR close to in-tree LBR.
3. **Abstraction A/Bs, one variable at a time, two seeds, held-out plus LBR.**
   - V3 against V1.
   - Exact preflop (§C).
   - River percentile (§D).
   - Longer runs: 200M iterations is about 5.5 h.
   - Later: suit-isomorphic, potential-aware flop and turn with more buckets. This needs `u16` tables.
4. **Search, only after steps 1–3.**
   - A safe re-solve gadget, with a time budget instead of 10 inner iterations.
   - Evaluate against an independent opponent or LBR, not against the blueprint.
5. **Do not do yet:** wider action abstraction (each new size multiplies the tree), more CI and playbook infrastructure, or wiring `public_br.rs` (its own header says it is wrong).

## 7. Honest ceiling

On an M1 you can plausibly reach a bot that beats most bots and humans, with a low measured LBR and robust handling of off-tree sizes. Matching a Nash-grade full-tree solver in every spot is not realistic. I would only call it competitive once held-out BR, LBR with off-tree sizes, and a head-to-head against an independent opponent agree. Those measurements do not exist yet, so everything above step 1 is unproven.
