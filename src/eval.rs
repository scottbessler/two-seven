use crate::cards::Card;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum Category {
    HighCard,
    Pair,
    TwoPair,
    ThreeOfAKind,
    Straight,
    Flush,
    FullHouse,
    FourOfAKind,
    StraightFlush,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct HandRank {
    pub category: Category,
    pub kickers: Vec<u8>,
}

impl fmt::Display for HandRank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} {:?}", self.category, self.kickers)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvaluatedHand {
    pub rank: HandRank,
    pub cards: Vec<Card>,
    pub label: String,
}

pub fn evaluate(cards: &[Card]) -> EvaluatedHand {
    assert!((5..=7).contains(&cards.len()));
    let mut best: Option<EvaluatedHand> = None;
    let mut picked = [0usize; 5];
    visit(cards, 0, 0, &mut picked, &mut best);
    best.expect("at least one five-card hand")
}

fn visit(
    cards: &[Card],
    start: usize,
    depth: usize,
    picked: &mut [usize; 5],
    best: &mut Option<EvaluatedHand>,
) {
    if depth == 5 {
        let chosen = [
            cards[picked[0]],
            cards[picked[1]],
            cards[picked[2]],
            cards[picked[3]],
            cards[picked[4]],
        ];
        let hand = eval_five(&chosen);
        if best.as_ref().is_none_or(|current| hand.rank > current.rank) {
            *best = Some(hand);
        }
        return;
    }
    for i in start..cards.len() {
        picked[depth] = i;
        visit(cards, i + 1, depth + 1, picked, best);
    }
}

fn eval_five(cards: &[Card; 5]) -> EvaluatedHand {
    let mut counts = [0u8; 15];
    for card in cards {
        counts[card.rank as usize] += 1;
    }
    let flush = cards.iter().all(|card| card.suit == cards[0].suit);
    let straight = straight_high(&counts);
    let mut groups = [(0u8, 0u8); 5];
    let mut len = 0;
    for rank in (2..=14).rev() {
        if counts[rank] > 0 {
            groups[len] = (counts[rank], rank as u8);
            len += 1;
        }
    }
    for i in 1..len {
        let mut j = i;
        while j > 0 && groups[j] > groups[j - 1] {
            groups.swap(j, j - 1);
            j -= 1;
        }
    }
    let (category, kickers) = if let Some(high) = straight.filter(|_| flush) {
        (Category::StraightFlush, vec![high])
    } else if groups[0].0 == 4 {
        (Category::FourOfAKind, vec![groups[0].1, groups[1].1])
    } else if groups[0].0 == 3 && groups[1].0 == 2 {
        (Category::FullHouse, vec![groups[0].1, groups[1].1])
    } else if flush {
        (Category::Flush, ranks_desc(cards))
    } else if let Some(high) = straight {
        (Category::Straight, vec![high])
    } else if groups[0].0 == 3 {
        (
            Category::ThreeOfAKind,
            vec![groups[0].1, groups[1].1, groups[2].1],
        )
    } else if groups[0].0 == 2 && groups[1].0 == 2 {
        (
            Category::TwoPair,
            vec![groups[0].1, groups[1].1, groups[2].1],
        )
    } else if groups[0].0 == 2 {
        (
            Category::Pair,
            vec![groups[0].1, groups[1].1, groups[2].1, groups[3].1],
        )
    } else {
        (Category::HighCard, ranks_desc(cards))
    };
    let rank = HandRank { category, kickers };
    let mut selected = cards.to_vec();
    selected.sort_by(|a, b| b.cmp(a));
    EvaluatedHand {
        label: label(&rank),
        rank,
        cards: selected,
    }
}

