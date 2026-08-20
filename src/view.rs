use crate::{
    bank::Account,
    cards::Card,
    holdem::{Hand, HandEvent, HandSummary, LegalActions},
    money::Cents,
    table::{Seat, SeatOccupant, Table},
};
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct SeatView {
    pub index: usize,
    pub stack: Cents,
    pub occupant: String,
    /// True when the house is sitting here, which a person may take over.
    pub bot: bool,
    pub display_name: Option<String>,
    pub sitting_out: bool,
    pub hole_cards: Option<Vec<Card>>,
    pub bank_balance: Option<Cents>,
    pub bank_entries: Vec<crate::bank::LedgerEntry>,
}
#[derive(Clone, Debug, Serialize)]
pub struct HandView {
    pub street: String,
    pub button: usize,
    pub big_blind: Cents,
    pub board: Vec<Card>,
    pub your_hole_cards: Option<Vec<Card>>,
    pub seats: Vec<SeatView>,
    pub pot: Cents,
    pub current_player: Option<usize>,
    pub legal_actions: Option<LegalActions>,
    pub summary: Option<HandSummary>,
    pub players: Vec<HandPlayerView>,
    pub events: Vec<HandEvent>,
    pub last_bet: Cents,
    pub to_call: Cents,
}

#[derive(Clone, Debug, Serialize)]
pub struct HandPlayerView {
    pub seat: usize,
    pub stack: Cents,
    pub contribution: Cents,
    pub street_contribution: Cents,
    pub folded: bool,
    pub all_in: bool,
    pub acted: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct TableView {
    pub id: uuid::Uuid,
    pub name: String,
    pub stakes: crate::table::Stakes,
    pub buy_in: Cents,
    pub bank_balance: Option<Cents>,
    pub seats: Vec<SeatView>,
    pub button: usize,
    pub viewer_seat: Option<usize>,
    pub viewer_leaving: bool,
    pub hand: Option<HandView>,
    pub last_hand: Option<HandSummary>,
    pub next_hand_at: Option<DateTime<Utc>>,
    pub result_pause_seconds: i64,
    /// Nobody is sitting at this table, so it only plays when asked to.
    pub can_deal: bool,
    pub tournament: Option<TournamentView>,
}

/// One row of the leaderboard: the money, and how sharp they are at reading a
/// board, which is a different kind of good.
#[derive(Clone, Debug, Serialize)]
pub struct LeaderboardRow {
    pub rank: usize,
    pub name: String,
    /// House regulars are ranked alongside people, and marked as such.
    pub house: bool,
    pub balance: Cents,
    pub loan_count: u64,
    pub poker: crate::stats::PlayerStats,
    pub blitz: Vec<LeaderboardBlitz>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LeaderboardBlitz {
    pub difficulty: String,
    pub attempts: u64,
    pub accuracy_percent: u64,
    pub best_streak: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct LobbyTableView {
    pub id: uuid::Uuid,
    pub name: String,
    pub stakes: crate::table::Stakes,
    pub buy_in: Cents,
    pub occupied: usize,
    /// How many of those seats hold a person rather than the house.
    pub humans: usize,
    pub max_seats: usize,
    pub no_debt: bool,
    pub affordable: bool,
    pub tournament: Option<LobbyTournamentView>,
    pub your_seat: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LobbyTournamentView {
    pub buy_in: Cents,
    pub registered: usize,
    pub seat_count: usize,
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
    pub started: bool,
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
            bot: false,
            display_name: None,
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
        button: hand.button,
        big_blind: hand.stakes.blinds().1,
        board: hand.board.clone(),
        your_hole_cards,
        seats,
        pot: hand.pot,
        current_player: hand.current_player,
        legal_actions: viewer
            .filter(|seat| hand.current_player == Some(*seat))
            .and_then(|_| hand.legal_actions()),
        summary: hand.summary.clone(),
        players: hand
            .players
            .iter()
            .map(|player| HandPlayerView {
                seat: player.seat,
                stack: player.stack,
                contribution: player.contribution,
                street_contribution: player.street_contribution,
                folded: player.folded,
                all_in: player.all_in,
                acted: player.acted,
            })
            .collect(),
        events: hand.events.clone(),
        last_bet: hand.last_bet,
        to_call: hand.current_player.map_or(0, |seat| {
            hand.players
                .iter()
                .find(|player| player.seat == seat)
                .map_or(0, |player| {
                    hand.last_bet.saturating_sub(player.street_contribution)
                })
        }),
    }
}

pub fn table_view(table: &Table, viewer: Option<usize>) -> TableView {
    table_view_with_banks(
        table,
        viewer,
        None,
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
    )
}

pub fn table_view_with_banks(
    table: &Table,
    viewer: Option<usize>,
    bank_balance: Option<Cents>,
    banks: &std::collections::HashMap<usize, Account>,
    names: &std::collections::HashMap<usize, String>,
) -> TableView {
    let tournament_result_visible = !terminal_tournament_result_pending(table);
    TableView {
        id: table.id,
        name: table.name.clone(),
        stakes: table.stakes,
        buy_in: table.buy_in,
        bank_balance,
        seats: table
            .seats
            .iter()
            .enumerate()
            .map(|(index, seat)| seat_view(index, seat, banks.get(&index), names.get(&index)))
            .collect(),
        button: table.button,
        viewer_seat: viewer,
        viewer_leaving: viewer
            .and_then(|index| table.seats.get(index))
            .is_some_and(|seat| seat.pending_departure),
        hand: table.hand.as_ref().map(|hand| hand_view(hand, viewer)),
        last_hand: table.last_hand.clone(),
        next_hand_at: if table.hand.is_none() && table.last_hand.is_some() {
            table.next_action_at
        } else {
            None
        },
        // The client paces the runout against this, so it must not guess it.
        result_pause_seconds: crate::table::result_pause_seconds(table.last_hand.as_ref()),
        can_deal: table.hand.is_none()
            && table.waits_for_a_watcher()
            && table.seats.iter().filter(|seat| seat.stack > 0).count() >= 2,
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
                    finish_order: if state.finished && !tournament_result_visible {
                        Vec::new()
                    } else {
                        state.finish_order.clone()
                    },
                    started: state.started,
                    finished: state.finished && tournament_result_visible,
                }),
            crate::table::TableMode::Cash { .. } => None,
        },
    }
}

