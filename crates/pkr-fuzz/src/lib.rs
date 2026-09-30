//! Rules fuzzing — differential testing of GameState against a reference
//! implementation over random action sequences.
//!
//! Covers: min-raise legality, all-in-below-min-raise, uncalled-bet return,
//! split pots, exact stack arithmetic. (Roadmap §4)

#![allow(clippy::assign_op_pattern)]
#![allow(clippy::map_clone)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::manual_memcpy)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::manual_is_multiple_of)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::unnecessary_map_or)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::explicit_counter_loop)]
#![allow(clippy::manual_div_ceil)]

use pkr_core::state::{Action, ActionKind, GameState, Street};
use pkr_eval::NlheEvaluator;
use rand::seq::IndexedRandom;
use rand::{Rng, RngExt};

/// Reference implementation of a poker hand tracker.
/// This is a simplified, deliberately verbose implementation used to
/// cross-check GameState. Any discrepancy indicates a bug in GameState.
#[derive(Debug, Clone)]
struct ReferenceState {
    pot: f32,
    stacks: [f32; 2],
    total_invested: [f32; 2],
    street_bets: [f32; 2],
    actor: usize,
    street: u8,
    actions_this_street: u8,
    folded: [bool; 2],
    history: Vec<Action>,
    /// E2: community cards as they are dealt. Needed for showdown
    /// payoff comparison in the harness.
    board: Vec<u8>,
}

impl ReferenceState {
    fn new(start_stack: f32, sb: f32, bb: f32) -> Self {
        ReferenceState {
            pot: sb + bb,
            stacks: [start_stack - sb, start_stack - bb],
            total_invested: [sb, bb],
            street_bets: [sb, bb],
            actor: 0,
            street: 0,
            actions_this_street: 0,
            folded: [false, false],
            history: Vec::new(),
            board: Vec::new(),
        }
    }

    fn bet_to_call(&self) -> f32 {
        let opp = 1 - self.actor;
        (self.street_bets[opp] - self.street_bets[self.actor]).max(0.0)
    }

    fn apply(&mut self, action: &Action) {
        let actor = self.actor;
        self.history.push(*action);

        match action.kind {
            ActionKind::Fold => {
                self.folded[actor] = true;
            }
            ActionKind::Check => {}
            ActionKind::Call => {
                let to_call = self.bet_to_call();
                let chips = to_call.min(self.stacks[actor]);
                self.stacks[actor] -= chips;
                self.pot += chips;
                self.total_invested[actor] += chips;
                self.street_bets[actor] += chips;
            }
            ActionKind::Bet(total) => {
                let current = self.street_bets[actor];
                let chips = (total - current).max(0.0).min(self.stacks[actor]);
                self.stacks[actor] -= chips;
                self.pot += chips;
                self.total_invested[actor] += chips;
                self.street_bets[actor] = self.street_bets[actor] + chips;
            }
        }

        self.actions_this_street += 1;
        self.actor = 1 - actor;
    }

    fn legal_actions(&self) -> Vec<ActionKind> {
        if self.folded[self.actor] {
            return vec![];
        }
        let mut actions = Vec::new();
        let to_call = self.bet_to_call();

        if to_call == 0.0 {
            actions.push(ActionKind::Check);
        } else {
            actions.push(ActionKind::Fold);
            actions.push(ActionKind::Call);
        }

        // C5b: mirrors GameState::legal_actions_into — no bets when
        // the opponent cannot respond (they are all-in).
        if self.stacks[self.actor] > 0.0 && self.stacks[1 - self.actor] > 0.0 {
            actions.push(ActionKind::Bet(
                self.stacks[self.actor] + self.street_bets[self.actor],
            ));
        }

        actions
    }

    #[allow(dead_code)]
    fn is_terminal(&self) -> bool {
        if self.folded.iter().any(|&f| f) {
            return true;
        }
        if self.street == 3 && self.actions_this_street >= 2 && self.bet_to_call() == 0.0 {
            return true;
        }
        false
    }

    fn winner(&self) -> Option<usize> {
        if self.folded[0] && !self.folded[1] {
            return Some(1);
        }
        if self.folded[1] && !self.folded[0] {
            return Some(0);
        }
        None
    }

    fn payoff(&self, player: usize) -> f32 {
        if let Some(w) = self.winner() {
            if w == player {
                self.pot - self.total_invested[player]
            } else {
                -self.total_invested[player]
            }
        } else {
            0.0
        }
    }

    fn advance_street(&mut self, cards: Vec<u8>) {
        if self.street == 3 {
            return;
        }
        // E2: record the community cards for showdown evaluation.
        for c in cards {
            self.board.push(c);
        }
        self.street += 1;
        self.actions_this_street = 0;
        self.street_bets = [0.0, 0.0];
        self.actor = 1 - self.dealer();
    }

    fn dealer(&self) -> usize {
        // In HU, the SB (player 0) is the dealer button.
        // Post-flop, the BB (player 1) acts first, so actor = 1.
        0
    }
}

/// Draw community cards for a street given the current street enum.
fn draw_board_cards(rng: &mut impl Rng, street: Street) -> Vec<u8> {
    let count = match street {
        Street::Preflop => 0,
        Street::Flop => 3,
        Street::Turn => 1,
        Street::River => 1,
    };
    (0..count).map(|_| rng.random_range(0..52) as u8).collect()
}

