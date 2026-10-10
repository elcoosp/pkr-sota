//! Blueprint-vs-blueprint duplicate tournament.
//!
//! The audit (F9) asked for a real-game evaluation. The existing
//! `eval_paired` compares a blueprint against a *scripted* bot; this
//! module compares two blueprints against each other under the same
//! paired design, which is what you need to decide whether one trained
//! checkpoint beats another.
//!
//! # Method
//!
//! For each hand:
//!
//!   pass 1: A plays seat 0, B plays seat 1 — chip delta to seat 0
//!   pass 2: B plays seat 0, A plays seat 1 — chip delta to seat 0
//!
//! Both passes deal the same hole cards and runout, and every decision
//! is drawn with common random numbers (decision K uses a seed derived
//! from `base_seed + K * const`). That removes runout variance and
//! sampling variance, leaving only strategic difference. The headline
//! number is `mean_diff(A - B) = mean(pass1) - mean(pass2)` per hand.
//!
//! # What this does NOT do (yet)
//!
//! - Enumerate remaining runouts for all-in-before-river hands. The
//!   paired design already cancels runout variance between A and B, so
//!   the *difference* is unbiased even with a fixed runout. The audit's
//!   suggestion matters for absolute chip counts, not the paired delta.
//! - Real-game LBR lower bound (audit F9 item 3).
//! - Off-abstraction opponent sizes. Both A and B play the abstracted
//!   tree, so this measures "which checkpoint is better at THIS game",
//!   not "which is better at HUNL".

use crate::EvalContext;
use pkr_contracts::{AbstractionBuilder, BlueprintProvider, Evaluator};
use pkr_core::state::{Action, ActionKind, GameState, Street};
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use std::path::{Path, PathBuf};

/// Per-opponent paired result.
#[derive(Debug, Clone)]
pub struct TournamentResult {
    pub hands: u32,
    pub seed: u64,
    /// Mean chips/deal to whoever is in seat 0 when A is in seat 0.
    /// Equivalently, A's mean chip profit from the SB position.
    pub mean_a_seat0: f64,
    /// Mean chips/deal to whoever is in seat 0 when B is in seat 0.
    /// Equivalently, B's mean chip profit from the SB position.
    pub mean_b_seat0: f64,
    /// Headline: mean(A's seat-0 profit - B's seat-0 profit). Both
    /// passes are measured from the SAME seat's perspective, which
    /// cancels runout variance and is exactly zero when A == B.
    pub mean_diff: f64,
    /// Standard error of `mean_diff` on the per-hand paired differences.
    pub se_diff: f64,
    /// t-statistic = mean_diff / se_diff.
    pub t: f64,
}

