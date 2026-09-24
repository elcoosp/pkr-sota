#!/usr/bin/env bash
# Daily supply-chain gate. Exits non-zero on:
#  - any RUSTSEC advisory with severity >= medium
#  - any license violation per deny.toml
#  - any wildcard dependency
set -euo pipefail
cd "$(dirname "$0")/../.."

echo "==> cargo audit"
cargo audit --deny warnings

echo "==> cargo deny check"
cargo deny check advisories bans licenses sources
