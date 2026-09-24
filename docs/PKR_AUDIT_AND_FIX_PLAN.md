# pkr-sota — Audit & Fix Plan (from `dump.txt`)

**Audience:** an automated coding agent. Follow the phases **in order**. Every step says
*file → FIND → REPLACE*. Do not improvise, do not "clean up" anything not listed.
After each phase run the **Gate** commands; if a gate fails, fix it before moving on.

> **Honesty note.** I only had the text dump, not a compiler. Snippets were written against the
> exact code in the dump but are **not compiled**. Expect to fix small things (imports, a missing
> `pub`). The Gate after each phase tells you when you are done.

---

## 0. Verdict on the "i64 is the right fix" argument

The reasoning is sound: the i32 table saturates at 2.147e9 units = 2.147e6 chips, and clipping
(v19) throws away real signal. i64 is the right *direction*. But four things need to be known
before you trust the v20 numbers:

1. **The dump is a half-finished i64 migration and does not compile.** `table.rs` imports
   `AtomicI32` but uses `AtomicI64`, and `with_capacity` builds `Vec<AtomicI32>` with
   `AtomicI64::new`. Doc comments still say "i32" and "clip at 500 chips". (Phase A1.)
2. **Checkpoints are silently ambiguous.** The magic is still `PKRCKPT6` although the payload
   width changed from 4 to 8 bytes per cell. A v17/v19 file must be *rejected*, not mis-parsed.
   (Phase A2.)
3. **The exploitability number you are using as the success criterion is biased.** The best
   response is fitted **and** scored on the *same* 2000 sampled deals (over-fit → biased **high**),
   is under-converged at 3 passes (biased **low**), defaults illegal buckets to 0.0 (biased
   **low**), and uses a *different* set of deals at every eval (noise between points). So
   "5091 vs 4079 vs 6482 mbb" cannot be read as a clean trend, and the targets
   (<3000 @5M, <2000 @10M …) are not meaningful until Phase C is done. **Re-baseline after Phase C.**
4. **Two "algorithm" features are dead or degenerate**, independent of i64:
   * FBRS pruning can never fire (regrets are floored at 0, threshold is −400 000). (B2)
   * Strategy averaging is *uniform*. DCFR/CFR+ prescribe weight `t^γ`; uniform averaging keeps
     early garbage in the exported blueprint. This is probably the largest remaining quality
     lever. (D2)

Side notes on the pasted analysis: memory (72→144 MB) and checkpoint (120→190 MB) numbers are
right. The "−5…−10 % it/s" estimate is plausible for the wider array, but the current flush path
does an `i128` divide per cell update; Phase E1 removes that and should more than pay for it.
`max|r|` printed by the trainer currently **includes the momentum cells**; after A5 it is regret-only,
so do not compare it 1:1 with old logs.

---

## 1. Findings index

| ID | Sev | Where | Problem |
|----|-----|-------|---------|
| A1 | **P0** | `pkr-cfr/src/table.rs` | i64 migration does not compile; stale docs |
| A2 | **P0** | `table.rs`, `main.rs` | Checkpoint magic not bumped; unaligned `from_bytes`; failed load leaves table poisoned; trainer silently "starts fresh" and then overwrites the good checkpoint |
| A3 | **P0** | `pkr-cfr/src/dcfr.rs` | `discounted + predicted` can overflow i64; per-update `i128` division on the hot path |
| A4 | **P0** | `table.rs` tests | The CPU regression tests only compile under `feature = "gpu"` → never run |
| B1 | **P0** | `traversal.rs`, `table.rs` | Pruned actions push `delta = NaN`; in batched mode one NaN wipes the whole (idx, action) group |
| B2 | P1 | `traversal.rs` | FBRS pruning is dead code (regret ≥ 0 always) |
| B3 | P1 | `table.rs` | `PKR_F5_SEQUENTIAL` default contradicts its comment; env var read on every flush |
| B4 | P1 | `table.rs`, `lib.rs`, `main.rs` | Capacity check uses map `len()` but slots are exhausted by `next_idx` (leaked slots) → `panic!` + `panic="abort"` loses run; no checkpoint on capacity stop; no Ctrl-C handling |
| B5 | P1 | `pkr-export/src/writer.rs`, `main.rs` | Blueprint written non-atomically with `expect/unwrap` panics mid-training; CDF last byte not guaranteed 255 and rounding mass can land on a zero-prob action |
| B6 | P1 | `pkr-runtime/src/mmap.rs` | Versions > 4 accepted as v4; cdf length not validated; unaligned casts |
| B7 | P1 | `pkr-core/src/state.rs` | `legal_actions()` (used by fuzz tests) ≠ `legal_actions_into()` (used by training): no raise cap, no all-in dedup |
| B8 | P2 | `table.rs` | `snapshot()` mixes regret and momentum cells |
| B9 | P2 | `table.rs`, `Cargo.toml` | `flush_gpu_batch` with the stub → `chunks(0)` panic; GPU shader is i32 |
| B10 | P2 | `pkr-abstraction/src/lib.rs` | River board-bucket miss returns 0 silently (not counted as fallback) |
| C1 | **P0** | `pkr-exploit/src/best_response.rs` | BR over-fit (train = eval deals) |
| C2 | **P0** | same | Illegal buckets have cfv 0.0 and can win the argmax; unknown-infoset default is "Fold"; only 3 improvement passes |
| C3 | P1 | same, `main.rs` | Different deals each eval; promote-gate 3 mbb ≪ noise |
| D1 | P2 | `dcfr.rs` | PCFR+ "momentum" is *added to regret* (not a real PCFR+); needs A/B switch |
| D2 | P2 | `traversal.rs` | Uniform strategy averaging; add `t^p` weight (flag) |
| D3 | info | — | ε-exploration bias, sizing semantics, signature aliasing (documented, not changed) |
| E1–E6 | P3 | various | perf: exact u64 discount, no `/SCALE`, chunk size, forced-move nodes, `valuenet` allocs, optional dep |

**Behaviour-changing items are behind env flags that default to the current behaviour**
(`PKR_MOMENTUM`, `PKR_AVG_POWER`, `PKR_SKIP_FORCED`) so v20 stays comparable.

---

## PHASE A — make the i64 build correct

### A1. `crates/pkr-cfr/src/table.rs` — compile fix + stale docs

**FIND**
```rust
use std::sync::atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering};
```
**REPLACE**
```rust
use std::sync::atomic::{AtomicI64, AtomicU64, AtomicUsize, Ordering};
```

**FIND**
```rust
        let mut data: Vec<AtomicI32> = Vec::with_capacity(capacity * RM_STRIDE);
```
**REPLACE**
```rust
        let mut data: Vec<AtomicI64> = Vec::with_capacity(capacity * RM_STRIDE);
```

**FIND** (the whole doc-comment + const; it still describes the i32/500-chip clip)
```rust
/// Maximum |regret| / |momentum| stored in the i32 fixed-point tables,
/// expressed at `SCALE`. Clipping below `i32::MAX` leaves headroom for
/// the next batch's delta and prevents the saturation pathology that
/// silently uniformizes regret-matching on high-traffic infosets.
///
/// 500_000 / SCALE=1000 = 500 chips = 2.5× starting stack.
/// Any strategy preference stronger than that is indistinguishable in
/// practice, so clipping there costs nothing.
pub(crate) const R_MAX: i64 = i64::MAX / 4;
```
**REPLACE**
```rust
/// Safety ceiling for |regret| / |momentum| in the i64 fixed-point table
/// (units of 1/SCALE chips). `i64::MAX / 4` ≈ 2.3e18 units ≈ 2.3e15 chips,
/// about 10^9 x larger than any value observed in practice, so this clamp
/// exists only to keep intermediate arithmetic overflow-free. It is NOT a
/// regret clip in the CFR sense (v19 clipped at 500 chips; that is gone).
pub(crate) const R_MAX: i64 = i64::MAX / 4;
```

**FIND**
```rust
    /// Interleaved regret+momentum, i32 fixed-point at scale 1000.
    data: Vec<AtomicI64>,
```
**REPLACE**
```rust
    /// Interleaved regret+momentum, i64 fixed-point at scale 1000.
    data: Vec<AtomicI64>,
```

**FIND** (in `regret_scaled` doc)
```rust
    /// Raw i32 regret for an action at a known idx. Used by FBRS pruning
    /// (Brown & Sandholm, NeurIPS 2015) to decide when to skip exploring
    /// a hopeless action. Cheap inline read.
```
**REPLACE**
```rust
    /// Raw i64 fixed-point regret (units of 1/SCALE chips) at a known idx.
```

**FIND** (redundant casts in `flush_cpu_batch`; will be rewritten in B3, but if you do A1 alone)
```rust
                let mut cur_i64 = self.load_rm(idx, a, RM_REGRET) as i64;
                let mut mom_i64 = self.load_rm(idx, a, RM_MOMENTUM) as i64;
```
**REPLACE**
```rust
                let mut cur_i64 = self.load_rm(idx, a, RM_REGRET);
                let mut mom_i64 = self.load_rm(idx, a, RM_MOMENTUM);
```
*(B3 replaces this whole function; if you apply B3 you can skip this last edit.)*

