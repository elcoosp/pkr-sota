#!/usr/bin/env bash
# Run miri on the two crates with most unsafe code.
# Skips integration tests and binaries — too slow for weekly budget.
set -euo pipefail
cd "$(dirname "$0")/../.."

# Install miri via rustup component. The dtolnay/rust-toolchain action
# in weekly.yml should already have it if we ask for it.
rustup component add miri 2>/dev/null || true

# Scope: pkr-core + pkr-cfr.
for CRATE in pkr-core pkr-cfr; do
    echo "==> miri: $CRATE"
    MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-strict-init" \
        cargo miri test -p "$CRATE" --lib --quiet 2>&1 | tail -30
done
