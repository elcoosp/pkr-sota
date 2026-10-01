# HANDOFF — audit follow-up + bug hunt session (2026-10-01, late)

**Continues:** `docs/HANDOFF_2026-10-01_audit-followup.md`
**HEAD at handoff:** see `git log -1`
**Tree:** clean

---

## Section 1 — Running at handoff

**v45 training** — F4 potential-feature A/B against v42.

```
outputs/v45-potential/
  started: 2026-10-01 ~15:03
  30M iters, seed 42, 8 threads, plateau-stop 5, promote-min-sigma 2
  best so far: 3392.3 mbb @ 3M
```

Watcher armed at `/tmp/v45-watch.sh` (PID logged by `pgrep -f v45-watch`).
Result lands in `/tmp/v45-result.txt` when training completes. The
watcher writes the CSV, the stats.json env, and runs `pkr-arena` on
the final checkpoint.

**Decision rule when v45 finishes:**

| v45 best vs v42 best (3313.4 ± 130) | verdict |
|---|---|
| v45 < 3313.4 - 260 | F4 feature wins — ship `PKR_CENTROID_FEATURE_V=1` default |
| within ±260 | equivalent — keep legacy (more checkpoints match) |
| v45 > 3313.4 + 260 | F4 hurts — revert the F4 direction |

---

## Section 2 — This session's actual wins

### Bug hunt — 3 real bugs found and fixed

All three are **real bugs** in shipped code that would have fired in
configs the project hasn't exercised yet (mostly turn subgame
solving). Each landed with a commit and a test.

**Bug 1 — `RangeTracker` fallback violated board exclusion**
(`crates/pkr-subgame/src/range_tracker.rs`, commit `7cb8780`)

When `update_range_for_action` collapsed the actor's range to near-zero
sum, the fallback set every entry to `1/N_HANDS` — including hands that
share a card with the board. That violates the tracker's own invariant.
`sample_hands_weighted` would then sample an impossible "opponent"
hand and the solver would evaluate a deal that can't happen.

Fix: fallback is uniform over board-free hands only, zero elsewhere.

Not caught by existing tests because average strategies always leave
mass on legal buckets, so the fallback never fires on real blueprints.

**Bug 2 — `TableProvider` zero-sum CDF mismatched the exporter**
(`crates/pkr-fuzz/src/provider.rs`, commit `bc5ce4e`)

When `get_average_strategy_slice` returned a zero-sum strategy, the
encoder wrote all-zero CDF bytes. A consumer decodes that as "bucket 0
gets 100% of the mass". The exporter (`pkr_export::writer::quantize_cdf`)
correctly emits a uniform increasing CDF in that case. So `pkr-arena`
and `pkr-tournament` evaluated a fuzzed checkpoint's zero-sum infosets
against the wrong distribution.

Fix: match the exporter's encoding exactly.

**Bug 3 — `mirror_to_seat0` did not swap `dealer`**
(`crates/pkr-runtime/src/subgame.rs`, commit `b867608`)

The mirror for seat-1 actors swapped stacks, street_bets, holes, actor,
etc. but not `dealer`. `advance_street_in_place` computes the next
street's first actor as `1 - dealer`, so a mirrored subgame that
crosses a street boundary (turn subgames) would hand the first postflop
action to the wrong seat.

River-only subgames never advance, so the shipped config was safe.
Turn extensions and any future turn solve were not.

Fix: swap `dealer` too. Two tests in
`crates/pkr-runtime/tests/mirror_dealer.rs`.

### Other work

