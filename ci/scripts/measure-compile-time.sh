#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

OUT="${OUT:-compile-time.json}"
cargo clean -p pkr-trainer -p pkr-abstraction 2>/dev/null || true
T_START=$(date +%s)
cargo build --release -p pkr-trainer -p pkr-abstraction --bins
T_END=$(date +%s)
python3 -c "
import json
elapsed = $T_END - $T_START
open('$OUT', 'w').write(json.dumps({
    'benchmark': 'compile_time/release_cold',
    'value': elapsed,
    'unit': 's',
    'higher_is_better': False,
}) + '\n')
"
cat "$OUT"
