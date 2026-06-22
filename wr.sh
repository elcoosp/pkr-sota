#!/usr/bin/env bash
set -uo pipefail
COMPILE_OK=true
INCOMPLETE=false

echo "Patching traversal.rs: add use rand::RngExt and fix unused variable"

# Add the import after the existing 'use rand::Rng;' line
sed -i '' '/^use rand::Rng;$/a\
use rand::RngExt;
' crates/pkr-cfr/src/traversal.rs
if [ $? -ne 0 ]; then
  echo "ERROR: sed import insertion failed"
fi

# Replace ActionKind::Bet(amt) with ActionKind::Bet(_) to silence warning
OLD_TMP=$(mktemp) || { echo "ERROR: cannot create temp file"; exit 1; }
NEW_TMP=$(mktemp)
cat > "$OLD_TMP" << 'OLD_AMT'
        ActionKind::Bet(amt) => {
OLD_AMT
cat > "$NEW_TMP" << 'NEW_AMT'
        ActionKind::Bet(_) => {
NEW_AMT
if python3 - "$OLD_TMP" "$NEW_TMP" crates/pkr-cfr/src/traversal.rs << 'PYAMT'
import sys
with open(sys.argv[1]) as f: old = f.read()
with open(sys.argv[2]) as f: new = f.read()
with open(sys.argv[3], 'r') as f: content = f.read()
content = content.replace(old, new)
with open(sys.argv[3], 'w') as f: f.write(content)
PYAMT
then
  echo "Unused variable patched"
  rm "$OLD_TMP" "$NEW_TMP"
else
  echo "ERROR: patch failed for amt"
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

echo "Running tests"
cargo test -p pkr-cfr -p pkr-eval -p pkr-core
if [ $? -eq 0 ]; then
  echo "All tests passed. Committing."
  git add -A
  git commit -m "fix(cfr): add missing RngExt import and silence unused variable"
else
  echo "Tests failed. Fix errors then run the next script."
  exit 1
fi
