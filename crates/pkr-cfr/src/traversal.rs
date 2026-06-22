use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::{ActionKind, GameState, Street};
use rand::Rng;
use rand::RngExt;
use rand::distr::{Distribution, weighted::WeightedIndex};
use crate::dcfr;
use crate::table::CompactRegretTable;

const K: usize = 4;

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
    deck: &mut Vec<u8>,               // remaining cards in shuffled order
) -> f32 {
    let mut current = state.clone();

    // Advance streets by dealing from deck when street complete and not terminal
    while current.is_street_complete() && !current.is_terminal() {
        let cards_needed = match current.street {
            Street::Preflop => 3,   // flop
            Street::Flop => 1,      // turn
            Street::Turn => 1,      // river
            Street::River => break,
        };
        let new_cards: Vec<u8> = deck.drain(..cards_needed).collect();
        current.advance_street(&new_cards);
    }

    if current.is_terminal() {
        return current.terminal_payoff(traverser, evaluator);
    }

    let acting_player = current.actor;
    let num_actions = current.legal_actions();
    if num_actions.is_empty() {
        return 0.0;
    }

    let mut abstract_actions: [Vec<usize>; K] = [const { Vec::new() }; 4];
    for (idx, action) in num_actions.iter().enumerate() {
        let a_idx = abstract_action_index(&action.kind);
        if a_idx < K {
            abstract_actions[a_idx].push(idx);
        }
    }

    let hole = &current.hole[acting_player];
    let board = &current.board;
    let history_bytes: Vec<u8> = current.history.iter().map(|a| match a.kind {
        ActionKind::Fold => 0,
        ActionKind::Check | ActionKind::Call => 1,
        ActionKind::Bet(_) => 2,
    }).collect();
    let street_code = current.street as u8;
    let infoset_hash = abstraction.get_infoset_hash(hole, board, &history_bytes, street_code);

    let strategy = table.get_strategy(infoset_hash);
    for a in 0..K {
        table.add_strategy_sum(infoset_hash, a, strategy[a] * opponent_reach);
    }

    if acting_player == traverser {
        let mut v = vec![0.0f32; K];
        for a in 0..K {
            if abstract_actions[a].is_empty() {
                v[a] = 0.0;
                continue;
            }
            let mut sum_val = 0.0;
            let count = abstract_actions[a].len() as f32;
            for &concrete_idx in &abstract_actions[a] {
                let next_state = current.apply_action(&num_actions[concrete_idx]);
                sum_val += traverse(
                    &next_state, table, abstraction, evaluator,
                    rng, iteration, traverser, reach_prob, opponent_reach,
                    deck,
                );
            }
            v[a] = sum_val / count;
        }

        let v_sigma: f32 = strategy.iter().zip(v.iter()).map(|(p, u)| p * u).sum();
        for a in 0..K {
            let delta = v[a] - v_sigma;
            let cur = table.get_regret(infoset_hash, a);
            let is_pos = delta >= 0.0;
            let new_regret = dcfr::update_regret(cur, iteration, delta, is_pos);
            table.set_regret(infoset_hash, a, new_regret);
        }
        v_sigma
    } else {
        let dist = WeightedIndex::new(&strategy).expect("strategy non-empty");
        let sampled_abstract = dist.sample(rng);
        if abstract_actions[sampled_abstract].is_empty() {
            return 0.0;
        }
        let concrete_idx = abstract_actions[sampled_abstract]
            [rng.random_range(0..abstract_actions[sampled_abstract].len())];
        let next_state = current.apply_action(&num_actions[concrete_idx]);
        traverse(
            &next_state, table, abstraction, evaluator,
            rng, iteration, traverser,
            reach_prob, opponent_reach * strategy[sampled_abstract],
            deck,
        )
    }
}

fn abstract_action_index(kind: &ActionKind) -> usize {
    match kind {
        ActionKind::Fold => 0,
        ActionKind::Check | ActionKind::Call => 1,
        ActionKind::Bet(_) => 2,
    }
}
