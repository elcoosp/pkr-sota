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
            // C1.5: `Bet` is the actor's street-bet TOTAL, not the
            // incremental chips. `pot * frac` was correct postflop
            // (street_bets[actor] == 0); preflop (SB completing, BB
            // raising a limp) it forgot the already-posted blind.
            let base = self.street_bets[self.actor];
            for &frac in &[0.4, 0.8, 1.6] {
                let bet = base + pot * frac;
                let chips_needed = bet - base;
                if chips_needed <= self.stacks[self.actor] {
                    actions.push(Action {
                        player: self.actor,
                        kind: ActionKind::Bet(bet),
                    });
                }
            }
            if self.stacks[self.actor] > 0.0 {
                // C2: `Bet` is the actor's street-bet TOTAL, not the
                // incremental chips. Preflop BB facing a limp has
                // street_bets[BB] == 2 already posted; `Bet(stacks)`
                // would ask for a 199-total (chips moved = 197, 1 chip
                // stays behind) and mis-bucket as bucket 4. The correct
                // all-in total is stacks + street_bets.
                actions.push(Action {
                    player: self.actor,
                    kind: ActionKind::Bet(self.stacks[self.actor] + self.street_bets[self.actor]),
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
            // C1.5: raise TOTAL is opponent's committed street bet
            // + a pot-fraction on top. Old form `to_call + pot * frac`
            // under-counted by street_bets[actor] preflop.
            let opp_bet = self.street_bets[1 - self.actor];
            for &frac in &[0.4, 0.8, 1.6] {
                let raise = opp_bet + pot * frac;
                let chips_needed = raise - self.street_bets[self.actor];
                if chips_needed <= self.stacks[self.actor] {
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
                let base = self.street_bets[self.actor];
                for &frac in &[0.4, 0.8, 1.6] {
                    if n >= 8 {
                        break;
                    }
                    let bet = base + pot * frac; // C1.5
                    let chips_needed = bet - base;
                    if chips_needed <= self.stacks[self.actor] {
                        out[n] = Action {
                            player: self.actor,
                            kind: ActionKind::Bet(bet),
                        };
                        n += 1;
                    }
                }
                if n < 8 && self.stacks[self.actor] > 0.0 {
                    // C2: see legal_actions — all-in total is
                    // stacks + street_bets, not stacks alone.
                    out[n] = Action {
                        player: self.actor,
                        kind: ActionKind::Bet(
                            self.stacks[self.actor] + self.street_bets[self.actor],
                        ),
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
                let opp_bet = self.street_bets[1 - self.actor];
                for &frac in &[0.4, 0.8, 1.6] {
                    if n >= 8 {
                        break;
                    }
                    let raise = opp_bet + pot * frac; // C1.5
                    let chips_needed = raise - self.street_bets[self.actor];
                    if chips_needed <= self.stacks[self.actor] {
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
                // C1: `street_bets` is DERIVED from actual chips moved,
                // never trusted from the caller. Two divergence modes
                // are fixed:
                //
                //   1. total > current + stacks (overbet): old code wrote
                //      `street_bets = total`, recording more chips in the
                //      street bet than actually moved into the pot. From
                //      then on, pot/stacks/street_bets were mutually
                //      inconsistent.
                //   2. total < current (illegal under-bet, producible by
                //      bots/harness): old code wrote a smaller
                //      `street_bets` without refunding chips — money
                //      vanished.
                //
                // For every legal action this is bit-identical to the
                // old behaviour, because all legal `total` satisfy
                // `total == current + chips`.
                let current = self.street_bets[actor];
                // NOTE (C1.5 follow-up): `legal_actions_into` currently
                // produces `Bet(total)` values that can be BELOW the
                // actor's current street bet, because its raise formula
                // is `to_call + pot * frac` instead of
                // `street_bets[opp] + pot * frac`. This is invisible
                // postflop (actor has street_bets == 0) but breaks
                // preflop lines. The correct fix is in `legal_actions*`
                // (tracked separately); here we only ensure that any
                // such under-bet is a strict no-op rather than the
                // pre-C1 behaviour of silently reducing street_bets.
                //
                // Diagnostic: set PKR_STRICT_BETS=1 to make this an
                // assertion during development.
                #[cfg(debug_assertions)]
                if total < current && std::env::var("PKR_STRICT_BETS").as_deref() == Ok("1") {
                    panic!(
                        "Bet({total}) below current street bet {current} \
                         (PKR_STRICT_BETS=1)"
                    );
                }
                let chips = (total - current).max(0.0).min(self.stacks[actor]);
                self.stacks[actor] -= chips;
                self.pot += chips;
                self.total_invested[actor] += chips;
                self.street_bets[actor] = current + chips;
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
            } else if fraction < 0.6 {
                2
            } else if fraction < 1.2 {
                3
            } else {
                4
            }
        }
    }
}

#[cfg(test)]
mod c1_tests {
    use super::*;

    /// C1: illegal under-bet (total < current) is a benign no-op —
    /// no chips move, street_bets unchanged, pot unchanged.
    #[test]
    fn bet_below_current_is_benign_noop() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        // SB limps: chips 1, street_bets[0] = 2
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Call,
        });
        // Now BB acts. street_bets[1] = 2, stacks[1] = 198.
        let pot_before = s.pot;
        let sb_before = s.street_bets[1];
        let stack_before = s.stacks[1];
        // Illegal: ask to "bet" 0.5, less than current street bet of 2.0
        s.apply_action_in_place(&Action {
            player: 1,
            kind: ActionKind::Bet(0.5),
        });
        assert_eq!(s.street_bets[1], sb_before, "street_bets unchanged");
        assert_eq!(s.pot, pot_before, "pot unchanged");
        assert_eq!(s.stacks[1], stack_before, "stacks unchanged");
    }

    /// C1: overbet clamps to all-in exactly — stacks zeroed,
    /// street_bets equals starting_stack.
    #[test]
    fn bet_overbet_clamps_to_all_in_exactly() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Call,
        });
        // BB asks for Bet(10_000) but has 198 chips behind after posting.
        s.apply_action_in_place(&Action {
            player: 1,
            kind: ActionKind::Bet(10_000.0),
        });
        assert_eq!(s.stacks[1], 0.0, "all-in leaves zero behind");
        assert_eq!(
            s.street_bets[1], 200.0,
            "street_bet total equals start_stack when all-in"
        );
    }

    /// C1: a legal bet produces street_bets == current + chips (the
    /// invariant the fix restores). This is what training relies on.
    #[test]
    fn legal_bet_satisfies_street_bets_invariant() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Call,
        });
        let current = s.street_bets[1];
        let pot = s.pot;
        // Legal: bet 2x pot = 8 chips on top of the 2 already in.
        let total = current + 8.0;
        s.apply_action_in_place(&Action {
            player: 1,
            kind: ActionKind::Bet(total),
        });
        assert_eq!(s.street_bets[1], total);
        assert_eq!(s.stacks[1], 200.0 - 2.0 - 8.0);
        assert_eq!(s.pot, pot + 8.0);
    }
}

