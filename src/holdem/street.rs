//! Hand machine (street progression).
//!
//! Top level of the statechart (see STATECHART.md). States are the variants
//! of [`Street`]: the betting states `Preflop -> Flop -> Turn -> River` each
//! host one betting round machine, and the terminal resolution happens in
//! `Showdown` or, when a fold leaves a single live player, directly in
//! `Complete`.
//!
//! Transitions fire only when the hosted betting round reports
//! [`RoundStatus::Complete`](super::RoundStatus::Complete). On entry to each
//! post-flop street the machine deals board cards, resets the round state,
//! and seats the first actor.
//!
//! If nobody can act -- everyone live is all in -- the machine does not run the
//! board out. It parks in `Runout`: betting is closed, the board is incomplete,
//! and it waits for [`Hand::advance_runout`] to deal exactly one street. The
//! result is therefore computed as the last card lands rather than up front and
//! hidden afterwards, which is what keeps a showdown from being spoiled (§V59).

use super::{Award, Hand, HandEvent, HandEventKind, HandSummary, Street};
use crate::table::Stakes;

/// The transition the hand machine takes when a betting round completes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreetTransition {
    /// Deal the next street and open a new betting round.
    Deal(Street),
    /// River betting finished: evaluate hands and award pots.
    Showdown,
    /// Only one live player remains: award the pot without showdown.
    FoldWin,
}

impl Hand {
    /// The transition that fires when the current betting round completes.
    pub fn street_transition(&self) -> StreetTransition {
        if self.live_count() <= 1 {
            return StreetTransition::FoldWin;
        }
        match self.street {
            Street::Preflop => StreetTransition::Deal(Street::Flop),
            Street::Flop => StreetTransition::Deal(Street::Turn),
            Street::Turn => StreetTransition::Deal(Street::River),
            Street::River | Street::Showdown | Street::Complete => StreetTransition::Showdown,
        }
    }

    /// Fire the street transition: deal the next street (entering a fresh
    /// betting round), or resolve the hand at showdown / by fold-win.
    pub(crate) fn advance_street(&mut self) {
        match self.street_transition() {
            StreetTransition::FoldWin => {
                self.finish_fold();
                return;
            }
            StreetTransition::Showdown => {
                if !self.complete {
                    self.showdown();
                }
                return;
            }
            StreetTransition::Deal(next) => {
                // Betting is closed for good, so the rest of the board is a
                // runout. Park instead of dealing it: each street waits for an
                // explicit advance, and the result is computed as the last card
                // lands rather than now and hidden afterwards (§V59).
                if self.betting_is_closed() {
                    if self.runout_from.is_none() {
                        self.runout_from = Some(self.board.len());
                    }
                    self.current_player = None;
                    self.awaiting_advance = true;
                    return;
                }
                self.deal_street(next);
            }
        }
        self.enter_betting_round();
    }

    /// Deal one street's cards and log the deal.
    fn deal_street(&mut self, next: Street) {
        let cards = if next == Street::Flop { 3 } else { 1 };
        for _ in 0..cards {
            self.board.push(self.deck.deal().expect("deck has cards"));
        }
        self.street = next;
        self.events.push(HandEvent {
            street: self.street,
            seat: None,
            kind: HandEventKind::Deal,
            amount: 0,
        });
    }

    /// Deal the next street of a parked runout: one call, one street. Returns
    /// whether anything moved, so a press that beats the deadline and the
    /// deadline itself cannot deal the same street twice. Dealing the river
    /// resolves the hand, which is the only place a result comes from once
    /// betting is closed.
    pub fn advance_runout(&mut self) -> bool {
        if !self.awaits_runout() {
            return false;
        }
        let next = match self.street {
            Street::Preflop => Street::Flop,
            Street::Flop => Street::Turn,
            Street::Turn => Street::River,
            Street::River | Street::Showdown | Street::Complete => return false,
        };
        self.awaiting_advance = false;
        self.deal_street(next);
        // Dealing the river resolves the hand. The result pause that follows is
        // the board's moment -- the river and what it decided are read together
        // -- so the runout does not hold a finished board (§V59).
        self.enter_betting_round();
        true
    }