/// Run differential fuzzing: play N random hands, comparing GameState vs
/// ReferenceState on pot, stacks, actor, and terminal payoffs.
pub fn run_fuzz(num_hands: u32) -> FuzzingResult {
    let mut mismatches = 0u32;
    let mut hands_completed = 0u32;
    let mut rng = rand::rng();
    let evaluator = NlheEvaluator;

    for _ in 0..num_hands {
        let mut state = GameState::new(200.0, 1.0, 2.0);
        let mut ref_state = ReferenceState::new(200.0, 1.0, 2.0);

        // Deal hole cards
        let hero = [rng.random_range(0..52), rng.random_range(0..52)];
        let villain = [rng.random_range(0..52), rng.random_range(0..52)];
        if hero[0] == hero[1]
            || villain[0] == villain[1]
            || hero.iter().any(|&c| villain.contains(&c))
        {
            continue;
        }
        state.set_hole_cards(hero, villain);

        let mut steps = 0u32;
        let max_steps = 50;
        let mut mismatch = false;

        while !state.is_terminal() && steps < max_steps {
            steps += 1;

            let gs_actions = state.legal_actions();

            // Filter to only Check/Call/Fold/All-in for comparison with reference.
            // GameState also generates pot-fraction bets that the simplified
            // reference doesn't enumerate — those are tested separately.
            let comparable: Vec<Action> = gs_actions
                .into_iter()
                .filter(|a| match a.kind {
                    ActionKind::Fold | ActionKind::Check | ActionKind::Call => true,
                    ActionKind::Bet(amt) => {
                        // All-in only: when checking, all-in = stack; when raising, all-in = stack + street_bets
                        let stack = state.stacks[a.player];
                        let street_bets = state.street_bets[a.player];
                        amt >= (stack + street_bets - 0.01) || amt >= (stack - 0.01)
                    }
                })
                .collect();

            let ref_actions = ref_state.legal_actions();

            if comparable.len() != ref_actions.len() {
                mismatches += 1;
                mismatch = true;
                break;
            }

            if comparable.is_empty() {
                break;
            }

            let action = comparable
                .choose(&mut rng)
                .map(|a| a.clone())
                .unwrap_or(comparable[0]);

            state.apply_action_in_place(&action);
            ref_state.apply(&action);

            // Advance street if round is complete
            if state.is_street_complete() && state.street != Street::River {
                let board_cards = draw_board_cards(&mut rng, state.street);
                state.advance_street_in_place(&board_cards);
                ref_state.advance_street(board_cards);
            }

            // Compare key state invariants
            if (state.pot - ref_state.pot).abs() > 0.01 {
                mismatches += 1;
                mismatch = true;
                break;
            }
            if (state.stacks[0] - ref_state.stacks[0]).abs() > 0.01 {
                mismatches += 1;
                mismatch = true;
                break;
            }
            if (state.stacks[1] - ref_state.stacks[1]).abs() > 0.01 {
                mismatches += 1;
                mismatch = true;
                break;
            }
            if state.actor != ref_state.actor {
                mismatches += 1;
                mismatch = true;
                break;
            }
        }

        if !mismatch && state.is_terminal() {
            hands_completed += 1;
            if let Some(winner) = ref_state.winner() {
                let gs_payoff = state.terminal_payoff(winner, &evaluator);
                let ref_payoff = ref_state.payoff(winner);
                if (gs_payoff - ref_payoff).abs() > 0.5 {
                    mismatches += 1;
                }
            }
        }
    }

    FuzzingResult {
        hands_run: num_hands,
        hands_completed,
        mismatches,
    }
}

/// A scripted opponent for eval harness testing.
pub trait ScriptedBot: Send + Sync {
    fn act(&self, state: &GameState) -> Action;
}

/// Station bot — calls every bet, never raises, never folds.
pub struct StationBot;

impl ScriptedBot for StationBot {
    fn act(&self, state: &GameState) -> Action {
        let to_call = state.bet_to_call();
        if to_call == 0.0 {
            Action {
                player: state.actor,
                kind: ActionKind::Check,
            }
        } else {
            Action {
                player: state.actor,
                kind: ActionKind::Call,
            }
        }
    }
}

/// Nit bot — folds to any bet, checks when free.
pub struct NitBot;

impl ScriptedBot for NitBot {
    fn act(&self, state: &GameState) -> Action {
        let to_call = state.bet_to_call();
        if to_call == 0.0 {
            Action {
                player: state.actor,
                kind: ActionKind::Check,
            }
        } else {
            Action {
                player: state.actor,
                kind: ActionKind::Fold,
            }
        }
    }
}

/// Aggro bot — always goes all-in.
pub struct AggroBot;

impl ScriptedBot for AggroBot {
    fn act(&self, state: &GameState) -> Action {
        let to_call = state.bet_to_call();
        let stack = state.stacks[state.actor];
        let street_bets = state.street_bets[state.actor];
        if to_call == 0.0 {
            // C2: all-in is stacks + street_bets, not stacks alone.
            // Preflop BB facing a limp has 2 chips already posted; the
            // old `Bet(stack)` was a 199-total raise that left 1 chip
            // behind and never actually exercised the all-in bucket.
            Action {
                player: state.actor,
                kind: ActionKind::Bet(stack + street_bets),
            }
        } else if stack + street_bets > to_call {
            Action {
                player: state.actor,
                kind: ActionKind::Bet(stack + street_bets),
            }
        } else {
            Action {
                player: state.actor,
                kind: ActionKind::Call,
            }
        }
    }
}

/// Run eval harness: play hands against scripted opponents, tracking bb/100.
/// Context for the eval harness. Takes the three trait objects the host
/// application would provide at runtime — a blueprint source, an
/// abstraction builder that hashes GameState, and a hand evaluator.
///
/// This mirrors what pkr-runtime exposes to a real host: the host knows
/// the game state, hashes it via the abstraction, and looks up the
/// blueprint. `run_eval_harness` does the same thing.
pub struct EvalContext<'a> {
    pub provider: &'a dyn pkr_contracts::BlueprintProvider,
    pub abstraction: &'a dyn pkr_contracts::AbstractionBuilder,
    pub evaluator: &'a dyn pkr_contracts::Evaluator,
}

const K_BUCKETS: usize = 6;

/// Decode a u8 CDF into a probability vector of length K_BUCKETS.
fn decode_cdf_into(advice: &pkr_contracts::SotaAdvice, out: &mut [f32; K_BUCKETS]) {
    let mut prev = 0u16;
    let n = (advice.len as usize).min(K_BUCKETS);
    for i in 0..n {
        let c = advice.cdf_probabilities[i] as u16;
        out[i] = (c.saturating_sub(prev)) as f32 / 255.0;
        prev = c;
    }
    for i in n..K_BUCKETS {
        out[i] = 0.0;
    }
}