| commit | what |
|---|---|
| `b68c301` | F4 `centroids-potential` + `abs-potential` subcommands |
| `dd54b1f` | F4 flop rebuild — 99.6% of buckets changed |
| `24a270f` | F4 `turn-potential` subcommand (ready, not launched) |
| `dcef4af` | `centroid_feature_v` now env-driven (`PKR_CENTROID_FEATURE_V`) |
| `e7b42e5` | F1 pin filled in — v42 reads 10253.8 mbb at 500 deals, seed 42 |
| `70fdcd2` | F5 averaging-site A/B concluded: equivalent, keep default |
| `fb892be` | F5 Kuhn grid — RM+ floor wins on the toy game |
| `c053558` | `neg_floor` wired into the f32 CFR path (Kuhn harness) |
| `73669a0` | Fingerprint guard regression tests |
| `b5a0f29` | F6 legal-action-tree regression tests |
| `e3d4601` | F7 allocation-helper regression tests |
| `80d36dc` | F8 purify regression tests |
| `57e4374` | F2 experiment-defaults pin |
| `d93eb65` | F9 `pkr-arena` bin |
| `173bae7` | F9 `pkr-tournament` bin |
| `94aa321` | Extract `TableProvider` into a shared module |
| `b1fb4cf` | Arena hand-count sensitivity: 2000 minimum for quoted readings |
| `1b26acc` | Session handoff from earlier |

---

## Section 3 — Bug hunt state

The bug hunt was in progress when the session ended. Three bugs fixed.
The last exploration round looked at:

- Card-id-used-as-rank patterns (the F8 class) — no new finds.
- `1.0 / N_HANDS` — one place in `best_response.rs` (a
  `uniform_opp_range()` helper, correct usage).
- `debug_assert` count in `pkr-core/src/state.rs` — 1.
- Postcard framing of `centroids.bin` — 2-byte prefix for k=200, then
  8 bytes per centroid, verified.

**Areas not yet investigated:**

1. **The `pkr-cfr/src/traversal.rs` strategy-batch ordering** — F5's
   averaging-site A/B came out equivalent, but the *within-batch*
   ordering of the opponent-site accumulation wasn't audited. See the
   `avg_at_traverser=false` path added in `8695f84`.
2. **`crates/pkr-exploit/src/best_response.rs::collect_cfv` opp_reach
   threading** — the F1 fix is committed, but the walker's `opp_reach`
   propagation at deeper nodes could be audited against a hand-worked
   example on Kuhn (the F1 testing doc explains why synthetic tests
   failed).
3. **`crates/pkr-abstraction/src/potential.rs::ehs_and_potential`** —
   the noise-subtraction formula is `(raw_var - noise/n).max(0)`. The
   `noise/n` term uses `n = num_next_cards` implicitly (the code
   accumulates `noise += e*(1-e)/inner` per next card, then divides by
   `n`). Verify the derivation once more against a small board where
   the answer is known by hand.
4. **`pkr-fuzz/src/bin/tournament.rs` table-directory assumption** —
   the bin assumes A and B share a table directory (uses A's parent by
   default). If B's tables are different (e.g. an F4 checkpoint vs a
   legacy one), the fingerprint catches `centroid_feature_v` mismatch
   *only if the env var is exported*. Documented; not yet enforced.
5. **`RuntimeSession::observe_action` and `observe_street` silently
   swallow tracker errors.** They use `let _ = t.apply_action(...)`.
   A tracker error (e.g. `HandOver`) leaves the session's state and
   the caller's state out of sync with no signal. Low priority but
   worth a return value or a `debug_assert`.

---

## Section 4 — What "done" looks like

If you're taking over cold, the highest-value next steps:

1. **Wait for v45, decide F4.** The decision rule is in Section 1.
2. **If F4 helps:** launch the turn rebuild in the background
   (`cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- turn-potential <centroids> <rank> <out>`)
   — measured at ~2.5 days on 8 cores. Use the checkpoint/resume
   mechanism in `generate_turn_table_potential` (`.progress` and
   `.tmp` files).
3. **If F4 is neutral:** keep the legacy feature, skip the turn
   rebuild.
4. **Either way:** continue the bug hunt from Section 3, focusing on
   items 1, 2, 4.

---

## Section 5 — Process lessons recorded this session

1. **Binary freshness before launching a training run.** v41 was
   wasted (18M iterations, ~1h) because it was launched on a binary
   that predated F2 and F6. The v42 launcher has a preflight that
   checks the binary's mtime against the newest source file and runs a
   500-iteration smoke to verify the fingerprint. Copy that pattern
   into any new launcher.
