# pkr-sota Documentation Index

**Source of truth for implementation status:** `status.md`
**Source of truth for architecture:** `arch-overview.md`

---

## Current (matches the codebase)

| Document | Purpose |
|---|---|
| `../README.md` | Project overview, quickstart, workspace map |
| `status.md` | What works, what doesn't, measured performance, known issues |
| `arch-overview.md` | Architecture and design decisions as implemented |
| `spec/architecture.md` | Level 3 architectural specification |
| `spec/bst.md` | Level 4 behavioral specification and test plan |

## Roadmap (aspirational, partially done)

| Document | Notes |
|---|---|
| `pkr-sota-winning-roadmap.md` | Phase-by-phase roadmap. Items P0-P1 mostly done; exploitation overlay and eval harness pending. |

## Historical (kept for reference)

| Document | Notes |
|---|---|
| `archive/untoy-2025-04.md` | Early critique of the codebase. Predates the current implementation; several claims are no longer true. |
| `tasks/**` | Machine-readable task definitions from the wave-based parallel development. Historical; not maintained. |

---

## For external analysis

If you are an AI or human analyzing this codebase, start with:

1. `status.md` — what the code actually does
2. `.proftest/metrics.csv` — live training health, one row per window
3. `.proftest/stats.json` — end-of-run strategy analysis + 200 sampled infosets
4. `.proftest/blueprint.bin` — the trained artifact

The `metrics.csv` columns are documented in `arch-overview.md` §3.4 and
the writer in `binaries/pkr-trainer/src/main.rs`.
