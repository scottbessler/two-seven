use crate::{
    cards::{Card, Deck},
    eval::{EvaluatedHand, evaluate},
    money::Cents,
    table::Stakes,
};
use serde::{Deserialize, Serialize};

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
    pub revealed_hole_cards: Vec<(usize, Vec<Card>)>,
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
        };
        if ante > 0 {
            let seats = hand
                .players
                .iter()
                .map(|player| player.seat)
                .collect::<Vec<_>>();
            for seat in seats {
                hand.put_chips(seat, ante);
            }
        }
        let (small_blind, big_blind) = stakes.blinds();
        let sb = if hand.players.len() == 2 {
            button
        } else {
            hand.next_live(button)
        };
        let bb = hand.next_live(sb);
        hand.put_chips(sb, small_blind);
        hand.put_chips(bb, big_blind);
        hand.last_bet = hand.players[hand.player_index(bb)].street_contribution;
        hand.current_player = Some(if hand.players.len() == 2 {
            sb
        } else {
            hand.next_live(bb)
        });
        hand
    }

    fn player_index(&self, seat: usize) -> usize {
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
    fn next_actor(&self, from: usize) -> Option<usize> {
        let start = self.player_index(from);
        for n in 1..=self.players.len() {
            let player = &self.players[(start + n) % self.players.len()];
            if !player.folded && !player.all_in && (!player.acted || player.must_call) {
                return Some(player.seat);
            }
        }
        None
    }
    fn put_chips(&mut self, seat: usize, amount: Cents) -> Cents {
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
                let fixed = if self.street <= Street::Flop {
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

    pub fn apply_action(&mut self, action: Action) -> Result<(), String> {
        if self.complete {
            return Err("hand is complete".into());
        }
        let seat = self.current_player.ok_or("no player to act")?;
        let legal = self.legal_actions().ok_or("no legal action")?;
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
        if !legal
            .actions
            .iter()
            .any(|candidate| std::mem::discriminant(candidate) == std::mem::discriminant(&action))
        {
            return Err("action is not legal".into());
        }
        match action {
            Action::Fold => {
                self.fold_seat_internal(seat)?;
            }
            Action::Check => self.finish_action(seat, 0, false)?,
            Action::Call => self.finish_action(seat, legal.to_call, false)?,
            Action::Bet { amount } | Action::Raise { amount } => {
                let bounds = legal.wager.ok_or("raising is not legal")?;
                if amount < bounds.min || amount > bounds.max {
                    return Err(format!(
                        "wager must be between {} and {}",
                        bounds.min, bounds.max
                    ));
                }
                self.finish_action(seat, amount, true)?;
            }
            Action::AllIn => unreachable!("all-in is normalized before rules processing"),
        }
        Ok(())
    }

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
        let was_current = self.current_player == Some(seat);
        self.players[i].folded = true;
        if self.live_count() == 1 {
            self.finish_fold();
        } else if was_current {
            self.current_player = self.next_actor(seat);
            if self.current_player.is_none() {
                self.advance_street();
            }
        } else if self.street_complete() {
            self.advance_street();
        }
        Ok(())
    }

    fn finish_action(&mut self, seat: usize, amount: Cents, is_wager: bool) -> Result<(), String> {
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
                self.last_raise = increment;
                self.wagers += 1;
                for player in &mut self.players {
                    player.acted = player.folded
                        || player.all_in
                        || player.street_contribution == self.last_bet;
                    player.must_call = false;
                }
            } else {
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
        if self.street_complete() {
            self.advance_street();
        } else {
            self.current_player = self.next_actor(seat);
        }
        Ok(())
    }

    fn live_count(&self) -> usize {
        self.players.iter().filter(|player| !player.folded).count()
    }
    fn street_complete(&self) -> bool {
        self.players
            .iter()
            .filter(|player| !player.folded && !player.all_in)
            .count()
            <= 1
            || self
                .players
                .iter()
                .filter(|player| !player.folded && !player.all_in)
                .all(|player| !player.must_call && player.street_contribution == self.last_bet)
    }
    fn advance_street(&mut self) {
        if self.live_count() <= 1 {
            self.finish_fold();
            return;
        }
        match self.street {
            Street::Preflop => {
                for _ in 0..3 {
                    self.board.push(self.deck.deal().expect("deck has cards"));
                }
                self.street = Street::Flop;
            }
            Street::Flop => {
                self.board.push(self.deck.deal().expect("deck has cards"));
                self.street = Street::Turn;
            }
            Street::Turn => {
                self.board.push(self.deck.deal().expect("deck has cards"));
                self.street = Street::River;
            }
            Street::River => {
                self.showdown();
                return;
            }
            _ => return,
        }
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
            player.acted = player.folded || player.all_in;
            player.must_call = false;
        }
        self.current_player = self.next_actor(self.button);
        if self.current_player.is_none() {
            self.advance_street();
        }
    }

    fn return_uncalled(&mut self) {
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
    fn finish_fold(&mut self) {
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
        self.summary = Some(HandSummary {
            board: self.board.clone(),
            results: Vec::new(),
            awards: vec![Award {
                seat: winner,
                amount,
            }],
            revealed_hole_cards: Vec::new(),
        });
        self.complete = true;
        self.street = Street::Complete;
        self.current_player = None;
        self.pot = 0;
    }
    fn total_contributions(&self) -> Cents {
        self.players.iter().map(|player| player.contribution).sum()
    }

    pub fn form_pots(&self) -> Vec<Pot> {
        let mut levels: Vec<Cents> = self
            .players
            .iter()
            .map(|player| player.contribution)
            .filter(|amount| *amount > 0)
            .collect();
        levels.sort_unstable();
        levels.dedup();
        let mut previous = 0;
        levels
            .into_iter()
            .map(|level| {
                let amount = (level - previous)
                    * self
                        .players
                        .iter()
                        .filter(|player| player.contribution >= level)
                        .count() as Cents;
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
            .collect()
    }

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
        self.summary = Some(HandSummary {
            board: self.board.clone(),
            results,
            awards,
            revealed_hole_cards: self
                .players
                .iter()
                .filter(|player| !player.folded)
                .map(|player| (player.seat, player.hole_cards.clone()))
                .collect(),
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
    fn out_of_turn_fold_keeps_hand_reachable() {
        let mut hand = Hand::new(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &[100, 100, 100],
            0,
            12,
        );
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
        let mut hand = Hand::new(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &[100, 100],
            0,
            13,
        );
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
    fn incomplete_raise_requires_call_without_reopen() {
        let mut h = Hand::new(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &[100, 100, 5],
            0,
            3,
        );
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

#[cfg(test)]
mod randomized_tests {
    use super::*;
    use rand::{SeedableRng, seq::SliceRandom};

    #[test]
    fn random_legal_play_conserves_awards() {
        for seed in 0..100u64 {
            let mut hand = Hand::new(
                Stakes::NoLimit {
                    small_blind: 1,
                    big_blind: 2,
                },
                &[100, 100, 100],
                0,
                seed,
            );
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
                let action = *legal
                    .actions
                    .choose(&mut rng)
                    .expect("non-empty legal actions");
                hand.apply_action(action).expect("legal action accepted");
            }
            assert!(hand.complete, "seed {seed} did not finish");
            let awarded: Cents = hand
                .summary
                .as_ref()
                .expect("summary")
                .awards
                .iter()
                .map(|award| award.amount)
                .sum();
            let contributed: Cents = hand.players.iter().map(|player| player.contribution).sum();
            assert_eq!(awarded, contributed, "seed {seed}");
        }
    }
}
