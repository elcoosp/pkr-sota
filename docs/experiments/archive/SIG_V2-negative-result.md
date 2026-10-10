# SIG_V2_STREET_MONEY: negative result (2026-09-25)

## Experiment

Flipped `SIG_V2_STREET_MONEY` from `false` to `true`, adding SPR bucket
(and optionally last-bet-fraction) to the infoset signature. Same
abstraction tables as v25final. 200M iterations target.

Hypothesis: adding SPR and pot-size-faced information to the infoset
key would let the model distinguish strategically distinct situations
(e.g. "facing 0.5x pot" vs "facing 2x pot") that v1 signature collapsed.

## Result

Killed at 20M after the first eval, which came in at:
