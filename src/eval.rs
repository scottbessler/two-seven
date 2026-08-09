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
    for combo in combinations(cards, 5) {
        let h = eval_five(&combo);
        if best.as_ref().is_none_or(|b| h.rank > b.rank) {
            best = Some(h)
        }
    }
    best.unwrap()
}
fn combinations(cards: &[Card], n: usize) -> Vec<Vec<Card>> {
    fn go(src: &[Card], n: usize, start: usize, cur: &mut Vec<Card>, out: &mut Vec<Vec<Card>>) {
        if cur.len() == n {
            out.push(cur.clone());
            return;
        }
        for i in start..src.len() {
            cur.push(src[i]);
            go(src, n, i + 1, cur, out);
            cur.pop();
        }
    }
    let mut out = Vec::new();
    go(cards, n, 0, &mut Vec::new(), &mut out);
    out
}
fn eval_five(cards: &[Card]) -> EvaluatedHand {
    let mut counts = [0u8; 15];
    for c in cards {
        counts[c.rank as usize] += 1
    }
    let flush = cards.iter().all(|c| c.suit == cards[0].suit);
    let straight = straight_high(&counts);
    let mut groups: Vec<(u8, u8)> = (2..=14)
        .rev()
        .filter_map(|r| {
            let n = counts[r as usize];
            (n > 0).then_some((n, r))
        })
        .collect();
    groups.sort_by(|a, b| b.cmp(a));
    let (cat, kickers) = if let Some(high) = straight.filter(|_| flush) {
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
        let mut k = vec![groups[0].1];
        k.extend(groups.iter().filter(|x| x.0 == 1).map(|x| x.1));
        (Category::ThreeOfAKind, k)
    } else if groups[0].0 == 2 && groups[1].0 == 2 {
        (
            Category::TwoPair,
            vec![groups[0].1, groups[1].1, groups[2].1],
        )
    } else if groups[0].0 == 2 {
        let mut k = vec![groups[0].1];
        k.extend(groups.iter().filter(|x| x.0 == 1).map(|x| x.1));
        (Category::Pair, k)
    } else {
        (Category::HighCard, ranks_desc(cards))
    };
    let rank = HandRank {
        category: cat,
        kickers,
    };
    let mut chosen = cards.to_vec();
    chosen.sort_by(|a, b| b.cmp(a));
    EvaluatedHand {
        label: label(&rank),
        rank,
        cards: chosen,
    }
}
fn ranks_desc(cards: &[Card]) -> Vec<u8> {
    let mut r: Vec<u8> = cards.iter().map(|c| c.rank as u8).collect();
    r.sort_unstable_by(|a, b| b.cmp(a));
    r
}
fn straight_high(counts: &[u8; 15]) -> Option<u8> {
    for high in (6..=14).rev() {
        if (0..5).all(|i| counts[(high - i) as usize] > 0) {
            return Some(high);
        }
    }
    if counts[14] > 0 && counts[2] > 0 && counts[3] > 0 && counts[4] > 0 && counts[5] > 0 {
        Some(5)
    } else {
        None
    }
}
fn label(rank: &HandRank) -> String {
    let n = |i: usize| rank_name(rank.kickers[i]);
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
fn rank_name(r: u8) -> &'static str {
    match r {
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
        let x = evaluate(&h("Ah 2d 3c 4s 5h Kd Qc"));
        assert_eq!(
            x.rank,
            HandRank {
                category: Category::Straight,
                kickers: vec![5]
            }
        );
    }
    #[test]
    fn kicker_order() {
        assert!(
            evaluate(&h("Ah Ad Kc Qc 2s 3d 4h")).rank > evaluate(&h("Ah Ad Jc Qc 2s 3d 4h")).rank
        );
    }
    #[test]
    fn steel_wheel() {
        assert_eq!(
            evaluate(&h("Ah 2h 3h 4h 5h Kd Qc")).rank.category,
            Category::StraightFlush
        );
    }
}
