use crate::{
    bank::Account,
    cards::Card,
    holdem::{Hand, HandSummary, LegalActions},
    money::Cents,
    table::{Seat, SeatOccupant, Table},
};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct SeatView {
    pub index: usize,
    pub stack: Cents,
    pub occupant: String,
    pub sitting_out: bool,
    pub hole_cards: Option<Vec<Card>>,
    pub bank_balance: Option<Cents>,
    pub bank_entries: Vec<crate::bank::LedgerEntry>,
}
#[derive(Clone, Debug, Serialize)]
pub struct HandView {
    pub street: String,
    pub board: Vec<Card>,
    pub your_hole_cards: Option<Vec<Card>>,
    pub seats: Vec<SeatView>,
    pub pot: Cents,
    pub current_player: Option<usize>,
    pub legal_actions: Option<LegalActions>,
    pub summary: Option<HandSummary>,
}
#[derive(Clone, Debug, Serialize)]
pub struct TableView {
    pub id: uuid::Uuid,
    pub name: String,
    pub stakes: crate::table::Stakes,
    pub seats: Vec<SeatView>,
    pub button: usize,
    pub hand: Option<HandView>,
    pub last_hand: Option<HandSummary>,
    pub tournament: Option<TournamentView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TournamentView {
    pub level: usize,
    pub small_blind: Cents,
    pub big_blind: Cents,
    pub ante: Cents,
    pub hands_at_level: u32,
    pub hands_per_level: u32,
    pub next_level: Option<usize>,
    pub next_small_blind: Option<Cents>,
    pub next_big_blind: Option<Cents>,
    pub next_ante: Option<Cents>,
    pub finish_order: Vec<usize>,
    pub finished: bool,
}

pub fn hand_view(hand: &Hand, viewer: Option<usize>) -> HandView {
    let your_hole_cards = viewer.and_then(|seat| {
        hand.players
            .iter()
            .find(|player| player.seat == seat)
            .map(|player| player.hole_cards.clone())
    });
    let seats = hand
        .players
        .iter()
        .map(|player| SeatView {
            index: player.seat,
            stack: player.stack,
            occupant: format!("seat {}", player.seat),
            sitting_out: false,
            hole_cards: viewer
                .filter(|seat| *seat == player.seat)
                .map(|_| player.hole_cards.clone())
                .or_else(|| {
                    hand.summary.as_ref().and_then(|summary| {
                        summary
                            .revealed_hole_cards
                            .iter()
                            .find(|(seat, _)| *seat == player.seat)
                            .map(|(_, cards)| cards.clone())
                    })
                }),
            bank_balance: None,
            bank_entries: Vec::new(),
        })
        .collect();
    HandView {
        street: format!("{:?}", hand.street),
        board: hand.board.clone(),
        your_hole_cards,
        seats,
        pot: hand.pot,
        current_player: hand.current_player,
        legal_actions: viewer
            .filter(|seat| hand.current_player == Some(*seat))
            .and_then(|_| hand.legal_actions()),
        summary: hand.summary.clone(),
    }
}

pub fn table_view(table: &Table, viewer: Option<usize>) -> TableView {
    table_view_with_banks(table, viewer, &std::collections::HashMap::new())
}

pub fn table_view_with_banks(
    table: &Table,
    viewer: Option<usize>,
    banks: &std::collections::HashMap<usize, Account>,
) -> TableView {
    TableView {
        id: table.id,
        name: table.name.clone(),
        stakes: table.stakes,
        seats: table
            .seats
            .iter()
            .enumerate()
            .map(|(index, seat)| seat_view(index, seat, banks.get(&index)))
            .collect(),
        button: table.button,
        hand: table.hand.as_ref().map(|hand| hand_view(hand, viewer)),
        last_hand: table.last_hand.clone(),
        tournament: match &table.mode {
            crate::table::TableMode::Tournament(state) => state
                .config
                .levels
                .get(state.current_level)
                .map(|level| TournamentView {
                    level: state.current_level + 1,
                    small_blind: level.small_blind,
                    big_blind: level.big_blind,
                    ante: level.ante,
                    hands_at_level: state.hands_at_level,
                    hands_per_level: level.hands,
                    next_level: state
                        .config
                        .levels
                        .get(state.current_level + 1)
                        .map(|_| state.current_level + 2),
                    next_small_blind: state
                        .config
                        .levels
                        .get(state.current_level + 1)
                        .map(|level| level.small_blind),
                    next_big_blind: state
                        .config
                        .levels
                        .get(state.current_level + 1)
                        .map(|level| level.big_blind),
                    next_ante: state
                        .config
                        .levels
                        .get(state.current_level + 1)
                        .map(|level| level.ante),
                    finish_order: state.finish_order.clone(),
                    finished: state.finished,
                }),
            crate::table::TableMode::Cash { .. } => None,
        },
    }
}
fn seat_view(index: usize, seat: &Seat, account: Option<&Account>) -> SeatView {
    SeatView {
        index,
        stack: seat.stack,
        occupant: match seat.occupant {
            SeatOccupant::Empty => "empty".into(),
            SeatOccupant::Human { .. } => "human".into(),
            SeatOccupant::Bot { kind } => format!("{kind:?}"),
        },
        sitting_out: seat.sitting_out,
        hole_cards: None,
        bank_balance: account.map(|account| account.balance),
        bank_entries: account.map_or_else(Vec::new, |account| account.entries.clone()),
    }
}
