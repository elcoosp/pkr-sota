#!/usr/bin/env bash
# Epsilon A/B sweep. READ this, then run one at a time.
# Template only. Does NOT auto-execute.
#
# Goal: find the local optimum around the known-good eps=0.01.
# Each run is 20M iters, ~20 min at ~40K it/s, 8 threads.
# Do NOT run them in parallel — 8 cores total.
#
# Baseline:
#   v23     eps=0.05 @ 20M: 8158  (old BR estimator, 1000 deals)
#   v23a3   eps=0.01 @ 20M: 5908  (old BR estimator, 2000 deals)
#   v25fast7 eps=0.01 @ 20M: 5767 (new BR estimator)
#   v25final eps=0.01 @ 20M: 5735 (full main)
#
# Pick the winner by lowest expl_mbb at 20M; then extend that config to 200M.
set -euo pipefail
cd "$(dirname "$0")/.."

# ---------- Run A: eps=0.005 (finer) ----------
PKR_EXPLORE_EPSILON=0.005 \
  VERSION=v26e005 ITERATIONS=20000000 \
  EVAL_EVERY=5000000 EVAL_DEALS=2000 \
  PKR_MOMENTUM=0 PKR_AVG_POWER=2 \
  ./run.sh

# ---------- Run B: eps=0.02 (coarser) ----------
PKR_EXPLORE_EPSILON=0.02 \
  VERSION=v26e02 ITERATIONS=20000000 \
  EVAL_EVERY=5000000 EVAL_DEALS=2000 \
  PKR_MOMENTUM=0 PKR_AVG_POWER=2 \
  ./run.sh

# ---------- Optional: eps=0.0 (no exploration; risk of frozen infosets) ----------
# Only if A and B both lose to eps=0.01 AND you want to see if exploration
# is purely a cost. Watch for 'uniform_fallback' growing in stats.json.
PKR_EXPLORE_EPSILON=0.0 \
  VERSION=v26e00 ITERATIONS=20000000 \
  EVAL_EVERY=5000000 EVAL_DEALS=2000 \
  PKR_MOMENTUM=0 PKR_AVG_POWER=2 \
  ./run.sh
