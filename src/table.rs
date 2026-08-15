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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum BotKind {
    Fish,
    Rock,
    Grinder,
    Shark,
}

/// One of the house players. A kind is a way of playing; a bot is somebody who
/// plays that way, with their own name and their own bankroll.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct Bot {
    pub kind: BotKind,
    /// Which of the kind's five regulars this is.
    #[serde(default)]
    pub seat: u8,
}

impl Bot {
    pub const PER_KIND: u8 = 5;

    pub fn new(kind: BotKind, seat: u8) -> Self {
        Self {
            kind,
            seat: seat % Self::PER_KIND,
        }
    }

    /// Everyone who plays for the house, in a stable order.
    pub fn roster() -> Vec<Self> {
        BotKind::ALL
            .into_iter()
            .flat_map(|kind| (0..Self::PER_KIND).map(move |seat| Self { kind, seat }))
            .collect()
    }

    pub fn name(self) -> &'static str {
        let names = match self.kind {
            BotKind::Fish => ["Marlon", "Dede", "Ollie", "Pip", "Wanda"],
            BotKind::Rock => ["Agnes", "Bernard", "Constance", "Dov", "Edda"],
            BotKind::Grinder => ["Hark", "Ines", "Jules", "Kip", "Lena"],
            BotKind::Shark => ["Nadia", "Osman", "Prisha", "Quill", "Rune"],
        };
        names[(self.seat % Self::PER_KIND) as usize]
    }
}

impl fmt::Display for Bot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind, self.seat)
    }
}

impl FromStr for Bot {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.split_once(':') {
            Some((kind, seat)) => Ok(Self::new(
                kind.parse()?,
                seat.parse().map_err(|_| format!("bad bot seat: {seat}"))?,
            )),
            // A bare kind is how a bot was named before they had names.
            None => Ok(Self::new(value.parse()?, 0)),
        }
    }
}

impl BotKind {
    pub const ALL: [Self; 4] = [Self::Fish, Self::Rock, Self::Grinder, Self::Shark];
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
        Bot, BotKind, FOLD_RESULT_PAUSE_SECONDS, RUNOUT_STEP_SECONDS, SHOWDOWN_PAUSE_SECONDS,
        SeatOccupant, Stakes, Table, TableMode, next_button, result_pause_seconds,
    };
    use crate::holdem::HandSummary;
    use std::{collections::BTreeMap, str::FromStr};

    #[test]
    fn every_bot_is_a_distinct_person() {
        let roster = Bot::roster();
        assert_eq!(roster.len(), 20, "five regulars for each of four kinds");
        let names: std::collections::BTreeSet<&str> = roster.iter().map(|bot| bot.name()).collect();
        assert_eq!(names.len(), roster.len(), "no two share a name");
        // A seat round-trips through its text form, and a bare kind still reads
        // as the first regular of that kind.
        for bot in &roster {
            assert_eq!(bot.to_string().parse::<Bot>().unwrap(), *bot);
        }
        assert_eq!("shark".parse::<Bot>().unwrap(), Bot::new(BotKind::Shark, 0));
        assert_eq!(Bot::new(BotKind::Fish, 7), Bot::new(BotKind::Fish, 2));
    }

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
    fn the_button_skips_seats_that_are_out_of_the_hand() {
        let mut table = Table::new(
            "elimination".into(),
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            TableMode::Cash { no_debt: false },
            4,
            20_000,
        );
        for (index, seat) in table.seats.iter_mut().enumerate() {
            seat.occupant = SeatOccupant::bot(crate::table::Bot::new(BotKind::Rock, 0));
            // Seats 1 and 2 are busted; only 0 and 3 still have chips.
            seat.stack = if index == 1 || index == 2 { 0 } else { 20_000 };
        }
        table.button = 0;
        assert_eq!(
            next_button(&table),
            3,
            "the button must land on a seat that is dealt in"
        );
        table.button = 3;
        assert_eq!(next_button(&table), 0, "and wrap past the busted seats");

        // A sitting-out seat is skipped the same way.
        table.seats[0].sitting_out = true;
        table.button = 3;
        assert_eq!(next_button(&table), 3);
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
    Human {
        user_id: Uuid,
    },
    Bot {
        kind: BotKind,
        #[serde(default)]
        seat: u8,
    },
}

impl SeatOccupant {
    pub fn bot(bot: Bot) -> Self {
        Self::Bot {
            kind: bot.kind,
            seat: bot.seat,
        }
    }

    pub fn as_bot(&self) -> Option<Bot> {
        match self {
            Self::Bot { kind, seat } => Some(Bot::new(*kind, *seat)),
            _ => None,
        }
    }
}

