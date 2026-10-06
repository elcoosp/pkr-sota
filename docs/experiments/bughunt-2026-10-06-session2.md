# Bug hunt — 2026-10-06 (session 2)

While the V4/V5 A/B ran, audited the search-integration code path that
session 1 added, plus every binary that loads a checkpoint. Found and
fixed 5 real defects.

## Bug 1 — CRITICAL: vsearch flop hang

**File:** `crates/pkr-runtime/src/vsearch.rs`, `crates/pkr-runtime/src/session.rs`

**Symptom:** With `PKR_VSEARCH=1`, reaching a **flop** decision hangs (or
OOMs). A flop solve enumerates 47 turn cards × 46 river cards = 2162
full river subgames, each a 1326-hand showdown — an easy multi-minute
hang per decision.

**Root cause (two parts):**

1. `solve_root` (the free function) did **not** check
   `VSearchCfg.enabled_streets`. The gate existed only in
   `VSearchHandle::decide`, so callers using the free function bypassed
   it.
2. `RuntimeSession::advise` gated on `st.street != Street::Preflop` —
   "postflop" as a proxy for "enabled". The default `VSearchCfg` enables
   only turn+river, but `!= Preflop` also lets flop through.

**Fix:**
- `solve_root` now returns `None` if
  `!cfg.enabled_streets[st.street as usize]`, before touching the tree.
- `advise` reads `VSearchCfg::default().enabled_streets` and gates on the
  actual config, not a street comparison.
- Regression test `solve_root_refuses_flop` in `vsearch.rs`.

**Commit:** `a7c5b44`

## Bug 2 — tournament bin had no `--sig` flag

**File:** `crates/pkr-fuzz/src/bin/tournament.rs`

**Symptom:** `target/release/tournament --a <v4.ckpt>` without
`PKR_SIG_V4=1` exported aborts with a bare `sig_version: 4 vs 3` message.
The env-var-only interface is easy to miss and produces an unhelpful
error.

**Fix:** Added `--sig N` (default 3), which sets `PKR_SIG_V3/V4/V5`
before any `OnceLock` fingerprint read. Mirrors the existing flag on
`lbr_eval`.

**Commit:** `777f152`

## Bug 3 — arena bin had no `--sig` flag

**File:** `crates/pkr-fuzz/src/bin/arena.rs`

Same trap as tournament. Same fix, same commit pattern.

**Commit:** `9889552`

## Bug 4 — expl_eval bin had no `--sig` flag

**File:** `crates/pkr-exploit/src/bin/expl_eval.rs`

Same trap again. Same fix.

**Commit:** `d2852d3`

## Bug 5 — `pkr-bot-pipe --sig` was a no-op

**File:** `crates/pkr-runtime/src/bin/pkr-bot-pipe.rs`

The binary documented `--sig N` in its usage string and parsed it into a
variable named `_sig` — then never used it. A user passing `--sig 4` got
the default (V3) hashing and would see all-zero blueprint hits with no
error.

**Fix:** The flag now sets the `PKR_SIG_V*` env vars before any
signature/fingerprint call.

**Commit:** `d2852d3`

## Not bugs (verified)

- **`range_tracker::index_of_hole` vs `vector::index_of`** — identical
  colex ordering `C(hi,2)+lo`. `hole_at(idx)` and `hole_table()[idx]`
  agree for every idx. This is the correctness foundation of the trait
  hand-off; both were correct.
- **`sample_bucket` all-zero fallback** — returns 0 (fold). Matches
  `decide_from_blueprint`'s "uniform over legal" behaviour in spirit
  (picks *an* action, not random); no divergence from the blueprint's
  own default.
- **`action_bucket` bet semantics** — "raise-above-call over pre-call
  pot" is consistent across `abstraction.rs`, `shadow.rs`,
  `translate_live.rs`, `vsearch.rs`. Documented, uniform.
- **`AdviceContext.search` ordering in tournament** — `ctx_seat0` picks
  the correct `search_hero`/`search_villain` for whichever seat hero is
  in. Correct.
- **Fingerprint normalizations** (`stack_bb_div2==0 → 50`,
  `bucket_width==0 → 1`) — handle pre-field checkpoints correctly.

## Not investigated (out of scope for this session)

- CFR-D gadget correctness (`safe_resolve_gadget`). Untested path; the
  vector solver does not use it.
- `binaries/pkr-trainer/src/main.rs` env-var handling beyond the
  fingerprint fields. The trainer's flags are already well-tested via
  the A/B runs.
- Full `pkr-subgame` tests/ directory — mostly integration smoke tests
  that require fixture tables.
