/// Represents the four suits in a standard deck.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Suit {
    Spade,
    Heart,
    Diamond,
    Club,
}

/// Represents the thirteen ranks in a standard deck.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Rank {
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    Jack,
    Queen,
    King,
    Ace,
}

/// A playing card with a suit and rank.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Card {
    pub suit: Suit,
    pub rank: Rank,
}

impl Card {
    /// Creates a new card. This is always valid because the types restrict the values.
    pub fn new(suit: Suit, rank: Rank) -> Self {
        Card { suit, rank }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    #[test]
    fn card_new_creates_valid_card() {
        let card = Card::new(Suit::Spade, Rank::Ace);
        assert_eq!(card.suit, Suit::Spade);
        assert_eq!(card.rank, Rank::Ace);
    }

    #[test]
    fn there_are_52_unique_cards() {
        let suits = [Suit::Spade, Suit::Heart, Suit::Diamond, Suit::Club];
        let ranks = [
            Rank::Two,
            Rank::Three,
            Rank::Four,
            Rank::Five,
            Rank::Six,
            Rank::Seven,
            Rank::Eight,
            Rank::Nine,
            Rank::Ten,
            Rank::Jack,
            Rank::Queen,
            Rank::King,
            Rank::Ace,
        ];
        let mut set = HashSet::new();
        for &suit in &suits {
            for &rank in &ranks {
                set.insert(Card::new(suit, rank));
            }
        }
        assert_eq!(set.len(), 52);
    }

    #[test]
    fn card_is_copy() {
        let c1 = Card::new(Suit::Club, Rank::Five);
        let c2 = c1; // Copy, not move
        assert_eq!(c1, c2);
    }

    #[test]
    fn card_equality() {
        let c1 = Card::new(Suit::Diamond, Rank::King);
        let c2 = Card::new(Suit::Diamond, Rank::King);
        assert_eq!(c1, c2);
        let c3 = Card::new(Suit::Heart, Rank::King);
        assert_ne!(c1, c3);
    }

    #[test]
    fn suit_discriminants() {
        assert_eq!(Suit::Spade as u8, 0);
        assert_eq!(Suit::Heart as u8, 1);
        assert_eq!(Suit::Diamond as u8, 2);
        assert_eq!(Suit::Club as u8, 3);
    }

    #[test]
    fn rank_discriminants() {
        assert_eq!(Rank::Two as u8, 0);
        assert_eq!(Rank::Three as u8, 1);
        assert_eq!(Rank::Four as u8, 2);
        assert_eq!(Rank::Five as u8, 3);
        assert_eq!(Rank::Six as u8, 4);
        assert_eq!(Rank::Seven as u8, 5);
        assert_eq!(Rank::Eight as u8, 6);
        assert_eq!(Rank::Nine as u8, 7);
        assert_eq!(Rank::Ten as u8, 8);
        assert_eq!(Rank::Jack as u8, 9);
        assert_eq!(Rank::Queen as u8, 10);
        assert_eq!(Rank::King as u8, 11);
        assert_eq!(Rank::Ace as u8, 12);
    }

    #[test]
    fn hash_is_consistent() {
        let c1 = Card::new(Suit::Spade, Rank::Ace);
        let c2 = Card::new(Suit::Spade, Rank::Ace);
        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();
        c1.hash(&mut h1);
        c2.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }
}