Also update the header comment of `crates/pkr-cfr/src/dcfr.rs` line
`` `current_i64` and `delta_i64` are raw i32/i64 fixed-point values `` → "raw i64 fixed-point values".

### A2. Checkpoint format v7 + safe resume

#### A2.1 `table.rs::save_checkpoint`

**FIND**
```rust
        w.write_all(b"PKRCKPT6")?;
        w.write_all(&6u32.to_le_bytes())?;
```
**REPLACE**
```rust
        w.write_all(b"PKRCKPT7")?; // v7: regret/momentum cells are i64 (v6 was i32)
        w.write_all(&7u32.to_le_bytes())?;
```

#### A2.2 `table.rs::load_checkpoint`

**FIND**
```rust
        let magic = read(&mut p, 8)?;
        if magic != b"PKRCKPT6" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "bad checkpoint magic (expected v5 format)",
            ));
        }
        let version = u32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap());
        if version != 6 {
```
**REPLACE**
```rust
        let magic = read(&mut p, 8)?;
        if magic == b"PKRCKPT6" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "checkpoint is v6 (i32 regret cells, pre-i64). It is incompatible \
                 with this build; start a fresh run (pass --fresh).",
            ));
        }
        if magic != b"PKRCKPT7" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "bad checkpoint magic (expected PKRCKPT7)",
            ));
        }
        let version = u32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap());
        if version != 7 {
```

**FIND** (unaligned cast — `Vec<u8>` gives no alignment guarantee for `AbstractionFingerprint`)
```rust
        let fp_bytes = read(&mut p, 40)?;
        let stored_fp: &pkr_core::abstraction::AbstractionFingerprint =
            bytemuck::from_bytes(fp_bytes);
        if stored_fp != current {
```
**REPLACE**
```rust
        let fp_bytes = read(&mut p, 40)?;
        let stored_fp: pkr_core::abstraction::AbstractionFingerprint =
            bytemuck::pod_read_unaligned(fp_bytes);
        if stored_fp != *current {
```

**FIND** (right after the fingerprint/K/iteration/`n`/`map_len` header is parsed, *before* `let guard = self.hash_to_idx.pin();` in `load_checkpoint`)
```rust
        let guard = self.hash_to_idx.pin();
        guard.clear();
        for _ in 0..map_len {
```
**REPLACE**
```rust
        // Reset ALL state first. A previous failed/partial load (or a load of
        // a larger checkpoint followed by a smaller one) must not leave stale
        // cells that a later `alloc_idx` would hand out as "fresh" slots.
        self.data.par_iter().for_each(|c| c.store(0, Ordering::Relaxed));
        self.strategy_sum
            .par_iter()
            .for_each(|c| c.store(0, Ordering::Relaxed));
        self.next_idx.store(0, Ordering::Relaxed);
        let guard = self.hash_to_idx.pin();
        guard.clear();
        for _ in 0..map_len {
```

Add to the doc-comment of `load_checkpoint`:
```rust
    /// On `Err` the table contents are UNDEFINED. The caller must abort or
    /// call `load_checkpoint` again successfully; it must never train on it.
```

#### A2.3 `binaries/pkr-trainer/src/main.rs` — never silently start fresh over an existing checkpoint

Add a CLI flag inside `struct Cli`:
```rust
    /// Ignore (and later overwrite) an existing checkpoint instead of resuming.
    #[arg(long, default_value_t = false)]
    fresh: bool,
```

**FIND** the whole block that begins `let start_iter = if let Some(ckpt) = &cli.checkpoint {` and ends with the matching `} else { 0 };` (the one containing `"WARNING: primary checkpoint failed"`). **REPLACE** with:
```rust
    let start_iter: u32 = match &cli.checkpoint {
        Some(ckpt) if ckpt.exists() && !cli.fresh => {
            match trainer.load_checkpoint(ckpt.to_str().unwrap(), &fingerprint) {
                Ok(()) => {
                    let it = trainer.iteration();
                    eprintln!("Resumed from checkpoint at iteration {}", it);
                    it
                }
                // Format / abstraction mismatch: resuming would corrupt training and
                // "starting fresh" would overwrite the user's checkpoint. Refuse.
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                    return Err(format!(
                        "checkpoint {} is incompatible: {e}. \
                         Delete it or pass --fresh to discard it.",
                        ckpt.display()
                    )
                    .into());
                }
                // Truncated / unreadable: try the rolling .prev copy, else refuse.
                Err(e) => {
                    let prev = ckpt.with_extension("ckpt.prev");
                    if !prev.exists() {
                        return Err(format!(
                            "checkpoint {} unreadable ({e}) and no .prev exists. \
                             Pass --fresh to discard it.",
                            ckpt.display()
                        )
                        .into());
                    }
                    eprintln!("WARNING: primary checkpoint failed ({e}), trying .prev");
                    trainer
                        .load_checkpoint(prev.to_str().unwrap(), &fingerprint)
                        .map_err(|e2| format!("both checkpoint and .prev failed: {e2}"))?;
                    let it = trainer.iteration();
                    eprintln!("Resumed from .prev checkpoint at iteration {}", it);
                    it
                }
            }
        }
        Some(ckpt) if ckpt.exists() && cli.fresh => {
            eprintln!("WARNING: --fresh: existing checkpoint {} will be overwritten", ckpt.display());
            0
        }
        _ => 0,
    };
```

#### A2.4 tests — append to `table.rs`
```rust
#[cfg(test)]
mod ckpt_tests {
    use super::*;
    use pkr_core::abstraction::AbstractionFingerprint;

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("pkr_{}_{}.ckpt", name, std::process::id()))
    }

    #[test]
    fn checkpoint_roundtrip_preserves_values_beyond_i32() {
        let fp = AbstractionFingerprint::from_constants(4);
        let a = CompactRegretTable::with_capacity(64);
        let idx = a.get_or_create_idx(0xABCD);
        a.store_rm(idx, 2, RM_REGRET, 5_000_000_000_000i64); // > i32::MAX
        a.add_strategy_sum_at(idx, 1, 0.25);
        let p = tmp("rt");
        a.save_checkpoint(p.to_str().unwrap(), 42, &fp).unwrap();

        let b = CompactRegretTable::with_capacity(64);
        assert_eq!(b.load_checkpoint(p.to_str().unwrap(), &fp).unwrap(), 42);
        let j = b.get_or_create_idx(0xABCD);
        assert_eq!(b.regret_scaled(j, 2), 5_000_000_000_000i64);
        assert!((b.load_sum(j, 1) - 0.25).abs() < 1e-12);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn legacy_v6_checkpoint_is_rejected() {
        let fp = AbstractionFingerprint::from_constants(4);
        let p = tmp("v6");
        std::fs::write(&p, b"PKRCKPT6\x06\x00\x00\x00").unwrap();
        let t = CompactRegretTable::with_capacity(8);
        let e = t.load_checkpoint(p.to_str().unwrap(), &fp).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
        std::fs::remove_file(p).ok();
    }
}
```

### A3. `crates/pkr-cfr/src/dcfr.rs` — overflow-safe, exact, fast update

Problems in the current `update_regret_i64`:
* `discounted_i64 + predicted_i64` can overflow `i64` (panics in debug, wraps in release).
* `r_pos * num_pos * den_neg` can approach `i128::MAX` if a caller ever passes an un-clamped value.
* One `i128` division per cell update in the hottest serial-ish loop (`flush_cpu_batch`).

Exact identity used: for `r ≥ 0`, `floor(r·t²/(t²+1)) = r − ceil(r/(t²+1))`, computable in `u64`.
For `r < 0` and `t ≥ TAU` the β=0 discount is exactly `trunc(r/2)`. Regrets are floored at 0 so the
negative branch is only defensive.

**FIND** the entire function `pub fn update_regret_i64( … ) -> (i64, i64) { … }` (from its doc-comment
`/// Exact regret/momentum update in i64 (at SCALE=1000).` to the closing brace before
`/// Exact strategy-sum update with the γ=2 discount applied.`). **REPLACE** with:

