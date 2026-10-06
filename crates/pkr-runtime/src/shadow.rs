//! Shadow state (Phase 1.1): the abstract game the blueprint trained on,
//! advanced by TRANSLATED actions. The real state keeps real chips.
//! Blueprint hashing + the RangeTracker use the shadow only, so an
//! off-tree opponent bet size cannot desync them from training.

use crate::translate_live::translate_bet;
use pkr_core::state::{Action, ActionKind, GameState};
use rand::RngExt;

pub struct Shadow {
    pub st: GameState,
}

impl Shadow {
    pub fn new(root: GameState) -> Self {
        Shadow { st: root }
    }

    /// Opponent acted with `real` (real chips). Apply the in-tree
    /// equivalent to the shadow and return the shadow action actually
    /// applied — feed THAT to the RangeTracker.
    pub fn observe_opp(
        &mut self,
        real_before: &GameState,
        real: &Action,
        rng: &mut impl rand::RngExt,
    ) -> Action {
        let mut buf = [Action { player: 0, kind: ActionKind::Fold }; 8];
        let n = self.st.legal_actions_into(&mut buf);
        if n == 0 { return *real; }
        let chosen = match real.kind {
            ActionKind::Bet(total) => {
                let a = real_before.actor;
                let committed = real_before.street_bets[a]
                    .max(real_before.street_bets[1 - a]);
                let pot = real_before.pot.max(1.2);
                let x = (total - committed).max(0.0) / pot;
                let jam_frac = (real_before.stacks[a] + real_before.street_bets[a]
                    - committed) / pot;
                let bucket = translate_bet(x, jam_frac, rng);
                let sh = &self.st;
                let mut best: Option<Action> = None;
                for act in &buf[..n] {
                    if matches!(act.kind, ActionKind::Bet(_)) {
                        let b = pkr_core::abstraction::action_bucket(
                            &act.kind, sh.stacks[sh.actor], sh.street_bets[sh.actor],
                            sh.street_bets[1 - sh.actor], sh.pot);
                        if b == bucket { best = Some(*act); break; }
                    }
                }
                best.or_else(|| buf[..n].iter().filter(|a| matches!(a.kind, ActionKind::Bet(_))).last().copied())
                    .unwrap_or(buf[0])
            }
            _ => *buf[..n].iter()
                .find(|a| std::mem::discriminant(&a.kind) == std::mem::discriminant(&real.kind))
                .unwrap_or(&buf[0]),
        };
        self.st.apply_action_in_place(&chosen);
        chosen
    }

    /// We acted with abstract bucket `b`. Apply the matching in-tree
    /// action to the shadow; return it.
    pub fn observe_own_bucket(&mut self, bucket: u8) -> Action {
        let mut buf = [Action { player: 0, kind: ActionKind::Fold }; 8];
        let n = self.st.legal_actions_into(&mut buf);
        let sh = &self.st;
        let act = buf[..n].iter()
            .find(|a| pkr_core::abstraction::action_bucket(
                &a.kind, sh.stacks[sh.actor], sh.street_bets[sh.actor],
                sh.street_bets[1 - sh.actor], sh.pot) == bucket)
            .copied()
            .unwrap_or(buf[0]);
        self.st.apply_action_in_place(&act);
        act
    }

    pub fn advance_street(&mut self, cards: &[u8]) {
        self.st.advance_street_in_place(cards);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn shadow_stays_in_sync_on_in_tree_line() {
        let root = GameState::new(200.0, 1.0, 2.0);
        let mut real = root.clone();
        let mut sh = Shadow::new(root);
        let mut rng = SmallRng::seed_from_u64(1);
        // SB call, BB check — all in-tree.
        for _ in 0..2 {
            let before = real.clone();
            let a = if real.actor == 0 {
                Action { player: 0, kind: ActionKind::Call }
            } else {
                Action { player: 1, kind: ActionKind::Check }
            };
            sh.observe_opp(&before, &a, &mut rng);
            real.apply_action_in_place(&a);
        }
        assert_eq!(sh.st.street, real.street);
        assert_eq!(sh.st.actor, real.actor);
    }

    #[test]
    fn shadow_maps_off_tree_bet_to_a_legal_in_tree_bet() {
        let root = GameState::new(200.0, 1.0, 2.0);
        let mut real = root.clone();
        // SB limps; BB faces no bet (checking) — instead give BB a bet.
        real.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
        let before = real.clone();
        // an off-tree 0.7x pot bet
        let off = Action { player: 1, kind: ActionKind::Bet(2.0 + 4.0 * 0.7) };
        let mut sh = Shadow::new(root);
        sh.st.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
        let mut rng = SmallRng::seed_from_u64(2);
        let applied = sh.observe_opp(&before, &off, &mut rng);
        assert!(matches!(applied.kind, ActionKind::Bet(_)),
            "off-tree bet must map to an in-tree bet, got {:?}", applied.kind);
        assert!(sh.st.legal_actions_into(&mut [Action{player:0,kind:ActionKind::Fold};8]) > 0);
    }
}
