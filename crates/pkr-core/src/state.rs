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
}

impl GameState {
    pub fn new(start_stack: f32, sb: f32, bb: f32) -> Self {
        let mut state = Self {
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

    pub fn apply_action(&self, action: &Action) -> Self {
        let mut new = self.clone();
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
        if new.folded[next] {
            // other player folded – terminal handled by is_terminal
        }
        new.actor = next;
        new
    }

    pub fn is_street_complete(&self) -> bool {
        // Street is complete if no pending bet and at least 2 actions have occurred this street
        // (both players have had at least one chance to act). Exception: preflop after blinds
        // we start with actions_this_street = 0 but blinds are already posted. We need both
        // players to act at least once after the start. So actions_this_street >= 2 and
        // bet_to_call == 0 for the current actor.
        self.bet_to_call() == 0.0 && self.actions_this_street >= 2
    }

    pub fn is_terminal(&self) -> bool {
        if self.folded.iter().any(|&f| f) {
            return true;
        }
        // After River, if street complete, terminal
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
