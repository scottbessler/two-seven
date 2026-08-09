use crate::{
    cards::{Card, Deck},
    eval::{EvaluatedHand, evaluate},
    table::{Cents, Stakes},
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub stack: Cents,
    pub hole_cards: Vec<Card>,
    pub folded: bool,
    pub all_in: bool,
    pub contribution: Cents,
    pub street_contribution: Cents,
    pub acted: bool,
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
    pub min_raise: Cents,
    pub max_wager: Cents,
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
    pub complete: bool,
    pub summary: Option<HandSummary>,
}
impl Hand {
    pub fn new(stakes: Stakes, stacks: &[Cents], button: usize, seed: u64) -> Self {
        assert!((2..=9).contains(&stacks.len()));
        let mut deck = Deck::seeded(seed);
        let mut players = stacks
            .iter()
            .map(|s| Player {
                stack: *s,
                hole_cards: Vec::new(),
                folded: false,
                all_in: false,
                contribution: 0,
                street_contribution: 0,
                acted: false,
            })
            .collect::<Vec<_>>();
        for _ in 0..2 {
            for p in &mut players {
                p.hole_cards.push(deck.deal().unwrap())
            }
        }
        let mut h = Self {
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
            last_raise: 0,
            wagers: 1,
            complete: false,
            summary: None,
        };
        let (sb, bb) = stakes.blinds();
        let sbseat = if h.players.len() == 2 {
            button
        } else {
            h.next_live(button)
        };
        let bbseat = h.next_live(sbseat);
        h.put_chips(sbseat, sb);
        h.put_chips(bbseat, bb);
        h.last_bet = bb;
        h.last_raise = bb;
        h.current_player = Some(if h.players.len() == 2 {
            sbseat
        } else {
            h.next_live(bbseat)
        });
        h
    }
    fn next_live(&self, from: usize) -> usize {
        for n in 1..=self.players.len() {
            let i = (from + n) % self.players.len();
            if self.players[i].stack > 0 {
                return i;
            }
        }
        from
    }
    fn put_chips(&mut self, i: usize, amount: Cents) -> Cents {
        let x = amount.max(0).min(self.players[i].stack);
        self.players[i].stack -= x;
        self.players[i].contribution += x;
        self.players[i].street_contribution += x;
        if self.players[i].stack == 0 {
            self.players[i].all_in = true
        }
        self.pot += x;
        x
    }
    pub fn legal_actions(&self) -> LegalActions {
        let i = self.current_player.expect("complete hand has no turn");
        let p = &self.players[i];
        let max = p.stack;
        let to_call = (self.last_bet - p.street_contribution).max(0);
        let mut actions = vec![Action::Fold];
        if to_call == 0 {
            actions.push(Action::Check)
        } else if max > 0 {
            actions.push(Action::Call)
        }
        let wager_cap = match self.stakes {
            Stakes::Limit { small_bet, big_bet } => {
                if self.street <= Street::Flop {
                    small_bet
                } else {
                    big_bet
                }
            }
            Stakes::NoLimit { .. } => max,
        };
        if max > 0 && self.wagers < 4 && (to_call == 0 || self.stakes_is_no_limit()) {
            if to_call == 0 {
                if self.last_bet > 0 {
                    actions.push(Action::Raise {
                        amount: (self.last_bet + wager_cap).min(max),
                    });
                } else {
                    actions.push(Action::Bet {
                        amount: wager_cap.min(max),
                    });
                }
            } else {
                let min = self.last_raise.max(wager_cap);
                if max > to_call {
                    actions.push(Action::Raise {
                        amount: (to_call + min).min(max),
                    })
                }
            }
        }
        if max > 0 {
            actions.push(Action::AllIn)
        }
        LegalActions {
            seat: i,
            actions,
            to_call,
            min_raise: self.last_raise,
            max_wager: max,
        }
    }
    fn stakes_is_no_limit(&self) -> bool {
        matches!(self.stakes, Stakes::NoLimit { .. })
    }
    pub fn apply_action(&mut self, action: Action) -> Result<(), String> {
        if self.complete {
            return Err("hand is complete".into());
        }
        let i = self.current_player.ok_or("no player to act")?;
        let legal = self.legal_actions();
        let to_put = match action {
            Action::Fold | Action::Check | Action::Call | Action::AllIn => None,
            Action::Bet { amount } | Action::Raise { amount } => Some(amount),
        };
        if !legal
            .actions
            .iter()
            .any(|a| std::mem::discriminant(a) == std::mem::discriminant(&action))
        {
            return Err("action is not legal".into());
        }
        if let Some(amount) = to_put {
            if amount <= 0 || amount > self.players[i].stack {
                return Err("invalid wager amount".into());
            }
            if matches!(action, Action::Bet { .. }) && legal.to_call != 0 {
                return Err("cannot bet facing a wager".into());
            }
            if matches!(action, Action::Raise { .. }) && amount <= legal.to_call {
                return Err("raise must include the call".into());
            }
        }
        match action {
            Action::Fold => {
                self.players[i].folded = true;
                if self.players.iter().filter(|p| !p.folded).count() == 1 {
                    self.finish_fold(i)
                } else {
                    self.current_player = Some(self.next_actor(i));
                }
                return Ok(());
            }
            Action::Check => {}
            Action::Call => {
                self.put_chips(i, legal.to_call);
            }
            Action::Bet { amount } | Action::Raise { amount } => {
                let before = self.last_bet;
                let added = self.put_chips(i, amount);
                if self.players[i].street_contribution > before {
                    self.last_bet = self.players[i].street_contribution;
                    self.last_raise = (self.last_bet - before).max(self.last_raise);
                    self.wagers += 1;
                    for p in &mut self.players {
                        p.acted = p.folded || p.all_in || p.street_contribution == self.last_bet;
                    }
                } else if added > 0 {
                    self.players[i].acted = true
                }
            }
            Action::AllIn => {
                let before = self.last_bet;
                let stack = self.players[i].stack;
                let added = self.put_chips(i, stack);
                if self.players[i].street_contribution > before {
                    self.last_bet = self.players[i].street_contribution;
                    let raise = self.last_bet - before;
                    if raise >= self.last_raise {
                        self.last_raise = raise;
                        self.wagers += 1;
                        for p in &mut self.players {
                            p.acted =
                                p.folded || p.all_in || p.street_contribution == self.last_bet;
                        }
                    }
                } else if added > 0 {
                    self.players[i].acted = true
                }
            }
        }
        self.players[i].acted = true;
        if self.street_complete() {
            self.advance_street()
        } else {
            self.current_player = Some(self.next_actor(i));
        }
        Ok(())
    }
    fn next_actor(&self, from: usize) -> usize {
        for n in 1..=self.players.len() {
            let i = (from + n) % self.players.len();
            let p = &self.players[i];
            if !p.folded && !p.all_in && !p.acted {
                return i;
            }
        }
        from
    }
    fn finish_fold(&mut self, _: usize) {
        let live: Vec<usize> = self
            .players
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.folded)
            .map(|(i, _)| i)
            .collect();
        if live.len() == 1 {
            let winner = live[0];
            let amount = self.total_contributions();
            self.players[winner].stack += amount;
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
            self.current_player = None
        }
    }
    fn total_contributions(&self) -> Cents {
        self.players.iter().map(|p| p.contribution).sum()
    }
    fn street_complete(&self) -> bool {
        self.players
            .iter()
            .filter(|p| !p.folded && !p.all_in)
            .all(|p| p.acted && p.street_contribution == self.last_bet)
            || self
                .players
                .iter()
                .filter(|p| !p.folded && !p.all_in)
                .count()
                <= 1
    }
    fn advance_street(&mut self) {
        if self.players.iter().filter(|p| !p.folded).count() <= 1 {
            self.finish_fold(usize::MAX);
            return;
        }
        match self.street {
            Street::Preflop => {
                for _ in 0..3 {
                    self.board.push(self.deck.deal().unwrap())
                }
                self.street = Street::Flop
            }
            Street::Flop => {
                self.board.push(self.deck.deal().unwrap());
                self.street = Street::Turn
            }
            Street::Turn => {
                self.board.push(self.deck.deal().unwrap());
                self.street = Street::River
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
            Stakes::NoLimit { .. } => 0,
        };
        self.wagers = 0;
        for p in &mut self.players {
            p.street_contribution = 0;
            p.acted = p.folded || p.all_in
        }
        self.current_player = Some(self.next_live(self.button));
        self.current_player = Some(self.next_actor(self.button));
    }
    pub fn form_pots(&self) -> Vec<Pot> {
        let mut levels: Vec<Cents> = self
            .players
            .iter()
            .map(|p| p.contribution)
            .filter(|x| *x > 0)
            .collect();
        levels.sort_unstable();
        levels.dedup();
        let mut prev = 0;
        levels
            .into_iter()
            .map(|level| {
                let amount = (level - prev)
                    * self
                        .players
                        .iter()
                        .filter(|p| p.contribution >= level)
                        .count() as Cents;
                let eligible = self
                    .players
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.contribution >= level && !p.folded)
                    .map(|(i, _)| i)
                    .collect();
                prev = level;
                Pot {
                    amount,
                    eligible,
                    level,
                }
            })
            .collect()
    }
    pub fn showdown(&mut self) {
        self.street = Street::Showdown;
        let pots = self.form_pots();
        let mut awards = Vec::new();
        let mut results = Vec::new();
        for (i, p) in self.players.iter().enumerate() {
            if !p.folded {
                results.push(SeatResult {
                    seat: i,
                    hand: Some(evaluate(
                        &p.hole_cards
                            .iter()
                            .chain(self.board.iter())
                            .copied()
                            .collect::<Vec<_>>(),
                    )),
                });
            }
        }
        for pot in pots {
            let eligible: Vec<_> = results
                .iter()
                .filter(|r| pot.eligible.contains(&r.seat))
                .collect();
            if eligible.is_empty() {
                continue;
            }
            let best = eligible
                .iter()
                .map(|r| &r.hand.as_ref().unwrap().rank)
                .max()
                .unwrap();
            let winners: Vec<_> = eligible
                .into_iter()
                .filter(|r| r.hand.as_ref().unwrap().rank == *best)
                .collect();
            let share = pot.amount / winners.len() as Cents;
            let odd = (pot.amount % winners.len() as Cents) as usize;
            let order = self.winner_order(winners.iter().map(|r| r.seat).collect());
            for r in winners.iter() {
                let extra = if order.iter().take(odd).any(|seat| *seat == r.seat) {
                    1
                } else {
                    0
                };
                awards.push(Award {
                    seat: r.seat,
                    amount: share + extra,
                });
                self.players[r.seat].stack += share + extra;
            }
        }
        self.summary = Some(HandSummary {
            board: self.board.clone(),
            results,
            awards,
            revealed_hole_cards: self
                .players
                .iter()
                .enumerate()
                .filter(|(_, p)| !p.folded)
                .map(|(i, p)| (i, p.hole_cards.clone()))
                .collect(),
        });
        self.complete = true;
        self.current_player = None;
    }
    fn winner_order(&self, mut seats: Vec<usize>) -> Vec<usize> {
        seats.sort_by_key(|seat| {
            (*seat + self.players.len() - (self.button + 1) % self.players.len())
                % self.players.len()
        });
        seats
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::Card;
    fn c(s: &str) -> Card {
        s.parse().unwrap()
    }
    #[test]
    fn heads_up_blinds_and_order() {
        let h = Hand::new(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &[100, 100],
            0,
            4,
        );
        assert_eq!(h.players[0].contribution, 1);
        assert_eq!(h.players[1].contribution, 2);
        assert_eq!(h.current_player, Some(0));
    }
    #[test]
    fn big_blind_option() {
        let mut h = Hand::new(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &[100, 100, 100],
            0,
            5,
        );
        assert_eq!(h.current_player, Some(0));
        h.apply_action(Action::Call).unwrap();
        assert_eq!(h.current_player, Some(1));
        h.apply_action(Action::Call).unwrap();
        assert_eq!(h.current_player, Some(2));
        assert!(
            h.legal_actions()
                .actions
                .iter()
                .any(|a| matches!(a, Action::Raise { .. }))
        );
        h.apply_action(Action::Check).unwrap();
        assert_eq!(h.current_player, Some(1));
    }
    #[test]
    fn side_pots() {
        let h = Hand {
            seed: 0,
            stakes: Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            players: vec![
                Player {
                    stack: 0,
                    hole_cards: vec![],
                    folded: false,
                    all_in: true,
                    contribution: 10,
                    street_contribution: 0,
                    acted: true,
                },
                Player {
                    stack: 0,
                    hole_cards: vec![],
                    folded: false,
                    all_in: true,
                    contribution: 20,
                    street_contribution: 0,
                    acted: true,
                },
                Player {
                    stack: 0,
                    hole_cards: vec![],
                    folded: false,
                    all_in: true,
                    contribution: 30,
                    street_contribution: 0,
                    acted: true,
                },
            ],
            button: 0,
            street: Street::Showdown,
            board: vec![],
            current_player: None,
            deck: Deck::seeded(0),
            pot: 60,
            last_bet: 0,
            last_raise: 0,
            wagers: 0,
            complete: false,
            summary: None,
        };
        let p = h.form_pots();
        assert_eq!(
            p.iter().map(|p| p.amount).collect::<Vec<_>>(),
            vec![30, 20, 10]
        );
    }
    #[test]
    fn fold_awards_contributions() {
        let mut h = Hand::new(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &[100, 100],
            0,
            8,
        );
        let total = h.total_contributions();
        h.apply_action(Action::Fold).unwrap();
        assert!(h.complete);
        assert_eq!(h.summary.unwrap().awards[0].amount, total);
    }
    #[test]
    fn showdown_split_odd_chip() {
        let mut h = Hand::new(
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            &[100, 100],
            0,
            9,
        );
        h.players[0].hole_cards = vec![c("Ah"), c("Kd")];
        h.players[1].hole_cards = vec![c("As"), c("Kc")];
        h.players[0].contribution = 3;
        h.players[1].contribution = 2;
        h.players[0].stack = 0;
        h.players[1].stack = 0;
        h.pot = 5;
        h.board = vec![c("2h"), c("3d"), c("4s"), c("5c"), c("9h")];
        h.showdown();
        assert_eq!(
            h.summary
                .as_ref()
                .unwrap()
                .awards
                .iter()
                .map(|a| a.amount)
                .sum::<i64>(),
            5
        );
        assert_eq!(h.players[0].stack + h.players[1].stack, 5);
    }
}
