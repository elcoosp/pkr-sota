use pkr_contracts::Evaluator;

/// Feature flag: when true, the traverser and every hash consumer
/// should use `history_signature_v2()` instead of `history_signature()`.
///
/// Flipping this constant to `true` is a SEMANTIC CHANGE — it changes
/// infoset identity. Before flipping:
///   1. Wipe every checkpoint in the tree (rule 0.1).
///   2. Bump the blueprint format version (r3 F2, format v4).
///   3. Confirm the capacity math in r3 P6 says v16 fits.
///
/// Default: OFF. The v2 helpers are landed and tested but not wired
/// into the traverser, so this commit is a no-op for training.
/// Signature v2 adds SPR bucket and (optionally) last-bet-fraction to
/// the infoset hash. Tested 2026-09-25: enabling it produced 3.6x more
/// infosets, and 20M eval jumped to 11170 mbb vs 5735 baseline. The
/// extra dimensions split strategically-similar situations into
/// distinct infosets faster than the fixed iteration budget can fill
/// them — same failure mode as T2.2 (finer discrete splits).
///
/// The code path is kept (fingerprint still has `sig_version`, the v2
/// function still exists) but production default is OFF. Do not enable
/// without a corresponding 10-100x iteration budget increase.
pub const SIG_V2_STREET_MONEY: bool = false;

/// Version tag stored in bits 60..64 of `history_signature_v2()`.
pub const SIG_V2_VERSION: u64 = 2;

/// F3: enable the size-aware signature `history_signature_v3`.
///
/// When true, `infoset_signature_into` emits 8 bytes of v3 signature:
/// street, current-street action-bucket sequence, street-start pot
/// class, and total raises. When false (default), it emits the current
/// 4-byte v1 signature.
///
/// Turning this on invalidates every existing checkpoint: the abstract
/// game changes because the infoset key can now distinguish bet sizes
/// and pot classes. Retrain or `--fresh`.
pub const SIG_V3_SIZE_AWARE: bool = false;

/// Version tag written into the top 4 bits of the v3 signature.
pub const SIG_V3_VERSION: u64 = 3;

/// When `SIG_V2_STREET_MONEY` is on, controls whether the
/// `last_bet_fraction_bucket` bits (24..28) participate in the hash.
///
/// `false` (SPR-only) = capacity growth ≤ 6×; recommended first
/// deployable config per r3 P6, because it stays well under the
/// 16 GB memory budget without needing the 72 B/infoset variant.
/// `true` = full v2 with ≤ 36× growth; flip once P5 lands and the
/// measured infoset count from an SPR-only run is known.
pub const SIG_V2_INCLUDE_LBF: bool = false;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Action {
    pub player: usize,
    pub kind: ActionKind,
}

