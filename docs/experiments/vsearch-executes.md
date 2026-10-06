# Vector search: integration executes (2026-10-06)

First end-to-end run of the Phase-4 path (`RuntimeSession::advise` ->
`advise_vsearch` -> vector solver). Same state, two configs:

| path | root distribution [b0..b5] |
|---|---|
| blueprint | [0, 0, 0.09, 0.17, 0.15, 0.59] |
| PKR_VSEARCH=1 | [0, 0.001, 0, 0.067, 0.93, 0.006] |

**Different -> the vector solver overrides the blueprint; the integration
is live, not a silent fall-through.**

## NOT proven

That the vsearch decision is BETTER. It is a different strategy; whether
it lowers real-game exploitability is the next measurement (LBR with
`PKR_VSEARCH=1` vs off, paired, + H2H). The sampled-hand search looked
better on LBR and lost H2H, so LBR alone is not enough.

Run: PKR_DATA_DIR=<dir> PKR_PREFLOP_EXACT=0 PKR_SIG_V3=1 [PKR_VSEARCH=1] \
     cargo run --release -p pkr-runtime --example bot_loop
