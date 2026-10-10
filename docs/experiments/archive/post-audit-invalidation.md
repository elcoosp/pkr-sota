> **CAVEAT (2026-10-02):** exploitability numbers in this doc were
> measured at 5000 eval deals with an in-sample best response. That
> estimator overfits a growing infoset table; the same v42 18M model
> reads 3796 mbb @ 5k deals but 1707 @ 20k. Absolute numbers here are
> inflated (by an amount that varies with infoset count). Relative
> comparisons at the SAME deal count remain valid. See
> `turn-up-investigation.md`.

# Post-audit checkpoint invalidation

**Date:** 2026-09-30
**Context:** F1, F2, F3, F6 all landed in one session. This is the
record of what those changes invalidated and what needs redoing.

## What changed in the abstract game

Three findings modified the game the bot trains on:

| finding | change | invalidates checkpoints? |
|---|---|---|
| F1 | estimator threads opponent reach | No — only the eval estimator changed, not training |
| F2 | config defaults now match experiment scripts | No — but every historical A/B was measured with mismatched defaults |
| F3 | optional size-aware signature (gated OFF) | Not yet — the gate is off |
| F6 | jam always legal + min-raise clamp | **Yes** — the legal-action tree changed |

Only F6 directly invalidates. It's enforced by the fingerprint:
`action_legal_v` is 1 in the new code, 0 in every pre-F6 checkpoint.

## What's invalidated

Every checkpoint in `outputs/` was trained with the pre-F6 legal-action
tree. Loading any of them into a post-F6 binary fails with:

    action_legal_v mismatch: stored=0 current=1

This includes:
- `outputs/v34long/train.ckpt` — the shipped champion (2526 mbb)
- `outputs/v40-k250/train.ckpt` — the k=250 sweep, launched before F6
- all four `outputs/v33retest/*/train.ckpt`
- every `outputs/v38/confirmation/*/train.ckpt`

## What survives

- All `exploitability.csv` and `metrics.csv` files: the historical
  readings, unaffected by F6.
- The trained blueprint binaries: loadable as artifacts (they're
  just key→CDF maps) but their *behavior* was fit to a different game.
  In a post-F6 real game, they play slightly illegal sizes.

## What has to happen

1. **Every future training run starts from scratch.** There is no
   warm-start path across the F6 boundary.
2. **The v34long-era exploitability readings are not comparable to
   post-F6 readings.** They measured a game with a smaller legal-action
   set. Cross-boundary A/Bs are invalid.
3. **Test fixtures that load v34long** (17 files, all `#[ignore]`d)
   will fail until either:
   - a fresh post-F6 checkpoint is trained and the paths are updated,
     or
   - `PKR_STRICT_BETS=1` style escape hatch is added to the fingerprint
     check (NOT recommended — that's how the v9-v13 incident happened).

## The correct sequence from here

1. Run the F5 grid on Kuhn/Leduc (cheap, minutes) to pick the update
   rule before spending hours on NLHE.
2. Train a fresh NLHE checkpoint from scratch with the chosen F5
   config. Estimated: ~50 min at 30M iterations, 8 threads.
3. Measure it with `pkr_fuzz::tournament` against scripted bots.
4. Optionally flip F3, retrain, and A/B it.

Steps 1-3 are the minimum path to having *any* post-F6 result. The
current session ends with that path laid out but not run.

## What the audit predicted

The audit's own summary said:

> If, after F1, the fixed BR still reads about 2200 mbb and tournaments
> show the blueprint beating the scripted bots by wide margins, then
> F3 and F4 are lower priority than I've ranked them.

The F1 fix this session changed the 1-deal reading from 8981 to 1748
mbb — a factor of 5. That's evidence the old estimator was
substantially wrong. Every published number in `docs/experiments/`
should be treated as *measured with a broken estimator* until re-run
with the fixed one.
