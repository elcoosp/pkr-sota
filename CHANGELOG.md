# Changelog

All notable changes to pkr-sota will be documented here.

## Unreleased

### Added
- `smoke.sh`: end-to-end proof-of-concept — precompute, train 10 iters,
  export blueprint, load via pkr-runtime. Verified working.
- `load_external_blueprint` ignored test in `pkr-trainer` for verifying
  the file the real CLI produces is loadable.
- Checkpointing: `pkr-trainer --checkpoint <path> --checkpoint-every <N>`.
  Resumes automatically when the checkpoint file exists.
- `CompactRegretTable::with_capacity(n)` and `Trainer::with_capacity`.
- `SolverHandle::debug_keys()` for diagnostics and tests.

### Fixed
- `get_or_create_idx` no longer overflows `next_idx` past capacity at
  checkpoint time (CAS loop, clamp on save).
- `write_blueprint` sorts keys defensively and normalizes the CDF via
  `get_average_strategy_into` (previously wrote raw strategy-sum and
  saturated to 255).
- `pkr-runtime` re-exports `SolverHandle` at the crate root.
- `precompute` gained `hand_ranks` and `centroids` subcommands that
  `run.sh` already referenced.
- `load_external_blueprint` copies `FileHeader` instead of holding a
  borrow across `SolverHandle::new`.
- `smoke.sh` now uses absolute paths and cleans `.smoke/` before each
  run (fixes test-CWD mismatch and stale-checkpoint resumption).

### Changed
- Test wall time reduced ~8x via `with_capacity` in tests and a smaller
  valuenet training scope.
- `run.sh` and `justfile` updated with checkpoint flags and a clippy gate.
- `.gitignore` ignores `*.ckpt` and `.smoke/`.
