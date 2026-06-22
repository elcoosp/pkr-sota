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
    /// Total chips the player will have invested this street after this action.
    Bet(f32),
}

#[derive(Debug, Clone, Copy)]
pub struct Action {
    pub player: usize,
    pub kind: ActionKind,
}

#[derive(Debug, Clone)]
pub struct GameState {
    pub hole: [[u8; 2]; 2],       // [hero, villain]
    pub board: Vec<u8>,           // 0..5 community cards
    pub pot: f32,
    pub stacks: [f32; 2],         // remaining chips before current street bets
    pub total_invested: [f32; 2], // total chips contributed to the pot from stack
    pub street: Street,
    pub actor: usize,
    pub dealer: usize,            // 0 = hero is SB/button (assume hero is SB)
    pub street_bets: [f32; 2],    // chips each player has put in this street
    pub history: Vec<Action>,
    pub folded: [bool; 2],
}

impl GameState {
    /// Heads-up with hero as SB/button (dealer=0), blinds sb/bb, stacks start.
    /// Villain is BB (dealer=1). Preflop actor = dealer (SB acts first).
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
        };
        // After posting blinds, actor is dealer (SB) with bet_to_call = bb - sb
        state.actor = 0;
        state
    }

    pub fn set_hole_cards(&mut self, hero: [u8; 2], villain: [u8; 2]) {
        self.hole[0] = hero;
        self.hole[1] = villain;
    }

    /// The amount the current player must put in to call.
    pub fn bet_to_call(&self) -> f32 {
        let opp = 1 - self.actor;
        (self.street_bets[opp] - self.street_bets[self.actor]).max(0.0)
    }

    /// All legal actions for the current player.
    pub fn legal_actions(&self) -> Vec<Action> {
        if self.folded[self.actor] {
            return vec![];
        }
        let mut actions = Vec::new();
        let to_call = self.bet_to_call();
        if to_call == 0.0 {
            actions.push(Action { player: self.actor, kind: ActionKind::Check });
            // Bet sizes: fractions of the pot, plus all-in
            let pot = self.pot;
            for &frac in &[0.5, 0.75, 1.0, 1.5, 2.0] {
                let bet = pot * frac;
                if bet <= self.stacks[self.actor] {
                    actions.push(Action { player: self.actor, kind: ActionKind::Bet(bet) });
                }
            }
            // All-in
            if self.stacks[self.actor] > 0.0 {
                actions.push(Action { player: self.actor, kind: ActionKind::Bet(self.stacks[self.actor]) });
            }
        } else {
            actions.push(Action { player: self.actor, kind: ActionKind::Fold });
            actions.push(Action { player: self.actor, kind: ActionKind::Call });
            // Raise: total chips = to_call + pot * frac
            let pot = self.pot;
            for &frac in &[0.5, 0.75, 1.0, 1.5, 2.0] {
                let raise = to_call + pot * frac;
                if raise <= self.stacks[self.actor] + self.street_bets[self.actor] {
                    actions.push(Action { player: self.actor, kind: ActionKind::Bet(raise) });
                }
            }
            // All-in
            if self.stacks[self.actor] > 0.0 {
                actions.push(Action { player: self.actor, kind: ActionKind::Bet(self.stacks[self.actor] + self.street_bets[self.actor]) });
            }
        }
        actions
    }

    /// Apply an action, returning the new state.
    pub fn apply_action(&self, action: &Action) -> Self {
        let mut new = self.clone();
        let actor = self.actor;
        match action.kind {
            ActionKind::Fold => {
                new.folded[actor] = true;
            }
            ActionKind::Check => {
                // nothing changes in money
            }
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
        // Switch actor to the other non-folded player
        let next = 1 - actor;
        if new.folded[next] {
            // if other folded, terminal later
        }
        new.actor = next;
        new
    }

    pub fn is_terminal(&self) -> bool {
        if self.folded.iter().any(|&f| f) {
            return true;
        }
        if self.street == Street::River && self.bet_to_call() == 0.0 {
            // Both players have acted on river at least once
            return true;
        }
        false
    }

    /// Compute terminal payoff for a given player (net chips gained relative to starting stack).
    pub fn terminal_payoff(&self, player: usize, evaluator: &dyn Evaluator) -> f32 {
        if self.folded[player] {
            // player lost their total investment
            return -self.total_invested[player];
        }
        let other = 1 - player;
        if self.folded[other] {
            // player wins pot, net gain = pot - own investment
            return self.pot - self.total_invested[player];
        }
        // Showdown
        let hero_rank = evaluator.evaluate_hand(&self.hole[0], &self.board);
        let vill_rank = evaluator.evaluate_hand(&self.hole[1], &self.board);
        let win = hero_rank < vill_rank;
        let tie = hero_rank == vill_rank;
        if tie {
            // each gets back half the pot
            (self.pot / 2.0) - self.total_invested[player]
        } else if (player == 0 && win) || (player == 1 && !win) {
            self.pot - self.total_invested[player]
        } else {
            -self.total_invested[player]
        }
    }

    /// Advance to next street, adding community cards and resetting bets.
    pub fn advance_street(&mut self, cards: &[u8]) {
        self.board.extend_from_slice(cards);
        self.street = match self.street {
            Street::Preflop => Street::Flop,
            Street::Flop => Street::Turn,
            Street::Turn => Street::River,
            Street::River => unreachable!(),
        };
        self.street_bets = [0.0; 2];
        self.actor = 1 - self.dealer; // postflop non-dealer acts first
    }
}