impl fmt::Display for SeatOccupant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty"),
            Self::Human { user_id } => write!(f, "human:{user_id}"),
            Self::Bot { kind, seat } => write!(f, "bot:{}", Bot::new(*kind, *seat).name()),
        }
    }
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
    /// Which rung of the standing cash ladder this is, if it is one of them.
    #[serde(default)]
    pub cash_tier: Option<usize>,
    /// Hands the house has been asked to play with nobody sitting down. A
    /// table with no people at it deals only when somebody watching says so.
    #[serde(default)]
    pub bot_hands_requested: u32,
}

/// A finished hand, kept whole for later inspection: who sat where, what they
/// held whether or not they showed it, and everything that happened.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HandRecord {
    pub table: Uuid,
    pub hand_no: u64,
    pub at: DateTime<Utc>,
    pub stakes: Stakes,
    pub button: usize,
    pub seats: Vec<HandRecordSeat>,
    pub summary: HandSummary,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HandRecordSeat {
    pub seat: usize,
    pub occupant: SeatOccupant,
    /// Every hole card, including folded hands, which the live view redacts.
    pub hole_cards: Vec<crate::cards::Card>,
    pub stack_before: Cents,
    pub stack_after: Cents,
}

pub const SHOWDOWN_PAUSE_SECONDS: i64 = 6;
pub const FOLD_RESULT_PAUSE_SECONDS: i64 = 3;
/// An all-in board runs out one street at a time so the table can watch it.
pub const RUNOUT_STEP_SECONDS: i64 = 5;

/// How long the board takes to run out after a hand ends. Nobody may skip it:
/// the point of the reveal is that the table watches the cards land.
pub fn runout_seconds(summary: Option<&HandSummary>) -> i64 {
    summary.map_or(0, |summary| {
        summary.runout.len() as i64 * RUNOUT_STEP_SECONDS
    })
}

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
            cash_tier: None,
            bot_hands_requested: 0,
            next_action_at: None,
        }
    }
}

/// Whether a seat takes part in the next hand.
fn deals_in(seat: &Seat) -> bool {
    !seat.sitting_out && seat.stack > 0 && !matches!(seat.occupant, SeatOccupant::Empty)
}

/// The next seat the button belongs on. It has to skip empty and busted seats:
/// a hand is dealt only to the seats that deal in, and a button pointing at one
/// of the others collapses onto the lowest live seat, which is how the same
/// players end up in the blinds hand after hand.
fn next_button(table: &Table) -> usize {
    let seats = table.seats.len();
    (1..=seats)
        .map(|step| (table.button + step) % seats)
        .find(|candidate| table.seats.get(*candidate).is_some_and(deals_in))
        .unwrap_or((table.button + 1) % seats)
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
    if table.hand.is_some() || table.seats.iter().filter(|seat| deals_in(seat)).count() < 2 {
        return;
    }
    // A standing cash table waits for the house to fill it rather than locking
    // itself into a short-handed game the moment two players are down.
    if table.cash_tier.is_some()
        && table
            .seats
            .iter()
            .any(|seat| matches!(seat.occupant, SeatOccupant::Empty))
    {
        return;
    }
    // The house does not play to an empty room. With nobody sitting down, a
    // hand is dealt only when a watcher asks for one.
    if !table
        .seats
        .iter()
        .any(|seat| matches!(seat.occupant, SeatOccupant::Human { .. }))
    {
        if table.bot_hands_requested == 0 {
            return;
        }
        table.bot_hands_requested -= 1;
    }
    let stacks: Vec<(usize, Cents)> = table
        .seats
        .iter()
        .enumerate()
        .filter_map(|(seat, value)| deals_in(value).then_some((seat, value.stack)))
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

/// Settle a completed hand and hand back its record for the history log.
pub fn settle_finished_hand(table: &mut Table) -> Option<HandRecord> {
    let hand = table.hand.take()?;
    if !hand.complete {
        table.hand = Some(hand);
        return None;
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
    table.button = next_button(table);
    let summary = hand.summary?;
    let record = HandRecord {
        table: table.id,
        hand_no: table.hand_no,
        at: Utc::now(),
        stakes: table.stakes,
        button: hand.button,
        seats: hand
            .players
            .iter()
            .map(|player| {
                let awarded: Cents = summary
                    .awards
                    .iter()
                    .filter(|award| award.seat == player.seat)
                    .map(|award| award.amount)
                    .sum();
                HandRecordSeat {
                    seat: player.seat,
                    occupant: table
                        .seats
                        .get(player.seat)
                        .map_or(SeatOccupant::Empty, |seat| seat.occupant.clone()),
                    hole_cards: player.hole_cards.clone(),
                    stack_before: player.stack + player.contribution - awarded,
                    stack_after: player.stack,
                }
            })
            .collect(),
        summary: summary.clone(),
    };
    table.last_hand = Some(summary);
    table.next_action_at = Some(
        Utc::now() + chrono::Duration::seconds(result_pause_seconds(table.last_hand.as_ref())),
    );
    Some(record)
}
