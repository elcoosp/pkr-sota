use crate::card::{Card, Rank, Suit};

/// A standard 52-card deck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deck {
    cards: Vec<Card>,
}

impl Deck {
    /// Creates a new deck with all 52 cards in an unspecified order.
    pub fn new() -> Self {
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
        let mut cards = Vec::with_capacity(52);
        for &suit in &suits {
            for &rank in &ranks {
                cards.push(Card::new(suit, rank));
            }
        }
        Deck { cards }
    }

    /// Removes and returns a single card from the top of the deck.
    /// Returns `None` if the deck is empty.
    pub fn deal(&mut self) -> Option<Card> {
        self.cards.pop()
    }

    /// Returns the number of cards remaining.
    pub fn remaining(&self) -> usize {
        self.cards.len()
    }
}

impl Default for Deck {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_deck_has_52_cards() {
        let deck = Deck::new();
        assert_eq!(deck.remaining(), 52);
    }

    #[test]
    fn deal_reduces_card_count() {
        let mut deck = Deck::new();
        let card = deck.deal();
        assert!(card.is_some());
        assert_eq!(deck.remaining(), 51);
    }

    #[test]
    fn deck_is_empty_after_dealing_all() {
        let mut deck = Deck::new();
        for _ in 0..52 {
            deck.deal();
        }
        assert!(deck.deal().is_none());
        assert_eq!(deck.remaining(), 0);
    }
}
