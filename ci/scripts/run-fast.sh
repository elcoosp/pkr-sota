#!/usr/bin/env bash
# CI wrapper for ./fast.sh. Used by .github/workflows/fast.yml.
# Runs fmt-check, clippy -D warnings, nextest --no-fail-fast, doctest.
# Exits non-zero on any failure. Logs are kept raw for the GitHub UI.
set -euo pipefail
cd "$(dirname "$0")/../.."

echo "==> [1/4] cargo fmt --check"
cargo fmt --all -- --check

echo "==> [2/4] cargo clippy (workspace, -D warnings)"
cargo clippy --workspace --all-targets --quiet -- -D warnings

echo "==> [3/4] cargo nextest (workspace, --no-fail-fast)"
cargo nextest run --workspace --no-fail-fast

echo "==> [4/4] cargo test --doc"
cargo test --doc --workspace --quiet

echo "=== run-fast.sh passed ==="
