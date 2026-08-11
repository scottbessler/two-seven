use crate::{
    cards::{Card, Deck},
    money::{Cents, valid_game_amount},
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

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
}
#[derive(Clone, Debug, Serialize)]
pub struct BlackjackHandView {
    pub cards: Vec<Card>,
    pub bet: Cents,
    pub score: u8,
    pub status: BlackjackHandStatus,
    pub blackjack: bool,
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
}
#[derive(Clone, Debug)]
struct BlackjackHand {
    cards: Vec<Card>,
    bet: Cents,
    status: BlackjackHandStatus,
    split: bool,
    split_aces: bool,
}
#[derive(Clone, Debug)]
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
}
const MAX_HANDS: usize = 4;

impl Default for BlackjackStore {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
impl BlackjackStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub async fn view(&self, user: Uuid, id: Uuid) -> Result<BlackjackView, BlackjackError> {
        let g = self.inner.lock().await;
        let game = g.get(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        Ok(game.view(false))
    }
    pub async fn resume(&self, user: Uuid) -> Option<BlackjackView> {
        let g = self.inner.lock().await;
        g.values()
            .find(|x| x.user == user && x.status == BlackjackStatus::Playing)
            .map(|x| x.view(false))
    }
    pub async fn start(
        &self,
        user: Uuid,
        bet: Cents,
        id: Uuid,
    ) -> Result<BlackjackView, BlackjackError> {
        if !valid_game_amount(bet) {
            return Err(BlackjackError::IllegalAction(
                "bet must be between $1 and $10,000",
            ));
        }
        let mut g = self.inner.lock().await;
        if g.values()
            .any(|x| x.user == user && x.status == BlackjackStatus::Playing)
        {
            return Err(BlackjackError::ActiveGame);
        }
        g.retain(|_, x| x.user != user || x.status == BlackjackStatus::Playing);
        let mut deck = Deck::seeded(rand::thread_rng().r#gen());
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
        if game.dealer[0].rank as u8 != 14 {
            game.peek();
        } else if natural {
            game.hands[0].status = BlackjackHandStatus::Playing;
        }
        let v = game.view(false);
        g.insert(id, game);
        Ok(v)
    }
    pub async fn hit(&self, user: Uuid, id: Uuid) -> Result<BlackjackView, BlackjackError> {
        let mut g = self.inner.lock().await;
        let game = g.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        game.wager(Action::Hit)?;
        game.peek_if_needed();
        if game.status != BlackjackStatus::Playing {
            return Ok(game.view(true));
        }
        let i = game.active_index();
        game.hands[i].cards.push(game.deck.deal().expect("card"));
        if score(&game.hands[i].cards).0 > 21 {
            game.hands[i].status = BlackjackHandStatus::Bust;
            game.advance();
        }
        Ok(game.view(false))
    }
    pub async fn stand(&self, user: Uuid, id: Uuid) -> Result<BlackjackView, BlackjackError> {
        let mut g = self.inner.lock().await;
        let game = g.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        game.wager(Action::Stand)?;
        game.peek_if_needed();
        if game.status != BlackjackStatus::Playing {
            return Ok(game.view(true));
        }
        let i = game.active_index();
        game.hands[i].status = BlackjackHandStatus::Stand;
        game.advance();
        Ok(game.view(true))
    }
    pub async fn double(
        &self,
        user: Uuid,
        id: Uuid,
    ) -> Result<(BlackjackView, Cents), BlackjackError> {
        let mut g = self.inner.lock().await;
        let game = g.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        let w = game.wager(Action::Double)?;
        game.peek_if_needed();
        if game.status != BlackjackStatus::Playing {
            return Ok((game.view(true), 0));
        }
        let i = game.active_index();
        game.hands[i].bet += w;
        game.hands[i].cards.push(game.deck.deal().expect("card"));
        game.hands[i].status = if score(&game.hands[i].cards).0 > 21 {
            BlackjackHandStatus::Bust
        } else {
            BlackjackHandStatus::Stand
        };
        game.advance();
        Ok((game.view(true), w))
    }
    pub async fn split(
        &self,
        user: Uuid,
        id: Uuid,
    ) -> Result<(BlackjackView, Cents), BlackjackError> {
        let mut g = self.inner.lock().await;
        let game = g.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        let w = game.wager(Action::Split)?;
        game.peek_if_needed();
        if game.status != BlackjackStatus::Playing {
            return Ok((game.view(true), 0));
        }
        let i = game.active_index();
        let c = game.hands[i].cards.remove(1);
        let ace = game.hands[i].cards[0].rank as u8 == 14;
        let second = BlackjackHand {
            cards: vec![c],
            bet: game.hands[i].bet,
            status: BlackjackHandStatus::Playing,
            split: true,
            split_aces: ace,
        };
        game.hands[i].split = true;
        game.hands[i].split_aces = ace;
        game.hands.insert(i + 1, second);
        let a = game.deck.deal().expect("card");
        let b = game.deck.deal().expect("card");
        game.hands[i].cards.push(a);
        game.hands[i + 1].cards.push(b);
        if ace {
            game.hands[i].status = BlackjackHandStatus::Stand;
            game.hands[i + 1].status = BlackjackHandStatus::Stand;
            game.advance();
        }
        Ok((game.view(false), w))
    }
    pub async fn insure(
        &self,
        user: Uuid,
        id: Uuid,
    ) -> Result<(BlackjackView, Cents), BlackjackError> {
        let mut g = self.inner.lock().await;
        let game = g.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        let w = game.wager(Action::Insure)?;
        game.insurance = w;
        game.peek_if_needed();
        Ok((game.view(true), w))
    }
}
#[derive(Clone, Copy)]
enum Action {
    Hit,
    Stand,
    Double,
    Split,
    Insure,
}
impl BlackjackGame {
    fn check_user(&self, u: Uuid) -> Result<(), BlackjackError> {
        (self.user == u)
            .then_some(())
            .ok_or(BlackjackError::NotFound)
    }
    fn active_index(&self) -> usize {
        self.hands
            .iter()
            .position(|h| h.status == BlackjackHandStatus::Playing)
            .unwrap_or(self.hands.len())
    }
    fn wager(&self, a: Action) -> Result<Cents, BlackjackError> {
        if self.status != BlackjackStatus::Playing {
            return Err(BlackjackError::Finished);
        }
        let i = self.active_index();
        let h = self.hands.get(i).ok_or(BlackjackError::Finished)?;
        let legal = match a {
            Action::Hit => self.can_hit(),
            Action::Stand => self.can_stand(),
            Action::Double => self.can_double(),
            Action::Split => self.can_split(),
            Action::Insure => self.can_insure(),
        };
        if !legal {
            return Err(BlackjackError::IllegalAction(match a {
                Action::Hit => "hit is not legal",
                Action::Stand => "stand is not legal",
                Action::Double => "double is only legal on the first two cards",
                Action::Split => "hand cannot be split",
                Action::Insure => "insurance is not legal",
            }));
        }
        let w = match a {
            Action::Double | Action::Split => h.bet,
            Action::Insure => h.bet / 2,
            _ => 0,
        };
        if !matches!(a, Action::Hit | Action::Stand) && !valid_game_amount(w) {
            return Err(BlackjackError::IllegalAction(
                "additional wager must be between $1 and $10,000",
            ));
        }
        Ok(w)
    }
    fn can_hit(&self) -> bool {
        let i = self.active_index();
        self.status == BlackjackStatus::Playing
            && i < self.hands.len()
            && !self.hands[i].split_aces
            && self.hands[i].status != BlackjackHandStatus::Blackjack
    }
    fn can_stand(&self) -> bool {
        self.status == BlackjackStatus::Playing && self.active_index() < self.hands.len()
    }
    fn can_double(&self) -> bool {
        let i = self.active_index();
        self.status == BlackjackStatus::Playing
            && i < self.hands.len()
            && self.hands[i].cards.len() == 2
            && !self.hands[i].split_aces
            && valid_game_amount(self.hands[i].bet)
    }
    fn can_split(&self) -> bool {
        let i = self.active_index();
        self.status == BlackjackStatus::Playing
            && i < self.hands.len()
            && self.hands[i].cards.len() == 2
            && self.hands[i].cards[0].rank == self.hands[i].cards[1].rank
            && self.hands.len() < MAX_HANDS
            && valid_game_amount(self.hands[i].bet)
    }
    fn can_insure(&self) -> bool {
        self.status == BlackjackStatus::Playing
            && !self.dealer_peeked
            && self.insurance == 0
            && self.hands.len() == 1
            && self.hands[0].cards.len() == 2
            && !self.hands[0].split
            && self.hands[0].status == BlackjackHandStatus::Playing
            && self.dealer[0].rank as u8 == 14
            && valid_game_amount(self.hands[0].bet / 2)
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
                    .filter(|h| h.status == BlackjackHandStatus::Push)
                    .map(|h| h.bet)
                    .sum::<Cents>();
            self.status = if self
                .hands
                .iter()
                .all(|h| h.status == BlackjackHandStatus::Push)
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
            .any(|h| h.status == BlackjackHandStatus::Playing)
        {
            return;
        }
        while score(&self.dealer).0 < 17 {
            self.dealer.push(self.deck.deal().expect("card"));
        }
        let d = score(&self.dealer).0;
        for h in &mut self.hands {
            if h.status == BlackjackHandStatus::Stand {
                let p = score(&h.cards).0;
                h.status = if d > 21 || p > d {
                    BlackjackHandStatus::Win
                } else if p < d {
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
                .map(|h| match h.status {
                    BlackjackHandStatus::Win => h.bet * 2,
                    BlackjackHandStatus::Push => h.bet,
                    _ => 0,
                })
                .sum::<Cents>();
        self.status = if self
            .hands
            .iter()
            .all(|h| h.status == BlackjackHandStatus::Bust)
        {
            BlackjackStatus::PlayerBust
        } else if d > 21 {
            BlackjackStatus::DealerBust
        } else if self
            .hands
            .iter()
            .any(|h| h.status == BlackjackHandStatus::Win)
        {
            BlackjackStatus::PlayerWin
        } else if self
            .hands
            .iter()
            .all(|h| h.status == BlackjackHandStatus::Push)
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
    fn view(&self, reveal: bool) -> BlackjackView {
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
            .map(|h| BlackjackHandView {
                cards: h.cards.clone(),
                bet: h.bet,
                score: score(&h.cards).0,
                status: h.status,
                blackjack: h.status == BlackjackHandStatus::Blackjack && !h.split,
            })
            .collect();
        let first = self.hands.first().expect("hand");
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
            can_double: self.can_double(),
            can_split: self.can_split(),
            can_insure: self.can_insure(),
            insurance: self.insurance,
            hands,
            active_hand: active,
        }
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
            x => x,
        };
    }
    while total > 21 && aces > 0 {
        total -= 10;
        aces -= 1;
    }
    (total, aces > 0)
}
fn message(s: BlackjackStatus) -> String {
    match s {
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
        assert!(!game.can_insure());
        game.hands[0].bet = 200;
        assert!(game.can_insure());
        game.hands[0].cards.push(card(Rank::Two));
        assert!(!game.can_insure());
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
        assert!(game.can_double());
        assert!(game.can_split());
        assert_eq!(game.wager(Action::Double).unwrap(), 100);
    }

    #[test]
    fn insurance_pays_three_times_stake() {
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
}
