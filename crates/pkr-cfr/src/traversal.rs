use crate::table::CompactRegretTable;
use pkr_abstraction::calculate_ehs;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::{ActionKind, GameState, Street};
use rand::Rng;
use rand::RngExt; // <--- Import EHS calculator

const K: usize = 6;
const MAX_DEPTH: u32 = 50; // Prevents infinite recursion from endless min-raises

pub fn traverse(
    current: &mut GameState,
    table: &CompactRegretTable,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    rng: &mut impl Rng,
    global_iteration: u32,
    traverser: usize,
    reach_prob: f32,
    opponent_reach: f32,
    deck: &[u8],
    deck_idx: &mut usize,
    depth: u32,
) -> f32 {
    // If we hit max depth, stop recursing to prevent stack overflow
    if depth > MAX_DEPTH {
        return 0.0;
    }

    // Use `if` instead of `while` to deal exactly one street transition per step.
    if current.is_street_complete() && !current.is_terminal() {
        let cards_needed = match current.street {
            Street::Preflop => 3,
            Street::Flop => 1,
            Street::Turn => 1,
            Street::River => 0,
        };
        let start = *deck_idx;
        *deck_idx += cards_needed;

        if *deck_idx > deck.len() {
            return 0.0;
        }

        let new_cards = &deck[start..*deck_idx];
        current.advance_street_in_place(new_cards);
    }

    if current.is_terminal() {
        return current.terminal_payoff(traverser, evaluator);
    }

    // =========================================================
    // DEPTH-LIMITED SOLVING (DeepStack Architecture)
    // If we reach the Turn, we DO NOT recurse to the River.
    // We evaluate the leaf node using EHS * Pot as the expected value.
    // This skips the entire turn/river CFR tree, giving a 100x speedup.
    // =========================================================
    if current.street == Street::Turn {
        let hole = &current.hole[traverser];
        let board = &current.board;
        let (ehs, _ehs_sq) = calculate_ehs(hole, board, evaluator);
        // Return expected chip value
        return ehs * current.pot;
    }

    let acting_player = current.actor;
    let num_actions = current.legal_actions();
    if num_actions.is_empty() {
        return 0.0;
    }

    let mut action_counts = [0usize; K];
    let mut action_indices = [[0usize; 10]; K];
    for (idx, action) in num_actions.iter().enumerate() {
        if let Some(a) = abstract_action_index(&action.kind, current) {
            if action_counts[a] < 10 {
                action_indices[a][action_counts[a]] = idx;
                action_counts[a] += 1;
            }
        }
    }

    // Use precomputed abstract history bytes (includes bet size encoding)
    let hist_len = current.abstract_history.len().min(32);
    let mut history_bytes = [0u8; 32];
    history_bytes[..hist_len].copy_from_slice(&current.abstract_history[..hist_len]);

    let hole = &current.hole[acting_player];
    let board = &current.board;
    let street_code = current.street as u8;
    let infoset_hash =
        abstraction.get_infoset_hash(hole, board, &history_bytes[..hist_len], street_code);

    let mut strategy = [0.0f32; K];
    table.get_strategy_into(infoset_hash, &mut strategy);

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
            let pick_idx = action_indices[a][rng.random_range(0..count)];

            // Apply in-place, recurse, then undo
            current.apply_action_in_place(&num_actions[pick_idx]);
            let mut local_deck_idx = *deck_idx;
            v[a] = traverse(
                current,
                table,
                abstraction,
                evaluator,
                rng,
                global_iteration,
                traverser,
                reach_prob * strategy[a],
                opponent_reach,
                deck,
                &mut local_deck_idx,
                depth + 1,
            );
            current.undo_action();
        }

        let v_sigma: f32 = strategy.iter().zip(v.iter()).map(|(p, u)| p * u).sum();

        // Collect regret updates into a batch to apply them
        let mut batch = [(0u64, 0usize, 0u32, 0.0f32); K];
        for a in 0..K {
            let delta = v[a] - v_sigma;
            batch[a] = (infoset_hash, a, global_iteration, delta);
        }
        table.apply_regret_batch(&batch);

        v_sigma
    } else {
        let r = rng.random::<f32>();
        let mut acc = 0.0;
        let mut sampled_abstract = K - 1;
        for i in 0..K {
            acc += strategy[i];
            if r <= acc {
                sampled_abstract = i;
                break;
            }
        }
        let count = action_counts[sampled_abstract];
        if count == 0 {
            return 0.0;
        }
        let pick_idx = action_indices[sampled_abstract][rng.random_range(0..count)];

        // Apply in-place, recurse, then undo
        current.apply_action_in_place(&num_actions[pick_idx]);
        let mut local_deck_idx = *deck_idx;
        let result = traverse(
            current,
            table,
            abstraction,
            evaluator,
            rng,
            global_iteration,
            traverser,
            reach_prob,
            opponent_reach * strategy[sampled_abstract],
            deck,
            &mut local_deck_idx,
            depth + 1,
        );
        current.undo_action();

        result
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
                Some(5)
            } else if fraction < 0.5 {
                Some(2)
            } else if fraction < 1.0 {
                Some(3)
            } else {
                Some(4)
            }
        }
    }
}
