//! Betting round machine.
//!
//! One instance of this machine runs inside each betting street of the hand
//! machine (see STATECHART.md). Its state is `AwaitingAction { seat }` — the
//! seat in `Hand::current_player` — and it settles to `Complete` only when no
//! live, non-all-in player still owes an action.
//!
//! A player owes an action ([`needs_action`]) while any of these hold:
//! * they have not voluntarily acted this street (blind posts don't count,
//!   which is what gives the big blind its preflop option);
//! * their street contribution is below the current bet;
//! * an incomplete all-in raise obliges them to call (`must_call`), without
//!   reopening the betting.
//!
//! The same predicate drives both actor rotation and round completion, so a
//! street can never be skipped while somebody still owes a call or a check.

use super::{Action, Hand, HandEvent, HandEventKind, LegalActions, Player, WagerBounds};
use crate::{money::Cents, table::Stakes};

/// Where the betting round machine currently stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoundStatus {
    /// A player still owes an action; the round stays open.
    AwaitingAction { seat: usize },
    /// Every live, non-all-in player has acted and matched the bet.
    Complete,
}

/// Guard: does this player still owe an action this street?
pub fn needs_action(player: &Player, last_bet: Cents) -> bool {
    !player.folded
        && !player.all_in
        && (!player.acted || player.must_call || player.street_contribution < last_bet)
}

impl Hand {
    /// Current state of the betting round machine.
    pub fn round_status(&self) -> RoundStatus {
        match self
            .current_player
            .filter(|seat| needs_action(&self.players[self.player_index(*seat)], self.last_bet))
        {
            Some(seat) => RoundStatus::AwaitingAction { seat },
            None => self
                .players
                .iter()
                .find(|player| needs_action(player, self.last_bet))
                .map_or(RoundStatus::Complete, |player| {
                    RoundStatus::AwaitingAction { seat: player.seat }
                }),
        }
    }

    pub(crate) fn round_complete(&self) -> bool {
        !self
            .players
            .iter()
            .any(|player| needs_action(player, self.last_bet))
    }

    /// Next seat after `from` (clockwise) that still owes an action.
    pub(crate) fn next_actor(&self, from: usize) -> Option<usize> {
        let start = self.player_index(from);
        for n in 1..=self.players.len() {
            let player = &self.players[(start + n) % self.players.len()];
            if needs_action(player, self.last_bet) {
                return Some(player.seat);
            }
        }
        None
    }

    pub fn legal_actions(&self) -> Option<LegalActions> {
        let seat = self.current_player?;
        let player = &self.players[self.player_index(seat)];
        if player.folded || player.all_in {
            return None;
        }
        let max = player.stack;
        let to_call = (self.last_bet - player.street_contribution).max(0);
        let mut actions = vec![Action::Fold];
        if to_call == 0 {
            actions.push(Action::Check);
        } else if max > 0 {
            actions.push(Action::Call);
        }
        let wager = if max > 0 && self.wagers < 4 && !player.must_call {
            Some(self.wager_bounds(to_call, max))
        } else {
            None
        };
        if let Some(bounds) = wager {
            if to_call == 0 {
                actions.push(if self.last_bet == 0 {
                    Action::Bet {
                        amount: bounds.fixed.unwrap_or(bounds.min),
                    }
                } else {
                    Action::Raise {
                        amount: bounds.fixed.unwrap_or(bounds.min),
                    }
                });
            } else {
                actions.push(Action::Raise {
                    amount: bounds.fixed.unwrap_or(bounds.min),
                });
            }
        }
        let all_in_allowed = match self.stakes {
            Stakes::Limit { .. } => self
                .wager_bounds(to_call, max)
                .fixed
                .is_some_and(|fixed| max <= to_call + fixed),
            Stakes::NoLimit { .. } => true,
        };
        if max > 0 && all_in_allowed && (!player.must_call || max <= to_call) {
            actions.push(Action::AllIn);
        }
        Some(LegalActions {
            seat,
            actions,
            to_call,
            wager,
        })
    }

    fn wager_bounds(&self, to_call: Cents, max: Cents) -> WagerBounds {
        match self.stakes {
            Stakes::Limit { small_bet, big_bet } => {
                let fixed = if self.street <= super::Street::Flop {
                    small_bet
                } else {
                    big_bet
                };
                let short = max < to_call + fixed;
                let amount = if short { max } else { fixed };
                WagerBounds {
                    min: amount,
                    max: amount,
                    fixed: Some(amount),
                }
            }
            Stakes::NoLimit { big_blind, .. } => {
                let increment = if self.last_bet == 0 {
                    big_blind
                } else {
                    self.last_raise.max(big_blind)
                };
                WagerBounds {
                    min: (to_call + increment).min(max),
                    max,
                    fixed: None,
                }
            }
        }
    }

