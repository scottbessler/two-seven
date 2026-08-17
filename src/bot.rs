use crate::{
    cards::{Card, Deck, Rank},
    eval::{Category, evaluate},
    holdem::{Action, LegalActions},
    money::Cents,
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
    if premium || made {
        return wager_or_call(legal);
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
    if view.board.is_empty() {
        return shark_preflop(view, legal, seed);
    }
    let mut rng = StdRng::seed_from_u64(seed ^ 0x0053_4841_524b);
    let opponents = active_opponents(view, legal.seat);
    let behind = players_behind(view, legal.seat);
    let in_position = behind <= 1;
    let effective = effective_stack(view, legal.seat);
    let equity = estimate_equity(view, legal.seat, seed, opponents);
    let fair_share = 1.0 / (opponents + 1) as f64;
    let pot_odds = if legal.to_call == 0 {
        0.0
    } else {
        legal.to_call as f64 / (view.pot + legal.to_call) as f64
    };
    let edge = equity - fair_share;
    let bet_edge = if opponents == 1 {
        if in_position { 0.10 } else { 0.16 }
    } else if in_position {
        0.14
    } else {
        0.20
    };
    if edge >= bet_edge || equity > 0.72 {
        let target = legal.to_call + (view.pot * 2) / 3;
        return sized_wager(legal, target, effective).unwrap_or_else(|| first_calling(legal));
    }
    if in_position && edge > 0.05 && rng.gen_ratio(1, 4) {
        let target = legal.to_call + view.pot / 2;
        return sized_wager(legal, target, effective).unwrap_or_else(|| first_calling(legal));
    }
    if legal.to_call > 0 && equity < pot_odds {
        return first(legal, Action::Fold);
    }
    first_calling(legal)
}

fn shark_preflop(view: &HandView, legal: &LegalActions, _seed: u64) -> Action {
    let score = preflop_score(view);
    let (small_blind, big_blind) = blind_seats(view);
    let is_big_blind = big_blind == Some(legal.seat);
    let is_small_blind = small_blind == Some(legal.seat);
    let unraised = !view.events.iter().any(|event| {
        event.street == crate::holdem::Street::Preflop
            && matches!(
                event.kind,
                crate::holdem::HandEventKind::Bet
                    | crate::holdem::HandEventKind::Raise
                    | crate::holdem::HandEventKind::AllIn
            )
    });
    let pot_odds = if legal.to_call == 0 {
        0.0
    } else {
        legal.to_call as f64 / (view.pot + legal.to_call) as f64
    };
    let open_threshold = opening_threshold(view, legal.seat);
    let defense_threshold = if is_big_blind {
        3
    } else if is_small_blind {
        4
    } else {
        6
    };
    let effective = effective_stack(view, legal.seat);
    let short = effective < view.big_blind * 20;
    let single_raise = view
        .players
        .iter()
        .filter(|player| player.seat != legal.seat && !player.folded)
        .filter(|player| {
            view.events.iter().any(|event| {
                event.street == crate::holdem::Street::Preflop
                    && event.seat == Some(player.seat)
                    && matches!(
                        event.kind,
                        crate::holdem::HandEventKind::Bet
                            | crate::holdem::HandEventKind::Raise
                            | crate::holdem::HandEventKind::AllIn
                    )
            })
        })
        .count()
        == 1;
    if short && score >= open_threshold && (unraised || score >= defense_threshold) {
        return all_in_or_wager(legal, effective).unwrap_or_else(|| first_calling(legal));
    }
    if score >= 10 {
        let target = legal.to_call + view.pot + view.pot / 2;
        return sized_wager(legal, target, effective).unwrap_or_else(|| first_calling(legal));
    }
    if unraised && score >= open_threshold {
        let target = legal.to_call + view.pot;
        return sized_wager(legal, target, effective).unwrap_or_else(|| first_calling(legal));
    }
    if single_raise
        && (is_big_blind || is_small_blind)
        && score >= defense_threshold
        && pot_odds <= if is_big_blind { 0.32 } else { 0.25 }
    {
        return first_calling(legal);
    }
    if score >= 9 && pot_odds < 0.30 {
        let target = legal.to_call + view.pot;
        return sized_wager(legal, target, effective).unwrap_or_else(|| first_calling(legal));
    }
    if legal.to_call == 0 {
        return first_calling(legal);
    }
    first(legal, Action::Fold)
}

/// Rough Chen-style preflop hand score: pairs and big suited/connected
/// combinations score high, offsuit trash scores near zero.
fn preflop_score(view: &HandView) -> i32 {
    let Some(cards) = view.your_hole_cards.as_ref() else {
        return 0;
    };
    preflop_score_cards(cards)
}

fn preflop_score_cards(cards: &[Card]) -> i32 {
    if cards.len() != 2 {
        return 0;
    }
    let (high, low) = if cards[0].rank >= cards[1].rank {
        (cards[0], cards[1])
    } else {
        (cards[1], cards[0])
    };
    let rank_points = |rank: Rank| match rank {
        Rank::Ace => 6,
        Rank::King => 5,
        Rank::Queen => 4,
        Rank::Jack => 3,
        Rank::Ten => 2,
        Rank::Nine | Rank::Eight => 1,
        _ => 0,
    };
    let mut score = rank_points(high.rank) + rank_points(low.rank);
    if high.rank == low.rank {
        score = (score * 2).max(5);
    }
    if high.suit == low.suit {
        score += 1;
    }
    let gap = (high.rank as i32) - (low.rank as i32);
    if gap == 1 {
        score += 1;
    } else if gap > 2 {
        score -= (gap - 2).min(3);
    }
    score
}

fn live_order(view: &HandView) -> Vec<usize> {
    table_order(view)
        .into_iter()
        .filter(|seat| {
            view.players
                .iter()
                .find(|player| player.seat == *seat)
                .is_some_and(|player| !player.folded)
        })
        .collect()
}

fn table_order(view: &HandView) -> Vec<usize> {
    let seats: Vec<usize> = view.players.iter().map(|player| player.seat).collect();
    if seats.is_empty() {
        return seats;
    }
    let start = seats
        .iter()
        .position(|seat| *seat == view.button)
        .unwrap_or(0);
    (0..seats.len())
        .map(|offset| seats[(start + offset) % seats.len()])
        .collect()
}

fn players_behind(view: &HandView, seat: usize) -> usize {
    players_behind_excluding(view, seat, [None, None])
}

fn players_behind_excluding(view: &HandView, seat: usize, excluded: [Option<usize>; 2]) -> usize {
    let order = street_action_order(view);
    let Some(hero) = order.iter().position(|candidate| *candidate == seat) else {
        return 0;
    };
    order
        .iter()
        .skip(hero + 1)
        .filter(|candidate| {
            !excluded.contains(&Some(**candidate))
                && view.players.iter().any(|player| {
                    player.seat == **candidate
                        && !player.folded
                        && !player.all_in
                        && (!player.acted || player.street_contribution < view.last_bet)
                })
        })
        .count()
}

fn street_action_order(view: &HandView) -> Vec<usize> {
    let seats = table_order(view);
    if seats.is_empty() {
        return seats;
    }
    let start = if view.board.is_empty() {
        if seats.len() == 2 {
            seats.iter().position(|seat| *seat == view.button)
        } else {
            blind_seats(view)
                .1
                .and_then(|big_blind| seats.iter().position(|seat| *seat == big_blind))
                .map(|index| (index + 1) % seats.len())
        }
    } else {
        seats
            .iter()
            .position(|seat| *seat == view.button)
            .map(|index| (index + 1) % seats.len())
    }
    .unwrap_or(0);
    (0..seats.len())
        .map(|offset| seats[(start + offset) % seats.len()])
        .filter(|seat| {
            view.players
                .iter()
                .find(|player| player.seat == *seat)
                .is_some_and(|player| !player.folded)
        })
        .collect()
}

fn opening_threshold(view: &HandView, seat: usize) -> i32 {
    let (small_blind, big_blind) = blind_seats(view);
    let behind = players_behind_excluding(view, seat, [small_blind, big_blind]);
    if behind == 0 {
        4
    } else if behind <= 2 {
        5
    } else {
        6
    }
}

fn blind_seats(view: &HandView) -> (Option<usize>, Option<usize>) {
    let small_blind = view.events.iter().find_map(|event| {
        (event.street == crate::holdem::Street::Preflop
            && matches!(event.kind, crate::holdem::HandEventKind::SmallBlind))
        .then_some(event.seat)
        .flatten()
    });
    let big_blind = view.events.iter().find_map(|event| {
        (event.street == crate::holdem::Street::Preflop
            && matches!(event.kind, crate::holdem::HandEventKind::BigBlind))
        .then_some(event.seat)
        .flatten()
    });
    if small_blind.is_some() || big_blind.is_some() {
        return (small_blind, big_blind);
    }
    let order = live_order(view);
    if order.len() < 2 {
        return (None, None);
    }
    if order.len() == 2 {
        (order.first().copied(), order.get(1).copied())
    } else {
        (order.get(1).copied(), order.get(2).copied())
    }
}

fn active_opponents(view: &HandView, seat: usize) -> usize {
    let live = view
        .players
        .iter()
        .filter(|player| player.seat != seat && !player.folded)
        .count();
    live.max(1)
}

fn effective_stack(view: &HandView, seat: usize) -> Cents {
    let hero = view
        .players
        .iter()
        .find(|player| player.seat == seat)
        .map_or(0, |player| player.stack);
    let largest_opponent = view
        .players
        .iter()
        .filter(|player| player.seat != seat && !player.folded)
        .map(|player| player.stack)
        .max()
        .unwrap_or(hero);
    hero.min(largest_opponent)
}

fn all_in_or_wager(legal: &LegalActions, effective: Cents) -> Option<Action> {
    if legal
        .actions
        .iter()
        .any(|action| matches!(action, Action::AllIn))
    {
        Some(Action::AllIn)
    } else {
        sized_wager(legal, effective, effective)
    }
}

fn sized_wager(legal: &LegalActions, target: Cents, effective: Cents) -> Option<Action> {
    if target > 0
        && target * 3 >= effective * 2
        && legal
            .actions
            .iter()
            .any(|action| matches!(action, Action::AllIn))
    {
        return Some(Action::AllIn);
    }
    legal.actions.iter().find_map(|action| match action {
        Action::Bet { amount } => Some(Action::Bet {
            amount: sized_amount(legal, target, *amount),
        }),
        Action::Raise { amount } => Some(Action::Raise {
            amount: sized_amount(legal, target, *amount),
        }),
        _ => None,
    })
}

fn sized_amount(legal: &LegalActions, target: Cents, offered: Cents) -> Cents {
    match legal.wager {
        Some(bounds) => target.clamp(bounds.min, bounds.max),
        None => offered,
    }
}

fn estimate_equity(view: &HandView, hero_seat: usize, seed: u64, opponents: usize) -> f64 {
    let Some(hero) = view.your_hole_cards.as_ref() else {
        return 0.25;
    };
    if hero.len() != 2 {
        return 0.25;
    }
    let mut deck = Deck::seeded(0);
    let mut unseen: Vec<Card> = (0..52).filter_map(|_| deck.deal()).collect();
    unseen.retain(|card| !hero.contains(card) && !view.board.contains(card));
    let opponents = opponents.max(1);
    let opponent_seats: Vec<usize> = view
        .players
        .iter()
        .filter(|player| player.seat != hero_seat && !player.folded)
        .map(|player| player.seat)
        .collect();
    let mut tiers: Vec<OpponentTier> = opponent_seats
        .iter()
        .map(|seat| opponent_tier(view, *seat))
        .collect();
    if tiers.is_empty() {
        tiers.push(OpponentTier::Passive);
    }
    debug_assert_eq!(tiers.len(), opponents);
    let mut wins = 0.0;
    let street_samples = match view.board.len() {
        3 => 160,
        4 => 224,
        _ => 320,
    };
    let samples = match opponents {
        1 => street_samples,
        2 => street_samples * 3 / 4,
        _ => street_samples / 2,
    };
    let mut rng = StdRng::seed_from_u64(seed ^ 0x0053_4841_524b);
    for _ in 0..samples {
        let mut available = unseen.clone();
        let mut opponent_hands = Vec::with_capacity(opponents);
        for index in 0..opponents {
            opponent_hands.push(sample_opponent(
                &mut available,
                &mut rng,
                tiers.get(index).copied().unwrap_or(OpponentTier::Passive),
            ));
        }
        let mut board = view.board.clone();
        if available.len() < 5 - board.len() {
            continue;
        }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OpponentTier {
    Aggressive,
    Caller,
    Passive,
}

fn opponent_tier(view: &HandView, seat: usize) -> OpponentTier {
    let mut aggressive = false;
    let mut caller = false;
    let mut blind = false;
    for event in view.events.iter().filter(|event| event.seat == Some(seat)) {
        match event.kind {
            crate::holdem::HandEventKind::SmallBlind | crate::holdem::HandEventKind::BigBlind => {
                blind = true
            }
            crate::holdem::HandEventKind::Bet
            | crate::holdem::HandEventKind::Raise
            | crate::holdem::HandEventKind::AllIn => aggressive = true,
            crate::holdem::HandEventKind::Call => caller = true,
            _ => {}
        }
    }
    if aggressive {
        OpponentTier::Aggressive
    } else if caller && !blind {
        OpponentTier::Caller
    } else {
        OpponentTier::Passive
    }
}

fn range_accepts(tier: OpponentTier, cards: &[Card]) -> bool {
    match tier {
        OpponentTier::Aggressive => preflop_score_cards(cards) >= 7,
        OpponentTier::Caller => preflop_score_cards(cards) >= 4,
        OpponentTier::Passive => true,
    }
}

fn sample_opponent(available: &mut Vec<Card>, rng: &mut StdRng, tier: OpponentTier) -> Vec<Card> {
    if available.len() < 2 {
        return Vec::new();
    }
    let filtered = !matches!(tier, OpponentTier::Passive);
    for _ in 0..12 {
        let Some((first, second)) = pair_indices(available.len(), rng) else {
            return Vec::new();
        };
        let cards = vec![available[first], available[second]];
        if !filtered || range_accepts(tier, &cards) {
            return take_pair(available, first, second);
        }
    }
    pair_indices(available.len(), rng).map_or_else(Vec::new, |(first, second)| {
        take_pair(available, first, second)
    })
}

fn pair_indices(len: usize, rng: &mut StdRng) -> Option<(usize, usize)> {
    if len < 2 {
        return None;
    }
    let first = rng.gen_range(0..len);
    let offset = rng.gen_range(0..len - 1);
    let second = if offset >= first { offset + 1 } else { offset };
    Some((first, second))
}

fn take_pair(available: &mut Vec<Card>, first: usize, second: usize) -> Vec<Card> {
    let first_card = available.swap_remove(first);
    let second_card = available.swap_remove(if second > first { second - 1 } else { second });
    vec![first_card, second_card]
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
        view::{HandPlayerView, HandView, hand_view},
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
            button: 0,
            big_blind: 2,
            board: Vec::new(),
            your_hole_cards: Some(vec![trash, trash_two]),
            seats: Vec::new(),
            pot: 100,
            current_player: Some(0),
            legal_actions: None,
            summary: None,
            players: Vec::new(),
            events: Vec::new(),
            last_bet: 0,
            to_call: 0,
        };
        let legal = LegalActions {
            seat: 0,
            actions: vec![Action::Fold, Action::Call],
            to_call: 10,
            wager: None,
            wagers_capped: false,
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
    fn every_policy_shows_aggression_on_deterministic_wager_spots() {
        let ace_clubs = Card::from_str("Ac").unwrap();
        let ace_diamonds = Card::from_str("Ad").unwrap();
        let view = HandView {
            street: "Preflop".into(),
            button: 0,
            big_blind: 2,
            board: Vec::new(),
            your_hole_cards: Some(vec![ace_clubs, ace_diamonds]),
            seats: Vec::new(),
            pot: 300,
            current_player: Some(0),
            legal_actions: None,
            summary: None,
            players: Vec::new(),
            events: Vec::new(),
            last_bet: 200,
            to_call: 200,
        };
        let legal = LegalActions {
            seat: 0,
            actions: vec![
                Action::Fold,
                Action::Call,
                Action::Raise { amount: 600 },
                Action::AllIn,
            ],
            to_call: 200,
            wager: None,
            wagers_capped: false,
        };

        for kind in [
            BotKind::Fish,
            BotKind::Rock,
            BotKind::Grinder,
            BotKind::Shark,
        ] {
            let aggressive = (0..64).any(|seed| {
                matches!(
                    kind.act(&view, &legal, seed),
                    Action::Bet { .. } | Action::Raise { .. } | Action::AllIn
                )
            });
            assert!(aggressive, "{kind:?} never wagered across the corpus");
        }
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

    #[test]
    fn shark_position_counts_match_six_max_order() {
        let players = (0..6)
            .map(|seat| HandPlayerView {
                seat,
                stack: 100,
                contribution: 0,
                street_contribution: 0,
                folded: false,
                all_in: false,
                acted: false,
            })
            .collect();
        let view = HandView {
            street: "Preflop".into(),
            button: 0,
            big_blind: 2,
            board: Vec::new(),
            your_hole_cards: None,
            seats: Vec::new(),
            pot: 5,
            current_player: None,
            legal_actions: None,
            summary: None,
            players,
            events: Vec::new(),
            last_bet: 2,
            to_call: 2,
        };
        assert_eq!(players_behind(&view, 0), 2);
        assert_eq!(players_behind(&view, 5), 3);
        assert_eq!(players_behind(&view, 3), 5);
        assert_eq!(opening_threshold(&view, 0), 4);
        assert_eq!(opening_threshold(&view, 5), 5);
        assert_eq!(opening_threshold(&view, 3), 6);
        let mut acted = view.clone();
        acted.players[4].acted = true;
        acted.players[4].street_contribution = acted.last_bet;
        assert_eq!(players_behind(&acted, 3), 4);

        let mut flop = view;
        flop.board = vec![
            Card::from_str("2c").unwrap(),
            Card::from_str("7d").unwrap(),
            Card::from_str("Ks").unwrap(),
        ];
        assert_eq!(players_behind(&flop, 0), 0);
        assert_eq!(players_behind(&flop, 1), 5);
    }

    #[test]
    fn shark_commits_instead_of_leaving_dust() {
        let view = HandView {
            street: "Flop".into(),
            button: 0,
            big_blind: 2,
            board: vec![
                Card::from_str("As").unwrap(),
                Card::from_str("Kd").unwrap(),
                Card::from_str("2c").unwrap(),
            ],
            your_hole_cards: Some(vec![
                Card::from_str("Ah").unwrap(),
                Card::from_str("Ad").unwrap(),
            ]),
            seats: vec![],
            pot: 100,
            current_player: Some(0),
            legal_actions: None,
            summary: None,
            players: vec![
                HandPlayerView {
                    seat: 0,
                    stack: 50,
                    contribution: 0,
                    street_contribution: 0,
                    folded: false,
                    all_in: false,
                    acted: false,
                },
                HandPlayerView {
                    seat: 1,
                    stack: 60,
                    contribution: 0,
                    street_contribution: 0,
                    folded: false,
                    all_in: false,
                    acted: false,
                },
            ],
            events: Vec::new(),
            last_bet: 0,
            to_call: 0,
        };
        let legal = LegalActions {
            seat: 0,
            actions: vec![Action::Check, Action::Bet { amount: 10 }, Action::AllIn],
            to_call: 0,
            wager: Some(crate::holdem::WagerBounds {
                min: 10,
                max: 50,
                fixed: None,
            }),
            wagers_capped: false,
        };
        assert_eq!(shark(&view, &legal, 5), Action::AllIn);
    }

    #[test]
    fn shark_filters_observed_opponent_ranges() {
        let strong = vec![Card::from_str("As").unwrap(), Card::from_str("Kh").unwrap()];
        let trash = vec![Card::from_str("2c").unwrap(), Card::from_str("7d").unwrap()];
        assert!(range_accepts(OpponentTier::Aggressive, &strong));
        assert!(!range_accepts(OpponentTier::Aggressive, &trash));
        assert!(range_accepts(OpponentTier::Passive, &trash));
        let mut rng = StdRng::seed_from_u64(1);
        assert_eq!(pair_indices(1, &mut rng), None);
        let view = HandView {
            street: "Flop".into(),
            button: 0,
            big_blind: 2,
            board: Vec::new(),
            your_hole_cards: None,
            seats: Vec::new(),
            pot: 0,
            current_player: Some(0),
            legal_actions: None,
            summary: None,
            players: Vec::new(),
            events: vec![crate::holdem::HandEvent {
                street: crate::holdem::Street::Preflop,
                seat: Some(1),
                kind: crate::holdem::HandEventKind::BigBlind,
                amount: 2,
            }],
            last_bet: 0,
            to_call: 0,
        };
        assert_eq!(opponent_tier(&view, 1), OpponentTier::Passive);
    }
}
