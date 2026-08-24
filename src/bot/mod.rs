use crate::{
    cards::Rank,
    eval::{Category, evaluate},
    holdem::{Action, LegalActions},
    table::BotKind,
    view::HandView,
};
use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};

const MAX_BOT_STREET_RAISES: usize = 3;

impl BotKind {
    pub fn act(self, view: &HandView, legal: &LegalActions, seed: u64) -> Action {
        let action = match self {
            Self::Fish => fish(view, legal, seed),
            Self::Rock => rock(view, legal),
            Self::Grinder => grinder(view, legal),
            Self::Shark => shark_with(&SharkParams::DEFAULT, view, legal, seed),
        };
        avoid_excessive_raise(avoid_free_fold(action, legal), view, legal)
    }
}

fn avoid_excessive_raise(action: Action, view: &HandView, legal: &LegalActions) -> Action {
    let street = match view.board.len() {
        0 => crate::holdem::Street::Preflop,
        3 => crate::holdem::Street::Flop,
        4 => crate::holdem::Street::Turn,
        _ => crate::holdem::Street::River,
    };
    let raises = view
        .events
        .iter()
        .filter(|event| {
            event.street == street && matches!(event.kind, crate::holdem::HandEventKind::Raise)
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

pub mod shark;
#[cfg(test)]
use shark::{
    OpponentTier, opening_threshold, opponent_tier, pair_indices, players_behind, range_accepts,
    shark,
};
pub use shark::{SharkFrequency, SharkParams, SharkRatio, shark_with};
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
        holdem::{Hand, HandEvent, WagerBounds},
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
        let mut view = hand_view(&hand, Some(legal.seat));
        view.events
            .extend((0..MAX_BOT_STREET_RAISES).map(|_| HandEvent {
                street: crate::holdem::Street::Preflop,
                seat: Some(1),
                kind: crate::holdem::HandEventKind::Raise,
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

    #[test]
    fn bots_check_instead_of_folding_free_actions() {
        let view = HandView {
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
