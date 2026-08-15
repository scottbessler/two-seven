use crate::{
    holdem::{Hand, HandSummary},
    money::{Cents, format_cents},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
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

impl fmt::Display for Stakes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit { small_bet, big_bet } => {
                write!(
                    f,
                    "{}/{} limit",
                    format_cents(*small_bet),
                    format_cents(*big_bet)
                )
            }
            Self::NoLimit {
                small_blind,
                big_blind,
            } => write!(
                f,
                "{}/{} no-limit",
                format_cents(*small_blind),
                format_cents(*big_blind)
            ),
        }
    }
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

impl fmt::Display for BotKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Fish => "fish",
            Self::Rock => "rock",
            Self::Grinder => "grinder",
            Self::Shark => "shark",
        })
    }
}

impl FromStr for BotKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "fish" => Ok(Self::Fish),
            "rock" => Ok(Self::Rock),
            "grinder" => Ok(Self::Grinder),
            "shark" => Ok(Self::Shark),
            _ => Err(format!("unknown bot kind: {value}")),
        }
    }
}

#[cfg(test)]
mod bot_kind_tests {
    use super::{
        BotKind, FOLD_RESULT_PAUSE_SECONDS, RUNOUT_STEP_SECONDS, SHOWDOWN_PAUSE_SECONDS, Stakes,
        Table, TableMode, result_pause_seconds,
    };
    use crate::holdem::HandSummary;
    use std::{collections::BTreeMap, str::FromStr};

    #[test]
    fn bot_kind_uses_stable_slugs() {
        assert_eq!(BotKind::Fish.to_string(), "fish");
        assert_eq!(BotKind::from_str("SHARK").unwrap(), BotKind::Shark);
    }

