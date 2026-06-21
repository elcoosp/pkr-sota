use pkr_contracts::{AbstractionBuilder, Evaluator, GameRules};
use rand::Rng;
use rand::distr::Distribution;
use rand::distr::weighted::WeightedIndex;

use crate::dcfr;
use crate::table::CompactRegretTable;

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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
    let num_actions = rules.max_actions_per_node() as usize;

    // Terminal: after 2 actions
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
        if acting_player == player {
            // Traversing player's node: evaluate all actions
            let infoset_idx = infoset_index(abstraction, table, hole, history);
            let mut v = vec![0.0f32; num_actions];
            for (a, val) in v.iter_mut().enumerate() {
                let mut new_history = history.to_vec();
                new_history.push(a as u8);
                *val = traverse(
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
            }

            let strategy = table.get_strategy(infoset_idx);
            let v_sigma: f32 = strategy.iter().zip(v.iter()).map(|(p, u)| p * u).sum();

            for (a, &val) in v.iter().enumerate() {
                let delta = val - v_sigma;
                let current_regret = table.get_regret(infoset_idx, a);
                let is_positive = delta >= 0.0;
                let new_regret = dcfr::update_regret(current_regret, iteration, delta, is_positive);
                let delta_i32 = new_regret as i32 - current_regret as i32;
                table.add_regret(infoset_idx, a, delta_i32);
            }

            v_sigma
        } else {
            // Opponent's node: sample one action according to its strategy
            let infoset_idx = infoset_index(abstraction, table, hole, history);
            let strategy = table.get_strategy(infoset_idx);
            let dist = WeightedIndex::new(&strategy).expect("strategy must have positive sum");
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

fn infoset_index(
    abstraction: &dyn AbstractionBuilder,
    table: &CompactRegretTable,
    hole: &[u8],
    history: &[u8],
) -> usize {
    let hash = abstraction.get_infoset_hash(hole, &[], history);
    (hash as usize) % (table.capacity().max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::CompactRegretTable;
    use pkr_contracts::{AbstractionBuilder, Evaluator, GameRules};
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    struct MockRules;
    impl GameRules for MockRules {
        fn max_actions_per_node(&self) -> u8 {
            2
        }
        fn deck_size(&self) -> usize {
            52
        }
        fn hand_size(&self) -> usize {
            2
        }
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

        let mut hero_avg = vec![0.0f64; 2];
        let mut opp_avg = vec![0.0f64; 2];
        const ITERATIONS: u32 = 5000;
        const WARMUP: u32 = 500;

        for iter in 1..=ITERATIONS {
            run_iteration(
                &rules,
                &mut table,
                &abstraction,
                &evaluator,
                &hole,
                &mut rng,
                iter,
                0,
            );
            run_iteration(
                &rules,
                &mut table,
                &abstraction,
                &evaluator,
                &hole,
                &mut rng,
                iter,
                1,
            );

            if iter > WARMUP {
                let hero = table.get_strategy(0);
                let opp = table.get_strategy(1);
                for i in 0..2 {
                    hero_avg[i] += hero[i] as f64;
                    opp_avg[i] += opp[i] as f64;
                }
            }
        }

        let n = (ITERATIONS - WARMUP) as f64;
        for i in 0..2 {
            hero_avg[i] /= n;
            opp_avg[i] /= n;
        }

        let expected = 0.5;
        let eps = 0.05;
        assert!(
            (hero_avg[0] - expected).abs() < eps,
            "hero avg strategy not uniform: {:?}",
            hero_avg
        );
        assert!(
            (hero_avg[1] - expected).abs() < eps,
            "hero avg strategy not uniform: {:?}",
            hero_avg
        );
        assert!(
            (opp_avg[0] - expected).abs() < eps,
            "opp avg strategy not uniform: {:?}",
            opp_avg
        );
        assert!(
            (opp_avg[1] - expected).abs() < eps,
            "opp avg strategy not uniform: {:?}",
            opp_avg
        );
    }
}
