#!/usr/bin/env bash
set -uo pipefail
COMPILE_OK=true
INCOMPLETE=false

echo "Patching traversal.rs: remove unused Street import and fix get_strategy call"
OLD_TMP=$(mktemp) || { echo "ERROR: cannot create temp file"; exit 1; }
NEW_TMP=$(mktemp)

# Fix import line: use pkr_core::state::{ActionKind, GameState};
cat > "$OLD_TMP" << 'OLD_TR_IMPORT'
use pkr_core::state::{ActionKind, GameState, Street};
OLD_TR_IMPORT
cat > "$NEW_TMP" << 'NEW_TR_IMPORT'
use pkr_core::state::{ActionKind, GameState};
NEW_TR_IMPORT
if python3 - "$OLD_TMP" "$NEW_TMP" crates/pkr-cfr/src/traversal.rs << 'PYTR1'
import sys
with open(sys.argv[1]) as f: old = f.read()
with open(sys.argv[2]) as f: new = f.read()
with open(sys.argv[3], 'r') as f: content = f.read()
content = content.replace(old, new)
with open(sys.argv[3], 'w') as f: f.write(content)
PYTR1
then
  echo "Import fixed"
  rm "$OLD_TMP" "$NEW_TMP"
else
  echo "ERROR: patch failed for traversal.rs import"
  rm -f "$OLD_TMP" "$NEW_TMP"
fi

# Fix get_strategy call: remove second argument
sed -i '' 's/table.get_strategy(infoset_hash, K)/table.get_strategy(infoset_hash)/' crates/pkr-cfr/src/traversal.rs
if [ $? -ne 0 ]; then
  echo "ERROR: sed failed for get_strategy call"
fi

# Remove unused 'k' parameter and rename function signature
OLD_TMP=$(mktemp)
NEW_TMP=$(mktemp)
cat > "$OLD_TMP" << 'OLD_FUNC_SIG'
fn abstract_action_index(kind: &ActionKind, k: usize) -> usize {
OLD_FUNC_SIG
cat > "$NEW_TMP" << 'NEW_FUNC_SIG'
fn abstract_action_index(kind: &ActionKind, _k: usize) -> usize {
NEW_FUNC_SIG
if python3 - "$OLD_TMP" "$NEW_TMP" crates/pkr-cfr/src/traversal.rs << 'PYFUNC'
import sys
with open(sys.argv[1]) as f: old = f.read()
with open(sys.argv[2]) as f: new = f.read()
with open(sys.argv[3], 'r') as f: content = f.read()
content = content.replace(old, new)
with open(sys.argv[3], 'w') as f: f.write(content)
PYFUNC
then
  echo "Function signature fixed"
  rm "$OLD_TMP" "$NEW_TMP"
else
  echo "ERROR: patch failed for function sig"
  rm -f "$OLD_TMP" "$NEW_TMP"
fi

echo "Patching lib.rs: remove unused import SliceRandom and mut"
# Remove the line with `use rand::seq::SliceRandom;`
sed -i '' '/use rand::seq::SliceRandom;/d' crates/pkr-cfr/src/lib.rs
# Remove `mut` from `let mut state_copy`
sed -i '' 's/let mut state_copy = state.clone();/let state_copy = state.clone();/' crates/pkr-cfr/src/lib.rs

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
  git commit -m "fix(cfr): resolve compilation errors and clean up imports"
else
  echo "Tests failed. Fix errors then run the next script."
  exit 1
fi
