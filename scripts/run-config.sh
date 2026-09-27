# scripts/run-config.sh — pkr-sota training config.
#
# Source this from every run script. Override by exporting before source:
#   VERSION=v17 CENTROID_K=400 bash -c 'source scripts/run-config.sh; ...'
#
# Every variable below is deliberately explicit so that a log diff of two
# runs surfaces any config change in one place. If you find yourself
# editing a run-vN.sh to tweak a value, add the variable here instead.

# ---------- versioning / output ----------
VERSION="${VERSION:-v16}"
OUT="${OUT:-outputs/${VERSION}}"

# ---------- threads / compute ----------
THREADS="${THREADS:-8}"

# ---------- abstraction (r3 §17) ----------
# k=200 is the production floor. smoke.sh uses k=8 and exports
# PKR_ALLOW_SMALL_K=1 to bypass the trainer's hard floor (V1).
CENTROID_K="${CENTROID_K:-200}"
CENTROID_SAMPLES="${CENTROID_SAMPLES:-2000}"
FLOP_BUCKETS="${FLOP_BUCKETS:-200}"
RIVER_BUCKETS="${RIVER_BUCKETS:-200}"
EHS_SAMPLES="${EHS_SAMPLES:-100}"
EHS_SAMPLES_TURN="${EHS_SAMPLES_TURN:-200}"
# Preflop feature space. 1 = rich 6D (EHS, EHS^2, rank_high, rank_low,
# suited, connector), the confirmed-win configuration from
# docs/experiments/v33-rich-preflop-confirmed.md. 0 = legacy 2D.
# Requires PKR_RICH_CENTROIDS=1 at centroid-precompute time and the
# `preflop-rich` subcommand; run.sh handles both automatically.
PREFLOP_RICH="${PREFLOP_RICH:-1}"

# ---------- training budget ----------
# 40M is the first checkpoint of a run that will be extended if the
# exploitability curve (E1) is still descending at 40M.
ITERATIONS="${ITERATIONS:-40000000}"
# Wall-clock training budget in seconds (0 = off, iteration count rules).
# When > 0 the trainer stops cleanly at the deadline (final checkpoint +
# stats still written). Used for time-boxed runs, e.g. BENCH_SECONDS=43200
# for 12h. Precompute is NOT counted (deadline starts with training).
BENCH_SECONDS="${BENCH_SECONDS:-0}"
CAPACITY="${CAPACITY:-60000000}"
CHECKPOINT_EVERY="${CHECKPOINT_EVERY:-5000000}"
REPORT_EVERY="${REPORT_EVERY:-1000000}"
ITERS_PER_SYNC="${ITERS_PER_SYNC:-2048}"

# ---------- evaluation / promotion (r3 E1) ----------
# Fire the sampled-BR exploitability check every EVAL_EVERY iterations
# and promote only if the new checkpoint is not worse than the best by
# more than PROMOTE_GATE mbb.
# Eval budget. The sampled game-tree BR walker runs at ~8 deals/s
# single-threaded, so:
#   500 deals    ~1 min   SE ~±1600 mbb  (too coarse)
#   5000 deals   ~10 min  SE ~±500 mbb   (usable for A/B)
#   20000 deals  ~40 min  SE ~±250 mbb   (for final validation)
# We default to 5000: enough precision that a promote-gate decision is
# meaningful, cheap enough to run at every 5M-iteration checkpoint.
#
# (The public-tree BR at `pkr-exploit::public_br` is WIP and cannot be
# used — see that module's doc comment.)
EVAL_EVERY="${EVAL_EVERY:-5000000}"
# Stop training after this many consecutive evals fail to set a new
# historical minimum. Retroactive check on 100M runs: average savings
# was 49% of the iteration budget with no change to the shipped
# blueprint. See docs/experiments/v38-30M-sweetspot.md for context.
STOP_ON_PLATEAU="${STOP_ON_PLATEAU:-5}"
EVAL_DEALS="${EVAL_DEALS:-5000}"
PROMOTE_GATE="${PROMOTE_GATE:-3.0}"

# ---------- CFR dynamics (r3 V2 / E4) ----------
# Epsilon-uniform exploration at opponent nodes to prevent the sampling
# collapse that froze v14's infosets (r3 §22).
PKR_EXPLORE_EPSILON="${PKR_EXPLORE_EPSILON:-0.05}"

# ---------- feature flags (must be 0/false in production) ----------
# PKR_ALLOW_SMALL_K and PKR_ALLOW_EHS_FALLBACK are deliberately NOT set
# in production — their absence is the guardrail. Override only in tests
# and the smoke script (see smoke.sh for the pattern).

# ---------- fast7 evaluator (T1.3, opt-in) ----------
# "table" = reference implementation (21-subset LUT).
# "fast7" = rank-count LUT (bit-identical, ~20-60x per-eval).
# The trainer's hot path is dominated by traversal, not evaluation, so
# the throughput delta is small; fast7's real payoff is in precompute.
EVALUATOR="${EVALUATOR:-table}"

# ---------- derived ----------
export RUSTFLAGS="${RUSTFLAGS:--C target-cpu=native}"
export RAYON_NUM_THREADS="$THREADS"
export PKR_EXPLORE_EPSILON
