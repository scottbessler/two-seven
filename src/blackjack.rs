//! Blackjack, played alone against the house.
//!
//! One game per player: your own shoe, your own dealer, and nobody to wait for.
//! That is the whole shape of this module, and most of what it buys is what is
//! *absent* — there are no seats to index, no round the table, no betting clock
//! and no turn clock, because a clock only exists to stop one player holding up
//! another. Every action is answered by the request that made it.
//!
//! There is no buy-in and no table stack: the chips are the bank account, and
//! a player sits down for nothing. All the slider picks is a ceiling, and all
//! the ceiling does is name the five wagers on offer — a fifth of it through
//! all of it. A wager leaves the bank as one `BlackjackBet` when it is staked
//! and comes back as one `BlackjackPayout` when the round settles, so the only
//! money this module holds is what is on the felt right now, which is what
//! keeps the chip conservation in SPEC §V1 to one number per player.

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

/// The smallest ceiling on offer, and the step every larger one lands on.
/// Every wager is a fifth of the ceiling, so a ceiling that is a whole number
/// of $5 is the only kind that prices five whole-dollar wagers.
pub const MIN_MAX_BET: Cents = 10_000;
pub const MAX_BET_STEP: Cents = 500;

/// The most one wager may be: everything the bank holds, rounded down to a
/// ceiling that divides, and never past the house limit on a single stake
/// (SPEC §V10).
pub fn max_bet_ceiling(balance: Cents) -> Cents {
    balance.clamp(0, crate::money::MAX_GAME_ENTRY) / MAX_BET_STEP * MAX_BET_STEP
}

/// The rungs the max-bet slider stops on: a 1-2-5 ladder from $100, and the
/// bank itself at the top. A slider that ran continuously would offer a
/// $1,337 ceiling; the ladder is the numbers a table would actually print,
/// and the last stop is there because the whole bank is a number the player
/// is entitled to name even when it is not a round one.
pub fn max_bet_rungs(balance: Cents) -> Vec<Cents> {
    let ceiling = max_bet_ceiling(balance);
    if ceiling < MIN_MAX_BET {
        return Vec::new();
    }
    let mut rungs = Vec::new();
    let mut decade = MIN_MAX_BET;
    'ladder: loop {
        for multiple in [1, 2, 5] {
            let rung = decade * multiple;
            if rung > ceiling {
                break 'ladder;
            }
            rungs.push(rung);
        }
        decade *= 10;
    }
    if rungs.last() != Some(&ceiling) {
        rungs.push(ceiling);
    }
    rungs
}

/// True for a ceiling this balance may sit down behind.
pub fn valid_max_bet(max_bet: Cents, balance: Cents) -> bool {
    max_bet >= MIN_MAX_BET && max_bet <= max_bet_ceiling(balance) && max_bet % MAX_BET_STEP == 0
}

/// The only wagers a game offers, in fifth steps of its ceiling.
pub fn bet_options(max_bet: Cents) -> [Cents; 5] {
    [
        max_bet / 5,
        max_bet * 2 / 5,
        max_bet * 3 / 5,
        max_bet * 4 / 5,
        max_bet,
    ]
}