2. **Synthetic F1 tests are a dead end.** Three approaches failed —
   see `docs/experiments/f1-testing-notes.md`. The right shape is a
   checkpoint pin (`f1_estimator_pin.rs`), not a synthetic test.
3. **Long-running background tasks deserve a watcher script** that
   writes a result file. Every training run in this session has one.
4. **`pkr-arena` needs at least 2000 hands for a readable bb/100.**
   500 hands overstates by 65% because StationBot and AggroBot have
   high per-hand variance.
5. **Don't touch `~/.rustup` or run cargo in other repos** — the
   terminal output races with other work. Use a dedicated log file
   (`>` redirect) for any exploration script.

---

## Section 6 — File map

| path | state |
|---|---|
| `crates/pkr-abstraction/src/bin/precompute.rs` | `centroids-potential`, `abs-potential`, `turn-potential` subcommands |
| `crates/pkr-abstraction/src/potential.rs` | `ehs_and_potential` feature function |
| `crates/pkr-core/src/abstraction.rs` | fingerprint with `centroid_feature_v` (env-driven) |
| `crates/pkr-fuzz/src/provider.rs` | shared `TableProvider` |
| `crates/pkr-fuzz/src/bin/arena.rs` | `pkr-arena` |
| `crates/pkr-fuzz/src/bin/tournament.rs` | `pkr-tournament` |
| `crates/pkr-runtime/src/subgame.rs` | `mirror_to_seat0` now swaps dealer |
| `crates/pkr-subgame/src/range_tracker.rs` | fallback preserves board exclusion |
| `crates/pkr-runtime/src/session.rs` | `RuntimeSession` + `advise_or_blueprint` |
| `docs/experiments/f5-grid.md` | F5 Kuhn grid conclusion |
| `docs/experiments/v42-vs-v43-ab.md` | F5 NLHE A/B conclusion |
| `docs/experiments/f4-abstraction-rebuild-plan.md` | F4 rebuild details + v45 progress |
| `docs/experiments/first-real-game-eval.md` | F9 arena result + hand-count sensitivity |
| `docs/experiments/f1-testing-notes.md` | why synthetic F1 tests failed |
| `docs/experiments/v42-post-audit-result.md` | v42 curve, plateau at 18M |
| `docs/HANDOFF_2026-10-01_audit-followup.md` | earlier handoff this day |

### Disk

- `outputs/v42-post-audit/` — v42 checkpoint, the current A/B baseline
- `outputs/v43-avg-opp/` — F5 averaging-site A/B
- `outputs/v44-potential/` — F4 flop table (`abstraction_pot.bin`)
- `outputs/v45-potential/` — v45 training (running)
- `outputs/v34long/` — legacy baseline still referenced by tests
- `outputs/v0-smoke/` — smoke abstraction used by tests and CI

---

## Section 7 — Test state

**320 tests, all passing, 45 skipped.** Zero workspace warnings.

The skipped tests are `#[ignore]`d either because they need a real
checkpoint or because they're slow diagnostics. Run them with
`cargo nextest run --workspace --run-ignored all`.

Notable ignored tests:
- `f1_estimator_pin.rs::v42_reading_matches_recorded_value`
  (~2.5 min, needs v42 checkpoint)
- `pkr-fuzz::tournament::tests::*` (fast but need the smoke abstraction)
- `gameplay_subgame.rs::gameplay_subgame_vs_blueprint` (needs v34long)
- `e2e_subgame_hook.rs::river_subgame_e2e` (needs v34long)

---

## Section 8 — If you only do one thing

**Check `/tmp/v45-result.txt`.** If it exists, v45 has finished and the
F4 decision (Section 1) is the highest-value next step. If it doesn't,
check `pgrep -f pkr-trainer` — the training is either still running
(leave it) or died (investigate `outputs/v45-potential/train.log`).

---

**END OF HANDOFF**
