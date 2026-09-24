#!/usr/bin/env bash
# Run kuhn_experiment.rs and capture stdout for parsing.
set -euo pipefail
cd "$(dirname "$0")/../.."

OUT="${OUT:-kuhn-results.txt}"
cargo run --release -p pkr-testgames --bin kuhn-experiment > "$OUT" 2>&1
