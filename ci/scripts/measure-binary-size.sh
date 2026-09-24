#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

OUT="${OUT:-binary-size.json}"
cargo build --release -p pkr-trainer -p pkr-abstraction --bins

{
python3 - "$OUT" <<'PY'
import json, os, sys

out = sys.argv[1]
entries = []
for name, path in [
    ("pkr-trainer", "target/release/pkr-trainer"),
    ("pkr-abstraction-precompute", "target/release/pkr-abstraction-precompute"),
]:
    if not os.path.exists(path):
        continue
    size = os.path.getsize(path)
    entries.append({
        "benchmark": f"binary_size/{name}",
        "value": size,
        "unit": "bytes",
        "higher_is_better": False,
    })
with open(out, "w") as f:
    for e in entries:
        f.write(json.dumps(e) + "\n")
PY
} 
cat "$OUT"
