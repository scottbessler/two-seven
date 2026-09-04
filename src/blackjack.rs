use crate::{
    cards::{Card, Deck},
    money::Cents,
};
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, broadcast};
use uuid::Uuid;

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

#[derive(Clone)]
pub struct BlackjackStore {
    tables: Arc<Mutex<Vec<BlackjackTable>>>,
    tables_path: Option<PathBuf>,
    changed: broadcast::Sender<Uuid>,
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

const MAX_HANDS: usize = 4;
const SAFE_RESERVE_CARDS: usize = 20;

impl Default for BlackjackStore {
    fn default() -> Self {
        let (changed, _) = broadcast::channel(32);
        Self {
            tables: Arc::new(Mutex::new(
                (0..TIER_MAX_BETS.len()).map(BlackjackTable::new).collect(),
            )),
            tables_path: None,
            changed,
        }
    }
}

fn cut_card(total_cards: usize, penetration_percent: u8) -> usize {
    let maximum_cut = total_cards.saturating_sub(SAFE_RESERVE_CARDS);
    ((total_cards * usize::from(penetration_percent) + 50) / 100).clamp(4, maximum_cut.max(4))
}

impl BlackjackShoe {
    pub fn table_default() -> Self {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlackjackError {
    NotFound,
    Finished,
    ActiveGame,
    IllegalAction(&'static str),
}
pub fn score(cards: &[Card]) -> (u8, bool) {
    let mut total = 0;
    let mut aces = 0;
    for c in cards {
        total += match c.rank as u8 {
            14 => {
                aces += 1;
                11
            }
            11..=13 => 10,
            value => value,
        };
    }
    while total > 21 && aces > 0 {
        total -= 10;
        aces -= 1;
    }
    (total, aces > 0)
}
pub const TIER_MAX_BETS: [Cents; 4] = [10_000, 100_000, 1_000_000, 10_000_000];
pub const TABLE_IDS: [Uuid; 4] = [
    Uuid::from_u128(0x2a7b1a5e_0000_4b00_8000_000000000001),
    Uuid::from_u128(0x2a7b1a5e_0000_4b00_8000_000000000002),
    Uuid::from_u128(0x2a7b1a5e_0000_4b00_8000_000000000003),
    Uuid::from_u128(0x2a7b1a5e_0000_4b00_8000_000000000004),
];
pub const SEAT_COUNT: usize = 5;
pub const TURN_SECONDS: i64 = crate::table::TURN_SECONDS;
pub const RESULT_PAUSE_SECONDS: i64 = 5;

pub fn buy_in_for(max_bet: Cents) -> Cents {
    max_bet * 10
}

pub fn bet_options(max_bet: Cents) -> [Cents; 4] {
    [max_bet / 4, max_bet / 2, max_bet * 3 / 4, max_bet]
}

pub fn table_id(tier: usize) -> Uuid {
    TABLE_IDS[tier]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Betting,
    Insurance,
    Playing,
    Settled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlackjackSeat {
    pub user: Uuid,
    pub stack: Cents,
    pub bet: Option<Cents>,
    pub hands: Vec<BlackjackHand>,
    pub insurance: Cents,
    pub insurance_decided: bool,
    pub leaving: bool,
    pub settings: BlackjackTrainerSettings,
    pub decisions: Vec<BlackjackDecision>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlackjackTable {
    pub id: Uuid,
    pub tier: usize,
    pub max_bet: Cents,
    pub seats: Vec<Option<BlackjackSeat>>,
    pub shoe: BlackjackShoe,
    pub phase: Phase,
    pub dealer: Vec<Card>,
    pub dealer_peeked: bool,
    pub current: Option<(usize, usize)>,
    pub deadline: Option<DateTime<Utc>>,
    pub round_no: u64,
    pub last_results: Vec<(usize, Cents, String)>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlackjackTableView {
    pub id: Uuid,
    pub tier: usize,
    pub max_bet: Cents,
    pub buy_in: Cents,
    pub bet_options: [Cents; 4],
    pub min_bet: Cents,
    pub seat_count: usize,
    pub phase: Phase,
    pub dealer: Vec<Card>,
    pub dealer_hidden: bool,
    pub dealer_score: Option<u8>,
    pub current_seat: Option<usize>,
    pub current_hand: Option<usize>,
    pub deadline: Option<DateTime<Utc>>,
    pub turn_seconds: i64,
    pub result_pause_seconds: i64,
    pub seats: Vec<BlackjackSeatView>,
    pub viewer_seat: Option<usize>,
    pub bank_balance: Cents,
    pub can_join: bool,
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
    pub shoe: BlackjackShoeView,
    pub trainer: Option<BlackjackTrainerView>,
    pub fresh_shuffle: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlackjackSeatView {
    pub index: usize,
    pub user: Uuid,
    pub display_name: String,
    pub stack: Cents,
    pub bet: Option<Cents>,
    pub insurance: Cents,
    pub leaving: bool,
    pub hands: Vec<BlackjackHandView>,
    pub is_viewer: bool,
    pub result: Option<String>,
    pub waiting: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlackjackTrainerView {
    pub count: Option<BlackjackCountView>,
    pub log: Vec<String>,
    pub analysis: Vec<String>,
    pub quiz: Option<BlackjackCountQuiz>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlackjackLobbyView {
    pub tier: usize,
    pub id: Uuid,
    pub max_bet: Cents,
    pub buy_in: Cents,
    pub occupied: usize,
    pub seat_count: usize,
    pub your_seat: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct BlackjackSettlement {
    pub user: Uuid,
    pub net: Cents,
    pub outcome: crate::blackjack_stats::RoundOutcome,
}

impl BlackjackTable {
    pub fn bet_options(max_bet: Cents) -> [Cents; 4] {
        bet_options(max_bet)
    }

    pub fn new(tier: usize) -> Self {
        let max_bet = TIER_MAX_BETS.get(tier).copied().unwrap_or(TIER_MAX_BETS[0]);
        Self {
            id: table_id(tier),
            tier,
            max_bet,
            seats: vec![None; SEAT_COUNT],
            shoe: BlackjackShoe::table_default(),
            phase: Phase::Betting,
            dealer: Vec::new(),
            dealer_peeked: false,
            current: None,
            deadline: None,
            round_no: 0,
            last_results: Vec::new(),
            updated_at: Utc::now(),
        }
    }

    pub fn humans_in_play(&self) -> usize {
        self.seats
            .iter()
            .filter(|seat| seat.as_ref().is_some_and(|seat| !seat.leaving))
            .count()
    }

    pub fn seat_of(&self, user: Uuid) -> Option<usize> {
        self.seats
            .iter()
            .position(|seat| seat.as_ref().is_some_and(|seat| seat.user == user))
    }

    fn round_seats(&self) -> Vec<usize> {
        self.seats
            .iter()
            .enumerate()
            .filter_map(|(index, seat)| {
                seat.as_ref()
                    .filter(|seat| seat.bet.is_some())
                    .map(|_| index)
            })
            .collect()
    }

    fn finish_pause(&mut self, now: DateTime<Utc>, force: bool) {
        if self.phase == Phase::Settled && (force || self.deadline.is_some_and(|at| at <= now)) {
            self.phase = Phase::Betting;
            self.deadline = None;
            self.last_results.clear();
            self.dealer.clear();
            self.dealer_peeked = false;
            self.current = None;
            for seat in self.seats.iter_mut().flatten() {
                seat.hands.clear();
                seat.insurance = 0;
                seat.insurance_decided = false;
                seat.decisions.clear();
            }
        }
    }

    pub fn place_bet(
        &mut self,
        user: Uuid,
        amount: Cents,
        now: DateTime<Utc>,
    ) -> Result<Vec<BlackjackSettlement>, BlackjackError> {
        self.updated_at = now;
        let seat_index = self.seat_of(user).ok_or(BlackjackError::NotFound)?;
        self.finish_pause(now, true);
        if self.phase != Phase::Betting {
            return Err(BlackjackError::IllegalAction("betting is closed"));
        }
        if !bet_options(self.max_bet).contains(&amount) {
            return Err(BlackjackError::IllegalAction("that wager is not offered"));
        }
        let seat = self.seats[seat_index].as_mut().expect("seat");
        if seat.bet.is_some() {
            return Err(BlackjackError::IllegalAction("you already placed a bet"));
        }
        if seat.stack < amount {
            return Err(BlackjackError::IllegalAction("insufficient table chips"));
        }
        seat.stack -= amount;
        seat.bet = Some(amount);
        let active = self.humans_in_play();
        if active < 2
            || self
                .seats
                .iter()
                .enumerate()
                .filter(|(_, seat)| seat.as_ref().is_some_and(|seat| !seat.leaving))
                .all(|(_, seat)| seat.as_ref().is_some_and(|seat| seat.bet.is_some()))
        {
            return self.deal(now);
        }
        if self.deadline.is_none() {
            self.deadline = Some(now + Duration::seconds(TURN_SECONDS));
        }
        Ok(Vec::new())
    }

    pub fn deal(&mut self, now: DateTime<Utc>) -> Result<Vec<BlackjackSettlement>, BlackjackError> {
        self.updated_at = now;
        if self.phase != Phase::Betting {
            return Err(BlackjackError::IllegalAction("cannot deal now"));
        }
        self.shoe.fresh_shuffle = false;
        self.deadline = None;
        let seats = self.round_seats();
        if seats.is_empty() {
            self.deadline = None;
            return Ok(Vec::new());
        }
        self.round_no += 1;
        self.shoe.hands_dealt += 1;
        self.dealer.clear();
        self.dealer_peeked = false;
        self.current = None;
        self.deadline = None;
        for index in &seats {
            let first = self.draw();
            let second = self.draw();
            let seat = self.seats[*index].as_mut().expect("seat");
            seat.hands.clear();
            seat.insurance = 0;
            seat.insurance_decided = false;
            seat.decisions.clear();
            let cards = vec![first, second];
            let natural = score(&cards).0 == 21;
            seat.hands.push(BlackjackHand {
                cards,
                bet: seat.bet.expect("round bet"),
                status: if natural {
                    BlackjackHandStatus::Blackjack
                } else {
                    BlackjackHandStatus::Playing
                },
                split: false,
                split_aces: false,
                doubled: false,
            });
        }
        let dealer_up = self.draw();
        let dealer_hole = self.draw();
        self.dealer.push(dealer_up);
        self.dealer.push(dealer_hole);
        if self.dealer[0].rank as u8 == 14
            && seats.iter().any(|index| {
                let seat = self.seats[*index].as_ref().expect("seat");
                seat.hands
                    .first()
                    .is_some_and(|hand| seat.stack >= hand.bet / 2)
            })
        {
            self.phase = Phase::Insurance;
            for index in seats {
                let seat = self.seats[index].as_mut().expect("seat");
                seat.insurance_decided = seat.stack < seat.bet.expect("bet") / 2;
            }
            if self
                .round_seats()
                .iter()
                .all(|index| self.seats[*index].as_ref().expect("seat").insurance_decided)
            {
                return self.peek(now);
            }
            if self.round_seats().len() >= 2 {
                self.deadline = Some(now + Duration::seconds(TURN_SECONDS));
            }
            return Ok(Vec::new());
        }
        self.peek(now)
    }

    fn draw(&mut self) -> Card {
        if self.shoe.deck.dealt() >= self.shoe.cut_card {
            self.shoe = BlackjackShoe::table_default();
            self.shoe.fresh_shuffle = true;
        }
        self.shoe.deck.deal().expect("fresh blackjack shoe")
    }

    pub fn insure(
        &mut self,
        user: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Vec<BlackjackSettlement>, BlackjackError> {
        self.updated_at = now;
        let index = self.seat_of(user).ok_or(BlackjackError::NotFound)?;
        if self.phase != Phase::Insurance {
            return Err(BlackjackError::IllegalAction("insurance is not available"));
        }
        let seat = self.seats[index].as_mut().expect("seat");
        let amount = seat.bet.expect("bet") / 2;
        if seat.insurance_decided || seat.stack < amount {
            return Err(BlackjackError::IllegalAction("insurance is not available"));
        }
        seat.stack -= amount;
        seat.insurance = amount;
        seat.insurance_decided = true;
        self.after_insurance(now)
    }

    pub fn decline(
        &mut self,
        user: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Vec<BlackjackSettlement>, BlackjackError> {
        self.updated_at = now;
        let index = self.seat_of(user).ok_or(BlackjackError::NotFound)?;
        if self.phase != Phase::Insurance {
            return Err(BlackjackError::IllegalAction("insurance is not available"));
        }
        let seat = self.seats[index].as_mut().expect("seat");
        if seat.insurance_decided {
            return Err(BlackjackError::IllegalAction(
                "insurance is already decided",
            ));
        }
        seat.insurance_decided = true;
        self.after_insurance(now)
    }

    fn after_insurance(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<Vec<BlackjackSettlement>, BlackjackError> {
        if self
            .round_seats()
            .iter()
            .any(|index| !self.seats[*index].as_ref().expect("seat").insurance_decided)
        {
            return Ok(Vec::new());
        }
        self.peek(now)
    }

    fn peek(&mut self, now: DateTime<Utc>) -> Result<Vec<BlackjackSettlement>, BlackjackError> {
        self.dealer_peeked = true;
        if score(&self.dealer).0 == 21 {
            for index in self.round_seats() {
                let seat = self.seats[index].as_mut().expect("seat");
                for hand in &mut seat.hands {
                    hand.status = if hand.status == BlackjackHandStatus::Blackjack {
                        BlackjackHandStatus::Push
                    } else {
                        BlackjackHandStatus::Loss
                    };
                }
            }
            return self.settle(now);
        }
        let first = self.round_seats().into_iter().find(|index| {
            self.seats[*index].as_ref().is_some_and(|seat| {
                seat.hands
                    .iter()
                    .any(|hand| hand.status == BlackjackHandStatus::Playing)
            })
        });
        if let Some(index) = first {
            self.phase = Phase::Playing;
            let hand = self.seats[index]
                .as_ref()
                .expect("seat")
                .hands
                .iter()
                .position(|hand| hand.status == BlackjackHandStatus::Playing)
                .expect("hand");
            self.current = Some((index, hand));
            if self.round_seats().len() >= 2 {
                self.deadline = Some(now + Duration::seconds(TURN_SECONDS));
            }
            Ok(Vec::new())
        } else {
            self.settle(now)
        }
    }

    pub fn act(
        &mut self,
        user: Uuid,
        action: Action,
        now: DateTime<Utc>,
    ) -> Result<Vec<BlackjackSettlement>, BlackjackError> {
        self.updated_at = now;
        let index = self.seat_of(user).ok_or(BlackjackError::NotFound)?;
        let (current, hand_index) = self.current.ok_or(BlackjackError::Finished)?;
        if self.phase != Phase::Playing || index != current {
            return Err(BlackjackError::IllegalAction("it is not your turn"));
        }
        let recommended = {
            let seat = self.seats[index].as_ref().expect("seat");
            let hand = seat.hands.get(hand_index).expect("hand");
            recommended_action(&self.dealer, hand, action)
        };
        self.seats[index]
            .as_mut()
            .expect("seat")
            .decisions
            .push(BlackjackDecision {
                action,
                recommended,
            });
        match action {
            Action::Hit => {
                let legal = self.seats[index]
                    .as_ref()
                    .expect("seat")
                    .hands
                    .get(hand_index)
                    .is_some_and(|hand| !hand.split_aces);
                if !legal {
                    return Err(BlackjackError::IllegalAction("that action is not legal"));
                }
                let card = self.draw();
                let seat = self.seats[index].as_mut().expect("seat");
                let hand = seat.hands.get_mut(hand_index).expect("hand");
                hand.cards.push(card);
                match score(&hand.cards).0 {
                    21 => hand.status = BlackjackHandStatus::Stand,
                    total if total > 21 => hand.status = BlackjackHandStatus::Bust,
                    _ => {}
                }
            }
            Action::Stand => {
                self.seats[index]
                    .as_mut()
                    .expect("seat")
                    .hands
                    .get_mut(hand_index)
                    .expect("hand")
                    .status = BlackjackHandStatus::Stand
            }
            Action::Double => {
                let (legal, bet) = {
                    let seat = self.seats[index].as_ref().expect("seat");
                    let hand = seat.hands.get(hand_index).expect("hand");
                    (
                        hand.cards.len() == 2 && !hand.split_aces && seat.stack >= hand.bet,
                        hand.bet,
                    )
                };
                if !legal {
                    return Err(BlackjackError::IllegalAction("that action is not legal"));
                }
                let card = self.draw();
                let seat = self.seats[index].as_mut().expect("seat");
                seat.stack -= bet;
                let hand = seat.hands.get_mut(hand_index).expect("hand");
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
                let legal = {
                    let seat = self.seats[index].as_ref().expect("seat");
                    let hand = seat.hands.get(hand_index).expect("hand");
                    hand.cards.len() == 2
                        && hand.cards[0].rank == hand.cards[1].rank
                        && seat.hands.len() < MAX_HANDS
                        && seat.stack >= hand.bet
                };
                if !legal {
                    return Err(BlackjackError::IllegalAction("that action is not legal"));
                }
                let first_card = self.draw();
                let second_card = self.draw();
                let seat = self.seats[index].as_mut().expect("seat");
                let hand = seat.hands.get_mut(hand_index).expect("hand");
                let bet = hand.bet;
                let second = hand.cards.pop().expect("pair");
                seat.stack -= bet;
                let split_aces = second.rank as u8 == 14;
                hand.split = true;
                hand.cards.push(first_card);
                hand.split_aces = split_aces;
                let status = if score(&hand.cards).0 > 21 {
                    BlackjackHandStatus::Bust
                } else if split_aces {
                    BlackjackHandStatus::Stand
                } else {
                    BlackjackHandStatus::Playing
                };
                hand.status = status;
                seat.hands.insert(
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
        self.advance_current(now)
    }

    fn advance_current(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<Vec<BlackjackSettlement>, BlackjackError> {
        let mut found = None;
        for (index, seat) in self.seats.iter().enumerate() {
            if !self.round_seats().contains(&index) {
                continue;
            }
            if let Some(seat) = seat
                && let Some(hand) = seat
                    .hands
                    .iter()
                    .position(|hand| hand.status == BlackjackHandStatus::Playing)
            {
                found = Some((index, hand));
                break;
            }
        }
        self.current = found;
        if let Some((_, _)) = found {
            self.deadline =
                (self.round_seats().len() >= 2).then_some(now + Duration::seconds(TURN_SECONDS));
            Ok(Vec::new())
        } else {
            self.settle(now)
        }
    }

    pub fn tick(&mut self, now: DateTime<Utc>) -> Result<Vec<BlackjackSettlement>, BlackjackError> {
        self.updated_at = now;
        if self.deadline.is_none_or(|at| at > now) {
            return Ok(Vec::new());
        }
        match self.phase {
            Phase::Betting => self.deal(now),
            Phase::Insurance => {
                for index in self.round_seats() {
                    self.seats[index].as_mut().expect("seat").insurance_decided = true;
                }
                self.after_insurance(now)
            }
            Phase::Playing => {
                if let Some((index, _)) = self.current {
                    self.seats[index]
                        .as_mut()
                        .expect("seat")
                        .hands
                        .iter_mut()
                        .find(|hand| hand.status == BlackjackHandStatus::Playing)
                        .expect("current hand")
                        .status = BlackjackHandStatus::Stand;
                }
                self.advance_current(now)
            }
            Phase::Settled => {
                self.finish_pause(now, false);
                Ok(Vec::new())
            }
        }
    }

    fn settle(&mut self, now: DateTime<Utc>) -> Result<Vec<BlackjackSettlement>, BlackjackError> {
        let visible_before_settlement = self.visible_cards();
        let all_busted = self.round_seats().iter().all(|index| {
            self.seats[*index]
                .as_ref()
                .expect("seat")
                .hands
                .iter()
                .all(|hand| hand.status == BlackjackHandStatus::Bust)
        });
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
        let mut settlements = Vec::new();
        self.last_results.clear();
        for (index, seat) in self.seats.iter_mut().enumerate() {
            let Some(seat) = seat else { continue };
            let Some(_base_bet) = seat.bet else { continue };
            let mut returned = seat.insurance_payout(self.dealer.as_slice());
            for hand in &mut seat.hands {
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
            seat.stack += returned;
            let wagered = seat.hands.iter().map(|hand| hand.bet).sum::<Cents>() + seat.insurance;
            let net = returned - wagered;
            let summary = if net >= 0 {
                format!("Won ${}", net / 100)
            } else {
                format!("Lost ${}", -net / 100)
            };
            self.last_results.push((index, net, summary.clone()));
            settlements.push(BlackjackSettlement {
                user: seat.user,
                net,
                outcome: outcome_for(seat, returned),
            });
            seat.bet = None;
        }
        self.shoe.running_count += count(&exposed_at_settlement);
        self.shoe.exposed_cards += exposed_at_settlement.len();
        self.phase = Phase::Settled;
        self.current = None;
        self.deadline = Some(now + Duration::seconds(RESULT_PAUSE_SECONDS));
        Ok(settlements)
    }

    pub fn action_flags(&self, user: Uuid) -> (bool, bool, bool, bool, bool, bool, bool, bool) {
        let Some(index) = self.seat_of(user) else {
            return (false, false, false, false, false, false, false, false);
        };
        let Some((current, hand_index)) = self.current else {
            return (false, false, false, false, false, false, false, false);
        };
        if current != index || self.phase != Phase::Playing {
            return (false, false, false, false, false, false, false, false);
        }
        let seat = self.seats[index].as_ref().expect("seat");
        let hand = seat.hands.get(hand_index).expect("hand");
        (
            true,
            true,
            hand.cards.len() == 2 && !hand.split_aces && seat.stack >= hand.bet,
            hand.cards.len() == 2
                && hand.cards[0].rank == hand.cards[1].rank
                && seat.hands.len() < MAX_HANDS
                && seat.stack >= hand.bet,
            false,
            false,
            false,
            false,
        )
    }

    pub fn view(&self, viewer: Option<Uuid>, bank_balance: Cents) -> BlackjackTableView {
        let viewer_seat = viewer.and_then(|user| self.seat_of(user));
        let (can_hit, can_stand, can_double, can_split, _, _, _, _) = viewer.map_or(
            (false, false, false, false, false, false, false, false),
            |user| self.action_flags(user),
        );
        let can_insure = viewer_seat.is_some_and(|index| {
            self.phase == Phase::Insurance
                && self.seats[index].as_ref().is_some_and(|seat| {
                    !seat.insurance_decided && seat.stack >= seat.bet.unwrap_or_default() / 2
                })
        });
        let can_decline = can_insure;
        let shoe = BlackjackShoeView {
            decks: 8,
            total_cards: 416,
            dealt_cards: 416usize.saturating_sub(self.shoe.deck.remaining()),
            remaining_cards: self.shoe.deck.remaining(),
            cut_card: self.shoe.cut_card,
            penetration_percent: 50,
            hands_dealt: self.shoe.hands_dealt,
            fresh_shuffle: self.shoe.fresh_shuffle,
        };
        let seats = self
            .seats
            .iter()
            .enumerate()
            .filter_map(|(index, seat)| {
                let seat = seat.as_ref()?;
                let result = self
                    .last_results
                    .iter()
                    .find(|result| result.0 == index)
                    .map(|result| result.2.clone());
                Some(BlackjackSeatView {
                    index,
                    user: seat.user,
                    display_name: format!("Player {}", index + 1),
                    stack: seat.stack,
                    bet: seat.bet,
                    insurance: seat.insurance,
                    leaving: seat.leaving,
                    hands: seat
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
                    is_viewer: viewer == Some(seat.user),
                    result,
                    waiting: (self.phase != Phase::Betting
                        && seat.bet.is_none()
                        && seat.hands.is_empty())
                        || (self.phase == Phase::Betting && seat.bet.is_none()),
                })
            })
            .collect();
        let trainer = viewer_seat.and_then(|index| {
            let seat = self.seats[index].as_ref()?;
            let cards = self.visible_cards();
            let running = if self.phase == Phase::Settled {
                self.shoe.running_count
            } else {
                self.shoe.running_count + count(&cards)
            };
            Some(BlackjackTrainerView {
                count: (seat.settings.counting_tutor || seat.settings.counting_quiz)
                    .then(|| count_view(running, cards.len(), shoe.dealt_cards)),
                log: if seat.settings.counting_tutor {
                    count_log(&cards, self.shoe.running_count)
                } else {
                    Vec::new()
                },
                analysis: if seat.settings.bet_analyzer {
                    seat.decisions
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
                quiz: (seat.settings.counting_quiz && self.phase == Phase::Settled)
                    .then(|| count_quiz(running)),
            })
        });
        BlackjackTableView {
            id: self.id,
            tier: self.tier,
            max_bet: self.max_bet,
            buy_in: buy_in_for(self.max_bet),
            bet_options: bet_options(self.max_bet),
            min_bet: self.max_bet / 4,
            seat_count: SEAT_COUNT,
            phase: self.phase,
            dealer: if self.phase == Phase::Playing || self.phase == Phase::Insurance {
                self.dealer.first().copied().into_iter().collect()
            } else {
                self.dealer.clone()
            },
            dealer_hidden: matches!(self.phase, Phase::Insurance | Phase::Playing)
                && self.dealer.len() > 1,
            dealer_score: (self.phase == Phase::Settled).then(|| score(&self.dealer).0),
            current_seat: self.current.map(|current| current.0),
            current_hand: self.current.map(|current| current.1),
            deadline: self.deadline,
            turn_seconds: TURN_SECONDS,
            result_pause_seconds: RESULT_PAUSE_SECONDS,
            seats,
            viewer_seat,
            bank_balance,
            can_join: viewer_seat.is_none(),
            can_leave: viewer_seat.is_some(),
            can_rebuy: viewer_seat.is_some_and(|index| {
                self.seats[index]
                    .as_ref()
                    .is_some_and(|seat| seat.bet.is_none() && seat.stack < buy_in_for(self.max_bet))
            }),
            can_bet: viewer_seat.is_some_and(|index| {
                matches!(self.phase, Phase::Betting | Phase::Settled)
                    && self.seats[index].as_ref().is_some_and(|seat| {
                        !seat.leaving && seat.bet.is_none() && seat.stack >= self.max_bet / 4
                    })
            }),
            can_insure,
            can_decline,
            can_hit,
            can_stand,
            can_double,
            can_split,
            message: self.status_message(),
            shoe,
            trainer,
            fresh_shuffle: self.shoe.fresh_shuffle,
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
        for (index, seat) in self.seats.iter().enumerate() {
            if let Some(seat) = seat {
                for hand in &seat.hands {
                    cards.extend(
                        hand.cards
                            .iter()
                            .copied()
                            .map(|card| (format!("Seat {}", index + 1), card)),
                    );
                }
            }
        }
        cards
    }

    fn status_message(&self) -> String {
        match self.phase {
            Phase::Betting => "Place your bet".into(),
            Phase::Insurance => "Dealer shows an Ace — insurance?".into(),
            Phase::Settled => "Round settled".into(),
            Phase::Playing => self.current.map_or_else(
                || "Dealer is playing".into(),
                |(index, _)| format!("Seat {} to act", index + 1),
            ),
        }
    }
}

impl BlackjackSeat {
    fn insurance_payout(&self, dealer: &[Card]) -> Cents {
        if self.insurance > 0 && score(dealer).0 == 21 && dealer.len() == 2 {
            self.insurance * 3
        } else {
            0
        }
    }
}

impl BlackjackStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn load(root: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let dir = root.as_ref().join("blackjack");
        tokio::fs::create_dir_all(&dir).await?;
        if tokio::fs::try_exists(dir.join("games.json"))
            .await
            .unwrap_or(false)
        {
            tracing::warn!("ignoring legacy blackjack/games.json persistence");
        }
        if tokio::fs::try_exists(dir.join("shoes.json"))
            .await
            .unwrap_or(false)
        {
            tracing::warn!("ignoring legacy blackjack/shoes.json persistence");
        }
        let tables_path = dir.join("tables.json");
        let mut tables = match tokio::fs::read(&tables_path).await {
            Ok(bytes) => serde_json::from_slice::<Vec<BlackjackTable>>(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        for tier in 0..TIER_MAX_BETS.len() {
            if !tables.iter().any(|table| table.tier == tier) {
                tables.push(BlackjackTable::new(tier));
            }
        }
        for table in &mut tables {
            if table.phase != Phase::Betting {
                for seat in table.seats.iter_mut().flatten() {
                    seat.stack +=
                        seat.hands.iter().map(|hand| hand.bet).sum::<Cents>() + seat.insurance;
                    seat.bet = None;
                    seat.insurance = 0;
                    seat.insurance_decided = false;
                    seat.hands.clear();
                    seat.decisions.clear();
                }
                table.phase = Phase::Betting;
                table.dealer.clear();
                table.dealer_peeked = false;
                table.current = None;
                table.deadline = None;
                table.last_results.clear();
            }
        }
        tables.sort_by_key(|table| table.tier);
        let (changed, _) = broadcast::channel(32);
        Ok(Self {
            tables: Arc::new(Mutex::new(tables)),
            tables_path: Some(tables_path),
            changed,
        })
    }

    pub async fn persist(&self) -> Result<(), anyhow::Error> {
        self.persist_tables().await
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Uuid> {
        self.changed.subscribe()
    }

    pub async fn ids(&self) -> Vec<Uuid> {
        self.tables
            .lock()
            .await
            .iter()
            .map(|table| table.id)
            .collect()
    }

    pub async fn lobby(&self, viewer: Option<Uuid>) -> Vec<BlackjackLobbyView> {
        self.tables
            .lock()
            .await
            .iter()
            .map(|table| BlackjackLobbyView {
                tier: table.tier,
                id: table.id,
                max_bet: table.max_bet,
                buy_in: buy_in_for(table.max_bet),
                occupied: table.seats.iter().flatten().count(),
                seat_count: SEAT_COUNT,
                your_seat: viewer.and_then(|user| table.seat_of(user)),
            })
            .collect()
    }

    pub async fn view(
        &self,
        id: Uuid,
        viewer: Option<Uuid>,
        balance: Cents,
    ) -> Result<BlackjackTableView, BlackjackError> {
        let tables = self.tables.lock().await;
        let table = tables
            .iter()
            .find(|table| table.id == id)
            .ok_or(BlackjackError::NotFound)?;
        let mut view = table.view(viewer, balance);
        view.can_join =
            viewer.is_some_and(|user| !tables.iter().any(|table| table.seat_of(user).is_some()));
        Ok(view)
    }

    async fn persist_tables(&self) -> Result<(), anyhow::Error> {
        let Some(path) = &self.tables_path else {
            return Ok(());
        };
        let body = serde_json::to_vec_pretty(&*self.tables.lock().await)?;
        let tmp = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
        tokio::fs::write(&tmp, body).await?;
        tokio::fs::rename(tmp, path).await?;
        Ok(())
    }

    async fn changed(&self, id: Uuid) -> Result<(), anyhow::Error> {
        self.persist_tables().await?;
        let _ = self.changed.send(id);
        Ok(())
    }

    pub async fn join(
        &self,
        id: Uuid,
        user: Uuid,
        settings: BlackjackTrainerSettings,
        bank: &crate::bank::BankStore,
    ) -> Result<(), BlackjackError> {
        let buy_in = {
            let tables = self.tables.lock().await;
            if tables.iter().any(|table| table.seat_of(user).is_some()) {
                return Err(BlackjackError::ActiveGame);
            }
            let table = tables
                .iter()
                .find(|table| table.id == id)
                .ok_or(BlackjackError::NotFound)?;
            if table.seats.iter().flatten().count() >= SEAT_COUNT {
                return Err(BlackjackError::IllegalAction("table is full"));
            }
            buy_in_for(table.max_bet)
        };
        bank.blackjack_buy_in(crate::bank::AccountOwner::User(user), id, buy_in)
            .await
            .map_err(|_| BlackjackError::IllegalAction("insufficient funds"))?;
        let mut tables = self.tables.lock().await;
        let table = tables
            .iter_mut()
            .find(|table| table.id == id)
            .ok_or(BlackjackError::NotFound)?;
        if table.seat_of(user).is_some() || table.seats.iter().flatten().count() >= SEAT_COUNT {
            let _ = bank
                .blackjack_cash_out(crate::bank::AccountOwner::User(user), id, buy_in)
                .await;
            return Err(BlackjackError::IllegalAction(
                "table is full or you are already seated",
            ));
        }
        let index = table
            .seats
            .iter()
            .position(Option::is_none)
            .expect("seat available");
        table.seats[index] = Some(BlackjackSeat {
            user,
            stack: buy_in,
            bet: None,
            hands: Vec::new(),
            insurance: 0,
            insurance_decided: false,
            leaving: false,
            settings: settings.sanitized(),
            decisions: Vec::new(),
        });
        drop(tables);
        self.changed(id)
            .await
            .map_err(|_| BlackjackError::IllegalAction("could not persist table"))
    }

    pub async fn leave(
        &self,
        id: Uuid,
        user: Uuid,
        bank: &crate::bank::BankStore,
    ) -> Result<(), BlackjackError> {
        let (amount, immediate) = {
            let mut tables = self.tables.lock().await;
            let table = tables
                .iter_mut()
                .find(|table| table.id == id)
                .ok_or(BlackjackError::NotFound)?;
            let index = table.seat_of(user).ok_or(BlackjackError::NotFound)?;
            let seat = table.seats[index].as_mut().expect("seat");
            let immediate = seat.bet.is_none();
            if immediate {
                let amount = seat.stack;
                table.updated_at = Utc::now();
                table.seats[index] = None;
                (amount, true)
            } else {
                table.updated_at = Utc::now();
                seat.leaving = true;
                (0, false)
            }
        };
        if immediate {
            bank.blackjack_cash_out(crate::bank::AccountOwner::User(user), id, amount)
                .await
                .map_err(|_| BlackjackError::IllegalAction("cash out failed"))?;
        }
        self.changed(id)
            .await
            .map_err(|_| BlackjackError::IllegalAction("could not persist table"))
    }

    pub async fn rebuy(
        &self,
        id: Uuid,
        user: Uuid,
        bank: &crate::bank::BankStore,
    ) -> Result<(), BlackjackError> {
        let amount = {
            let tables = self.tables.lock().await;
            let table = tables
                .iter()
                .find(|table| table.id == id)
                .ok_or(BlackjackError::NotFound)?;
            let seat = table
                .seats
                .iter()
                .flatten()
                .find(|seat| seat.user == user)
                .ok_or(BlackjackError::NotFound)?;
            if seat.bet.is_some() {
                return Err(BlackjackError::IllegalAction(
                    "rebuy is unavailable during a round",
                ));
            }
            buy_in_for(table.max_bet).saturating_sub(seat.stack)
        };
        if amount == 0 {
            return Err(BlackjackError::IllegalAction("your stack is already full"));
        }
        bank.blackjack_buy_in(crate::bank::AccountOwner::User(user), id, amount)
            .await
            .map_err(|_| BlackjackError::IllegalAction("insufficient funds"))?;
        let mut tables = self.tables.lock().await;
        let table = tables
            .iter_mut()
            .find(|table| table.id == id)
            .ok_or(BlackjackError::NotFound)?;
        let seat = table
            .seats
            .iter_mut()
            .flatten()
            .find(|seat| seat.user == user)
            .expect("seat");
        seat.stack += amount;
        table.updated_at = Utc::now();
        drop(tables);
        self.changed(id)
            .await
            .map_err(|_| BlackjackError::IllegalAction("could not persist table"))
    }

    async fn resolve(
        &self,
        id: Uuid,
        action: impl FnOnce(
            &mut BlackjackTable,
            DateTime<Utc>,
        ) -> Result<Vec<BlackjackSettlement>, BlackjackError>,
        now: DateTime<Utc>,
        bank: &crate::bank::BankStore,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        let settlements = {
            let mut tables = self.tables.lock().await;
            let table = tables
                .iter_mut()
                .find(|table| table.id == id)
                .ok_or(BlackjackError::NotFound)?;
            action(table, now)?
        };
        for settlement in settlements {
            let _ = stats.record(settlement.user, settlement.outcome).await;
        }
        let leaving = {
            let mut tables = self.tables.lock().await;
            let table = tables
                .iter_mut()
                .find(|table| table.id == id)
                .expect("table");
            let mut leaving = Vec::new();
            for seat in &mut table.seats {
                if seat
                    .as_ref()
                    .is_some_and(|seat| seat.leaving && seat.bet.is_none())
                {
                    let seat = seat.take().expect("seat");
                    leaving.push((seat.user, seat.stack));
                }
            }
            leaving
        };
        for (user, amount) in leaving {
            let _ = bank
                .blackjack_cash_out(crate::bank::AccountOwner::User(user), id, amount)
                .await;
        }
        self.changed(id)
            .await
            .map_err(|_| BlackjackError::IllegalAction("could not persist table"))
    }

    pub async fn bet(
        &self,
        id: Uuid,
        user: Uuid,
        amount: Cents,
        now: DateTime<Utc>,
        bank: &crate::bank::BankStore,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        self.resolve(
            id,
            |table, now| table.place_bet(user, amount, now),
            now,
            bank,
            stats,
        )
        .await
    }
    pub async fn insure(
        &self,
        id: Uuid,
        user: Uuid,
        now: DateTime<Utc>,
        bank: &crate::bank::BankStore,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        self.resolve(id, |table, now| table.insure(user, now), now, bank, stats)
            .await
    }
    pub async fn decline(
        &self,
        id: Uuid,
        user: Uuid,
        now: DateTime<Utc>,
        bank: &crate::bank::BankStore,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        self.resolve(id, |table, now| table.decline(user, now), now, bank, stats)
            .await
    }
    pub async fn act(
        &self,
        id: Uuid,
        user: Uuid,
        action: Action,
        now: DateTime<Utc>,
        bank: &crate::bank::BankStore,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Result<(), BlackjackError> {
        self.resolve(
            id,
            |table, now| table.act(user, action, now),
            now,
            bank,
            stats,
        )
        .await
    }
    pub async fn update_settings(
        &self,
        id: Uuid,
        user: Uuid,
        settings: BlackjackTrainerSettings,
    ) -> Result<(), BlackjackError> {
        let mut tables = self.tables.lock().await;
        let table = tables
            .iter_mut()
            .find(|table| table.id == id)
            .ok_or(BlackjackError::NotFound)?;
        let seat = table
            .seats
            .iter_mut()
            .flatten()
            .find(|seat| seat.user == user)
            .ok_or(BlackjackError::NotFound)?;
        seat.settings = settings.sanitized();
        table.updated_at = Utc::now();
        drop(tables);
        self.changed(id)
            .await
            .map_err(|_| BlackjackError::IllegalAction("could not persist table"))
    }

    pub async fn tick(
        &self,
        now: DateTime<Utc>,
        bank: &crate::bank::BankStore,
        stats: &crate::blackjack_stats::BlackjackStatsStore,
    ) -> Vec<Uuid> {
        let ids = self.ids().await;
        let mut changed = Vec::new();
        for id in ids {
            let due = self
                .tables
                .lock()
                .await
                .iter()
                .find(|table| table.id == id)
                .is_some_and(|table| table.deadline.is_some_and(|deadline| deadline <= now));
            if due
                && self
                    .resolve(id, |table, now| table.tick(now), now, bank, stats)
                    .await
                    .is_ok()
            {
                changed.push(id);
            }
        }
        changed
    }
}

fn outcome_for(seat: &BlackjackSeat, returned: Cents) -> crate::blackjack_stats::RoundOutcome {
    let mut outcome = crate::blackjack_stats::RoundOutcome {
        hands: seat.hands.len() as u64,
        splits: seat.hands.len().saturating_sub(1) as u64,
        insured: seat.insurance > 0,
        wagered: seat.hands.iter().map(|hand| hand.bet).sum::<Cents>() + seat.insurance,
        returned,
        ..Default::default()
    };
    for hand in &seat.hands {
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

#[cfg(test)]
mod shared_table_tests {
    use super::*;

    #[test]
    fn shared_table_constants_are_stable() {
        assert_eq!(bet_options(10_000), [2_500, 5_000, 7_500, 10_000]);
        assert_eq!(buy_in_for(10_000_000), 100_000_000);
        assert_eq!(table_id(0), TABLE_IDS[0]);
        assert_eq!(table_id(3), TABLE_IDS[3]);
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
    fn settled_view_retains_hands_until_pause_finishes() {
        let user = Uuid::new_v4();
        let mut table = BlackjackTable::new(0);
        table.seats[0] = Some(BlackjackSeat {
            user,
            stack: 10_000,
            bet: None,
            hands: vec![BlackjackHand {
                cards: vec![Card::new(
                    crate::cards::Rank::Ten,
                    crate::cards::Suit::Clubs,
                )],
                bet: 2_500,
                status: BlackjackHandStatus::Win,
                split: false,
                split_aces: false,
                doubled: false,
            }],
            insurance: 0,
            insurance_decided: false,
            leaving: false,
            settings: Default::default(),
            decisions: Vec::new(),
        });
        table.phase = Phase::Settled;
        table.deadline = Some(Utc::now() + Duration::seconds(5));
        assert_eq!(table.view(Some(user), 0).seats[0].hands.len(), 1);
        table.finish_pause(Utc::now(), true);
        assert!(table.seats[0].as_ref().expect("seat").hands.is_empty());
    }
}