fn ranks_desc(cards: &[Card; 5]) -> Vec<u8> {
    let mut ranks = [0u8; 5];
    for (i, card) in cards.iter().enumerate() {
        ranks[i] = card.rank as u8;
    }
    ranks.sort_unstable_by(|a, b| b.cmp(a));
    ranks.to_vec()
}
fn straight_high(counts: &[u8; 15]) -> Option<u8> {
    for high in (6..=14).rev() {
        if (0..5).all(|offset| counts[high - offset] > 0) {
            return Some(high as u8);
        }
    }
    (counts[14] > 0 && counts[2] > 0 && counts[3] > 0 && counts[4] > 0 && counts[5] > 0)
        .then_some(5)
}
fn label(rank: &HandRank) -> String {
    let n = |i| rank_name(rank.kickers[i]);
    match rank.category {
        Category::HighCard => format!("High card, {}", n(0)),
        Category::Pair => format!("Pair of {}, {} kicker", n(0), n(1)),
        Category::TwoPair => format!("Two pair, {} and {}", n(0), n(1)),
        Category::ThreeOfAKind => format!("Three of a kind, {}", n(0)),
        Category::Straight => format!("Straight, {} high", n(0)),
        Category::Flush => format!("Flush, {} high", n(0)),
        Category::FullHouse => format!("Full house, {} full of {}", n(0), n(1)),
        Category::FourOfAKind => format!("Four of a kind, {}", n(0)),
        Category::StraightFlush => format!("Straight flush, {} high", n(0)),
    }
}
fn rank_name(rank: u8) -> &'static str {
    match rank {
        14 => "aces",
        13 => "kings",
        12 => "queens",
        11 => "jacks",
        10 => "tens",
        9 => "nines",
        8 => "eights",
        7 => "sevens",
        6 => "sixes",
        5 => "fives",
        4 => "fours",
        3 => "threes",
        2 => "twos",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn h(s: &str) -> Vec<Card> {
        s.split_whitespace().map(|x| x.parse().unwrap()).collect()
    }
    #[test]
    fn categories() {
        for (s, c) in [
            ("Ah Kh Qh Jh Th 2c 3d", Category::StraightFlush),
            ("Ah Ad Ac As 2c 3d", Category::FourOfAKind),
            ("Ah Ad Ac Kh Kd 3d", Category::FullHouse),
            ("Ah Kh 9h 4h 2h 3d 4c", Category::Flush),
            ("Ah Kd Qc Jh Tc 2d 3c", Category::Straight),
            ("Ah Ad Ac Kh 2d 3c 4s", Category::ThreeOfAKind),
            ("Ah Ad Kh Kd 2c 3d 4s", Category::TwoPair),
            ("Ah Ad Kh Qd 2c 3d 4s", Category::Pair),
            ("Ah Kd Qc 9h 2c 3d 4s", Category::HighCard),
        ] {
            assert_eq!(evaluate(&h(s)).rank.category, c)
        }
    }
    #[test]
    fn wheel() {
        assert_eq!(evaluate(&h("Ah 2d 3c 4s 5h Kd Qc")).rank.kickers, vec![5])
    }
    #[test]
    fn returned_cards_are_five() {
        assert_eq!(evaluate(&h("Ah Kh Qh Jh Th 2c 3d")).cards.len(), 5)
    }
    #[test]
    fn kickers() {
        assert!(
            evaluate(&h("Ah Ad Kc Qc 2s 3d 4h")).rank > evaluate(&h("Ah Ad Jc Qc 2s 3d 4h")).rank
        )
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::cards::Deck;

    #[test]
    fn random_deals_have_plausible_categories_and_valid_best_five() {
        let mut counts = [0usize; 9];
        for seed in 0..5_000u64 {
            let mut deck = Deck::seeded(seed);
            let cards: Vec<Card> = (0..7).map(|_| deck.deal().expect("card")).collect();
            let hand = evaluate(&cards);
            let category = hand.rank.category as usize;
            counts[category] += 1;
            assert_eq!(evaluate(&hand.cards).rank, hand.rank);
        }
        assert!((700..=1_100).contains(&counts[0]));
        assert!((1_700..=2_700).contains(&counts[1]));
        assert!(counts[0] > counts[8]);
    }
}
