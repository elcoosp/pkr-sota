# Plan status (PKR_SOTA_PLAN) - 2026-10-09

## DONE

- 0.1 stack param (PKR_STACK_BB) + fingerprint
- 0.2 duplicate-deal H2H + min-deals guard
- 0.3 LBR JSON line
- 0.4 scripts/slumbot_client.py (write-only; wire format UNVERIFIED - needs network)
- 0.5 stop list
- 1.1 shadow state (PKR_SHADOW)
- 1.2 translation log
- 1.3 pkr-bot-pipe (now emits CONCRETE actions + advances board; seat-1 hole fixed)
- 2.1 preflop-exact 169 default ON
- 2.2 V5 signature
- 3.1 zero-prob action pruning (PKR_PRUNE)
- 4.1 vector solver ported (+ b0edfa2 primitives, safety bound, gadget)
- 4.2 gadget + bound acceptance tests (PROVEN safe)
- 4.4 CFR-D gadget (both sampled AND vector, PROVEN safe, EV cbv)

## NOT DONE

- 2.3 distribution-aware buckets (multi-hour precompute; potential.rs exists,
  histogram/EMD not implemented)
- 2.4 action abstraction K=6->7 (invasive; K hard-wired in lbr.rs, tests)
- 3.2 iters-per-sync sweep (flag exists; sweep not run)
- 3.3 long budget (200M+; not run)
- 3.4 perf profiling
- 4.3 our-range update with the SEARCH strategy (see note)
- 4.5 street policy (partial: budget + streets configurable; turn unaffordable)
- 4.6 perf work (Show cache, rayon chance loop)
- 5 depth-limited flop search
- 6 200bb build + Slumbot (0.4 + final retrain)

## Note on 4.3 (our-range update)

After a search decision the host calls observe_action(action); the
RangeTracker then updates OUR range using the BLUEPRINT probability of
that bucket, not the SEARCH probability. This is inconsistent - but it
only matters if a LATER street re-solves using our range. The live
default is now RIVER-ONLY, so there is no later street -> 4.3 is MOOT
for the shipping path. It would matter if turn/flop search is enabled.

## Latency (measured, live path)

- river decision (bound): 2.14s (at the 2s budget; the budget caps
  iterate, not the bound BR walks)
- turn decision (bound): 52s  -> turn DISABLED by default in the live path

## Verdict

Phases 0,1 fully; 2 partial (2.1,2.2 done; 2.3,2.4 not); 3 partial (3.1);
4 mostly (4.1,4.2,4.4 done; 4.3 moot for river-only; 4.5 partial; 4.6 not);
5,6 not started.
