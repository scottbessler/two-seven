use crate::{
    cards::{Card, Rank},
    eval::{Category, evaluate_showdown},
    poker::{Action, LegalActions},
    table::{Bot, BotKind},
    view::HandView,
};
use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};

const MAX_BOT_STREET_RAISES: usize = 3;

impl BotKind {
    /// How the kind plays, with its reference tuning. A seated regular acts
    /// through [`Bot::act`] instead, so a shark brings their own.
    pub fn act(self, view: &HandView, legal: &LegalActions, seed: u64) -> Action {
        act(Bot::new(self, 0), view, legal, seed)
    }
}

/// One regular's decision: their kind's style, and for a shark the tuning that
/// tells them apart from the rest of the house's sharks (§V62).
pub fn act(bot: Bot, view: &HandView, legal: &LegalActions, seed: u64) -> Action {
    let action = match bot.kind {
        BotKind::Fish => fish(view, legal, seed),
        BotKind::Rock => rock(view, legal),
        BotKind::Grinder => grinder(view, legal),
        BotKind::Shark => shark_with(&SharkParams::for_regular(bot.seat), view, legal, seed),
    };
    let action = if matches!(bot.kind, BotKind::Shark) {
        action
    } else {
        raise_once_per_street(action, view, legal)
    };
    avoid_excessive_raise(avoid_free_fold(action, legal), view, legal)
}

fn current_street(view: &HandView) -> crate::poker::Street {
    match view.board.len() {
        0 => crate::poker::Street::Preflop,
        3 => crate::poker::Street::Flop,
        4 => crate::poker::Street::Turn,
        _ => crate::poker::Street::River,
    }
}

/// The simple kinds put their own money in at most once a street: raised over
/// their own wager, they call rather than trade raises with whoever answered
/// it (§V79). The shark reads the raise and decides for itself.
fn raise_once_per_street(action: Action, view: &HandView, legal: &LegalActions) -> Action {
    let street = current_street(view);
    let already_wagered = view.events.iter().any(|event| {
        event.street == street
            && event.seat == Some(legal.seat)
            && matches!(
                event.kind,
                crate::poker::HandEventKind::Bet | crate::poker::HandEventKind::Raise
            )
    });
    if already_wagered && matches!(action, Action::Raise { .. }) {
        first_calling(legal)
    } else {
        action
    }
}

fn avoid_excessive_raise(action: Action, view: &HandView, legal: &LegalActions) -> Action {
    let street = current_street(view);
    let raises = view
        .events
        .iter()
        .filter(|event| {
            event.street == street && matches!(event.kind, crate::poker::HandEventKind::Raise)
        })
        .count();
    if raises >= MAX_BOT_STREET_RAISES && matches!(action, Action::Raise { .. }) {
        first_calling(legal)
    } else {
        action
    }
}

fn avoid_free_fold(action: Action, legal: &LegalActions) -> Action {
    if matches!(action, Action::Fold) && legal.actions.contains(&Action::Check) {
        Action::Check
    } else {
        action
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
        holds_a_pair(cards) || cards.iter().all(|card| card.rank >= Rank::Jack)
    });
    let made = made_category(view).is_some_and(|category| category >= Category::Pair);
    if !premium && !made && legal.to_call > 0 {
        return first(legal, Action::Fold);
    }
    if premium || made {
        return wager_or_call(view, legal);
    }
    first_calling(legal)
}

fn grinder(view: &HandView, legal: &LegalActions) -> Action {
    let preflop_strong = view.board.is_empty()
        && view.your_hole_cards.as_ref().is_some_and(|cards| {
            holds_a_pair(cards)
                || cards.iter().any(|card| card.rank >= Rank::Ace)
                || all_broadway(cards)
        });
    if preflop_strong {
        return wager_or_call(view, legal);
    }
    let strong = made_category(view).is_some_and(|category| category >= Category::TwoPair);
    if strong {
        return wager_or_call(view, legal);
    }
    if legal.to_call > 0 && made_category(view).is_none() {
        return first(legal, Action::Fold);
    }
    first_calling(legal)
}

pub mod shark;
#[cfg(test)]
use shark::{
    OpponentTier, opening_threshold, opponent_tier, players_behind, range_accepts, sample_indices,
    shark,
};
pub use shark::{SharkFrequency, SharkParams, SharkRatio, shark_with};
/// Every card a jack or better. An empty hand is not: `all` over nothing is
/// true, and a seat with no cards holds nothing premium.
fn all_broadway(cards: &[Card]) -> bool {
    !cards.is_empty() && cards.iter().all(|card| card.rank >= Rank::Jack)
}

