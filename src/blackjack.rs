use crate::{
    cards::{Card, Deck},
    money::{Cents, valid_game_amount},
};
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, broadcast};
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
    pub shoe: BlackjackShoeView,
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
    #[serde(default = "default_penetration_percent", alias = "penetration_hands")]
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
            penetration_percent: self.penetration_percent.clamp(25, 90),
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
    shoes: Arc<Mutex<HashMap<Uuid, BlackjackShoe>>>,
    path: Option<PathBuf>,
    shoes_path: Option<PathBuf>,
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
    #[serde(default)]
    base_count: i16,
    #[serde(default)]
    base_exposed_cards: usize,
    #[serde(default)]
    fresh_shuffle: bool,
    /// Set once this round has been folded into the player's record, so an
    /// action taken against an already-settled game cannot count it twice.
    #[serde(default)]
    recorded: bool,
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

fn default_decks() -> u8 {
    8
}

fn default_penetration_percent() -> u8 {
    50
}

const MAX_HANDS: usize = 4;
const SAFE_RESERVE_CARDS: usize = 20;

impl Default for BlackjackStore {
    fn default() -> Self {
        let (changed, _) = broadcast::channel(32);
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            shoes: Arc::new(Mutex::new(HashMap::new())),
            path: None,
            shoes_path: None,
            tables: Arc::new(Mutex::new(
                (0..TIER_MAX_BETS.len()).map(BlackjackTable::new).collect(),
            )),
            tables_path: None,
            changed,
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
        let shoes_path = dir.join("shoes.json");
        let games = match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice::<HashMap<Uuid, BlackjackGame>>(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => return Err(error.into()),
        };
        let shoes = match tokio::fs::read(&shoes_path).await {
            Ok(bytes) => serde_json::from_slice::<HashMap<Uuid, BlackjackShoe>>(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => return Err(error.into()),
        };
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
                    seat.hands.clear();
                }
                table.phase = Phase::Betting;
                table.dealer.clear();
                table.current = None;
                table.deadline = None;
            }
        }
        tables.sort_by_key(|table| table.tier);
        let (changed, _) = broadcast::channel(32);
        Ok(Self {
            inner: Arc::new(Mutex::new(
                games
                    .into_iter()
                    .filter(|(_, game)| game.status == BlackjackStatus::Playing)
                    .collect(),
            )),
            shoes: Arc::new(Mutex::new(shoes)),
            path: Some(path),
            shoes_path: Some(shoes_path),
            tables: Arc::new(Mutex::new(tables)),
            tables_path: Some(tables_path),
            changed,
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
        let shoes = self.shoes.lock().await.clone();
        let data = serde_json::to_vec_pretty(&games)?;
        let tmp = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
        tokio::fs::write(&tmp, data).await?;
        tokio::fs::rename(tmp, path).await?;
        if let Some(shoes_path) = &self.shoes_path {
            let data = serde_json::to_vec_pretty(&shoes)?;
            let tmp = shoes_path.with_extension(format!("tmp-{}", Uuid::new_v4()));
            tokio::fs::write(&tmp, data).await?;
            tokio::fs::rename(tmp, shoes_path).await?;
        }
        if let Some(tables_path) = &self.tables_path {
            let tables = self.tables.lock().await.clone();
            let data = serde_json::to_vec_pretty(&tables)?;
            let tmp = tables_path.with_extension(format!("tmp-{}", Uuid::new_v4()));
            tokio::fs::write(&tmp, data).await?;
            tokio::fs::rename(tmp, tables_path).await?;
        }
        Ok(())
    }

    pub async fn view(
        &self,
        user: Uuid,
        id: Uuid,
        balance: Cents,
    ) -> Result<BlackjackView, BlackjackError> {
        let guard = self.inner.lock().await;
        let shoes = self.shoes.lock().await;
        let game = guard.get(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        Ok(game.view(false, balance, shoes.get(&user)))
    }
    pub async fn resume(&self, user: Uuid, balance: Cents) -> Option<BlackjackView> {
        let guard = self.inner.lock().await;
        let shoes = self.shoes.lock().await;
        guard
            .values()
            .find(|game| game.user == user && game.status == BlackjackStatus::Playing)
            .map(|game| game.view(false, balance, shoes.get(&user)))
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
        let mut shoes = self.shoes.lock().await;
        if guard
            .values()
            .any(|game| game.user == user && game.status == BlackjackStatus::Playing)
        {
            return Err(BlackjackError::ActiveGame);
        }
        guard.retain(|_, game| game.user != user || game.status == BlackjackStatus::Playing);
        let settings = settings.sanitized();
        let mut fresh_shuffle = !shoes.contains_key(&user);
        let shoe = shoes
            .entry(user)
            .or_insert_with(|| BlackjackShoe::new(settings.decks, settings.penetration_percent));
        let effective_cut = cut_card(shoe.deck.total(), settings.penetration_percent);
        if shoe.decks != settings.decks
            || shoe.dealt_cards() >= effective_cut
            || shoe.deck.remaining() < SAFE_RESERVE_CARDS
        {
            *shoe = BlackjackShoe::new(settings.decks, settings.penetration_percent);
            fresh_shuffle = true;
        } else {
            shoe.cut_card = effective_cut;
        }
        let base_count = shoe.running_count;
        let base_exposed_cards = shoe.exposed_cards;
        shoe.hands_dealt += 1;
        let mut deck = shoe.deck.clone();
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
                doubled: false,
            }],
            base_count,
            base_exposed_cards,
            fresh_shuffle,
            recorded: false,
        };
        if !game.can_insure(balance) {
            game.peek();
        }
        if game.status != BlackjackStatus::Playing {
            shoe.settle(&game);
        }
        let view = game.view(false, balance, Some(shoe));
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
        let mut shoes = self.shoes.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        game.wager(Action::Hit, balance)?;
        game.record_decision(Action::Hit);
        game.peek_if_needed();
        if game.status != BlackjackStatus::Playing {
            settle_game(&mut shoes, game);
            return Ok(game.view(false, balance, shoes.get(&user)));
        }
        let i = game.active_index();
        let card = game.deal_card();
        game.hands[i].cards.push(card);
        if score(&game.hands[i].cards).0 > 21 {
            game.hands[i].status = BlackjackHandStatus::Bust;
            game.advance();
        }
        if game.status != BlackjackStatus::Playing {
            settle_game(&mut shoes, game);
        }
        Ok(game.view(false, balance, shoes.get(&user)))
    }

    pub async fn stand(
        &self,
        user: Uuid,
        id: Uuid,
        balance: Cents,
    ) -> Result<BlackjackView, BlackjackError> {
        let mut guard = self.inner.lock().await;
        let mut shoes = self.shoes.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        game.wager(Action::Stand, balance)?;
        game.record_decision(Action::Stand);
        game.peek_if_needed();
        if game.status != BlackjackStatus::Playing {
            settle_game(&mut shoes, game);
            return Ok(game.view(true, balance, shoes.get(&user)));
        }
        let i = game.active_index();
        game.hands[i].status = BlackjackHandStatus::Stand;
        game.advance();
        if game.status != BlackjackStatus::Playing {
            settle_game(&mut shoes, game);
        }
        Ok(game.view(true, balance, shoes.get(&user)))
    }

    pub async fn double(
        &self,
        user: Uuid,
        id: Uuid,
        balance: Cents,
    ) -> Result<(BlackjackView, Cents), BlackjackError> {
        let mut guard = self.inner.lock().await;
        let mut shoes = self.shoes.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        let wager = game.wager(Action::Double, balance)?;
        game.record_decision(Action::Double);
        game.peek_if_needed();
        if game.status != BlackjackStatus::Playing {
            settle_game(&mut shoes, game);
            return Ok((game.view(false, balance, shoes.get(&user)), 0));
        }
        let i = game.active_index();
        game.hands[i].bet += wager;
        game.hands[i].doubled = true;
        let card = game.deal_card();
        game.hands[i].cards.push(card);
        game.hands[i].status = if score(&game.hands[i].cards).0 > 21 {
            BlackjackHandStatus::Bust
        } else {
            BlackjackHandStatus::Stand
        };
        game.advance();
        // The stake just placed is still in the balance the route read.
        if game.status != BlackjackStatus::Playing {
            settle_game(&mut shoes, game);
        }
        Ok((game.view(true, balance - wager, shoes.get(&user)), wager))
    }

    pub async fn split(
        &self,
        user: Uuid,
        id: Uuid,
        balance: Cents,
    ) -> Result<(BlackjackView, Cents), BlackjackError> {
        let mut guard = self.inner.lock().await;
        let mut shoes = self.shoes.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        let wager = game.wager(Action::Split, balance)?;
        game.record_decision(Action::Split);
        game.peek_if_needed();
        if game.status != BlackjackStatus::Playing {
            settle_game(&mut shoes, game);
            return Ok((game.view(false, balance, shoes.get(&user)), 0));
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
            doubled: false,
        };
        game.hands[i].split = true;
        game.hands[i].split_aces = split_aces;
        game.hands.insert(i + 1, second);
        let first_card = game.deal_card();
        let second_card = game.deal_card();
        game.hands[i].cards.push(first_card);
        game.hands[i + 1].cards.push(second_card);
        if split_aces {
            game.hands[i].status = BlackjackHandStatus::Stand;
            game.hands[i + 1].status = BlackjackHandStatus::Stand;
            game.advance();
        }
        if game.status != BlackjackStatus::Playing {
            settle_game(&mut shoes, game);
        }
        Ok((game.view(false, balance - wager, shoes.get(&user)), wager))
    }

    pub async fn insure(
        &self,
        user: Uuid,
        id: Uuid,
        balance: Cents,
    ) -> Result<(BlackjackView, Cents), BlackjackError> {
        let mut guard = self.inner.lock().await;
        let mut shoes = self.shoes.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        let wager = game.wager(Action::Insure, balance)?;
        game.record_decision(Action::Insure);
        game.insurance = wager;
        game.peek_if_needed();
        if game.status != BlackjackStatus::Playing {
            settle_game(&mut shoes, game);
        }
        Ok((game.view(false, balance - wager, shoes.get(&user)), wager))
    }
}

