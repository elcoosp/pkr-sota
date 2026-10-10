# Training log

## 2026-09-23 — eval harness works, k=32 regime is undertrained

Launched `VERSION=v8` at k=32, 15M capacity. Three EVAL checkpoints:

```
EVAL iter=102400 expl_mbb=38186.90
EVAL iter=204800 expl_mbb=38106.94
EVAL iter=307200 expl_mbb=38277.05
```

Interpretation:
- Eval machinery is correct (checked by hand against the formula).
- Curve is flat because CFR is undertrained. 14M infosets / 400K
  iterations = 0.03 visits per infoset. Regret matching cannot
  differentiate at that density.
- The BR values themselves (~80 chips/hand) confirm the trained
  strategy is not merely uniform — it commits to actions that a
  best response punishes harder than random play would.

Conclusion: at k=32 the iteration budget needed for convergence is
~1-10 billion, which is 15-150 hours at current throughput. Not worth
running on this hardware at this abstraction.

Next: retrain at k=8 (100× fewer infosets). Regenerate abstraction
tables once (~45 min turn table). Then 100M iterations (~90 min) gives
~670 visits per infoset. That's the first regime where the eval curve
is expected to decline.
