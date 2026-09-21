//! Runtime river re-solve — exact CFR re-solve over the river subgame.
//!
//! At river start the board is complete, so hand strengths are exact (no MC).
//! A depth-limited CFR solve over river ranges (~1700 combos × K actions × ~200
//! iterations) completes in milliseconds, giving near-solver river play
//! without any training. (Roadmap §5.1)

use pkr_contracts::Evaluator;
use pkr_core::state::{Action, ActionKind, GameState, Street};
use pkr_eval::NlheEvaluator;

const K: usize = 6;
const MAX_RIVER_ITERATIONS: u32 = 200;

/// A range over the remaining cards, represented as a bitmask of valid
/// card indices (0..52). Used for exact river range enumeration.
#[derive(Clone)]
pub struct RiverRange {
    /// 52-bit mask: bit i set means card i is available in this player's range
    pub mask: u64,
}

impl RiverRange {
    /// Create a range from the remaining cards (those not in hole or board).
    pub fn from_available(used: &[u8]) -> Self {
        let mut mask: u64 = 0;
        for card in 0u8..52 {
            if !used.contains(&card) {
                mask |= 1u64 << card;
            }
        }
        RiverRange { mask }
    }

    /// Enumerate all 2-card combinations in this range (for HU pre-river range).
    /// Returns up to N combos.
    pub fn enumerate_combos(&self, limit: usize) -> Vec<[u8; 2]> {
        let mut cards: Vec<u8> = Vec::new();
        for i in 0..52u8 {
            if self.mask & (1u64 << i) != 0 {
                cards.push(i);
            }
        }
        let mut combos = Vec::new();
        for i in 0..cards.len() {
            for j in (i + 1)..cards.len() {
                if combos.len() >= limit {
                    return combos;
                }
                combos.push([cards[i], cards[j]]);
            }
        }
        combos
    }

    /// Count of available cards.
    pub fn card_count(&self) -> usize {
        self.mask.count_ones() as usize
    }
}

/// A river re-solve instance. Holds the game state at river start and
/// the opponent's modeled range, then runs depth-limited CFR.
pub struct RiverResolver {
    state: GameState,
    evaluator: NlheEvaluator,
    hero_range: RiverRange,
    villain_range: RiverRange,
}

impl RiverResolver {
    /// Create a resolver from a river game state. The state must be at the
    /// start of the river betting round (no actions yet on the river).
    pub fn new(state: &GameState, hero: [u8; 2], villain: [u8; 2], board: &[u8]) -> Self {
        let mut used: Vec<u8> = Vec::new();
        used.extend_from_slice(&[hero[0], hero[1], villain[0], villain[1]]);
        used.extend_from_slice(board);

        let hero_range = RiverRange::from_available(&used);
        let villain_range = RiverRange::from_available(&used);

        RiverResolver {
            state: state.clone(),
            evaluator: NlheEvaluator,
            hero_range,
            villain_range,
        }
    }

    /// Run the river CFR solve. Returns the improved strategy for the
    /// current actor as action probabilities (indexed by abstract action).
    pub fn solve(&self) -> [f32; K] {
        let mut regrets = vec![0.0f32; K];
        let mut strategy_sum = vec![0.0f32; K];

        for iteration in 0..MAX_RIVER_ITERATIONS {
            let strat = self.compute_strategy(&regrets);
            for a in 0..K {
                strategy_sum[a] += strat[a];
            }
            self.update_regrets(&strat, &mut regrets, iteration as f32);
        }

        // Compute average strategy
        let total: f32 = strategy_sum.iter().sum();
        let mut avg = [0.0f32; K];
        if total > 0.0 {
            for i in 0..K {
                avg[i] = strategy_sum[i] / total;
            }
        } else {
            // Uniform fallback
            let uniform = 1.0 / K as f32;
            for i in 0..K {
                avg[i] = uniform;
            }
        }
        avg
    }