/// A compact record of what changed in the state so we can undo an action.
#[derive(Debug, Clone, Copy)]
pub struct UndoRecord {
    /// Packed to u8: actor ∈ {0,1}, history_len ≤ 48, board_len ≤ 5.
    /// P2: shrinking these from usize saves 24 B per push_undo call,
    /// which runs on every action.
    actor: u8,
    street: Street,
    pot: f32,
    stacks: [f32; 2],
    street_bets: [f32; 2],
    total_invested: [f32; 2],
    actions_this_street: u8,
    raises_this_street: u8,
    total_raises: u8,
    history_len: u8,
    abstract_history_len: u8,
    board_len: u8,
    folded: [bool; 2],
    /// F3: pot at the moment the current street began. Used by the
    /// size-aware signature (`history_signature_v3`) to describe
    /// which pot class the current betting round is playing for.
    /// Restored by `undo_action`.
    street_start_pot: f32,
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
    pub history: [Action; 48], // fixed array for action history
    pub history_len: u8,
    pub folded: [bool; 2],
    pub actions_this_street: u8,
    pub raises_this_street: u8,
    /// Total raises across the whole hand. Maintained incrementally in
    /// `apply_action_internal` and restored in `undo_action`, so that
    /// `history_signature()` is O(1) instead of scanning the history
    /// array on every traverser node visit. Byte-identical semantics
    /// to the previous loop (counts every `Bet` action regardless of
    /// whether it moved chips).
    pub total_raises: u8,
    pub abstract_history: [u8; 48], // abstract action buckets
    pub abstract_history_len: u8,
    /// F3: pot size at the moment the current street started. Set on
    /// `advance_street_in_place` (from the pre-advance pot) and at
    /// hand start (to the blinds' forced total). Never reset within a
    /// street. Off-by-default consumer; the size-aware signature is
    /// gated on `SIG_V3_SIZE_AWARE`.
    pub street_start_pot: f32,
    pub undo_stack: [UndoRecord; 48],
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
            }; 48],
            history_len: 0,
            folded: [false; 2],
            actions_this_street: 0,
            raises_this_street: 0,
            total_raises: 0,
            abstract_history: [0u8; 48],
            abstract_history_len: 0,
            // F3: the pot at preflop start is the two forced bets.
            street_start_pot: sb + bb,
            undo_stack: [UndoRecord {
                actor: 0,
                street: Street::Preflop,
                pot: 0.0,
                stacks: [0.0; 2],
                street_bets: [0.0; 2],
                total_invested: [0.0; 2],
                actions_this_street: 0,
                raises_this_street: 0,
                total_raises: 0,
                history_len: 0,
                abstract_history_len: 0,
                board_len: 0,
                folded: [false; 2],
                street_start_pot: 0.0,
            }; 48],
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
        // B7: single source of truth. The allocating wrapper delegates to
        // the training-path implementation so fuzz tests validate the same
        // action set the CFR traversal actually uses (raise cap, all-in dedup).
        let mut buf = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = self.legal_actions_into(&mut buf);
        buf[..n].to_vec()
    }

    /// Non-allocating variant of `legal_actions`. Writes into `out` and
    /// returns the count. Reused by the CFR traversal to avoid one heap
    /// allocation per node visit — the allocator is the dominant
    /// multithread bottleneck otherwise. Callers must provide a buffer of
    /// at least 8 slots; the current action space tops out at 6.
    #[inline]
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
                for &frac in &crate::abstraction::BET_SIZINGS {
                    if n >= 8 {
                        break;
                    }
                    let bet = base + pot * frac; // C1.5
                    let chips_needed = bet - base;
                    if chips_needed <= self.stacks[self.actor] && self.opp_can_respond() {
                        out[n] = Action {
                            player: self.actor,
                            kind: ActionKind::Bet(bet),
                        };
                        n += 1;
                    }
                }
            }
            // F6: jam is always legal while opponent can respond.
            if n < 8 && self.stacks[self.actor] > 0.0 && self.opp_can_respond() {
                let all_in_amount = self.stacks[self.actor] + self.street_bets[self.actor];
                let already_offered = (0..n).any(|i| {
                    matches!(out[i].kind, ActionKind::Bet(b) if (b - all_in_amount).abs() < 1e-9)
                });
                if !already_offered {
                    out[n] = Action {
                        player: self.actor,
                        kind: ActionKind::Bet(all_in_amount),
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
                // F6: true NLHE min-raise-to is
                //     opp_bet + (size of the last bet or raise)
                //
                // Two cases:
                //   * No voluntary raise this street yet. The opponent's
                //     current bet IS the last aggressive action. Preflop
                //     that's the forced BB; postflop it's their own first
                //     bet. Either way, min-raise-to = 2 * opp_bet.
                //   * A raise already happened this street. The last
                //     raise delta is (opp_bet - our_bet), because our_bet
                //     was the previous raise level. min-raise-to =
                //     opp_bet + (opp_bet - our_bet) = 2*opp_bet - our_bet.
                //
                // Before this fix `opp_bet + pot * frac` could produce
                // sizes below the legal minimum (audit F6), which real
                // engines reject and which inflated the abstract game.
                let min_raise_to = if self.raises_this_street == 0 {
                    2.0 * opp_bet
                } else {
                    2.0 * opp_bet - self.street_bets[self.actor]
                };
                for &frac in &crate::abstraction::BET_SIZINGS {
                    if n >= 8 {
                        break;
                    }
                    let raise = (opp_bet + pot * frac).max(min_raise_to); // C1.5
                    let chips_needed = raise - self.street_bets[self.actor];
                    if chips_needed <= self.stacks[self.actor] && self.opp_can_respond() {
                        out[n] = Action {
                            player: self.actor,
                            kind: ActionKind::Bet(raise),
                        };
                        n += 1;
                    }
                }
            }
            // F6: the all-in is always legal while the opponent can
            // still respond, even after MAX_RAISES_PER_STREET. Without
            // this, a legitimate 4-bet jam in a 3-raise-capped spot is
            // silently disallowed. Moved out of `can_raise`.
            if n < 8 && self.stacks[self.actor] > 0.0 && self.opp_can_respond() {
                let all_in_amount = self.stacks[self.actor] + self.street_bets[self.actor];
                let already_offered = (0..n).any(|i| {
                    matches!(out[i].kind, ActionKind::Bet(b) if (b - all_in_amount).abs() < 1e-9)
                });
                if !already_offered {
                    out[n] = Action {
                        player: self.actor,
                        kind: ActionKind::Bet(all_in_amount),
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
    #[inline]
    pub fn history_signature(&self) -> u32 {
        // C4a: `total_raises` maintained incrementally in
        // apply_action_internal; restored in undo_action. This replaces
        // the previous O(history_len) loop that ran on every traverser
        // node visit. Byte-identical semantics.
        let raises: u8 = self.total_raises;
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
            // C5a: the overflow branch used to silently return, which
            // popped the wrong record on the next undo and silently
            // corrupted traversal state. With the array now at 48
            // slots (worst legal hand ~33 pushes), an overflow means
            // something is genuinely wrong — panic so we catch it.
            panic!(
                "undo stack overflow: {} pushes into {} slots",
                self.undo_len,
                self.undo_stack.len()
            );
        }
        let record = UndoRecord {
            actor: self.actor as u8,
            street: self.street,
            pot: self.pot,
            stacks: self.stacks,
            street_bets: self.street_bets,
            total_invested: self.total_invested,
            actions_this_street: self.actions_this_street,
            raises_this_street: self.raises_this_street,
            total_raises: self.total_raises,
            history_len: self.history_len,
            abstract_history_len: self.abstract_history_len,
            board_len: self.board_len,
            folded: self.folded,
            street_start_pot: self.street_start_pot,
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
    ///
    /// # `Action::player` is documentary
    ///
    /// The action is applied to `self.actor`, **not** to
    /// `action.player`. The `player` field is set by
    /// [`legal_actions_into`](Self::legal_actions_into) to the current
    /// actor and exists so downstream code (bot logs, replay, the
    /// wrapper in `RuntimeSession`) can see whose action it is without
    /// inspecting the state. It is not validated.
    ///
    /// Callers that construct `Action` manually should set
    /// `player = state.actor` to avoid confusing later inspection.
    /// Tests that hardcode `player: 0` for a P1 action still apply
    /// the action to the correct seat (self.actor); the field is just
    /// misleading.
    fn apply_action_internal(&mut self, action: &Action) {
        let actor = self.actor;
        // C3: snapshot pre-action scalars for bucket computation below.
        let pre_stacks = self.stacks[actor];
        let pre_street_bet = self.street_bets[actor];
        let pre_opp_street_bet = self.street_bets[1 - actor];
        let pre_pot = self.pot;
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
                self.total_raises = self.total_raises.saturating_add(1);
            }
        }

        // Record abstract action bucket. C3: derived from PRE-action
        // state (matches the traverser's convention).
        //
        // `abstract_history` is currently write-only, so the previous
        // post-action computation had no observable effect. C3 aligns
        // both conventions to remove the trap.
        let bucket = crate::abstraction::action_bucket(
            &action.kind,
            pre_stacks,
            pre_street_bet,
            pre_opp_street_bet,
            pre_pot,
        );
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
        self.actor = rec.actor as usize;
        self.street = rec.street;
        self.pot = rec.pot;
        self.stacks = rec.stacks;
        self.street_bets = rec.street_bets;
        self.total_invested = rec.total_invested;
        self.actions_this_street = rec.actions_this_street;
        self.raises_this_street = rec.raises_this_street;
        self.total_raises = rec.total_raises;
        self.history_len = rec.history_len;
        self.board_len = rec.board_len;
        self.folded = rec.folded;
        self.abstract_history_len = rec.abstract_history_len;
        self.street_start_pot = rec.street_start_pot;
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

    #[inline]
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
        // F3: the pot as of now is the pot at the start of the next
        // street. `push_undo` already captured the previous value, so
        // an `undo_action` restores it.
        self.street_start_pot = self.pot;
    }
}

// (C3) `abstract_action_index_static` deleted; use
// `crate::abstraction::action_bucket` instead.

impl GameState {
    /// True if the opponent has chips behind and can therefore respond
    /// to a Bet/Raise. When false, only Check/Call/Fold are legal for
    /// the current actor — any Bet would be dead money that the all-in
    /// player cannot call, and poker rules return it to the bettor at
    /// showdown anyway.
    ///
    /// C5b: gating bets on this removes the dead-money subtree from
    /// every all-in runout, saving ~2x nodes on those lines and
    /// eliminating a class of nonsense eval-harness action sequences.
    #[inline]
    pub fn opp_can_respond(&self) -> bool {
        self.stacks[1 - self.actor] > 0.0
    }

    /// Coarse bucket of the fraction of pot represented by the bet
    /// currently being faced. Returns 0 when no bet is faced.
    ///
    /// Buckets (r3 C4):
    ///   0 = no bet / check
    ///   1 = very small (<0.35 pot)
    ///   2 = ~half pot (0.35..0.75)
    ///   3 = ~pot (0.75..1.30)
    ///   4 = ~2x overbet (1.30..2.20)
    ///   5 = jam-scale (>= 2.20)
    pub fn last_bet_fraction_bucket(&self) -> u8 {
        let to_call = self.bet_to_call();
        if to_call <= 0.0 {
            return 0;
        }
        let pot_before = (self.pot - to_call).max(1.2);
        let frac = to_call / pot_before;
        if frac < 0.35 {
            1
        } else if frac < 0.75 {
            2
        } else if frac < 1.30 {
            3
        } else if frac < 2.20 {
            4
        } else {
            5
        }
    }

    /// Coarse bucket of pot / effective stack.
    ///   0 = SPR < 0.5
    ///   1 = 0.5..1.0
    ///   2 = 1.0..2.0
    ///   3 = 2.0..4.0
    ///   4 = 4.0..8.0
    ///   5 = >= 8.0
    pub fn spr_bucket(&self) -> u8 {
        let a = self.stacks[self.actor];
        let b = self.stacks[1 - self.actor];
        let eff = a.min(b).max(0.001);
        let spr = self.pot / eff;
        if spr < 0.5 {
            0
        } else if spr < 1.0 {
            1
        } else if spr < 2.0 {
            2
        } else if spr < 4.0 {
            3
        } else if spr < 8.0 {
            4
        } else {
            5
        }
    }

    /// Signature v2. Layout (u64):
    ///   bits  0..24 : legacy v1 (actions_this_street | raises<<8 | last_was_bet<<16)
    ///   bits 24..28 : last_bet_fraction_bucket (0..=5)
    ///   bits 28..32 : spr_bucket (0..=5)
    ///   bits 60..64 : version tag = SIG_V2_VERSION
    ///
    /// Not yet wired into the traverser. See `SIG_V2_STREET_MONEY`.
    /// F3: size-aware signature.
    ///
    /// Layout (low -> high bits):
    ///
    ///   bits  0.. 3  street (0..3)
    ///   bits  3..10  current-street action-bucket sequence (up to 7
    ///                actions, 3 bits each — see `BET_SIZINGS` doc)
    ///   bits 10..14  current-street action count (0..7)
    ///   bits 14..19  street-start pot class (log2 BB, 0..8)
    ///   bits 19..22  total_raises this street (0..7)
    ///   bits 60..64  version tag = SIG_V3_VERSION
    ///
    /// The pot class is `floor(log2(pot_bb)).min(8)` where `pot_bb` is
    /// the street-start pot expressed in big blinds. So pot sizes 1, 2,
    /// 4, 8, ..., 256+ bb map to classes 0..8. That is a coarse but
    /// real distinction: a 2 bb pot plays differently from a 100 bb
    /// pot.
    ///
    /// The action buckets here come from `abstract_history` (the same
    /// per-action bucket the traversal computes), so the current-street
    /// sequence reuses state the engine already maintains.
    pub fn history_signature_v3(&self) -> u64 {
        let n_actions = (self.actions_this_street as usize).min(7);
        let end = self.abstract_history_len as usize;
        let start = end.saturating_sub(n_actions);
        let mut seq: u64 = 0;
        for (i, &b) in self.abstract_history[start..end].iter().enumerate() {
            let b3 = (b as u64) & 0x7;
            seq |= b3 << (i * 3);
        }
        let street = (self.street as u64) & 0x3;
        let pot_bb = (self.street_start_pot / 2.0).max(1.0);
        let pot_class = (pot_bb.log2().floor() as u64).min(8) & 0xF;
        let raises = (self.total_raises as u64).min(7) & 0x7;

        (street)
            | (seq << 3)
            | ((n_actions as u64 & 0x7) << 10)
            | (pot_class << 14)
            | (raises << 19)
            | (SIG_V3_VERSION << 60)
    }

    pub fn history_signature_v2(&self) -> u64 {
        let v1 = self.history_signature() as u64;
        let lbf = if SIG_V2_INCLUDE_LBF {
            self.last_bet_fraction_bucket() as u64
        } else {
            0
        };
        let spr = self.spr_bucket() as u64;
        v1 | (lbf << 24) | (spr << 28) | (SIG_V2_VERSION << 60)
    }

    /// Fill `out` with the current infoset signature bytes and return
    /// the length (4 for v1, 8 for v2). The length matters: FNV-1a
    /// hashes the exact byte slice, so changing the length changes
    /// every hash downstream.
    ///
    /// `SIG_V2_STREET_MONEY` dispatches between the two. When false
    /// (default), output is byte-identical to `history_signature()
    /// .to_le_bytes()`, so training is unaffected.
    #[inline]
    pub fn infoset_signature_into(&self, out: &mut [u8; 8]) -> usize {
        // F3 dispatch: v3 takes precedence when enabled.
        if SIG_V3_SIZE_AWARE {
            out.copy_from_slice(&self.history_signature_v3().to_le_bytes());
            8
        } else if SIG_V2_STREET_MONEY {
            out.copy_from_slice(&self.history_signature_v2().to_le_bytes());
            8
        } else {
            out[..4].copy_from_slice(&self.history_signature().to_le_bytes());
            4
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

    /// Audit F10 (Task 10): a short all-in records actual chips contributed,
    /// not the requested total.
    #[test]
    fn short_allin_records_actual_chips() {
        // Start stack 10: after blinds, stacks = [9, 8], street_bets = [1, 2], pot = 3.
        let mut s = GameState::new(10.0, 1.0, 2.0);
        s.set_hole_cards([0, 1], [2, 3]);
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Bet(500.0),
        });
        // Player 0 only has 9 behind; street_bets must equal what was actually
        // contributed (1 + 9 = 10), not the requested 500.
        assert_eq!(s.street_bets[0], 10.0);
        assert_eq!(s.stacks[0], 0.0);
        assert_eq!(s.pot, 12.0);
        assert_eq!(s.total_invested[0], 10.0);
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
        let bucket = crate::abstraction::action_bucket(
            &all_in.kind,
            s.stacks[s.actor],
            s.street_bets[s.actor],
            s.street_bets[1 - s.actor],
            s.pot,
        );
        assert_eq!(bucket, 5, "all-in total must bucket as 5, not 4");
    }
}

#[cfg(test)]
mod c1_5_tests {
    use super::*;

    // Sizings currently used by `legal_actions*` after T0.2.
    const SIZINGS: [f32; 3] = crate::abstraction::BET_SIZINGS;

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
        } // NOTE: the old buggy total for a given sizing can numerically
          // collide with the correct total for a different sizing now
          // that BET_SIZINGS = [0.5, 1.0, 2.0]. Specifically,
          // buggy(1.0x) == correct(0.5x) == 4.0 when opp_bet=2, pot=4.
          // So we do not assert the absence of any particular buggy
          // value; the positive assertion above plus the C3 bucket test
          // (`anchors_map_to_distinct_buckets_preflop_raise_over_limp`)
          // jointly prove the new formula is in use.
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

#[cfg(test)]
mod c4b_tests {
    use super::*;

    #[test]
    fn v2_version_tag_set() {
        let s = GameState::new(200.0, 1.0, 2.0);
        let v2 = s.history_signature_v2();
        assert_eq!(
            v2 >> 60,
            SIG_V2_VERSION,
            "version tag must occupy bits 60..64"
        );
    }

    #[test]
    fn v2_low_24_bits_match_v1() {
        let s = GameState::new(200.0, 1.0, 2.0);
        let v1 = s.history_signature() as u64;
        let v2 = s.history_signature_v2();
        assert_eq!(
            v2 & 0x00FF_FFFF,
            v1 & 0x00FF_FFFF,
            "low 24 bits must match v1"
        );
    }

    #[test]
    fn last_bet_fraction_bucket_is_zero_when_no_bet() {
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
        assert_eq!(s.bet_to_call(), 0.0);
        assert_eq!(s.last_bet_fraction_bucket(), 0);
    }

    #[test]
    fn last_bet_fraction_bucket_distinguishes_sizes() {
        let setup = |bet: f32| -> GameState {
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
            s.apply_action_in_place(&Action {
                player: 1,
                kind: ActionKind::Check,
            });
            s.apply_action_in_place(&Action {
                player: 0,
                kind: ActionKind::Bet(bet),
            });
            s
        };
        // After limp/check, pot = 4. Villain (SB, actor 0 postflop) bets X.
        assert_eq!(setup(2.0).last_bet_fraction_bucket(), 2, "0.5x pot");
        assert_eq!(setup(4.0).last_bet_fraction_bucket(), 3, "1.0x pot");
        assert_eq!(setup(8.0).last_bet_fraction_bucket(), 4, "2.0x pot");
        assert_eq!(setup(10.0).last_bet_fraction_bucket(), 5, ">2.2x pot");
    }

    #[test]
    fn spr_bucket_ranges() {
        let s = GameState::new(200.0, 1.0, 2.0);
        // stacks [199, 198], effective 198, pot 3 → SPR ≈ 0.015 → bucket 0
        assert_eq!(s.spr_bucket(), 0, "fresh preflop SPR is near zero");
        let s = GameState::new(2000.0, 1.0, 2.0);
        // effective ≈ 1998, pot 3 → SPR ≈ 0.0015 → bucket 0 too
        assert_eq!(s.spr_bucket(), 0);
        // Synthetic: force pot/stack ratio.
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.pot = 100.0;
        s.stacks = [150.0, 150.0];
        assert_eq!(s.spr_bucket(), 1, "SPR 0.667 -> bucket 1");
        s.pot = 400.0;
        assert_eq!(s.spr_bucket(), 3, "SPR 2.67 -> bucket 3");
        s.pot = 1600.0;
        assert_eq!(s.spr_bucket(), 5, "SPR 10.7 -> bucket 5");
    }

    #[test]
    fn v2_distinguishes_bet_sizes() {
        let setup = |bet: f32| -> GameState {
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
            s.apply_action_in_place(&Action {
                player: 1,
                kind: ActionKind::Check,
            });
            s.apply_action_in_place(&Action {
                player: 0,
                kind: ActionKind::Bet(bet),
            });
            s
        };
        let half = setup(2.0);
        let full = setup(4.0);
        let over = setup(8.0);

        // v1 always collides — that is the bug we are fixing.
        assert_eq!(half.history_signature(), full.history_signature());
        assert_eq!(half.history_signature(), over.history_signature());

        // v2 only distinguishes bet sizes if the LBF bits are included.
        // With SIG_V2_INCLUDE_LBF = false (SPR-only config, the current
        // production default), the LBF bucket does not participate in
        // the hash and these three states collide under v2 as well.
        if SIG_V2_INCLUDE_LBF {
            assert_ne!(half.history_signature_v2(), full.history_signature_v2());
            assert_ne!(full.history_signature_v2(), over.history_signature_v2());
            assert_ne!(half.history_signature_v2(), over.history_signature_v2());
        } else {
            assert_eq!(
                half.history_signature_v2(),
                full.history_signature_v2(),
                "with INCLUDE_LBF off, v2 does not distinguish bet sizes (expected)"
            );
            assert_eq!(half.history_signature_v2(), over.history_signature_v2());
        }
    }
}

#[cfg(test)]
mod c4c_tests {
    use super::*;

    /// With the flag off, `infoset_signature_into` must produce exactly
    /// the same 4 bytes as `history_signature().to_le_bytes()`. This
    /// pins the flag-off no-op contract.
    #[test]
    fn flag_off_bytes_match_v1() {
        if SIG_V2_STREET_MONEY {
            eprintln!("SKIP: SIG_V2_STREET_MONEY is on; not a flag-off test");
            return;
        }
        let s = GameState::new(200.0, 1.0, 2.0);
        let mut buf = [0u8; 8];
        let n = s.infoset_signature_into(&mut buf);
        assert_eq!(n, 4, "flag off must return 4 bytes");
        assert_eq!(
            &buf[..4],
            &s.history_signature().to_le_bytes()[..],
            "flag-off bytes must match v1 exactly"
        );
    }

    /// With the flag on, returns 8 bytes and the low 24 match v1.
    /// Skipped when the flag is off so the suite stays green in either
    /// configuration.
    #[test]
    fn flag_on_bytes_are_8_bytes_with_v2_version() {
        if !SIG_V2_STREET_MONEY {
            eprintln!("SKIP: SIG_V2_STREET_MONEY is off; run after C4d flip");
            return;
        }
        let s = GameState::new(200.0, 1.0, 2.0);
        let mut buf = [0u8; 8];
        let n = s.infoset_signature_into(&mut buf);
        assert_eq!(n, 8, "flag on must return 8 bytes");
        let as_u64 = u64::from_le_bytes(buf);
        assert_eq!(as_u64 >> 60, SIG_V2_VERSION);
    }

    /// `SIG_V2_INCLUDE_LBF` must actually gate the lbf bits.
    #[test]
    fn include_lbf_flag_gates_lbf_bits() {
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
        s.apply_action_in_place(&Action {
            player: 1,
            kind: ActionKind::Check,
        });
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Bet(4.0),
        });

        let v2 = s.history_signature_v2();
        let lbf = (v2 >> 24) & 0xF;
        if SIG_V2_INCLUDE_LBF {
            assert!(lbf > 0, "lbf bits should be non-zero with INCLUDE_LBF on");
        } else {
            assert_eq!(lbf, 0, "lbf bits must be zero with INCLUDE_LBF off");
        }
    }
}

#[cfg(test)]
mod c5a_tests {
    use super::*;

    /// C5a: worst-case hand (three streets of max-raise wars) must
    /// leave undo_len < 48. This is the "48 slots is enough" claim,
    /// made executable.
    #[test]
    fn worst_case_hand_fits_undo_stack() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.set_hole_cards([0, 1], [2, 3]);
        // Play a maximum-length sequence: on every street, raise/re-raise
        // until the street caps out (MAX_RAISES_PER_STREET = 3 per player).
        // We do it crudely: apply the largest legal bet until the street
        // completes, then advance.
        for _street in 0..4 {
            let mut safety = 0;
            while !s.is_street_complete() && !s.is_terminal() && safety < 30 {
                safety += 1;
                let mut buf: [Action; 8] = [Action {
                    player: 0,
                    kind: ActionKind::Fold,
                }; 8];
                let n = s.legal_actions_into(&mut buf);
                if n == 0 {
                    break;
                }
                // Pick the largest non-all-in Bet, else call/check.
                let mut chosen = buf[0];
                let mut best_amt = -1.0f32;
                for a in buf.iter().take(n) {
                    if let ActionKind::Bet(amt) = a.kind {
                        if amt > best_amt && amt < 199.0 {
                            best_amt = amt;
                            chosen = *a;
                        }
                    }
                }
                if best_amt < 0.0 {
                    // No raise available; call/check.
                    for a in buf.iter().take(n) {
                        if matches!(a.kind, ActionKind::Call | ActionKind::Check) {
                            chosen = *a;
                            break;
                        }
                    }
                }
                s.apply_action_in_place(&chosen);
            }
            if s.is_terminal() {
                break;
            }
            s.advance_street_in_place(&[10, 11, 12]);
        }
        assert!(
            (s.undo_len as usize) < 48,
            "worst-case hand used {} undo slots; 48 was supposed to suffice",
            s.undo_len
        );
    }

    /// C5a: pushing more than 48 times must panic, not silently drop.
    #[test]
    #[should_panic(expected = "undo stack overflow")]
    fn push_undo_panics_on_overflow() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        // Apply many no-op actions (Check) directly via push_undo path
        // by using advance_street_in_place which also pushes.
        for _ in 0..50 {
            s.apply_action_in_place(&Action {
                player: s.actor,
                kind: ActionKind::Check,
            });
        }
    }
}

