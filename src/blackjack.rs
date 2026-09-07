//! Blackjack, played alone against the house.
//!
//! One game per player: your own shoe, your own dealer, and nobody to wait for.
//! That is the whole shape of this module, and most of what it buys is what is
//! *absent* — there are no seats to index, no round the table, no betting clock
//! and no turn clock, because a clock only exists to stop one player holding up
//! another. Every action is answered by the request that made it.
//!
//! Sitting down is the one thing that costs money up front. A player chooses
//! the most they want to be able to bet, and buys in for ten times it; from
//! then on the game moves chips between that stack and the house and the bank
//! is not touched again until they get up. So a seated player's stack is the
//! only money this module can win or lose for them, which is what keeps the
//! chip conservation in SPEC §V1 to one number per player.

use crate::{
    cards::{Card, Deck},
    money::Cents,
};
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;
use uuid::Uuid;

/// The rungs the max-bet slider stops on: a 1-2-5 ladder from $100 to
/// $100,000. A slider that ran continuously would offer a $1,337 ceiling;
/// these are the numbers a table would actually print, and every one of them
/// divides into four whole-dollar wagers.
pub const MAX_BETS: [Cents; 10] = [
    10_000, 20_000, 50_000, 100_000, 200_000, 500_000, 1_000_000, 2_000_000, 5_000_000, 10_000_000,
];

/// Ten times the ceiling: enough to lose four maximum bets and still have a
/// table's worth of chips in front of you.
pub fn buy_in_for(max_bet: Cents) -> Cents {
    max_bet * 10
}

/// The only wagers a game offers, in quarter steps of its ceiling.
pub fn bet_options(max_bet: Cents) -> [Cents; 4] {
    [max_bet / 4, max_bet / 2, max_bet * 3 / 4, max_bet]
}

/// The rungs a balance can afford to sit down on, so the slider cannot be
/// dragged past what the player has.
pub fn affordable_max_bets(balance: Cents) -> Vec<Cents> {
    MAX_BETS
        .into_iter()
        .filter(|max_bet| buy_in_for(*max_bet) <= balance)
        .collect()
}

const MAX_HANDS: usize = 4;
const SAFE_RESERVE_CARDS: usize = 20;