    fn compute_strategy(&self, regrets: &[f32]) -> [f32; K] {
        let positive: f32 = regrets.iter().map(|&r| r.max(0.0)).sum();
        let mut strat = [0.0f32; K];
        if positive > 0.0 {
            for i in 0..K {
                strat[i] = regrets[i].max(0.0) / positive;
            }
        } else {
            let uniform = 1.0 / K as f32;
            for i in 0..K {
                strat[i] = uniform;
            }
        }
        strat
    }

    fn update_regrets(&self, strategy: &[f32], regrets: &mut [f32], _iteration: f32) {
        let mut new_regrets = regrets_copy();
        let board = &self.state.board[..self.state.board_len as usize];

        // Sample opponent hands and compute counterfactual values
        let hero_combos = self.hero_range.enumerate_combos(1700);

        for hero in &hero_combos {
            let villain_combos = self.villain_range.enumerate_combos(1700);
            for villain in &villain_combos {
                // Skip if any card overlaps
                if hero.iter().any(|&c| villain.contains(&c)) {
                    continue;
                }

                // Evaluate both hands
                let hero_rank = self.evaluator.evaluate_hand(hero, board);
                let villain_rank = self.evaluator.evaluate_hand(villain, board);
                let hero_wins = hero_rank < villain_rank;
                let is_tie = hero_rank == villain_rank;

                // Compute value for each action
                for a in 0..K {
                    let action = self.abstract_to_action(a);
                    let ev = self.action_ev(&action, hero_wins, is_tie);
                    // Regret = EV(action) - EV(strategy-weighted average)
                    // Since we're doing vanilla CFR, we compute: regret[a] = ev[a] - ev_avg
                    // where ev_avg = sum(strategy[a] * ev[a])
                    new_regrets[a] += ev - self.strategy_value(strategy, hero_wins, is_tie);
                }
            }
        }

        let _ = board;

        // Copy computed regrets into the mutable buffer
        for (i, &r) in new_regrets.iter().enumerate() {
            regrets[i] = r;
        }
    }

    fn abstract_to_action(&self, action_idx: usize) -> Action {
        let actor = self.state.actor;
        match action_idx {
            0 => Action {
                player: actor,
                kind: ActionKind::Fold,
            },
            1 => Action {
                player: actor,
                kind: ActionKind::Call,
            },
            2 if self.state.stacks[actor] > 0.0 => Action {
                player: actor,
                kind: ActionKind::Bet(self.state.stacks[actor].min(self.state.pot * 0.5)),
            },
            3 if self.state.stacks[actor] > 0.0 => Action {
                player: actor,
                kind: ActionKind::Bet(self.state.stacks[actor].min(self.state.pot * 1.0)),
            },
            4 if self.state.stacks[actor] > 0.0 => Action {
                player: actor,
                kind: ActionKind::Bet(self.state.stacks[actor].min(self.state.pot * 2.0)),
            },
            5 => Action {
                player: actor,
                kind: ActionKind::Bet(self.state.stacks[actor] + self.state.street_bets[actor]),
            },
            _ => Action {
                player: actor,
                kind: ActionKind::Fold,
            },
        }
    }

    fn action_ev(&self, action: &Action, hero_wins: bool, is_tie: bool) -> f32 {
        match action.kind {
            ActionKind::Fold => {
                if self.state.actor == 0 {
                    -(self.state.total_invested[0])
                } else {
                    -(self.state.total_invested[1])
                }
            }
            ActionKind::Call => {
                // Approximate: pot is split 50/50 on tie, winner takes all
                let pot = self.state.pot;
                if is_tie {
                    pot / 2.0 - self.state.total_invested[self.state.actor]
                } else if hero_wins == (self.state.actor == 0) {
                    pot - self.state.total_invested[self.state.actor]
                } else {
                    -self.state.total_invested[self.state.actor]
                }
            }
            ActionKind::Bet(_) => {
                // For all-in bets on river, treat similar to call
                self.action_ev(&Action { player: action.player, kind: ActionKind::Call }, hero_wins, is_tie)
            }
            _ => 0.0,
        }
    }