#[cfg(test)]
mod c5b_tests {
    use super::*;

    /// C5b: facing an all-in preflop, only fold/call are legal.
    #[test]
    fn preflop_facing_all_in_offers_only_fold_or_call() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        // SB shoves for 200 total (= stack + street_bets).
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Bet(200.0),
        });
        assert_eq!(s.actor, 1, "BB acts after SB shove");
        assert_eq!(s.stacks[0], 0.0, "SB is all-in");

        let mut buf: [Action; 8] = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = s.legal_actions_into(&mut buf);
        for a in buf.iter().take(n) {
            assert!(
                matches!(a.kind, ActionKind::Fold | ActionKind::Call),
                "unexpected action {:?} when SB is all-in",
                a.kind
            );
        }
        assert!(buf[..n].iter().any(|a| matches!(a.kind, ActionKind::Fold)));
        assert!(buf[..n].iter().any(|a| matches!(a.kind, ActionKind::Call)));
    }

    /// C5b: post-flop with opp all-in, only check is offered (no bets).
    #[test]
    fn postflop_opp_all_in_offers_only_check() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Bet(200.0),
        });
        s.apply_action_in_place(&Action {
            player: 1,
            kind: ActionKind::Call,
        });
        s.advance_street_in_place(&[0, 1, 2]);
        assert_eq!(s.stacks[0], 0.0, "SB remains all-in postflop");
        assert_eq!(s.street_bets[0], 0.0, "street bets reset");

        let mut buf: [Action; 8] = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = s.legal_actions_into(&mut buf);
        for a in buf.iter().take(n) {
            assert!(
                matches!(
                    a.kind,
                    ActionKind::Check | ActionKind::Fold | ActionKind::Call
                ),
                "unexpected action {:?} postflop when SB is all-in",
                a.kind
            );
        }
        // Specifically no Bet.
        assert!(
            !buf[..n]
                .iter()
                .any(|a| matches!(a.kind, ActionKind::Bet(_))),
            "no Bet should be offered postflop vs all-in"
        );
    }

    /// C5b: `opp_can_respond` flips correctly.
    #[test]
    fn opp_can_respond_flips() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        assert!(s.opp_can_respond(), "BB has chips, SB (actor 0) can raise");
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Bet(200.0),
        });
        assert!(!s.opp_can_respond(), "SB all-in, BB (actor 1) cannot raise");
    }
}

