#!/usr/bin/env bash
set -uo pipefail
COMPILE_OK=true
INCOMPLETE=false

echo "Fixing mmap.rs test: replace seed with seed1 and seed2 in FmphHeader construction"

OLD_TMP=$(mktemp) || { echo "ERROR: cannot create temp file"; exit 1; }
NEW_TMP=$(mktemp)
cat > "$OLD_TMP" << 'OLD_FMPH_TEST_BLOCK'
        let fmp_hdr = FmphHeader {
            num_keys: infoset_count,
            seed: 42,
            max_level_size: fmph_max_level_size,
            level_count: fmph_level_count,
            _padding: [0; 4],
        };
OLD_FMPH_TEST_BLOCK
cat > "$NEW_TMP" << 'NEW_FMPH_TEST_BLOCK'
        let fmp_hdr = FmphHeader {
            num_keys: infoset_count,
            seed1: 42,
            seed2: 0,
            max_level_size: fmph_max_level_size,
            level_count: fmph_level_count,
            _padding: [0; 4],
        };
NEW_FMPH_TEST_BLOCK
if python3 - "$OLD_TMP" "$NEW_TMP" crates/pkr-runtime/src/mmap.rs << 'PYFMPH'
import sys
with open(sys.argv[1]) as f: old = f.read()
with open(sys.argv[2]) as f: new = f.read()
with open(sys.argv[3], 'r') as f: content = f.read()
content = content.replace(old, new)
with open(sys.argv[3], 'w') as f: f.write(content)
PYFMPH
then
  echo "FmphHeader construction patched"
  rm "$OLD_TMP" "$NEW_TMP"
else
  echo "ERROR: patch failed for mmap.rs"
  rm -f "$OLD_TMP" "$NEW_TMP"
fi

echo "Checking compilation"
if ! cargo check --workspace 2>&1; then
  echo "Compilation failed – will skip commit"
  COMPILE_OK=false
fi

if [ "$INCOMPLETE" = true ] || [ "$COMPILE_OK" = false ]; then
  echo "Skipping tests and commit due to errors"
  exit 1
fi

echo "Running full test suite"
cargo test --workspace
if [ $? -eq 0 ]; then
  echo "All tests passed. Committing."
  git add -A
  git commit -m "fix(runtime): correct FmphHeader fields in test (seed1/seed2)"
else
  echo "Tests failed. Fix errors then run the next script."
  exit 1
fi