/// Deal 9 unique cards from a fresh shuffle. Deterministic given `seed`.
/// splitmix64 finalizer: decorrelate adjacent seeds before they reach
/// SmallRng (xoshiro), whose streams for adjacent seeds are strongly
/// correlated.
#[inline]
fn mix_seed(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn deal_hand(seed: u64) -> ([u8; 2], [u8; 2], [u8; 5]) {
    let mut rng = SmallRng::seed_from_u64(mix_seed(seed));
    let mut deck: [u8; 52] = core::array::from_fn(|i| i as u8);
    for i in 0..9 {
        let k = i + rng.random_range(0..(52 - i));
        deck.swap(i, k);
    }
    (
        [deck[0], deck[1]],
        [deck[2], deck[3]],
        [deck[4], deck[5], deck[6], deck[7], deck[8]],
    )
}

/// Play one hand. `hero` is (provider, seat); the other provider plays
/// the other seat. Returns hero's chip profit.
fn play_one(
    hero: &dyn BlueprintProvider,
    villain: &dyn BlueprintProvider,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    hero_hole: [u8; 2],
    villain_hole: [u8; 2],
    runout: &[u8; 5],
    hero_seat: u8,
    base_seed: u64,
    search_hero: Option<&dyn pkr_runtime::subgame::SearchHandle>,
    search_villain: Option<&dyn pkr_runtime::subgame::SearchHandle>,
) -> f32 {
    assert!(hero_seat < 2, "hero_seat must be 0 or 1");
    let vill_seat = 1 - hero_seat;

    // Assign holes by seat.
    let holes: [[u8; 2]; 2] = if hero_seat == 0 {
        [hero_hole, villain_hole]
    } else {
        [villain_hole, hero_hole]
    };

    let mut state = GameState::new(pkr_core::state::stack_chips(), 1.0, 2.0);
    state.set_hole_cards(holes[0], holes[1]);

    // We need two contexts so seat 0 uses one provider and seat 1 the
    // other, even though they share an abstraction and evaluator.
    let ctx_seat0 = EvalContext {
        provider: if hero_seat == 0 { hero } else { villain },
        abstraction,
        evaluator,
        search: if hero_seat == 0 { search_hero } else { search_villain },
    };
    let ctx_seat1 = EvalContext {
        provider: if hero_seat == 0 { villain } else { hero },
        abstraction,
        evaluator,
        search: if hero_seat == 0 { search_villain } else { search_hero },
    };

    let mut deck_idx = 0usize;
    let mut decision_idx: u64 = 0;
    let mut steps = 0u32;

    while !state.is_terminal() && steps < 80 {
        steps += 1;

        let decision_seed =
            base_seed.wrapping_add(decision_idx.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        decision_idx += 1;
        let mut dec_rng = SmallRng::seed_from_u64(mix_seed(decision_seed));

        let act = if state.actor == 0 {
            let (a, _) = decide_via(&ctx_seat0, &state, &mut dec_rng);
            a
        } else {
            let (a, _) = decide_via(&ctx_seat1, &state, &mut dec_rng);
            a
        };
        state.apply_action_in_place(&act);

        if state.is_street_complete() && state.street != Street::River {
            let need = match state.street {
                Street::Preflop => 3,
                Street::Flop => 1,
                Street::Turn => 1,
                Street::River => 0,
            };
            if deck_idx + need > runout.len() {
                break;
            }
            let cards: Vec<u8> = runout[deck_idx..deck_idx + need].to_vec();
            deck_idx += need;
            state.advance_street_in_place(&cards);
        }
    }

    let payoff_seat0 = if state.is_terminal() {
        state.terminal_payoff(0, evaluator)
    } else {
        0.0
    };
    let hero_delta = if hero_seat == 0 {
        payoff_seat0
    } else {
        -payoff_seat0
    };
    let _ = vill_seat; // reserved for future use
    hero_delta
}

/// Wrapper: pick a concrete action for the actor using the ctx's
/// provider. Mirrors `decide_from_blueprint` (private in lib.rs) but
/// takes the ctx directly.
fn decide_via(
    ctx: &EvalContext,
    state: &GameState,
    rng: &mut SmallRng,
) -> (Action, bool) {
    // Legal concrete actions.
    let mut buf: [Action; 8] = [Action {
        player: 0,
        kind: ActionKind::Fold,
    }; 8];
    let n = state.legal_actions_into(&mut buf);
    if n == 0 {
        return (
            Action { player: state.actor, kind: ActionKind::Fold },
            false,
        );
    }

    // Search first, if configured: ask the subgame handle for a strategy
    // on the current street. Uniform opponent range (no tracker here), so
    // this UNDERSTATES search's benefit vs the tracker-based LBR test.
    if let Some(sh) = ctx.search {
        // Uniform opponent range, MASKED by the board: a board card
        // cannot be in a live hand, and passing it produces impossible
        // deals (hole/board overlap) the solver must reject.
        const N_HANDS: usize = 1326;
        let mut uniform = [0.0f64; N_HANDS];
        let board_cards = &state.board[..state.board_len as usize];
        let mut n_legal = 0usize;
        for idx in 0..N_HANDS {
            let h = pkr_subgame::range_tracker::hole_at(idx);
            if h.iter().any(|c| board_cards.contains(c)) { continue; }
            uniform[idx] = 1.0; n_legal += 1;
        }
        if n_legal > 0 {
            let u = 1.0 / n_legal as f64;
            for x in uniform.iter_mut() { if *x > 0.0 { *x = u; } }
        }
        let hole = &state.hole[state.actor];
        if let Some(strat) = sh.decide(state, hole, &uniform) {
            // strat is bucket-indexed; sample a legal bucket via the
            // shared helper so this loop cannot drift from the K value.
            let b2f = pkr_core::abstraction::first_legal_by_bucket(
                &buf[..n],
                state.stacks[state.actor],
                state.street_bets[state.actor],
                state.street_bets[1 - state.actor],
                state.pot,
            );
            let r = rng.random::<f64>();
            if let Some(i) = pkr_core::abstraction::sample_legal_index(&strat, &b2f, r) {
                return (buf[i as usize], true);
            }
        }
    }

    // Compute the infoset hash and look up the provider.
    let actor = state.actor;
    let mut sig_buf = [0u8; 8];
    let sig_len = state.infoset_signature_into(&mut sig_buf);
    let history = &sig_buf[..sig_len];
    let hole = &state.hole[actor];
    let board: &[u8] = &state.board[..state.board_len as usize];
    let hash = ctx
        .abstraction
        .get_infoset_hash(hole, board, history, state.street as u8);

    let advice = match ctx.provider.lookup(hash) {
        Some(a) => a,
        None => {
            // Fallback: byte-for-byte the same policy as
            // `crate::decide_from_blueprint`, so arena and tournament
            // resolve the same checkpoint to the same concrete action
            // on a hash miss.
            //
            // Before this fix the miss path was "check/call if legal,
            // else fold", which made the two F9 tools measure
            // different agents whenever the blueprint missed. Keep in
            // sync with lib.rs.
            let to_call = state.bet_to_call();
            if to_call <= 0.0 {
                for act in buf.iter().take(n) {
                    if matches!(act.kind, ActionKind::Check) {
                        return (*act, false);
                    }
                }
                return (buf[0], false);
            }
            let pot = state.pot.max(1.0);
            let pot_odds = to_call / (pot + to_call);
            // F8b: rank-based strength, not card-ID.
            let rank = |c: u8| (c % 13) as f32;
            let strength = (rank(hole[0]) + rank(hole[1])) / 24.0;
            if strength >= pot_odds {
                for act in buf.iter().take(n) {
                    if matches!(act.kind, ActionKind::Call) {
                        return (*act, false);
                    }
                }
            }
            for act in buf.iter().take(n) {
                if matches!(act.kind, ActionKind::Fold) {
                    return (*act, false);
                }
            }
            return (buf[0], false);
        }
    };

    // Decode CDF into per-bucket probabilities.
    let mut probs = [0.0f32; pkr_core::abstraction::NUM_ACTION_BUCKETS];
    let mut prev = 0u16;
    let len = (advice.len as usize).min(pkr_core::abstraction::NUM_ACTION_BUCKETS);
    for i in 0..len {
        let c = advice.cdf_probabilities[i] as u16;
        probs[i] = (c.saturating_sub(prev)) as f32 / 255.0;
        prev = c;
    }

    // Bucket each legal action.
    let bucket_to_first = pkr_core::abstraction::first_legal_by_bucket(
        &buf[..n],
        state.stacks[actor],
        state.street_bets[actor],
        state.street_bets[1 - actor],
        state.pot,
    );

    // Sample a bucket from the blueprint's distribution, restricted to
    // legal ones.
    let r = rng.random::<f64>();
    let idx = pkr_core::abstraction::sample_legal_index(&probs, &bucket_to_first, r)
        .unwrap_or_else(|| bucket_to_first.iter().find_map(|x| *x).unwrap_or(0));
    (buf[idx as usize], true)
}

/// Where the abstraction tables for a tournament come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TablesDirSource {
    /// The caller passed `--tables DIR`.
    Explicit,
    /// No `--tables` was given, so A's parent directory is used.
    AParent,
}

/// Resolved tables-directory choice for a tournament run.
#[derive(Debug, Clone)]
pub struct TablesDir {
    /// Directory the abstraction tables are loaded from.
    pub dir: PathBuf,
    /// Which rule produced `dir`.
    pub source: TablesDirSource,
    /// True when `source == AParent` AND A's and B's parent
    /// directories differ. Both checkpoints will be loaded against A's
    /// tables; if B was trained against a different abstraction the
    /// comparison is meaningless. The fingerprint check catches a
    /// feature-version mismatch only when the `PKR_CENTROID_FEATURE_V`
    /// environment variable is exported to match one of the two, so
    /// the default (unset) path can silently pair two checkpoints from
    /// different abstractions. The caller should warn when this is
    /// true.
    pub a_b_diverge: bool,
}

/// Resolve the abstraction-tables directory for a tournament.
///
/// `explicit` (`--tables DIR`) always wins. Otherwise A's parent
/// directory is used. Pure function; no filesystem access. Callers
/// that want the divergence warning log it themselves using
/// [`TablesDir::a_b_diverge`].
pub fn resolve_tables_dir(
    a: &Path,
    b: &Path,
    explicit: Option<&Path>,
) -> TablesDir {
    if let Some(d) = explicit {
        return TablesDir {
            dir: d.to_path_buf(),
            source: TablesDirSource::Explicit,
            a_b_diverge: false,
        };
    }
    let a_parent: &Path = a
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let b_parent: &Path = b
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    TablesDir {
        dir: a_parent.to_path_buf(),
        source: TablesDirSource::AParent,
        a_b_diverge: a_parent != b_parent,
    }
}

/// Duplicate tournament between two blueprints.
///
/// Prefer this over two independent `run_eval_harness` calls whenever
/// you're comparing two providers directly: the paired SE is typically
/// several times smaller at the same hand count.
pub fn tournament(
    a: &dyn BlueprintProvider,
    b: &dyn BlueprintProvider,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    hands: u32,
    base_seed: u64,
    search_a: Option<&dyn pkr_runtime::subgame::SearchHandle>,
    search_b: Option<&dyn pkr_runtime::subgame::SearchHandle>,
) -> TournamentResult {
    let mut diffs: Vec<f64> = Vec::with_capacity(hands as usize);
    let mut sum_a_seat0 = 0.0f64;
    let mut sum_b_seat0 = 0.0f64;

    // Progress: a 20k-deal search H2H runs for hours and the loop prints
    // nothing otherwise. Every ~10% (min 500, max 2000 deals) to stderr.
    let progress_every = (hands / 10).clamp(1, 2000);
    let t_start = std::time::Instant::now();

    for i in 0..hands as u64 {
        if i > 0 && i % progress_every as u64 == 0 {
            let done = i as f64;
            let elapsed = t_start.elapsed().as_secs_f64().max(1e-9);
            let rate = done / elapsed;
            let eta = (hands as f64 - done) / rate.max(1e-9);
            eprintln!(
                "[tournament] {i}/{} deals ({:.0}%), {:.2} deals/s, ETA {:.0}m",
                hands, 100.0 * done / hands as f64, rate, eta / 60.0
            );
        }
        // Spread the base seed by a large odd constant so different
        // --seed values give DISJOINT deal sets. A plain base_seed+i made
        // seed 43's deals = seed 42's shifted by one (19999/20000 shared).
        let seed = base_seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(i);
        let (h0, h1, runout) = deal_hand(seed);

        // Pass 1: A in seat 0, B in seat 1. Measured as seat-0 profit.
        let a_at_seat0 = play_one(a, b, abstraction, evaluator, h0, h1, &runout, 0, seed, search_a, search_b);

        // Pass 2: B in seat 0, A in seat 1. Measured as seat-0 profit.
        let b_at_seat0 = play_one(b, a, abstraction, evaluator, h0, h1, &runout, 0, seed, search_b, search_a);

        // Same-seat paired diff: how much more does A win from the
        // SB than B does from the SB, on the same deal?
        let diff = a_at_seat0 as f64 - b_at_seat0 as f64;

        sum_a_seat0 += a_at_seat0 as f64;
        sum_b_seat0 += b_at_seat0 as f64;
        diffs.push(diff);
    }

    let n = diffs.len().max(1) as f64;
    let mean_a_seat0 = sum_a_seat0 / n;
    let mean_b_seat0 = sum_b_seat0 / n;
    let mean_diff = diffs.iter().sum::<f64>() / n;
    let var = if diffs.len() > 1 {
        diffs.iter().map(|d| (d - mean_diff).powi(2)).sum::<f64>() / (n - 1.0)
    } else {
        0.0
    };
    let se_diff = (var / n).sqrt();
    let t = if se_diff > 0.0 { mean_diff / se_diff } else { 0.0 };

    TournamentResult {
        hands,
        seed: base_seed,
        mean_a_seat0,
        mean_b_seat0,
        mean_diff,
        se_diff,
        t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_contracts::SotaAdvice;
    use std::path::{Path, PathBuf};

    #[test]
    fn resolve_tables_dir_explicit_wins() {
        let a = Path::new("/x/a/ckpt.bin");
        let b = Path::new("/y/b/ckpt.bin");
        let r = resolve_tables_dir(a, b, Some(Path::new("/z/tables")));
        assert_eq!(r.dir, PathBuf::from("/z/tables"));
        assert_eq!(r.source, TablesDirSource::Explicit);
        assert!(!r.a_b_diverge);
    }

    #[test]
    fn resolve_tables_dir_falls_back_to_a_parent() {
        let a = Path::new("/x/a/ckpt.bin");
        let b = Path::new("/y/b/ckpt.bin");
        let r = resolve_tables_dir(a, b, None);
        assert_eq!(r.dir, PathBuf::from("/x/a"));
        assert_eq!(r.source, TablesDirSource::AParent);
        assert!(r.a_b_diverge, "different parents must set a_b_diverge");
    }

    #[test]
    fn resolve_tables_dir_same_parent_no_warning() {
        let a = Path::new("/x/a/ckpt_a.bin");
        let b = Path::new("/x/a/ckpt_b.bin");
        let r = resolve_tables_dir(a, b, None);
        assert_eq!(r.dir, PathBuf::from("/x/a"));
        assert!(!r.a_b_diverge);
    }

    #[test]
    fn resolve_tables_dir_bare_filenames_use_cwd() {
        let a = Path::new("ckpt_a.bin");
        let b = Path::new("ckpt_b.bin");
        let r = resolve_tables_dir(a, b, None);
        assert_eq!(r.dir, PathBuf::from("."));
        assert!(!r.a_b_diverge);
    }

    /// Task 0.2: A vs A must be EXACTLY zero (paired duplicate design).
    /// Regression: adjacent base seeds must yield DIFFERENT deals after
    /// the splitmix64 mix. Before the fix, seeds 42-45 produced nearly
    /// identical tournaments.
    /// Regression: two base seeds must not share deals. Before the fix,
    /// seed 43 == seed 42 shifted by one, so their samples overlapped
    /// 19999/20000.
    #[test]
    fn seeds_give_disjoint_deals() {
        let spread = |b: u64, i: u64| b.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(i);
        let mut shared = 0;
        let set: std::collections::HashSet<_> = (0..500).map(|i| deal_hand(spread(42, i))).collect();
        for i in 0..500 {
            if set.contains(&deal_hand(spread(43, i))) {
                shared += 1;
            }
        }
        assert!(shared < 5, "seeds 42/43 share {shared}/500 deals");
    }

    #[test]
    fn adjacent_seeds_decorrelate() {
        let a = deal_hand(mix_seed(42));
        let b = deal_hand(mix_seed(43));
        let c = deal_hand(mix_seed(44));
        assert!(a != b, "seed 42 and 43 produced the same deal");
        assert!(b != c, "seed 43 and 44 produced the same deal");
        // and the raw-mix values themselves differ wildly
        assert!((mix_seed(42) as i128 - mix_seed(43) as i128).abs() > 1_000_000);
    }

    #[test]
    fn self_h2h_is_exactly_zero() {
        // Uniform provider, no search, same instance both sides.
        let (abs, ev) = workspace_smoke();
        let r = tournament(&Uniform, &Uniform, &abs, &ev, 50, 42, None, None);
        assert_eq!(r.mean_diff, 0.0, "A vs A mean_diff must be exactly 0");
        assert_eq!(r.se_diff, 0.0, "A vs A se_diff must be exactly 0");
        assert_eq!(r.t, 0.0);
    }

    /// A trivial provider that always returns uniform over 6 buckets.
    struct Uniform;
    impl BlueprintProvider for Uniform {
        fn lookup(&self, _h: u64) -> Option<SotaAdvice> {
            Some(SotaAdvice {
                cdf_probabilities: [
                    42, 85, 128, 170, 212, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                ],
                len: 6,
            })
        }
    }

    /// A provider that always checks/calls.
    struct Passive;
    impl BlueprintProvider for Passive {
        fn lookup(&self, _h: u64) -> Option<SotaAdvice> {
            Some(SotaAdvice {
                cdf_probabilities: [
                    0, 128, 128, 128, 128, 128, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                ],
                len: 6,
            })
        }
    }

    fn workspace_smoke() -> (pkr_abstraction::KMeansAbstraction, pkr_eval::NlheEvaluator) {
        let m = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let ws = m.parent().unwrap().parent().unwrap();
        let dir = ws.join("outputs/v0-smoke");
        let store = pkr_abstraction::load_centroids(
            dir.join("centroids.bin").to_str().unwrap(),
        )
        .expect("smoke centroids");
        let abs = pkr_abstraction::KMeansAbstraction::from_store(
            store,
            std::sync::Arc::new(pkr_eval::NlheEvaluator),
        );
        abs.init_table(0, dir.join("preflop_abstraction.bin").to_str().unwrap()).unwrap();
        abs.init_table(1, dir.join("flop_abstraction.bin").to_str().unwrap()).unwrap();
        abs.init_table(2, dir.join("turn_abstraction.bin").to_str().unwrap()).unwrap();
        abs.init_table(3, dir.join("river_buckets.bin").to_str().unwrap()).unwrap();
        (abs, pkr_eval::NlheEvaluator)
    }

    /// A provider that puts 100% of the mass on the JAM bucket (6).
    struct AllJam;
    impl BlueprintProvider for AllJam {
        fn lookup(&self, _h: u64) -> Option<SotaAdvice> {
            let mut cdf = [255u8; 16];
            // buckets 0..5 get 0 cumulative, then 255 at index 6.
            for x in cdf.iter_mut().take(6) {
                *x = 0;
            }
            cdf[6] = 255;
            Some(SotaAdvice { cdf_probabilities: cdf, len: 7 })
        }
    }

    /// Regression (K=6->7): the jam bucket (6) must be playable by
    /// `decide_via`. Before the fix the decode loop was hard-coded to 6
    /// buckets, so a 100%-jam provider was read as Fold.
    #[test]
    fn jam_bucket_is_playable_by_decide_via() {
        use pkr_core::state::{ActionKind, GameState};
        let ev = pkr_eval::NlheEvaluator;
        let abs = pkr_abstraction::KMeansAbstraction::new(
            vec![(0.5, 0.25)],
            std::sync::Arc::new(pkr_eval::NlheEvaluator),
        );
        let ctx = EvalContext {
            provider: &AllJam,
            abstraction: &abs,
            evaluator: &ev,
            search: None,
        };
        // Postflop state, P0 to act -> can jam.
        let mut st = GameState::new(200.0, 1.0, 2.0);
        st.set_hole_cards([0, 5], [10, 15]);
        st.apply_action_in_place(&pkr_core::state::Action {
            player: 0,
            kind: ActionKind::Call,
        });
        st.apply_action_in_place(&pkr_core::state::Action {
            player: 1,
            kind: ActionKind::Check,
        });
        st.advance_street_in_place(&[20, 25, 30]);
        let mut rng = SmallRng::seed_from_u64(42);
        let (act, used_bp) = decide_via(&ctx, &st, &mut rng);
        assert!(used_bp, "provider had mass, must be used");
        assert!(
            matches!(act.kind, ActionKind::Bet(_)),
            "100%-jam provider must bet/jam, got {:?}",
            act.kind
        );
    }

    #[test]
    #[ignore]
    fn self_tournament_is_zero() {
        let (abs, ev) = workspace_smoke();
        let r = tournament(&Uniform, &Uniform, &abs, &ev, 50, 42, None, None);
        assert!(
            r.mean_diff.abs() < 1e-9,
            "A vs A must be exactly zero, got {}",
            r.mean_diff
        );
    }

    #[test]
    #[ignore]
    fn uniform_beats_passive_or_loses_sign_consistently() {
        let (abs, ev) = workspace_smoke();
        // No assertion on sign — this is a smoke check that the harness
        // produces a finite, non-degenerate number.
        let r = tournament(&Uniform, &Passive, &abs, &ev, 50, 42, None, None);
        assert!(r.mean_diff.is_finite());
        assert!(r.se_diff.is_finite());
    }
}
