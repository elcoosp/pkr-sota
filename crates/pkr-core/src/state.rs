use pkr_contracts::Evaluator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Street {
    Preflop,
    Flop,
    Turn,
    River,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActionKind {
    Fold,
    Check,
    Call,
    Bet(f32),
}

#[derive(Debug, Clone, Copy)]
pub struct Action {
    pub player: usize,
    pub kind: ActionKind,
}

#[derive(Debug, Clone)]
pub struct GameState {
    pub hole: [[u8; 2]; 2],
    pub board: Vec<u8>,
    pub pot: f32,
    pub stacks: [f32; 2],
    pub total_invested: [f32; 2],
    pub street: Street,
    pub actor: usize,
    pub dealer: usize,
    pub street_bets: [f32; 2],
    pub history: Vec<Action>,
    pub folded: [bool; 2],
    actions_this_street: usize,
    /// Abstract action bucket (0..5) for each action in `history`, used for infoset hashing.
    pub abstract_history: Vec<u8>,
}

impl GameState {
    pub fn new(start_stack: f32, sb: f32, bb: f32) -> Self {
        let state = Self {
            hole: [[0; 2]; 2],
            board: Vec::new(),
            pot: sb + bb,
            stacks: [start_stack - sb, start_stack - bb],
            total_invested: [sb, bb],
            street: Street::Preflop,
            actor: 0,
            dealer: 0,
            street_bets: [sb, bb],
            history: Vec::new(),
            folded: [false; 2],
            actions_this_street: 0,
            abstract_history: Vec::new(),
        };
        state
    }

    pub fn set_hole_cards(&mut self, hero: [u8; 2], villain: [u8; 2]) {
        self.hole[0] = hero;
        self.hole[1] = villain;
    }

    pub fn bet_to_call(&self) -> f32 {
        let opp = 1 - self.actor;
        (self.street_bets[opp] - self.street_bets[self.actor]).max(0.0)
    }

    pub fn legal_actions(&self) -> Vec<Action> {
        if self.folded[self.actor] {
            return vec![];
        }
        let mut actions = Vec::new();
        let to_call = self.bet_to_call();
        if to_call == 0.0 {
            actions.push(Action { player: self.actor, kind: ActionKind::Check });
            let pot = self.pot;
            for &frac in &[0.5, 0.75, 1.0, 1.5, 2.0] {
                let bet = pot * frac;
                if bet <= self.stacks[self.actor] {
                    actions.push(Action { player: self.actor, kind: ActionKind::Bet(bet) });
                }
            }
            if self.stacks[self.actor] > 0.0 {
                actions.push(Action { player: self.actor, kind: ActionKind::Bet(self.stacks[self.actor]) });
            }
        } else {
            actions.push(Action { player: self.actor, kind: ActionKind::Fold });
            actions.push(Action { player: self.actor, kind: ActionKind::Call });
            let pot = self.pot;
            for &frac in &[0.5, 0.75, 1.0, 1.5, 2.0] {
                let raise = to_call + pot * frac;
                if raise <= self.stacks[self.actor] + self.street_bets[self.actor] {
                    actions.push(Action { player: self.actor, kind: ActionKind::Bet(raise) });
                }
            }
            if self.stacks[self.actor] > 0.0 {
                actions.push(Action { player: self.actor, kind: ActionKind::Bet(self.stacks[self.actor] + self.street_bets[self.actor]) });
            }
        }
        actions
    }

    /// Apply an action, returning the new state. Also records the abstract action bucket in `abstract_history`.
    pub fn apply_action(&self, action: &Action) -> Self {
        let mut new = self.clone();
        // Compute abstract action bucket BEFORE applying (using current state)
        let bucket = abstract_action_index_static(&action.kind, &self);
        new.abstract_history.push(bucket);

        let actor = self.actor;
        match action.kind {
            ActionKind::Fold => {
                new.folded[actor] = true;
            }
            ActionKind::Check => {}
            ActionKind::Call => {
                let to_call = self.bet_to_call();
                let chips = to_call.min(new.stacks[actor]);
                new.stacks[actor] -= chips;
                new.pot += chips;
                new.total_invested[actor] += chips;
                new.street_bets[actor] += chips;
            }
            ActionKind::Bet(total) => {
                let current = new.street_bets[actor];
                let chips = (total - current).max(0.0).min(new.stacks[actor]);
                new.stacks[actor] -= chips;
                new.pot += chips;
                new.total_invested[actor] += chips;
                new.street_bets[actor] = total;
            }
        }
        new.history.push(*action);
        new.actions_this_street += 1;
        let next = 1 - actor;
        new.actor = next;
        new
    }

    pub fn is_street_complete(&self) -> bool {
        self.bet_to_call() == 0.0 && self.actions_this_street >= 2
    }

    pub fn is_terminal(&self) -> bool {
        if self.folded.iter().any(|&f| f) {
            return true;
        }
        if self.street == Street::River && self.is_street_complete() {
            return true;
        }
        false
    }

    pub fn terminal_payoff(&self, player: usize, evaluator: &dyn Evaluator) -> f32 {
        if self.folded[player] {
            return -self.total_invested[player];
        }
        let other = 1 - player;
        if self.folded[other] {
            return self.pot - self.total_invested[player];
        }
        let hero_rank = evaluator.evaluate_hand(&self.hole[0], &self.board);
        let vill_rank = evaluator.evaluate_hand(&self.hole[1], &self.board);
        let win = hero_rank < vill_rank;
        let tie = hero_rank == vill_rank;
        if tie {
            (self.pot / 2.0) - self.total_invested[player]
        } else if (player == 0 && win) || (player == 1 && !win) {
            self.pot - self.total_invested[player]
        } else {
            -self.total_invested[player]
        }
    }

    pub fn advance_street(&mut self, cards: &[u8]) {
        self.board.extend_from_slice(cards);
        self.street = match self.street {
            Street::Preflop => Street::Flop,
            Street::Flop => Street::Turn,
            Street::Turn => Street::River,
            Street::River => unreachable!(),
        };
        self.street_bets = [0.0; 2];
        self.actor = 1 - self.dealer;
        self.actions_this_street = 0;
    }
}

/// Map action kind to abstract bucket (0..5) given the state before the action.
fn abstract_action_index_static(kind: &ActionKind, state: &GameState) -> u8 {
    match kind {
        ActionKind::Fold => 0,
        ActionKind::Check | ActionKind::Call => 1,
        ActionKind::Bet(amount) => {
            let pot = state.pot.max(1.0);
            let fraction = amount / pot;
            if *amount >= state.stacks[state.actor] + state.street_bets[state.actor] {
                5 // all-in
            } else if fraction < 0.5 {
                2
            } else if fraction < 1.0 {
                3
            } else {
                4
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    struct DummyEvaluator;
    impl Evaluator for DummyEvaluator {
        fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u32 { 0 }
    }

    #[test]
    fn test_initial_state() {
        let state = GameState::new(200.0, 1.0, 2.0);
        assert_eq!(state.pot, 3.0);
        assert_eq!(state.stacks, [199.0, 198.0]);
        assert_eq!(state.street, Street::Preflop);
        assert_eq!(state.actor, 0);
        assert_eq!(state.folded, [false, false]);
    }

    #[test]
    fn test_preflop_betting() {
        let state = GameState::new(200.0, 1.0, 2.0);
        assert_eq!(state.bet_to_call(), 1.0);
        let actions = state.legal_actions();
        assert!(actions.iter().any(|a| matches!(a.kind, ActionKind::Fold)));
        assert!(actions.iter().any(|a| matches!(a.kind, ActionKind::Call)));
    }

    #[test]
    fn test_fold_ends_hand() {
        let mut state = GameState::new(200.0, 1.0, 2.0);
        state = state.apply_action(&Action { player: 0, kind: ActionKind::Fold });
        assert!(state.folded[0]);
        assert!(state.is_terminal());
    }

    #[test]
    fn test_check_check_completes_street() {
        let mut state = GameState::new(200.0, 1.0, 2.0);
        state = state.apply_action(&Action { player: 0, kind: ActionKind::Call });
        assert_eq!(state.actor, 1);
        state = state.apply_action(&Action { player: 1, kind: ActionKind::Check });
        assert!(state.is_street_complete());
    }

    #[test]
    fn test_advance_street() {
        let mut state = GameState::new(200.0, 1.0, 2.0);
        state = state.apply_action(&Action { player: 0, kind: ActionKind::Call });
        state = state.apply_action(&Action { player: 1, kind: ActionKind::Check });
        assert!(state.is_street_complete());
        state.advance_street(&[10, 11, 12]);
        assert_eq!(state.street, Street::Flop);
        assert_eq!(state.board, vec![10, 11, 12]);
        assert_eq!(state.street_bets, [0.0, 0.0]);
    }

    #[test]
    fn test_showdown_payoff() {
        let mut state = GameState::new(200.0, 1.0, 2.0);
        state.set_hole_cards([0, 1], [2, 3]);
        state = state.apply_action(&Action { player: 0, kind: ActionKind::Call });
        state = state.apply_action(&Action { player: 1, kind: ActionKind::Check });
        state.board = vec![4,5,6,7,8];
        state.street = Street::River;
        let eval = DummyEvaluator;
        let payoff = state.terminal_payoff(0, &eval);
        assert!((payoff - (state.pot/2.0 - state.total_invested[0])).abs() < 0.001);
    }

    #[test]
    fn test_bet_sizing() {
        let mut state = GameState::new(200.0, 1.0, 2.0);
        state = state.apply_action(&Action { player: 0, kind: ActionKind::Call });
        let actions = state.legal_actions();
        let bet_actions: Vec<_> = actions.iter().filter(|a| matches!(a.kind, ActionKind::Bet(_))).collect();
        assert!(!bet_actions.is_empty());
    }

    #[test]
    fn test_terminal_payoff_fold() {
        let mut state = GameState::new(200.0, 1.0, 2.0);
        state = state.apply_action(&Action { player: 0, kind: ActionKind::Fold });
        let eval = DummyEvaluator;
        assert!(state.terminal_payoff(0, &eval) < 0.0);
        assert!(state.terminal_payoff(1, &eval) > 0.0);
    }
}
