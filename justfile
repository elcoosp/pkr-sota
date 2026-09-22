wr:
    watchexec -w ./wr.sh --clear -r "./wr.sh"

check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo nextest run --workspace

smoke:
    ./smoke.sh

train:
    ./run.sh
