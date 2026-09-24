#!/usr/bin/env bash
# CI wrapper for ./smoke.sh. Restores/saves the .smoke/ cache so the
# 8-step pipeline only runs the train+export+reload tail on each PR.
set -euo pipefail
cd "$(dirname "$0")/../.."

# The smoke script writes to ./outputs/v0-smoke by default.
export SMOKE_DIR="${SMOKE_DIR:-./outputs/v0-smoke}"
mkdir -p "$SMOKE_DIR"

# Run smoke. The script's own `ensure` function skips artifacts that
# already exist on disk; the cache restore in the YAML populates them.
./smoke.sh

# Belt-and-suspenders: re-validate the byte-size asserts that smoke.sh
# already checks. This catches silent format regressions even if
# someone removes them from smoke.sh.
python3 - <<'PY'
import os, sys
d = os.environ["SMOKE_DIR"]
checks = {
    "turn_abstraction.bin": 305377800,
    "river_buckets.bin":    2598960,
}
for name, expected in checks.items():
    p = os.path.join(d, name)
    if not os.path.exists(p):
        print(f"FAIL: {p} missing", file=sys.stderr); sys.exit(1)
    actual = os.path.getsize(p)
    if actual != expected:
        print(f"FAIL: {name} size {actual} != {expected}", file=sys.stderr)
        sys.exit(1)
    print(f"OK: {name} = {actual} bytes")
PY

# Run the load test that smoke.sh runs at the end, in-process, so
# we get a JUnit-style failure if it breaks.
PKR_BLUEPRINT="$SMOKE_DIR/blueprint.bin" \
    cargo test --release -p pkr-trainer --test pipeline -- --ignored load_external_blueprint