/// Given a GameState at decision time, produce a legal concrete action by
/// querying the blueprint through the abstraction. Falls back to
/// check/call vs fold when the hash is missing.
fn decide_from_blueprint(
    ctx: &EvalContext,
    state: &pkr_core::state::GameState,
    rng: &mut impl rand::Rng,
) -> (pkr_core::state::Action, bool) {
    use pkr_core::state::{Action, ActionKind};

    // Legal concrete actions for the current actor.
    let mut buf: [Action; 8] = [Action {
        player: 0,
        kind: ActionKind::Fold,
    }; 8];
    let n_legal = state.legal_actions_into(&mut buf);

    // Defensive fallback if somehow nothing is legal (should not happen).
    if n_legal == 0 {
        return (
            Action {
                player: state.actor,
                kind: ActionKind::Fold,
            },
            false,
        );
    }

    // Compute the infoset hash. NOTE: older blueprints (v8 and earlier)
    // were trained with the raw [u8; 5] board array (length 5 always),
    // so we must match that here for lookups to hit. After the T0.2d
    // fix + retrain, this should change to &state.board[..board_len].
    let hole = &state.hole[state.actor];
    let board: &[u8] = &state.board[..state.board_len as usize];
    let mut sig_buf = [0u8; 8];
    let sig_len = state.infoset_signature_into(&mut sig_buf);
    let history_bytes: &[u8] = &sig_buf[..sig_len];
    let street = state.street as u8;
    let hash = ctx
        .abstraction
        .get_infoset_hash(hole, board, history_bytes, street);

    // Look up the advice. Missing => pot-odds fallback.
    let advice = match ctx.provider.lookup(hash) {
        Some(a) => a,
        None => {
            // Fallback: check when free, call if to_call small relative to
            // pot, fold otherwise. Deliberately conservative.
            let to_call = state.bet_to_call();
            if to_call <= 0.0 {
                for act in buf.iter().take(n_legal) {
                    if matches!(act.kind, ActionKind::Check) {
                        return (*act, false);
                    }
                }
                return (buf[0], false);
            }
            let pot = state.pot.max(1.0);
            let pot_odds = to_call / (pot + to_call);
            // F8b: rank-based strength, not card-ID. Cards are encoded
            // as `suit * 13 + rank`, so `hole[0] + hole[1]` was summing
            // suit-weighted IDs — a heart 2 plus a spade 2 scored far
            // higher than a club ace plus a club king. Ranks here are
            // 0=Two .. 12=Ace.
            let rank = |c: u8| (c % 13) as f32;
            let strength = (rank(hole[0]) + rank(hole[1])) / 24.0;
            if strength >= pot_odds {
                for act in buf.iter().take(n_legal) {
                    if matches!(act.kind, ActionKind::Call) {
                        return (*act, false);
                    }
                }
            }
            for act in buf.iter().take(n_legal) {
                if matches!(act.kind, ActionKind::Fold) {
                    return (*act, false);
                }
            }
            return (buf[0], false);
        }
    };

    // Decode the CDF.
    let mut probs = [0.0f32; K_BUCKETS];
    decode_cdf_into(&advice, &mut probs);

    // Mask: which buckets have at least one concrete legal action?
    // Same bucket mapping as the traversal:
    //   0 = fold, 1 = check/call, 2 = bet <0.75 pot, 3 = bet <1.5 pot,
    //   4 = bet >=1.5 pot, 5 = all-in
    let mut bucket_has_legal = [false; K_BUCKETS];
    let mut bucket_pick = [0usize; K_BUCKETS]; // index into buf for each bucket
                                               // E3a: single source of truth for the action→bucket mapping. The
                                               // previous inline match used pre-C3 thresholds (0.75/1.5) which
                                               // meant the eval harness resolved the CDF into the wrong action
                                               // space. All evaluations before this fix are invalid.
    let actor_stacks = state.stacks[state.actor];
    let actor_street = state.street_bets[state.actor];
    let opp_street = state.street_bets[1 - state.actor];
    let actor_pot = state.pot;
    for (i, act) in buf.iter().take(n_legal).enumerate() {
        let b = pkr_core::abstraction::action_bucket(
            &act.kind,
            actor_stacks,
            actor_street,
            opp_street,
            actor_pot,
        ) as usize;
        // Prefer a non-check representative if this is the first seen
        // (mirrors the pre-C3 selection but applied to the corrected
        // bucket index).
        if !bucket_has_legal[b] || !matches!(buf[i].kind, ActionKind::Check) {
            bucket_pick[b] = i;
        }
        bucket_has_legal[b] = true;
    }

    // Zero out illegal buckets, renormalize.
    let mut total = 0.0f32;
    for b in 0..K_BUCKETS {
        if !bucket_has_legal[b] {
            probs[b] = 0.0;
        }
        total += probs[b];
    }
    if total <= 0.0 {
        // No learned mass on any legal bucket: sample uniformly.
        let legal_count = bucket_has_legal.iter().filter(|&&x| x).count().max(1);
        let pick = rng.random_range(0..legal_count);
        let mut acc = 0;
        for b in 0..K_BUCKETS {
            if bucket_has_legal[b] {
                if acc == pick {
                    return (buf[bucket_pick[b]], true);
                }
                acc += 1;
            }
        }
        return (buf[0], true);
    }
    for b in 0..K_BUCKETS {
        probs[b] /= total;
    }

    // Sample from the masked distribution.
    let r: f32 = rng.random();
    let mut acc = 0.0f32;
    for b in 0..K_BUCKETS {
        if !bucket_has_legal[b] {
            continue;
        }
        acc += probs[b];
        if r <= acc {
            return (buf[bucket_pick[b]], true);
        }
    }
    for b in (0..K_BUCKETS).rev() {
        if bucket_has_legal[b] {
            return (buf[bucket_pick[b]], true);
        }
    }
    (buf[0], true)
}

