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
//! and seats the first actor; if nobody can act (everyone is all in) it
//! immediately advances again, running out the board to showdown.

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
                if self.runout_from.is_none() && self.betting_is_closed() {
                    self.runout_from = Some(self.board.len());
                }
                let cards = if next == Street::Flop { 3 } else { 1 };
                for _ in 0..cards {
                    self.board.push(self.deck.deal().expect("deck has cards"));
                }
                self.street = next;
            }
        }
        self.events.push(HandEvent {
            street: self.street,
            seat: None,
            kind: HandEventKind::Deal,
            amount: 0,
        });
        self.enter_betting_round();
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

    #[test]
    fn all_in_preflop_runs_out_the_board() {
        let mut hand = no_limit(&[50, 50], 22);
        hand.apply_action(Action::AllIn).unwrap();
        hand.apply_action(Action::Call).unwrap();
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
