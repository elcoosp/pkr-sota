// Stubs to allow tests to compile – implementations are incomplete / wrong.

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum Suit {
    Spade,
    Heart,
    Diamond,
    Club,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Card {
    pub suit: Suit,
    pub rank: Rank,
}

impl Card {
    pub fn new(suit: Suit, rank: Rank) -> Self {
        // placeholder – does not enforce validity; will be fixed later
        Card { suit, rank }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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
            Rank::Two, Rank::Three, Rank::Four, Rank::Five, Rank::Six,
            Rank::Seven, Rank::Eight, Rank::Nine, Rank::Ten,
            Rank::Jack, Rank::Queen, Rank::King, Rank::Ace,
        ];
        let mut set = HashSet::new();
        for &suit in &suits {
            for &rank in &ranks {
                set.insert(Card::new(suit, rank));
            }
        }
        assert_eq!(set.len(), 52);
    }
}
