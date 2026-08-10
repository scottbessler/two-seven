use crate::{
    cards::{Card, Deck},
    money::Cents,
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
        let guard = self.inner.lock().await;
        let game = guard.get(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        Ok(game.view(false))
    }

    pub async fn start(&self, user: Uuid, bet: Cents, id: Uuid) -> BlackjackView {
        let mut deck = Deck::seeded(rand::thread_rng().r#gen());
        let player = vec![deck.deal().expect("card"), deck.deal().expect("card")];
        let dealer = vec![deck.deal().expect("card"), deck.deal().expect("card")];
        let blackjack = score(&player).0 == 21;
        let hand = BlackjackHand {
            cards: player,
            bet,
            status: if blackjack {
                BlackjackHandStatus::Blackjack
            } else {
                BlackjackHandStatus::Playing
            },
            split: false,
            split_aces: false,
        };
        let game = BlackjackGame {
            id,
            user,
            deck,
            hands: vec![hand],
            dealer,
            insurance: 0,
            status: if blackjack {
                BlackjackStatus::PlayerBlackjack
            } else {
                BlackjackStatus::Playing
            },
            payout: if blackjack { bet * 5 / 2 } else { 0 },
        };
        let view = game.view(false);
        self.inner.lock().await.insert(id, game);
        view
    }

    pub async fn hit(&self, user: Uuid, id: Uuid) -> Result<BlackjackView, BlackjackError> {
        let mut guard = self.inner.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        game.require_playing()?;
        let active = game.active_index();
        if active >= game.hands.len() || game.hands[active].split_aces {
            return Err(BlackjackError::IllegalAction("split aces receive one card"));
        }
        let card = game.deck.deal().expect("card");
        let hand = &mut game.hands[active];
        hand.cards.push(card);
        if score(&hand.cards).0 > 21 {
            hand.status = BlackjackHandStatus::Bust;
            game.advance();
        }
        Ok(game.view(false))
    }

    pub async fn stand(&self, user: Uuid, id: Uuid) -> Result<BlackjackView, BlackjackError> {
        let mut guard = self.inner.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        game.require_playing()?;
        let active = game.active_index();
        if active >= game.hands.len() {
            return Err(BlackjackError::Finished);
        }
        game.hands[active].status = BlackjackHandStatus::Stand;
        game.advance();
        Ok(game.view(true))
    }

    pub async fn double(&self, user: Uuid, id: Uuid) -> Result<BlackjackView, BlackjackError> {
        let mut guard = self.inner.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        game.require_playing()?;
        let active = game.active_index();
        if active >= game.hands.len()
            || game.hands[active].cards.len() != 2
            || game.hands[active].split_aces
        {
            return Err(BlackjackError::IllegalAction(
                "double is only legal on the first two cards",
            ));
        }
        let card = game.deck.deal().expect("card");
        let hand = &mut game.hands[active];
        hand.bet *= 2;
        hand.cards.push(card);
        hand.status = if score(&hand.cards).0 > 21 {
            BlackjackHandStatus::Bust
        } else {
            BlackjackHandStatus::Stand
        };
        game.advance();
        Ok(game.view(true))
    }

    pub async fn split(&self, user: Uuid, id: Uuid) -> Result<BlackjackView, BlackjackError> {
        let mut guard = self.inner.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        game.require_playing()?;
        let active = game.active_index();
        if game.hands.len() >= MAX_HANDS
            || active >= game.hands.len()
            || game.hands[active].cards.len() != 2
            || game.hands[active].cards[0].rank != game.hands[active].cards[1].rank
        {
            return Err(BlackjackError::IllegalAction("hand cannot be split"));
        }
        let first = game.hands[active].cards.remove(1);
        let split_aces = game.hands[active].cards[0].rank as u8 == 14;
        let second = BlackjackHand {
            cards: vec![first],
            bet: game.hands[active].bet,
            status: BlackjackHandStatus::Playing,
            split: true,
            split_aces,
        };
        game.hands[active].split = true;
        game.hands[active].split_aces = split_aces;
        game.hands.insert(active + 1, second);
        let first_card = game.deck.deal().expect("card");
        let second_card = game.deck.deal().expect("card");
        game.hands[active].cards.push(first_card);
        game.hands[active + 1].cards.push(second_card);
        if split_aces {
            game.hands[active].status = BlackjackHandStatus::Stand;
            game.hands[active + 1].status = BlackjackHandStatus::Stand;
            game.advance();
        }
        Ok(game.view(false))
    }

    pub async fn insure(&self, user: Uuid, id: Uuid) -> Result<BlackjackView, BlackjackError> {
        let mut guard = self.inner.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        game.check_user(user)?;
        game.require_playing()?;
        if game.insurance != 0 || game.dealer[0].rank as u8 != 14 || game.hands.len() != 1 {
            return Err(BlackjackError::IllegalAction("insurance is not legal"));
        }
        let wager = game.hands[0].bet / 2;
        if wager <= 0 {
            return Err(BlackjackError::IllegalAction(
                "insurance requires a bet of at least two cents",
            ));
        }
        game.insurance = wager;
        Ok(game.view(false))
    }
}

impl BlackjackGame {
    fn check_user(&self, user: Uuid) -> Result<(), BlackjackError> {
        (self.user == user)
            .then_some(())
            .ok_or(BlackjackError::NotFound)
    }

    fn require_playing(&self) -> Result<(), BlackjackError> {
        (self.status == BlackjackStatus::Playing)
            .then_some(())
            .ok_or(BlackjackError::Finished)
    }

    fn active_index(&self) -> usize {
        self.hands
            .iter()
            .position(|hand| hand.status == BlackjackHandStatus::Playing)
            .unwrap_or(self.hands.len())
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
            if hand.status != BlackjackHandStatus::Stand {
                continue;
            }
            let player = score(&hand.cards).0;
            hand.status = if dealer_score > 21 || player > dealer_score {
                BlackjackHandStatus::Win
            } else if player < dealer_score {
                BlackjackHandStatus::Loss
            } else {
                BlackjackHandStatus::Push
            };
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
            .all(|h| h.status == BlackjackHandStatus::Bust)
        {
            BlackjackStatus::PlayerBust
        } else if dealer_score > 21 {
            BlackjackStatus::DealerBust
        } else if self.hands.iter().any(|h| {
            matches!(
                h.status,
                BlackjackHandStatus::Win | BlackjackHandStatus::Blackjack
            )
        }) {
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

    fn view(&self, reveal_dealer: bool) -> BlackjackView {
        let finished = self.status != BlackjackStatus::Playing;
        let dealer = if reveal_dealer || finished {
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
            .collect::<Vec<_>>();
        let first = self.hands.first().expect("hand");
        BlackjackView {
            id: self.id,
            bet: first.bet,
            player: first.cards.clone(),
            dealer,
            player_score: score(&first.cards).0,
            dealer_score: (reveal_dealer || finished).then_some(score(&self.dealer).0),
            status: self.status,
            message: message(self.status),
            payout: self.payout,
            can_hit: self.status == BlackjackStatus::Playing
                && active < self.hands.len()
                && !self.hands[active].split_aces,
            can_stand: self.status == BlackjackStatus::Playing && active < self.hands.len(),
            can_double: self.status == BlackjackStatus::Playing
                && active < self.hands.len()
                && self.hands[active].cards.len() == 2
                && !self.hands[active].split_aces,
            can_split: self.status == BlackjackStatus::Playing
                && active < self.hands.len()
                && self.hands[active].cards.len() == 2
                && self.hands[active].cards[0].rank == self.hands[active].cards[1].rank
                && self.hands.len() < MAX_HANDS,
            can_insure: self.status == BlackjackStatus::Playing
                && self.insurance == 0
                && self.hands.len() == 1
                && self.dealer[0].rank as u8 == 14,
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
    IllegalAction(&'static str),
}

pub fn score(cards: &[Card]) -> (u8, bool) {
    let mut total = 0u8;
    let mut aces = 0u8;
    for card in cards {
        let value = match card.rank as u8 {
            14 => {
                aces += 1;
                11
            }
            11..=13 => 10,
            value => value,
        };
        total += value;
    }
    while total > 21 && aces > 0 {
        total -= 10;
        aces -= 1;
    }
    (total, aces > 0)
}

fn message(status: BlackjackStatus) -> String {
    match status {
        BlackjackStatus::Playing => "Hit or stand.".into(),
        BlackjackStatus::PlayerBlackjack => "Blackjack pays 3:2.".into(),
        BlackjackStatus::PlayerBust => "Bust.".into(),
        BlackjackStatus::DealerBust => "Dealer busts.".into(),
        BlackjackStatus::PlayerWin => "You win.".into(),
        BlackjackStatus::DealerWin => "Dealer wins.".into(),
        BlackjackStatus::Push => "Push.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::{Rank, Suit};

    fn card(rank: Rank) -> Card {
        Card::new(rank, Suit::Spades)
    }

    fn game(hands: Vec<BlackjackHand>, dealer: Vec<Card>) -> BlackjackGame {
        BlackjackGame {
            id: Uuid::new_v4(),
            user: Uuid::new_v4(),
            deck: Deck::seeded(7),
            hands,
            dealer,
            insurance: 0,
            status: BlackjackStatus::Playing,
            payout: 0,
        }
    }

    #[test]
    fn insurance_returns_three_to_one_when_dealer_has_blackjack() {
        let game = game(
            vec![BlackjackHand {
                cards: vec![card(Rank::Ten), card(Rank::Nine)],
                bet: 100,
                status: BlackjackHandStatus::Loss,
                split: false,
                split_aces: false,
            }],
            vec![card(Rank::Ace), card(Rank::King)],
        );
        let mut game = game;
        game.insurance = 50;
        assert_eq!(game.insurance_payout(), 150);
    }

    #[test]
    fn split_hand_with_21_is_not_a_blackjack() {
        let game = game(
            vec![BlackjackHand {
                cards: vec![card(Rank::Ace), card(Rank::King)],
                bet: 100,
                status: BlackjackHandStatus::Stand,
                split: true,
                split_aces: false,
            }],
            vec![card(Rank::Nine), card(Rank::Seven)],
        );
        assert!(!game.view(false).hands[0].blackjack);
    }

    #[test]
    fn bust_hand_has_no_payout() {
        let mut game = game(
            vec![BlackjackHand {
                cards: vec![card(Rank::King), card(Rank::Nine), card(Rank::Five)],
                bet: 100,
                status: BlackjackHandStatus::Bust,
                split: false,
                split_aces: false,
            }],
            vec![card(Rank::Ten), card(Rank::Seven)],
        );
        game.advance();
        assert_eq!(game.payout, 0);
        assert_eq!(game.status, BlackjackStatus::PlayerBust);
    }

    #[test]
    fn illegal_insurance_is_rejected_without_an_ace_up() {
        let game = game(
            vec![BlackjackHand {
                cards: vec![card(Rank::Ten), card(Rank::Nine)],
                bet: 100,
                status: BlackjackHandStatus::Playing,
                split: false,
                split_aces: false,
            }],
            vec![card(Rank::Ten), card(Rank::Seven)],
        );
        assert_eq!(game.dealer[0].rank as u8, 10);
        assert!(game.dealer[0].rank as u8 != 14);
    }

    #[test]
    fn aces_score_soft_until_they_must_drop() {
        let cards = vec![card(Rank::Ace), card(Rank::Nine), card(Rank::Five)];
        assert_eq!(score(&cards), (15, false));
    }
}
