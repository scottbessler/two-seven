//! Randomized model checks for the hold'em statechart (see STATECHART.md).
//! Plays many hands with random legal actions and asserts the machine
//! invariants hold on every trace.

use rand::{Rng, SeedableRng, seq::SliceRandom};
use two_seven::{
    holdem::{Action, Hand, HandEvent, HandEventKind, Street},
    money::Cents,
    table::Stakes,
};

fn play_random(mut hand: Hand, seed: u64) -> Hand {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    for _ in 0..500 {
        if hand.complete {
            break;
        }
        let legal = hand.legal_actions().unwrap_or_else(|| {
            panic!(
                "seed {seed}: street {:?}, current {:?}, players {:?}",
                hand.street,
                hand.current_player,
                hand.players
                    .iter()
                    .map(|p| (
                        p.seat,
                        p.folded,
                        p.all_in,
                        p.acted,
                        p.must_call,
                        p.street_contribution,
                        p.stack
                    ))
                    .collect::<Vec<_>>()
            )
        });
        let mut action = *legal
            .actions
            .choose(&mut rng)
            .expect("non-empty legal actions");
        if let (Action::Bet { .. } | Action::Raise { .. }, Some(bounds)) = (action, legal.wager) {
            let amount = rng.gen_range(bounds.min..=bounds.max);
            action = if matches!(action, Action::Bet { .. }) {
                Action::Bet { amount }
            } else {
                Action::Raise { amount }
            };
        }
        hand.apply_action(action).expect("legal action accepted");
    }
    assert!(hand.complete, "seed {seed} did not terminate");
    hand
}

fn is_voluntary(kind: HandEventKind) -> bool {
    matches!(
        kind,
        HandEventKind::Fold
            | HandEventKind::Check
            | HandEventKind::Call
            | HandEventKind::Bet
            | HandEventKind::Raise
            | HandEventKind::AllIn
    )
}

/// Deals must be strictly ordered and each betting street may appear once.
fn assert_streets_ordered(events: &[HandEvent], seed: u64) {
    let deals: Vec<Street> = events
        .iter()
        .filter(|event| event.kind == HandEventKind::Deal)
        .map(|event| event.street)
        .collect();
    let expected = [Street::Flop, Street::Turn, Street::River];
    assert!(
        deals.len() <= 3 && deals == expected[..deals.len()],
        "seed {seed}: deals out of order: {deals:?}"
    );
    // No action for street S may appear after a later street was dealt.
    let mut max_street = Street::Preflop;
    for event in events {
        if event.kind == HandEventKind::Deal {
            max_street = event.street;
        } else if is_voluntary(event.kind) {
            assert!(
                event.street >= max_street || event.street == Street::Complete,
                "seed {seed}: {event:?} logged after {max_street:?} was dealt"
            );
        }
    }
}

/// Before each deal, every player who is neither folded nor all in must have
/// a voluntary action logged on the street that just ended.
fn assert_everyone_acted_before_each_deal(hand: &Hand, seed: u64) {
    let events = &hand.events;
    for (index, deal) in events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.kind == HandEventKind::Deal)
    {
        let prior = &events[..index];
        let previous_street = match deal.street {
            Street::Flop => Street::Preflop,
            Street::Turn => Street::Flop,
            Street::River => Street::Turn,
            other => panic!("seed {seed}: deal on {other:?}"),
        };
        for player in &hand.players {
            let folded_before = prior
                .iter()
                .any(|event| event.kind == HandEventKind::Fold && event.seat == Some(player.seat));
            if folded_before {
                continue;
            }
            let all_in_before = prior.iter().any(|event| {
                event.seat == Some(player.seat)
                    && (event.kind == HandEventKind::AllIn
                        || hand
                            .players
                            .iter()
                            .find(|p| p.seat == player.seat)
                            .is_some_and(|p| p.all_in))
            });
            let acted_on_street = prior.iter().any(|event| {
                event.seat == Some(player.seat)
                    && event.street == previous_street
                    && is_voluntary(event.kind)
            });
            assert!(
                acted_on_street || all_in_before,
                "seed {seed}: seat {} never acted on {previous_street:?} before {:?} was dealt\n{events:#?}",
                player.seat,
                deal.street
            );
        }
    }
}

#[test]
fn random_legal_play_upholds_statechart_invariants() {
    for seed in 0..300u64 {
        let players = 2 + (seed % 5) as usize;
        let hand = Hand::new(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &vec![100; players],
            0,
            seed,
        );
        let hand = play_random(hand, seed);
        let awarded: Cents = hand
            .summary
            .as_ref()
            .expect("summary")
            .awards
            .iter()
            .map(|award| award.amount)
            .sum();
        let contributed: Cents = hand.players.iter().map(|player| player.contribution).sum();
        assert_eq!(awarded, contributed, "seed {seed}: chips not conserved");
        assert!(
            matches!(hand.street, Street::Showdown | Street::Complete),
            "seed {seed}: terminal street {:?}",
            hand.street
        );
        assert_eq!(hand.current_player, None, "seed {seed}");
        assert_streets_ordered(&hand.events, seed);
        assert_everyone_acted_before_each_deal(&hand, seed);
    }
}

#[test]
fn random_legal_play_upholds_invariants_in_limit_games() {
    for seed in 0..150u64 {
        let players = 2 + (seed % 5) as usize;
        let hand = Hand::new(
            Stakes::Limit {
                small_bet: 2,
                big_bet: 4,
            },
            &vec![50; players],
            0,
            seed,
        );
        let hand = play_random(hand, seed);
        let awarded: Cents = hand
            .summary
            .as_ref()
            .expect("summary")
            .awards
            .iter()
            .map(|award| award.amount)
            .sum();
        let contributed: Cents = hand.players.iter().map(|player| player.contribution).sum();
        assert_eq!(awarded, contributed, "seed {seed}: chips not conserved");
        assert_streets_ordered(&hand.events, seed);
        assert_everyone_acted_before_each_deal(&hand, seed);
    }
}

#[test]
fn random_short_stacks_with_antes_terminate_and_conserve_chips() {
    for seed in 0..150u64 {
        let players = 2 + (seed % 5) as usize;
        let stacks: Vec<(usize, Cents)> = (0..players)
            .map(|seat| (seat, 3 + (seed as Cents * 7 + seat as Cents * 13) % 40))
            .collect();
        let hand = Hand::new_with_seats_and_ante(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &stacks,
            0,
            seed,
            1,
        );
        let hand = play_random(hand, seed);
        let awarded: Cents = hand
            .summary
            .as_ref()
            .expect("summary")
            .awards
            .iter()
            .map(|award| award.amount)
            .sum();
        let contributed: Cents = hand.players.iter().map(|player| player.contribution).sum();
        assert_eq!(awarded, contributed, "seed {seed}: chips not conserved");
        assert_streets_ordered(&hand.events, seed);
    }
}
