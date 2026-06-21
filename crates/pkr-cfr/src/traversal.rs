use pkr_contracts::{GameRules, AbstractionBuilder, Evaluator};
use rand::Rng;
use rand::distr::weighted::WeightedIndex;
use rand::distr::Distribution;

use crate::dcfr;
use crate::table::CompactRegretTable;

/// External sampling MCCFR iteration.
pub fn run_iteration(
    rules: &dyn GameRules,
    table: &mut CompactRegretTable,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    hole: &[u8],
    rng: &mut impl Rng,
    iteration: u32,
    player: usize,
) {
    let history: Vec<u8> = Vec::new();
    traverse(
        rules,
        table,
        abstraction,
        evaluator,
        hole,
        rng,
        iteration,
        player,
        &history,
        0,
    );
}

/// Recursive external‑sampling traversal.
/// Returns the counterfactual value for the traversing player (`player`).
fn traverse(
    rules: &dyn GameRules,
    table: &mut CompactRegretTable,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    hole: &[u8],
    rng: &mut impl Rng,
    iteration: u32,
    player: usize,
    history: &[u8],
    depth: usize,
) -> f32 {
    // Terminal test: in our mock game history length = 2 means end of game.
    if history.len() >= 2 {
        let board = history;
        let raw = evaluator.evaluate_hand(hole, board) as f32;
        if raw == 1.0 {
            if player == 0 { 1.0 } else { -1.0 }
        } else {
            if player == 0 { -1.0 } else { 1.0 }
        }
    } else {
        let acting_player = depth % 2;
        let infoset_hash = abstraction.get_infoset_hash(hole, &[], history);
        let infoset_idx = (infoset_hash as usize) % (table.capacity().max(1));

        let num_actions = rules.max_actions_per_node() as usize;

        if acting_player == player {
            // Traversing player's decision node: evaluate all actions
            let strategy = table.get_strategy(infoset_idx);

            // Compute utility for each action by recursing
            let mut utilities = Vec::with_capacity(num_actions);
            for a in 0..num_actions {
                let mut new_history = history.to_vec();
                new_history.push(a as u8);
                let u = traverse(
                    rules,
                    table,
                    abstraction,
                    evaluator,
                    hole,
                    rng,
                    iteration,
                    player,
                    &new_history,
                    depth + 1,
                );
                utilities.push(u);
            }

            // Expected utility under current strategy
            let v_sigma: f32 = strategy
                .iter()
                .zip(utilities.iter())
                .map(|(p, u)| p * u)
                .sum();

            // Update regrets for all actions
            for a in 0..num_actions {
                let delta = utilities[a] - v_sigma;
                let current_regret = table.get_regret(infoset_idx, a);
                let is_positive = delta >= 0.0;
                let new_regret = dcfr::update_regret(current_regret, iteration, delta, is_positive);
                let delta_i32 = new_regret as i32 - current_regret as i32;
                table.add_regret(infoset_idx, a, delta_i32);
            }

            v_sigma
        } else {
            // Opponent's node: sample one action according to its strategy
            let strategy = table.get_strategy(infoset_idx);
            let dist = WeightedIndex::new(&strategy)
                .expect("strategy probabilities must have positive sum");
            let action = dist.sample(rng) as u8;

            let mut new_history = history.to_vec();
            new_history.push(action);

            traverse(
                rules,
                table,
                abstraction,
                evaluator,
                hole,
                rng,
                iteration,
                player,
                &new_history,
                depth + 1,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_contracts::{AbstractionBuilder, Evaluator, GameRules};
    use rand::rngs::StdRng;
    use rand::SeedableRng;
    use crate::table::CompactRegretTable;

    struct MockRules;
    impl GameRules for MockRules {
        fn max_actions_per_node(&self) -> u8 { 2 }
        fn deck_size(&self) -> usize { 52 }
        fn hand_size(&self) -> usize { 2 }
    }

    struct MockAbstraction;
    impl AbstractionBuilder for MockAbstraction {
        fn get_infoset_hash(&self, _hole: &[u8], _board: &[u8], history: &[u8]) -> u64 {
            match history.len() {
                0 => 0,
                _ => 1,
            }
        }
    }

    struct MockEvaluator;
    impl Evaluator for MockEvaluator {
        fn evaluate_hand(&self, _hole: &[u8], board: &[u8]) -> u16 {
            if board.len() < 2 {
                return 0;
            }
            let hero_act = board[0];
            let opp_act = board[1];
            if hero_act == opp_act { 1 } else { 0 }
        }
    }

    #[test]
    fn matching_pennies_converges_to_uniform() {
        let rules = MockRules;
        let abstraction = MockAbstraction;
        let evaluator = MockEvaluator;
        let mut rng = StdRng::seed_from_u64(42);
        let mut table = CompactRegretTable::new(2, 2);
        let hole = vec![0u8, 0];

        const ITERATIONS: u32 = 2000;
        for iter in 1..=ITERATIONS {
            run_iteration(&rules, &mut table, &abstraction, &evaluator, &hole, &mut rng, iter, 0);
            run_iteration(&rules, &mut table, &abstraction, &evaluator, &hole, &mut rng, iter, 1);
        }

        let hero_strat = table.get_strategy(0);
        let opp_strat  = table.get_strategy(1);
        let expected = 0.5;
        assert!((hero_strat[0] - expected).abs() < 0.05,
            "hero strategy not uniform: {:?}", hero_strat);
        assert!((hero_strat[1] - expected).abs() < 0.05,
            "hero strategy not uniform: {:?}", hero_strat);
        assert!((opp_strat[0] - expected).abs() < 0.05,
            "opp strategy not uniform: {:?}", opp_strat);
        assert!((opp_strat[1] - expected).abs() < 0.05,
            "opp strategy not uniform: {:?}", opp_strat);
    }
}
