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

#[derive(Clone)]
pub struct BlackjackStore {
    inner: Arc<Mutex<HashMap<Uuid, BlackjackGame>>>,
}

#[derive(Clone, Debug)]
struct BlackjackGame {
    id: Uuid,
    user: Uuid,
    bet: Cents,
    deck: Deck,
    player: Vec<Card>,
    dealer: Vec<Card>,
    status: BlackjackStatus,
    payout: Cents,
}

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

    pub async fn start(&self, user: Uuid, bet: Cents, id: Uuid) -> BlackjackView {
        let mut deck = Deck::seeded(rand::thread_rng().r#gen());
        let mut game = BlackjackGame {
            id,
            user,
            bet,
            deck: deck.clone(),
            player: vec![deck.deal().expect("card"), deck.deal().expect("card")],
            dealer: vec![deck.deal().expect("card"), deck.deal().expect("card")],
            status: BlackjackStatus::Playing,
            payout: 0,
        };
        game.deck = deck;
        if score(&game.player).0 == 21 {
            game.status = BlackjackStatus::PlayerBlackjack;
            game.payout = bet * 5 / 2;
        }
        let view = game.view(false);
        self.inner.lock().await.insert(id, game);
        view
    }

    pub async fn hit(&self, user: Uuid, id: Uuid) -> Result<BlackjackView, BlackjackError> {
        let mut guard = self.inner.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        if game.user != user {
            return Err(BlackjackError::NotFound);
        }
        if game.status != BlackjackStatus::Playing {
            return Err(BlackjackError::Finished);
        }
        game.player.push(game.deck.deal().expect("card"));
        if score(&game.player).0 > 21 {
            game.status = BlackjackStatus::PlayerBust;
        }
        Ok(game.view(false))
    }

    pub async fn stand(&self, user: Uuid, id: Uuid) -> Result<BlackjackView, BlackjackError> {
        let mut guard = self.inner.lock().await;
        let game = guard.get_mut(&id).ok_or(BlackjackError::NotFound)?;
        if game.user != user {
            return Err(BlackjackError::NotFound);
        }
        if game.status != BlackjackStatus::Playing {
            return Err(BlackjackError::Finished);
        }
        while score(&game.dealer).0 < 17 {
            game.dealer.push(game.deck.deal().expect("card"));
        }
        let player = score(&game.player).0;
        let dealer = score(&game.dealer).0;
        game.status = if dealer > 21 {
            BlackjackStatus::DealerBust
        } else if player > dealer {
            BlackjackStatus::PlayerWin
        } else if player < dealer {
            BlackjackStatus::DealerWin
        } else {
            BlackjackStatus::Push
        };
        game.payout = match game.status {
            BlackjackStatus::DealerBust | BlackjackStatus::PlayerWin => game.bet * 2,
            BlackjackStatus::Push => game.bet,
            _ => 0,
        };
        Ok(game.view(true))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlackjackError {
    NotFound,
    Finished,
}

impl BlackjackGame {
    fn view(&self, reveal_dealer: bool) -> BlackjackView {
        let finished = self.status != BlackjackStatus::Playing;
        let dealer = if reveal_dealer || finished {
            self.dealer.clone()
        } else {
            vec![self.dealer[0]]
        };
        BlackjackView {
            id: self.id,
            bet: self.bet,
            player: self.player.clone(),
            dealer,
            player_score: score(&self.player).0,
            dealer_score: (reveal_dealer || finished).then_some(score(&self.dealer).0),
            status: self.status,
            message: message(self.status),
            payout: self.payout,
            can_hit: self.status == BlackjackStatus::Playing,
            can_stand: self.status == BlackjackStatus::Playing,
        }
    }
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
    use crate::cards::Suit;

    #[test]
    fn aces_score_soft_until_they_must_drop() {
        let cards = vec![
            Card::new(crate::cards::Rank::Ace, Suit::Spades),
            Card::new(crate::cards::Rank::Nine, Suit::Hearts),
            Card::new(crate::cards::Rank::Five, Suit::Clubs),
        ];
        assert_eq!(score(&cards), (15, false));
    }
}