    /// Transition of the betting round machine: apply the current player's
    /// action, then either rotate to the next actor or, when the round has
    /// settled, hand control back to the hand machine.
    pub fn apply_action(&mut self, action: Action) -> Result<(), String> {
        if self.complete {
            return Err("hand is complete".into());
        }
        let seat = self.current_player.ok_or("no player to act")?;
        let legal = self.legal_actions().ok_or("no legal action")?;
        let original_action = action;
        let action = if matches!(action, Action::AllIn) {
            let stack = self.players[self.player_index(seat)].stack;
            if legal.to_call >= stack {
                Action::Call
            } else if self.last_bet == 0 {
                Action::Bet { amount: stack }
            } else {
                Action::Raise { amount: stack }
            }
        } else {
            action
        };
        if !legal.actions.iter().any(|candidate| {
            std::mem::discriminant(candidate) == std::mem::discriminant(&original_action)
        }) {
            return Err("action is not legal".into());
        }
        if let Action::Bet { amount } | Action::Raise { amount } = original_action {
            let bounds = legal.wager.ok_or("raising is not legal")?;
            if amount < bounds.min || amount > bounds.max {
                return Err(format!(
                    "wager must be between {} and {}",
                    bounds.min, bounds.max
                ));
            }
        }
        let event = match original_action {
            Action::Fold => None,
            Action::Check => Some((HandEventKind::Check, 0)),
            Action::Call => Some((HandEventKind::Call, legal.to_call)),
            Action::Bet { amount } => Some((HandEventKind::Bet, amount)),
            Action::Raise { amount } => Some((HandEventKind::Raise, amount)),
            Action::AllIn => Some((
                HandEventKind::AllIn,
                self.players[self.player_index(seat)].stack,
            )),
        };
        if let Some((kind, amount)) = event {
            self.events.push(HandEvent {
                street: self.street,
                seat: Some(seat),
                kind,
                amount,
            });
        }
        match action {
            Action::Fold => {
                self.fold_seat_internal(seat)?;
            }
            Action::Check => self.finish_action(seat, 0, false),
            Action::Call => self.finish_action(seat, legal.to_call, false),
            Action::Bet { amount } | Action::Raise { amount } => {
                self.finish_action(seat, amount, true);
            }
            Action::AllIn => unreachable!("all-in is normalized before rules processing"),
        }
        Ok(())
    }

    /// External fold transition: a seat may fold even when it is not their
    /// turn (e.g. a player leaving the table mid-hand).
    pub fn fold_seat(&mut self, seat: usize) -> Result<(), String> {
        if self.complete {
            return Err("hand is complete".into());
        }
        if !self.players.iter().any(|player| player.seat == seat) {
            return Err("seat is not in hand".into());
        }
        self.fold_seat_internal(seat)
    }

    fn fold_seat_internal(&mut self, seat: usize) -> Result<(), String> {
        let i = self.player_index(seat);
        if self.players[i].folded {
            return Err("player has already folded".into());
        }
        self.events.push(HandEvent {
            street: self.street,
            seat: Some(seat),
            kind: HandEventKind::Fold,
            amount: 0,
        });
        let was_current = self.current_player == Some(seat);
        self.players[i].folded = true;
        if self.live_count() == 1 {
            self.finish_fold();
        } else if self.round_complete() {
            self.advance_street();
        } else if was_current {
            self.current_player = self.next_actor(seat);
        }
        Ok(())
    }