/// Two of a kind anywhere in the hand. Hold'em's pocket pair, and the Omaha
/// hand that holds one among its four.
fn holds_a_pair(cards: &[Card]) -> bool {
    cards.iter().enumerate().any(|(index, card)| {
        cards[index + 1..]
            .iter()
            .any(|other| other.rank == card.rank)
    })
}

/// The hand this seat has made, read the way its game reads one -- so an Omaha
/// bot never counts a flush it is holding four cards of but may only play two.
fn made_category(view: &HandView) -> Option<Category> {
    let hole = view.your_hole_cards.as_ref()?;
    if hole.len() != view.variant.hole_cards() || view.board.len() < 3 {
        return None;
    }
    Some(
        evaluate_showdown(view.variant, hole, &view.board)
            .rank
            .category,
    )
}

/// Bets or raises three quarters of the pot after calling, rounded up to the
/// big blind -- a wager that means something, rather than the minimum that
/// invites the minimum back -- or calls when no wager is offered.
fn wager_or_call(view: &HandView, legal: &LegalActions) -> Action {
    let unit = view.big_blind.max(1);
    let target = legal.to_call + (view.pot + legal.to_call) * 3 / 4;
    let target = (target + unit - 1) / unit * unit;
    let amount = |offered| match legal.wager {
        Some(bounds) => target.clamp(bounds.min, bounds.max),
        None => offered,
    };
    legal
        .actions
        .iter()
        .find_map(|action| match *action {
            Action::Bet { amount: offered } => Some(Action::Bet {
                amount: amount(offered),
            }),
            Action::Raise { amount: offered } => Some(Action::Raise {
                amount: amount(offered),
            }),
            _ => None,
        })
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
        poker::{Hand, HandEvent, WagerBounds},
        table::Stakes,
        view::{HandPlayerView, HandView, hand_view},
    };
    use std::str::FromStr;

    /// The house has to be able to sit at either game. A bot that reads a
    /// four-card hand as nothing folds every pot it is offered, so this is the
    /// check that they are playing Omaha rather than surviving it (§V67).
    #[test]
    fn every_kind_plays_omaha_legally_and_puts_money_in() {
        for kind in BotKind::ALL {
            let mut voluntary = 0;
            for seed in 0..40 {
                let mut hand = Hand::new_variant(
                    crate::table::Variant::Omaha,
                    Stakes::NoLimit {
                        small_blind: 1,
                        big_blind: 2,
                    },
                    &[(0, 100), (1, 100), (2, 100)],
                    0,
                    seed,
                    0,
                );
                for turn in 0..100 {
                    if hand.complete {
                        break;
                    }
                    if hand.advance_runout() {
                        continue;
                    }
                    let legal = hand.legal_actions().expect("action");
                    let view = hand_view(&hand, Some(legal.seat), &[]);
                    assert_eq!(view.variant, crate::table::Variant::Omaha);
                    let action = kind.act(&view, &legal, seed + turn);
                    if matches!(
                        action,
                        Action::Call | Action::Bet { .. } | Action::Raise { .. } | Action::AllIn
                    ) {
                        voluntary += 1;
                    }
                    hand.apply_action(action).unwrap_or_else(|error| {
                        panic!("{kind:?} seed {seed} turn {turn}: {action:?} rejected: {error}")
                    });
                }
            }
            assert!(
                voluntary > 0,
                "{kind:?} never put a chip in across forty Omaha hands"
            );
        }
    }

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
                    // A parked runout prompts nobody: the board runs out on
                    // its own before anyone can act again (§V59).
                    if hand.advance_runout() {
                        continue;
                    }
                    let legal = hand.legal_actions().expect("action");
                    let view = hand_view(&hand, Some(legal.seat), &[]);
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
    fn every_shark_regular_plays_legally_and_none_plays_the_same() {
        let mut lines: Vec<(u8, Vec<Action>)> = Vec::new();
        for regular in 0..BotKind::Shark.regulars() {
            let bot = Bot::new(BotKind::Shark, regular);
            let mut actions = Vec::new();
            for seed in 0..40 {
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
                    if hand.advance_runout() {
                        continue;
                    }
                    let legal = hand.legal_actions().expect("action");
                    let view = hand_view(&hand, Some(legal.seat), &[]);
                    let action = bot.act(&view, &legal, seed + turn);
                    actions.push(action);
                    hand.apply_action(action).unwrap_or_else(|error| {
                        panic!("{bot} seed {seed} turn {turn}: {action:?} rejected: {error}")
                    });
                }
            }
            lines.push((regular, actions));
        }
        // Regular 0 is the reference build; the other eight each sit on their
        // own (looseness, aggression) pair, so no two of them play alike.
        for (index, (regular, actions)) in lines.iter().enumerate() {
            for (other, other_actions) in &lines[index + 1..] {
                assert_ne!(
                    actions, other_actions,
                    "shark {regular} and shark {other} play the same corpus identically"
                );
            }
        }
        assert_eq!(
            SharkParams::for_regular(0),
            SharkParams::DEFAULT,
            "the first shark is the reference tuning"
        );
        assert_eq!(
            SharkParams::for_regular(BotKind::Shark.regulars()),
            SharkParams::for_regular(0),
            "tuning wraps with the roster"
        );
    }

    #[test]
    fn bots_stop_reraising_after_three_street_raises() {
        let hand = Hand::new(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &[100, 100, 100],
            0,
            7,
        );
        let legal = hand.legal_actions().unwrap();
        let mut view = hand_view(&hand, Some(legal.seat), &[]);
        view.events
            .extend((0..MAX_BOT_STREET_RAISES).map(|_| HandEvent {
                street: crate::poker::Street::Preflop,
                seat: Some(1),
                kind: crate::poker::HandEventKind::Raise,
                amount: 4,
            }));

        for kind in [
            BotKind::Fish,
            BotKind::Rock,
            BotKind::Grinder,
            BotKind::Shark,
        ] {
            for seed in 0..64 {
                assert!(
                    !matches!(kind.act(&view, &legal, seed), Action::Raise { .. }),
                    "{kind:?} must stop re-raising after {MAX_BOT_STREET_RAISES} raises"
                );
            }
        }
    }

    /// Two rocks holding something used to min-raise each other until the
    /// three-raise backstop stopped them, on every street (§B33).
    #[test]
    fn simple_bots_never_raise_over_their_own_street_wager() {
        for kind in [BotKind::Fish, BotKind::Rock, BotKind::Grinder] {
            for seed in 0..200 {
                let mut hand = Hand::new(
                    Stakes::NoLimit {
                        small_blind: 1,
                        big_blind: 2,
                    },
                    &[1_000, 1_000, 1_000],
                    0,
                    seed,
                );
                for turn in 0..200 {
                    if hand.complete {
                        break;
                    }
                    if hand.advance_runout() {
                        continue;
                    }
                    let legal = hand.legal_actions().expect("action");
                    let view = hand_view(&hand, Some(legal.seat), &[]);
                    hand.apply_action(kind.act(&view, &legal, seed + turn))
                        .unwrap();
                }
                for event in &hand.events {
                    let wagers = hand
                        .events
                        .iter()
                        .filter(|other| {
                            other.street == event.street
                                && other.seat == event.seat
                                && matches!(
                                    other.kind,
                                    crate::poker::HandEventKind::Bet
                                        | crate::poker::HandEventKind::Raise
                                )
                        })
                        .count();
                    assert!(
                        wagers <= 1,
                        "{kind:?} seed {seed}: seat {:?} wagered {wagers} times on {:?}",
                        event.seat,
                        event.street
                    );
                }
            }
        }
    }

    #[test]
    fn rocks_and_grinders_size_their_wagers_off_the_pot() {
        let view = HandView {
            variant: crate::table::Variant::Holdem,
            street: "Preflop".into(),
            button: 0,
            big_blind: 2,
            board: Vec::new(),
            your_hole_cards: Some(vec![
                Card::from_str("Ac").unwrap(),
                Card::from_str("Ad").unwrap(),
            ]),
            seats: Vec::new(),
            pot: 300,
            current_player: Some(0),
            legal_actions: None,
            summary: None,
            players: Vec::new(),
            events: Vec::new(),
            last_bet: 200,
            to_call: 200,
            awaiting_advance: false,
            runout_leaders: Vec::new(),
            runout_odds: Vec::new(),
        };
        let legal = LegalActions {
            seat: 0,
            actions: vec![
                Action::Fold,
                Action::Call,
                Action::Raise { amount: 400 },
                Action::AllIn,
            ],
            to_call: 200,
            wager: Some(WagerBounds {
                min: 400,
                max: 10_000,
                fixed: None,
            }),
            wagers_capped: false,
        };
        // Call 200, then three quarters of the 500 that makes: 575, rounded
        // up to the big blind.
        assert_eq!(rock(&view, &legal), Action::Raise { amount: 576 });
        assert_eq!(grinder(&view, &legal), Action::Raise { amount: 576 });
    }

    #[test]
    fn bots_check_instead_of_folding_free_actions() {
        let view = HandView {
            variant: crate::table::Variant::Holdem,
            street: "Flop".into(),
            button: 0,
            big_blind: 2,
            board: vec![
                Card::from_str("Ah").unwrap(),
                Card::from_str("7c").unwrap(),
                Card::from_str("2s").unwrap(),
            ],
            your_hole_cards: Some(vec![
                Card::from_str("As").unwrap(),
                Card::from_str("Kd").unwrap(),
            ]),
            seats: Vec::new(),
            pot: 100,
            current_player: Some(0),
            legal_actions: None,
            summary: None,
            players: vec![
                HandPlayerView {
                    seat: 0,
                    stack: 100,
                    contribution: 0,
                    street_contribution: 0,
                    folded: false,
                    all_in: false,
                    acted: false,
                },
                HandPlayerView {
                    seat: 1,
                    stack: 100,
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
            awaiting_advance: false,
            runout_leaders: Vec::new(),
            runout_odds: Vec::new(),
        };
        let legal = LegalActions {
            seat: 0,
            actions: vec![Action::Fold, Action::Check, Action::Bet { amount: 10 }],
            to_call: 0,
            wager: Some(WagerBounds {
                min: 10,
                max: 100,
                fixed: None,
            }),
            wagers_capped: false,
        };

        for kind in [
            BotKind::Fish,
            BotKind::Rock,
            BotKind::Grinder,
            BotKind::Shark,
        ] {
            for seed in 0..64 {
                assert_ne!(
                    kind.act(&view, &legal, seed),
                    Action::Fold,
                    "{kind:?} folded when check was free at seed {seed}"
                );
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
            variant: crate::table::Variant::Holdem,
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
            awaiting_advance: false,
            runout_leaders: Vec::new(),
            runout_odds: Vec::new(),
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
            variant: crate::table::Variant::Holdem,
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
            variant: crate::table::Variant::Holdem,
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
            awaiting_advance: false,
            runout_leaders: Vec::new(),
            runout_odds: Vec::new(),
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
                while hand.street == crate::poker::Street::Preflop && !hand.complete {
                    if hand.advance_runout() {
                        continue;
                    }
                    let legal = hand.legal_actions().expect("preflop action");
                    let view = hand_view(&hand, Some(legal.seat), &[]);
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
            variant: crate::table::Variant::Holdem,
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
            awaiting_advance: false,
            runout_leaders: Vec::new(),
            runout_odds: Vec::new(),
        };
        assert_eq!(players_behind(&view, 0), 2);
        assert_eq!(players_behind(&view, 5), 3);
        assert_eq!(players_behind(&view, 3), 5);
        assert_eq!(opening_threshold(&SharkParams::DEFAULT, &view, 0), 3);
        assert_eq!(opening_threshold(&SharkParams::DEFAULT, &view, 5), 4);
        assert_eq!(opening_threshold(&SharkParams::DEFAULT, &view, 3), 5);
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
            variant: crate::table::Variant::Holdem,
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
            awaiting_advance: false,
            runout_leaders: Vec::new(),
            runout_odds: Vec::new(),
        };
        let legal = LegalActions {
            seat: 0,
            actions: vec![Action::Check, Action::Bet { amount: 10 }, Action::AllIn],
            to_call: 0,
            wager: Some(crate::poker::WagerBounds {
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
        assert!(range_accepts(
            &SharkParams::DEFAULT,
            OpponentTier::Aggressive,
            &strong
        ));
        assert!(!range_accepts(
            &SharkParams::DEFAULT,
            OpponentTier::Aggressive,
            &trash
        ));
        assert!(range_accepts(
            &SharkParams::DEFAULT,
            OpponentTier::Passive,
            &trash
        ));
        let mut rng = StdRng::seed_from_u64(1);
        assert_eq!(sample_indices(1, 2, &mut rng), None);
        let view = HandView {
            variant: crate::table::Variant::Holdem,
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
            events: vec![crate::poker::HandEvent {
                street: crate::poker::Street::Preflop,
                seat: Some(1),
                kind: crate::poker::HandEventKind::BigBlind,
                amount: 2,
            }],
            last_bet: 0,
            to_call: 0,
            awaiting_advance: false,
            runout_leaders: Vec::new(),
            runout_odds: Vec::new(),
        };
        assert_eq!(opponent_tier(&view, 1), OpponentTier::Passive);
    }
}