    /// No further betting is possible once at most one live player still has
    /// chips to put in; the rest of the board simply runs out.
    fn betting_is_closed(&self) -> bool {
        self.players
            .iter()
            .filter(|player| !player.folded && !player.all_in && player.stack > 0)
            .count()
            <= 1
    }

    /// Entry action for a post-flop betting street: reset the round machine
    /// and seat the first actor clockwise from the button.
    fn enter_betting_round(&mut self) {
        self.last_bet = 0;
        self.last_raise = match self.stakes {
            Stakes::Limit { small_bet, big_bet } => {
                if self.street <= Street::Flop {
                    small_bet
                } else {
                    big_bet
                }
            }
            Stakes::NoLimit { big_blind, .. } => big_blind,
        };
        self.wagers = 0;
        for player in &mut self.players {
            player.street_contribution = 0;
            player.acted = false;
            player.must_call = false;
        }
        self.current_player = self.next_actor(self.button);
        if self.current_player.is_none() {
            self.advance_street();
        }
    }

    /// Terminal transition when a fold leaves one live player: refund any
    /// uncalled excess and award the pot without a showdown.
    pub(crate) fn finish_fold(&mut self) {
        self.return_uncalled();
        let winner = self
            .players
            .iter()
            .find(|player| !player.folded)
            .map(|player| player.seat)
            .expect("one live player");
        let amount = self.total_contributions();
        let i = self.player_index(winner);
        self.players[i].stack += amount;
        self.events.push(HandEvent {
            street: Street::Complete,
            seat: Some(winner),
            kind: HandEventKind::Award,
            amount,
        });
        self.summary = Some(HandSummary {
            board: self.board.clone(),
            results: Vec::new(),
            awards: vec![Award {
                seat: winner,
                amount,
            }],
            contributions: self
                .players
                .iter()
                .map(|player| (player.seat, player.contribution))
                .collect(),
            revealed_hole_cards: Vec::new(),
            events: self.events.clone(),
            // A fold win shows no runout: there is nothing left to watch.
            runout_from: self.board.len(),
            runout: Vec::new(),
            reveal_leaders: Vec::new(),
            reveal_odds: Vec::new(),
            stacks_before_awards: self
                .players
                .iter()
                .map(|player| {
                    (
                        player.seat,
                        if player.seat == winner {
                            player.stack - amount
                        } else {
                            player.stack
                        },
                    )
                })
                .collect(),
        });
        self.complete = true;
        self.street = Street::Complete;
        self.current_player = None;
        self.pot = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::holdem::Action;
    use crate::table::Stakes;

    fn no_limit(stacks: &[i64], seed: u64) -> Hand {
        Hand::new(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            stacks,
            0,
            seed,
        )
    }

    fn check_around(hand: &mut Hand) {
        let street = hand.street;
        while hand.street == street && !hand.complete {
            let legal = hand.legal_actions().unwrap();
            let action = if legal.actions.contains(&Action::Check) {
                Action::Check
            } else {
                Action::Call
            };
            hand.apply_action(action).unwrap();
        }
    }

    #[test]
    fn streets_progress_in_order_with_correct_board_sizes() {
        let mut hand = no_limit(&[100, 100, 100], 5);
        assert_eq!(hand.street, Street::Preflop);
        assert!(hand.board.is_empty());
        check_around(&mut hand);
        assert_eq!(hand.street, Street::Flop);
        assert_eq!(hand.board.len(), 3);
        check_around(&mut hand);
        assert_eq!(hand.street, Street::Turn);
        assert_eq!(hand.board.len(), 4);
        check_around(&mut hand);
        assert_eq!(hand.street, Street::River);
        assert_eq!(hand.board.len(), 5);
        check_around(&mut hand);
        assert!(hand.complete);
        assert_eq!(hand.street, Street::Showdown);
        assert!(hand.summary.is_some());
    }

    #[test]
    fn each_street_deal_is_logged_once() {
        let mut hand = no_limit(&[100, 100], 8);
        while !hand.complete {
            check_around(&mut hand);
        }
        for street in [Street::Flop, Street::Turn, Street::River] {
            assert_eq!(
                hand.events
                    .iter()
                    .filter(
                        |event| event.kind == super::HandEventKind::Deal && event.street == street
                    )
                    .count(),
                1
            );
        }
    }

    #[test]
    fn fold_win_completes_from_any_street() {
        for folds_after in 0..3 {
            let mut hand = no_limit(&[100, 100], 14 + folds_after);
            for _ in 0..folds_after {
                check_around(&mut hand);
            }
            hand.apply_action(Action::Fold).unwrap();
            assert!(hand.complete);
            assert_eq!(hand.street, Street::Complete);
            assert_eq!(hand.summary.as_ref().unwrap().awards.len(), 1);
        }
    }

    #[test]
    fn a_showdown_says_who_is_ahead_and_what_they_held_before_the_pot_moved() {
        let mut hand = Hand::new(
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            &[10_000, 10_000],
            0,
            77,
        );
        hand.apply_action(Action::AllIn).unwrap();
        hand.apply_action(Action::Call).unwrap();
        // The result exists only once the board is out (§V59).
        assert!(!hand.leaders_now().is_empty(), "a leader before any board");
        while hand.advance_runout() {}
        let summary = hand.summary.as_ref().expect("summary");

        // Somebody is ahead as the hands turn over, before any board lands.
        assert_eq!(summary.runout_from, 0);
        assert!(
            !summary.reveal_leaders.is_empty(),
            "a leader from the moment the cards are face up"
        );

        // Stacks read as the hand left them, not as the pot left them.
        let pot: crate::money::Cents = summary.awards.iter().map(|award| award.amount).sum();
        let held: crate::money::Cents = summary.stacks_before_awards.values().sum();
        assert_eq!(
            held + pot,
            hand.players
                .iter()
                .map(|player| player.stack)
                .sum::<crate::money::Cents>(),
            "every chip is either still in front of somebody or in the pot"
        );
        for (seat, stack) in &summary.stacks_before_awards {
            assert!(*stack >= 0, "seat {seat} cannot hold less than nothing");
        }
        // Both were all in, so neither has anything left until the pot moves.
        assert!(
            summary
                .stacks_before_awards
                .values()
                .all(|stack| *stack == 0)
        );
    }

    #[test]
    fn all_in_runout_records_every_street_and_who_leads() {
        let mut hand = Hand::new(
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            &[10_000, 10_000],
            0,
            77,
        );
        hand.apply_action(Action::AllIn).unwrap();
        hand.apply_action(Action::Call).unwrap();
        // Every street of the runout is advanced explicitly (§V59); the record
        // of who led on each is written as the hand resolves.
        while hand.advance_runout() {}
        assert!(hand.complete);
        let summary = hand.summary.as_ref().expect("summary");
        // Betting closed before the flop, so the whole board is a runout.
        assert_eq!(summary.runout_from, 0);
        assert_eq!(
            summary
                .runout
                .iter()
                .map(|step| step.cards)
                .collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        for step in &summary.runout {
            assert!(
                !step.leaders.is_empty(),
                "someone must lead at every street: {step:?}"
            );
            assert!(
                step.leaders
                    .iter()
                    .all(|seat| hand.players.iter().any(|player| player.seat == *seat)),
                "leaders must be seats in the hand: {step:?}"
            );
        }
        // The final leaders are the seats that actually take the pot.
        let last = summary.runout.last().expect("river step");
        let winners: Vec<usize> = summary.awards.iter().map(|award| award.seat).collect();
        assert!(last.leaders.iter().all(|seat| winners.contains(seat)));
    }

    #[test]
    fn a_contested_street_is_not_a_runout() {
        let mut hand = Hand::new(
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            &[10_000, 10_000],
            0,
            78,
        );
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Check).unwrap();
        assert_eq!(hand.street, Street::Flop);
        assert_eq!(hand.runout_from, None, "both players can still bet");
    }

    /// §V59: betting closing with the board incomplete parks the hand. No
    /// result exists until the last card is dealt, so there is nothing to
    /// embargo and nothing a client has to be trusted to replay.
    #[test]
    fn v59_an_all_in_runout_parks_until_each_street_is_advanced() {
        let mut hand = no_limit(&[10_000, 10_000], 77);
        hand.apply_action(Action::AllIn).unwrap();
        hand.apply_action(Action::Call).unwrap();

        // Betting is closed preflop: cards face up, board untouched, no result.
        assert!(hand.awaits_runout(), "parked on the reveal, not resolved");
        assert!(!hand.complete);
        assert!(hand.summary.is_none(), "no result exists yet");
        assert!(hand.board.is_empty(), "the flop waits to be advanced");
        assert_eq!(hand.exposed_hole_cards().len(), 2, "hands are turned over");
        assert!(!hand.leaders_now().is_empty(), "somebody is ahead already");

        for (streets, cards) in [(1, 3), (2, 4)] {
            assert!(hand.advance_runout(), "advance {streets} deals a street");
            assert_eq!(hand.board.len(), cards);
            assert!(hand.awaits_runout(), "still parked before the river");
            assert!(hand.summary.is_none(), "still no result at {cards} cards");
        }
        // The river is the card that resolves the hand: it and the result it
        // decided are read together, in the result pause that follows.
        assert!(hand.advance_runout());
        assert_eq!(hand.board.len(), 5);
        assert!(hand.complete, "the river settles the hand");
        assert!(hand.summary.is_some(), "the result arrives with the river");
        assert!(!hand.awaits_runout(), "a finished board is never held");
        assert!(
            !hand.advance_runout(),
            "a resolved hand advances no further"
        );
    }

    /// A press that beats the deadline and the deadline itself both call
    /// `advance_runout`; only one of them may deal the street.
    #[test]
    fn v59_advancing_twice_over_deals_one_street() {
        let mut hand = no_limit(&[10_000, 10_000], 91);
        hand.apply_action(Action::AllIn).unwrap();
        hand.apply_action(Action::Call).unwrap();
        assert!(hand.advance_runout());
        let board = hand.board.clone();
        hand.awaiting_advance = false;
        assert!(!hand.advance_runout(), "no second deal without a park");
        assert_eq!(hand.board, board);
    }

    /// A fold-win has nothing to reveal, so it never parks.
    #[test]
    fn v59_a_fold_win_does_not_park() {
        let mut hand = no_limit(&[10_000, 10_000], 12);
        hand.apply_action(Action::Fold).unwrap();
        assert!(hand.complete);
        assert!(!hand.awaits_runout());
    }

    #[test]
    fn all_in_preflop_runs_out_the_board() {
        let mut hand = no_limit(&[50, 50], 22);
        hand.apply_action(Action::AllIn).unwrap();
        hand.apply_action(Action::Call).unwrap();
        let advances = std::iter::from_fn(|| hand.advance_runout().then_some(())).count();
        assert_eq!(advances, 3, "flop, turn and river each take an advance");
        assert!(hand.complete);
        assert_eq!(hand.board.len(), 5);
        assert_eq!(hand.street, Street::Showdown);
    }

    #[test]
    fn postflop_first_actor_is_left_of_button() {
        let mut hand = no_limit(&[100, 100, 100], 33);
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Check).unwrap();
        assert_eq!(hand.street, Street::Flop);
        assert_eq!(hand.current_player, Some(1));
    }
}
