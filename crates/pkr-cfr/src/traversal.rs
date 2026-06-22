use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::{ActionKind, GameState};
use rand::Rng;
use rand::distr::{Distribution, weighted::WeightedIndex};
use crate::dcfr;
use crate::table::CompactRegretTable;

/// External-sampling MCCFR traversal for a given player.
/// Recurses through the game tree, updating regrets and average strategy.
/// Returns the counterfactual value for the player at this state.
#[allow(clippy::too_many_arguments)]
pub fn traverse(
    state: &GameState,
    table: &mut CompactRegretTable,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    rng: &mut impl Rng,
    iteration: u32,
    traverser: usize,          // player for whom we compute value
    reach_prob: f32,           // reach probability of the traverser at this point
    opponent_reach: f32,       // reach probability of the opponent at this point (for avg strategy weight)
) -> f32 {
    if state.is_terminal() {
        return state.terminal_payoff(traverser, evaluator);
    }

    let acting_player = state.actor;
    let num_actions = state.legal_actions();
    if num_actions.is_empty() {
        // Should not happen if not terminal, but safe
        return 0.0;
    }

    // Map abstract action to index 0..K-1 (abstract buckets)
    // We need a fixed number of abstract actions K (e.g., 4: fold, check/call, bet, all-in)
    // For now we define a simple mapping: index 0 = fold, 1 = check/call, 2 = bet (first size), 3 = all-in
    // Actual bet sizes within the abstract bucket are handled later via action translation.
    // We'll just use 4 buckets for now.
    const K: usize = 4;

    // Get infoset hash
    let hole = &state.hole[acting_player]; // note: for opponent node, the hole is their hand
    let board = &state.board;
    let history_bytes: Vec<u8> = state.history.iter().flat_map(|a| {
        let b: u8 = match a.kind {
            ActionKind::Fold => 0,
            ActionKind::Check => 1,
            ActionKind::Call => 1, // merge check/call into abstract action 1
            ActionKind::Bet(_) => 2, // any bet/raise
        };
        Some(b)
    }).collect();
    let infoset_hash = abstraction.get_infoset_hash(hole, board, &history_bytes);

    // Get current strategy from regrets
    let strategy = table.get_strategy(infoset_hash);

    // Accumulate average strategy (weighted by reach probability)
    for a in 0..K {
        table.add_strategy_sum(infoset_hash, a, strategy[a] * opponent_reach);
    }

    if acting_player == traverser {
        // Hero node: compute counterfactual values for each action, then update regrets
        let mut v = vec![0.0f32; K];
        // Enumerate actions, mapping abstract action to actual action
        for (i, action) in num_actions.iter().enumerate() {
            let abstract_idx = abstract_action_index(&action.kind, K);
            // For now, we just use the first matching concrete action for each abstract index (if multiple bet sizes, pick first)
            // This is a simplification. A full implementation would consider all concrete actions within the bucket.
            let concrete = num_actions.iter().find(|a| abstract_action_index(&a.kind, K) == abstract_idx).unwrap();
            let next_state = state.apply_action(concrete);
            v[abstract_idx] = traverse(
                &next_state,
                table,
                abstraction,
                evaluator,
                rng,
                iteration,
                traverser,
                reach_prob,
                opponent_reach, // opponent reach stays same? Actually opponent reach is independent; we pass traverser reach.
            );
        }

        let v_sigma: f32 = strategy.iter().zip(v.iter()).map(|(p, u)| p * u).sum();

        for a in 0..K {
            let delta = v[a] - v_sigma;
            let current_regret = table.get_regret(infoset_hash, a);
            let is_positive = delta >= 0.0;
            let new_regret = dcfr::update_regret(current_regret, iteration, delta, is_positive);
            table.set_regret(infoset_hash, a, new_regret);
        }

        v_sigma
    } else {
        // Opponent node: sample one action from strategy
        let dist = WeightedIndex::new(&strategy).expect("strategy must have positive sum");
        let sampled_abstract = dist.sample(rng);
        // Find first concrete action matching the abstract bucket
        let concrete = num_actions.iter().find(|a| abstract_action_index(&a.kind, K) == sampled_abstract).unwrap();
        let next_state = state.apply_action(concrete);
        traverse(
            &next_state,
            table,
            abstraction,
            evaluator,
            rng,
            iteration,
            traverser,
            reach_prob,
            opponent_reach * strategy[sampled_abstract],
        )
    }
}

/// Map action kind to abstract bucket index (0..K)
fn abstract_action_index(kind: &ActionKind, _k: usize) -> usize {
    match kind {
        ActionKind::Fold => 0,
        ActionKind::Check | ActionKind::Call => {
            // Merge check/call into index 1
            1
        }
        ActionKind::Bet(_) => {
            // Bet/raise: map to index 2 or 3 (all-in). We'll just use 2 for now, all-in would be 3.
            // We need to distinguish all-in. For simplicity, we use 2 for any bet. All-in could be last abstract index.
            2
        }
    }
}
