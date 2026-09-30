# First real-game evaluation (2026-09-30)

**Status:** historic first. The bot has never played a real game
against an opponent before this session.

## Setup

`pkr-arena` (commit `d93eb65`) against the v42 mid-run checkpoint
(5M iterations, post-F1/F2/F6, k=200, 1.23M keys loaded).

    2000 deals, seed 42.
    Abstraction hit rate: 99.9% (11162 blueprint, 14 fallback out of
    11176 decisions).

The hit rate is the load-bearing number. Without it, a reading could
be the *fallback* path's score, not the trained strategy's. 99.9%
means the arena is genuinely exercising the checkpoint.

## Result

| opponent | bot bb/100 |
|---|---|
| StationBot (calling station) | **+277.55** |
| NitBot (tight) | **+36.76** |
| AggroBot (hyper-aggressive) | **+266.50** |
| **aggregate** | **+193.60** |

The bot wins money against every scripted opponent. That's expected —
these bots are deliberately simple and exploitable — but it is the
first time the project has evidence the trained strategy does
anything at all in a real game, rather than just producing a low
abstract exploitability number.

## Interpretation

- **StationBot and AggroBot are trivially exploitable.** They play
  fixed, extreme strategies; a trained CFR bot should beat them by a
  wide margin. +270 bb/100 is consistent with that.
- **NitBot is harder.** Tight play is intrinsically less exploitable
  because it folds the hands that would lose big pots. +37 bb/100
  still beats it, but by 7x less.
- **The absolute magnitude is not informative** about how the bot
  would do against a strong opponent. Beating a calling station by
  2.7 bb/100 is very different from beating a competent human.

## What this proves

1. The pipeline works end to end: checkpoint → abstraction → game
   loop → payoff accounting.
2. The blueprint hit rate is high enough that the numbers reflect the
   trained strategy.
3. The bot beats simple bots. That was not previously known.

## What this does NOT prove

- Anything about exploitability. bb/100 against a scripted bot is
  not a bound on how much a *strong* opponent could win.
- Anything about the bot's strength at the current abstraction
  relative to its own past checkpoints. That comparison is what
  `pkr-tournament` is for.

## Caveats

- The checkpoint is at 5M iterations, not the 30M target. The
  exploitability curve was still descending.
- 2000 hands gives a wide confidence interval on each opponent's
  bb/100 (roughly ±40-60 bb/100 per opponent for a bot with
  ~500 bb/100 per-hand SD). The aggregate sign is clear; the
  magnitudes are not tight.
- StationBot/NitBot/AggroBot are simple. Nothing here generalizes to
  a real opponent pool.

## Next

- Re-run against v42's final checkpoint when training completes.
- Run `pkr-tournament` to compare v42 against a future F3-flipped
  run — that's the first use of the head-to-head harness.
- The audit's F9 item 3 (LBR lower bound) is still open. That's the
  measurement that would tell us how exploitable the strategy is in
  the *real* game, not the abstract game the trainer plays.