fn terminal_tournament_result_pending(table: &Table) -> bool {
    matches!(&table.mode, crate::table::TableMode::Tournament(state) if state.finished)
        && table.hand.is_none()
        && table.last_hand.is_some()
        && table.next_action_at.is_some_and(|at| at > Utc::now())
}
fn seat_view(
    index: usize,
    seat: &Seat,
    account: Option<&Account>,
    display_name: Option<&String>,
) -> SeatView {
    SeatView {
        index,
        stack: seat.stack,
        occupant: match seat.occupant {
            SeatOccupant::Empty => "empty".into(),
            SeatOccupant::Human { .. } => "human".into(),
            SeatOccupant::Bot { kind, seat } => {
                crate::table::Bot::new(kind, seat).name().to_string()
            }
        },
        bot: seat.occupant.as_bot().is_some(),
        display_name: display_name.cloned(),
        sitting_out: seat.sitting_out,
        hole_cards: None,
        bank_balance: account.map(|account| account.balance),
        bank_entries: account.map_or_else(Vec::new, |account| account.entries.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{
        BlindLevel, Seat, SeatOccupant, Stakes, Table, TableMode, TournamentConfig, TournamentState,
    };
    use chrono::Duration;
    use std::collections::BTreeMap;

    fn terminal_tournament() -> Table {
        let mut table = Table::new(
            "final".into(),
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            TableMode::Tournament(TournamentState {
                config: TournamentConfig {
                    buy_in: 1_000,
                    seat_count: 2,
                    starting_chips: 10_000,
                    levels: vec![BlindLevel {
                        small_blind: 100,
                        big_blind: 200,
                        ante: 0,
                        hands: 4,
                    }],
                    payout_percentages: vec![100],
                    no_debt: false,
                },
                current_level: 0,
                hands_at_level: 1,
                finish_order: vec![1],
                registered: 2,
                started: true,
                prize_pool: 2_000,
                finished: true,
                paid_out: false,
            }),
            2,
            1_000,
        );
        table.seats[0] = Seat {
            occupant: SeatOccupant::Human {
                user_id: uuid::Uuid::new_v4(),
            },
            stack: 20_000,
            sitting_out: false,
            pending_departure: false,
        };
        table.seats[1] = Seat {
            occupant: SeatOccupant::Human {
                user_id: uuid::Uuid::new_v4(),
            },
            stack: 0,
            sitting_out: false,
            pending_departure: false,
        };
        table.last_hand = Some(crate::holdem::HandSummary {
            board: Vec::new(),
            results: Vec::new(),
            awards: vec![crate::holdem::Award {
                seat: 0,
                amount: 20_000,
            }],
            contributions: BTreeMap::new(),
            revealed_hole_cards: vec![(0, Vec::new()), (1, Vec::new())],
            events: Vec::new(),
            runout_from: 0,
            runout: Vec::new(),
            stacks_before_awards: BTreeMap::new(),
            reveal_leaders: vec![0],
        });
        table
    }

    #[test]
    fn tournament_result_waits_for_reveal_before_exposing_winner_v33() {
        let mut table = terminal_tournament();
        table.next_action_at = Some(Utc::now() + Duration::seconds(6));

        let hidden = table_view(&table, None).tournament.unwrap();

        assert!(!hidden.finished, "terminal winner waits for reveal");
        assert!(
            hidden.finish_order.is_empty(),
            "terminal finish order would identify the winner too early"
        );

        table.next_action_at = Some(Utc::now() - Duration::seconds(1));
        let visible = table_view(&table, None).tournament.unwrap();

        assert!(visible.finished);
        assert_eq!(visible.finish_order, vec![1]);
    }
}
