//! Fuzz legal-action generation across random game states.
//!
//! NOTE (worklog B19): the plan draft called `GameState::new_heads_up()`,
//! `legal_actions_into(&mut [u8; 16])`, and `state.apply(u8)`. The real
//! API is `GameState::new(start_stack, sb, bb)`,
//! `legal_actions_into(&mut [Action; 8]) -> usize`, and
//! `apply_action_in_place(&Action)`. Each input byte selects among the
//! currently-legal concrete actions; applying it must never panic.

use libfuzzer_sys::fuzz_target;
use pkr_core::state::{Action, ActionKind, GameState};

fuzz_target!(|data: &[u8]| {
    let mut state = GameState::new(200.0, 1.0, 2.0);
    // Treat each byte as an action index, apply until no actions legal.
    for &a in data.iter().take(50) {
        let mut buf = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = state.legal_actions_into(&mut buf);
        if n == 0 {
            break;
        }
        let pick = (a as usize) % n;
        let action = buf[pick];
        // Apply the action. If this panics, the fuzzer found a bug.
        state.apply_action_in_place(&action);
        if state.is_terminal() {
            break;
        }
    }
});
