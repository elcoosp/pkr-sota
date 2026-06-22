#!/usr/bin/env bash
set -uo pipefail
COMPILE_OK=true
INCOMPLETE=false

echo "Reducing NUM_SAMPLES in ehs.rs to 100 for fast precomputation"
OLD_EHS=$(mktemp) || exit 1
NEW_EHS=$(mktemp)
cat > "$OLD_EHS" << 'OLD_NUM_SAMPLES'
const NUM_SAMPLES: usize = 1000;
OLD_NUM_SAMPLES
cat > "$NEW_EHS" << 'NEW_NUM_SAMPLES'
const NUM_SAMPLES: usize = 100; // reduced for precomputation; runtime uses precomputed table
NEW_NUM_SAMPLES
if python3 - "$OLD_EHS" "$NEW_EHS" crates/pkr-abstraction/src/ehs.rs << 'PYEHS'
import sys
with open(sys.argv[1]) as f: old = f.read()
with open(sys.argv[2]) as f: new = f.read()
with open(sys.argv[3], 'r') as f: content = f.read()
content = content.replace(old, new)
with open(sys.argv[3], 'w') as f: f.write(content)
PYEHS
then
  echo "NUM_SAMPLES updated"
  rm "$OLD_EHS" "$NEW_EHS"
else
  echo "ERROR: patch failed"
  rm -f "$OLD_EHS" "$NEW_EHS"
fi

echo "Building release binary for precompute"
cargo build --release --bin pkr-abstraction-precompute

echo "Generating hand rank table (takes ~5 seconds on M1)"
./target/release/pkr-abstraction-precompute table hand_ranks.bin

echo "Generating centroids (if not already present or to refresh)"
./target/release/pkr-abstraction-precompute centroids 10000 200 centroids.bin

echo "Generating full abstraction table (this will take ~20-40 minutes with 8 cores)"
./target/release/pkr-abstraction-precompute abstraction centroids.bin hand_ranks.bin abstraction.bin

echo "All assets generated. Committing binary files (if within size limits) or just the scripts."
# The binary files are large; we'll gitignore them later. For now, just commit the code changes.
echo "Running final tests"
cargo test --workspace
if [ $? -eq 0 ]; then
  echo "All tests passed. Committing code changes."
  git add -A
  git commit -m "perf(ehs): reduce MC samples to 100, add asset generation commands"
else
  echo "Tests failed. Fix errors then run the next script."
  exit 1
fi
