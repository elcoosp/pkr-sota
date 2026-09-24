#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

cargo install cargo-fuzz 2>&1 | tail -1 || true

# 5 minutes per target — keeps weekly budget manageable.
for T in blueprint_loader state_transitions; do
    echo "==> fuzz: $T (5 min)"
    cargo fuzz run "$T" -- -max_total_time=300 --release || {
        # Fuzz crash — save the input.
        echo "FAIL: $T crashed. Artifacts in fuzz/artifacts/"
        mkdir -p fuzz-artifacts/
        cp -r crates/pkr-fuzz/fuzz/artifacts "fuzz-artifacts/$T" 2>/dev/null || true
        exit 1
    }
done