pub fn run_eval_harness(ctx: &EvalContext, num_hands: u32, rng_seed: u64) -> EvalResult {
    use pkr_core::state::GameState;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    let mut decisions: u64 = 0;
    let mut blueprint_hits: u64 = 0;
    let mut fallback_hits: u64 = 0;

    let mut results = EvalResult {
        bot_bb_per_100: 0.0,
        opponents: Vec::new(),
        decisions: 0,
        blueprint_hits: 0,
        fallback_hits: 0,
    };

    let opponents: Vec<(&str, &dyn ScriptedBot)> = vec![
        ("station", &StationBot),
        ("nit", &NitBot),
        ("aggro", &AggroBot),
    ];

    let mut rng = SmallRng::seed_from_u64(rng_seed);

    for (name, bot) in &opponents {
        let mut bot_profit = 0.0f32;
        let mut hands_played = 0u32;

        for hand_no in 0..num_hands {
            let mut state = GameState::new(200.0, 1.0, 2.0);
            let c1 = (rng.random_range(0u32..52)) as u8;
            let c2 = (rng.random_range(0u32..52)) as u8;
            let c3 = (rng.random_range(0u32..52)) as u8;
            let c4 = (rng.random_range(0u32..52)) as u8;
            if c1 == c2 || c3 == c4 || c1 == c3 || c1 == c4 || c2 == c3 || c2 == c4 {
                continue;
            }
            state.set_hole_cards([c1, c2], [c3, c4]);

            // Pre-deal the runout deterministically from the RNG.
            // IMPORTANT: the runout indices must advance monotonically,
            // otherwise the "turn" reuses a flop card and the "river"
            // reuses a turn card. The board would then contain duplicate
            // cards, and the evaluator's dedup logic would silently
            // corrupt hand strengths.
            let mut deck: Vec<u8> = (0..52)
                .filter(|c| c != &c1 && c != &c2 && c != &c3 && c != &c4)
                .collect();
            for i in 0..9 {
                let k = rng.random_range(i..deck.len());
                deck.swap(i, k);
            }
            let mut deck_idx: usize = 0;

            let mut steps = 0u32;
            while !state.is_terminal() && steps < 60 {
                steps += 1;

                let action = if state.actor == 0 {
                    let (act, used_bp) = decide_from_blueprint(ctx, &state, &mut rng);
                    decisions += 1;
                    if used_bp {
                        blueprint_hits += 1;
                    } else {
                        fallback_hits += 1;
                    }
                    act
                } else {
                    bot.act(&state)
                };

                state.apply_action_in_place(&action);

                if state.is_street_complete() && state.street != pkr_core::state::Street::River {
                    let need = match state.street {
                        pkr_core::state::Street::Preflop => 3,
                        pkr_core::state::Street::Flop => 1,
                        pkr_core::state::Street::Turn => 1,
                        pkr_core::state::Street::River => 0,
                    };
                    if deck_idx + need > deck.len() {
                        break;
                    }
                    let cards: Vec<u8> = deck[deck_idx..deck_idx + need].to_vec();
                    deck_idx += need;
                    state.advance_street_in_place(&cards);
                }
            }

            if state.is_terminal() {
                hands_played += 1;
                let payoff = state.terminal_payoff(0, ctx.evaluator);
                bot_profit += payoff;
            }
            let _ = hand_no;
        }

        // bb/100: profit / (hands / 100) / big_blind (2.0)
        let bb_per_100 = if hands_played == 0 {
            0.0
        } else {
            (bot_profit / (hands_played as f32 / 100.0)) / 2.0
        };

        results.opponents.push(OpponentResult {
            name: name.to_string(),
            bb_per_100: bb_per_100 as f64,
            actions_per_street: [[0u32; 32]; 4],
        });
    }

    results.bot_bb_per_100 = results.opponents.iter().map(|o| o.bb_per_100).sum::<f64>()
        / results.opponents.len().max(1) as f64;
    results.decisions = decisions;
    results.blueprint_hits = blueprint_hits;
    results.fallback_hits = fallback_hits;

    results
}

pub struct FuzzingResult {
    pub hands_run: u32,
    pub hands_completed: u32,
    pub mismatches: u32,
}

pub struct EvalResult {
    pub bot_bb_per_100: f64,
    pub opponents: Vec<OpponentResult>,
    /// How many decisions resolved through the blueprint vs the
    /// conservative fallback. Zero hits means the eval's hashes don't
    /// match the trainer's, and the printed bb/100 is the fallback's
    /// score, not the trained strategy's.
    pub decisions: u64,
    pub blueprint_hits: u64,
    pub fallback_hits: u64,
}

pub struct OpponentResult {
    pub name: String,
    pub bb_per_100: f64,
    pub actions_per_street: [[u32; 32]; 4],
}

// =============================================================================
// E2: seeded differential fuzz harness
// =============================================================================
//
// Differences from `run_fuzz` (kept for backward compat):
//   1. Deterministic: takes a `seed: u64`.
//   2. Shared deck: 7 unique cards pre-dealt from a Fisher-Yates shuffle.
//      Both GameState and ReferenceState receive the same board cards.
//      The old harness drew board cards with `random_range(0..52)`,
//      which could produce duplicates or reuse hole cards — every
//      showdown was therefore garbage.
//   3. Showdown payoff comparison: previously only fold endings were
//      compared. Now the harness computes terminal payoffs for both
//      implementations using the same evaluator and compares them,
//      including ties.
//   4. Action multiset comparison: previously only `len()` was checked.
//      Now the filtered action sets are compared element-wise (tolerance
//      1e-4 on Bet amounts), which catches the C2-style bucket bugs.

/// Discriminant-like ordering key for canonical sorting of action kinds.
fn kind_key(k: &ActionKind) -> u8 {
    match k {
        ActionKind::Fold => 0,
        ActionKind::Check => 1,
        ActionKind::Call => 2,
        ActionKind::Bet(_) => 3,
    }
}