```rust
/// Exact `floor(r * t^2 / (t^2 + 1))` for `r >= 0`, `t >= TAU`; identity for warmup.
#[inline(always)]
fn discount_pos_i64(r: i64, t: u32) -> i64 {
    debug_assert!(r >= 0);
    if t < TAU || r == 0 {
        return r;
    }
    // (2^32-1)^2 + 1 < 2^64: no overflow for any u32 t.
    let d = (t as u64) * (t as u64) + 1;
    let ru = r as u64;
    let q = ru / d;
    let ceil = if ru % d != 0 { q + 1 } else { q };
    (ru - ceil) as i64
}

/// β = 0 discount for negative regret: exactly 1/2 (truncated toward zero).
#[inline(always)]
fn discount_neg_i64(r: i64, t: u32) -> i64 {
    if t < TAU {
        r
    } else {
        r / 2
    }
}

/// Exact regret/momentum update in i64 (at SCALE=1000).
///
/// `momentum_on = true`  → production PCFR+-style update (see D1 in the audit).
/// `momentum_on = false` → plain CFR+/DCFR: `r' = max(0, disc(r) + delta)`.
///
/// Returns `(new_regret, new_momentum)`. Never overflows: the add saturates.
#[inline]
pub fn update_regret_i64_mode(
    current_i64: i64,
    prev_momentum_i64: i64,
    iteration: u32,
    delta_i64: i64,
    momentum_on: bool,
) -> (i64, i64) {
    let t = iteration;
    if t == 0 {
        return (delta_i64, delta_i64);
    }
    let predicted_i64 = if momentum_on {
        let gamma = 1.0 / ((t as f64) + 1.0).sqrt();
        ((1.0 - gamma) * (prev_momentum_i64 as f64) + gamma * (delta_i64 as f64)).round() as i64
    } else {
        delta_i64
    };
    let discounted = if current_i64 >= 0 {
        discount_pos_i64(current_i64, t)
    } else {
        discount_neg_i64(current_i64, t)
    };
    let new_r = discounted.saturating_add(predicted_i64).max(0);
    (new_r, predicted_i64)
}

/// Production entry point (momentum on). Kept for API/test compatibility.
#[inline]
pub fn update_regret_i64(
    current_i64: i64,
    prev_momentum_i64: i64,
    iteration: u32,
    delta_i64: i64,
) -> (i64, i64) {
    update_regret_i64_mode(current_i64, prev_momentum_i64, iteration, delta_i64, true)
}
```

Append this test module at the end of `dcfr.rs` (it keeps the *old* i128 code as an oracle):
```rust
#[cfg(test)]
mod fast_path_equivalence {
    use super::*;

    fn reference(cur: i64, mom: i64, t: u32, delta: i64) -> (i64, i64) {
        if t == 0 {
            return (delta, delta);
        }
        let gamma = 1.0 / (((t as f64) + 1.0).sqrt());
        let pred = ((1.0 - gamma) * (mom as f64) + gamma * (delta as f64)).round() as i64;
        let (np, dp) = discount_num_den(t, 2);
        let (nn, dn) = discount_num_den(t, 0);
        let rp = cur.max(0) as i128;
        let rn = cur.min(0) as i128;
        let cd = dp * dn;
        let d = ((rp * np * dn) + (rn * nn * dp)) / cd;
        let d = d.clamp(i64::MIN as i128, i64::MAX as i128) as i64;
        ((d + pred).max(0), pred)
    }

    #[test]
    fn matches_i128_reference_on_random_inputs() {
        let mut s: u64 = 0x1234_5678_9ABC_DEF1;
        let mut next = move || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            s
        };
        for _ in 0..300_000 {
            let t = (next() % 6_000_000) as u32; // includes 0 and warmup
            let sign = |x: u64| if x & 1 == 0 { 1i64 } else { -1i64 };
            let cur = ((next() >> 8) as i64 % 4_000_000_000_000) * sign(next());
            let mom = ((next() >> 8) as i64 % 40_000_000) * sign(next());
            let delta = ((next() >> 8) as i64 % 400_000_000) * sign(next());
            assert_eq!(
                update_regret_i64(cur, mom, t, delta),
                reference(cur, mom, t, delta),
                "cur={cur} mom={mom} t={t} delta={delta}"
            );
        }
    }

    #[test]
    fn never_overflows_at_extremes() {
        let (r, _) = update_regret_i64(i64::MAX / 4, 0, u32::MAX, i64::MAX / 2);
        assert!(r >= 0);
        let (r, _) = update_regret_i64_mode(i64::MAX, 0, 5_000, i64::MAX, false);
        assert_eq!(r, i64::MAX); // saturated, no panic
    }
}
```

### A4. Make the CPU regression tests actually run

In `table.rs` the module

```rust
#[cfg(test)]
#[cfg(feature = "gpu")]
mod tests {
```
contains `cpu_flush_sort_dedup_equals_sum` and `deep_reach_prob_contributes_to_strategy_sum`, which
are CPU-path tests but are **compiled only with the gpu feature** (i.e. never).

**FIND**
```rust
#[cfg(test)]
#[cfg(feature = "gpu")]
mod tests {
    use super::*;

