#!/usr/bin/env bash
set -uo pipefail
COMPILE_OK=true
INCOMPLETE=false

echo "Fixing choose function to handle n < k without overflow"
OLD_TMP=$(mktemp) || exit 1
NEW_TMP=$(mktemp)
cat > "$OLD_TMP" << 'OLD_CHOOSE_FN'
pub fn choose(n: u32, k: u32) -> u32 {
    match (n, k) {
        (_, 0) => 1,
        (n, 1) => n,
        (n, 2) => n * (n - 1) / 2,
        (n, 3) => n * (n - 1) * (n - 2) / 6,
        (n, 4) => n * (n - 1) * (n - 2) * (n - 3) / 24,
        (n, 5) => n * (n - 1) * (n - 2) * (n - 3) * (n - 4) / 120,
        _ => panic!("unsupported k"),
    }
}
OLD_CHOOSE_FN
cat > "$NEW_TMP" << 'NEW_CHOOSE_FN'
pub fn choose(n: u32, k: u32) -> u32 {
    if k > n {
        return 0;
    }
    match k {
        0 => 1,
        1 => n,
        2 => n * (n - 1) / 2,
        3 => n * (n - 1) * (n - 2) / 6,
        4 => n * (n - 1) * (n - 2) * (n - 3) / 24,
        5 => n * (n - 1) * (n - 2) * (n - 3) * (n - 4) / 120,
        _ => panic!("k > 5 not supported"),
    }
}
NEW_CHOOSE_FN
if python3 - "$OLD_TMP" "$NEW_TMP" crates/pkr-eval/src/lookup.rs << 'PYLOOK'
import sys
with open(sys.argv[1]) as f: old = f.read()
with open(sys.argv[2]) as f: new = f.read()
with open(sys.argv[3], 'r') as f: content = f.read()
content = content.replace(old, new)
with open(sys.argv[3], 'w') as f: f.write(content)
PYLOOK
then
  echo "choose fixed in lookup.rs"
  rm "$OLD_TMP" "$NEW_TMP"
else
  echo "ERROR: fix failed"
  rm -f "$OLD_TMP" "$NEW_TMP"
fi

# Also fix the precompute binary's choose function (same issue)
OLD_TMP2=$(mktemp) || exit 1
NEW_TMP2=$(mktemp)
cat > "$OLD_TMP2" << 'OLD_CHOOSE_PR'
fn choose(n: u32, k: u32) -> u32 {
    match (n, k) {
        (_, 0) => 1,
        (n, 1) => n,
        (n, 2) => n * (n - 1) / 2,
        (n, 3) => n * (n - 1) * (n - 2) / 6,
        (n, 4) => n * (n - 1) * (n - 2) * (n - 3) / 24,
        (n, 5) => n * (n - 1) * (n - 2) * (n - 3) * (n - 4) / 120,
        _ => panic!("unsupported k"),
    }
}
OLD_CHOOSE_PR
cat > "$NEW_TMP2" << 'NEW_CHOOSE_PR'
fn choose(n: u32, k: u32) -> u32 {
    if k > n {
        return 0;
    }
    match k {
        0 => 1,
        1 => n,
        2 => n * (n - 1) / 2,
        3 => n * (n - 1) * (n - 2) / 6,
        4 => n * (n - 1) * (n - 2) * (n - 3) / 24,
        5 => n * (n - 1) * (n - 2) * (n - 3) * (n - 4) / 120,
        _ => panic!("k > 5 not supported"),
    }
}
NEW_CHOOSE_PR
if python3 - "$OLD_TMP2" "$NEW_TMP2" crates/pkr-abstraction/src/bin/precompute.rs << 'PYPR'
import sys
with open(sys.argv[1]) as f: old = f.read()
with open(sys.argv[2]) as f: new = f.read()
with open(sys.argv[3], 'r') as f: content = f.read()
content = content.replace(old, new)
with open(sys.argv[3], 'w') as f: f.write(content)
PYPR
then
  echo "choose fixed in precompute"
  rm "$OLD_TMP2" "$NEW_TMP2"
else
  echo "ERROR: fix failed"
  rm -f "$OLD_TMP2" "$NEW_TMP2"
fi

# Clean unused store variable in trainer
sed -i '' 's/let store = load_centroids/let _store = load_centroids/' binaries/pkr-trainer/src/main.rs

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
cargo test -p pkr-abstraction -p pkr-eval
if [ $? -eq 0 ]; then
  echo "All tests passed. Committing."
  git add -A
  git commit -m "fix(eval,abstraction): handle n<k in choose, prevent overflow"
else
  echo "Tests failed. Fix errors then run the next script."
  exit 1
fi