#[cfg(test)]
mod invariants_tests {
    use super::*;
    use rand::rngs::SmallRng;
    use rand::{RngExt, SeedableRng};

    /// Snapshot the logical game state (excludes undo stack, history
    /// content, cache fields that legitimately differ across undo).
    #[allow(clippy::type_complexity)]
    fn logical_snapshot(
        s: &GameState,
    ) -> (
        f32,
        [f32; 2],
        [f32; 2],
        [f32; 2],
        usize,
        u8,
        u8,
        Street,
        [bool; 2],
        u8,
    ) {
        (
            s.pot,
            s.stacks,
            s.total_invested,
            s.street_bets,
            s.actor,
            s.actions_this_street,
            s.raises_this_street,
            s.street,
            s.folded,
            s.board_len,
        )
    }

    fn deal_runout(rng: &mut SmallRng) -> [u8; 5] {
        let mut deck: [u8; 52] = core::array::from_fn(|i| i as u8);
        for i in 0..9 {
            let k = i + rng.random_range(0..(52 - i));
            deck.swap(i, k);
        }
        [deck[4], deck[5], deck[6], deck[7], deck[8]]
    }

    /// R-1: applying a single action and undoing it must restore the
    /// logical game state exactly. This is the property the traverser's
    /// backtracking relies on for every node visit.
    #[test]
    fn apply_undo_roundtrip_preserves_state() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        let mut buf: [Action; 8] = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = s.legal_actions_into(&mut buf);
        assert!(n > 0);

