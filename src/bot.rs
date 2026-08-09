use crate::{
    cards::{Card, Deck, Rank},
    eval::{Category, evaluate},
    holdem::{Action, LegalActions},
    table::BotKind,
    view::HandView,
};
use rand::{Rng, SeedableRng, rngs::StdRng, seq::SliceRandom};

impl BotKind {
    pub fn act(self, view: &HandView, legal: &LegalActions, seed: u64) -> Action {
        match self {
            Self::Fish => fish(view, legal, seed),
            Self::Rock => rock(view, legal),
            Self::Grinder => grinder(view, legal),
            Self::Shark => shark(view, legal, seed),
        }
    }
}

fn fish(view: &HandView, legal: &LegalActions, seed: u64) -> Action {
    let mut rng = StdRng::seed_from_u64(seed);
    let pair = made_category(view).is_some_and(|category| category >= Category::Pair);
    let has_call = legal
        .actions
        .iter()
        .any(|action| matches!(action, Action::Call));
    let choices: Vec<Action> = legal
        .actions
        .iter()
        .copied()
        .filter(|action| !has_call || !matches!(action, Action::AllIn))
        .filter(|action| pair || !matches!(action, Action::Fold))
        .collect();
    *choices
        .choose(&mut rng)
        .or_else(|| legal.actions.choose(&mut rng))
        .expect("legal action set is non-empty")
}

fn rock(view: &HandView, legal: &LegalActions) -> Action {
    let premium = view.your_hole_cards.as_ref().is_some_and(|cards| {
        cards.len() == 2
            && (cards[0].rank == cards[1].rank || cards.iter().all(|card| card.rank >= Rank::Jack))
    });
    let made = made_category(view).is_some_and(|category| category >= Category::Pair);
    if !premium && !made && legal.to_call > 0 {
        return first(legal, Action::Fold);
    }
    first_calling(legal)
}

fn grinder(view: &HandView, legal: &LegalActions) -> Action {
    let preflop_strong = view.board.is_empty()
        && view.your_hole_cards.as_ref().is_some_and(|cards| {
            cards.len() == 2
                && (cards[0].rank == cards[1].rank
                    || cards.iter().any(|card| card.rank >= Rank::Ace)
                    || cards.iter().all(|card| card.rank >= Rank::Jack))
        });
    if preflop_strong {
        return wager_or_call(legal);
    }
    let strong = made_category(view).is_some_and(|category| category >= Category::TwoPair);
    if strong {
        return wager_or_call(legal);
    }
    if legal.to_call > 0 && made_category(view).is_none() {
        return first(legal, Action::Fold);
    }
    first_calling(legal)
}

fn shark(view: &HandView, legal: &LegalActions, seed: u64) -> Action {
    let mut rng = StdRng::seed_from_u64(seed ^ 0x0053_4841_524b);
    let equity = estimate_equity(view, seed);
    let pot_odds = if legal.to_call == 0 {
        0.0
    } else {
        legal.to_call as f64 / (view.pot + legal.to_call) as f64
    };
    if equity < pot_odds && legal.to_call > 0 {
        return first(legal, Action::Fold);
    }
    if equity > 0.65 || (equity > 0.4 && rng.gen_ratio(1, 5)) {
        return wager_or_call(legal);
    }
    first_calling(legal)
}

fn estimate_equity(view: &HandView, seed: u64) -> f64 {
    let Some(hero) = view.your_hole_cards.as_ref() else {
        return 0.25;
    };
    if hero.len() != 2 {
        return 0.25;
    }
    let mut deck = Deck::seeded(0);
    let mut unseen: Vec<Card> = (0..52).filter_map(|_| deck.deal()).collect();
    unseen.retain(|card| !hero.contains(card) && !view.board.contains(card));
    let opponents = view.seats.len().saturating_sub(1).max(1);
    let mut wins = 0.0;
    let samples = 64;
    let mut rng = StdRng::seed_from_u64(seed ^ 0x0053_4841_524b);
    for _ in 0..samples {
        let mut available = unseen.clone();
        let mut opponent_hands = Vec::with_capacity(opponents);
        for _ in 0..opponents {
            let first = rng.gen_range(0..available.len());
            let first_card = available.swap_remove(first);
            let second = rng.gen_range(0..available.len());
            opponent_hands.push(vec![first_card, available.swap_remove(second)]);
        }
        let mut board = view.board.clone();
        while board.len() < 5 {
            let index = rng.gen_range(0..available.len());
            board.push(available.swap_remove(index));
        }
        let mut hero_cards = hero.clone();
        hero_cards.extend(board.iter().copied());
        let hero_rank = evaluate(&hero_cards).rank;
        let best_opponent = opponent_hands
            .iter()
            .map(|hand| {
                let mut cards = hand.clone();
                cards.extend(board.iter().copied());
                evaluate(&cards).rank
            })
            .max()
            .expect("at least one opponent");
        if hero_rank > best_opponent {
            wins += 1.0;
        } else if hero_rank == best_opponent {
            wins += 0.5;
        }
    }
    wins / samples as f64
}

