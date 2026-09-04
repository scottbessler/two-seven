use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Suit {
    Clubs,
    Diamonds,
    Hearts,
    Spades,
}
impl Suit {
    pub const ALL: [Suit; 4] = [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades];
    fn code(self) -> char {
        match self {
            Self::Clubs => 'c',
            Self::Diamonds => 'd',
            Self::Hearts => 'h',
            Self::Spades => 's',
        }
    }
}
impl fmt::Display for Suit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())
    }
}
impl FromStr for Suit {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "c" | "C" => Ok(Self::Clubs),
            "d" | "D" => Ok(Self::Diamonds),
            "h" | "H" => Ok(Self::Hearts),
            "s" | "S" => Ok(Self::Spades),
            _ => Err(format!("invalid suit {s}")),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Rank {
    Two = 2,
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
impl Rank {
    pub const ALL: [Rank; 13] = [
        Self::Two,
        Self::Three,
        Self::Four,
        Self::Five,
        Self::Six,
        Self::Seven,
        Self::Eight,
        Self::Nine,
        Self::Ten,
        Self::Jack,
        Self::Queen,
        Self::King,
        Self::Ace,
    ];
    fn code(self) -> char {
        match self {
            Self::Two => '2',
            Self::Three => '3',
            Self::Four => '4',
            Self::Five => '5',
            Self::Six => '6',
            Self::Seven => '7',
            Self::Eight => '8',
            Self::Nine => '9',
            Self::Ten => 'T',
            Self::Jack => 'J',
            Self::Queen => 'Q',
            Self::King => 'K',
            Self::Ace => 'A',
        }
    }
}
impl fmt::Display for Rank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())
    }
}
impl FromStr for Rank {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "2" => Ok(Self::Two),
            "3" => Ok(Self::Three),
            "4" => Ok(Self::Four),
            "5" => Ok(Self::Five),
            "6" => Ok(Self::Six),
            "7" => Ok(Self::Seven),
            "8" => Ok(Self::Eight),
            "9" => Ok(Self::Nine),
            "T" | "t" => Ok(Self::Ten),
            "J" | "j" => Ok(Self::Jack),
            "Q" | "q" => Ok(Self::Queen),
            "K" | "k" => Ok(Self::King),
            "A" | "a" => Ok(Self::Ace),
            _ => Err(format!("invalid rank {s}")),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Card {
    pub rank: Rank,
    pub suit: Suit,
}
impl Card {
    pub fn new(rank: Rank, suit: Suit) -> Self {
        Self { rank, suit }
    }
}
impl Ord for Card {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.rank, self.suit).cmp(&(other.rank, other.suit))
    }
}
impl PartialOrd for Card {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.rank, self.suit)
    }
}
impl FromStr for Card {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 2 {
            return Err(format!("invalid card {s}"));
        }
        Ok(Self::new(s[0..1].parse()?, s[1..2].parse()?))
    }
}
impl Serialize for Card {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for Card {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(d)?.parse().map_err(de::Error::custom)
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Deck {
    cards: Vec<Card>,
    next: usize,
}
impl Deck {
    #[cfg(test)]
    pub fn from_cards(cards: Vec<Card>) -> Self {
        Self { cards, next: 0 }
    }

    pub fn seeded(seed: u64) -> Self {
        Self::shoe_seeded(seed, 1)
    }

    pub fn shoe_seeded(seed: u64, decks: u8) -> Self {
        let deck_count = decks.max(1);
        let mut cards = Vec::with_capacity(52 * usize::from(deck_count));
        for _ in 0..deck_count {
            for rank in Rank::ALL {
                for suit in Suit::ALL {
                    cards.push(Card::new(rank, suit));
                }
            }
        }
        let mut rng = StdRng::seed_from_u64(seed);
        cards.shuffle(&mut rng);
        Self { cards, next: 0 }
    }
    pub fn deal(&mut self) -> Option<Card> {
        let card = self.cards.get(self.next).copied();
        self.next += card.is_some() as usize;
        card
    }
    pub fn remaining(&self) -> usize {
        self.cards.len() - self.next
    }
    pub fn dealt(&self) -> usize {
        self.next
    }
    pub fn total(&self) -> usize {
        self.cards.len()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notation() {
        assert_eq!("Ah".parse::<Card>().unwrap().to_string(), "Ah");
        assert_eq!("Td".parse::<Card>().unwrap().rank, Rank::Ten);
    }
    #[test]
    fn seeded_decks_match() {
        let mut a = Deck::seeded(7);
        let mut b = Deck::seeded(7);
        for _ in 0..52 {
            assert_eq!(a.deal(), b.deal());
        }
    }
    #[test]
    fn full_deck() {
        let mut d = Deck::seeded(1);
        let mut seen = std::collections::HashSet::new();
        while let Some(c) = d.deal() {
            seen.insert(c);
        }
        assert_eq!(seen.len(), 52);
        assert_eq!(d.remaining(), 0);
    }
}