#[cfg(test)]
mod c2_tests {
    use super::*;

    /// C2: preflop BB facing a limp must be offered a TRUE all-in
    /// (leaves 0 chips behind, street_bets == starting stack).
    #[test]
    fn bb_check_jam_is_true_all_in() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        // SB limps (calls the extra 1 chip): SB street_bets -> 2, pot -> 4.
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Call,
        });
        // Now actor is BB. to_call == 0, street_bets[BB] == 2, stacks[BB] == 198.
        assert_eq!(s.actor, 1);
        assert_eq!(s.bet_to_call(), 0.0);
        assert_eq!(s.street_bets[1], 2.0);
        assert_eq!(s.stacks[1], 198.0);

        let mut buf: [Action; 8] = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = s.legal_actions_into(&mut buf);

        // Find the all-in: it should be Bet(200.0), i.e. stacks + street_bets.
        let all_in = buf[..n]
            .iter()
            .find(|a| matches!(a.kind, ActionKind::Bet(x) if (x - 200.0).abs() < 1e-4))
            .expect("BB facing a limp must be offered a true 200-total all-in");

        // Old bug: Bet(198.0) was offered instead, leaving 1 chip behind.
        let has_buggy_form = buf[..n]
            .iter()
            .any(|a| matches!(a.kind, ActionKind::Bet(x) if (x - 198.0).abs() < 1e-4));
        assert!(
            !has_buggy_form,
            "old buggy Bet(stacks) form must not appear; buf={:?}",
            &buf[..n]
        );

        s.apply_action_in_place(all_in);
        assert_eq!(s.stacks[1], 0.0, "all-in leaves zero behind");
        assert_eq!(
            s.street_bets[1], 200.0,
            "street-bet total equals start_stack"
        );
    }

    /// C2: postflop with no bet facing, all-in total is just stacks
    /// (street_bets already 0), unchanged from pre-C2 behaviour.
    #[test]
    fn postflop_check_jam_unchanged_when_street_bets_zero() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        // SB limp, BB check -> flop
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Call,
        });
        s.apply_action_in_place(&Action {
            player: 1,
            kind: ActionKind::Check,
        });
        s.advance_street_in_place(&[0, 1, 2]);

        assert_eq!(s.street_bets[1], 0.0, "post-flop street_bets reset to 0");
        let stack = s.stacks[1];

        let mut buf: [Action; 8] = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = s.legal_actions_into(&mut buf);
        let all_in = buf[..n]
            .iter()
            .find(|a| matches!(a.kind, ActionKind::Bet(x) if (x - stack).abs() < 1e-4))
            .expect("postflop all-in must equal stacks (street_bets == 0)");
        s.apply_action_in_place(all_in);
        assert_eq!(s.stacks[1], 0.0);
    }

    /// C2 regression: the offered all-in must be bucketed 5, not 4.
    /// (Uses the same thresholds as the trainer's `abstract_action_index`.)
    #[test]
    fn bb_check_jam_buckets_as_all_in() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Call,
        });
        let mut buf: [Action; 8] = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = s.legal_actions_into(&mut buf);
        let all_in = buf[..n]
            .iter()
            .find(|a| matches!(a.kind, ActionKind::Bet(x) if (x - 200.0).abs() < 1e-4))
            .expect("true all-in offered");
        // Recompute bucket via state's own static mapper.
        // (abstract_action_index_static is private but callable from this module.)
        let bucket = super::abstract_action_index_static(&all_in.kind, &s);
        assert_eq!(bucket, 5, "all-in total must bucket as 5, not 4");
    }
}

