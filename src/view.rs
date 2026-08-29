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
    /// Somebody has already paid for this seat and is waiting on the hand in
    /// progress, so it is not on offer to anyone else.
    pub reserved: bool,
    /// Set when a person is sitting here, so the UI can link to their page.
    pub user_id: Option<uuid::Uuid>,
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
    /// Betting is closed and the board is incomplete: the table is waiting on
    /// the next card, which a press or the deadline turns over (§V59).
    pub awaiting_advance: bool,
    /// Who leads, and on what equity, on the board as it stands right now.
    /// Live, not a replay of a decided result.
    pub runout_leaders: Vec<usize>,
    pub runout_odds: Vec<crate::holdem::ShowdownOdds>,
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
    pub viewer_eliminated: bool,
    pub viewer_leaving: bool,
    /// The viewer has paid to sit down but the hand already running has to
    /// finish before the seat is theirs.
    pub viewer_joining: bool,
    /// When the next card of a parked runout turns itself. The deadline is
    /// always armed, so the table can see it coming and a press only brings it
    /// forward (§V59).
    pub advance_at: Option<DateTime<Utc>>,
    pub hand: Option<HandView>,
    pub last_hand: Option<HandSummary>,
    pub next_hand_at: Option<DateTime<Utc>>,
    pub result_pause_seconds: i64,
    /// When the person to act runs out of time and the table acts for them.
    /// Absent when nobody is on the clock, which is most of the time: a table
    /// with only one person at it never puts anybody on one.
    pub turn_deadline: Option<DateTime<Utc>>,
    /// The whole length of that clock, so the client can draw how much is left
    /// without knowing the rule.
    pub turn_seconds: i64,
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
    /// Set for a person, whose page the standings link to. The house has none.
    pub player_id: Option<uuid::Uuid>,
    /// House regulars are ranked alongside people, and marked as such.
    pub house: bool,
    pub balance: Cents,
    pub net_balance: Cents,
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

