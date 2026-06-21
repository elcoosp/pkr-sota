# W4-T4: Trainer Binary

## Objective
Create the `pkr-trainer` binary that orchestrates the training process, runs for a specified number of iterations, and exports the blueprint.

## Dependencies
- All previous waves (`pkr-contracts`, `pkr-core`, `pkr-eval`, `pkr-abstraction`, `pkr-cfr`, `pkr-export`)

## Exclusive File Paths
- `binaries/pkr-trainer/Cargo.toml`
- `binaries/pkr-trainer/src/main.rs`

## TDD Instructions
1. **Red**: As this is a binary, write an integration test in `main.rs` (or a separate test module) that runs the binary with `--iterations 10 --output test_blueprint.bin`. Assert that the file `test_blueprint.bin` is created and its header contains the correct iteration count.
2. **Green**: Use `clap` to parse CLI arguments: `--iterations`, `--output`, `--variant`. Initialize the rules, evaluator, abstraction, and CFR Trainer. Run a loop for `--iterations` calling `trainer.run_iteration()`. After the loop, call `pkr_export::write_blueprint()` with the path from `--output`.
3. **Refactor**: Add `tracing` logs to report progress (e.g., every 10,000 iterations). Ensure memory usage remains stable during the loop.

## Acceptance Criteria
- `cargo run --bin pkr-trainer -- --iterations 1000 --output blueprint.bin` succeeds and generates a valid file.
- The binary correctly wires together all the separate crate components.
