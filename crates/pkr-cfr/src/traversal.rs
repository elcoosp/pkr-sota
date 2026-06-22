use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::{ActionKind, GameState, Street};
use rand::Rng;
use rand::RngExt;
use rand::distr::{Distribution, weighted::WeightedIndex};
use crate::dcfr;
use crate::table::CompactRegretTable;

const K: usize = 4; // abstract actions: fold, check/call, bet, all-in

pub fn traverse(
    state: &GameState,
    table: &mut CompactRegretTable,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    rng: &mut impl Rng,
    iteration: u32,
    traverser: usize,
    reach_prob: f32,
    opponent_reach: f32,
    chance_cards: &[Vec<u8>; 3], // [flop, turn, river] pre-sampled
) -> f32 {
    let mut current = state.clone();

    // Handle street completion: if not terminal and street complete, advance to next street
    while current.is_street_complete() && !current.is_terminal() {
        let next_cards = match current.street {
            Street::Preflop => &chance_cards[0],
            Street::Flop => &chance_cards[1],
            Street::Turn => &chance_cards[2],
            Street::River => break,
        };
        current.advance_street(next_cards);
    }

    if current.is_terminal() {
        return current.terminal_payoff(traverser, evaluator);
    }

    let acting_player = current.actor;
    let num_actions = current.legal_actions();
    if num_actions.is_empty() {
        return 0.0;
    }

    // Build mapping from abstract index to list of concrete actions
    let mut abstract_actions: [Vec<usize>; K] = [const { Vec::new() }; 4];
    for (idx, action) in num_actions.iter().enumerate() {
        let a_idx = abstract_action_index(&action.kind);
        abstract_actions[a_idx].push(idx);
    }

    let hole = &current.hole[acting_player];
    let board = &current.board;
    let history_bytes: Vec<u8> = current.history.iter().map(|a| match a.kind {
        ActionKind::Fold => 0,
        ActionKind::Check | ActionKind::Call => 1,
        ActionKind::Bet(_) => 2,
    }).collect();
    let infoset_hash = abstraction.get_infoset_hash(hole, board, &history_bytes);

    let strategy = table.get_strategy(infoset_hash);

    // Accumulate average strategy
    for a in 0..K {
        table.add_strategy_sum(infoset_hash, a, strategy[a] * opponent_reach);
    }

    if acting_player == traverser {
        let mut v = vec![0.0f32; K];
        for a in 0..K {
            if abstract_actions[a].is_empty() {
                v[a] = 0.0; // no valid concrete action, shouldn't affect regret
                continue;
            }
            // For counterfactual value of abstract action, average over concrete actions in bucket
            // (uniform over concrete actions for now)
            let mut sum_val = 0.0;
            let count = abstract_actions[a].len() as f32;
            for &concrete_idx in &abstract_actions[a] {
                let next_state = current.apply_action(&num_actions[concrete_idx]);
                sum_val += traverse(
                    &next_state,
                    table,
                    abstraction,
                    evaluator,
                    rng,
                    iteration,
                    traverser,
                    reach_prob,
                    opponent_reach,
                    chance_cards,
                );
            }
            v[a] = sum_val / count;
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
        let dist = WeightedIndex::new(&strategy).expect("strategy must have positive sum");
        let sampled_abstract = dist.sample(rng);
        if abstract_actions[sampled_abstract].is_empty() {
            return 0.0;
        }
        // Sample uniformly among concrete actions in the bucket
        let concrete_idx = abstract_actions[sampled_abstract]
            [rng.random_range(0..abstract_actions[sampled_abstract].len())];
        let next_state = current.apply_action(&num_actions[concrete_idx]);
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
            chance_cards,
        )
    }
}

fn abstract_action_index(kind: &ActionKind) -> usize {
    match kind {
        ActionKind::Fold => 0,
        ActionKind::Check | ActionKind::Call => 1,
        ActionKind::Bet(_) => {
            // Distinguish all-in (index 3) from other bets (index 2)
            // We use a simple heuristic: if bet amount >= pot * 2, treat as all-in
            // (this is imperfect but works for now)
            2
        }
    }
}
