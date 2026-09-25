# Fast inner loop — check + clippy + tests only. Seconds when warm.
fast:
    ./fast.sh

# End-to-end pipeline check — trains and exports a real blueprint.
# Caches abstraction artifacts; use SMOKE_FRESH=1 to force a full rebuild.
smoke:
    ./smoke.sh

# Force-rebuild everything and run the full end-to-end check.
smoke-fresh:
    SMOKE_FRESH=1 ./smoke.sh

# Production-scale profile with metrics.
prof:
    ./proftest.sh

# Thread-scaling benchmark.
bench:
    ./bench.sh

# Full training run (edit run.sh first).
train:
    ./run.sh

# Format + clippy + tests, strict. Slow but thorough.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo nextest run --workspace

versions:
    @ls -1 outputs/ 2>/dev/null | grep -E "^v[0-9]" || echo "(none yet)"

manifest v="v1":
    @cat "outputs/{{ v }}/manifest.txt"

drop v:
    rm -rf "outputs/{{ v }}"
    @echo "dropped outputs/{{ v }}"

wr:
    watchexec -w ./wr.sh --clear -r "./wr.sh"
wr1:
    watchexec -w ./wr1.sh --clear -r "./wr1.sh"