        for a in buf.iter().take(n) {
            let before = logical_snapshot(&s);
            let undo_len_before = s.undo_len;
            s.apply_action_in_place(a);
            s.undo_action();
            let after = logical_snapshot(&s);
            assert_eq!(before, after, "apply+undo diverged for action {:?}", a.kind);
            assert_eq!(s.undo_len, undo_len_before, "undo_len must round-trip");
        }
    }

    /// R-2: chips must be conserved. `total_invested[0] + total_invested[1]`
    /// must always equal `pot`. This is stricter than stacks+pot == const
    /// (which can drift in f32) because it holds by construction.
    #[test]
    fn chips_are_conserved_under_random_play() {
        let mut rng = SmallRng::seed_from_u64(0xC0FFEE);
        for hand in 0..500 {
            let mut s = GameState::new(200.0, 1.0, 2.0);
            let runout = deal_runout(&mut rng);
            let mut runout_idx = 0usize;
            let mut steps = 0u32;
            while !s.is_terminal() && steps < 60 {
                steps += 1;
                let mut buf: [Action; 8] = [Action {
                    player: 0,
                    kind: ActionKind::Fold,
                }; 8];
                let n = s.legal_actions_into(&mut buf);
                if n == 0 {
                    break;
                }
                let a = buf[rng.random_range(0..n)];
                s.apply_action_in_place(&a);

                // Invariant: total_invested sums to pot.
                let sum_inv = s.total_invested[0] + s.total_invested[1];
                assert!(
                    (sum_inv - s.pot).abs() < 1e-3,
                    "hand {}: total_invested sums to {} but pot is {}",
                    hand,
                    sum_inv,
                    s.pot
                );

                // Invariant: stacks + total_invested == start_stack (200).
                for p in 0..2 {
                    let total = s.stacks[p] + s.total_invested[p];
                    assert!(
                        (total - 200.0).abs() < 1e-3,
                        "hand {} player {}: stacks {} + invested {} != 200",
                        hand,
                        p,
                        s.stacks[p],
                        s.total_invested[p]
                    );
                }

                if s.is_street_complete() && s.street != Street::River {
                    let need = match s.street {
                        Street::Preflop => 3,
                        Street::Flop => 1,
                        Street::Turn => 1,
                        Street::River => 0,
                    };
                    if runout_idx + need > runout.len() {
                        break;
                    }
                    let cards = &runout[runout_idx..runout_idx + need];
                    s.advance_street_in_place(cards);
                    runout_idx += need;
                }
            }
        }
    }

    /// R-3: every action offered by `legal_actions_into` must apply
    /// without panicking. This is the "offer-and-apply" contract the
    /// traverser relies on.
    #[test]
    fn every_offered_action_applies_cleanly() {
        let mut rng = SmallRng::seed_from_u64(0xBADF00D);
        for _hand in 0..200 {
            let mut s = GameState::new(200.0, 1.0, 2.0);
            let runout = deal_runout(&mut rng);
            let mut runout_idx = 0usize;
            let mut steps = 0u32;
            while !s.is_terminal() && steps < 40 {
                steps += 1;
                let mut buf: [Action; 8] = [Action {
                    player: 0,
                    kind: ActionKind::Fold,
                }; 8];
                let n = s.legal_actions_into(&mut buf);
                for a in buf.iter().take(n) {
                    let mut s2 = s.clone();
                    s2.apply_action_in_place(a);
                    s2.undo_action();
                }
                if n == 0 {
                    break;
                }
                let a = buf[rng.random_range(0..n)];
                s.apply_action_in_place(&a);
                if s.is_street_complete() && s.street != Street::River {
                    let need = match s.street {
                        Street::Preflop => 3,
                        Street::Flop => 1,
                        Street::Turn => 1,
                        Street::River => 0,
                    };
                    if runout_idx + need > runout.len() {
                        break;
                    }
                    let cards = &runout[runout_idx..runout_idx + need];
                    s.advance_street_in_place(cards);
                    runout_idx += need;
                }
            }
        }
    }

    /// R-4: every offered Bet must be strictly greater than the actor's
    /// current street bet (a legal raise must move chips into the pot,
    /// never reduce street_bets or be a no-op).
    #[test]
    fn no_offered_bet_reduces_street_bets() {
        let mut rng = SmallRng::seed_from_u64(0x13579BDF);
        for _hand in 0..200 {
            let mut s = GameState::new(200.0, 1.0, 2.0);
            let runout = deal_runout(&mut rng);
            let mut runout_idx = 0usize;
            let mut steps = 0u32;
            while !s.is_terminal() && steps < 40 {
                steps += 1;
                let actor_street = s.street_bets[s.actor];
                let mut buf: [Action; 8] = [Action {
                    player: 0,
                    kind: ActionKind::Fold,
                }; 8];
                let n = s.legal_actions_into(&mut buf);
                for a in buf.iter().take(n) {
                    if let ActionKind::Bet(amt) = a.kind {
                        assert!(
                            amt > actor_street + 1e-4,
                            "offered Bet({}) not above actor street_bets {}",
                            amt,
                            actor_street
                        );
                    }
                }
                if n == 0 {
                    break;
                }
                let a = buf[rng.random_range(0..n)];
                s.apply_action_in_place(&a);
                if s.is_street_complete() && s.street != Street::River {
                    let need = match s.street {
                        Street::Preflop => 3,
                        Street::Flop => 1,
                        Street::Turn => 1,
                        Street::River => 0,
                    };
                    if runout_idx + need > runout.len() {
                        break;
                    }
                    let cards = &runout[runout_idx..runout_idx + need];
                    s.advance_street_in_place(cards);
                    runout_idx += need;
                }
            }
        }
    }

    /// R-5: no Bet/Raise offered when the opponent cannot respond
    /// (post-C5b contract).
    #[test]
    fn no_offered_bet_when_opponent_all_in() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        // SB shoves.
        s.apply_action_in_place(&Action {
            player: 0,
            kind: ActionKind::Bet(200.0),
        });
        assert_eq!(s.stacks[0], 0.0, "SB should be all-in");

        let mut buf: [Action; 8] = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = s.legal_actions_into(&mut buf);
        for a in buf.iter().take(n) {
            assert!(
                !matches!(a.kind, ActionKind::Bet(_)),
                "Bet offered when opponent is all-in: {:?}",
                a.kind
            );
        }
    }
}