/// The smallest wager a ceiling offers, which is also what a player needs in
/// the bank to be dealt another hand.
pub fn min_bet_for(max_bet: Cents) -> Cents {
    max_bet / 5
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
#[derive(Clone, Copy, Debug)]
pub struct BlackjackSettlement {
    pub user: Uuid,
    pub net: Cents,
    pub outcome: crate::blackjack_stats::RoundOutcome,
}

/// What one move owes the bank, so the store can move the money the game
/// only decided on. Everything the felt takes is a debit at the moment it is
/// staked; everything it gives back rides on the settlement, which already
/// counts it.
#[derive(Clone, Copy, Debug, Default)]
pub struct BlackjackMove {
    pub staked: Cents,
    pub settlement: Option<BlackjackSettlement>,
}

impl BlackjackMove {
    pub fn returned(&self) -> Cents {
        self.settlement
            .map(|settlement| settlement.outcome.returned)
            .unwrap_or_default()
    }
}

/// One player's whole blackjack world: their shoe, their dealer, their felt.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlackjackGame {
    pub id: Uuid,
    pub user: Uuid,
    pub max_bet: Cents,
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
    /// Every rung the slider stops on for this balance, the whole bank last.
    pub max_bets: Vec<Cents>,
    pub max_bet: Cents,
    pub bet_options: Vec<Cents>,
    pub min_bet: Cents,
    pub phase: Phase,
    /// What is on the felt right now: every hand's stake plus insurance.
    pub staked: Cents,
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
    pub fn new(id: Uuid, user: Uuid, max_bet: Cents) -> Self {
        Self {
            id,
            user,
            max_bet,
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

    /// True while chips are committed to the felt — the one time leaving has
    /// to wait.
    pub fn in_round(&self) -> bool {
        self.bet.is_some()
    }

    /// Everything the felt is holding: each hand's stake plus insurance. It is
    /// bank money that has already left the account, so a round that cannot be
    /// resumed has to hand exactly this back (SPEC §V1).
    ///
    /// A settled round is still on the felt to be looked at, but its stakes
    /// were paid out the moment it settled — counting those hands again is how
    /// a restart would mint money — so only a live round holds anything.
    pub fn staked(&self) -> Cents {
        if !self.in_round() {
            return 0;
        }
        self.hands.iter().map(|hand| hand.bet).sum::<Cents>() + self.insurance
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

    /// `balance` is what the bank holds, which is already net of anything on
    /// the felt: a stake is withdrawn the moment it is made.
    pub fn place_bet(
        &mut self,
        amount: Cents,
        balance: Cents,
    ) -> Result<BlackjackMove, BlackjackError> {
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
        if balance < amount {
            return Err(BlackjackError::IllegalAction("not enough in the bank"));
        }
        self.bet = Some(amount);
        let settlement = self.deal(balance - amount)?;
        Ok(BlackjackMove {
            staked: amount,
            settlement,
        })
    }

    fn deal(&mut self, balance: Cents) -> Result<Option<BlackjackSettlement>, BlackjackError> {
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
        if self.dealer[0].rank as u8 == 14 && balance >= bet / 2 {
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

    pub fn insure(&mut self, balance: Cents) -> Result<BlackjackMove, BlackjackError> {
        if self.phase != Phase::Insurance {
            return Err(BlackjackError::IllegalAction("insurance is not available"));
        }
        let amount = self.bet.unwrap_or_default() / 2;
        if self.insurance_decided || balance < amount {
            return Err(BlackjackError::IllegalAction("insurance is not available"));
        }
        self.insurance = amount;
        self.insurance_decided = true;
        let settlement = self.peek()?;
        Ok(BlackjackMove {
            staked: amount,
            settlement,
        })
    }

    pub fn decline(&mut self, _balance: Cents) -> Result<BlackjackMove, BlackjackError> {
        if self.phase != Phase::Insurance {
            return Err(BlackjackError::IllegalAction("insurance is not available"));
        }
        if self.insurance_decided {
            return Err(BlackjackError::IllegalAction(
                "insurance is already decided",
            ));
        }
        self.insurance_decided = true;
        Ok(BlackjackMove {
            staked: 0,
            settlement: self.peek()?,
        })
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

    pub fn act(&mut self, action: Action, balance: Cents) -> Result<BlackjackMove, BlackjackError> {
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
        let mut staked = 0;
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
                if hand.cards.len() != 2 || hand.split_aces || balance < bet {
                    return Err(BlackjackError::IllegalAction("that action is not legal"));
                }
                let card = self.draw();
                staked = bet;
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
                    && balance >= hand.bet;
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
                staked = bet;
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
        Ok(BlackjackMove {
            staked,
            settlement: self.advance_current()?,
        })
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

    pub fn action_flags(&self, balance: Cents) -> ActionFlags {
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
            double: hand.cards.len() == 2 && !hand.split_aces && balance >= hand.bet,
            split: hand.cards.len() == 2
                && hand.cards[0].rank == hand.cards[1].rank
                && self.hands.len() < MAX_HANDS
                && balance >= hand.bet,
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
        let flags = self.action_flags(bank_balance);
        let can_insure = self.phase == Phase::Insurance
            && !self.insurance_decided
            && bank_balance >= self.bet.unwrap_or_default() / 2;
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
            max_bets: max_bet_rungs(bank_balance),
            max_bet: self.max_bet,
            bet_options: bet_options(self.max_bet).to_vec(),
            min_bet: min_bet_for(self.max_bet),
            phase: self.phase,
            staked: self.staked(),
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
            can_bet: matches!(self.phase, Phase::Betting | Phase::Settled)
                && self.bet.is_none()
                && bank_balance >= min_bet_for(self.max_bet),
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
    let rungs = max_bet_rungs(bank_balance);
    let max_bet = rungs.first().copied().unwrap_or(MIN_MAX_BET);
    BlackjackView {
        seated: false,
        id: None,
        bank_balance,
        max_bets: rungs.clone(),
        max_bet,
        bet_options: bet_options(max_bet).to_vec(),
        min_bet: min_bet_for(max_bet),
        phase: Phase::Betting,
        staked: 0,
        bet: None,
        insurance: 0,
        hands: Vec::new(),
        current_hand: None,
        dealer: Vec::new(),
        dealer_hidden: false,
        dealer_score: None,
        result: None,
        can_sit: !rungs.is_empty(),
        can_leave: false,
        can_bet: false,
        can_insure: false,
        can_decline: false,
        can_hit: false,
        can_stand: false,
        can_double: false,
        can_split: false,
        message: if rungs.is_empty() {
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

/// What the bank says a player has right now. A brand new account reads as
/// nothing rather than an error: there is no game to refuse yet.
async fn balance_of(bank: &crate::bank::BankStore, user: Uuid) -> Cents {
    bank.account(crate::bank::AccountOwner::User(user))
        .await
        .map(|account| account.balance)
        .unwrap_or_default()
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

    /// Reads the games back and hands the bank everything the felt was still
    /// holding.
    ///
    /// Two kinds of money come home here. A hand can only have been
    /// interrupted by a restart, so it never resumes: its stakes were
    /// withdrawn when they were made and are returned now. And a game saved
    /// by the buy-in era carries a `stack` this module no longer has a place
    /// for — that is bank money too, so it goes back the same way. Both are
    /// paid before the file is rewritten without them, which is what keeps
    /// §V1 true across the restart that does it.
    pub async fn load(
        root: impl AsRef<Path>,
        bank: &crate::bank::BankStore,
    ) -> Result<Self, anyhow::Error> {
        let dir = root.as_ref().join("blackjack");
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join("solo.json");
        let saved = match tokio::fs::read(&path).await {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let mut games: HashMap<Uuid, BlackjackGame> = match &saved {
            Some(bytes) => serde_json::from_slice::<Vec<BlackjackGame>>(bytes)?
                .into_iter()
                .map(|game| (game.user, game))
                .collect(),
            None => HashMap::new(),
        };
        let mut owed: Vec<Carryover> = match &saved {
            Some(bytes) => carried_stacks(bytes)?,
            None => Vec::new(),
        };
        let migrated = migrate_shared_tables(&dir, &mut games, &mut owed).await?;
        for game in games.values_mut() {
            if game.phase != Phase::Betting {
                owed.push(Carryover {
                    user: game.user,
                    game: game.id,
                    amount: game.staked(),
                });
                game.bet = None;
                game.clear_round();
            }
        }
        let store = Self {
            games: Arc::new(Mutex::new(games)),
            path: Some(path),
        };
        let mut returned = 0usize;
        for carryover in owed.into_iter().filter(|owed| owed.amount > 0) {
            bank.blackjack_cash_out(
                crate::bank::AccountOwner::User(carryover.user),
                carryover.game,
                carryover.amount,
            )
            .await?;
            returned += 1;
        }
        if migrated || returned > 0 {
            tracing::info!(returned, "returned interrupted blackjack chips to the bank");
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

    /// Sitting down costs nothing: it picks the ceiling the five wagers are
    /// cut from and deals a shoe. The bank is read to bound the ceiling and is
    /// not touched until the first wager.
    pub async fn sit(
        &self,
        user: Uuid,
        max_bet: Cents,
        settings: BlackjackTrainerSettings,
        bank: &crate::bank::BankStore,
    ) -> Result<(), BlackjackError> {
        if !valid_max_bet(max_bet, balance_of(bank, user).await) {
            return Err(BlackjackError::IllegalAction(
                "that maximum bet is not offered",
            ));
        }
        {
            let mut games = self.games.lock().await;
            if games.contains_key(&user) {
                return Err(BlackjackError::ActiveGame);
            }
            let mut game = BlackjackGame::new(Uuid::new_v4(), user, max_bet);
            game.settings = settings.sanitized();
            games.insert(user, game);
        }
        self.persist()
            .await
            .map_err(|_| BlackjackError::IllegalAction("could not persist the game"))
    }

    /// Getting up leaves the money where it already is. All that goes is the
    /// shoe, its running count and the ceiling.
    pub async fn leave(&self, user: Uuid) -> Result<(), BlackjackError> {
        {
            let mut games = self.games.lock().await;
            let game = games.get(&user).ok_or(BlackjackError::NotFound)?;
            if game.in_round() {
                return Err(BlackjackError::IllegalAction(
                    "finish the hand you are playing first",
                ));
            }
            games.remove(&user);
        }
        self.persist()
            .await
            .map_err(|_| BlackjackError::IllegalAction("could not persist the game"))
    }

    /// Runs one move against a draft of the game and only keeps it once the
    /// bank has agreed to the money it costs. A debit the bank refuses — the
    /// balance moved between the read and the stake — leaves the felt exactly
    /// as it was.
    async fn resolve(
        &self,
        user: Uuid,
        action: impl FnOnce(&mut BlackjackGame, Cents) -> Result<BlackjackMove, BlackjackError>,
        bank: &crate::bank::BankStore,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        let owner = crate::bank::AccountOwner::User(user);
        let balance = balance_of(bank, user).await;
        let settlement = {
            let mut games = self.games.lock().await;
            let game = games.get_mut(&user).ok_or(BlackjackError::NotFound)?;
            let mut draft = game.clone();
            let moved = action(&mut draft, balance)?;
            if moved.staked > 0 {
                bank.blackjack_bet(owner.clone(), draft.id, moved.staked)
                    .await
                    .map_err(|_| BlackjackError::IllegalAction("not enough in the bank"))?;
            }
            let returned = moved.returned();
            if returned > 0 {
                bank.blackjack_payout(owner, draft.id, returned)
                    .await
                    .map_err(|_| BlackjackError::IllegalAction("the payout could not be paid"))?;
            }
            draft.updated_at = Utc::now();
            *game = draft;
            moved.settlement
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
        bank: &crate::bank::BankStore,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        self.resolve(
            user,
            |game, balance| game.place_bet(amount, balance),
            bank,
            stats,
        )
        .await
    }

    pub async fn insure(
        &self,
        user: Uuid,
        bank: &crate::bank::BankStore,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        self.resolve(user, BlackjackGame::insure, bank, stats).await
    }

    pub async fn decline(
        &self,
        user: Uuid,
        bank: &crate::bank::BankStore,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        self.resolve(user, BlackjackGame::decline, bank, stats)
            .await
    }

    pub async fn act(
        &self,
        user: Uuid,
        action: Action,
        bank: &crate::bank::BankStore,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        self.resolve(user, |game, balance| game.act(action, balance), bank, stats)
            .await
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

/// Money a saved game is holding that this module no longer has a place for,
/// on its way back to the bank it came out of.
struct Carryover {
    user: Uuid,
    game: Uuid,
    amount: Cents,
}

/// The buy-in era, read out of the saved games so its stacks can be handed
/// back. Nothing else in the file has changed shape, so the games themselves
/// parse as they are and only the vanished `stack` is dug out here.
fn carried_stacks(bytes: &[u8]) -> Result<Vec<Carryover>, anyhow::Error> {
    #[derive(Deserialize)]
    struct SavedStack {
        id: Uuid,
        user: Uuid,
        #[serde(default)]
        stack: Cents,
    }
    Ok(serde_json::from_slice::<Vec<SavedStack>>(bytes)?
        .into_iter()
        .map(|saved| Carryover {
            user: saved.user,
            game: saved.id,
            amount: saved.stack,
        })
        .collect())
}

/// The shared-table era, read only so its chips can be handed back.
///
/// Seats were bought in against the *table's* id, so each imported game keeps
/// that id: the cash-out that hands the chips back lands in the same player's
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
    owed: &mut Vec<Carryover>,
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
        let clamped = table
            .max_bet
            .clamp(MIN_MAX_BET, crate::money::MAX_GAME_ENTRY);
        let max_bet = (clamped + MAX_BET_STEP - 1) / MAX_BET_STEP * MAX_BET_STEP;
        for seat in table.seats.into_iter().flatten() {
            if games.contains_key(&seat.user) {
                continue;
            }
            let live = seat.hands.iter().map(|hand| hand.bet).sum::<Cents>() + seat.insurance;
            owed.push(Carryover {
                user: seat.user,
                game: table.id,
                amount: seat.stack + live,
            });
            let mut game = BlackjackGame::new(table.id, seat.user, max_bet);
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

    fn game() -> BlackjackGame {
        BlackjackGame::new(Uuid::new_v4(), Uuid::new_v4(), 10_000)
    }

    /// Stacks the shoe and moves the cut card out of reach, so a rigged deal
    /// is not reshuffled out from under the test.
    fn rig(game: &mut BlackjackGame, cards: Vec<Card>) {
        game.shoe.deck = Deck::from_cards(cards);
        game.shoe.cut_card = usize::MAX;
    }

    #[test]
    fn the_slider_offers_the_bank_and_prices_five_wagers() {
        assert_eq!(bet_options(10_000), [2_000, 4_000, 6_000, 8_000, 10_000]);
        // Nothing under the cheapest ceiling is a seat at all.
        assert_eq!(max_bet_rungs(0), Vec::<Cents>::new());
        assert_eq!(max_bet_rungs(9_999), Vec::<Cents>::new());
        assert_eq!(max_bet_rungs(10_000), vec![10_000]);
        // The ladder runs 1-2-5 and the bank itself is always the last stop.
        assert_eq!(
            max_bet_rungs(1_000_000),
            vec![10_000, 20_000, 50_000, 100_000, 200_000, 500_000, 1_000_000]
        );
        assert_eq!(
            max_bet_rungs(17_172_980),
            vec![
                10_000, 20_000, 50_000, 100_000, 200_000, 500_000, 1_000_000, 2_000_000, 5_000_000,
                10_000_000, 17_172_500,
            ]
        );
        // A ceiling never runs past the house limit on a single stake (§V10).
        assert_eq!(
            max_bet_rungs(crate::money::MAX_GAME_ENTRY * 3).last(),
            Some(&crate::money::MAX_GAME_ENTRY)
        );
        // Every rung the slider stops on pays five whole-dollar wagers.
        for balance in [10_000, 999_999, 17_172_980, 4_000_000_000] {
            for rung in max_bet_rungs(balance) {
                assert!(valid_max_bet(rung, balance), "{rung} is off the ladder");
                for option in bet_options(rung) {
                    assert_eq!(option % 100, 0, "{rung} pays a fraction of a dollar");
                }
            }
        }
        // A ceiling the bank cannot cover, or one that does not divide, is not
        // a seat this player may take.
        assert!(!valid_max_bet(20_000, 19_999));
        assert!(!valid_max_bet(10_100, 1_000_000));
        assert!(!valid_max_bet(9_500, 1_000_000));
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
        let mut game = game();
        rig(
            &mut game,
            vec![
                card(crate::cards::Rank::Ten),
                card(crate::cards::Rank::Eight),
                card(crate::cards::Rank::Nine),
                card(crate::cards::Rank::Seven),
            ],
        );
        let moved = game.place_bet(2_000, 100_000).unwrap();
        assert!(moved.settlement.is_none());
        assert_eq!(moved.staked, 2_000);
        assert_eq!(game.phase, Phase::Playing);
        assert_eq!(game.current, Some(0));
        assert_eq!(game.staked(), 2_000);
        assert_eq!(game.hands[0].cards.len(), 2);
        // The hole card stays face down while the hand is live.
        let view = game.view(0);
        assert_eq!(view.dealer.len(), 1);
        assert!(view.dealer_hidden);
    }

    #[test]
    fn an_unoffered_wager_is_refused_and_nothing_is_staked() {
        let mut game = game();
        assert_eq!(
            game.place_bet(99, 100_000).unwrap_err(),
            BlackjackError::IllegalAction("that wager is not offered")
        );
        // A quarter of the ceiling was a wager once; five steps do not offer it.
        assert_eq!(
            game.place_bet(2_500, 100_000).unwrap_err(),
            BlackjackError::IllegalAction("that wager is not offered")
        );
        assert_eq!(game.staked(), 0);
        assert_eq!(game.phase, Phase::Betting);
    }

    #[test]
    fn a_wager_the_bank_cannot_cover_is_refused() {
        let mut game = game();
        assert_eq!(
            game.place_bet(10_000, 9_999).unwrap_err(),
            BlackjackError::IllegalAction("not enough in the bank")
        );
        assert_eq!(game.staked(), 0);
        // The ceiling is out of reach but the smallest wager is not, so the
        // round is still on — with the buttons it cannot cover disabled.
        assert!(game.view(9_999).can_bet);
        assert!(game.view(2_000).can_bet);
        // Under the smallest wager there is no round to be had.
        assert!(!game.view(1_999).can_bet);
    }

    #[test]
    fn settlement_pays_a_win_a_push_and_a_natural() {
        for (cards, status, expected) in [
            (
                [crate::cards::Rank::Ten, crate::cards::Rank::Eight],
                BlackjackHandStatus::Stand,
                4_000,
            ),
            (
                [crate::cards::Rank::Ten, crate::cards::Rank::Seven],
                BlackjackHandStatus::Stand,
                2_000,
            ),
            (
                [crate::cards::Rank::Ace, crate::cards::Rank::King],
                BlackjackHandStatus::Blackjack,
                5_000,
            ),
        ] {
            let mut game = game();
            game.bet = Some(2_000);
            game.hands = vec![BlackjackHand {
                cards: cards.into_iter().map(card).collect(),
                bet: 2_000,
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
            assert_eq!(settlement.outcome.returned, expected);
            assert_eq!(settlement.net, expected - 2_000);
            assert_eq!(game.phase, Phase::Settled);
            assert!(game.bet.is_none());
            assert!(game.last_result.is_some());
        }
    }

    #[test]
    fn a_settled_round_stays_on_the_felt_until_the_next_bet() {
        let mut game = game();
        game.bet = Some(2_000);
        game.hands = vec![BlackjackHand {
            cards: vec![
                card(crate::cards::Rank::Ten),
                card(crate::cards::Rank::Eight),
            ],
            bet: 2_000,
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
        game.place_bet(2_000, 100_000).unwrap();
        assert_eq!(game.hands.len(), 1);
        assert_eq!(game.hands[0].cards[0].rank, crate::cards::Rank::Two);
    }

    #[test]
    fn action_flags_require_a_bank_for_double_and_split() {
        let mut game = game();
        game.bet = Some(2_000);
        game.hands = vec![BlackjackHand {
            cards: vec![
                card(crate::cards::Rank::Eight),
                card(crate::cards::Rank::Eight),
            ],
            bet: 2_000,
            status: BlackjackHandStatus::Playing,
            split: false,
            split_aces: false,
            doubled: false,
        }];
        game.phase = Phase::Playing;
        game.current = Some(0);
        let flags = game.action_flags(1_999);
        assert!(flags.hit && flags.stand);
        assert!(!flags.double);
        assert!(!flags.split);
        // One more cent in the bank and both are back on.
        let flags = game.action_flags(2_000);
        assert!(flags.double && flags.split);
    }

    #[test]
    fn an_ace_up_offers_insurance_and_a_dealer_natural_pays_it() {
        let mut game = game();
        rig(
            &mut game,
            vec![
                card(crate::cards::Rank::Ten),
                card(crate::cards::Rank::Eight),
                card(crate::cards::Rank::Ace),
                card(crate::cards::Rank::King),
            ],
        );
        game.place_bet(2_000, 100_000).unwrap();
        assert_eq!(game.phase, Phase::Insurance);
        let moved = game.insure(98_000).unwrap();
        // Insurance pays 2:1 (the stake plus twice it) and the hand is lost.
        assert_eq!(game.insurance, 1_000);
        assert_eq!(moved.staked, 1_000);
        assert_eq!(moved.returned(), 3_000);
        assert_eq!(game.hands[0].status, BlackjackHandStatus::Loss);
        assert_eq!(game.phase, Phase::Settled);
    }

    /// Sitting down and getting up are free; only the wagers between them
    /// move money, and each one is a pair of ledger rows against the game.
    #[tokio::test]
    async fn a_seat_is_free_and_a_round_moves_the_bank_both_ways() {
        let root = std::env::temp_dir().join(format!("blackjack-solo-{}", Uuid::new_v4()));
        let bank = crate::bank::BankStore::load(&root).await.unwrap();
        let stats = crate::blackjack_stats::BlackjackStatsStore::new();
        let store = BlackjackStore::load(&root, &bank).await.unwrap();
        let owner = crate::bank::AccountOwner::User(Uuid::new_v4());
        let crate::bank::AccountOwner::User(user) = owner else {
            unreachable!()
        };
        bank.re_up(owner.clone()).await.unwrap();
        let before = bank.account(owner.clone()).await.unwrap().balance;
        // $1,000 in the bank reaches the $1,000 ceiling and no further.
        assert_eq!(max_bet_ceiling(before), before);
        assert_eq!(
            store
                .sit(
                    user,
                    before + MAX_BET_STEP,
                    BlackjackTrainerSettings::default(),
                    &bank
                )
                .await,
            Err(BlackjackError::IllegalAction(
                "that maximum bet is not offered"
            ))
        );
        store
            .sit(user, 10_000, BlackjackTrainerSettings::default(), &bank)
            .await
            .unwrap();
        assert_eq!(bank.account(owner.clone()).await.unwrap().balance, before);
        let view = store.view(Some(user), before).await;
        assert!(view.seated);
        assert_eq!(view.bet_options, vec![2_000, 4_000, 6_000, 8_000, 10_000]);
        assert_eq!(
            store
                .sit(user, 10_000, BlackjackTrainerSettings::default(), &bank)
                .await,
            Err(BlackjackError::ActiveGame)
        );

        store.bet(user, 2_000, &bank, &stats).await.unwrap();
        let account = bank.account(owner.clone()).await.unwrap();
        let staked: Cents = account
            .entries
            .iter()
            .filter(|entry| matches!(entry.kind, crate::bank::LedgerKind::BlackjackBet { .. }))
            .map(|entry| -entry.delta)
            .sum();
        assert_eq!(staked, 2_000);
        let returned: Cents = account
            .entries
            .iter()
            .filter(|entry| matches!(entry.kind, crate::bank::LedgerKind::BlackjackPayout { .. }))
            .map(|entry| entry.delta)
            .sum();
        // Whatever the shoe did — a natural can settle in the same request —
        // the bank plus the felt is the balance less what is still staked on
        // it, and the two ledger rows are the whole of the movement (§V1).
        let game_staked = store.games.lock().await[&user].staked();
        assert_eq!(account.balance, before - staked + returned);
        assert_eq!(game_staked, if returned > 0 { 0 } else { 2_000 });

        // Getting up mid-round waits; once the felt is clear it is free.
        if store.games.lock().await[&user].in_round() {
            assert_eq!(
                store.leave(user).await,
                Err(BlackjackError::IllegalAction(
                    "finish the hand you are playing first"
                ))
            );
        } else {
            let balance = bank.account(owner.clone()).await.unwrap().balance;
            store.leave(user).await.unwrap();
            assert_eq!(bank.account(owner).await.unwrap().balance, balance);
            assert!(!store.view(Some(user), balance).await.seated);
        }
        tokio::fs::remove_dir_all(&root).await.ok();
    }

    #[tokio::test]
    async fn leaving_mid_hand_is_refused_rather_than_deferred() {
        let mut game = game();
        game.bet = Some(2_000);
        let user = game.user;
        let store = BlackjackStore::from_games(vec![game]);
        assert_eq!(
            store.leave(user).await,
            Err(BlackjackError::IllegalAction(
                "finish the hand you are playing first"
            ))
        );
        assert!(store.view(Some(user), 0).await.seated);
    }

    /// A restart cannot resume a hand, and the chips on it left the bank when
    /// they were staked. They are handed back on the way in, so the money is
    /// where it was before the round (SPEC §V1).
    #[tokio::test]
    async fn an_interrupted_round_hands_its_chips_back_to_the_bank() {
        let root = std::env::temp_dir().join(format!("blackjack-restart-{}", Uuid::new_v4()));
        let bank = crate::bank::BankStore::load(&root).await.unwrap();
        let stats = crate::blackjack_stats::BlackjackStatsStore::new();
        let store = BlackjackStore::load(&root, &bank).await.unwrap();
        let user = Uuid::new_v4();
        let owner = crate::bank::AccountOwner::User(user);
        bank.re_up(owner.clone()).await.unwrap();
        let before = bank.account(owner.clone()).await.unwrap().balance;
        store
            .sit(user, 10_000, BlackjackTrainerSettings::default(), &bank)
            .await
            .unwrap();
        // Rig the shoe so the round cannot settle in the same request.
        {
            let mut games = store.games.lock().await;
            let game = games.get_mut(&user).expect("game");
            rig(
                game,
                vec![
                    card(crate::cards::Rank::Ten),
                    card(crate::cards::Rank::Eight),
                    card(crate::cards::Rank::Nine),
                    card(crate::cards::Rank::Seven),
                ],
            );
        }
        store.bet(user, 2_000, &bank, &stats).await.unwrap();
        assert_eq!(
            bank.account(owner.clone()).await.unwrap().balance,
            before - 2_000
        );

        let store = BlackjackStore::load(&root, &bank).await.unwrap();
        assert_eq!(bank.account(owner).await.unwrap().balance, before);
        let view = store.view(Some(user), before).await;
        assert!(view.seated);
        assert_eq!(view.phase, Phase::Betting);
        assert_eq!(view.staked, 0);
        tokio::fs::remove_dir_all(&root).await.ok();
    }

    /// A settled round is paid out before the file is written, so the restart
    /// that clears it off the felt must not pay for it twice (SPEC §V1).
    #[tokio::test]
    async fn a_settled_round_is_not_paid_for_twice_by_a_restart() {
        let root = std::env::temp_dir().join(format!("blackjack-settled-{}", Uuid::new_v4()));
        let dir = root.join("blackjack");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let bank = crate::bank::BankStore::load(&root).await.unwrap();
        let user = Uuid::new_v4();
        let owner = crate::bank::AccountOwner::User(user);
        let mut settled = game();
        settled.user = user;
        settled.phase = Phase::Settled;
        settled.bet = None;
        settled.hands = vec![BlackjackHand {
            cards: vec![
                card(crate::cards::Rank::Ten),
                card(crate::cards::Rank::Eight),
            ],
            bet: 2_000,
            status: BlackjackHandStatus::Win,
            split: false,
            split_aces: false,
            doubled: false,
        }];
        assert_eq!(settled.staked(), 0);
        tokio::fs::write(
            dir.join("solo.json"),
            serde_json::to_vec(&[&settled]).unwrap(),
        )
        .await
        .unwrap();

        let store = BlackjackStore::load(&root, &bank).await.unwrap();
        assert_eq!(bank.account(owner).await.unwrap().balance, 0);
        let view = store.view(Some(user), 0).await;
        assert_eq!(view.phase, Phase::Betting);
        assert_eq!(view.staked, 0);
        tokio::fs::remove_dir_all(&root).await.ok();
    }

    /// The buy-in era parked a stack at the table. There is no stack any more,
    /// so the first load after the change hands it back rather than losing it.
    #[tokio::test]
    async fn a_saved_buy_in_stack_goes_back_to_the_bank() {
        let root = std::env::temp_dir().join(format!("blackjack-carry-{}", Uuid::new_v4()));
        let dir = root.join("blackjack");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let bank = crate::bank::BankStore::load(&root).await.unwrap();
        let user = Uuid::new_v4();
        let owner = crate::bank::AccountOwner::User(user);
        let mut saved = game();
        saved.user = user;
        let mut value = serde_json::to_value([&saved]).unwrap();
        value[0]["stack"] = serde_json::json!(97_500);
        tokio::fs::write(dir.join("solo.json"), serde_json::to_vec(&value).unwrap())
            .await
            .unwrap();

        let store = BlackjackStore::load(&root, &bank).await.unwrap();
        assert_eq!(bank.account(owner.clone()).await.unwrap().balance, 97_500);
        assert!(store.view(Some(user), 97_500).await.seated);
        // The file has been rewritten without the stack, so a second load is
        // not a second payday.
        let store = BlackjackStore::load(&root, &bank).await.unwrap();
        assert_eq!(bank.account(owner).await.unwrap().balance, 97_500);
        assert!(store.view(Some(user), 97_500).await.seated);
        tokio::fs::remove_dir_all(&root).await.ok();
    }

    /// The shared-table era left seats with chips in front of them; those chips
    /// are the same money after the move, live bets included — they go back to
    /// the bank the seat bought them out of (SPEC §V1).
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
        let bank = crate::bank::BankStore::load(&root).await.unwrap();
        let owner = crate::bank::AccountOwner::User(user);
        let store = BlackjackStore::load(&root, &bank).await.unwrap();
        // Stack and live bets alike are bank money now, all of it back.
        let carried = 7_500 + 2_500 + 1_250;
        assert_eq!(bank.account(owner.clone()).await.unwrap().balance, carried);
        let view = store.view(Some(user), carried).await;
        assert!(view.seated);
        assert_eq!(view.staked, 0);
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
        let store = BlackjackStore::load(&root, &bank).await.unwrap();
        assert_eq!(bank.account(owner).await.unwrap().balance, carried);
        assert!(store.view(Some(user), carried).await.seated);
        tokio::fs::remove_dir_all(&root).await.ok();
    }
}
