# Range-aware subgame solving — status 2026-09-29

**Supersedes:** `docs/status/range-aware-2026-09-28.md`

## Headline

River subgame solving, with tracked opponent ranges, is verified
across **three checkpoints** at game play:

| checkpoint | deals | delta (chips/deal) | t |
|---|---|---|---|
| v34long (shipping) | 20000 | +2.40 | 6.04 |
| seed42-B6D | 20000 | +1.12 | 3.07 |
| seed42-A2D | 5000 | +1.89 | 2.34 |

Same sign everywhere. Not checkpoint-specific.

## What shipped this session

- `RuntimeSession` in `crates/pkr-runtime/src/session.rs` (re-exported
  at crate root). The shipping API for bot callers — subgame if it can,
  blueprint average otherwise, uniform as a fallback.
- Runnable example: `cargo run --release -p pkr-runtime --example bot_loop`.
- `PKR_GP_OUT` env var for running the gameplay test against any
  checkpoint+table set.
- `debug_assert` in `pkr-core::GameState::apply_action_internal` catches
  `Action { player: <wrong seat> }` — a real footgun the example tripped.

## Turn extension — clean negative

| config | iters | delta | t |
|---|---|---|---|
| river only | 10 | +1.43 | 2.44 |
| river+turn | 10 | -0.67 | -0.91 |
| river+turn | 50 | -0.16 | -0.22 |

Correct (runtime turn tests pass) but not useful. Ship river-only.

## Cost

- River-10: ~0.5 s/deal. River-50: ~0.5 s/deal (converged by 10).
- Turn-50: ~2.2 s/deal.
- 100-deal static-exploitability e2e at 10 iters: ~72 min.

## What's open

- A runtime bot binary that actually calls `RuntimeSession`. The API is
  done; the consumer doesn't exist yet.
- The v33 preflop retest: seed42 shows the +425 mbb original claim was
  inflated to ~243 mbb under deterministic training; seed43 running.

## Reference

- `docs/experiments/range-aware-solving-poc.md` — full record
- `docs/roadmap/runtime-tracker-integration.md` — shipping API
- `docs/experiments/v33-rich-preflop-confirmed.md` — retest section