    #[test]
    fn stakes_display_formats_whole_dollar_stakes() {
        assert_eq!(
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            }
            .to_string(),
            "$1.00/$2.00 no-limit"
        );
    }

    #[test]
    fn result_pause_distinguishes_showdown_from_fold_win() {
        let summary = |revealed_hole_cards, runout: Vec<usize>| HandSummary {
            board: Vec::new(),
            results: Vec::new(),
            awards: Vec::new(),
            contributions: BTreeMap::new(),
            revealed_hole_cards,
            events: Vec::new(),
            runout_from: 0,
            runout: runout
                .into_iter()
                .map(|cards| crate::holdem::RunoutStep {
                    cards,
                    leaders: Vec::new(),
                })
                .collect(),
        };
        assert_eq!(
            result_pause_seconds(Some(&summary(Vec::new(), Vec::new()))),
            FOLD_RESULT_PAUSE_SECONDS
        );
        let showdown = vec![(0, Vec::new()), (1, Vec::new())];
        assert_eq!(
            result_pause_seconds(Some(&summary(showdown.clone(), Vec::new()))),
            SHOWDOWN_PAUSE_SECONDS
        );
        // Every runout street buys the table time to watch it land.
        assert_eq!(
            result_pause_seconds(Some(&summary(showdown, vec![3, 4, 5]))),
            SHOWDOWN_PAUSE_SECONDS + 3 * RUNOUT_STEP_SECONDS
        );
    }

    #[test]
    fn legacy_buy_in_range_loads_as_the_fixed_maximum() {
        let table = Table::new(
            "legacy".into(),
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            TableMode::Cash { no_debt: false },
            6,
            20_000,
        );
        let mut value = serde_json::to_value(table).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("buy_in");
        object.insert("min_buy_in".into(), 5_000.into());
        object.insert("max_buy_in".into(), 20_000.into());

        let migrated: Table = serde_json::from_value(value).unwrap();

        assert_eq!(migrated.buy_in, 20_000);
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TableMode {
    Cash { no_debt: bool },
    Tournament(TournamentState),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlindLevel {
    pub small_blind: Cents,
    pub big_blind: Cents,
    pub ante: Cents,
    pub hands: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TournamentConfig {
    pub buy_in: Cents,
    pub seat_count: usize,
    pub starting_chips: Cents,
    pub levels: Vec<BlindLevel>,
    pub payout_percentages: Vec<u8>,
    pub no_debt: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TournamentState {
    pub config: TournamentConfig,
    pub current_level: usize,
    pub hands_at_level: u32,
    pub finish_order: Vec<usize>,
    pub registered: usize,
    #[serde(default)]
    pub started: bool,
    pub prize_pool: Cents,
    pub finished: bool,
    pub paid_out: bool,
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
    #[serde(default)]
    pub pending_departure: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Table {
    pub id: Uuid,
    pub name: String,
    pub variant: Variant,
    pub stakes: Stakes,
    pub mode: TableMode,
    pub max_seats: usize,
    #[serde(alias = "max_buy_in")]
    pub buy_in: Cents,
    pub seats: Vec<Seat>,
    pub button: usize,
    pub hand_no: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub hand: Option<Hand>,
    pub last_hand: Option<HandSummary>,
    pub next_action_at: Option<DateTime<Utc>>,
}

pub const SHOWDOWN_PAUSE_SECONDS: i64 = 6;
pub const FOLD_RESULT_PAUSE_SECONDS: i64 = 3;
/// An all-in board runs out one street at a time so the table can watch it.
pub const RUNOUT_STEP_SECONDS: i64 = 5;

pub fn result_pause_seconds(summary: Option<&HandSummary>) -> i64 {
    let Some(summary) = summary else {
        return FOLD_RESULT_PAUSE_SECONDS;
    };
    let runout = summary.runout.len() as i64 * RUNOUT_STEP_SECONDS;
    if summary.revealed_hole_cards.len() > 1 {
        SHOWDOWN_PAUSE_SECONDS + runout
    } else {
        FOLD_RESULT_PAUSE_SECONDS + runout
    }
}

impl Table {
    pub fn new(
        name: String,
        stakes: Stakes,
        mode: TableMode,
        max_seats: usize,
        buy_in: Cents,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            variant: Variant::Holdem,
            stakes,
            mode,
            max_seats,
            buy_in,
            seats: (0..max_seats)
                .map(|_| Seat {
                    occupant: SeatOccupant::Empty,
                    stack: 0,
                    sitting_out: false,
                    pending_departure: false,
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

pub fn maybe_start_hand(table: &mut Table) {
    if let TableMode::Tournament(state) = &table.mode
        && !state.started
        && state.registered < state.config.seat_count
    {
        return;
    }
    if let TableMode::Tournament(state) = &mut table.mode {
        state.started = true;
    }
    if table.hand.is_some()
        || table
            .seats
            .iter()
            .filter(|seat| {
                !seat.sitting_out && seat.stack > 0 && !matches!(seat.occupant, SeatOccupant::Empty)
            })
            .count()
            < 2
    {
        return;
    }
    let stacks: Vec<(usize, Cents)> = table
        .seats
        .iter()
        .enumerate()
        .filter_map(|(seat, value)| {
            (!value.sitting_out
                && value.stack > 0
                && !matches!(value.occupant, SeatOccupant::Empty))
            .then_some((seat, value.stack))
        })
        .collect();
    table.hand_no += 1;
    let ante = match &table.mode {
        TableMode::Tournament(state) => state
            .config
            .levels
            .get(state.current_level)
            .map_or(0, |level| level.ante),
        TableMode::Cash { .. } => 0,
    };
    table.hand = Some(Hand::new_with_seats_and_ante(
        table.stakes,
        &stacks,
        table.button,
        table.hand_no,
        ante,
    ));
    table.next_action_at = None;
}

pub fn settle_finished_hand(table: &mut Table) {
    let Some(hand) = table.hand.take() else {
        return;
    };
    if !hand.complete {
        table.hand = Some(hand);
        return;
    }
    for player in &hand.players {
        if let Some(seat) = table.seats.get_mut(player.seat) {
            seat.stack = player.stack;
        }
    }
    if let TableMode::Tournament(state) = &mut table.mode {
        state.hands_at_level += 1;
        for player in &hand.players {
            if player.stack == 0
                && !state.finish_order.contains(&player.seat)
                && !table
                    .seats
                    .get(player.seat)
                    .is_some_and(|seat| seat.sitting_out)
            {
                state.finish_order.push(player.seat);
            }
        }
        if state
            .config
            .levels
            .get(state.current_level)
            .is_some_and(|level| state.hands_at_level >= level.hands)
            && state.current_level + 1 < state.config.levels.len()
        {
            state.current_level += 1;
            state.hands_at_level = 0;
            if let Some(level) = state.config.levels.get(state.current_level) {
                table.stakes = Stakes::NoLimit {
                    small_blind: level.small_blind,
                    big_blind: level.big_blind,
                };
            }
        }
        let alive = table
            .seats
            .iter()
            .filter(|seat| !matches!(seat.occupant, SeatOccupant::Empty) && seat.stack > 0)
            .count();
        if alive <= 1 {
            for (seat, value) in table.seats.iter().enumerate() {
                if !matches!(value.occupant, SeatOccupant::Empty)
                    && value.stack == 0
                    && !state.finish_order.contains(&seat)
                {
                    state.finish_order.push(seat);
                }
            }
            state.finished = true;
        }
    }
    table.button = (table.button + 1) % table.seats.len();
    table.last_hand = hand.summary;
    table.next_action_at = Some(
        Utc::now() + chrono::Duration::seconds(result_pause_seconds(table.last_hand.as_ref())),
    );
}
