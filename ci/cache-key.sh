#!/usr/bin/env bash
# Emits a cache key for the .smoke/ abstraction artifacts.
# Key = hash of every file that influences abstraction output.
set -euo pipefail
cd "$(dirname "$0")/../.."

FILES=(
  Cargo.toml
  crates/pkr-abstraction/Cargo.toml
  crates/pkr-abstraction/src/bin/precompute.rs
  crates/pkr-abstraction/src/ehs.rs
  crates/pkr-abstraction/src/lib.rs
  crates/pkr-core/src/card.rs
  crates/pkr-core/src/deck.rs
  crates/pkr-core/src/rules.rs
  crates/pkr-core/src/state.rs
  crates/pkr-eval/src/lib.rs
  crates/pkr-eval/src/lookup.rs
  crates/pkr-eval/src/lookup_fast.rs
  crates/pkr-eval/src/slow.rs
  smoke.sh
)
# Some files in the list may not exist (e.g. lookup_fast.rs is
# feature-gated but present; keep the list tolerant).
EXISTING=()
for f in "${FILES[@]}"; do
  if [ -f "$f" ]; then
    EXISTING+=("$f")
  fi
done
cat "${EXISTING[@]}" | sha256sum | cut -d' ' -f1
