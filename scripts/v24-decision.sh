#!/usr/bin/env bash
# scripts/v24-decision.sh
#
# v24 launcher. READ THE DECISION MATRIX BELOW before running.
# This file is a template; it does NOT auto-execute.
#
# v23 result so far (corrected estimator, 1000 deals):
#     20M: 8158
#     40M: 8305
#     60M: 9216
#     80M: 9466   <-- rising, not flat
#
# Decision matrix (fill in once v23 has all 10 evals):
#
#   A) v23 finished FLAT in 9000-10000 (|last - first| < 2*SE):
#      => k=200 is the abstraction ceiling.
#      => Run T2.2 (finer river hash + turn k=400). See docs/PKR_AUDIT section 8.
#      => v24 config: same as v23 (PKR_MOMENTUM=0 PKR_AVG_POWER=2).
#
#   B) v23 finished DECLINING (last < first - 2*SE):
#      => Real convergence is happening. Extend.
#      => Resume v23's checkpoint, 400M iters:
#         ITERATIONS=400000000 VERSION=v24 ./run.sh
#         (run.sh will resume from outputs/v24/train.ckpt if you
#          symlink or copy v23's checkpoint; otherwise start fresh.)
#
#   C) v23 finished RISING (last > first + 2*SE):  <-- CURRENT SIGNAL
#      => The algorithm is not converging on the corrected scale.
#      => Test the D2 hypothesis. Two 5M-iter A/Bs:
#           A/B-1: PKR_MOMENTUM=0 PKR_AVG_POWER=1  (linear averaging)
#           A/B-2: PKR_MOMENTUM=0 PKR_AVG_POWER=0  (uniform averaging)
#         Whichever gives the LOWER 5M number wins; then scale it up.
#
#   D) v23 died early (crash, capacity, disk):
#      => Inspect /tmp/v23.log tail. Use --fresh if the checkpoint
#         is incompatible; otherwise resume.
#
# All three commands below are ready to paste. Only uncomment the one
# you chose. Do NOT launch two at once.
#
# ---------- common env ----------
# export VERSION=v24
# export ITERATIONS=200000000
# export CAPACITY=20000000
# export EVAL_EVERY=20000000
# export EVAL_DEALS=1000
# export CHECKPOINT_EVERY=20000000
#
# ---------- A) plateau -> T2.2 ----------
# (edit scripts/run-config.sh RIVER_BUCKETS / TURN_CENTROIDS first)
# PKR_MOMENTUM=0 PKR_AVG_POWER=2 ./run.sh
#
# ---------- B) declining -> extend ----------
# (copy outputs/v23/train.ckpt outputs/v24/train.ckpt first)
# PKR_MOMENTUM=0 PKR_AVG_POWER=2 ./run.sh
#
# ---------- C) rising -> D2 A/B ----------
# A/B-1 (linear averaging, 5M only):
#   VERSION=v24a ITERATIONS=5000000 EVAL_EVERY=2500000 EVAL_DEALS=2000 \
#     PKR_MOMENTUM=0 PKR_AVG_POWER=1 ./run.sh
# A/B-2 (uniform averaging, 5M only):
#   VERSION=v24b ITERATIONS=5000000 EVAL_EVERY=2500000 EVAL_DEALS=2000 \
#     PKR_MOMENTUM=0 PKR_AVG_POWER=0 ./run.sh
#
# ---------- D) died ----------
#   tail -50 /tmp/v23.log
#   ls -la outputs/v23/train.ckpt outputs/v23/train.ckpt.prev
#
# ---------- verification (run once, AFTER v23 has fully exited) ----------
#   ./smoke.sh