/// Compare two multisets of action kinds. Bet amounts are compared
/// with 1e-4 tolerance.
fn actions_match_multiset(a: &[ActionKind], b: &[ActionKind]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut ca: Vec<ActionKind> = a.to_vec();
    let mut cb: Vec<ActionKind> = b.to_vec();
    let cmp = |x: &ActionKind, y: &ActionKind| {
        kind_key(x).cmp(&kind_key(y)).then_with(|| {
            if let (ActionKind::Bet(m), ActionKind::Bet(n)) = (x, y) {
                m.partial_cmp(n).unwrap_or(std::cmp::Ordering::Equal)
            } else {
                std::cmp::Ordering::Equal
            }
        })
    };
    ca.sort_by(cmp);
    cb.sort_by(cmp);
    for (x, y) in ca.iter().zip(cb.iter()) {
        match (x, y) {
            (ActionKind::Bet(m), ActionKind::Bet(n)) => {
                if (m - n).abs() > 1e-4 {
                    return false;
                }
            }
            _ => {
                if std::mem::discriminant(x) != std::mem::discriminant(y) {
                    return false;
                }
            }
        }
    }
    true
}

/// Terminal payoff in the reference implementation, using the same
/// evaluator and the same (hole, board) that GameState uses.
/// This mirrors `GameState::terminal_payoff` exactly, but computes
/// from the reference's own `pot`/`total_invested` fields.
fn ref_terminal_payoff(
    r: &ReferenceState,
    player: usize,
    hero: &[u8; 2],
    villain: &[u8; 2],
    evaluator: &dyn pkr_contracts::Evaluator,
) -> f32 {
    if r.folded[player] {
        return -r.total_invested[player];
    }
    let other = 1 - player;
    if r.folded[other] {
        return r.pot - r.total_invested[player];
    }
    let hero_rank = evaluator.evaluate_hand(hero, &r.board);
    let vill_rank = evaluator.evaluate_hand(villain, &r.board);
    let win = hero_rank < vill_rank;
    let tie = hero_rank == vill_rank;
    if tie {
        (r.pot / 2.0) - r.total_invested[player]
    } else if (player == 0 && win) || (player == 1 && !win) {
        r.pot - r.total_invested[player]
    } else {
        -r.total_invested[player]
    }
}

/// Run the differential fuzz harness with a fixed seed. Acceptance:
/// `mismatches == 0` over >= 2000 hands after C1+C2+C1.5+C5b semantics.
pub fn run_fuzz_seeded(num_hands: u32, seed: u64) -> FuzzingResult {
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    let mut rng = SmallRng::seed_from_u64(seed);
    let evaluator = NlheEvaluator;

    let mut mismatches = 0u32;
    let mut hands_completed = 0u32;
    let mut hands_run = 0u32;

    for hand_no in 0..num_hands {
        hands_run += 1;
        let mut state = GameState::new(200.0, 1.0, 2.0);
        let mut ref_state = ReferenceState::new(200.0, 1.0, 2.0);

        // Fisher-Yates shuffle the first 9 slots; 4 for holes, 5 for runout.
        let mut deck: [u8; 52] = std::array::from_fn(|i| i as u8);
        for i in 0..9 {
            let j = i + rng.random_range(0..(52 - i));
            deck.swap(i, j);
        }
        let hero: [u8; 2] = [deck[0], deck[1]];
        let villain: [u8; 2] = [deck[2], deck[3]];
        let runout: [u8; 5] = [deck[4], deck[5], deck[6], deck[7], deck[8]];
        let mut runout_idx = 0usize;

        state.set_hole_cards(hero, villain);

        let mut steps = 0u32;
        let max_steps = 60;
        let mut mismatch = false;

        while !state.is_terminal() && steps < max_steps {
            steps += 1;

            // Filter GameState's actions down to what the reference also
            // produces: {Fold, Check, Call, All-in}. The frac bets are
            // tested through the trainer; here we validate the core
            // arithmetic and the all-in path.
            let gs_actions_raw = state.legal_actions();
            let gs_kinds: Vec<ActionKind> = gs_actions_raw
                .iter()
                .filter(|a| match a.kind {
                    ActionKind::Fold | ActionKind::Check | ActionKind::Call => true,
                    ActionKind::Bet(amt) => {
                        let stack = state.stacks[a.player];
                        let street_bets = state.street_bets[a.player];
                        amt >= (stack + street_bets - 0.01) || amt >= (stack - 0.01)
                    }
                })
                .map(|a| a.kind)
                .collect();
            let ref_kinds = ref_state.legal_actions();

            if !actions_match_multiset(&gs_kinds, &ref_kinds) {
                mismatches += 1;
                mismatch = true;
                eprintln!(
                    "MISMATCH seed={} hand={} step={}: action-set divergence",
                    seed, hand_no, steps
                );
                eprintln!("  GS  legal_actions (filtered): {:?}", gs_kinds);
                eprintln!("  REF legal_actions          : {:?}", ref_kinds);
                eprintln!(
                    "  GS raw: {:?}",
                    gs_actions_raw.iter().map(|a| a.kind).collect::<Vec<_>>()
                );
                break;
            }

            if gs_kinds.is_empty() {
                break;
            }

            // Pick a concrete action from GS's raw set whose kind is in
            // the filtered set.
            let candidates: Vec<Action> = gs_actions_raw
                .iter()
                .filter(|a| {
                    gs_kinds.iter().any(|k| {
                        std::mem::discriminant(k) == std::mem::discriminant(&a.kind)
                            && match (k, &a.kind) {
                                (ActionKind::Bet(m), ActionKind::Bet(n)) => (m - n).abs() < 1e-4,
                                _ => true,
                            }
                    })
                })
                .copied()
                .collect();
            let pick_idx = rng.random_range(0..candidates.len());
            let action = candidates[pick_idx];

            state.apply_action_in_place(&action);
            ref_state.apply(&action);

            // Advance street if complete.
            if state.is_street_complete() && state.street != Street::River {
                let need = match state.street {
                    Street::Preflop => 3,
                    Street::Flop => 1,
                    Street::Turn => 1,
                    Street::River => 0,
                };
                if runout_idx + need > runout.len() {
                    break;
                }
                let cards = &runout[runout_idx..runout_idx + need];
                state.advance_street_in_place(cards);
                ref_state.advance_street(cards.to_vec());
                runout_idx += need;
            }

            // Compare pot, stacks, actor after every step.
            if (state.pot - ref_state.pot).abs() > 0.01
                || (state.stacks[0] - ref_state.stacks[0]).abs() > 0.01
                || (state.stacks[1] - ref_state.stacks[1]).abs() > 0.01
                || state.actor != ref_state.actor
            {
                mismatches += 1;
                mismatch = true;
                eprintln!(
                    "MISMATCH seed={} hand={} step={}: state divergence",
                    seed, hand_no, steps
                );
                eprintln!(
                    "  GS : pot={} stacks={:?} street_bets={:?} actor={}",
                    state.pot, state.stacks, state.street_bets, state.actor
                );
                eprintln!(
                    "  REF: pot={} stacks={:?} street_bets={:?} actor={}",
                    ref_state.pot, ref_state.stacks, ref_state.street_bets, ref_state.actor
                );
                break;
            }
        }

        if !mismatch && state.is_terminal() {
            hands_completed += 1;

            // Terminal payoff comparison, including showdowns.
            let gs_p0 = state.terminal_payoff(0, &evaluator);
            let ref_p0 = ref_terminal_payoff(&ref_state, 0, &hero, &villain, &evaluator);
            if (gs_p0 - ref_p0).abs() > 0.5 {
                mismatches += 1;
                eprintln!(
                    "MISMATCH seed={} hand={}: terminal payoff (p0)",
                    seed, hand_no
                );
                eprintln!("  GS  p0={:.4}", gs_p0);
                eprintln!("  REF p0={:.4}", ref_p0);
                eprintln!(
                    "  hero={:?} villain={:?} board={:?}",
                    hero, villain, ref_state.board
                );
                eprintln!(
                    "  GS folded={:?} REF folded={:?}",
                    state.folded, ref_state.folded
                );
            }
        }
    }

    FuzzingResult {
        hands_run,
        hands_completed,
        mismatches,
    }
}

