# k=400 abstraction: NEGATIVE (2026-10-05)

LBR 20k hands, seed 42, V4 signature, 100M iters, u16 tables.

| k | infosets | LBR mbb/hand |
|---|---|---|
| 200 (V4) | 6.79M | **770.7** |
| 400 | 8.72M | **877.7** |

k=400 is ~0.9 sigma WORSE. Abstraction depth did not help; likely the
4000-sample centroid fit (10 points/cluster) or 2D-feature bucket noise.
Keep k=200. (A proper k=400 test would refit centroids with >=20k samples
and re-verify buckets are non-degenerate.)