impl BlackjackShoe {
    pub fn table_default() -> Self {
        Self::new(8, 50)
    }

    fn new(decks: u8, penetration_percent: u8) -> Self {
        let decks = decks.max(1);
        let total_cards = usize::from(decks) * 52;
        Self {
            decks,
            deck: Deck::shoe_seeded(rand::thread_rng().r#gen(), decks),
            cut_card: cut_card(total_cards, penetration_percent),
            hands_dealt: 0,
            running_count: 0,
            exposed_cards: 0,
            fresh_shuffle: false,
        }
    }

    fn dealt_cards(&self) -> usize {
        self.deck.dealt()
    }

    fn settle(&mut self, game: &BlackjackGame) {
        let visible = game.visible_cards(true);
        self.deck = game.deck.clone();
        self.running_count = game.base_count + count(&visible);
        self.exposed_cards = game.base_exposed_cards + visible.len();
    }

    fn view(&self, game: &BlackjackGame, settings: &BlackjackTrainerSettings) -> BlackjackShoeView {
        let total_cards = settings.shoe_cards();
        let remaining_cards = game.deck.remaining();
        BlackjackShoeView {
            decks: self.decks,
            total_cards,
            dealt_cards: total_cards.saturating_sub(remaining_cards),
            remaining_cards,
            cut_card: self.cut_card,
            penetration_percent: settings.penetration_percent,
            hands_dealt: self.hands_dealt,
            fresh_shuffle: game.fresh_shuffle,
        }
    }
}

impl BlackjackShoeView {
    fn from_game(game: &BlackjackGame, settings: &BlackjackTrainerSettings) -> Self {
        let total_cards = settings.shoe_cards();
        let remaining_cards = game.deck.remaining();
        Self {
            decks: settings.decks,
            total_cards,
            dealt_cards: total_cards.saturating_sub(remaining_cards),
            remaining_cards,
            cut_card: cut_card(total_cards, settings.penetration_percent),
            penetration_percent: settings.penetration_percent,
            hands_dealt: 1,
            fresh_shuffle: game.fresh_shuffle,
        }
    }
}

fn cut_card(total_cards: usize, penetration_percent: u8) -> usize {
    let maximum_cut = total_cards.saturating_sub(SAFE_RESERVE_CARDS);
    ((total_cards * usize::from(penetration_percent) + 50) / 100).clamp(4, maximum_cut.max(4))
}

fn settle_game(shoes: &mut HashMap<Uuid, BlackjackShoe>, game: &BlackjackGame) {
    if let Some(shoe) = shoes.get_mut(&game.user) {
        shoe.settle(game);
    }
}

impl BlackjackStore {
    /// The outcome of a finished round, once and only once.
    ///
    /// Called by every route that can end a hand. Settlement happens deep
    /// inside the game rather than at one seam, so rather than thread a return
    /// value through six methods, the round is claimed here and flagged as
    /// counted.
    pub async fn take_settlement(
        &self,
        user: Uuid,
        id: Uuid,
    ) -> Option<crate::blackjack_stats::RoundOutcome> {
        let mut guard = self.inner.lock().await;
        let game = guard.get_mut(&id)?;
        if game.user != user || game.recorded || game.status == BlackjackStatus::Playing {
            return None;
        }
        game.recorded = true;
        Some(game.outcome())
    }
}

impl BlackjackGame {
    /// What this round cost and paid, and how it got there.
    fn outcome(&self) -> crate::blackjack_stats::RoundOutcome {
        let mut outcome = crate::blackjack_stats::RoundOutcome {
            hands: self.hands.len() as u64,
            // The first hand is dealt, not split; every one after it is.
            splits: self.hands.len().saturating_sub(1) as u64,
            insured: self.insurance > 0,
            wagered: self.hands.iter().map(|hand| hand.bet).sum::<Cents>() + self.insurance,
            returned: self.payout,
            ..Default::default()
        };
        for hand in &self.hands {
            match hand.status {
                BlackjackHandStatus::Blackjack => {
                    outcome.won += 1;
                    outcome.naturals += 1;
                }
                BlackjackHandStatus::Win => outcome.won += 1,
                BlackjackHandStatus::Push => outcome.push += 1,
                BlackjackHandStatus::Bust => {
                    outcome.lost += 1;
                    outcome.busts += 1;
                }
                _ => outcome.lost += 1,
            }
            // A doubled hand carries twice the stake it was dealt with, and
            // is the only way a hand of three cards can stop on its own.
            if hand.doubled {
                outcome.doubles += 1;
            }
        }
        outcome
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
impl BlackjackGame {
    fn check_user(&self, user: Uuid) -> Result<(), BlackjackError> {
        (self.user == user)
            .then_some(())
            .ok_or(BlackjackError::NotFound)
    }

    fn deal_card(&mut self) -> Card {
        if let Some(card) = self.deck.deal() {
            return card;
        }
        self.deck = Deck::shoe_seeded(rand::thread_rng().r#gen(), self.settings.decks);
        self.base_count = 0;
        self.base_exposed_cards = 0;
        self.fresh_shuffle = true;
        self.deck.deal().expect("fresh shoe has a card")
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
            let card = self.deal_card();
            self.dealer.push(card);
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

    fn view(&self, reveal: bool, balance: Cents, shoe: Option<&BlackjackShoe>) -> BlackjackView {
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
        let running = self.base_count + count(&visible);
        let exposed_cards = self.base_exposed_cards + visible.len();
        let dealt_cards = self
            .settings
            .shoe_cards()
            .saturating_sub(self.deck.remaining());
        let count_view = (self.settings.counting_tutor || self.settings.counting_quiz)
            .then(|| count_view(running, exposed_cards, dealt_cards, &self.settings));
        let shoe_view = shoe
            .map(|shoe| shoe.view(self, &self.settings))
            .unwrap_or_else(|| BlackjackShoeView::from_game(self, &self.settings));
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
                count_log(&visible, self.base_count)
            } else {
                Vec::new()
            },
            quiz: if self.settings.counting_quiz && finished {
                Some(count_quiz(running))
            } else {
                None
            },
            analysis: if self.settings.bet_analyzer {
                self.analysis()
            } else {
                Vec::new()
            },
            shoe: shoe_view,
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
            .filter(|decision| decision.action != decision.recommended)
            .map(|decision| {
                format!(
                    "{} was off; basic strategy prefers {} here.",
                    decision.action.label(),
                    decision.recommended.label()
                )
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

fn count_view(
    running: i16,
    exposed_cards: usize,
    dealt_cards: usize,
    settings: &BlackjackTrainerSettings,
) -> BlackjackCountView {
    let shoe_cards = settings.shoe_cards();
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

pub const TIER_MAX_BETS: [Cents; 4] = [10_000, 100_000, 1_000_000, 10_000_000];
pub const SEAT_COUNT: usize = 5;
pub const TURN_SECONDS: i64 = crate::table::TURN_SECONDS;
pub const RESULT_PAUSE_SECONDS: i64 = 5;

pub fn buy_in_for(max_bet: Cents) -> Cents {
    max_bet * 10
}

pub fn table_bet_options(max_bet: Cents) -> [Cents; 4] {
    [max_bet / 4, max_bet / 2, max_bet * 3 / 4, max_bet]
}

/// The table wager buttons.  Kept distinct from the legacy bankroll helper
/// above so old persisted trainer sessions remain readable during migration.
pub fn fixed_bet_options(max_bet: Cents) -> [Cents; 4] {
    table_bet_options(max_bet)
}

pub fn table_id(tier: usize) -> Uuid {
    let name = format!("two-seven/blackjack/{tier}");
    let mut input = Vec::with_capacity(16 + name.len());
    input.extend_from_slice(Uuid::NAMESPACE_OID.as_bytes());
    input.extend_from_slice(name.as_bytes());
    let digest = sha1_digest(&input);
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn sha1_digest(input: &[u8]) -> [u8; 20] {
    let bit_len = (input.len() as u64) * 8;
    let padded_len = (input.len() + 9).div_ceil(64) * 64;
    let mut message = vec![0; padded_len];
    message[..input.len()].copy_from_slice(input);
    message[input.len()] = 0x80;
    message[padded_len - 8..].copy_from_slice(&bit_len.to_be_bytes());

    let mut state = [
        0x6745_2301u32,
        0xefcd_ab89,
        0x98ba_dcfe,
        0x1032_5476,
        0xc3d2_e1f0,
    ];
    for chunk in message.chunks_exact(64) {
        let mut words = [0u32; 80];
        for (index, word) in chunk.chunks_exact(4).enumerate() {
            words[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) =
            (state[0], state[1], state[2], state[3], state[4]);
        for (index, word) in words.iter().enumerate() {
            let (function, constant) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(function)
                .wrapping_add(e)
                .wrapping_add(constant)
                .wrapping_add(*word);
            (a, b, c, d, e) = (temp, a, b.rotate_left(30), c, d);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
    let mut digest = [0; 20];
    for (index, value) in state.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    digest
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
        table_bet_options(max_bet)
    }

    pub fn new(tier: usize) -> Self {
        let max_bet = TIER_MAX_BETS.get(tier).copied().unwrap_or(TIER_MAX_BETS[0]);
        Self {
            id: table_id(tier),
            tier,
            max_bet,
            seats: vec![None; SEAT_COUNT],
            shoe: BlackjackShoe::new(8, 50),
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

    fn finish_pause(&mut self, now: DateTime<Utc>) {
        if self.phase == Phase::Settled && self.deadline.is_some_and(|at| at <= now) {
            self.phase = Phase::Betting;
            self.deadline = None;
            self.last_results.clear();
            self.dealer.clear();
            self.dealer_peeked = false;
        }
    }

    pub fn place_bet(
        &mut self,
        user: Uuid,
        amount: Cents,
        now: DateTime<Utc>,
    ) -> Result<Vec<BlackjackSettlement>, BlackjackError> {
        self.updated_at = now;
        self.finish_pause(now);
        let seat_index = self.seat_of(user).ok_or(BlackjackError::NotFound)?;
        if self.phase != Phase::Betting {
            return Err(BlackjackError::IllegalAction("betting is closed"));
        }
        if !table_bet_options(self.max_bet).contains(&amount) {
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
            self.shoe = BlackjackShoe::new(8, 50);
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
                if score(&hand.cards).0 > 21 {
                    hand.status = BlackjackHandStatus::Bust;
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
            Phase::Betting => {
                for seat in self.seats.iter_mut().flatten() {
                    if !seat.leaving && seat.bet.is_none() {
                        seat.hands.clear();
                    }
                }
                self.deal(now)
            }
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
                self.finish_pause(now);
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
            seat.insurance = 0;
            seat.insurance_decided = false;
            seat.hands.clear();
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
                    waiting: self.phase == Phase::Betting && seat.bet.is_none(),
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
            let count_settings = BlackjackTrainerSettings {
                decks: 8,
                penetration_percent: 50,
                ..seat.settings.clone()
            };
            Some(BlackjackTrainerView {
                count: (seat.settings.counting_tutor || seat.settings.counting_quiz)
                    .then(|| count_view(running, cards.len(), shoe.dealt_cards, &count_settings)),
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
            bet_options: table_bet_options(self.max_bet),
            min_bet: self.max_bet / 4,
            seat_count: SEAT_COUNT,
            phase: self.phase,
            dealer: if self.phase == Phase::Playing || self.phase == Phase::Insurance {
                self.dealer.first().copied().into_iter().collect()
            } else {
                self.dealer.clone()
            },
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
                self.phase == Phase::Betting
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

    pub async fn get_view(
        &self,
        id: Uuid,
        viewer: Option<Uuid>,
    ) -> Result<BlackjackTableView, BlackjackError> {
        let tables = self.tables.lock().await;
        let table = tables
            .iter()
            .find(|table| table.id == id)
            .ok_or(BlackjackError::NotFound)?;
        let mut view = table.view(viewer, 0);
        view.can_join =
            viewer.is_some_and(|user| !tables.iter().any(|table| table.seat_of(user).is_some()));
        Ok(view)
    }

    pub async fn view_with_balance(
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
            let immediate = seat.bet.is_none() && seat.hands.is_empty();
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
            if seat.bet.is_some() || !seat.hands.is_empty() {
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
            let leaving = {
                let tables = self.tables.lock().await;
                tables
                    .iter()
                    .find(|table| table.id == id)
                    .and_then(|table| {
                        table.seat_of(settlement.user).map(|index| {
                            table.seats[index].as_ref().is_some_and(|seat| seat.leaving)
                        })
                    })
                    .unwrap_or(false)
            };
            if leaving {
                let amount = {
                    let mut tables = self.tables.lock().await;
                    let table = tables
                        .iter_mut()
                        .find(|table| table.id == id)
                        .expect("table");
                    let index = table.seat_of(settlement.user).expect("seat");
                    let amount = table.seats[index].as_ref().expect("seat").stack;
                    table.seats[index] = None;
                    amount
                };
                let _ = bank
                    .blackjack_cash_out(
                        crate::bank::AccountOwner::User(settlement.user),
                        id,
                        amount,
                    )
                    .await;
            }
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
    pub async fn insure_table(
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
            doubled: false,
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
            base_count: 0,
            base_exposed_cards: 0,
            fresh_shuffle: false,
            recorded: false,
        }
    }

    fn non_zero_finished_count_seed() -> u64 {
        (0..)
            .find(|seed| {
                let mut deck = Deck::shoe_seeded(*seed, 1);
                let player = vec![deck.deal().unwrap(), deck.deal().unwrap()];
                let dealer = vec![deck.deal().unwrap(), deck.deal().unwrap()];
                if score(&player).0 == 21 || score(&dealer).0 == 21 {
                    return false;
                }
                let mut game = game(
                    vec![hand(player, 100, BlackjackHandStatus::Playing)],
                    dealer,
                );
                game.deck = deck;
                game.peek();
                if game.status != BlackjackStatus::Playing {
                    return false;
                }
                game.hands[0].status = BlackjackHandStatus::Stand;
                game.advance();
                count(&game.visible_cards(true)) != 0
            })
            .unwrap()
    }

    #[test]
    fn shared_tables_have_stable_uuid_v5_ids_and_fixed_wagers() {
        assert_eq!(
            table_id(0),
            "75429ffc-5308-5446-948a-16e1be466313"
                .parse::<Uuid>()
                .unwrap()
        );
        assert_eq!(
            BlackjackTable::bet_options(10_000),
            [2_500, 5_000, 7_500, 10_000]
        );
        assert_eq!(buy_in_for(TIER_MAX_BETS[3]), 100_000_000);
    }

    #[test]
    fn loaded_round_refund_includes_doubles_and_insurance() {
        let user = Uuid::new_v4();
        let mut table = BlackjackTable::new(0);
        table.seats[0] = Some(BlackjackSeat {
            user,
            stack: 700,
            bet: Some(100),
            hands: vec![hand(
                vec![card(Rank::Eight), card(Rank::Eight)],
                200,
                BlackjackHandStatus::Playing,
            )],
            insurance: 50,
            insurance_decided: true,
            leaving: false,
            settings: BlackjackTrainerSettings::default(),
            decisions: Vec::new(),
        });
        for seat in table.seats.iter_mut().flatten() {
            seat.stack += seat.hands.iter().map(|hand| hand.bet).sum::<Cents>() + seat.insurance;
            seat.bet = None;
            seat.insurance = 0;
            seat.hands.clear();
        }
        assert_eq!(table.seats[0].as_ref().unwrap().stack, 950);
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
        assert!(!game.view(false, 0, None).hands[0].blackjack);
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
        let hidden = game.view(false, 0, None);
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
        let revealed = game.view(false, 0, None);
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
            game.view(false, 0, None).analysis[0].contains("Hit"),
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
            game.view(false, 0, None).analysis.is_empty(),
            "hard 5 should hit even when insurance was available"
        );
    }

    #[test]
    fn settled_count_carries_into_the_next_hand() {
        let mut shoe = BlackjackShoe::new(1, 50);
        let mut first = game(
            vec![hand(
                vec![card(Rank::Two), card(Rank::King)],
                100,
                BlackjackHandStatus::Stand,
            )],
            vec![card(Rank::Six), card(Rank::Nine)],
        );
        first.base_count = 0;
        first.base_exposed_cards = 0;
        shoe.hands_dealt = 1;
        shoe.settle(&first);
        let carried = shoe.running_count;
        let mut second = game(
            vec![hand(
                vec![card(Rank::Five), card(Rank::Ten)],
                100,
                BlackjackHandStatus::Playing,
            )],
            vec![card(Rank::Seven), card(Rank::Ace)],
        );
        second.base_count = shoe.running_count;
        second.base_exposed_cards = shoe.exposed_cards;
        second.settings.counting_tutor = true;
        let view = second.view(false, 0, Some(&shoe));
        assert_eq!(view.count.unwrap().running, carried);
    }

    #[tokio::test]
    async fn cut_card_reshuffle_resets_count_and_dealt_cards() {
        let store = BlackjackStore::new();
        let user = Uuid::new_v4();
        let mut shoe = BlackjackShoe::new(1, 25);
        shoe.running_count = 4;
        shoe.exposed_cards = 4;
        for _ in 0..13 {
            shoe.deck.deal();
        }
        store.shoes.lock().await.insert(user, shoe);
        let view = store
            .start(
                user,
                100,
                Uuid::new_v4(),
                0,
                BlackjackTrainerSettings {
                    decks: 1,
                    penetration_percent: 25,
                    counting_tutor: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(view.shoe.fresh_shuffle);
        assert_eq!(view.shoe.dealt_cards, 4);
        assert_eq!(view.shoe.remaining_cards, 48);
        assert_eq!(
            store.inner.lock().await.values().next().unwrap().base_count,
            0
        );
    }

    #[tokio::test]
    async fn store_count_continuity_and_dealt_cards_span_finished_hands() {
        let store = BlackjackStore::new();
        let user = Uuid::new_v4();
        let seed = non_zero_finished_count_seed();
        let mut shoe = BlackjackShoe::new(1, 50);
        shoe.deck = Deck::shoe_seeded(seed, 1);
        store.shoes.lock().await.insert(user, shoe);
        let settings = BlackjackTrainerSettings {
            decks: 1,
            counting_tutor: true,
            ..Default::default()
        };

        let first = store
            .start(user, 100, Uuid::new_v4(), 0, settings.clone())
            .await
            .unwrap();
        let first = if first.status == BlackjackStatus::Playing {
            store.stand(user, first.id, 0).await.unwrap()
        } else {
            first
        };
        let first_running = first.count.as_ref().unwrap().running;
        assert_ne!(first_running, 0);
        let first_dealt = first.shoe.dealt_cards;

        let second = store
            .start(user, 100, Uuid::new_v4(), 0, settings)
            .await
            .unwrap();
        let current_visible = second
            .player
            .iter()
            .chain(second.dealer.iter())
            .map(|card| ("current".to_string(), *card))
            .collect::<Vec<_>>();
        let carried = second.count.as_ref().unwrap().running - count(&current_visible);
        assert_eq!(carried, first_running);
        assert_eq!(second.shoe.dealt_cards, first_dealt + 4);
        assert_eq!(second.shoe.hands_dealt, 2);
    }

    #[tokio::test]
    async fn persisted_shoe_keeps_settled_count_and_dealt_cards() {
        let root = std::env::temp_dir().join(format!("two-seven-blackjack-{}", Uuid::new_v4()));
        let user = Uuid::new_v4();
        let seed = non_zero_finished_count_seed();
        let store = BlackjackStore::load(&root).await.unwrap();
        let mut shoe = BlackjackShoe::new(1, 50);
        shoe.deck = Deck::shoe_seeded(seed, 1);
        store.shoes.lock().await.insert(user, shoe);

        let started = store
            .start(
                user,
                100,
                Uuid::new_v4(),
                0,
                BlackjackTrainerSettings {
                    decks: 1,
                    counting_tutor: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let finished = if started.status == BlackjackStatus::Playing {
            store.stand(user, started.id, 0).await.unwrap()
        } else {
            started
        };
        let expected_count = finished.count.as_ref().unwrap().running;
        let expected_dealt = finished.shoe.dealt_cards;
        store.persist().await.unwrap();

        let restored = BlackjackStore::load(&root).await.unwrap();
        let restored_shoe = restored.shoes.lock().await.get(&user).cloned().unwrap();
        assert_eq!(restored_shoe.running_count, expected_count);
        assert_eq!(
            restored_shoe.exposed_cards,
            finished.count.unwrap().visible_cards
        );
        assert_eq!(restored_shoe.dealt_cards(), expected_dealt);
    }

    #[tokio::test]
    async fn changing_deck_count_reshuffles_the_shoe() {
        let store = BlackjackStore::new();
        let user = Uuid::new_v4();
        store
            .shoes
            .lock()
            .await
            .insert(user, BlackjackShoe::new(1, 50));
        let view = store
            .start(
                user,
                100,
                Uuid::new_v4(),
                0,
                BlackjackTrainerSettings {
                    decks: 2,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(view.shoe.fresh_shuffle);
        assert_eq!(view.shoe.decks, 2);
        assert_eq!(view.shoe.remaining_cards, 100);
    }

    #[test]
    fn settling_twice_does_not_double_count() {
        let mut shoe = BlackjackShoe::new(1, 50);
        let game = game(
            vec![hand(
                vec![card(Rank::Two), card(Rank::King)],
                100,
                BlackjackHandStatus::Stand,
            )],
            vec![card(Rank::Six), card(Rank::Nine)],
        );
        shoe.settle(&game);
        let first = (shoe.running_count, shoe.exposed_cards);
        shoe.settle(&game);
        assert_eq!((shoe.running_count, shoe.exposed_cards), first);
    }

    #[test]
    fn exhausted_hand_deal_reshuffles_instead_of_panicking() {
        let mut game = game(
            vec![hand(
                vec![card(Rank::Two), card(Rank::Three)],
                100,
                BlackjackHandStatus::Playing,
            )],
            vec![card(Rank::Six), card(Rank::Nine)],
        );
        game.settings.decks = 1;
        game.deck = Deck::shoe_seeded(1, 1);
        while game.deck.deal().is_some() {}
        let _ = game.deal_card();
        assert!(game.fresh_shuffle);
        assert_eq!(game.base_count, 0);
        assert_eq!(game.base_exposed_cards, 0);
    }
}