/// B7: `legal_actions()` and `legal_actions_into()` must agree exactly.
/// Guards the single-source-of-truth refactor: if the allocating wrapper
/// ever diverges from the training path, the fuzz harness silently stops
/// validating the code the trainer runs.
#[cfg(test)]
mod b7_single_source_tests {
    use super::*;

    fn fresh_state() -> GameState {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.set_hole_cards([0, 1], [2, 3]);
        s
    }

    #[test]
    fn legal_actions_matches_legal_actions_into_at_start() {
        let s = fresh_state();
        let a = s.legal_actions();
        let mut buf = [Action {
            player: 0,
            kind: ActionKind::Fold,
        }; 8];
        let n = s.legal_actions_into(&mut buf);
        assert_eq!(a.len(), n, "counts differ: alloc={} into={}", a.len(), n);
        for i in 0..n {
            assert_eq!(a[i], buf[i], "action {i} differs");
        }
    }

    #[test]
    fn legal_actions_matches_legal_actions_into_after_raises() {
        // Drive several raises to exercise the MAX_RAISES_PER_STREET cap.
        let mut s = fresh_state();
        for _ in 0..5 {
            if s.is_terminal() {
                break;
            }
            let mut buf = [Action {
                player: 0,
                kind: ActionKind::Fold,
            }; 8];
            let n = s.legal_actions_into(&mut buf);
            if n == 0 {
                break;
            }
            // Pick the most aggressive legal action each iteration.
            let mut pick = 0usize;
            for (i, a) in buf[..n].iter().enumerate() {
                if let ActionKind::Bet(_) = a.kind {
                    pick = i;
                }
            }
            s.apply_action_in_place(&buf[pick]);
            let a = s.legal_actions();
            let mut buf2 = [Action {
                player: 0,
                kind: ActionKind::Fold,
            }; 8];
            let n2 = s.legal_actions_into(&mut buf2);
            assert_eq!(a.len(), n2);
            for i in 0..n2 {
                assert_eq!(a[i], buf2[i]);
            }
        }
    }

