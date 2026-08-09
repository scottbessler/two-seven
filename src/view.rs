use crate::{
    cards::Card,
    holdem::{Hand, HandSummary},
    table::{Cents, Seat, SeatOccupant},
};
use serde::Serialize;
#[derive(Clone, Debug, Serialize)]
pub struct HandView {
    pub street: String,
    pub board: Vec<Card>,
    pub your_hole_cards: Option<Vec<Card>>,
    pub seats: Vec<SeatView>,
    pub pot: Cents,
    pub current_player: Option<usize>,
    pub summary: Option<HandSummary>,
}
#[derive(Clone, Debug, Serialize)]
pub struct SeatView {
    pub index: usize,
    pub stack: Cents,
    pub occupant: String,
    pub hole_cards: Option<Vec<Card>>,
}
pub fn hand_view(hand: &Hand, viewer: usize) -> HandView {
    HandView {
        street: format!("{:?}", hand.street),
        board: hand.board.clone(),
        your_hole_cards: Some(hand.players[viewer].hole_cards.clone()),
        seats: hand
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| SeatView {
                index: i,
                stack: p.stack,
                occupant: format!("seat {i}"),
                hole_cards: (hand.complete || i == viewer).then(|| p.hole_cards.clone()),
            })
            .collect(),
        pot: hand.pot,
        current_player: hand.current_player,
        summary: hand.summary.clone(),
    }
}
pub fn table_seat_view(seat: &Seat) -> SeatView {
    SeatView {
        index: 0,
        stack: seat.stack,
        occupant: match seat.occupant {
            SeatOccupant::Empty => "empty".into(),
            SeatOccupant::Human { .. } => "human".into(),
            SeatOccupant::Bot { .. } => "bot".into(),
        },
        hole_cards: None,
    }
}
