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

---

## Second round (as the A/B kept running)

### Bug 6 — masked opponent range could be all-zero

`crates/pkr-runtime/src/vsearch.rs`

`VSearchHandle::decide` masked board and hero cards out of the
opponent's range but never checked the **masked** mass. If masking
removed every hand with weight (degenerate input, or a caller that
only passed our-cards hands), the solver got an all-zero range and
returned a uniform strategy — indistinguishable from a real answer.
The sampled `SubgameHandle` guards the same way; the vector path
didn't. Fixed with an `opp_mass <= 1e-9 → None` early return.

Regression test: `vsearch_handle_returns_none_on_empty_masked_range`.

### Bug 7 — our own actions were translated, not applied

`crates/pkr-runtime/src/session.rs`

`RuntimeSession::observe_action` routed **both** sides through
`Shadow::observe_opp`, which runs a pseudo-harmonic translation over
off-tree bet sizes. For an opponent's off-tree bet that's correct.
For **our own** action it is wrong: we picked a specific bucket from
the blueprint, and the shadow should advance with *that bucket*,
snapped to the shadow's legal set — not re-randomized. In-tree bets
happen to round-trip (bucket 3 = 1.0 pot → x=1.0 → deterministic 3),
but any off-tree resolution (host clamp, alternative bucket-to-chips
convention) could snap our own bucket to a neighbour and desync the
shadow state used for every subsequent hash.

`Shadow::observe_own_bucket` existed for exactly this and was **dead
code** (no callers). Now wired.

### Bug 8 — `observe_own_bucket` fallback folded

`crates/pkr-runtime/src/shadow.rs`

With bug 7 fixed, a latent defect in `observe_own_bucket` became
load-bearing: it looked for a legal action with the exact requested
bucket, and on failure fell back to `buf[0]` — which is **Fold** when
facing a bet and **Check** otherwise. A bucket-5 (jam) request in a
shadow whose pot had shrunk (because the opponent's off-tree bet
translated to a smaller in-tree size) would silently fold the shadow
out of the hand. The fallback is now the **closest legal bucket**,
with a deterministic tie-break toward the higher (more aggressive)
bucket.

Regression test: `observe_own_bucket_picks_closest_when_exact_missing`.

### Bug 9 — LBR hook got an unmasked opponent range

`crates/pkr-exploit/src/lbr.rs`

The LBR seat's own best-response walk computes a `masked_posterior`
(hero cards + board removed) at every decision. The **hook** path (the
bot side, where search is substituted) passed the **raw** tracker
range. The tracker does not enforce card removal, so the two sides of
the same measurement saw different opponent ranges, and the two
`SearchHandle` implementations responded differently to the unmasked
input:

- `SubgameHandle` sampled from it and let the solver's `incompatible`
  check reject overlapping deals → a rejection-sampled distribution.
- `VSearchHandle` masked internally → the masked distribution.

So LBR-vs-vsearch and LBR-vs-sampled could not be compared cleanly.
The hook now receives the same `masked_posterior` the BR path uses.

### Not bugs (second pass)

- `legal_actions_into` ordering (Check/Call first when not facing,
  Fold first when facing) is not relied on by any caller's correctness
  — every consumer masks by legal *bucket*, not buffer position.
- `pkr-bot-pipe` and `bot_loop` example remain compilable and correct.
- `--fresh` guard at 1 MB threshold, with `PKR_FORCE_FRESH=1` escape,
  matches the handoff §5 description.

---

## Second round (as the A/B kept running)

### Bug 6 — masked opponent range could be all-zero

`crates/pkr-runtime/src/vsearch.rs`

`VSearchHandle::decide` masked board and hero cards out of the
opponent's range but never checked the **masked** mass. If masking
removed every hand with weight (degenerate input, or a caller that
only passed our-cards hands), the solver got an all-zero range and
returned a uniform strategy — indistinguishable from a real answer.
The sampled `SubgameHandle` guards the same way; the vector path
didn't. Fixed with an `opp_mass <= 1e-9 → None` early return.

Regression test: `vsearch_handle_returns_none_on_empty_masked_range`.

### Bug 7 — our own actions were translated, not applied

`crates/pkr-runtime/src/session.rs`

`RuntimeSession::observe_action` routed **both** sides through
`Shadow::observe_opp`, which runs a pseudo-harmonic translation over
off-tree bet sizes. For an opponent's off-tree bet that's correct.
For **our own** action it is wrong: we picked a specific bucket from
the blueprint, and the shadow should advance with *that bucket*,
snapped to the shadow's legal set — not re-randomized. In-tree bets
happen to round-trip (bucket 3 = 1.0 pot → x=1.0 → deterministic 3),
but any off-tree resolution (host clamp, alternative bucket-to-chips
convention) could snap our own bucket to a neighbour and desync the
shadow state used for every subsequent hash.

`Shadow::observe_own_bucket` existed for exactly this and was **dead
code** (no callers). Now wired.

### Bug 8 — `observe_own_bucket` fallback folded

`crates/pkr-runtime/src/shadow.rs`

With bug 7 fixed, a latent defect in `observe_own_bucket` became
load-bearing: it looked for a legal action with the exact requested
bucket, and on failure fell back to `buf[0]` — which is **Fold** when
facing a bet and **Check** otherwise. A bucket-5 (jam) request in a
shadow whose pot had shrunk (because the opponent's off-tree bet
translated to a smaller in-tree size) would silently fold the shadow
out of the hand. The fallback is now the **closest legal bucket**,
with a deterministic tie-break toward the higher (more aggressive)
bucket.

Regression test: `observe_own_bucket_picks_closest_when_exact_missing`.

### Bug 9 — LBR hook got an unmasked opponent range

`crates/pkr-exploit/src/lbr.rs`

The LBR seat's own best-response walk computes a `masked_posterior`
(hero cards + board removed) at every decision. The **hook** path (the
bot side, where search is substituted) passed the **raw** tracker
range. The tracker does not enforce card removal, so the two sides of
the same measurement saw different opponent ranges, and the two
`SearchHandle` implementations responded differently to the unmasked
input:

- `SubgameHandle` sampled from it and let the solver's `incompatible`
  check reject overlapping deals → a rejection-sampled distribution.
- `VSearchHandle` masked internally → the masked distribution.

So LBR-vs-vsearch and LBR-vs-sampled could not be compared cleanly.
The hook now receives the same `masked_posterior` the BR path uses.

### Not bugs (second pass)

- `legal_actions_into` ordering (Check/Call first when not facing,
  Fold first when facing) is not relied on by any caller's correctness
  — every consumer masks by legal *bucket*, not buffer position.
- `pkr-bot-pipe` and `bot_loop` example remain compilable and correct.
- `--fresh` guard at 1 MB threshold, with `PKR_FORCE_FRESH=1` escape,
  matches the handoff §5 description.
