#!/usr/bin/env bash
# The tight-loop check. Runs in seconds when nothing changed.
# Use this between code edits. Run ./smoke.sh before commits.
set -euo pipefail
cd "$(dirname "$0")"

echo "=== fast check ==="

echo "--> cargo check (workspace, all targets)"
cargo check --workspace --all-targets --quiet 2>&1 | tail -20

echo "--> cargo clippy (workspace, all targets, warnings as errors)"
if ! cargo clippy --workspace --all-targets --quiet -- -D warnings 2>&1 | tail -30; then
    echo ""
    echo "clippy FAILED (see above)"
    exit 1
fi

echo "--> cargo nextest (workspace, fail-fast off)"
cargo nextest run --workspace --no-fail-fast 2>&1 | tail -20

echo ""
echo "=== fast check passed ==="
