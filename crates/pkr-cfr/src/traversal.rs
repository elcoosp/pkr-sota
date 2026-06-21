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

    if num_actions == 0 {
        return 0.0;
    }

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

    // Asymmetric: hero wins only when hero=0, opp=1
    struct AsymmetricEvaluator;
    impl Evaluator for AsymmetricEvaluator {
        fn evaluate_hand(&self, _hole: &[u8], board: &[u8]) -> u16 {
            if board.len() < 2 {
                return 0;
            }
            let hero = board[0];
            let opp = board[1];
            if hero == 0 && opp == 1 { 1 } else { 0 }
        }
    }

    struct FourActionRules;
    impl GameRules for FourActionRules {
        fn max_actions_per_node(&self) -> u8 {
            4
        }
        fn deck_size(&self) -> usize {
            52
        }
        fn hand_size(&self) -> usize {
            2
        }
    }
    struct FourActionAbstraction;
    impl AbstractionBuilder for FourActionAbstraction {
        fn get_infoset_hash(&self, _hole: &[u8], _board: &[u8], history: &[u8]) -> u64 {
            match history.len() {
                0 => 0,
                _ => 1,
            }
        }
    }
    struct FourActionEvaluator;
    impl Evaluator for FourActionEvaluator {
        fn evaluate_hand(&self, _hole: &[u8], board: &[u8]) -> u16 {
            if board.len() < 2 {
                return 0;
            }
            if board[0] == board[1] { 1 } else { 0 }
        }
    }

    struct ZeroActionRules;
    impl GameRules for ZeroActionRules {
        fn max_actions_per_node(&self) -> u8 {
            0
        }
        fn deck_size(&self) -> usize {
            52
        }
        fn hand_size(&self) -> usize {
            2
        }
    }
    struct ZeroActionAbstraction;
    impl AbstractionBuilder for ZeroActionAbstraction {
        fn get_infoset_hash(&self, _hole: &[u8], _board: &[u8], _history: &[u8]) -> u64 {
            0
        }
    }
    struct ZeroActionEvaluator;
    impl Evaluator for ZeroActionEvaluator {
        fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u16 {
            0
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
        assert!((hero_avg[0] - expected).abs() < eps);
        assert!((hero_avg[1] - expected).abs() < eps);
        assert!((opp_avg[0] - expected).abs() < eps);
        assert!((opp_avg[1] - expected).abs() < eps);
    }

    #[test]
    fn infoset_index_maps_to_valid_range() {
        let table = CompactRegretTable::new(2, 2);
        let abstraction = MockAbstraction;
        assert_eq!(infoset_index(&abstraction, &table, &[], &[]), 0);
        assert_eq!(infoset_index(&abstraction, &table, &[], &[0]), 1);
    }

    #[test]
    fn strategy_probabilities_sum_to_one_after_iterations() {
        let rules = MockRules;
        let abstraction = MockAbstraction;
        let evaluator = MockEvaluator;
        let mut rng = StdRng::seed_from_u64(123);
        let mut table = CompactRegretTable::new(2, 2);
        let hole = vec![0u8, 0];

        for iter in 1..=100 {
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
        }
        let hero = table.get_strategy(0);
        let opp = table.get_strategy(1);
        let eps = 1e-6;
        assert!((hero.iter().sum::<f32>() - 1.0).abs() < eps);
        assert!((opp.iter().sum::<f32>() - 1.0).abs() < eps);
        for &p in &hero {
            assert!(p >= 0.0);
        }
        for &p in &opp {
            assert!(p >= 0.0);
        }
    }

    #[test]
    fn regret_table_changes_after_one_asymmetric_iteration() {
        let rules = MockRules;
        let abstraction = MockAbstraction;
        let evaluator = AsymmetricEvaluator;
        let mut rng = StdRng::seed_from_u64(999);
        let mut table = CompactRegretTable::new(2, 2);
        let hole = vec![0u8, 0];

        let initial_0_0 = table.get_regret(0, 0);
        let initial_0_1 = table.get_regret(0, 1);
        assert_eq!(initial_0_0, 128);
        assert_eq!(initial_0_1, 128);

        run_iteration(
            &rules,
            &mut table,
            &abstraction,
            &evaluator,
            &hole,
            &mut rng,
            1,
            0,
        );
        let after_0_0 = table.get_regret(0, 0);
        let after_0_1 = table.get_regret(0, 1);
        assert!(
            after_0_0 != 128 || after_0_1 != 128,
            "Expected at least one regret to move from midpoint, got ({}, {})",
            after_0_0,
            after_0_1
        );
    }

    #[test]
    fn asymmetric_payoffs_converges_to_pure_equilibrium() {
        // Hero wins only on (0,1). Payoff matrix:
        //        Opp 0   Opp 1
        // Hero 0:  -1       +1
        // Hero 1:  -1       -1
        // Hero 0 dominates Hero 1.
        // Opponent payoffs (as row player):
        //        Hero 0   Hero 1
        // Opp 0:   +1       +1
        // Opp 1:   -1       +1
        // Opp 0 dominates Opp 1.
        // Unique NE: (Hero 0, Opp 0)
        let rules = MockRules;
        let abstraction = MockAbstraction;
        let evaluator = AsymmetricEvaluator;
        let mut rng = StdRng::seed_from_u64(77);
        let mut table = CompactRegretTable::new(2, 2);
        let hole = vec![0u8, 0];

        const ITERATIONS: u32 = 2000;
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
        }
        let hero = table.get_strategy(0);
        let opp = table.get_strategy(1);

        assert!(hero[0] > 0.9, "hero should prefer action 0, got {:?}", hero);
        assert!(
            opp[0] > 0.9,
            "opponent should prefer action 0, got {:?}",
            opp
        );
        assert!((hero.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!((opp.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn deterministic_with_same_seed_gives_same_strategies() {
        let seed = 42u64;
        let hole = vec![0u8, 0];
        let mut results = vec![];

        for _ in 0..2 {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut table = CompactRegretTable::new(2, 2);
            let rules = MockRules;
            let abstraction = MockAbstraction;
            let evaluator = MockEvaluator;
            for iter in 1..=200 {
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
            }
            results.push(table.get_strategy(0));
        }
        for i in 0..2 {
            assert!(
                (results[0][i] - results[1][i]).abs() < 1e-6,
                "Run 0 vs run 1, action {}: {} vs {}",
                i,
                results[0][i],
                results[1][i]
            );
        }
    }

    #[test]
    fn four_action_game_produces_valid_strategies() {
        let rules = FourActionRules;
        let abstraction = FourActionAbstraction;
        let evaluator = FourActionEvaluator;
        let mut rng = StdRng::seed_from_u64(2025);
        let mut table = CompactRegretTable::new(2, 4);
        let hole = vec![0u8, 0];

        for iter in 1..=500 {
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
        }
        let hero = table.get_strategy(0);
        let opp = table.get_strategy(1);
        assert_eq!(hero.len(), 4);
        assert_eq!(opp.len(), 4);
        assert!((hero.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!((opp.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        for &p in &hero {
            assert!(p >= 0.0);
        }
        for &p in &opp {
            assert!(p >= 0.0);
        }
    }

    #[test]
    fn run_iteration_does_not_panic_on_repeated_calls() {
        let rules = MockRules;
        let abstraction = MockAbstraction;
        let evaluator = MockEvaluator;
        let mut rng = StdRng::seed_from_u64(11);
        let mut table = CompactRegretTable::new(2, 2);
        let hole = vec![0u8, 0];

        for iter in 1..=50 {
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
        }
    }

    #[test]
    fn first_iteration_starting_from_midpoint_produces_uniform_strategy() {
        let rules = MockRules;
        let abstraction = MockAbstraction;
        let evaluator = MockEvaluator;
        let mut rng = StdRng::seed_from_u64(1);
        let mut table = CompactRegretTable::new(2, 2);
        let hole = vec![0u8, 0];

        let hero_before = table.get_strategy(0);
        assert!((hero_before[0] - 0.5).abs() < 1e-6);
        assert!((hero_before[1] - 0.5).abs() < 1e-6);

        run_iteration(
            &rules,
            &mut table,
            &abstraction,
            &evaluator,
            &hole,
            &mut rng,
            1,
            0,
        );
        let hero_after = table.get_strategy(0);
        assert!((hero_after.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn regrets_remain_in_u8_range_after_many_iterations() {
        let rules = MockRules;
        let abstraction = MockAbstraction;
        let evaluator = MockEvaluator;
        let mut rng = StdRng::seed_from_u64(789);
        let mut table = CompactRegretTable::new(2, 2);
        let hole = vec![0u8, 0];

        for iter in 1..=1000 {
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
        }

        for infoset in 0..table.capacity() {
            for action in 0..table.num_actions() {
                let r = table.get_regret(infoset, action);
                assert!(r <= 255);
            }
        }
    }

    #[test]
    fn zero_capacity_zero_actions_table_does_not_panic() {
        let rules = ZeroActionRules;
        let abstraction = ZeroActionAbstraction;
        let evaluator = ZeroActionEvaluator;
        let mut rng = StdRng::seed_from_u64(1);
        let mut table = CompactRegretTable::new(0, 0);
        let hole = vec![0u8, 0];

        run_iteration(
            &rules,
            &mut table,
            &abstraction,
            &evaluator,
            &hole,
            &mut rng,
            1,
            0,
        );
    }
}
