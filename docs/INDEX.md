# pkr-sota Documentation Index

**Source of truth for implementation status:** `status.md`
**Source of truth for architecture:** `arch-overview.md`
**Source of truth for findings (numbers):** `experiments/VERIFIED_FINDINGS.md`
**Source of truth for plan progress:** `roadmap/PLAN_STATUS_2026-10-10.md`

---

## Entry points

| Document | Purpose |
|---|---|
| `../README.md` | Project overview, quickstart, workspace map |
| `status.md` | What works, what doesn't, measured performance |
| `arch-overview.md` | Architecture and design decisions |
| `spec/architecture.md` | Level 3 architectural specification |
| `spec/bst.md` | Level 4 behavioral specification and test plan |
| `PKR_AUDIT_AND_FIX_PLAN.md` | Audit findings F1-F9 and disposition |
| `blueprint-compat.md` | Blueprint format compatibility notes |

## Layout

```
docs/
  INDEX.md                  <- you are here
  status.md                 <- current implementation status
  arch-overview.md
  PKR_AUDIT_AND_FIX_PLAN.md
  blueprint-compat.md
  experiments/              <- one doc per training run or investigation
  experiments/archive/      <- superseded (Oct 2 or earlier), kept for history
  handoffs/                 <- session handoffs (recent, unarchived)
  handoffs/archive/         <- sessions Oct 1-7
  roadmap/                  <- plan status, design notes, queues
  reviews/                  <- external code and competitiveness reviews
  playbooks/                <- process and improvement playbooks
  spec/                     <- architecture and test plans
  archive/                  <- pre-project notes and one-shot relics
```

## Current state (2026-10-10)

- Live job: K=7 baseline train (PID 59819, ETA ~16:52). Search H2H
  (PID 9576) queued behind it. See `roadmap/queue-2026-10-10.md`.
- Decision pending: ship K=7 or revert to K=6. Harness: `/tmp/k7decide.sh`.
- Next experiment (queued): NLHE epsilon A/B, `/tmp/eps-ab.sh`.

## Experiments (`experiments/`)

One doc per training run or investigation. Every exploitability number
written before 2026-10-02 predates the in-sample-BR caveat; those are
in `experiments/archive/`. Start with:

- `experiments/VERIFIED_FINDINGS.md` - authoritative numbers
- `experiments/leduc-cfr-grid-2026-10-10.md` - eps=0.002 beats 0.01 by ~35%
- `experiments/k6-vs-k7-incomparable.md` - why K=6 and K=7 cannot meet
- `experiments/k7-landed-review-fixes.md` - K=7 downstream consumers fixed
- `experiments/phase-4.3-design-note.md` - deferred; needs API change
- `experiments/gadget-h2h-corrected.md`, `shipped-safe-search-config.md`
