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

/// A compact record of what changed in the state so we can undo an action.
#[derive(Debug, Clone, Copy)]
pub struct UndoRecord {
    actor: usize,
    street: Street,
    pot: f32,
    stacks: [f32; 2],
    street_bets: [f32; 2],
    total_invested: [f32; 2],
    actions_this_street: u8,
    raises_this_street: u8,
    history_len: usize, // length of abstract_history before action
    board_len: usize,   // length of board before action
    folded: [bool; 2],
}

/// Stack-allocated game state. No heap allocations during traversal.
#[derive(Debug, Clone)]
pub struct GameState {
    pub hole: [[u8; 2]; 2],
    pub board: [u8; 5], // fixed 5 cards, board_len indicates how many are valid
    pub board_len: u8,
    pub pot: f32,
    pub stacks: [f32; 2],
    pub total_invested: [f32; 2],
    pub street: Street,
    pub actor: usize,
    pub dealer: usize,
    pub street_bets: [f32; 2],
    pub history: [Action; 32], // fixed array for action history
    pub history_len: u8,
    pub folded: [bool; 2],
    pub actions_this_street: u8,
    pub raises_this_street: u8,
    pub abstract_history: [u8; 32], // abstract action buckets
    pub abstract_history_len: u8,
    pub undo_stack: [UndoRecord; 32],
    pub undo_len: u8,
}

