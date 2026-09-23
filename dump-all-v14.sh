#!/usr/bin/env bash
# dump-all-v14.sh — dump CDFs for all v14 chunks side by side
set -uo pipefail
cd "$(dirname "$0")"

for chunk in 5000000 10000000 15000000 20000000; do
    bp="outputs/v14/blueprint_${chunk}.bin"
    [ ! -f "$bp" ] && continue

    echo ""
    echo "============================================================"
    echo "  v14 @ $chunk"
    echo "============================================================"
    PKR_BLUEPRINT="$bp" \
    PKR_ABS_DIR=outputs/v14 \
    cargo test --release -q -p pkr-trainer --test dump_cdf -- --ignored --nocapture 2>&1 \
        | grep -vE "^(warning|    Finished|   Compiling|     Running|running 1 test|\\.)" \
        | grep -E "^(###|  hand|  AA|  KK|  QQ|  JJ|  99|  72o|  AKs|  T9s|  33|  22)"
done