#[cfg(test)]
mod c1_5_tests {
    use super::*;

    // Sizings currently used by `legal_actions*` after T0.2.
    const SIZINGS: [f32; 3] = [0.4, 0.8, 1.6];

    fn bets(buf: &[Action], n: usize) -> Vec<f32> {
        buf[..n]
            .iter()
            .filter_map(|a| {
                if let ActionKind::Bet(x) = a.kind {
                    Some(x)
                } else {
                    None
                }
            })
            .collect()
    }

    fn find_bet(buf: &[Action], n: usize, want: f32) -> bool {
        bets(buf, n).iter().any(|&b| (b - want).abs() < 1e-3)
    }

    /// C1.5 regression: BB raising over an SB limp uses
    /// `street_bets[SB] + pot * frac` as the raise TOTAL, not the old
    /// `to_call + pot * frac`. Postflop these coincide.
    #[test]
    fn bb_raise_over_limp_includes_sb_street_bet() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Call,
        }); // SB limp
            // BB: street_bets == 2, opp_bet == 2, pot == 4
        assert_eq!(s.actor, 1);
        assert_eq!(s.street_bets[1], 2.0);
        assert_eq!(s.street_bets[0], 2.0);
        assert_eq!(s.pot, 4.0);

        let mut buf: [Action; 8] = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = s.legal_actions_into(&mut buf);

        // Expected raise totals: opp_bet(2) + pot(4) * frac.
        for frac in SIZINGS {
            let want = 2.0 + 4.0 * frac;
            assert!(
                find_bet(&buf, n, want),
                "missing raise total {} for frac {}; bets={:?}",
                want,
                frac,
                bets(&buf, n)
            );
        }

        // Old buggy form: to_call(0) + pot(4) * frac would give 1.6/3.2/6.4.
        // None of those should appear (they collide with nothing else here).
        for frac in SIZINGS {
            let buggy = 0.0 + 4.0 * frac;
            assert!(
                !find_bet(&buf, n, buggy),
                "old buggy raise total {} still present; bets={:?}",
                buggy,
                bets(&buf, n)
            );
        }
    }

    /// C1.5: SB re-raises over BB's open. Expected totals:
    ///   opp_bet + pot * frac
    ///   = BB_street_bet + (SB_street + BB_street) * frac
    #[test]
    fn sb_raise_over_bb_open_includes_both_street_bets() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        // SB limps (street_bets[0] = 2)
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Call,
        });
        // BB raises to 6 (legal: BB street_bets was 2, +4 more = 6)
        s.apply_action_in_place(&Action {
            player: 1,
            kind: ActionKind::Bet(6.0),
        });

        // actor = SB. street_bets[0] == 2, street_bets[1] == 6, pot == 8.
        assert_eq!(s.actor, 0);
        assert_eq!(s.street_bets[0], 2.0);
        assert_eq!(s.street_bets[1], 6.0);
        assert_eq!(s.pot, 8.0);

        let mut buf: [Action; 8] = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = s.legal_actions_into(&mut buf);

        for frac in SIZINGS {
            let want = 6.0 + 8.0 * frac; // opp_bet + pot * frac
            assert!(
                find_bet(&buf, n, want),
                "missing re-raise total {} for frac {}; bets={:?}",
                want,
                frac,
                bets(&buf, n)
            );
        }
    }

    /// C1.5: postflop, street_bets[actor] == 0, so check-branch totals
    /// are identical to pre-C1.5 behaviour. Regression guard.
    #[test]
    fn postflop_check_sizings_unchanged() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Call,
        });
        s.apply_action_in_place(&Action {
            player: 1,
            kind: ActionKind::Check,
        });
        s.advance_street_in_place(&[0, 1, 2]);

        assert_eq!(s.street_bets[1], 0.0);
        assert_eq!(s.pot, 4.0);

        let mut buf: [Action; 8] = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = s.legal_actions_into(&mut buf);

        for frac in SIZINGS {
            let want = 0.0 + 4.0 * frac;
            assert!(
                find_bet(&buf, n, want),
                "postflop check sizings changed: want {}, bets={:?}",
                want,
                bets(&buf, n)
            );
        }
    }

    /// C1.5 + C1: applying every offered Bet leaves street_bets == the
    /// requested total and stacks reduce by exactly (total - prior).
    #[test]
    fn applying_offered_bets_matches_total_semantics() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Call,
        });
        // Fresh state for each offered non-all-in bet
        let mut buf: [Action; 8] = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = s.legal_actions_into(&mut buf);
        for a in buf[..n].iter() {
            if let ActionKind::Bet(total) = a.kind {
                if total >= 200.0 - 1e-3 {
                    continue;
                } // skip all-in
                let mut s2 = s.clone();
                let prior = s2.street_bets[1];
                let stack_before = s2.stacks[1];
                s2.apply_action_in_place(a);
                assert!(
                    (s2.street_bets[1] - total).abs() < 1e-3,
                    "street_bets[1]={} != requested total {}",
                    s2.street_bets[1],
                    total
                );
                let expected_stack = stack_before - (total - prior);
                assert!(
                    (s2.stacks[1] - expected_stack).abs() < 1e-2,
                    "stack math mismatch: {} vs {}",
                    s2.stacks[1],
                    expected_stack
                );
            }
        }
    }
}
