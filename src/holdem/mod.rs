//! Texas hold'em hand engine, structured as a statechart of two composed
//! machines (see STATECHART.md for diagrams and invariants):
//!
//! * the **hand machine** ([`street`]) owns the top-level lifecycle
//!   `Preflop -> Flop -> Turn -> River -> Showdown -> Complete`, dealing
//!   board cards on entry to each street and resolving the pot on exit;
//! * the **betting round machine** ([`round`]) runs inside each betting
//!   street, tracking who still owes an action and applying player actions
//!   until the round settles.
//!
//! This module owns the shared data types plus pot formation and showdown
//! resolution; all transitions live in the two machine modules.

pub mod round;
pub mod street;

pub use round::RoundStatus;
pub use street::StreetTransition;

use crate::{
    cards::{Card, Deck},
    eval::{EvaluatedHand, evaluate},
    money::Cents,
    table::Stakes,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Street {
    Preflop,
    Flop,
    Turn,
    River,
    Showdown,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Fold,
    Check,
    Call,
    Bet { amount: Cents },
    Raise { amount: Cents },
    AllIn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WagerBounds {
    pub min: Cents,
    pub max: Cents,
    pub fixed: Option<Cents>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub seat: usize,
    pub stack: Cents,
    pub hole_cards: Vec<Card>,
    pub folded: bool,
    pub all_in: bool,
    pub contribution: Cents,
    pub street_contribution: Cents,
    pub acted: bool,
    pub must_call: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Pot {
    pub amount: Cents,
    pub eligible: Vec<usize>,
    pub level: Cents,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Award {
    pub seat: usize,
    pub amount: Cents,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SeatResult {
    pub seat: usize,
    pub hand: Option<EvaluatedHand>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HandSummary {
    pub board: Vec<Card>,
    pub results: Vec<SeatResult>,
    pub awards: Vec<Award>,
    pub contributions: BTreeMap<usize, Cents>,
    pub revealed_hole_cards: Vec<(usize, Vec<Card>)>,
    #[serde(default)]
    pub events: Vec<HandEvent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HandEventKind {
    Ante,
    SmallBlind,
    BigBlind,
    Fold,
    Check,
    Call,
    Bet,
    Raise,
    AllIn,
    Deal,
    Award,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HandEvent {
    pub street: Street,
    pub seat: Option<usize>,
    pub kind: HandEventKind,
    pub amount: Cents,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegalActions {
    pub seat: usize,
    pub actions: Vec<Action>,
    pub to_call: Cents,
    pub wager: Option<WagerBounds>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Hand {
    pub seed: u64,
    pub stakes: Stakes,
    pub players: Vec<Player>,
    pub button: usize,
    pub street: Street,
    pub board: Vec<Card>,
    pub current_player: Option<usize>,
    pub deck: Deck,
    pub pot: Cents,
    pub last_bet: Cents,
    pub last_raise: Cents,
    pub wagers: u8,
    pub ante: Cents,
    pub complete: bool,
    pub summary: Option<HandSummary>,
    #[serde(default)]
    pub events: Vec<HandEvent>,
}

impl Hand {
    pub fn new(stakes: Stakes, stacks: &[Cents], button: usize, seed: u64) -> Self {
        let seats: Vec<(usize, Cents)> = stacks.iter().copied().enumerate().collect();
        Self::new_with_seats(stakes, &seats, button, seed)
    }

    pub fn new_with_seats(
        stakes: Stakes,
        stacks: &[(usize, Cents)],
        button: usize,
        seed: u64,
    ) -> Self {
        Self::new_with_seats_and_ante(stakes, stacks, button, seed, 0)
    }

    /// Entry action of the hand machine's initial `Preflop` state: deal hole
    /// cards, post antes and blinds, and hand control to the preflop betting
    /// round with the correct first actor.
    pub fn new_with_seats_and_ante(
        stakes: Stakes,
        stacks: &[(usize, Cents)],
        button: usize,
        seed: u64,
        ante: Cents,
    ) -> Self {
        assert!((2..=9).contains(&stacks.len()));
        let mut deck = Deck::seeded(seed);
        let mut players: Vec<Player> = stacks
            .iter()
            .map(|(seat, stack)| Player {
                seat: *seat,
                stack: *stack,
                hole_cards: Vec::new(),
                folded: false,
                all_in: false,
                contribution: 0,
                street_contribution: 0,
                acted: false,
                must_call: false,
            })
            .collect();
        for _ in 0..2 {
            for player in &mut players {
                player.hole_cards.push(deck.deal().expect("deck has cards"));
            }
        }
        let button = stacks
            .iter()
            .find(|(seat, _)| *seat == button)
            .map_or(stacks[0].0, |(seat, _)| *seat);
        let mut hand = Self {
            seed,
            stakes,
            players,
            button,
            street: Street::Preflop,
            board: Vec::new(),
            current_player: None,
            deck,
            pot: 0,
            last_bet: 0,
            last_raise: stakes.blinds().1,
            wagers: 1,
            ante,
            complete: false,
            summary: None,
            events: Vec::new(),
        };
        if ante > 0 {
            let seats = hand
                .players
                .iter()
                .map(|player| player.seat)
                .collect::<Vec<_>>();
            for seat in seats {
                let amount = hand.put_chips(seat, ante);
                hand.events.push(HandEvent {
                    street: Street::Preflop,
                    seat: Some(seat),
                    kind: HandEventKind::Ante,
                    amount,
                });
            }
        }
        let (small_blind, big_blind) = stakes.blinds();
        let sb = if hand.players.len() == 2 {
            button
        } else {
            hand.next_live(button)
        };
        let bb = hand.next_live(sb);
        let small_blind = hand.put_chips(sb, small_blind);
        hand.events.push(HandEvent {
            street: Street::Preflop,
            seat: Some(sb),
            kind: HandEventKind::SmallBlind,
            amount: small_blind,
        });
        let big_blind = hand.put_chips(bb, big_blind);
        hand.events.push(HandEvent {
            street: Street::Preflop,
            seat: Some(bb),
            kind: HandEventKind::BigBlind,
            amount: big_blind,
        });
        hand.last_bet = hand.players[hand.player_index(bb)].street_contribution;
        hand.current_player = Some(if hand.players.len() == 2 {
            sb
        } else {
            hand.next_live(bb)
        });
        if let Some(seat) = hand.current_player
            && !round::needs_action(&hand.players[hand.player_index(seat)], hand.last_bet)
        {
            hand.current_player = hand.next_actor(seat);
        }
        if hand.round_complete() {
            hand.advance_street();
        }
        hand
    }

    pub(crate) fn player_index(&self, seat: usize) -> usize {
        self.players
            .iter()
            .position(|player| player.seat == seat)
            .expect("seat is in hand")
    }

    fn next_live(&self, from: usize) -> usize {
        let start = self.player_index(from);
        for n in 1..=self.players.len() {
            let player = &self.players[(start + n) % self.players.len()];
            if player.stack > 0 {
                return player.seat;
            }
        }
        self.players
            .iter()
            .find(|player| !player.folded)
            .map_or(from, |player| player.seat)
    }

    pub(crate) fn put_chips(&mut self, seat: usize, amount: Cents) -> Cents {
        let i = self.player_index(seat);
        let paid = amount.max(0).min(self.players[i].stack);
        self.players[i].stack -= paid;
        self.players[i].contribution += paid;
        self.players[i].street_contribution += paid;
        if self.players[i].stack == 0 {
            self.players[i].all_in = true;
        }
        self.pot += paid;
        paid
    }

    pub(crate) fn live_count(&self) -> usize {
        self.players.iter().filter(|player| !player.folded).count()
    }

    pub(crate) fn total_contributions(&self) -> Cents {
        self.players.iter().map(|player| player.contribution).sum()
    }

    /// Returns any uncalled excess of the largest contribution to its owner
    /// before the pot is resolved.
    pub(crate) fn return_uncalled(&mut self) {
        let active: Vec<Cents> = self
            .players
            .iter()
            .filter(|player| !player.folded)
            .map(|player| player.contribution)
            .collect();
        let Some(highest) = active.iter().max().copied() else {
            return;
        };
        if active.iter().filter(|value| **value == highest).count() != 1 {
            return;
        }
        let matched = self
            .players
            .iter()
            .filter(|player| player.contribution < highest)
            .map(|player| player.contribution)
            .max()
            .unwrap_or(0);
        let winner = self
            .players
            .iter()
            .position(|player| !player.folded && player.contribution == highest)
            .expect("highest player");
        let refund = highest - matched;
        if refund > 0 {
            self.players[winner].contribution -= refund;
            self.players[winner].street_contribution = self.players[winner]
                .street_contribution
                .saturating_sub(refund);
            self.players[winner].stack += refund;
            self.pot -= refund;
        }
    }

    /// Side-pot levels are the distinct contributions of live players; every
    /// chip (including folded contributions, up to each level) lands in some
    /// pot, and dead money above the highest live level joins the last pot.
    pub fn form_pots(&self) -> Vec<Pot> {
        let mut levels: Vec<Cents> = self
            .players
            .iter()
            .filter(|player| !player.folded)
            .map(|player| player.contribution)
            .filter(|amount| *amount > 0)
            .collect();
        levels.sort_unstable();
        levels.dedup();
        let mut previous = 0;
        let mut pots: Vec<Pot> = levels
            .into_iter()
            .map(|level| {
                let amount = self
                    .players
                    .iter()
                    .map(|player| {
                        player.contribution.min(level) - player.contribution.min(previous)
                    })
                    .sum();
                let eligible = self
                    .players
                    .iter()
                    .filter(|player| player.contribution >= level && !player.folded)
                    .map(|player| player.seat)
                    .collect();
                previous = level;
                Pot {
                    amount,
                    eligible,
                    level,
                }
            })
            .collect();
        let dead: Cents = self
            .players
            .iter()
            .map(|player| (player.contribution - previous).max(0))
            .sum();
        if dead > 0
            && let Some(last) = pots.last_mut()
        {
            last.amount += dead;
        }
        pots
    }

    /// Final transition of the hand machine: evaluate live hands, award every
    /// pot, and move to the terminal `Complete` state.
    pub fn showdown(&mut self) {
        self.return_uncalled();
        self.street = Street::Showdown;
        let pots = self.form_pots();
        let mut results = Vec::new();
        for player in self.players.iter().filter(|player| !player.folded) {
            results.push(SeatResult {
                seat: player.seat,
                hand: Some(evaluate(
                    &player
                        .hole_cards
                        .iter()
                        .chain(self.board.iter())
                        .copied()
                        .collect::<Vec<_>>(),
                )),
            });
        }
        let mut awards = Vec::new();
        for pot in pots {
            let eligible: Vec<&SeatResult> = results
                .iter()
                .filter(|result| pot.eligible.contains(&result.seat))
                .collect();
            if eligible.is_empty() {
                continue;
            }
            let best = eligible
                .iter()
                .map(|result| &result.hand.as_ref().expect("hand").rank)
                .max()
                .expect("eligible hand");
            let winners: Vec<&SeatResult> = eligible
                .into_iter()
                .filter(|result| result.hand.as_ref().expect("hand").rank == *best)
                .collect();
            let share = pot.amount / winners.len() as Cents;
            let odd = (pot.amount % winners.len() as Cents) as usize;
            let order = self.winner_order(winners.iter().map(|winner| winner.seat).collect());
            for winner in winners {
                let amount = share
                    + usize::from(order.iter().take(odd).any(|seat| *seat == winner.seat)) as Cents;
                awards.push(Award {
                    seat: winner.seat,
                    amount,
                });
                let i = self.player_index(winner.seat);
                self.players[i].stack += amount;
            }
        }
        for award in &awards {
            self.events.push(HandEvent {
                street: Street::Showdown,
                seat: Some(award.seat),
                kind: HandEventKind::Award,
                amount: award.amount,
            });
        }
        self.summary = Some(HandSummary {
            board: self.board.clone(),
            results,
            awards,
            contributions: self
                .players
                .iter()
                .map(|player| (player.seat, player.contribution))
                .collect(),
            revealed_hole_cards: self
                .players
                .iter()
                .filter(|player| !player.folded)
                .map(|player| (player.seat, player.hole_cards.clone()))
                .collect(),
            events: self.events.clone(),
        });
        self.complete = true;
        self.current_player = None;
        self.pot = 0;
    }

    fn winner_order(&self, mut seats: Vec<usize>) -> Vec<usize> {
        let button_index = self.player_index(self.button);
        seats.sort_by_key(|seat| {
            let index = self.player_index(*seat);
            (index + self.players.len() - ((button_index + 1) % self.players.len()))
                % self.players.len()
        });
        seats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn h(s: &str) -> Vec<Card> {
        s.split_whitespace().map(|x| x.parse().unwrap()).collect()
    }

    #[test]
    fn heads_up_order() {
        let h = Hand::new(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &[100, 100],
            0,
            1,
        );
        assert_eq!(h.players[0].contribution, 1);
        assert_eq!(h.players[1].contribution, 2);
        assert_eq!(h.current_player, Some(0));
    }

    #[test]
    fn short_blinds_are_all_in() {
        let h = Hand::new(
            Stakes::NoLimit {
                small_blind: 5,
                big_blind: 10,
            },
            &[100, 3, 100],
            0,
            4,
        );
        assert!(h.players.iter().any(|p| p.stack == 0 && p.all_in));
    }

    #[test]
    fn side_pots() {
        let h = Hand::new_with_seats(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &[(1, 10), (4, 20), (7, 30)],
            1,
            5,
        );
        let mut h = h;
        for p in &mut h.players {
            p.contribution = match p.seat {
                1 => 10,
                4 => 20,
                _ => 30,
            };
        }
        assert_eq!(
            h.form_pots()
                .iter()
                .map(|pot| pot.amount)
                .collect::<Vec<_>>(),
            vec![30, 20, 10]
        );
    }

    #[test]
    fn fold_and_refund() {
        let mut h = Hand::new(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &[100, 100],
            0,
            6,
        );
        h.players[0].contribution = 500;
        h.players[1].contribution = 200;
        h.players[0].stack = 0;
        h.players[1].stack = 0;
        h.pot = 700;
        h.players[1].folded = true;
        h.current_player = Some(0);
        h.finish_fold();
        assert_eq!(h.summary.unwrap().awards[0].amount, 400);
    }

    #[test]
    fn split_awards_conserved() {
        let mut hand = Hand::new(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &[100, 100],
            0,
            7,
        );
        hand.players[0].hole_cards = h("Ah Kd");
        hand.players[1].hole_cards = h("As Kc");
        hand.players[0].contribution = 3;
        hand.players[1].contribution = 2;
        hand.players[0].stack = 0;
        hand.players[1].stack = 0;
        hand.pot = 5;
        hand.board = h("2h 3d 4s 5c 9h");
        hand.showdown();
        assert_eq!(
            hand.summary
                .as_ref()
                .unwrap()
                .awards
                .iter()
                .map(|award| award.amount)
                .sum::<i64>(),
            4
        );
    }
}