// =============================================================================
// E3b: paired-seed A/B eval
// =============================================================================
//
// Design: for each hand, deal ONE shared deck and play it twice — once
// with `a` as hero, once with `b`. The two passes see identical cards
// AND identical bot decisions (the bots are deterministic). Hero
// decisions use a per-decision RNG seeded from the hand seed, so even
// when the two passes diverge they see the same "random draw at
// decision K". This is textbook common random numbers (CRN): the
// variance of the *difference* (profit_a - profit_b) collapses by an
// order of magnitude versus comparing two unpaired runs.
//
// Seat alternation (hero-as-BB) is deferred to E3c. For now hero is
// always seat 0 (SB preflop). This still gives the variance win on
// the paired diff, which is the primary purpose.

/// Per-opponent paired result.
#[derive(Debug, Clone)]
pub struct PairedOpponent {
    pub name: String,
    pub n_hands: u32,
    /// Mean hero profit (chips) with provider A as hero.
    pub mean_a: f64,
    /// Mean hero profit (chips) with provider B as hero.
    pub mean_b: f64,
    /// Mean of per-hand (profit_a - profit_b). The headline number.
    pub mean_diff: f64,
    /// Standard error of `mean_diff`. The paired CI is
    /// `mean_diff ± 1.96 * se_diff` at 95%.
    pub se_diff: f64,
}

/// Whole-run paired result.
#[derive(Debug, Clone)]
pub struct PairedResult {
    pub hands_per_opp: u32,
    pub seed: u64,
    pub opponents: Vec<PairedOpponent>,
}

impl PairedResult {
    /// Simple average of `mean_diff` across opponents, unweighted.
    /// All opponents currently play the same number of hands, so this
    /// is fine; if that changes, use a hand-weighted formula.
    pub fn overall_mean_diff(&self) -> f64 {
        if self.opponents.is_empty() {
            return 0.0;
        }
        self.opponents.iter().map(|o| o.mean_diff).sum::<f64>() / self.opponents.len() as f64
    }
}