    #[test]
    fn flush_writes_back_only_touched_entries_and_is_idempotent_for_untouched() {
```
**REPLACE**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "gpu")]
    #[test]
    fn flush_writes_back_only_touched_entries_and_is_idempotent_for_untouched() {
```

Then replace `flush_cpu_batch(&mut batch)` inside `cpu_flush_sort_dedup_equals_sum` with the explicit-mode
call (so the test no longer depends on an env var):
```rust
        let (input, unique) = table.flush_cpu_batch_with(
            &mut batch,
            FlushMode { sequential: true, momentum: true },
        );
```
And in `mod f5_tests` replace the whole `if std::env::var("PKR_F5_SEQUENTIAL")… return; }` skip block with nothing,
and call `table.flush_cpu_batch_with(&mut batch, FlushMode { sequential: true, momentum: true });`
instead of `table.flush_cpu_batch(&mut batch);`. (I re-derived both expected values by hand:
0.655 / 0.536 and 6.595 are correct for the sequential fold.)

`FlushMode` is defined in B3.

### A5. `snapshot()` — regret only (B8, do it now)

**FIND** the body of `pub fn snapshot(&self) -> TableSnapshot` from `let n = …` through the `for i in 0..entries_rm { … }` loop, and replace the function with:
```rust
    pub fn snapshot(&self) -> TableSnapshot {
        let n = self.allocated();
        let mut max_abs = 0.0f32;
        let mut sum_abs = 0.0f64;
        let mut nonfinite = 0usize;
        for idx in 0..n {
            for a in 0..K {
                let v = self.load_rm(idx, a, RM_REGRET) as f32 / SCALE;
                if !v.is_finite() {
                    nonfinite += 1;
                    continue;
                }
                let av = v.abs();
                if av > max_abs {
                    max_abs = av;
                }
                sum_abs += av as f64;
            }
        }
        let mut strat_mass = 0.0f64;
        for i in 0..n * SUM_STRIDE {
            strat_mass += f64::from_bits(self.strategy_sum[i].load(Ordering::Relaxed));
        }
        let infosets = {
            let guard = self.hash_to_idx.pin();
            guard.len()
        };
        TableSnapshot {
            infosets,
            capacity: self.capacity,
            max_abs_regret: max_abs,
            mean_abs_regret: if n > 0 { (sum_abs / (n * K) as f64) as f32 } else { 0.0 },
            nonfinite_count: nonfinite,
            strategy_sum_mass: strat_mass,
        }
    }
```
(`allocated()` is added in B4.) Note in the CHANGELOG that `max_abs_regret`/`mean_abs_regret` are now regret-only.

**Gate A**
```bash
cargo fmt --all
cargo build --workspace --release
cargo test -p pkr-cfr --release
cargo clippy --workspace --all-targets -- -D warnings
```
Expected: `cpu_flush_sort_dedup_equals_sum`, `deep_reach_prob_contributes_to_strategy_sum`,
`fast_path_equivalence::*`, `ckpt_tests::*` all run and pass.

---

## PHASE B — correctness bugs

### B1. NaN deltas from pruned/illegal actions (traversal + flush)

`v[a]` is `f32::NAN` for illegal buckets **and** for pruned actions. The regret push loop only skips
illegal buckets, so a pruned action pushes `delta = v[a] - v_sigma = NaN`. In `flush_cpu_batch`
(batched mode) `delta_sum += NaN` turns the *whole group* into NaN → `NaN as i64 = 0`, i.e. every
legitimate delta for that (infoset, action) in the sync batch is silently dropped.

**`traversal.rs` — FIND**
```rust
        for a in 0..K {
            if action_counts[a] == 0 {
                continue;
            }
            let delta = v[a] - v_sigma;
```
**REPLACE**
```rust
        for a in 0..K {
            if action_counts[a] == 0 || v[a].is_nan() {
                continue;
            }
            let delta = v[a] - v_sigma;
```
The flush side is made robust in B3 (`to_fixed` maps non-finite → 0 and the batched sum skips non-finite items).

### B2. Delete the dead FBRS pruning

Stored regret is always `≥ 0` (`update_regret_i64` ends in `.max(0)`), so
`table.regret_scaled(idx, a) < PRUNE_THRESHOLD` (= −400 000) is never true. The branch short-circuits
before touching the RNG, so deleting it leaves training **bit-identical** and removes a misleading
feature.

**`traversal.rs` — DELETE** the three constants and their doc comment:
```rust
const PRUNE_WARMUP: u32 = 1_000_000;
const PRUNE_THRESHOLD: i64 = -400_000; // -400 chips at SCALE=1000
const PRUNE_SKIP_PROB: f32 = 0.95;
```
(and the `/// FBRS (Brown & Sandholm, NeurIPS 2015) pruning …` comment above them), and **DELETE** this block inside the traverser loop:
```rust
            // FBRS pruning: skip a hopeless action (regret very negative,
            // probability already zero) most of the time. Sentinal value
            // is NaN, same as illegal buckets, so v_sigma and the push
            // loop already skip it.
            if global_iteration > PRUNE_WARMUP
                && strategy[a] == 0.0
                && table.regret_scaled(idx, a) < PRUNE_THRESHOLD
                && rng.random::<f32>() < PRUNE_SKIP_PROB
            {
                v[a] = f32::NAN;
                continue;
            }
```
*(If you want pruning later: with CFR+-style flooring you have no negative-regret signal; you would need a separate per-cell "consecutive zero-regret visits" counter. Out of scope here.)*

### B3. One explicit flush configuration (`FlushMode`), robust to non-finite deltas

Today `sequential = env("PKR_F5_SEQUENTIAL") != Ok("0")`, i.e. **sequential when unset**, while the comment says
"Default is the batched-sum form". The code wins (tests assume sequential); fix the comment and read the env once.

**`table.rs` — add** (near the top, after `RM_MOMENTUM`):
```rust
/// How `flush_cpu_batch` folds deltas. Read once from the environment.
///   PKR_F5_SEQUENTIAL=0  → batched-sum fold (legacy v9..v16 behaviour)
///   PKR_MOMENTUM=0|off   → plain CFR+/DCFR update, no PCFR+ momentum term
/// Defaults (both true) reproduce the current production behaviour.
#[derive(Clone, Copy, Debug)]
pub struct FlushMode {
    pub sequential: bool,
    pub momentum: bool,
}

impl FlushMode {
    pub fn from_env() -> Self {
        let off = |n: &str| {
            matches!(std::env::var(n).as_deref(), Ok("0") | Ok("off") | Ok("false"))
        };
        Self {
            sequential: !off("PKR_F5_SEQUENTIAL"),
            momentum: !off("PKR_MOMENTUM"),
        }
    }
    pub fn production() -> Self {
        static M: OnceLock<FlushMode> = OnceLock::new();
        *M.get_or_init(Self::from_env)
    }
}

#[inline]
fn to_fixed(x: f64) -> i64 {
    if x.is_finite() {
        (x * SCALE as f64).round() as i64
    } else {
        0
    }
}
```
(`use std::sync::OnceLock;` is already imported at the top of the file; keep it un-gated — see B9.)

**Replace the whole `pub fn flush_cpu_batch(...)`** with these two functions:
```rust
    /// Apply a batch of deferred regret updates. Returns (input_len, unique_count).
    pub fn flush_cpu_batch(&self, batch: &mut Vec<BatchItem>) -> (u64, u64) {
        self.flush_cpu_batch_with(batch, FlushMode::production())
    }

    pub fn flush_cpu_batch_with(&self, batch: &mut Vec<BatchItem>, mode: FlushMode) -> (u64, u64) {
        let input_len = batch.len() as u64;
        if batch.is_empty() {
            return (0, 0);
        }
        batch.par_sort_unstable_by_key(|item| (item.index, item.action, item.iteration));

        let mut groups: Vec<(usize, usize, u32, u32)> = Vec::with_capacity(batch.len() / 4 + 16);
        let mut i = 0usize;
        while i < batch.len() {
            let (idx, act) = (batch[i].index, batch[i].action);
            let mut end = i + 1;
            while end < batch.len() && batch[end].index == idx && batch[end].action == act {
                end += 1;
            }
            groups.push((i, end, idx, act));
            i = end;
        }

        let unique_len = groups.len() as u64;
        let batch_ref: &[BatchItem] = batch.as_slice();
        let n_threads = rayon::current_num_threads().max(1);
        // Many small chunks → work stealing can balance skewed group sizes.
        let chunk_size = (groups.len() / (n_threads * 8)).max(64);

        groups.par_chunks(chunk_size).for_each(|grp_slice| {
            for &(start, end, idx_u32, act_u32) in grp_slice {
                let idx = idx_u32 as usize;
                let a = act_u32 as usize;
                let mut cur = self.load_rm(idx, a, RM_REGRET);
                let mut mom = self.load_rm(idx, a, RM_MOMENTUM);

                if mode.sequential {
                    for k in start..end {
                        let d = to_fixed(batch_ref[k].delta as f64);
                        let (r, m) = crate::dcfr::update_regret_i64_mode(
                            cur,
                            mom,
                            batch_ref[k].iteration,
                            d,
                            mode.momentum,
                        );
                        cur = r;
                        mom = m;
                    }
                } else {
                    let mut delta_sum = 0.0f64;
                    let mut max_iter = batch_ref[start].iteration;
                    for k in start..end {
                        let d = batch_ref[k].delta;
                        if d.is_finite() {
                            delta_sum += d as f64;
                        }
                        max_iter = max_iter.max(batch_ref[k].iteration);
                    }
                    let (r, m) = crate::dcfr::update_regret_i64_mode(
                        cur,
                        mom,
                        max_iter,
                        to_fixed(delta_sum),
                        mode.momentum,
                    );
                    cur = r;
                    mom = m;
                }
                self.store_rm(idx, a, RM_REGRET, cur.clamp(-R_MAX, R_MAX));
                self.store_rm(idx, a, RM_MOMENTUM, mom.clamp(-R_MAX, R_MAX));
            }
        });
        (input_len, unique_len)
    }
```
Delete the now-unused `warn_nonfinite_regret_once` **only if** clippy complains (it is `#[allow(dead_code)]`).

Apply the same `chunk_size` formula in `apply_strategy_batch`:
`let chunk_size = (groups.len() / (n_threads * 8)).max(64);`

**Tests — append to `table.rs`:**
```rust
#[cfg(test)]
mod i64_tests {
    use super::*;
    const SEQ: FlushMode = FlushMode { sequential: true, momentum: true };
    const SUM: FlushMode = FlushMode { sequential: false, momentum: true };

    fn item(index: usize, action: u32, iteration: u32, delta: f32) -> BatchItem {
        BatchItem { index: index as u32, action, iteration, delta }
    }

    #[test]
    fn regret_exceeds_old_i32_ceiling() {
        let t = CompactRegretTable::with_capacity(16);
        let idx = t.get_or_create_idx(0x1111);
        let mut b = vec![item(idx, 0, 1, 4.0e6)]; // 4e6 chips -> 4e9 units > i32::MAX
        t.flush_cpu_batch_with(&mut b, SEQ);
        assert!(t.get_regret(0x1111, 0) > 2.2e6, "got {}", t.get_regret(0x1111, 0));
    }

    #[test]
    fn nan_delta_does_not_poison_group_in_batched_mode() {
        let t = CompactRegretTable::with_capacity(16);
        let idx = t.get_or_create_idx(0x2222);
        let mut b = vec![item(idx, 0, 1, 1.0), item(idx, 0, 1, f32::NAN), item(idx, 0, 1, 2.0)];
        t.flush_cpu_batch_with(&mut b, SUM);
        // sum = 3.0, gamma = 1/sqrt(2) -> 2.121
        assert!((t.get_regret(0x2222, 0) - 2.121).abs() < 0.01);
    }

    #[test]
    fn momentum_off_is_plain_cfr_plus() {
        let t = CompactRegretTable::with_capacity(16);
        let idx = t.get_or_create_idx(0x3333);
        let mut b = vec![item(idx, 0, 1, 10.0), item(idx, 0, 2, -6.0)];
        t.flush_cpu_batch_with(&mut b, FlushMode { sequential: true, momentum: false });
        assert!((t.get_regret(0x3333, 0) - 4.0).abs() < 0.01); // max(0, 10 - 6)
    }
}
```

### B4. Capacity accounting, checkpoint on capacity stop, Ctrl-C

`alloc_idx` consumes slots (`next_idx`), but `get_or_create_idx*` allocates a slot **before**
`try_insert`; when another thread wins the race the slot is leaked. So `hash_to_idx.len()` under-counts
used slots, `is_near_capacity()` (which uses `len()`) can fire late, and `alloc_idx` `panic!`s — with
`panic = "abort"` in the release profile that kills the run and loses everything since the last
checkpoint. The trainer also skips the final checkpoint whenever it stops early.

**`table.rs` — add**
```rust
    /// Number of slots handed out (>= number of distinct infosets: lost races leak slots).
    #[inline]
    pub fn allocated(&self) -> usize {
        self.next_idx.load(Ordering::Relaxed).min(self.capacity)
    }
```
In `save_checkpoint` and elsewhere `let n = self.next_idx.load(Ordering::Relaxed).min(self.capacity);` may be replaced with `self.allocated()`.

**`crates/pkr-cfr/src/lib.rs` — FIND**
```rust
    pub fn is_near_capacity(&self) -> bool {
        let cap = self.table.capacity();
        cap > 0 && self.table.len() * 100 / cap >= 95
    }
```
**REPLACE**
```rust
    pub fn is_near_capacity(&self) -> bool {
        let cap = self.table.capacity();
        cap > 0 && self.table.allocated() * 100 / cap >= 95
    }
```

**`main.rs`** — (1) check capacity **before every batch** instead of only at report boundaries;
(2) always save the final checkpoint on capacity stop / Ctrl-C; (3) add a Ctrl-C handler.

`binaries/pkr-trainer/Cargo.toml` `[dependencies]` add:
```toml
ctrlc = { version = "3", features = ["termination"] }
```

In `run()` after `let mut stopped_early = false;` add:
```rust
    let mut hit_capacity = false;
    let mut interrupted = false;
    let stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let f = stop_flag.clone();
        ctrlc::set_handler(move || f.store(true, std::sync::atomic::Ordering::SeqCst))
            .expect("failed to install signal handler");
    }
```
At the very top of `while done < max_iters {` add:
```rust
        if stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
            eprintln!("Signal received: stopping after iteration {done}");
            interrupted = true;
            break;
        }
        if trainer.is_near_capacity() {
            eprintln!("WARN: table >=95% of capacity ({} slots), stopping", trainer.get_table().allocated());
            stopped_early = true;
            hit_capacity = true;
            break;
        }
```
**DELETE** the old block inside `if should_report { … }`:
```rust
            if trainer.is_near_capacity() {
                eprintln!(
                    "WARN: table near capacity ({} infosets), stopping early",
                    snap.infosets
                );
                stopped_early = true;
                break;
            }
```
**FIND**
```rust
    if !stopped_early {
        if let Some(ckpt) = &cli.checkpoint {
```
**REPLACE**
```rust
    if !stopped_early || hit_capacity || interrupted {
        if let Some(ckpt) = &cli.checkpoint {
```

### B5. Blueprint writer: atomic, fallible, well-formed CDF

**`crates/pkr-export/src/writer.rs`**

(1) Change the signature and error handling. **FIND** `pub fn write_blueprint(` … `) {` and make it
```rust
pub fn write_blueprint(
    path: &str,
    table: &CompactRegretTable,
    keys: &[u64],
    fingerprint: &AbstractionFingerprint,
) -> std::io::Result<()> {
```
(2) Replace the CDF quantisation loop
```rust
        let mut cumulative = 0.0f32;
        for a in 0..K {
            cumulative += strat[a];
            let byte = (cumulative * 255.0).round().clamp(0.0, 255.0) as u8;
            cdf_bytes.push(byte);
        }
```
with
```rust
        cdf_bytes.extend_from_slice(&quantize_cdf(&strat));
```
and add above `write_blueprint`:
```rust
/// Quantise a probability vector to a monotone u8 CDF that ALWAYS closes at
/// 255 on the last action with non-zero probability. Rounding slack therefore
/// never lands on an action whose probability is zero (e.g. an illegal bucket).
pub(crate) fn quantize_cdf(strat: &[f32; K]) -> [u8; K] {
    let mut out = [0u8; K];
    let total: f32 = strat.iter().sum();
    if !(total > 0.0) {
        for a in 0..K {
            out[a] = ((((a + 1) as f32) / K as f32) * 255.0).round() as u8;
        }
        out[K - 1] = 255;
        return out;
    }
    let mut cum = 0.0f32;
    let mut prev = 0u8;
    for a in 0..K {
        cum += strat[a] / total;
        let b = (cum * 255.0).round().clamp(0.0, 255.0) as u8;
        out[a] = b.max(prev);
        prev = out[a];
    }
    let last = (0..K).rev().find(|&a| strat[a] > 0.0).unwrap_or(K - 1);
    for a in last..K {
        out[a] = 255;
    }
    out
}
```
(3) Atomic file write. Replace from `let mut file = File::create(path).expect(...)` to `file.flush().unwrap();` with:
```rust
    let tmp = format!("{path}.tmp");
    {
        let mut file = File::create(&tmp)?;
        file.write_all(bytemuck::bytes_of(&file_header))?;
        file.write_all(bytemuck::bytes_of(&anchors))?;
        file.write_all(bytemuck::bytes_of(fingerprint))?;
        file.write_all(&(num_keys as u32).to_le_bytes())?;
        file.write_all(&((K * num_keys) as u32).to_le_bytes())?;
        file.write_all(&key_bytes)?;
        file.write_all(&cdf_bytes)?;
        file.flush()?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
```
(4) Tests in the same file: append `.unwrap()` to both `write_blueprint(...)` calls, and add
```rust
#[cfg(test)]
mod cdf_tests {
    use super::*;
    #[test]
    fn cdf_is_monotone_and_closes_on_last_nonzero_action() {
        let s = [0.2f32, 0.3, 0.5, 0.0, 0.0, 0.0];
        let c = quantize_cdf(&s);
        assert!(c.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(c[2], 255);
        assert_eq!(c[5], 255);
        assert!(c[1] < 255); // trailing zero-prob actions receive no mass
    }
    #[test]
    fn all_zero_is_uniform_and_closed() {
        let c = quantize_cdf(&[0.0; K]);
        assert_eq!(c[K - 1], 255);
    }
}
```

**`main.rs`** — de-duplicate and stop panicking on a full disk. Add this helper next to `save_checkpoint_rolling`:
```rust
fn export_blueprint(
    trainer: &pkr_cfr::Trainer,
    output: &std::path::Path,
    min_visits: f32,
    fingerprint: &pkr_core::abstraction::AbstractionFingerprint,
) -> std::io::Result<usize> {
    let table = trainer.get_table();
    let mut keys = table.get_keys();
    keys.sort_unstable();
    if min_visits > 0.0 {
        keys.retain(|k| {
            table
                .get_average_strategy_slice(*k)
                .map_or(false, |s| s.iter().sum::<f32>() >= min_visits)
        });
    }
    write_blueprint(output.to_str().expect("invalid output path"), table, &keys, fingerprint)?;
    Ok(keys.len())
}
```
Replace the two inline "keys → sort → retain → write_blueprint" sequences:
* promotion branch (inside `else { // Export the current table as the promoted blueprint. … }`):
  ```rust
  match export_blueprint(&trainer, &cli.output, cli.min_visits, &fingerprint) {
      Ok(n) => {
          eprintln!("PROMOTE iter={} expl_mbb={:.2} (prev best {:?}) -> {} ({} infosets)",
              done, br.exploitability_mbb, best_expl_mbb, cli.output.display(), n);
          best_expl_mbb = Some(br.exploitability_mbb);
          promoted = true;
      }
      Err(e) => eprintln!("WARNING: blueprint export failed at iter {done}: {e}"),
  }
  ```
* end-of-run branch (`if !promoted { … }`):
  ```rust
  if !promoted {
      let n = export_blueprint(&trainer, &cli.output, cli.min_visits, &fingerprint)?;
      eprintln!("Blueprint written to {} ({} infosets)", cli.output.display(), n);
  }
  ```
Also remove the unused `use pkr_contracts;` at the top of `main.rs` (clippy `single_component_path_imports`).

### B6. Runtime blueprint reader hardening — `crates/pkr-runtime/src/mmap.rs`

**FIND**
```rust
        // Accept v2 (no anchors) and v3 (anchors section present).
        if file_header.version < FORMAT_VERSION_V2 {
```
**REPLACE**
```rust
        // Accept v2 (no anchors), v3 (anchors), v4 (anchors + fingerprint). Anything
        // newer must NOT be parsed as v4.
        if file_header.version < FORMAT_VERSION_V2 || file_header.version > FORMAT_VERSION_V4 {
```
**FIND**
```rust
            let fp: &pkr_core::abstraction::AbstractionFingerprint = bytemuck::from_bytes(raw);
            Some(*fp)
```
**REPLACE**
```rust
            Some(bytemuck::pod_read_unaligned::<pkr_core::abstraction::AbstractionFingerprint>(raw))
```
**FIND**
```rust
        let offset_keys = after_header + 8;
        let keys_bytes = key_count * 8;
        let offset_cdf = offset_keys + keys_bytes;

        if mmap.len() < offset_cdf + cdf_bytes_len {
            return Err(MmapError::InvalidOffset("data truncated"));
        }
```
**REPLACE**
```rust
        let max_k = file_header.max_actions_k as usize;
        if max_k == 0 || max_k > 16 {
            return Err(MmapError::InvalidOffset("max_actions_k out of range"));
        }
        let keys_bytes = key_count
            .checked_mul(8)
            .ok_or(MmapError::InvalidOffset("key_count overflow"))?;
        if Some(cdf_bytes_len) != key_count.checked_mul(max_k) {
            return Err(MmapError::InvalidOffset("cdf size != key_count * max_actions_k"));
        }
        let offset_keys = after_header + 8;
        let offset_cdf = offset_keys + keys_bytes;
        if mmap.len() < offset_cdf + cdf_bytes_len {
            return Err(MmapError::InvalidOffset("data truncated"));
        }
```
Optionally delete `unsafe impl Send/Sync for MmapReader {}` (`memmap2::Mmap` is already `Send + Sync`).
The existing `test_open_valid_blueprint` builds `keys = 10 * 8`, `cdf = 10 * 3` → still valid.

### B7. One source of truth for legal actions — `crates/pkr-core/src/state.rs`

Training uses `legal_actions_into` (3-raise cap, all-in dedup). `legal_actions()` (the allocating twin,
~90 duplicated lines) has **neither**, and the fuzz harness in `pkr-fuzz` compares the *twin* against its
reference model — so the fuzz tests validate code the trainer never runs.

**Replace the whole body of `pub fn legal_actions(&self) -> Vec<Action>`** with:
```rust
    pub fn legal_actions(&self) -> Vec<Action> {
        let mut buf = [Action { player: 0, kind: ActionKind::Fold }; 8];
        let n = self.legal_actions_into(&mut buf);
        buf[..n].to_vec()
    }
```
**Expected fallout:** `pkr-fuzz` may report an action-set divergence *only* on nodes where
`raises_this_street >= 3` (the reference model still offers raises/all-in there). Fix by applying the same
cap in the reference model (`MAX_RAISES_PER_STREET = 3`) — do **not** revert this change.
Run `cargo test -p pkr-fuzz --release` and fix until green. `pkr-exploit/public_br.rs::PublicState` has its own
`legal_actions`; it is marked WIP/unused, leave it.

### B9. GPU path: don't compile it into the CPU build, don't `chunks(0)`

`flush_gpu_batch` with the stub has `max_batch_size() == 0` → `deduped.chunks(0)` **panics**. The WGSL shader
also still uses `array<i32>` and its own buffers (never seeded from the CPU table), so it cannot be a drop-in for
the i64 table anyway. It is dead in production.

**`table.rs`**:
* gate the import: `#[cfg(feature = "gpu")] use crate::gpu::GpuState;` and keep `use crate::gpu::BatchItem;`
* gate the field: `#[cfg(feature = "gpu")] gpu: OnceLock<GpuState>,` and in `with_capacity`
  `#[cfg(feature = "gpu")] gpu: OnceLock::new(),`
* gate the method: put `#[cfg(feature = "gpu")]` on `pub fn flush_gpu_batch`, and add `assert!(max > 0)` before `deduped.chunks(max)`.
* add to the top-of-file comment: *"GPU path is i32-only and unmaintained; it does not mirror the i64 table."*

**`crates/pkr-cfr/Cargo.toml`**
```toml
pollster = { version = "0.3", optional = true }
...
[features]
default = []
gpu = ["wgpu", "pollster"]
```

### B10. River table miss must be counted — `crates/pkr-abstraction/src/lib.rs`

In the `5 => { … }` river arm the `board_bucket` falls to `0` silently when the table is missing or the
index is out of range, defeating the C5c "abort on fallback" guard.

**FIND**
```rust
                let board_bucket = if let Some(table) = self.tables.get(&3u8).and_then(|l| l.get())
                {
                    let idx = Self::flat_index_river_board(board);
                    if idx < table.len() {
                        table[idx] as u64
                    } else {
                        0
                    }
                } else {
                    0
                };
```
**REPLACE**
```rust
                let board_bucket = match self.tables.get(&3u8).and_then(|l| l.get()) {
                    Some(table) => {
                        let idx = Self::flat_index_river_board(board);
                        if idx < table.len() {
                            table[idx] as u64
                        } else {
                            FALLBACK_COUNTS[3].fetch_add(1, Ordering::Relaxed);
                            0
                        }
                    }
                    None => {
                        FALLBACK_COUNTS[3].fetch_add(1, Ordering::Relaxed);
                        0
                    }
                };
```
Unit tests that build an abstraction without a river table are unaffected (the trainer, not the
abstraction, aborts, and only when `PKR_ALLOW_EHS_FALLBACK != 1`). Also fix the stale test comment
`"river >> 3"` in `test_history_street_hash_golden` (the code uses `>> 15`).

**Gate B**
```bash
cargo fmt --all
cargo build --workspace --release
cargo test --workspace --release
cargo clippy --workspace --all-targets -- -D warnings
./smoke.sh          # end-to-end: precompute → train 10 iters → export → runtime load
```

---

## PHASE C — make the exploitability number trustworthy

File: `crates/pkr-exploit/src/best_response.rs` (+ two call sites in `main.rs`).

**Why (all three bugs bias the metric you are steering by):**

* **C1 over-fit (biased high).** Policy improvement (`br_action[h] = argmax cfv[h]`) is computed from the
  same 2000 deals that pass 3 then scores. Each abstract infoset is seen by only a few deals, so the argmax
  is fitted to noise. Fix: fit on a *train* deal set, score on a disjoint *held-out* set (held-out is
  unbiased-to-slightly-low; keep the in-sample number only as a diagnostic).
* **C2a illegal buckets win (biased low).** `cfv` entries are initialised to `0.0`; if every legal action has
  negative value (common: chips lost), an *illegal* bucket with `0.0` wins the argmax and the walker then
  falls back to something else.
* **C2b default action.** Unknown infoset ⇒ bucket 0 (Fold) in pass 1 and in `walk_fixed`. Use the blueprint's
  own argmax instead.
* **C2c under-convergence (biased low).** 3 passes propagate information only 3 decision levels up the tree;
  a hand has many more BR decisions on a path. Iterate until the policy stops changing (cap 12).
* **C3 noise.** `seed = done` ⇒ a *different* deal set at every eval; use a fixed seed (common random numbers)
  so successive points are comparable, and make the promote gate at least 2 standard errors.

### C-edits in `best_response.rs`

1. **Iteration count.** In `br_iterations()` change `.unwrap_or(3)` → `.unwrap_or(12)` and the filter to
   `(1..=100)` (unchanged). The loop now exits early (below).

2. **Add** (near `mask`):
   ```rust
   /// BR action choice: the learned BR action if it is legal here, otherwise the
   /// blueprint's own most-likely legal action (never a blind "Fold").
   fn br_choice(
       br_action: &HashMap<u64, u8>,
       hash: u64,
       counts: &[usize; K],
       table: &CompactRegretTable,
   ) -> usize {
       if let Some(&a) = br_action.get(&hash) {
           if counts[a as usize] > 0 {
               return a as usize;
           }
       }
       let mut raw = [0.0f32; K];
       table.get_average_strategy_into(hash, &mut raw);
       let strat = mask(raw, counts);
       let mut best = 0usize;
       let mut best_p = -1.0f32;
       for a in 0..K {
           if counts[a] > 0 && strat[a] > best_p {
               best_p = strat[a];
               best = a;
           }
       }
       best
   }
   ```

3. **`collect_cfv`, BR-seat branch.**
   FIND
   ```rust
                child_values[a] = cv;
                cfv.entry(hash).or_insert([0.0; K])[a] += cv * deal_prior;
   ```
   REPLACE
   ```rust
                child_values[a] = cv;
                let e = cfv.entry(hash).or_insert([f64::NEG_INFINITY; K]);
                if e[a] == f64::NEG_INFINITY {
                    e[a] = 0.0; // "seen legal at least once"
                }
                e[a] += cv * deal_prior;
   ```
   FIND
   ```rust
        let chosen = br_action.get(&hash).copied().unwrap_or(0) as usize;
        let ret = child_values[chosen];
   ```
   REPLACE
   ```rust
        let chosen = br_choice(br_action, hash, &counts, table);
        let ret = child_values[chosen];
   ```

4. **`walk_fixed`, BR-seat branch.**
   FIND
   ```rust
        if br_action.get(&hash).is_none() {
            MISSING_POLICY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let a = br_action.get(&hash).copied().unwrap_or(0) as usize;
        let a = if counts[a] == 0 {
            (0..K).find(|&x| counts[x] > 0).unwrap_or(0)
        } else {
            a
        };
   ```
   REPLACE
   ```rust
        if !br_action.contains_key(&hash) {
            MISSING_POLICY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let a = br_choice(br_action, hash, &counts, table);
   ```

5. **Replace `BrResult`, `sampled_exploitability` and `sampled_br_one_seat`** with the following (keep `sample_deal`, the tests, and — if you like them — the `BR-DEBUG` prints, computed from `ev0/ev1`):

   ```rust
   /// Result of a sampled exploitability run. `exploitability_mbb`, `expl_std_err_mbb`,
   /// `br0`, `br1` are HELD-OUT (fit on train deals, scored on disjoint deals).
   #[derive(Debug, Clone, Copy)]
   pub struct BrResult {
       pub exploitability_mbb: f64,
       pub expl_std_err_mbb: f64,
       /// Score on the deals the BR was fitted on. Biased high; diagnostic only.
       pub expl_insample_mbb: f64,
       pub br0: f32,
       pub br1: f32,
       pub deals_sampled: u32,
   }

   pub fn sampled_exploitability(
       table: &CompactRegretTable,
       abstraction: &dyn AbstractionBuilder,
       evaluator: &dyn Evaluator,
       deals: u32,
       seed: u64,
   ) -> BrResult {
       let mk = |salt: u64| -> Vec<u64> {
           (0..deals as u64)
               .map(|i| {
                   (seed ^ salt)
                       .wrapping_add(i)
                       .wrapping_mul(0x9E3779B97F4A7C15)
                       .wrapping_add(1)
               })
               .collect()
       };
       let train_seeds = mk(0x7A11_7A11_7A11_7A11);
       let eval_seeds = mk(0xE7A1_E7A1_E7A1_E7A1);

       MISSING_POLICY.store(0, std::sync::atomic::Ordering::Relaxed);
       let ((tr0, ev0), (tr1, ev1)) = rayon::join(
           || sampled_br_one_seat(table, abstraction, evaluator, &train_seeds, &eval_seeds, 0),
           || sampled_br_one_seat(table, abstraction, evaluator, &train_seeds, &eval_seeds, 1),
       );

       let mean = |v: &[f32]| v.iter().map(|&x| x as f64).sum::<f64>() / v.len().max(1) as f64;
       let (br0, br1) = (mean(&ev0), mean(&ev1));
       let insample = (mean(&tr0) + mean(&tr1)) * 250.0;

       let n = ev0.len();
       let m = br0 + br1;
       let sem = if n > 1 {
           let var = ev0
               .iter()
               .zip(ev1.iter())
               .map(|(a, b)| {
                   let e = (*a as f64 + *b as f64) - m;
                   e * e
               })
               .sum::<f64>()
               / (n - 1) as f64;
           (var / n as f64).sqrt()
       } else {
           0.0
       };

       // chips -> mbb: (br0+br1)/2 chips per player, /2 (BB=2), x1000  =>  x250
       BrResult {
           exploitability_mbb: m * 250.0,
           expl_std_err_mbb: sem * 250.0,
           expl_insample_mbb: insample,
           br0: br0 as f32,
           br1: br1 as f32,
           deals_sampled: deals,
       }
   }

   /// Returns (values on TRAIN deals, values on HELD-OUT deals) for `br_seat`.
   fn sampled_br_one_seat(
       table: &CompactRegretTable,
       abstraction: &dyn AbstractionBuilder,
       evaluator: &dyn Evaluator,
       train_seeds: &[u64],
       eval_seeds: &[u64],
       br_seat: usize,
   ) -> (Vec<f32>, Vec<f32>) {
       let deal_prior = 1.0 / train_seeds.len().max(1) as f64;
       let mut br_action: HashMap<u64, u8> = HashMap::new();

       for _iter in 0..br_iterations() {
           let chunks: Vec<HashMap<u64, [f64; K]>> = train_seeds
               .par_iter()
               .map(|&seed| {
                   let mut rng = SmallRng::seed_from_u64(seed);
                   let (hero, villain, deck) = sample_deal(&mut rng);
                   let ranks = DealRanks::new(evaluator, &hero, &villain, &deck[4..9]);
                   let mut state = GameState::new(200.0, 1.0, 2.0);
                   state.set_hole_cards(hero, villain);
                   let mut deck_idx = 4usize;
                   let mut local_cfv: HashMap<u64, [f64; K]> = HashMap::new();
                   collect_cfv(
                       &mut state, table, abstraction, br_seat, &br_action,
                       &mut local_cfv, deal_prior, &deck, &mut deck_idx, 0, &ranks,
                   );
                   local_cfv
               })
               .collect();

           let mut cfv: HashMap<u64, [f64; K]> = HashMap::new();
           for chunk in chunks {
               for (k, v) in chunk {
                   let e = cfv.entry(k).or_insert([f64::NEG_INFINITY; K]);
                   for a in 0..K {
                       e[a] = match (e[a] == f64::NEG_INFINITY, v[a] == f64::NEG_INFINITY) {
                           (true, _) => v[a],
                           (_, true) => e[a],
                           _ => e[a] + v[a],
                       };
                   }
               }
           }

           let mut changed = 0usize;
           for (hash, vals) in cfv {
               let mut best_a = 0u8;
               let mut best_v = f64::NEG_INFINITY;
               for a in 0..K {
                   if vals[a] > best_v {
                       best_v = vals[a];
                       best_a = a as u8;
                   }
               }
               if br_action.insert(hash, best_a) != Some(best_a) {
                   changed += 1;
               }
           }
           if changed == 0 {
               break; // policy is a fixed point
           }
       }

       let value = |seeds: &[u64]| -> Vec<f32> {
           seeds
               .par_iter()
               .map(|&seed| {
                   let mut rng = SmallRng::seed_from_u64(seed);
                   let (hero, villain, deck) = sample_deal(&mut rng);
                   let ranks = DealRanks::new(evaluator, &hero, &villain, &deck[4..9]);
                   let mut state = GameState::new(200.0, 1.0, 2.0);
                   state.set_hole_cards(hero, villain);
                   let mut deck_idx = 4usize;
                   walk_fixed(
                       &mut state, table, abstraction, br_seat, &br_action,
                       &deck, &mut deck_idx, 0, &ranks,
                   ) as f32
               })
               .collect()
       };
       (value(train_seeds), value(eval_seeds))
   }
   ```

### C-edits in `main.rs`

Add a constant near the top: `const EVAL_SEED: u64 = 0xE7A1_0000_0000_0001;`

* Both calls to `sampled_exploitability(..., cli.eval_deals, start_iter as u64)` / `(..., done as u64)` →
  pass `cli.seed ^ EVAL_SEED` as the last argument (same deals at every eval → comparable points).
* Both `EVAL iter=…` prints: append `insample={:.2}` with `br.expl_insample_mbb`.
* Promotion gate. **FIND**
  ```rust
                let rejected = match best_expl_mbb {
                    Some(b) => br.exploitability_mbb > b + cli.promote_gate,
                    None => false,
                };
  ```
  **REPLACE**
  ```rust
                // Gate must exceed the measurement noise, else we chase winner's-curse minima.
                let gate = cli.promote_gate.max(2.0 * br.expl_std_err_mbb);
                let rejected = match best_expl_mbb {
                    Some(b) => br.exploitability_mbb > b + gate,
                    None => false,
                };
  ```
  and use `gate` in the `SKIP-PROMOTE` message.
* Change the `--eval-deals` default from `2000` to `10000` (BR fitting needs many more deals than the
  old in-sample scoring did; cost is dominated by the fit passes).

**Gate C (sanity, manual)**
```bash
pkr-trainer ... --eval-now --eval-deals 10000 --checkpoint <any ckpt>
# Expect: insample >= expl_mbb (the gap is the over-fit you were reading as exploitability),
# and the gap shrinks as --eval-deals grows. Run it twice: identical numbers (fixed seed).
```
**Re-baseline** every previous "expl_mbb" figure with this estimator before comparing to targets.

---

## PHASE D — convergence-quality levers (behind flags, default = unchanged)

### D1. `PKR_MOMENTUM` (already wired by A3 + B3)

Current update (per cell): `pred = (1-γ)·m_prev + γ·δ`, then **`r' = max(0, disc(r) + pred)`**, `m' = pred`, `γ = 1/√(t+1)`.
That adds the *prediction* to the stored regret. Real PCFR+ accumulates the true increment
(`r' = [r + δ]⁺`) and uses the prediction only when *computing the strategy*. At t = 10⁶, γ = 10⁻³, so each
visit adds ≈ the EMA of old deltas and only 0.1 % of the fresh one; with rare visits this delays and smooths
the signal. It may be fine, may be the reason v17 plateaued — **measure it**:

`PKR_MOMENTUM=0 pkr-trainer …` runs plain CFR+/DCFR. Add to the CHANGELOG that this is the A/B knob.

### D2. Strategy averaging weight `t^p` (`PKR_AVG_POWER`, default 0 = uniform = current)

DCFR/CFR+/Linear-CFR average with weight `t^γ` (γ = 2 for DCFR, 1 for CFR+/Linear). The code comment in
`dcfr.rs` ("cumulative effect of γ=2 < 0.1 %") is measuring a *different* formula (`(t/τ)^γ/((t/τ)^γ+1)`),
not the paper's per-iteration `(t/(t+1))^γ`, whose product telescopes to weight `∝ t^γ` — a large effect that
removes early-iteration garbage from the exported strategy.

**`traversal.rs` — add** below `exploration_epsilon`:
```rust
/// PKR_AVG_POWER=p → strategy-sum weight t^p (0 = uniform, 1 = linear, 2 = DCFR gamma).
fn avg_weight_power() -> f32 {
    use std::sync::OnceLock;
    static P: OnceLock<f32> = OnceLock::new();
    *P.get_or_init(|| {
        std::env::var("PKR_AVG_POWER")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|p| (0.0..=4.0).contains(p))
            .unwrap_or(0.0)
    })
}

#[inline]
fn avg_weight(t: u32) -> f32 {
    let p = avg_weight_power();
    if p == 0.0 {
        1.0
    } else if p == 1.0 {
        t as f32
    } else if p == 2.0 {
        let x = t as f32;
        x * x
    } else {
        (t as f32).powf(p)
    }
}
```
**FIND**
```rust
    if let Some(idx) = traverser_idx {
        for a in 0..K {
            if strategy[a] <= 0.0 {
                continue;
            }
            strategy_batch.push(StrategyOp {
                index: idx as u32,
                action: a as u8,
                prob: strategy[a] * reach_prob,
            });
        }
    }
```
**REPLACE**
```rust
    if let Some(idx) = traverser_idx {
        let w_avg = avg_weight(global_iteration);
        for a in 0..K {
            if strategy[a] <= 0.0 {
                continue;
            }
            strategy_batch.push(StrategyOp {
                index: idx as u32,
                action: a as u8,
                prob: strategy[a] * reach_prob * w_avg,
            });
        }
    }
```
Ranges are safe (`t ≤ 4.3e9`, `p ≤ 4` ⇒ ≤ 3.4e38 only at p=4/t=4e9; the flag is capped at p ≤ 4, recommended 1–2; sums are f64).

**Caveats to document in the CHANGELOG:**
* `--min-visits` compares the *strategy-sum mass* to a threshold; with `p > 0` that mass is `t^p`-weighted, so the
  threshold no longer means "visits". Keep `min_visits = 0` when using `p > 0`.
* `strategy_sum_mass` in the metrics CSV changes scale.
* Do **not** resume a `p = 0` checkpoint with `p > 0`; start fresh.

### D3. Known limitations (documented, not changed)

* **ε-uniform opponent sampling (5 %)** trains the traverser against an ε-perturbed opponent (biased
  equilibrium of a slightly different game). It fixes real reachability freezes, and the *average* strategy is
  taken at traverser nodes only (unperturbed). A cleaner variant is an importance weight `σ(a)/σ_ε(a)` on the
  sampled child; test before adopting.
* **Raise sizing semantics:** raise total = `opp_bet + pot·frac` where `pot` excludes the call. The 0.5× preflop
  raise from the SB is `3.5` (< min-raise `4`). Fine inside the abstraction, but a **live host must clamp to the
  game's legal min-raise** and treat the bucket as "≥ this size".
* **Signature v1 aliasing:** infosets ignore bet size faced and SPR (`SIG_V2_STREET_MONEY = false`), so "facing 0.5×"
  and "facing 2×" share a strategy. After D2 this is the next biggest quality lever (capacity ×6 per your notes).

---

## PHASE E — performance (all bit-safe unless stated)

### E1. Exact u64 discount — done in A3 (removes an `i128` div per cell).

### E2. Drop the redundant `/SCALE` in regret matching, dedupe the two copies

Regret matching normalises, so dividing by `SCALE` first is pointless. Add to `impl CompactRegretTable`:
```rust
    /// Regret-matching+ strategy from the stored regrets (uniform if none positive).
    #[inline(always)]
    fn regret_match_into(&self, idx: usize, out: &mut [f32; K]) {
        let mut sum = 0.0f32;
        for i in 0..K {
            let raw = self.load_rm(idx, i, RM_REGRET);
            let v = if raw > 0 { raw as f32 } else { 0.0 };
            out[i] = v;
            sum += v;
        }
        if sum > 0.0 {
            let inv = 1.0 / sum;
            for i in 0..K {
                out[i] *= inv;
            }
        } else {
            out.fill(1.0 / K as f32);
        }
    }
```
* In `get_strategy_and_idx`: replace everything after `let idx = match cache_lookup(...) {...};` with
  `self.regret_match_into(idx, out); idx`.
* In `get_strategy_into`: replace the `if let Some(idx) = idx_opt { … } else { out.fill(…) }` body with
  `if let Some(idx) = idx_opt { self.regret_match_into(idx, out); } else { out.fill(1.0 / K as f32); }`.
* In `compute_export_strategy`: load the sums once, then use the helper for the fallback:
  ```rust
        let mut sums = [0.0f64; K];
        for i in 0..K {
            sums[i] = self.load_sum(idx, i);
        }
        let sum: f64 = sums.iter().sum();
        if sum > 0.0 {
            let inv = 1.0 / sum;
            for i in 0..K {
                out[i] = (sums[i] * inv) as f32;
            }
            return out;
        }
        self.regret_match_into(idx, &mut out); // regret-matched, or uniform if flat
        out
  ```
Note: results can differ in the last ulp from previous runs (division order) → a run is not bit-identical to v20.

### E3. Skip forced-move nodes (`PKR_SKIP_FORCED=1`, default off)

After an all-in call the remaining streets are check/check with **one legal action** each. Today each such node
computes the infoset hash (incl. river `evaluate_hand`), allocates a table slot, and pushes zero-delta regret
items and strategy ops. Skipping them removes work and frees capacity.

**`traversal.rs` — add**
```rust
fn skip_forced_nodes() -> bool {
    use std::sync::OnceLock;
    static S: OnceLock<bool> = OnceLock::new();
    *S.get_or_init(|| std::env::var("PKR_SKIP_FORCED").as_deref() == Ok("1"))
}
```
**Insert** immediately after the `for (idx, action) in num_actions.iter().enumerate() { … }` bucket loop and **before**
`// Compact history signature`:
```rust
    // Forced move: a single legal bucket has no decision, no regret, no strategy.
    // Recurse without touching the table.
    let n_legal_buckets = action_counts.iter().filter(|&&c| c > 0).count();
    if n_legal_buckets == 1 && skip_forced_nodes() {
        let a = (0..K).find(|&a| action_counts[a] > 0).unwrap();
        let count = action_counts[a];
        let pick = if count > 1 { rng.random_range(0..count) } else { 0 };
        current.apply_action_in_place(&num_actions[action_indices[a][pick]]);
        let child_deck_idx = *deck_idx;
        let v = traverse(
            current, table, abstraction, evaluator, rng, global_iteration, traverser,
            reach_prob, deck, &mut *deck_idx, depth + 1, batch, strategy_batch, metrics,
        );
        *deck_idx = child_deck_idx;
        current.undo_action();
        if advanced {
            current.undo_action();
        }
        *deck_idx = saved_deck_idx;
        return v;
    }
```
**Consequences:** forced-move infosets will not exist in the blueprint → runtime `get_advice_fast` returns
`None` for them. The host must not require a hit when only one action is legal (mask the 6-bucket
`fallback_advice` by legality, which it must do anyway). Changes RNG streams → not comparable to v20 bit-for-bit.

### E4. Smaller items
* `crates/pkr-cfr/src/valuenet.rs::forward`: replace `vec![0.0f32; HIDDEN1]` / `vec![0.0f32; HIDDEN2]` with
  stack arrays `[0.0f32; HIDDEN1]` / `[0.0f32; HIDDEN2]` (no heap alloc per inference).
* Strategy-op sort key (bit-identical grouping): in `apply_strategy_batch` use
  `ops.par_sort_unstable_by_key(|op| ((op.index as u64) << 8) | op.action as u64);`
* `Cargo.toml` / `pollster`: done in B9.
* `CompactRegretTable::len()` is O(n) on papaya; do not call it in loops (use `allocated()`).

---

## Experiment plan (after Phases A–C)

Same `--seed`, same `--eval-deals 10000`, `--eval-every 1000000`, 10 M iterations each:

| Run | Flags | Question |
|-----|-------|----------|
| v20  | none (i64 + fixes) | Is saturation gone? Does held-out expl still plateau? |
| v21a | `PKR_MOMENTUM=0` | Is the momentum-as-increment update hurting? |
| v21b | `PKR_AVG_POWER=1` | Does linear averaging help the exported blueprint? |
| v21c | `PKR_AVG_POWER=2` | DCFR γ=2 |
| v21d | `PKR_SKIP_FORCED=1` | Throughput / capacity gain, no quality change expected |

Decide by **held-out** `expl_mbb ± SE` (not in-sample). Old absolute targets (<3000 @ 5 M …) were calibrated
on the biased estimator — re-derive them from v20 under the new one.

**v20 sanity signals (unchanged from the other agent's list, with corrected metric):**
* `max|r|` (regret-only now) < 1e6 in the first 500 K iterations; passes 2.15e6 by ~10 M ⇒ i64 is doing its job.
* `nonfinite=0` throughout; RSS ≈ +72 MB vs v19 at 1.5 M infosets; checkpoint ≈ 190 MB.
* `it/s` within −10 % of v19 (E1 should claw back most of it).

## Final full gate
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release
./smoke.sh
git diff --stat   # confirm only files named above changed
```
Add to `CHANGELOG.md`: *i64 regret table, checkpoint v7 (v6 rejected), FlushMode + PKR_MOMENTUM, held-out BR,
atomic blueprint write, pruning removed, snapshot regret-only, `--fresh`, Ctrl-C/capacity checkpoints.*