    #[test]
    fn legal_actions_carries_raise_cap() {
        // F6 contract: after MAX_RAISES_PER_STREET pot-fraction raises,
        // the ONLY Bet action still offered is the all-in jam. The
        // cap exists to bound tree size, not to forbid a legit 4-bet
        // shove. Before F6, the cap disabled every Bet including the
        // jam — a real bug the audit caught.
        let mut s = fresh_state();
        let mut raises = 0u32;
        while raises < 3 && !s.is_terminal() {
            let mut buf = [Action {
                player: 0,
                kind: ActionKind::Fold,
            }; 8];
            let n = s.legal_actions_into(&mut buf);
            // Pick the smallest Bet, which is a pot-fraction raise, not
            // the jam. That way the loop advances through the cap.
            let mut smallest: Option<(usize, f32)> = None;
            for (i, a) in buf[..n].iter().enumerate() {
                if let ActionKind::Bet(x) = a.kind {
                    match smallest {
                        None => smallest = Some((i, x)),
                        Some((_, cur)) if x < cur => smallest = Some((i, x)),
                        _ => {}
                    }
                }
            }
            match smallest {
                Some((i, _)) => {
                    s.apply_action_in_place(&buf[i]);
                    raises += 1;
                }
                None => break,
            }
        }

        let a = s.legal_actions();
        let bets: Vec<f32> = a
            .iter()
            .filter_map(|x| match x.kind {
                ActionKind::Bet(v) => Some(v),
                _ => None,
            })
            .collect();

        // At most one Bet should be offered, and if there is one, it
        // must be the all-in (the actor's full stack plus their street
        // bet).
        assert!(
            bets.len() <= 1,
            "raise cap allows at most the jam, got bets={:?}",
            bets
        );
        if let Some(&b) = bets.first() {
            let all_in = s.stacks[s.actor] + s.street_bets[s.actor];
            assert!(
                (b - all_in).abs() < 1e-6,
                "only the jam should survive the cap; got {b} != all-in {all_in}",
            );
        }
    }
}

#[cfg(test)]
mod p2_undo_size_tests {
    use super::*;

    /// P2: UndoRecord should stay small. It's memcpy'd on every action
    /// push. Target: <= 48 bytes (fits comfortably within one cache line
    /// alongside bookkeeping when 48 records are packed per state).
    #[test]
    fn undo_record_is_compact() {
        let sz = std::mem::size_of::<UndoRecord>();
        assert!(sz <= 48, "UndoRecord grew to {} bytes; revisit packing", sz);
    }
}


#[cfg(test)]
mod f3_size_aware_tests {
    use super::*;