    /// Settle a non-fold action: move chips, record who has acted, reopen or
    /// restrict the action after raises, then rotate or complete the round.
    fn finish_action(&mut self, seat: usize, amount: Cents, is_wager: bool) {
        let i = self.player_index(seat);
        let before = self.last_bet;
        self.put_chips(seat, amount);
        self.players[i].acted = true;
        self.players[i].must_call = false;
        if is_wager && self.players[i].street_contribution > before {
            self.last_bet = self.players[i].street_contribution;
            let increment = self.last_bet - before;
            let full_raise = increment >= self.last_raise;
            if full_raise {
                // A full bet or raise reopens the action to everyone.
                self.last_raise = increment;
                self.wagers += 1;
                for player in &mut self.players {
                    player.must_call = false;
                }
            } else {
                // An incomplete all-in raise obliges a call but does not
                // reopen raising for players who already acted.
                for player in &mut self.players {
                    if !player.folded
                        && !player.all_in
                        && player.seat != seat
                        && player.street_contribution < self.last_bet
                    {
                        player.must_call = true;
                    }
                }
            }
        }
        if self.round_complete() {
            self.advance_street();
        } else {
            self.current_player = self.next_actor(seat);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Action, Hand, HandEvent, HandEventKind, Street};
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

    #[test]
    fn big_blind_gets_the_preflop_option() {
        let mut hand = no_limit(&[100, 100, 100], 42);
        assert_eq!(hand.current_player, Some(0));
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Call).unwrap();
        assert_eq!(hand.street, Street::Preflop, "BB must get the option");
        assert_eq!(hand.current_player, Some(2));
        let legal = hand.legal_actions().unwrap();
        assert!(legal.actions.contains(&Action::Check));
        assert!(
            legal
                .actions
                .iter()
                .any(|action| matches!(action, Action::Raise { .. }))
        );
        hand.apply_action(Action::Check).unwrap();
        assert_eq!(hand.street, Street::Flop);
    }

