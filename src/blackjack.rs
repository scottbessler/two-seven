use crate::{
    cards::{Card, Deck},
    money::{Cents, valid_game_amount},
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;
use uuid::Uuid;

pub const CHEAPEST_STARTING_BET_CAP: Cents = 10_000;

pub fn max_starting_bet(balance: Cents) -> Cents {
    (balance / 2 / 100 * 100)
        .max(crate::money::MIN_GAME_AMOUNT)
        .min(balance)
}

/// The stakes offered for a bankroll: a nibble, a real bet, and a big one.
pub fn bet_options(balance: Cents) -> Vec<Cents> {
    if balance < crate::money::MIN_GAME_AMOUNT {
        return Vec::new();
    }
    let max_start = max_starting_bet(balance);
    let mut bets: Vec<Cents> = [
        (balance / 100).min(CHEAPEST_STARTING_BET_CAP),
        balance / 20,
        balance / 4,
    ]
    .into_iter()
    // Whole dollars read better on a button, and nothing under a dollar.
    .map(|bet| {
        (bet / 100 * 100)
            .max(crate::money::MIN_GAME_AMOUNT)
            .min(max_start)
    })
    .collect();
    bets.push(max_start);
    bets.sort_unstable();
    bets.dedup();
    bets
}

#[derive(Clone, Debug, Serialize)]
pub struct BlackjackView {
    pub id: Uuid,
    pub bet: Cents,
    pub player: Vec<Card>,
    pub dealer: Vec<Card>,
    pub player_score: u8,
    pub dealer_score: Option<u8>,
    pub status: BlackjackStatus,
    pub message: String,
    pub payout: Cents,
    pub can_hit: bool,
    pub can_stand: bool,
    pub can_double: bool,
    pub can_split: bool,
    pub can_insure: bool,
    pub insurance: Cents,
    pub hands: Vec<BlackjackHandView>,
    pub active_hand: usize,
    pub settings: BlackjackTrainerSettings,
    pub count: Option<BlackjackCountView>,
    pub trainer_log: Vec<String>,
    pub quiz: Option<BlackjackCountQuiz>,
    pub analysis: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlackjackHandView {
    pub cards: Vec<Card>,
    pub bet: Cents,
    pub score: u8,
    pub status: BlackjackHandStatus,
    pub blackjack: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlackjackTrainerSettings {
    #[serde(default = "default_decks")]
    pub decks: u8,
    #[serde(default = "default_penetration_percent")]
    pub penetration_percent: u8,
    #[serde(default)]
    pub counting_tutor: bool,
    #[serde(default)]
    pub counting_quiz: bool,
    #[serde(default)]
    pub bet_analyzer: bool,
}

impl Default for BlackjackTrainerSettings {
    fn default() -> Self {
        Self {
            decks: default_decks(),
            penetration_percent: default_penetration_percent(),
            counting_tutor: false,
            counting_quiz: false,
            bet_analyzer: false,
        }
    }
}

impl BlackjackTrainerSettings {
    pub fn sanitized(self) -> Self {
        Self {
            decks: match self.decks {
                1 | 2 | 8 => self.decks,
                _ => default_decks(),
            },
            penetration_percent: self.penetration_percent.clamp(25, 95),
            counting_tutor: self.counting_tutor,
            counting_quiz: self.counting_quiz,
            bet_analyzer: self.bet_analyzer,
        }
    }

    fn shoe_cards(&self) -> usize {
        usize::from(self.decks) * 52
    }
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
pub enum BlackjackStatus {
    Playing,
    PlayerBlackjack,
    PlayerBust,
    DealerBust,
    PlayerWin,
    DealerWin,
    Push,
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
    inner: Arc<Mutex<HashMap<Uuid, BlackjackGame>>>,
    path: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BlackjackHand {
    cards: Vec<Card>,
    bet: Cents,
    status: BlackjackHandStatus,
    split: bool,
    split_aces: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BlackjackGame {
    id: Uuid,
    user: Uuid,
    deck: Deck,
    hands: Vec<BlackjackHand>,
    dealer: Vec<Card>,
    insurance: Cents,
    dealer_peeked: bool,
    status: BlackjackStatus,
    payout: Cents,
    #[serde(default)]
    settings: BlackjackTrainerSettings,
    #[serde(default)]
    decisions: Vec<BlackjackDecision>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BlackjackDecision {
    action: Action,
    recommended: Action,
}

fn default_decks() -> u8 {
    8
}

fn default_penetration_percent() -> u8 {
    75
}

const MAX_HANDS: usize = 4;

impl Default for BlackjackStore {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            path: None,
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
        let path = dir.join("games.json");
        let games = match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice::<HashMap<Uuid, BlackjackGame>>(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(
                games
                    .into_iter()
                    .filter(|(_, game)| game.status == BlackjackStatus::Playing)
                    .collect(),
            )),
            path: Some(path),
        })
    }

    pub async fn persist(&self) -> Result<(), anyhow::Error> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let games = self
            .inner
            .lock()
            .await
            .iter()
            .filter(|(_, game)| game.status == BlackjackStatus::Playing)
            .map(|(id, game)| (*id, game.clone()))
            .collect::<HashMap<_, _>>();
        let data = serde_json::to_vec_pretty(&games)?;
        let tmp = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
        tokio::fs::write(&tmp, data).await?;
        tokio::fs::rename(tmp, path).await?;
        Ok(())
    }

    pub async fn view(
        &self,
        user: Uuid,
        id: Uuid,
        balance: Cents,
    ) -> Result<BlackjackView, BlackjackError> {
        let guard = self.inner.lock().await;
        let game = guard.get(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        Ok(game.view(false, balance))
    }
    pub async fn resume(&self, user: Uuid, balance: Cents) -> Option<BlackjackView> {
        let guard = self.inner.lock().await;
        guard
            .values()
            .find(|game| game.user == user && game.status == BlackjackStatus::Playing)
            .map(|game| game.view(false, balance))
    }

    pub async fn start(
        &self,
        user: Uuid,
        bet: Cents,
        id: Uuid,
        balance: Cents,
        settings: BlackjackTrainerSettings,
    ) -> Result<BlackjackView, BlackjackError> {
        if !valid_game_amount(bet) {
            return Err(BlackjackError::IllegalAction("bet must be at least $1"));
        }
        let mut guard = self.inner.lock().await;
        if guard
            .values()
            .any(|game| game.user == user && game.status == BlackjackStatus::Playing)
        {
            return Err(BlackjackError::ActiveGame);
        }
        guard.retain(|_, game| game.user != user || game.status == BlackjackStatus::Playing);
        let settings = settings.sanitized();
        let mut deck = Deck::shoe_seeded(rand::thread_rng().r#gen(), settings.decks);
        let player = vec![deck.deal().expect("card"), deck.deal().expect("card")];
        let dealer = vec![deck.deal().expect("card"), deck.deal().expect("card")];
        let natural = score(&player).0 == 21;
        let mut game = BlackjackGame {
            id,
            user,
            deck,
            dealer,
            insurance: 0,
            dealer_peeked: false,
            payout: 0,
            status: BlackjackStatus::Playing,
            settings,
            decisions: vec![BlackjackDecision {
                action: Action::Bet,
                recommended: Action::Bet,
            }],
            hands: vec![BlackjackHand {
                cards: player,
                bet,
                status: if natural {
                    BlackjackHandStatus::Blackjack
                } else {
                    BlackjackHandStatus::Playing
                },
                split: false,
                split_aces: false,
            }],
        };
        if !game.can_insure(balance) {
            game.peek();
        }
        let view = game.view(false, balance);
        guard.insert(id, game);
        Ok(view)
    }

    pub async fn hit(
        &self,
        user: Uuid,
        id: Uuid,
        balance: Cents,
    ) -> Result<BlackjackView, BlackjackError> {
        let mut guard = self.inner.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        game.wager(Action::Hit, balance)?;
        game.record_decision(Action::Hit);
        game.peek_if_needed();
        if game.status != BlackjackStatus::Playing {
            return Ok(game.view(false, balance));
        }
        let i = game.active_index();
        game.hands[i].cards.push(game.deck.deal().expect("card"));
        if score(&game.hands[i].cards).0 > 21 {
            game.hands[i].status = BlackjackHandStatus::Bust;
            game.advance();
        }
        Ok(game.view(false, balance))
    }

    pub async fn stand(
        &self,
        user: Uuid,
        id: Uuid,
        balance: Cents,
    ) -> Result<BlackjackView, BlackjackError> {
        let mut guard = self.inner.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        game.wager(Action::Stand, balance)?;
        game.record_decision(Action::Stand);
        game.peek_if_needed();
        if game.status != BlackjackStatus::Playing {
            return Ok(game.view(true, balance));
        }
        let i = game.active_index();
        game.hands[i].status = BlackjackHandStatus::Stand;
        game.advance();
        Ok(game.view(true, balance))
    }

    pub async fn double(
        &self,
        user: Uuid,
        id: Uuid,
        balance: Cents,
    ) -> Result<(BlackjackView, Cents), BlackjackError> {
        let mut guard = self.inner.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        let wager = game.wager(Action::Double, balance)?;
        game.record_decision(Action::Double);
        game.peek_if_needed();
        if game.status != BlackjackStatus::Playing {
            return Ok((game.view(false, balance), 0));
        }
        let i = game.active_index();
        game.hands[i].bet += wager;
        game.hands[i].cards.push(game.deck.deal().expect("card"));
        game.hands[i].status = if score(&game.hands[i].cards).0 > 21 {
            BlackjackHandStatus::Bust
        } else {
            BlackjackHandStatus::Stand
        };
        game.advance();
        // The stake just placed is still in the balance the route read.
        Ok((game.view(true, balance - wager), wager))
    }

    pub async fn split(
        &self,
        user: Uuid,
        id: Uuid,
        balance: Cents,
    ) -> Result<(BlackjackView, Cents), BlackjackError> {
        let mut guard = self.inner.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        let wager = game.wager(Action::Split, balance)?;
        game.record_decision(Action::Split);
        game.peek_if_needed();
        if game.status != BlackjackStatus::Playing {
            return Ok((game.view(false, balance), 0));
        }
        let i = game.active_index();
        let first = game.hands[i].cards.remove(1);
        let split_aces = game.hands[i].cards[0].rank as u8 == 14;
        let second = BlackjackHand {
            cards: vec![first],
            bet: game.hands[i].bet,
            status: BlackjackHandStatus::Playing,
            split: true,
            split_aces,
        };
        game.hands[i].split = true;
        game.hands[i].split_aces = split_aces;
        game.hands.insert(i + 1, second);
        let first_card = game.deck.deal().expect("card");
        let second_card = game.deck.deal().expect("card");
        game.hands[i].cards.push(first_card);
        game.hands[i + 1].cards.push(second_card);
        if split_aces {
            game.hands[i].status = BlackjackHandStatus::Stand;
            game.hands[i + 1].status = BlackjackHandStatus::Stand;
            game.advance();
        }
        Ok((game.view(false, balance - wager), wager))
    }

    pub async fn insure(
        &self,
        user: Uuid,
        id: Uuid,
        balance: Cents,
    ) -> Result<(BlackjackView, Cents), BlackjackError> {
        let mut guard = self.inner.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        let wager = game.wager(Action::Insure, balance)?;
        game.record_decision(Action::Insure);
        game.insurance = wager;
        game.peek_if_needed();
        Ok((game.view(false, balance - wager), wager))
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Action {
    Bet,
    Hit,
    Stand,
    Double,
    Split,
    Insure,
}
impl BlackjackGame {
    fn check_user(&self, user: Uuid) -> Result<(), BlackjackError> {
        (self.user == user)
            .then_some(())
            .ok_or(BlackjackError::NotFound)
    }

    fn active_index(&self) -> usize {
        self.hands
            .iter()
            .position(|hand| hand.status == BlackjackHandStatus::Playing)
            .unwrap_or(self.hands.len())
    }

    fn wager(&self, a: Action, balance: Cents) -> Result<Cents, BlackjackError> {
        if self.status != BlackjackStatus::Playing {
            return Err(BlackjackError::Finished);
        }
        let i = self.active_index();
        let hand = self.hands.get(i).ok_or(BlackjackError::Finished)?;
        let legal = match a {
            Action::Bet => false,
            Action::Hit => self.can_hit(),
            Action::Stand => self.can_stand(),
            Action::Double => self.can_double(balance),
            Action::Split => self.can_split(balance),
            Action::Insure => self.can_insure(balance),
        };
        if !legal {
            return Err(BlackjackError::IllegalAction(match a {
                Action::Bet => "bet is not legal during a hand",
                Action::Hit => "hit is not legal",
                Action::Stand => "stand is not legal",
                Action::Double => "double is not legal or you cannot afford the additional wager",
                Action::Split => "hand cannot be split or you cannot afford the additional wager",
                Action::Insure => {
                    "insurance is not legal or you cannot afford the additional wager"
                }
            }));
        }
        let wager = match a {
            Action::Double | Action::Split => hand.bet,
            Action::Insure => hand.bet / 2,
            _ => 0,
        };
        if !matches!(a, Action::Hit | Action::Stand) && !valid_game_amount(wager) {
            return Err(BlackjackError::IllegalAction(
                "additional wager must be between $1 and $10,000",
            ));
        }
        Ok(wager)
    }

    fn record_decision(&mut self, action: Action) {
        if self.settings.bet_analyzer {
            self.decisions.push(BlackjackDecision {
                action,
                recommended: self.recommended_action(action),
            });
        }
    }

    fn recommended_action(&self, action: Action) -> Action {
        let i = self.active_index();
        let Some(hand) = self.hands.get(i) else {
            return Action::Stand;
        };
        if action == Action::Insure {
            return if count(&self.visible_cards(false)) >= 3 {
                Action::Insure
            } else {
                Action::Stand
            };
        }
        let dealer = self.dealer[0].rank as u8;
        if self.can_split(i64::MAX) && should_split(hand.cards[0].rank as u8, dealer) {
            return Action::Split;
        }
        let (total, soft) = score(&hand.cards);
        if hand.cards.len() == 2 && self.can_double(i64::MAX) && should_double(total, soft, dealer)
        {
            return Action::Double;
        }
        if should_hit(total, soft, dealer) {
            Action::Hit
        } else {
            Action::Stand
        }
    }

    fn can_hit(&self) -> bool {
        let i = self.active_index();
        self.status == BlackjackStatus::Playing && i < self.hands.len() && !self.hands[i].split_aces
    }

    fn can_stand(&self) -> bool {
        self.status == BlackjackStatus::Playing && self.active_index() < self.hands.len()
    }

    fn can_double(&self, balance: Cents) -> bool {
        let i = self.active_index();
        self.status == BlackjackStatus::Playing
            && i < self.hands.len()
            && self.hands[i].cards.len() == 2
            && !self.hands[i].split_aces
            && valid_game_amount(self.hands[i].bet)
            && balance >= self.hands[i].bet
    }

    fn can_split(&self, balance: Cents) -> bool {
        let i = self.active_index();
        self.status == BlackjackStatus::Playing
            && i < self.hands.len()
            && self.hands[i].cards.len() == 2
            && self.hands[i].cards[0].rank == self.hands[i].cards[1].rank
            && self.hands.len() < MAX_HANDS
            && valid_game_amount(self.hands[i].bet)
            && balance >= self.hands[i].bet
    }

    fn can_insure(&self, balance: Cents) -> bool {
        self.status == BlackjackStatus::Playing
            && !self.dealer_peeked
            && self.insurance == 0
            && self.hands.len() == 1
            && self.hands[0].cards.len() == 2
            && !self.hands[0].split
            && self.hands[0].status == BlackjackHandStatus::Playing
            && self.dealer[0].rank as u8 == 14
            && valid_game_amount(self.hands[0].bet / 2)
            && balance >= self.hands[0].bet / 2
    }

    fn peek_if_needed(&mut self) {
        if !self.dealer_peeked && self.dealer[0].rank as u8 == 14 {
            self.peek();
        }
    }

    fn peek(&mut self) {
        self.dealer_peeked = true;
        if score(&self.dealer).0 == 21 && self.dealer.len() == 2 {
            for h in &mut self.hands {
                h.status = if h.status == BlackjackHandStatus::Blackjack {
                    BlackjackHandStatus::Push
                } else {
                    BlackjackHandStatus::Loss
                };
            }
            self.payout = self.insurance_payout()
                + self
                    .hands
                    .iter()
                    .filter(|hand| hand.status == BlackjackHandStatus::Push)
                    .map(|hand| hand.bet)
                    .sum::<Cents>();
            self.status = if self
                .hands
                .iter()
                .all(|hand| hand.status == BlackjackHandStatus::Push)
            {
                BlackjackStatus::Push
            } else {
                BlackjackStatus::DealerWin
            };
        } else if self.hands[0].status == BlackjackHandStatus::Blackjack {
            self.payout = self.hands[0].bet * 5 / 2;
            self.status = BlackjackStatus::PlayerBlackjack;
        }
    }

    fn advance(&mut self) {
        if self
            .hands
            .iter()
            .any(|hand| hand.status == BlackjackHandStatus::Playing)
        {
            return;
        }
        while score(&self.dealer).0 < 17 {
            self.dealer.push(self.deck.deal().expect("card"));
        }
        let dealer_score = score(&self.dealer).0;
        for hand in &mut self.hands {
            if hand.status == BlackjackHandStatus::Stand {
                let player_score = score(&hand.cards).0;
                hand.status = if dealer_score > 21 || player_score > dealer_score {
                    BlackjackHandStatus::Win
                } else if player_score < dealer_score {
                    BlackjackHandStatus::Loss
                } else {
                    BlackjackHandStatus::Push
                };
            }
        }
        self.payout = self.insurance_payout()
            + self
                .hands
                .iter()
                .map(|hand| match hand.status {
                    BlackjackHandStatus::Win => hand.bet * 2,
                    BlackjackHandStatus::Push => hand.bet,
                    BlackjackHandStatus::Blackjack => hand.bet * 5 / 2,
                    _ => 0,
                })
                .sum::<Cents>();
        self.status = if self
            .hands
            .iter()
            .all(|hand| hand.status == BlackjackHandStatus::Bust)
        {
            BlackjackStatus::PlayerBust
        } else if dealer_score > 21 {
            BlackjackStatus::DealerBust
        } else if self
            .hands
            .iter()
            .any(|hand| hand.status == BlackjackHandStatus::Win)
        {
            BlackjackStatus::PlayerWin
        } else if self
            .hands
            .iter()
            .all(|hand| hand.status == BlackjackHandStatus::Push)
        {
            BlackjackStatus::Push
        } else {
            BlackjackStatus::DealerWin
        };
    }

    fn insurance_payout(&self) -> Cents {
        if self.insurance > 0 && score(&self.dealer).0 == 21 && self.dealer.len() == 2 {
            self.insurance * 3
        } else {
            0
        }
    }

    fn view(&self, reveal: bool, balance: Cents) -> BlackjackView {
        let finished = self.status != BlackjackStatus::Playing;
        let dealer = if reveal || finished {
            self.dealer.clone()
        } else {
            vec![self.dealer[0]]
        };
        let active = self.active_index();
        let hands = self
            .hands
            .iter()
            .map(|hand| BlackjackHandView {
                cards: hand.cards.clone(),
                bet: hand.bet,
                score: score(&hand.cards).0,
                status: hand.status,
                blackjack: hand.status == BlackjackHandStatus::Blackjack && !hand.split,
            })
            .collect();
        let first = self.hands.first().expect("hand");
        let visible = self.visible_cards(reveal || finished);
        let count_view = (self.settings.counting_tutor || self.settings.counting_quiz)
            .then(|| count_view(&visible, &self.settings));
        BlackjackView {
            id: self.id,
            bet: first.bet,
            player: first.cards.clone(),
            dealer,
            player_score: score(&first.cards).0,
            dealer_score: (reveal || finished).then_some(score(&self.dealer).0),
            status: self.status,
            message: message(self.status),
            payout: self.payout,
            can_hit: self.can_hit(),
            can_stand: self.can_stand(),
            can_double: self.can_double(balance),
            can_split: self.can_split(balance),
            can_insure: self.can_insure(balance),
            insurance: self.insurance,
            hands,
            active_hand: active,
            settings: self.settings.clone(),
            count: count_view,
            trainer_log: if self.settings.counting_tutor {
                count_log(&visible)
            } else {
                Vec::new()
            },
            quiz: if self.settings.counting_quiz && finished {
                Some(count_quiz(count(&visible)))
            } else {
                None
            },
            analysis: if self.settings.bet_analyzer {
                self.analysis()
            } else {
                Vec::new()
            },
        }
    }

    fn visible_cards(&self, reveal_dealer_hole: bool) -> Vec<(String, Card)> {
        let mut cards = Vec::new();
        if let Some(card) = self.dealer.first() {
            cards.push(("Dealer up".into(), *card));
        }
        if reveal_dealer_hole {
            for card in self.dealer.iter().skip(1) {
                cards.push(("Dealer".into(), *card));
            }
        }
        for (index, hand) in self.hands.iter().enumerate() {
            for card in &hand.cards {
                cards.push((format!("Hand {}", index + 1), *card));
            }
        }
        cards
    }

    fn analysis(&self) -> Vec<String> {
        self.decisions
            .iter()
            .filter_map(|decision| {
                (decision.action != decision.recommended).then(|| {
                    format!(
                        "{} was off; basic strategy prefers {} here.",
                        decision.action.label(),
                        decision.recommended.label()
                    )
                })
            })
            .collect()
    }
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

fn count_view(cards: &[(String, Card)], settings: &BlackjackTrainerSettings) -> BlackjackCountView {
    let running = count(cards);
    let shoe_cards = settings.shoe_cards();
    let visible_cards = cards.len();
    let remaining_cards = shoe_cards.saturating_sub(visible_cards).max(1);
    let decks_remaining = (remaining_cards as f32 / 52.0).max(0.25);
    BlackjackCountView {
        running,
        true_count: running as f32 / decks_remaining,
        visible_cards,
        penetration_percent: ((visible_cards * 100) / shoe_cards.max(1)) as u8,
    }
}

fn count_log(cards: &[(String, Card)]) -> Vec<String> {
    let mut running = 0;
    cards
        .iter()
        .map(|(label, card)| {
            let delta = count_value(*card);
            running += delta;
            let signed = if delta >= 0 {
                format!("+{delta}")
            } else {
                delta.to_string()
            };
            format!("{label} {card}: {signed} -> {running}")
        })
        .collect()
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
fn message(status: BlackjackStatus) -> String {
    match status {
        BlackjackStatus::Playing => "Hit or stand.",
        BlackjackStatus::PlayerBlackjack => "Blackjack pays 3:2.",
        BlackjackStatus::PlayerBust => "Bust.",
        BlackjackStatus::DealerBust => "Dealer busts.",
        BlackjackStatus::PlayerWin => "You win.",
        BlackjackStatus::DealerWin => "Dealer wins.",
        BlackjackStatus::Push => "Push.",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::{Rank, Suit};

    fn card(rank: Rank) -> Card {
        Card::new(rank, Suit::Spades)
    }

    fn hand(cards: Vec<Card>, bet: Cents, status: BlackjackHandStatus) -> BlackjackHand {
        BlackjackHand {
            cards,
            bet,
            status,
            split: false,
            split_aces: false,
        }
    }

    fn game(hands: Vec<BlackjackHand>, dealer: Vec<Card>) -> BlackjackGame {
        BlackjackGame {
            id: Uuid::new_v4(),
            user: Uuid::new_v4(),
            deck: Deck::seeded(7),
            hands,
            dealer,
            insurance: 0,
            dealer_peeked: false,
            status: BlackjackStatus::Playing,
            payout: 0,
            settings: BlackjackTrainerSettings::default(),
            decisions: Vec::new(),
        }
    }

    #[test]
    fn dealer_natural_and_player_natural_push() {
        let mut game = game(
            vec![hand(
                vec![card(Rank::Ace), card(Rank::King)],
                1_000,
                BlackjackHandStatus::Blackjack,
            )],
            vec![card(Rank::Ace), card(Rank::Queen)],
        );
        game.peek();
        assert_eq!(game.status, BlackjackStatus::Push);
        assert_eq!(game.payout, 1_000);
    }

    #[test]
    fn insurance_requires_two_dollar_bet_and_two_cards() {
        let mut game = game(
            vec![hand(
                vec![card(Rank::Ten), card(Rank::Nine)],
                100,
                BlackjackHandStatus::Playing,
            )],
            vec![card(Rank::Ace), card(Rank::Seven)],
        );
        assert!(!game.can_insure(0));
        game.hands[0].bet = 200;
        assert!(game.can_insure(100));
        game.hands[0].cards.push(card(Rank::Two));
        assert!(!game.can_insure(100));
    }

    #[test]
    fn split_and_double_use_same_legality_as_view() {
        let game = game(
            vec![hand(
                vec![card(Rank::Eight), card(Rank::Eight)],
                100,
                BlackjackHandStatus::Playing,
            )],
            vec![card(Rank::Nine), card(Rank::Seven)],
        );
        assert!(game.can_double(100));
        assert!(game.can_split(100));
        assert!(!game.can_double(99));
        assert!(!game.can_split(99));
        assert_eq!(game.wager(Action::Double, 100).unwrap(), 100);
        assert!(matches!(
            game.wager(Action::Double, 99),
            Err(BlackjackError::IllegalAction(message)) if message.contains("afford")
        ));
    }

    #[test]
    fn insurance_returns_three_to_one_when_dealer_has_blackjack() {
        let mut game = game(
            vec![hand(
                vec![card(Rank::Ten), card(Rank::Nine)],
                200,
                BlackjackHandStatus::Loss,
            )],
            vec![card(Rank::Ace), card(Rank::King)],
        );
        game.insurance = 100;
        assert_eq!(game.insurance_payout(), 300);
    }

    #[test]
    fn split_hand_with_21_is_not_a_blackjack() {
        let game = game(
            vec![hand(
                vec![card(Rank::Ace), card(Rank::King)],
                100,
                BlackjackHandStatus::Stand,
            )],
            vec![card(Rank::Nine), card(Rank::Seven)],
        );
        assert!(!game.view(false, 0).hands[0].blackjack);
    }

    #[test]
    fn bust_hand_has_no_payout() {
        let mut game = game(
            vec![hand(
                vec![card(Rank::King), card(Rank::Nine), card(Rank::Five)],
                100,
                BlackjackHandStatus::Bust,
            )],
            vec![card(Rank::Ten), card(Rank::Seven)],
        );
        game.advance();
        assert_eq!(game.payout, 0);
        assert_eq!(game.status, BlackjackStatus::PlayerBust);
    }

    #[test]
    fn aces_score_soft_until_they_must_drop() {
        let cards = vec![card(Rank::Ace), card(Rank::Nine), card(Rank::Five)];
        assert_eq!(score(&cards), (15, false));
    }

    #[test]
    fn stakes_leave_room_for_an_additional_wager() {
        assert_eq!(bet_options(100_000), vec![1_000, 5_000, 25_000, 50_000]);
        // A million-dollar roll caps the starting bet at half the bankroll.
        assert_eq!(
            bet_options(100_000_000),
            vec![10_000, 5_000_000, 25_000_000, 50_000_000]
        );
        assert_eq!(bet_options(100_000_000).first(), Some(&10_000));
        // Small rolls collapse toward a dollar while preserving room to double.
        assert_eq!(bet_options(300), vec![100]);
        assert_eq!(bet_options(100), vec![100]);
        assert!(bet_options(99).is_empty(), "you cannot bet what you lack");
        for balance in [100, 137, 900, 12_345, 1_000_000] {
            let bets = bet_options(balance);
            assert!(
                bets.iter().all(|bet| *bet <= balance),
                "never over the roll"
            );
            assert!(
                bets.windows(2).all(|pair| pair[0] < pair[1]),
                "sorted, no repeats"
            );
            assert_eq!(*bets.last().unwrap(), max_starting_bet(balance));
        }
    }

    #[tokio::test]
    async fn live_games_survive_persistence_but_finished_games_do_not() {
        let root = std::env::temp_dir().join(format!("two-seven-blackjack-{}", Uuid::new_v4()));
        let user = Uuid::new_v4();
        let store = BlackjackStore::load(&root).await.unwrap();
        // The deal is randomly seeded, so keep dealing until one survives it:
        // a natural blackjack resolves the game before it can be resumed.
        let id = {
            let mut live = None;
            for _ in 0..50 {
                let candidate = Uuid::new_v4();
                let view = store
                    .start(user, 500, candidate, 0, BlackjackTrainerSettings::default())
                    .await
                    .unwrap();
                if view.status == BlackjackStatus::Playing {
                    live = Some(candidate);
                    break;
                }
                store.inner.lock().await.clear();
            }
            live.expect("deal a hand that is still in play")
        };
        store.persist().await.unwrap();

        let restored = BlackjackStore::load(&root).await.unwrap();
        assert_eq!(restored.resume(user, 0).await.unwrap().id, id);

        {
            let mut guard = restored.inner.lock().await;
            guard.get_mut(&id).unwrap().status = BlackjackStatus::PlayerWin;
        }
        restored.persist().await.unwrap();
        let restored_again = BlackjackStore::load(&root).await.unwrap();
        assert!(restored_again.resume(user, 0).await.is_none());
    }

    #[tokio::test]
    async fn charged_wagers_use_remaining_balance_for_action_flags() {
        let store = BlackjackStore::new();
        let user = Uuid::new_v4();
        let split_id = Uuid::new_v4();
        let mut split_game = game(
            vec![hand(
                vec![card(Rank::Eight), card(Rank::Eight)],
                100,
                BlackjackHandStatus::Playing,
            )],
            vec![card(Rank::Nine), card(Rank::Seven)],
        );
        split_game.id = split_id;
        split_game.user = user;
        store.inner.lock().await.insert(split_id, split_game);

        let (view, wager) = store.split(user, split_id, 100).await.unwrap();
        assert_eq!(wager, 100);
        assert!(!view.can_double);
        assert!(!view.can_split);

        let insurance_id = Uuid::new_v4();
        let mut insurance_game = game(
            vec![hand(
                vec![card(Rank::Eight), card(Rank::Eight)],
                200,
                BlackjackHandStatus::Playing,
            )],
            vec![card(Rank::Ace), card(Rank::Seven)],
        );
        insurance_game.id = insurance_id;
        insurance_game.user = user;
        store
            .inner
            .lock()
            .await
            .insert(insurance_id, insurance_game);

        let (view, wager) = store.insure(user, insurance_id, 200).await.unwrap();
        assert_eq!(wager, 100);
        assert!(!view.can_double);
        assert!(!view.can_split);
    }

    #[test]
    fn counting_tutor_uses_visible_cards_until_reveal() {
        let mut game = game(
            vec![hand(
                vec![card(Rank::Two), card(Rank::King)],
                100,
                BlackjackHandStatus::Playing,
            )],
            vec![card(Rank::Six), card(Rank::Ace)],
        );
        game.settings.counting_tutor = true;
        game.settings.counting_quiz = true;
        let hidden = game.view(false, 0);
        assert_eq!(hidden.count.as_ref().unwrap().running, 1);
        assert!(
            hidden
                .trainer_log
                .iter()
                .any(|line| line.contains("Dealer up")),
            "dealer upcard is counted"
        );
        assert!(
            hidden.trainer_log.iter().all(|line| !line.contains("As")),
            "dealer hole card is hidden from the count"
        );
        assert!(hidden.quiz.is_none());

        game.hands[0].status = BlackjackHandStatus::Stand;
        game.advance();
        let revealed = game.view(false, 0);
        assert_eq!(revealed.count.as_ref().unwrap().running, 0);
        assert_eq!(revealed.quiz.as_ref().unwrap().answer, 0);
    }

    #[test]
    fn analyzer_reports_basic_strategy_misses() {
        let mut game = game(
            vec![hand(
                vec![card(Rank::Ten), card(Rank::Six)],
                100,
                BlackjackHandStatus::Playing,
            )],
            vec![card(Rank::Ten), card(Rank::Seven)],
        );
        game.settings.bet_analyzer = true;
        game.record_decision(Action::Stand);
        assert!(
            game.view(false, 0).analysis[0].contains("Hit"),
            "16 against 10 should prefer a hit"
        );
    }

    #[test]
    fn analyzer_keeps_insurance_separate_from_hand_strategy_v40() {
        let mut game = game(
            vec![hand(
                vec![card(Rank::Two), card(Rank::Three)],
                100,
                BlackjackHandStatus::Playing,
            )],
            vec![card(Rank::Ace), card(Rank::Ten)],
        );
        game.settings.bet_analyzer = true;
        game.record_decision(Action::Hit);
        assert!(
            game.view(false, 0).analysis.is_empty(),
            "hard 5 should hit even when insurance was available"
        );
    }
}