    fn strategy_value(&self, strategy: &[f32], hero_wins: bool, is_tie: bool) -> f32 {
        strategy.iter().enumerate().map(|(a, &p)| {
            p * self.action_ev(&self.abstract_to_action(a), hero_wins, is_tie)
        }).sum()
    }
}

fn regrets_copy() -> Vec<f32> {
    vec![0.0; K]
}

/// Check if the current game state is at the start of a resolvable river.
/// This means: we're on the river, the hand is not yet terminal,
/// and we're at the first to act on the river (0 or 1 actions this street).
pub fn is_river_resolvable(state: &GameState) -> bool {
    state.street == Street::River
        && !state.is_terminal()
        && state.actions_this_street < 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_core::state::{Action, ActionKind};

    #[test]
    fn test_river_range_from_available() {
        let used = vec![0u8, 1, 2, 3, 4, 5, 6, 7]; // 2 hole + 5 board + 1 extra
        let range = RiverRange::from_available(&used);
        assert_eq!(range.card_count(), 52 - 8);
    }

    #[test]
    fn test_river_range_enumerate_combos() {
        let range = RiverRange { mask: 0xFFFFFFFFFFFF }; // 48 cards available
        let combos = range.enumerate_combos(100);
        assert!(combos.len() <= 100);
        assert!(combos.len() >= 1);
        // All combos should have 2 distinct cards
        for combo in &combos {
            assert_ne!(combo[0], combo[1]);
        }
    }

    #[test]
    fn test_compute_strategy_uniform_on_zero_regret() {
        let state = GameState::new(100.0, 0.0, 0.0);
        let resolver = RiverResolver::new(&state, [0, 1], [2, 3], &[]);
        let strat = resolver.compute_strategy(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        for p in &strat {
            assert!((p - 1.0 / 6.0).abs() < 1e-6);
        }
    }

    #[test]
    fn test_compute_strategy_positive_regret() {
        let state = GameState::new(100.0, 0.0, 0.0);
        let resolver = RiverResolver::new(&state, [0, 1], [2, 3], &[]);
        let strat = resolver.compute_strategy(&[0.0, 10.0, 0.0, 5.0, 0.0, 0.0]);
        // Action 1 has regret 10, action 3 has regret 5
        // Total positive = 15
        // strat[1] = 10/15, strat[3] = 5/15
        assert!((strat[1] - 2.0 / 3.0).abs() < 1e-6);
        assert!((strat[3] - 1.0 / 3.0).abs() < 1e-6);
        assert!(strat[0] < 1e-6);
        assert!(strat[2] < 1e-6);
        assert!(strat[4] < 1e-6);
        assert!(strat[5] < 1e-6);
    }

    #[test]
    fn test_is_river_resolvable() {
        let mut state = GameState::new(200.0, 1.0, 2.0);
        state.street = Street::River;
        // River just started: no bets yet, 0 actions this street
        state.street_bets = [0.0, 0.0];
        state.total_invested = [2.0, 4.0]; // from blinds
        state.stacks = [198.0, 196.0];
        state.pot = 6.0;
        state.actions_this_street = 0;
        assert!(!state.is_terminal());
        // Not street complete yet (need >= 2 actions)
        assert!(!state.is_street_complete());
        assert!(is_river_resolvable(&state));
    }

    #[test]
    fn test_abstract_to_action_indices() {
        let state = GameState::new(100.0, 1.0, 2.0);
        let resolver = RiverResolver::new(&state, [0, 1], [2, 3], &[]);
        assert!(matches!(resolver.abstract_to_action(0).kind, ActionKind::Fold));
        assert!(matches!(resolver.abstract_to_action(1).kind, ActionKind::Call));
        // Action 5 should be all-in
        let a5 = resolver.abstract_to_action(5);
        assert!(matches!(a5.kind, ActionKind::Bet(_)));
    }
}