pub fn hand_view(hand: &Hand, viewer: Option<usize>, x_ray: &[usize]) -> HandView {
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
            reserved: false,
            user_id: None,
            display_name: None,
            sitting_out: false,
            hole_cards: viewer
                .filter(|seat| *seat == player.seat)
                .map(|_| player.hole_cards.clone())
                .or_else(|| {
                    x_ray
                        .contains(&player.seat)
                        .then(|| player.hole_cards.clone())
                })
                .or_else(|| {
                    hand.summary
                        .as_ref()
                        .map(|summary| summary.revealed_hole_cards.clone())
                        .unwrap_or_else(|| hand.exposed_hole_cards())
                        .iter()
                        .find(|(seat, _)| *seat == player.seat)
                        .map(|(_, cards)| cards.clone())
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
        awaiting_advance: hand.awaits_runout(),
        runout_leaders: hand.leaders_now(),
        runout_odds: hand.odds_now(),
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

/// Whether this viewer is the only person at the table -- every other seat is
/// a bot or empty. Watchers are not seated, so this speaks to who is playing.
fn only_human_seated(table: &Table, viewer_id: Option<uuid::Uuid>) -> bool {
    !table.seats.iter().any(|seat| {
        matches!(seat.occupant, crate::table::SeatOccupant::Human { user_id }
            if Some(user_id) != viewer_id)
    })
}

pub fn table_view(table: &Table, viewer: Option<usize>) -> TableView {
    table_view_with_banks(
        table,
        viewer,
        None,
        None,
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        false,
    )
}

pub fn table_view_with_banks(
    table: &Table,
    viewer: Option<usize>,
    viewer_id: Option<uuid::Uuid>,
    bank_balance: Option<Cents>,
    banks: &std::collections::HashMap<usize, Account>,
    names: &std::collections::HashMap<usize, String>,
    see_bot_cards: bool,
) -> TableView {
    // Seat identity is fixed for as long as the viewer is a player in the hand
    // in progress -- runout included. It answers who was dealt in, not who is
    // currently holding chips, so a bust-out cannot move them off their own row
    // before the last card is face up (§V33).
    let dealt_into_live_hand = table.hand.as_ref().is_some_and(|hand| {
        viewer.is_some_and(|seat| hand.players.iter().any(|player| player.seat == seat))
    });
    let viewer_eliminated = viewer.is_some_and(|seat| table.tournament_seat_is_eliminated(seat))
        && !dealt_into_live_hand;
    let viewer = viewer.filter(|_| !viewer_eliminated);
    let tournament_result_visible = !terminal_tournament_result_pending(table);
    // Looking at the bots' cards is a solitaire privilege: the moment anybody
    // else takes a seat the table goes back to being opaque to everyone.
    let x_ray: Vec<usize> = if see_bot_cards && only_human_seated(table, viewer_id) {
        table
            .seats
            .iter()
            .enumerate()
            .filter(|(_, seat)| seat.occupant.as_bot().is_some())
            .map(|(index, _)| index)
            .collect()
    } else {
        Vec::new()
    };
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
        viewer_eliminated,
        viewer_leaving: viewer
            .and_then(|index| table.seats.get(index))
            .is_some_and(|seat| seat.pending_departure),
        viewer_joining: viewer_id.is_some_and(|user| {
            table
                .seats
                .iter()
                .any(|seat| seat.pending_arrival == Some(user))
        }),
        hand: table
            .hand
            .as_ref()
            .map(|hand| hand_view(hand, viewer, &x_ray)),
        last_hand: table.last_hand.clone(),
        next_hand_at: if table.hand.is_none() && table.last_hand.is_some() {
            table.next_action_at
        } else {
            None
        },
        advance_at: table
            .hand
            .as_ref()
            .is_some_and(crate::holdem::Hand::awaits_runout)
            .then_some(table.next_action_at)
            .flatten(),
        // The client paces the runout against this, so it must not guess it.
        result_pause_seconds: crate::table::result_pause_seconds(table.last_hand.as_ref()),
        turn_deadline: table
            .turn_clock
            .filter(|clock| {
                table
                    .hand
                    .as_ref()
                    .is_some_and(|hand| hand.current_player == Some(clock.seat))
            })
            .map(|clock| clock.deadline),
        turn_seconds: crate::table::TURN_SECONDS,
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
        reserved: seat.pending_arrival.is_some(),
        user_id: match seat.occupant {
            SeatOccupant::Human { user_id } => Some(user_id),
            SeatOccupant::Empty | SeatOccupant::Bot { .. } => None,
        },
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
            pending_arrival: None,
        };
        table.seats[1] = Seat {
            occupant: SeatOccupant::Human {
                user_id: uuid::Uuid::new_v4(),
            },
            stack: 0,
            sitting_out: false,
            pending_departure: false,
            pending_arrival: None,
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
            reveal_odds: Vec::new(),
        });
        table
    }

    /// A heads-up table against the house: the viewer sits, a bot sits, and
    /// the x-ray option is what decides whether the bot's cards are visible.
    fn solitaire_table(viewer: uuid::Uuid) -> Table {
        let mut table = Table::new(
            "practice".into(),
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            TableMode::Cash { no_debt: false },
            2,
            10_000,
        );
        table.seats[0].occupant = SeatOccupant::Human { user_id: viewer };
        table.seats[0].stack = 10_000;
        table.seats[1].occupant =
            SeatOccupant::bot(crate::table::Bot::new(crate::table::BotKind::Fish, 1));
        table.seats[1].stack = 10_000;
        table.hand = Some(crate::holdem::Hand::new(
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            &[10_000, 10_000],
            0,
            9,
        ));
        table
    }

    fn seat_cards(view: &TableView, seat: usize) -> Option<Vec<crate::cards::Card>> {
        view.hand
            .as_ref()
            .and_then(|hand| hand.seats.iter().find(|value| value.index == seat))
            .and_then(|value| value.hole_cards.clone())
    }

    #[test]
    fn the_bot_x_ray_shows_bot_cards_only_with_the_option_on() {
        let viewer = uuid::Uuid::new_v4();
        let table = solitaire_table(viewer);
        let banks = std::collections::HashMap::new();
        let names = std::collections::HashMap::new();

        let closed =
            table_view_with_banks(&table, Some(0), Some(viewer), None, &banks, &names, false);
        assert_eq!(
            seat_cards(&closed, 1),
            None,
            "the option is off, so the bot keeps its cards"
        );

        let open = table_view_with_banks(&table, Some(0), Some(viewer), None, &banks, &names, true);
        assert!(
            seat_cards(&open, 1).is_some_and(|cards| cards.len() == 2),
            "the option is on and nobody else is seated, so the bot is face up"
        );
        assert!(
            seat_cards(&open, 0).is_some(),
            "you still see your own hand"
        );
    }

    #[test]
    fn the_bot_x_ray_closes_as_soon_as_somebody_else_sits() {
        let viewer = uuid::Uuid::new_v4();
        let mut table = solitaire_table(viewer);
        table.max_seats = 3;
        table.seats.push(crate::table::Seat {
            occupant: SeatOccupant::Human {
                user_id: uuid::Uuid::new_v4(),
            },
            stack: 10_000,
            sitting_out: false,
            pending_departure: false,
            pending_arrival: None,
        });

        let view = table_view_with_banks(
            &table,
            Some(0),
            Some(viewer),
            None,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            true,
        );
        assert_eq!(
            seat_cards(&view, 1),
            None,
            "another person at the table closes the x-ray for everyone"
        );
    }

    /// §V33: the reported regression. A tournament bust-out must not take the
    /// viewer's seat away while the hand that busted them is still on the
    /// table -- that is what moved their player off the viewer row and up
    /// among the opponents before the board had finished running out.
    #[test]
    fn v33_a_busted_seat_is_held_until_the_hand_leaves_the_table() {
        let mut table = terminal_tournament();
        // Seat 1 is broke, and still a player in the hand in progress.
        table.seats[1].stack = 0;
        table.hand = Some(crate::holdem::Hand::new(
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            &[20_000, 10_000],
            0,
            7,
        ));

        let live = table_view(&table, Some(1));
        assert_eq!(
            live.viewer_seat,
            Some(1),
            "a player in the live hand keeps their own seat"
        );
        assert!(
            !live.viewer_eliminated,
            "elimination cannot land before the hand leaves the table"
        );

        // Once the hand is gone the seat is honestly eliminated.
        table.hand = None;
        let settled = table_view(&table, Some(1));
        assert_eq!(settled.viewer_seat, None);
        assert!(settled.viewer_eliminated);
    }

    /// §V59: a hand parked on its runout exposes the cards and who leads, and
    /// still carries no result for anything to leak.
    #[test]
    fn v59_a_parked_runout_shows_cards_but_no_result() {
        let mut table = terminal_tournament();
        let mut hand = crate::holdem::Hand::new(
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            &[10_000, 10_000],
            0,
            77,
        );
        hand.apply_action(crate::holdem::Action::AllIn).unwrap();
        hand.apply_action(crate::holdem::Action::Call).unwrap();
        assert!(hand.awaits_runout());
        table.hand = Some(hand);

        let view = table_view(&table, Some(0));
        let hand_view = view.hand.expect("a hand is on the table");
        assert!(
            hand_view.awaiting_advance,
            "the table waits on the next card"
        );
        assert!(hand_view.board.is_empty(), "no board until it is advanced");
        assert!(hand_view.summary.is_none(), "no result to leak");
        assert!(!hand_view.runout_leaders.is_empty(), "somebody is ahead");
        assert_eq!(
            hand_view
                .seats
                .iter()
                .filter(|seat| seat.hole_cards.is_some())
                .count(),
            2,
            "both hands are face up once betting is closed"
        );
    }

    /// §V59: the JSON the client actually reads. Opponent hole cards must be
    /// on `hand.seats` during the runout -- nothing consumed that field before,
    /// so a rename or a missed branch would silently leave cards face down.
    #[test]
    fn v59_runout_json_carries_every_opponent_hand() {
        let mut table = terminal_tournament();
        let mut hand = crate::holdem::Hand::new(
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            &[10_000, 10_000],
            0,
            77,
        );
        hand.apply_action(crate::holdem::Action::AllIn).unwrap();
        hand.apply_action(crate::holdem::Action::Call).unwrap();
        table.hand = Some(hand);

        // Seat 0 is looking: seat 1 is the opponent whose cards must be up.
        let json = serde_json::to_value(table_view(&table, Some(0))).expect("serializes");
        let seats = json["hand"]["seats"].as_array().expect("hand.seats");
        assert_eq!(seats.len(), 2);
        for seat in seats {
            let cards = seat["hole_cards"]
                .as_array()
                .unwrap_or_else(|| panic!("seat {} has no hole_cards in {seat}", seat["index"]));
            assert_eq!(cards.len(), 2, "both cards are up for {}", seat["index"]);
        }
        assert!(json["advance_at"].is_null() || json["advance_at"].is_string());
        assert_eq!(json["hand"]["awaiting_advance"], true);
        assert!(json["hand"]["summary"].is_null(), "no result to leak");
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
