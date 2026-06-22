use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::{ActionKind, GameState, Street};
use rand::Rng;
use rand::RngExt;
use rand::distr::{Distribution, weighted::WeightedIndex};
use crate::dcfr;
use crate::table::CompactRegretTable;

const K: usize = 6;

pub fn traverse(
    state: &GameState,
    table: &mut CompactRegretTable,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    rng: &mut impl Rng,
    global_iteration: u32,
    traverser: usize,
    reach_prob: f32,
    opponent_reach: f32,
    deck: &[u8],                // immutable reference to remaining cards
    deck_idx: &mut usize,       // mutable index into deck
) -> f32 {
    let mut current = state.clone();

    // Advance streets by reading from deck without draining
    while current.is_street_complete() && !current.is_terminal() {
        let cards_needed = match current.street {
            Street::Preflop => 3,
            Street::Flop => 1,
            Street::Turn => 1,
            Street::River => break,
        };
        let start = *deck_idx;
        *deck_idx += cards_needed;
        let new_cards = &deck[start..*deck_idx];
        current.advance_street(new_cards);
    }

    if current.is_terminal() {
        return current.terminal_payoff(traverser, evaluator);
    }

    let acting_player = current.actor;
    let num_actions = current.legal_actions();
    if num_actions.is_empty() { return 0.0; }

    // Build fixed-size abstract action mapping (stack arrays, no allocs)
    let mut action_counts = [0usize; K];
    let mut action_indices = [[0usize; 10]; K]; // max 10 concrete actions per abstract bucket
    for (idx, action) in num_actions.iter().enumerate() {
        if let Some(a) = abstract_action_index(&action.kind, &current) {
            if action_counts[a] < 10 {
                action_indices[a][action_counts[a]] = idx;
                action_counts[a] += 1;
            }
        }
    }

    // History bytes on stack (max 32 actions)
    let mut history_bytes = [0u8; 32];
    let hist_len = current.history.len().min(32);
    for (i, a) in current.history.iter().take(32).enumerate() {
        history_bytes[i] = match a.kind {
            ActionKind::Fold => 0,
            ActionKind::Check | ActionKind::Call => 1,
            ActionKind::Bet(_) => 2,
        };
    }

    let hole = &current.hole[acting_player];
    let board = &current.board;
    let street_code = current.street as u8;
    let infoset_hash = abstraction.get_infoset_hash(hole, board, &history_bytes[..hist_len], street_code);

    // Get strategy into stack buffer
    let mut strategy = [0.0f32; K];
    table.get_strategy_into(infoset_hash, &mut strategy);

    // Only update strategy sum for the traverser
    if acting_player == traverser {
        for a in 0..K {
            table.add_strategy_sum(infoset_hash, a, strategy[a] * reach_prob);
        }
    }

    if acting_player == traverser {
        let mut v = [0.0f32; K];
        for a in 0..K {
            let count = action_counts[a];
            if count == 0 {
                v[a] = 0.0;
                continue;
            }
            // Sample ONE concrete action uniformly (unbiased)
            let pick_idx = action_indices[a][rng.random_range(0..count)];
            let next_state = current.apply_action(&num_actions[pick_idx]);
            v[a] = traverse(
                &next_state, table, abstraction, evaluator,
                rng, global_iteration, traverser, reach_prob, opponent_reach,
                deck, deck_idx,
            );
        }

        let v_sigma: f32 = strategy.iter().zip(v.iter()).map(|(p, u)| p * u).sum();

        for a in 0..K {
            let delta = v[a] - v_sigma;
            let cur = table.get_regret(infoset_hash, a);
            let new_regret = dcfr::update_regret(cur, global_iteration, delta);
            table.set_regret(infoset_hash, a, new_regret);
        }
        v_sigma
    } else {
        let dist = WeightedIndex::new(&strategy).expect("strategy must have positive sum");
        let sampled_abstract = dist.sample(rng);
        let count = action_counts[sampled_abstract];
        if count == 0 { return 0.0; }
        let pick_idx = action_indices[sampled_abstract][rng.random_range(0..count)];
        let next_state = current.apply_action(&num_actions[pick_idx]);
        traverse(
            &next_state, table, abstraction, evaluator,
            rng, global_iteration, traverser,
            reach_prob, opponent_reach * strategy[sampled_abstract],
            deck, deck_idx,
        )
    }
}

fn abstract_action_index(kind: &ActionKind, state: &GameState) -> Option<usize> {
    match kind {
        ActionKind::Fold => Some(0),
        ActionKind::Check | ActionKind::Call => Some(1),
        ActionKind::Bet(amount) => {
            let pot = state.pot.max(1.0);
            let fraction = amount / pot;
            if *amount >= state.stacks[state.actor] + state.street_bets[state.actor] {
                Some(5) // all-in
            } else if fraction < 0.5 {
                Some(2) // small
            } else if fraction < 1.0 {
                Some(3) // medium
            } else {
                Some(4) // large
            }
        }
    }
}
