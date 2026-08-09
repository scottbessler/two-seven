use crate::{
    holdem::{Hand, HandSummary},
    money::Cents,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BotKind {
    Fish,
    Rock,
    Grinder,
    Shark,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TableMode {
    Cash { no_debt: bool },
    Tournament,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SeatOccupant {
    Empty,
    Human { user_id: Uuid },
    Bot { kind: BotKind },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
    pub hand: Option<Hand>,
    pub last_hand: Option<HandSummary>,
    pub next_action_at: Option<DateTime<Utc>>,
}
impl Table {
    pub fn new(
        name: String,
        stakes: Stakes,
        mode: TableMode,
        max_seats: usize,
        min_buy_in: Cents,
        max_buy_in: Cents,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            variant: Variant::Holdem,
            stakes,
            mode,
            max_seats,
            min_buy_in,
            max_buy_in,
            seats: (0..max_seats)
                .map(|_| Seat {
                    occupant: SeatOccupant::Empty,
                    stack: 0,
                    sitting_out: false,
                })
                .collect(),
            button: 0,
            hand_no: 0,
            created_at: now,
            updated_at: now,
            hand: None,
            last_hand: None,
            next_action_at: None,
        }
    }
}