fn made_category(view: &HandView) -> Option<Category> {
    let hole = view.your_hole_cards.as_ref()?;
    if hole.len() != 2 || view.board.len() + hole.len() < 5 {
        return None;
    }
    let mut cards = hole.clone();
    cards.extend(view.board.iter().copied());
    Some(evaluate(&cards).rank.category)
}

fn wager_or_call(legal: &LegalActions) -> Action {
    legal
        .actions
        .iter()
        .copied()
        .find(|action| matches!(action, Action::Raise { .. } | Action::Bet { .. }))
        .unwrap_or_else(|| first_calling(legal))
}

fn first_calling(legal: &LegalActions) -> Action {
    legal
        .actions
        .iter()
        .copied()
        .find(|action| matches!(action, Action::Check | Action::Call))
        .or_else(|| legal.actions.first().copied())
        .expect("legal action set is non-empty")
}

fn first(legal: &LegalActions, desired: Action) -> Action {
    legal
        .actions
        .iter()
        .copied()
        .find(|action| std::mem::discriminant(action) == std::mem::discriminant(&desired))
        .unwrap_or_else(|| first_calling(legal))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cards::Card,
        holdem::Hand,
        table::Stakes,
        view::{HandView, hand_view},
    };
    use std::str::FromStr;

    #[test]
    fn every_kind_returns_an_action_accepted_by_the_engine() {
        for seed in 0..100 {
            for kind in [
                BotKind::Fish,
                BotKind::Rock,
                BotKind::Grinder,
                BotKind::Shark,
            ] {
                let mut hand = Hand::new(
                    Stakes::NoLimit {
                        small_blind: 1,
                        big_blind: 2,
                    },
                    &[100, 100, 100],
                    0,
                    seed,
                );
                for turn in 0..100 {
                    if hand.complete {
                        break;
                    }
                    let legal = hand.legal_actions().expect("action");
                    let view = hand_view(&hand, Some(legal.seat));
                    let action = kind.act(&view, &legal, seed + turn);
                    hand.apply_action(action).unwrap_or_else(|error| {
                        panic!(
                            "{kind:?} seed {seed} turn {turn}: {action:?} not in {legal:?}: {error}"
                        )
                    });
                }
            }
        }
    }

    #[test]
    fn policies_have_distinct_style_signals() {
        let trash = Card::from_str("2c").unwrap();
        let trash_two = Card::from_str("7d").unwrap();
        let pair = Card::from_str("Ac").unwrap();
        let pair_two = Card::from_str("Ad").unwrap();
        let view = HandView {
            street: "Preflop".into(),
            board: Vec::new(),
            your_hole_cards: Some(vec![trash, trash_two]),
            seats: Vec::new(),
            pot: 100,
            current_player: Some(0),
            legal_actions: None,
            summary: None,
        };
        let legal = LegalActions {
            seat: 0,
            actions: vec![Action::Fold, Action::Call],
            to_call: 10,
            wager: None,
        };
        assert_eq!(rock(&view, &legal), Action::Fold);
        assert_eq!(fish(&view, &legal, 3), Action::Call);
        let pair_view = HandView {
            your_hole_cards: Some(vec![pair, pair_two]),
            ..view
        };
        assert_eq!(fish(&pair_view, &legal, 4), Action::Call);
    }

    #[test]
    fn aggregate_vpip_orders_fish_grinder_rock() {
        let mut totals = [0usize; 3];
        for seed in 0..300 {
            for (slot, kind) in [BotKind::Fish, BotKind::Grinder, BotKind::Rock]
                .into_iter()
                .enumerate()
            {
                let mut hand = Hand::new(
                    Stakes::NoLimit {
                        small_blind: 1,
                        big_blind: 2,
                    },
                    &[100, 100, 100, 100],
                    0,
                    seed,
                );
                while hand.street == crate::holdem::Street::Preflop && !hand.complete {
                    let legal = hand.legal_actions().expect("preflop action");
                    let view = hand_view(&hand, Some(legal.seat));
                    let action = kind.act(&view, &legal, seed + 10_000);
                    if matches!(
                        action,
                        Action::Call | Action::Bet { .. } | Action::Raise { .. } | Action::AllIn
                    ) {
                        totals[slot] += 1;
                    }
                    hand.apply_action(action).unwrap();
                }
            }
        }
        assert!(totals[0] > totals[1], "{totals:?}");
        assert!(totals[1] > totals[2], "{totals:?}");
    }
}