    /// When SIG_V3_SIZE_AWARE is off (default), the signature is the
    /// legacy v1 4-byte form. This test documents the gate.
    #[test]
    fn gate_off_emits_v1_signature() {
        let s = GameState::new(200.0, 1.0, 2.0);
        let mut buf = [0u8; 8];
        let n = s.infoset_signature_into(&mut buf);
        assert_eq!(n, if SIG_V3_SIZE_AWARE { 8 } else { 4 });
    }

    /// `history_signature_v3` includes the top-bits version tag even
    /// when the gate is off — the function is callable for tests.
    #[test]
    fn v3_signature_carries_version_tag() {
        let s = GameState::new(200.0, 1.0, 2.0);
        let v3 = s.history_signature_v3();
        assert_eq!(v3 >> 60, SIG_V3_VERSION, "top 4 bits are version tag");
    }

    /// Two states that differ only in the street-start pot must produce
    /// different v3 signatures when the pot classes differ. This is the
    /// whole point of F3: the current key cannot see pot size.
    #[test]
    fn v3_distinguishes_street_start_pot_class() {
        let mut a = GameState::new(200.0, 1.0, 2.0);
        a.street_start_pot = 4.0;   // pot_bb = 2.0  -> class 1
        let mut b = GameState::new(200.0, 1.0, 2.0);
        b.street_start_pot = 256.0; // pot_bb = 128 -> class 7
        assert_ne!(
            a.history_signature_v3(),
            b.history_signature_v3(),
            "different pot classes must hash differently",
        );
    }

    /// Same pot class collapses to the same bits — coarse on purpose.
    #[test]
    fn v3_collapses_pots_within_one_class() {
        let mut a = GameState::new(200.0, 1.0, 2.0);
        a.street_start_pot = 8.0;  // pot_bb = 4.0 -> log2 = 2
        let mut b = GameState::new(200.0, 1.0, 2.0);
        b.street_start_pot = 12.0; // pot_bb = 6.0 -> log2 floor = 2
        assert_eq!(a.history_signature_v3(), b.history_signature_v3());
    }

    /// `advance_street_in_place` records the current pot as the new
    /// street's start, and `undo_action` restores the previous value.
    #[test]
    fn street_start_pot_survives_advance_and_undo() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        assert_eq!(s.street_start_pot, 3.0, "preflop start = SB + BB");

        // SB calls (pot 4), BB checks (pot 4 still 4: check adds nothing).
        s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
        s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
        let pot_before = s.pot;
        s.advance_street_in_place(&[0, 4, 8]);
        assert_eq!(
            s.street_start_pot, pot_before,
            "flop street_start_pot = preflop pot",
        );

        s.undo_action();
        assert_eq!(s.street_start_pot, 3.0, "undo restores preflop start");
    }
}

#[cfg(test)]
mod f6_legal_action_tests {
    //! Regression guards for the F6 fix. Before F6, the raise cap
    //! disabled every Bet after three raises (including the jam), and
    //! pot-fraction raises could fall below the true NLHE min-raise-to.
    //! These tests pin the new contract.

    use super::*;

    /// The jam is always legal while the opponent can respond, even
    /// after the pot-fraction raise cap has fired.
    #[test]
    fn jam_is_always_legal_even_after_raise_cap() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        // Preflop SB calls, BB raises, SB re-raises, BB re-raises,
        // SB re-raises (that's 3 raises from the cap's perspective).
        // Then BB should still see the jam as an option.
        s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
        s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Bet(6.0) });
        s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Bet(14.0) });
        s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Bet(30.0) });
        s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Bet(62.0) });

        let mut buf: [Action; 8] = [Action { player: 0, kind: ActionKind::Fold }; 8];
        let n = s.legal_actions_into(&mut buf);
        let bets: Vec<f32> = buf[..n]
            .iter()
            .filter_map(|a| match a.kind {
                ActionKind::Bet(v) => Some(v),
                _ => None,
            })
            .collect();
        assert!(!bets.is_empty(), "raise cap must leave at least the jam");
        let all_in = s.stacks[s.actor] + s.street_bets[s.actor];
        for b in &bets {
            assert!(
                (*b - all_in).abs() < 1e-3,
                "post-cap Bet {b} is not the jam (all-in {all_in}); bets={bets:?}"
            );
        }
    }

    /// Facing the forced BB preflop, the smallest offered raise must
    /// be at least 2x the BB. Before F6, `opp_bet + pot * 0.5` could
    /// produce a smaller number, which real engines reject.
    #[test]
    fn facing_bb_min_raise_clears_legal_floor() {
        let s = GameState::new(200.0, 1.0, 2.0);
        // actor = 0 (SB) facing BB street_bet 2.0. pot = 3.0.
        // Pre-F6 the smallest offered raise would be
        //   2.0 + 3.0 * 0.5 = 3.5, which is below the legal min of 4.0.
        let mut buf: [Action; 8] = [Action { player: 0, kind: ActionKind::Fold }; 8];
        let n = s.legal_actions_into(&mut buf);
        let bb = s.street_bets[1];
        let legal_min = 2.0 * bb;
        for a in buf[..n].iter() {
            if let ActionKind::Bet(v) = a.kind {
                assert!(
                    v >= legal_min - 1e-3,
                    "offered raise {v} is below legal min-raise-to {legal_min}"
                );
            }
        }
    }

    /// Facing a raise where our previous bet was already on the street,
    /// the min-raise-to is `opp_bet + last_raise_delta`, which for
    /// preflop SB facing BB after a limp is still 2*BB. After an
    /// actual raise, the min is 2*opp_bet - our_bet.
    #[test]
    fn facing_raise_min_raise_clears_legal_floor() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        // SB calls (2 total), BB raises to 6, actor = 0.
        s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
        s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Bet(6.0) });
        // opp_bet = 6, our_bet = 2, raises_this_street = 1.
        // Legal min-raise-to = 6 + (6 - 2) = 10.
        let legal_min = 2.0 * s.street_bets[1] - s.street_bets[0];
        let mut buf: [Action; 8] = [Action { player: 0, kind: ActionKind::Fold }; 8];
        let n = s.legal_actions_into(&mut buf);
        for a in buf[..n].iter() {
            if let ActionKind::Bet(v) = a.kind {
                assert!(
                    v >= legal_min - 1e-3,
                    "offered raise {v} is below legal min-raise-to {legal_min}"
                );
            }
        }
    }
}
