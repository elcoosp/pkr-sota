# M-31: replace `unsafe { &mut *node }` in the vector solver with an arena

## Problem

`crates/pkr-subgame/src/vector.rs` builds its tree as a recursive `Node`
enum where each `ActNode`/`ChanceNode` owns its kids (`Vec<Node>`). CFR
needs to mutate regrets deep in that tree while holding `&self`, so the
code launders the node into a raw pointer:

    fn cfr(&self, node: *mut Node, p: usize, reach: [&[f32]; 2], t: f32) -> Vec<f32> {
        let node = unsafe { &mut *node };
        ...
        let kid: *mut Node = &mut an.kids[a];
        let v = self.cfr(kid, p, rr, t);
        ...
    }

`self.cfr(...)` re-enters through `&self` while an `&mut Node` created
from `self.root` is still alive. That is aliasing UB by the book: the
compiler is told there is one `&mut` but the same memory is reachable
through `&self`.

## Fix: arena

Tree nodes live in a `Vec<Node>` on the solver. Ids are `u32` indices
into that arena. Kids become `Vec<u32>`. Recursion takes `idx: u32`
and a `&mut self`, and re-acquires the node only when it must write.

Layout:

    pub struct Solver<'a> {
        ...
        arena: Vec<Node>,
        root_idx: u32,     // index into arena; 0 for an empty solver
        ...
    }

    struct ActNode { player: usize, acts: Vec<Act>, kids: Vec<u32>, ... }
    struct ChanceNode { cards: Vec<u8>, kids: Vec<u32>, n_deck: f32 }

## Steps (each a separate commit)

1. **Arena skeleton.** Add `arena`, `root_idx`. `build`/`end_street`/
   `showdown` become `&mut self -> u32` and push into `arena`. The
   public-facing `root` accessors go through `root_idx`.

2. **CFR.** `cfr(&mut self, idx: u32, ...)`. Inside the Act branch,
   read what you need, recurse, then reacquire `&mut self.arena[idx]`
   to write regrets. Same for the opponent branch (no writes).

3. **Read walks.** `br`, `bp_walk`, `solved_br_p1` become
   `&self, idx: u32`. Kids are indices; reads are safe.

4. **Public API.** `p0_root`, `p0_root_mut`, `install_gadget`,
   `root_strategy`, `root_strategy_all`, `set_root_strategy`,
   `blueprint_root_strategy`, `blueprint_strategy_at`, `opp_br_value`,
   `exploitability`, `iterate`, `n_root_actions` all go via `root_idx`.

5. **Tests.** The existing suite already covers the solver
   end-to-end. The safety sweep (4-subgame gadget check) and the
   convergence tests must produce the SAME exploitability numbers as
   before the refactor. Any drift is a bug.

## Safety check

`cargo test -p pkr-subgame` and the pkr-runtime integration tests
(`gadget_both_seats`, `bound_live_path`, `session_smoke`) must all
pass unchanged. The number that must not move is the exploitability
in `vector.rs`'s own tests.

## Risk

Moderate. The borrowck will reject anything unsafe, so the refactor
cannot accidentally introduce aliasing. The functional risk is a bug
in the arena index management (off-by-one, wrong child). Tests catch
those because the exploitability is very sensitive to tree shape.

## Why now

K=7 is decided; the K=7 baseline is safe on disk. This is the last
audit item with unbounded scope and no gating dependency.
