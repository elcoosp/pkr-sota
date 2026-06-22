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
    opp_hole: &[u8],
    board: &[u8],
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
        opp_hole,
        board,
        rng,
        iteration,
        player,
        &history,
        0,
        1.0,
    );
}

#[allow(clippy::too_many_arguments)]
fn traverse(
    rules: &dyn GameRules,
    table: &mut CompactRegretTable,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    hole: &[u8],
    opp_hole: &[u8],
    board: &[u8],
    rng: &mut impl Rng,
    iteration: u32,
    player: usize,
    history: &[u8],
    depth: usize,
    reach_prob: f32,
) -> f32 {
    let num_actions = rules.max_actions_per_node() as usize;
    if num_actions == 0 {
        return 0.0;
    }

    if history.len() >= 2 {
        let hero_rank = evaluator.evaluate_hand(hole, board);
        let opp_rank = evaluator.evaluate_hand(opp_hole, board);
        let payoff = if hero_rank < opp_rank {
            1.0
        } else if hero_rank == opp_rank {
            0.0
        } else {
            -1.0
        };
        return if player == 0 { payoff } else { -payoff };
    }

    let infoset_hash = abstraction.get_infoset_hash(hole, board, history);
    let strategy = table.get_strategy(infoset_hash);

    for a in 0..num_actions {
        table.add_strategy_sum(infoset_hash, a, strategy[a] * reach_prob);
    }

    let acting_player = depth % 2;
    if acting_player == player {
        let mut v = vec![0.0f32; num_actions];
        for a in 0..num_actions {
            let mut new_history = history.to_vec();
            new_history.push(a as u8);
            v[a] = traverse(
                rules,
                table,
                abstraction,
                evaluator,
                hole,
                opp_hole,
                board,
                rng,
                iteration,
                player,
                &new_history,
                depth + 1,
                reach_prob,
            );
        }

        let v_sigma: f32 = strategy.iter().zip(v.iter()).map(|(p, u)| p * u).sum();

        for a in 0..num_actions {
            let delta = v[a] - v_sigma;
            let current_regret = table.get_regret(infoset_hash, a);
            let is_positive = delta >= 0.0;
            let new_regret = dcfr::update_regret(current_regret, iteration, delta, is_positive);
            table.set_regret(infoset_hash, a, new_regret);
        }
        v_sigma
    } else {
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
            opp_hole,
            board,
            rng,
            iteration,
            player,
            &new_history,
            depth + 1,
            reach_prob * strategy[action as usize],
        )
    }
}