    #[test]
    fn big_blind_option_can_raise() {
        let mut hand = no_limit(&[100, 100, 100], 42);
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Raise { amount: 4 }).unwrap();
        assert_eq!(hand.street, Street::Preflop);
        assert_eq!(hand.current_player, Some(0));
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Call).unwrap();
        assert_eq!(hand.street, Street::Flop);
    }

    #[test]
    fn every_player_must_act_before_a_checked_street_ends() {
        let mut hand = no_limit(&[100, 100, 100], 42);
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Check).unwrap();
        assert_eq!(hand.street, Street::Flop);
        assert_eq!(hand.current_player, Some(1), "small blind acts first");
        hand.apply_action(Action::Check).unwrap();
        assert_eq!(hand.street, Street::Flop, "one check must not end a round");
        hand.apply_action(Action::Check).unwrap();
        assert_eq!(hand.street, Street::Flop);
        hand.apply_action(Action::Check).unwrap();
        assert_eq!(hand.street, Street::Turn);
    }

    #[test]
    fn every_caller_is_logged_before_the_next_street_deals() {
        let mut hand = no_limit(&[100; 5], 9);
        // preflop: everyone calls, BB checks the option
        for _ in 0..4 {
            hand.apply_action(Action::Call).unwrap();
        }
        hand.apply_action(Action::Check).unwrap();
        assert_eq!(hand.street, Street::Flop);
        // flop: first player bets, everyone else must call before the turn
        hand.apply_action(Action::Bet { amount: 10 }).unwrap();
        for n in 0..4 {
            assert_eq!(hand.street, Street::Flop, "street ended after {n} calls");
            hand.apply_action(Action::Call).unwrap();
        }
        assert_eq!(hand.street, Street::Turn);
        let flop_calls = hand
            .events
            .iter()
            .filter(|event| event.street == Street::Flop && event.kind == HandEventKind::Call)
            .count();
        assert_eq!(flop_calls, 4);
        let turn_deal = hand
            .events
            .iter()
            .position(|event| event.kind == HandEventKind::Deal && event.street == Street::Turn)
            .unwrap();
        let last_flop_call = hand
            .events
            .iter()
            .rposition(|event| event.street == Street::Flop && event.kind == HandEventKind::Call)
            .unwrap();
        assert!(last_flop_call < turn_deal);
    }

    #[test]
    fn full_raise_reopens_action() {
        let mut hand = no_limit(&[100, 100, 100], 11);
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Check).unwrap();
        // flop: sb bets, bb raises, action reopens to sb and the bettor
        hand.apply_action(Action::Bet { amount: 4 }).unwrap();
        hand.apply_action(Action::Raise { amount: 12 }).unwrap();
        assert_eq!(hand.street, Street::Flop);
        hand.apply_action(Action::Call).unwrap();
        assert_eq!(hand.street, Street::Flop, "original bettor must respond");
        assert_eq!(hand.current_player, Some(1));
        let legal = hand.legal_actions().unwrap();
        assert!(
            legal
                .actions
                .iter()
                .any(|action| matches!(action, Action::Raise { .. })),
            "full raise reopens raising"
        );
        hand.apply_action(Action::Call).unwrap();
        assert_eq!(hand.street, Street::Turn);
    }

    #[test]
    fn incomplete_raise_requires_call_without_reopen() {
        let mut h = no_limit(&[100, 100, 5], 3);
        h.apply_action(Action::Raise { amount: 4 }).unwrap();
        h.apply_action(Action::Call).unwrap();
        h.apply_action(Action::AllIn).unwrap();
        assert!(h.legal_actions().is_some());
        assert!(
            !h.legal_actions()
                .unwrap()
                .actions
                .iter()
                .any(|a| matches!(a, Action::Raise { .. }))
        );
    }

    #[test]
    fn short_all_in_call_does_not_reopen_action() {
        let mut hand = no_limit(&[100, 100, 6], 21);
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Check).unwrap();
        assert_eq!(hand.street, Street::Flop);
        hand.apply_action(Action::Bet { amount: 10 }).unwrap();
        // seat 2 calls all-in short; the bettor must not act again
        hand.apply_action(Action::AllIn).unwrap();
        assert_eq!(hand.current_player, Some(0));
        hand.apply_action(Action::Call).unwrap();
        assert_eq!(hand.street, Street::Turn);
    }

    #[test]
    fn all_in_short_of_the_bet_completes_the_round() {
        let mut hand = no_limit(&[100, 100], 31);
        hand.apply_action(Action::Raise { amount: 50 }).unwrap();
        hand.apply_action(Action::Call).unwrap();
        assert_eq!(hand.street, Street::Flop);
        hand.apply_action(Action::AllIn).unwrap();
        hand.apply_action(Action::Call).unwrap();
        assert!(hand.complete, "all-in showdown runs out the board");
        assert_eq!(hand.board.len(), 5);
    }

    #[test]
    fn limit_offers_raise_and_rejects_wrong_size() {
        let mut h = Hand::new(
            Stakes::Limit {
                small_bet: 2,
                big_bet: 4,
            },
            &[100, 100, 100],
            0,
            2,
        );
        assert_eq!(h.current_player, Some(0));
        h.apply_action(Action::Raise { amount: 2 }).unwrap();
        assert!(
            h.legal_actions()
                .unwrap()
                .actions
                .iter()
                .any(|a| matches!(a, Action::Raise { amount: 2 }))
        );
        assert!(h.apply_action(Action::Raise { amount: 5 }).is_err());
    }

    #[test]
    fn rejected_wager_logs_no_event() {
        let mut hand = no_limit(&[100, 100, 100], 17);
        let before = hand.events.clone();
        assert!(hand.apply_action(Action::Raise { amount: 1 }).is_err());
        assert_eq!(hand.events, before, "rejected actions must not be logged");
    }

    #[test]
    fn out_of_turn_fold_keeps_hand_reachable() {
        let mut hand = no_limit(&[100, 100, 100], 12);
        let departing = hand
            .players
            .iter()
            .map(|player| player.seat)
            .find(|seat| Some(*seat) != hand.current_player)
            .unwrap();
        hand.fold_seat(departing).unwrap();
        assert!(hand.complete || hand.legal_actions().is_some());
    }

    #[test]
    fn out_of_turn_fold_awards_uncontested_pot() {
        let mut hand = no_limit(&[100, 100], 13);
        let departing = hand
            .players
            .iter()
            .find(|player| Some(player.seat) != hand.current_player)
            .unwrap()
            .seat;
        hand.fold_seat(departing).unwrap();
        assert!(hand.complete);
        assert_eq!(hand.summary.as_ref().unwrap().awards.len(), 1);
        assert_eq!(hand.pot, 0);
    }

    #[test]
    fn hand_events_track_blinds_actions_and_streets() {
        let mut hand = Hand::new(
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            &[10_000, 10_000],
            0,
            91,
        );
        assert!(matches!(
            hand.events.as_slice(),
            [
                HandEvent {
                    kind: HandEventKind::SmallBlind,
                    amount: 100,
                    ..
                },
                HandEvent {
                    kind: HandEventKind::BigBlind,
                    amount: 200,
                    ..
                }
            ]
        ));
        hand.apply_action(Action::Call).unwrap();
        hand.apply_action(Action::Check).unwrap();
        assert!(hand.events.iter().any(|event| {
            event.kind == HandEventKind::Call && event.seat == Some(0) && event.amount == 100
        }));
        assert!(
            hand.events
                .iter()
                .any(|event| { event.kind == HandEventKind::Deal && event.street == Street::Flop })
        );
    }
}
