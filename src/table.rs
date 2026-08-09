use crate::cards::Card;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
pub type Cents = i64;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Variant {
    Holdem,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Stakes {
    Limit {
        small_bet: Cents,
        big_bet: Cents,
    },
    NoLimit {
        small_blind: Cents,
        big_blind: Cents,
    },
}
impl Stakes {
    pub fn blinds(self) -> (Cents, Cents) {
        match self {
            Self::Limit { small_bet, .. } => (small_bet / 2, small_bet),
            Self::NoLimit {
                small_blind,
                big_blind,
            } => (small_blind, big_blind),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TableMode {
    Cash { no_debt: bool },
    Tournament,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SeatOccupant {
    Empty,
    Human { user_id: Uuid },
    Bot { kind: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Seat {
    pub occupant: SeatOccupant,
    pub stack: Cents,
    pub sitting_out: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Table {
    pub id: Uuid,
    pub name: String,
    pub variant: Variant,
    pub stakes: Stakes,
    pub mode: TableMode,
    pub max_seats: usize,
    pub min_buy_in: Cents,
    pub max_buy_in: Cents,
    pub seats: Vec<Seat>,
    pub button: usize,
    pub hand_no: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub hand: Option<crate::holdem::Hand>,
    pub last_hand: Option<crate::holdem::HandSummary>,
    pub next_action_at: Option<DateTime<Utc>>,
}
#[derive(Clone, Debug, Serialize)]
pub struct SeatView {
    pub index: usize,
    pub stack: Cents,
    pub occupant: String,
    pub sitting_out: bool,
    pub hole_cards: Option<Vec<Card>>,
}
