# pkr-sota Documentation Index

**Source of truth for implementation status:** `status.md`
**Source of truth for architecture:** `arch-overview.md`

---

## Entry points

| Document | Purpose |
|---|---|
| `../README.md` | Project overview, quickstart, workspace map |
| `status.md` | What works, what doesn't, measured performance, known issues |
| `arch-overview.md` | Architecture and design decisions as implemented |
| `spec/architecture.md` | Level 3 architectural specification |
| `spec/bst.md` | Level 4 behavioral specification and test plan |
| `PKR_AUDIT_AND_FIX_PLAN.md` | Audit findings F1-F9 and their disposition |

## Experiments (`experiments/`)

One doc per training run or investigation. Every exploitaibility number
in these predates the 2026-10-02 in-sample-BR caveat; see the banner at
the top of each and `experiments/turn-up-investigation.md`.

Start with:
- `experiments/turn-up-investigation.md` — the ~3-6M turn-up, resolved as an estimator artifact
- `experiments/f5-grid.md` — regret floor / averaging site / averaging weight
- `experiments/f4-abstraction-rebuild-plan.md` — potential-feature rebuild (equivalent; keep legacy)

## Roadmap (`roadmap/`)

| Document | Notes |
|---|---|
| `roadmap/post-ab-plan.md` | What to do after the abstraction A/Bs |
| `roadmap/range-aware-solving.md` | Range-aware subgame solving |
| `roadmap/runtime-tracker-integration.md` | RangeTracker wiring into the runtime |
| `roadmap/subgame-solving-plan.md` | Subgame solve plan |
| `roadmap/solve-first-river-node.md` | First river-node solve |
| `roadmap/novel-directions.md` | Speculative directions |

## Handoffs (`handoffs/`)

Session handoffs, newest last. Each is a point-in-time snapshot, not
maintained.

## Playbooks (`playbooks/`)

Large aspirational plans. Background reading; not status.

## Historical (`archive/`)

| Document | Notes |
|---|---|
| `archive/untoy-2025-04.md` | Early critique. Predates the current implementation; several claims are no longer true. |
| `archive/aivat-plan.md` | Superseded plan. |

---

## For external analysis

1. `status.md` — what the code actually does
2. `outputs/<run>/metrics.csv` — training health per window
3. `outputs/<run>/stats.json` — end-of-run config + summary
4. `outputs/<run>/blueprint.bin` — the trained artifact