impl GameState {
    pub fn new(start_stack: f32, sb: f32, bb: f32) -> Self {
        Self {
            hole: [[0; 2]; 2],
            board: [0u8; 5],
            board_len: 0,
            pot: sb + bb,
            stacks: [start_stack - sb, start_stack - bb],
            total_invested: [sb, bb],
            street: Street::Preflop,
            actor: 0,
            dealer: 0,
            street_bets: [sb, bb],
            history: [Action {
                player: 0,
                kind: ActionKind::Fold,
            }; 32],
            history_len: 0,
            folded: [false; 2],
            actions_this_street: 0,
            raises_this_street: 0,
            abstract_history: [0u8; 32],
            abstract_history_len: 0,
            undo_stack: [UndoRecord {
                actor: 0,
                street: Street::Preflop,
                pot: 0.0,
                stacks: [0.0; 2],
                street_bets: [0.0; 2],
                total_invested: [0.0; 2],
                actions_this_street: 0,
                raises_this_street: 0,
                history_len: 0,
                board_len: 0,
                folded: [false; 2],
            }; 32],
            undo_len: 0,
        }
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
            actions.push(Action {
                player: self.actor,
                kind: ActionKind::Check,
            });
            let pot = self.pot;
            for &frac in &[0.5, 1.0, 2.0] {
                let bet = pot * frac;
                if bet <= self.stacks[self.actor] {
                    actions.push(Action {
                        player: self.actor,
                        kind: ActionKind::Bet(bet),
                    });
                }
            }
            if self.stacks[self.actor] > 0.0 {
                actions.push(Action {
                    player: self.actor,
                    kind: ActionKind::Bet(self.stacks[self.actor]),
                });
            }
        } else {
            actions.push(Action {
                player: self.actor,
                kind: ActionKind::Fold,
            });
            actions.push(Action {
                player: self.actor,
                kind: ActionKind::Call,
            });
            let pot = self.pot;
            for &frac in &[0.5, 1.0, 2.0] {
                let raise = to_call + pot * frac;
                if raise <= self.stacks[self.actor] + self.street_bets[self.actor] {
                    actions.push(Action {
                        player: self.actor,
                        kind: ActionKind::Bet(raise),
                    });
                }
            }
            if self.stacks[self.actor] > 0.0 {
                actions.push(Action {
                    player: self.actor,
                    kind: ActionKind::Bet(self.stacks[self.actor] + self.street_bets[self.actor]),
                });
            }
        }
        actions
    }

    /// Non-allocating variant of `legal_actions`. Writes into `out` and
    /// returns the count. Reused by the CFR traversal to avoid one heap
    /// allocation per node visit — the allocator is the dominant
    /// multithread bottleneck otherwise. Callers must provide a buffer of
    /// at least 8 slots; the current action space tops out at 6.
    pub fn legal_actions_into(&self, out: &mut [Action; 8]) -> usize {
        if self.folded[self.actor] {
            return 0;
        }
        // Raise cap: after MAX_RAISES_PER_STREET aggressive actions on this
        // street, only fold/check/call remain legal. This is a standard
        // action abstraction (Libratus, DeepStack). It bounds the tree
        // regardless of bet sizing and eliminates the rare 5-raise wars
        // that dominate tree size when small sizings are available.
        const MAX_RAISES_PER_STREET: u8 = 3;
        let can_raise = self.raises_this_street < MAX_RAISES_PER_STREET;

        let mut n = 0usize;
        let to_call = self.bet_to_call();
        if to_call == 0.0 {
            out[n] = Action {
                player: self.actor,
                kind: ActionKind::Check,
            };
            n += 1;
            if can_raise {
                let pot = self.pot;
                for &frac in &[0.5, 1.0, 2.0] {
                    if n >= 8 {
                        break;
                    }
                    let bet = pot * frac;
                    if bet <= self.stacks[self.actor] {
                        out[n] = Action {
                            player: self.actor,
                            kind: ActionKind::Bet(bet),
                        };
                        n += 1;
                    }
                }
                if n < 8 && self.stacks[self.actor] > 0.0 {
                    out[n] = Action {
                        player: self.actor,
                        kind: ActionKind::Bet(self.stacks[self.actor]),
                    };
                    n += 1;
                }
            }
        } else {
            out[n] = Action {
                player: self.actor,
                kind: ActionKind::Fold,
            };
            n += 1;
            out[n] = Action {
                player: self.actor,
                kind: ActionKind::Call,
            };
            n += 1;
            if can_raise {
                let pot = self.pot;
                for &frac in &[0.5, 1.0, 2.0] {
                    if n >= 8 {
                        break;
                    }
                    let raise = to_call + pot * frac;
                    if raise <= self.stacks[self.actor] + self.street_bets[self.actor] {
                        out[n] = Action {
                            player: self.actor,
                            kind: ActionKind::Bet(raise),
                        };
                        n += 1;
                    }
                }
                if n < 8 && self.stacks[self.actor] > 0.0 {
                    out[n] = Action {
                        player: self.actor,
                        kind: ActionKind::Bet(
                            self.stacks[self.actor] + self.street_bets[self.actor],
                        ),
                    };
                    n += 1;
                }
            }
        }
        n
    }

    /// Canonical signature of the betting history that actually matters
    /// to CFR: how many actions this street, how many raises, and whether
    /// the acting player is the aggressor. This replaces the raw history
    /// bytes (which are 6^32 possibilities) with a compact ~16-bit key,
    /// collapsing the infoset space by orders of magnitude without
    /// changing the legal action space at any node.
    pub fn history_signature(&self) -> u32 {
        let mut raises: u8 = 0;
        for i in 0..self.history_len as usize {
            if matches!(self.history[i].kind, ActionKind::Bet(_)) {
                raises = raises.saturating_add(1);
            }
        }
        let last_was_bet = if self.history_len > 0 {
            matches!(
                self.history[self.history_len as usize - 1].kind,
                ActionKind::Bet(_)
            )
        } else {
            false
        };
        (self.actions_this_street as u32 & 0xFF)
            | ((raises as u32 & 0xFF) << 8)
            | ((last_was_bet as u32) << 16)
    }

    /// Save current state before applying an action.
    fn push_undo(&mut self) {
        if self.undo_len as usize == self.undo_stack.len() {
            // Should never happen in reasonable play; if it does, we'd panic,
            // but 32 undo slots is plenty for a hand.
            return;
        }
        let record = UndoRecord {
            actor: self.actor,
            street: self.street,
            pot: self.pot,
            stacks: self.stacks,
            street_bets: self.street_bets,
            total_invested: self.total_invested,
            actions_this_street: self.actions_this_street,
            raises_this_street: self.raises_this_street,
            history_len: self.history_len as usize,
            board_len: self.board_len as usize,
            folded: self.folded,
        };
        self.undo_stack[self.undo_len as usize] = record;
        self.undo_len += 1;
    }

    /// Apply an action in place, saving undo info.
    pub fn apply_action_in_place(&mut self, action: &Action) {
        self.push_undo();
        self.apply_action_internal(action);
    }

    /// Internal apply without undo (for initial state setup).
    fn apply_action_internal(&mut self, action: &Action) {
        let actor = self.actor;
        match action.kind {
            ActionKind::Fold => {
                self.folded[actor] = true;
            }
            ActionKind::Check => {}
            ActionKind::Call => {
                let to_call = self.bet_to_call();
                let chips = to_call.min(self.stacks[actor]);
                self.stacks[actor] -= chips;
                self.pot += chips;
                self.total_invested[actor] += chips;
                self.street_bets[actor] += chips;
            }
            ActionKind::Bet(total) => {
                let current = self.street_bets[actor];
                let chips = (total - current).max(0.0).min(self.stacks[actor]);
                self.stacks[actor] -= chips;
                self.pot += chips;
                self.total_invested[actor] += chips;
                self.street_bets[actor] = total;
                self.raises_this_street = self.raises_this_street.saturating_add(1);
            }
        }

        // Record abstract action bucket
        let bucket = abstract_action_index_static(&action.kind, self);
        if (self.abstract_history_len as usize) < self.abstract_history.len() {
            self.abstract_history[self.abstract_history_len as usize] = bucket;
            self.abstract_history_len += 1;
        }

        // Record history
        if (self.history_len as usize) < self.history.len() {
            self.history[self.history_len as usize] = *action;
            self.history_len += 1;
        }

        self.actions_this_street += 1;
        let next = 1 - actor;
        if self.folded[next] {
            // other player folded – terminal handled by is_terminal
        }
        self.actor = next;
    }

    /// Undo the last applied action.
    pub fn undo_action(&mut self) {
        if self.undo_len == 0 {
            return;
        }
        self.undo_len -= 1;
        let rec = self.undo_stack[self.undo_len as usize];
        self.actor = rec.actor;
        self.street = rec.street;
        self.pot = rec.pot;
        self.stacks = rec.stacks;
        self.street_bets = rec.street_bets;
        self.total_invested = rec.total_invested;
        self.actions_this_street = rec.actions_this_street;
        self.raises_this_street = rec.raises_this_street;
        self.history_len = rec.history_len as u8;
        self.board_len = rec.board_len as u8;
        self.folded = rec.folded;
        self.abstract_history_len = rec.history_len as u8; // same as history length
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
        let hero_rank =
            evaluator.evaluate_hand(&self.hole[0], &self.board[..self.board_len as usize]);
        let vill_rank =
            evaluator.evaluate_hand(&self.hole[1], &self.board[..self.board_len as usize]);
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

    /// Advance to next street, adding community cards.
    pub fn advance_street_in_place(&mut self, cards: &[u8]) {
        self.push_undo(); // allow undoing street advance if needed (though we won't typically undo streets)
        for &c in cards {
            if (self.board_len as usize) < 5 {
                self.board[self.board_len as usize] = c;
                self.board_len += 1;
            }
        }
        self.street = match self.street {
            Street::Preflop => Street::Flop,
            Street::Flop => Street::Turn,
            Street::Turn => Street::River,
            Street::River => unreachable!(),
        };
        self.street_bets = [0.0; 2];
        self.actor = 1 - self.dealer;
        self.actions_this_street = 0;
        self.raises_this_street = 0;
    }
}

/// Map action kind to abstract bucket (0..5) given the state before the action.
fn abstract_action_index_static(kind: &ActionKind, state: &GameState) -> u8 {
    match kind {
        ActionKind::Fold => 0,
        ActionKind::Check | ActionKind::Call => 1,
        ActionKind::Bet(amount) => {
            let pot = state.pot.max(1.2);
            let fraction = amount / pot;
            if *amount >= state.stacks[state.actor] + state.street_bets[state.actor] {
                5 // all-in
            } else if fraction < 1.5 {
                2
            } else if fraction < 1.2 {
                3
            } else {
                4
            }
        }
    }
}
