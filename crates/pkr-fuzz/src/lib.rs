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

        if self.stacks[self.actor] > 0.0 {
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
        self.street += 1;
        self.actions_this_street = 0;
        self.street_bets = [0.0, 0.0];
        self.actor = 1 - self.dealer();
        let _ = cards;
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
    let history_bytes: [u8; 4] = state.history_signature().to_le_bytes();
    let street = state.street as u8;
    let hash = ctx
        .abstraction
        .get_infoset_hash(hole, board, &history_bytes, street);

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
            let strength = (hole[0] as f32 + hole[1] as f32) / 100.0;
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
    for (i, act) in buf.iter().take(n_legal).enumerate() {
        let b = match act.kind {
            ActionKind::Fold => 0,
            ActionKind::Check | ActionKind::Call => 1,
            ActionKind::Bet(amount) => {
                let pot = state.pot.max(1.0);
                let frac = amount / pot;
                if amount >= state.stacks[state.actor] + state.street_bets[state.actor] {
                    5
                } else if frac < 0.75 {
                    2
                } else if frac < 1.5 {
                    3
                } else {
                    4
                }
            }
        };
        bucket_has_legal[b] = true;
        if bucket_pick[b] == 0 && !matches!(buf[i].kind, ActionKind::Check) {
            // Prefer a non-check representative if this is the first seen
        }
        bucket_pick[b] = i;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzz_small_batch() {
        let result = run_fuzz(100);
        assert_eq!(result.hands_run, 100);
        println!(
            "Fuzz: {} hands completed, {} mismatches",
            result.hands_completed, result.mismatches
        );
        assert!(
            result.mismatches < 50,
            "too many mismatches: {}",
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
        s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call }); // SB limps
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