#[derive(Clone, Debug, Serialize)]
pub struct BlackjackHandView {
    pub cards: Vec<Card>,
    pub bet: Cents,
    pub score: u8,
    pub status: BlackjackHandStatus,
    pub blackjack: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlackjackTrainerSettings {
    #[serde(default)]
    pub counting_tutor: bool,
    #[serde(default)]
    pub counting_quiz: bool,
    #[serde(default)]
    pub bet_analyzer: bool,
}

impl BlackjackTrainerSettings {
    pub fn sanitized(self) -> Self {
        Self {
            counting_tutor: self.counting_tutor,
            counting_quiz: self.counting_quiz,
            bet_analyzer: self.bet_analyzer,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct BlackjackShoeView {
    pub decks: u8,
    pub total_cards: usize,
    pub dealt_cards: usize,
    pub remaining_cards: usize,
    pub cut_card: usize,
    pub penetration_percent: u8,
    pub hands_dealt: usize,
    pub fresh_shuffle: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlackjackCountView {
    pub running: i16,
    pub true_count: f32,
    pub visible_cards: usize,
    pub penetration_percent: u8,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlackjackCountQuiz {
    pub prompt: String,
    pub choices: Vec<i16>,
    pub answer: i16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BlackjackHandStatus {
    Playing,
    Stand,
    Bust,
    Win,
    Loss,
    Push,
    Blackjack,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlackjackHand {
    pub cards: Vec<Card>,
    pub bet: Cents,
    pub status: BlackjackHandStatus,
    pub split: bool,
    pub split_aces: bool,
    /// Doubled down: the stake was raised and exactly one card taken.
    #[serde(default)]
    pub doubled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlackjackShoe {
    pub decks: u8,
    pub deck: Deck,
    pub cut_card: usize,
    pub hands_dealt: usize,
    pub running_count: i16,
    pub exposed_cards: usize,
    #[serde(default)]
    pub fresh_shuffle: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlackjackDecision {
    pub action: Action,
    pub recommended: Action,
}

fn cut_card(total_cards: usize, penetration_percent: u8) -> usize {
    let maximum_cut = total_cards.saturating_sub(SAFE_RESERVE_CARDS);
    ((total_cards * usize::from(penetration_percent) + 50) / 100).clamp(4, maximum_cut.max(4))
}

impl BlackjackShoe {
    pub fn fresh() -> Self {
        let decks = 8;
        let total_cards = usize::from(decks) * 52;
        Self {
            decks,
            deck: Deck::shoe_seeded(rand::thread_rng().r#gen(), decks),
            cut_card: cut_card(total_cards, 50),
            hands_dealt: 0,
            running_count: 0,
            exposed_cards: 0,
            fresh_shuffle: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Bet,
    Hit,
    Stand,
    Double,
    Split,
    Insure,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Action::Bet => "Bet",
            Action::Hit => "Hit",
            Action::Stand => "Stand",
            Action::Double => "Double",
            Action::Split => "Split",
            Action::Insure => "Insurance",
        }
    }
}

fn count_value(card: Card) -> i16 {
    match card.rank as u8 {
        2..=6 => 1,
        10..=14 => -1,
        _ => 0,
    }
}

fn count(cards: &[(String, Card)]) -> i16 {
    cards.iter().map(|(_, card)| count_value(*card)).sum()
}

fn count_view(running: i16, exposed_cards: usize, dealt_cards: usize) -> BlackjackCountView {
    let shoe_cards: usize = 8 * 52;
    let remaining_cards = shoe_cards.saturating_sub(exposed_cards).max(1);
    let decks_remaining = (remaining_cards as f32 / 52.0).max(0.25);
    BlackjackCountView {
        running,
        true_count: running as f32 / decks_remaining,
        visible_cards: exposed_cards,
        penetration_percent: ((dealt_cards * 100) / shoe_cards.max(1)) as u8,
    }
}

fn count_log(cards: &[(String, Card)], base_count: i16) -> Vec<String> {
    let mut running = base_count;
    let mut lines = vec![format!("Carry-in running count: {base_count}")];
    lines.extend(cards.iter().map(|(label, card)| {
        let delta = count_value(*card);
        running += delta;
        let signed = if delta >= 0 {
            format!("+{delta}")
        } else {
            delta.to_string()
        };
        format!("{label} {card}: {signed} -> {running}")
    }));
    lines
}

fn count_quiz(answer: i16) -> BlackjackCountQuiz {
    let mut choices = vec![answer - 2, answer - 1, answer, answer + 1];
    choices.sort_unstable();
    choices.dedup();
    BlackjackCountQuiz {
        prompt: "What is the running count?".into(),
        choices,
        answer,
    }
}

fn dealer_value(rank: u8) -> u8 {
    rank.min(10)
}

fn should_split(rank: u8, dealer: u8) -> bool {
    let dealer = dealer_value(dealer);
    match rank {
        14 | 8 => true,
        10 | 5 => false,
        9 => matches!(dealer, 2..=6 | 8 | 9),
        7 => matches!(dealer, 2..=7),
        6 => matches!(dealer, 2..=6),
        4 => matches!(dealer, 5 | 6),
        3 | 2 => matches!(dealer, 2..=7),
        _ => false,
    }
}

fn should_double(total: u8, soft: bool, dealer: u8) -> bool {
    let dealer = dealer_value(dealer);
    if soft {
        return matches!(
            (total, dealer),
            (13 | 14, 5 | 6) | (15 | 16, 4..=6) | (17, 3..=6) | (18, 2..=6)
        );
    }
    matches!((total, dealer), (9, 3..=6) | (10, 2..=9) | (11, 2..=10))
}

fn should_hit(total: u8, soft: bool, dealer: u8) -> bool {
    let dealer = dealer_value(dealer);
    if soft {
        return total <= 17 || (total == 18 && dealer >= 9);
    }
    total <= 11
        || (total == 12 && !matches!(dealer, 4..=6))
        || (13..=16).contains(&total) && dealer >= 7
}

fn recommended_action(dealer: &[Card], hand: &BlackjackHand, action: Action) -> Action {
    let Some(up_card) = dealer.first() else {
        return Action::Stand;
    };
    if action == Action::Insure {
        return Action::Stand;
    }
    let dealer = up_card.rank as u8;
    if hand.cards.len() == 2
        && hand.cards[0].rank == hand.cards[1].rank
        && should_split(hand.cards[0].rank as u8, dealer)
    {
        return Action::Split;
    }
    let (total, soft) = score(&hand.cards);
    if hand.cards.len() == 2 && should_double(total, soft, dealer) {
        return Action::Double;
    }
    if should_hit(total, soft, dealer) {
        Action::Hit
    } else {
        Action::Stand
    }
}

pub fn score(cards: &[Card]) -> (u8, bool) {
    let mut total = 0;
    let mut aces = 0;
    for c in cards {
        let v = match c.rank as u8 {
            14 => {
                aces += 1;
                11
            }
            r if r >= 10 => 10,
            r => r,
        };
        total += v;
    }
    while total > 21 && aces > 0 {
        total -= 10;
        aces -= 1;
    }
    (total, aces > 0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlackjackError {
    NotFound,
    Finished,
    ActiveGame,
    IllegalAction(&'static str),
}

impl BlackjackError {
    pub const fn message(self) -> &'static str {
        match self {
            BlackjackError::NotFound => "you are not sitting at a blackjack table",
            BlackjackError::Finished => "that hand is over",
            BlackjackError::ActiveGame => "finish the hand you are playing first",
            BlackjackError::IllegalAction(message) => message,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Betting,
    Insurance,
    Playing,
    Settled,
}

/// What one round did, on its way to the stats store.
#[derive(Clone, Debug)]
pub struct BlackjackSettlement {
    pub user: Uuid,
    pub net: Cents,
    pub outcome: crate::blackjack_stats::RoundOutcome,
}

/// One player's whole blackjack world: their shoe, their dealer, their stack.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlackjackGame {
    pub id: Uuid,
    pub user: Uuid,
    pub max_bet: Cents,
    pub stack: Cents,
    pub bet: Option<Cents>,
    pub hands: Vec<BlackjackHand>,
    pub insurance: Cents,
    pub insurance_decided: bool,
    pub shoe: BlackjackShoe,
    pub phase: Phase,
    pub dealer: Vec<Card>,
    pub dealer_peeked: bool,
    pub current: Option<usize>,
    pub round_no: u64,
    #[serde(default)]
    pub last_result: Option<String>,
    pub settings: BlackjackTrainerSettings,
    pub decisions: Vec<BlackjackDecision>,
    pub updated_at: DateTime<Utc>,
}

/// Everything the page draws, for a player who is sitting or one who is not.
#[derive(Clone, Debug, Serialize)]
pub struct BlackjackView {
    pub seated: bool,
    pub id: Option<Uuid>,
    pub bank_balance: Cents,
    /// Every rung the slider has, and the ones this balance can afford.
    pub max_bets: Vec<Cents>,
    pub affordable_max_bets: Vec<Cents>,
    pub max_bet: Cents,
    pub buy_in: Cents,
    pub bet_options: Vec<Cents>,
    pub min_bet: Cents,
    pub phase: Phase,
    pub stack: Cents,
    pub bet: Option<Cents>,
    pub insurance: Cents,
    pub hands: Vec<BlackjackHandView>,
    pub current_hand: Option<usize>,
    pub dealer: Vec<Card>,
    pub dealer_hidden: bool,
    pub dealer_score: Option<u8>,
    pub result: Option<String>,
    pub can_sit: bool,
    pub can_leave: bool,
    pub can_rebuy: bool,
    pub can_bet: bool,
    pub can_insure: bool,
    pub can_decline: bool,
    pub can_hit: bool,
    pub can_stand: bool,
    pub can_double: bool,
    pub can_split: bool,
    pub message: String,
    pub shoe: Option<BlackjackShoeView>,
    pub trainer: Option<BlackjackTrainerView>,
    pub settings: BlackjackTrainerSettings,
    pub fresh_shuffle: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlackjackTrainerView {
    pub count: Option<BlackjackCountView>,
    pub log: Vec<String>,
    pub analysis: Vec<String>,
    pub quiz: Option<BlackjackCountQuiz>,
}

/// What the player may do right now: hit, stand, double, split.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActionFlags {
    pub hit: bool,
    pub stand: bool,
    pub double: bool,
    pub split: bool,
}

impl BlackjackGame {
    pub fn new(id: Uuid, user: Uuid, max_bet: Cents, stack: Cents) -> Self {
        Self {
            id,
            user,
            max_bet,
            stack,
            bet: None,
            hands: Vec::new(),
            insurance: 0,
            insurance_decided: false,
            shoe: BlackjackShoe::fresh(),
            phase: Phase::Betting,
            dealer: Vec::new(),
            dealer_peeked: false,
            current: None,
            round_no: 0,
            last_result: None,
            settings: BlackjackTrainerSettings::default(),
            decisions: Vec::new(),
            updated_at: Utc::now(),
        }
    }

    /// True while chips are committed to the felt — the one time leaving or
    /// rebuying has to wait.
    pub fn in_round(&self) -> bool {
        self.bet.is_some()
    }

    /// Clears a settled round off the felt. Nothing is on a clock here, so the
    /// last hand stays visible until the player asks for another.
    fn clear_round(&mut self) {
        self.phase = Phase::Betting;
        self.last_result = None;
        self.dealer.clear();
        self.dealer_peeked = false;
        self.current = None;
        self.hands.clear();
        self.insurance = 0;
        self.insurance_decided = false;
        self.decisions.clear();
    }

    pub fn place_bet(
        &mut self,
        amount: Cents,
    ) -> Result<Option<BlackjackSettlement>, BlackjackError> {
        if self.phase == Phase::Settled {
            self.clear_round();
        }
        if self.phase != Phase::Betting {
            return Err(BlackjackError::IllegalAction("betting is closed"));
        }
        if !bet_options(self.max_bet).contains(&amount) {
            return Err(BlackjackError::IllegalAction("that wager is not offered"));
        }
        if self.bet.is_some() {
            return Err(BlackjackError::IllegalAction("you already placed a bet"));
        }
        if self.stack < amount {
            return Err(BlackjackError::IllegalAction("insufficient table chips"));
        }
        self.stack -= amount;
        self.bet = Some(amount);
        self.deal()
    }

    fn deal(&mut self) -> Result<Option<BlackjackSettlement>, BlackjackError> {
        let bet = self
            .bet
            .ok_or(BlackjackError::IllegalAction("place a bet first"))?;
        self.shoe.fresh_shuffle = false;
        self.round_no += 1;
        self.shoe.hands_dealt += 1;
        self.dealer.clear();
        self.dealer_peeked = false;
        self.current = None;
        self.hands.clear();
        self.insurance = 0;
        self.insurance_decided = false;
        self.decisions.clear();
        self.last_result = None;
        let first = self.draw();
        let second = self.draw();
        let cards = vec![first, second];
        let natural = score(&cards).0 == 21;
        self.hands.push(BlackjackHand {
            cards,
            bet,
            status: if natural {
                BlackjackHandStatus::Blackjack
            } else {
                BlackjackHandStatus::Playing
            },
            split: false,
            split_aces: false,
            doubled: false,
        });
        let dealer_up = self.draw();
        let dealer_hole = self.draw();
        self.dealer.push(dealer_up);
        self.dealer.push(dealer_hole);
        if self.dealer[0].rank as u8 == 14 && self.stack >= bet / 2 {
            self.phase = Phase::Insurance;
            return Ok(None);
        }
        self.peek()
    }

    fn draw(&mut self) -> Card {
        if self.shoe.deck.dealt() >= self.shoe.cut_card {
            self.shoe = BlackjackShoe::fresh();
            self.shoe.fresh_shuffle = true;
        }
        self.shoe.deck.deal().expect("fresh blackjack shoe")
    }

    pub fn insure(&mut self) -> Result<Option<BlackjackSettlement>, BlackjackError> {
        if self.phase != Phase::Insurance {
            return Err(BlackjackError::IllegalAction("insurance is not available"));
        }
        let amount = self.bet.unwrap_or_default() / 2;
        if self.insurance_decided || self.stack < amount {
            return Err(BlackjackError::IllegalAction("insurance is not available"));
        }
        self.stack -= amount;
        self.insurance = amount;
        self.insurance_decided = true;
        self.peek()
    }

    pub fn decline(&mut self) -> Result<Option<BlackjackSettlement>, BlackjackError> {
        if self.phase != Phase::Insurance {
            return Err(BlackjackError::IllegalAction("insurance is not available"));
        }
        if self.insurance_decided {
            return Err(BlackjackError::IllegalAction(
                "insurance is already decided",
            ));
        }
        self.insurance_decided = true;
        self.peek()
    }

    fn peek(&mut self) -> Result<Option<BlackjackSettlement>, BlackjackError> {
        self.dealer_peeked = true;
        if score(&self.dealer).0 == 21 {
            for hand in &mut self.hands {
                hand.status = if hand.status == BlackjackHandStatus::Blackjack {
                    BlackjackHandStatus::Push
                } else {
                    BlackjackHandStatus::Loss
                };
            }
            return self.settle();
        }
        match self
            .hands
            .iter()
            .position(|hand| hand.status == BlackjackHandStatus::Playing)
        {
            Some(index) => {
                self.phase = Phase::Playing;
                self.current = Some(index);
                Ok(None)
            }
            None => self.settle(),
        }
    }

    pub fn act(&mut self, action: Action) -> Result<Option<BlackjackSettlement>, BlackjackError> {
        let hand_index = self.current.ok_or(BlackjackError::Finished)?;
        if self.phase != Phase::Playing {
            return Err(BlackjackError::IllegalAction("it is not your turn"));
        }
        let recommended = {
            let hand = self.hands.get(hand_index).expect("hand");
            recommended_action(&self.dealer, hand, action)
        };
        self.decisions.push(BlackjackDecision {
            action,
            recommended,
        });
        match action {
            Action::Hit => {
                if self.hands[hand_index].split_aces {
                    return Err(BlackjackError::IllegalAction("that action is not legal"));
                }
                let card = self.draw();
                let hand = self.hands.get_mut(hand_index).expect("hand");
                hand.cards.push(card);
                match score(&hand.cards).0 {
                    21 => hand.status = BlackjackHandStatus::Stand,
                    total if total > 21 => hand.status = BlackjackHandStatus::Bust,
                    _ => {}
                }
            }
            Action::Stand => {
                self.hands.get_mut(hand_index).expect("hand").status = BlackjackHandStatus::Stand;
            }
            Action::Double => {
                let hand = self.hands.get(hand_index).expect("hand");
                let bet = hand.bet;
                if hand.cards.len() != 2 || hand.split_aces || self.stack < bet {
                    return Err(BlackjackError::IllegalAction("that action is not legal"));
                }
                let card = self.draw();
                self.stack -= bet;
                let hand = self.hands.get_mut(hand_index).expect("hand");
                hand.bet *= 2;
                hand.doubled = true;
                hand.cards.push(card);
                hand.status = if score(&hand.cards).0 > 21 {
                    BlackjackHandStatus::Bust
                } else {
                    BlackjackHandStatus::Stand
                };
            }
            Action::Split => {
                let hand = self.hands.get(hand_index).expect("hand");
                let legal = hand.cards.len() == 2
                    && hand.cards[0].rank == hand.cards[1].rank
                    && self.hands.len() < MAX_HANDS
                    && self.stack >= hand.bet;
                if !legal {
                    return Err(BlackjackError::IllegalAction("that action is not legal"));
                }
                let first_card = self.draw();
                let second_card = self.draw();
                let hand = self.hands.get_mut(hand_index).expect("hand");
                let bet = hand.bet;
                let second = hand.cards.pop().expect("pair");
                let split_aces = second.rank as u8 == 14;
                hand.split = true;
                hand.cards.push(first_card);
                hand.split_aces = split_aces;
                hand.status = if score(&hand.cards).0 > 21 {
                    BlackjackHandStatus::Bust
                } else if split_aces {
                    BlackjackHandStatus::Stand
                } else {
                    BlackjackHandStatus::Playing
                };
                self.stack -= bet;
                self.hands.insert(
                    hand_index + 1,
                    BlackjackHand {
                        cards: vec![second, second_card],
                        bet,
                        status: if split_aces {
                            BlackjackHandStatus::Stand
                        } else {
                            BlackjackHandStatus::Playing
                        },
                        split: true,
                        split_aces,
                        doubled: false,
                    },
                );
            }
            _ => return Err(BlackjackError::IllegalAction("that action is not legal")),
        }
        self.advance_current()
    }

    fn advance_current(&mut self) -> Result<Option<BlackjackSettlement>, BlackjackError> {
        self.current = self
            .hands
            .iter()
            .position(|hand| hand.status == BlackjackHandStatus::Playing);
        if self.current.is_some() {
            Ok(None)
        } else {
            self.settle()
        }
    }

    fn insurance_payout(&self) -> Cents {
        if self.insurance > 0 && score(&self.dealer).0 == 21 && self.dealer.len() == 2 {
            self.insurance * 3
        } else {
            0
        }
    }

    fn settle(&mut self) -> Result<Option<BlackjackSettlement>, BlackjackError> {
        let visible_before_settlement = self.visible_cards();
        let all_busted = self
            .hands
            .iter()
            .all(|hand| hand.status == BlackjackHandStatus::Bust);
        if !all_busted {
            while score(&self.dealer).0 < 17 {
                let card = self.draw();
                self.dealer.push(card);
            }
        }
        let mut exposed_at_settlement = visible_before_settlement;
        exposed_at_settlement.extend(
            self.dealer
                .iter()
                .skip(1)
                .copied()
                .map(|card| ("Dealer".into(), card)),
        );
        let dealer_score = score(&self.dealer).0;
        let mut returned = self.insurance_payout();
        for hand in &mut self.hands {
            if hand.status == BlackjackHandStatus::Stand {
                let player = score(&hand.cards).0;
                hand.status = if dealer_score > 21 || player > dealer_score {
                    BlackjackHandStatus::Win
                } else if player < dealer_score {
                    BlackjackHandStatus::Loss
                } else {
                    BlackjackHandStatus::Push
                };
            }
            returned += match hand.status {
                BlackjackHandStatus::Win => hand.bet * 2,
                BlackjackHandStatus::Push => hand.bet,
                BlackjackHandStatus::Blackjack if !hand.split => hand.bet * 5 / 2,
                _ => 0,
            };
        }
        self.stack += returned;
        let wagered = self.hands.iter().map(|hand| hand.bet).sum::<Cents>() + self.insurance;
        let net = returned - wagered;
        self.last_result = Some(if net >= 0 {
            format!("Won ${}", net / 100)
        } else {
            format!("Lost ${}", -net / 100)
        });
        let settlement = BlackjackSettlement {
            user: self.user,
            net,
            outcome: outcome_for(self, returned),
        };
        self.shoe.running_count += count(&exposed_at_settlement);
        self.shoe.exposed_cards += exposed_at_settlement.len();
        self.phase = Phase::Settled;
        self.current = None;
        self.bet = None;
        Ok(Some(settlement))
    }

    pub fn action_flags(&self) -> ActionFlags {
        let Some(hand_index) = self.current else {
            return ActionFlags::default();
        };
        if self.phase != Phase::Playing {
            return ActionFlags::default();
        }
        let hand = self.hands.get(hand_index).expect("hand");
        ActionFlags {
            hit: true,
            stand: true,
            double: hand.cards.len() == 2 && !hand.split_aces && self.stack >= hand.bet,
            split: hand.cards.len() == 2
                && hand.cards[0].rank == hand.cards[1].rank
                && self.hands.len() < MAX_HANDS
                && self.stack >= hand.bet,
        }
    }

    fn visible_cards(&self) -> Vec<(String, Card)> {
        let mut cards = self
            .dealer
            .first()
            .copied()
            .map(|card| vec![("Dealer up".into(), card)])
            .unwrap_or_default();
        if self.phase == Phase::Settled {
            cards.extend(
                self.dealer
                    .iter()
                    .skip(1)
                    .copied()
                    .map(|card| ("Dealer".into(), card)),
            );
        }
        for hand in &self.hands {
            cards.extend(hand.cards.iter().copied().map(|card| ("You".into(), card)));
        }
        cards
    }

    fn status_message(&self) -> String {
        match self.phase {
            Phase::Betting => "Place your bet".into(),
            Phase::Insurance => "Dealer shows an Ace — insurance?".into(),
            Phase::Playing => "Your move".into(),
            Phase::Settled => self
                .last_result
                .clone()
                .unwrap_or_else(|| "Round settled".into()),
        }
    }

    pub fn view(&self, bank_balance: Cents) -> BlackjackView {
        let flags = self.action_flags();
        let can_insure = self.phase == Phase::Insurance
            && !self.insurance_decided
            && self.stack >= self.bet.unwrap_or_default() / 2;
        let dealt_cards = 416usize.saturating_sub(self.shoe.deck.remaining());
        let shoe = BlackjackShoeView {
            decks: self.shoe.decks,
            total_cards: 416,
            dealt_cards,
            remaining_cards: self.shoe.deck.remaining(),
            cut_card: self.shoe.cut_card,
            penetration_percent: 50,
            hands_dealt: self.shoe.hands_dealt,
            fresh_shuffle: self.shoe.fresh_shuffle,
        };
        let cards = self.visible_cards();
        let running = if self.phase == Phase::Settled {
            self.shoe.running_count
        } else {
            self.shoe.running_count + count(&cards)
        };
        let trainer = BlackjackTrainerView {
            count: (self.settings.counting_tutor || self.settings.counting_quiz)
                .then(|| count_view(running, cards.len(), dealt_cards)),
            log: if self.settings.counting_tutor {
                count_log(&cards, self.shoe.running_count)
            } else {
                Vec::new()
            },
            analysis: if self.settings.bet_analyzer {
                self.decisions
                    .iter()
                    .filter(|decision| decision.action != decision.recommended)
                    .map(|decision| {
                        format!(
                            "{} was off; basic strategy prefers {} here.",
                            decision.action.label(),
                            decision.recommended.label()
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            },
            quiz: (self.settings.counting_quiz && self.phase == Phase::Settled)
                .then(|| count_quiz(running)),
        };
        BlackjackView {
            seated: true,
            id: Some(self.id),
            bank_balance,
            max_bets: MAX_BETS.to_vec(),
            affordable_max_bets: affordable_max_bets(bank_balance),
            max_bet: self.max_bet,
            buy_in: buy_in_for(self.max_bet),
            bet_options: bet_options(self.max_bet).to_vec(),
            min_bet: self.max_bet / 4,
            phase: self.phase,
            stack: self.stack,
            bet: self.bet,
            insurance: self.insurance,
            hands: self
                .hands
                .iter()
                .map(|hand| BlackjackHandView {
                    cards: hand.cards.clone(),
                    bet: hand.bet,
                    score: score(&hand.cards).0,
                    status: hand.status,
                    blackjack: hand.status == BlackjackHandStatus::Blackjack && !hand.split,
                })
                .collect(),
            current_hand: self.current,
            dealer: if matches!(self.phase, Phase::Playing | Phase::Insurance) {
                self.dealer.first().copied().into_iter().collect()
            } else {
                self.dealer.clone()
            },
            dealer_hidden: matches!(self.phase, Phase::Insurance | Phase::Playing)
                && self.dealer.len() > 1,
            dealer_score: (self.phase == Phase::Settled).then(|| score(&self.dealer).0),
            result: self.last_result.clone(),
            can_sit: false,
            can_leave: !self.in_round(),
            can_rebuy: !self.in_round() && self.stack < buy_in_for(self.max_bet),
            can_bet: matches!(self.phase, Phase::Betting | Phase::Settled)
                && self.bet.is_none()
                && self.stack >= self.max_bet / 4,
            can_insure,
            can_decline: can_insure,
            can_hit: flags.hit,
            can_stand: flags.stand,
            can_double: flags.double,
            can_split: flags.split,
            message: self.status_message(),
            shoe: Some(shoe),
            trainer: Some(trainer),
            settings: self.settings.clone(),
            fresh_shuffle: self.shoe.fresh_shuffle,
        }
    }
}

/// The view a player who has not sat down yet sees: the slider, and nothing
/// else that would need a game to exist.
fn empty_view(bank_balance: Cents) -> BlackjackView {
    let affordable = affordable_max_bets(bank_balance);
    let max_bet = affordable.first().copied().unwrap_or(MAX_BETS[0]);
    BlackjackView {
        seated: false,
        id: None,
        bank_balance,
        max_bets: MAX_BETS.to_vec(),
        affordable_max_bets: affordable.clone(),
        max_bet,
        buy_in: buy_in_for(max_bet),
        bet_options: bet_options(max_bet).to_vec(),
        min_bet: max_bet / 4,
        phase: Phase::Betting,
        stack: 0,
        bet: None,
        insurance: 0,
        hands: Vec::new(),
        current_hand: None,
        dealer: Vec::new(),
        dealer_hidden: false,
        dealer_score: None,
        result: None,
        can_sit: !affordable.is_empty(),
        can_leave: false,
        can_rebuy: false,
        can_bet: false,
        can_insure: false,
        can_decline: false,
        can_hit: false,
        can_stand: false,
        can_double: false,
        can_split: false,
        message: if affordable.is_empty() {
            "You need more in the bank to sit down".into()
        } else {
            "Choose your maximum bet".into()
        },
        shoe: None,
        trainer: None,
        settings: BlackjackTrainerSettings::default(),
        fresh_shuffle: false,
    }
}

fn outcome_for(game: &BlackjackGame, returned: Cents) -> crate::blackjack_stats::RoundOutcome {
    let mut outcome = crate::blackjack_stats::RoundOutcome {
        hands: game.hands.len() as u64,
        splits: game.hands.len().saturating_sub(1) as u64,
        insured: game.insurance > 0,
        wagered: game.hands.iter().map(|hand| hand.bet).sum::<Cents>() + game.insurance,
        returned,
        ..Default::default()
    };
    for hand in &game.hands {
        match hand.status {
            BlackjackHandStatus::Win => outcome.won += 1,
            BlackjackHandStatus::Push => outcome.push += 1,
            BlackjackHandStatus::Bust => {
                outcome.lost += 1;
                outcome.busts += 1;
            }
            BlackjackHandStatus::Blackjack => {
                outcome.won += 1;
                outcome.naturals += 1;
            }
            _ => outcome.lost += 1,
        }
        outcome.doubles += u64::from(hand.doubled);
    }
    outcome
}

/// Every game in play, keyed by the player it belongs to.
#[derive(Clone, Default)]
pub struct BlackjackStore {
    games: Arc<Mutex<HashMap<Uuid, BlackjackGame>>>,
    path: Option<PathBuf>,
}

impl BlackjackStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub(crate) fn from_games(games: Vec<BlackjackGame>) -> Self {
        Self {
            games: Arc::new(Mutex::new(
                games.into_iter().map(|game| (game.user, game)).collect(),
            )),
            path: None,
        }
    }

    pub async fn load(root: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let dir = root.as_ref().join("blackjack");
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join("solo.json");
        let mut games: HashMap<Uuid, BlackjackGame> = match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice::<Vec<BlackjackGame>>(&bytes)?
                .into_iter()
                .map(|game| (game.user, game))
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => return Err(error.into()),
        };
        let migrated = migrate_shared_tables(&dir, &mut games).await?;
        // A hand can only have been interrupted by a restart, so it never
        // resumes: the chips on the felt go back to the stack they came from,
        // which is what keeps §V1 true across a restart.
        for game in games.values_mut() {
            if game.phase != Phase::Betting {
                game.stack +=
                    game.hands.iter().map(|hand| hand.bet).sum::<Cents>() + game.insurance;
                game.bet = None;
                game.clear_round();
            }
        }
        let store = Self {
            games: Arc::new(Mutex::new(games)),
            path: Some(path),
        };
        if migrated {
            store.persist().await?;
        }
        Ok(store)
    }

    pub async fn persist(&self) -> Result<(), anyhow::Error> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let games = self.games.lock().await;
        let mut rows: Vec<&BlackjackGame> = games.values().collect();
        rows.sort_by_key(|game| game.user);
        let body = serde_json::to_vec_pretty(&rows)?;
        drop(games);
        let tmp = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
        tokio::fs::write(&tmp, body).await?;
        tokio::fs::rename(tmp, path).await?;
        Ok(())
    }

    pub async fn view(&self, user: Option<Uuid>, balance: Cents) -> BlackjackView {
        let games = self.games.lock().await;
        match user.and_then(|user| games.get(&user)) {
            Some(game) => game.view(balance),
            None => empty_view(balance),
        }
    }

    /// Buys in at ten times the chosen ceiling. The bank is touched here and
    /// on the way out, and nowhere in between.
    pub async fn sit(
        &self,
        user: Uuid,
        max_bet: Cents,
        settings: BlackjackTrainerSettings,
        bank: &crate::bank::BankStore,
    ) -> Result<(), BlackjackError> {
        if !MAX_BETS.contains(&max_bet) {
            return Err(BlackjackError::IllegalAction(
                "that maximum bet is not offered",
            ));
        }
        if self.games.lock().await.contains_key(&user) {
            return Err(BlackjackError::ActiveGame);
        }
        let id = Uuid::new_v4();
        let buy_in = buy_in_for(max_bet);
        bank.blackjack_buy_in(crate::bank::AccountOwner::User(user), id, buy_in)
            .await
            .map_err(|_| BlackjackError::IllegalAction("insufficient funds"))?;
        {
            let mut games = self.games.lock().await;
            if games.contains_key(&user) {
                drop(games);
                let _ = bank
                    .blackjack_cash_out(crate::bank::AccountOwner::User(user), id, buy_in)
                    .await;
                return Err(BlackjackError::ActiveGame);
            }
            let mut game = BlackjackGame::new(id, user, max_bet, buy_in);
            game.settings = settings.sanitized();
            games.insert(user, game);
        }
        self.persist()
            .await
            .map_err(|_| BlackjackError::IllegalAction("could not persist the game"))
    }

    pub async fn leave(
        &self,
        user: Uuid,
        bank: &crate::bank::BankStore,
    ) -> Result<(), BlackjackError> {
        let (id, stack) = {
            let mut games = self.games.lock().await;
            let game = games.get(&user).ok_or(BlackjackError::NotFound)?;
            if game.in_round() {
                return Err(BlackjackError::IllegalAction(
                    "finish the hand you are playing first",
                ));
            }
            let game = games.remove(&user).expect("game");
            (game.id, game.stack)
        };
        bank.blackjack_cash_out(crate::bank::AccountOwner::User(user), id, stack)
            .await
            .map_err(|_| BlackjackError::IllegalAction("cash out failed"))?;
        self.persist()
            .await
            .map_err(|_| BlackjackError::IllegalAction("could not persist the game"))
    }

    pub async fn rebuy(
        &self,
        user: Uuid,
        bank: &crate::bank::BankStore,
    ) -> Result<(), BlackjackError> {
        let (id, amount) = {
            let games = self.games.lock().await;
            let game = games.get(&user).ok_or(BlackjackError::NotFound)?;
            if game.in_round() {
                return Err(BlackjackError::IllegalAction(
                    "rebuy is unavailable during a round",
                ));
            }
            (game.id, buy_in_for(game.max_bet).saturating_sub(game.stack))
        };
        if amount == 0 {
            return Err(BlackjackError::IllegalAction("your stack is already full"));
        }
        bank.blackjack_buy_in(crate::bank::AccountOwner::User(user), id, amount)
            .await
            .map_err(|_| BlackjackError::IllegalAction("insufficient funds"))?;
        {
            let mut games = self.games.lock().await;
            let game = games.get_mut(&user).ok_or(BlackjackError::NotFound)?;
            game.stack += amount;
            game.updated_at = Utc::now();
        }
        self.persist()
            .await
            .map_err(|_| BlackjackError::IllegalAction("could not persist the game"))
    }

    async fn resolve(
        &self,
        user: Uuid,
        action: impl FnOnce(&mut BlackjackGame) -> Result<Option<BlackjackSettlement>, BlackjackError>,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        let settlement = {
            let mut games = self.games.lock().await;
            let game = games.get_mut(&user).ok_or(BlackjackError::NotFound)?;
            let settlement = action(game)?;
            game.updated_at = Utc::now();
            settlement
        };
        if let Some(settlement) = settlement {
            let _ = stats.record(settlement.user, settlement.outcome).await;
        }
        self.persist()
            .await
            .map_err(|_| BlackjackError::IllegalAction("could not persist the game"))
    }

    pub async fn bet(
        &self,
        user: Uuid,
        amount: Cents,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        self.resolve(user, |game| game.place_bet(amount), stats)
            .await
    }

    pub async fn insure(
        &self,
        user: Uuid,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        self.resolve(user, BlackjackGame::insure, stats).await
    }

    pub async fn decline(
        &self,
        user: Uuid,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        self.resolve(user, BlackjackGame::decline, stats).await
    }

    pub async fn act(
        &self,
        user: Uuid,
        action: Action,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        self.resolve(user, |game| game.act(action), stats).await
    }

    pub async fn update_settings(
        &self,
        user: Uuid,
        settings: BlackjackTrainerSettings,
    ) -> Result<(), BlackjackError> {
        {
            let mut games = self.games.lock().await;
            let game = games.get_mut(&user).ok_or(BlackjackError::NotFound)?;
            game.settings = settings.sanitized();
            game.updated_at = Utc::now();
        }
        self.persist()
            .await
            .map_err(|_| BlackjackError::IllegalAction("could not persist the game"))
    }
}

/// The shared-table era, read only so its chips can be handed back.
///
/// Seats were bought in against the *table's* id, so each imported game keeps
/// that id: the cash-out that eventually closes it lands in the same player's
/// ledger against the same game, and §V1 never sees the money go missing.
#[derive(Deserialize)]
struct LegacyTable {
    id: Uuid,
    max_bet: Cents,
    seats: Vec<Option<LegacySeat>>,
}

#[derive(Deserialize)]
struct LegacySeat {
    user: Uuid,
    stack: Cents,
    #[serde(default)]
    hands: Vec<LegacyHand>,
    #[serde(default)]
    insurance: Cents,
    #[serde(default)]
    settings: BlackjackTrainerSettings,
}

#[derive(Deserialize)]
struct LegacyHand {
    bet: Cents,
}

async fn migrate_shared_tables(
    dir: &Path,
    games: &mut HashMap<Uuid, BlackjackGame>,
) -> Result<bool, anyhow::Error> {
    let path = dir.join("tables.json");
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let tables: Vec<LegacyTable> = serde_json::from_slice(&bytes)?;
    let mut imported = 0usize;
    for table in tables {
        let max_bet = MAX_BETS
            .into_iter()
            .find(|rung| *rung >= table.max_bet)
            .unwrap_or(MAX_BETS[MAX_BETS.len() - 1]);
        for seat in table.seats.into_iter().flatten() {
            if games.contains_key(&seat.user) {
                continue;
            }
            let live = seat.hands.iter().map(|hand| hand.bet).sum::<Cents>() + seat.insurance;
            let mut game = BlackjackGame::new(table.id, seat.user, max_bet, seat.stack + live);
            game.settings = seat.settings.sanitized();
            games.insert(seat.user, game);
            imported += 1;
        }
    }
    tokio::fs::rename(&path, dir.join("tables.migrated.json")).await?;
    tracing::info!(imported, "carried blackjack seats into solo games");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(rank: crate::cards::Rank) -> Card {
        Card::new(rank, crate::cards::Suit::Spades)
    }

    fn game(stack: Cents) -> BlackjackGame {
        BlackjackGame::new(Uuid::new_v4(), Uuid::new_v4(), 10_000, stack)
    }

    /// Stacks the shoe and moves the cut card out of reach, so a rigged deal
    /// is not reshuffled out from under the test.
    fn rig(game: &mut BlackjackGame, cards: Vec<Card>) {
        game.shoe.deck = Deck::from_cards(cards);
        game.shoe.cut_card = usize::MAX;
    }

    #[test]
    fn the_slider_prices_a_seat() {
        assert_eq!(bet_options(10_000), [2_500, 5_000, 7_500, 10_000]);
        assert_eq!(buy_in_for(10_000_000), 100_000_000);
        // Every rung divides into four whole-dollar wagers.
        for max_bet in MAX_BETS {
            for option in bet_options(max_bet) {
                assert_eq!(option % 100, 0, "{max_bet} pays a fraction of a dollar");
            }
        }
        // A balance buys every rung it can cover ten times over, and no more.
        assert_eq!(affordable_max_bets(0), Vec::<Cents>::new());
        assert_eq!(affordable_max_bets(99_999), Vec::<Cents>::new());
        assert_eq!(affordable_max_bets(100_000), vec![10_000]);
        assert_eq!(
            affordable_max_bets(1_000_000),
            vec![10_000, 20_000, 50_000, 100_000]
        );
    }

    #[test]
    fn old_trainer_payload_fields_are_ignored() {
        let settings: BlackjackTrainerSettings =
            serde_json::from_str(r#"{"decks":2,"penetration_percent":25,"counting_tutor":true}"#)
                .expect("legacy trainer payload");
        assert!(settings.counting_tutor);
        assert_eq!(
            settings,
            BlackjackTrainerSettings {
                counting_tutor: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn a_bet_deals_immediately_and_nothing_is_on_a_clock() {
        let mut game = game(100_000);
        rig(
            &mut game,
            vec![
                card(crate::cards::Rank::Ten),
                card(crate::cards::Rank::Eight),
                card(crate::cards::Rank::Nine),
                card(crate::cards::Rank::Seven),
            ],
        );
        assert!(game.place_bet(2_500).unwrap().is_none());
        assert_eq!(game.phase, Phase::Playing);
        assert_eq!(game.current, Some(0));
        assert_eq!(game.stack, 97_500);
        assert_eq!(game.hands[0].cards.len(), 2);
        // The hole card stays face down while the hand is live.
        let view = game.view(0);
        assert_eq!(view.dealer.len(), 1);
        assert!(view.dealer_hidden);
    }

    #[test]
    fn an_unoffered_wager_is_refused_and_the_stack_is_untouched() {
        let mut game = game(100_000);
        assert_eq!(
            game.place_bet(99).unwrap_err(),
            BlackjackError::IllegalAction("that wager is not offered")
        );
        assert_eq!(game.stack, 100_000);
        assert_eq!(game.phase, Phase::Betting);
    }

    #[test]
    fn settlement_pays_a_win_a_push_and_a_natural() {
        for (cards, status, expected) in [
            (
                [crate::cards::Rank::Ten, crate::cards::Rank::Eight],
                BlackjackHandStatus::Stand,
                12_500,
            ),
            (
                [crate::cards::Rank::Ten, crate::cards::Rank::Seven],
                BlackjackHandStatus::Stand,
                10_000,
            ),
            (
                [crate::cards::Rank::Ace, crate::cards::Rank::King],
                BlackjackHandStatus::Blackjack,
                13_750,
            ),
        ] {
            let mut game = game(7_500);
            game.bet = Some(2_500);
            game.hands = vec![BlackjackHand {
                cards: cards.into_iter().map(card).collect(),
                bet: 2_500,
                status,
                split: false,
                split_aces: false,
                doubled: false,
            }];
            game.dealer = vec![
                card(crate::cards::Rank::Ten),
                card(crate::cards::Rank::Seven),
            ];
            game.phase = Phase::Playing;
            let settlement = game.settle().unwrap().expect("settlement");
            assert_eq!(game.stack, expected);
            assert_eq!(settlement.net, expected - 10_000);
            assert_eq!(game.phase, Phase::Settled);
            assert!(game.bet.is_none());
            assert!(game.last_result.is_some());
        }
    }

    #[test]
    fn a_settled_round_stays_on_the_felt_until_the_next_bet() {
        let mut game = game(7_500);
        game.bet = Some(2_500);
        game.hands = vec![BlackjackHand {
            cards: vec![
                card(crate::cards::Rank::Ten),
                card(crate::cards::Rank::Eight),
            ],
            bet: 2_500,
            status: BlackjackHandStatus::Stand,
            split: false,
            split_aces: false,
            doubled: false,
        }];
        game.dealer = vec![
            card(crate::cards::Rank::Ten),
            card(crate::cards::Rank::Seven),
        ];
        game.phase = Phase::Playing;
        game.settle().unwrap();
        assert_eq!(game.view(0).hands.len(), 1);
        rig(
            &mut game,
            vec![
                card(crate::cards::Rank::Two),
                card(crate::cards::Rank::Three),
                card(crate::cards::Rank::Four),
                card(crate::cards::Rank::Five),
            ],
        );
        game.place_bet(2_500).unwrap();
        assert_eq!(game.hands.len(), 1);
        assert_eq!(game.hands[0].cards[0].rank, crate::cards::Rank::Two);
    }

    #[test]
    fn action_flags_require_stack_for_double_and_split() {
        let mut game = game(2_499);
        game.bet = Some(2_500);
        game.hands = vec![BlackjackHand {
            cards: vec![
                card(crate::cards::Rank::Eight),
                card(crate::cards::Rank::Eight),
            ],
            bet: 2_500,
            status: BlackjackHandStatus::Playing,
            split: false,
            split_aces: false,
            doubled: false,
        }];
        game.phase = Phase::Playing;
        game.current = Some(0);
        let flags = game.action_flags();
        assert!(flags.hit && flags.stand);
        assert!(!flags.double);
        assert!(!flags.split);
    }

    #[test]
    fn an_ace_up_offers_insurance_and_a_dealer_natural_pays_it() {
        let mut game = game(100_000);
        rig(
            &mut game,
            vec![
                card(crate::cards::Rank::Ten),
                card(crate::cards::Rank::Eight),
                card(crate::cards::Rank::Ace),
                card(crate::cards::Rank::King),
            ],
        );
        game.place_bet(2_500).unwrap();
        assert_eq!(game.phase, Phase::Insurance);
        game.insure().unwrap();
        // Insurance pays 2:1 (the stake plus twice it) and the hand is lost.
        assert_eq!(game.insurance, 1_250);
        assert_eq!(game.stack, 97_500 - 1_250 + 3_750);
        assert_eq!(game.hands[0].status, BlackjackHandStatus::Loss);
        assert_eq!(game.phase, Phase::Settled);
    }

    #[tokio::test]
    async fn sitting_buys_in_and_leaving_cashes_out_the_stack() {
        let root = std::env::temp_dir().join(format!("blackjack-solo-{}", Uuid::new_v4()));
        let bank = crate::bank::BankStore::load(&root).await.unwrap();
        let store = BlackjackStore::load(&root).await.unwrap();
        let user = Uuid::new_v4();
        bank.re_up(crate::bank::AccountOwner::User(user))
            .await
            .unwrap();
        let before = bank
            .account(crate::bank::AccountOwner::User(user))
            .await
            .unwrap()
            .balance;
        store
            .sit(user, 10_000, BlackjackTrainerSettings::default(), &bank)
            .await
            .unwrap();
        assert_eq!(
            bank.account(crate::bank::AccountOwner::User(user))
                .await
                .unwrap()
                .balance,
            before - 100_000
        );
        assert_eq!(store.view(Some(user), 0).await.stack, 100_000);
        assert_eq!(
            store
                .sit(user, 10_000, BlackjackTrainerSettings::default(), &bank)
                .await,
            Err(BlackjackError::ActiveGame)
        );
        store.leave(user, &bank).await.unwrap();
        assert_eq!(
            bank.account(crate::bank::AccountOwner::User(user))
                .await
                .unwrap()
                .balance,
            before
        );
        assert!(!store.view(Some(user), 0).await.seated);
        tokio::fs::remove_dir_all(&root).await.ok();
    }

    #[tokio::test]
    async fn leaving_mid_hand_is_refused_rather_than_deferred() {
        let root = std::env::temp_dir().join(format!("blackjack-midhand-{}", Uuid::new_v4()));
        let bank = crate::bank::BankStore::load(&root).await.unwrap();
        let mut game = game(7_500);
        game.bet = Some(2_500);
        let user = game.user;
        let store = BlackjackStore::from_games(vec![game]);
        assert_eq!(
            store.leave(user, &bank).await,
            Err(BlackjackError::IllegalAction(
                "finish the hand you are playing first"
            ))
        );
        assert!(store.view(Some(user), 0).await.seated);
        tokio::fs::remove_dir_all(&root).await.ok();
    }

    /// The shared-table era left seats with chips in front of them; those chips
    /// are the same money after the move, live bets included (SPEC §V1).
    #[tokio::test]
    async fn shared_table_seats_become_solo_games_without_losing_a_cent() {
        let root = std::env::temp_dir().join(format!("blackjack-migrate-{}", Uuid::new_v4()));
        let dir = root.join("blackjack");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let table = Uuid::new_v4();
        let user = Uuid::new_v4();
        tokio::fs::write(
            dir.join("tables.json"),
            serde_json::to_vec(&serde_json::json!([{
                "id": table,
                "max_bet": 10_000,
                "seats": [
                    null,
                    {
                        "user": user,
                        "stack": 7_500,
                        "insurance": 1_250,
                        "hands": [{"bet": 2_500}],
                        "settings": {"counting_tutor": true},
                    },
                ],
            }]))
            .unwrap(),
        )
        .await
        .unwrap();
        let store = BlackjackStore::load(&root).await.unwrap();
        let view = store.view(Some(user), 0).await;
        assert!(view.seated);
        assert_eq!(view.stack, 7_500 + 2_500 + 1_250);
        assert_eq!(view.max_bet, 10_000);
        assert!(view.settings.counting_tutor);
        // The seat keeps its table's id so the cash-out pairs with the buy-in.
        assert_eq!(view.id, Some(table));
        assert!(
            !tokio::fs::try_exists(dir.join("tables.json"))
                .await
                .unwrap()
        );
        // A second load finds the games already carried over, not the seats.
        let store = BlackjackStore::load(&root).await.unwrap();
        assert_eq!(store.view(Some(user), 0).await.stack, 11_250);
        tokio::fs::remove_dir_all(&root).await.ok();
    }
}