/// Deal 7 unique cards from a Fisher-Yates shuffled deck.
fn deal_runout(rng: &mut rand::rngs::SmallRng) -> ([u8; 2], [u8; 2], [u8; 5]) {
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

/// Play one hand with `provider` as hero (seat 0) and `bot` as villain
/// (seat 1). Returns hero's chip profit.
///
/// `base_seed` drives per-decision RNG reseeding for common random
/// numbers: decision K uses `SmallRng::seed_from_u64(base_seed + K*const)`,
/// so both passes at position K see the same uniform [0,1) sample
/// regardless of how many hands were played before.
fn play_one_hand(
    provider: &dyn pkr_contracts::BlueprintProvider,
    abstraction: &dyn pkr_contracts::AbstractionBuilder,
    evaluator: &dyn pkr_contracts::Evaluator,
    bot: &dyn ScriptedBot,
    hero: [u8; 2],
    villain: [u8; 2],
    runout: &[u8; 5],
    base_seed: u64,
) -> f32 {
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    let mut state = GameState::new(200.0, 1.0, 2.0);
    state.set_hole_cards(hero, villain);
    let mut deck_idx = 0usize;
    let mut decision_idx: u64 = 0;
    let ctx = EvalContext {
        provider,
        abstraction,
        evaluator,
    };

    let mut steps = 0u32;
    while !state.is_terminal() && steps < 60 {
        steps += 1;
        let action = if state.actor == 0 {
            // Common random numbers: seed per-decision from base_seed.
            let decision_seed =
                base_seed.wrapping_add(decision_idx.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut dec_rng = SmallRng::seed_from_u64(decision_seed);
            decision_idx += 1;
            let (act, _used_bp) = decide_from_blueprint(&ctx, &state, &mut dec_rng);
            act
        } else {
            bot.act(&state)
        };
        state.apply_action_in_place(&action);

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

    if state.is_terminal() {
        state.terminal_payoff(0, evaluator)
    } else {
        0.0
    }
}

/// Paired-seed A/B comparison. See module comment for the design.
///
/// Prefer `eval_paired` over two calls to `run_eval_harness` whenever
/// the two providers are compared directly: paired SE is typically
/// 3-10x smaller at the same hand count.
pub fn eval_paired(
    a: &dyn pkr_contracts::BlueprintProvider,
    b: &dyn pkr_contracts::BlueprintProvider,
    abstraction: &dyn pkr_contracts::AbstractionBuilder,
    evaluator: &dyn pkr_contracts::Evaluator,
    num_hands: u32,
    base_seed: u64,
) -> PairedResult {
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    let bots: Vec<(&str, &dyn ScriptedBot)> = vec![
        ("station", &StationBot),
        ("nit", &NitBot),
        ("aggro", &AggroBot),
    ];

    let mut opponents = Vec::with_capacity(bots.len());
    for (name, bot) in &bots {
        let mut profits_a: Vec<f64> = Vec::with_capacity(num_hands as usize);
        let mut profits_b: Vec<f64> = Vec::with_capacity(num_hands as usize);

        for hand_no in 0..num_hands {
            let hand_seed = base_seed
                .wrapping_add(hand_no as u64)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(0x1);
            let mut deal_rng = SmallRng::seed_from_u64(hand_seed);
            let (hero, villain, runout) = deal_runout(&mut deal_rng);

            let pa = play_one_hand(
                a,
                abstraction,
                evaluator,
                *bot,
                hero,
                villain,
                &runout,
                hand_seed,
            );
            let pb = play_one_hand(
                b,
                abstraction,
                evaluator,
                *bot,
                hero,
                villain,
                &runout,
                hand_seed,
            );

            profits_a.push(pa as f64);
            profits_b.push(pb as f64);
        }

        let n = profits_a.len() as f64;
        if n == 0.0 {
            opponents.push(PairedOpponent {
                name: name.to_string(),
                n_hands: 0,
                mean_a: 0.0,
                mean_b: 0.0,
                mean_diff: 0.0,
                se_diff: 0.0,
            });
            continue;
        }

        let mean_a = profits_a.iter().sum::<f64>() / n;
        let mean_b = profits_b.iter().sum::<f64>() / n;
        let diffs: Vec<f64> = profits_a
            .iter()
            .zip(profits_b.iter())
            .map(|(x, y)| x - y)
            .collect();
        let mean_diff = diffs.iter().sum::<f64>() / n;
        let var = if n > 1.0 {
            diffs.iter().map(|d| (d - mean_diff).powi(2)).sum::<f64>() / (n - 1.0)
        } else {
            0.0
        };
        let se_diff = (var / n).sqrt();

        opponents.push(PairedOpponent {
            name: name.to_string(),
            n_hands: num_hands,
            mean_a,
            mean_b,
            mean_diff,
            se_diff,
        });
    }

    PairedResult {
        hands_per_opp: num_hands,
        seed: base_seed,
        opponents,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzz_small_batch() {
        // E2: after C1+C2+C1.5+C5b semantics, mismatches must be zero
        // on a seeded differential run. The old harness tolerated <50%
        // because it dealt board cards with replacement and never
        // compared showdown payoffs; both defects are fixed here.
        let result = run_fuzz_seeded(2000, 42);
        assert_eq!(result.hands_run, 2000);
        eprintln!(
            "Fuzz (seed=42): {}/{} hands completed, {} mismatches",
            result.hands_completed, result.hands_run, result.mismatches
        );
        assert_eq!(
            result.mismatches, 0,
            "differential fuzz found {} mismatches; see stderr for details",
            result.mismatches
        );
    }

    #[test]
    fn test_reference_state_basic() {
        let ref_state = ReferenceState::new(200.0, 1.0, 2.0);
        assert_eq!(ref_state.pot, 3.0);
        assert_eq!(ref_state.stacks, [199.0, 198.0]);
        assert_eq!(ref_state.actor, 0);
    }

    #[test]
    fn test_scripted_bots_basic() {
        let state = GameState::new(200.0, 1.0, 2.0);
        assert_eq!(state.bet_to_call(), 1.0);

        let station = StationBot;
        let action = station.act(&state);
        assert_eq!(action.player, 0);
        assert!(matches!(action.kind, ActionKind::Call));
    }
}

#[cfg(test)]
mod c2_aggro_tests {
    use super::*;

    /// C2: after AggroBot acts on a check, it must have 0 chips behind.
    #[test]
    fn aggro_jam_from_check_leaves_zero_chips() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Call,
        }); // SB limps
            // Now BB (actor 1) faces a check-equivalent situation.
        let bot = AggroBot;
        let act = bot.act(&s);
        s.apply_action_in_place(&act);
        assert_eq!(
            s.stacks[1], 0.0,
            "AggroBot check-jam left {} chips behind (act={:?})",
            s.stacks[1], act
        );
        assert_eq!(s.street_bets[1], 200.0);
    }
}

#[cfg(test)]
mod e3b_tests {
    use super::*;
    use pkr_contracts::{AbstractionBuilder, BlueprintProvider, Evaluator, SotaAdvice};

    /// Provider that always returns the same CDF for any hash.
    struct FixedProvider {
        cdf: [u8; 16],
        len: u8,
    }

    impl BlueprintProvider for FixedProvider {
        fn lookup(&self, _hash: u64) -> Option<SotaAdvice> {
            Some(SotaAdvice {
                cdf_probabilities: self.cdf,
                len: self.len,
            })
        }
    }

    /// Provider that always returns None (fallback path).
    struct MissingProvider;
    impl BlueprintProvider for MissingProvider {
        fn lookup(&self, _hash: u64) -> Option<SotaAdvice> {
            None
        }
    }

    /// Trivial abstraction: every state hashes to 0.
    struct NullAbstraction;
    impl AbstractionBuilder for NullAbstraction {
        fn get_infoset_hash(
            &self,
            _hole: &[u8],
            _board: &[u8],
            _history: &[u8],
            _street: u8,
        ) -> u64 {
            0
        }
    }

    /// Trivial evaluator: all hands tie.
    struct NullEval;
    impl Evaluator for NullEval {
        fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u32 {
            0
        }
    }

    fn cdf_call_heavy() -> FixedProvider {
        // Mostly call, some fold. Valid monotone CDF ending at 255.
        FixedProvider {
            cdf: [128, 200, 220, 240, 250, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            len: 6,
        }
    }

    fn cdf_fold_heavy() -> FixedProvider {
        // Mostly fold, some call.
        FixedProvider {
            cdf: [200, 240, 250, 253, 254, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            len: 6,
        }
    }

    /// A vs A must have zero mean diff and zero SE — the paired design
    /// guarantees this exactly (same strategy, same deals, same CRN).
    #[test]
    fn eval_paired_self_is_exactly_zero() {
        let a = cdf_call_heavy();
        let r = eval_paired(&a, &a, &NullAbstraction, &NullEval, 200, 1234);
        assert_eq!(r.opponents.len(), 3);
        for opp in &r.opponents {
            assert_eq!(
                opp.mean_diff, 0.0,
                "A vs A on {} must have zero mean diff, got {}",
                opp.name, opp.mean_diff
            );
            assert_eq!(
                opp.se_diff, 0.0,
                "A vs A on {} must have zero SE, got {}",
                opp.name, opp.se_diff
            );
        }
    }

    /// Two different providers should give a nonzero mean diff (call-heavy
    /// vs fold-heavy vs a calling bot). This is a directional check, not
    /// a precise value.
    #[test]
    fn eval_paired_distinguishes_providers() {
        let call_heavy = cdf_call_heavy();
        let fold_heavy = cdf_fold_heavy();
        // 500 hands gives enough signal.
        let r = eval_paired(
            &call_heavy,
            &fold_heavy,
            &NullAbstraction,
            &NullEval,
            500,
            777,
        );
        // At least one opponent must see a nonzero diff.
        let any_nonzero = r.opponents.iter().any(|o| o.mean_diff.abs() > 0.01);
        assert!(
            any_nonzero,
            "call-heavy vs fold-heavy should produce a nonzero diff on at least one bot"
        );
        // SE must be finite and non-negative.
        for opp in &r.opponents {
            assert!(opp.se_diff.is_finite());
            assert!(opp.se_diff >= 0.0);
        }
    }

    /// Paired SE must be strictly smaller than the naive unpaired SE
    /// on the same data. We can approximate unpaired SE as the SE of
    /// profit_a and profit_b treated independently; paired SE should
    /// be smaller because the shared deal dominates the variance.
    #[test]
    fn paired_se_is_tighter_than_independent() {
        use rand::rngs::SmallRng;
        use rand::SeedableRng;
        let call_heavy = cdf_call_heavy();
        let fold_heavy = cdf_fold_heavy();

        let n: u32 = 500;
        let base_seed: u64 = 2024;

        // Collect per-hand profits from both providers, same deals.
        let mut profits_a = Vec::with_capacity(n as usize);
        let mut profits_b = Vec::with_capacity(n as usize);
        for hand_no in 0..n {
            let hand_seed = base_seed
                .wrapping_add(hand_no as u64)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(0x1);
            let mut deal_rng = SmallRng::seed_from_u64(hand_seed);
            let (hero, villain, runout) = deal_runout(&mut deal_rng);
            // Use StationBot for both passes.
            let pa = play_one_hand(
                &call_heavy,
                &NullAbstraction,
                &NullEval,
                &StationBot,
                hero,
                villain,
                &runout,
                hand_seed,
            );
            let pb = play_one_hand(
                &fold_heavy,
                &NullAbstraction,
                &NullEval,
                &StationBot,
                hero,
                villain,
                &runout,
                hand_seed,
            );
            profits_a.push(pa as f64);
            profits_b.push(pb as f64);
        }
        let nf = n as f64;
        let ma = profits_a.iter().sum::<f64>() / nf;
        let mb = profits_b.iter().sum::<f64>() / nf;
        let va = profits_a.iter().map(|x| (x - ma).powi(2)).sum::<f64>() / (nf - 1.0);
        let vb = profits_b.iter().map(|x| (x - mb).powi(2)).sum::<f64>() / (nf - 1.0);
        // Unpaired SE of the difference assumes independence:
        let unpaired_se = (va / nf + vb / nf).sqrt();

        let diffs: Vec<f64> = profits_a
            .iter()
            .zip(profits_b.iter())
            .map(|(x, y)| x - y)
            .collect();
        let md = diffs.iter().sum::<f64>() / nf;
        let vd = diffs.iter().map(|d| (d - md).powi(2)).sum::<f64>() / (nf - 1.0);
        let paired_se = (vd / nf).sqrt();

        eprintln!(
            "paired_se={:.4}  unpaired_se={:.4}  ratio={:.2}",
            paired_se,
            unpaired_se,
            unpaired_se / paired_se.max(1e-9),
        );
        assert!(
            paired_se <= unpaired_se,
            "paired SE ({:.4}) should not exceed unpaired SE ({:.4})",
            paired_se,
            unpaired_se
        );
    }

    /// MissingProvider (fallback path) vs a call-heavy provider: the
    /// fallback folds vs bets, so call-heavy should win vs StationBot.
    /// Directional check only.
    #[test]
    fn eval_paired_missing_vs_call_heavy() {
        let call_heavy = cdf_call_heavy();
        let missing = MissingProvider;
        let r = eval_paired(&call_heavy, &missing, &NullAbstraction, &NullEval, 300, 99);
        // Call-heavy should beat fallback vs station (station never
        // bets, so call-heavy calls and sees showdowns; fallback
        // check-calls when free — actually both call when free, but
        // the CDF-based provider can fold vs station's checks too).
        // We only assert the result is a finite number here; the
        // direction depends on NullEval's all-ties behaviour.
        for opp in &r.opponents {
            assert!(opp.mean_diff.is_finite());
        }
    }
}

pub mod tournament;
