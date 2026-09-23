# Blueprint Compatibility

Blueprints are loadable only by code whose infoset-hash scheme matches
the code that produced them. Two recent changes invalidated older files:

| Blueprint | Produced with | Loadable with current code? |
|-----------|---------------|-----------------------------|
| v0–v6 | initial hash (flop_bucket included, river `>>6`) | No |
| v7 | same, plus the smaller k=32/64 experiments | No |
| v8 | same, plus refined 4.0.5 sizing probes | No |
| v9+ | current: river `>>3`, no flop_bucket, valid-board slice | Yes |

The change that broke compatibility was T2.2 (river bucket count `>>6`
→ `>>3` and removing `flop_bucket` from the hash). Because the *format
version* was not bumped at that time, older blueprints still load but
silently return `None` on every lookup. The runtime never raises an
error — it just treats them as fully out-of-abstraction.

## What this means in practice

- **Any blueprint produced before T2.2 must be retrained.** There is no
  fix that recovers the old hashes at load time; the hash function
  itself changed.
- The eval harness's `blueprint_hits: 0` output is the diagnostic for
  this. When the hit rate is 0%, the eval is measuring the *fallback*
  policy, not the trained strategy.
- Future hash changes must bump `FORMAT_VERSION` in
  `crates/pkr-export/src/header.rs` and the runtime must reject older
  files loudly, not silently.

## How to check a blueprint's provenance

Load it and query any hash returned by the current abstraction. If
`lookup(hash)` is `None` for a hash the abstraction produced from the
same code, the blueprint is stale.
