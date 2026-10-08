# Search H2H — independent replication (2026-10-08)

Bounded river search vs blueprint, V5, 20k deals, decorrelated seeds
(tournament seed-spacing fixed this session).

| seed | diff (A-B) chips/deal | t |
|---|---|---|
| 42 | +1.1763 | +6.16 |
| 43 | +0.8211 | +4.11 |
| 44 | (running) | |
| 45 | (queued) | |

Both completed seeds are positive and significant. These are genuinely
independent deal sets (the earlier seeds 42-45 were correlated /
shifted-by-one; fixed in commit 8103dd4).

## Arena (third instrument, scripted bots)

V5, 300 hands, seed 42, sig 5:

| config | bot bb/100 |
|---|---|
| blueprint only | +69.25 |
| + vector river search (20 iters) | +102.27 |

Search adds ~33 bb/100 vs scripted bots. 300 hands = high variance;
preliminary, but directionally consistent with the H2H (+0.82 to +1.18
chips/deal). Arena search verified firing: 204 solve_root calls.
